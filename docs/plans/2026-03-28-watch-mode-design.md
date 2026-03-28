# Watch Mode Design

## Summary

Add a `--watch` mode to `toll` that tracks usage deltas from the moment watch starts until the user stops it. In table mode, `toll` redraws a live summary in place. In JSON and CSV modes, it stays quiet while capturing and emits one final delta snapshot on exit.

## Goals

- Report incremental token and query usage between watch start and watch end
- Include growth from sessions that already existed before watch started
- Preserve the current output formats and table layout
- Keep one-shot behavior unchanged when `--watch` is not used

## Non-Goals

- Streaming JSON or CSV updates during capture
- File-tail or offset-based log watching
- Support for combining `--watch` with `--today` or `--days`

## CLI Contract

### New Flag

- `--watch`: enter bounded watch mode until interrupted

### Supported Combinations

- `--watch` with agent filters such as `--claude`, `--codex`, `--kimi`, `--gemini`
- `--watch` with `--detail`
- `--watch` with `--json` or `--csv`
- `--watch` with `--by-day`

### Invalid Combinations

- `--watch --today`
- `--watch --days N`

These combinations should be rejected by clap as invalid because watch mode already defines its own time window: from process start until interruption.

## Runtime Behavior

### Table Mode

When `--watch` is enabled without `--json` or `--csv`:

1. Capture a baseline snapshot at watch start
2. Rescan on a fixed interval
3. Compute deltas relative to the baseline
4. Clear and redraw the normal summary table using delta values
5. Show a short status line such as `Watching since ... Press Ctrl-C to stop.`
6. On `Ctrl-C`, perform one final scan, print the final delta table once, and exit with code `0`

### JSON and CSV Modes

When `--watch` is combined with `--json` or `--csv`:

- Capture the baseline snapshot
- Continue rescanning on the watch interval
- Do not print intermediate updates
- On `Ctrl-C`, perform one final scan and emit exactly one final delta payload

### By-Day Mode

`--by-day --watch` is allowed. It groups the post-start delta by local calendar day. Most watch sessions produce one row for the current day. Sessions that cross midnight produce multiple rows.

## Implementation Approach

Use baseline snapshot diffing instead of tailing files.

### Why This Approach

- It matches the requested meaning of watch mode: usage between watch start and watch end
- It correctly includes incremental growth for sessions that were already active
- It fits the current scan-based architecture
- It avoids provider-specific complexity around file offsets, rotation, and cumulative versus incremental log records

## Internal Data Model

The current code aggregates directly into `TokenUsage` and `DailyUsage`. Watch mode needs a session-granular snapshot so it can diff current usage against the baseline.

### New Structures

- `SessionUsage`: cumulative usage for one logical session
- `AgentSnapshot`: `session_id -> SessionUsage`

Each agent should expose enough information to build a stable per-session snapshot keyed by a logical session identifier, such as a path-derived session id.

### Delta Computation

For each refresh:

1. Build a fresh `AgentSnapshot`
2. For each session in the current snapshot:
   - If the session existed at baseline, subtract baseline counters from current counters
   - If the session is new, use the full current counters
3. Clamp negative per-field deltas to zero if a session log shrinks or resets
4. Aggregate the per-session deltas back into the existing `TokenUsage` and `DailyUsage` shapes

This keeps the renderers unchanged. Table, JSON, and CSV code still consume the same aggregated types they already understand.

## Refresh Loop

The watch loop should:

1. Build the baseline snapshot once
2. Sleep for a fixed interval, likely 2 seconds by default
3. Rebuild current snapshots
4. Produce aggregated delta usage
5. Render or hold the latest result depending on output mode
6. Exit cleanly on interrupt after one final scan

The interval can stay internal for the first version. A configurable interval can be added later if needed.

## Error Handling

- Missing agent directories continue to behave as zero-usage sources
- If one session file is unreadable during a refresh, skip that session for that refresh instead of aborting the whole watch
- If a session counter regresses, clamp that session delta to zero for the current refresh
- If a final scan on interrupt fails for one provider, return the best available final snapshot rather than discarding all collected data

## Testing Plan

### CLI Parsing

- `--watch` parses successfully
- `--watch --by-day` parses successfully
- `--watch --today` is rejected
- `--watch --days 7` is rejected

### Snapshot Diffing

- Existing session grows after baseline
- New session appears after baseline
- Unchanged session yields zero delta
- Counter regression clamps to zero

### Rendering

- Delta data renders through the existing summary table path
- Delta data renders through the existing daily table path
- JSON and CSV final payloads preserve current schema and headings

### Signal Handling

An end-to-end interrupt test is optional. Core correctness should live in pure diffing tests so watch semantics stay easy to verify without relying on OS-level signal timing.

## Expected Code Changes

- Extend CLI args in `src/main.rs` with `--watch` and conflicts for `--today` and `--days`
- Add watch-loop orchestration in `src/main.rs`
- Introduce session-level snapshot types and diff helpers in shared code
- Update each agent implementation to produce session snapshots
- Reuse existing display and output renderers by feeding them aggregated delta usage

## Future Extensions

- Configurable refresh interval
- Streaming JSON lines mode for machine consumers
- Highlighting which agents changed since the previous refresh
