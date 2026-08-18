//! Usage providers — one module per AI CLI (Claude Code, Codex, Gemini CLI,
//! Antigravity, OpenCode, Hermes Agent, Cursor, Grok CLI, Copilot CLI, Kiro CLI).
//!
//! Each implements a common `UsageProvider` trait and registers in a central
//! `providers![]` list (mirrors the original `Core/UsageProvider.swift` +
//! `UsageStore.init`). Also hosts the PokéAPI client and provider-status checkers.
//!
//! Rules carried over from the original (see docs/architecture.md §7):
//! - No literal `providerID == "..."` branches on generic paths.
//! - External log formats are validated against the upstream *writer*.
//! - Dedup by the turn's own globally-unique id; timestamp by turn time;
//!   subagent sessions fold into their parent.

pub mod reader;

pub use reader::{
    active_block, claude_entries, claude_entries_in_root, claude_project_roots, codex_entries,
    daily, dedup_keep_max, gemini_entries, grok_entries, period, Entry,
};
