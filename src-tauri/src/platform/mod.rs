//! Platform plumbing: filesystem paths, shell environment lookup, and process
//! spawning.
//!
//! Mirrors the original `BinaryLocator.swift`, `UsageEnvironment.swift`,
//! `AppEnv.swift`, and `AppLog.swift`. XDG on Linux, `%APPDATA%` /
//! `%LOCALAPPDATA%` on Windows, `~/Library` on macOS.
//!
//! Single source of truth for env lookup — no provider reads a usage-location
//! environment variable directly (see docs/architecture.md §7).
