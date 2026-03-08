use chrono::{DateTime, Local, TimeZone, Utc};
use clap::Parser;
use serde_json::Value;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

#[derive(Parser)]
#[command(name = "toll", about = "Token usage statistics for Claude Code and Codex CLI")]
#[command(after_help = "Examples:
  toll              # all-time stats
  toll --today      # today only
  toll --days 7     # last 7 days
  toll --claude     # Claude only
  toll --codex      # Codex only")]
struct Args {
    #[arg(long, conflicts_with = "days", help = "Show today's usage only")]
    today: bool,

    #[arg(long, value_name = "N", help = "Show last N days")]
    days: Option<u32>,

    #[arg(long, conflicts_with = "codex", help = "Show Claude stats only")]
    claude: bool,

    #[arg(long, conflicts_with = "claude", help = "Show Codex stats only")]
    codex: bool,
}

#[derive(Default, Debug)]
struct TokenUsage {
    input_tokens: u64,
    cached_input_tokens: u64,
    output_tokens: u64,
    sessions: u32,
}

impl TokenUsage {
    fn add(&mut self, other: &TokenUsage) {
        self.input_tokens += other.input_tokens;
        self.cached_input_tokens += other.cached_input_tokens;
        self.output_tokens += other.output_tokens;
        self.sessions += other.sessions;
    }

    fn total_tokens(&self) -> u64 {
        self.input_tokens + self.output_tokens
    }

    fn net_input_tokens(&self) -> u64 {
        self.input_tokens.saturating_sub(self.cached_input_tokens)
    }
}

fn home_dir() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/root"))
}

/// Parse Codex token_count events from any BufRead source.
/// Returns usage from the last token_count event (cumulative total for the session).
fn parse_codex_lines(reader: impl BufRead) -> Option<TokenUsage> {
    let mut last_total: Option<Value> = None;

    for line in reader.lines().map_while(Result::ok) {
        let line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }
        let Ok(v): Result<Value, _> = serde_json::from_str(&line) else {
            continue;
        };

        if v.get("type").and_then(|t| t.as_str()) == Some("event_msg") {
            let Some(payload) = v.get("payload") else {
                continue;
            };
            if payload.get("type").and_then(|t| t.as_str()) == Some("token_count") {
                if let Some(total) = payload
                    .get("info")
                    .and_then(|i| i.get("total_token_usage"))
                {
                    last_total = Some(total.clone());
                }
            }
        }
    }

    let total = last_total?;
    Some(TokenUsage {
        input_tokens: total.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
        cached_input_tokens: total
            .get("cached_input_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0),
        output_tokens: total.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
        sessions: 1,
    })
}

/// Parse a Codex session file, return total token usage from last token_count event.
fn parse_codex_session(path: &Path) -> Option<TokenUsage> {
    let file = fs::File::open(path).ok()?;
    parse_codex_lines(BufReader::new(file))
}

/// Extract UTC timestamp from Codex session filename.
/// Filename format: rollout-YYYY-MM-DDTHH-MM-SS-<uuid>.jsonl
fn codex_session_date(path: &Path) -> Option<DateTime<Utc>> {
    let stem = path.file_name()?.to_str()?;
    // strip "rollout-" prefix, then parse "YYYY-MM-DDTHH-MM-SS"
    let rest = stem.strip_prefix("rollout-")?;
    // replace dashes in time part: YYYY-MM-DDTHH-MM-SS -> YYYY-MM-DDTHH:MM:SS
    // The date portion is 10 chars (YYYY-MM-DD), T is index 10, then HH-MM-SS
    if rest.len() < 19 {
        return None;
    }
    let ts_raw = &rest[..19]; // "YYYY-MM-DDTHH-MM-SS"
    let ts_str = format!(
        "{}:{}:{}Z",
        &ts_raw[..13], // "YYYY-MM-DDTHH"
        &ts_raw[14..16],
        &ts_raw[17..19]
    );
    ts_str.parse::<DateTime<Utc>>().ok()
}

fn collect_codex_usage(since: Option<DateTime<Utc>>) -> TokenUsage {
    let sessions_dir = home_dir().join(".codex").join("sessions");
    let mut total = TokenUsage::default();

    if !sessions_dir.exists() {
        return total;
    }

    for entry in WalkDir::new(&sessions_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_type().is_file()
                && e.file_name()
                    .to_str()
                    .map(|n| n.starts_with("rollout-") && n.ends_with(".jsonl"))
                    .unwrap_or(false)
        })
    {
        let path = entry.path();

        if let Some(since_dt) = since {
            if let Some(session_date) = codex_session_date(path) {
                if session_date < since_dt {
                    continue;
                }
            }
        }

        if let Some(usage) = parse_codex_session(path) {
            total.add(&usage);
        }
    }

    total
}

/// Parse Claude usage entries from any BufRead source.
fn parse_claude_lines(reader: impl BufRead, since: Option<DateTime<Utc>>) -> TokenUsage {
    let mut usage = TokenUsage { sessions: 1, ..Default::default() };

    for line in reader.lines().map_while(Result::ok) {
        let line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }
        let Ok(v): Result<Value, _> = serde_json::from_str(&line) else {
            continue;
        };

        // Date filter
        if let Some(since_dt) = since {
            let ts_str = v.get("timestamp").and_then(|t| t.as_str()).or_else(|| {
                v.get("message")
                    .and_then(|m| m.get("timestamp"))
                    .and_then(|t| t.as_str())
            });
            if let Some(ts) = ts_str {
                if let Ok(dt) = ts.parse::<DateTime<Utc>>() {
                    if dt < since_dt {
                        continue;
                    }
                }
            }
        }

        let Some(msg) = v.get("message") else { continue };
        let Some(u) = msg.get("usage") else { continue };

        let inp = u.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
        let cache_create = u
            .get("cache_creation_input_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let cache_read = u
            .get("cache_read_input_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let out = u.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0);

        usage.input_tokens += inp + cache_create + cache_read;
        usage.cached_input_tokens += cache_read;
        usage.output_tokens += out;
    }

    usage
}

/// Parse a Claude session file, summing all message.usage entries.
fn parse_claude_session(path: &Path, since: Option<DateTime<Utc>>) -> TokenUsage {
    let Ok(file) = fs::File::open(path) else {
        return TokenUsage { sessions: 1, ..Default::default() };
    };
    parse_claude_lines(BufReader::new(file), since)
}

fn collect_claude_usage(since: Option<DateTime<Utc>>) -> TokenUsage {
    let projects_dir = home_dir().join(".claude").join("projects");
    let mut total = TokenUsage::default();

    if !projects_dir.exists() {
        return total;
    }

    for entry in WalkDir::new(&projects_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_type().is_file()
                && e.file_name()
                    .to_str()
                    .map(|n| n.ends_with(".jsonl"))
                    .unwrap_or(false)
        })
    {
        let usage = parse_claude_session(entry.path(), since);
        if usage.total_tokens() > 0 {
            total.add(&usage);
        }
    }

    total
}

fn fmt_num(n: u64) -> String {
    let s = n.to_string();
    let mut result = String::new();
    for (i, ch) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push(',');
        }
        result.push(ch);
    }
    result.chars().rev().collect()
}

fn fmt_pct(part: u64, total: u64) -> String {
    if total == 0 {
        return "  0.0%".to_string();
    }
    format!("{:5.1}%", part as f64 / total as f64 * 100.0)
}

fn print_table(claude: &TokenUsage, codex: &TokenUsage) {
    let combined = TokenUsage {
        input_tokens: claude.input_tokens + codex.input_tokens,
        cached_input_tokens: claude.cached_input_tokens + codex.cached_input_tokens,
        output_tokens: claude.output_tokens + codex.output_tokens,
        sessions: claude.sessions + codex.sessions,
    };

    let col_w = 15usize;
    let label_w = 28usize;
    let total_w = label_w + 2 + (col_w + 2) * 3;

    let row = |label: &str, c: &str, d: &str, t: &str| {
        println!(
            "  {:<lw$} {:>cw$}  {:>cw$}  {:>cw$}",
            label, c, d, t,
            lw = label_w,
            cw = col_w
        );
    };

    println!();
    println!(
        "  {:<lw$} {:>cw$}  {:>cw$}  {:>cw$}",
        "", "Claude Code", "Codex", "Combined",
        lw = label_w, cw = col_w
    );
    println!("  {}", "═".repeat(total_w));

    row(
        "Sessions",
        &fmt_num(claude.sessions as u64),
        &fmt_num(codex.sessions as u64),
        &fmt_num(combined.sessions as u64),
    );
    println!("  {}", "─".repeat(total_w));

    row(
        "Input tokens",
        &fmt_num(claude.input_tokens),
        &fmt_num(codex.input_tokens),
        &fmt_num(combined.input_tokens),
    );
    row(
        "  ↳ cached",
        &format!(
            "{} ({})",
            fmt_num(claude.cached_input_tokens),
            fmt_pct(claude.cached_input_tokens, claude.input_tokens)
        ),
        &format!(
            "{} ({})",
            fmt_num(codex.cached_input_tokens),
            fmt_pct(codex.cached_input_tokens, codex.input_tokens)
        ),
        &format!(
            "{} ({})",
            fmt_num(combined.cached_input_tokens),
            fmt_pct(combined.cached_input_tokens, combined.input_tokens)
        ),
    );
    row(
        "  ↳ net (non-cached)",
        &fmt_num(claude.net_input_tokens()),
        &fmt_num(codex.net_input_tokens()),
        &fmt_num(combined.net_input_tokens()),
    );
    row(
        "Output tokens",
        &fmt_num(claude.output_tokens),
        &fmt_num(codex.output_tokens),
        &fmt_num(combined.output_tokens),
    );
    println!("  {}", "─".repeat(total_w));
    row(
        "Total tokens",
        &fmt_num(claude.total_tokens()),
        &fmt_num(codex.total_tokens()),
        &fmt_num(combined.total_tokens()),
    );
    println!();
}

fn print_single(label: &str, usage: &TokenUsage) {
    let col_w = 15usize;
    let label_w = 28usize;
    let total_w = label_w + 2 + col_w;
    println!();
    println!("  {:<lw$} {:>cw$}", "", label, lw = label_w, cw = col_w);
    println!("  {}", "═".repeat(total_w));
    println!(
        "  {:<lw$} {:>cw$}",
        "Sessions",
        fmt_num(usage.sessions as u64),
        lw = label_w,
        cw = col_w
    );
    println!("  {}", "─".repeat(total_w));
    println!(
        "  {:<lw$} {:>cw$}",
        "Input tokens",
        fmt_num(usage.input_tokens),
        lw = label_w,
        cw = col_w
    );
    println!(
        "  {:<lw$} {:>cw$} ({})",
        "  ↳ cached",
        fmt_num(usage.cached_input_tokens),
        fmt_pct(usage.cached_input_tokens, usage.input_tokens),
        lw = label_w,
        cw = col_w
    );
    println!(
        "  {:<lw$} {:>cw$}",
        "  ↳ net (non-cached)",
        fmt_num(usage.net_input_tokens()),
        lw = label_w,
        cw = col_w
    );
    println!(
        "  {:<lw$} {:>cw$}",
        "Output tokens",
        fmt_num(usage.output_tokens),
        lw = label_w,
        cw = col_w
    );
    println!("  {}", "─".repeat(total_w));
    println!(
        "  {:<lw$} {:>cw$}",
        "Total tokens",
        fmt_num(usage.total_tokens()),
        lw = label_w,
        cw = col_w
    );
    println!();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    // ── helpers ──────────────────────────────────────────────────────────────

    fn cursor(s: &str) -> Cursor<Vec<u8>> {
        Cursor::new(s.as_bytes().to_vec())
    }

    // ── TokenUsage ───────────────────────────────────────────────────────────

    #[test]
    fn token_usage_add() {
        let mut a = TokenUsage { input_tokens: 100, cached_input_tokens: 80, output_tokens: 20, sessions: 1 };
        let b = TokenUsage { input_tokens: 50, cached_input_tokens: 10, output_tokens: 5, sessions: 1 };
        a.add(&b);
        assert_eq!(a.input_tokens, 150);
        assert_eq!(a.cached_input_tokens, 90);
        assert_eq!(a.output_tokens, 25);
        assert_eq!(a.sessions, 2);
    }

    #[test]
    fn token_usage_total_and_net() {
        let u = TokenUsage { input_tokens: 1000, cached_input_tokens: 800, output_tokens: 200, sessions: 1 };
        assert_eq!(u.total_tokens(), 1200);
        assert_eq!(u.net_input_tokens(), 200);
    }

    #[test]
    fn token_usage_net_no_underflow() {
        // cached > input should not underflow
        let u = TokenUsage { input_tokens: 10, cached_input_tokens: 20, output_tokens: 0, sessions: 1 };
        assert_eq!(u.net_input_tokens(), 0);
    }

    // ── codex_session_date ───────────────────────────────────────────────────

    #[test]
    fn codex_session_date_parses_correctly() {
        let path = Path::new("/home/user/.codex/sessions/2026/03/08/rollout-2026-03-08T20-55-09-019ccd84-0e5f-7870-9c33-097188e35a30.jsonl");
        let dt = codex_session_date(path).expect("should parse");
        assert_eq!(dt.to_rfc3339(), "2026-03-08T20:55:09+00:00");
    }

    #[test]
    fn codex_session_date_rejects_non_rollout() {
        let path = Path::new("/home/user/.codex/sessions/2026/03/08/other-file.jsonl");
        assert!(codex_session_date(path).is_none());
    }

    #[test]
    fn codex_session_date_rejects_short_name() {
        let path = Path::new("/home/user/.codex/sessions/2026/03/08/rollout-short.jsonl");
        assert!(codex_session_date(path).is_none());
    }

    // ── parse_codex_lines ────────────────────────────────────────────────────

    #[test]
    fn codex_parses_last_token_count() {
        let data = r#"
{"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":100,"cached_input_tokens":80,"output_tokens":10,"total_tokens":110},"last_token_usage":{}}}}
{"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":200,"cached_input_tokens":150,"output_tokens":25,"total_tokens":225},"last_token_usage":{}}}}
"#;
        let usage = parse_codex_lines(cursor(data)).expect("should parse");
        // must use the LAST token_count, not the first
        assert_eq!(usage.input_tokens, 200);
        assert_eq!(usage.cached_input_tokens, 150);
        assert_eq!(usage.output_tokens, 25);
        assert_eq!(usage.sessions, 1);
    }

    #[test]
    fn codex_ignores_non_token_count_events() {
        let data = r#"
{"type":"event_msg","payload":{"type":"task_started","turn_id":"abc"}}
{"type":"response_item","payload":{"type":"message","role":"assistant"}}
"#;
        assert!(parse_codex_lines(cursor(data)).is_none());
    }

    #[test]
    fn codex_skips_malformed_lines() {
        let data = "not json\n{\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"info\":{\"total_token_usage\":{\"input_tokens\":50,\"cached_input_tokens\":0,\"output_tokens\":5}}}}\n";
        let usage = parse_codex_lines(cursor(data)).expect("should parse despite bad line");
        assert_eq!(usage.input_tokens, 50);
    }

    #[test]
    fn codex_empty_input_returns_none() {
        assert!(parse_codex_lines(cursor("")).is_none());
    }

    // ── parse_claude_lines ───────────────────────────────────────────────────

    fn make_claude_line(ts: &str, inp: u64, cache_create: u64, cache_read: u64, out: u64) -> String {
        serde_json::json!({
            "timestamp": ts,
            "message": {
                "role": "assistant",
                "usage": {
                    "input_tokens": inp,
                    "cache_creation_input_tokens": cache_create,
                    "cache_read_input_tokens": cache_read,
                    "output_tokens": out
                }
            }
        })
        .to_string()
    }

    #[test]
    fn claude_sums_all_messages() {
        let data = format!(
            "{}\n{}\n",
            make_claude_line("2026-03-09T01:00:00Z", 100, 50, 200, 30),
            make_claude_line("2026-03-09T02:00:00Z", 80,  20, 100, 15),
        );
        let usage = parse_claude_lines(cursor(&data), None);
        // input = (100+50+200) + (80+20+100) = 350 + 200 = 550
        assert_eq!(usage.input_tokens, 550);
        // cached = cache_read only: 200 + 100 = 300
        assert_eq!(usage.cached_input_tokens, 300);
        assert_eq!(usage.output_tokens, 45);
        assert_eq!(usage.sessions, 1);
    }

    #[test]
    fn claude_date_filter_excludes_old_entries() {
        let data = format!(
            "{}\n{}\n",
            make_claude_line("2026-03-08T12:00:00Z", 500, 0, 0, 50),
            make_claude_line("2026-03-09T12:00:00Z", 100, 0, 0, 10),
        );
        let since: DateTime<Utc> = "2026-03-09T00:00:00Z".parse().unwrap();
        let usage = parse_claude_lines(cursor(&data), Some(since));
        assert_eq!(usage.input_tokens, 100);
        assert_eq!(usage.output_tokens, 10);
    }

    #[test]
    fn claude_date_filter_includes_exact_boundary() {
        let data = format!("{}\n", make_claude_line("2026-03-09T00:00:00Z", 100, 0, 0, 10));
        let since: DateTime<Utc> = "2026-03-09T00:00:00Z".parse().unwrap();
        let usage = parse_claude_lines(cursor(&data), Some(since));
        assert_eq!(usage.input_tokens, 100);
    }

    #[test]
    fn claude_skips_lines_without_usage() {
        let data = "{\"type\":\"file-history-snapshot\",\"messageId\":\"abc\"}\n";
        let usage = parse_claude_lines(cursor(data), None);
        assert_eq!(usage.total_tokens(), 0);
    }

    #[test]
    fn claude_skips_malformed_lines() {
        let data = format!("bad json\n{}\n", make_claude_line("2026-03-09T01:00:00Z", 100, 0, 0, 10));
        let usage = parse_claude_lines(cursor(&data), None);
        assert_eq!(usage.input_tokens, 100);
    }

    // ── fmt_num ───────────────────────────────────────────────────────────────

    #[test]
    fn fmt_num_formats_with_commas() {
        assert_eq!(fmt_num(0), "0");
        assert_eq!(fmt_num(999), "999");
        assert_eq!(fmt_num(1_000), "1,000");
        assert_eq!(fmt_num(1_234_567), "1,234,567");
        assert_eq!(fmt_num(1_000_000_000), "1,000,000,000");
    }

    // ── fmt_pct ───────────────────────────────────────────────────────────────

    #[test]
    fn fmt_pct_zero_total() {
        assert_eq!(fmt_pct(0, 0), "  0.0%");
    }

    #[test]
    fn fmt_pct_full() {
        assert_eq!(fmt_pct(100, 100), "100.0%");
    }

    #[test]
    fn fmt_pct_half() {
        assert_eq!(fmt_pct(50, 100), " 50.0%");
    }
}

fn main() {
    let args = Args::parse();

    let now = Utc::now();
    let since: Option<DateTime<Utc>>;
    let period: String;

    if args.today {
        let local_today = Local::now().date_naive().and_hms_opt(0, 0, 0).unwrap();
        since = Some(
            Local
                .from_local_datetime(&local_today)
                .unwrap()
                .with_timezone(&Utc),
        );
        period = "today".to_string();
    } else if let Some(days) = args.days {
        since = Some(now - chrono::Duration::days(days as i64));
        period = format!("last {} days", days);
    } else {
        since = None;
        period = "all time".to_string();
    }

    println!("\nToken usage — {}", period);
    println!("Collected: {}", Local::now().format("%Y-%m-%d %H:%M:%S %Z"));

    let show_claude = !args.codex;
    let show_codex = !args.claude;

    let claude_usage = if show_claude {
        collect_claude_usage(since)
    } else {
        TokenUsage::default()
    };

    let codex_usage = if show_codex {
        collect_codex_usage(since)
    } else {
        TokenUsage::default()
    };

    if !show_claude {
        print_single("Codex", &codex_usage);
    } else if !show_codex {
        print_single("Claude Code", &claude_usage);
    } else {
        print_table(&claude_usage, &codex_usage);
    }
}
