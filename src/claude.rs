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
use crate::watch::{AgentSnapshot, SessionUsage};

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

    fn collect_snapshot(&self, data_dir: &Path, since: Option<DateTime<Utc>>) -> AgentSnapshot {
        collect_claude_snapshot(data_dir, since)
    }
}

fn snapshot_key(root: &Path, path: &Path) -> Option<String> {
    Some(
        path.strip_prefix(root)
            .ok()?
            .to_string_lossy()
            .replace('\\', "/"),
    )
}

/// Parse Claude usage entries from any BufRead source.
pub fn parse_claude_lines(reader: impl BufRead, since: Option<DateTime<Utc>>) -> TokenUsage {
    let mut usage = TokenUsage {
        sessions: 1,
        ..Default::default()
    };
    let mut has_unknown_model = false;
    let mut prev_ts: Option<DateTime<Utc>> = None;

    for line in reader.lines().map_while(Result::ok) {
        let line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }
        let Ok(v): Result<Value, _> = serde_json::from_str(&line) else {
            continue;
        };

        // Extract timestamp for delta accumulation (before since filter)
        let ts_str = v.get("timestamp").and_then(|t| t.as_str()).or_else(|| {
            v.get("message")
                .and_then(|m| m.get("timestamp"))
                .and_then(|t| t.as_str())
        });
        let current_ts = ts_str.and_then(|ts| ts.parse::<DateTime<Utc>>().ok());

        // Accumulate processing time delta
        if let (Some(curr), Some(prev)) = (current_ts, prev_ts) {
            let delta_ms = (curr - prev).num_milliseconds() as u64;
            usage.processing_time_ms += delta_ms;
        }
        if current_ts.is_some() {
            prev_ts = current_ts;
        }

        // Date filter: check top-level timestamp or message.timestamp
        if let Some(since_dt) = since
            && let Some(dt) = current_ts
            && dt < since_dt
        {
            continue;
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

pub fn collect_claude_snapshot(projects_dir: &Path, since: Option<DateTime<Utc>>) -> AgentSnapshot {
    let mut snapshot = AgentSnapshot::default();

    if !projects_dir.exists() {
        return snapshot;
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
        let path = entry.path();
        let key = snapshot_key(projects_dir, path).expect("session path should be relative");
        let totals = parse_claude_session(path, since);
        let Ok(file) = fs::File::open(path) else {
            continue;
        };
        let by_day = parse_claude_lines_by_day(BufReader::new(file), since);

        if totals.total_tokens() == 0 && totals.user_queries == 0 && by_day.is_empty() {
            continue;
        }

        snapshot.insert(key, SessionUsage { totals, by_day });
    }

    snapshot
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
    use std::fs;
    use std::io::Cursor;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

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

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("{prefix}-{nanos}"))
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
    fn parse_claude_lines_accumulates_processing_time() {
        let json = r#"{"timestamp":"2026-03-28T10:00:00Z","message":{"model":"claude-opus-4-6","usage":{"input_tokens":100,"output_tokens":50}}}
{"timestamp":"2026-03-28T10:00:01Z","message":{"model":"claude-opus-4-6","usage":{"input_tokens":200,"output_tokens":100}}}"#;
        let usage = parse_claude_lines(cursor(json), None);
        assert_eq!(usage.processing_time_ms, 1000);
        assert_eq!(usage.total_tokens(), 450);
        assert!((usage.tps().unwrap() - 450.0).abs() < 1e-6);
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

    #[test]
    fn collect_claude_snapshot_uses_relative_file_path() {
        let root = unique_temp_dir("toll-claude-snapshot-test");
        let session_dir = root.join("project-a");
        fs::create_dir_all(&session_dir).expect("should create claude snapshot dir");

        fs::write(
            session_dir.join("session.jsonl"),
            format!(
                "{}\n{}\n",
                user_line("2026-03-09T00:00:00Z", "prompt"),
                make_line("2026-03-09T00:00:01Z", "claude-sonnet-4-6", 100, 0, 0, 10),
            ),
        )
        .expect("should write claude session");

        let snapshot = collect_claude_snapshot(&root, None);
        let session = snapshot
            .get("project-a/session.jsonl")
            .expect("session should exist");

        assert_eq!(session.totals.user_queries, 1);
        assert_eq!(session.totals.output_tokens, 10);
        assert_eq!(session.by_day.len(), 1);

        fs::remove_dir_all(root).expect("should clean temp claude dir");
    }
}
