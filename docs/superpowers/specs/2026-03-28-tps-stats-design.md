# TPS (Tokens Per Second) Stats Design

## Overview

Add tokens-per-second (TPS) tracking to toll, measuring actual model processing time (CPU time) rather than wall-clock time. This reflects true model throughput, excluding user idle time.

## Data Model

### Changes to `TokenUsage` (`src/usage.rs`)

Add new field:
```rust
pub struct TokenUsage {
    // ... existing fields ...
    pub processing_time_ms: u64,  // NEW: accumulated model processing time in ms
}
```

Add helper method:
```rust
impl TokenUsage {
    /// Tokens per second = total_tokens / processing_time_ms * 1000
    /// Returns None if processing_time_ms is 0.
    pub fn tps(&self) -> Option<f64> {
        if self.processing_time_ms == 0 {
            return None;
        }
        Some(self.total_tokens() as f64 / self.processing_time_ms as f64 * 1000.0)
    }
}
```

Update `add()` and `saturating_sub()` to merge/subtract `processing_time_ms`.

### Per-Model Breakdown

The `by_model` inner `TokenUsage` entries will also track `processing_time_ms` per model, enabling TPS calculation in the by-model table.

## Log Parsing

Each agent extracts per-call processing time from timestamps in log files:

| Agent | Log Format | How to Extract Processing Time |
|-------|------------|-------------------------------|
| Claude Code | JSONL | `message.usage.input_tokens`, `message.usage.output_tokens`, use timestamp diff between calls |
| Codex | JSONL | `payload.model` in `turn_context` events + timestamp delta between requests |
| Kimi | JSONL (`wire.jsonl`) | `StatusUpdate` events — has timestamp field; diff between successive events |
| Gemini | JSON (`chats/*.json`) | `tokens` field with timestamps, diff between successive agent messages |

For Claude Code specifically, the JSONL entries contain `time` field (ISO8601). Processing time per call = difference between consecutive `time` values for the same session.

## Display

### Summary Table

Add TPS column after Cost:
```
              Sessions  Queries   Input  Cached  Hit Rate  Net Input  Output   Total      Cost     TPS
  ════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════
  Claude Code       38      141  316.1m  304.2m     96.2%      11.9m    1.3m  317.5m  $168.59   1.2k
```

### By-Model Table

Add TPS column:
```
  Model                                 Tokens           Output             Cost        TPS
  ══════════════════════════════════════════════════════════════════════════════════════════════
  claude-haiku-4-5-20251001               9.4m            40.5k            $2.71       890
  claude-opus-4-6                        49.7m           317.2k           $45.17       420
```

### Number Formatting

Format TPS values similarly to tokens (compact k/m/b suffix for large values).

## Implementation Steps

1. Add `processing_time_ms: u64` field to `TokenUsage` in `src/usage.rs`
2. Add `tps()` helper method and update `add()` / `saturating_sub()`
3. Add TPS rendering helper in `src/display.rs`
4. Update `render_summary_table()` and `render_model_breakdown()` to include TPS column
5. Update `MULTI_SUMMARY_HEADERS` constant
6. Parse processing time in `src/claude.rs` — extract from timestamp diffs between JSONL entries
7. Parse processing time in `src/codex.rs` — extract from `turn_context` events
8. Parse processing time in `src/kimi.rs` — extract from `StatusUpdate` timestamps
9. Parse processing time in `src/gemini.rs` — extract from message timestamps
10. Update tests in `usage.rs`
11. Run `cargo fmt` and `cargo clippy`
12. Update README example output

## Edge Cases

- If `processing_time_ms == 0`, display TPS as `—` (not applicable)
- If TPS would overflow a reasonable range, cap display at 999.9b (just use compact formatting like tokens)
- Sessions with no timing data are skipped in TPS calculation (only calls with valid timestamps contribute)
