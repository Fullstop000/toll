use chrono::{DateTime, Utc};
use serde_json::Value;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

use crate::usage::TokenUsage;

/// Parse Claude usage entries from any BufRead source.
pub fn parse_claude_lines(reader: impl BufRead, since: Option<DateTime<Utc>>) -> TokenUsage {
    let mut usage = TokenUsage { sessions: 1, ..Default::default() };

    for line in reader.lines().map_while(Result::ok) {
        let line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }
        let Ok(v): Result<Value, _> = serde_json::from_str(&line) else {
            continue;
        };

        // Date filter: check top-level timestamp or message.timestamp
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

        // cache_creation counts toward billable input; cache_read is the cached portion
        usage.input_tokens += inp + cache_create + cache_read;
        usage.cached_input_tokens += cache_read;
        usage.output_tokens += out;
    }

    usage
}

/// Parse a Claude session file, summing all message.usage entries.
pub fn parse_claude_session(path: &Path, since: Option<DateTime<Utc>>) -> TokenUsage {
    let Ok(file) = fs::File::open(path) else {
        return TokenUsage { sessions: 1, ..Default::default() };
    };
    parse_claude_lines(BufReader::new(file), since)
}

pub fn collect_claude_usage(projects_dir: &PathBuf, since: Option<DateTime<Utc>>) -> TokenUsage {
    let mut total = TokenUsage::default();

    if !projects_dir.exists() {
        return total;
    }

    for entry in WalkDir::new(projects_dir)
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn cursor(s: &str) -> Cursor<Vec<u8>> {
        Cursor::new(s.as_bytes().to_vec())
    }

    fn make_line(ts: &str, inp: u64, cache_create: u64, cache_read: u64, out: u64) -> String {
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
    fn sums_all_messages() {
        let data = format!(
            "{}\n{}\n",
            make_line("2026-03-09T01:00:00Z", 100, 50, 200, 30),
            make_line("2026-03-09T02:00:00Z", 80, 20, 100, 15),
        );
        let usage = parse_claude_lines(cursor(&data), None);
        // input = (100+50+200) + (80+20+100) = 550
        assert_eq!(usage.input_tokens, 550);
        // cached = cache_read only: 200 + 100 = 300
        assert_eq!(usage.cached_input_tokens, 300);
        assert_eq!(usage.output_tokens, 45);
        assert_eq!(usage.sessions, 1);
    }

    #[test]
    fn date_filter_excludes_old_entries() {
        let data = format!(
            "{}\n{}\n",
            make_line("2026-03-08T12:00:00Z", 500, 0, 0, 50),
            make_line("2026-03-09T12:00:00Z", 100, 0, 0, 10),
        );
        let since: DateTime<Utc> = "2026-03-09T00:00:00Z".parse().unwrap();
        let usage = parse_claude_lines(cursor(&data), Some(since));
        assert_eq!(usage.input_tokens, 100);
        assert_eq!(usage.output_tokens, 10);
    }

    #[test]
    fn date_filter_includes_exact_boundary() {
        let data = format!("{}\n", make_line("2026-03-09T00:00:00Z", 100, 0, 0, 10));
        let since: DateTime<Utc> = "2026-03-09T00:00:00Z".parse().unwrap();
        let usage = parse_claude_lines(cursor(&data), Some(since));
        assert_eq!(usage.input_tokens, 100);
    }

    #[test]
    fn skips_lines_without_usage() {
        let data = "{\"type\":\"file-history-snapshot\",\"messageId\":\"abc\"}\n";
        let usage = parse_claude_lines(cursor(data), None);
        assert_eq!(usage.total_tokens(), 0);
    }

    #[test]
    fn skips_malformed_lines() {
        let data = format!("bad json\n{}\n", make_line("2026-03-09T01:00:00Z", 100, 0, 0, 10));
        let usage = parse_claude_lines(cursor(&data), None);
        assert_eq!(usage.input_tokens, 100);
    }
}
