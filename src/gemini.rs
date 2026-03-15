use crate::agent::Agent;
use chrono::{DateTime, Local, NaiveDate, Utc};
use serde_json::Value;
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use walkdir::WalkDir;

use crate::usage::{DailyUsage, DailyUsageReport, TokenUsage, add_daily_usage};

/// Gemini CLI usage collector.
pub struct GeminiAgent;

impl GeminiAgent {
    /// Create a Gemini CLI agent collector.
    pub const fn new() -> Self {
        Self
    }
}

impl Agent for GeminiAgent {
    fn name(&self) -> &'static str {
        "Gemini"
    }

    fn data_dir(&self, home: &Path) -> std::path::PathBuf {
        home.join(".gemini").join("tmp")
    }

    fn collect_usage(&self, data_dir: &Path, since: Option<DateTime<Utc>>) -> TokenUsage {
        collect_gemini_usage(data_dir, since)
    }

    fn collect_daily_usage(
        &self,
        data_dir: &Path,
        since: Option<DateTime<Utc>>,
    ) -> DailyUsageReport {
        collect_gemini_daily_usage(data_dir, since)
    }
}

fn collect_gemini_usage(data_dir: &Path, since: Option<DateTime<Utc>>) -> TokenUsage {
    let mut total_usage = TokenUsage::default();

    for entry in WalkDir::new(data_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().ends_with(".json"))
    {
        if entry.path().to_string_lossy().contains("/chats/")
            && let Ok(content) = fs::read_to_string(entry.path())
            && let Ok(v) = serde_json::from_str::<Value>(&content)
        {
            let session_usage = parse_gemini_session(&v, since);
            if session_usage.total_tokens() > 0 {
                total_usage.add(&session_usage);
            }
        }
    }

    total_usage
}

fn collect_gemini_daily_usage(data_dir: &Path, since: Option<DateTime<Utc>>) -> DailyUsageReport {
    let mut report = DailyUsageReport::default();

    for entry in WalkDir::new(data_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().ends_with(".json"))
    {
        if entry.path().to_string_lossy().contains("/chats/")
            && let Ok(content) = fs::read_to_string(entry.path())
            && let Ok(v) = serde_json::from_str::<Value>(&content)
        {
            let session_by_day = parse_gemini_session_by_day(&v, since);
            if !session_by_day.is_empty() {
                report.sessions_scanned += 1;
            }
            for (date, usage) in session_by_day {
                add_daily_usage(&mut report.by_day, date, &usage);
            }
        }
    }

    report
}

fn parse_timestamp(msg: &Value, session_start: Option<DateTime<Utc>>) -> Option<DateTime<Utc>> {
    msg.get("timestamp")
        .and_then(|t| t.as_str())
        .and_then(|ts| ts.parse::<DateTime<Utc>>().ok())
        .or(session_start)
}

fn parse_gemini_session(v: &Value, since: Option<DateTime<Utc>>) -> TokenUsage {
    let messages = match v.get("messages").and_then(|m| m.as_array()) {
        Some(m) => m,
        None => return TokenUsage::default(),
    };
    let session_start = v
        .get("startTime")
        .and_then(|t| t.as_str())
        .and_then(|ts| ts.parse::<DateTime<Utc>>().ok());

    let mut usage = TokenUsage::default();
    let mut has_unknown_model = false;

    for msg in messages {
        if msg.get("type").and_then(|t| t.as_str()) != Some("gemini") {
            continue;
        }

        let Some(tokens) = msg.get("tokens") else {
            continue;
        };

        if let Some(since_dt) = since
            && let Some(dt) = parse_timestamp(msg, session_start)
            && dt < since_dt
        {
            continue;
        }

        let input = tokens.get("input").and_then(|v| v.as_u64()).unwrap_or(0);
        let output = tokens.get("output").and_then(|v| v.as_u64()).unwrap_or(0);
        let cached = tokens.get("cached").and_then(|v| v.as_u64()).unwrap_or(0);
        let pure_input = input.saturating_sub(cached);

        usage.input_tokens += input;
        usage.output_tokens += output;
        usage.cached_input_tokens += cached;

        let model = msg.get("model").and_then(|m| m.as_str()).unwrap_or("");
        match model {
            "" => has_unknown_model = true,
            _ => match crate::pricing::lookup(model) {
                Some(p) => {
                    let cost = p.cost(pure_input, 0, cached, output);
                    usage.cost_usd += cost;
                    usage.record_model(model, pure_input, 0, cached, output, cost);
                }
                None => has_unknown_model = true,
            },
        }
    }

    if usage.total_tokens() > 0 {
        usage.sessions = 1;
        if has_unknown_model {
            usage.unknown_cost_sessions = 1;
        }
    }

    usage
}

fn parse_gemini_session_by_day(v: &Value, since: Option<DateTime<Utc>>) -> DailyUsage {
    let messages = match v.get("messages").and_then(|m| m.as_array()) {
        Some(m) => m,
        None => return DailyUsage::default(),
    };
    let session_start = v
        .get("startTime")
        .and_then(|t| t.as_str())
        .and_then(|ts| ts.parse::<DateTime<Utc>>().ok());

    let mut by_day = DailyUsage::default();
    let mut session_days = HashSet::new();
    let mut unknown_cost_days = HashSet::new();

    for msg in messages {
        if msg.get("type").and_then(|t| t.as_str()) != Some("gemini") {
            continue;
        }

        let Some(tokens) = msg.get("tokens") else {
            continue;
        };
        let Some(dt) = parse_timestamp(msg, session_start) else {
            continue;
        };
        if let Some(since_dt) = since
            && dt < since_dt
        {
            continue;
        }

        let date: NaiveDate = dt.with_timezone(&Local).date_naive();
        let input = tokens.get("input").and_then(|v| v.as_u64()).unwrap_or(0);
        let output = tokens.get("output").and_then(|v| v.as_u64()).unwrap_or(0);
        let cached = tokens.get("cached").and_then(|v| v.as_u64()).unwrap_or(0);
        let pure_input = input.saturating_sub(cached);
        let model = msg.get("model").and_then(|m| m.as_str()).unwrap_or("");

        let (cost_usd, unknown_cost_sessions) = match model {
            "" => (0.0, if unknown_cost_days.insert(date) { 1 } else { 0 }),
            _ => match crate::pricing::lookup(model) {
                Some(p) => (p.cost(pure_input, 0, cached, output), 0),
                None => (0.0, if unknown_cost_days.insert(date) { 1 } else { 0 }),
            },
        };

        add_daily_usage(
            &mut by_day,
            date,
            &TokenUsage {
                input_tokens: input,
                cached_input_tokens: cached,
                output_tokens: output,
                sessions: if session_days.insert(date) { 1 } else { 0 },
                cost_usd,
                unknown_cost_sessions,
                ..Default::default()
            },
        );
    }

    by_day
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn sums_all_gemini_messages_in_a_session() {
        let session = json!({
            "startTime": "2026-03-15T00:00:00Z",
            "messages": [
                {
                    "type": "gemini",
                    "timestamp": "2026-03-15T00:00:01Z",
                    "model": "gemini-3.1-flash",
                    "tokens": {
                        "input": 1000,
                        "output": 100,
                        "cached": 0
                    }
                },
                {
                    "type": "gemini",
                    "timestamp": "2026-03-15T00:01:00Z",
                    "model": "gemini-3.1-flash",
                    "tokens": {
                        "input": 2000,
                        "output": 200,
                        "cached": 1000
                    }
                }
            ]
        });

        let usage = parse_gemini_session(&session, None);

        assert_eq!(usage.sessions, 1);
        assert_eq!(usage.input_tokens, 3000);
        assert_eq!(usage.output_tokens, 300);
        assert_eq!(usage.cached_input_tokens, 1000);
        assert_eq!(usage.net_input_tokens(), 2000);
    }

    #[test]
    fn respects_since_filter_per_message() {
        let since = "2026-03-15T00:00:00Z".parse::<DateTime<Utc>>().unwrap();

        let session = json!({
            "startTime": "2026-03-14T23:59:00Z",
            "messages": [
                {
                    "type": "gemini",
                    "timestamp": "2026-03-14T23:59:59Z",
                    "model": "gemini-3.1-flash",
                    "tokens": {"input": 200, "output": 20, "cached": 0}
                },
                {
                    "type": "gemini",
                    "timestamp": "2026-03-15T00:00:01Z",
                    "model": "gemini-3.1-flash",
                    "tokens": {"input": 300, "output": 30, "cached": 100}
                }
            ]
        });

        let usage = parse_gemini_session(&session, Some(since));
        assert_eq!(usage.sessions, 1);
        assert_eq!(usage.input_tokens, 300);
        assert_eq!(usage.output_tokens, 30);
        assert_eq!(usage.cached_input_tokens, 100);
    }

    #[test]
    fn prefix_matching_for_previews() {
        let session = json!({
            "messages": [
                {
                    "type": "gemini",
                    "timestamp": "2026-03-15T00:00:01Z",
                    "model": "gemini-3.1-flash-preview-2026",
                    "tokens": {
                        "input": 1000,
                        "output": 0,
                        "cached": 0
                    }
                }
            ]
        });

        let usage = parse_gemini_session(&session, None);
        // Should match gemini-3.1-flash ($0.50/M) -> 1000 * 0.5 / 1,000,000 = 0.0005
        assert!((usage.cost_usd - 0.0005).abs() < 1e-9);
    }

    #[test]
    fn buckets_daily_usage_by_message_date() {
        let since = "2026-03-15T00:00:00Z".parse::<DateTime<Utc>>().unwrap();
        let session = json!({
            "startTime": "2026-03-14T23:59:00Z",
            "messages": [
                {
                    "type": "gemini",
                    "timestamp": "2026-03-14T23:59:59Z",
                    "model": "gemini-3.1-flash",
                    "tokens": {"input": 200, "output": 20, "cached": 0}
                },
                {
                    "type": "gemini",
                    "timestamp": "2026-03-15T00:00:01Z",
                    "model": "gemini-3.1-flash",
                    "tokens": {"input": 300, "output": 30, "cached": 100}
                },
                {
                    "type": "gemini",
                    "timestamp": "2026-03-15T01:00:00Z",
                    "model": "gemini-3.1-flash",
                    "tokens": {"input": 400, "output": 40, "cached": 0}
                }
            ]
        });

        let by_day = parse_gemini_session_by_day(&session, Some(since));
        assert_eq!(by_day.len(), 1);

        let usage = by_day.values().next().unwrap();
        assert_eq!(usage.sessions, 1);
        assert_eq!(usage.input_tokens, 700);
        assert_eq!(usage.output_tokens, 70);
        assert_eq!(usage.cached_input_tokens, 100);
    }

    #[test]
    fn marks_unknown_cost_when_model_is_missing() {
        let session = json!({
            "messages": [
                {
                    "type": "gemini",
                    "timestamp": "2026-03-15T00:00:01Z",
                    "tokens": {
                        "input": 1000,
                        "output": 100,
                        "cached": 0
                    }
                }
            ]
        });

        let usage = parse_gemini_session(&session, None);
        assert_eq!(usage.sessions, 1);
        assert_eq!(usage.unknown_cost_sessions, 1);
    }
}
