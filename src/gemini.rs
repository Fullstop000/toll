use crate::agent::Agent;
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::fs;
use std::path::Path;
use walkdir::WalkDir;

use crate::usage::{DailyUsageReport, TokenUsage, add_daily_usage};

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
        "Gemini CLI"
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
            process_session(&v, &mut total_usage, since);
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
            let mut session_usage = TokenUsage::default();
            process_session(&v, &mut session_usage, since);

            if session_usage.sessions > 0 {
                report.sessions_scanned += 1;
            }

            // We need a date for the session. Use startTime if available.
            if let Some(start_time_str) = v.get("startTime").and_then(|t| t.as_str())
                && let Ok(dt) = start_time_str.parse::<DateTime<Utc>>()
                && since.is_none_or(|s| dt >= s)
            {
                add_daily_usage(&mut report.by_day, dt.date_naive(), &session_usage);
            }
        }
    }

    report
}

fn process_session(v: &Value, total_usage: &mut TokenUsage, since: Option<DateTime<Utc>>) {
    let messages = match v.get("messages").and_then(|m| m.as_array()) {
        Some(m) => m,
        None => return,
    };

    let mut session_has_usage = false;

    for msg in messages {
        if msg.get("type").and_then(|t| t.as_str()) != Some("gemini") {
            continue;
        }

        if let Some(since_dt) = since
            && let Some(ts_str) = msg.get("timestamp").and_then(|t| t.as_str())
            && let Ok(dt) = ts_str.parse::<DateTime<Utc>>()
            && dt < since_dt
        {
            continue;
        }

        if let Some(tokens) = msg.get("tokens") {
            let input = tokens.get("input").and_then(|v| v.as_u64()).unwrap_or(0);
            let output = tokens.get("output").and_then(|v| v.as_u64()).unwrap_or(0);
            let cached = tokens.get("cached").and_then(|v| v.as_u64()).unwrap_or(0);
            let _thoughts = tokens.get("thoughts").and_then(|v| v.as_u64()).unwrap_or(0);

            let model = msg.get("model").and_then(|m| m.as_str()).unwrap_or("");
            let (cost, unknown) = if let Some(p) = crate::pricing::lookup(model) {
                // Gemini: input includes cached and thoughts.
                // pure_input = input - cached.
                let pure_input = input.saturating_sub(cached);
                (p.cost(pure_input, 0, cached, output), 0)
            } else {
                (0.0, 1)
            };

            total_usage.input_tokens += input;
            total_usage.output_tokens += output;
            total_usage.cached_input_tokens += cached;
            total_usage.cost_usd += cost;
            total_usage.unknown_cost_sessions += unknown;

            session_has_usage = true;

            if !model.is_empty() {
                total_usage.record_model(
                    model,
                    input.saturating_sub(cached),
                    0,
                    cached,
                    output,
                    cost,
                );
            }
        }
    }

    if session_has_usage {
        total_usage.sessions += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_gemini_message_with_usage() {
        let mut usage = TokenUsage::default();
        let session = json!({
            "messages": [
                {
                    "type": "user",
                    "content": [{"text": "hello"}]
                },
                {
                    "type": "gemini",
                    "model": "gemini-3.1-flash",
                    "tokens": {
                        "input": 1000,
                        "output": 100,
                        "cached": 200,
                        "thoughts": 50
                    }
                }
            ]
        });

        process_session(&session, &mut usage, None);

        assert_eq!(usage.sessions, 1);
        assert_eq!(usage.input_tokens, 1000);
        assert_eq!(usage.output_tokens, 100);
        assert_eq!(usage.cached_input_tokens, 200);
        assert_eq!(usage.net_input_tokens(), 800);
        // Cost for gemini-3.1-flash: $0.50 in / $3.00 out / $0.05 cache_read
        // (800 * 0.5 + 200 * 0.05 + 100 * 3.0) / 1,000,000 = (400 + 10 + 300) / 1,000,000 = 0.00071
        assert!((usage.cost_usd - 0.00071).abs() < 1e-9);
    }

    #[test]
    fn respects_since_filter() {
        let mut usage = TokenUsage::default();
        let since = "2026-03-15T00:00:00Z".parse::<DateTime<Utc>>().unwrap();
        
        let session = json!({
            "messages": [
                {
                    "type": "gemini",
                    "timestamp": "2026-03-14T23:59:59Z",
                    "tokens": {"input": 100, "output": 10, "cached": 0}
                },
                {
                    "type": "gemini",
                    "timestamp": "2026-03-15T00:00:01Z",
                    "tokens": {"input": 200, "output": 20, "cached": 0}
                }
            ]
        });

        process_session(&session, &mut usage, Some(since));

        assert_eq!(usage.input_tokens, 200);
        assert_eq!(usage.output_tokens, 20);
    }

    #[test]
    fn session_without_usage_is_skipped() {
        let mut usage = TokenUsage::default();
        let session = json!({
            "messages": [
                {
                    "type": "user",
                    "content": [{"text": "hello"}]
                }
            ]
        });

        process_session(&session, &mut usage, None);
        assert_eq!(usage.sessions, 0);
    }

    #[test]
    fn prefix_matching_for_previews() {
        let mut usage = TokenUsage::default();
        let session = json!({
            "messages": [
                {
                    "type": "gemini",
                    "model": "gemini-3.1-flash-preview-2026",
                    "tokens": {
                        "input": 1000,
                        "output": 0,
                        "cached": 0
                    }
                }
            ]
        });

        process_session(&session, &mut usage, None);
        // Should match gemini-3.1-flash ($0.50/M) -> 1000 * 0.5 / 1,000,000 = 0.0005
        assert!((usage.cost_usd - 0.0005).abs() < 1e-9);
    }
}
