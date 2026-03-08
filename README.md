# toll

Token usage statistics for CLI coding agents — Claude Code and Codex CLI.

The name is a double metaphor: tokens are the *toll* you pay to use AI coding agents, and heavy usage *takes a toll*.

## Features

- Tracks token usage from **Claude Code** (`~/.claude/projects/`) and **Codex CLI** (`~/.codex/sessions/`)
- Shows input, output, cached tokens and cache hit rate
- Filter by today, last N days, or all time
- View per-tool or combined stats

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
Collected: 2026-03-09 00:24:26 +08:00

                                   Claude Code            Codex         Combined
  ═════════════════════════════════════════════════════════════════════════════════
  Sessions                                  21               14               35
  ─────────────────────────────────────────────────────────────────────────────────
  Input tokens                     166,542,631       96,880,203      263,422,834
    ↳ cached                   160,842,732 ( 96.6%)  90,152,704 ( 93.1%)  250,995,436 ( 95.3%)
    ↳ net (non-cached)               5,699,899        6,727,499       12,427,398
  Output tokens                        582,892          537,407        1,120,299
  ─────────────────────────────────────────────────────────────────────────────────
  Total tokens                     167,125,523       97,417,610      264,543,133
```

## Data sources

| Tool | Log path | Token field |
|------|----------|-------------|
| Claude Code | `~/.claude/projects/**/*.jsonl` | `message.usage` per API call |
| Codex CLI | `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl` | last `token_count` event per session |

## License

MIT
