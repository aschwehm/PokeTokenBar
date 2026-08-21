//! Shared lenient-decoding helpers.
//!
//! The original Swift deliberately absorbs corrupt data so a single bad field
//! never wipes the whole Pokédex/state. These helpers reproduce that strategy:
//! missing key, JSON `null`, and type mismatch all collapse to a default (or
//! `None`), and only a top-level non-object is treated as fatal.

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use serde::de::{self, Deserialize, Deserializer};
use serde_json::{Map, Value};

use crate::domain::companion::{AppLanguage, PokemonNature, Rarity};

/// Parses an RFC3339/ISO8601 timestamp with fractional seconds of any length.
/// `resets_at` arrives as microseconds (`"…034464+00:00"`) or milliseconds
/// (`"….303Z"`) — chrono accepts both without manual truncation.
pub fn parse_iso8601(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|d| d.with_timezone(&Utc))
}

/// i64 from key, `0` on missing / null / wrong type.
pub fn get_i64(m: &Map<String, Value>, key: &str) -> i64 {
    m.get(key).and_then(Value::as_i64).unwrap_or(0)
}

/// Option<i64>, `None` on missing / null / wrong type.
pub fn get_opt_i64(m: &Map<String, Value>, key: &str) -> Option<i64> {
    m.get(key).and_then(Value::as_i64)
}

/// bool from key, `false` on missing / null / wrong type.
pub fn get_bool(m: &Map<String, Value>, key: &str) -> bool {
    m.get(key).and_then(Value::as_bool).unwrap_or(false)
}

/// String from key, `""` on missing / null / wrong type.
pub fn get_string(m: &Map<String, Value>, key: &str) -> String {
    m.get(key).and_then(Value::as_str).unwrap_or("").to_string()
}

/// Option<String>, `None` on missing / null / wrong type.
pub fn get_opt_string(m: &Map<String, Value>, key: &str) -> Option<String> {
    m.get(key).and_then(Value::as_str).map(str::to_string)
}

/// f64 from key, `0.0` on missing / null / wrong type.
pub fn get_f64(m: &Map<String, Value>, key: &str) -> f64 {
    m.get(key).and_then(Value::as_f64).unwrap_or(0.0)
}

/// Option<f64>, `None` on missing / null / wrong type.
pub fn get_opt_f64(m: &Map<String, Value>, key: &str) -> Option<f64> {
    m.get(key).and_then(Value::as_f64)
}

/// HashSet<String> from an array key. A single non-string element makes the
/// whole field fall back to empty (mirrors `try? decode(Set<String>.self)`).
pub fn get_string_set(m: &Map<String, Value>, key: &str) -> HashSet<String> {
    match m.get(key) {
        Some(Value::Array(arr)) => {
            let mut out = HashSet::new();
            for v in arr {
                match v.as_str() {
                    Some(s) => {
                        out.insert(s.to_string());
                    }
                    None => return HashSet::new(),
                }
            }
            out
        }
        _ => HashSet::new(),
    }
}

/// HashMap<String, i64> from an object key. A single non-integer value makes
/// the whole field fall back to empty (mirrors `try? decode([String: Int].self)`).
pub fn get_string_i64_map(m: &Map<String, Value>, key: &str) -> HashMap<String, i64> {
    match m.get(key) {
        Some(Value::Object(obj)) => {
            let mut out = HashMap::new();
            for (k, v) in obj {
                match v.as_i64() {
                    Some(i) => {
                        out.insert(k.clone(), i);
                    }
                    None => return HashMap::new(),
                }
            }
            out
        }
        _ => HashMap::new(),
    }
}

/// Rarity from an enum raw-value key; `None` on missing / null / unknown value.
/// An unknown rawValue must NOT fabricate a guarantee (eggTier).
pub fn get_rarity(m: &Map<String, Value>, key: &str) -> Option<Rarity> {
    m.get(key)
        .and_then(Value::as_str)
        .and_then(Rarity::from_raw)
}

/// AppLanguage from a raw-value key; `system_default()` on missing / null / unknown.
pub fn get_language(m: &Map<String, Value>, key: &str) -> AppLanguage {
    m.get(key)
        .and_then(Value::as_str)
        .and_then(AppLanguage::from_raw)
        .unwrap_or_else(AppLanguage::system_default)
}

/// PokemonNature from a raw-value key; `None` on missing / null / unknown.
pub fn get_opt_nature(m: &Map<String, Value>, key: &str) -> Option<PokemonNature> {
    m.get(key)
        .and_then(Value::as_str)
        .and_then(PokemonNature::from_raw)
}

/// Required i64 — `Err` on missing / null / wrong type (drop the item).
pub fn get_required_i64<E: de::Error>(m: &Map<String, Value>, key: &str) -> Result<i64, E> {
    m.get(key)
        .and_then(Value::as_i64)
        .ok_or_else(|| E::custom(format!("missing or invalid {key}")))
}

/// Required array of i64 — `Err` on missing / null / wrong type / non-integer element.
pub fn get_required_i64_vec<E: de::Error>(
    m: &Map<String, Value>,
    key: &str,
) -> Result<Vec<i64>, E> {
    match m.get(key).and_then(Value::as_array) {
        Some(arr) => {
            let mut out = Vec::with_capacity(arr.len());
            for v in arr {
                match v.as_i64() {
                    Some(i) => out.push(i),
                    None => return Err(E::custom(format!("invalid {key} element"))),
                }
            }
            Ok(out)
        }
        None => Err(E::custom(format!("missing or invalid {key}"))),
    }
}

/// Optional array of i64 — `None` on missing / null / wrong type / non-integer element.
pub fn get_opt_i64_vec(m: &Map<String, Value>, key: &str) -> Option<Vec<i64>> {
    match m.get(key).and_then(Value::as_array) {
        Some(arr) => {
            let mut out = Vec::with_capacity(arr.len());
            for v in arr {
                out.push(v.as_i64()?);
            }
            Some(out)
        }
        None => None,
    }
}

/// Optional array of String — empty Vec on missing / null.
pub fn get_string_vec(m: &Map<String, Value>, key: &str) -> Vec<String> {
    match m.get(key).and_then(Value::as_array) {
        Some(arr) => arr
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect(),
        None => Vec::new(),
    }
}

/// Required Rarity raw-value — `Err` on missing / null / unknown value.
pub fn get_required_rarity<E: de::Error>(m: &Map<String, Value>, key: &str) -> Result<Rarity, E> {
    m.get(key)
        .and_then(Value::as_str)
        .and_then(Rarity::from_raw)
        .ok_or_else(|| E::custom(format!("missing or invalid {key}")))
}

/// speciesID → (langCode → name). The JSON object keys are integer-strings;
/// unparseable keys are skipped. Any malformed inner value fails the whole field.
pub fn species_names_from_value(v: &Value) -> Option<HashMap<i64, HashMap<String, String>>> {
    let obj = v.as_object()?;
    let mut out = HashMap::new();
    for (k, inner) in obj {
        let id = match k.parse::<i64>() {
            Ok(id) => id,
            Err(_) => continue,
        };
        let inner_obj = inner.as_object()?;
        let mut names = HashMap::new();
        for (lk, lv) in inner_obj {
            names.insert(lk.clone(), lv.as_str()?.to_string());
        }
        out.insert(id, names);
    }
    Some(out)
}

/// Option map for `DexEntry.names` — `None` on missing / null / any failure
/// (legacy flat formats degrade to nil rather than dropping the whole entry).
pub fn get_opt_species_names(
    m: &Map<String, Value>,
    key: &str,
) -> Option<HashMap<i64, HashMap<String, String>>> {
    let v = m.get(key)?;
    species_names_from_value(v)
}

/// `#[serde(deserialize_with = "crate::domain::decoding::de_species_names")]`
/// helper for int-keyed name maps.
pub fn de_species_names<'de, D>(
    deserializer: D,
) -> Result<HashMap<i64, HashMap<String, String>>, D::Error>
where
    D: Deserializer<'de>,
{
    let v = Value::deserialize(deserializer)?;
    species_names_from_value(&v).ok_or_else(|| de::Error::custom("invalid species names"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_micro_milli_and_plain_seconds() {
        // microseconds
        assert!(parse_iso8601("2026-06-10T11:10:00.034464+00:00").is_some());
        // milliseconds
        assert!(parse_iso8601("2026-06-10T11:10:00.303Z").is_some());
        // no fractional part
        assert!(parse_iso8601("2026-06-10T11:10:00Z").is_some());
    }

    #[test]
    fn returns_none_for_garbage() {
        assert!(parse_iso8601("not-a-date").is_none());
        assert!(parse_iso8601("").is_none());
    }

    #[test]
    fn micro_and_milli_resolve_to_same_instant() {
        let micro = parse_iso8601("2026-06-10T11:10:00.000000Z").unwrap();
        let plain = parse_iso8601("2026-06-10T11:10:00Z").unwrap();
        assert_eq!(micro, plain);
    }

    #[test]
    fn de_species_names_skips_unparseable_keys() {
        let json = r#"{"1":{"en":"P1","ko":"포1"},"abc":{"en":"X"},"2":{"en":"P2"}}"#;
        let v: Value = serde_json::from_str(json).unwrap();
        let m: HashMap<i64, HashMap<String, String>> = de_species_names(v).unwrap();
        assert_eq!(m.len(), 2);
        assert_eq!(m[&1]["ko"], "포1");
        assert_eq!(m[&2]["en"], "P2");
    }
}
