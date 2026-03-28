use crate::agent::Agent;
use chrono::{DateTime, Local, NaiveDate, Utc};
use serde_json::Value;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::collections::HashMap;
use std::thread;
use walkdir::WalkDir;

use crate::pricing;
use crate::usage::{DailyUsageReport, TokenUsage, add_daily_usage};
use crate::watch::{AgentSnapshot, SessionUsage};

/// Codex usage collector.
pub struct CodexAgent;

impl CodexAgent {
    /// Create a Codex agent collector.
    pub const fn new() -> Self {
        Self
    }
}

impl Agent for CodexAgent {
    fn name(&self) -> &'static str {
        "Codex"
    }

    fn data_dir(&self, home: &Path) -> PathBuf {
        home.join(".codex").join("sessions")
    }

    fn collect_usage(&self, data_dir: &Path, since: Option<DateTime<Utc>>) -> TokenUsage {
        collect_codex_usage(data_dir, since)
    }

    fn collect_daily_usage(
        &self,
        data_dir: &Path,
        since: Option<DateTime<Utc>>,
    ) -> DailyUsageReport {
        collect_codex_daily_usage(data_dir, since)
    }

    fn collect_snapshot(&self, data_dir: &Path, since: Option<DateTime<Utc>>) -> AgentSnapshot {
        collect_codex_snapshot(data_dir, since)
    }
}

/// Fast-path filter for Codex lines worth deserializing.
fn codex_line_is_relevant(line: &str) -> bool {
    line.contains(r#""type":"turn_context""#)
        || line.contains(r#""type":"token_count""#)
        || line.contains(r#""type":"task_started""#)
}

/// Parse Codex session lines from any BufRead source.
///
/// Extracts the model from the first `session_meta` event and token counts
/// from the last `token_count` event (which holds cumulative session totals).
pub fn parse_codex_lines(reader: impl BufRead) -> Option<TokenUsage> {
    let mut model: Option<String> = None;
    let mut last_total: Option<Value> = None;
    let mut user_queries = 0u32;
    let mut prev_ts: Option<DateTime<Utc>> = None;
    let mut prev_model: Option<String> = None;
    let mut prev_model_valid: bool = false;
    let mut processing_time_ms: u64 = 0;
    let mut model_processing_time: HashMap<String, u64> = HashMap::new();

    for line in reader.lines().map_while(Result::ok) {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if !codex_line_is_relevant(line) {
            continue;
        }
        let Ok(v): Result<Value, _> = serde_json::from_str(line) else {
            continue;
        };

        match v.get("type").and_then(|t| t.as_str()) {
            // Model lives in turn_context (not session_meta)
            Some("turn_context") => {
                if model.is_none()
                    && let Some(m) = v
                        .get("payload")
                        .and_then(|p| p.get("model"))
                        .and_then(|m| m.as_str())
                {
                    model = Some(m.to_string());
                }

                // Extract timestamp and accumulate delta
                if let Some(ts_str) = v
                    .get("payload")
                    .and_then(|p| p.get("timestamp"))
                    .and_then(|t| t.as_str())
                    .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                {
                    let ts = ts_str.with_timezone(&Utc);
                    if let Some(prev) = prev_ts {
                        let delta = ts.signed_duration_since(prev);
                        let delta_ms = delta.num_milliseconds() as u64;
                        processing_time_ms += delta_ms;
                        // Add delta to previous model's per-model processing time
                        if prev_model_valid {
                            if let Some(ref pm) = prev_model {
                                *model_processing_time.entry(pm.clone()).or_insert(0) += delta_ms;
                            }
                        }
                    }
                    prev_ts = Some(ts);
                    // Update prev_model for next iteration
                    if let Some(ref m) = model {
                        prev_model = Some(m.clone());
                        prev_model_valid = crate::pricing::lookup(m).is_some();
                    }
                }
            }
            Some("event_msg") => {
                let Some(payload) = v.get("payload") else {
                    continue;
                };
                if payload.get("type").and_then(|t| t.as_str()) == Some("task_started") {
                    user_queries += 1;
                }
                if payload.get("type").and_then(|t| t.as_str()) == Some("token_count")
                    && let Some(total) =
                        payload.get("info").and_then(|i| i.get("total_token_usage"))
                {
                    last_total = Some(total.clone());
                }
            }
            _ => {}
        }
    }

    let total = last_total?;
    let input_tokens = total
        .get("input_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let cached_input_tokens = total
        .get("cached_input_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let output_tokens = total
        .get("output_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    // OpenAI: no separate cache write charge; pure_input = total_input - cached
    let pure_input = input_tokens.saturating_sub(cached_input_tokens);

    let (cost_usd, unknown_cost_sessions) = match model.as_deref().and_then(pricing::lookup) {
        Some(p) => (p.cost(pure_input, 0, cached_input_tokens, output_tokens), 0),
        None => (0.0, 1),
    };

    let mut usage = TokenUsage {
        input_tokens,
        cached_input_tokens,
        cache_write_tokens: 0,
        output_tokens,
        sessions: 1,
        user_queries,
        cost_usd,
        unknown_cost_sessions,
        processing_time_ms,
        ..Default::default()
    };

    // Populate per-model breakdown when model is known
    if let Some(m) = model.as_deref().filter(|m| !m.is_empty()) {
        let model_proc_time = model_processing_time.get(m).copied().unwrap_or(0);
        usage.record_model(
            m,
            pure_input,
            0,
            cached_input_tokens,
            output_tokens,
            cost_usd,
            model_proc_time,
        );
    }

    Some(usage)
}

/// Parse a Codex session file, return total token usage from last token_count event.
pub fn parse_codex_session(path: &Path) -> Option<TokenUsage> {
    let file = fs::File::open(path).ok()?;
    parse_codex_lines(BufReader::new(file))
}

/// Extract UTC timestamp from Codex session filename.
/// Filename format: rollout-YYYY-MM-DDTHH-MM-SS-<uuid>.jsonl
pub fn codex_session_date(path: &Path) -> Option<DateTime<Utc>> {
    let stem = path.file_name()?.to_str()?;
    let rest = stem.strip_prefix("rollout-")?;
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

/// Collect all rollout files that match the optional date filter.
fn codex_session_paths(sessions_dir: &Path, since: Option<DateTime<Utc>>) -> Vec<PathBuf> {
    WalkDir::new(sessions_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_type().is_file()
                && e.file_name()
                    .to_str()
                    .map(|n| n.starts_with("rollout-") && n.ends_with(".jsonl"))
                    .unwrap_or(false)
        })
        .filter_map(|entry| {
            let path = entry.into_path();
            if let Some(since_dt) = since
                && let Some(session_date) = codex_session_date(&path)
                && session_date < since_dt
            {
                return None;
            }
            Some(path)
        })
        .collect()
}

fn snapshot_key(root: &Path, path: &Path) -> Option<String> {
    Some(
        path.strip_prefix(root)
            .ok()?
            .to_string_lossy()
            .replace('\\', "/"),
    )
}

pub fn collect_codex_usage(sessions_dir: &Path, since: Option<DateTime<Utc>>) -> TokenUsage {
    if !sessions_dir.exists() {
        return TokenUsage::default();
    }

    let paths = codex_session_paths(sessions_dir, since);
    if paths.is_empty() {
        return TokenUsage::default();
    }

    let workers = thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .min(paths.len());
    let chunk_size = paths.len().div_ceil(workers);

    let mut total = TokenUsage::default();
    thread::scope(|scope| {
        let mut handles = Vec::new();
        for chunk in paths.chunks(chunk_size) {
            handles.push(scope.spawn(move || {
                let mut subtotal = TokenUsage::default();
                for path in chunk {
                    if let Some(usage) = parse_codex_session(path) {
                        subtotal.add(&usage);
                    }
                }
                subtotal
            }));
        }

        for handle in handles {
            let subtotal = handle.join().expect("codex worker should not panic");
            total.add(&subtotal);
        }
    });

    total
}

pub fn collect_codex_snapshot(sessions_dir: &Path, since: Option<DateTime<Utc>>) -> AgentSnapshot {
    let mut snapshot = AgentSnapshot::default();

    if !sessions_dir.exists() {
        return snapshot;
    }

    for path in codex_session_paths(sessions_dir, since) {
        let Some(totals) = parse_codex_session(&path) else {
            continue;
        };
        let Some(dt) = codex_session_date(&path) else {
            continue;
        };

        let mut by_day = crate::usage::DailyUsage::default();
        add_daily_usage(&mut by_day, dt.with_timezone(&Local).date_naive(), &totals);
        let key = snapshot_key(sessions_dir, &path).expect("session path should be relative");
        snapshot.insert(key, SessionUsage { totals, by_day });
    }

    snapshot
}

/// Collect Codex usage aggregated by local calendar date.
pub fn collect_codex_daily_usage(
    sessions_dir: &Path,
    since: Option<DateTime<Utc>>,
) -> DailyUsageReport {
    if !sessions_dir.exists() {
        return DailyUsageReport::default();
    }

    let paths = codex_session_paths(sessions_dir, since);
    let mut report = DailyUsageReport::default();

    for path in paths {
        let Some(dt) = codex_session_date(&path) else {
            continue;
        };
        let Some(usage) = parse_codex_session(&path) else {
            continue;
        };
        let date: NaiveDate = dt.with_timezone(&Local).date_naive();
        add_daily_usage(&mut report.by_day, date, &usage);
        report.sessions_scanned += 1;
    }

    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Cursor;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn cursor(s: &str) -> Cursor<Vec<u8>> {
        Cursor::new(s.as_bytes().to_vec())
    }

    fn token_count_line(inp: u64, cached: u64, out: u64) -> String {
        serde_json::json!({
            "type": "event_msg",
            "payload": {
                "type": "token_count",
                "info": {
                    "total_token_usage": {
                        "input_tokens": inp,
                        "cached_input_tokens": cached,
                        "output_tokens": out
                    }
                }
            }
        })
        .to_string()
    }

    fn turn_context_line(model: &str) -> String {
        serde_json::json!({
            "type": "turn_context",
            "payload": { "turn_id": "abc", "model": model }
        })
        .to_string()
    }

    fn turn_context_line_with_timestamp(model: &str, timestamp: &str) -> String {
        serde_json::json!({
            "type": "turn_context",
            "payload": { "turn_id": "abc", "model": model, "timestamp": timestamp }
        })
        .to_string()
    }

    fn task_started_line(turn_id: &str) -> String {
        serde_json::json!({
            "type": "event_msg",
            "payload": {
                "type": "task_started",
                "turn_id": turn_id
            }
        })
        .to_string()
    }

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("{prefix}-{nanos}"))
    }

    #[test]
    fn parses_last_token_count() {
        let data = format!(
            "{}\n{}\n{}\n{}\n",
            task_started_line("turn-1"),
            turn_context_line("gpt-4o"),
            token_count_line(100, 80, 10),
            token_count_line(200, 150, 25),
        );
        let usage = parse_codex_lines(cursor(&data)).expect("should parse");
        assert_eq!(usage.input_tokens, 200);
        assert_eq!(usage.cached_input_tokens, 150);
        assert_eq!(usage.output_tokens, 25);
        assert_eq!(usage.sessions, 1);
        assert_eq!(usage.user_queries, 1);
    }

    #[test]
    fn counts_task_started_events_as_user_queries() {
        let data = format!(
            "{}\n{}\n{}\n{}\n{}\n",
            task_started_line("turn-1"),
            turn_context_line("gpt-5.4"),
            token_count_line(100, 80, 10),
            task_started_line("turn-2"),
            token_count_line(200, 150, 25),
        );
        let usage = parse_codex_lines(cursor(&data)).expect("should parse");
        assert_eq!(usage.user_queries, 2);
        assert_eq!(usage.total_tokens(), 225);
    }

    #[test]
    fn computes_cost_for_known_model() {
        // gpt-4o: $2.50/M input, $1.25/M cached, $10/M output
        // 1M input (0 cached) + 0 output → $2.50
        let data = format!(
            "{}\n{}\n",
            turn_context_line("gpt-4o"),
            token_count_line(1_000_000, 0, 0),
        );
        let usage = parse_codex_lines(cursor(&data)).expect("should parse");
        assert!((usage.cost_usd - 2.50).abs() < 0.001);
        assert_eq!(usage.unknown_cost_sessions, 0);
    }

    #[test]
    fn cost_with_cache_discount() {
        // gpt-4o: 500K non-cached + 500K cached + 100K output
        // = 500K*$2.50 + 500K*$1.25 + 100K*$10 = $1.25 + $0.625 + $1.00 = $2.875
        let data = format!(
            "{}\n{}\n",
            turn_context_line("gpt-4o"),
            token_count_line(1_000_000, 500_000, 100_000),
        );
        let usage = parse_codex_lines(cursor(&data)).expect("should parse");
        let expected = 500_000.0 * 2.50 / 1e6 + 500_000.0 * 1.25 / 1e6 + 100_000.0 * 10.0 / 1e6;
        assert!((usage.cost_usd - expected).abs() < 0.001);
    }

    #[test]
    fn marks_unknown_cost_when_model_missing() {
        let data = format!("{}\n", token_count_line(1000, 0, 100));
        let usage = parse_codex_lines(cursor(&data)).expect("should parse");
        assert_eq!(usage.cost_usd, 0.0);
        assert_eq!(usage.unknown_cost_sessions, 1);
    }

    #[test]
    fn marks_unknown_cost_for_unknown_model() {
        let data = format!(
            "{}\n{}\n",
            turn_context_line("future-model-xyz"),
            token_count_line(1000, 0, 100),
        );
        let usage = parse_codex_lines(cursor(&data)).expect("should parse");
        assert_eq!(usage.cost_usd, 0.0);
        assert_eq!(usage.unknown_cost_sessions, 1);
    }

    #[test]
    fn gpt5_variant_uses_prefix_pricing() {
        // "gpt-5.4" is explicitly priced and should use its own rate.
        let data = format!(
            "{}\n{}\n",
            turn_context_line("gpt-5.4"),
            token_count_line(1_000_000, 0, 0),
        );
        let usage = parse_codex_lines(cursor(&data)).expect("should parse");
        assert!((usage.cost_usd - 2.50).abs() < 0.001);
        assert_eq!(usage.unknown_cost_sessions, 0);
    }

    #[test]
    fn parse_codex_lines_accumulates_processing_time() {
        // Turn context timestamps are 1 second apart
        // Deltas: (T2-T1) + (T3-T2) = 1000 + 1000 = 2000ms
        let data = format!(
            "{}\n{}\n{}\n{}\n",
            turn_context_line_with_timestamp("gpt-4o", "2026-03-28T10:00:00Z"),
            turn_context_line_with_timestamp("gpt-4o", "2026-03-28T10:00:01Z"),
            turn_context_line_with_timestamp("gpt-4o", "2026-03-28T10:00:02Z"),
            token_count_line(100, 0, 10),
        );
        let usage = parse_codex_lines(cursor(&data)).expect("should parse");
        assert_eq!(usage.processing_time_ms, 2000);
    }

    #[test]
    fn ignores_non_token_count_events() {
        let data = r#"
{"type":"event_msg","payload":{"type":"task_started","turn_id":"abc"}}
{"type":"response_item","payload":{"type":"message","role":"assistant"}}
"#;
        assert!(parse_codex_lines(cursor(data)).is_none());
    }

    #[test]
    fn skips_malformed_lines() {
        let data = format!("not json\n{}\n", token_count_line(50, 0, 5));
        let usage = parse_codex_lines(cursor(&data)).expect("should parse despite bad line");
        assert_eq!(usage.input_tokens, 50);
    }

    #[test]
    fn empty_input_returns_none() {
        assert!(parse_codex_lines(cursor("")).is_none());
    }

    #[test]
    fn session_date_parses_correctly() {
        let path = Path::new(
            "/home/user/.codex/sessions/2026/03/08/rollout-2026-03-08T20-55-09-019ccd84-0e5f-7870-9c33-097188e35a30.jsonl",
        );
        let dt = codex_session_date(path).expect("should parse");
        assert_eq!(dt.to_rfc3339(), "2026-03-08T20:55:09+00:00");
    }

    #[test]
    fn session_date_rejects_non_rollout() {
        let path = Path::new("/home/user/.codex/sessions/2026/03/08/other-file.jsonl");
        assert!(codex_session_date(path).is_none());
    }

    #[test]
    fn session_date_rejects_short_name() {
        let path = Path::new("/home/user/.codex/sessions/2026/03/08/rollout-short.jsonl");
        assert!(codex_session_date(path).is_none());
    }

    #[test]
    fn codex_line_relevance_filters_unrelated_lines() {
        assert!(codex_line_is_relevant(&turn_context_line("gpt-5.4")));
        assert!(codex_line_is_relevant(&token_count_line(100, 0, 10)));
        assert!(!codex_line_is_relevant(
            r#"{"type":"response_item","payload":{"type":"message"}}"#
        ));
        assert!(!codex_line_is_relevant("not json at all"));
    }

    #[test]
    fn collect_codex_usage_aggregates_matching_rollout_files() {
        let root = unique_temp_dir("toll-codex-test");
        let day_dir = root.join("2026/03/09");
        fs::create_dir_all(&day_dir).expect("should create temp session dir");

        fs::write(
            day_dir.join("rollout-2026-03-09T08-00-00-aaaa.jsonl"),
            format!(
                "{}\n{}\n",
                turn_context_line("gpt-5.4"),
                token_count_line(100, 25, 10)
            ),
        )
        .expect("should write first rollout");
        fs::write(
            day_dir.join("rollout-2026-03-09T09-00-00-bbbb.jsonl"),
            format!(
                "{}\n{}\n",
                turn_context_line("gpt-4o"),
                token_count_line(300, 100, 20)
            ),
        )
        .expect("should write second rollout");
        fs::write(day_dir.join("ignore-me.jsonl"), "{}\n").expect("should write ignored file");

        let usage = collect_codex_usage(&root, None);
        assert_eq!(usage.sessions, 2);
        assert_eq!(usage.input_tokens, 400);
        assert_eq!(usage.cached_input_tokens, 125);
        assert_eq!(usage.output_tokens, 30);

        fs::remove_dir_all(root).expect("should clean temp session dir");
    }

    #[test]
    fn collect_codex_daily_usage_groups_by_local_date() {
        let root = unique_temp_dir("toll-codex-daily-test");
        let day_dir = root.join("2026/03/09");
        fs::create_dir_all(&day_dir).expect("should create temp session dir");

        fs::write(
            day_dir.join("rollout-2026-03-09T00-30-00-aaaa.jsonl"),
            format!(
                "{}\n{}\n",
                turn_context_line("gpt-5.4"),
                token_count_line(100, 25, 10)
            ),
        )
        .expect("should write first rollout");
        fs::write(
            day_dir.join("rollout-2026-03-09T15-30-00-bbbb.jsonl"),
            format!(
                "{}\n{}\n",
                turn_context_line("gpt-4o"),
                token_count_line(300, 100, 20)
            ),
        )
        .expect("should write second rollout");

        let report = collect_codex_daily_usage(&root, None);
        let date = chrono::NaiveDate::from_ymd_opt(2026, 3, 9).expect("valid date");
        assert_eq!(report.sessions_scanned, 2);
        assert_eq!(report.by_day.len(), 1);
        assert_eq!(report.by_day[&date].sessions, 2);
        assert_eq!(report.by_day[&date].input_tokens, 400);
        assert_eq!(report.by_day[&date].cached_input_tokens, 125);
        assert_eq!(report.by_day[&date].output_tokens, 30);

        fs::remove_dir_all(root).expect("should clean temp session dir");
    }

    #[test]
    fn collect_codex_snapshot_uses_rollout_relative_path() {
        let root = unique_temp_dir("toll-codex-snapshot-test");
        let day_dir = root.join("2026/03/09");
        fs::create_dir_all(&day_dir).expect("should create temp session dir");

        fs::write(
            day_dir.join("rollout-2026-03-09T08-00-00-aaaa.jsonl"),
            format!(
                "{}\n{}\n",
                turn_context_line("gpt-5.4"),
                token_count_line(100, 25, 10)
            ),
        )
        .expect("should write rollout");

        let snapshot = collect_codex_snapshot(&root, None);
        let session = snapshot
            .get("2026/03/09/rollout-2026-03-09T08-00-00-aaaa.jsonl")
            .expect("session should exist");

        assert_eq!(session.totals.total_tokens(), 110);
        assert_eq!(session.by_day.len(), 1);

        fs::remove_dir_all(root).expect("should clean temp session dir");
    }
}
