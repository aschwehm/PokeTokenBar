//! Append-only file logger with single-generation rotation (debug/troubleshooting
//! aid). Port of `AppLog.swift`.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, SyncSender};
use std::sync::OnceLock;

use crate::platform::app_env;

/// Log ceiling — past this the current log is rotated to `PokeTokenBar.old.log`
/// (disk ceiling ≈ 2× max = 4 MB). A 24/7 menu-bar app rotates often; the
/// `.old` generation keeps pre-crash context around.
const MAX_BYTES: u64 = 2 * 1024 * 1024;

enum Msg {
    Line(String),
    Flush(SyncSender<()>),
}

static CHANNEL: OnceLock<Sender<Msg>> = OnceLock::new();

/// Log file location: `~/.local/share/poketokenbar/logs/PokeTokenBar.log` on
/// Unix, `%LOCALAPPDATA%\poketokenbar\logs\PokeTokenBar.log` on Windows. Parent
/// directories are created on demand.
pub fn log_file_path() -> PathBuf {
    static PATH: OnceLock<PathBuf> = OnceLock::new();
    PATH.get_or_init(|| {
        let path = compute_log_file_path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        path
    })
    .clone()
}

fn compute_log_file_path() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        let base = std::env::var("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::new());
        base.join("poketokenbar")
            .join("logs")
            .join("PokeTokenBar.log")
    }
    #[cfg(not(target_os = "windows"))]
    {
        let home =
            crate::platform::binary_locator::home_dir().unwrap_or_else(|| PathBuf::from("."));
        home.join(".local/share/poketokenbar/logs/PokeTokenBar.log")
    }
}

/// Wait until queued lines have reached the file. Use only where the process is
/// about to exit — `write` is async, so a record just before `exit(0)` would
/// otherwise be lost entirely.
pub fn flush() {
    if let Some(sender) = CHANNEL.get() {
        let (ack_tx, ack_rx) = std::sync::mpsc::sync_channel(1);
        if sender.send(Msg::Flush(ack_tx)).is_ok() {
            let _ = ack_rx.recv();
        }
    }
}

/// Append a line to the log. Only active in a packaged (non-debug) build so
/// `cargo test` / dev runs never pollute a production log. Returns immediately;
/// the worker thread owns all file I/O.
pub fn write(message: &str) {
    if !app_env::is_bundled_app() {
        return;
    }
    let line = format!("[{}] {message}\n", timestamp());
    let sender = CHANNEL.get_or_init(spawn_worker);
    let _ = sender.send(Msg::Line(line));
}

fn spawn_worker() -> Sender<Msg> {
    let (tx, rx) = std::sync::mpsc::channel();
    let _ = std::thread::Builder::new()
        .name("poketokenbar.log".to_string())
        .spawn(move || worker(rx));
    tx
}

fn worker(rx: Receiver<Msg>) {
    for msg in rx {
        match msg {
            Msg::Line(line) => {
                let path = log_file_path();
                rotate_if_needed(&path);
                append_line(&path, &line);
            }
            // FIFO ordering guarantees every previously queued line has been
            // written by the time we acknowledge.
            Msg::Flush(ack) => {
                let _ = ack.send(());
            }
        }
    }
}

fn rotate_if_needed(path: &Path) {
    if let Ok(meta) = std::fs::metadata(path) {
        if meta.len() > MAX_BYTES {
            let old = path.with_extension("old.log");
            let _ = std::fs::remove_file(&old);
            let _ = std::fs::rename(path, &old);
        }
    }
}

fn append_line(path: &Path, line: &str) {
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = file.write_all(line.as_bytes());
    }
}

fn timestamp() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}
