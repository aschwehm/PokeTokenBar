//! Per-file incremental usage cache.
//!
//! Port of the macOS original `Core/LocalUsageCache.swift`. Files are parsed
//! once per `(path, mtime, size)` triple and the blobs persist to a JSON
//! snapshot, so a cold start (full parse) only happens once.
//!
//! **Deviation from the original: plain JSON, no zlib.** The snapshot format is
//! our own (the original's zlib-compressed plist/JSON snapshot is not loadable
//! cross-platform anyway), and the compression bought little for a menu-bar
//! app's refresh cadence.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

use crate::providers::reader;

const LOCAL_DAY_FORMAT: &str = "%Y-%m-%d";
/// Bump to re-parse only the Codex blobs (fork-replay and same-state rewrite
/// handling changed).
const CODEX_PARSER_VERSION: i64 = 4;
/// Bump only when the session-id extraction rules (`session_meta` id/session_id
/// resolution, probe termination) change. Separated from `CODEX_PARSER_VERSION`
/// so a resolver change does not wipe the whole index and resurrect the
/// full-scan probes. v2 drops v1 entries that froze read failures as "no id".
const CODEX_SESSION_INDEX_VERSION: i64 = 2;
/// Bump to re-parse only the Grok blobs (token mapping / cost-trust change).
const GROK_PARSER_VERSION: i64 = 1;
/// Blobs older than this never fall inside any scan window (week/month/5h
/// block) and are dropped at save time — bounds cache growth.
const PRUNE_CUTOFF_DAYS: i64 = 40;
/// Throttle between disk writes.
const SAVE_THROTTLE_SECS: i64 = 60;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Blob {
    mtime: DateTime<Utc>,
    size: u64,
    entries: Vec<reader::Entry>,
}

/// Codex caches parsed rollouts (not final entries) — a fork file alone cannot
/// settle its own usage.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CodexBlob {
    mtime: DateTime<Utc>,
    size: u64,
    rollout: reader::CodexParsedRollout,
}

/// A lightweight per-file index holding only the session id — lets parent
/// candidates be filtered without opening the file. `None` ("no session id in
/// this file") is stored too: not persisting that negative result would reopen
/// every rollout each refresh when one orphaned fork is missing its parent.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CodexSessionProbe {
    mtime: DateTime<Utc>,
    size: u64,
    session_id: Option<String>,
}

/// On-disk snapshot. Every field is `#[serde(default)]` so an older snapshot
/// missing a key still loads (lenient, mirroring the original's `init(from:)`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Snapshot {
    #[serde(default)]
    claude: HashMap<String, Blob>,
    #[serde(default)]
    codex: HashMap<String, CodexBlob>,
    #[serde(default)]
    codex_session_ids: HashMap<String, CodexSessionProbe>,
    #[serde(default)]
    gemini: HashMap<String, Blob>,
    #[serde(default)]
    grok: HashMap<String, Blob>,
    #[serde(default)]
    codex_parser_version: i64,
    #[serde(default)]
    codex_session_index_version: i64,
    #[serde(default)]
    grok_parser_version: i64,
}

/// File-level incremental cache. Methods take `&mut self` (production access is
/// serialized behind the `Mutex` from `shared()`).
///
/// **Borrow-checker note:** the reader's `expand_codex_parent_closure` takes
/// shared-borrow `Fn` closures for `load` / `probe_session_id`, but those must
/// mutate cache state. Instead of switching the reader's API to `FnMut` (which
/// would ripple into `reader::codex_entries` and the shared `adopt` helper for
/// no behavioral gain), `codex_cache`, `codex_session_ids`, and `dirty` are
/// interior-mutable (`RefCell` / `Cell`), so `collect_codex_rollouts` only
/// needs `&self` and the closures capture it. This is safe because only one
/// thread ever touches an instance at a time.
pub struct LocalUsageCache {
    claude_roots: Option<Vec<PathBuf>>,
    claude_root: Option<PathBuf>,
    codex_root: Option<PathBuf>,
    gemini_root: Option<PathBuf>,
    grok_root: Option<PathBuf>,
    file_url: PathBuf,
    now: fn() -> DateTime<Utc>,
    /// Throwing probe — a read failure (`Err`) and "no metadata" (`Ok(None)`)
    /// differ in whether they persist to the index.
    codex_probe: fn(&Path) -> std::io::Result<Option<String>>,
    claude_cache: HashMap<String, Blob>,
    codex_cache: RefCell<HashMap<String, CodexBlob>>,
    codex_session_ids: RefCell<HashMap<String, CodexSessionProbe>>,
    gemini_cache: HashMap<String, Blob>,
    grok_cache: HashMap<String, Blob>,
    loaded: bool,
    dirty: Cell<bool>,
    last_save: Option<DateTime<Utc>>,
}

fn default_file_url() -> PathBuf {
    crate::platform::data_dir().join("usage-cache.json")
}

fn default_codex_probe(path: &Path) -> std::io::Result<Option<String>> {
    reader::probe_codex_rollout_session_id(path, reader::CODEX_PROBE_BYTE_LIMIT)
}

impl LocalUsageCache {
    /// All dependencies injectable for tests. Defaults: no roots, the
    /// platform data-dir cache file, wall-clock `Utc::now`, the real metadata
    /// probe.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        claude_roots: Option<Vec<PathBuf>>,
        claude_root: Option<PathBuf>,
        codex_root: Option<PathBuf>,
        gemini_root: Option<PathBuf>,
        grok_root: Option<PathBuf>,
        file_url: PathBuf,
        now: fn() -> DateTime<Utc>,
        codex_probe: fn(&Path) -> std::io::Result<Option<String>>,
    ) -> Self {
        Self {
            claude_roots,
            claude_root,
            codex_root,
            gemini_root,
            grok_root,
            file_url,
            now,
            codex_probe,
            claude_cache: HashMap::new(),
            codex_cache: RefCell::new(HashMap::new()),
            codex_session_ids: RefCell::new(HashMap::new()),
            gemini_cache: HashMap::new(),
            grok_cache: HashMap::new(),
            loaded: false,
            dirty: Cell::new(false),
            last_save: None,
        }
    }

    /// Process-wide instance (production) — the original's `actor shared`.
    pub fn shared() -> &'static Mutex<LocalUsageCache> {
        static SHARED: OnceLock<Mutex<LocalUsageCache>> = OnceLock::new();
        SHARED.get_or_init(|| Mutex::new(LocalUsageCache::default()))
    }
}

impl Default for LocalUsageCache {
    fn default() -> Self {
        Self::new(
            None,
            None,
            None,
            None,
            None,
            default_file_url(),
            Utc::now,
            default_codex_probe,
        )
    }
}

impl LocalUsageCache {
    pub fn claude_entries(&mut self, modified_since: DateTime<Utc>) -> Vec<reader::Entry> {
        self.ensure_loaded();
        // Several roots (CLI default + CLAUDE_CONFIG_DIR + Claude Desktop
        // embedded sessions). Blob keys are absolute paths, so adding roots
        // reuses the cache, and a turn copied into several roots is counted
        // once by the global dedup.
        let roots = match &self.claude_roots {
            Some(roots) => roots.clone(),
            None => match &self.claude_root {
                Some(root) => vec![root.clone()],
                None => reader::claude_project_roots(),
            },
        };
        let mut all = Vec::new();
        for root in roots {
            all.extend(collect(
                &root,
                modified_since,
                &mut self.claude_cache,
                &self.dirty,
                false,
                None,
                |f| reader::parse_claude_file(f, LOCAL_DAY_FORMAT),
            ));
        }
        self.save_if_needed();
        reader::dedup_keep_max(all)
    }

    pub fn codex_entries(&mut self, modified_since: DateTime<Utc>) -> Vec<reader::Entry> {
        self.ensure_loaded();
        let home = reader::home_dir().unwrap_or_default();
        let root = match &self.codex_root {
            Some(root) => root.clone(),
            None => reader::codex_sessions_dir(&home),
        };
        let (rollouts, included_paths) = self.collect_codex_rollouts(&root, modified_since);
        let entries = reader::resolve_codex_rollouts(rollouts, included_paths);
        self.save_if_needed();
        entries
    }

    /// Test observation hook — verifies index entries for deleted rollouts are
    /// dropped rather than accumulating forever.
    pub fn codex_session_index_count(&mut self) -> usize {
        self.ensure_loaded();
        self.codex_session_ids.borrow().len()
    }

    pub fn gemini_entries(&mut self, modified_since: DateTime<Utc>) -> Vec<reader::Entry> {
        self.ensure_loaded();
        let home = reader::home_dir().unwrap_or_default();
        let root = match &self.gemini_root {
            Some(root) => root.clone(),
            None => reader::gemini_tmp_dir(&home),
        };
        let entries = collect(
            &root,
            modified_since,
            &mut self.gemini_cache,
            &self.dirty,
            true,
            None,
            |f| reader::parse_gemini_file(f, LOCAL_DAY_FORMAT),
        );
        self.save_if_needed();
        entries
    }

    pub fn grok_entries(&mut self, modified_since: DateTime<Utc>) -> Vec<reader::Entry> {
        self.ensure_loaded();
        let root = match &self.grok_root {
            Some(root) => root.clone(),
            None => reader::grok_sessions_dir(),
        };
        let all = collect(
            &root,
            modified_since,
            &mut self.grok_cache,
            &self.dirty,
            false,
            Some(reader::is_grok_usage_file),
            |f| reader::parse_grok_file(f, LOCAL_DAY_FORMAT),
        );
        self.save_if_needed();
        // A fork session copying the parent's updates carries the same turn ids
        // — the global dedup counts each turn once.
        reader::dedup_keep_max(all)
    }

    /// Record (or refresh) the session-id index entry for a rollout file.
    fn remember_session_id(&self, file: &reader::CodexRolloutFile, session_id: Option<String>) {
        self.codex_session_ids.borrow_mut().insert(
            file.path(),
            CodexSessionProbe {
                mtime: file.mtime,
                size: file.size,
                session_id,
            },
        );
        self.dirty.set(true);
    }

    /// Codex caches parsed rollouts (not final entries) because a fork file
    /// alone cannot settle its own usage. Parents outside the query window are
    /// still needed for replay comparisons, so they are found by session id and
    /// returned as dependencies alongside.
    fn collect_codex_rollouts(
        &self,
        root: &Path,
        since: DateTime<Utc>,
    ) -> (Vec<reader::CodexParsedRollout>, HashSet<String>) {
        let files = reader::codex_rollout_files(root);

        let load = |file: &reader::CodexRolloutFile| {
            // Cache hit → reuse the parsed rollout. If the session-index probe
            // for this path is stale, re-remember the id from the blob.
            let cached = self.codex_cache.borrow().get(&file.path()).cloned();
            if let Some(blob) = cached {
                if blob.mtime == file.mtime && blob.size == file.size {
                    let stale = self
                        .codex_session_ids
                        .borrow()
                        .get(&file.path())
                        .is_none_or(|p| p.mtime != file.mtime || p.size != file.size);
                    if stale {
                        self.remember_session_id(file, blob.rollout.session_id.clone());
                    }
                    return blob.rollout;
                }
            }
            let rollout = reader::parse_codex_rollout(&file.url, LOCAL_DAY_FORMAT);
            self.codex_cache.borrow_mut().insert(
                file.path(),
                CodexBlob {
                    mtime: file.mtime,
                    size: file.size,
                    rollout: rollout.clone(),
                },
            );
            self.dirty.set(true);
            // Blobs are 40-day-pruned, so old parents soon vanish — keep just
            // the session id so candidates stay filterable without reopening.
            self.remember_session_id(file, rollout.session_id.clone());
            rollout
        };

        // Known session id without opening the file — valid blob, then index.
        // `Known(None)` is a valid result too.
        let session_id_knowledge =
            |file: &reader::CodexRolloutFile| -> reader::CodexSessionIDKnowledge {
                let cache = self.codex_cache.borrow();
                if let Some(blob) = cache.get(&file.path()) {
                    if blob.mtime == file.mtime && blob.size == file.size {
                        return reader::CodexSessionIDKnowledge::Known(
                            blob.rollout.session_id.clone(),
                        );
                    }
                }
                drop(cache);
                let probes = self.codex_session_ids.borrow();
                if let Some(probe) = probes.get(&file.path()) {
                    if probe.mtime == file.mtime && probe.size == file.size {
                        return reader::CodexSessionIDKnowledge::Known(probe.session_id.clone());
                    }
                }
                reader::CodexSessionIDKnowledge::Unknown
            };

        // Files whose probe failed this call. Not persisted to the index (the
        // next refresh retries) — only stops the same file from being reopened
        // once per orphaned parent within one refresh.
        let temporarily_failed_paths: RefCell<HashSet<String>> = RefCell::new(HashSet::new());
        let probe_session_id = |file: &reader::CodexRolloutFile| -> Option<String> {
            if temporarily_failed_paths.borrow().contains(&file.path()) {
                return None;
            }
            match (self.codex_probe)(&file.url) {
                Ok(id) => {
                    // `None` ("no session id in this file") is a completed probe
                    // too — persist it so the file is not reopened every refresh.
                    self.remember_session_id(file, id.clone());
                    id
                }
                Err(_) => {
                    temporarily_failed_paths.borrow_mut().insert(file.path());
                    None
                }
            }
        };

        let window_files: Vec<reader::CodexRolloutFile> =
            files.iter().filter(|f| f.mtime >= since).cloned().collect();
        let existing: HashSet<String> = files.iter().map(reader::CodexRolloutFile::path).collect();
        let result = reader::expand_codex_parent_closure(
            window_files,
            files,
            load,
            session_id_knowledge,
            probe_session_id,
        );

        // Drop index entries for vanished rollouts. The criterion is file
        // existence, not age — the index exists to find old parents, so it must
        // outlive the 40-day blob prune. A failed enumeration (root missing,
        // transient permissions) yields an empty list; treating that as "all
        // deleted" would wipe the index.
        if !existing.is_empty() {
            let mut ids = self.codex_session_ids.borrow_mut();
            let survivors: HashMap<String, CodexSessionProbe> = ids
                .iter()
                .filter(|(path, _)| existing.contains(*path))
                .map(|(path, probe)| (path.clone(), probe.clone()))
                .collect();
            if survivors.len() != ids.len() {
                *ids = survivors;
                self.dirty.set(true);
            }
        }
        result
    }

    fn ensure_loaded(&mut self) {
        if self.loaded {
            return;
        }
        self.loaded = true;
        let raw = match std::fs::read(&self.file_url) {
            Ok(raw) => raw,
            Err(_) => return,
        };
        // Plain JSON snapshot (no zlib — our format, see the module doc).
        let snap: Snapshot = match serde_json::from_slice(&raw) {
            Ok(s) => s,
            Err(_) => return,
        };
        self.claude_cache = snap.claude;
        *self.codex_cache.get_mut() = snap.codex;
        *self.codex_session_ids.get_mut() = snap.codex_session_ids;
        self.gemini_cache = snap.gemini;
        self.grok_cache = snap.grok;

        if snap.codex_parser_version != CODEX_PARSER_VERSION {
            self.codex_cache.get_mut().clear();
            self.dirty.set(true);
        }
        if snap.codex_session_index_version != CODEX_SESSION_INDEX_VERSION {
            self.codex_session_ids.get_mut().clear();
            self.dirty.set(true);
        }
        if snap.grok_parser_version != GROK_PARSER_VERSION {
            self.grok_cache.clear();
            self.dirty.set(true);
        }
    }

    /// Drop blobs that fall outside every query window (month/week start is the
    /// widest, so 40 days is generous headroom); deleted session files are
    /// cleaned up in passing. `codex_session_ids` is untouched — finding
    /// parents older than 40 days is its whole purpose.
    fn prune(&mut self) {
        let cutoff = (self.now)() - chrono::Duration::days(PRUNE_CUTOFF_DAYS);
        for cache in [
            &mut self.claude_cache,
            &mut self.gemini_cache,
            &mut self.grok_cache,
        ] {
            cache.retain(|_, blob| blob.mtime >= cutoff);
        }
        self.codex_cache
            .get_mut()
            .retain(|_, blob| blob.mtime >= cutoff);
    }

    /// Persist when dirty, throttled to at most one write per 60 seconds.
    fn save_if_needed(&mut self) {
        if !self.dirty.get() {
            return;
        }
        if let Some(last) = self.last_save {
            if ((self.now)() - last).num_seconds() < SAVE_THROTTLE_SECS {
                return;
            }
        }
        self.prune();
        let snap = Snapshot {
            claude: self.claude_cache.clone(),
            codex: self.codex_cache.borrow().clone(),
            codex_session_ids: self.codex_session_ids.borrow().clone(),
            gemini: self.gemini_cache.clone(),
            grok: self.grok_cache.clone(),
            codex_parser_version: CODEX_PARSER_VERSION,
            codex_session_index_version: CODEX_SESSION_INDEX_VERSION,
            grok_parser_version: GROK_PARSER_VERSION,
        };
        let data = match serde_json::to_vec(&snap) {
            Ok(d) => d,
            Err(_) => return,
        };
        // Atomic-ish write (temp + rename), mirroring the original's `.atomic`
        // option. On failure `dirty` stays set, so the next refresh retries.
        if write_atomic(&self.file_url, &data).is_ok() {
            self.dirty.set(false);
            self.last_save = Some((self.now)());
        }
    }
}

fn write_atomic(path: &Path, data: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, data)?;
    #[cfg(windows)]
    let _ = std::fs::remove_file(path);
    std::fs::rename(&tmp, path)
}

fn is_hidden_name(name: &std::ffi::OsStr) -> bool {
    name.to_string_lossy().starts_with('.')
}

/// Walk `root` recursively, parsing `.jsonl` (plus `.json` when `allow_json`,
/// Gemini only) files modified at/after `since`. Hidden items are skipped.
///
/// `include` is evaluated BEFORE the blob cache hit — a decision that depends
/// on out-of-file state (a sibling file) must not be frozen into the blob.
fn collect(
    root: &Path,
    since: DateTime<Utc>,
    cache: &mut HashMap<String, Blob>,
    dirty: &Cell<bool>,
    allow_json: bool,
    include: Option<fn(&Path) -> bool>,
    parse: fn(&Path) -> Vec<reader::Entry>,
) -> Vec<reader::Entry> {
    let mut result = Vec::new();
    for entry in WalkDir::new(root)
        .into_iter()
        .filter_entry(|e| e.depth() == 0 || !is_hidden_name(e.file_name()))
    {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let ext = entry.path().extension();
        let matches = ext.map(|e| e == "jsonl").unwrap_or(false)
            || (allow_json && ext.map(|e| e == "json").unwrap_or(false));
        if !matches {
            continue;
        }
        if let Some(include) = include {
            if !include(entry.path()) {
                continue;
            }
        }
        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        let mtime = match meta.modified() {
            Ok(m) => DateTime::<Utc>::from(m),
            Err(_) => continue,
        };
        if mtime < since {
            continue;
        }
        let size = meta.len();
        let key = entry.path().to_string_lossy().into_owned();
        if let Some(blob) = cache.get(&key) {
            if blob.mtime == mtime && blob.size == size {
                result.extend(blob.entries.iter().cloned());
                continue;
            }
        }
        let entries = parse(entry.path());
        cache.insert(
            key,
            Blob {
                mtime,
                size,
                entries: entries.clone(),
            },
        );
        dirty.set(true);
        result.extend(entries);
    }
    result
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::io;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};
    use std::sync::Mutex;

    use chrono::{Duration, Utc};
    use serde_json::Value;

    use super::*;

    /// Serializes the tests that share injected clock / probe state.
    static STATE_LOCK: Mutex<()> = Mutex::new(());
    static FAKE_NOW: AtomicI64 = AtomicI64::new(0);
    static PROBE_COUNT: AtomicUsize = AtomicUsize::new(0);
    static FAIL_ONCE: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    static FAIL_COUNTS: OnceLock<Mutex<HashMap<String, usize>>> = OnceLock::new();

    fn fail_once() -> &'static Mutex<HashSet<String>> {
        FAIL_ONCE.get_or_init(|| Mutex::new(HashSet::new()))
    }

    fn fail_counts() -> &'static Mutex<HashMap<String, usize>> {
        FAIL_COUNTS.get_or_init(|| Mutex::new(HashMap::new()))
    }

    fn epoch() -> DateTime<Utc> {
        DateTime::from_timestamp(0, 0).unwrap()
    }

    /// Integer-second "now" so fixture mtimes round-trip exactly through the
    /// filesystem (sub-second precision can differ between stat calls).
    fn recent_second() -> DateTime<Utc> {
        DateTime::from_timestamp(Utc::now().timestamp(), 0).unwrap()
    }

    fn temp_dir() -> PathBuf {
        let d = std::env::temp_dir().join(format!("ptb-cache-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn project_root(base: &Path) -> PathBuf {
        let root = base.join("projects");
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    fn fake_now() -> DateTime<Utc> {
        DateTime::from_timestamp(FAKE_NOW.load(Ordering::SeqCst), 0).unwrap()
    }

    /// Throwing probe with injection seams: counts every call, fails once per
    /// filename in `FAIL_ONCE`, and tallies per-filename call counts.
    fn counting_probe(path: &Path) -> io::Result<Option<String>> {
        PROBE_COUNT.fetch_add(1, Ordering::SeqCst);
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let should_fail = {
            let mut pending = fail_once().lock().unwrap_or_else(|p| p.into_inner());
            pending.remove(&name)
        };
        {
            let mut counts = fail_counts().lock().unwrap_or_else(|p| p.into_inner());
            *counts.entry(name).or_default() += 1;
        }
        if should_fail {
            return Err(io::Error::other("injected probe failure"));
        }
        reader::probe_codex_rollout_session_id(path, reader::CODEX_PROBE_BYTE_LIMIT)
    }

    fn set_fail_once(names: &[&str]) {
        let mut pending = fail_once().lock().unwrap_or_else(|p| p.into_inner());
        pending.clear();
        pending.extend(names.iter().map(|s| s.to_string()));
    }

    fn call_count(name: &str) -> usize {
        fail_counts()
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(name)
            .copied()
            .unwrap_or(0)
    }

    /// Zero the process-global probe counters. Tests that assert on probe
    /// counts must call this first (the statics persist across tests and across
    /// cache instances within one test).
    fn reset_probe_state() {
        PROBE_COUNT.store(0, Ordering::SeqCst);
        fail_counts()
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clear();
    }

    fn make_cache_with(root: &Path, now: fn() -> DateTime<Utc>) -> LocalUsageCache {
        // The scan root lives inside a unique temp base; the cache file sits
        // next to it (outside the scanned tree), like the original's
        // Application Support location.
        let cache_file = root.parent().unwrap().join("usage-cache.json");
        LocalUsageCache::new(
            None,
            Some(root.to_path_buf()),
            Some(root.to_path_buf()),
            None,
            None,
            cache_file,
            now,
            counting_probe,
        )
    }

    fn make_cache(root: &Path) -> LocalUsageCache {
        make_cache_with(root, Utc::now)
    }

    fn set_mtime(path: &Path, t: DateTime<Utc>) {
        let file = std::fs::File::options().write(true).open(path).unwrap();
        let times = std::fs::FileTimes::new().set_modified(t.into());
        file.set_times(times).unwrap();
    }

    fn write_file(
        dir: &Path,
        name: &str,
        lines: &[String],
        mtime: Option<DateTime<Utc>>,
    ) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, lines.join("\n")).unwrap();
        if let Some(t) = mtime {
            set_mtime(&path, t);
        }
        path
    }

    // MARK: - Fixture builders (mirror the Swift test helpers)

    fn claude_line(id: &str, output: i64, ts: &str) -> String {
        format!(
            r#"{{"type":"assistant","requestId":"r-{id}","timestamp":"{ts}","message":{{"id":"m-{id}","model":"claude-opus-4-8","usage":{{"input_tokens":10,"output_tokens":{output},"cache_creation_input_tokens":5,"cache_read_input_tokens":100}}}}}}"#
        )
    }

    fn codex_line(ts: &str, output: i64) -> String {
        serde_json::to_string(&serde_json::json!({
            "type": "event_msg",
            "timestamp": ts,
            "payload": {
                "type": "token_count",
                "info": {
                    "last_token_usage": {
                        "input_tokens": 1000,
                        "cached_input_tokens": 200,
                        "cache_write_input_tokens": 0,
                        "output_tokens": output,
                        "reasoning_output_tokens": 10,
                        "total_tokens": 1000 + output,
                    }
                }
            }
        }))
        .unwrap()
    }

    #[allow(clippy::too_many_arguments)]
    fn codex_state_line(
        ts: &str,
        cumulative_input: i64,
        cumulative_output: i64,
        last_input: i64,
        last_output: i64,
    ) -> String {
        serde_json::to_string(&serde_json::json!({
            "type": "event_msg",
            "timestamp": ts,
            "payload": {
                "type": "token_count",
                "info": {
                    "total_token_usage": {
                        "input_tokens": cumulative_input,
                        "cached_input_tokens": 0,
                        "output_tokens": cumulative_output,
                        "reasoning_output_tokens": 0,
                        "total_tokens": cumulative_input + cumulative_output,
                    },
                    "last_token_usage": {
                        "input_tokens": last_input,
                        "cached_input_tokens": 0,
                        "output_tokens": last_output,
                        "reasoning_output_tokens": 0,
                        "total_tokens": last_input + last_output,
                    },
                }
            }
        }))
        .unwrap()
    }

    fn forked_session_meta(ts: &str) -> String {
        format!(
            r#"{{"type":"session_meta","timestamp":"{ts}","payload":{{"id":"child","forked_from_id":"parent","parent_thread_id":"parent","thread_source":"user"}}}}"#
        )
    }

    fn session_meta(id: &str, ts: &str) -> String {
        format!(
            r#"{{"type":"session_meta","timestamp":"{ts}","payload":{{"id":"{id}","session_id":"{id}"}}}}"#
        )
    }

    fn fork_meta(id: &str, parent_id: &str, ts: &str) -> String {
        format!(
            r#"{{"type":"session_meta","timestamp":"{ts}","payload":{{"id":"{id}","forked_from_id":"{parent_id}","parent_thread_id":"{parent_id}","thread_source":"user"}}}}"#
        )
    }

    fn orphaned_fork_lines(last_output: i64) -> Vec<String> {
        vec![
            fork_meta("child", "gone-parent", "2026-07-30T01:00:00.000Z"),
            codex_state_line("2026-07-30T01:00:05.000Z", 100, 10, 100, 10),
            codex_state_line("2026-07-30T01:00:10.000Z", 300, 30, 200, last_output),
        ]
    }

    /// One orphaned fork (whose parent is gone) plus two out-of-window
    /// rollouts that can only be judged by opening them. Fixture mtimes are in
    /// the 1970s so the blob prune (40 days) drops them at the first save.
    fn write_orphaned_fork_tree(root: &Path) {
        write_file(
            root,
            "rollout-alpha.jsonl",
            &[
                session_meta("alpha", "2026-07-29T01:00:00.000Z"),
                codex_state_line("2026-07-29T01:00:01.000Z", 10, 1, 10, 1),
            ],
            Some(DateTime::from_timestamp(1_000, 0).unwrap()),
        );
        write_file(
            root,
            "rollout-beta.jsonl",
            &[
                session_meta("beta", "2026-07-29T02:00:00.000Z"),
                codex_state_line("2026-07-29T02:00:01.000Z", 20, 2, 20, 2),
            ],
            Some(DateTime::from_timestamp(1_000, 0).unwrap()),
        );
        write_file(
            root,
            "rollout-child.jsonl",
            &orphaned_fork_lines(20),
            Some(DateTime::from_timestamp(3_000, 0).unwrap()),
        );
    }

    fn outputs(entries: &[reader::Entry]) -> Vec<i64> {
        entries.iter().map(|e| e.output).collect()
    }

    // MARK: - Claude: incremental parse + persistence

    /// Same `(mtime, size)` → no re-parse. Content swapped behind the cache's
    /// back (same length, same mtime) must still return the cached value.
    #[test]
    fn unchanged_file_is_not_reparsed() {
        let base = temp_dir();
        let root = project_root(&base);
        let t = recent_second() - Duration::hours(1);
        let ts = "2026-07-02T01:00:00.000Z";
        write_file(&root, "a.jsonl", &[claude_line("1", 111, ts)], Some(t));

        let mut cache = make_cache(&root);
        assert_eq!(outputs(&cache.claude_entries(epoch())), vec![111]);

        write_file(&root, "a.jsonl", &[claude_line("1", 222, ts)], Some(t));
        assert_eq!(
            outputs(&cache.claude_entries(epoch())),
            vec![111],
            "mtime/size unchanged — must not re-parse"
        );
    }

    #[test]
    fn changed_file_is_reparsed() {
        let base = temp_dir();
        let root = project_root(&base);
        let t = recent_second() - Duration::hours(1);
        let ts = "2026-07-02T01:00:00.000Z";
        write_file(&root, "a.jsonl", &[claude_line("1", 111, ts)], Some(t));

        let mut cache = make_cache(&root);
        cache.claude_entries(epoch());

        write_file(
            &root,
            "a.jsonl",
            &[claude_line("1", 999, ts)],
            Some(t + Duration::seconds(10)),
        );
        assert_eq!(outputs(&cache.claude_entries(epoch())), vec![999]);
    }

    /// Production scans several roots (CLI default + CLAUDE_CONFIG_DIR + Claude
    /// Desktop embedded sessions). A single injected root short-circuits the
    /// multi-root loop, so test it explicitly.
    #[test]
    fn multiple_roots_are_scanned_and_deduped_across_roots() {
        let base = temp_dir();
        let root = project_root(&base);
        let second = base.join("second");
        std::fs::create_dir_all(&second).unwrap();
        let ts = "2026-07-02T01:00:00.000Z";

        write_file(&root, "a.jsonl", &[claude_line("shared", 10, ts)], None);
        write_file(
            &second,
            "b.jsonl",
            &[
                claude_line("shared", 10, ts),
                claude_line("only-second", 7, ts),
            ],
            None,
        );

        let mut cache = LocalUsageCache::new(
            Some(vec![root.clone(), second.clone()]),
            None,
            None,
            None,
            None,
            base.join("usage-cache.json"),
            Utc::now,
            counting_probe,
        );
        let entries = cache.claude_entries(epoch());
        assert_eq!(
            entries.iter().map(|e| e.output).collect::<HashSet<_>>(),
            HashSet::from([10, 7]),
            "both roots must be summed"
        );
        assert_eq!(
            entries
                .iter()
                .filter(|e| e.id == "m-shared|r-shared")
                .count(),
            1,
            "a turn in two roots must be counted once"
        );

        // Control: dropping the second root removes only-second — proves the
        // multi-root branch was actually exercised.
        let mut single = LocalUsageCache::new(
            None,
            Some(root),
            None,
            None,
            None,
            base.join("usage-cache-single.json"),
            Utc::now,
            counting_probe,
        );
        assert_eq!(single.claude_entries(epoch()).len(), 1);
    }

    /// Disk persistence: a new instance (cold start) loads the snapshot and
    /// returns the same result instead of re-parsing.
    #[test]
    fn disk_round_trip_across_instances() {
        let base = temp_dir();
        let root = project_root(&base);
        let t = recent_second() - Duration::hours(1);
        let ts = "2026-07-02T01:00:00.000Z";
        write_file(&root, "a.jsonl", &[claude_line("1", 42, ts)], Some(t));
        let cache_file = base.join("usage-cache.json");

        {
            let mut c1 = make_cache(&root);
            assert_eq!(outputs(&c1.claude_entries(epoch())), vec![42]);
        }
        assert!(cache_file.exists(), "snapshot must be persisted");

        // Content swapped behind the cache's back (same length, same mtime) —
        // a fresh instance must use the disk snapshot (42), not re-parse (43).
        write_file(&root, "a.jsonl", &[claude_line("1", 43, ts)], Some(t));
        let mut c2 = make_cache(&root);
        assert_eq!(outputs(&c2.claude_entries(epoch())), vec![42]);
    }

    #[test]
    fn prune_drops_blobs_older_than_40_days() {
        let _guard = STATE_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let base = temp_dir();
        let root = project_root(&base);
        let ts = "2026-07-02T01:00:00.000Z";
        FAKE_NOW.store(1_700_000_000, Ordering::SeqCst);
        let now = fake_now();
        let old = now - Duration::days(45);
        write_file(&root, "old.jsonl", &[claude_line("o", 1, ts)], Some(old));
        write_file(&root, "new.jsonl", &[claude_line("n", 2, ts)], Some(now));

        let mut cache = make_cache_with(&root, fake_now);
        assert_eq!(
            outputs(&cache.claude_entries(epoch()))
                .into_iter()
                .collect::<HashSet<_>>(),
            HashSet::from([1, 2]),
            "the query itself returns both"
        );

        let snap = std::fs::read_to_string(base.join("usage-cache.json")).unwrap();
        assert!(
            !snap.contains("old.jsonl"),
            "45-day-old blob must be pruned from the snapshot"
        );
        assert!(snap.contains("new.jsonl"));
    }

    /// Save throttle: no write within 60s, write after (injected clock).
    #[test]
    fn save_throttle_60s() {
        let _guard = STATE_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let base = temp_dir();
        let root = project_root(&base);
        let cache_file = base.join("usage-cache.json");
        let ts = "2026-07-02T01:00:00.000Z";
        FAKE_NOW.store(1_700_000_000, Ordering::SeqCst);

        write_file(
            &root,
            "a.jsonl",
            &[claude_line("1", 1, ts)],
            Some(fake_now()),
        );
        let mut cache = make_cache_with(&root, fake_now);
        cache.claude_entries(epoch()); // first save
        let first_snap = std::fs::read(&cache_file).unwrap();

        // 30s later the file changes → dirty, but the throttle skips the save.
        FAKE_NOW.store(1_700_000_030, Ordering::SeqCst);
        write_file(
            &root,
            "a.jsonl",
            &[claude_line("1", 2, ts)],
            Some(fake_now()),
        );
        cache.claude_entries(epoch());
        assert_eq!(
            std::fs::read(&cache_file).unwrap(),
            first_snap,
            "save within 60s must be skipped"
        );

        // 91s after the first save → written.
        FAKE_NOW.store(1_700_000_091, Ordering::SeqCst);
        cache.claude_entries(epoch());
        assert_ne!(
            std::fs::read(&cache_file).unwrap(),
            first_snap,
            "throttle released → saved"
        );
    }

    #[test]
    fn modified_since_filters() {
        let base = temp_dir();
        let root = project_root(&base);
        let ts = "2026-07-02T01:00:00.000Z";
        let now = recent_second();
        let old = now - Duration::days(10);
        write_file(&root, "old.jsonl", &[claude_line("o", 1, ts)], Some(old));
        write_file(&root, "new.jsonl", &[claude_line("n", 2, ts)], Some(now));

        let mut cache = make_cache(&root);
        assert_eq!(
            outputs(&cache.claude_entries(now - Duration::days(1))),
            vec![2]
        );
    }

    // MARK: - Codex: parsed-rollout caching, session-id index, prune interplay

    /// A fork burst (parent absent → time fallback) collapses to the one real
    /// turn. Exercises the whole codex pipeline through the cache.
    #[test]
    fn codex_cache_drops_forked_replay_burst() {
        let base = temp_dir();
        let root = project_root(&base);
        write_file(
            &root,
            "rollout-child.jsonl",
            &[
                forked_session_meta("2026-07-29T01:00:00.000Z"),
                codex_line("2026-07-29T01:00:00.010Z", 50),
                codex_line("2026-07-29T01:00:00.020Z", 51),
                codex_line("2026-07-29T01:00:03.000Z", 52),
            ],
            None,
        );

        let mut cache = make_cache(&root);
        let entries = cache.codex_entries(epoch());
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].output, 52);
    }

    /// An old snapshot's codex blobs must not be trusted once the parser
    /// version has moved on — re-parse instead of reusing a stale blob.
    #[test]
    fn codex_cache_invalidates_outdated_parser_version() {
        let base = temp_dir();
        let root = project_root(&base);
        write_file(
            &root,
            "rollout-child.jsonl",
            &[
                forked_session_meta("2026-07-29T01:00:00.000Z"),
                codex_line("2026-07-29T01:00:00.010Z", 50),
                codex_line("2026-07-29T01:00:00.020Z", 51),
                codex_line("2026-07-29T01:00:03.000Z", 52),
            ],
            None,
        );
        let cache_file = base.join("usage-cache.json");
        {
            let mut cache = make_cache(&root);
            cache.codex_entries(epoch());
        }
        rewrite_codex_cache_as_prior_parser_version(&cache_file);

        let mut cache = make_cache(&root);
        let entries = cache.codex_entries(epoch());
        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0].output, 52,
            "stale parser-version blob must be discarded"
        );
    }

    fn rewrite_codex_cache_as_prior_parser_version(cache_file: &Path) {
        let raw = std::fs::read(cache_file).unwrap();
        let mut snap: Value = serde_json::from_slice(&raw).unwrap();
        let codex = snap
            .get_mut("codex")
            .and_then(Value::as_object_mut)
            .unwrap();
        for (_, blob) in codex.iter_mut() {
            let rollout = blob
                .get_mut("rollout")
                .and_then(Value::as_object_mut)
                .unwrap();
            let events = rollout
                .get_mut("events")
                .and_then(Value::as_array_mut)
                .unwrap();
            let last = events.last_mut().unwrap();
            let entry = last
                .get_mut("entry")
                .and_then(Value::as_object_mut)
                .unwrap();
            entry.insert("output".to_string(), Value::from(999));
        }
        snap["codexParserVersion"] = Value::from(3);
        std::fs::write(cache_file, serde_json::to_vec(&snap).unwrap()).unwrap();
    }

    /// The fact that a parent was not found is state too — the second refresh
    /// must not reopen the same files.
    #[test]
    fn codex_orphaned_parent_is_not_rescanned_on_every_refresh() {
        let _guard = STATE_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let base = temp_dir();
        let root = project_root(&base);
        write_orphaned_fork_tree(&root);
        reset_probe_state();
        let window = DateTime::from_timestamp(2_000, 0).unwrap();

        let mut cache = make_cache(&root);
        cache.codex_entries(window);
        let first = PROBE_COUNT.load(Ordering::SeqCst);
        assert_eq!(
            first, 2,
            "first lookup must probe the two out-of-window rollouts"
        );

        cache.codex_entries(window);
        assert_eq!(
            PROBE_COUNT.load(Ordering::SeqCst),
            first,
            "second refresh must not reopen files"
        );
    }

    /// The index ships in the snapshot, so a cold start filters candidates
    /// without a full scan.
    #[test]
    fn codex_session_index_persists_across_instances() {
        let _guard = STATE_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let base = temp_dir();
        let root = project_root(&base);
        write_orphaned_fork_tree(&root);
        let window = DateTime::from_timestamp(2_000, 0).unwrap();

        reset_probe_state();
        make_cache(&root).codex_entries(window);

        reset_probe_state();
        let mut cache = make_cache(&root);
        cache.codex_entries(window);
        assert_eq!(
            PROBE_COUNT.load(Ordering::SeqCst),
            0,
            "snapshot session-id index must filter candidates"
        );
    }

    /// `session_id == None` is a completed probe — confusing it with "not yet
    /// searched" would reopen the file every refresh.
    #[test]
    fn codex_negative_session_id_probe_persists_across_refreshes_and_instances() {
        let _guard = STATE_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let base = temp_dir();
        let root = project_root(&base);
        write_orphaned_fork_tree(&root);
        write_file(
            &root,
            "rollout-without-session-meta.jsonl",
            &[r#"{"type":"response_item","timestamp":"2026-07-29T00:00:00.000Z","payload":{"type":"message"}}"#.to_string()],
            Some(DateTime::from_timestamp(1_000, 0).unwrap()),
        );
        let window = DateTime::from_timestamp(2_000, 0).unwrap();

        reset_probe_state();
        let mut cache = make_cache(&root);
        cache.codex_entries(window);
        let first = PROBE_COUNT.load(Ordering::SeqCst);
        assert_eq!(first, 3);

        cache.codex_entries(window);
        assert_eq!(
            PROBE_COUNT.load(Ordering::SeqCst),
            first,
            "negative probe reused in same instance"
        );

        reset_probe_state();
        let mut restored = make_cache(&root);
        restored.codex_entries(window);
        assert_eq!(
            PROBE_COUNT.load(Ordering::SeqCst),
            0,
            "negative probe restored from snapshot"
        );
    }

    /// A read failure differs from "no session id": it is not persisted, so the
    /// next refresh retries — but within one refresh it is opened only once
    /// even across multiple orphaned parents.
    #[test]
    fn codex_probe_read_failure_is_retried_instead_of_persisted() {
        let _guard = STATE_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let base = temp_dir();
        let root = project_root(&base);
        write_orphaned_fork_tree(&root);
        write_file(
            &root,
            "rollout-child2.jsonl",
            &[
                fork_meta("child2", "gone-parent-2", "2026-07-30T02:00:00.000Z"),
                codex_state_line("2026-07-30T02:00:05.000Z", 10, 1, 10, 1),
            ],
            Some(DateTime::from_timestamp(3_000, 0).unwrap()),
        );
        reset_probe_state();
        set_fail_once(&["rollout-alpha.jsonl"]);
        let window = DateTime::from_timestamp(2_000, 0).unwrap();
        let mut cache = make_cache(&root);

        cache.codex_entries(window);
        assert_eq!(
            call_count("rollout-alpha.jsonl"),
            1,
            "same refresh must not reopen a failed file"
        );
        assert_eq!(
            call_count("rollout-beta.jsonl"),
            1,
            "successful probe reused in the index"
        );

        cache.codex_entries(window);
        assert_eq!(
            call_count("rollout-alpha.jsonl"),
            2,
            "read failure is not frozen — retried next refresh"
        );
        assert_eq!(call_count("rollout-beta.jsonl"), 1);

        cache.codex_entries(window);
        assert_eq!(
            call_count("rollout-alpha.jsonl"),
            2,
            "after the retry succeeded it is not reopened"
        );
    }

    /// A failed probe is also absent from the on-disk snapshot — a new instance
    /// must try the file again.
    #[test]
    fn codex_probe_read_failure_is_not_persisted_to_snapshot() {
        let _guard = STATE_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let base = temp_dir();
        let root = project_root(&base);
        write_orphaned_fork_tree(&root);
        let window = DateTime::from_timestamp(2_000, 0).unwrap();

        reset_probe_state();
        set_fail_once(&["rollout-alpha.jsonl"]);
        {
            let mut cache = make_cache(&root);
            cache.codex_entries(window);
        }

        // The second instance is a fresh meter (the Swift test uses two probe
        // instances) — zero the counters so the assertions see only its calls.
        reset_probe_state();
        let mut restored = make_cache(&root);
        restored.codex_entries(window);
        assert_eq!(
            call_count("rollout-alpha.jsonl"),
            1,
            "failure not persisted — new instance retries"
        );
        assert_eq!(
            call_count("rollout-beta.jsonl"),
            0,
            "successful probe restored from snapshot"
        );
    }

    /// Blobs are 40-day-pruned, the session-id index is not — finding parents
    /// older than the prune window is its purpose.
    #[test]
    fn codex_session_index_outlives_blob_prune() {
        let _guard = STATE_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let base = temp_dir();
        let root = project_root(&base);
        write_orphaned_fork_tree(&root);
        let window = DateTime::from_timestamp(2_000, 0).unwrap();

        reset_probe_state();
        {
            let mut cache = make_cache(&root);
            cache.codex_entries(window);
        }

        // If the blob had survived, this same-mtime/same-size rewrite would be
        // ignored (220 kept). A pruned blob re-parses (230).
        write_file(
            &root,
            "rollout-child.jsonl",
            &orphaned_fork_lines(30),
            Some(DateTime::from_timestamp(3_000, 0).unwrap()),
        );

        reset_probe_state();
        let mut cache = make_cache(&root);
        let entries = cache.codex_entries(window);
        assert_eq!(
            entries.iter().map(|e| e.total()).collect::<Vec<_>>(),
            vec![230],
            "blob pruned → must re-parse"
        );
        assert_eq!(
            PROBE_COUNT.load(Ordering::SeqCst),
            0,
            "index alone filters parents after blob prune"
        );
    }

    /// Index entries for deleted rollouts are removed, and the removal reaches
    /// the snapshot.
    #[test]
    fn codex_session_index_drops_deleted_files() {
        let _guard = STATE_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let base = temp_dir();
        let root = project_root(&base);
        write_orphaned_fork_tree(&root);
        let window = DateTime::from_timestamp(2_000, 0).unwrap();

        FAKE_NOW.store(10_000_000, Ordering::SeqCst);
        let mut cache = make_cache_with(&root, fake_now);
        cache.codex_entries(window);
        assert_eq!(cache.codex_session_index_count(), 3);

        std::fs::remove_file(root.join("rollout-alpha.jsonl")).unwrap();
        FAKE_NOW.store(10_000_120, Ordering::SeqCst); // past the 60s save throttle
        cache.codex_entries(window);
        assert_eq!(cache.codex_session_index_count(), 2);
        assert_eq!(
            make_cache_with(&root, fake_now).codex_session_index_count(),
            2,
            "deletion must be reflected in the snapshot"
        );
    }

    /// A parsed rollout round-trips across instances: same mtime/size keeps the
    /// persisted rollout, not a re-parse of swapped content.
    #[test]
    fn codex_parsed_rollout_cache_round_trips_across_instances() {
        let base = temp_dir();
        let root = project_root(&base);
        let mtime = recent_second() - Duration::hours(1);
        let lines = vec![
            session_meta("session-a", "2026-07-30T01:00:00.000Z"),
            codex_state_line("2026-07-30T01:00:01.000Z", 100, 10, 100, 10),
        ];
        write_file(&root, "rollout-session.jsonl", &lines, Some(mtime));

        let mut cache1 = make_cache(&root);
        assert_eq!(
            cache1
                .codex_entries(epoch())
                .iter()
                .map(|e| e.total())
                .collect::<Vec<_>>(),
            vec![110]
        );

        // Same length, same mtime, but 10→20 — a fresh instance must reuse the
        // persisted rollout (110), not re-parse (120).
        let changed = vec![
            session_meta("session-a", "2026-07-30T01:00:00.000Z"),
            codex_state_line("2026-07-30T01:00:01.000Z", 100, 20, 100, 20),
        ];
        write_file(&root, "rollout-session.jsonl", &changed, Some(mtime));
        let mut cache2 = make_cache(&root);
        assert_eq!(
            cache2
                .codex_entries(epoch())
                .iter()
                .map(|e| e.total())
                .collect::<Vec<_>>(),
            vec![110]
        );
    }

    /// An index-version bump rebuilds only the index — parsed-rollout blobs are
    /// reused (not a full re-parse).
    #[test]
    fn codex_session_index_version_bump_rebuilds_index_without_reparsing_blobs() {
        let _guard = STATE_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let base = temp_dir();
        let root = project_root(&base);
        let second = recent_second();
        let outside_window = DateTime::from_timestamp(second.timestamp() - 7_200, 0).unwrap();
        let inside_window = DateTime::from_timestamp(second.timestamp() - 3_600, 0).unwrap();
        let window = DateTime::from_timestamp(second.timestamp() - 5_400, 0).unwrap();

        write_file(
            &root,
            "rollout-alpha.jsonl",
            &[
                session_meta("alpha", "2026-07-29T01:00:00.000Z"),
                codex_state_line("2026-07-29T01:00:01.000Z", 10, 1, 10, 1),
            ],
            Some(outside_window),
        );
        write_file(
            &root,
            "rollout-child.jsonl",
            &orphaned_fork_lines(20),
            Some(inside_window),
        );
        let cache_file = base.join("usage-cache.json");
        {
            let mut cache = make_cache(&root);
            cache.codex_entries(window);
        }
        downgrade_codex_session_index_version(&cache_file);

        // Blob alive → this same-mtime/same-size rewrite is ignored (220 kept).
        write_file(
            &root,
            "rollout-child.jsonl",
            &orphaned_fork_lines(30),
            Some(inside_window),
        );

        reset_probe_state();
        let mut cache = make_cache(&root);
        let entries = cache.codex_entries(window);
        assert_eq!(
            PROBE_COUNT.load(Ordering::SeqCst),
            1,
            "old index entries are untrustworthy → re-probed"
        );
        assert_eq!(
            entries.iter().map(|e| e.total()).collect::<Vec<_>>(),
            vec![220],
            "blob reused — not a full re-parse"
        );
    }

    fn downgrade_codex_session_index_version(cache_file: &Path) {
        let raw = std::fs::read(cache_file).unwrap();
        let mut snap: Value = serde_json::from_slice(&raw).unwrap();
        // serde camelCase renders `codex_session_ids` as `codexSessionIds`.
        let ids = snap
            .get("codexSessionIds")
            .and_then(Value::as_object)
            .unwrap();
        assert!(!ids.is_empty(), "index under test must be non-empty");
        snap["codexSessionIndexVersion"] = Value::from(1);
        std::fs::write(cache_file, serde_json::to_vec(&snap).unwrap()).unwrap();
    }
}
