//! Token/cost number formatting for menu-bar and popover display.
//!
//! Mirrors the original `Core/TokenFormatter.swift`. Output matches Swift's
//! `String(format:)` (e.g. `{:.1}`, `{:.2}`, `{:.0}`).

/// 987 → "987", 12_345 → "12.3K", 190_612_940 → "190.6M", 1_240_000_000 → "1.24B"
pub struct TokenFormatter;

impl TokenFormatter {
    pub fn compact(value: i64) -> String {
        let v = value.unsigned_abs() as f64;
        let sign = if value < 0 { "-" } else { "" };
        if v < 1_000.0 {
            value.to_string()
        } else if v < 1_000_000.0 {
            format!("{sign}{}K", Self::trim(v / 1_000.0, 1))
        } else if v < 1_000_000_000.0 {
            format!("{sign}{}M", Self::trim(v / 1_000_000.0, 1))
        } else {
            format!("{sign}{}B", Self::trim(v / 1_000_000_000.0, 2))
        }
    }

    /// Thousands separators for popover detail rows (190,612,940).
    ///
    /// TODO: On macOS the separator followed `Locale.current` (en/ko/ja ",",
    /// es/de ".", fr/ru " "). This port only implements the comma convention
    /// used by en/ko/ja; other-locale separators are not yet supported.
    pub fn grouped(value: i64) -> String {
        let negative = value < 0;
        let digits = value.unsigned_abs().to_string();
        let mut out = String::with_capacity(digits.len() + digits.len() / 3);
        let first = digits.len() % 3;
        for (i, c) in digits.chars().enumerate() {
            if i > 0 && (i - first).is_multiple_of(3) {
                out.push(',');
            }
            out.push(c);
        }
        if negative {
            format!("-{out}")
        } else {
            out
        }
    }

    pub fn cost(usd: f64) -> String {
        format!("${usd:.2}")
    }

    /// Short cost notation for the menu bar: $9.5 / $311 / $1.2K
    pub fn cost_compact(usd: f64) -> String {
        if usd < 100.0 {
            format!("${usd:.1}")
        } else if usd < 10_000.0 {
            format!("${usd:.0}")
        } else {
            format!("${:.1}K", usd / 1_000.0)
        }
    }

    pub fn percent(value: f64) -> String {
        if value == value.round() {
            format!("{value:.0}%")
        } else {
            format!("{value:.1}%")
        }
    }

    fn trim(value: f64, decimals: usize) -> String {
        let mut s = format!("{value:.decimals$}");
        while s.ends_with('0') {
            s.pop();
        }
        if s.ends_with('.') {
            s.pop();
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact() {
        assert_eq!(TokenFormatter::compact(0), "0");
        assert_eq!(TokenFormatter::compact(987), "987");
        assert_eq!(TokenFormatter::compact(12_345), "12.3K");
        assert_eq!(TokenFormatter::compact(190_612_940), "190.6M");
        assert_eq!(TokenFormatter::compact(1_240_000_000), "1.24B");
        assert_eq!(TokenFormatter::compact(1_000_000), "1M");
    }

    #[test]
    fn grouped() {
        assert_eq!(TokenFormatter::grouped(253_412_890), "253,412,890");
        assert_eq!(TokenFormatter::grouped(1234), "1,234");
    }

    #[test]
    fn cost_and_cost_compact() {
        assert_eq!(TokenFormatter::cost(48.104), "$48.10");
        assert_eq!(TokenFormatter::cost_compact(9.54), "$9.5"); // < 100 → 1 decimal
        assert_eq!(TokenFormatter::cost_compact(311.4), "$311"); // < 10K → integer
        assert_eq!(TokenFormatter::cost_compact(12_340.0), "$12.3K"); // ≥ 10K → K
    }

    #[test]
    fn percent() {
        assert_eq!(TokenFormatter::percent(88.0), "88%");
        assert_eq!(TokenFormatter::percent(88.35), "88.3%");
    }
}
