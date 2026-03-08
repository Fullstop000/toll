use chrono::{DateTime, Utc};
use serde_json::Value;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

use crate::pricing;
use crate::usage::TokenUsage;

/// Parse Codex session lines from any BufRead source.
///
/// Extracts the model from the first `session_meta` event and token counts
/// from the last `token_count` event (which holds cumulative session totals).
pub fn parse_codex_lines(reader: impl BufRead) -> Option<TokenUsage> {
    let mut model: Option<String> = None;
    let mut last_total: Option<Value> = None;

    for line in reader.lines().map_while(Result::ok) {
        let line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }
        let Ok(v): Result<Value, _> = serde_json::from_str(&line) else {
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
            }
            Some("event_msg") => {
                let Some(payload) = v.get("payload") else {
                    continue;
                };
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
        cost_usd,
        unknown_cost_sessions,
        ..Default::default()
    };

    // Populate per-model breakdown when model is known
    if let Some(m) = model.as_deref().filter(|m| !m.is_empty()) {
        usage.record_model(
            m,
            pure_input,
            0,
            cached_input_tokens,
            output_tokens,
            cost_usd,
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

pub fn collect_codex_usage(sessions_dir: &PathBuf, since: Option<DateTime<Utc>>) -> TokenUsage {
    let mut total = TokenUsage::default();

    if !sessions_dir.exists() {
        return total;
    }

    for entry in WalkDir::new(sessions_dir)
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

        if let Some(since_dt) = since
            && let Some(session_date) = codex_session_date(path)
            && session_date < since_dt
        {
            continue;
        }

        if let Some(usage) = parse_codex_session(path) {
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

    #[test]
    fn parses_last_token_count() {
        let data = format!(
            "{}\n{}\n{}\n",
            turn_context_line("gpt-4o"),
            token_count_line(100, 80, 10),
            token_count_line(200, 150, 25),
        );
        let usage = parse_codex_lines(cursor(&data)).expect("should parse");
        assert_eq!(usage.input_tokens, 200);
        assert_eq!(usage.cached_input_tokens, 150);
        assert_eq!(usage.output_tokens, 25);
        assert_eq!(usage.sessions, 1);
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
        // "gpt-5.4" should match "gpt-5" prefix → $1.25/M input
        let data = format!(
            "{}\n{}\n",
            turn_context_line("gpt-5.4"),
            token_count_line(1_000_000, 0, 0),
        );
        let usage = parse_codex_lines(cursor(&data)).expect("should parse");
        assert!((usage.cost_usd - 1.25).abs() < 0.001);
        assert_eq!(usage.unknown_cost_sessions, 0);
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
}
