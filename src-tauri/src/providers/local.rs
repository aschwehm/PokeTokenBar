//! Local log-based providers (port of `Core/LocalUsageProvider.swift`).
//!
//! Each reads its provider's local logs through the shared incremental cache and
//! aggregates today / this week / this month. Codex is a subscription — its cost
//! is zeroed to match the original (the source reports no price).

use chrono::{DateTime, Local, Utc};

use crate::domain::models::{DailyUsage, PeriodUsage};
use crate::providers::reader::{self, Entry};
use crate::providers::{LocalUsageCache, ProviderEnrichment, UsageProvider};

fn lock_cache() -> std::sync::MutexGuard<'static, LocalUsageCache> {
    LocalUsageCache::shared()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Active block + week/month totals derived from a single scan. The mtime floor
/// is the earliest of the three windows' starts (`enrichment_scan_start`).
fn build_enrichment(entries: &[Entry], now: DateTime<Local>) -> ProviderEnrichment {
    let month_start = reader::start_of_month(now);
    let week_start = reader::start_of_week(now);
    let week_key = reader::local_day(week_start.with_timezone(&Utc));
    let to_day = reader::local_day(now.with_timezone(&Utc));
    ProviderEnrichment {
        active_block: reader::active_block(entries, now.with_timezone(&Utc)),
        blocks_ok: true,
        week_total: Some(reader::period(entries, &week_key, &week_key, &to_day)),
        month_total: Some(reader::period(
            entries,
            &reader::month_key(now),
            &reader::local_day(month_start.with_timezone(&Utc)),
            &to_day,
        )),
        periods_ok: true,
    }
}

pub struct LocalClaudeProvider;

impl UsageProvider for LocalClaudeProvider {
    fn id(&self) -> &'static str {
        "claude_code"
    }

    fn display_name(&self) -> &'static str {
        "Claude Code"
    }

    fn fetch_daily(&self) -> Option<DailyUsage> {
        let now = Local::now();
        let since = reader::start_of_day(now).with_timezone(&Utc);
        let entries = lock_cache().claude_entries(since);
        reader::daily(&entries, &reader::today_key())
    }

    fn fetch_enrichment(&self) -> ProviderEnrichment {
        let now = Local::now();
        let since = reader::enrichment_scan_start(now).with_timezone(&Utc);
        let entries = lock_cache().claude_entries(since);
        build_enrichment(&entries, now)
    }
}

pub struct LocalGeminiProvider;

impl UsageProvider for LocalGeminiProvider {
    fn id(&self) -> &'static str {
        "gemini"
    }

    fn display_name(&self) -> &'static str {
        "Gemini"
    }

    fn fetch_daily(&self) -> Option<DailyUsage> {
        let now = Local::now();
        let since = reader::start_of_day(now).with_timezone(&Utc);
        let entries = lock_cache().gemini_entries(since);
        reader::daily(&entries, &reader::today_key())
    }

    fn fetch_enrichment(&self) -> ProviderEnrichment {
        let now = Local::now();
        let since = reader::enrichment_scan_start(now).with_timezone(&Utc);
        let entries = lock_cache().gemini_entries(since);
        build_enrichment(&entries, now)
    }
}

pub struct LocalGrokProvider;

impl UsageProvider for LocalGrokProvider {
    fn id(&self) -> &'static str {
        "grok"
    }

    fn display_name(&self) -> &'static str {
        "Grok"
    }

    fn fetch_daily(&self) -> Option<DailyUsage> {
        let now = Local::now();
        let since = reader::start_of_day(now).with_timezone(&Utc);
        let entries = lock_cache().grok_entries(since);
        reader::daily(&entries, &reader::today_key())
    }

    fn fetch_enrichment(&self) -> ProviderEnrichment {
        let now = Local::now();
        let since = reader::enrichment_scan_start(now).with_timezone(&Utc);
        let entries = lock_cache().grok_entries(since);
        build_enrichment(&entries, now)
    }
}

/// Codex usage is subscription-based, so the source reports no cost — zero it,
/// keeping the tokens (matches the original's $0 behavior).
fn zero_cost_daily(entries: &[Entry]) -> Option<DailyUsage> {
    let d = reader::daily(entries, &reader::today_key())?;
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

fn zero_cost_period(p: PeriodUsage) -> PeriodUsage {
    PeriodUsage::new(p.period, p.total_tokens, 0.0)
}

pub struct LocalCodexProvider;

impl UsageProvider for LocalCodexProvider {
    fn id(&self) -> &'static str {
        "codex"
    }

    fn display_name(&self) -> &'static str {
        "Codex"
    }

    fn fetch_daily(&self) -> Option<DailyUsage> {
        let now = Local::now();
        let since = reader::start_of_day(now).with_timezone(&Utc);
        let entries = lock_cache().codex_entries(since);
        zero_cost_daily(&entries)
    }

    fn fetch_enrichment(&self) -> ProviderEnrichment {
        let now = Local::now();
        let since = reader::enrichment_scan_start(now).with_timezone(&Utc);
        let entries = lock_cache().codex_entries(since);
        let month_start = reader::start_of_month(now);
        let week_start = reader::start_of_week(now);
        let week_key = reader::local_day(week_start.with_timezone(&Utc));
        let to_day = reader::local_day(now.with_timezone(&Utc));
        let week = reader::period(&entries, &week_key, &week_key, &to_day);
        let month = reader::period(
            &entries,
            &reader::month_key(now),
            &reader::local_day(month_start.with_timezone(&Utc)),
            &to_day,
        );
        ProviderEnrichment {
            active_block: reader::active_block(&entries, now.with_timezone(&Utc)),
            blocks_ok: true,
            week_total: Some(zero_cost_period(week)),
            month_total: Some(zero_cost_period(month)),
            periods_ok: true,
        }
    }
}
