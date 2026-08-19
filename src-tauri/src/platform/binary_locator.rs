//! Locating CLI binaries (`ccusage`, `codex`, ...) installed through version
//! managers (mise/nvm/fnm/asdf/volta/bun) and their Homebrew peers.
//!
//! Port of `BinaryLocator.swift`. A GUI app launched by launchd does not
//! inherit the user's login-shell PATH, so version-manager tools cannot be
//! found from hardcoded paths alone. Strategy: static paths (fast) first, then
//! a login+interactive shell `command -v` lookup. Results are cached per binary
//! to avoid the cost of spawning a shell on every call.

use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{mpsc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use crate::platform::log;

/// Total budget (seconds) for a single shell spawn: wait for exit, then drain.
const SHELL_DEADLINE: Duration = Duration::from_secs(8);
/// How long a "not found" result stays cached before being re-resolved, so an
/// app that stays resident picks up tools installed while running. A found
/// path is cached forever but re-validated on each call.
const NOT_FOUND_TTL: Duration = Duration::from_secs(600);

/// Single-value marker script: prints `<<<BIN:<value>:BIN>>>` around the result
/// of `command -v`. The binary name arrives as `$1` (positional argument) and is
/// never interpolated into the script — injection-safe.
const SHELL_RESOLVE_SCRIPT: &str = r#"printf '<<<BIN:%s:BIN>>>' "$(command -v "$1" 2>/dev/null)""#;
/// Single environment variable lookup: reads the variable *named* by `$1` via
/// `eval` indirection. The expansion result is never re-parsed, so metacharacters
/// in the value come through unchanged.
const SHELL_ENV_VALUE_SCRIPT: &str =
    r#"printf '<<<BIN:%s:BIN>>>' "$(eval printf '%s' \"\$$1\" 2>/dev/null)""#;
/// Batch environment lookup: names arrive as positional arguments (`$@`), never
/// interpolated into the script. Each pair carries its name so values are
/// matched by name, not by order.
const SHELL_ENV_VALUES_SCRIPT: &str = r#"for n in "$@"; do printf '<<<BIN:%s:%s:BIN>>>' "$n" "$(eval printf '%s' \"\$$n\" 2>/dev/null)"; done"#;

struct Cached {
    path: Option<String>,
    at: Instant,
}

static CACHE: OnceLock<Mutex<HashMap<String, Cached>>> = OnceLock::new();

fn cache() -> &'static Mutex<HashMap<String, Cached>> {
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Absolute path to `binary`, or `None` if it cannot be found. Thread-safe and
/// cached: a found path is re-validated per call (re-resolved if the file is no
/// longer executable); a miss is cached for `NOT_FOUND_TTL`.
pub fn resolve(binary: &str, static_paths: &[String]) -> Option<String> {
    let mut cache = cache()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(hit) = cache.get(binary) {
        if let Some(path) = &hit.path {
            if is_executable(path) {
                return Some(path.clone());
            }
            log::write(&format!("{binary} cached path gone, re-resolving: {path}"));
        } else if hit.at.elapsed() < NOT_FOUND_TTL {
            return None;
        }
    }
    let result = locate(binary, static_paths);
    cache.insert(
        binary.to_string(),
        Cached {
            path: result.clone(),
            at: Instant::now(),
        },
    );
    log::write(&match &result {
        Some(path) => format!("{binary} resolved: {path}"),
        None => format!("{binary} NOT found on PATH"),
    });
    result
}

/// Child-process PATH augmentation. A GUI app's minimal PATH cannot find the
/// version-manager binary (mise, etc.) that a shim must exec, so the resolved
/// binary's directory plus the common tool directories are prepended to the
/// base PATH (deduplicated, first occurrence wins).
pub fn augmented_environment(
    binary_path: &str,
    mut base: HashMap<String, String>,
) -> HashMap<String, String> {
    let binary_dir = Path::new(binary_path.trim_end_matches(['/', '\\']))
        .parent()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut paths = vec![binary_dir];
    paths.extend(common_tool_directories());

    #[cfg(target_os = "windows")]
    let (sep, default_path) = (';', "");
    #[cfg(not(target_os = "windows"))]
    let (sep, default_path) = (':', "/usr/bin:/bin:/usr/sbin:/sbin");

    if let Some(existing) = base.get("PATH").map(String::as_str) {
        for entry in existing.split(sep) {
            if !entry.is_empty() {
                paths.push(entry.to_string());
            }
        }
    } else if !default_path.is_empty() {
        for entry in default_path.split(sep) {
            paths.push(entry.to_string());
        }
    }

    let mut seen = HashSet::new();
    let merged = paths
        .into_iter()
        .filter(|p| !p.is_empty() && seen.insert(p.clone()))
        .collect::<Vec<String>>()
        .join(&sep.to_string());
    base.insert("PATH".to_string(), merged);
    base
}

/// Invalidate the resolution cache (settings change / re-detect).
pub fn reset() {
    cache()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clear();
}

/// Common bin/shims directories for version managers and package managers —
/// the single source shared by `common_node_tool_paths` (lookup) and
/// `augmented_environment` (child PATH augmentation). Adding a new manager
/// touches this one place only.
pub fn common_tool_directories() -> Vec<String> {
    #[cfg(target_os = "windows")]
    {
        // Windows has no Homebrew/version-manager shim layout of this kind.
        Vec::new()
    }
    #[cfg(not(target_os = "windows"))]
    {
        let mut dirs = Vec::new();
        // Homebrew (Intel) / npm prefix. `/opt/homebrew/bin` (Apple Silicon) is
        // macOS-only and deliberately omitted on Linux.
        dirs.push("/usr/local/bin".to_string());
        if let Some(home) = home_dir() {
            let home = home.to_string_lossy();
            let home = home.trim_end_matches('/');
            dirs.push(format!("{home}/.local/share/mise/shims")); // mise (shims mode)
            dirs.push(format!("{home}/.asdf/shims")); // asdf
            dirs.push(format!("{home}/.volta/bin")); // Volta
            dirs.push(format!("{home}/.bun/bin")); // Bun
            dirs.push(format!("{home}/.npm-global/bin")); // npm prefix=~/.npm-global
            dirs.push(format!("{home}/.local/bin"));
        }
        dirs.push("/usr/bin".to_string());
        dirs
    }
}

/// Version-manager shim/bin paths plus the given static paths, each with
/// `binary` appended (absolute-path-first lookup).
pub fn common_node_tool_paths(binary: &str) -> Vec<String> {
    common_tool_directories()
        .into_iter()
        .map(|dir| format!("{dir}/{binary}"))
        .collect()
}

/// Read one environment variable from the login shell. Finder/launchd-launched
/// apps do not inherit the shell environment, so process env alone misses values
/// the user exported in `~/.zshrc`. Costs a shell spawn — callers must cache.
pub fn shell_environment_value(name: &str) -> Option<String> {
    // Only ASCII uppercase/digits/underscore — guards against shell injection
    // (see `is_shell_safe_environment_name`).
    if !is_shell_safe_environment_name(name) {
        return None;
    }
    // The name is passed as a positional argument and `eval` expands only the
    // value of that variable, so metacharacters in the value pass through intact.
    shell_marked_value(
        SHELL_ENV_VALUE_SCRIPT,
        &[name.to_string()],
        &format!("{name} shell env lookup"),
    )
}

/// Read several environment variables in a single shell spawn. Unset/empty
/// values are excluded, so a missing key means "this user does not use it".
pub fn shell_environment_values(names: &[String]) -> HashMap<String, String> {
    let safe: Vec<String> = names
        .iter()
        .filter(|n| is_shell_safe_environment_name(n))
        .cloned()
        .collect();
    if safe.is_empty() {
        return HashMap::new();
    }
    let Some(raw) = shell_marked_output(
        SHELL_ENV_VALUES_SCRIPT,
        &safe,
        &format!("env batch lookup({})", safe.len()),
    ) else {
        return HashMap::new();
    };
    let mut out = HashMap::new();
    for name in safe {
        if let Some(value) = parse_marked_value(&raw, &name) {
            out.insert(name, value);
        }
    }
    out
}

/// Shell-injection guard — ASCII uppercase letters, digits, and underscore only.
/// Unicode-aware checks (`isUppercase`/`isNumber`) would admit `Σ`, Cyrillic
/// `А`, or `٣`; gating on ASCII keeps the guard's promise and the accepted set
/// in agreement.
pub fn is_shell_safe_environment_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii() && (c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_'))
}

/// Extract the value for `NAME` from `<<<BIN:NAME:value:BIN>>>`. Profile noise
/// and pairs for other names are ignored — names ride in the marker so pairs
/// are matched by name, not by position.
pub fn parse_marked_value(s: &str, name: &str) -> Option<String> {
    let marker = format!("<<<BIN:{name}:");
    let value_start = s.find(&marker)? + marker.len();
    let end = s[value_start..].find(":BIN>>>")? + value_start;
    let value = s[value_start..end].trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

/// Extract the path from `<<<BIN:/path/to/tool:BIN>>>`. Profile noise is ignored.
pub fn parse_marked_path(s: &str) -> Option<String> {
    const MARKER: &str = "<<<BIN:";
    let value_start = s.find(MARKER)? + MARKER.len();
    let end = s[value_start..].find(":BIN>>>")? + value_start;
    let path = s[value_start..end].trim();
    if path.is_empty() {
        None
    } else {
        Some(path.to_string())
    }
}

/// User home directory: `$HOME` on Unix, `%USERPROFILE%` on Windows.
pub fn home_dir() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        std::env::var("USERPROFILE").ok().map(PathBuf::from)
    }
    #[cfg(not(target_os = "windows"))]
    {
        std::env::var("HOME").ok().map(PathBuf::from)
    }
}

/// Returns all discovered WSL home directories on Windows (e.g. `\\wsl.localhost\Ubuntu\home\username`).
/// On non-Windows platforms, returns an empty vector.
pub fn wsl_home_dirs() -> Vec<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        let mut distros = Vec::new();
        // Method 1: Query wsl.exe -l -q
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x08000000;
            if let Ok(output) = std::process::Command::new("wsl.exe")
                .args(["-l", "-q"])
                .creation_flags(CREATE_NO_WINDOW)
                .output()
            {
                let u16_vec: Vec<u16> = output
                    .stdout
                    .chunks_exact(2)
                    .map(|c| u16::from_le_bytes([c[0], c[1]]))
                    .collect();
                let text = String::from_utf16_lossy(&u16_vec);
                for line in text.lines() {
                    let name = line.trim().trim_matches('\0');
                    if !name.is_empty() && !name.starts_with("docker-desktop") {
                        distros.push(name.to_string());
                    }
                }
            }
        }
        // Method 2: Common distribution names fallback
        for candidate in [
            "Ubuntu",
            "Ubuntu-24.04",
            "Ubuntu-22.04",
            "Ubuntu-20.04",
            "Debian",
            "kali-linux",
            "Arch",
            "openSUSE",
        ] {
            if !distros.iter().any(|d| d.eq_ignore_ascii_case(candidate)) {
                let p = format!(r"\\wsl.localhost\{}\home", candidate);
                if Path::new(&p).is_dir() {
                    distros.push(candidate.to_string());
                }
            }
        }

        let mut dirs = Vec::new();
        for distro in distros {
            let home_parent = PathBuf::from(format!(r"\\wsl.localhost\{}\home", distro));
            if let Ok(user_entries) = std::fs::read_dir(&home_parent) {
                for user_entry in user_entries.flatten() {
                    let user_path = user_entry.path();
                    if user_path.is_dir() {
                        dirs.push(user_path);
                    }
                }
            }
        }
        dirs
    }
    #[cfg(not(target_os = "windows"))]
    {
        Vec::new()
    }
}

fn locate(binary: &str, static_paths: &[String]) -> Option<String> {
    // On macOS a user-supplied override ("<binary>Path" in UserDefaults) is
    // checked before the static paths; there is no cross-platform equivalent yet.
    if let Some(hit) = static_paths.iter().find(|p| is_executable(p)) {
        return Some(hit.clone());
    }
    shell_resolve(binary)
}

/// Resolve via a login+interactive shell: `command -v <binary>`, the result
/// wrapped in markers so interactive-profile noise (neofetch, ...) is stripped.
fn shell_resolve(binary: &str) -> Option<String> {
    // The binary name travels as a positional argument (`$1`) — never string-
    // interpolated, so external input cannot be injected.
    let path = shell_marked_value(
        SHELL_RESOLVE_SCRIPT,
        &[binary.to_string()],
        &format!("{binary} shell resolve"),
    )?;
    if is_executable(&path) {
        Some(path)
    } else {
        None
    }
}

/// Spawn the shell, wrap a single marker-wrapped value, and parse it back out.
fn shell_marked_value(script: &str, arguments: &[String], label: &str) -> Option<String> {
    shell_marked_output(script, arguments, label).and_then(|raw| parse_marked_path(&raw))
}

/// Spawn `$SHELL -ilc <script> sh <arguments>` with stdin nulled, capture
/// stdout, and return the whole output (marker extraction is the caller's job).
///
/// stdout is drained in a background thread *before* waiting for exit: an
/// interactive profile that floods the pipe (64 KB) would otherwise block the
/// child forever, and a wait-then-drain structure would burn the whole timeout
/// before returning `None`. An 8-second deadline covers exit + drain; a shell
/// that outlives it is killed.
fn shell_marked_output(script: &str, arguments: &[String], label: &str) -> Option<String> {
    let shell = shell_path()?;
    let mut command = Command::new(&shell);
    command
        .args(["-ilc", script, "sh"])
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = command.spawn().ok()?;
    let mut stdout = child.stdout.take()?;

    let (tx, rx) = mpsc::channel::<Vec<u8>>();
    thread::spawn(move || {
        let mut data = Vec::new();
        let _ = stdout.read_to_end(&mut data);
        let _ = tx.send(data);
    });

    let deadline = Instant::now() + SHELL_DEADLINE;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                log::write(&format!("{label} timed out"));
                return None;
            }
            Ok(None) => thread::sleep(Duration::from_millis(50)),
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }

    // The drain gets the *entire* remaining budget. Even after the shell exits,
    // a background job it spawned (zsh-async, zinit turbo, ...) may keep stdout's
    // write end open, delaying EOF — a short drain cap here would regress users
    // who previously got a value.
    let remaining = deadline.saturating_duration_since(Instant::now());
    match rx.recv_timeout(remaining) {
        Ok(data) => String::from_utf8(data).ok(),
        Err(_) => {
            log::write(&format!("{label} output drain timed out"));
            None
        }
    }
}

#[cfg(target_os = "windows")]
fn shell_path() -> Option<String> {
    None
}

#[cfg(not(target_os = "windows"))]
fn shell_path() -> Option<String> {
    match std::env::var("SHELL") {
        Ok(shell) if is_executable(&shell) => return Some(shell),
        _ => {}
    }
    if is_executable("/bin/bash") {
        return Some("/bin/bash".to_string());
    }
    None
}

fn is_executable(path: &str) -> bool {
    match std::fs::metadata(path) {
        Ok(meta) if meta.is_file() => {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                (meta.permissions().mode() & 0o111) != 0
            }
            #[cfg(not(unix))]
            {
                true
            }
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    static TEST_BIN_SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

    #[test]
    #[cfg(not(target_os = "windows"))]
    fn augmented_environment_prepends_tool_paths() {
        let home = home_dir().expect("HOME set in test environment");
        let home = home.to_string_lossy();
        let binary = format!("{home}/.local/share/mise/shims/codex");
        let base = HashMap::from([
            ("PATH".to_string(), "/usr/bin:/bin".to_string()),
            ("LANG".to_string(), "en_US.UTF-8".to_string()),
        ]);
        let env = augmented_environment(&binary, base);
        let paths: Vec<&str> = env["PATH"].split(':').collect();

        assert_eq!(paths[0], format!("{home}/.local/share/mise/shims"));
        assert!(paths.contains(&"/usr/local/bin"));
        assert!(paths.contains(&format!("{home}/.local/bin").as_str()));
        assert!(paths.contains(&"/usr/bin"));
        assert!(
            !paths.contains(&"/opt/homebrew/bin"),
            "macOS-only path on Linux"
        );
        assert_eq!(
            paths
                .iter()
                .filter(|p| **p == format!("{home}/.local/share/mise/shims"))
                .count(),
            1,
            "PATH must be deduplicated"
        );
        assert_eq!(env["LANG"], "en_US.UTF-8");
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn augmented_environment_prepends_tool_paths_windows() {
        let binary = "C:\\Program Files\\Codex\\codex.exe";
        let base = HashMap::from([
            (
                "PATH".to_string(),
                "C:\\Windows\\System32;C:\\Windows".to_string(),
            ),
            ("LANG".to_string(), "en_US.UTF-8".to_string()),
        ]);
        let env = augmented_environment(binary, base);
        let paths: Vec<&str> = env["PATH"].split(';').collect();

        assert_eq!(paths[0], "C:\\Program Files\\Codex");
        assert!(paths.contains(&"C:\\Windows\\System32"));
        assert!(paths.contains(&"C:\\Windows"));
        assert_eq!(env["LANG"], "en_US.UTF-8");
    }

    #[test]
    #[cfg(not(target_os = "windows"))]
    fn augmented_environment_uses_default_path_when_base_has_none() {
        let env = augmented_environment("/usr/local/bin/codex", HashMap::new());
        let paths: Vec<&str> = env["PATH"].split(':').collect();
        assert_eq!(paths[0], "/usr/local/bin");
        assert!(paths.contains(&"/usr/bin"));
    }

    #[test]
    #[cfg(not(target_os = "windows"))]
    fn augmented_environment_dedups_preserving_first_occurrence() {
        let base = HashMap::from([(
            "PATH".to_string(),
            "/usr/bin:/bin:/usr/local/bin".to_string(),
        )]);
        let env = augmented_environment("/usr/local/bin/codex", base);
        let paths: Vec<&str> = env["PATH"].split(':').collect();
        assert_eq!(paths[0], "/usr/local/bin", "binary dir first");
        assert_eq!(
            paths.iter().filter(|p| **p == "/usr/local/bin").count(),
            1,
            "base PATH duplicate must be dropped"
        );
        assert!(paths.contains(&"/bin"));
    }

    #[test]
    #[cfg(not(target_os = "windows"))]
    fn common_tool_directories_have_expected_layout() {
        let dirs = common_tool_directories();
        assert!(dirs.contains(&"/usr/local/bin".to_string()));
        assert!(dirs.contains(&"/usr/bin".to_string()));
        assert!(
            !dirs.contains(&"/opt/homebrew/bin".to_string()),
            "macOS-only Homebrew path on Linux"
        );
        if let Some(home) = home_dir() {
            let home = home.to_string_lossy();
            for suffix in [
                "/.local/share/mise/shims",
                "/.asdf/shims",
                "/.volta/bin",
                "/.bun/bin",
                "/.npm-global/bin",
                "/.local/bin",
            ] {
                assert!(
                    dirs.contains(&format!("{home}{suffix}")),
                    "missing {suffix}"
                );
            }
        }
    }

    #[test]
    #[cfg(not(target_os = "windows"))]
    fn common_node_tool_paths_include_manager_dirs() {
        let paths = common_node_tool_paths("ccusage");
        assert!(paths.iter().any(|p| p.ends_with("/.asdf/shims/ccusage")));
        assert!(paths.iter().any(|p| p.ends_with("/.volta/bin/ccusage")));
        assert!(paths
            .iter()
            .any(|p| p.ends_with("/.local/share/mise/shims/ccusage")));
        assert!(paths.contains(&"/usr/local/bin/ccusage".to_string()));
        assert!(!paths.contains(&"/opt/homebrew/bin/ccusage".to_string()));
    }

    #[test]
    fn parse_marked_path_clean() {
        assert_eq!(
            parse_marked_path("<<<BIN:/opt/homebrew/bin/ccusage:BIN>>>"),
            Some("/opt/homebrew/bin/ccusage".to_string())
        );
    }

    #[test]
    fn parse_marked_path_ignores_profile_noise() {
        let noisy = "⠀⣴⣶⣷ neofetch art line 1\nOS: macOS / Shell: zsh\n\
                     <<<BIN:/Users/x/.local/share/mise/installs/node/22.14.0/bin/ccusage:BIN>>>\n";
        assert_eq!(
            parse_marked_path(noisy),
            Some("/Users/x/.local/share/mise/installs/node/22.14.0/bin/ccusage".to_string())
        );
    }

    #[test]
    fn parse_marked_path_empty_and_missing() {
        assert_eq!(parse_marked_path("noise\n<<<BIN::BIN>>>\n"), None);
        assert_eq!(
            parse_marked_path("just some neofetch output, no marker"),
            None
        );
        assert_eq!(parse_marked_path("<<<BIN:/path/without/closing"), None);
    }

    #[test]
    fn parse_marked_path_trims_whitespace() {
        assert_eq!(
            parse_marked_path("<<<BIN:  /usr/local/bin/codex \n :BIN>>>"),
            Some("/usr/local/bin/codex".to_string())
        );
    }

    #[test]
    fn parse_marked_value_picks_pair_by_name_across_noise() {
        let raw = "neofetch banner line\n\
                   <<<BIN:A_HOME:/first:BIN>>>oh-my-zsh noise<<<BIN:B_HOME:/second:BIN>>>\n\
                   trailing noise\n";
        assert_eq!(
            parse_marked_value(raw, "A_HOME"),
            Some("/first".to_string())
        );
        assert_eq!(
            parse_marked_value(raw, "B_HOME"),
            Some("/second".to_string())
        );
        assert_eq!(parse_marked_value(raw, "C_HOME"), None);
    }

    #[test]
    fn parse_marked_value_treats_empty_pair_as_absent() {
        assert_eq!(parse_marked_value("<<<BIN:A_HOME::BIN>>>", "A_HOME"), None);
    }

    #[test]
    fn parse_marked_value_does_not_confuse_prefix_names() {
        let raw = "<<<BIN:A_HOME_EXTRA:/extra:BIN>>><<<BIN:A_HOME:/plain:BIN>>>";
        assert_eq!(
            parse_marked_value(raw, "A_HOME_EXTRA"),
            Some("/extra".to_string())
        );
    }

    #[test]
    fn parse_marked_value_special_char_names_not_found() {
        assert_eq!(
            parse_marked_value("<<<BIN:A_HOME:/x:BIN>>>", "A-HOME"),
            None
        );
        assert_eq!(
            parse_marked_value("<<<BIN:A_HOME:/x:BIN>>>", "A_HOME_EXTRA"),
            None
        );
    }

    #[test]
    fn shell_safe_environment_name_rejects_injection_shapes() {
        for good in ["GROK_HOME", "A1_B2", "X"] {
            assert!(is_shell_safe_environment_name(good), "{good}");
        }
        for bad in [
            "",
            "grok_home",
            "A-B",
            "A B",
            "A;rm -rf /",
            "Σ",
            "А",
            "٣",
            "A$B",
        ] {
            assert!(!is_shell_safe_environment_name(bad), "{bad}");
        }
    }

    #[test]
    fn shell_environment_values_rejects_all_unsafe_names_without_spawning() {
        assert!(shell_environment_values(&[]).is_empty());
        assert!(shell_environment_values(&["bad name".to_string(), "x".to_string()]).is_empty());
    }

    #[test]
    fn resolve_finds_static_path_without_shell() {
        let seq = TEST_BIN_SEQ.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("ptb-test-bin-{seq}"));
        std::fs::create_dir_all(&dir).unwrap();
        let bin = dir.join("fake-codex");
        std::fs::write(&bin, b"#!/bin/sh\nexit 0\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let path = bin.to_string_lossy().into_owned();

        reset();
        assert_eq!(
            resolve("fake-codex", std::slice::from_ref(&path)),
            Some(path.clone())
        );
        assert_eq!(
            resolve("fake-codex", std::slice::from_ref(&path)),
            Some(path.clone())
        );
        reset();

        std::fs::remove_dir_all(&dir).unwrap();
    }
}
