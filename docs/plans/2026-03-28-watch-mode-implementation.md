# Watch Mode Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add `toll --watch` so the CLI captures usage deltas from watch start until interrupt, live-refreshes table output, and emits one final delta snapshot for table, JSON, or CSV output.

**Architecture:** Keep the existing scan-based collectors, but introduce session-granular snapshots so watch mode can compute `current - baseline` per session and then reuse the current aggregate renderers. Implement watch orchestration in `src/main.rs`, isolate snapshot and delta helpers in a new shared module, and extend each agent with session snapshot collection rather than building a separate file-tailing engine.

**Tech Stack:** Rust 2024, `clap`, `chrono`, `serde`, `serde_json`, `walkdir`, plus a small signal-handling crate such as `ctrlc`

---

### Task 1: Add CLI parsing for watch mode

**Files:**
- Modify: `Cargo.toml`
- Modify: `src/main.rs`

**Step 1: Write the failing CLI parse tests**

Add tests near the existing `Args::try_parse_from` coverage in `src/main.rs`:

```rust
#[test]
fn parses_watch_flag() {
    let args = Args::try_parse_from(["toll", "--watch"]).expect("should parse");
    assert!(args.watch);
}

#[test]
fn parses_watch_with_by_day() {
    let args = Args::try_parse_from(["toll", "--watch", "--by-day"]).expect("should parse");
    assert!(args.watch);
    assert!(args.by_day);
}

#[test]
fn rejects_watch_with_today() {
    assert!(Args::try_parse_from(["toll", "--watch", "--today"]).is_err());
}

#[test]
fn rejects_watch_with_days() {
    assert!(Args::try_parse_from(["toll", "--watch", "--days", "7"]).is_err());
}
```

**Step 2: Run the targeted test command and verify it fails**

Run: `cargo test -q parses_watch_flag parses_watch_with_by_day rejects_watch_with_today rejects_watch_with_days`

Expected: FAIL because `Args` does not yet contain `watch` or the required conflicts.

**Step 3: Add the minimal CLI surface**

Update `Args` in `src/main.rs`:

```rust
#[arg(long, conflicts_with = "today", conflicts_with = "days", help = "Watch usage deltas from now until interrupted")]
watch: bool,
```

Add `--watch` to the CLI examples in the command help text:

```text
  toll --watch            # live delta stats until Ctrl-C
  toll --watch --by-day   # live per-day delta stats
```

**Step 4: Re-run the targeted test command**

Run: `cargo test -q parses_watch_flag parses_watch_with_by_day rejects_watch_with_today rejects_watch_with_days`

Expected: PASS

**Step 5: Commit**

```bash
git add Cargo.toml src/main.rs
git commit -m "feat(cli): add watch flag parsing"
```

### Task 2: Add shared session snapshot and delta helpers

**Files:**
- Create: `src/watch.rs`
- Modify: `src/usage.rs`
- Modify: `src/main.rs`

**Step 1: Write failing unit tests for snapshot diffing**

Add tests in `src/watch.rs` that lock down the core semantics:

```rust
#[test]
fn diff_existing_session_uses_incremental_growth() {
    let baseline = snapshot([("session-a", usage(100, 40, 0, 10, 1, 2))]);
    let current = snapshot([("session-a", usage(160, 70, 0, 16, 1, 3))]);

    let delta = diff_snapshot(&baseline, &current);

    assert_eq!(delta.total.input_tokens, 60);
    assert_eq!(delta.total.cached_input_tokens, 30);
    assert_eq!(delta.total.output_tokens, 6);
    assert_eq!(delta.total.user_queries, 1);
}

#[test]
fn diff_new_session_takes_full_current_usage() {
    let baseline = AgentSnapshot::default();
    let current = snapshot([("session-b", usage(80, 20, 0, 8, 1, 1))]);

    let delta = diff_snapshot(&baseline, &current);

    assert_eq!(delta.total.total_tokens(), 88);
}

#[test]
fn diff_regression_clamps_to_zero() {
    let baseline = snapshot([("session-a", usage(100, 40, 0, 10, 1, 2))]);
    let current = snapshot([("session-a", usage(50, 10, 0, 5, 1, 1))]);

    let delta = diff_snapshot(&baseline, &current);

    assert_eq!(delta.total.total_tokens(), 0);
    assert_eq!(delta.total.user_queries, 0);
}
```

**Step 2: Run the targeted test command and verify it fails**

Run: `cargo test -q diff_existing_session_uses_incremental_growth diff_new_session_takes_full_current_usage diff_regression_clamps_to_zero`

Expected: FAIL because `src/watch.rs` and the snapshot types do not yet exist.

**Step 3: Implement the minimal snapshot layer**

Create `src/watch.rs` with shared types and pure helpers:

```rust
use std::collections::BTreeMap;

use crate::usage::{DailyUsage, TokenUsage, add_daily_usage};

#[derive(Clone, Debug, Default)]
pub struct SessionUsage {
    pub totals: TokenUsage,
    pub by_day: DailyUsage,
}

pub type AgentSnapshot = BTreeMap<String, SessionUsage>;

#[derive(Debug, Default)]
pub struct SnapshotDelta {
    pub total: TokenUsage,
    pub by_day: DailyUsage,
}

pub fn diff_snapshot(baseline: &AgentSnapshot, current: &AgentSnapshot) -> SnapshotDelta {
    // iterate current sessions, subtract baseline counters per field with saturating math,
    // aggregate per-session totals into SnapshotDelta
}
```

Add a small subtraction helper in `src/usage.rs` so the diffing logic stays explicit and testable:

```rust
impl TokenUsage {
    pub fn saturating_sub(&self, baseline: &TokenUsage) -> TokenUsage {
        TokenUsage {
            input_tokens: self.input_tokens.saturating_sub(baseline.input_tokens),
            cached_input_tokens: self.cached_input_tokens.saturating_sub(baseline.cached_input_tokens),
            cache_write_tokens: self.cache_write_tokens.saturating_sub(baseline.cache_write_tokens),
            output_tokens: self.output_tokens.saturating_sub(baseline.output_tokens),
            sessions: self.sessions.saturating_sub(baseline.sessions),
            user_queries: self.user_queries.saturating_sub(baseline.user_queries),
            cost_usd: (self.cost_usd - baseline.cost_usd).max(0.0),
            unknown_cost_sessions: self.unknown_cost_sessions.saturating_sub(baseline.unknown_cost_sessions),
            by_model: BTreeMap::new(),
        }
    }
}
```

Register the new module in `src/main.rs`:

```rust
mod watch;
```

**Step 4: Re-run the targeted test command**

Run: `cargo test -q diff_existing_session_uses_incremental_growth diff_new_session_takes_full_current_usage diff_regression_clamps_to_zero`

Expected: PASS

**Step 5: Commit**

```bash
git add src/watch.rs src/usage.rs src/main.rs
git commit -m "feat(watch): add snapshot diff helpers"
```

### Task 3: Extend the agent abstraction to collect session snapshots

**Files:**
- Modify: `src/agent.rs`
- Modify: `src/claude.rs`
- Modify: `src/codex.rs`
- Modify: `src/kimi.rs`
- Modify: `src/gemini.rs`

**Step 1: Write failing agent-level snapshot tests**

Add one focused test per provider in the existing inline test modules. Example targets:

```rust
#[test]
fn codex_snapshot_uses_rollout_path_as_session_id() {
    let usage = parse_codex_lines(BufReader::new(SAMPLE.as_bytes())).expect("usage");
    let snapshot = collect_codex_snapshot(temp_sessions_dir.path(), None);

    assert!(snapshot.contains_key("2026/03/28/rollout-2026-03-28T12-00-00-abc.jsonl"));
    assert_eq!(snapshot.values().next().unwrap().totals.total_tokens(), usage.total_tokens());
}
```

```rust
#[test]
fn claude_snapshot_keeps_per_day_buckets_for_existing_log() {
    let snapshot = collect_claude_snapshot(temp_projects_dir.path(), None);
    let session = snapshot.get("project-a/session.jsonl").expect("session");

    assert_eq!(session.totals.user_queries, 2);
    assert_eq!(session.by_day.len(), 1);
}
```

**Step 2: Run the targeted test command and verify it fails**

Run: `cargo test -q codex_snapshot_uses_rollout_path_as_session_id claude_snapshot_keeps_per_day_buckets_for_existing_log`

Expected: FAIL because the agent trait and collectors still return only aggregates.

**Step 3: Implement snapshot collection for each provider**

Extend `src/agent.rs`:

```rust
use crate::watch::AgentSnapshot;

pub trait Agent {
    fn name(&self) -> &'static str;
    fn data_dir(&self, home: &Path) -> PathBuf;
    fn collect_usage(&self, data_dir: &Path, since: Option<DateTime<Utc>>) -> TokenUsage;
    fn collect_daily_usage(&self, data_dir: &Path, since: Option<DateTime<Utc>>) -> DailyUsageReport;
    fn collect_snapshot(&self, data_dir: &Path, since: Option<DateTime<Utc>>) -> AgentSnapshot;
}
```

Implement `collect_snapshot` in each provider by reusing existing parse functions:

- `src/claude.rs`: one snapshot entry per `.jsonl` file, keyed by project-relative path
- `src/codex.rs`: one snapshot entry per `rollout-*.jsonl` file, keyed by path relative to `.codex/sessions`
- `src/kimi.rs`: one snapshot entry per `wire.jsonl`, keyed by the session directory path
- `src/gemini.rs`: one snapshot entry per `chats/*.json`, keyed by path relative to `.gemini/tmp`

Each snapshot entry should populate both:

```rust
SessionUsage {
    totals: existing_total_usage,
    by_day: existing_daily_usage_for_that_session,
}
```

Avoid rewriting the parsing rules. Refactor existing collectors so aggregate collection becomes “collect snapshots, then fold them,” not two divergent implementations.

**Step 4: Re-run the targeted provider tests**

Run: `cargo test -q codex_snapshot_uses_rollout_path_as_session_id claude_snapshot_keeps_per_day_buckets_for_existing_log`

Expected: PASS

**Step 5: Commit**

```bash
git add src/agent.rs src/claude.rs src/codex.rs src/kimi.rs src/gemini.rs
git commit -m "feat(watch): add per-session agent snapshots"
```

### Task 4: Implement the watch runtime and live table redraw

**Files:**
- Modify: `Cargo.toml`
- Modify: `src/main.rs`
- Modify: `src/display.rs`
- Modify: `src/output.rs`
- Modify: `src/watch.rs`

**Step 1: Write failing tests for watch orchestration helpers**

Keep signal and redraw mechanics thin. Test the pure pieces:

```rust
#[test]
fn watch_table_mode_uses_latest_delta_for_render() {
    let baseline = snapshot([("session-a", usage(100, 20, 0, 10, 1, 1))]);
    let current = snapshot([("session-a", usage(130, 30, 0, 14, 1, 2))]);

    let delta = diff_snapshot(&baseline, &current);
    let rendered = render_multi_table(&[("Codex", &delta.total)], NumberFormat::Full);

    assert!(rendered.contains("30"));
    assert!(rendered.contains("4"));
}
```

Add a parser test in `src/main.rs` for output-mode interaction if needed, but keep the loop itself mostly integration-free.

**Step 2: Run the targeted test command and verify it fails**

Run: `cargo test -q watch_table_mode_uses_latest_delta_for_render`

Expected: FAIL until the watch helpers are wired into rendering paths.

**Step 3: Implement the runtime**

Add `ctrlc` to `Cargo.toml`:

```toml
ctrlc = "3"
```

In `src/main.rs`, split the current one-shot flow into:

```rust
fn run_once(args: &Args) { /* existing behavior */ }

fn run_watch(args: &Args) {
    let baseline = collect_selected_snapshots(&agents, &home, None);
    let interrupted = Arc::new(AtomicBool::new(false));
    ctrlc::set_handler({
        let interrupted = interrupted.clone();
        move || interrupted.store(true, Ordering::SeqCst)
    }).expect("ctrl-c handler should install");

    while !interrupted.load(Ordering::SeqCst) {
        let current = collect_selected_snapshots(&agents, &home, None);
        let delta = diff_selected_snapshots(&baseline, &current);
        render_watch_frame(args, &delta);
        std::thread::sleep(std::time::Duration::from_secs(2));
    }

    let final_current = collect_selected_snapshots(&agents, &home, None);
    let final_delta = diff_selected_snapshots(&baseline, &final_current);
    render_watch_final(args, &final_delta);
}
```

Use ANSI escape sequences for redraw in table mode:

```rust
print!("\x1B[2J\x1B[H");
```

Keep JSON and CSV watch mode silent during the loop. Only print the final payload after interrupt.

**Step 4: Re-run the targeted tests and then the full suite**

Run: `cargo test -q watch_table_mode_uses_latest_delta_for_render`

Expected: PASS

Run: `cargo test -q`

Expected: PASS for the full suite

**Step 5: Commit**

```bash
git add Cargo.toml src/main.rs src/display.rs src/output.rs src/watch.rs
git commit -m "feat(watch): add live watch runtime"
```

### Task 5: Update docs and add user-facing watch examples

**Files:**
- Modify: `README.md`

**Step 1: Write the failing doc expectations as a quick grep check**

Add the new usage lines and examples mentally first, then verify they are not present:

Run: `rg -n -- '--watch|live delta|Ctrl-C' README.md`

Expected: no matches for the new watch documentation.

**Step 2: Update the README**

Add `--watch` to the usage block:

```text
      --watch        Watch usage deltas from now until interrupted
```

Add examples:

```text
  toll --watch            # live delta stats until Ctrl-C
  toll --watch --json     # final JSON delta on exit
  toll --watch --by-day   # live per-day delta stats
```

Add one short behavior note under usage or example output:

```text
Watch mode reports only usage accumulated after watch start, including new activity on sessions that were already open.
```

**Step 3: Run the grep check again**

Run: `rg -n -- '--watch|live delta|Ctrl-C' README.md`

Expected: matches for the new option, examples, and behavior note

**Step 4: Run the full test suite one last time**

Run: `cargo test -q`

Expected: PASS

**Step 5: Commit**

```bash
git add README.md
git commit -m "docs: document watch mode"
```

### Task 6: Final verification and integration review

**Files:**
- Review only: `Cargo.toml`
- Review only: `src/main.rs`
- Review only: `src/watch.rs`
- Review only: `src/agent.rs`
- Review only: `src/claude.rs`
- Review only: `src/codex.rs`
- Review only: `src/kimi.rs`
- Review only: `src/gemini.rs`
- Review only: `README.md`

**Step 1: Run the complete verification set**

Run: `cargo test -q`

Expected: PASS

Run: `cargo run -- --watch --help`

Expected: help output includes `--watch`

Run: `timeout 5 cargo run -- --watch --json`

Expected: process starts cleanly, exits after timeout or interrupt, and prints a JSON object only once at shutdown

**Step 2: Review for scope drift**

Confirm the implementation did not add:

- streamed JSON snapshots
- support for `--watch --days`
- provider-specific tailing logic
- renderer-specific watch schemas

**Step 3: Create the final integration commit if needed**

If verification required follow-up fixes:

```bash
git add Cargo.toml src/main.rs src/watch.rs src/agent.rs src/claude.rs src/codex.rs src/kimi.rs src/gemini.rs README.md
git commit -m "fix: polish watch mode integration"
```
