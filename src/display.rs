use std::collections::BTreeMap;

use crate::usage::{DailyUsage, TokenUsage};

/// Controls whether token counts are shown in compact or full-detail form.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NumberFormat {
    Compact,
    Full,
}

/// Summary headers used by multi-agent tables.
const MULTI_SUMMARY_HEADERS: [&str; 9] = [
    "Sessions",
    "Queries",
    "Input",
    "Cached",
    "Hit Rate",
    "Net Input",
    "Output",
    "Total",
    "Cost",
];

/// Format a raw integer with comma separators.
pub fn fmt_num(n: u64) -> String {
    let s = n.to_string();
    let mut result = String::new();
    for (i, ch) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push(',');
        }
        result.push(ch);
    }
    result.chars().rev().collect()
}

/// Format a token count according to the selected display mode.
pub fn fmt_num_with_format(n: u64, format: NumberFormat) -> String {
    match format {
        NumberFormat::Full => fmt_num(n),
        NumberFormat::Compact => {
            const BILLION: f64 = 1_000_000_000.0;
            const MILLION: f64 = 1_000_000.0;
            const THOUSAND: f64 = 1_000.0;

            if n >= 1_000_000_000 {
                format!("{:.1}b", round_to_one_decimal(n as f64 / BILLION))
            } else if n >= 1_000_000 {
                format!("{:.1}m", round_to_one_decimal(n as f64 / MILLION))
            } else if n >= 1_000 {
                format!("{:.1}k", round_to_one_decimal(n as f64 / THOUSAND))
            } else {
                n.to_string()
            }
        }
    }
}

/// Round to one decimal place using arithmetic rounding before formatting.
fn round_to_one_decimal(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

/// Format a percentage cell used in cached-token displays.
pub fn fmt_pct(part: u64, total: u64) -> String {
    if total == 0 {
        return "  0.0%".to_string();
    }
    format!("{:5.1}%", part as f64 / total as f64 * 100.0)
}

/// Format the cost cell while preserving unknown-cost markers.
pub fn fmt_cost(usage: &TokenUsage) -> String {
    if usage.sessions == 0 {
        return "—".to_string();
    }
    if usage.unknown_cost_sessions == usage.sessions {
        return "unknown".to_string();
    }
    let s = format!("${:.2}", usage.cost_usd);
    if usage.has_unknown_cost() {
        format!("{}*", s)
    } else {
        s
    }
}

/// Format the cached-token count cell.
fn fmt_cached_tokens(cached: u64, format: NumberFormat) -> String {
    fmt_num_with_format(cached, format)
}

/// Render all summary cells for one usage snapshot.
fn summary_values(usage: &TokenUsage, format: NumberFormat) -> [String; 9] {
    [
        fmt_num(usage.sessions as u64),
        fmt_num(usage.user_queries as u64),
        fmt_num_with_format(usage.input_tokens, format),
        fmt_cached_tokens(usage.cached_input_tokens, format),
        fmt_pct(usage.cached_input_tokens, usage.input_tokens),
        fmt_num_with_format(usage.net_input_tokens(), format),
        fmt_num_with_format(usage.output_tokens, format),
        fmt_num_with_format(usage.total_tokens(), format),
        fmt_cost(usage),
    ]
}

/// Combine all usage snapshots into a single aggregate total.
fn combined_usage(usages: &[(&str, &TokenUsage)]) -> TokenUsage {
    let mut combined = TokenUsage::default();
    for (_, usage) in usages {
        combined.add(usage);
    }
    combined
}

/// Render the aggregated per-model breakdown table.
fn render_model_breakdown(by_model: &BTreeMap<String, TokenUsage>, format: NumberFormat) -> String {
    if by_model.is_empty() {
        return String::new();
    }

    let col_w = 15usize;
    let label_w = 28usize;
    let total_w = label_w + 2 + (col_w + 2) * 3;

    let mut out = String::new();
    out.push_str("  By model:\n");
    out.push_str(&format!("  {}\n", "─".repeat(total_w)));
    out.push_str(&format!(
        "  {:<lw$} {:>cw$}  {:>cw$}  {:>cw$}\n",
        "Model",
        "Tokens",
        "Output",
        "Cost",
        lw = label_w,
        cw = col_w
    ));
    out.push_str(&format!("  {}\n", "─".repeat(total_w)));

    for (model, usage) in by_model {
        let label = if model.len() > label_w {
            format!("…{}", &model[model.len() - (label_w - 1)..])
        } else {
            model.clone()
        };
        out.push_str(&format!(
            "  {:<lw$} {:>cw$}  {:>cw$}  {:>cw$}\n",
            label,
            fmt_num_with_format(usage.total_tokens(), format),
            fmt_num_with_format(usage.output_tokens, format),
            fmt_cost(usage),
            lw = label_w,
            cw = col_w
        ));
    }

    out.push_str(&format!("  {}\n\n", "─".repeat(total_w)));
    out
}

/// Render the summary table for a single tool in a vertical key/value layout.
pub fn render_single_table(label: &str, usage: &TokenUsage, format: NumberFormat) -> String {
    let mut out = String::new();
    let values = summary_values(usage, format);
    let rows = [
        ("Sessions", values[0].as_str()),
        ("User queries", values[1].as_str()),
        ("Input tokens", values[2].as_str()),
        ("  ↳ cached", values[3].as_str()),
        ("  ↳ hit rate", values[4].as_str()),
        ("  ↳ net (non-cached)", values[5].as_str()),
        ("Output tokens", values[6].as_str()),
        ("Total tokens", values[7].as_str()),
        ("Estimated cost (USD)", values[8].as_str()),
    ];
    let label_w = rows
        .iter()
        .map(|(row_label, _)| row_label.len())
        .max()
        .unwrap_or(28);
    let col_w = std::iter::once(label.len())
        .chain(rows.iter().map(|(_, value)| value.len()))
        .max()
        .unwrap_or(15);
    let total_w = label_w + 2 + col_w;

    out.push('\n');
    out.push_str(&format!(
        "  {:<lw$} {:>cw$}\n",
        "",
        label,
        lw = label_w,
        cw = col_w
    ));
    out.push_str(&format!("  {}\n", "═".repeat(total_w)));
    for (idx, (row_label, value)) in rows.iter().enumerate() {
        out.push_str(&format!(
            "  {:<lw$} {:>cw$}\n",
            row_label,
            value,
            lw = label_w,
            cw = col_w
        ));
        if matches!(idx, 1 | 5 | 7) {
            out.push_str(&format!("  {}\n", "─".repeat(total_w)));
        }
    }
    out.push('\n');

    if usage.has_unknown_cost() {
        out.push_str(&format!(
            "  * pricing unavailable for {} session(s) — cost is understated\n\n",
            usage.unknown_cost_sessions
        ));
    }

    out.push_str(&render_model_breakdown(&usage.by_model, format));
    out
}

/// Print the summary table for a single tool.
pub fn print_single(label: &str, usage: &TokenUsage, format: NumberFormat) {
    print!("{}", render_single_table(label, usage, format));
}

/// Render the summary table for any number of agents plus the combined total.
pub fn render_multi_table(usages: &[(&str, &TokenUsage)], format: NumberFormat) -> String {
    let combined = combined_usage(usages);
    let mut rows: Vec<(&str, [String; 9])> = usages
        .iter()
        .map(|(name, usage)| (*name, summary_values(usage, format)))
        .collect();
    rows.push(("Combined", summary_values(&combined, format)));

    let label_w = rows.iter().map(|(name, _)| name.len()).max().unwrap_or(15);
    let mut col_widths: Vec<usize> = MULTI_SUMMARY_HEADERS
        .iter()
        .map(|header| header.len())
        .collect();
    for row in &rows {
        for (idx, cell) in row.1.iter().enumerate() {
            col_widths[idx] = col_widths[idx].max(cell.len());
        }
    }
    let total_w =
        label_w + 2 + col_widths.iter().sum::<usize>() + 2 * col_widths.len().saturating_sub(1);

    let mut out = String::new();
    out.push('\n');
    out.push_str(&format!("  {:<lw$}", "", lw = label_w));
    for (header, width) in MULTI_SUMMARY_HEADERS.iter().zip(col_widths.iter()) {
        out.push_str(&format!(" {:>cw$} ", header, cw = *width));
    }
    out.push('\n');
    out.push_str(&format!("  {}\n", "═".repeat(total_w)));

    for (idx, (name, values)) in rows.iter().enumerate() {
        if idx + 1 == rows.len() {
            out.push_str(&format!("  {}\n", "─".repeat(total_w)));
        }
        out.push_str(&format!("  {:<lw$}", name, lw = label_w));
        for (cell, width) in values.iter().zip(col_widths.iter()) {
            out.push_str(&format!(" {:>cw$} ", cell, cw = *width));
        }
        out.push('\n');
    }
    out.push_str(&format!("  {}\n", "─".repeat(total_w)));
    out.push('\n');

    if combined.has_unknown_cost() {
        out.push_str(&format!(
            "  * pricing unavailable for {} session(s) — cost is understated\n\n",
            combined.unknown_cost_sessions
        ));
    }

    let mut all_models: BTreeMap<String, TokenUsage> = BTreeMap::new();
    for (_, usage) in usages {
        for (model, model_usage) in &usage.by_model {
            all_models
                .entry(model.clone())
                .or_default()
                .add(model_usage);
        }
    }
    out.push_str(&render_model_breakdown(&all_models, format));
    out
}

/// Print the summary table for any number of agents plus the combined total.
pub fn print_multi_table(usages: &[(&str, &TokenUsage)], format: NumberFormat) {
    print!("{}", render_multi_table(usages, format));
}

/// Render the daily summary table for the selected period.
pub fn render_daily_table(period: &str, by_day: &DailyUsage, format: NumberFormat) -> String {
    let mut out = String::new();
    out.push('\n');
    out.push_str(&format!("Daily usage — {}\n\n", period));

    if by_day.is_empty() {
        out.push_str("  No usage found.\n");
        return out;
    }

    let headers = [
        "Date",
        "Sessions",
        "Queries",
        "Input",
        "Cached",
        "Net Input",
        "Output",
        "Total",
        "Cost",
    ];

    let rows: Vec<[String; 9]> = by_day
        .iter()
        .rev()
        .map(|(date, usage)| {
            [
                date.format("%Y-%m-%d").to_string(),
                fmt_num(usage.sessions as u64),
                fmt_num(usage.user_queries as u64),
                fmt_num_with_format(usage.input_tokens, format),
                fmt_num_with_format(usage.cached_input_tokens, format),
                fmt_num_with_format(usage.net_input_tokens(), format),
                fmt_num_with_format(usage.output_tokens, format),
                fmt_num_with_format(usage.total_tokens(), format),
                fmt_cost(usage),
            ]
        })
        .collect();

    let mut widths: Vec<usize> = headers.iter().map(|header| header.len()).collect();
    for row in &rows {
        for (idx, cell) in row.iter().enumerate() {
            widths[idx] = widths[idx].max(cell.len());
        }
    }

    out.push_str(&format!(
        "  {:<w0$}  {:>w1$}  {:>w2$}  {:>w3$}  {:>w4$}  {:>w5$}  {:>w6$}  {:>w7$}  {:>w8$}\n",
        headers[0],
        headers[1],
        headers[2],
        headers[3],
        headers[4],
        headers[5],
        headers[6],
        headers[7],
        headers[8],
        w0 = widths[0],
        w1 = widths[1],
        w2 = widths[2],
        w3 = widths[3],
        w4 = widths[4],
        w5 = widths[5],
        w6 = widths[6],
        w7 = widths[7],
        w8 = widths[8],
    ));

    let rule_len: usize = widths.iter().sum::<usize>() + 2 * (widths.len() - 1);
    out.push_str(&format!("  {}\n", "─".repeat(rule_len)));

    for row in rows {
        out.push_str(&format!(
            "  {:<w0$}  {:>w1$}  {:>w2$}  {:>w3$}  {:>w4$}  {:>w5$}  {:>w6$}  {:>w7$}  {:>w8$}\n",
            row[0],
            row[1],
            row[2],
            row[3],
            row[4],
            row[5],
            row[6],
            row[7],
            row[8],
            w0 = widths[0],
            w1 = widths[1],
            w2 = widths[2],
            w3 = widths[3],
            w4 = widths[4],
            w5 = widths[5],
            w6 = widths[6],
            w7 = widths[7],
            w8 = widths[8],
        ));
    }

    out
}

/// Print the daily summary table.
pub fn print_daily_table(period: &str, by_day: &DailyUsage, format: NumberFormat) {
    print!("{}", render_daily_table(period, by_day, format));
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    #[test]
    fn fmt_num_formats_with_commas() {
        assert_eq!(fmt_num(0), "0");
        assert_eq!(fmt_num(999), "999");
        assert_eq!(fmt_num(1_000), "1,000");
        assert_eq!(fmt_num(1_234_567), "1,234,567");
        assert_eq!(fmt_num(1_000_000_000), "1,000,000,000");
    }

    #[test]
    fn fmt_num_compact_uses_suffixes() {
        assert_eq!(fmt_num_with_format(999, NumberFormat::Compact), "999");
        assert_eq!(fmt_num_with_format(1_250, NumberFormat::Compact), "1.3k");
        assert_eq!(
            fmt_num_with_format(1_234_567, NumberFormat::Compact),
            "1.2m"
        );
        assert_eq!(
            fmt_num_with_format(1_500_000_000, NumberFormat::Compact),
            "1.5b"
        );
    }

    #[test]
    fn fmt_num_full_keeps_raw_value() {
        assert_eq!(
            fmt_num_with_format(1_234_567, NumberFormat::Full),
            "1,234,567"
        );
    }

    #[test]
    fn fmt_pct_zero_total() {
        assert_eq!(fmt_pct(0, 0), "  0.0%");
    }

    #[test]
    fn fmt_pct_full() {
        assert_eq!(fmt_pct(100, 100), "100.0%");
    }

    #[test]
    fn fmt_pct_half() {
        assert_eq!(fmt_pct(50, 100), " 50.0%");
    }

    #[test]
    fn fmt_cost_known() {
        let usage = TokenUsage {
            sessions: 1,
            cost_usd: 12.345,
            ..Default::default()
        };
        assert_eq!(fmt_cost(&usage), "$12.35");
    }

    #[test]
    fn fmt_cost_all_unknown() {
        let usage = TokenUsage {
            sessions: 2,
            unknown_cost_sessions: 2,
            ..Default::default()
        };
        assert_eq!(fmt_cost(&usage), "unknown");
    }

    #[test]
    fn fmt_cost_partial_unknown() {
        let usage = TokenUsage {
            sessions: 3,
            cost_usd: 5.0,
            unknown_cost_sessions: 1,
            ..Default::default()
        };
        assert_eq!(fmt_cost(&usage), "$5.00*");
    }

    #[test]
    fn fmt_cost_no_sessions() {
        let usage = TokenUsage::default();
        assert_eq!(fmt_cost(&usage), "—");
    }

    #[test]
    fn render_single_table_keeps_vertical_layout() {
        let usage = TokenUsage {
            input_tokens: 1_000_000,
            cached_input_tokens: 750_000,
            sessions: 1,
            cost_usd: 12.34,
            ..Default::default()
        };

        let rendered = render_single_table("Codex", &usage, NumberFormat::Full);

        assert!(rendered.contains("  Sessions"));
        assert!(rendered.contains("  Input tokens"));
        assert!(rendered.contains("  ↳ hit rate"));
        assert!(rendered.contains("Estimated cost (USD)"));
        assert!(rendered.contains("Codex"));
        assert!(rendered.contains("$12.34"));
        assert!(!rendered.contains("Cached              Net input"));
    }

    #[test]
    fn render_multi_table_supports_three_agents() {
        let claude = TokenUsage {
            input_tokens: 100,
            output_tokens: 20,
            sessions: 1,
            cost_usd: 1.0,
            ..Default::default()
        };
        let codex = TokenUsage {
            input_tokens: 200,
            output_tokens: 30,
            sessions: 2,
            cost_usd: 2.0,
            ..Default::default()
        };
        let gemini = TokenUsage {
            input_tokens: 300,
            output_tokens: 40,
            sessions: 3,
            cost_usd: 3.0,
            ..Default::default()
        };

        let rendered = render_multi_table(
            &[
                ("Claude Code", &claude),
                ("Codex", &codex),
                ("Gemini", &gemini),
            ],
            NumberFormat::Full,
        );

        assert!(rendered.contains("Claude Code"));
        assert!(rendered.contains("Codex"));
        assert!(rendered.contains("Gemini"));
        assert!(rendered.contains("Combined"));
        assert!(rendered.contains("$6.00"));
        assert!(rendered.contains("Sessions"));
        assert!(rendered.contains("Input"));
        assert!(rendered.contains("Hit Rate"));
        assert!(rendered.contains("Net Input"));
        assert!(rendered.contains("Cost"));
        assert!(!rendered.contains("Input tokens"));
        assert!(!rendered.contains("Output tokens"));
        assert!(!rendered.contains("Total tokens"));
        assert!(rendered.contains("0.0%"));

        let header_pos = rendered
            .find("Sessions")
            .expect("header should contain metrics");
        let claude_pos = rendered
            .find("Claude Code")
            .expect("table should contain a Claude row");
        let combined_pos = rendered
            .find("Combined")
            .expect("table should contain a combined row");
        assert!(header_pos < claude_pos);
        assert!(claude_pos < combined_pos);

        let combined_line = rendered
            .find("\n  Combined")
            .expect("combined row should exist");
        let separator_before_combined = rendered[..combined_line]
            .rfind("\n  ─")
            .expect("separator should precede combined row");
        assert!(separator_before_combined < combined_line);
    }

    #[test]
    fn render_daily_table_lists_latest_day_first() {
        let mut by_day = DailyUsage::default();
        by_day.insert(
            NaiveDate::from_ymd_opt(2026, 3, 10).expect("valid date"),
            TokenUsage {
                sessions: 1,
                input_tokens: 1_000,
                output_tokens: 50,
                cost_usd: 1.25,
                ..Default::default()
            },
        );
        by_day.insert(
            NaiveDate::from_ymd_opt(2026, 3, 12).expect("valid date"),
            TokenUsage {
                sessions: 2,
                input_tokens: 2_000,
                cached_input_tokens: 500,
                output_tokens: 100,
                cost_usd: 2.50,
                ..Default::default()
            },
        );

        let rendered = render_daily_table("last 7 days", &by_day, NumberFormat::Compact);

        let latest = rendered
            .find("2026-03-12")
            .expect("latest date should appear");
        let older = rendered
            .find("2026-03-10")
            .expect("older date should appear");
        assert!(latest < older);
        assert!(rendered.contains("Daily usage"));
        assert!(rendered.contains("Net Input"));
    }
}
