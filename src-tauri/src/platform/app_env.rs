//! Execution-environment detection — the single gate for "real app" side
//! effects (logging, notifications). Port of `AppEnv.swift`.

/// Whether we are running as a packaged app.
///
/// The macOS original checks for a `.app` bundle (`Bundle.main.bundleIdentifier
/// != nil` plus a `.app` path suffix). Linux/Windows builds have no bundle to
/// inspect, so we approximate: Tauri only packages release builds, hence a
/// release build means packaged. Refine once packaging lands.
pub fn is_bundled_app() -> bool {
    !cfg!(debug_assertions)
}
