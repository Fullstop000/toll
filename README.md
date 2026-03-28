# toll

[![crates.io](https://img.shields.io/crates/v/toll.svg)](https://crates.io/crates/toll)

Token and usage statistics for local AI agents.

The name is a double metaphor: tokens are the *toll* you pay to use AI coding agents, and heavy usage *takes a toll*.

## Features

- Shows input, output, cached tokens, cache hit rate, and estimated USD cost
- Counts top-level user queries across supported tools
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
| [Kimi Code](https://www.kimi.com/code) | All versions | `StatusUpdate` token_usage events in `~/.kimi/sessions/**/<session>/wire.jsonl` |
| [Gemini](https://github.com/google/gemini-cli) | ≥ 0.5 (antigravity log format) | `tokens` entries in `~/.gemini/tmp/**/chats/*.json` |

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
Token usage statistics for Claude Code, Codex CLI, Kimi Code, and Gemini

Usage: toll [OPTIONS]

Options:
  -v, --version      Show version information and exit
      --today        Show today's usage only
      --days <N>     Show last N days
      --watch        Watch usage deltas from now until interrupted
      --claude       Show Claude stats only
      --codex        Show Codex stats only
      --kimi         Show Kimi Code stats only
      --gemini       Show Gemini stats only
      --json         Emit JSON to stdout
      --csv          Emit CSV to stdout
      --list-prices  List all supported models and their prices, then exit
      --detail       Show full token counts instead of compact b/m/k units
      --by-day       Show usage aggregated by day
  -h, --help         Print help

Examples:
  toll              # all-time stats
  toll --today      # today only
  toll --days 7     # last 7 days
  toll --by-day --days 7  # daily summary table
  toll --watch      # live delta stats until Ctrl-C
  toll --watch --json  # final JSON delta on exit
  toll --watch --by-day  # live per-day delta stats
  toll --claude     # Claude only
  toll --codex      # Codex only
  toll --kimi       # Kimi Code only
  toll --gemini     # Gemini only
  toll --detail     # full token counts
  toll --json       # machine-readable JSON
  toll --csv        # terminal CSV export
```

Watch mode reports only usage accumulated after watch start, including additional activity on sessions that were already open.

## Example output

```text
Token usage — all time
Collected: 2026-03-13 20:20:38 +08:00

              Sessions  Queries   Input  Cached  Hit Rate  Net Input  Output   Total      Cost     TPS
  ════════════════════════════════════════════════════════════════════════════════════════
  Claude Code       38      141  316.1m  304.2m     96.2%      11.9m    1.3m  317.5m  $168.59   1.2k
  Codex             21       62  148.9m  137.4m     92.3%      11.5m  777.8k  149.7m   $74.70   6.5k
  Kimi Code         15       44   34.6m   32.4m     93.6%       2.2m  206.2k   34.8m    $6.70   8.1k
  Gemini            10       31   28.5m   25.8m     90.5%       2.7m  185.5k   28.7m    $2.15   4.2k
  ───────────────────────────────────────────────────────────────────────────────────────────────────────────────────────
  Combined          84      278  528.1m  499.8m     94.6%      28.3m    2.5m  530.6m  $252.14   3.8k
  ───────────────────────────────────────────────────────────────────────────────────────────────────────────────────────

  By model:
  ─────────────────────────────────────────────────────────────────────────────────
  Model                                 Tokens           Output             Cost        TPS
  ─────────────────────────────────────────────────────────────────────────────────────────────────
  claude-haiku-4-5-20251001               9.4m            40.5k            $2.71        2.3k
  claude-opus-4-6                        49.7m           317.2k           $45.17       0.9k
  claude-sonnet-4-6                     258.3m           988.2k          $120.72       1.4k
  gemini-3-flash-preview                 28.7m           185.5k            $2.15       4.2k
  gpt-5.4                               149.7m           777.8k           $74.70       6.5k
  kimi-for-coding                        34.8m           206.2k            $6.70       8.1k
  ─────────────────────────────────────────────────────────────────────────────────────────────────

  Scanned 114 session(s) in 1.42s
```

## Data sources

| Tool | Log path | Token field |
|------|----------|-------------|
| Claude Code | `~/.claude/projects/**/*.jsonl` | `message.usage` per API call — sums `input_tokens`, `cache_creation_input_tokens`, `cache_read_input_tokens`, `output_tokens` |
| Codex CLI | `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl` | last `token_count` event per session (cumulative totals) |
| Kimi Code | `~/.kimi/sessions/**/<session>/wire.jsonl` | `StatusUpdate` events — sums `input_other`, `input_cache_read`, `input_cache_creation`, `output` per API call |
| Gemini | `~/.gemini/tmp/**/chats/*.json` | `tokens` per agent message — sums `input`, `cached`, `output` per call; totals can differ from Gemini CLI `/stats model` because Gemini CLI also counts internal utility-model requests, while `toll` reads only persisted chat turns |

## License

MIT
