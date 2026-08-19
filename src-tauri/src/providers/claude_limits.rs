//! Claude OAuth usage limits provider (`api.anthropic.com/api/oauth/usage`).
//!
//! Reads `~/.claude/.credentials.json` to get official 5-hour and 7-day rate limit usage.

use std::fs;
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::domain::decoding::parse_iso8601;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LimitWindow {
    pub utilization: Option<f64>,
    pub resets_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LimitStatus {
    pub five_hour: Option<LimitWindow>,
    pub seven_day: Option<LimitWindow>,
    pub subscription_type: Option<String>,
    pub rate_limit_tier: Option<String>,
    pub plan_display: Option<String>,
}

pub struct OAuthCredential {
    pub access_token: String,
    pub expires_at: Option<DateTime<Utc>>,
    pub subscription_type: Option<String>,
    pub rate_limit_tier: Option<String>,
}

impl OAuthCredential {
    pub fn is_expired(&self) -> bool {
        if let Some(exp) = self.expires_at {
            exp <= Utc::now() + chrono::Duration::seconds(60)
        } else {
            false
        }
    }
}

pub fn default_credentials_path() -> PathBuf {
    if let Ok(val) = std::env::var("CLAUDE_CONFIG_DIR") {
        if !val.trim().is_empty() {
            return PathBuf::from(val).join(".credentials.json");
        }
    }
    let home = crate::platform::binary_locator::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".claude/.credentials.json")
}

pub fn read_claude_credentials(path: Option<PathBuf>) -> Option<OAuthCredential> {
    let p = path.unwrap_or_else(default_credentials_path);
    let data = fs::read_to_string(p).ok()?;
    let val: Value = serde_json::from_str(&data).ok()?;
    parse_credential(&val)
}

pub fn parse_credential(val: &Value) -> Option<OAuthCredential> {
    let oauth = val.get("claudeAiOauth")?.as_object()?;
    let token = oauth.get("accessToken")?.as_str()?;
    if token.trim().is_empty() {
        return None;
    }

    let expires_at = oauth.get("expiresAt").and_then(|v| match v {
        Value::Number(n) => {
            let num = n.as_i64()?;
            if num > 10_000_000_000 {
                DateTime::from_timestamp_millis(num)
            } else {
                DateTime::from_timestamp(num, 0)
            }
        }
        Value::String(s) => parse_iso8601(s),
        _ => None,
    });

    let subscription_type = oauth
        .get("subscriptionType")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let rate_limit_tier = oauth
        .get("rateLimitTier")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    Some(OAuthCredential {
        access_token: token.to_string(),
        expires_at,
        subscription_type,
        rate_limit_tier,
    })
}

pub fn format_plan_display(sub_type: Option<&str>, tier: Option<&str>) -> Option<String> {
    let sub = sub_type?.trim();
    if sub.is_empty() {
        return None;
    }
    let mut chars = sub.chars();
    let first = chars.next()?;
    let capitalized = first.to_uppercase().collect::<String>() + chars.as_str();

    if let Some(t) = tier {
        for part in t.split('_') {
            if let Some(digits) = part.strip_suffix('x') {
                if !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()) {
                    return Some(format!("{} {}", capitalized, part));
                }
            }
        }
    }

    Some(capitalized)
}

static LIMITS_CACHE: std::sync::Mutex<Option<(std::time::Instant, LimitStatus)>> =
    std::sync::Mutex::new(None);

pub fn fetch_claude_limits(credential: &OAuthCredential) -> Result<LimitStatus, String> {
    if let Ok(guard) = LIMITS_CACHE.lock() {
        if let Some((cached_at, ref status)) = *guard {
            if cached_at.elapsed() < std::time::Duration::from_secs(60) {
                return Ok(status.clone());
            }
        }
    }

    let resp = ureq::get("https://api.anthropic.com/api/oauth/usage")
        .set(
            "Authorization",
            &format!("Bearer {}", credential.access_token),
        )
        .set("anthropic-beta", "oauth-2025-04-20")
        .timeout(std::time::Duration::from_secs(3))
        .call()
        .map_err(|e| format!("HTTP request error: {e}"))?;

    let val: Value = resp
        .into_json()
        .map_err(|e| format!("JSON decode error: {e}"))?;

    let five_hour = val.get("five_hour").map(|fh| LimitWindow {
        utilization: fh.get("utilization").and_then(|v| v.as_f64()),
        resets_at: fh
            .get("resets_at")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
    });

    let seven_day = val.get("seven_day").map(|sd| LimitWindow {
        utilization: sd.get("utilization").and_then(|v| v.as_f64()),
        resets_at: sd
            .get("resets_at")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
    });

    let plan_display = format_plan_display(
        credential.subscription_type.as_deref(),
        credential.rate_limit_tier.as_deref(),
    );

    let status = LimitStatus {
        five_hour,
        seven_day,
        subscription_type: credential.subscription_type.clone(),
        rate_limit_tier: credential.rate_limit_tier.clone(),
        plan_display,
    };

    if let Ok(mut guard) = LIMITS_CACHE.lock() {
        *guard = Some((std::time::Instant::now(), status.clone()));
    }

    Ok(status)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_credentials() {
        let json = serde_json::json!({
            "claudeAiOauth": {
                "accessToken": "sk-ant-test1234",
                "expiresAt": 1772618400000_i64,
                "subscriptionType": "max",
                "rateLimitTier": "default_claude_max_20x"
            }
        });

        let cred = parse_credential(&json).expect("credential");
        assert_eq!(cred.access_token, "sk-ant-test1234");
        assert_eq!(cred.subscription_type.as_deref(), Some("max"));
        assert_eq!(
            cred.rate_limit_tier.as_deref(),
            Some("default_claude_max_20x")
        );
        assert_eq!(
            format_plan_display(
                cred.subscription_type.as_deref(),
                cred.rate_limit_tier.as_deref()
            ),
            Some("Max 20x".to_string())
        );
    }

    #[test]
    fn test_plan_display_pro() {
        assert_eq!(
            format_plan_display(Some("pro"), Some("default_claude_pro")),
            Some("Pro".to_string())
        );
    }
}
