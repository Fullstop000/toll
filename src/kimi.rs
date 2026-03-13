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

/// Model identifier used in Kimi Code sessions.
const KIMI_MODEL: &str = "kimi-for-coding";

/// Kimi Code usage collector.
pub struct KimiAgent;

impl KimiAgent {
    pub const fn new() -> Self {
        Self
    }
}

impl Agent for KimiAgent {
    fn name(&self) -> &'static str {
        "Kimi Code"
    }

    fn data_dir(&self, home: &Path) -> std::path::PathBuf {
        home.join(".kimi").join("sessions")
    }

    fn collect_usage(&self, data_dir: &Path, since: Option<DateTime<Utc>>) -> TokenUsage {
        collect_kimi_usage(data_dir, since)
    }

    fn collect_daily_usage(
        &self,
        data_dir: &Path,
        since: Option<DateTime<Utc>>,
    ) -> DailyUsageReport {
        collect_kimi_daily_usage(data_dir, since)
    }
}

/// Parse a Unix float timestamp (seconds since epoch) to `DateTime<Utc>`.
fn parse_unix_ts(v: &Value) -> Option<DateTime<Utc>> {
    let f = v.as_f64()?;
    let secs = f as i64;
    let nanos = ((f - secs as f64) * 1_000_000_000.0) as u32;
    DateTime::from_timestamp(secs, nanos)
}

/// Token counts extracted from a single `StatusUpdate` payload.
struct KimiTokens {
    inp_other: u64,
    cache_read: u64,
    cache_create: u64,
    out: u64,
}

fn extract_tokens(tu: &Value) -> KimiTokens {
    KimiTokens {
        inp_other: tu.get("input_other").and_then(|v| v.as_u64()).unwrap_or(0),
        cache_read: tu
            .get("input_cache_read")
            .and_then(|v| v.as_u64())
            .unwrap_or(0),
        cache_create: tu
            .get("input_cache_creation")
            .and_then(|v| v.as_u64())
            .unwrap_or(0),
        out: tu.get("output").and_then(|v| v.as_u64()).unwrap_or(0),
    }
}

/// Return the `token_usage` object from a `StatusUpdate` line, or `None`.
///
/// Also applies the optional `since` date filter against the line's timestamp.
fn status_update_tokens<'a>(
    v: &'a Value,
    since: Option<DateTime<Utc>>,
) -> Option<&'a Value> {
    let msg = v.get("message")?;
    if msg.get("type")?.as_str()? != "StatusUpdate" {
        return None;
    }
    if let Some(since_dt) = since {
        let dt = v.get("timestamp").and_then(parse_unix_ts)?;
        if dt < since_dt {
            return None;
        }
    }
    msg.get("payload")?.get("token_usage")
}

/// Parse Kimi Code usage entries from any `BufRead` source.
///
/// Kimi Code logs token usage in `StatusUpdate` messages:
/// ```json
/// {"timestamp": 1772971394.14, "message": {"type": "StatusUpdate", "payload": {
///   "token_usage": {"input_other": 2492, "output": 63,
///                   "input_cache_read": 5376, "input_cache_creation": 0}}}}
/// ```
pub fn parse_kimi_lines(reader: impl BufRead, since: Option<DateTime<Utc>>) -> TokenUsage {
    let mut usage = TokenUsage {
        sessions: 1,
        ..Default::default()
    };
    let pricing = pricing::lookup(KIMI_MODEL);

    for line in reader.lines().map_while(Result::ok) {
        let line = line.trim().to_string();
        if line.is_empty() || !line.contains("StatusUpdate") {
            continue;
        }
        let Ok(v): Result<Value, _> = serde_json::from_str(&line) else {
            continue;
        };
        let Some(tu) = status_update_tokens(&v, since) else {
            continue;
        };
        let KimiTokens { inp_other, cache_read, cache_create, out } = extract_tokens(tu);

        usage.input_tokens += inp_other + cache_read + cache_create;
        usage.cached_input_tokens += cache_read;
        usage.cache_write_tokens += cache_create;
        usage.output_tokens += out;

        if let Some(p) = pricing {
            let cost = p.cost(inp_other, cache_create, cache_read, out);
            usage.cost_usd += cost;
            usage.record_model(KIMI_MODEL, inp_other, cache_create, cache_read, out, cost);
        }
    }

    if pricing.is_none() && usage.total_tokens() > 0 {
        usage.unknown_cost_sessions = 1;
    }
    usage
}

/// Parse Kimi Code usage entries into local calendar-date buckets.
pub fn parse_kimi_lines_by_day(reader: impl BufRead, since: Option<DateTime<Utc>>) -> DailyUsage {
    let mut by_day = DailyUsage::default();
    let mut session_days = HashSet::new();
    let mut unknown_cost_days = HashSet::new();
    let pricing = pricing::lookup(KIMI_MODEL);

    for line in reader.lines().map_while(Result::ok) {
        let line = line.trim().to_string();
        if line.is_empty() || !line.contains("StatusUpdate") {
            continue;
        }
        let Ok(v): Result<Value, _> = serde_json::from_str(&line) else {
            continue;
        };

        // Need the timestamp for bucketing — extract before the since filter strips it.
        let msg = v.get("message");
        if msg.and_then(|m| m.get("type")).and_then(|t| t.as_str()) != Some("StatusUpdate") {
            continue;
        }
        let Some(dt) = v.get("timestamp").and_then(parse_unix_ts) else {
            continue;
        };
        if let Some(since_dt) = since {
            if dt < since_dt {
                continue;
            }
        }
        let Some(tu) = msg
            .and_then(|m| m.get("payload"))
            .and_then(|p| p.get("token_usage"))
        else {
            continue;
        };

        let KimiTokens { inp_other, cache_read, cache_create, out } = extract_tokens(tu);
        let date: NaiveDate = dt.with_timezone(&Local).date_naive();

        let (cost_usd, unknown_cost_sessions) = match pricing {
            Some(p) => (p.cost(inp_other, cache_create, cache_read, out), 0),
            None => (0.0, if unknown_cost_days.insert(date) { 1 } else { 0 }),
        };

        add_daily_usage(
            &mut by_day,
            date,
            &TokenUsage {
                input_tokens: inp_other + cache_read + cache_create,
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

pub fn collect_kimi_usage(sessions_dir: &Path, since: Option<DateTime<Utc>>) -> TokenUsage {
    let mut total = TokenUsage::default();

    for entry in WalkDir::new(sessions_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file() && e.file_name().to_str() == Some("wire.jsonl"))
    {
        let Ok(file) = fs::File::open(entry.path()) else {
            continue;
        };
        let usage = parse_kimi_lines(BufReader::new(file), since);
        if usage.total_tokens() > 0 {
            total.add(&usage);
        }
    }

    total
}

pub fn collect_kimi_daily_usage(
    sessions_dir: &Path,
    since: Option<DateTime<Utc>>,
) -> DailyUsageReport {
    let mut report = DailyUsageReport::default();

    for entry in WalkDir::new(sessions_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file() && e.file_name().to_str() == Some("wire.jsonl"))
    {
        let Ok(file) = fs::File::open(entry.path()) else {
            continue;
        };
        let session_by_day = parse_kimi_lines_by_day(BufReader::new(file), since);
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

    fn make_line(ts: f64, inp_other: u64, cache_read: u64, cache_create: u64, out: u64) -> String {
        serde_json::json!({
            "timestamp": ts,
            "message": {
                "type": "StatusUpdate",
                "payload": {
                    "token_usage": {
                        "input_other": inp_other,
                        "input_cache_read": cache_read,
                        "input_cache_creation": cache_create,
                        "output": out
                    }
                }
            }
        })
        .to_string()
    }

    #[test]
    fn sums_all_status_updates() {
        // Two calls in one session
        let data = format!(
            "{}\n{}\n",
            make_line(1_772_971_394.0, 2492, 5376, 0, 63),
            make_line(1_772_971_405.0, 901, 7680, 0, 60),
        );
        let usage = parse_kimi_lines(cursor(&data), None);
        assert_eq!(usage.input_tokens, (2492 + 5376) + (901 + 7680));
        assert_eq!(usage.cached_input_tokens, 5376 + 7680);
        assert_eq!(usage.cache_write_tokens, 0);
        assert_eq!(usage.output_tokens, 63 + 60);
        assert_eq!(usage.sessions, 1);
    }

    #[test]
    fn skips_non_status_update_lines() {
        let metadata = r#"{"type": "metadata", "protocol_version": "1.3"}"#;
        let turn_begin = r#"{"timestamp": 1772971392.0, "message": {"type": "TurnBegin", "payload": {"user_input": []}}}"#;
        let data = format!("{}\n{}\n", metadata, turn_begin);
        let usage = parse_kimi_lines(cursor(&data), None);
        assert_eq!(usage.total_tokens(), 0);
    }

    #[test]
    fn date_filter_excludes_old_entries() {
        // 1772884800 = 2026-03-08T00:00:00Z, 1772971200 = 2026-03-09T00:00:00Z
        let data = format!(
            "{}\n{}\n",
            make_line(1_772_884_800.0, 500, 0, 0, 50),
            make_line(1_772_971_200.0, 100, 0, 0, 10),
        );
        let since = DateTime::from_timestamp(1_772_971_200, 0).unwrap();
        let usage = parse_kimi_lines(cursor(&data), Some(since));
        assert_eq!(usage.input_tokens, 100);
        assert_eq!(usage.output_tokens, 10);
    }

    #[test]
    fn computes_cost_for_known_model() {
        // 1M input_other + 0 cache_read + 0 cache_create + 0 output → $0.60
        let data = format!("{}\n", make_line(1_772_971_394.0, 1_000_000, 0, 0, 0));
        let usage = parse_kimi_lines(cursor(&data), None);
        assert!((usage.cost_usd - 0.60).abs() < 0.001);
        assert_eq!(usage.unknown_cost_sessions, 0);
    }

    #[test]
    fn parse_by_day_groups_local_dates() {
        let data = format!(
            "{}\n{}\n",
            make_line(1_772_971_394.0, 100, 0, 0, 10),
            make_line(1_772_975_000.0, 200, 0, 0, 20),
        );
        let by_day = parse_kimi_lines_by_day(cursor(&data), None);
        // Both entries should land on the same local date
        assert_eq!(by_day.len(), 1);
        let (_, day) = by_day.iter().next().unwrap();
        assert_eq!(day.input_tokens, 300);
        assert_eq!(day.output_tokens, 30);
        assert_eq!(day.sessions, 1);
    }
}
