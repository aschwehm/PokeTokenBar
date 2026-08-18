//! User-supplied usage-log location environment variables.
//!
//! Port of `UsageEnvironment.swift`. A GUI app launched by Finder/launchd does
//! not inherit the login-shell environment, so providers that point at usage
//! logs via a variable the user exported in `~/.zshrc` would silently see
//! nothing. Providers must register their override variable in `names()`; the
//! lookup is batched into a single login-shell spawn, so more providers do not
//! cost more startup time.

use std::collections::HashMap;
use std::sync::OnceLock;

use crate::platform::binary_locator;

/// Canonical registry of user-exported usage-location overrides. Adding a new
/// provider that uses one is a one-line change here.
pub fn names() -> &'static [&'static str] {
    &[
        "CLAUDE_CONFIG_DIR", // Claude CLI config directory (comma-separated)
        "OPENCODE_DATA_DIR", // OpenCode data directory
        "HERMES_HOME",       // Hermes home
        "COPILOT_HOME",      // Copilot CLI home
        "GROK_HOME",         // Grok CLI home
    ]
}

/// Resolved once per process. Env vars do not change while the app runs, so no
/// TTL is needed, and a missing key *is* the negative cache — re-resolving would
/// make the majority of users (no overrides) pay a shell spawn on every refresh.
static RESOLVED: OnceLock<HashMap<String, String>> = OnceLock::new();

/// Value of `name`: process environment first, then the login shell, resolved
/// exactly once per process.
pub fn value(name: &str) -> Option<String> {
    resolved().get(name).cloned()
}

fn resolved() -> &'static HashMap<String, String> {
    RESOLVED.get_or_init(|| {
        resolve(
            names(),
            &std::env::vars().collect::<HashMap<_, _>>(),
            binary_locator::shell_environment_values,
        )
    })
}

/// The lookup policy, injectable for testing — the two branches each get
/// exercised: if the process environment has everything, the shell is *not*
/// spawned; if something is missing, only the missing names are looked up, in a
/// single spawn. A real shell spawn cannot be reproduced in tests, so this
/// branch would let a "process-env-only" regression pass unchecked.
pub fn resolve(
    names: &[&str],
    process_environment: &HashMap<String, String>,
    mut shell_lookup: impl FnMut(&[String]) -> HashMap<String, String>,
) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let mut missing = Vec::new();
    for name in names {
        // A blank value (`export FOO=`) counts as unset — treating it as set
        // would scan a nonexistent path and silently read 0.
        match process_environment.get(*name) {
            Some(value) if !value.trim().is_empty() => {
                out.insert((*name).to_string(), value.clone());
            }
            _ => missing.push((*name).to_string()),
        }
    }
    // The majority of users (terminal-launched, no overrides) stop here — no
    // shell is spawned.
    if missing.is_empty() {
        return out;
    }
    for (name, value) in shell_lookup(&missing) {
        if !value.trim().is_empty() {
            out.insert(name, value);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_all_present_skips_shell_lookup() {
        let mut calls: Vec<Vec<String>> = Vec::new();
        let names = ["A_HOME", "B_HOME"];
        let process = HashMap::from([
            ("A_HOME".to_string(), "/a".to_string()),
            ("B_HOME".to_string(), "/b".to_string()),
        ]);
        let out = resolve(&names, &process, |missing| {
            calls.push(missing.to_vec());
            HashMap::new()
        });
        assert_eq!(out.get("A_HOME").map(String::as_str), Some("/a"));
        assert_eq!(out.get("B_HOME").map(String::as_str), Some("/b"));
        assert!(calls.is_empty(), "shell was invoked: {calls:?}");
    }

    #[test]
    fn resolve_missing_names_batched_into_single_shell_lookup() {
        let mut calls: Vec<Vec<String>> = Vec::new();
        let names = ["A_HOME", "B_HOME", "C_HOME"];
        let process = HashMap::from([("B_HOME".to_string(), "/b".to_string())]);
        let out = resolve(&names, &process, |missing| {
            calls.push(missing.to_vec());
            HashMap::from([
                ("A_HOME".to_string(), "/shell/a".to_string()),
                ("C_HOME".to_string(), "/shell/c".to_string()),
            ])
        });
        assert_eq!(calls.len(), 1, "shell must be invoked exactly once");
        assert_eq!(calls[0], vec!["A_HOME".to_string(), "C_HOME".to_string()]);
        assert_eq!(out.get("A_HOME").map(String::as_str), Some("/shell/a"));
        assert_eq!(out.get("B_HOME").map(String::as_str), Some("/b"));
        assert_eq!(out.get("C_HOME").map(String::as_str), Some("/shell/c"));
    }

    #[test]
    fn resolve_value_visible_only_to_login_shell_is_picked_up() {
        // Defect-triggering branch: the value is *absent* from the process env
        // and only the shell provides it. A process-env-only regression shows up
        // exactly here.
        let names = ["COPILOT_HOME"];
        let process = HashMap::new();
        let out = resolve(&names, &process, |_| {
            HashMap::from([(
                "COPILOT_HOME".to_string(),
                "/Users/someone/relocated".to_string(),
            )])
        });
        assert_eq!(
            out.get("COPILOT_HOME").map(String::as_str),
            Some("/Users/someone/relocated")
        );
    }

    #[test]
    fn resolve_blank_process_value_falls_back_to_shell() {
        let names = ["HERMES_HOME"];
        let process = HashMap::from([("HERMES_HOME".to_string(), "   ".to_string())]);
        let out = resolve(&names, &process, |_| {
            HashMap::from([("HERMES_HOME".to_string(), "/real".to_string())])
        });
        assert_eq!(out.get("HERMES_HOME").map(String::as_str), Some("/real"));
    }

    #[test]
    fn resolve_blank_shell_value_is_discarded() {
        let names = ["HERMES_HOME"];
        let process = HashMap::new();
        let out = resolve(&names, &process, |_| {
            HashMap::from([("HERMES_HOME".to_string(), "  \n ".to_string())])
        });
        assert!(!out.contains_key("HERMES_HOME"));
    }

    #[test]
    fn registered_names_cover_every_provider_override() {
        for name in [
            "CLAUDE_CONFIG_DIR",
            "OPENCODE_DATA_DIR",
            "HERMES_HOME",
            "COPILOT_HOME",
            "GROK_HOME",
        ] {
            assert!(names().contains(&name), "{name} missing from registry");
        }
        let mut seen = std::collections::HashSet::new();
        for name in names() {
            assert!(seen.insert(*name), "duplicate name {name}");
            assert!(
                binary_locator::is_shell_safe_environment_name(name),
                "{name} rejected by shell lookup"
            );
        }
    }
}
