use std::collections::BTreeMap;

use crate::usage::TokenUsage;

/// Controls whether token counts are shown in compact or full-detail form.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NumberFormat {
    Compact,
    Full,
}

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

pub fn fmt_pct(part: u64, total: u64) -> String {
    if total == 0 {
        return "  0.0%".to_string();
    }
    format!("{:5.1}%", part as f64 / total as f64 * 100.0)
}

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

/// Build the cached-token cell with its hit-rate percentage.
fn fmt_cached_tokens(cached: u64, total_input: u64, format: NumberFormat) -> String {
    format!(
        "{} ({})",
        fmt_num_with_format(cached, format),
        fmt_pct(cached, total_input)
    )
}

/// Compute the summary-table column width from headers and rendered values.
fn summary_col_width(
    claude: &TokenUsage,
    codex: &TokenUsage,
    combined: &TokenUsage,
    format: NumberFormat,
) -> usize {
    let headers = ["Claude Code", "Codex", "Combined"];
    let values = [
        fmt_num(claude.sessions as u64),
        fmt_num(codex.sessions as u64),
        fmt_num(combined.sessions as u64),
        fmt_num_with_format(claude.input_tokens, format),
        fmt_num_with_format(codex.input_tokens, format),
        fmt_num_with_format(combined.input_tokens, format),
        fmt_cached_tokens(claude.cached_input_tokens, claude.input_tokens, format),
        fmt_cached_tokens(codex.cached_input_tokens, codex.input_tokens, format),
        fmt_cached_tokens(combined.cached_input_tokens, combined.input_tokens, format),
        fmt_num_with_format(claude.net_input_tokens(), format),
        fmt_num_with_format(codex.net_input_tokens(), format),
        fmt_num_with_format(combined.net_input_tokens(), format),
        fmt_num_with_format(claude.output_tokens, format),
        fmt_num_with_format(codex.output_tokens, format),
        fmt_num_with_format(combined.output_tokens, format),
        fmt_num_with_format(claude.total_tokens(), format),
        fmt_num_with_format(codex.total_tokens(), format),
        fmt_num_with_format(combined.total_tokens(), format),
        fmt_cost(claude),
        fmt_cost(codex),
        fmt_cost(combined),
    ];

    headers
        .into_iter()
        .map(str::len)
        .chain(values.iter().map(String::len))
        .max()
        .unwrap_or(15)
}

/// Compute the single-app column width from rendered values.
fn single_col_width(label: &str, usage: &TokenUsage, format: NumberFormat) -> usize {
    let values = [
        label.to_string(),
        fmt_num(usage.sessions as u64),
        fmt_num_with_format(usage.input_tokens, format),
        fmt_cached_tokens(usage.cached_input_tokens, usage.input_tokens, format),
        fmt_num_with_format(usage.net_input_tokens(), format),
        fmt_num_with_format(usage.output_tokens, format),
        fmt_num_with_format(usage.total_tokens(), format),
        fmt_cost(usage),
    ];

    values.iter().map(String::len).max().unwrap_or(15)
}

/// Print the side-by-side summary table for Claude Code, Codex, and the combined totals.
pub fn print_table(claude: &TokenUsage, codex: &TokenUsage, format: NumberFormat) {
    let combined = TokenUsage {
        input_tokens: claude.input_tokens + codex.input_tokens,
        cached_input_tokens: claude.cached_input_tokens + codex.cached_input_tokens,
        cache_write_tokens: claude.cache_write_tokens + codex.cache_write_tokens,
        output_tokens: claude.output_tokens + codex.output_tokens,
        sessions: claude.sessions + codex.sessions,
        cost_usd: claude.cost_usd + codex.cost_usd,
        unknown_cost_sessions: claude.unknown_cost_sessions + codex.unknown_cost_sessions,
        ..Default::default()
    };

    let col_w = summary_col_width(claude, codex, &combined, format);
    let label_w = 28usize;
    let total_w = label_w + 2 + (col_w + 2) * 3;

    let row = |label: &str, c: &str, d: &str, t: &str| {
        println!(
            "  {:<lw$} {:>cw$}  {:>cw$}  {:>cw$}",
            label,
            c,
            d,
            t,
            lw = label_w,
            cw = col_w
        );
    };

    println!();
    println!(
        "  {:<lw$} {:>cw$}  {:>cw$}  {:>cw$}",
        "",
        "Claude Code",
        "Codex",
        "Combined",
        lw = label_w,
        cw = col_w
    );
    println!("  {}", "═".repeat(total_w));

    row(
        "Sessions",
        &fmt_num(claude.sessions as u64),
        &fmt_num(codex.sessions as u64),
        &fmt_num(combined.sessions as u64),
    );
    println!("  {}", "─".repeat(total_w));

    row(
        "Input tokens",
        &fmt_num_with_format(claude.input_tokens, format),
        &fmt_num_with_format(codex.input_tokens, format),
        &fmt_num_with_format(combined.input_tokens, format),
    );
    row(
        "  ↳ cached",
        &fmt_cached_tokens(claude.cached_input_tokens, claude.input_tokens, format),
        &fmt_cached_tokens(codex.cached_input_tokens, codex.input_tokens, format),
        &fmt_cached_tokens(combined.cached_input_tokens, combined.input_tokens, format),
    );
    row(
        "  ↳ net (non-cached)",
        &fmt_num_with_format(claude.net_input_tokens(), format),
        &fmt_num_with_format(codex.net_input_tokens(), format),
        &fmt_num_with_format(combined.net_input_tokens(), format),
    );
    row(
        "Output tokens",
        &fmt_num_with_format(claude.output_tokens, format),
        &fmt_num_with_format(codex.output_tokens, format),
        &fmt_num_with_format(combined.output_tokens, format),
    );
    println!("  {}", "─".repeat(total_w));
    row(
        "Total tokens",
        &fmt_num_with_format(claude.total_tokens(), format),
        &fmt_num_with_format(codex.total_tokens(), format),
        &fmt_num_with_format(combined.total_tokens(), format),
    );
    println!("  {}", "─".repeat(total_w));
    row(
        "Estimated cost (USD)",
        &fmt_cost(claude),
        &fmt_cost(codex),
        &fmt_cost(&combined),
    );
    println!();

    if combined.has_unknown_cost() {
        println!(
            "  * pricing unavailable for {} session(s) — cost is understated",
            combined.unknown_cost_sessions
        );
        println!();
    }

    // Merge both breakdowns and display
    let mut all_models: BTreeMap<String, TokenUsage> = BTreeMap::new();
    for (m, u) in &claude.by_model {
        all_models.entry(m.clone()).or_default().add(u);
    }
    for (m, u) in &codex.by_model {
        all_models.entry(m.clone()).or_default().add(u);
    }
    print_model_breakdown(&all_models, format);
}

/// Print the aggregated per-model breakdown table.
fn print_model_breakdown(by_model: &BTreeMap<String, TokenUsage>, format: NumberFormat) {
    if by_model.is_empty() {
        return;
    }

    let col_w = 15usize;
    let label_w = 28usize;
    let total_w = label_w + 2 + (col_w + 2) * 3;

    println!("  By model:");
    println!("  {}", "─".repeat(total_w));
    println!(
        "  {:<lw$} {:>cw$}  {:>cw$}  {:>cw$}",
        "Model",
        "Tokens",
        "Output",
        "Cost",
        lw = label_w,
        cw = col_w
    );
    println!("  {}", "─".repeat(total_w));

    for (model, u) in by_model {
        let label = if model.len() > label_w {
            format!("…{}", &model[model.len() - (label_w - 1)..])
        } else {
            model.clone()
        };
        println!(
            "  {:<lw$} {:>cw$}  {:>cw$}  {:>cw$}",
            label,
            fmt_num_with_format(u.total_tokens(), format),
            fmt_num_with_format(u.output_tokens, format),
            fmt_cost(u),
            lw = label_w,
            cw = col_w
        );
    }
    println!("  {}", "─".repeat(total_w));
    println!();
}

/// Print the summary table for a single tool.
pub fn print_single(label: &str, usage: &TokenUsage, format: NumberFormat) {
    let col_w = single_col_width(label, usage, format);
    let label_w = 28usize;
    let total_w = label_w + 2 + col_w;

    let row = |lbl: &str, val: &str| {
        println!("  {:<lw$} {:>cw$}", lbl, val, lw = label_w, cw = col_w);
    };

    println!();
    println!("  {:<lw$} {:>cw$}", "", label, lw = label_w, cw = col_w);
    println!("  {}", "═".repeat(total_w));
    row("Sessions", &fmt_num(usage.sessions as u64));
    println!("  {}", "─".repeat(total_w));
    row(
        "Input tokens",
        &fmt_num_with_format(usage.input_tokens, format),
    );
    row(
        "  ↳ cached",
        &fmt_cached_tokens(usage.cached_input_tokens, usage.input_tokens, format),
    );
    row(
        "  ↳ net (non-cached)",
        &fmt_num_with_format(usage.net_input_tokens(), format),
    );
    row(
        "Output tokens",
        &fmt_num_with_format(usage.output_tokens, format),
    );
    println!("  {}", "─".repeat(total_w));
    row(
        "Total tokens",
        &fmt_num_with_format(usage.total_tokens(), format),
    );
    println!("  {}", "─".repeat(total_w));
    row("Estimated cost (USD)", &fmt_cost(usage));
    println!();

    if usage.has_unknown_cost() {
        println!(
            "  * pricing unavailable for {} session(s) — cost is understated",
            usage.unknown_cost_sessions
        );
        println!();
    }

    print_model_breakdown(&usage.by_model, format);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usage::TokenUsage;

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
        let u = TokenUsage {
            sessions: 1,
            cost_usd: 12.345,
            ..Default::default()
        };
        assert_eq!(fmt_cost(&u), "$12.35");
    }

    #[test]
    fn fmt_cost_all_unknown() {
        let u = TokenUsage {
            sessions: 2,
            unknown_cost_sessions: 2,
            ..Default::default()
        };
        assert_eq!(fmt_cost(&u), "unknown");
    }

    #[test]
    fn fmt_cost_partial_unknown() {
        let u = TokenUsage {
            sessions: 3,
            cost_usd: 5.0,
            unknown_cost_sessions: 1,
            ..Default::default()
        };
        assert_eq!(fmt_cost(&u), "$5.00*");
    }

    #[test]
    fn fmt_cost_no_sessions() {
        let u = TokenUsage::default();
        assert_eq!(fmt_cost(&u), "—");
    }

    #[test]
    fn summary_col_width_expands_for_cached_row() {
        let claude = TokenUsage {
            input_tokens: 200_000_000,
            cached_input_tokens: 193_981_784,
            sessions: 1,
            ..Default::default()
        };
        let codex = TokenUsage {
            input_tokens: 120_000_000,
            cached_input_tokens: 103_838_208,
            sessions: 1,
            ..Default::default()
        };
        let combined = TokenUsage {
            input_tokens: claude.input_tokens + codex.input_tokens,
            cached_input_tokens: claude.cached_input_tokens + codex.cached_input_tokens,
            sessions: 2,
            ..Default::default()
        };

        assert!(summary_col_width(&claude, &codex, &combined, NumberFormat::Full) > 15);
    }

    #[test]
    fn single_col_width_expands_for_cached_row() {
        let usage = TokenUsage {
            input_tokens: 200_000_000,
            cached_input_tokens: 193_981_784,
            sessions: 1,
            ..Default::default()
        };

        assert!(single_col_width("Codex", &usage, NumberFormat::Full) > 15);
    }
}
