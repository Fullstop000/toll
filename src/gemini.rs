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

    // Gemini CLI logs are CUMULATIVE for the entire session.
    // To avoid overcounting, we only take the usage from the LAST message that has usage data.
    // However, if a 'since' filter is applied, we might need more complex logic.
    // For now, if 'since' is present, we filter the whole session by its start time or the last message time.
    
    let last_usage_msg = messages
        .iter()
        .rev()
        .find(|msg| {
            msg.get("type").and_then(|t| t.as_str()) == Some("gemini") && msg.get("tokens").is_some()
        });

    if let Some(msg) = last_usage_msg {
        if let Some(since_dt) = since
            && let Some(ts_str) = msg.get("timestamp").and_then(|t| t.as_str())
            && let Ok(dt) = ts_str.parse::<DateTime<Utc>>()
            && dt < since_dt
        {
            return;
        }

        if let Some(tokens) = msg.get("tokens") {
            let input = tokens.get("input").and_then(|v| v.as_u64()).unwrap_or(0);
            let output = tokens.get("output").and_then(|v| v.as_u64()).unwrap_or(0);
            let cached = tokens.get("cached").and_then(|v| v.as_u64()).unwrap_or(0);
            // Thoughts and tool tokens are usually already included in 'input' or 'output' 
            // depending on the model, but in cumulative logs, they represent the total for the session.
            
            let model = msg.get("model").and_then(|m| m.as_str()).unwrap_or("");
            let (cost, unknown) = if let Some(p) = crate::pricing::lookup(model) {
                // Gemini: input includes cached.
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
            total_usage.sessions += 1;

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
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_gemini_cumulative_usage() {
        let mut usage = TokenUsage::default();
        let session = json!({
            "messages": [
                {
                    "type": "gemini",
                    "model": "gemini-3.1-flash",
                    "tokens": {
                        "input": 1000,
                        "output": 100,
                        "cached": 0
                    }
                },
                {
                    "type": "gemini",
                    "model": "gemini-3.1-flash",
                    "tokens": {
                        "input": 2000,
                        "output": 200,
                        "cached": 1000
                    }
                }
            ]
        });

        process_session(&session, &mut usage, None);

        // Should only take the LAST message's usage
        assert_eq!(usage.sessions, 1);
        assert_eq!(usage.input_tokens, 2000);
        assert_eq!(usage.output_tokens, 200);
        assert_eq!(usage.cached_input_tokens, 1000);
        assert_eq!(usage.net_input_tokens(), 1000);
    }

    #[test]
    fn respects_since_filter_on_last_message() {
        let mut usage = TokenUsage::default();
        let since = "2026-03-15T00:00:00Z".parse::<DateTime<Utc>>().unwrap();

        let session = json!({
            "messages": [
                {
                    "type": "gemini",
                    "timestamp": "2026-03-15T00:00:01Z",
                    "tokens": {"input": 200, "output": 20, "cached": 0}
                }
            ]
        });

        process_session(&session, &mut usage, Some(since));
        assert_eq!(usage.sessions, 1);

        let mut usage2 = TokenUsage::default();
        let session2 = json!({
            "messages": [
                {
                    "type": "gemini",
                    "timestamp": "2026-03-14T23:59:59Z",
                    "tokens": {"input": 200, "output": 20, "cached": 0}
                }
            ]
        });
        process_session(&session2, &mut usage2, Some(since));
        assert_eq!(usage2.sessions, 0);
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
