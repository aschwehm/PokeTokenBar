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

pub mod cache;
pub mod local;
pub mod reader;

pub use cache::LocalUsageCache;
pub use local::{LocalClaudeProvider, LocalCodexProvider, LocalGeminiProvider, LocalGrokProvider};
pub use reader::{
    active_block, claude_entries, claude_entries_in_root, claude_project_roots, codex_entries,
    daily, dedup_keep_max, gemini_entries, grok_entries, period, Entry,
};

use crate::domain::models::{BlockUsage, DailyUsage, PeriodUsage};

/// Provider extension point — a new source (Gemini, OpenCode, …) is added by
/// implementing this trait (port of `Core/UsageProvider.swift`).
pub trait UsageProvider: Send + Sync {
    fn id(&self) -> &'static str;
    fn display_name(&self) -> &'static str;
    /// Whether this provider contributes to cost aggregates / per-row cost UI.
    /// Flat-rate subscriptions (e.g. Cursor) report tokens only.
    fn reports_cost(&self) -> bool {
        true
    }

    /// Today's total (critical path) — the basis of the menu-bar number and the
    /// staleness decision. `None` when the source has no data or today is empty.
    fn fetch_daily(&self) -> Option<DailyUsage>;

    /// Active block / week-month totals (best effort) — slow or failed results
    /// must not affect the menu-bar number.
    fn fetch_enrichment(&self) -> ProviderEnrichment;
}

/// Result of a best-effort enrichment collection. An `*_ok` flag of `false`
/// means the collection failed → the caller keeps the previous value.
#[derive(Debug, Clone, Default)]
pub struct ProviderEnrichment {
    pub active_block: Option<BlockUsage>,
    pub blocks_ok: bool,
    pub week_total: Option<PeriodUsage>,
    pub month_total: Option<PeriodUsage>,
    pub periods_ok: bool,
}
