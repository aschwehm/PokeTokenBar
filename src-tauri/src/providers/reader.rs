//! Local usage-log reader — Claude/Codex/Gemini/Grok local log parsing and
//! aggregation (port of the original `Core/LocalUsageReader.swift`).
//!
//! - Claude: `~/.claude/projects/**/*.jsonl` `type:"assistant"` lines
//!   (`message.usage` 4-token kinds, `message.model`, `message.id`+`requestId`,
//!   `timestamp`). Same message re-logged across files (session resume /
//!   sidechain) is deduped by `(message.id, requestId)`.
//! - Codex: `~/.codex/sessions/**/rollout-*.jsonl` `event_msg.payload.type:
//!   "token_count"` (`info.last_token_usage` turn delta) summed.
//!
//! Performance: mtime windows limit the scanned files (a file modified before
//! the window start cannot contain entries in the window).
//!
//! **Week start**: the original uses `Calendar.current` (locale-dependent); the
//! tests only require that week-start is sometimes before month-start and that
//! a mid-month `enrichmentScanStart` equals month-start, which both Monday and
//! Sunday satisfy. This port uses **Monday / ISO** (`num_days_from_monday`).

use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Instant;

use chrono::{DateTime, Datelike, Local, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use walkdir::WalkDir;

use crate::domain::decoding::parse_iso8601;
use crate::domain::models::{BlockUsage, DailyUsage, PeriodUsage};
use crate::domain::pricing::ModelPricing;

/// Active-block (burn rate) rolling window length shared with the enrichment
/// scan floor — 5 hours.
pub const BLOCK_WINDOW_SECS: i64 = 5 * 3600;
/// Fork replay records are written milliseconds apart. The first gap longer
/// than this marks the first real child turn.
const FORK_REPLAY_MAXIMUM_GAP: f64 = 1.0;
/// Parsing ceiling — 100,000× real-world usage, so normal usage is never cut.
/// `i64::MAX` would clamp but the *immediately-after* additions
/// (`output + thoughts`, `input − cached + tool`) would trap again; this bound
/// stays inside i64 when summed multiple times.
pub const MAX_PARSED_TOKEN_VALUE: i64 = 1_000_000_000_000_000;
/// Total bytes the metadata probe reads. A `session_meta` first line is ~22 KB
/// median / ~46 KB worst, so 1 MiB is ~22× headroom.
pub const CODEX_PROBE_BYTE_LIMIT: usize = 1 << 20;
const CODEX_PROBE_CHUNK_SIZE: usize = 64 * 1024;

/// Local-day format used for `Entry.local_day` (port of `localDayFormatter`).
const LOCAL_DAY_FORMAT: &str = "%Y-%m-%d";

// MARK: - Normalized record

/// One usage record. Mirrors `LocalUsageReader.Entry`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub id: String,
    pub date: DateTime<Utc>,
    pub local_day: String,
    pub model: String,
    pub input: i64,
    pub output: i64,
    pub cache_write: i64,
    pub cache_read: i64,
    /// Some agents persist the exact charge alongside token usage. Prefer it
    /// over model-table pricing when present so local reports match the source
    /// of truth.
    pub explicit_cost: Option<f64>,
}

impl Entry {
    pub fn total(&self) -> i64 {
        self.input + self.output + self.cache_write + self.cache_read
    }
}

/// Summing bucket shared by `daily` / `period` / `active_block`.
#[derive(Debug, Clone, Default)]
pub struct Bucket {
    pub input: i64,
    pub output: i64,
    pub cache_write: i64,
    pub cache_read: i64,
    pub cost: f64,
}

impl Bucket {
    pub fn total(&self) -> i64 {
        self.input + self.output + self.cache_write + self.cache_read
    }

    pub fn add(&mut self, e: &Entry) {
        self.input += e.input;
        self.output += e.output;
        self.cache_write += e.cache_write;
        self.cache_read += e.cache_read;
        self.cost += match e.explicit_cost {
            Some(c) if c > 0.0 => c,
            _ => ModelPricing::cost(&e.model, e.input, e.output, e.cache_write, e.cache_read),
        };
    }
}

// MARK: - Numeric / boolean helpers

/// A missing key, JSON `null`, or a non-number is `None` — a `null` must not
/// pass through as "present" (that would zero out a cache read and eat input).
fn field<'a>(obj: Option<&'a Map<String, Value>>, key: &str) -> &'a Value {
    obj.and_then(|m| m.get(key)).unwrap_or(&Value::Null)
}

/// `doubleOrNil` — only JSON numbers count; reject `null`/strings/bools.
pub fn double_or_nil(v: &Value) -> Option<f64> {
    let d = v.as_f64()?;
    if d.is_finite() {
        Some(d)
    } else {
        None
    }
}

/// `intOrNil` — a number → i64, clamped to `MAX_PARSED_TOKEN_VALUE`; negative
/// and zero collapse to 0; non-numbers → `None`.
pub fn int_or_nil(v: &Value) -> Option<i64> {
    let d = double_or_nil(v)?;
    if d <= 0.0 {
        return Some(0);
    }
    if d >= MAX_PARSED_TOKEN_VALUE as f64 {
        return Some(MAX_PARSED_TOKEN_VALUE);
    }
    Some(d as i64)
}

/// `intValue` — `intOrNil` folded to 0 on absence.
pub fn int_value(v: &Value) -> i64 {
    int_or_nil(v).unwrap_or(0)
}

/// `boolValue` — only a JSON boolean counts.
pub fn bool_value(v: &Value) -> bool {
    v.as_bool().unwrap_or(false)
}

/// `nonEmpty` — trimmed whitespace; `None` when empty.
pub fn non_empty(v: &str) -> Option<String> {
    let t = v.trim();
    if t.is_empty() {
        None
    } else {
        Some(t.to_string())
    }
}

// MARK: - Paths & roots

pub fn default_relative_projects_path() -> &'static str {
    ".claude/projects"
}

pub fn config_relative_projects_path() -> &'static str {
    ".config/claude/projects"
}

pub fn claude_projects_dir(home: &Path) -> PathBuf {
    home.join(default_relative_projects_path())
}

/// User home directory: `$HOME` on Unix, `%USERPROFILE%` on Windows.
pub fn home_dir() -> Option<PathBuf> {
    crate::platform::binary_locator::home_dir()
}

/// Every `projects` root that could hold Claude usage logs. Config dir parts
/// are comma-separated, tilde-expanded, each with `projects` appended; then the
/// two CLI default locations; then Claude Desktop embedded session stores.
pub fn compute_claude_project_roots(config_dir_value: Option<&str>, home: &Path) -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();
    if let Some(raw) = config_dir_value {
        for part in raw.split(',') {
            let trimmed = part.trim();
            if trimmed.is_empty() {
                continue;
            }
            let expanded = if trimmed == "~" {
                home.to_path_buf()
            } else if let Some(rest) = trimmed.strip_prefix("~/") {
                home.join(rest)
            } else {
                PathBuf::from(trimmed)
            };
            roots.push(expanded.join("projects"));
        }
    }
    roots.push(home.join(config_relative_projects_path()));
    roots.push(home.join(default_relative_projects_path()));

    let desktop = home.join("Library/Application Support/Claude");
    for store in ["local-agent-mode-sessions", "claude-code-sessions"] {
        roots.extend(embedded_claude_project_roots(&desktop.join(store), 7));
    }
    normalized_roots(&roots)
}

/// `CLAUDE_CONFIG_DIR` via `UsageEnvironment` (process env + login shell).
pub fn shell_aware_claude_config_dir() -> Option<String> {
    crate::platform::env::value("CLAUDE_CONFIG_DIR")
}

static ROOTS_CACHE: Mutex<Option<(Vec<PathBuf>, Instant)>> = Mutex::new(None);

/// Cached `compute_claude_project_roots` (300 s TTL). The lock only guards the
/// cache fields; the (login-shell + filesystem) computation runs outside the
/// lock, so concurrent first callers re-compute rather than block.
pub fn claude_project_roots() -> Vec<PathBuf> {
    {
        let cache = ROOTS_CACHE.lock().unwrap_or_else(|p| p.into_inner());
        if let Some((roots, at)) = cache.as_ref() {
            if at.elapsed().as_secs() < 300 {
                return roots.clone();
            }
        }
    }
    let home = home_dir().unwrap_or_else(|| PathBuf::from("."));
    let fresh = compute_claude_project_roots(shell_aware_claude_config_dir().as_deref(), &home);
    let mut cache = ROOTS_CACHE.lock().unwrap_or_else(|p| p.into_inner());
    *cache = Some((fresh.clone(), Instant::now()));
    fresh
}

/// Directories we never descend into during the embedded-root scan. Only
/// non-user-worktree names (packages/VCS). Work-directory names like
/// `uploads`/`outputs`/`build`/`target` are deliberately NOT here — Claude
/// sessions run inside them legitimately.
const ROOT_SCAN_SKIPPED_DIRECTORIES: [&str; 4] = ["node_modules", ".git", "venv", ".venv"];

/// Find `.claude/projects` directories inside a Claude Desktop embedded-session
/// store. Session paths nest UUID layers (`<store>/<uuid>/<uuid>/local_<uuid>/
/// .claude/projects`), so a fixed path cannot reach them and `.claude` is
/// hidden — `skipsHiddenFiles` would miss it. Depth upper bound is the primary
/// width control (default 7; the real layout reaches 5, a repo under a session
/// workdir reaches 7).
pub fn embedded_claude_project_roots(base: &Path, max_depth: usize) -> Vec<PathBuf> {
    if !base.is_dir() {
        return vec![];
    }
    let mut found: Vec<PathBuf> = Vec::new();
    let mut pruned_by_depth = false;
    for _entry in WalkDir::new(base)
        .max_depth(max_depth)
        .into_iter()
        .filter_entry(|e| {
            let depth = e.depth();
            let name = e.file_name().to_string_lossy();
            let is_dir = e.file_type().is_dir();
            // A `projects` dir whose parent is `.claude` is a root; record it
            // and skip its descendants (project logs are not root candidates).
            if is_dir && name == "projects" {
                if let Some(parent) = e.path().parent() {
                    if parent.file_name().map(|n| n == ".claude").unwrap_or(false) {
                        found.push(e.path().to_path_buf());
                        return false;
                    }
                }
            }
            // Prune at >= max_depth *after* the projects check, so a root that
            // sits exactly on the boundary is still recorded.
            if depth >= max_depth {
                pruned_by_depth = true;
                return false;
            }
            if is_dir && ROOT_SCAN_SKIPPED_DIRECTORIES.contains(&name.as_ref()) {
                return false;
            }
            true
        })
    {
        let _ = _entry;
    }
    if pruned_by_depth {
        crate::platform::log::write(&format!(
            "claude desktop scan: depth {max_depth} reached under {}, found {} root(s) — deeper roots may be missed",
            base
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            found.len()
        ));
    }
    found
}

/// Lexically normalize `.`/`..` components (the `standardizedFileURL` half of
/// the Swift path normalization), used when the path does not exist.
fn lexical_normalize(path: &Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// `resolvingSymlinksInPath().standardizedFileURL.path` — canonicalize when the
/// path exists (resolves symlinks), else lexically standardize.
fn standardized_resolved(path: &Path) -> PathBuf {
    match path.canonicalize() {
        Ok(p) => p,
        Err(_) => lexical_normalize(path),
    }
}

/// Dedup + nesting fold for project roots. Dedup by resolved path compared
/// case-insensitively; then keep only paths that are not a strict descendant of
/// an already-kept path (`p == k || p.starts_with(k + "/")`); the original
/// priority order is preserved.
pub fn normalized_roots(roots: &[PathBuf]) -> Vec<PathBuf> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut unique: Vec<String> = Vec::new();
    for root in roots {
        let path = standardized_resolved(root);
        let lowered = path.to_string_lossy().to_lowercase();
        if seen.insert(lowered) {
            unique.push(path.to_string_lossy().into_owned());
        }
    }
    let mut kept: Vec<String> = Vec::new();
    let mut sorted: Vec<String> = unique.clone();
    sorted.sort_by_key(|p| p.len());
    for path in sorted {
        let p = path.to_lowercase();
        let nested = kept.iter().any(|k| {
            let k = k.to_lowercase();
            p == k || p.starts_with(&format!("{k}/")) || p.starts_with(&format!("{k}\\"))
        });
        if !nested {
            kept.push(path);
        }
    }
    unique
        .into_iter()
        .filter(|p| kept.contains(p))
        .map(PathBuf::from)
        .collect()
}

pub fn codex_sessions_dir(home: &Path) -> PathBuf {
    home.join(".codex/sessions")
}

pub fn gemini_tmp_dir(home: &Path) -> PathBuf {
    home.join(".gemini/tmp")
}

/// Grok session root: `$GROK_HOME/sessions` when set, else `~/.grok/sessions`.
pub fn grok_sessions_dir() -> PathBuf {
    if let Some(home) = crate::platform::env::value("GROK_HOME") {
        let trimmed = home.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed).join("sessions");
        }
    }
    home_dir()
        .map(|h| h.join(".grok/sessions"))
        .unwrap_or_default()
}

fn is_hidden_name(name: &std::ffi::OsStr) -> bool {
    name.to_string_lossy().starts_with('.')
}

// MARK: - Scan (mtime window)

/// Recursive `.jsonl` files under `root` modified at/after `modified_since`
/// (`.json` too when `allow_json` — Gemini only). Hidden items are skipped
/// (`.skipsHiddenFiles`), but never the root itself.
pub fn jsonl_files(root: &Path, modified_since: DateTime<Utc>, allow_json: bool) -> Vec<PathBuf> {
    let mut out = Vec::new();
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
        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        let mtime = match meta.modified() {
            Ok(m) => m,
            Err(_) => continue,
        };
        let mtime = DateTime::<Utc>::from(mtime);
        if mtime >= modified_since {
            out.push(entry.path().to_path_buf());
        }
    }
    out.sort();
    out
}

// MARK: - Claude parsing

/// Same `(message.id, requestId)` can be logged several times by streaming /
/// resume — cacheRead/input stay fixed while output grows — so the entry with
/// the largest `total` (= the completed one) wins (global dedup).
pub fn dedup_keep_max(entries: Vec<Entry>) -> Vec<Entry> {
    let mut by_id: HashMap<String, Entry> = HashMap::new();
    for e in entries {
        match by_id.get(&e.id) {
            Some(existing) => {
                if e.total() > existing.total() {
                    by_id.insert(e.id.clone(), e);
                }
            }
            None => {
                by_id.insert(e.id.clone(), e);
            }
        }
    }
    by_id.into_values().collect()
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Local day string of a UTC instant (port of `localDayFormatter`).
fn day(date: DateTime<Utc>, fmt: &str) -> String {
    date.with_timezone(&Local).format(fmt).to_string()
}

/// `local_day` with the standard `%Y-%m-%d` format.
pub fn local_day(date: DateTime<Utc>) -> String {
    day(date, LOCAL_DAY_FORMAT)
}

/// Returns the local-day helper used across the parser entry points (port of
/// `LocalUsageReader.localDayFormatter()`).
pub fn local_day_formatter() -> fn(DateTime<Utc>) -> String {
    local_day
}

/// Parse one Claude file (with in-file dedup). The cache calls this per file.
pub fn parse_claude_file(path: &Path, fmt: &str) -> Vec<Entry> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(_) => return Vec::new(),
    };
    let mut out = Vec::new();
    for line in text.lines() {
        if line.contains("\"usage\"") && line.contains("\"assistant\"") {
            if let Some(e) = parse_claude_line(line, fmt) {
                out.push(e);
            }
        }
    }
    dedup_keep_max(out)
}

fn parse_claude_line(line: &str, fmt: &str) -> Option<Entry> {
    let obj: Value = serde_json::from_str(line).ok()?;
    if obj.get("type").and_then(Value::as_str) != Some("assistant") {
        return None;
    }
    let msg = obj.get("message")?.as_object()?;
    let usage = msg.get("usage")?.as_object()?;
    let ts = obj.get("timestamp")?.as_str()?;
    let date = parse_iso8601(ts)?;
    let model = msg
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string();
    let id = format!(
        "{}|{}",
        msg.get("id").and_then(Value::as_str).unwrap_or(""),
        obj.get("requestId").and_then(Value::as_str).unwrap_or("")
    );
    Some(Entry {
        id,
        date,
        local_day: day(date, fmt),
        model,
        input: int_value(field(Some(usage), "input_tokens")),
        output: int_value(field(Some(usage), "output_tokens")),
        cache_write: int_value(field(Some(usage), "cache_creation_input_tokens")),
        cache_read: int_value(field(Some(usage), "cache_read_input_tokens")),
        explicit_cost: None,
    })
}

/// Claude usage entries from files modified after `modified_since` across the
/// given roots (global dedup). Same turn copied into several roots is counted
/// once by the `(message.id, requestId)` global dedup.
pub fn claude_entries(modified_since: DateTime<Utc>, roots: &[PathBuf]) -> Vec<Entry> {
    let mut all = Vec::new();
    for root in roots {
        for file in jsonl_files(root, modified_since, false) {
            all.extend(parse_claude_file(&file, LOCAL_DAY_FORMAT));
        }
    }
    dedup_keep_max(all)
}

/// Single-root convenience (mirrors `claudeEntries(modifiedSince:root:)`).
pub fn claude_entries_in_root(modified_since: DateTime<Utc>, root: &Path) -> Vec<Entry> {
    claude_entries(modified_since, &[root.to_path_buf()])
}

// MARK: - Codex parsing

/// Full usage vector; cumulative usage is read with `int_or_nil` so a hostile
/// `1e30` clamps instead of trapping on every launch.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexUsageVector {
    pub input: i64,
    pub cached_input: i64,
    pub cache_write_input: i64,
    pub output: i64,
    pub reasoning_output: i64,
    pub total: i64,
}

impl CodexUsageVector {
    pub fn from_value(v: &Value) -> Self {
        let obj = v.as_object();
        let get = |key: &str| int_or_nil(field(obj, key)).unwrap_or(0);
        Self {
            input: get("input_tokens"),
            cached_input: get("cached_input_tokens"),
            cache_write_input: get("cache_write_input_tokens"),
            output: get("output_tokens"),
            reasoning_output: get("reasoning_output_tokens"),
            total: get("total_tokens"),
        }
    }

    pub fn fingerprint(&self) -> String {
        format!(
            "{},{},{},{},{},{}",
            self.input,
            self.cached_input,
            self.cache_write_input,
            self.output,
            self.reasoning_output,
            self.total
        )
    }

    /// Any field strictly less — a cumulative reset (new epoch).
    pub fn is_lower_than(&self, previous: &Self) -> bool {
        self.input < previous.input
            || self.cached_input < previous.cached_input
            || self.cache_write_input < previous.cache_write_input
            || self.output < previous.output
            || self.reasoning_output < previous.reasoning_output
            || self.total < previous.total
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexUsageState {
    pub cumulative: CodexUsageVector,
    pub last: CodexUsageVector,
}

impl CodexUsageState {
    pub fn fingerprint(&self) -> String {
        format!(
            "{}|{}",
            self.cumulative.fingerprint(),
            self.last.fingerprint()
        )
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexUsageEvent {
    pub entry: Entry,
    pub usage_state: Option<CodexUsageState>,
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexParsedRollout {
    pub path: String,
    pub session_id: Option<String>,
    pub parent_session_id: Option<String>,
    pub forked_at: Option<DateTime<Utc>>,
    pub is_subagent: bool,
    pub events: Vec<CodexUsageEvent>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CodexRolloutFile {
    pub url: PathBuf,
    pub mtime: DateTime<Utc>,
    pub size: u64,
}

impl CodexRolloutFile {
    pub fn path(&self) -> String {
        self.url.to_string_lossy().into_owned()
    }
}

/// Session-id lookup state. `Known(None)` means the probe already ran but found
/// no id — distinct from `Unknown`, so the same file is not reopened every
/// refresh.
#[derive(Debug, Clone, PartialEq)]
pub enum CodexSessionIDKnowledge {
    Unknown,
    Known(Option<String>),
}

struct ParsedCodexToken {
    entry: Entry,
    /// Old records may lack cumulative usage; such records skip same-state
    /// detection.
    usage_state: Option<CodexUsageState>,
}

struct CodexSessionMeta {
    id: Option<String>,
    parent_id: Option<String>,
    date: Option<DateTime<Utc>>,
    is_subagent: bool,
}

#[derive(Debug, Clone)]
struct CodexResolvedEvent {
    #[allow(dead_code)]
    entry: Entry,
    usage_state: Option<CodexUsageState>,
}

#[derive(Debug, Clone)]
struct CodexResolvedRollout {
    history: Vec<CodexResolvedEvent>,
    owned_entries: Vec<Entry>,
}

/// Parse one Codex rollout (session-level — turn deltas of `token_count`
/// events). The cache calls this per file.
pub fn parse_codex_file(path: &Path, fmt: &str) -> Vec<Entry> {
    let rollout = parse_codex_rollout(path, fmt);
    resolve_codex_rollouts(vec![rollout.clone()], HashSet::from([rollout.path]))
}

/// Parse only the information inside one file. Fork-replay trimming needs other
/// rollouts to compare against.
pub fn parse_codex_rollout(path: &Path, fmt: &str) -> CodexParsedRollout {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(_) => {
            return CodexParsedRollout {
                path: path.to_string_lossy().into_owned(),
                session_id: None,
                parent_session_id: None,
                forked_at: None,
                is_subagent: false,
                events: Vec::new(),
            };
        }
    };
    let mut events: Vec<CodexUsageEvent> = Vec::new();
    let mut turn = 0i64;
    let mut session_id: Option<String> = None;
    let mut parent_session_id: Option<String> = None;
    let mut forked_at: Option<DateTime<Utc>> = None;
    let mut is_subagent = false;
    let mut current_session_id: Option<String> = None;
    let mut previous_usage_state: Option<(String, CodexUsageState)> = None;
    // The real model is extracted from the log below (`codexModel`); this value
    // is only the version-agnostic fallback when the session has no model line.
    let mut model = "codex".to_string();

    for line in text.lines() {
        if let Some(meta) = codex_session_meta(line) {
            if session_id.is_none() {
                // A subagent meta's `id` is the child and `session_id` the
                // parent, so `id` wins.
                session_id = meta.id.clone();
                parent_session_id = meta.parent_id.clone();
                forked_at = meta.date;
                is_subagent = meta.is_subagent;
            }
            if let Some(id) = &meta.id {
                if Some(id) != current_session_id.as_ref() {
                    current_session_id = Some(id.clone());
                    previous_usage_state = None;
                }
            }
        }
        if line.contains("\"model\"") {
            if let Some(m) = codex_model(line) {
                model = m;
            }
        }
        if !line.contains("token_count") {
            continue;
        }
        let Some(parsed) = parse_codex_line(line, &file_name(path), turn, &model, fmt) else {
            continue;
        };

        // Codex can re-record the same cumulative/last usage state verbatim.
        // Normalize before replay trimming: consecutive identical full-vector
        // snapshots for the same session contribute no new tokens, so keep one.
        let skip_duplicate = if let Some(state) = &parsed.usage_state {
            if let Some(session) = &current_session_id {
                if let Some((previous_session, previous_state)) = &previous_usage_state {
                    if previous_session == session && previous_state == state {
                        true
                    } else {
                        previous_usage_state = Some((session.clone(), state.clone()));
                        false
                    }
                } else {
                    previous_usage_state = Some((session.clone(), state.clone()));
                    false
                }
            } else {
                previous_usage_state = None;
                false
            }
        } else {
            previous_usage_state = None;
            false
        };
        if skip_duplicate {
            continue;
        }

        events.push(CodexUsageEvent {
            entry: parsed.entry,
            usage_state: parsed.usage_state,
            session_id: current_session_id.clone(),
        });
        turn += 1;
    }

    CodexParsedRollout {
        path: path.to_string_lossy().into_owned(),
        session_id,
        parent_session_id,
        forked_at,
        is_subagent,
        events,
    }
}

fn parse_codex_line(
    line: &str,
    file: &str,
    turn: i64,
    model: &str,
    fmt: &str,
) -> Option<ParsedCodexToken> {
    let obj: Value = serde_json::from_str(line).ok()?;
    let payload = obj.get("payload")?.as_object()?;
    if payload.get("type").and_then(Value::as_str) != Some("token_count") {
        return None;
    }
    let info = payload.get("info")?.as_object()?;
    let last = info.get("last_token_usage")?.as_object()?;
    let ts = obj.get("timestamp")?.as_str()?;
    let date = parse_iso8601(ts)?;

    let input_total = int_value(field(Some(last), "input_tokens"));
    let cached = int_value(field(Some(last), "cached_input_tokens"));
    let output = int_value(field(Some(last), "output_tokens"));
    let non_cached_input = i64::max(0, input_total - cached);
    let entry = Entry {
        id: format!("codex|{file}|{turn}"),
        date,
        local_day: day(date, fmt),
        model: model.to_string(),
        input: non_cached_input,
        output,
        cache_write: 0,
        cache_read: cached,
        explicit_cost: None,
    };
    let usage_state = info
        .get("total_token_usage")
        .and_then(Value::as_object)
        .map(|o| CodexUsageState {
            cumulative: CodexUsageVector::from_value(&Value::Object(o.clone())),
            last: CodexUsageVector::from_value(&Value::Object(last.clone())),
        });
    Some(ParsedCodexToken { entry, usage_state })
}

fn codex_session_meta(line: &str) -> Option<CodexSessionMeta> {
    let obj: Value = serde_json::from_str(line).ok()?;
    if obj.get("type").and_then(Value::as_str) != Some("session_meta") {
        return None;
    }
    let payload = obj.get("payload")?.as_object()?;
    let id = non_empty(payload.get("id").and_then(Value::as_str).unwrap_or("")).or_else(|| {
        non_empty(
            payload
                .get("session_id")
                .and_then(Value::as_str)
                .unwrap_or(""),
        )
    });
    let parent_id = non_empty(
        payload
            .get("forked_from_id")
            .and_then(Value::as_str)
            .unwrap_or(""),
    )
    .or_else(|| {
        non_empty(
            payload
                .get("parent_thread_id")
                .and_then(Value::as_str)
                .unwrap_or(""),
        )
    });
    let date = obj
        .get("timestamp")
        .and_then(Value::as_str)
        .and_then(parse_iso8601);
    let source = payload.get("source").and_then(Value::as_object);
    let is_subagent = payload.get("thread_source").and_then(Value::as_str) == Some("subagent")
        || source.map(|s| s.contains_key("subagent")).unwrap_or(false);
    Some(CodexSessionMeta {
        id,
        parent_id,
        date,
        is_subagent,
    })
}

fn codex_model(line: &str) -> Option<String> {
    let obj: Value = serde_json::from_str(line).ok()?;
    let payload = obj.get("payload")?.as_object()?;
    if let Some(m) = payload.get("model").and_then(Value::as_str) {
        return Some(m.to_string());
    }
    if let Some(tc) = payload.get("turn_context").and_then(Value::as_object) {
        if let Some(m) = tc.get("model").and_then(Value::as_str) {
            return Some(m.to_string());
        }
    }
    None
}

/// All rollout files (with mtime + size) under a Codex root. Files outside the
/// scan window are still parent candidates, so they are not filtered here.
pub fn codex_rollout_files(root: &Path) -> Vec<CodexRolloutFile> {
    let mut out = Vec::new();
    for entry in WalkDir::new(root)
        .into_iter()
        .filter_entry(|e| e.depth() == 0 || !is_hidden_name(e.file_name()))
    {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        if !entry.file_type().is_file()
            || !entry
                .path()
                .extension()
                .map(|e| e == "jsonl")
                .unwrap_or(false)
        {
            continue;
        }
        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        let mtime = match meta.modified() {
            Ok(m) => m,
            Err(_) => continue,
        };
        out.push(CodexRolloutFile {
            url: entry.path().to_path_buf(),
            mtime: DateTime::<Utc>::from(mtime),
            size: meta.len(),
        });
    }
    out
}

/// Pull in the parents (and their parents) needed to trim fork replays, starting
/// from the rollouts in the scan window. A Codex fork file alone cannot settle
/// its own usage — replay overlap is decided against the parent.
///
/// The reader (direct parse) and the cache (blob reuse) share this expansion
/// rule; only the three injectable pieces differ — parsing, known session ids,
/// and content probing.
pub fn expand_codex_parent_closure(
    window_files: Vec<CodexRolloutFile>,
    all_files: Vec<CodexRolloutFile>,
    load: impl Fn(&CodexRolloutFile) -> CodexParsedRollout,
    session_id_knowledge: impl Fn(&CodexRolloutFile) -> CodexSessionIDKnowledge,
    probe_session_id: impl Fn(&CodexRolloutFile) -> Option<String>,
) -> (Vec<CodexParsedRollout>, HashSet<String>) {
    let mut rollouts_by_path: HashMap<String, CodexParsedRollout> = HashMap::new();
    for file in &window_files {
        let rollout = load(file);
        rollouts_by_path.insert(rollout.path.clone(), rollout);
    }
    let included_paths: HashSet<String> = window_files.iter().map(CodexRolloutFile::path).collect();

    let mut pending_parent_ids: HashSet<String> = rollouts_by_path
        .values()
        .filter_map(|r| r.parent_session_id.clone())
        .collect();
    let mut searched_parent_ids: HashSet<String> = HashSet::new();

    while let Some(parent_id) = pending_parent_ids
        .iter()
        .find(|id| !searched_parent_ids.contains(*id))
        .cloned()
    {
        searched_parent_ids.insert(parent_id.clone());
        if rollouts_by_path
            .values()
            .any(|r| r.session_id.as_ref() == Some(&parent_id))
        {
            continue;
        }

        // A hint only narrows candidates; adoption is decided by the actual
        // payload's session id.
        let unresolved: Vec<CodexRolloutFile> = all_files
            .iter()
            .filter(|f| !rollouts_by_path.contains_key(&f.path()))
            .cloned()
            .collect();
        // Known session ids + filename hints narrow candidates first (no file
        // open). A file whose id is already known is judged by that value only —
        // matching the filename too would re-full-parse it every refresh.
        let hinted: Vec<CodexRolloutFile> = unresolved
            .iter()
            .filter(|f| match session_id_knowledge(f) {
                CodexSessionIDKnowledge::Known(id) => id.as_deref() == Some(parent_id.as_str()),
                CodexSessionIDKnowledge::Unknown => {
                    is_usable_filename_hint(&parent_id)
                        && f.url
                            .file_name()
                            .map(|n| n.to_string_lossy().contains(&parent_id))
                            .unwrap_or(false)
                }
            })
            .cloned()
            .collect();
        if adopt(
            &hinted,
            &parent_id,
            &load,
            &mut rollouts_by_path,
            &mut pending_parent_ids,
        ) {
            continue;
        }

        // No hint (or all failed verification) — only open files whose content
        // is unknown. Files with a known id were judged != parentID above, so
        // reopening them is pointless.
        let hinted_paths: HashSet<String> = hinted.iter().map(CodexRolloutFile::path).collect();
        let probed: Vec<CodexRolloutFile> = unresolved
            .iter()
            .filter(|f| {
                if hinted_paths.contains(&f.path()) {
                    return false;
                }
                if !matches!(session_id_knowledge(f), CodexSessionIDKnowledge::Unknown) {
                    return false;
                }
                probe_session_id(f) == Some(parent_id.clone())
            })
            .cloned()
            .collect();
        let _ = adopt(
            &probed,
            &parent_id,
            &load,
            &mut rollouts_by_path,
            &mut pending_parent_ids,
        );
    }

    (rollouts_by_path.into_values().collect(), included_paths)
}

/// Try to adopt every candidate whose payload session id equals `parent_id`.
/// Returns whether any candidate matched (and inserts its ancestors' ids as new
/// pending parents).
fn adopt(
    candidates: &[CodexRolloutFile],
    parent_id: &str,
    load: &impl Fn(&CodexRolloutFile) -> CodexParsedRollout,
    rollouts_by_path: &mut HashMap<String, CodexParsedRollout>,
    pending_parent_ids: &mut HashSet<String>,
) -> bool {
    let mut resolved = false;
    for candidate in candidates {
        let parent = load(candidate);
        if parent.session_id.as_deref() != Some(parent_id) {
            continue;
        }
        if let Some(ancestor_id) = parent.parent_session_id.clone() {
            pending_parent_ids.insert(ancestor_id);
        }
        rollouts_by_path.insert(parent.path.clone(), parent);
        resolved = true;
    }
    resolved
}

pub fn codex_entries(modified_since: DateTime<Utc>, root: Option<&Path>) -> Vec<Entry> {
    let owned_root;
    let root = match root {
        Some(r) => r,
        None => {
            owned_root = codex_sessions_dir(&home_dir().unwrap_or_default());
            &owned_root
        }
    };
    let all_files = codex_rollout_files(root);
    // Reader path — no known session ids, so only filename hints + probes find
    // parents.
    let (rollouts, included_paths) = expand_codex_parent_closure(
        all_files
            .iter()
            .filter(|f| f.mtime >= modified_since)
            .cloned()
            .collect(),
        all_files,
        |f| parse_codex_rollout(&f.url, LOCAL_DAY_FORMAT),
        |_| CodexSessionIDKnowledge::Unknown,
        |f| codex_rollout_session_id(&f.url),
    );
    resolve_codex_rollouts(rollouts, included_paths)
}

/// Is `id` usable to narrow parent candidates by filename? Degenerate values
/// (empty, separator-only) match nearly every rollout filename and the filter
/// would no-op while still full-parsing everything.
pub fn is_usable_filename_hint(id: &str) -> bool {
    id.chars().count() >= 4 && id.chars().any(|c| c.is_alphabetic() || c.is_numeric())
}

enum ProbeOutcome {
    SessionId(Option<String>),
    Stop,
    Invalid,
    KeepScanning,
}

/// Metadata-only probe for old parent dependencies — never reads a whole large
/// rollout.
///
/// **Never decode a fixed-size prefix wholesale**: a cut in the middle of a
/// multi-byte character would make a strict decode fail (measured: 14/109 local
/// rollouts failed at a 64 KB boundary) and misreport "no session id"; a first
/// line longer than the cut hits the same result. Instead read chunks but only
/// decode newline-completed lines.
///
/// `Ok(None)` means the file was read fine but has no usable metadata (no meta,
/// `token_count` first, corrupt line, limit reached — all deterministic file
/// properties). Open/read failures are `Err`, so a transient I/O failure does
/// not freeze as "no session id".
pub fn probe_codex_rollout_session_id(
    path: &Path,
    byte_limit: usize,
) -> std::io::Result<Option<String>> {
    let mut file = std::fs::File::open(path)?;
    let mut buffer: Vec<u8> = Vec::new();
    let mut read_bytes = 0usize;
    let mut chunk = vec![0u8; CODEX_PROBE_CHUNK_SIZE];

    while read_bytes < byte_limit {
        // Read only within the remaining budget (a 0-byte request would be
        // misread as EOF).
        let to_read = CODEX_PROBE_CHUNK_SIZE.min(byte_limit - read_bytes);
        let n = file.read(&mut chunk[..to_read])?;
        if n == 0 {
            // EOF — a final unterminated line counts as complete.
            return Ok(probe_final_outcome(&buffer));
        }
        read_bytes += n;
        buffer.extend_from_slice(&chunk[..n]);

        let mut line_start = 0usize;
        while line_start < buffer.len() {
            match buffer[line_start..].iter().position(|&b| b == b'\n') {
                Some(rel) => {
                    let newline = line_start + rel;
                    let line = &buffer[line_start..newline];
                    match codex_probe_outcome(line) {
                        ProbeOutcome::SessionId(id) => return Ok(id),
                        ProbeOutcome::Stop | ProbeOutcome::Invalid => return Ok(None),
                        ProbeOutcome::KeepScanning => line_start = newline + 1,
                    }
                }
                None => break,
            }
        }
        // Remove the consumed prefix of the buffer in one pass per chunk.
        if line_start > 0 {
            buffer.drain(..line_start);
        }
    }
    // Limit reached — a final unterminated line counts as complete if it is a
    // finished JSON line.
    Ok(probe_final_outcome(&buffer))
}

fn probe_final_outcome(buffer: &[u8]) -> Option<String> {
    match codex_probe_outcome(buffer) {
        ProbeOutcome::SessionId(id) => id,
        _ => None,
    }
}

fn codex_probe_outcome(line: &[u8]) -> ProbeOutcome {
    if line.is_empty() {
        return ProbeOutcome::KeepScanning;
    }
    // Skipping a corrupt line could let a re-inserted parent meta be mistaken
    // for this file's id, so stop instead.
    let text = match std::str::from_utf8(line) {
        Ok(t) => t,
        Err(_) => return ProbeOutcome::Invalid,
    };
    if let Some(meta) = codex_session_meta(text) {
        return ProbeOutcome::SessionId(meta.id);
    }
    if text.contains("token_count") {
        return ProbeOutcome::Stop;
    }
    ProbeOutcome::KeepScanning
}

/// Convenience wrapper that folds read failures into `None`. Callers that
/// persist the result must use the throwing `probe_codex_rollout_session_id` —
/// this wrapper would freeze a transient I/O failure as "no session id".
pub fn codex_rollout_session_id(path: &Path) -> Option<String> {
    probe_codex_rollout_session_id(path, CODEX_PROBE_BYTE_LIMIT)
        .ok()
        .flatten()
}

/// Between confirmed-parent rollouts, compare usage-state prefixes and trim only
/// the actually copied replay. Manual forks whose parent cannot be found fall
/// back to the 1-second timing trim. Subagents (verified to carry no replay in
/// fixtures) are always preserved.
pub fn resolve_codex_rollouts(
    rollouts: Vec<CodexParsedRollout>,
    included_paths: HashSet<String>,
) -> Vec<Entry> {
    let mut by_session: HashMap<String, Vec<&CodexParsedRollout>> = HashMap::new();
    for rollout in &rollouts {
        if let Some(sid) = &rollout.session_id {
            by_session.entry(sid.clone()).or_default().push(rollout);
        }
    }
    for candidates in by_session.values_mut() {
        candidates.sort_by(|a, b| a.path.cmp(&b.path));
    }
    let by_path: HashMap<String, &CodexParsedRollout> =
        rollouts.iter().map(|r| (r.path.clone(), r)).collect();
    let mut memo: HashMap<String, CodexResolvedRollout> = HashMap::new();

    fn resolve(
        rollout: &CodexParsedRollout,
        visiting: &mut HashSet<String>,
        memo: &mut HashMap<String, CodexResolvedRollout>,
        by_session: &HashMap<String, Vec<&CodexParsedRollout>>,
    ) -> CodexResolvedRollout {
        if let Some(cached) = memo.get(&rollout.path) {
            return cached.clone();
        }
        if !visiting.insert(rollout.path.clone()) {
            return resolve_owned_events(rollout, fallback_replay_count(rollout), &[]);
        }

        let mut best_parent_match: Option<(usize, Vec<CodexResolvedEvent>)> = None;
        if let Some(parent_id) = &rollout.parent_session_id {
            if let Some(candidates) = by_session.get(parent_id) {
                for candidate in candidates {
                    if candidate.path == rollout.path {
                        continue;
                    }
                    let resolved_parent = resolve(candidate, visiting, memo, by_session);
                    // A prefix that does not overlap at all (0) means "found a
                    // parent but nothing to compare" — counting it as a match
                    // would skip the timing fallback and be *worse* than not
                    // finding the parent. Filter here so `bestParentMatch` only
                    // ever holds a parent with an actually-overlapping prefix.
                    if let Some(replay_count) =
                        comparable_usage_prefix_count(&rollout.events, &resolved_parent.history)
                    {
                        if replay_count > 0 {
                            let is_better = best_parent_match
                                .as_ref()
                                .is_none_or(|(current, _)| replay_count > *current);
                            if is_better {
                                best_parent_match =
                                    Some((replay_count, resolved_parent.history.clone()));
                            }
                        }
                    }
                }
            }
        }
        visiting.remove(&rollout.path);

        let resolved = if let Some((replay_count, history)) = &best_parent_match {
            resolve_owned_events(rollout, *replay_count, &history[..*replay_count])
        } else if rollout.parent_session_id.is_some() {
            // Parent not found, or old records without cumulative state make
            // structural comparison impossible. Real subagents are preserved by
            // `fallback_replay_count`; only manual forks take the timing trim.
            resolve_owned_events(rollout, fallback_replay_count(rollout), &[])
        } else {
            resolve_owned_events(rollout, 0, &[])
        };
        memo.insert(rollout.path.clone(), resolved.clone());
        resolved
    }

    let mut result: Vec<Entry> = Vec::new();
    let mut sorted_paths: Vec<&String> = included_paths.iter().collect();
    sorted_paths.sort();
    for path in sorted_paths {
        if let Some(rollout) = by_path.get(path) {
            let mut visiting: HashSet<String> = HashSet::new();
            result.extend(resolve(rollout, &mut visiting, &mut memo, &by_session).owned_entries);
        }
    }
    dedup_codex_canonical_entries(result)
}

/// The same canonical state left in several files keeps the record closest to
/// the original time — earliest wins (the token vector is part of the id, so
/// keep-earliest, not keep-max, is what matches Codex's date semantics).
fn dedup_codex_canonical_entries(entries: Vec<Entry>) -> Vec<Entry> {
    let mut by_id: HashMap<String, Entry> = HashMap::new();
    let mut order: Vec<String> = Vec::new();
    for entry in entries {
        if let Some(existing) = by_id.get(&entry.id) {
            if entry.date < existing.date {
                by_id.insert(entry.id.clone(), entry);
            }
        } else {
            order.push(entry.id.clone());
            by_id.insert(entry.id.clone(), entry);
        }
    }
    order
        .into_iter()
        .filter_map(|id| by_id.get(&id).cloned())
        .collect()
}

/// Length of the common prefix of comparable full usage states. `None` means
/// structural comparison is impossible (a missing cumulative state) — not a
/// prefix of 0.
fn comparable_usage_prefix_count(
    child: &[CodexUsageEvent],
    parent: &[CodexResolvedEvent],
) -> Option<usize> {
    if child.is_empty() {
        return Some(0);
    }
    if parent.is_empty() {
        return None;
    }
    let mut count = 0;
    while count < child.len() && count < parent.len() {
        match (&child[count].usage_state, &parent[count].usage_state) {
            (Some(child_state), Some(parent_state)) => {
                if child_state != parent_state {
                    break;
                }
            }
            _ => return None,
        }
        count += 1;
    }
    Some(count)
}

fn fallback_replay_count(rollout: &CodexParsedRollout) -> usize {
    // Confirmed 0.142.5 / 0.145.0 subagents insert parent metadata but do not
    // replay token_count records — a missing parent file is no reason to throw
    // away the subagent's first real turn.
    if rollout.is_subagent {
        return 0;
    }
    let events = &rollout.events;
    if events.len() <= 1 {
        return if events.is_empty() { 0 } else { 1 };
    }
    let mut count = 1;
    while count < events.len() {
        let gap_ms = (events[count].entry.date - events[count - 1].entry.date).num_milliseconds();
        let gap = gap_ms as f64 / 1000.0;
        // NaN-safe negation mirroring the Swift `guard gap < max else { break }`
        // (NaN would otherwise continue the burst instead of breaking).
        #[allow(clippy::neg_cmp_op_on_partial_ord)]
        if !(gap < FORK_REPLAY_MAXIMUM_GAP) {
            break;
        }
        count += 1;
    }
    count
}

fn resolve_owned_events(
    rollout: &CodexParsedRollout,
    replay_count: usize,
    inherited_history: &[CodexResolvedEvent],
) -> CodexResolvedRollout {
    let mut history = inherited_history.to_vec();
    let mut owned_entries: Vec<Entry> = Vec::new();
    let mut epoch = 0i64;
    let mut previous_cumulative: Option<CodexUsageVector> = None;
    let mut previous_owner: Option<String> = None;

    for event in rollout.events.iter().skip(replay_count) {
        // A fork file's unmatched suffix belongs to the child even when it sits
        // after the embedded parent meta; a non-fork file follows the event's
        // own session id.
        let owner = if rollout.parent_session_id.is_none() {
            event
                .session_id
                .clone()
                .or_else(|| rollout.session_id.clone())
        } else {
            rollout.session_id.clone()
        };
        if owner != previous_owner {
            epoch = 0;
            previous_cumulative = None;
            previous_owner = owner.clone();
        }
        if let Some(cumulative) = event.usage_state.as_ref().map(|s| &s.cumulative) {
            if let Some(previous) = &previous_cumulative {
                if cumulative.is_lower_than(previous) {
                    epoch += 1;
                }
            }
            previous_cumulative = Some(cumulative.clone());
        } else {
            previous_cumulative = None;
        }

        let entry = if let (Some(owner_id), Some(state)) = (&owner, &event.usage_state) {
            replacing_id(
                &event.entry,
                &format!("codex|{owner_id}|{epoch}|{}", state.fingerprint()),
            )
        } else {
            // Old records without cumulative usage or a session id keep the
            // positional id.
            event.entry.clone()
        };
        owned_entries.push(entry.clone());
        history.push(CodexResolvedEvent {
            entry,
            usage_state: event.usage_state.clone(),
        });
    }
    CodexResolvedRollout {
        history,
        owned_entries,
    }
}

fn replacing_id(entry: &Entry, id: &str) -> Entry {
    Entry {
        id: id.to_string(),
        date: entry.date,
        local_day: entry.local_day.clone(),
        model: entry.model.clone(),
        input: entry.input,
        output: entry.output,
        cache_write: entry.cache_write,
        cache_read: entry.cache_read,
        explicit_cost: entry.explicit_cost,
    }
}

// MARK: - Gemini parsing

fn absorb(
    obj: &Map<String, Value>,
    fallback_timestamp: Option<DateTime<Utc>>,
    file: &str,
    fmt: &str,
    by_id: &mut HashMap<String, Entry>,
    order: &mut Vec<String>,
) {
    let tokens = match obj.get("tokens").and_then(Value::as_object) {
        Some(t) => t,
        None => return,
    };
    let id = obj
        .get("id")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let date = match obj
        .get("timestamp")
        .and_then(Value::as_str)
        .and_then(parse_iso8601)
        .or(fallback_timestamp)
    {
        Some(d) => d,
        None => return,
    };
    let input = int_value(field(Some(tokens), "input"));
    let cached = int_value(field(Some(tokens), "cached"));
    let entry = Entry {
        id: format!("gemini|{file}|{id}"),
        date,
        local_day: day(date, fmt),
        model: obj
            .get("model")
            .and_then(Value::as_str)
            .unwrap_or("gemini")
            .to_string(),
        input: i64::max(0, input - cached) + int_value(field(Some(tokens), "tool")),
        output: int_value(field(Some(tokens), "output"))
            + int_value(field(Some(tokens), "thoughts")),
        cache_write: 0,
        cache_read: cached,
        explicit_cost: None,
    };
    if !by_id.contains_key(&id) {
        order.push(id.clone());
    }
    by_id.insert(id, entry); // a `message_update` record is the final value
}

/// Gemini CLI session file parser. New `.jsonl` records carry inline tokens on
/// `type == "gemini"` messages or `type == "message_update"` (same id: last
/// wins); legacy `.json` is a single ConversationRecord `messages[]`. Token
/// mapping preserves `Entry.total == totalTokenCount`:
/// input = (input − cached) + tool / cacheRead = cached;
/// output = output + thoughts / cacheWrite = 0.
pub fn parse_gemini_file(path: &Path, fmt: &str) -> Vec<Entry> {
    let data = match std::fs::read(path) {
        Ok(d) => d,
        Err(_) => return Vec::new(),
    };
    let file = file_name(path);
    let mut by_id: HashMap<String, Entry> = HashMap::new();
    let mut order: Vec<String> = Vec::new();

    let is_jsonl = path.extension().map(|e| e == "jsonl").unwrap_or(false);
    if is_jsonl {
        let text = match String::from_utf8(data) {
            Ok(t) => t,
            Err(_) => return Vec::new(),
        };
        let mut last_timestamp: Option<DateTime<Utc>> = None;
        for line in text.lines() {
            if !(line.contains("\"tokens\"") || line.contains("\"timestamp\"")) {
                continue;
            }
            let obj = match serde_json::from_str::<Value>(line) {
                Ok(o) => o,
                Err(_) => continue,
            };
            let Some(obj) = obj.as_object() else {
                continue;
            };
            if let Some(ts) = obj.get("timestamp").and_then(Value::as_str) {
                if let Some(d) = parse_iso8601(ts) {
                    last_timestamp = Some(d);
                }
            }
            absorb(obj, last_timestamp, &file, fmt, &mut by_id, &mut order);
        }
    } else {
        // Legacy single JSON — `messages` array.
        let obj = match serde_json::from_slice::<Value>(&data) {
            Ok(o) => o,
            Err(_) => return Vec::new(),
        };
        let Some(obj) = obj.as_object() else {
            return Vec::new();
        };
        let Some(messages) = obj.get("messages").and_then(Value::as_array) else {
            return Vec::new();
        };
        let session_start = obj
            .get("startTime")
            .and_then(Value::as_str)
            .and_then(parse_iso8601);
        for m in messages {
            let Some(m) = m.as_object() else {
                continue;
            };
            absorb(m, session_start, &file, fmt, &mut by_id, &mut order);
        }
    }
    order
        .into_iter()
        .filter_map(|id| by_id.remove(&id))
        .collect()
}

pub fn gemini_entries(modified_since: DateTime<Utc>, root: Option<&Path>) -> Vec<Entry> {
    let owned_root;
    let root = match root {
        Some(r) => r,
        None => {
            owned_root = gemini_tmp_dir(&home_dir().unwrap_or_default());
            &owned_root
        }
    };
    let mut entries = Vec::new();
    for file in jsonl_files(root, modified_since, true) {
        entries.extend(parse_gemini_file(&file, LOCAL_DAY_FORMAT));
    }
    entries
}

// MARK: - Grok parsing

/// The only file in a session directory that holds token usage. Dialogue items
/// in `chat_history.jsonl` have no usage field and `events.jsonl` only records
/// turn outcomes — scanning them would inflate cache blobs.
pub fn grok_updates_file_name() -> &'static str {
    "updates.jsonl"
}

/// Grok session file parser: only `sessionUpdate == "turn_completed"` lines.
/// The line envelope is `{timestamp, method, params:{sessionId, update, _meta}}`.
///
/// Token mapping keeps `Entry.total == usage.totalTokens`:
/// - `inputTokens` (camelCase) includes the cached read →
///   `input = inputTokens − cachedReadTokens`, `cacheRead = cachedReadTokens`.
/// - `input_tokens` (snake_case) is already cache-excluded → used as input
///   directly. The two spellings mean opposite things, so we branch on the
///   spelling (treating them equal would subtract the cache twice).
/// - `output = outputTokens` (reasoning included), `cacheWrite = 0`.
pub fn parse_grok_file(path: &Path, fmt: &str) -> Vec<Entry> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(_) => return Vec::new(),
    };
    let mut out = Vec::new();
    for line in text.lines() {
        // `updates.jsonl` keeps every streaming chunk (tens of thousands of
        // lines per session) — filter by string before JSON parsing.
        if line.contains("turn_completed") {
            if let Some(e) = parse_grok_line(line, fmt) {
                out.push(e);
            }
        }
    }
    dedup_keep_max(out)
}

pub fn grok_entries(modified_since: DateTime<Utc>, root: Option<&Path>) -> Vec<Entry> {
    let owned_root;
    let root = match root {
        Some(r) => r,
        None => {
            owned_root = grok_sessions_dir();
            &owned_root
        }
    };
    let mut entries = Vec::new();
    for file in jsonl_files(root, modified_since, false) {
        if is_grok_usage_file(&file) {
            entries.extend(parse_grok_file(&file, LOCAL_DAY_FORMAT));
        }
    }
    // A fork session copying the parent's updates carries the same turn ids —
    // the global dedup counts each turn once.
    dedup_keep_max(entries)
}

/// Is this a file to aggregate — the token-bearing `updates.jsonl` of a
/// non-subagent session?
///
/// Subagent tokens are already folded into the parent turn's usage
/// (RecordSubagentUsage), so counting them again double-counts. The decision is
/// made at **file-selection** time, not parse time: the parse cache is keyed by
/// `updates.jsonl`'s mtime/size while the evidence lives in the sibling
/// `summary.json`, so a parse-time filter would freeze a subagent result into
/// the blob until that file changes.
pub fn is_grok_usage_file(path: &Path) -> bool {
    if path
        .file_name()
        .map(|n| n == "updates.jsonl")
        .unwrap_or(false)
    {
        if let Some(dir) = path.parent() {
            return !grok_session_is_subagent(dir);
        }
    }
    false
}

/// `summary.json`'s `session_kind` being subagent-family. A missing/unreadable
/// summary means a user session — the CLI writes it at session creation, so
/// absence is "a new session with no turns yet".
fn grok_session_is_subagent(session_dir: &Path) -> bool {
    let data = match std::fs::read(session_dir.join("summary.json")) {
        Ok(d) => d,
        Err(_) => return false,
    };
    let obj = match serde_json::from_slice::<Value>(&data) {
        Ok(o) => o,
        Err(_) => return false,
    };
    let kind = match obj.get("session_kind").and_then(Value::as_str) {
        Some(k) => k,
        None => return false,
    };
    kind.starts_with("subagent")
}

fn parse_grok_line(line: &str, fmt: &str) -> Option<Entry> {
    let envelope: Value = serde_json::from_str(line).ok()?;
    let envelope = envelope.as_object()?;
    // Disk lines are three layers — envelope → notification → update. Old lines
    // without `method` are the notification itself (no envelope), one layer less.
    let notification = envelope
        .get("params")
        .and_then(Value::as_object)
        .unwrap_or(envelope);
    let update = notification.get("update")?.as_object()?;
    if update.get("sessionUpdate").and_then(Value::as_str) != Some("turn_completed") {
        return None;
    }
    let usage = update.get("usage")?.as_object()?;
    let meta = notification.get("_meta").and_then(Value::as_object);
    // Replay-marked lines are skipped. The turn-id dedup below is the primary
    // defense; this is auxiliary (replay usually only carries the flag on the
    // wire, not on disk).
    if bool_value(field(meta, "isReplay")) {
        return None;
    }
    // The turn identifier is `prompt_id` only. Session path is deliberately not
    // mixed in, so a fork copying the parent's updates folds the same turn once.
    let turn_id = non_empty(
        update
            .get("prompt_id")
            .and_then(Value::as_str)
            .unwrap_or(""),
    )?;
    let date = grok_date(envelope, meta)?;

    // `null` counts as absent — mistaking it for a value would subtract a
    // phantom cache read or zero tokens.
    let output = int_or_nil(field(Some(usage), "outputTokens"))
        .or_else(|| int_or_nil(field(Some(usage), "output_tokens")))
        .unwrap_or(0);
    let reported_cache_read = int_or_nil(field(Some(usage), "cachedReadTokens"))
        .or_else(|| int_or_nil(field(Some(usage), "cached_read_tokens")))
        .unwrap_or(0);
    let (input, cache_read) = if let Some(full) = int_or_nil(field(Some(usage), "inputTokens")) {
        // Cache read is a subset of the prompt, so it cannot exceed the total.
        // Clamping preserves identity — `max(0, ·)` would silently inflate
        // total past inputTokens.
        let clamped = reported_cache_read.min(full);
        (full - clamped, clamped)
    } else {
        // Headless projection: `input_tokens` is already cache-excluded.
        (
            int_or_nil(field(Some(usage), "input_tokens")).unwrap_or(0),
            reported_cache_read,
        )
    };
    // When the source's total disagrees with the parts, attribute the remainder
    // to output so the sum follows the source (same rule as other readers).
    if let Some(reported_total) = int_or_nil(field(Some(usage), "totalTokens"))
        .or_else(|| int_or_nil(field(Some(usage), "total_tokens")))
    {
        let parts = input + output + cache_read;
        if reported_total > parts {
            return grok_entry(
                turn_id,
                date,
                fmt,
                usage,
                input,
                output + (reported_total - parts),
                cache_read,
            );
        }
    }
    grok_entry(turn_id, date, fmt, usage, input, output, cache_read)
}

fn grok_entry(
    turn_id: String,
    date: DateTime<Utc>,
    fmt: &str,
    usage: &Map<String, Value>,
    input: i64,
    output: i64,
    cache_read: i64,
) -> Option<Entry> {
    // Zero-token turns (cancelled etc.) are not recorded.
    if input + output + cache_read <= 0 {
        return None;
    }
    Some(Entry {
        id: format!("grok|{turn_id}"),
        date,
        local_day: day(date, fmt),
        model: grok_model(usage).unwrap_or_else(|| "grok".to_string()),
        input,
        output,
        cache_write: 0,
        cache_read,
        explicit_cost: grok_cost(usage),
    })
}

/// Display model name — the highest-totalTokens row of `modelUsage`/
/// `model_usage` (tie → name order). Numbers always aggregate from totals, so a
/// row-sum divergence from totals is never introduced.
fn grok_model(usage: &Map<String, Value>) -> Option<String> {
    let by_model = usage
        .get("modelUsage")
        .and_then(Value::as_object)
        .or_else(|| usage.get("model_usage").and_then(Value::as_object))?;
    let mut best: Option<(String, i64)> = None;
    let mut keys: Vec<&String> = by_model.keys().collect();
    keys.sort();
    for model in keys {
        let fields = by_model.get(model).and_then(Value::as_object);
        let total = int_or_nil(
            fields
                .and_then(|m| m.get("totalTokens"))
                .unwrap_or(&Value::Null),
        )
        .or_else(|| {
            int_or_nil(
                fields
                    .and_then(|m| m.get("total_tokens"))
                    .unwrap_or(&Value::Null),
            )
        })
        .unwrap_or(0);
        match &best {
            Some((_, best_total)) if total > *best_total => best = Some((model.clone(), total)),
            Some(_) => {}
            None => best = Some((model.clone(), total)),
        }
    }
    best.and_then(|(m, _)| non_empty(&m))
}

/// Only the server-computed cost (1e10 ticks = $1) is used; partial/incomplete
/// aggregation flags discard it (Grok has no price table, so an estimate would
/// misdisplay an amount).
fn grok_cost(usage: &Map<String, Value>) -> Option<f64> {
    if bool_value(field(Some(usage), "usageIsIncomplete"))
        || bool_value(field(Some(usage), "usage_is_incomplete"))
    {
        return None;
    }
    if bool_value(field(Some(usage), "costIsPartial"))
        || bool_value(field(Some(usage), "cost_is_partial"))
    {
        return None;
    }
    let ticks = double_or_nil(field(Some(usage), "costUsdTicks"))
        .or_else(|| double_or_nil(field(Some(usage), "cost_usd_ticks")))
        .unwrap_or(0.0);
    if ticks > 0.0 {
        Some(ticks / 1e10)
    } else {
        None
    }
}

fn seconds_to_date(secs: f64) -> DateTime<Utc> {
    let whole = secs.floor() as i64;
    let nanos = ((secs - whole as f64) * 1e9).round() as u32;
    DateTime::from_timestamp(whole, nanos).unwrap_or(DateTime::<Utc>::UNIX_EPOCH)
}

/// Turn time: `_meta.agentTimestampMs` (the agent's timestamp for that turn)
/// wins — the envelope `timestamp` is the *record* time (Unix seconds), and a
/// fork copying the parent's updates re-stamps it, which would re-date all the
/// forked session's history to the fork moment. `_meta` is preserved on copy.
fn grok_date(
    envelope: &Map<String, Value>,
    meta: Option<&Map<String, Value>>,
) -> Option<DateTime<Utc>> {
    let ms = double_or_nil(field(meta, "agentTimestampMs")).unwrap_or(0.0);
    if ms > 0.0 {
        return Some(seconds_to_date(ms / 1000.0));
    }
    let raw = double_or_nil(field(Some(envelope), "timestamp")).unwrap_or(0.0);
    // The envelope is seconds, but absorb the milliseconds variant too (same
    // threshold as the other local readers).
    if raw > 0.0 {
        let secs = if raw >= 100_000_000_000.0 {
            raw / 1000.0
        } else {
            raw
        };
        return Some(seconds_to_date(secs));
    }
    if let Some(ts) = envelope.get("timestamp").and_then(Value::as_str) {
        return parse_iso8601(ts);
    }
    None
}

// MARK: - Aggregation

/// Daily total for one local day → `DailyUsage`. `None` when that day has no
/// data (or only zero-token entries).
pub fn daily(entries: &[Entry], local_day: &str) -> Option<DailyUsage> {
    let mut b = Bucket::default();
    for e in entries {
        if e.local_day == local_day {
            b.add(e);
        }
    }
    if b.total() <= 0 {
        return None;
    }
    Some(DailyUsage::new(
        local_day.to_string(),
        b.input,
        b.output,
        b.cache_write,
        b.cache_read,
        b.total(),
        b.cost,
    ))
}

/// Local-day inclusive `[from_day, to_day]` sum → `PeriodUsage`.
pub fn period(entries: &[Entry], period_key: &str, from_day: &str, to_day: &str) -> PeriodUsage {
    let mut b = Bucket::default();
    for e in entries {
        if e.local_day.as_str() >= from_day && e.local_day.as_str() <= to_day {
            b.add(e);
        }
    }
    PeriodUsage::new(period_key.to_string(), b.total(), b.cost)
}

fn iso_string(d: DateTime<Utc>) -> String {
    d.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

/// Recent-5h rolling window active block (burn-rate estimate).
pub fn active_block(entries: &[Entry], now: DateTime<Utc>) -> Option<BlockUsage> {
    let window_start = now - chrono::Duration::seconds(BLOCK_WINDOW_SECS);
    let mut recent: Vec<&Entry> = entries.iter().filter(|e| e.date >= window_start).collect();
    recent.sort_by_key(|a| a.date);
    let first = *recent.first()?;
    let mut b = Bucket::default();
    for e in &recent {
        b.add(e);
    }
    let minutes = ((now - first.date).num_milliseconds() as f64 / 60_000.0).max(1.0);
    let tpm = b.total() as f64 / minutes;
    Some(BlockUsage {
        id: format!("block-{}", first.date.timestamp()),
        start_time: iso_string(first.date),
        end_time: iso_string(first.date + chrono::Duration::seconds(BLOCK_WINDOW_SECS)),
        is_active: true,
        total_tokens: b.total(),
        cost_usd: b.cost,
        tokens_per_minute: Some(tpm),
    })
}

// MARK: - Date utilities

pub fn start_of_day(now: DateTime<Local>) -> DateTime<Local> {
    now.date_naive()
        .and_hms_opt(0, 0, 0)
        .and_then(|d| d.and_local_timezone(Local).single())
        .unwrap_or(now)
}

pub fn start_of_month(now: DateTime<Local>) -> DateTime<Local> {
    now.date_naive()
        .with_day(1)
        .and_then(|d| d.and_hms_opt(0, 0, 0))
        .and_then(|d| d.and_local_timezone(Local).single())
        .unwrap_or(now)
}

/// Monday (ISO) week start. The original uses `Calendar.current.dateInterval(of:
/// .weekOfYear)` which follows the machine locale (Sunday for en_US, Monday for
/// ISO locales); the tests pass under either, and Monday/ISO is the stable
/// choice documented at the top of this module.
pub fn start_of_week(now: DateTime<Local>) -> DateTime<Local> {
    let days_back = i64::from(now.weekday().num_days_from_monday());
    start_of_day(now - chrono::Duration::days(days_back))
}

/// Enrichment (active block, this week, this month) is derived from a single
/// scan, so the mtime floor must be the **earliest** start of those three
/// windows. Pitfall: month-start alone misses the month boundary when week-start
/// rolls into the previous month (11 of 12 months), and just after midnight the
/// 5h block rolls into yesterday — `min` absorbs both.
pub fn enrichment_scan_start(now: DateTime<Local>) -> DateTime<Local> {
    start_of_month(now)
        .min(start_of_week(now))
        .min(now - chrono::Duration::seconds(BLOCK_WINDOW_SECS))
}

pub fn month_key(now: DateTime<Local>) -> String {
    now.format("%Y-%m").to_string()
}

pub fn today_key() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

#[cfg(test)]
mod tests;
