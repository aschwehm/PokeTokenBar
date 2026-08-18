//! Platform plumbing: filesystem paths, shell environment lookup, and process
//! spawning.
//!
//! Mirrors the original `BinaryLocator.swift`, `UsageEnvironment.swift`,
//! `AppEnv.swift`, and `AppLog.swift`. XDG on Linux, `%APPDATA%` /
//! `%LOCALAPPDATA%` on Windows, `~/Library` on macOS.
//!
//! Single source of truth for env lookup — no provider reads a usage-location
//! environment variable directly (see docs/architecture.md §7).

pub mod app_env;
pub mod binary_locator;
pub mod env;
pub mod log;

use std::path::PathBuf;
use std::sync::OnceLock;

/// App data directory: `~/.local/share/poketokenbar` on Unix,
/// `%LOCALAPPDATA%\poketokenbar` on Windows. Parent directories are created on
/// demand. Mirrors the macOS original's Application Support folder.
pub fn data_dir() -> PathBuf {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let path = compute_data_dir();
        if !path.as_os_str().is_empty() {
            let _ = std::fs::create_dir_all(&path);
        }
        path
    })
    .clone()
}

fn compute_data_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        std::env::var("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_default()
            .join("poketokenbar")
    }
    #[cfg(not(target_os = "windows"))]
    {
        let home =
            crate::platform::binary_locator::home_dir().unwrap_or_else(|| PathBuf::from("."));
        home.join(".local/share/poketokenbar")
    }
}
