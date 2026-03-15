use serde::Serialize;
use std::collections::BTreeMap;

use crate::display::{NumberFormat, fmt_cost, fmt_num_with_format, fmt_pct};
use crate::usage::{DailyUsage, TokenUsage};

/// Machine-readable output mode selected by CLI flags.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutputMode {
    Table,
    Json,
    Csv,
}

/// Parsed filter metadata emitted in JSON output.
#[derive(Debug, Serialize)]
pub struct OutputFilters {
    pub today: bool,
    pub days: Option<u32>,
    pub claude: bool,
    pub codex: bool,
    pub kimi: bool,
    pub gemini: bool,
    pub by_day: bool,
    pub detail: bool,
}

/// JSON summary payload for a usage report.
#[derive(Debug, Serialize)]
pub struct JsonOutput {
    pub period: String,
    pub collected_at: String,
    pub filters: OutputFilters,
    pub elapsed_seconds: f64,
    pub sessions_scanned: u32,
    pub view: JsonView,
}

/// Variant payload for summary vs. daily responses.
#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum JsonView {
    Summary {
        agents: Vec<JsonAgentUsage>,
        combined: Option<JsonUsageRecord>,
    },
    Daily {
        days: Vec<JsonDailyUsage>,
    },
}

/// Agent-level usage entry emitted in JSON summary output.
#[derive(Debug, Serialize)]
pub struct JsonAgentUsage {
    pub name: String,
    pub usage: JsonUsageRecord,
    pub models: Vec<JsonModelUsage>,
}

/// Model-level usage entry emitted in JSON summary output.
#[derive(Debug, Serialize)]
pub struct JsonModelUsage {
    pub name: String,
    pub usage: JsonUsageRecord,
}

/// Shared usage fields used by JSON responses.
#[derive(Debug, Serialize)]
pub struct JsonUsageRecord {
    pub input_tokens: u64,
    pub cached_input_tokens: u64,
    pub cache_hit_rate_pct: f64,
    pub net_input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    pub sessions: u32,
    pub cost_usd: f64,
    pub unknown_cost_sessions: u32,
}

/// Per-day usage entry emitted in JSON daily output.
#[derive(Debug, Serialize)]
pub struct JsonDailyUsage {
    pub date: String,
    #[serde(flatten)]
    pub usage: JsonUsageRecord,
}

/// Build a JSON usage record from an internal usage snapshot.
fn json_usage_record(usage: &TokenUsage) -> JsonUsageRecord {
    let hit_rate = if usage.input_tokens == 0 {
        0.0
    } else {
        usage.cached_input_tokens as f64 / usage.input_tokens as f64 * 100.0
    };

    JsonUsageRecord {
        input_tokens: usage.input_tokens,
        cached_input_tokens: usage.cached_input_tokens,
        cache_hit_rate_pct: hit_rate,
        net_input_tokens: usage.net_input_tokens(),
        output_tokens: usage.output_tokens,
        total_tokens: usage.total_tokens(),
        sessions: usage.sessions,
        cost_usd: usage.cost_usd,
        unknown_cost_sessions: usage.unknown_cost_sessions,
    }
}

/// Combine the provided usage rows into a single total.
fn combined_usage(usages: &[(&str, &TokenUsage)]) -> TokenUsage {
    let mut combined = TokenUsage::default();
    for (_, usage) in usages {
        combined.add(usage);
    }
    combined
}

/// Escape a single CSV field for terminal output.
fn csv_field(value: &str) -> String {
    if value.contains([',', '"', '\n']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

/// Aggregate per-model usage across all selected agents.
fn combined_models(usages: &[(&str, &TokenUsage)]) -> BTreeMap<String, TokenUsage> {
    let mut all_models = BTreeMap::new();
    for (_, usage) in usages {
        for (model, model_usage) in &usage.by_model {
            all_models
                .entry(model.clone())
                .or_insert_with(TokenUsage::default)
                .add(model_usage);
        }
    }
    all_models
}

/// Render one CSV summary row.
fn csv_summary_row(name: &str, usage: &TokenUsage, format: NumberFormat) -> String {
    let values = [
        name.to_string(),
        usage.sessions.to_string(),
        fmt_num_with_format(usage.input_tokens, format),
        fmt_num_with_format(usage.cached_input_tokens, format),
        fmt_pct(usage.cached_input_tokens, usage.input_tokens)
            .trim()
            .to_string(),
        fmt_num_with_format(usage.net_input_tokens(), format),
        fmt_num_with_format(usage.output_tokens, format),
        fmt_num_with_format(usage.total_tokens(), format),
        fmt_cost(usage),
    ];

    values
        .iter()
        .map(|value| csv_field(value))
        .collect::<Vec<_>>()
        .join(",")
}

/// Render one CSV by-model row.
fn csv_model_row(name: &str, usage: &TokenUsage, format: NumberFormat) -> String {
    let values = [
        name.to_string(),
        fmt_num_with_format(usage.total_tokens(), format),
        fmt_num_with_format(usage.output_tokens, format),
        fmt_cost(usage),
    ];

    values
        .iter()
        .map(|value| csv_field(value))
        .collect::<Vec<_>>()
        .join(",")
}

/// Render summary CSV for the currently selected agents.
pub fn render_summary_csv(usages: &[(&str, &TokenUsage)], format: NumberFormat) -> String {
    let mut lines =
        vec!["Agent,Sessions,Input,Cached,Hit Rate,Net Input,Output,Total,Cost".to_string()];

    for (name, usage) in usages {
        lines.push(csv_summary_row(name, usage, format));
    }

    if usages.len() > 1 {
        let combined = combined_usage(usages);
        lines.push(csv_summary_row("Combined", &combined, format));
    }

    let all_models = combined_models(usages);
    if !all_models.is_empty() {
        lines.push(String::new());
        lines.push("Model,Tokens,Output,Cost".to_string());
        for (model, usage) in all_models {
            lines.push(csv_model_row(&model, &usage, format));
        }
    }

    lines.join("\n")
}

/// Render daily CSV for the current aggregated daily view.
pub fn render_daily_csv(by_day: &DailyUsage, format: NumberFormat) -> String {
    let mut lines =
        vec!["Date,Sessions,Input,Cached,Hit Rate,Net Input,Output,Total,Cost".to_string()];

    for (date, usage) in by_day.iter().rev() {
        let values = [
            date.format("%Y-%m-%d").to_string(),
            usage.sessions.to_string(),
            fmt_num_with_format(usage.input_tokens, format),
            fmt_num_with_format(usage.cached_input_tokens, format),
            fmt_pct(usage.cached_input_tokens, usage.input_tokens)
                .trim()
                .to_string(),
            fmt_num_with_format(usage.net_input_tokens(), format),
            fmt_num_with_format(usage.output_tokens, format),
            fmt_num_with_format(usage.total_tokens(), format),
            fmt_cost(usage),
        ];
        lines.push(
            values
                .iter()
                .map(|value| csv_field(value))
                .collect::<Vec<_>>()
                .join(","),
        );
    }

    lines.join("\n")
}

/// Render summary JSON for the currently selected agents.
pub fn render_summary_json(
    period: &str,
    collected_at: &str,
    filters: OutputFilters,
    elapsed_seconds: f64,
    sessions_scanned: u32,
    usages: &[(&str, &TokenUsage)],
) -> Result<String, serde_json::Error> {
    let agents = usages
        .iter()
        .map(|(name, usage)| JsonAgentUsage {
            name: (*name).to_string(),
            usage: json_usage_record(usage),
            models: usage
                .by_model
                .iter()
                .map(|(model, model_usage)| JsonModelUsage {
                    name: model.clone(),
                    usage: json_usage_record(model_usage),
                })
                .collect(),
        })
        .collect::<Vec<_>>();

    let combined = if usages.len() > 1 {
        Some(json_usage_record(&combined_usage(usages)))
    } else {
        None
    };

    serde_json::to_string_pretty(&JsonOutput {
        period: period.to_string(),
        collected_at: collected_at.to_string(),
        filters,
        elapsed_seconds,
        sessions_scanned,
        view: JsonView::Summary { agents, combined },
    })
}

/// Render daily JSON for the current aggregated daily view.
pub fn render_daily_json(
    period: &str,
    collected_at: &str,
    filters: OutputFilters,
    elapsed_seconds: f64,
    sessions_scanned: u32,
    by_day: &DailyUsage,
) -> Result<String, serde_json::Error> {
    let days = by_day
        .iter()
        .rev()
        .map(|(date, usage)| JsonDailyUsage {
            date: date.format("%Y-%m-%d").to_string(),
            usage: json_usage_record(usage),
        })
        .collect::<Vec<_>>();

    serde_json::to_string_pretty(&JsonOutput {
        period: period.to_string(),
        collected_at: collected_at.to_string(),
        filters,
        elapsed_seconds,
        sessions_scanned,
        view: JsonView::Daily { days },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    #[test]
    fn render_summary_csv_prints_terminal_friendly_rows() {
        let mut usage = TokenUsage {
            input_tokens: 12_500,
            cached_input_tokens: 2_500,
            output_tokens: 500,
            sessions: 2,
            cost_usd: 1.25,
            ..Default::default()
        };
        usage.record_model("gpt-5.4", 10_000, 0, 2_500, 500, 1.25);

        let rendered = render_summary_csv(&[("Codex", &usage)], NumberFormat::Compact);

        assert!(rendered.starts_with("Agent,Sessions,Input,Cached,Hit Rate"));
        assert!(rendered.contains("Codex,2,12.5k,2.5k,20.0%,10.0k,500,13.0k,$1.25"));
        assert!(rendered.contains("\n\nModel,Tokens,Output,Cost\n"));
        assert!(rendered.contains("gpt-5.4,13.0k,500,$1.25"));
    }

    #[test]
    fn render_daily_csv_respects_latest_day_first() {
        let mut by_day = DailyUsage::default();
        by_day.insert(
            NaiveDate::from_ymd_opt(2026, 3, 12).expect("valid date"),
            TokenUsage {
                sessions: 1,
                input_tokens: 1_000,
                ..Default::default()
            },
        );
        by_day.insert(
            NaiveDate::from_ymd_opt(2026, 3, 13).expect("valid date"),
            TokenUsage {
                sessions: 2,
                input_tokens: 2_000,
                cached_input_tokens: 500,
                ..Default::default()
            },
        );

        let rendered = render_daily_csv(&by_day, NumberFormat::Compact);
        let latest = rendered
            .find("2026-03-13")
            .expect("latest date should appear");
        let older = rendered
            .find("2026-03-12")
            .expect("older date should appear");
        assert!(latest < older);
        assert!(rendered.contains("Date,Sessions,Input,Cached,Hit Rate"));
    }

    #[test]
    fn render_summary_json_includes_combined_usage() {
        let claude = TokenUsage {
            input_tokens: 100,
            output_tokens: 10,
            sessions: 1,
            cost_usd: 1.0,
            ..Default::default()
        };
        let codex = TokenUsage {
            input_tokens: 200,
            output_tokens: 20,
            sessions: 2,
            cost_usd: 2.0,
            ..Default::default()
        };

        let rendered = render_summary_json(
            "all time",
            "2026-03-13T01:00:00+08:00",
            OutputFilters {
                today: false,
                days: None,
                claude: false,
                codex: false,
                kimi: false,
                gemini: false,
                by_day: false,
                detail: false,
            },
            0.25,
            3,
            &[("Claude Code", &claude), ("Codex", &codex)],
        )
        .expect("json should render");

        assert!(rendered.contains("\"type\": \"summary\""));
        assert!(rendered.contains("\"combined\""));
        assert!(rendered.contains("\"total_tokens\": 330"));
    }
}
