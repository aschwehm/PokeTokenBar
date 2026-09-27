//! OpenCode Zen Go official usage quota provider.
//!
//! Fetches the cloud quota behind the OpenCode Zen Go API
//! (`GET https://opencode.ai/zen/go/v1/usage`) — the same feed as the web
//! panel — and exposes the rolling (5-hour), weekly and monthly utilisation
//! buckets with their reset times.
//!
//! The API key is taken from the `OPENCODE_GO_API_KEY` environment variable,
//! falling back to `${HERMES_HOME}/.env` (the Hermes portable convention).
//! Only `/usage` exists on the Zen Go API — it returns utilisation as a
//! percent plus a reset time, not raw token counts, and there are no other
//! endpoints.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const USAGE_URL: &str = "https://opencode.ai/zen/go/v1/usage";

/// The Zen Go API 403s the default `ureq`/Python user agent — a browser UA is
/// required (verified on the live endpoint).
pub const BROWSER_USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36";

const KEY_ENV_NAME: &str = "OPENCODE_GO_API_KEY";
const CACHE_TTL: Duration = Duration::from_secs(60);

/// One usage bucket from the Zen Go API.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ZenGoBucket {
    pub status: Option<String>,
    /// Utilisation as a percentage (0.0–100.0), same scale the web panel shows.
    pub percent: Option<f64>,
    pub resets_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ZenGoUsage {
    pub rolling: Option<ZenGoBucket>,
    pub weekly: Option<ZenGoBucket>,
    pub monthly: Option<ZenGoBucket>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ZenGoUsageResponse {
    pub usage: Option<ZenGoUsage>,
}

/// Read the API key from `OPENCODE_GO_API_KEY` first, then from
/// `${HERMES_HOME}/.env` (Hermes portable layout).
pub fn resolve_api_key() -> Option<String> {
    if let Ok(val) = std::env::var(KEY_ENV_NAME) {
        let trimmed = val.trim().to_string();
        if !trimmed.is_empty() {
            return Some(trimmed);
        }
    }
    default_env_path().and_then(|p| parse_env_file_key(&p))
}

pub fn default_env_path() -> Option<PathBuf> {
    let home = std::env::var("HERMES_HOME").ok()?;
    let trimmed = home.trim().to_string();
    if trimmed.is_empty() {
        return None;
    }
    Some(PathBuf::from(trimmed).join(".env"))
}

/// Extract the API key from a `.env` file (line `OPENCODE_GO_API_KEY=…`).
/// Lines must start with the exact key name — a longer name that merely
/// *contains* it (e.g. `SOME_OPENCODE_GO_API_KEY_X=…`) must not match.
pub fn parse_env_file_key(path: &Path) -> Option<String> {
    let data = fs::read_to_string(path).ok()?;
    let prefix = format!("{KEY_ENV_NAME}=");
    for line in data.lines() {
        let line = line.trim();
        if line.starts_with(&prefix) {
            let val = line[prefix.len()..].trim().trim_matches('"').trim();
            if !val.is_empty() {
                return Some(val.to_string());
            }
        }
    }
    None
}

/// Parse the `/usage` response into buckets. `None` when the payload has no
/// `usage` object at all; a missing individual bucket is fine.
pub fn parse_zen_go_usage(val: &Value) -> Option<ZenGoUsage> {
    let usage = val.get("usage")?;
    let parse_bucket = |key: &str| -> Option<ZenGoBucket> {
        let b = usage.get(key)?;
        Some(ZenGoBucket {
            status: b
                .get("status")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            percent: b.get("percent").and_then(|v| v.as_f64()),
            resets_at: b
                .get("resetsAt")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
        })
    };
    Some(ZenGoUsage {
        rolling: parse_bucket("rolling"),
        weekly: parse_bucket("weekly"),
        monthly: parse_bucket("monthly"),
    })
}

static USAGE_CACHE: Mutex<Option<(Instant, ZenGoUsage)>> = Mutex::new(None);

/// Fetch the current quota. Cached for 60 s like the Claude limits fetch.
pub fn fetch_zen_go_usage(key: &str) -> Result<ZenGoUsage, String> {
    if let Ok(guard) = USAGE_CACHE.lock() {
        if let Some((cached_at, ref usage)) = *guard {
            if cached_at.elapsed() < CACHE_TTL {
                return Ok(usage.clone());
            }
        }
    }

    let resp = ureq::get(USAGE_URL)
        .set("Authorization", &format!("Bearer {key}"))
        .set("User-Agent", BROWSER_USER_AGENT)
        .timeout(Duration::from_secs(5))
        .call()
        .map_err(|e| format!("OpenCode Go usage request failed: {e}"))?;

    let val: Value = resp
        .into_json()
        .map_err(|e| format!("OpenCode Go usage JSON decode failed: {e}"))?;

    let usage = parse_zen_go_usage(&val)
        .ok_or_else(|| "OpenCode Go usage response missing `usage` object".to_string())?;

    if let Ok(mut guard) = USAGE_CACHE.lock() {
        *guard = Some((Instant::now(), usage.clone()));
    }

    Ok(usage)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_usage_response() {
        let json = serde_json::json!({
            "usage": {
                "rolling": { "status": "ok", "percent": 0, "resetsAt": "2026-08-28T12:00:00Z" },
                "weekly": { "status": "ok", "percent": 12, "resetsAt": "2026-08-31T00:00:00Z" },
                "monthly": { "status": "ok", "percent": 28, "resetsAt": "2026-09-01T00:00:00Z" }
            }
        });
        let parsed = parse_zen_go_usage(&json).expect("parse");
        let rolling = parsed.rolling.expect("rolling");
        assert_eq!(rolling.status.as_deref(), Some("ok"));
        assert_eq!(rolling.percent, Some(0.0));
        assert_eq!(rolling.resets_at.as_deref(), Some("2026-08-28T12:00:00Z"));
        assert_eq!(parsed.weekly.and_then(|w| w.percent), Some(12.0));
        let monthly = parsed.monthly.expect("monthly");
        assert_eq!(monthly.percent, Some(28.0));
        assert_eq!(
            monthly.resets_at.as_deref(),
            Some("2026-09-01T00:00:00Z")
        );
    }

    #[test]
    fn test_parse_usage_missing_bucket() {
        let json = serde_json::json!({ "usage": { "weekly": { "status": "ok", "percent": 5 } } });
        let parsed = parse_zen_go_usage(&json).expect("parse");
        assert!(parsed.rolling.is_none());
        assert_eq!(parsed.weekly.and_then(|w| w.percent), Some(5.0));
        assert!(parsed.monthly.is_none());
    }

    #[test]
    fn test_parse_rejects_non_usage() {
        assert!(parse_zen_go_usage(&serde_json::json!({ "error": "nope" })).is_none());
    }

    #[test]
    fn test_parse_env_file_key() {
        let dir = tempfile::tempdir().expect("tmp");
        let env_path = dir.path().join(".env");
        let prefix = format!("{KEY_ENV_NAME}=");
        std::fs::write(
            &env_path,
            format!("FOO=bar\r\n{prefix}sk_test_value_123\r\nBAZ=qux\r\n"),
        )
        .expect("write");
        assert_eq!(parse_env_file_key(&env_path).as_deref(), Some("sk_test_value_123"));
    }

    #[test]
    fn test_parse_env_file_ignores_containing_names() {
        let dir = tempfile::tempdir().expect("tmp");
        let env_path = dir.path().join(".env");
        let prefix = format!("{KEY_ENV_NAME}=");
        std::fs::write(
            &env_path,
            format!("SOMETHING_{KEY_ENV_NAME}X=not-it\n{prefix}real-key\n"),
        )
        .expect("write");
        assert_eq!(parse_env_file_key(&env_path).as_deref(), Some("real-key"));
    }

    #[test]
    fn test_parse_env_file_missing_key() {
        let dir = tempfile::tempdir().expect("tmp");
        let env_path = dir.path().join(".env");
        std::fs::write(&env_path, "FOO=bar\n").expect("write");
        assert!(parse_env_file_key(&env_path).is_none());
    }

    #[test]
    fn test_resolve_key_without_any_source() {
        // Env mutation is unsafe under parallel tests, so assert the
        // graceful-empty path: no env var and no HERMES_HOME -> None.
        if std::env::var(KEY_ENV_NAME).is_err() && std::env::var("HERMES_HOME").is_err() {
            assert!(resolve_api_key().is_none());
        }
    }

    #[test]
    fn test_live_fetch_zen_go_usage() {
        let Some(key) = resolve_api_key() else {
            eprintln!("skipped: no {KEY_ENV_NAME} env var and no HERMES_HOME/.env");
            return;
        };
        let usage = fetch_zen_go_usage(&key).expect("live fetch");
        assert!(
            usage.rolling.is_some() || usage.weekly.is_some() || usage.monthly.is_some(),
            "expected at least one usage bucket"
        );
    }
}