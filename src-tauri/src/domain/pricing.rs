//! Per-model token pricing (USD/token).
//!
//! Mirrors the original `Core/ModelPricing.swift`. Rates are derived by linear
//! regression against real `ccusage --breakdown` output (fit error 0.000%).

use std::collections::HashMap;
use std::sync::LazyLock;

/// USD per **million** tokens is how the table is declared; this stores the
/// per-token value.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ModelRate {
    pub input: f64, // USD per token
    pub output: f64,
    pub cache_write: f64, // cache creation
    pub cache_read: f64,
}

impl ModelRate {
    pub fn zero() -> Self {
        Self {
            input: 0.0,
            output: 0.0,
            cache_write: 0.0,
            cache_read: 0.0,
        }
    }

    /// Declared in USD per million tokens → converted to per-token.
    pub fn per_million(input: f64, output: f64, cache_write: f64, cache_read: f64) -> Self {
        Self {
            input: input / 1_000_000.0,
            output: output / 1_000_000.0,
            cache_write: cache_write / 1_000_000.0,
            cache_read: cache_read / 1_000_000.0,
        }
    }
}

/// Exact-match pricing table (USD/Mtok), matching ccusage (reverse-derived, 0% error).
static TABLE: LazyLock<HashMap<&'static str, ModelRate>> = LazyLock::new(|| {
    let mut m = HashMap::new();
    m.insert(
        "claude-opus-4-8",
        ModelRate::per_million(5.0, 25.0, 6.25, 0.5),
    );
    m.insert(
        "claude-opus-4-7",
        ModelRate::per_million(5.0, 25.0, 6.25, 0.5),
    );
    m.insert(
        "claude-sonnet-4-6",
        ModelRate::per_million(3.0, 15.0, 3.75, 0.3),
    );
    m.insert(
        "claude-haiku-4-5-20251001",
        ModelRate::per_million(1.0, 5.0, 1.25, 0.1),
    );
    m.insert("claude-fable-5", ModelRate::zero()); // unlisted in ccusage → $0
    m.insert("gpt-5.5", ModelRate::per_million(5.0, 30.0, 0.0, 0.5));
    // Gemini official API rates (base tier, ≤200K prompt). Cache is read-only
    // (storage time charges excluded).
    m.insert(
        "gemini-2.5-pro",
        ModelRate::per_million(1.25, 10.0, 0.0, 0.3125),
    );
    m.insert(
        "gemini-2.5-flash",
        ModelRate::per_million(0.30, 2.5, 0.0, 0.075),
    );
    m.insert(
        "gemini-2.0-flash",
        ModelRate::per_million(0.10, 0.4, 0.0, 0.025),
    );
    m
});

pub struct ModelPricing;

impl ModelPricing {
    /// Model name → rate. Exact match first, then family fallback
    /// (opus/sonnet/haiku/gpt) for version drift, then 0 (ccusage treats
    /// unpriced models as 0).
    pub fn rate(model: &str) -> ModelRate {
        if let Some(r) = TABLE.get(model) {
            return *r;
        }
        let m = model.to_ascii_lowercase();
        // Grok only reports server-side cost (costUsdTicks) — no price table, so
        // cut off before the family fallback so `grok-codex-*` / `grok-4o-*`
        // names don't inherit GPT pricing and show invented amounts.
        if m.starts_with("grok") {
            return ModelRate::zero();
        }
        // Antigravity is a subscription — no per-token billing and the source
        // reports no amount. The `antigravity/` prefix also keeps these names
        // out of the exact table (this CLI really does call `claude-sonnet-4-6`).
        if m.starts_with("antigravity/") {
            return ModelRate::zero();
        }
        if m.contains("opus") {
            return ModelRate::per_million(5.0, 25.0, 6.25, 0.5);
        }
        if m.contains("sonnet") {
            return ModelRate::per_million(3.0, 15.0, 3.75, 0.3);
        }
        if m.contains("haiku") {
            return ModelRate::per_million(1.0, 5.0, 1.25, 0.1);
        }
        if m.contains("gpt") || m.contains("codex") || m.contains("o4") || m.contains("o3") {
            return ModelRate::per_million(5.0, 30.0, 0.0, 0.5);
        }
        // Gemini family fallback — pro/flash only; other gemini variants are 0
        // (prevents mis-display).
        if m.starts_with("gemini") {
            if m.contains("pro") {
                return ModelRate::per_million(1.25, 10.0, 0.0, 0.3125);
            }
            if m.contains("flash") {
                return ModelRate::per_million(0.30, 2.5, 0.0, 0.075);
            }
        }
        ModelRate::zero()
    }

    pub fn cost(model: &str, input: i64, output: i64, cache_write: i64, cache_read: i64) -> f64 {
        let r = Self::rate(model);
        input as f64 * r.input
            + output as f64 * r.output
            + cache_write as f64 * r.cache_write
            + cache_read as f64 * r.cache_read
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_approx(a: f64, b: f64, eps: f64) {
        assert!((a - b).abs() < eps, "{a} != {b}");
    }

    #[test]
    fn pricing_exact_and_fallback_and_zero() {
        assert_approx(
            ModelPricing::cost("claude-opus-4-8", 1_000_000, 0, 0, 0),
            5.0,
            1e-6,
        );
        assert_approx(
            ModelPricing::cost("claude-opus-4-8", 0, 1_000_000, 0, 0),
            25.0,
            1e-6,
        );
        assert_approx(
            ModelPricing::cost("claude-haiku-4-5-20251001", 1_000_000, 0, 0, 0),
            1.0,
            1e-6,
        );
        assert_approx(
            ModelPricing::cost("claude-fable-5", 1_000_000, 1_000_000, 1_000_000, 1_000_000),
            0.0,
            1e-9,
        );
        // Unknown model → family fallback.
        assert_approx(
            ModelPricing::cost("claude-opus-4-99", 1_000_000, 0, 0, 0),
            5.0,
            1e-6,
        );
        assert_approx(
            ModelPricing::cost("totally-unknown", 1_000_000, 0, 0, 0),
            0.0,
            1e-9,
        );
    }

    #[test]
    fn gemini_pricing() {
        assert_eq!(
            ModelPricing::rate("gemini-2.5-pro"),
            ModelRate::per_million(1.25, 10.0, 0.0, 0.3125)
        );
        assert_eq!(
            ModelPricing::rate("gemini-2.5-flash"),
            ModelRate::per_million(0.30, 2.5, 0.0, 0.075)
        );
        assert_eq!(
            ModelPricing::rate("gemini-3.1-pro-preview"),
            ModelRate::per_million(1.25, 10.0, 0.0, 0.3125)
        );
        assert_eq!(
            ModelPricing::rate("gemini-3-flash-lite"),
            ModelRate::per_million(0.30, 2.5, 0.0, 0.075)
        );
        assert_eq!(ModelPricing::rate("gemini-nano-banana"), ModelRate::zero());
        // Actual cost arithmetic (m2 case): 420 in + 80 out + 600 cacheR @2.5-pro.
        let c = ModelPricing::cost("gemini-2.5-pro", 420, 80, 0, 600);
        assert_approx(c, 420.0 * 1.25e-6 + 80.0 * 10e-6 + 600.0 * 0.3125e-6, 1e-12);
    }

    #[test]
    fn grok_names_never_inherit_other_family_pricing() {
        // `grok-codex-*` / `grok-4o-*` would match the `codex`/`o4` substrings
        // and invent GPT amounts — must be cut to 0 before the family fallback.
        for name in [
            "grok-build-1",
            "grok-4-fast",
            "grok-code-fast-1",
            "grok-codex-next",
            "grok-4o-mini",
        ] {
            assert_eq!(ModelPricing::rate(name), ModelRate::zero(), "{name}");
        }
        assert_eq!(
            ModelPricing::cost("grok-codex-next", 1_000_000, 1_000_000, 0, 0),
            0.0
        );
        // Other providers' fallbacks stay alive (no over-cutting).
        assert_eq!(
            ModelPricing::rate("gpt-5.6-codex"),
            ModelRate::per_million(5.0, 30.0, 0.0, 0.5)
        );
    }

    #[test]
    fn antigravity_usage_is_not_priced() {
        for model in [
            "gemini-3.6-flash",
            "gemini-3-flash-e",
            "gemini-default",
            "claude-sonnet-4-6",
        ] {
            let cost = ModelPricing::cost(
                &format!("antigravity/{model}"),
                1_000_000,
                1_000_000,
                1_000_000,
                1_000_000,
            );
            assert_approx(cost, 0.0, 0.0000001);
        }
        assert!(
            ModelPricing::cost("claude-sonnet-4-6", 1_000_000, 0, 0, 0) > 0.0,
            "the unprefixed name must keep its rate"
        );
    }
}
