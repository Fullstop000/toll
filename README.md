# toll

[![crates.io](https://img.shields.io/crates/v/toll.svg)](https://crates.io/crates/toll)

Token usage statistics for CLI coding agents — Claude Code and Codex CLI.

The name is a double metaphor: tokens are the *toll* you pay to use AI coding agents, and heavy usage *takes a toll*.

## Features

- Tracks token usage from **Claude Code** (`~/.claude/projects/`) and **Codex CLI** (`~/.codex/sessions/`)
- Shows input, output, cached tokens, cache hit rate, and estimated USD cost
- Per-model breakdown across all sessions
- Filter by today, last N days, or all time
- View per-tool or combined stats
- List all supported model prices with `--list-prices`

## Supported versions

| Tool | Supported versions | Log format |
|------|--------------------|------------|
| [Claude Code](https://github.com/anthropics/claude-code) | All versions (JSONL logs present since launch) | `message.usage` entries in `~/.claude/projects/**/*.jsonl` |
| [Codex CLI](https://github.com/openai/codex) | ≥ 0.1 (rollout log format) | `token_count` events in `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl` |

## Installation

### One-liner (recommended)

Requires [Rust](https://rustup.rs/).

```sh
cargo install toll
```

### From source

```sh
git clone https://github.com/Fullstop000/toll.git
cd toll
cargo install --path .
```

## Usage

```sh
toll                # all-time stats
toll --today        # today only
toll --days 7       # last 7 days
toll --claude       # Claude Code only
toll --codex        # Codex CLI only
toll --list-prices  # show all supported model prices
```

## Example output

```
Token usage — all time
Collected: 2026-03-09 01:44:23 +08:00

                                   Claude Code            Codex         Combined
  ═════════════════════════════════════════════════════════════════════════════════
  Sessions                                  24               14               38
  ─────────────────────────────────────────────────────────────────────────────────
  Input tokens                     200,440,264      111,363,538      311,803,802
    ↳ cached                   193,981,784 ( 96.8%)  103,838,208 ( 93.2%)  297,819,992 ( 95.5%)
    ↳ net (non-cached)               6,458,480        7,525,330       13,983,810
  Output tokens                        766,870          578,486        1,345,356
  ─────────────────────────────────────────────────────────────────────────────────
  Total tokens                     201,207,134      111,942,024      313,149,158
  ─────────────────────────────────────────────────────────────────────────────────
  Estimated cost (USD)                  $91.29           $28.17          $119.46

  By model:
  ─────────────────────────────────────────────────────────────────────────────────
  Model                                 Tokens           Output             Cost
  ─────────────────────────────────────────────────────────────────────────────────
  claude-haiku-4-5-20251001          6,838,087           21,964            $1.72
  claude-opus-4-6                    1,010,175           19,191            $2.09
  claude-sonnet-4-6                193,358,872          725,715           $87.49
  gpt-5.4                          111,942,024          578,486           $28.17
  ─────────────────────────────────────────────────────────────────────────────────

  Scanned 38 session(s) in 1.00s
```

## Data sources

| Tool | Log path | Token field |
|------|----------|-------------|
| Claude Code | `~/.claude/projects/**/*.jsonl` | `message.usage` per API call — sums `input_tokens`, `cache_creation_input_tokens`, `cache_read_input_tokens`, `output_tokens` |
| Codex CLI | `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl` | last `token_count` event per session (cumulative totals) |

## License

MIT
