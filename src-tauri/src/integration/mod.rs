//! Thin, per-platform wrappers over Tauri plugins and OS services.
//!
//! Home of: tray icon, notifications, launch-at-login, single-instance,
//! in-app updates, crash reporting, and the secret store. Mirrors the original
//! `LoginItem.swift`, `SingleInstance.swift`, `UpdateChecker.swift`,
//! `CrashReporter.swift`, and `KeychainAccess.swift`.
//!
//! Linux → Secret Service / XDG autostart / DBus; Windows → Credential Manager /
//! `HKCU\...\Run` / named mutex. The rest of the app never talks to these
//! services directly.

pub mod app;
pub mod tray;
