use crate::agent::Agent;
use chrono::{DateTime, Local, NaiveDate, Utc};
use serde_json::Value;
use std::collections::HashSet;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::Path;
use walkdir::WalkDir;

use crate::pricing;
use crate::usage::{DailyUsage, DailyUsageReport, TokenUsage, add_daily_usage};

fn is_top_level_user_query(v: &Value) -> bool {
    v.get("type").and_then(|t| t.as_str()) == Some("user")
        && v.get("message")
            .and_then(|m| m.get("role"))
            .and_then(|r| r.as_str())
            == Some("user")
        && !v.get("isMeta").and_then(|m| m.as_bool()).unwrap_or(false)
        && v.get("sourceToolAssistantUUID").is_none()
}

/// Claude Code usage collector.
pub struct ClaudeAgent;

impl ClaudeAgent {
    /// Create a Claude Code agent collector.
    pub const fn new() -> Self {
        Self
    }
}

impl Agent for ClaudeAgent {
    fn name(&self) -> &'static str {
        "Claude Code"
    }

    fn data_dir(&self, home: &Path) -> std::path::PathBuf {
        home.join(".claude").join("projects")
    }

    fn collect_usage(&self, data_dir: &Path, since: Option<DateTime<Utc>>) -> TokenUsage {
        collect_claude_usage(data_dir, since)
    }

    fn collect_daily_usage(
        &self,
        data_dir: &Path,
        since: Option<DateTime<Utc>>,
    ) -> DailyUsageReport {
        collect_claude_daily_usage(data_dir, since)
    }
}

/// Parse Claude usage entries from any BufRead source.
pub fn parse_claude_lines(reader: impl BufRead, since: Option<DateTime<Utc>>) -> TokenUsage {
    let mut usage = TokenUsage {
        sessions: 1,
        ..Default::default()
    };
    let mut has_unknown_model = false;

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
            if let Some(ts) = ts_str
                && let Ok(dt) = ts.parse::<DateTime<Utc>>()
                && dt < since_dt
            {
                continue;
            }
        }

        if is_top_level_user_query(&v) {
            usage.user_queries += 1;
            continue;
        }

        let Some(msg) = v.get("message") else {
            continue;
        };
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
        usage.cache_write_tokens += cache_create;
        usage.output_tokens += out;

        // Cost + per-model breakdown. Skip internal synthetic values (e.g. "<synthetic>").
        let model = msg.get("model").and_then(|m| m.as_str()).unwrap_or("");
        if !model.is_empty() && !model.starts_with('<') {
            match pricing::lookup(model) {
                Some(p) => {
                    let cost = p.cost(inp, cache_create, cache_read, out);
                    usage.cost_usd += cost;
                    usage.record_model(model, inp, cache_create, cache_read, out, cost);
                }
                None => has_unknown_model = true,
            }
        }
    }

    if has_unknown_model {
        usage.unknown_cost_sessions = 1;
    }
    usage
}

/// Parse Claude usage entries into local calendar-date buckets.
pub fn parse_claude_lines_by_day(reader: impl BufRead, since: Option<DateTime<Utc>>) -> DailyUsage {
    let mut by_day = DailyUsage::default();
    let mut session_days = HashSet::new();
    let mut unknown_cost_days = HashSet::new();

    for line in reader.lines().map_while(Result::ok) {
        let line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }
        let Ok(v): Result<Value, _> = serde_json::from_str(&line) else {
            continue;
        };

        let ts_str = v.get("timestamp").and_then(|t| t.as_str()).or_else(|| {
            v.get("message")
                .and_then(|m| m.get("timestamp"))
                .and_then(|t| t.as_str())
        });
        let Some(dt) = ts_str.and_then(|ts| ts.parse::<DateTime<Utc>>().ok()) else {
            continue;
        };
        if let Some(since_dt) = since
            && dt < since_dt
        {
            continue;
        }

        if is_top_level_user_query(&v) {
            let date: NaiveDate = dt.with_timezone(&Local).date_naive();
            add_daily_usage(
                &mut by_day,
                date,
                &TokenUsage {
                    user_queries: 1,
                    ..Default::default()
                },
            );
            continue;
        }

        let Some(msg) = v.get("message") else {
            continue;
        };
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

        let date: NaiveDate = dt.with_timezone(&Local).date_naive();

        let model = msg.get("model").and_then(|m| m.as_str()).unwrap_or("");
        let (cost_usd, unknown_cost_sessions) = if !model.is_empty() && !model.starts_with('<') {
            match pricing::lookup(model) {
                Some(p) => (p.cost(inp, cache_create, cache_read, out), 0),
                None => (0.0, if unknown_cost_days.insert(date) { 1 } else { 0 }),
            }
        } else {
            (0.0, 0)
        };

        add_daily_usage(
            &mut by_day,
            date,
            &TokenUsage {
                input_tokens: inp + cache_create + cache_read,
                cached_input_tokens: cache_read,
                cache_write_tokens: cache_create,
                output_tokens: out,
                sessions: if session_days.insert(date) { 1 } else { 0 },
                cost_usd,
                unknown_cost_sessions,
                ..Default::default()
            },
        );
    }

    by_day
}

/// Parse a Claude session file, summing all message.usage entries.
pub fn parse_claude_session(path: &Path, since: Option<DateTime<Utc>>) -> TokenUsage {
    let Ok(file) = fs::File::open(path) else {
        return TokenUsage {
            sessions: 1,
            ..Default::default()
        };
    };
    parse_claude_lines(BufReader::new(file), since)
}

pub fn collect_claude_usage(projects_dir: &Path, since: Option<DateTime<Utc>>) -> TokenUsage {
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
        if usage.total_tokens() > 0 || usage.user_queries > 0 {
            total.add(&usage);
        }
    }

    total
}

/// Collect Claude usage aggregated by local calendar date.
pub fn collect_claude_daily_usage(
    projects_dir: &Path,
    since: Option<DateTime<Utc>>,
) -> DailyUsageReport {
    let mut report = DailyUsageReport::default();

    if !projects_dir.exists() {
        return report;
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
        let Ok(file) = fs::File::open(entry.path()) else {
            continue;
        };
        let session_by_day = parse_claude_lines_by_day(BufReader::new(file), since);
        if session_by_day.is_empty() {
            continue;
        }
        report.sessions_scanned += 1;
        for (date, day_usage) in session_by_day {
            add_daily_usage(&mut report.by_day, date, &day_usage);
        }
    }

    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn cursor(s: &str) -> Cursor<Vec<u8>> {
        Cursor::new(s.as_bytes().to_vec())
    }

    fn make_line(
        ts: &str,
        model: &str,
        inp: u64,
        cache_create: u64,
        cache_read: u64,
        out: u64,
    ) -> String {
        serde_json::json!({
            "timestamp": ts,
            "message": {
                "role": "assistant",
                "model": model,
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

    fn user_line(ts: &str, content: &str) -> String {
        serde_json::json!({
            "timestamp": ts,
            "type": "user",
            "message": {
                "role": "user",
                "content": content
            }
        })
        .to_string()
    }

    #[test]
    fn sums_all_messages() {
        let data = format!(
            "{}\n{}\n",
            make_line(
                "2026-03-09T01:00:00Z",
                "claude-sonnet-4-6",
                100,
                50,
                200,
                30
            ),
            make_line("2026-03-09T02:00:00Z", "claude-sonnet-4-6", 80, 20, 100, 15),
        );
        let usage = parse_claude_lines(cursor(&data), None);
        assert_eq!(usage.input_tokens, 550); // (100+50+200) + (80+20+100)
        assert_eq!(usage.cached_input_tokens, 300); // 200 + 100
        assert_eq!(usage.cache_write_tokens, 70); // 50 + 20
        assert_eq!(usage.output_tokens, 45);
        assert_eq!(usage.sessions, 1);
    }

    #[test]
    fn counts_top_level_user_queries_once_per_user_message() {
        let data = format!(
            "{}\n{}\n{}\n{}\n",
            user_line("2026-03-09T00:59:59Z", "first prompt"),
            make_line(
                "2026-03-09T01:00:00Z",
                "claude-sonnet-4-6",
                100,
                50,
                200,
                30
            ),
            make_line("2026-03-09T01:00:30Z", "claude-sonnet-4-6", 80, 20, 100, 15),
            user_line("2026-03-09T02:00:00Z", "follow-up"),
        );
        let usage = parse_claude_lines(cursor(&data), None);
        assert_eq!(usage.user_queries, 2);
        assert_eq!(usage.output_tokens, 45);
    }

    #[test]
    fn computes_cost_for_known_model() {
        let data = format!(
            "{}\n",
            // 1M inp, 0 cache_write, 0 cache_read, 0 out → $3.00
            make_line(
                "2026-03-09T01:00:00Z",
                "claude-sonnet-4-6",
                1_000_000,
                0,
                0,
                0
            ),
        );
        let usage = parse_claude_lines(cursor(&data), None);
        assert!((usage.cost_usd - 3.0).abs() < 0.001);
        assert_eq!(usage.unknown_cost_sessions, 0);
    }

    #[test]
    fn marks_unknown_cost_for_unknown_model() {
        let data = format!(
            "{}\n",
            make_line("2026-03-09T01:00:00Z", "future-model-xyz", 1000, 0, 0, 100),
        );
        let usage = parse_claude_lines(cursor(&data), None);
        assert_eq!(usage.cost_usd, 0.0);
        assert_eq!(usage.unknown_cost_sessions, 1);
    }

    #[test]
    fn date_filter_excludes_old_entries() {
        let data = format!(
            "{}\n{}\n",
            make_line("2026-03-08T12:00:00Z", "claude-sonnet-4-6", 500, 0, 0, 50),
            make_line("2026-03-09T12:00:00Z", "claude-sonnet-4-6", 100, 0, 0, 10),
        );
        let since: DateTime<Utc> = "2026-03-09T00:00:00Z".parse().unwrap();
        let usage = parse_claude_lines(cursor(&data), Some(since));
        assert_eq!(usage.input_tokens, 100);
        assert_eq!(usage.output_tokens, 10);
    }

    #[test]
    fn date_filter_includes_exact_boundary() {
        let data = format!(
            "{}\n",
            make_line("2026-03-09T00:00:00Z", "claude-sonnet-4-6", 100, 0, 0, 10)
        );
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
        let data = format!(
            "bad json\n{}\n",
            make_line("2026-03-09T01:00:00Z", "claude-sonnet-4-6", 100, 0, 0, 10)
        );
        let usage = parse_claude_lines(cursor(&data), None);
        assert_eq!(usage.input_tokens, 100);
    }

    #[test]
    fn parse_claude_lines_by_day_groups_local_dates() {
        let data = format!(
            "{}\n{}\n",
            make_line("2026-03-09T00:30:00Z", "claude-sonnet-4-6", 100, 0, 0, 10),
            make_line("2026-03-09T15:30:00Z", "claude-sonnet-4-6", 200, 0, 0, 20),
        );

        let by_day = parse_claude_lines_by_day(cursor(&data), None);

        let date = chrono::NaiveDate::from_ymd_opt(2026, 3, 9).expect("valid date");
        assert_eq!(by_day.len(), 1);
        assert_eq!(by_day[&date].input_tokens, 300);
        assert_eq!(by_day[&date].output_tokens, 30);
        assert_eq!(by_day[&date].sessions, 1);
    }

    #[test]
    fn parse_claude_lines_by_day_counts_unknown_cost_once_per_session_day() {
        let data = format!(
            "{}\n{}\n",
            make_line("2026-03-09T00:30:00Z", "future-model-xyz", 100, 0, 0, 10),
            make_line("2026-03-09T15:30:00Z", "future-model-xyz", 200, 0, 0, 20),
        );

        let by_day = parse_claude_lines_by_day(cursor(&data), None);

        let date = chrono::NaiveDate::from_ymd_opt(2026, 3, 9).expect("valid date");
        assert_eq!(by_day[&date].sessions, 1);
        assert_eq!(by_day[&date].unknown_cost_sessions, 1);
        assert_eq!(crate::display::fmt_cost(&by_day[&date]), "unknown");
    }
}
