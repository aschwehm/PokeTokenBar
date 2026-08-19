//! Additional local SQLite providers: OpenCode, Hermes Agent, Cursor, Copilot, Kiro.
//!
//! Port of `Core/LocalAdditionalUsageProvider.swift`.

use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Local, TimeZone, Utc};
use rusqlite::{Connection, OpenFlags};
use serde_json::Value;

use crate::domain::decoding::parse_iso8601;
use crate::domain::models::DailyUsage;
use crate::providers::reader::{self, Entry};
use crate::providers::{ProviderEnrichment, UsageProvider};

fn enrichment(entries: &[Entry]) -> ProviderEnrichment {
    let now = Utc::now();
    let local_now = now.with_timezone(&Local);
    let month_start = reader::start_of_month(local_now);
    let week_start = reader::start_of_week(local_now);
    let week_key = reader::local_day(week_start.with_timezone(&Utc));
    let to_day = reader::local_day(now);

    let week = reader::period(entries, &week_key, &week_key, &to_day);
    let month = reader::period(
        entries,
        &reader::month_key(local_now),
        &reader::local_day(month_start.with_timezone(&Utc)),
        &to_day,
    );

    ProviderEnrichment {
        active_block: reader::active_block(entries, now),
        blocks_ok: true,
        week_total: Some(week),
        month_total: Some(month),
        periods_ok: true,
    }
}

fn open_ro(database: &Path) -> Option<Connection> {
    if !database.exists() {
        return None;
    }
    let path_str = database.to_str()?;
    let normalized = path_str.replace('\\', "/");
    for params in ["mode=ro", "immutable=1"] {
        let uri = format!("file:{}?{}", normalized, params);
        let flags = OpenFlags::SQLITE_OPEN_READ_ONLY
            | OpenFlags::SQLITE_OPEN_URI
            | OpenFlags::SQLITE_OPEN_NO_MUTEX;
        if let Ok(conn) = Connection::open_with_flags(&uri, flags) {
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

// MARK: - OpenCode

pub struct LocalOpenCodeProvider;

impl UsageProvider for LocalOpenCodeProvider {
    fn id(&self) -> &'static str {
        "opencode"
    }
    fn display_name(&self) -> &'static str {
        "OpenCode"
    }

    fn fetch_daily(&self) -> Option<DailyUsage> {
        let now = Utc::now();
        let since = reader::start_of_day(now.with_timezone(&Local)).with_timezone(&Utc);
        let entries = opencode_entries(since, None);
        reader::daily(&entries, &reader::today_key())
    }

    fn fetch_enrichment(&self) -> ProviderEnrichment {
        let now = Utc::now();
        let since = reader::enrichment_scan_start(now.with_timezone(&Local)).with_timezone(&Utc);
        let entries = opencode_entries(since, None);
        enrichment(&entries)
    }
}

pub fn opencode_default_roots() -> Vec<PathBuf> {
    if let Ok(val) = std::env::var("OPENCODE_DATA_DIR") {
        if !val.trim().is_empty() {
            return vec![PathBuf::from(val)];
        }
    }
    let home = crate::platform::binary_locator::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let mut roots = vec![home.join(".local/share/opencode")];
    #[cfg(target_os = "windows")]
    {
        for wsl_home in crate::platform::binary_locator::wsl_home_dirs() {
            let wsl_opencode = wsl_home.join(".local/share/opencode");
            if wsl_opencode.is_dir() {
                roots.push(wsl_opencode);
            }
            let wsl_opencode_root = wsl_home.join(".opencode");
            if wsl_opencode_root.is_dir() {
                roots.push(wsl_opencode_root);
            }
        }
    }
    roots
}

pub fn opencode_entries(modified_since: DateTime<Utc>, roots: Option<Vec<PathBuf>>) -> Vec<Entry> {
    let source_roots = roots.unwrap_or_else(opencode_default_roots);
    let mut entries = Vec::new();
    for root in source_roots {
        if let Some(db) = preferred_opencode_db(&root) {
            entries.extend(opencode_db_entries(&db, modified_since));
        }
        let legacy_root = root.join("storage/message");
        if let Ok(read_dir) = fs::read_dir(legacy_root) {
            for file in read_dir.flatten() {
                let p = file.path();
                if p.extension().and_then(|s| s.to_str()) == Some("json") {
                    if let Ok(content) = fs::read_to_string(&p) {
                        if let Ok(val) = serde_json::from_str::<Value>(&content) {
                            let fb = p.file_stem().and_then(|s| s.to_str()).unwrap_or("unknown");
                            if let Some(e) = parse_opencode_message(&val, fb) {
                                if e.date >= modified_since {
                                    entries.push(e);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    reader::dedup_keep_max(entries)
}

fn preferred_opencode_db(root: &Path) -> Option<PathBuf> {
    if root.extension().and_then(|s| s.to_str()) == Some("db") {
        return Some(root.to_path_buf());
    }
    let std_db = root.join("opencode.db");
    if std_db.exists() {
        return Some(std_db);
    }
    if let Ok(dir) = fs::read_dir(root) {
        let mut dbs: Vec<PathBuf> = dir
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .map(|s| s.starts_with("opencode-") && s.ends_with(".db"))
                    .unwrap_or(false)
            })
            .collect();
        dbs.sort();
        return dbs.into_iter().next();
    }
    None
}

fn opencode_db_entries(database: &Path, modified_since: DateTime<Utc>) -> Vec<Entry> {
    let conn = match open_ro(database) {
        Some(c) => c,
        None => return Vec::new(),
    };
    let cutoff = modified_since.timestamp_millis();
    let mut stmt = conn
        .prepare("SELECT id, session_id, data FROM message WHERE time_created >= ?1")
        .or_else(|_| conn.prepare("SELECT id, session_id, data FROM message"));
    let query_stmt = match stmt {
        Ok(ref mut s) => s,
        Err(_) => return Vec::new(),
    };

    let rows = query_stmt.query_map([cutoff], |row| {
        let id: String = row.get(0).unwrap_or_default();
        let payload: String = row.get(2).unwrap_or_default();
        Ok((id, payload))
    });

    let mut entries = Vec::new();
    if let Ok(rows) = rows {
        for item in rows.flatten() {
            if let Ok(val) = serde_json::from_str::<Value>(&item.1) {
                if let Some(entry) = parse_opencode_message(&val, &item.0) {
                    if entry.date >= modified_since {
                        entries.push(entry);
                    }
                }
            }
        }
    }
    entries
}

fn parse_opencode_message(val: &Value, fallback_id: &str) -> Option<Entry> {
    let tokens = val.get("tokens")?.as_object()?;
    let created = val.get("time")?.get("created")?;
    let date = match created {
        Value::Number(n) => {
            let millis = n.as_i64()?;
            Utc.timestamp_millis_opt(millis).single()?
        }
        Value::String(s) => parse_iso8601(s)?,
        _ => return None,
    };
    let model = val.get("modelID")?.as_str()?.to_string();
    let _ = val.get("providerID")?.as_str()?;
    let id_str = val
        .get("id")
        .and_then(|v| v.as_str())
        .unwrap_or(fallback_id);

    let cache = tokens.get("cache").and_then(|v| v.as_object());
    let input = tokens.get("input").and_then(|v| v.as_i64()).unwrap_or(0);
    let output = tokens.get("output").and_then(|v| v.as_i64()).unwrap_or(0);
    let cache_write = cache
        .and_then(|c| c.get("write"))
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let cache_read = cache
        .and_then(|c| c.get("read"))
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let cost = val.get("cost").and_then(|v| v.as_f64());

    if input + output + cache_write + cache_read == 0 {
        return None;
    }

    Some(Entry {
        id: format!("opencode|{id_str}"),
        date,
        local_day: reader::local_day(date),
        model,
        input,
        output,
        cache_write,
        cache_read,
        explicit_cost: cost,
    })
}

// MARK: - Hermes

pub struct LocalHermesProvider;

impl UsageProvider for LocalHermesProvider {
    fn id(&self) -> &'static str {
        "hermes"
    }
    fn display_name(&self) -> &'static str {
        "Hermes Agent"
    }

    fn fetch_daily(&self) -> Option<DailyUsage> {
        let now = Utc::now();
        let since = reader::start_of_day(now.with_timezone(&Local)).with_timezone(&Utc);
        let entries = hermes_entries(since, None);
        reader::daily(&entries, &reader::today_key())
    }

    fn fetch_enrichment(&self) -> ProviderEnrichment {
        let now = Utc::now();
        let since = reader::enrichment_scan_start(now.with_timezone(&Local)).with_timezone(&Utc);
        let entries = hermes_entries(since, None);
        enrichment(&entries)
    }
}

pub fn hermes_default_roots() -> Vec<PathBuf> {
    if let Ok(val) = std::env::var("HERMES_HOME") {
        if !val.trim().is_empty() {
            return vec![PathBuf::from(val)];
        }
    }
    let home = crate::platform::binary_locator::home_dir().unwrap_or_else(|| PathBuf::from("."));
    vec![home.join(".hermes")]
}

pub fn hermes_entries(modified_since: DateTime<Utc>, roots: Option<Vec<PathBuf>>) -> Vec<Entry> {
    let source_roots = roots.unwrap_or_else(hermes_default_roots);
    let mut entries = Vec::new();
    for root in source_roots {
        let db_path = if root.extension().and_then(|s| s.to_str()) == Some("db") {
            root
        } else {
            root.join("state.db")
        };
        entries.extend(hermes_db_entries(&db_path, modified_since));
    }
    reader::dedup_keep_max(entries)
}

fn hermes_db_entries(database: &Path, modified_since: DateTime<Utc>) -> Vec<Entry> {
    let conn = match open_ro(database) {
        Some(c) => c,
        None => return Vec::new(),
    };
    let sql = "SELECT id, model, billing_provider, started_at, message_count, \
               input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, \
               reasoning_tokens, estimated_cost_usd, actual_cost_usd \
               FROM sessions \
               WHERE model IS NOT NULL AND TRIM(model) != '' AND started_at >= ?1";

    let mut stmt = match conn.prepare(sql) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };

    let cutoff = modified_since.timestamp();
    let rows = stmt.query_map([cutoff], |row| {
        let id: String = row.get(0).unwrap_or_default();
        let model: String = row.get(1).unwrap_or_default();
        let started_at: f64 = row.get(3).unwrap_or(0.0);
        let input: i64 = row.get(5).unwrap_or(0);
        let output: i64 = row.get(6).unwrap_or(0);
        let cache_read: i64 = row.get(7).unwrap_or(0);
        let cache_write: i64 = row.get(8).unwrap_or(0);
        let reasoning: i64 = row.get(9).unwrap_or(0);
        let est_cost: f64 = row.get(10).unwrap_or(0.0);
        let act_cost: f64 = row.get(11).unwrap_or(0.0);
        Ok((
            id,
            model,
            started_at,
            input,
            output + reasoning,
            cache_read,
            cache_write,
            if act_cost > 0.0 { act_cost } else { est_cost },
        ))
    });

    let mut entries = Vec::new();
    if let Ok(rows) = rows {
        for r in rows.flatten() {
            let (id, model, started_at, input, output, cache_read, cache_write, cost) = r;
            if id.trim().is_empty() || model.trim().is_empty() {
                continue;
            }
            let date = Utc
                .timestamp_opt(started_at as i64, ((started_at.fract()) * 1e9) as u32)
                .single();
            if let Some(date) = date {
                if date >= modified_since && input + output + cache_write + cache_read > 0 {
                    entries.push(Entry {
                        id: format!("hermes|{}", id.trim()),
                        date,
                        local_day: reader::local_day(date),
                        model: model.trim().to_string(),
                        input,
                        output,
                        cache_write,
                        cache_read,
                        explicit_cost: Some(cost),
                    });
                }
            }
        }
    }
    entries
}

// MARK: - Cursor

pub struct LocalCursorProvider;

impl UsageProvider for LocalCursorProvider {
    fn id(&self) -> &'static str {
        "cursor"
    }
    fn display_name(&self) -> &'static str {
        "Cursor"
    }
    fn reports_cost(&self) -> bool {
        false
    }

    fn fetch_daily(&self) -> Option<DailyUsage> {
        let now = Utc::now();
        let since = reader::start_of_day(now.with_timezone(&Local)).with_timezone(&Utc);
        let entries = cursor_entries(since, None);
        let d = reader::daily(&entries, &reader::today_key())?;
        Some(DailyUsage::new(
            d.date,
            d.input_tokens,
            d.output_tokens,
            d.cache_creation_tokens,
            d.cache_read_tokens,
            d.total_tokens,
            0.0,
        ))
    }

    fn fetch_enrichment(&self) -> ProviderEnrichment {
        let now = Utc::now();
        let since = reader::enrichment_scan_start(now.with_timezone(&Local)).with_timezone(&Utc);
        let entries = cursor_entries(since, None);
        let mut e = enrichment(&entries);
        if let Some(ref mut w) = e.week_total {
            w.total_cost = 0.0;
        }
        if let Some(ref mut m) = e.month_total {
            m.total_cost = 0.0;
        }
        e
    }
}

pub fn cursor_default_roots() -> Vec<PathBuf> {
    if let Ok(val) = std::env::var("CURSOR_DATA_DIR") {
        if !val.trim().is_empty() {
            return vec![PathBuf::from(val)];
        }
    }
    #[cfg(target_os = "macos")]
    {
        let home = crate::platform::binary_locator::home_dir().unwrap_or_else(|| PathBuf::from("."));
        vec![
            home.join("Library/Application Support/Cursor/User/globalStorage"),
            home.join("Library/Application Support/Cursor Nightly/User/globalStorage"),
        ]
    }
    #[cfg(target_os = "windows")]
    {
        let appdata = std::env::var("APPDATA").unwrap_or_default();
        vec![
            PathBuf::from(&appdata).join("Cursor/User/globalStorage"),
            PathBuf::from(&appdata).join("Cursor Nightly/User/globalStorage"),
        ]
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let home = crate::platform::binary_locator::home_dir().unwrap_or_else(|| PathBuf::from("."));
        vec![
            home.join(".config/Cursor/User/globalStorage"),
            home.join(".config/Cursor Nightly/User/globalStorage"),
        ]
    }
}

pub fn cursor_entries(modified_since: DateTime<Utc>, roots: Option<Vec<PathBuf>>) -> Vec<Entry> {
    let source_roots = roots.unwrap_or_else(cursor_default_roots);
    let mut entries = Vec::new();
    for root in source_roots {
        let db_path = if root.extension().and_then(|s| s.to_str()) == Some("vscdb") {
            root
        } else {
            root.join("state.vscdb")
        };
        entries.extend(cursor_db_entries(&db_path, modified_since));
    }
    reader::dedup_keep_max(entries)
}

fn cursor_db_entries(database: &Path, modified_since: DateTime<Utc>) -> Vec<Entry> {
    let conn = match open_ro(database) {
        Some(c) => c,
        None => return Vec::new(),
    };
    let sql = "SELECT rowid, key, value FROM cursorDiskKV WHERE key GLOB 'bubbleId:*'";
    let mut stmt = match conn.prepare(sql) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };

    let rows = stmt.query_map([], |row| {
        let key: String = row.get(1).unwrap_or_default();
        let val: String = row.get(2).unwrap_or_default();
        Ok((key, val))
    });

    let mut entries = Vec::new();
    if let Ok(rows) = rows {
        for item in rows.flatten() {
            if let Ok(obj) = serde_json::from_str::<Value>(&item.1) {
                if let Some(entry) = parse_cursor_bubble(&obj, &item.0, modified_since) {
                    entries.push(entry);
                }
            }
        }
    }
    entries
}

fn parse_cursor_bubble(obj: &Value, key: &str, modified_since: DateTime<Utc>) -> Option<Entry> {
    let tc = obj.get("tokenCount")?.as_object()?;
    let input = tc.get("inputTokens").and_then(|v| v.as_i64()).unwrap_or(0);
    let output = tc.get("outputTokens").and_then(|v| v.as_i64()).unwrap_or(0);
    if input + output == 0 {
        return None;
    }
    let created = obj.get("createdAt")?;
    let date = match created {
        Value::String(s) => parse_iso8601(s)?,
        Value::Number(n) => {
            let num = n.as_i64()?;
            if num > 1_000_000_000_000 {
                Utc.timestamp_millis_opt(num).single()?
            } else {
                Utc.timestamp_opt(num, 0).single()?
            }
        }
        _ => return None,
    };
    if date < modified_since {
        return None;
    }
    let model = obj
        .get("modelType")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");

    Some(Entry {
        id: format!("cursor|{key}"),
        date,
        local_day: reader::local_day(date),
        model: model.to_string(),
        input,
        output,
        cache_write: 0,
        cache_read: 0,
        explicit_cost: Some(0.0),
    })
}

// MARK: - Copilot CLI

pub struct LocalCopilotProvider;

impl UsageProvider for LocalCopilotProvider {
    fn id(&self) -> &'static str {
        "copilot"
    }
    fn display_name(&self) -> &'static str {
        "Copilot"
    }
    fn reports_cost(&self) -> bool {
        false
    }

    fn fetch_daily(&self) -> Option<DailyUsage> {
        let now = Utc::now();
        let since = reader::start_of_day(now.with_timezone(&Local)).with_timezone(&Utc);
        let entries = copilot_entries(since, None);
        let d = reader::daily(&entries, &reader::today_key())?;
        Some(DailyUsage::new(
            d.date,
            d.input_tokens,
            d.output_tokens,
            d.cache_creation_tokens,
            d.cache_read_tokens,
            d.total_tokens,
            0.0,
        ))
    }

    fn fetch_enrichment(&self) -> ProviderEnrichment {
        let now = Utc::now();
        let since = reader::enrichment_scan_start(now.with_timezone(&Local)).with_timezone(&Utc);
        let entries = copilot_entries(since, None);
        let mut e = enrichment(&entries);
        if let Some(ref mut w) = e.week_total {
            w.total_cost = 0.0;
        }
        if let Some(ref mut m) = e.month_total {
            m.total_cost = 0.0;
        }
        e
    }
}

pub fn copilot_default_roots() -> Vec<PathBuf> {
    if let Ok(val) = std::env::var("COPILOT_HOME") {
        if !val.trim().is_empty() {
            return vec![PathBuf::from(val)];
        }
    }
    let home = crate::platform::binary_locator::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let mut roots = vec![home.join(".copilot")];
    #[cfg(target_os = "windows")]
    {
        for wsl_home in crate::platform::binary_locator::wsl_home_dirs() {
            let wsl_copilot = wsl_home.join(".copilot");
            if wsl_copilot.is_dir() {
                roots.push(wsl_copilot);
            }
        }
    }
    roots
}

pub fn copilot_entries(modified_since: DateTime<Utc>, roots: Option<Vec<PathBuf>>) -> Vec<Entry> {
    let source_roots = roots.unwrap_or_else(copilot_default_roots);
    let mut entries = Vec::new();
    for root in source_roots {
        let db_path = if root.extension().and_then(|s| s.to_str()) == Some("db") {
            root
        } else {
            root.join("session-store.db")
        };
        entries.extend(copilot_db_entries(&db_path, modified_since));
    }
    reader::dedup_keep_max(entries)
}

fn copilot_db_entries(database: &Path, modified_since: DateTime<Utc>) -> Vec<Entry> {
    let conn = match open_ro(database) {
        Some(c) => c,
        None => return Vec::new(),
    };
    let sql = "SELECT id, model, input_tokens, output_tokens, cache_read_tokens, \
               cache_write_tokens, created_at \
               FROM assistant_usage_events";
    let mut stmt = match conn.prepare(sql) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };

    let rows = stmt.query_map([], |row| {
        let id: i64 = row.get(0).unwrap_or(0);
        let model: String = row.get(1).unwrap_or_default();
        let input_raw: i64 = row.get(2).unwrap_or(0);
        let output: i64 = row.get(3).unwrap_or(0);
        let cache_read: i64 = row.get(4).unwrap_or(0);
        let cache_write: i64 = row.get(5).unwrap_or(0);
        let created_at: String = row.get(6).unwrap_or_default();
        Ok((
            id,
            model,
            input_raw,
            output,
            cache_read,
            cache_write,
            created_at,
        ))
    });

    let mut entries = Vec::new();
    if let Ok(rows) = rows {
        for item in rows.flatten() {
            let (id, model, input_raw, output, cache_read, cache_write, created_at) = item;
            let date = parse_copilot_date(&created_at);
            if let Some(date) = date {
                if date >= modified_since {
                    let input = (input_raw - cache_read - cache_write).max(0);
                    if input + output + cache_write + cache_read > 0 {
                        entries.push(Entry {
                            id: format!("copilot|{}|{}", database.to_string_lossy(), id),
                            date,
                            local_day: reader::local_day(date),
                            model: if model.is_empty() {
                                "unknown".to_string()
                            } else {
                                model
                            },
                            input,
                            output,
                            cache_write,
                            cache_read,
                            explicit_cost: Some(0.0),
                        });
                    }
                }
            }
        }
    }
    entries
}

fn parse_copilot_date(raw: &str) -> Option<DateTime<Utc>> {
    let mut text = raw.trim().to_string();
    if text.len() < 19 {
        return None;
    }
    if let Some(idx) = text.find(' ') {
        text.replace_range(idx..idx + 1, "T");
    }
    if text.len() >= 19 && !text[19..].contains(['Z', '+', '-']) {
        text.push('Z');
    }
    parse_iso8601(&text)
}

// MARK: - Kiro

pub struct LocalKiroProvider;

impl UsageProvider for LocalKiroProvider {
    fn id(&self) -> &'static str {
        "kiro"
    }
    fn display_name(&self) -> &'static str {
        "Kiro"
    }
    fn reports_cost(&self) -> bool {
        false
    }

    fn fetch_daily(&self) -> Option<DailyUsage> {
        let now = Utc::now();
        let since = reader::start_of_day(now.with_timezone(&Local)).with_timezone(&Utc);
        let entries = kiro_entries(since, None);
        let d = reader::daily(&entries, &reader::today_key())?;
        Some(DailyUsage::new(
            d.date,
            d.input_tokens,
            d.output_tokens,
            d.cache_creation_tokens,
            d.cache_read_tokens,
            d.total_tokens,
            0.0,
        ))
    }

    fn fetch_enrichment(&self) -> ProviderEnrichment {
        let now = Utc::now();
        let since = reader::enrichment_scan_start(now.with_timezone(&Local)).with_timezone(&Utc);
        let entries = kiro_entries(since, None);
        let mut e = enrichment(&entries);
        if let Some(ref mut w) = e.week_total {
            w.total_cost = 0.0;
        }
        if let Some(ref mut m) = e.month_total {
            m.total_cost = 0.0;
        }
        e
    }
}

pub fn kiro_default_roots() -> Vec<PathBuf> {
    if let Ok(val) = std::env::var("KIRO_CLI_HOME") {
        if !val.trim().is_empty() {
            return vec![PathBuf::from(val)];
        }
    }
    #[cfg(target_os = "macos")]
    {
        let home = crate::platform::binary_locator::home_dir().unwrap_or_else(|| PathBuf::from("."));
        vec![home.join("Library/Application Support/kiro-cli")]
    }
    #[cfg(target_os = "windows")]
    {
        let mut roots = Vec::new();
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            roots.push(PathBuf::from(local).join("kiro-cli"));
        }
        if let Ok(appdata) = std::env::var("APPDATA") {
            roots.push(PathBuf::from(appdata).join("kiro-cli"));
        }
        for wsl_home in crate::platform::binary_locator::wsl_home_dirs() {
            let wsl_kiro = wsl_home.join(".local/share/kiro-cli");
            if wsl_kiro.is_dir() {
                roots.push(wsl_kiro);
            }
            let wsl_kiro_cfg = wsl_home.join(".config/kiro-cli");
            if wsl_kiro_cfg.is_dir() {
                roots.push(wsl_kiro_cfg);
            }
        }
        roots
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let home = crate::platform::binary_locator::home_dir().unwrap_or_else(|| PathBuf::from("."));
        vec![
            home.join(".local/share/kiro-cli"),
            home.join(".config/kiro-cli"),
        ]
    }
}

pub fn kiro_entries(modified_since: DateTime<Utc>, roots: Option<Vec<PathBuf>>) -> Vec<Entry> {
    let source_roots = roots.unwrap_or_else(kiro_default_roots);
    let mut entries = Vec::new();
    for root in source_roots {
        let db_path = if root.extension().and_then(|s| s.to_str()) == Some("sqlite3") {
            root
        } else {
            root.join("data.sqlite3")
        };
        entries.extend(kiro_db_entries(&db_path, modified_since));
    }
    reader::dedup_keep_max(entries)
}

fn kiro_db_entries(database: &Path, modified_since: DateTime<Utc>) -> Vec<Entry> {
    let conn = match open_ro(database) {
        Some(c) => c,
        None => return Vec::new(),
    };
    let mut entries = Vec::new();

    // conversations_v2
    if let Ok(mut stmt) = conn.prepare("SELECT conversation_id, value FROM conversations_v2") {
        if let Ok(rows) = stmt.query_map([], |row| {
            let id: Option<String> = row.get(0).ok();
            let val: String = row.get(1).unwrap_or_default();
            Ok((id, val))
        }) {
            for item in rows.flatten() {
                if let Ok(obj) = serde_json::from_str::<Value>(&item.1) {
                    let conv_id = item
                        .0
                        .or_else(|| {
                            obj.get("conversation_id")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string())
                        })
                        .unwrap_or_else(|| database.to_string_lossy().to_string());
                    entries.extend(kiro_turn_entries(&conv_id, &obj, modified_since));
                }
            }
        }
    }

    // conversations (v1)
    if let Ok(mut stmt) = conn.prepare("SELECT value FROM conversations") {
        if let Ok(rows) = stmt.query_map([], |row| {
            let val: String = row.get(0).unwrap_or_default();
            Ok(val)
        }) {
            for val in rows.flatten() {
                if let Ok(obj) = serde_json::from_str::<Value>(&val) {
                    if let Some(conv_id) = obj.get("conversation_id").and_then(|v| v.as_str()) {
                        entries.extend(kiro_turn_entries(conv_id, &obj, modified_since));
                    }
                }
            }
        }
    }

    entries
}

fn kiro_turn_entries(
    conversation_id: &str,
    obj: &Value,
    modified_since: DateTime<Utc>,
) -> Vec<Entry> {
    let turns = match obj.get("history").and_then(|v| v.as_array()) {
        Some(t) => t,
        None => return Vec::new(),
    };
    let mut entries = Vec::new();
    let mut cumulative_history_bytes = kiro_json_byte_len(obj.get("latest_summary"));

    for turn in turns {
        let user_bytes = kiro_field_byte_len(turn.get("user"));
        let assistant_bytes = kiro_field_byte_len(turn.get("assistant"));

        let meta = turn.get("request_metadata").and_then(|v| v.as_object());
        if let Some(meta) = meta {
            let raw_ts = meta
                .get("request_start_timestamp_ms")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);
            if raw_ts > 0.0 {
                let date = Utc.timestamp_millis_opt(raw_ts as i64).single();
                if let Some(date) = date {
                    if date >= modified_since {
                        let prompt_bytes = cumulative_history_bytes + user_bytes;
                        let resp_size = meta
                            .get("response_size")
                            .and_then(|v| v.as_i64())
                            .unwrap_or(0);
                        let model = meta
                            .get("model_id")
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown");
                        let input = (prompt_bytes / 4) as i64;
                        let output = resp_size / 4;
                        if input + output > 0 {
                            entries.push(Entry {
                                id: format!("kiro|{}|{}", conversation_id, raw_ts as i64),
                                date,
                                local_day: reader::local_day(date),
                                model: model.to_string(),
                                input,
                                output,
                                cache_write: 0,
                                cache_read: 0,
                                explicit_cost: Some(0.0),
                            });
                        }
                    }
                }
            }
        }
        cumulative_history_bytes += user_bytes + assistant_bytes;
    }

    entries
}

fn kiro_json_byte_len(v: Option<&Value>) -> usize {
    match v {
        Some(Value::String(s)) => s.len(),
        Some(Value::Object(map)) => map.values().map(|val| kiro_json_byte_len(Some(val))).sum(),
        Some(Value::Array(arr)) => arr.iter().map(|val| kiro_json_byte_len(Some(val))).sum(),
        _ => 0,
    }
}

fn kiro_field_byte_len(v: Option<&Value>) -> usize {
    let obj = match v.and_then(|val| val.as_object()) {
        Some(o) => o,
        None => return 0,
    };
    let mut total = 0;
    for (k, val) in obj {
        if k == "images" {
            continue;
        }
        total += kiro_json_byte_len(Some(val));
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_opencode_parsing() {
        let tmp = tempdir().expect("tempdir");
        let db_path = tmp.path().join("opencode.db");
        let conn = Connection::open(&db_path).expect("open");
        conn.execute(
            "CREATE TABLE message (id TEXT PRIMARY KEY, session_id TEXT, time_created INTEGER, data TEXT);",
            [],
        )
        .expect("create table");

        let json = r#"{"id":"m1","modelID":"claude-3-5-sonnet","providerID":"anthropic","time":{"created":1772618400000},"tokens":{"input":100,"output":50,"cache":{"read":10,"write":5}},"cost":0.005}"#;
        conn.execute(
            "INSERT INTO message VALUES ('m1', 's1', 1772618400000, ?1);",
            [json],
        )
        .expect("insert");

        let entries = opencode_entries(
            Utc.timestamp_opt(1772618400, 0).unwrap(),
            Some(vec![tmp.path().to_path_buf()]),
        );
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, "opencode|m1");
        assert_eq!(entries[0].input, 100);
        assert_eq!(entries[0].output, 50);
        assert_eq!(entries[0].cache_read, 10);
        assert_eq!(entries[0].cache_write, 5);
        assert_eq!(entries[0].explicit_cost, Some(0.005));
    }

    #[test]
    fn test_hermes_parsing() {
        let tmp = tempdir().expect("tempdir");
        let db_path = tmp.path().join("state.db");
        let conn = Connection::open(&db_path).expect("open");
        conn.execute(
            "CREATE TABLE sessions (id TEXT PRIMARY KEY, model TEXT, billing_provider TEXT, \
             started_at REAL, message_count INTEGER, input_tokens INTEGER, output_tokens INTEGER, \
             cache_read_tokens INTEGER, cache_write_tokens INTEGER, reasoning_tokens INTEGER, \
             estimated_cost_usd REAL, actual_cost_usd REAL);",
            [],
        )
        .expect("create");

        conn.execute(
            "INSERT INTO sessions VALUES ('s1', 'gpt-4o', 'openai', 1772618400.0, 5, 200, 50, 20, 0, 10, 0.01, 0.012);",
            [],
        )
        .expect("insert");

        let entries = hermes_entries(
            Utc.timestamp_opt(1772618400, 0).unwrap(),
            Some(vec![tmp.path().to_path_buf()]),
        );
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, "hermes|s1");
        assert_eq!(entries[0].input, 200);
        assert_eq!(entries[0].output, 60); // 50 + 10 reasoning
        assert_eq!(entries[0].cache_read, 20);
        assert_eq!(entries[0].explicit_cost, Some(0.012));
    }

    #[test]
    fn test_cursor_parsing() {
        let tmp = tempdir().expect("tempdir");
        let db_path = tmp.path().join("state.vscdb");
        let conn = Connection::open(&db_path).expect("open");
        conn.execute(
            "CREATE TABLE cursorDiskKV (key TEXT UNIQUE, value TEXT);",
            [],
        )
        .expect("create");

        let json = r#"{"tokenCount":{"inputTokens":300,"outputTokens":150},"createdAt":"2026-03-04T10:00:00Z","modelType":"claude-3-5-sonnet"}"#;
        conn.execute(
            "INSERT INTO cursorDiskKV VALUES ('bubbleId:b1', ?1);",
            [json],
        )
        .expect("insert");

        let entries = cursor_entries(
            Utc.timestamp_opt(1772618400, 0).unwrap(),
            Some(vec![tmp.path().to_path_buf()]),
        );
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, "cursor|bubbleId:b1");
        assert_eq!(entries[0].input, 300);
        assert_eq!(entries[0].output, 150);
        assert_eq!(entries[0].explicit_cost, Some(0.0));
    }

    #[test]
    fn test_copilot_parsing() {
        let tmp = tempdir().expect("tempdir");
        let db_path = tmp.path().join("session-store.db");
        let conn = Connection::open(&db_path).expect("open");
        conn.execute(
            "CREATE TABLE assistant_usage_events (id INTEGER PRIMARY KEY, model TEXT, \
             input_tokens INTEGER, output_tokens INTEGER, cache_read_tokens INTEGER, \
             cache_write_tokens INTEGER, created_at TEXT);",
            [],
        )
        .expect("create");

        conn.execute(
            "INSERT INTO assistant_usage_events VALUES (1, 'gpt-4o', 500, 80, 100, 50, '2026-03-04 10:00:00');",
            [],
        )
        .expect("insert");

        let entries = copilot_entries(
            Utc.timestamp_opt(1772618400, 0).unwrap(),
            Some(vec![tmp.path().to_path_buf()]),
        );
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].input, 350); // 500 - 100 - 50
        assert_eq!(entries[0].output, 80);
        assert_eq!(entries[0].cache_read, 100);
        assert_eq!(entries[0].cache_write, 50);
        assert_eq!(entries[0].explicit_cost, Some(0.0));
    }

    #[test]
    fn test_kiro_parsing() {
        let tmp = tempdir().expect("tempdir");
        let db_path = tmp.path().join("data.sqlite3");
        let conn = Connection::open(&db_path).expect("open");
        conn.execute(
            "CREATE TABLE conversations_v2 (conversation_id TEXT, value TEXT);",
            [],
        )
        .expect("create");

        let json = r#"{"history":[{"user":{"content":"Hello world!"},"assistant":{"content":"Hi there!"},"request_metadata":{"request_start_timestamp_ms":1772618400000.0,"response_size":40,"model_id":"claude-3-5"}}]}"#;
        conn.execute("INSERT INTO conversations_v2 VALUES ('conv1', ?1);", [json])
            .expect("insert");

        let entries = kiro_entries(
            Utc.timestamp_opt(1772618400, 0).unwrap(),
            Some(vec![tmp.path().to_path_buf()]),
        );
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, "kiro|conv1|1772618400000");
        assert_eq!(entries[0].output, 10); // 40 / 4
        assert_eq!(entries[0].explicit_cost, Some(0.0));
    }
}
