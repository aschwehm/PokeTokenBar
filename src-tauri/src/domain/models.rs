//! Usage/limit models from the ccusage CLI and the OAuth/Codex app servers.
//!
//! Mirrors the original `Core/Models.swift`. All types here only need to be
//! *decoded* (the Swift types are `Decodable` only), and decode is deliberately
//! lenient about a single bad field: one corrupted field never wipes a report.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::de::{self, Deserializer};
use serde::Deserialize;
use serde_json::Value;

use crate::domain::decoding::{
    get_bool, get_f64, get_i64, get_opt_f64, get_opt_i64, get_opt_string, get_string, parse_iso8601,
};

// MARK: - ccusage daily

/// One day of ccusage output.
#[derive(Debug, Clone, PartialEq)]
pub struct DailyUsage {
    pub date: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_creation_tokens: i64,
    pub cache_read_tokens: i64,
    pub total_tokens: i64,
    pub total_cost: f64,
}

impl DailyUsage {
    pub fn new(
        date: String,
        input_tokens: i64,
        output_tokens: i64,
        cache_creation_tokens: i64,
        cache_read_tokens: i64,
        total_tokens: i64,
        total_cost: f64,
    ) -> Self {
        Self {
            date,
            input_tokens,
            output_tokens,
            cache_creation_tokens,
            cache_read_tokens,
            total_tokens,
            total_cost,
        }
    }
}

impl<'de> Deserialize<'de> for DailyUsage {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let v = Value::deserialize(deserializer)?;
        let m = v
            .as_object()
            .ok_or_else(|| de::Error::custom("not an object"))?;
        // ccusage ≤18 reports "date", ≥20 reports "period" for the day.
        let date = get_opt_string(m, "date")
            .or_else(|| get_opt_string(m, "period"))
            .unwrap_or_default();
        let input_tokens = get_i64(m, "inputTokens");
        let output_tokens = get_i64(m, "outputTokens");
        let cache_creation_tokens = get_i64(m, "cacheCreationTokens");
        let cache_read_tokens = get_opt_i64(m, "cacheReadTokens")
            .or_else(|| get_opt_i64(m, "cachedInputTokens"))
            .unwrap_or(0);
        // totalTokens missing → sum of the four token kinds.
        let total_tokens = get_opt_i64(m, "totalTokens").unwrap_or_else(|| {
            input_tokens + output_tokens + cache_creation_tokens + cache_read_tokens
        });
        let total_cost = get_opt_f64(m, "totalCost")
            .or_else(|| get_opt_f64(m, "costUSD"))
            .unwrap_or(0.0);
        Ok(Self {
            date,
            input_tokens,
            output_tokens,
            cache_creation_tokens,
            cache_read_tokens,
            total_tokens,
            total_cost,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DailyReport {
    pub daily: Vec<DailyUsage>,
}

impl<'de> Deserialize<'de> for DailyReport {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let v = Value::deserialize(deserializer)?;
        let m = v
            .as_object()
            .ok_or_else(|| de::Error::custom("not an object"))?;
        let daily = decode_usage_array(m, "daily")?;
        Ok(Self { daily })
    }
}

/// `decodeIfPresent([T].self, forKey:) ?? []` — absent/null → empty, wrong type → error.
fn decode_usage_array<T, E>(m: &serde_json::Map<String, Value>, key: &str) -> Result<Vec<T>, E>
where
    T: serde::de::DeserializeOwned,
    E: de::Error,
{
    match m.get(key) {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(arr)) => {
            let mut out = Vec::with_capacity(arr.len());
            for item in arr {
                out.push(T::deserialize(item).map_err(|e| E::custom(e.to_string()))?);
            }
            Ok(out)
        }
        Some(_) => Err(E::custom(format!("{key} is not an array"))),
    }
}

// MARK: - ccusage blocks

/// One active/closed block from ccusage.
#[derive(Debug, Clone, PartialEq)]
pub struct BlockUsage {
    pub id: String,
    pub start_time: String,
    pub end_time: String,
    pub is_active: bool,
    pub total_tokens: i64,
    pub cost_usd: f64,
    /// ccusage blocks `burnRate.tokensPerMinute` — used for limit-depletion
    /// forecasts and companion display state.
    pub tokens_per_minute: Option<f64>,
}

impl BlockUsage {
    pub fn end_date(&self) -> Option<DateTime<Utc>> {
        parse_iso8601(&self.end_time)
    }
}

impl<'de> Deserialize<'de> for BlockUsage {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let v = Value::deserialize(deserializer)?;
        let m = v
            .as_object()
            .ok_or_else(|| de::Error::custom("not an object"))?;
        let tokens_per_minute = match m.get("burnRate") {
            Some(Value::Object(burn)) => get_opt_f64(burn, "tokensPerMinute"),
            _ => None,
        };
        Ok(Self {
            id: get_string(m, "id"),
            start_time: get_string(m, "startTime"),
            end_time: get_string(m, "endTime"),
            is_active: get_bool(m, "isActive"),
            total_tokens: get_i64(m, "totalTokens"),
            cost_usd: get_f64(m, "costUSD"),
            tokens_per_minute,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct BlocksReport {
    pub blocks: Vec<BlockUsage>,
}

impl<'de> Deserialize<'de> for BlocksReport {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let v = Value::deserialize(deserializer)?;
        let m = v
            .as_object()
            .ok_or_else(|| de::Error::custom("not an object"))?;
        let blocks = decode_usage_array(m, "blocks")?;
        Ok(Self { blocks })
    }
}

// MARK: - ccusage weekly / monthly

/// One week or month row.
#[derive(Debug, Clone, PartialEq)]
pub struct PeriodUsage {
    /// Week start ("2026-05-31") or month ("2026-06").
    pub period: String,
    pub total_tokens: i64,
    pub total_cost: f64,
}

impl PeriodUsage {
    pub fn new(period: String, total_tokens: i64, total_cost: f64) -> Self {
        Self {
            period,
            total_tokens,
            total_cost,
        }
    }

    pub fn from_daily(period: String, daily: &[DailyUsage]) -> Self {
        Self {
            period,
            total_tokens: daily.iter().map(|d| d.total_tokens).sum(),
            total_cost: daily.iter().map(|d| d.total_cost).sum(),
        }
    }
}

impl<'de> Deserialize<'de> for PeriodUsage {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let v = Value::deserialize(deserializer)?;
        let m = v
            .as_object()
            .ok_or_else(|| de::Error::custom("not an object"))?;
        let period = get_opt_string(m, "week")
            .or_else(|| get_opt_string(m, "month"))
            .or_else(|| get_opt_string(m, "period"))
            .unwrap_or_default();
        let input_tokens = get_i64(m, "inputTokens");
        let output_tokens = get_i64(m, "outputTokens");
        let cache_creation_tokens = get_i64(m, "cacheCreationTokens");
        let cache_read_tokens = get_opt_i64(m, "cacheReadTokens")
            .or_else(|| get_opt_i64(m, "cachedInputTokens"))
            .unwrap_or(0);
        let total_tokens = get_opt_i64(m, "totalTokens").unwrap_or_else(|| {
            input_tokens + output_tokens + cache_creation_tokens + cache_read_tokens
        });
        let total_cost = get_opt_f64(m, "totalCost")
            .or_else(|| get_opt_f64(m, "costUSD"))
            .unwrap_or(0.0);
        Ok(Self {
            period,
            total_tokens,
            total_cost,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct WeeklyReport {
    pub weekly: Vec<PeriodUsage>,
}

impl<'de> Deserialize<'de> for WeeklyReport {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let v = Value::deserialize(deserializer)?;
        let m = v
            .as_object()
            .ok_or_else(|| de::Error::custom("not an object"))?;
        let weekly = decode_usage_array(m, "weekly")?;
        Ok(Self { weekly })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct MonthlyReport {
    pub monthly: Vec<PeriodUsage>,
}

impl<'de> Deserialize<'de> for MonthlyReport {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let v = Value::deserialize(deserializer)?;
        let m = v
            .as_object()
            .ok_or_else(|| de::Error::custom("not an object"))?;
        let monthly = decode_usage_array(m, "monthly")?;
        Ok(Self { monthly })
    }
}

// MARK: - OAuth limits (api.anthropic.com/api/oauth/usage)

#[derive(Debug, Clone, Default)]
pub struct LimitWindow {
    pub utilization: Option<f64>,
    pub resets_at: Option<String>,
}

impl LimitWindow {
    pub fn reset_date(&self) -> Option<DateTime<Utc>> {
        self.resets_at.as_deref().and_then(parse_iso8601)
    }
}

impl<'de> Deserialize<'de> for LimitWindow {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(rename_all = "snake_case")]
        struct Raw {
            utilization: Option<f64>,
            resets_at: Option<String>,
        }
        let raw = Raw::deserialize(deserializer)?;
        Ok(Self {
            utilization: raw.utilization,
            resets_at: raw.resets_at,
        })
    }
}

#[derive(Debug, Clone, Default)]
pub struct LimitStatus {
    pub five_hour: Option<LimitWindow>,
    pub seven_day: Option<LimitWindow>,
    pub seven_day_opus: Option<LimitWindow>,
    pub seven_day_sonnet: Option<LimitWindow>,
    pub limits: Option<Vec<OAuthLimitEntry>>,
    /// Injected from the OAuth credential (Keychain), not from the HTTP response.
    pub subscription_type: Option<String>,
    pub rate_limit_tier: Option<String>,
}

impl LimitStatus {
    /// Combined display string of subscriptionType + rateLimitTier multiplier.
    /// e.g. subscriptionType="max", rateLimitTier="default_claude_max_20x" → "Max 20x".
    /// The multiplier only appears when the tier actually carries one (not Max-only);
    /// without a multiplier only the grade name is shown ("Pro"/"Free"); nil when
    /// there is no subscription info.
    pub fn plan_display(&self) -> Option<String> {
        let subscription = self.subscription_type.as_deref()?;
        if subscription.is_empty() {
            return None;
        }
        let base = capitalize(subscription);
        if let Some(tier) = self.rate_limit_tier.as_deref() {
            if let Some(multiplier) = Self::tier_multiplier(tier) {
                return Some(format!("{base} {multiplier}"));
            }
        }
        Some(base)
    }

    /// Extracts the trailing multiplier token ("20x"/"5x") of a rateLimitTier —
    /// split on "_" and find a digit+x part. Tiers without one ("default_claude_pro")
    /// yield None so only the grade name is shown.
    fn tier_multiplier(tier: &str) -> Option<String> {
        for part in tier.split('_') {
            if let Some(digits) = part.strip_suffix('x') {
                if !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()) {
                    return Some(part.to_string());
                }
            }
        }
        None
    }

    /// Windows the legacy fields cannot express. session(=five_hour) and
    /// weekly_all(=seven_day) are already shown by the legacy rows, so only the
    /// rest (weekly_scoped, …) is added. When every legacy field is empty
    /// (newer responses), the whole `limits` list is the fallback.
    pub fn scoped_limit_entries(&self) -> Vec<&OAuthLimitEntry> {
        let Some(entries) = &self.limits else {
            return Vec::new();
        };
        if self.five_hour.is_none() && self.seven_day.is_none() {
            return entries.iter().collect();
        }
        entries
            .iter()
            .filter(|e| {
                e.kind.as_deref() != Some("session") && e.kind.as_deref() != Some("weekly_all")
            })
            .collect()
    }
}

impl<'de> Deserialize<'de> for LimitStatus {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(rename_all = "snake_case")]
        struct Raw {
            five_hour: Option<LimitWindow>,
            seven_day: Option<LimitWindow>,
            seven_day_opus: Option<LimitWindow>,
            seven_day_sonnet: Option<LimitWindow>,
            limits: Option<Vec<OAuthLimitEntry>>,
            #[serde(default)]
            subscription_type: Option<String>,
            #[serde(default)]
            rate_limit_tier: Option<String>,
        }
        let raw = Raw::deserialize(deserializer)?;
        Ok(Self {
            five_hour: raw.five_hour,
            seven_day: raw.seven_day,
            seven_day_opus: raw.seven_day_opus,
            seven_day_sonnet: raw.seven_day_sonnet,
            limits: raw.limits,
            subscription_type: raw.subscription_type,
            rate_limit_tier: raw.rate_limit_tier,
        })
    }
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// A new-style `limits[]` entry from oauth/usage — the generalized form of the
/// legacy five_hour/seven_day windows. Old seven_day_opus/seven_day_sonnet are
/// now null, and per-model weekly limits arrive here as kind=weekly_scoped +
/// scope.model.displayName.
#[derive(Debug, Clone, Default)]
pub struct OAuthLimitEntry {
    pub kind: Option<String>,
    pub group: Option<String>,
    pub percent: Option<f64>,
    pub severity: Option<String>,
    pub resets_at: Option<String>,
    pub scope: Option<OAuthLimitScope>,
    pub is_active: Option<bool>,
}

impl OAuthLimitEntry {
    pub fn reset_date(&self) -> Option<DateTime<Utc>> {
        self.resets_at.as_deref().and_then(parse_iso8601)
    }
}

#[derive(Debug, Clone, Default)]
pub struct OAuthLimitScope {
    pub model: Option<OAuthLimitModel>,
}

#[derive(Debug, Clone, Default)]
pub struct OAuthLimitModel {
    pub display_name: Option<String>,
}

impl<'de> Deserialize<'de> for OAuthLimitEntry {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(rename_all = "snake_case")]
        struct RawScope {
            model: Option<RawModel>,
        }
        #[derive(Deserialize)]
        struct RawModel {
            #[serde(default)]
            display_name: Option<String>,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "snake_case")]
        struct Raw {
            kind: Option<String>,
            group: Option<String>,
            percent: Option<f64>,
            severity: Option<String>,
            resets_at: Option<String>,
            scope: Option<RawScope>,
            is_active: Option<bool>,
        }
        let raw = Raw::deserialize(deserializer)?;
        Ok(Self {
            kind: raw.kind,
            group: raw.group,
            percent: raw.percent,
            severity: raw.severity,
            resets_at: raw.resets_at,
            scope: raw.scope.map(|s| OAuthLimitScope {
                model: s.model.map(|m| OAuthLimitModel {
                    display_name: m.display_name,
                }),
            }),
            is_active: raw.is_active,
        })
    }
}

// MARK: - Codex app-server rate limits

#[derive(Debug, Clone, Default)]
pub struct CodexRateLimitWindow {
    pub used_percent: i32,
    pub window_duration_mins: Option<i32>,
    pub resets_at: Option<i64>,
}

impl CodexRateLimitWindow {
    pub fn reset_date(&self) -> Option<DateTime<Utc>> {
        self.resets_at.and_then(|s| DateTime::from_timestamp(s, 0))
    }

    pub fn display_name(&self) -> String {
        match self.window_duration_mins {
            Some(300) => "5시간 세션".to_string(),
            Some(10_080) => "주간".to_string(),
            Some(mins) if mins >= 60 && mins % 60 == 0 => format!("{}시간", mins / 60),
            Some(mins) => format!("{}분", mins),
            None => "한도".to_string(),
        }
    }
}

impl<'de> Deserialize<'de> for CodexRateLimitWindow {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Raw {
            used_percent: i32,
            #[serde(default)]
            window_duration_mins: Option<i32>,
            #[serde(default)]
            resets_at: Option<i64>,
        }
        let raw = Raw::deserialize(deserializer)?;
        Ok(Self {
            used_percent: raw.used_percent,
            window_duration_mins: raw.window_duration_mins,
            resets_at: raw.resets_at,
        })
    }
}

#[derive(Debug, Clone, Default)]
pub struct CodexCreditsSnapshot {
    pub balance: Option<String>,
    pub has_credits: bool,
    pub unlimited: bool,
}

impl<'de> Deserialize<'de> for CodexCreditsSnapshot {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Raw {
            #[serde(default)]
            balance: Option<String>,
            #[serde(default)]
            has_credits: bool,
            #[serde(default)]
            unlimited: bool,
        }
        let raw = Raw::deserialize(deserializer)?;
        Ok(Self {
            balance: raw.balance,
            has_credits: raw.has_credits,
            unlimited: raw.unlimited,
        })
    }
}

#[derive(Debug, Clone, Default)]
pub struct CodexSpendControlLimit {
    pub limit: String,
    pub remaining_percent: i32,
    pub resets_at: i64,
    pub used: String,
}

impl CodexSpendControlLimit {
    pub fn used_percent(&self) -> i32 {
        (100 - self.remaining_percent).clamp(0, 100)
    }

    pub fn reset_date(&self) -> DateTime<Utc> {
        DateTime::from_timestamp(self.resets_at, 0).unwrap_or(DateTime::<Utc>::UNIX_EPOCH)
    }
}

impl<'de> Deserialize<'de> for CodexSpendControlLimit {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Raw {
            limit: String,
            remaining_percent: i32,
            resets_at: i64,
            used: String,
        }
        let raw = Raw::deserialize(deserializer)?;
        Ok(Self {
            limit: raw.limit,
            remaining_percent: raw.remaining_percent,
            resets_at: raw.resets_at,
            used: raw.used,
        })
    }
}

#[derive(Debug, Clone, Default)]
pub struct CodexRateLimitSnapshot {
    pub limit_id: Option<String>,
    pub limit_name: Option<String>,
    pub primary: Option<CodexRateLimitWindow>,
    pub secondary: Option<CodexRateLimitWindow>,
    pub credits: Option<CodexCreditsSnapshot>,
    pub individual_limit: Option<CodexSpendControlLimit>,
    pub plan_type: Option<String>,
    pub rate_limit_reached_type: Option<String>,
}

impl CodexRateLimitSnapshot {
    pub fn has_visible_limit(&self) -> bool {
        self.primary.is_some() || self.secondary.is_some() || self.individual_limit.is_some()
    }

    /// Bucket display name based on limitName/limitId ("codex" → "Codex",
    /// "codex_other" → "Codex other").
    pub fn bucket_display_name(&self) -> String {
        let raw = self
            .limit_name
            .clone()
            .or_else(|| self.limit_id.clone())
            .unwrap_or_else(|| "codex".to_string());
        let spaced = raw.replace('_', " ");
        capitalize(&spaced)
    }
}

impl<'de> Deserialize<'de> for CodexRateLimitSnapshot {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Raw {
            #[serde(default)]
            limit_id: Option<String>,
            #[serde(default)]
            limit_name: Option<String>,
            #[serde(default)]
            primary: Option<CodexRateLimitWindow>,
            #[serde(default)]
            secondary: Option<CodexRateLimitWindow>,
            #[serde(default)]
            credits: Option<CodexCreditsSnapshot>,
            #[serde(default)]
            individual_limit: Option<CodexSpendControlLimit>,
            #[serde(default)]
            plan_type: Option<String>,
            #[serde(default)]
            rate_limit_reached_type: Option<String>,
        }
        let raw = Raw::deserialize(deserializer)?;
        Ok(Self {
            limit_id: raw.limit_id,
            limit_name: raw.limit_name,
            primary: raw.primary,
            secondary: raw.secondary,
            credits: raw.credits,
            individual_limit: raw.individual_limit,
            plan_type: raw.plan_type,
            rate_limit_reached_type: raw.rate_limit_reached_type,
        })
    }
}

#[derive(Debug, Clone, Default)]
pub struct CodexRateLimitStatus {
    pub rate_limits: CodexRateLimitSnapshot,
    pub rate_limits_by_limit_id: Option<HashMap<String, CodexRateLimitSnapshot>>,
}

impl CodexRateLimitStatus {
    /// The full bucket list, mirroring the codex TUI
    /// `app_server_rate_limit_snapshots`. The server top-level (rateLimits) is
    /// the "codex" bucket first, so the remaining buckets only live in
    /// rateLimitsByLimitId. Combines top-level + the byLimitId rest, deduped by
    /// limitId.
    pub fn snapshots(&self) -> Vec<CodexRateLimitSnapshot> {
        let mut result = vec![self.rate_limits.clone()];
        let Some(by_limit_id) = &self.rate_limits_by_limit_id else {
            return result;
        };
        // The server puts limitId-less snapshots under the "codex" key — the
        // same key/ID as the top-level is a duplicate. Sorting removes the
        // nondeterministic dict iteration order.
        let primary_key = self
            .rate_limits
            .limit_id
            .clone()
            .unwrap_or_else(|| "codex".to_string());
        let mut keys: Vec<&String> = by_limit_id.keys().collect();
        keys.sort();
        for limit_id in keys {
            if limit_id == &primary_key {
                continue;
            }
            let snapshot = &by_limit_id[limit_id];
            if let Some(id) = &snapshot.limit_id {
                if Some(id) == self.rate_limits.limit_id.as_ref() {
                    continue;
                }
            }
            result.push(snapshot.clone());
        }
        result
    }

    pub fn visible_snapshots(&self) -> Vec<CodexRateLimitSnapshot> {
        self.snapshots()
            .into_iter()
            .filter(|s| s.has_visible_limit())
            .collect()
    }

    pub fn has_visible_limit(&self) -> bool {
        !self.visible_snapshots().is_empty()
    }

    /// Menu-bar indicator / warning threshold — the max 5h (primary) utilization
    /// across all buckets.
    pub fn max_primary_used_percent(&self) -> Option<i32> {
        self.visible_snapshots()
            .iter()
            .filter_map(|s| s.primary.as_ref().map(|p| p.used_percent))
            .max()
    }
}

impl<'de> Deserialize<'de> for CodexRateLimitStatus {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Raw {
            rate_limits: CodexRateLimitSnapshot,
            #[serde(default)]
            rate_limits_by_limit_id: Option<HashMap<String, CodexRateLimitSnapshot>>,
        }
        let raw = Raw::deserialize(deserializer)?;
        Ok(Self {
            rate_limits: raw.rate_limits,
            rate_limits_by_limit_id: raw.rate_limits_by_limit_id,
        })
    }
}

// MARK: - Provider snapshot

/// A per-provider display snapshot. Mirrors `UsageProvider.reportsCost`.
#[derive(Debug, Clone)]
pub struct ProviderSnapshot {
    pub provider_id: String,
    pub display_name: String,
    pub today: Option<DailyUsage>,
    pub active_block: Option<BlockUsage>,
    pub week_total: Option<PeriodUsage>,
    pub month_total: Option<PeriodUsage>,
    pub fetched_at: DateTime<Utc>,
    pub reports_cost: bool,
}

impl ProviderSnapshot {
    pub fn new(
        provider_id: String,
        display_name: String,
        today: Option<DailyUsage>,
        active_block: Option<BlockUsage>,
        week_total: Option<PeriodUsage>,
        month_total: Option<PeriodUsage>,
        fetched_at: DateTime<Utc>,
    ) -> Self {
        Self {
            provider_id,
            display_name,
            today,
            active_block,
            week_total,
            month_total,
            fetched_at,
            reports_cost: true,
        }
    }

    pub fn id(&self) -> &str {
        &self.provider_id
    }

    pub fn today_total_tokens(&self) -> i64 {
        self.today.as_ref().map_or(0, |d| d.total_tokens)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode<T: serde::de::DeserializeOwned>(json: &str) -> T {
        serde_json::from_str(json).expect("decode failed")
    }

    // MARK: DailyReport

    #[test]
    fn daily_report() {
        let json = r#"{"daily":[{"date":"2026-06-10","inputTokens":907436,"outputTokens":1334526,
        "cacheCreationTokens":18905002,"cacheReadTokens":169465976,
        "totalTokens":190612940,"totalCost":311.30462895,
        "modelsUsed":["claude-fable-5"],"modelBreakdowns":[]}],"totals":null}"#;
        let report: DailyReport = decode(json);
        assert_eq!(report.daily.len(), 1);
        assert_eq!(report.daily[0].total_tokens, 190_612_940);
        assert_eq!(report.daily[0].date, "2026-06-10");
    }

    #[test]
    fn daily_report_v20_period() {
        // ccusage ≥20 reports the day via "period" instead of "date".
        let json = r#"{"daily":[{"agent":"all","period":"2026-06-15","metadata":{"agents":["claude"]},
        "inputTokens":372486,"outputTokens":167660,"cacheCreationTokens":2050363,
        "cacheReadTokens":18225182,"totalTokens":20815691,"totalCost":27.98}],"totals":null}"#;
        let report: DailyReport = decode(json);
        assert_eq!(report.daily[0].date, "2026-06-15");
        assert_eq!(report.daily[0].total_tokens, 20_815_691);
    }

    #[test]
    fn codex_daily_report_maps_cached_input_tokens() {
        let json = r#"{"daily":[{"date":"2026-06-17","inputTokens":10,"outputTokens":20,
        "cachedInputTokens":30,"totalTokens":60,"costUSD":0.25}]}"#;
        let report: DailyReport = decode(json);
        assert_eq!(report.daily[0].cache_read_tokens, 30);
        assert_eq!(report.daily[0].total_cost, 0.25);
    }

    #[test]
    fn daily_report_empty() {
        let report: DailyReport = decode(r#"{"daily":[],"totals":null}"#);
        assert!(report.daily.is_empty());
    }

    #[test]
    fn total_tokens_fallback() {
        // totalTokens missing → sum of the four token kinds.
        let json = r#"{"daily":[{"date":"2026-06-10","inputTokens":10,"outputTokens":20,
        "cacheCreationTokens":30,"cacheReadTokens":40,"costUSD":1.5}]}"#;
        let report: DailyReport = decode(json);
        assert_eq!(report.daily[0].total_tokens, 100);
        assert_eq!(report.daily[0].total_cost, 1.5);
    }

    // MARK: BlocksReport

    #[test]
    fn blocks_report() {
        let json = r#"{"blocks":[{"id":"2026-06-10T06:00:00.000Z","startTime":"2026-06-10T06:00:00.000Z",
        "endTime":"2026-06-10T11:00:00.000Z","isActive":true,"isGap":false,"entries":399,
        "totalTokens":26910731,"costUSD":51.8358331}]}"#;
        let report: BlocksReport = decode(json);
        assert_eq!(report.blocks.len(), 1);
        assert!(report.blocks[0].is_active);
        assert!(report.blocks[0].end_date().is_some());
    }

    #[test]
    fn block_burn_rate_decoding() {
        let json = r#"{"blocks":[{"id":"b","startTime":"2026-06-11T01:00:00.000Z","endTime":"2026-06-11T06:00:00.000Z",
        "isActive":true,"totalTokens":26910731,"costUSD":51.8,
        "burnRate":{"tokensPerMinute":457194.05,"costPerHour":1.18}}]}"#;
        let report: BlocksReport = decode(json);
        let tpm = report.blocks[0].tokens_per_minute.unwrap_or(0.0);
        assert!((tpm - 457_194.05).abs() < 0.01);
    }

    // MARK: Weekly/Monthly

    #[test]
    fn weekly_monthly_report() {
        let weekly = r#"{"weekly":[{"week":"2026-05-31","inputTokens":79280,"outputTokens":634270,
        "cacheCreationTokens":4355252,"cacheReadTokens":141260644,
        "totalTokens":146329446,"totalCost":107.0425086}]}"#;
        let w: WeeklyReport = decode(weekly);
        assert_eq!(
            w.weekly.last().map(|p| p.period.as_str()),
            Some("2026-05-31")
        );
        assert_eq!(w.weekly.last().map(|p| p.total_tokens), Some(146_329_446));

        let monthly =
            r#"{"monthly":[{"month":"2026-06","totalTokens":671185849,"totalCost":589.255}]}"#;
        let m: MonthlyReport = decode(monthly);
        assert_eq!(m.monthly.last().map(|p| p.period.as_str()), Some("2026-06"));
        assert_eq!(m.monthly.last().map(|p| p.total_tokens), Some(671_185_849));
    }

    #[test]
    fn codex_monthly_report_cost_usd() {
        let json = r#"{"monthly":[{"month":"2026-06","inputTokens":10,"outputTokens":20,
        "cachedInputTokens":30,"totalTokens":60,"costUSD":0.25}]}"#;
        let report: MonthlyReport = decode(json);
        assert_eq!(report.monthly[0].total_tokens, 60);
        assert_eq!(report.monthly[0].total_cost, 0.25);
    }

    #[test]
    fn period_usage_sums_daily_rows() {
        let daily = [
            DailyUsage::new("2026-06-16".to_string(), 1, 2, 3, 4, 10, 0.1),
            DailyUsage::new("2026-06-17".to_string(), 5, 6, 7, 8, 26, 0.2),
        ];
        let period = PeriodUsage::from_daily("2026-06-14".to_string(), &daily);
        assert_eq!(period.period, "2026-06-14");
        assert_eq!(period.total_tokens, 36);
        assert!((period.total_cost - 0.3).abs() < 0.001);
    }

    // MARK: LimitStatus

    #[test]
    fn limit_status() {
        let json = r#"{"five_hour":{"utilization":23.0,"resets_at":"2026-06-10T11:10:00.034464+00:00"},
        "seven_day":{"utilization":16.0,"resets_at":"2026-06-14T03:00:01.034496+00:00"},
        "seven_day_opus":null,
        "seven_day_sonnet":{"utilization":0.0,"resets_at":"2026-06-14T03:00:01.034508+00:00"},
        "seven_day_omelette":{"utilization":0.0,"resets_at":null},
        "extra_usage":{"is_enabled":false}}"#;
        let status: LimitStatus = decode(json);
        assert_eq!(
            status.five_hour.as_ref().and_then(|w| w.utilization),
            Some(23.0)
        );
        assert!(status
            .five_hour
            .as_ref()
            .and_then(LimitWindow::reset_date)
            .is_some());
        assert!(status.seven_day_opus.is_none());
        assert_eq!(
            status.seven_day.as_ref().and_then(|w| w.utilization),
            Some(16.0)
        );
    }

    #[test]
    fn limit_status_scoped_entries() {
        let json = r#"{"five_hour":{"utilization":32.0,"resets_at":"2026-07-10T04:10:00.497904+00:00"},
        "seven_day":{"utilization":7.0,"resets_at":"2026-07-12T03:00:00.497928+00:00"},
        "seven_day_opus":null,"seven_day_sonnet":null,
        "limits":[
        {"kind":"session","group":"session","percent":32,"severity":"normal","resets_at":"2026-07-10T04:10:00.497904+00:00","scope":null,"is_active":true},
        {"kind":"weekly_all","group":"weekly","percent":7,"severity":"normal","resets_at":"2026-07-12T03:00:00.497928+00:00","scope":null,"is_active":false},
        {"kind":"weekly_scoped","group":"weekly","percent":41,"severity":"normal","resets_at":"2026-07-12T03:00:00.498239+00:00","scope":{"model":{"id":null,"display_name":"Fable"},"surface":null},"is_active":false}
        ]}"#;
        let status: LimitStatus = decode(json);
        assert_eq!(status.limits.as_ref().map(Vec::len), Some(3));
        let scoped = status.scoped_limit_entries();
        assert_eq!(scoped.len(), 1);
        assert_eq!(scoped[0].kind.as_deref(), Some("weekly_scoped"));
        assert_eq!(scoped[0].percent, Some(41.0));
        assert_eq!(
            scoped[0]
                .scope
                .as_ref()
                .and_then(|s| s.model.as_ref())
                .and_then(|m| m.display_name.as_deref()),
            Some("Fable")
        );
        assert!(scoped[0].reset_date().is_some());
    }

    #[test]
    fn limit_status_legacy_empty_falls_back_to_all_entries() {
        let json = r#"{"five_hour":null,"seven_day":null,
        "limits":[
        {"kind":"session","group":"session","percent":10,"severity":"normal","resets_at":"2026-07-10T04:10:00+00:00","scope":null,"is_active":true},
        {"kind":"weekly_all","group":"weekly","percent":5,"severity":"normal","resets_at":"2026-07-12T03:00:00+00:00","scope":null,"is_active":false}
        ]}"#;
        let status: LimitStatus = decode(json);
        assert_eq!(status.scoped_limit_entries().len(), 2);
    }

    #[test]
    fn plan_display() {
        let mut status: LimitStatus = decode("{}");
        status.subscription_type = Some("max".to_string());
        status.rate_limit_tier = Some("default_claude_max_20x".to_string());
        assert_eq!(status.plan_display().as_deref(), Some("Max 20x"));

        status.rate_limit_tier = Some("default_claude_max_5x".to_string());
        assert_eq!(status.plan_display().as_deref(), Some("Max 5x"));

        // No multiplier token → grade name only.
        status.subscription_type = Some("pro".to_string());
        status.rate_limit_tier = Some("default_claude_pro".to_string());
        assert_eq!(status.plan_display().as_deref(), Some("Pro"));

        // nil tier → grade name still shown.
        status.subscription_type = Some("free".to_string());
        status.rate_limit_tier = None;
        assert_eq!(status.plan_display().as_deref(), Some("Free"));

        // No subscription info → no plan row.
        status.subscription_type = None;
        assert!(status.plan_display().is_none());

        // Empty string also hides the row.
        status.subscription_type = Some(String::new());
        assert!(status.plan_display().is_none());
    }

    // MARK: Codex rate limits

    #[test]
    fn codex_rate_limit_status() {
        let json = r#"{"rateLimits":{"limitId":"codex","limitName":null,
        "primary":{"usedPercent":86,"windowDurationMins":300,"resetsAt":1781694161},
        "secondary":{"usedPercent":58,"windowDurationMins":10080,"resetsAt":1781855658},
        "credits":{"hasCredits":false,"unlimited":false,"balance":null},
        "individualLimit":null,"planType":"team","rateLimitReachedType":null},
        "rateLimitsByLimitId":{"codex":{"limitId":"codex","limitName":null,
        "primary":{"usedPercent":86,"windowDurationMins":300,"resetsAt":1781694161},
        "secondary":{"usedPercent":58,"windowDurationMins":10080,"resetsAt":1781855658},
        "credits":{"hasCredits":false,"unlimited":false,"balance":null},
        "individualLimit":null,"planType":"team","rateLimitReachedType":null}}}"#;
        let status: CodexRateLimitStatus = decode(json);

        // Single bucket — top-level and byLimitId["codex"] are the same → deduped to 1.
        assert_eq!(status.snapshots().len(), 1);
        let visible = status.visible_snapshots();
        let codex = visible.first().unwrap();
        assert_eq!(codex.primary.as_ref().map(|w| w.used_percent), Some(86));
        assert_eq!(
            codex.primary.as_ref().map(|w| w.display_name()).as_deref(),
            Some("5시간 세션")
        );
        assert_eq!(
            codex
                .secondary
                .as_ref()
                .map(|w| w.display_name())
                .as_deref(),
            Some("주간")
        );
        assert_eq!(codex.plan_type.as_deref(), Some("team"));
        assert!(status.has_visible_limit());
        assert!(codex
            .primary
            .as_ref()
            .and_then(CodexRateLimitWindow::reset_date)
            .is_some());
        assert_eq!(status.max_primary_used_percent(), Some(86));
    }

    #[test]
    fn codex_rate_limit_status_multi_bucket() {
        let json = r#"{"rateLimits":{"limitId":"codex","limitName":null,
        "primary":{"usedPercent":0,"windowDurationMins":300,"resetsAt":1781694161},
        "secondary":{"usedPercent":0,"windowDurationMins":10080,"resetsAt":1781855658},
        "credits":null,"individualLimit":null,"planType":"plus","rateLimitReachedType":null},
        "rateLimitsByLimitId":{
        "codex":{"limitId":"codex","limitName":null,
        "primary":{"usedPercent":0,"windowDurationMins":300,"resetsAt":1781694161},
        "secondary":{"usedPercent":0,"windowDurationMins":10080,"resetsAt":1781855658},
        "credits":null,"individualLimit":null,"planType":"plus","rateLimitReachedType":null},
        "codex_other":{"limitId":"codex_other","limitName":"codex_other",
        "primary":{"usedPercent":41,"windowDurationMins":300,"resetsAt":1781694161},
        "secondary":{"usedPercent":93,"windowDurationMins":10080,"resetsAt":1781855658},
        "credits":null,"individualLimit":null,"planType":"plus","rateLimitReachedType":null}}}"#;
        let status: CodexRateLimitStatus = decode(json);

        let snapshots = status.snapshots();
        assert_eq!(snapshots.len(), 2);
        assert_eq!(snapshots[0].limit_id.as_deref(), Some("codex"));
        assert_eq!(snapshots[1].limit_id.as_deref(), Some("codex_other"));
        assert_eq!(
            snapshots[1].secondary.as_ref().map(|w| w.used_percent),
            Some(93)
        );
        assert_eq!(status.max_primary_used_percent(), Some(41));
        assert_eq!(snapshots[1].bucket_display_name(), "Codex other");
        assert_eq!(snapshots[0].bucket_display_name(), "Codex");
    }

    #[test]
    fn codex_rate_limit_status_legacy_nil_limit_id_dedup() {
        let json = r#"{"rateLimits":{"limitId":null,"limitName":null,
        "primary":{"usedPercent":30,"windowDurationMins":300,"resetsAt":1},
        "secondary":null,"credits":null,"individualLimit":null,"planType":null,"rateLimitReachedType":null},
        "rateLimitsByLimitId":{"codex":{"limitId":null,"limitName":null,
        "primary":{"usedPercent":30,"windowDurationMins":300,"resetsAt":1},
        "secondary":null,"credits":null,"individualLimit":null,"planType":null,"rateLimitReachedType":null}}}"#;
        let status: CodexRateLimitStatus = decode(json);
        assert_eq!(status.snapshots().len(), 1);
        assert_eq!(status.max_primary_used_percent(), Some(30));
    }

    // MARK: Derived values

    #[test]
    fn window_display_name() {
        fn name(mins: Option<i32>) -> String {
            CodexRateLimitWindow {
                used_percent: 0,
                window_duration_mins: mins,
                resets_at: None,
            }
            .display_name()
        }
        assert_eq!(name(Some(300)), "5시간 세션");
        assert_eq!(name(Some(10_080)), "주간");
        assert_eq!(name(Some(120)), "2시간"); // whole hours
        assert_eq!(name(Some(90)), "90분"); // not a whole hour
        assert_eq!(name(None), "한도");
    }

    #[test]
    fn spend_control_used_percent_clamped() {
        fn used(remaining: i32) -> i32 {
            CodexSpendControlLimit {
                limit: "$10".to_string(),
                remaining_percent: remaining,
                resets_at: 0,
                used: "$3".to_string(),
            }
            .used_percent()
        }
        assert_eq!(used(30), 70);
        assert_eq!(used(-10), 100); // negative remaining → clamp to 100
        assert_eq!(used(150), 0); // >100 → clamp to 0
    }

    #[test]
    fn has_visible_limit_reflects_windows() {
        let none: CodexRateLimitSnapshot = CodexRateLimitSnapshot {
            primary: None,
            secondary: None,
            individual_limit: None,
            ..Default::default()
        };
        assert!(!none.has_visible_limit());
        let some = CodexRateLimitSnapshot {
            primary: Some(CodexRateLimitWindow {
                used_percent: 10,
                window_duration_mins: Some(300),
                resets_at: None,
            }),
            ..Default::default()
        };
        assert!(some.has_visible_limit());
    }
}
