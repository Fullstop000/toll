# toll

[![crates.io](https://img.shields.io/crates/v/toll.svg)](https://crates.io/crates/toll)

Token usage statistics for CLI coding agents — Claude Code and Codex CLI.

The name is a double metaphor: tokens are the *toll* you pay to use AI coding agents, and heavy usage *takes a toll*.

## Features

- Tracks token usage from **Claude Code** (`~/.claude/projects/`) and **Codex CLI** (`~/.codex/sessions/`)
- Shows input, output, cached tokens, cache hit rate, and estimated USD cost
- Uses compact `k` / `m` / `b` token units by default, with `--detail` for raw counts
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

### Quick Install (Linux/macOS)

Downloads the latest GitHub Release for your platform. If a prebuilt binary is not available, the installer falls back to `cargo install toll`.

```sh
curl -fsSL https://raw.githubusercontent.com/Fullstop000/toll/refs/heads/master/install.sh | sh
```

Install to a custom directory:

```sh
curl -fsSL https://raw.githubusercontent.com/Fullstop000/toll/refs/heads/master/install.sh | TOLL_INSTALL_DIR="$HOME/.local/bin" sh
```

### Cargo Install

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
toll --detail       # show full token counts
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
  Input tokens                          200.4m          111.4m          311.8m
    ↳ cached                     194.0m ( 96.8%)  103.8m ( 93.2%)  297.8m ( 95.5%)
    ↳ net (non-cached)                  6.5m             7.5m            14.0m
  Output tokens                          766.9k          578.5k            1.3m
  ─────────────────────────────────────────────────────────────────────────────────
  Total tokens                          201.2m          111.9m          313.1m
  ─────────────────────────────────────────────────────────────────────────────────
  Estimated cost (USD)                  $91.29           $28.17          $119.46

  By model:
  ─────────────────────────────────────────────────────────────────────────────────
  Model                                 Tokens           Output             Cost
  ─────────────────────────────────────────────────────────────────────────────────
  claude-haiku-4-5-20251001              6.8m            22.0k            $1.72
  claude-opus-4-6                        1.0m            19.2k            $2.09
  claude-sonnet-4-6                    194.1m           725.7k           $87.49
  gpt-5.4                              111.9m           578.5k           $28.17
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
