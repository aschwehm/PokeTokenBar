//! Antigravity CLI usage reader and provider.
//!
//! Reads conversation SQLite stores at `~/.gemini/antigravity-cli/conversations/*.db`.
//! Decodes protobuf metadata (`CortexStepGeneratorMetadata`) stored in `gen_metadata.data`.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Local, TimeZone, Utc};
use rusqlite::{Connection, OpenFlags};

use crate::domain::models::{DailyUsage, PeriodUsage};
use crate::providers::reader::{self, Entry};
use crate::providers::{ProviderEnrichment, UsageProvider};

pub const TOKEN_CEILING: u64 = 1_000_000_000;
pub const NAMED_LOSS_LIMIT: usize = 5;

/// Default root directory for Antigravity conversation databases.
pub fn default_root() -> PathBuf {
    if let Ok(dir) = std::env::var("PTB_ANTIGRAVITY_DIR") {
        if !dir.trim().is_empty() {
            return PathBuf::from(dir);
        }
    }
    crate::platform::binary_locator::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".gemini/antigravity-cli/conversations")
}

/// All root directories for Antigravity, including discovered WSL directories on Windows.
pub fn all_roots() -> Vec<PathBuf> {
    #[allow(unused_mut)]
    let mut roots = vec![default_root()];
    #[cfg(target_os = "windows")]
    {
        for wsl_home in crate::platform::binary_locator::wsl_home_dirs() {
            let wsl_root = wsl_home.join(".gemini/antigravity-cli/conversations");
            if wsl_root.is_dir() {
                roots.push(wsl_root);
            }
        }
    }
    roots
}

// MARK: - Protobuf wire format parser

pub mod proto {
    use super::TOKEN_CEILING;

    pub fn token_count(data: &[u8], field: usize) -> Option<i64> {
        match varint(data, field) {
            Some(val) => {
                if val <= TOKEN_CEILING {
                    Some(val as i64)
                } else {
                    None
                }
            }
            None => Some(0),
        }
    }

    pub fn varint(data: &[u8], field: usize) -> Option<u64> {
        let mut result = None;
        walk(data, |f, val, payload| {
            if f == field && payload.is_none() {
                result = Some(val);
                false
            } else {
                true
            }
        });
        result
    }

    pub fn string(data: &[u8], field: usize) -> Option<String> {
        let payload = message(data, field)?;
        if payload.is_empty() {
            return None;
        }
        let s = std::str::from_utf8(payload).ok()?;
        if s.is_empty() {
            None
        } else {
            Some(s.to_string())
        }
    }

    pub fn message(data: &[u8], field: usize) -> Option<&[u8]> {
        let mut result = None;
        walk(data, |f, _, payload| {
            if f == field && payload.is_some() {
                result = payload;
                false
            } else {
                true
            }
        });
        result
    }

    pub fn walk<'a, F>(data: &'a [u8], mut visit: F)
    where
        F: FnMut(usize, u64, Option<&'a [u8]>) -> bool,
    {
        let mut index = 0;
        while index < data.len() {
            let (key, after_key) = match read_varint(data, index) {
                Some(v) => v,
                None => return,
            };
            index = after_key;
            let field = (key >> 3) as usize;
            if field == 0 {
                return;
            }
            match key & 7 {
                0 => {
                    let (val, after_val) = match read_varint(data, index) {
                        Some(v) => v,
                        None => return,
                    };
                    index = after_val;
                    if !visit(field, val, None) {
                        return;
                    }
                }
                1 => {
                    if data.len() - index < 8 {
                        return;
                    }
                    index += 8;
                }
                2 => {
                    let (len, after_len) = match read_varint(data, index) {
                        Some(v) => v,
                        None => return,
                    };
                    if len as usize > data.len() - after_len {
                        return;
                    }
                    let end = after_len + len as usize;
                    if !visit(field, 0, Some(&data[after_len..end])) {
                        return;
                    }
                    index = end;
                }
                5 => {
                    if data.len() - index < 4 {
                        return;
                    }
                    index += 4;
                }
                _ => return,
            }
        }
    }

    fn read_varint(data: &[u8], start: usize) -> Option<(u64, usize)> {
        let mut value: u64 = 0;
        let mut shift: u64 = 0;
        let mut index = start;
        while index < data.len() {
            let byte = data[index];
            index += 1;
            value |= ((byte & 0x7f) as u64) << shift;
            if (byte & 0x80) == 0 {
                return Some((value, index));
            }
            shift += 7;
            if shift > 63 {
                return None;
            }
        }
        None
    }
}

// MARK: - Record parsing

#[derive(Debug, Clone)]
pub struct Record {
    pub entry: Option<Entry>,
    pub discarded_counters: usize,
}

pub fn parse_generation_metadata(blob: &[u8], conversation: &str, index: i64) -> Record {
    parse_generation_metadata_with_fallback(blob, conversation, index, None)
}

pub fn extract_last_step_index(chat_model: &[u8]) -> Option<i64> {
    let mut found = None;
    proto::walk(chat_model, |f, _, p| {
        if f == 20 {
            if let Some(pair_blob) = p {
                let mut key = None;
                let mut val = None;
                proto::walk(pair_blob, |pf, _, pp| {
                    if let Some(str_bytes) = pp {
                        if let Ok(s) = std::str::from_utf8(str_bytes) {
                            if pf == 1 {
                                key = Some(s.trim().to_string());
                            } else if pf == 2 {
                                val = Some(s.trim().to_string());
                            }
                        }
                    }
                    true
                });
                if key.as_deref() == Some("last_step_index") {
                    if let Some(v) = val {
                        if let Ok(idx) = v.parse::<i64>() {
                            found = Some(idx);
                            return false;
                        }
                    }
                }
            }
        }
        true
    });
    found
}

pub fn parse_generation_metadata_with_fallback(
    blob: &[u8],
    conversation: &str,
    index: i64,
    fallback_date: Option<DateTime<Utc>>,
) -> Record {
    let chat_model = match proto::message(blob, 1).or_else(|| proto::message(blob, 2)) {
        Some(m) => m,
        None => {
            return Record {
                entry: None,
                discarded_counters: 0,
            }
        }
    };
    let usage = match proto::message(chat_model, 4) {
        Some(u) => u,
        None => {
            return Record {
                entry: None,
                discarded_counters: 0,
            }
        }
    };
    let date = match created_at(chat_model).or(fallback_date) {
        Some(d) => d,
        None => {
            return Record {
                entry: None,
                discarded_counters: 0,
            }
        }
    };

    let identity = proto::string(usage, 11)
        .map(|id| format!("antigravity|{id}"))
        .unwrap_or_else(|| format!("antigravity|{conversation}|{index}"));

    let model = proto::string(chat_model, 19).unwrap_or_else(|| "unknown".to_string());

    let input_opt = proto::token_count(usage, 2);
    let output_opt = proto::token_count(usage, 3);
    let cache_write_opt = proto::token_count(usage, 4);
    let cache_read_opt = proto::token_count(usage, 5);

    let discarded = [input_opt, output_opt, cache_write_opt, cache_read_opt]
        .iter()
        .filter(|o| o.is_none())
        .count();

    let input = input_opt.unwrap_or(0);
    let output = output_opt.unwrap_or(0);
    let cache_write = cache_write_opt.unwrap_or(0);
    let cache_read = cache_read_opt.unwrap_or(0);

    let total = input + output + cache_write + cache_read;
    let entry = if total > 0 {
        Some(Entry {
            id: identity,
            date,
            local_day: reader::local_day(date),
            model: format!("antigravity/{model}"),
            input,
            output,
            cache_write,
            cache_read,
            explicit_cost: Some(0.0),
        })
    } else {
        None
    };

    Record {
        entry,
        discarded_counters: discarded,
    }
}

fn created_at(chat_model: &[u8]) -> Option<DateTime<Utc>> {
    let start = proto::message(chat_model, 9)?;
    let stamp = proto::message(start, 4)?;
    let seconds = proto::varint(stamp, 1)?;
    if !(1_000_000_000..=4_102_444_800).contains(&seconds) {
        return None;
    }
    let nanos = proto::varint(stamp, 2)
        .filter(|&n| n < 1_000_000_000)
        .unwrap_or(0);
    Utc.timestamp_opt(seconds as i64, nanos as u32).single()
}

// MARK: - SQLite database reader

#[derive(Debug, Clone)]
pub enum ConversationRead {
    Complete {
        entries: Vec<Entry>,
        discarded_counters: usize,
    },
    IncompleteScan {
        status: i32,
        rows: usize,
    },
    Unreadable {
        status: Option<i32>,
    },
    NotAConversation,
}

impl ConversationRead {
    pub fn entries(&self) -> &[Entry] {
        match self {
            ConversationRead::Complete { entries, .. } => entries,
            _ => &[],
        }
    }
}

pub fn conversation_entries(database: &Path) -> ConversationRead {
    let conversation = database
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown");

    let conn = match open_read_only(database) {
        Some(c) => c,
        None => return ConversationRead::Unreadable { status: None },
    };

    let file_mtime = fs::metadata(database)
        .ok()
        .and_then(|m| m.modified().ok())
        .map(DateTime::<Utc>::from);

    let mut step_timestamps = HashMap::new();
    if let Ok(mut step_stmt) =
        conn.prepare("SELECT idx, metadata FROM steps WHERE metadata IS NOT NULL")
    {
        if let Ok(mut step_rows) = step_stmt.query([]) {
            while let Ok(Some(row)) = step_rows.next() {
                let idx: i64 = row.get(0).unwrap_or(0);
                if let Ok(blob) = row.get::<_, Vec<u8>>(1) {
                    if let Some(stamp) = proto::message(&blob, 1) {
                        let sec = proto::varint(stamp, 1);
                        let nano = proto::varint(stamp, 2);
                        if let Some(s) = sec {
                            if (1_000_000_000..=4_102_444_800).contains(&s) {
                                if let Some(dt) =
                                    DateTime::from_timestamp(s as i64, nano.unwrap_or(0) as u32)
                                {
                                    step_timestamps.insert(idx, dt);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    let mut stmt = match conn.prepare("SELECT idx, data FROM gen_metadata WHERE data IS NOT NULL") {
        Ok(s) => s,
        Err(e) => {
            let err_str = e.to_string();
            if err_str.contains("no such table") {
                return ConversationRead::NotAConversation;
            }
            return ConversationRead::Unreadable { status: None };
        }
    };

    let mut entries = Vec::new();
    let mut discarded_counters = 0;
    let mut rows = 0;

    let mut query_rows = match stmt.query([]) {
        Ok(r) => r,
        Err(_) => return ConversationRead::Unreadable { status: None },
    };

    loop {
        match query_rows.next() {
            Ok(Some(row)) => {
                rows += 1;
                let idx: i64 = row.get(0).unwrap_or(0);
                if let Ok(blob) = row.get::<_, Vec<u8>>(1) {
                    if !blob.is_empty() {
                        let chat_model_opt = proto::message(&blob, 1).or_else(|| proto::message(&blob, 2));
                        let last_step_opt = chat_model_opt.and_then(extract_last_step_index);
                        let fallback = last_step_opt
                            .and_then(|s_idx| step_timestamps.get(&s_idx).copied())
                            .or_else(|| step_timestamps.get(&idx).copied())
                            .or(file_mtime);
                        let record = parse_generation_metadata_with_fallback(
                            &blob,
                            conversation,
                            idx,
                            fallback,
                        );
                        discarded_counters += record.discarded_counters;
                        if let Some(entry) = record.entry {
                            entries.push(entry);
                        }
                    }
                }
            }
            Ok(None) => break,
            Err(e) => {
                let code = e.sqlite_error_code().map(|c| c as i32).unwrap_or(-1);
                return ConversationRead::IncompleteScan { status: code, rows };
            }
        }
    }

    ConversationRead::Complete {
        entries,
        discarded_counters,
    }
}

fn open_read_only(database: &Path) -> Option<Connection> {
    if !database.exists() {
        return None;
    }
    // Try mode=ro URI first, then immutable=1 URI
    let path_str = database.to_str()?;
    let normalized = path_str.replace('\\', "/");
    for params in ["mode=ro", "immutable=1"] {
        let uri = format!("file:{}?{}", normalized, params);
        let flags = OpenFlags::SQLITE_OPEN_READ_ONLY
            | OpenFlags::SQLITE_OPEN_URI
            | OpenFlags::SQLITE_OPEN_NO_MUTEX;
        if let Ok(conn) = Connection::open_with_flags(&uri, flags) {
            // Verify with test read
            if conn
                .query_row("SELECT count(*) FROM sqlite_master", [], |_| Ok(()))
                .is_ok()
            {
                return Some(conn);
            }
        }
    }
    // Direct open fallback if URI mode fails
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    if let Ok(conn) = Connection::open_with_flags(database, flags) {
        if conn
            .query_row("SELECT count(*) FROM sqlite_master", [], |_| Ok(()))
            .is_ok()
        {
            return Some(conn);
        }
    }
    None
}

// MARK: - Blob & Scanning

#[derive(Debug, Clone)]
pub struct Blob {
    pub mtime: DateTime<Utc>,
    pub size: usize,
    pub entries: Vec<Entry>,
}

#[derive(Debug, Clone, Default)]
pub struct ScanResult {
    pub blobs: HashMap<PathBuf, Blob>,
    pub log: Vec<String>,
}

pub fn signature(database: &Path) -> Option<(DateTime<Utc>, usize)> {
    let mut newest: Option<DateTime<Utc>> = None;
    let mut total_size: usize = 0;

    for ext in ["", "-wal"] {
        let p = if ext.is_empty() {
            database.to_path_buf()
        } else {
            let mut s = database.as_os_str().to_os_string();
            s.push(ext);
            PathBuf::from(s)
        };
        if let Ok(meta) = fs::metadata(&p) {
            if let Ok(mtime) = meta.modified() {
                let dt: DateTime<Utc> = mtime.into();
                newest = Some(match newest {
                    Some(prev) => prev.max(dt),
                    None => dt,
                });
            }
            total_size += meta.len() as usize;
        }
    }

    newest.map(|dt| (dt, total_size))
}

pub fn scan(
    root: &Path,
    modified_since: DateTime<Utc>,
    known: &HashMap<PathBuf, Blob>,
) -> ScanResult {
    let mut blobs = HashMap::new();
    let mut reads = Vec::new();

    let entries = match fs::read_dir(root) {
        Ok(e) => e,
        Err(_) => return ScanResult::default(),
    };

    let mut db_paths = Vec::new();
    for entry in entries.flatten() {
        let p = entry.path();
        if p.extension().and_then(|s| s.to_str()) == Some("db") {
            db_paths.push(p);
        }
    }
    db_paths.sort();

    for db_path in db_paths {
        let sig = match signature(&db_path) {
            Some(s) if s.0 >= modified_since => s,
            _ => continue,
        };

        if let Some(existing) = known.get(&db_path) {
            if existing.mtime == sig.0 && existing.size == sig.1 {
                blobs.insert(db_path, existing.clone());
                continue;
            }
        }

        let conv_name = db_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();

        let read = conversation_entries(&db_path);
        reads.push((conv_name, read.clone()));

        match read {
            ConversationRead::Complete { entries, .. } => {
                blobs.insert(
                    db_path,
                    Blob {
                        mtime: sig.0,
                        size: sig.1,
                        entries,
                    },
                );
            }
            ConversationRead::NotAConversation => {
                blobs.insert(
                    db_path,
                    Blob {
                        mtime: sig.0,
                        size: sig.1,
                        entries: Vec::new(),
                    },
                );
            }
            ConversationRead::IncompleteScan { .. } | ConversationRead::Unreadable { .. } => {
                if let Some(stale) = known.get(&db_path) {
                    blobs.insert(db_path, stale.clone());
                }
            }
        }
    }

    let mut log = loss_log(&reads);
    log.extend(discard_log(&reads));

    ScanResult { blobs, log }
}

pub fn assemble(blobs: &HashMap<PathBuf, Blob>, since: DateTime<Utc>) -> Vec<Entry> {
    let all: Vec<Entry> = blobs
        .values()
        .flat_map(|b| b.entries.iter().cloned())
        .filter(|e| e.date >= since)
        .collect();
    reader::dedup_keep_max(all)
}

pub fn loss_log(reads: &[(String, ConversationRead)]) -> Vec<String> {
    let mut lines = Vec::new();
    for (conv, read) in reads {
        match read {
            ConversationRead::Complete { .. } | ConversationRead::NotAConversation => {}
            ConversationRead::IncompleteScan { status, rows } => {
                lines.push(format!(
                    "antigravity: lost conversation={conv} reason=scan-incomplete status={status} rows={rows}"
                ));
            }
            ConversationRead::Unreadable { status } => {
                let status_str = status.map(|s| format!(" status={s}")).unwrap_or_default();
                lines.push(format!(
                    "antigravity: lost conversation={conv} reason=unreadable{status_str}"
                ));
            }
        }
    }
    cap_log(lines, |total, hidden| {
        format!("antigravity: lost {total} conversation(s) this scan ({hidden} not named)")
    })
}

pub fn discard_log(reads: &[(String, ConversationRead)]) -> Vec<String> {
    let mut lines = Vec::new();
    for (conv, read) in reads {
        if let ConversationRead::Complete {
            discarded_counters, ..
        } = read
        {
            if *discarded_counters > 0 {
                lines.push(format!(
                    "antigravity: conversation={conv} discarded={discarded_counters} token counter(s) over {TOKEN_CEILING}"
                ));
            }
        }
    }
    cap_log(lines, |total, hidden| {
        format!(
            "antigravity: discarded token counters in {total} conversation(s) ({hidden} not named)"
        )
    })
}

fn cap_log<F>(lines: Vec<String>, summary: F) -> Vec<String>
where
    F: FnOnce(usize, usize) -> String,
{
    if lines.len() <= NAMED_LOSS_LIMIT {
        lines
    } else {
        let hidden = lines.len() - NAMED_LOSS_LIMIT;
        let mut out: Vec<String> = lines.into_iter().take(NAMED_LOSS_LIMIT).collect();
        out.push(summary(out.len() + hidden, hidden));
        out
    }
}

// MARK: - Cache & Provider

pub struct LocalAntigravityUsageCache {
    root: Option<PathBuf>,
    blobs: HashMap<PathBuf, Blob>,
}

impl LocalAntigravityUsageCache {
    pub fn new(root: Option<PathBuf>) -> Self {
        Self {
            root,
            blobs: HashMap::new(),
        }
    }

    pub fn shared() -> Arc<Mutex<Self>> {
        static INSTANCE: std::sync::OnceLock<Arc<Mutex<LocalAntigravityUsageCache>>> =
            std::sync::OnceLock::new();
        INSTANCE
            .get_or_init(|| Arc::new(Mutex::new(LocalAntigravityUsageCache::new(None))))
            .clone()
    }

    pub fn entries(&mut self, now: DateTime<Utc>) -> Vec<Entry> {
        let since = reader::enrichment_scan_start(now.with_timezone(&Local)).with_timezone(&Utc);
        let roots = match &self.root {
            Some(r) => vec![r.clone()],
            None => all_roots(),
        };
        let mut all_blobs = self.blobs.clone();
        for root in roots {
            let scan_result = scan(&root, since, &all_blobs);
            all_blobs.extend(scan_result.blobs);
        }
        self.blobs = all_blobs;
        assemble(&self.blobs, since)
    }
}

pub struct LocalAntigravityProvider;

impl UsageProvider for LocalAntigravityProvider {
    fn id(&self) -> &'static str {
        "antigravity"
    }

    fn display_name(&self) -> &'static str {
        "Antigravity"
    }

    fn reports_cost(&self) -> bool {
        false
    }

    fn fetch_daily(&self) -> Option<DailyUsage> {
        let now = Utc::now();
        let entries = LocalAntigravityUsageCache::shared()
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .entries(now);
        let today = reader::daily(&entries, &reader::today_key())?;
        Some(DailyUsage::new(
            today.date,
            today.input_tokens,
            today.output_tokens,
            today.cache_creation_tokens,
            today.cache_read_tokens,
            today.total_tokens,
            0.0,
        ))
    }

    fn fetch_enrichment(&self) -> ProviderEnrichment {
        let now = Utc::now();
        let local_now = now.with_timezone(&Local);
        let entries = LocalAntigravityUsageCache::shared()
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .entries(now);

        let month_start = reader::start_of_month(local_now);
        let week_start = reader::start_of_week(local_now);
        let week_key = reader::local_day(week_start.with_timezone(&Utc));
        let to_day = reader::local_day(now);

        let week = reader::period(&entries, &week_key, &week_key, &to_day);
        let month = reader::period(
            &entries,
            &reader::month_key(local_now),
            &reader::local_day(month_start.with_timezone(&Utc)),
            &to_day,
        );

        ProviderEnrichment {
            active_block: reader::active_block(&entries, now),
            blocks_ok: true,
            week_total: Some(PeriodUsage::new(week.period, week.total_tokens, 0.0)),
            month_total: Some(PeriodUsage::new(month.period, month.total_tokens, 0.0)),
            periods_ok: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encode_varint(mut val: u64) -> Vec<u8> {
        let mut buf = Vec::new();
        while val >= 0x80 {
            buf.push(((val & 0x7f) | 0x80) as u8);
            val >>= 7;
        }
        buf.push(val as u8);
        buf
    }

    fn encode_field_varint(field: usize, val: u64) -> Vec<u8> {
        let tag = (field as u64) << 3;
        let mut bytes = encode_varint(tag);
        bytes.extend(encode_varint(val));
        bytes
    }

    fn encode_field_bytes(field: usize, payload: &[u8]) -> Vec<u8> {
        let tag = ((field as u64) << 3) | 2;
        let mut bytes = encode_varint(tag);
        bytes.extend(encode_varint(payload.len() as u64));
        bytes.extend_from_slice(payload);
        bytes
    }

    fn encode_field_str(field: usize, s: &str) -> Vec<u8> {
        encode_field_bytes(field, s.as_bytes())
    }

    #[allow(clippy::too_many_arguments)]
    fn build_test_blob(
        response_id: Option<&str>,
        model: &str,
        created_sec: u64,
        created_nanos: u64,
        input: u64,
        output: u64,
        cache_read: u64,
        cache_write: u64,
    ) -> Vec<u8> {
        let mut usage = Vec::new();
        if input > 0 {
            usage.extend(encode_field_varint(2, input));
        }
        if output > 0 {
            usage.extend(encode_field_varint(3, output));
        }
        if cache_write > 0 {
            usage.extend(encode_field_varint(4, cache_write));
        }
        if cache_read > 0 {
            usage.extend(encode_field_varint(5, cache_read));
        }
        if let Some(rid) = response_id {
            usage.extend(encode_field_str(11, rid));
        }

        let mut stamp = Vec::new();
        stamp.extend(encode_field_varint(1, created_sec));
        if created_nanos > 0 {
            stamp.extend(encode_field_varint(2, created_nanos));
        }

        let mut start_meta = Vec::new();
        start_meta.extend(encode_field_bytes(4, &stamp));

        let mut chat_model = Vec::new();
        chat_model.extend(encode_field_bytes(4, &usage));
        chat_model.extend(encode_field_bytes(9, &start_meta));
        chat_model.extend(encode_field_str(19, model));

        let mut root = Vec::new();
        root.extend(encode_field_bytes(1, &chat_model));
        root
    }

    use tempfile::tempdir;

    fn write_conversation(dir: &Path, name: &str, blobs: &[Vec<u8>]) {
        let db_path = dir.join(format!("{name}.db"));
        let conn = Connection::open(&db_path).expect("open test db");
        conn.execute(
            "CREATE TABLE gen_metadata (idx INTEGER PRIMARY KEY, data BLOB);",
            [],
        )
        .expect("create table");
        for (i, blob) in blobs.iter().enumerate() {
            conn.execute(
                "INSERT INTO gen_metadata (idx, data) VALUES (?1, ?2);",
                rusqlite::params![i as i64, blob],
            )
            .expect("insert row");
        }
    }

    #[test]
    fn test_protobuf_parsing_and_token_mapping() {
        let blob = build_test_blob(
            Some("r1"),
            "gemini-3.6-flash",
            1772618400,
            0,
            4667,
            462,
            52968,
            0,
        );
        let rec = parse_generation_metadata(&blob, "c1", 0);
        let entry = rec.entry.expect("entry present");
        assert_eq!(entry.id, "antigravity|r1");
        assert_eq!(entry.input, 4667);
        assert_eq!(entry.output, 462);
        assert_eq!(entry.cache_read, 52968);
        assert_eq!(entry.cache_write, 0);
        assert_eq!(entry.total(), 4667 + 462 + 52968);
        assert_eq!(entry.model, "antigravity/gemini-3.6-flash");
        assert_eq!(rec.discarded_counters, 0);
    }

    #[test]
    fn test_total_is_the_sum_of_the_counters() {
        let blob = build_test_blob(
            Some("r1"),
            "gemini-3.6-flash",
            1772618400,
            0,
            100,
            20,
            300,
            0,
        );
        let rec = parse_generation_metadata(&blob, "c1", 0);
        let entry = rec.entry.expect("entry present");
        assert_eq!(entry.total(), 420);
    }

    #[test]
    fn test_sentinel_token_count_discarded() {
        let blob = build_test_blob(
            Some("r1"),
            "gemini-3.6-flash",
            1772618400,
            0,
            u64::MAX,
            20,
            300,
            0,
        );
        let rec = parse_generation_metadata(&blob, "c1", 0);
        let entry = rec.entry.expect("entry present");
        assert_eq!(entry.input, 0);
        assert_eq!(entry.output, 20);
        assert_eq!(entry.cache_read, 300);
        assert_eq!(entry.total(), 320);
        assert_eq!(rec.discarded_counters, 1);
    }

    #[test]
    fn test_zero_tokens_produces_no_entry() {
        let blob = build_test_blob(Some("r1"), "gemini-3.6-flash", 1772618400, 0, 0, 0, 0, 0);
        let rec = parse_generation_metadata(&blob, "c1", 0);
        assert!(rec.entry.is_none());
    }

    #[test]
    fn test_implausible_timestamp_rejected() {
        let blob = build_test_blob(Some("r1"), "gemini-3.6-flash", 0, 0, 100, 20, 300, 0);
        let rec = parse_generation_metadata(&blob, "c1", 0);
        assert!(rec.entry.is_none());
    }

    #[test]
    fn test_response_id_deduplicates_across_conversations() {
        let tmp = tempdir().expect("tempdir");
        let shared = build_test_blob(
            Some("same-call"),
            "gemini-3.6-flash",
            1772618400,
            0,
            100,
            20,
            300,
            0,
        );
        write_conversation(tmp.path(), "c1", std::slice::from_ref(&shared));
        write_conversation(tmp.path(), "c2", std::slice::from_ref(&shared));

        let entries = LocalAntigravityUsageCache::new(Some(tmp.path().to_path_buf()))
            .entries(Utc.timestamp_opt(1772618400, 0).unwrap());
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, "antigravity|same-call");
    }

    #[test]
    fn test_record_without_response_id_falls_back_to_conversation_and_index() {
        let tmp = tempdir().expect("tempdir");
        let blob = build_test_blob(None, "gemini-3.6-flash", 1772618400, 0, 100, 20, 300, 0);
        write_conversation(tmp.path(), "c1", &[blob]);

        let entries = LocalAntigravityUsageCache::new(Some(tmp.path().to_path_buf()))
            .entries(Utc.timestamp_opt(1772618400, 0).unwrap());
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, "antigravity|c1|0");
    }

    #[test]
    fn test_discard_log_names_a_few_stores_and_counts_the_rest() {
        let limit = NAMED_LOSS_LIMIT;
        let mut reads = Vec::new();
        for i in 0..(limit + 2) {
            reads.push((
                format!("c{i}"),
                ConversationRead::Complete {
                    entries: Vec::new(),
                    discarded_counters: 3,
                },
            ));
        }

        let lines = discard_log(&reads);
        assert_eq!(lines.len(), limit + 1);
        assert!(lines
            .last()
            .unwrap()
            .contains(&format!("in {} conversation(s)", limit + 2)));
        assert!(lines.last().unwrap().contains("(2 not named)"));
    }

    #[test]
    fn test_database_without_the_expected_table_is_ignored() {
        let tmp = tempdir().expect("tempdir");
        let db_path = tmp.path().join("c1.db");
        let conn = Connection::open(&db_path).expect("open db");
        conn.execute("CREATE TABLE something_else (a INTEGER);", [])
            .expect("create other table");

        let entries =
            LocalAntigravityUsageCache::new(Some(tmp.path().to_path_buf())).entries(Utc::now());
        assert!(entries.is_empty());
    }

    #[test]
    fn test_unreadable_store_is_not_an_empty_conversation() {
        let tmp = tempdir().expect("tempdir");
        let db_path = tmp.path().join("c1.db");
        fs::write(&db_path, b"not a sqlite database").expect("write bad file");

        let read = conversation_entries(&db_path);
        match read {
            ConversationRead::Unreadable { .. } => {}
            _ => panic!("expected unreadable, got {:?}", read),
        }
        let lines = loss_log(&[("c1".to_string(), read)]);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("conversation=c1"));
        assert!(lines[0].contains("reason=unreadable"));
    }

    #[test]
    fn test_parse_legacy_field2_format() {
        let blob = build_test_blob(
            Some("legacy-1"),
            "gemini-3.7-flash",
            1772618400,
            0,
            1200,
            300,
            4000,
            0,
        );
        let rec = parse_generation_metadata(&blob, "c1", 0);
        let entry = rec.entry.expect("entry present");
        assert_eq!(entry.id, "antigravity|legacy-1");
        assert_eq!(entry.model, "antigravity/gemini-3.7-flash");
        assert_eq!(entry.total(), 5500);
    }

    #[test]
    fn test_parse_modern_cortex_format_with_last_step_index() {
        let mut usage = Vec::new();
        usage.extend(encode_field_varint(2, 5000)); // input
        usage.extend(encode_field_varint(3, 1000)); // output
        usage.extend(encode_field_varint(5, 200000)); // cache read
        usage.extend(encode_field_str(11, "modern-resp-1"));

        let mut step_pair = Vec::new();
        step_pair.extend(encode_field_str(1, "last_step_index"));
        step_pair.extend(encode_field_str(2, "42"));

        let mut chat_model = Vec::new();
        chat_model.extend(encode_field_bytes(4, &usage));
        chat_model.extend(encode_field_str(19, "gemini-3.7-flash"));
        chat_model.extend(encode_field_bytes(20, &step_pair));

        // Modern cortex puts chat_model in field 1
        let mut root = Vec::new();
        root.extend(encode_field_bytes(1, &chat_model));

        let tmp = tempdir().expect("tempdir");
        let db_path = tmp.path().join("c_modern.db");
        let conn = Connection::open(&db_path).expect("open test db");
        conn.execute("CREATE TABLE gen_metadata (idx INTEGER PRIMARY KEY, data BLOB);", []).unwrap();
        conn.execute("CREATE TABLE steps (idx INTEGER PRIMARY KEY, metadata BLOB);", []).unwrap();

        // Insert step 42 with timestamp for 2026-08-20
        let mut step_stamp = Vec::new();
        step_stamp.extend(encode_field_varint(1, 1787140000));
        let mut step_meta = Vec::new();
        step_meta.extend(encode_field_bytes(1, &step_stamp));
        conn.execute("INSERT INTO steps (idx, metadata) VALUES (42, ?1);", rusqlite::params![step_meta]).unwrap();

        // Insert gen_metadata with idx 0
        conn.execute("INSERT INTO gen_metadata (idx, data) VALUES (0, ?1);", rusqlite::params![root]).unwrap();

        let read = conversation_entries(&db_path);
        let entries = match read {
            ConversationRead::Complete { entries, .. } => entries,
            _ => panic!("expected complete read"),
        };
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, "antigravity|modern-resp-1");
        assert_eq!(entries[0].model, "antigravity/gemini-3.7-flash");
        assert_eq!(entries[0].total(), 206000);
        assert_eq!(entries[0].date.timestamp(), 1787140000);
    }
}
