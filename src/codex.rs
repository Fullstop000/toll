use chrono::{DateTime, Utc};
use serde_json::Value;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

use crate::usage::TokenUsage;

/// Parse Codex token_count events from any BufRead source.
/// Returns usage from the last token_count event (cumulative total for the session).
pub fn parse_codex_lines(reader: impl BufRead) -> Option<TokenUsage> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn cursor(s: &str) -> Cursor<Vec<u8>> {
        Cursor::new(s.as_bytes().to_vec())
    }

    #[test]
    fn session_date_parses_correctly() {
        let path = Path::new("/home/user/.codex/sessions/2026/03/08/rollout-2026-03-08T20-55-09-019ccd84-0e5f-7870-9c33-097188e35a30.jsonl");
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
    fn parses_last_token_count() {
        let data = r#"
{"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":100,"cached_input_tokens":80,"output_tokens":10},"last_token_usage":{}}}}
{"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":200,"cached_input_tokens":150,"output_tokens":25},"last_token_usage":{}}}}
"#;
        let usage = parse_codex_lines(cursor(data)).expect("should parse");
        assert_eq!(usage.input_tokens, 200);
        assert_eq!(usage.cached_input_tokens, 150);
        assert_eq!(usage.output_tokens, 25);
        assert_eq!(usage.sessions, 1);
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
        let data = "not json\n{\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"info\":{\"total_token_usage\":{\"input_tokens\":50,\"cached_input_tokens\":0,\"output_tokens\":5}}}}\n";
        let usage = parse_codex_lines(cursor(data)).expect("should parse despite bad line");
        assert_eq!(usage.input_tokens, 50);
    }

    #[test]
    fn empty_input_returns_none() {
        assert!(parse_codex_lines(cursor("")).is_none());
    }
}
