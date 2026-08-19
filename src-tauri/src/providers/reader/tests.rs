use std::collections::HashSet;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Local, Utc};
use serde_json::json;

use super::*;

fn epoch() -> DateTime<Utc> {
    DateTime::from_timestamp(0, 0).unwrap()
}

fn temp_dir() -> PathBuf {
    let d = std::env::temp_dir().join(format!("ptb-rust-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn write_lines(dir: &Path, lines: &[String], name: &str, sub: Option<&str>) -> PathBuf {
    let folder = match sub {
        Some(s) => dir.join(s),
        None => dir.to_path_buf(),
    };
    std::fs::create_dir_all(&folder).unwrap();
    let path = folder.join(name);
    std::fs::write(&path, lines.join("\n")).unwrap();
    path
}

fn write_str(path: &Path, content: &str) -> PathBuf {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, content).unwrap();
    path.to_path_buf()
}

fn write_probe_file(lines: &[String]) -> PathBuf {
    let url = temp_dir().join("rollout-probe.jsonl");
    std::fs::write(&url, lines.join("\n")).unwrap();
    url
}

fn codex_entries_in(dir: &Path) -> Vec<Entry> {
    super::codex_entries(epoch(), Some(dir))
}

fn usage_entry(id: &str, date: DateTime<Utc>, tokens: i64) -> Entry {
    Entry {
        id: id.to_string(),
        date,
        local_day: local_day(date),
        model: "claude-opus-4-8".to_string(),
        input: tokens,
        output: 0,
        cache_write: 0,
        cache_read: 0,
        explicit_cost: None,
    }
}

// MARK: - Fixture builders (mirror the Swift test helpers)

#[allow(clippy::too_many_arguments)]
fn claude_line(
    id: &str,
    req: &str,
    model: &str,
    ts: &str,
    i: i64,
    o: i64,
    cw: i64,
    cr: i64,
) -> String {
    serde_json::to_string(&json!({
        "type": "assistant",
        "requestId": req,
        "timestamp": ts,
        "message": {
            "id": id,
            "model": model,
            "usage": {
                "input_tokens": i,
                "output_tokens": o,
                "cache_creation_input_tokens": cw,
                "cache_read_input_tokens": cr,
            },
        },
    }))
    .unwrap()
}

fn codex_line(
    ts: &str,
    input: i64,
    cached: i64,
    output: i64,
    reasoning: i64,
    cache_write: i64,
) -> String {
    format!(
        r#"{{"type":"event_msg","timestamp":"{ts}","payload":{{"type":"token_count","info":{{"last_token_usage":{{"input_tokens":{input},"cached_input_tokens":{cached},"cache_write_input_tokens":{cache_write},"output_tokens":{output},"reasoning_output_tokens":{reasoning},"total_tokens":{total}}}}}}}}}"#,
        total = input + output,
    )
}

fn codex_line_default(ts: &str) -> String {
    codex_line(ts, 1_000, 200, 50, 10, 0)
}

fn codex_session_meta_line(id: &str, ts: &str) -> String {
    format!(
        r#"{{"type":"session_meta","timestamp":"{ts}","payload":{{"id":"{id}","session_id":"{id}"}}}}"#
    )
}

#[allow(clippy::too_many_arguments)]
fn codex_state_line(
    ts: &str,
    cumulative_input: i64,
    cumulative_cached: i64,
    cumulative_output: i64,
    cumulative_reasoning: i64,
    last_input: i64,
    last_cached: i64,
    last_output: i64,
    last_reasoning: i64,
    last_total: Option<i64>,
    cache_write: Option<i64>,
) -> String {
    let cumulative_total = cumulative_input + cumulative_output;
    let reported_last_total = last_total.unwrap_or(last_input + last_output);
    let cache_write_field = cache_write
        .map(|v| format!(",\"cache_write_input_tokens\":{v}"))
        .unwrap_or_default();
    format!(
        r#"{{"type":"event_msg","timestamp":"{ts}","payload":{{"type":"token_count","info":{{"total_token_usage":{{"input_tokens":{ci},"cached_input_tokens":{cc}{cw},"output_tokens":{co},"reasoning_output_tokens":{cr},"total_tokens":{ct}}},"last_token_usage":{{"input_tokens":{li},"cached_input_tokens":{lc}{cw},"output_tokens":{lo},"reasoning_output_tokens":{lr},"total_tokens":{lt}}}}}}}}}"#,
        ts = ts,
        ci = cumulative_input,
        cc = cumulative_cached,
        cw = cache_write_field,
        co = cumulative_output,
        cr = cumulative_reasoning,
        ct = cumulative_total,
        li = last_input,
        lc = last_cached,
        lo = last_output,
        lr = last_reasoning,
        lt = reported_last_total,
    )
}

fn codex_state_default(ts: &str, ci: i64, cc: i64, co: i64, li: i64, lc: i64, lo: i64) -> String {
    codex_state_line(ts, ci, cc, co, 0, li, lc, lo, 0, None, None)
}

fn forked_session_meta(ts: &str) -> String {
    format!(
        r#"{{"type":"session_meta","timestamp":"{ts}","payload":{{"id":"child","forked_from_id":"parent","parent_thread_id":"parent","thread_source":"user"}}}}"#
    )
}

fn forked_session_meta_2(id: &str, parent_id: &str, ts: &str) -> String {
    format!(
        r#"{{"type":"session_meta","timestamp":"{ts}","payload":{{"id":"{id}","session_id":"{parent_id}","forked_from_id":"{parent_id}","parent_thread_id":"{parent_id}","thread_source":"subagent"}}}}"#
    )
}

fn padded_codex_session_meta(id: &str, total_bytes: usize, tail: &str) -> String {
    let head = format!(
        "{{\"type\":\"session_meta\",\"timestamp\":\"2026-07-29T01:00:00.000Z\",\"payload\":{{\"id\":\"{id}\",\"session_id\":\"{id}\",\"base_instructions\":\""
    );
    let close = "\"}}";
    let pad = total_bytes as i64 - head.len() as i64 - close.len() as i64 - tail.len() as i64;
    assert!(pad > 0, "padding must be positive");
    format!("{head}{}{tail}{close}", "a".repeat(pad as usize))
}

const CODEX_FORK_CHILD: &str = include_str!("../../../tests/fixtures/CodexFork/child.jsonl");
const CODEX_FORK_PARENT: &str = include_str!("../../../tests/fixtures/CodexFork/parent.jsonl");
const CODEX_FORK_SIBLING: &str = include_str!("../../../tests/fixtures/CodexFork/sibling.jsonl");
const CODEX_SUBAGENT_CHILD: &str =
    include_str!("../../../tests/fixtures/CodexSubagent/child.jsonl");
const CODEX_SUBAGENT_CHILD_V145: &str =
    include_str!("../../../tests/fixtures/CodexSubagent/child-v145.jsonl");
const CODEX_SUBAGENT_PARENT: &str =
    include_str!("../../../tests/fixtures/CodexSubagent/parent.jsonl");
const CODEX_SUBAGENT_PARENT_V145: &str =
    include_str!("../../../tests/fixtures/CodexSubagent/parent-v145.jsonl");

// MARK: - Claude parsing + dedup (keep-max) + dates

#[test]
fn claude_dedup_keeps_max_output() {
    let dir = temp_dir();
    let ts = "2026-06-30T10:00:00.000Z";
    // Same (id, req) streamed twice: output 5 → 200, cacheRead fixed at 1000.
    write_lines(
        &dir,
        &[
            claude_line("A", "R1", "claude-opus-4-8", ts, 100, 5, 0, 1000),
            claude_line("A", "R1", "claude-opus-4-8", ts, 100, 200, 0, 1000),
            claude_line("B", "R2", "claude-sonnet-4-6", ts, 50, 10, 0, 0),
        ],
        "s.jsonl",
        Some("proj/sub"),
    );

    let entries = claude_entries_in_root(epoch(), &dir);
    assert_eq!(entries.len(), 2); // A (deduped), B
    let a = entries.iter().find(|e| e.id.starts_with("A|")).unwrap();
    assert_eq!(a.output, 200); // keep-max: the completed output
    assert_eq!(a.cache_read, 1000);
}

#[test]
fn claude_daily_and_cost() {
    let dir = temp_dir();
    let ts = "2026-06-30T10:00:00.000Z";
    let date = parse_iso8601(ts).unwrap();
    let day = local_day(date);
    write_lines(
        &dir,
        &[claude_line(
            "A",
            "R1",
            "claude-opus-4-8",
            ts,
            1_000_000,
            0,
            0,
            0,
        )],
        "s.jsonl",
        Some("p"),
    );
    let entries = claude_entries_in_root(epoch(), &dir);
    let d = daily(&entries, &day).unwrap();
    assert_eq!(d.total_tokens, 1_000_000);
    assert!((d.total_cost - 5.0).abs() < 1e-6); // opus input $5/Mtok
    assert!(daily(&entries, "2000-01-01").is_none());
}

#[test]
fn multiple_roots_sum_but_share_global_dedup() {
    let cli = temp_dir();
    let desktop = temp_dir();
    let ts = "2026-06-30T10:00:00.000Z";
    write_lines(
        &cli,
        &[claude_line("A", "R1", "claude-opus-4-8", ts, 100, 10, 0, 0)],
        "s.jsonl",
        Some("p"),
    );
    write_lines(
        &desktop,
        &[
            claude_line("A", "R1", "claude-opus-4-8", ts, 100, 10, 0, 0),
            claude_line("B", "R2", "claude-opus-4-8", ts, 7, 3, 0, 0),
        ],
        "s.jsonl",
        Some("p"),
    );

    let entries = claude_entries(epoch(), &[cli.clone(), desktop]);
    assert_eq!(entries.len(), 2); // A counted once
    let sum: i64 = entries.iter().map(Entry::total).sum();
    assert_eq!(sum, 110 + 10);

    // Control: without the Desktop root, B disappears — proves multi-root scan.
    assert_eq!(claude_entries(epoch(), &[cli]).len(), 1);
}

// MARK: - Scan roots (CLI default + CLAUDE_CONFIG_DIR + Desktop embedded)

#[test]
fn embedded_roots_find_hidden_claude_projects_dirs() {
    let base = temp_dir();
    let projects = base.join("2eb6d133/a3a236da/local_35a9f8a7/.claude/projects");
    std::fs::create_dir_all(&projects).unwrap();
    // Audit/upload logs are not usage-log roots.
    std::fs::create_dir_all(base.join("2eb6d133/a3a236da/local_35a9f8a7/uploads")).unwrap();

    let found = embedded_claude_project_roots(&base, 7);
    assert_eq!(found, vec![projects]);
}

#[test]
fn embedded_roots_ignore_missing_base_and_depth_limit() {
    let missing = temp_dir().join(format!("nonexistent-{}", uuid::Uuid::new_v4()));
    assert!(embedded_claude_project_roots(&missing, 7).is_empty());

    let base = temp_dir();
    std::fs::create_dir_all(base.join("a/b/c/d/e/f/g/.claude/projects")).unwrap();
    assert!(embedded_claude_project_roots(&base, 4).is_empty());
}

#[test]
fn embedded_roots_depth_boundary_matches_real_layout_with_headroom() {
    let base = temp_dir();
    let real = base.join("2eb6d133/a3a236da/local_35a9f8a7/.claude/projects");
    std::fs::create_dir_all(&real).unwrap();

    assert!(
        embedded_claude_project_roots(&base, 4).is_empty(),
        "depth-5 layout must not be found with maxDepth 4 (boundary check)"
    );
    assert_eq!(embedded_claude_project_roots(&base, 5).len(), 1);
    assert_eq!(embedded_claude_project_roots(&base, 7).len(), 1);

    // A repo under a session workdir (depth 7) is found by the default; 6 misses it.
    let nested = base.join("2eb6d133/a3a236da/local_35a9f8a7/outputs/myrepo/.claude/projects");
    std::fs::create_dir_all(&nested).unwrap();
    assert_eq!(embedded_claude_project_roots(&base, 7).len(), 2);
    assert_eq!(
        embedded_claude_project_roots(&base, 6).len(),
        1,
        "depth 6 misses workdir-repo roots"
    );
}

#[test]
fn embedded_roots_do_not_descend_into_bulk_directories() {
    let base = temp_dir();
    let session = base.join("s1/s2/local_x");
    std::fs::create_dir_all(session.join(".claude/projects")).unwrap();
    // A `.claude/projects` shape inside node_modules must not be followed.
    std::fs::create_dir_all(session.join("node_modules/pkg/.claude/projects")).unwrap();

    let found = embedded_claude_project_roots(&base, 7);
    assert_eq!(found.len(), 1);
    assert!(!found[0].to_string_lossy().contains("node_modules"));
}

#[test]
fn embedded_roots_find_roots_under_work_directory_names() {
    let base = temp_dir();
    let session = base.join("u1/u2/local_x");
    for work in ["outputs/myrepo", "uploads/repo2", "build", "target"] {
        std::fs::create_dir_all(session.join(format!("{work}/.claude/projects"))).unwrap();
    }
    let found: Vec<String> = embedded_claude_project_roots(&base, 7)
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    for work in ["outputs", "uploads", "build", "target"] {
        assert!(
            found.iter().any(|p| {
                let norm = p.replace('\\', "/");
                norm.contains(&format!("/{work}/")) || norm.contains(&format!("/{work}/.claude"))
            }),
            "{work} workdir root was pruned"
        );
    }
}

#[test]
fn config_dir_parsing_handles_commas_whitespace_and_tilde() {
    let home = Path::new("/Users/testhome");
    let roots = compute_claude_project_roots(Some(" /a/one , ,~/two "), home);
    assert!(roots.contains(&PathBuf::from("/a/one/projects")));
    assert!(roots.contains(&home.join("two/projects"))); // ~ expands to home
    assert!(!roots.iter().any(|r| r == &PathBuf::from("/projects")));
    assert!(roots.contains(&home.join(".claude/projects")));
    assert!(roots.contains(&home.join(".config/claude/projects")));

    let none = compute_claude_project_roots(None, home);
    assert!(!none
        .iter()
        .any(|r| r.to_string_lossy().starts_with("/a/one")));
}

#[test]
fn normalized_roots_fold_symlinked_duplicates() {
    #[cfg(unix)]
    {
        let base = temp_dir();
        let real = base.join("real/projects");
        std::fs::create_dir_all(&real).unwrap();
        let link = base.join("linked");
        std::os::unix::fs::symlink(base.join("real"), &link).unwrap();

        let folded = normalized_roots(&[real.clone(), link.join("projects")]);
        assert_eq!(folded.len(), 1, "symlinked duplicate roots must fold");
    }
}

#[test]
fn normalized_roots_drops_duplicates_and_nested_roots_keeping_order() {
    let roots = [
        PathBuf::from("/Users/x/.claude/projects"),
        PathBuf::from("/Users/x/.config/claude/projects"),
        PathBuf::from("/Users/x/.claude/projects"), // full duplicate
        PathBuf::from("/Users/x/.claude/projects/sub"), // nested
        PathBuf::from("/Users/x/.claude/projects-other"), // prefix only — kept
    ];
    let actual: Vec<String> = normalized_roots(&roots)
        .into_iter()
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .collect();
    assert_eq!(
        actual,
        vec![
            "/Users/x/.claude/projects".to_string(),
            "/Users/x/.config/claude/projects".to_string(),
            "/Users/x/.claude/projects-other".to_string(),
        ]
    );
}

#[test]
fn default_roots_contain_cli_path_and_are_unique() {
    let home = Path::new("/Users/testhome");
    let roots = compute_claude_project_roots(None, home);
    assert!(roots.contains(&home.join(default_relative_projects_path())));
    let mut seen = HashSet::new();
    assert!(roots.iter().all(|r| seen.insert(r.clone())));
}

#[test]
fn default_projects_path_has_single_source() {
    let p = claude_projects_dir(Path::new("/Users/x"));
    let norm = p.to_string_lossy().replace('\\', "/");
    assert!(norm.ends_with(&format!("/{}", default_relative_projects_path())));
}

// MARK: - Codex parsing

#[test]
fn codex_parsing() {
    let dir = temp_dir();
    let line = codex_line_default("2026-06-30T11:00:00.000Z");
    write_lines(&dir, &[line], "rollout-x.jsonl", Some("2026/06/30"));
    let entries = codex_entries_in(&dir);
    assert_eq!(entries.len(), 1);
    let e = &entries[0];
    assert_eq!(e.input, 800); // 1000 - 200
    assert_eq!(e.cache_read, 200);
    assert_eq!(e.output, 50);
    assert_eq!(e.cache_write, 0);
}

#[test]
fn codex_non_fork_resolver_preserves_parsed_entries_except_canonical_ids() {
    let dir = temp_dir();
    let path = write_lines(
        &dir,
        &[
            codex_session_meta_line("session-a", "2026-07-29T01:00:00.000Z"),
            codex_state_default("2026-07-29T01:00:01.000Z", 100, 20, 10, 100, 20, 10),
            codex_state_default("2026-07-29T01:00:02.000Z", 300, 120, 30, 200, 100, 20),
            codex_state_default("2026-07-29T01:00:03.000Z", 450, 170, 45, 150, 50, 15),
        ],
        "rollout.jsonl",
        None,
    );
    let rollout = parse_codex_rollout(&path, LOCAL_DAY_FORMAT);
    let parsed: Vec<Entry> = rollout.events.iter().map(|e| e.entry.clone()).collect();
    assert_eq!(parsed.len(), 3);

    let resolved =
        resolve_codex_rollouts(vec![rollout.clone()], HashSet::from([rollout.path.clone()]));
    assert_eq!(resolved.len(), parsed.len());
    for (before, after) in parsed.iter().zip(&resolved) {
        assert_ne!(after.id, before.id);
        assert!(after.id.starts_with("codex|session-a|0|"));
        assert_eq!(after.date, before.date);
        assert_eq!(after.local_day, before.local_day);
        assert_eq!(after.model, before.model);
        assert_eq!(after.input, before.input);
        assert_eq!(after.output, before.output);
        assert_eq!(after.cache_write, before.cache_write);
        assert_eq!(after.cache_read, before.cache_read);
        assert_eq!(after.explicit_cost, before.explicit_cost);
    }
}

#[test]
fn codex_drops_consecutive_same_state_rerecords_and_matches_cumulative_total() {
    let dir = temp_dir();
    write_lines(
        &dir,
        &[
            codex_session_meta_line("session-a", "2026-07-29T01:00:00.000Z"),
            codex_state_default("2026-07-29T01:00:01.000Z", 100, 20, 10, 100, 20, 10),
            // Plain re-record of the same snapshot.
            codex_state_default("2026-07-29T01:00:02.000Z", 100, 20, 10, 100, 20, 10),
            codex_state_default("2026-07-29T01:00:03.000Z", 300, 120, 30, 200, 100, 20),
            // Re-recorded session_meta keeps the token_count state continuity.
            codex_session_meta_line("session-a", "2026-07-29T01:00:04.000Z"),
            codex_state_default("2026-07-29T01:00:05.000Z", 300, 120, 30, 200, 100, 20),
        ],
        "s.jsonl",
        None,
    );

    let entries = codex_entries_in(&dir);
    let totals: Vec<i64> = entries.iter().map(Entry::total).collect();
    assert_eq!(totals, vec![110, 220]);
    assert_eq!(totals.iter().sum::<i64>(), 330);
}

#[test]
fn codex_same_scalar_totals_with_different_full_vectors_are_preserved() {
    let dir = temp_dir();
    write_lines(
        &dir,
        &[
            codex_session_meta_line("session-a", "2026-07-29T01:00:00.000Z"),
            codex_state_default("2026-07-29T01:00:01.000Z", 100, 20, 10, 100, 20, 10),
            // Same cumulative/last totals (110 each) but different composition.
            codex_state_default("2026-07-29T01:00:02.000Z", 90, 10, 20, 90, 10, 20),
        ],
        "s.jsonl",
        None,
    );

    let entries = codex_entries_in(&dir);
    assert_eq!(entries.len(), 2);
    let totals: Vec<i64> = entries.iter().map(Entry::total).collect();
    assert_eq!(totals, vec![110, 110]);
}

#[test]
fn codex_unchanged_cumulative_with_different_last_vector_is_preserved() {
    let dir = temp_dir();
    write_lines(
        &dir,
        &[
            codex_session_meta_line("session-a", "2026-07-29T01:00:00.000Z"),
            codex_state_default("2026-07-29T01:00:01.000Z", 100, 0, 10, 100, 0, 10),
            // Post-replay fixture shape: same cumulative but `last.total_tokens`
            // only is non-zero — not an identical snapshot.
            codex_state_line(
                "2026-07-29T01:00:02.000Z",
                100,
                0,
                10,
                0,
                0,
                0,
                0,
                0,
                Some(6_742),
                None,
            ),
        ],
        "s.jsonl",
        None,
    );

    let entries = codex_entries_in(&dir);
    let totals: Vec<i64> = entries.iter().map(Entry::total).collect();
    assert_eq!(totals, vec![110, 0]);
}

#[test]
fn codex_session_change_resets_same_state_comparison() {
    let dir = temp_dir();
    let state_a = codex_state_default("2026-07-29T01:00:01.000Z", 100, 0, 10, 100, 0, 10);
    let state_b = codex_state_default("2026-07-29T01:00:03.000Z", 100, 0, 10, 100, 0, 10);
    write_lines(
        &dir,
        &[
            codex_session_meta_line("session-a", "2026-07-29T01:00:00.000Z"),
            state_a,
            codex_session_meta_line("session-b", "2026-07-29T01:00:02.000Z"),
            state_b,
        ],
        "s.jsonl",
        None,
    );

    let entries = codex_entries_in(&dir);
    let totals: Vec<i64> = entries.iter().map(Entry::total).collect();
    assert_eq!(totals, vec![110, 110]);
}

#[test]
fn codex_missing_cumulative_usage_preserves_repeated_records() {
    let dir = temp_dir();
    write_lines(
        &dir,
        &[
            codex_session_meta_line("session-a", "2026-07-29T01:00:00.000Z"),
            codex_line_default("2026-07-29T01:00:01.000Z"),
            codex_line_default("2026-07-29T01:00:02.000Z"),
        ],
        "s.jsonl",
        None,
    );

    let entries = codex_entries_in(&dir);
    assert_eq!(entries.len(), 2);
}

#[test]
fn codex_manual_fork_falls_back_when_parent_usage_state_is_unavailable() {
    let dir = temp_dir();
    write_lines(
        &dir,
        &[
            codex_session_meta_line("parent", "2026-07-29T01:00:00.000Z"),
            codex_line("2026-07-29T01:00:00.010Z", 1_000, 200, 50, 10, 0),
            codex_line("2026-07-29T01:00:00.020Z", 1_000, 200, 51, 10, 0),
        ],
        "parent.jsonl",
        None,
    );
    write_lines(
        &dir,
        &[
            forked_session_meta("2026-07-30T01:00:00.000Z"),
            codex_session_meta_line("parent", "2026-07-30T01:00:00.001Z"),
            // Old replay without total_token_usage cannot be structurally
            // compared — timing fallback applies.
            codex_line("2026-07-30T01:00:03.000Z", 1_000, 200, 50, 10, 0),
            codex_line("2026-07-30T01:00:03.010Z", 1_000, 200, 51, 10, 0),
            codex_line("2026-07-30T01:00:06.000Z", 1_000, 200, 99, 10, 0),
        ],
        "child.jsonl",
        None,
    );

    let entries = codex_entries_in(&dir);
    let mut outputs: Vec<i64> = entries.iter().map(|e| e.output).collect();
    outputs.sort();
    assert_eq!(outputs, vec![50, 51, 99]);
}

#[test]
fn codex_manual_fork_falls_back_when_found_parent_prefix_does_not_match() {
    let dir = temp_dir();
    write_lines(
        &dir,
        &[
            codex_session_meta_line("parent", "2026-07-29T01:00:00.000Z"),
            codex_state_default("2026-07-29T01:00:01.000Z", 100, 0, 10, 100, 0, 10),
            codex_state_default("2026-07-29T01:00:02.000Z", 300, 0, 30, 200, 0, 20),
        ],
        "parent.jsonl",
        None,
    );
    write_lines(
        &dir,
        &[
            forked_session_meta("2026-07-30T01:00:00.000Z"),
            codex_session_meta_line("parent", "2026-07-30T01:00:00.001Z"),
            // Same two parent turns replayed but a newer CLI writes
            // cache_write — the vector differs from the parent's.
            codex_state_line(
                "2026-07-30T01:00:00.010Z",
                100,
                0,
                10,
                0,
                100,
                0,
                10,
                0,
                None,
                Some(7),
            ),
            codex_state_line(
                "2026-07-30T01:00:00.020Z",
                300,
                0,
                30,
                0,
                200,
                0,
                20,
                0,
                None,
                Some(7),
            ),
            // The child's own real turn (1s+ after the replay burst).
            codex_state_line(
                "2026-07-30T01:00:03.000Z",
                1_300,
                0,
                128,
                0,
                1_000,
                0,
                98,
                0,
                None,
                Some(7),
            ),
        ],
        "child.jsonl",
        None,
    );

    let entries = codex_entries_in(&dir);
    let mut totals: Vec<i64> = entries.iter().map(Entry::total).collect();
    totals.sort();
    assert_eq!(totals, vec![110, 220, 1_098]);
}

#[test]
fn codex_cumulative_usage_clamps_out_of_range_number() {
    let dir = temp_dir();
    let absurd = r#"{"type":"event_msg","timestamp":"2026-07-30T01:00:01.000Z","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":1e30,"cached_input_tokens":0,"output_tokens":10,"total_tokens":1e30},"last_token_usage":{"input_tokens":100,"cached_input_tokens":0,"output_tokens":10,"total_tokens":110}}}}"#;
    write_lines(
        &dir,
        &[
            codex_session_meta_line("huge", "2026-07-30T01:00:00.000Z"),
            absurd.to_string(),
        ],
        "rollout-huge.jsonl",
        None,
    );

    let entries = codex_entries_in(&dir);
    let totals: Vec<i64> = entries.iter().map(Entry::total).collect();
    assert_eq!(
        totals,
        vec![110],
        "last usage intact, only cumulative clamped"
    );
}

#[test]
fn codex_fork_trims_replay_before_dropping_actual_same_state_rerecord() {
    let dir = temp_dir();
    write_lines(
        &dir,
        &[
            forked_session_meta("2026-07-29T01:00:00.000Z"),
            codex_state_default("2026-07-29T01:00:00.010Z", 100, 0, 10, 100, 0, 10),
            codex_state_default("2026-07-29T01:00:03.000Z", 300, 0, 30, 200, 0, 20),
            codex_state_default("2026-07-29T01:00:04.000Z", 300, 0, 30, 200, 0, 20),
        ],
        "rollout-child.jsonl",
        Some("child"),
    );

    let entries = codex_entries_in(&dir);
    let totals: Vec<i64> = entries.iter().map(Entry::total).collect();
    assert_eq!(totals, vec![220]);
}

#[test]
fn codex_forked_rollout_drops_leading_replay_burst() {
    let dir = temp_dir();
    write_lines(
        &dir,
        &[
            forked_session_meta("2026-07-29T01:00:00.000Z"),
            codex_line("2026-07-29T01:00:00.010Z", 1_000, 200, 50, 10, 0),
            codex_line("2026-07-29T01:00:00.020Z", 1_000, 200, 51, 10, 0),
            codex_line("2026-07-29T01:00:03.000Z", 1_000, 200, 52, 10, 0),
        ],
        "rollout-child.jsonl",
        Some("child"),
    );

    let entries = codex_entries_in(&dir);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].output, 52);
}

#[test]
fn codex_fork_drops_replay_burst_that_starts_after_metadata_delay() {
    let dir = temp_dir();
    write_lines(
        &dir,
        &[
            forked_session_meta("2026-07-29T01:00:00.000Z"),
            codex_line("2026-07-29T01:00:03.000Z", 1_000, 200, 1, 10, 0),
            codex_line("2026-07-29T01:00:03.010Z", 1_000, 200, 2, 10, 0),
            codex_line("2026-07-29T01:00:03.020Z", 1_000, 200, 3, 10, 0),
            codex_line("2026-07-29T01:00:43.000Z", 1_000, 200, 99, 10, 0),
        ],
        "rollout-child.jsonl",
        Some("child"),
    );

    let entries = codex_entries_in(&dir);
    let outputs: Vec<i64> = entries.iter().map(|e| e.output).collect();
    assert_eq!(outputs, vec![99]);
}

#[test]
fn codex_fork_keeps_real_turns_after_replay_burst_when_they_are_less_than_two_seconds_apart() {
    let dir = temp_dir();
    write_lines(
        &dir,
        &[
            forked_session_meta("2026-07-29T01:00:00.000Z"),
            codex_line("2026-07-29T01:00:00.010Z", 1_000, 200, 1, 10, 0),
            codex_line("2026-07-29T01:00:00.020Z", 1_000, 200, 2, 10, 0),
            codex_line("2026-07-29T01:00:00.030Z", 1_000, 200, 3, 10, 0),
            codex_line("2026-07-29T01:00:01.530Z", 1_000, 200, 11, 10, 0),
            codex_line("2026-07-29T01:00:03.030Z", 1_000, 200, 22, 10, 0),
            codex_line("2026-07-29T01:00:04.530Z", 1_000, 200, 33, 10, 0),
            codex_line("2026-07-29T01:01:00.000Z", 1_000, 200, 44, 10, 0),
        ],
        "rollout-child.jsonl",
        Some("child"),
    );

    let entries = codex_entries_in(&dir);
    let outputs: Vec<i64> = entries.iter().map(|e| e.output).collect();
    assert_eq!(outputs, vec![11, 22, 33, 44]);
}

#[test]
fn codex_fork_detects_metadata_after_leading_non_token_record() {
    let dir = temp_dir();
    write_lines(
        &dir,
        &[
            r#"{"type":"turn_context","timestamp":"2026-07-29T01:00:00.000Z","payload":{}}"#
                .to_string(),
            forked_session_meta("2026-07-29T01:00:00.001Z"),
            codex_line("2026-07-29T01:00:00.010Z", 1_000, 200, 1, 10, 0),
            codex_line("2026-07-29T01:00:03.000Z", 1_000, 200, 99, 10, 0),
        ],
        "rollout-child.jsonl",
        Some("child"),
    );

    let entries = codex_entries_in(&dir);
    let outputs: Vec<i64> = entries.iter().map(|e| e.output).collect();
    assert_eq!(outputs, vec![99]);
}

// MARK: - Codex metadata probe

#[test]
fn codex_probe_reads_session_id_when_metadata_line_exceeds_chunk() {
    let url = write_probe_file(&[
        padded_codex_session_meta("parent-xl", 200_000, ""),
        codex_line_default("2026-07-29T01:00:01.000Z"),
    ]);
    let bytes = std::fs::read(&url).unwrap();
    assert!(
        !bytes[..64 * 1024].contains(&b'\n'),
        "a fixed-prefix decode would fail without a complete line in the first 64KB"
    );
    assert_eq!(codex_rollout_session_id(&url).as_deref(), Some("parent-xl"));
}

#[test]
fn codex_probe_decodes_multibyte_straddling_chunk_boundary() {
    let boundary = 64 * 1024;
    let meta = padded_codex_session_meta("parent-utf8", boundary + 4, "가");
    let bytes = meta.as_bytes();
    assert!(
        (0x80..=0xBF).contains(&bytes[boundary]),
        "boundary byte must be a UTF-8 continuation byte or the test misses the regression"
    );
    assert!(
        std::str::from_utf8(&bytes[..boundary]).is_err(),
        "a fixed-prefix decode must fail on this boundary"
    );

    let url = write_probe_file(&[meta, codex_line_default("2026-07-29T01:00:01.000Z")]);
    assert_eq!(
        codex_rollout_session_id(&url).as_deref(),
        Some("parent-utf8")
    );
}

#[test]
fn codex_probe_scans_many_lines_across_chunks() {
    let mut lines: Vec<String> = (0..2000)
        .map(|i| format!(r#"{{"type":"response_item","seq":{i},"payload":{{"text":"filler-filler-filler"}}}}"#))
        .collect();
    lines.push(codex_session_meta_line(
        "parent-after-many-lines",
        "2026-07-29T01:00:00.000Z",
    ));
    let url = write_probe_file(&lines);
    assert_eq!(
        codex_rollout_session_id(&url).as_deref(),
        Some("parent-after-many-lines")
    );
}

#[test]
fn codex_probe_stops_at_byte_limit() {
    let url = write_probe_file(&[padded_codex_session_meta("parent-capped", 4_096, "")]);
    assert_eq!(
        probe_codex_rollout_session_id(&url, 1_024).ok().flatten(),
        None,
        "line not finished within the limit → nil"
    );
    assert_eq!(
        probe_codex_rollout_session_id(&url, 4_096)
            .ok()
            .flatten()
            .as_deref(),
        Some("parent-capped"),
        "a newline-less metadata exactly at the limit counts as a final line"
    );
    assert_eq!(
        codex_rollout_session_id(&url).as_deref(),
        Some("parent-capped"),
        "default limit (1 MiB) reads the same file"
    );
}

#[test]
fn codex_probe_stops_at_invalid_utf8_before_session_meta() {
    let url = temp_dir().join("rollout-invalid-utf8.jsonl");
    let mut data = vec![0xFFu8, 0x0Au8];
    data.extend_from_slice(
        codex_session_meta_line("wrong-parent", "2026-07-29T01:00:00.001Z").as_bytes(),
    );
    std::fs::write(&url, data).unwrap();

    assert_eq!(codex_rollout_session_id(&url), None);
}

#[test]
fn codex_probe_stops_at_token_count_before_session_meta() {
    let url = write_probe_file(&[
        codex_line_default("2026-07-29T01:00:00.000Z"),
        codex_session_meta_line("too-late", "2026-07-29T01:00:01.000Z"),
    ]);
    assert_eq!(codex_rollout_session_id(&url), None);
}

#[test]
fn codex_probe_finds_metadata_after_leading_non_token_record() {
    let url = write_probe_file(&[
        r#"{"type":"turn_context","timestamp":"2026-07-29T01:00:00.000Z","payload":{}}"#
            .to_string(),
        codex_session_meta_line("parent-late", "2026-07-29T01:00:00.001Z"),
    ]);
    assert_eq!(
        codex_rollout_session_id(&url).as_deref(),
        Some("parent-late")
    );
}

// MARK: - Codex fixtures (fork / subagent)

#[test]
fn codex_manual_fork_fixture_keeps_only_post_replay_usage() {
    let dir = temp_dir();
    let child = write_str(&dir.join("child.jsonl"), CODEX_FORK_CHILD);
    let entries = parse_codex_file(&child, LOCAL_DAY_FORMAT);

    // Real `codex fork` file: child meta (forked_from_id only) followed by the
    // parent meta and 8 replayed parent token_count records. The zero-token
    // event after the replay is kept; only the new turn is aggregated.
    let totals: Vec<i64> = entries.iter().map(Entry::total).collect();
    assert_eq!(totals, vec![0, 28_138]);
}

#[test]
fn codex_manual_fork_fixture_keeps_parent_and_child_usage_on_their_own_days() {
    let dir = temp_dir();
    let parent = write_str(&dir.join("parent.jsonl"), CODEX_FORK_PARENT);
    let _child = write_str(&dir.join("child.jsonl"), CODEX_FORK_CHILD);
    let parent_entries = parse_codex_file(&parent, LOCAL_DAY_FORMAT);
    let parent_day = parent_entries[0].local_day.clone();
    let entries = codex_entries_in(&dir);
    let child_day = entries
        .iter()
        .find(|e| e.total() == 28_138)
        .map(|e| e.local_day.clone())
        .unwrap();

    assert_eq!(daily(&entries, &parent_day).unwrap().total_tokens, 312_814);
    assert_eq!(daily(&entries, &child_day).unwrap().total_tokens, 28_138);
    assert_eq!(
        period(&entries, "fixture", &parent_day, &child_day).total_tokens,
        340_952
    );
    let mut ids = HashSet::new();
    assert!(entries.iter().all(|e| ids.insert(e.id.clone())));
}

#[test]
fn codex_sibling_fork_fixtures_keep_independent_post_replay_usage() {
    let dir = temp_dir();
    write_str(&dir.join("parent.jsonl"), CODEX_FORK_PARENT);
    write_str(&dir.join("child.jsonl"), CODEX_FORK_CHILD);
    write_str(&dir.join("sibling.jsonl"), CODEX_FORK_SIBLING);

    let entries = codex_entries_in(&dir);
    let mut fork_totals: Vec<i64> = entries
        .iter()
        .map(Entry::total)
        .filter(|t| *t == 28_138 || *t == 28_263)
        .collect();
    fork_totals.sort();
    assert_eq!(fork_totals, vec![28_138, 28_263]);
    let sum: i64 = entries.iter().map(Entry::total).sum();
    assert_eq!(sum, 369_215);
}

#[test]
fn codex_subagent_fixtures_keep_all_own_usage_without_replay_prefix() {
    let fixtures = [
        (
            CODEX_SUBAGENT_PARENT,
            CODEX_SUBAGENT_CHILD,
            vec![22_992, 23_043, 23_062, 23_219, 23_291],
            115_607i64,
            "00000000-0000-7000-8000-000000000002",
        ),
        (
            CODEX_SUBAGENT_PARENT_V145,
            CODEX_SUBAGENT_CHILD_V145,
            vec![20_863, 21_175, 21_365, 21_458, 21_722],
            106_583i64,
            "00000000-0000-7000-8000-000000000146",
        ),
    ];
    for (parent_fixture, child_fixture, totals, combined, child_id) in fixtures {
        let dir = temp_dir();
        write_str(&dir.join("parent.jsonl"), parent_fixture);
        write_str(&dir.join("child.jsonl"), child_fixture);

        let entries = codex_entries_in(&dir);
        let mut entry_totals: Vec<i64> = entries.iter().map(Entry::total).collect();
        entry_totals.sort();
        assert_eq!(entry_totals, totals, "{child_fixture}");
        let sum: i64 = entries.iter().map(Entry::total).sum();
        assert_eq!(sum, combined, "{child_fixture}");
        assert_eq!(
            entries
                .iter()
                .filter(|e| e.id.starts_with(&format!("codex|{child_id}|")))
                .count(),
            2,
            "{child_fixture}"
        );
    }
}

#[test]
fn codex_subagent_child_fixtures_keep_first_turn_when_parent_is_missing() {
    let fixtures = [
        (
            CODEX_SUBAGENT_CHILD,
            "00000000-0000-7000-8000-000000000002",
            "00000000-0000-7000-8000-000000000001",
            vec![23_062, 23_291],
        ),
        (
            CODEX_SUBAGENT_CHILD_V145,
            "00000000-0000-7000-8000-000000000146",
            "00000000-0000-7000-8000-000000000145",
            vec![21_458, 21_722],
        ),
    ];
    for (fixture, child_id, parent_id, totals) in fixtures {
        let dir = temp_dir();
        let child = write_str(&dir.join("child.jsonl"), fixture);
        let rollout = parse_codex_rollout(&child, LOCAL_DAY_FORMAT);
        assert_eq!(rollout.session_id.as_deref(), Some(child_id), "{fixture}");
        assert_eq!(
            rollout.parent_session_id.as_deref(),
            Some(parent_id),
            "{fixture}"
        );
        assert!(rollout.is_subagent, "{fixture}");
        let entries = parse_codex_file(&child, LOCAL_DAY_FORMAT);
        let entry_totals: Vec<i64> = entries.iter().map(Entry::total).collect();
        assert_eq!(entry_totals, totals, "{fixture}");
        assert!(
            entries
                .iter()
                .all(|e| e.id.starts_with(&format!("codex|{child_id}|"))),
            "{fixture}"
        );
    }
}

#[test]
fn codex_fork_of_fork_reuses_resolved_ancestor_history() {
    let dir = temp_dir();
    let first = codex_state_default("2026-07-30T01:00:01.000Z", 100, 0, 10, 100, 0, 10);
    let second = codex_state_default("2026-07-30T01:00:02.000Z", 200, 0, 20, 100, 0, 10);
    let third = codex_state_default("2026-07-30T01:00:03.000Z", 300, 0, 30, 100, 0, 10);
    let fourth = codex_state_default("2026-07-30T01:00:04.000Z", 400, 0, 40, 100, 0, 10);

    write_lines(
        &dir,
        &[
            codex_session_meta_line("root", "2026-07-30T01:00:00.000Z"),
            first.clone(),
            second.clone(),
        ],
        "root.jsonl",
        None,
    );
    write_lines(
        &dir,
        &[
            forked_session_meta_2("child", "root", "2026-07-30T02:00:00.000Z"),
            codex_session_meta_line("root", "2026-07-30T02:00:00.001Z"),
            first.clone(),
            second.clone(),
            third.clone(),
        ],
        "child.jsonl",
        None,
    );
    write_lines(
        &dir,
        &[
            forked_session_meta_2("grandchild", "child", "2026-07-30T03:00:00.000Z"),
            codex_session_meta_line("child", "2026-07-30T03:00:00.001Z"),
            first,
            second,
            third,
            fourth,
        ],
        "grandchild.jsonl",
        None,
    );

    let entries = codex_entries_in(&dir);
    let totals: Vec<i64> = entries.iter().map(Entry::total).collect();
    assert_eq!(totals, vec![110, 110, 110, 110]);
    assert_eq!(
        entries
            .iter()
            .filter(|e| e.id.starts_with("codex|root|"))
            .count(),
        2
    );
    assert_eq!(
        entries
            .iter()
            .filter(|e| e.id.starts_with("codex|child|"))
            .count(),
        1
    );
    assert_eq!(
        entries
            .iter()
            .filter(|e| e.id.starts_with("codex|grandchild|"))
            .count(),
        1
    );
}

#[test]
fn codex_sibling_forks_with_identical_own_usage_keep_distinct_ids() {
    let dir = temp_dir();
    let replay = codex_state_default("2026-07-30T01:00:01.000Z", 100, 0, 10, 100, 0, 10);
    let own = codex_state_default("2026-07-30T02:00:01.000Z", 200, 0, 20, 100, 0, 10);
    write_lines(
        &dir,
        &[
            codex_session_meta_line("root", "2026-07-30T01:00:00.000Z"),
            replay.clone(),
        ],
        "root.jsonl",
        None,
    );
    for child_id in ["left", "right"] {
        write_lines(
            &dir,
            &[
                forked_session_meta_2(child_id, "root", "2026-07-30T02:00:00.000Z"),
                codex_session_meta_line("root", "2026-07-30T02:00:00.001Z"),
                replay.clone(),
                own.clone(),
            ],
            &format!("{child_id}.jsonl"),
            None,
        );
    }

    let entries = codex_entries_in(&dir);
    assert_eq!(entries.len(), 3);
    let mut ids = HashSet::new();
    assert!(entries.iter().all(|e| ids.insert(e.id.clone())));
    assert_eq!(
        entries
            .iter()
            .filter(|e| e.id.starts_with("codex|left|"))
            .count(),
        1
    );
    assert_eq!(
        entries
            .iter()
            .filter(|e| e.id.starts_with("codex|right|"))
            .count(),
        1
    );
}

#[test]
fn codex_cumulative_reset_starts_new_canonical_epoch() {
    let dir = temp_dir();
    write_lines(
        &dir,
        &[
            codex_session_meta_line("session-a", "2026-07-30T01:00:00.000Z"),
            codex_state_default("2026-07-30T01:00:01.000Z", 100, 0, 10, 100, 0, 10),
            codex_state_default("2026-07-30T01:00:02.000Z", 10, 0, 1, 10, 0, 1),
        ],
        "s.jsonl",
        None,
    );

    let entries = codex_entries_in(&dir);
    assert_eq!(entries.len(), 2);
    assert!(entries[0].id.starts_with("codex|session-a|0|"));
    assert!(entries[1].id.starts_with("codex|session-a|1|"));
}

#[test]
fn codex_canonical_id_collapses_same_session_state_across_files_keeping_earliest_date() {
    let dir = temp_dir();
    for (name, timestamp) in [
        ("later.jsonl", "2026-07-30T02:00:00.000Z"),
        ("earlier.jsonl", "2026-07-30T01:00:00.000Z"),
    ] {
        write_lines(
            &dir,
            &[
                codex_session_meta_line("session-a", timestamp),
                codex_state_default(timestamp, 100, 0, 10, 100, 0, 10),
            ],
            name,
            None,
        );
    }

    let entries = codex_entries_in(&dir);
    assert_eq!(entries.len(), 1);
    assert_eq!(
        entries[0].date,
        parse_iso8601("2026-07-30T01:00:00.000Z").unwrap()
    );
}

#[test]
fn degenerate_parent_hint_is_not_used_to_narrow_candidates() {
    assert!(
        !is_usable_filename_hint("-"),
        "a single separator matches every filename"
    );
    assert!(!is_usable_filename_hint(""));
    assert!(
        !is_usable_filename_hint("----"),
        "no alphanumerics → no hint value"
    );
    assert!(is_usable_filename_hint("parent"));
    assert!(is_usable_filename_hint(
        "00000000-0000-7000-8000-000000000001"
    ));
}

#[test]
fn degenerate_parent_hint_still_resolves_usage_correctly() {
    let dir = temp_dir();
    write_lines(
        &dir,
        &[
            r#"{"type":"session_meta","timestamp":"2026-07-29T01:00:00.000Z","payload":{"id":"child","forked_from_id":"-","thread_source":"user"}}"#
                .to_string(),
            codex_state_default("2026-07-29T01:00:00.010Z", 100, 0, 10, 100, 0, 10),
            codex_state_default("2026-07-29T01:00:05.000Z", 300, 0, 30, 200, 0, 20),
        ],
        "rollout-child.jsonl",
        Some("child"),
    );
    let entries = codex_entries_in(&dir);
    let totals: Vec<i64> = entries.iter().map(Entry::total).collect();
    assert_eq!(
        totals,
        vec![220],
        "parentless fork — existing timing trim result"
    );
}

#[test]
fn fork_replay_is_trimmed_against_the_parent_rollout() {
    let dir = temp_dir();
    write_lines(
        &dir,
        &[
            codex_session_meta_line("parent", "2026-07-29T01:00:00.000Z"),
            codex_state_default("2026-07-29T01:00:00.010Z", 100, 0, 10, 100, 0, 10),
        ],
        "rollout-parent.jsonl",
        Some("parent"),
    );
    write_lines(
        &dir,
        &[
            forked_session_meta("2026-07-29T02:00:00.000Z"),
            codex_state_default("2026-07-29T02:00:00.010Z", 100, 0, 10, 100, 0, 10),
            codex_state_default("2026-07-29T02:00:05.000Z", 300, 0, 30, 200, 0, 20),
        ],
        "rollout-child.jsonl",
        Some("child"),
    );
    let entries = codex_entries_in(&dir);
    let mut totals: Vec<i64> = entries.iter().map(Entry::total).collect();
    totals.sort();
    assert_eq!(
        totals,
        vec![110, 220],
        "parent replay trimmed by comparison"
    );
}

// MARK: - Parsing boundary clamps (crash class)

#[test]
fn claude_parsing_clamps_absurd_token_counts_instead_of_trapping() {
    let dir = temp_dir();
    let line = r#"{"type":"assistant","requestId":"r1","timestamp":"2026-06-30T10:00:00.000Z","message":{"id":"m1","model":"claude","usage":{"input_tokens":1e30,"output_tokens":1e30,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}"#;
    write_lines(&dir, &[line.to_string()], "a.jsonl", None);
    let entries = claude_entries_in_root(epoch(), &dir);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].input, MAX_PARSED_TOKEN_VALUE);
    assert_eq!(entries[0].output, MAX_PARSED_TOKEN_VALUE);
}

#[test]
fn codex_last_usage_clamps_absurd_token_counts_instead_of_trapping() {
    let dir = temp_dir();
    let line = r#"{"type":"event_msg","timestamp":"2026-07-29T01:00:00.000Z","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":10,"cached_input_tokens":0,"output_tokens":5,"reasoning_output_tokens":0,"total_tokens":15},"last_token_usage":{"input_tokens":1e30,"cached_input_tokens":0,"output_tokens":1e30,"reasoning_output_tokens":0,"total_tokens":1e30}}}}"#;
    write_lines(
        &dir,
        &[line.to_string()],
        "rollout-huge.jsonl",
        Some("huge"),
    );
    let entries = codex_entries_in(&dir);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].input, MAX_PARSED_TOKEN_VALUE);
}

#[test]
fn gemini_parsing_clamps_and_its_additions_stay_in_range() {
    let dir = temp_dir();
    let line = r#"{"id":"g1","timestamp":"2026-06-30T10:00:00.000Z","model":"gemini-2.5-pro","tokens":{"input":1e30,"cached":0,"tool":1e30,"output":1e30,"thoughts":1e30}}"#;
    write_lines(
        &dir,
        &[line.to_string()],
        "session-x.jsonl",
        Some("hash/chats"),
    );
    let entries = gemini_entries(epoch(), Some(&dir));
    assert_eq!(entries.len(), 1);
    let e = &entries[0];
    assert_eq!(
        e.input,
        MAX_PARSED_TOKEN_VALUE * 2,
        "input − cached + tool must sum without trapping"
    );
    assert_eq!(
        e.output,
        MAX_PARSED_TOKEN_VALUE * 2,
        "output + thoughts likewise"
    );
}

#[test]
fn parsing_still_folds_missing_and_negative_to_zero() {
    let dir = temp_dir();
    let line = r#"{"type":"assistant","requestId":"r2","timestamp":"2026-06-30T10:00:00.000Z","message":{"id":"m2","model":"claude","usage":{"input_tokens":-5,"output_tokens":null,"cache_read_input_tokens":"nope"}}}"#;
    write_lines(&dir, &[line.to_string()], "b.jsonl", None);
    let entries = claude_entries_in_root(epoch(), &dir);
    assert_eq!(entries[0].input, 0);
    assert_eq!(entries[0].output, 0);
    assert_eq!(entries[0].cache_read, 0);
}

#[test]
fn int_or_nil_clamps_and_folds() {
    assert_eq!(int_or_nil(&json!(5)), Some(5));
    assert_eq!(int_or_nil(&json!(5.7)), Some(5));
    assert_eq!(int_or_nil(&json!(-5)), Some(0));
    assert_eq!(int_or_nil(&json!(0)), Some(0));
    assert_eq!(int_or_nil(&json!(1e30)), Some(MAX_PARSED_TOKEN_VALUE));
    assert_eq!(int_or_nil(&json!(null)), None);
    assert_eq!(int_or_nil(&json!("5")), None);
    assert_eq!(int_or_nil(&json!(true)), None);
    assert_eq!(int_value(&json!(null)), 0);
    assert_eq!(int_value(&json!("x")), 0);
}

#[test]
fn dedup_keep_max_keeps_largest_total() {
    let e = |id: &str, total: i64| Entry {
        id: id.to_string(),
        date: epoch(),
        local_day: "2026-07-28".to_string(),
        model: "m".to_string(),
        input: total,
        output: 0,
        cache_write: 0,
        cache_read: 0,
        explicit_cost: None,
    };
    let deduped = dedup_keep_max(vec![e("A", 100), e("A", 250), e("B", 50)]);
    assert_eq!(deduped.len(), 2);
    let a = deduped.iter().find(|x| x.id == "A").unwrap();
    assert_eq!(a.total(), 250);
}

// MARK: - Gemini parsing

const NEW_JSONL: &str = r#"{"type":"session_metadata","sessionId":"s1","startTime":"2026-07-03T01:00:00.000Z"}
{"type":"user","id":"m1","timestamp":"2026-07-03T01:00:05.000Z","content":[{"text":"hi"}]}
{"type":"gemini","id":"m2","timestamp":"2026-07-03T01:00:10.000Z","model":"gemini-2.5-pro","tokens":{"input":1000,"output":50,"cached":600,"thoughts":30,"tool":20,"total":1100}}
{"type":"gemini","id":"m3","timestamp":"2026-07-03T01:01:00.000Z","model":"gemini-2.5-flash","tokens":{"input":10,"output":5,"cached":0,"thoughts":0,"tool":0,"total":15}}
{"type":"message_update","id":"m3","tokens":{"input":10,"output":8,"cached":0,"thoughts":2,"tool":0,"total":20}}"#;

const LEGACY_JSON: &str = r#"{"sessionId":"s0","startTime":"2026-07-02T00:00:00.000Z","messages":[
  {"id":"a1","type":"gemini","timestamp":"2026-07-02T00:10:00.000Z","model":"gemini-2.5-pro","tokens":{"input":100,"output":10,"cached":0,"thoughts":0,"tool":0,"total":110}},
  {"id":"a2","type":"user","content":[{"text":"x"}]}
]}"#;

#[test]
fn parse_new_jsonl_mapping_and_update() {
    let root = temp_dir().join("tmp/hash1/chats");
    std::fs::create_dir_all(&root).unwrap();
    let url = root.join("session-2026-07-03T01-00-abcd1234.jsonl");
    std::fs::write(&url, NEW_JSONL).unwrap();
    let entries = parse_gemini_file(&url, LOCAL_DAY_FORMAT);
    assert_eq!(
        entries.len(),
        2,
        "2 token-bearing messages (user/metadata excluded)"
    );

    let m2 = &entries[0];
    assert_eq!(m2.model, "gemini-2.5-pro");
    assert_eq!(m2.input, 420, "input = (1000−600 non-cached) + 20 tool");
    assert_eq!(m2.cache_read, 600);
    assert_eq!(m2.output, 80, "output = 50 + 30 thoughts");
    assert_eq!(m2.cache_write, 0);
    assert_eq!(m2.total(), 1100, "Entry.total == totalTokenCount preserved");

    let m3 = &entries[1];
    assert_eq!(
        m3.output, 10,
        "message_update (output 8 + thoughts 2) is final"
    );
    assert_eq!(m3.total(), 20);
}

#[test]
fn parse_legacy_json() {
    let root = temp_dir().join("tmp/hash1/chats");
    std::fs::create_dir_all(&root).unwrap();
    let url = root.join("checkpoint-old.json");
    std::fs::write(&url, LEGACY_JSON).unwrap();
    let entries = parse_gemini_file(&url, LOCAL_DAY_FORMAT);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].input, 100);
    assert_eq!(entries[0].output, 10);
    assert_eq!(entries[0].total(), 110);
}

#[test]
fn gemini_file_without_tokens_yields_nothing() {
    let root = temp_dir().join("tmp/hash1/chats");
    std::fs::create_dir_all(&root).unwrap();
    let url = root.join("logs.json");
    std::fs::write(
        &url,
        r#"{"entries":[{"sessionId":"x","type":"user","message":"hello"}]}"#,
    )
    .unwrap();
    let entries = parse_gemini_file(&url, LOCAL_DAY_FORMAT);
    assert!(entries.is_empty());
}

// MARK: - Grok parsing

fn chunk_line() -> String {
    r#"{"timestamp":1785000000,"method":"_x.ai/session/update","params":{"sessionId":"s1","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"hi"}},"_meta":{"totalTokens":100,"eventId":"e0","agentTimestampMs":1785000000000,"chunkId":0}}}"#
        .to_string()
}

#[allow(clippy::too_many_arguments)]
fn turn_line(
    prompt_id: &str,
    input: i64,
    output: i64,
    cached_read: i64,
    total: i64,
    cost_ticks: Option<i64>,
    cost_is_partial: bool,
    usage_is_incomplete: bool,
    model: Option<&str>,
    envelope_seconds: Option<i64>,
    agent_timestamp_ms: Option<i64>,
    is_replay: bool,
) -> String {
    let mut usage = serde_json::Map::new();
    usage.insert("inputTokens".to_string(), json!(input));
    usage.insert("outputTokens".to_string(), json!(output));
    usage.insert("totalTokens".to_string(), json!(total));
    usage.insert("cachedReadTokens".to_string(), json!(cached_read));
    usage.insert("reasoningTokens".to_string(), json!(260));
    usage.insert("modelCalls".to_string(), json!(3));
    usage.insert("numTurns".to_string(), json!(1));
    if let Some(ticks) = cost_ticks {
        usage.insert("costUsdTicks".to_string(), json!(ticks));
    }
    if cost_is_partial {
        usage.insert("costIsPartial".to_string(), json!(true));
    }
    if usage_is_incomplete {
        usage.insert("usageIsIncomplete".to_string(), json!(true));
    }
    if let Some(model) = model {
        let mut mu = serde_json::Map::new();
        mu.insert("inputTokens".to_string(), json!(input));
        mu.insert("outputTokens".to_string(), json!(output));
        mu.insert("totalTokens".to_string(), json!(total));
        mu.insert("cachedReadTokens".to_string(), json!(cached_read));
        let mut by_model = serde_json::Map::new();
        by_model.insert(model.to_string(), serde_json::Value::Object(mu));
        usage.insert(
            "modelUsage".to_string(),
            serde_json::Value::Object(by_model),
        );
    }

    let mut meta = serde_json::Map::new();
    meta.insert("totalTokens".to_string(), json!(total));
    meta.insert("eventId".to_string(), json!(format!("ev-{prompt_id}")));
    meta.insert("promptId".to_string(), json!(prompt_id));
    if let Some(ms) = agent_timestamp_ms {
        meta.insert("agentTimestampMs".to_string(), json!(ms));
    }
    if is_replay {
        meta.insert("isReplay".to_string(), json!(true));
    }

    let mut update = serde_json::Map::new();
    update.insert("sessionUpdate".to_string(), json!("turn_completed"));
    update.insert("prompt_id".to_string(), json!(prompt_id));
    update.insert("stop_reason".to_string(), json!("end_turn"));
    update.insert("usage".to_string(), serde_json::Value::Object(usage));

    let mut params = serde_json::Map::new();
    params.insert("sessionId".to_string(), json!("s1"));
    params.insert("update".to_string(), serde_json::Value::Object(update));
    params.insert("_meta".to_string(), serde_json::Value::Object(meta));

    let mut envelope = serde_json::Map::new();
    if let Some(secs) = envelope_seconds {
        envelope.insert("timestamp".to_string(), json!(secs));
    }
    envelope.insert("method".to_string(), json!("_x.ai/session/update"));
    envelope.insert("params".to_string(), serde_json::Value::Object(params));
    serde_json::to_string(&serde_json::Value::Object(envelope)).unwrap()
}

fn turn_line_default(prompt_id: &str) -> String {
    turn_line(
        prompt_id,
        41_203,
        812,
        38_400,
        42_015,
        Some(12_000_000_000),
        false,
        false,
        Some("grok-build-1"),
        Some(1_785_000_010),
        Some(1_785_000_010_000),
        false,
    )
}

fn write_session(
    dir: &Path,
    id: &str,
    lines: &[String],
    session_kind: Option<&str>,
    summary: bool,
) -> PathBuf {
    let session_dir = dir.join("cwd-group").join(id);
    std::fs::create_dir_all(&session_dir).unwrap();
    let updates = session_dir.join("updates.jsonl");
    std::fs::write(&updates, lines.join("\n")).unwrap();
    if summary {
        let summary_json = match session_kind {
            Some(kind) => format!(r#"{{"session_summary":"x","session_kind":"{kind}"}}"#),
            None => r#"{"session_summary":"x"}"#.to_string(),
        };
        std::fs::write(session_dir.join("summary.json"), summary_json).unwrap();
    }
    updates
}

#[test]
fn turn_completed_token_mapping_preserves_total_identity() {
    let root = temp_dir().join("sessions");
    let url = write_session(
        &root,
        "s1",
        &[chunk_line(), turn_line_default("p-1")],
        None,
        true,
    );
    let entries = parse_grok_file(&url, LOCAL_DAY_FORMAT);
    assert_eq!(entries.len(), 1, "chunk lines have no usage");
    let e = &entries[0];
    assert_eq!(
        e.input, 2_803,
        "input = inputTokens(41203) − cachedReadTokens(38400)"
    );
    assert_eq!(e.cache_read, 38_400);
    assert_eq!(e.output, 812, "reasoning already inside output");
    assert_eq!(
        e.cache_write, 0,
        "Grok folds cache writes into prompt tokens"
    );
    assert_eq!(e.total(), 42_015, "Entry.total == usage.totalTokens");
    assert_eq!(e.model, "grok-build-1");
    assert!(
        (e.explicit_cost.unwrap() - 1.2).abs() < 1e-9,
        "12e9 ticks = $1.2"
    );
}

#[test]
fn headless_snake_case_input_is_not_cache_adjusted_again() {
    let root = temp_dir().join("sessions");
    let line = r#"{"timestamp":1785000020,"method":"_x.ai/session/update","params":{"sessionId":"s2","update":{"sessionUpdate":"turn_completed","prompt_id":"p-snake","stop_reason":"end_turn","usage":{"input_tokens":60,"output_tokens":10,"total_tokens":110,"cached_read_tokens":40}},"_meta":{"eventId":"ev-snake","agentTimestampMs":1785000020000}}}"#;
    let url = write_session(&root, "s2", &[line.to_string()], None, true);
    let e = parse_grok_file(&url, LOCAL_DAY_FORMAT)[0].clone();
    assert_eq!(
        e.input, 60,
        "snake_case input_tokens is already cache-excluded"
    );
    assert_eq!(e.cache_read, 40);
    assert_eq!(e.output, 10);
    assert_eq!(e.total(), 110);
}

#[test]
fn multiple_turns_aggregate() {
    let root = temp_dir().join("sessions");
    let url = write_session(
        &root,
        "s3",
        &[
            chunk_line(),
            turn_line_default("p-1"),
            chunk_line(),
            turn_line(
                "p-2",
                100,
                20,
                0,
                120,
                None,
                false,
                false,
                None,
                Some(1_785_000_010),
                Some(1_785_000_010_000),
                false,
            ),
        ],
        None,
        true,
    );
    let entries = parse_grok_file(&url, LOCAL_DAY_FORMAT);
    assert_eq!(entries.len(), 2);
    let sum: i64 = entries.iter().map(Entry::total).sum();
    assert_eq!(sum, 42_015 + 120);
    let day = entries[0].local_day.clone();
    let daily = daily(&entries, &day).unwrap();
    assert_eq!(daily.total_tokens, 42_015 + 120);
    assert!(
        (daily.total_cost - 1.2).abs() < 1e-9,
        "server ticks only — second turn 0"
    );
}

#[test]
fn replay_lines_are_not_counted_twice() {
    let root = temp_dir().join("sessions");
    let deduped = write_session(
        &root,
        "s4",
        &[
            turn_line_default("p-1"),
            turn_line(
                "p-1",
                41_203,
                812,
                38_400,
                42_015,
                Some(12_000_000_000),
                false,
                false,
                Some("grok-build-1"),
                Some(1_785_000_010),
                Some(1_785_000_010_000),
                true,
            ),
        ],
        None,
        true,
    );
    let by_id = parse_grok_file(&deduped, LOCAL_DAY_FORMAT);
    assert_eq!(by_id.len(), 1, "same turn id counted once");
    assert_eq!(by_id[0].total(), 42_015);

    // isReplay branch alone: a different id the dedup cannot catch.
    let replay_only = write_session(
        &root,
        "s4-replay",
        &[
            turn_line_default("p-live"),
            turn_line(
                "p-replayed",
                41_203,
                812,
                38_400,
                42_015,
                Some(12_000_000_000),
                false,
                false,
                Some("grok-build-1"),
                Some(1_785_000_010),
                Some(1_785_000_010_000),
                true,
            ),
        ],
        None,
        true,
    );
    let ids: Vec<String> = parse_grok_file(&replay_only, LOCAL_DAY_FORMAT)
        .iter()
        .map(|e| e.id.clone())
        .collect();
    assert_eq!(ids, vec!["grok|p-live".to_string()]);
}

#[test]
fn forked_session_copy_does_not_double_count() {
    let root = temp_dir().join("sessions");
    write_session(&root, "parent", &[turn_line_default("p-1")], None, true);
    write_session(
        &root,
        "child-fork",
        &[turn_line_default("p-1")],
        Some("fork"),
        true,
    );
    let entries = grok_entries(epoch(), Some(&root));
    assert_eq!(entries.len(), 1, "same prompt_id across files is one turn");
    assert_eq!(entries[0].total(), 42_015);
}

#[test]
fn subagent_sessions_are_skipped_but_user_sessions_kept() {
    let root = temp_dir().join("sessions");
    write_session(&root, "main", &[turn_line_default("p-main")], None, true);
    write_session(
        &root,
        "sub",
        &[turn_line_default("p-sub")],
        Some("subagent"),
        true,
    );
    write_session(
        &root,
        "sub2",
        &[turn_line_default("p-sub2")],
        Some("subagent_fork"),
        true,
    );
    write_session(
        &root,
        "wt",
        &[turn_line_default("p-wt")],
        Some("worktree"),
        true,
    );

    let entries = grok_entries(epoch(), Some(&root));
    let ids: HashSet<String> = entries.iter().map(|e| e.id.clone()).collect();
    assert_eq!(
        ids,
        HashSet::from(["grok|p-main".to_string(), "grok|p-wt".to_string()])
    );
}

#[test]
fn untrustworthy_costs_are_dropped() {
    let root = temp_dir().join("sessions");
    let partial = write_session(
        &root,
        "cost-partial",
        &[turn_line(
            "p-partial",
            41_203,
            812,
            38_400,
            42_015,
            Some(12_000_000_000),
            true,
            false,
            Some("grok-build-1"),
            Some(1_785_000_010),
            Some(1_785_000_010_000),
            false,
        )],
        None,
        true,
    );
    assert!(parse_grok_file(&partial, LOCAL_DAY_FORMAT)[0]
        .explicit_cost
        .is_none());

    let incomplete = write_session(
        &root,
        "cost-incomplete",
        &[turn_line(
            "p-incomplete",
            41_203,
            812,
            38_400,
            42_015,
            Some(12_000_000_000),
            false,
            true,
            Some("grok-build-1"),
            Some(1_785_000_010),
            Some(1_785_000_010_000),
            false,
        )],
        None,
        true,
    );
    assert!(parse_grok_file(&incomplete, LOCAL_DAY_FORMAT)[0]
        .explicit_cost
        .is_none());

    let day = parse_grok_file(&partial, LOCAL_DAY_FORMAT)[0]
        .local_day
        .clone();
    let d = daily(&parse_grok_file(&partial, LOCAL_DAY_FORMAT), &day).unwrap();
    assert_eq!(
        d.total_cost, 0.0,
        "no price table for grok → 0 (no invented amount)"
    );
}

#[test]
fn zero_usage_turn_produces_no_entry() {
    let root = temp_dir().join("sessions");
    let url = write_session(
        &root,
        "zero",
        &[turn_line(
            "p-zero",
            0,
            0,
            0,
            0,
            None,
            false,
            false,
            Some("grok-build-1"),
            None,
            None,
            false,
        )],
        None,
        true,
    );
    assert!(parse_grok_file(&url, LOCAL_DAY_FORMAT).is_empty());
}

#[test]
fn agent_timestamp_wins_over_envelope_write_time() {
    let root = temp_dir().join("sessions");
    let fork_write_time = 1_785_600_000; // ~6.9 days after the real turn
    let url = write_session(
        &root,
        "forked",
        &[turn_line(
            "p-old",
            41_203,
            812,
            38_400,
            42_015,
            Some(12_000_000_000),
            false,
            false,
            Some("grok-build-1"),
            Some(fork_write_time),
            Some(1_785_000_010_000),
            false,
        )],
        None,
        true,
    );
    let e = &parse_grok_file(&url, LOCAL_DAY_FORMAT)[0];
    assert!(
        (e.date.timestamp() - 1_785_000_010).abs() <= 1,
        "turn time (agentTimestampMs), not the fork write time"
    );
}

#[test]
fn envelope_seconds_used_when_agent_timestamp_missing() {
    let root = temp_dir().join("sessions");
    let url = write_session(
        &root,
        "ts",
        &[turn_line(
            "p-ts",
            41_203,
            812,
            38_400,
            42_015,
            Some(12_000_000_000),
            false,
            false,
            Some("grok-build-1"),
            Some(1_785_000_010),
            None,
            false,
        )],
        None,
        true,
    );
    let e = &parse_grok_file(&url, LOCAL_DAY_FORMAT)[0];
    assert!((e.date.timestamp() - 1_785_000_010).abs() <= 1);
}

#[test]
fn null_token_fields_do_not_zero_the_turn() {
    let root = temp_dir().join("sessions");
    let line = r#"{"timestamp":1785000030,"method":"_x.ai/session/update","params":{"sessionId":"s5","update":{"sessionUpdate":"turn_completed","prompt_id":"p-null","stop_reason":"end_turn","usage":{"inputTokens":null,"input_tokens":60,"outputTokens":null,"output_tokens":10,"cachedReadTokens":40,"totalTokens":110}},"_meta":{"eventId":"ev-null","agentTimestampMs":1785000030000}}}"#;
    let url = write_session(&root, "nulls", &[line.to_string()], None, true);
    let e = &parse_grok_file(&url, LOCAL_DAY_FORMAT)[0];
    assert_eq!(e.input, 60, "camelCase null → snake_case value");
    assert_eq!(e.output, 10);
    assert_eq!(e.cache_read, 40);
    assert_eq!(e.total(), 110);
}

#[test]
fn cache_read_is_clamped_to_prompt_total() {
    let root = temp_dir().join("sessions");
    let url = write_session(
        &root,
        "clamp",
        &[turn_line(
            "p-clamp",
            1_000,
            50,
            1_200,
            1_050,
            None,
            false,
            false,
            Some("grok-build-1"),
            Some(1_785_000_010),
            Some(1_785_000_010_000),
            false,
        )],
        None,
        true,
    );
    let e = &parse_grok_file(&url, LOCAL_DAY_FORMAT)[0];
    assert_eq!(e.input, 0);
    assert_eq!(
        e.cache_read, 1_000,
        "cache read clamped to prompt total (1000)"
    );
    assert_eq!(e.total(), 1_050, "matches the source totalTokens");
}

#[test]
fn residual_against_reported_total_goes_to_output() {
    let root = temp_dir().join("sessions");
    let url = write_session(
        &root,
        "residual",
        &[turn_line(
            "p-res",
            500,
            10,
            100,
            700,
            None,
            false,
            false,
            Some("grok-build-1"),
            Some(1_785_000_010),
            Some(1_785_000_010_000),
            false,
        )],
        None,
        true,
    );
    let e = &parse_grok_file(&url, LOCAL_DAY_FORMAT)[0];
    assert_eq!(e.total(), 700);
    assert_eq!(e.input, 400);
    assert_eq!(e.cache_read, 100);
    assert_eq!(e.output, 200, "10 + (700 − 510) residual");
}

#[test]
fn missing_model_usage_falls_back_to_generic_model() {
    let root = temp_dir().join("sessions");
    let url = write_session(
        &root,
        "nomodel",
        &[turn_line(
            "p-nm",
            41_203,
            812,
            38_400,
            42_015,
            None,
            false,
            false,
            None,
            Some(1_785_000_010),
            Some(1_785_000_010_000),
            false,
        )],
        None,
        true,
    );
    let e = &parse_grok_file(&url, LOCAL_DAY_FORMAT)[0];
    assert_eq!(e.model, "grok");
    assert_eq!(e.total(), 42_015);
}

#[test]
fn grok_file_without_turn_completed_yields_nothing() {
    let root = temp_dir().join("sessions");
    let url = write_session(
        &root,
        "chunks",
        &[chunk_line(), chunk_line(), chunk_line()],
        None,
        true,
    );
    assert!(parse_grok_file(&url, LOCAL_DAY_FORMAT).is_empty());
}

#[test]
fn large_updates_file_aggregates_only_turn_endings() {
    let root = temp_dir().join("sessions");
    let mut lines = Vec::new();
    for turn in 0..50 {
        lines.extend(std::iter::repeat_n(chunk_line(), 400));
        lines.push(turn_line(
            &format!("p-{turn}"),
            1_000,
            100,
            400,
            1_100,
            None,
            false,
            false,
            Some("grok-build-1"),
            Some(1_785_000_010),
            Some(1_785_000_010_000),
            false,
        ));
    }
    let url = write_session(&root, "big", &lines, None, true);
    let entries = parse_grok_file(&url, LOCAL_DAY_FORMAT);
    assert_eq!(entries.len(), 50);
    let sum: i64 = entries.iter().map(Entry::total).sum();
    assert_eq!(sum, 50 * 1_100);
}

#[test]
fn block_week_month_aggregates_from_real_files() {
    let root = temp_dir().join("sessions");
    let now_local = chrono::Local::now();
    let ten_minutes_ago = now_local - chrono::Duration::seconds(600);
    let recent = ten_minutes_ago.max(start_of_day(now_local) + chrono::Duration::seconds(1));
    let recent_secs = recent.timestamp();
    let line = turn_line(
        "p-block",
        1_000,
        100,
        0,
        1_100,
        None,
        false,
        false,
        Some("grok-build-1"),
        Some(recent_secs),
        Some(recent_secs * 1_000),
        false,
    );
    write_session(&root, "agg", &[line], None, true);
    let entries = grok_entries(epoch(), Some(&root));
    assert_eq!(entries.len(), 1);

    let now_utc = now_local.with_timezone(&Utc);
    let block = active_block(&entries, now_utc).unwrap();
    assert_eq!(block.total_tokens, 1_100);
    assert!(block.is_active);

    let today = local_day(now_utc);
    let week_start = start_of_week(now_local).with_timezone(&Utc);
    let week = period(&entries, "W", &local_day(week_start), &today);
    assert_eq!(week.total_tokens, 1_100);
    let month_start = start_of_month(now_local).with_timezone(&Utc);
    let month = period(
        &entries,
        &month_key(now_local),
        &local_day(month_start),
        &today,
    );
    assert_eq!(month.total_tokens, 1_100);
    assert_eq!(daily(&entries, &today).unwrap().total_tokens, 1_100);
}

// MARK: - Aggregation + date utilities

#[test]
fn period_and_active_block() {
    let mut now_local = chrono::Local::now();
    let noon = now_local
        .date_naive()
        .and_hms_opt(12, 0, 0)
        .and_then(|d| d.and_local_timezone(Local).single());
    if let Some(n) = noon {
        now_local = n;
    }
    let now = now_local.with_timezone(&Utc);
    let recent = now - chrono::Duration::minutes(30); // 30 min ago (same day)
    let old = now - chrono::Duration::hours(10); // 10 h ago (block-external, same day)
    let entries = vec![
        usage_entry("recent", recent, 600_000),
        usage_entry("old", old, 999),
    ];

    let block = active_block(&entries, now).unwrap();
    assert_eq!(block.total_tokens, 600_000); // 5h-window entries only
    assert!(block.is_active);
    assert!(block.tokens_per_minute.unwrap_or(0.0) > 0.0);

    let today = local_day(now);
    let p = period(&entries, "w", &today, &today);
    assert!(p.total_tokens >= 600_000);
}

#[test]
fn enrichment_scan_start_covers_all_windows() {
    use chrono::TimeZone;
    let base = Local
        .with_ymd_and_hms(2026, 1, 1, 2, 0, 0)
        .single()
        .unwrap();
    let mut straddle: Option<DateTime<Local>> = None;
    for offset in 0..14u32 {
        if let Some(candidate) = base.checked_add_months(chrono::Months::new(offset)) {
            if start_of_week(candidate) < start_of_month(candidate) {
                straddle = Some(candidate);
                break;
            }
        }
    }
    let now = straddle.expect("a month whose week-start precedes month-start must exist");
    let scan = enrichment_scan_start(now);
    assert!(scan <= start_of_month(now));
    assert!(scan <= start_of_week(now));
    assert!(scan <= now - chrono::Duration::seconds(BLOCK_WINDOW_SECS));
    assert!(
        scan < start_of_month(now),
        "at month start the week start rolls into the previous month"
    );

    let mid = Local
        .with_ymd_and_hms(2026, 7, 20, 12, 0, 0)
        .single()
        .unwrap();
    assert_eq!(enrichment_scan_start(mid), start_of_month(mid));
}
