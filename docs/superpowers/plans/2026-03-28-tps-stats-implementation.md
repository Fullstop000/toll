# TPS (Tokens Per Second) Stats Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add tokens-per-second (TPS) tracking to toll, measuring actual model processing time (CPU time) rather than wall-clock time, and display TPS in both the summary table and by-model table.

**Architecture:** Add `processing_time_ms: u64` field to `TokenUsage`. Each agent parses per-call timestamps from its log files, accumulating model processing time as the difference between consecutive call timestamps within the same session. TPS = `total_tokens / processing_time_ms * 1000`.

**Tech Stack:** Rust, chrono for timestamps, existing JSONL/JSON parsing infrastructure.

---

## File Map

| File | Responsibility |
|------|---------------|
| `src/usage.rs` | Add `processing_time_ms` field to `TokenUsage`, add `tps()` helper |
| `src/display.rs` | Add TPS column to summary table and by-model table; add `fmt_tps()` helper |
| `src/claude.rs` | Parse processing time from timestamp diffs between JSONL entries |
| `src/codex.rs` | Parse processing time from `turn_context` event timestamps |
| `src/kimi.rs` | Parse processing time from `StatusUpdate` event timestamps |
| `src/gemini.rs` | Parse processing time from message timestamps in chats JSON |
| `src/output.rs` | Add TPS to `JsonUsageRecord` and CSV output |

---

## Task 1: Add `processing_time_ms` field and `tps()` helper to `TokenUsage`

**Files:**
- Modify: `src/usage.rs:5-22`

- [ ] **Step 1: Add failing test for `tps()` and `processing_time_ms`**

Add to the test module in `src/usage.rs`:

```rust
#[test]
fn tps_returns_tokens_per_second() {
    let usage = TokenUsage {
        input_tokens: 1000,
        output_tokens: 500,
        processing_time_ms: 1000, // 1 second
        ..Default::default()
    };
    // total_tokens = 1500, processing_time_ms = 1000
    // tps = 1500 / 1000 * 1000 = 1500.0
    assert!((usage.tps().unwrap() - 1500.0).abs() < 1e-6);
}

#[test]
fn tps_returns_none_when_no_processing_time() {
    let usage = TokenUsage {
        input_tokens: 1000,
        output_tokens: 500,
        processing_time_ms: 0,
        ..Default::default()
    };
    assert!(usage.tps().is_none());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test usage::tests::tps_returns_tokens_per_second`
Expected: FAIL — `processing_time_ms` field does not exist

- [ ] **Step 3: Add `processing_time_ms` field to `TokenUsage`**

In `src/usage.rs:6-22`, add `pub processing_time_ms: u64` to the struct:

```rust
#[derive(Clone, Default, Debug)]
pub struct TokenUsage {
    pub input_tokens: u64,
    pub cached_input_tokens: u64,
    pub cache_write_tokens: u64,
    pub output_tokens: u64,
    pub sessions: u32,
    pub user_queries: u32,
    pub cost_usd: f64,
    pub unknown_cost_sessions: u32,
    pub processing_time_ms: u64,  // NEW
    pub by_model: BTreeMap<String, TokenUsage>,
}
```

- [ ] **Step 4: Add `tps()` helper method**

Add after `has_unknown_cost()`:

```rust
/// Tokens per second = total_tokens / processing_time_ms * 1000
/// Returns None if processing_time_ms is 0.
pub fn tps(&self) -> Option<f64> {
    if self.processing_time_ms == 0 {
        return None;
    }
    Some(self.total_tokens() as f64 / self.processing_time_ms as f64 * 1000.0)
}
```

- [ ] **Step 5: Update `add()` to merge `processing_time_ms`**

In `src/usage.rs:35-47`, add:
```rust
self.processing_time_ms += other.processing_time_ms;
```
to the `add()` method, before the `by_model` loop.

- [ ] **Step 6: Update `saturating_sub()` to subtract `processing_time_ms`**

In `src/usage.rs:97-114`, add:
```rust
processing_time_ms: self.processing_time_ms.saturating_sub(baseline.processing_time_ms),
```
to the returned struct.

- [ ] **Step 7: Update test struct initializers**

Update all `TokenUsage { ... }` literals in the test module to include `processing_time_ms: 0` (or omit it since `Default::default()` will set it to 0).

- [ ] **Step 8: Run tests to verify they pass**

Run: `cargo test usage::tests -- --nocapture`
Expected: PASS

- [ ] **Step 9: Commit**

```bash
git add src/usage.rs
git commit -m "feat(usage): add processing_time_ms field and tps() helper"
```

---

## Task 2: Add TPS column to display tables

**Files:**
- Modify: `src/display.rs:13-23` (MULTI_SUMMARY_HEADERS)
- Modify: `src/display.rs:65-87` (add fmt_tps)
- Modify: `src/display.rs:95-107` (summary_values adds TPS cell)
- Modify: `src/display.rs:119-161` (render_model_breakdown adds TPS column)
- Modify: `src/display.rs:164-221` (render_single_table — add TPS to rows)
- Modify: `src/display.rs:229-290` (render_multi_table — add TPS column)
- Modify: `src/display.rs:298-395` (render_daily_table — add TPS column)

- [ ] **Step 1: Add failing test for TPS in render_multi_table**

Add to `src/display.rs` tests:

```rust
#[test]
fn render_multi_table_includes_tps_column() {
    let usage = TokenUsage {
        input_tokens: 1_000_000,
        output_tokens: 500_000,
        processing_time_ms: 1000,
        sessions: 1,
        cost_usd: 1.0,
        ..Default::default()
    };
    let rendered = render_multi_table(&[("Claude Code", &usage)], NumberFormat::Full);
    // TPS = 1_500_000 / 1000 * 1000 = 1_500_000
    assert!(rendered.contains("1,500,000"));
    assert!(rendered.contains("TPS"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test render_multi_table_includes_tps_column`
Expected: FAIL — TPS column does not exist

- [ ] **Step 3: Add `fmt_tps()` helper function**

Add after `fmt_cost()` in `src/display.rs`:

```rust
/// Format TPS value: tokens per second, using compact notation.
/// Returns "—" if usage has no processing time.
pub fn fmt_tps(usage: &TokenUsage) -> String {
    match usage.tps() {
        None => "—".to_string(),
        Some(tps) => fmt_num_with_format(tps as u64, NumberFormat::Compact),
    }
}
```

- [ ] **Step 4: Update `MULTI_SUMMARY_HEADERS` from 9 to 10 columns**

Change `const MULTI_SUMMARY_HEADERS: [&str; 9]` to `const MULTI_SUMMARY_HEADERS: [&str; 10]` and add `"TPS"` after `"Cost"`.

- [ ] **Step 5: Update `summary_values()` array size and content**

Change `[String; 9]` to `[String; 10]` and append TPS cell:
```rust
fn summary_values(usage: &TokenUsage, format: NumberFormat) -> [String; 10] {
    [
        fmt_num(usage.sessions as u64),
        fmt_num(usage.user_queries as u64),
        fmt_num_with_format(usage.input_tokens, format),
        fmt_cached_tokens(usage.cached_input_tokens, format),
        fmt_pct(usage.cached_input_tokens, usage.input_tokens),
        fmt_num_with_format(usage.net_input_tokens(), format),
        fmt_num_with_format(usage.output_tokens, format),
        fmt_num_with_format(usage.total_tokens(), format),
        fmt_cost(usage),
        fmt_tps(usage),  // NEW
    ]
}
```

- [ ] **Step 6: Update `render_model_breakdown()` to add TPS column**

Change column width calculation from 3 columns to 4:
```rust
let total_w = label_w + 2 + (col_w + 2) * 4;  // 4 columns now
```

Update the header and format strings:
```rust
out.push_str(&format!(
    "  {:<lw$} {:>cw$}  {:>cw$}  {:>cw$}  {:>cw$}\n",
    "Model", "Tokens", "Output", "Cost", "TPS",
    lw = label_w, cw = col_w
));
```

Update the per-row format:
```rust
out.push_str(&format!(
    "  {:<lw$} {:>cw$}  {:>cw$}  {:>cw$}  {:>cw$}\n",
    label,
    fmt_num_with_format(usage.total_tokens(), format),
    fmt_num_with_format(usage.output_tokens, format),
    fmt_cost(usage),
    fmt_tps(usage),  // NEW
    lw = label_w, cw = col_w
));
```

- [ ] **Step 7: Update `render_single_table()` rows array to include TPS**

Add TPS row after the cost row:
```rust
let rows = [
    ("Sessions", values[0].as_str()),
    ("User queries", values[1].as_str()),
    ("Input tokens", values[2].as_str()),
    ("  ↳ cached", values[3].as_str()),
    ("  ↳ hit rate", values[4].as_str()),
    ("  ↳ net (non-cached)", values[5].as_str()),
    ("Output tokens", values[6].as_str()),
    ("Total tokens", values[7].as_str()),
    ("Estimated cost (USD)", values[8].as_str()),
    ("TPS", values[9].as_str()),  // NEW
];
```

Update the separator index (was 1, 5, 7 — now 1, 5, 8):
```rust
if matches!(idx, 1 | 5 | 8) {
```

- [ ] **Step 8: Update `render_multi_table()` column width calculation**

The header count changed from 9 to 10, so the `col_widths` array will automatically accommodate 10 columns. Verify that `summary_values` now returns `[String; 10]` and the headers are 10 items.

- [ ] **Step 9: Update `render_daily_table()` headers and rows**

Add `"TPS"` to the headers array (now 10 items) and add TPS cell to each row.

- [ ] **Step 10: Update test assertions in display.rs**

Update `render_multi_table_supports_three_agents` test to check for 10 header columns and `"TPS"`.

- [ ] **Step 11: Run tests and fmt/clippy**

Run: `cargo fmt && cargo clippy -- -D warnings && cargo test display::tests -- --nocapture`
Expected: PASS

- [ ] **Step 12: Commit**

```bash
git add src/display.rs
git commit -m "feat(display): add TPS column to summary and by-model tables"
```

---

## Task 3: Parse processing time in Claude Code agent

**Files:**
- Modify: `src/claude.rs:70-145` (`parse_claude_lines` function)

- [ ] **Step 1: Read current parse_claude_lines**

The key insight: each JSONL entry has a `timestamp` field. Processing time per call = timestamp of current call − timestamp of previous call within the same session.

- [ ] **Step 2: Add failing test for processing time extraction**

Add to test module in `claude.rs`:

```rust
#[test]
fn parse_claude_lines_accumulates_processing_time() {
    let json = r#"{"timestamp":"2026-03-28T10:00:00Z","message":{"model":"claude-opus-4-6","usage":{"input_tokens":100,"output_tokens":50}}}
{"timestamp":"2026-03-28T10:00:01Z","message":{"model":"claude-opus-4-6","usage":{"input_tokens":200,"output_tokens":100}}}"#;
    let usage = parse_claude_lines(json.as_bytes(), None);
    // Delta: 1 second = 1000ms between calls
    assert_eq!(usage.processing_time_ms, 1000);
    // total_tokens = 100+50+200+100 = 450
    assert_eq!(usage.total_tokens(), 450);
    // TPS = 450 / 1000 * 1000 = 450
    assert!((usage.tps().unwrap() - 450.0).abs() < 1e-6);
}
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test parse_claude_lines_accumulates_processing_time`
Expected: FAIL — `processing_time_ms` not accumulated

- [ ] **Step 4: Modify `parse_claude_lines` to track timestamp deltas**

In `src/claude.rs:70-145`, track the previous timestamp and accumulate delta:

```rust
pub fn parse_claude_lines(reader: impl BufRead, since: Option<DateTime<Utc>>) -> TokenUsage {
    let mut usage = TokenUsage {
        sessions: 1,
        ..Default::default()
    };
    let mut has_unknown_model = false;
    let mut prev_ts: Option<DateTime<Utc>> = None;  // NEW

    for line in reader.lines().map_while(Result::ok) {
        // ... existing filtering code ...

        // Get current timestamp for processing time delta
        let current_ts = ts_str.and_then(|ts| ts.parse::<DateTime<Utc>>().ok());

        // Accumulate processing time delta (skip first call — no previous to compare)
        if let (Some(curr), Some(prev)) = (current_ts, prev_ts) {
            let delta_ms = (curr - prev).num_milliseconds() as u64;
            usage.processing_time_ms += delta_ms;
        }

        // Update prev_ts for next iteration
        if current_ts.is_some() {
            prev_ts = current_ts;
        }

        // ... rest of existing code (user_queries check, usage extraction, cost calculation) ...
    }
    // ...
}
```

Note: Be careful to place the timestamp delta accumulation BEFORE the `since` filter continue — we want to include the delta even for filtered calls if they help measure real processing gaps. Actually, since we skip filtered lines with `continue`, we should only accumulate delta for calls we actually process.

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test parse_claude_lines_accumulates_processing_time -- --nocapture`
Expected: PASS

- [ ] **Step 6: Run fmt/clippy**

Run: `cargo fmt && cargo clippy -- -D warnings`
Expected: PASS

- [ ] **Step 7: Commit**

```bash
git add src/claude.rs
git commit -m "feat(claude): parse processing time from timestamp deltas"
```

---

## Task 4: Parse processing time in Codex agent

**Files:**
- Modify: `src/codex.rs`

- [ ] **Step 1: Find how Codex stores timestamps**

Look at `src/codex.rs` for timestamp fields in turn_context events.

- [ ] **Step 2: Add test and implementation**

Similar pattern to Claude — track previous timestamp and accumulate delta per session. The exact field names depend on the Codex log format.

- [ ] **Step 3: Run fmt/clippy and commit**

```bash
cargo fmt && cargo clippy -- -D warnings
git add src/codex.rs
git commit -m "feat(codex): parse processing time from timestamp deltas"
```

---

## Task 5: Parse processing time in Kimi agent

**Files:**
- Modify: `src/kimi.rs`

- [ ] **Step 1: Find how Kimi stores timestamps**

Look at `src/kimi.rs` for `StatusUpdate` events and timestamp fields.

- [ ] **Step 2: Add test and implementation**

- [ ] **Step 3: Run fmt/clippy and commit**

---

## Task 6: Parse processing time in Gemini agent

**Files:**
- Modify: `src/gemini.rs`

- [ ] **Step 1: Find how Gemini stores timestamps**

Look at `src/gemini.rs` for message timestamp fields.

- [ ] **Step 2: Add test and implementation**

- [ ] **Step 3: Run fmt/clippy and commit**

---

## Task 7: Add TPS to JSON and CSV output

**Files:**
- Modify: `src/output.rs`

- [ ] **Step 1: Add `tps` field to `JsonUsageRecord`**

In `src/output.rs:70-81`:
```rust
pub struct JsonUsageRecord {
    pub input_tokens: u64,
    pub cached_input_tokens: u64,
    pub cache_hit_rate_pct: f64,
    pub net_input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    pub sessions: u32,
    pub user_queries: u32,
    pub cost_usd: f64,
    pub unknown_cost_sessions: u32,
    pub tps: Option<f64>,  // NEW: tokens per second
}
```

- [ ] **Step 2: Update `json_usage_record()` to include TPS**

```rust
fn json_usage_record(usage: &TokenUsage) -> JsonUsageRecord {
    // ... existing fields ...
    JsonUsageRecord {
        // ... existing assignments ...
        tps: usage.tps(),  // NEW
    }
}
```

- [ ] **Step 3: Update CSV summary row header and values**

In `csv_summary_row()`, add TPS to the header and values arrays (10 → 11 columns).

- [ ] **Step 4: Update CSV by-model row header and values**

In `csv_model_row()`, add TPS to header and values (4 → 5 columns).

- [ ] **Step 5: Update CSV daily header**

In `render_daily_csv()`, add TPS column header and values to daily rows (10 → 11 columns).

- [ ] **Step 6: Update test assertions in output.rs**

Update all tests that check CSV headers or JSON fields.

- [ ] **Step 7: Run fmt/clippy and commit**

```bash
cargo fmt && cargo clippy -- -D warnings && cargo test output::tests -- --nocapture
git add src/output.rs
git commit -m "feat(output): add TPS to JSON and CSV output"
```

---

## Task 8: Final integration — fmt, clippy, README

- [ ] **Step 1: Run full test suite**

```bash
cargo fmt && cargo clippy -- -D warnings && cargo test
```

- [ ] **Step 2: Update README example output**

In `README.md`, update the example output to include TPS column:

```
              Sessions  Queries   Input  Cached  Hit Rate  Net Input  Output   Total      Cost     TPS
  ════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════
  Claude Code       38      141  316.1m  304.2m     96.2%      11.9m    1.3m  317.5m  $168.59   1.2k
```

And update By-model table:
```
  Model                                 Tokens           Output             Cost        TPS
  ══════════════════════════════════════════════════════════════════════════════════════════════
  ...
```

- [ ] **Step 3: Final commit**

```bash
git add README.md
git commit -m "docs: add TPS to example output"
git push
```

---

## Self-Review Checklist

- [ ] All spec requirements covered? TPS in summary table, by-model table, per-agent + combined, processing_time_ms from actual model time (not wall-clock)
- [ ] No placeholder/TODO in plan
- [ ] Types consistent: `processing_time_ms: u64`, `tps() -> Option<f64>`, `fmt_tps() -> String`
- [ ] All 4 agents implement timestamp delta extraction
- [ ] JSON and CSV output updated with TPS field
- [ ] README example output updated
- [ ] Tests pass with `cargo test`, `cargo fmt`, `cargo clippy -- -D warnings`
