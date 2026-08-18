//! Usage aggregation — mirrors the portable core of `UsageStore.swift`.
//!
//! Fetches daily/enrichment data from the registered providers, aggregates
//! today/week/month totals, and drives the [`CompanionStore`] growth loop.
//!
//! Deferred (Phase 2/4, all need macOS services or official-limit providers):
//! - official Claude/Codex limit windows, five-hour forecast, limit alerts
//!   (notification + floating-pet bubble), provider-status/incident polling,
//!   429 backoff, keychain, the poll timer and sleep/wake suspension.

use std::collections::HashMap;

use chrono::{DateTime, Duration, Utc};

use crate::companion::store::{BurnTier, CompanionStore};
use crate::domain::companion::WindowClass;
use crate::domain::format::TokenFormatter;
use crate::domain::models::ProviderSnapshot;
use crate::providers::reader;
use crate::providers::UsageProvider;

/// How a limit percentage is displayed: amount used (default) or amount left.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LimitDisplayMode {
    #[default]
    Used,
    Remaining,
}

/// A five-hour-limit depletion forecast.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FiveHourForecast {
    pub depletion_date: DateTime<Utc>,
    pub before_reset: bool,
}

/// One limit-alert firing instruction (pure decision result, separated from
/// side effects so it can be unit-tested).
#[derive(Debug, Clone, PartialEq)]
pub struct LimitAlert {
    /// Unique key per window — used for tier tracking / notification ids.
    pub key: String,
    /// Display name shown in the notification body.
    pub window: String,
    pub is_critical: bool,
    pub utilization: f64,
}

/// Aggregates provider usage and drives the companion.
pub struct UsageStore {
    pub providers: Vec<Box<dyn UsageProvider>>,
    pub snapshots: Vec<ProviderSnapshot>,
    pub last_updated: Option<DateTime<Utc>>,

    // Settings (persistence to a config file is deferred to Phase 2).
    pub refresh_interval: i64,
    pub warn_threshold: f64,
    pub crit_threshold: f64,
    pub show_tokens_in_menu: bool,
    pub show_cost_in_menu: bool,
    pub show_limit_in_menu: bool,
    pub limit_display_mode: LimitDisplayMode,

    /// Limit-alert edge state: window key → highest tier already notified
    /// (0 = none, 1 = warn, 2 = critical). Not persisted across restarts.
    pub notified_tier: HashMap<String, i64>,
}

impl UsageStore {
    pub fn new(providers: Vec<Box<dyn UsageProvider>>) -> Self {
        Self {
            providers,
            snapshots: Vec::new(),
            last_updated: None,
            refresh_interval: 120,
            warn_threshold: 80.0,
            crit_threshold: 95.0,
            show_tokens_in_menu: true,
            show_cost_in_menu: false,
            show_limit_in_menu: false,
            limit_display_mode: LimitDisplayMode::Used,
            notified_tier: HashMap::new(),
        }
    }

    /// Registered provider ids (registry-integrity helper).
    pub fn registered_provider_ids(&self) -> Vec<String> {
        self.providers.iter().map(|p| p.id().to_string()).collect()
    }

    /// Fetch daily + enrichment from all providers, aggregate, and drive the
    /// companion growth loop. Synchronous; call from a background thread.
    pub fn refresh(&mut self, companion: &mut CompanionStore) {
        let today_key = reader::today_key();
        let now = Utc::now();

        // Phase 1: daily (critical path) — a provider contributes a snapshot
        // only when it actually has today's data.
        let mut new_snapshots: Vec<ProviderSnapshot> = Vec::new();
        for provider in &self.providers {
            if let Some(today) = provider.fetch_daily() {
                let mut snapshot = ProviderSnapshot::new(
                    provider.id().to_string(),
                    provider.display_name().to_string(),
                    Some(today),
                    None,
                    None,
                    None,
                    now,
                );
                snapshot.reports_cost = provider.reports_cost();
                new_snapshots.push(snapshot);
            }
        }
        self.snapshots = new_snapshots;
        self.last_updated = Some(now);

        // Phase 2: block/week/month enrichment (best effort — failures keep
        // the values unset; see the Swift's keep-previous semantics).
        for provider in &self.providers {
            let enrichment = provider.fetch_enrichment();
            if let Some(snapshot) = self
                .snapshots
                .iter_mut()
                .find(|s| s.provider_id == provider.id())
            {
                if enrichment.blocks_ok {
                    snapshot.active_block = enrichment.active_block;
                }
                if enrichment.periods_ok {
                    snapshot.week_total = enrichment.week_total;
                    snapshot.month_total = enrichment.month_total;
                }
            }
        }

        // Drive the companion.
        let by_provider = self.today_tokens_by_provider();
        let month_total = self.month_total_tokens();
        let burn_tier = self.burn_tier();
        let has_usage_data = self.has_usage_data();
        companion.update(
            &by_provider,
            &today_key,
            month_total,
            burn_tier,
            false,
            has_usage_data,
        );
    }

    // MARK: derived values

    pub fn today_total_tokens(&self) -> i64 {
        let today_key = reader::today_key();
        self.snapshots
            .iter()
            .filter(|s| s.today.as_ref().is_some_and(|t| t.date == today_key))
            .map(|s| s.today.as_ref().map(|t| t.total_tokens).unwrap_or(0))
            .sum()
    }

    /// Today's usage keyed by provider id (date-guarded: only providers whose
    /// `today` is actually today's date participate).
    pub fn today_tokens_by_provider(&self) -> HashMap<String, i64> {
        let today_key = reader::today_key();
        let mut out = HashMap::new();
        for s in &self.snapshots {
            if let Some(today) = &s.today {
                if today.date == today_key {
                    out.insert(s.provider_id.clone(), today.total_tokens);
                }
            }
        }
        out
    }

    pub fn has_usage_data(&self) -> bool {
        !self.snapshots.is_empty()
    }

    /// Snapshots that participate in cost aggregates (excludes flat-rate
    /// providers that report tokens only).
    pub fn costing_snapshots(&self) -> impl Iterator<Item = &ProviderSnapshot> {
        self.snapshots.iter().filter(|s| s.reports_cost)
    }

    pub fn shows_cost(&self) -> bool {
        self.snapshots.iter().any(|s| s.reports_cost)
    }

    pub fn today_cost_total(&self) -> f64 {
        let today_key = reader::today_key();
        self.costing_snapshots()
            .filter(|s| s.today.as_ref().is_some_and(|t| t.date == today_key))
            .map(|s| s.today.as_ref().map(|t| t.total_cost).unwrap_or(0.0))
            .sum()
    }

    pub fn week_total_tokens(&self) -> i64 {
        self.snapshots
            .iter()
            .map(|s| s.week_total.as_ref().map(|w| w.total_tokens).unwrap_or(0))
            .sum()
    }

    pub fn week_cost_total(&self) -> f64 {
        self.costing_snapshots()
            .map(|s| s.week_total.as_ref().map(|w| w.total_cost).unwrap_or(0.0))
            .sum()
    }

    pub fn month_total_tokens(&self) -> i64 {
        self.snapshots
            .iter()
            .map(|s| s.month_total.as_ref().map(|m| m.total_tokens).unwrap_or(0))
            .sum()
    }

    pub fn month_cost_total(&self) -> f64 {
        self.costing_snapshots()
            .map(|s| s.month_total.as_ref().map(|m| m.total_cost).unwrap_or(0.0))
            .sum()
    }

    /// Burn-rate tier (summed across all providers' active blocks) — the
    /// companion display state (idle/working/focus) derives from this.
    pub fn burn_tier(&self) -> BurnTier {
        let burn: f64 = self
            .snapshots
            .iter()
            .filter_map(|s| s.active_block.as_ref().and_then(|b| b.tokens_per_minute))
            .sum();
        if burn <= 1_000.0 {
            BurnTier::Idle
        } else if burn < 100_000.0 {
            BurnTier::Normal
        } else if burn < 400_000.0 {
            BurnTier::Fast
        } else {
            BurnTier::Blazing
        }
    }

    pub fn is_stale(&self) -> bool {
        let Some(last) = self.last_updated else {
            return true;
        };
        let allowance = if self.refresh_interval > 0 {
            self.refresh_interval * 2
        } else {
            1800
        };
        Utc::now().signed_duration_since(last).num_seconds() > allowance
    }

    // MARK: menu-bar display

    /// Menu-bar lines. (The limit line is omitted until official limits land in
    /// Phase 2 — `menu_limit_line` returns `None` for now.)
    pub fn menu_lines(&self) -> Vec<String> {
        if self.last_updated.is_none() {
            return vec!["—".to_string()];
        }
        let mut usage: Vec<String> = Vec::new();
        if self.show_tokens_in_menu {
            usage.push(TokenFormatter::compact(self.today_total_tokens()));
        }
        if self.show_cost_in_menu && self.shows_cost() {
            usage.push(TokenFormatter::cost_compact(self.today_cost_total()));
        }
        usage
    }

    pub fn menu_title(&self) -> String {
        self.menu_lines().join(" · ")
    }

    // MARK: pure, testable helpers

    pub fn display_percent(utilization: f64, mode: LimitDisplayMode) -> f64 {
        match mode {
            LimitDisplayMode::Remaining => (100.0 - utilization).max(0.0),
            LimitDisplayMode::Used => utilization,
        }
    }

    /// Extrapolate when the 5h limit reaches 100%. Returns `None` when the
    /// estimate would be unstable (utilization <5%, burn <10k tokens/min,
    /// already ≥100%, or the projection exceeds 24h).
    pub fn forecast_depletion(
        block_tokens: i64,
        tokens_per_minute: f64,
        utilization: f64,
        now: DateTime<Utc>,
    ) -> Option<DateTime<Utc>> {
        if !(5.0..100.0).contains(&utilization) || block_tokens <= 0 || tokens_per_minute < 10_000.0
        {
            return None;
        }
        let tokens_per_percent = block_tokens as f64 / utilization;
        let minutes_left = (100.0 - utilization) * tokens_per_percent / tokens_per_minute;
        if !minutes_left.is_finite() || minutes_left >= 60.0 * 24.0 {
            return None;
        }
        Some(now + Duration::seconds((minutes_left * 60.0) as i64))
    }

    /// Codex window classification — ≤24h (1440 min) = session, over = weekly;
    /// unknown (None) treated as session (conservative).
    pub fn window_class(minutes: Option<i64>) -> WindowClass {
        if let Some(m) = minutes {
            if m > 1440 {
                return WindowClass::Weekly;
            }
        }
        WindowClass::Session
    }

    /// Edge-triggered limit-alert evaluation. Fires only when a window crosses
    /// a threshold for the first time; re-arms when utilization drops below the
    /// warn line; never re-alerts while staying in the same tier. Mutates `tiers`
    /// (the per-window notified tier).
    pub fn evaluate_limit_alerts(
        windows: &[(String, String, f64)],
        warn: f64,
        crit: f64,
        tiers: &mut HashMap<String, i64>,
    ) -> Vec<LimitAlert> {
        let mut alerts = Vec::new();
        for (key, name, utilization) in windows {
            let tier = if *utilization >= crit {
                2
            } else if *utilization >= warn {
                1
            } else {
                0
            };
            if tier == 0 {
                tiers.remove(key);
                continue;
            }
            let previous = tiers.get(key).copied().unwrap_or(0);
            if tier <= previous {
                continue;
            }
            tiers.insert(key.clone(), tier);
            alerts.push(LimitAlert {
                key: key.clone(),
                window: name.clone(),
                is_critical: tier == 2,
                utilization: *utilization,
            });
        }
        alerts
    }

    /// Pick the single bubble to show for a refresh: critical beats warn, then
    /// the highest utilization.
    pub fn bubble_alert(alerts: &[LimitAlert]) -> Option<LimitAlert> {
        let mut best: Option<&LimitAlert> = None;
        for a in alerts {
            let better = match best {
                None => true,
                Some(b) => {
                    if a.is_critical != b.is_critical {
                        a.is_critical
                    } else {
                        a.utilization > b.utilization
                    }
                }
            };
            if better {
                best = Some(a);
            }
        }
        best.cloned()
    }

    /// Whether a bubble shown at `shown_at` should clear by `now` (default TTL 6s).
    pub fn should_dismiss_bubble(
        shown_at: DateTime<Utc>,
        now: DateTime<Utc>,
        ttl_secs: i64,
    ) -> bool {
        now.signed_duration_since(shown_at).num_seconds() >= ttl_secs
    }

    /// Exponential backoff for rate-limited limit fetches: 5min → 10min → … → 60min.
    pub fn next_limits_backoff(after: i64) -> i64 {
        if after == 0 {
            300
        } else {
            (after * 2).min(3600)
        }
    }
}

impl Default for UsageStore {
    /// Default providers: all ten providers (Claude Code, Codex, Gemini, Grok,
    /// Antigravity, OpenCode, Hermes Agent, Cursor, Copilot, Kiro).
    fn default() -> Self {
        Self::new(vec![
            Box::new(crate::providers::local::LocalClaudeProvider),
            Box::new(crate::providers::local::LocalCodexProvider),
            Box::new(crate::providers::local::LocalGeminiProvider),
            Box::new(crate::providers::local::LocalGrokProvider),
            Box::new(crate::providers::antigravity::LocalAntigravityProvider),
            Box::new(crate::providers::additional::LocalOpenCodeProvider),
            Box::new(crate::providers::additional::LocalHermesProvider),
            Box::new(crate::providers::additional::LocalCursorProvider),
            Box::new(crate::providers::additional::LocalCopilotProvider),
            Box::new(crate::providers::additional::LocalKiroProvider),
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::models::{BlockUsage, DailyUsage};

    fn window(key: &str, name: &str, u: f64) -> (String, String, f64) {
        (key.to_string(), name.to_string(), u)
    }

    #[test]
    fn evaluate_limit_alerts_fires_once_per_tier_and_rearms() {
        let mut tiers = HashMap::new();
        // crosses warn (1) then crit (2) — two alerts
        let alerts =
            UsageStore::evaluate_limit_alerts(&[window("w", "5h", 90.0)], 80.0, 95.0, &mut tiers);
        assert_eq!(alerts.len(), 1);
        assert!(!alerts[0].is_critical);
        // stays above warn but below crit — no re-alert
        let alerts =
            UsageStore::evaluate_limit_alerts(&[window("w", "5h", 92.0)], 80.0, 95.0, &mut tiers);
        assert!(alerts.is_empty());
        // crosses crit
        let alerts =
            UsageStore::evaluate_limit_alerts(&[window("w", "5h", 96.0)], 80.0, 95.0, &mut tiers);
        assert_eq!(alerts.len(), 1);
        assert!(alerts[0].is_critical);
        // drops below warn → re-arm
        let alerts =
            UsageStore::evaluate_limit_alerts(&[window("w", "5h", 10.0)], 80.0, 95.0, &mut tiers);
        assert!(alerts.is_empty());
        assert!(!tiers.contains_key("w"));
        // crossing warn again fires again
        let alerts =
            UsageStore::evaluate_limit_alerts(&[window("w", "5h", 85.0)], 80.0, 95.0, &mut tiers);
        assert_eq!(alerts.len(), 1);
    }

    #[test]
    fn evaluate_limit_alerts_distinguishes_windows_by_key() {
        let mut tiers = HashMap::new();
        let alerts = UsageStore::evaluate_limit_alerts(
            &[
                window("a", "same name", 90.0),
                window("b", "same name", 90.0),
            ],
            80.0,
            95.0,
            &mut tiers,
        );
        assert_eq!(alerts.len(), 2);
    }

    #[test]
    fn bubble_alert_prefers_critical_then_highest_utilization() {
        let warn = LimitAlert {
            key: "a".into(),
            window: "w".into(),
            is_critical: false,
            utilization: 90.0,
        };
        let crit = LimitAlert {
            key: "b".into(),
            window: "w".into(),
            is_critical: true,
            utilization: 70.0,
        };
        let picked = UsageStore::bubble_alert(&[warn, crit]).unwrap();
        assert!(picked.is_critical);
        assert_eq!(picked.key, "b");
    }

    #[test]
    fn should_dismiss_bubble_respects_ttl() {
        let now = Utc::now();
        let shown = now - Duration::seconds(7);
        assert!(UsageStore::should_dismiss_bubble(shown, now, 6));
        let shown = now - Duration::seconds(5);
        assert!(!UsageStore::should_dismiss_bubble(shown, now, 6));
    }

    #[test]
    fn display_percent_clamps_remaining_at_zero() {
        assert_eq!(
            UsageStore::display_percent(120.0, LimitDisplayMode::Remaining),
            0.0
        );
        assert_eq!(
            UsageStore::display_percent(40.0, LimitDisplayMode::Remaining),
            60.0
        );
        assert_eq!(
            UsageStore::display_percent(40.0, LimitDisplayMode::Used),
            40.0
        );
    }

    #[test]
    fn forecast_depletion_guards() {
        let now = Utc::now();
        assert!(UsageStore::forecast_depletion(0, 20_000.0, 50.0, now).is_none());
        assert!(UsageStore::forecast_depletion(100_000, 5_000.0, 50.0, now).is_none());
        assert!(UsageStore::forecast_depletion(100_000, 20_000.0, 4.0, now).is_none());
        assert!(UsageStore::forecast_depletion(100_000, 20_000.0, 100.0, now).is_none());
        let f = UsageStore::forecast_depletion(100_000, 20_000.0, 50.0, now).unwrap();
        assert!(f > now);
    }

    #[test]
    fn window_class_threshold() {
        assert_eq!(UsageStore::window_class(None), WindowClass::Session);
        assert_eq!(UsageStore::window_class(Some(1440)), WindowClass::Session);
        assert_eq!(UsageStore::window_class(Some(1441)), WindowClass::Weekly);
    }

    #[test]
    fn next_limits_backoff_exponential_capped() {
        assert_eq!(UsageStore::next_limits_backoff(0), 300);
        assert_eq!(UsageStore::next_limits_backoff(300), 600);
        assert_eq!(UsageStore::next_limits_backoff(1800), 3600);
        assert_eq!(UsageStore::next_limits_backoff(3600), 3600);
    }

    fn snapshot_with_block(tpm: f64) -> ProviderSnapshot {
        let mut s = ProviderSnapshot::new(
            "test".into(),
            "Test".into(),
            None,
            Some(BlockUsage {
                id: "b".into(),
                start_time: "x".into(),
                end_time: "y".into(),
                is_active: true,
                total_tokens: 0,
                cost_usd: 0.0,
                tokens_per_minute: Some(tpm),
            }),
            None,
            None,
            Utc::now(),
        );
        s.reports_cost = true;
        s
    }

    #[test]
    fn burn_tier_matches_thresholds() {
        let mut store = UsageStore::new(Vec::new());
        store.snapshots = vec![snapshot_with_block(500.0)];
        assert_eq!(store.burn_tier(), BurnTier::Idle);
        store.snapshots = vec![snapshot_with_block(50_000.0)];
        assert_eq!(store.burn_tier(), BurnTier::Normal);
        store.snapshots = vec![snapshot_with_block(200_000.0)];
        assert_eq!(store.burn_tier(), BurnTier::Fast);
        store.snapshots = vec![snapshot_with_block(500_000.0)];
        assert_eq!(store.burn_tier(), BurnTier::Blazing);
    }

    #[test]
    fn today_totals_are_date_guarded() {
        let today_key = reader::today_key();
        let mut store = UsageStore::new(Vec::new());
        let make = |date: &str| {
            let mut s = ProviderSnapshot::new(
                "p".into(),
                "P".into(),
                Some(DailyUsage::new(date.into(), 100, 0, 0, 0, 100, 0.0)),
                None,
                None,
                None,
                Utc::now(),
            );
            s.reports_cost = true;
            s
        };
        store.snapshots = vec![make(&today_key), make("2000-01-01")];
        assert_eq!(store.today_total_tokens(), 100);
        assert_eq!(store.today_tokens_by_provider().get("p"), Some(&100));
    }
}
