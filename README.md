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

```text
Token usage statistics for Claude Code and Codex CLI

Usage: toll [OPTIONS]

Options:
  -v, --version      Show version information and exit
      --today        Show today's usage only
      --days <N>     Show last N days
      --claude       Show Claude stats only
      --codex        Show Codex stats only
      --list-prices  List all supported models and their prices, then exit
      --detail       Show full token counts instead of compact b/m/k units
      --by-day       Show usage aggregated by day
  -h, --help         Print help

Examples:
  toll              # all-time stats
  toll --today      # today only
  toll --days 7     # last 7 days
  toll --by-day --days 7  # daily summary table
  toll --claude     # Claude only
  toll --codex      # Codex only
  toll --detail     # full token counts
```

## Example output

```text
Token usage — all time
Collected: 2026-03-13 00:57:37 +08:00

              Sessions   Input  Cached  Hit Rate  Net Input  Output   Total      Cost 
  ════════════════════════════════════════════════════════════════════════════════════
  Claude Code       17   69.8m   59.1m     84.7%      10.7m  282.0k   70.1m    $79.36 
  Codex             87  615.5m  579.7m     94.2%      35.8m    3.2m  618.7m  $219.18* 
  ────────────────────────────────────────────────────────────────────────────────────
  Combined         104  685.3m  638.8m     93.2%      46.5m    3.5m  688.8m  $298.54* 
  ────────────────────────────────────────────────────────────────────────────────────

  * pricing unavailable for 1 session(s) — cost is understated

  By model:
  ─────────────────────────────────────────────────────────────────────────────────
  Model                                 Tokens           Output             Cost
  ─────────────────────────────────────────────────────────────────────────────────
  claude-haiku-4-5-20251001               3.4m            22.7k            $1.38
  claude-opus-4-6                        42.4m           138.6k           $51.20
  claude-sonnet-4-6                      24.4m           120.8k           $26.78
  gpt-5-codex                             1.4m            33.8k            $0.77
  gpt-5.1-codex                           2.4m            17.4k            $0.67
  gpt-5.1-codex-max                      20.4m           124.0k            $5.42
  gpt-5.2-codex                           4.5m           113.6k            $3.21
  gpt-5.3-codex                         470.7m             2.4m          $155.90
  gpt-5.4                               116.9m           470.5k           $53.20
  ─────────────────────────────────────────────────────────────────────────────────

  Scanned 104 session(s) in 0.48s
```

## Data sources

| Tool | Log path | Token field |
|------|----------|-------------|
| Claude Code | `~/.claude/projects/**/*.jsonl` | `message.usage` per API call — sums `input_tokens`, `cache_creation_input_tokens`, `cache_read_input_tokens`, `output_tokens` |
| Codex CLI | `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl` | last `token_count` event per session (cumulative totals) |

## License

MIT
