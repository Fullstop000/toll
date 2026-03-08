use crate::usage::TokenUsage;

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

pub fn print_table(claude: &TokenUsage, codex: &TokenUsage) {
    let combined = TokenUsage {
        input_tokens: claude.input_tokens + codex.input_tokens,
        cached_input_tokens: claude.cached_input_tokens + codex.cached_input_tokens,
        cache_write_tokens: claude.cache_write_tokens + codex.cache_write_tokens,
        output_tokens: claude.output_tokens + codex.output_tokens,
        sessions: claude.sessions + codex.sessions,
        cost_usd: claude.cost_usd + codex.cost_usd,
        unknown_cost_sessions: claude.unknown_cost_sessions + codex.unknown_cost_sessions,
    };

    let col_w = 15usize;
    let label_w = 28usize;
    let total_w = label_w + 2 + (col_w + 2) * 3;

    let row = |label: &str, c: &str, d: &str, t: &str| {
        println!(
            "  {:<lw$} {:>cw$}  {:>cw$}  {:>cw$}",
            label, c, d, t, lw = label_w, cw = col_w
        );
    };

    println!();
    println!(
        "  {:<lw$} {:>cw$}  {:>cw$}  {:>cw$}",
        "", "Claude Code", "Codex", "Combined", lw = label_w, cw = col_w
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
        &fmt_num(claude.input_tokens),
        &fmt_num(codex.input_tokens),
        &fmt_num(combined.input_tokens),
    );
    row(
        "  ↳ cached",
        &format!("{} ({})", fmt_num(claude.cached_input_tokens), fmt_pct(claude.cached_input_tokens, claude.input_tokens)),
        &format!("{} ({})", fmt_num(codex.cached_input_tokens),  fmt_pct(codex.cached_input_tokens,  codex.input_tokens)),
        &format!("{} ({})", fmt_num(combined.cached_input_tokens), fmt_pct(combined.cached_input_tokens, combined.input_tokens)),
    );
    row(
        "  ↳ net (non-cached)",
        &fmt_num(claude.net_input_tokens()),
        &fmt_num(codex.net_input_tokens()),
        &fmt_num(combined.net_input_tokens()),
    );
    row(
        "Output tokens",
        &fmt_num(claude.output_tokens),
        &fmt_num(codex.output_tokens),
        &fmt_num(combined.output_tokens),
    );
    println!("  {}", "─".repeat(total_w));
    row(
        "Total tokens",
        &fmt_num(claude.total_tokens()),
        &fmt_num(codex.total_tokens()),
        &fmt_num(combined.total_tokens()),
    );
    println!("  {}", "─".repeat(total_w));
    row(
        "Estimated cost (USD)",
        &fmt_cost(claude),
        &fmt_cost(codex),
        &fmt_cost(&combined),
    );
    println!();

    let has_unknown = combined.has_unknown_cost();
    if has_unknown {
        println!("  * pricing unavailable for {} session(s) — cost is understated", combined.unknown_cost_sessions);
        println!();
    }
}

pub fn print_single(label: &str, usage: &TokenUsage) {
    let col_w = 15usize;
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
    row("Input tokens", &fmt_num(usage.input_tokens));
    println!(
        "  {:<lw$} {:>cw$} ({})",
        "  ↳ cached",
        fmt_num(usage.cached_input_tokens),
        fmt_pct(usage.cached_input_tokens, usage.input_tokens),
        lw = label_w,
        cw = col_w
    );
    row("  ↳ net (non-cached)", &fmt_num(usage.net_input_tokens()));
    row("Output tokens", &fmt_num(usage.output_tokens));
    println!("  {}", "─".repeat(total_w));
    row("Total tokens", &fmt_num(usage.total_tokens()));
    println!("  {}", "─".repeat(total_w));
    row("Estimated cost (USD)", &fmt_cost(usage));
    println!();

    if usage.has_unknown_cost() {
        println!("  * pricing unavailable for {} session(s) — cost is understated", usage.unknown_cost_sessions);
        println!();
    }
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
        let u = TokenUsage { sessions: 1, cost_usd: 12.345, ..Default::default() };
        assert_eq!(fmt_cost(&u), "$12.35");
    }

    #[test]
    fn fmt_cost_all_unknown() {
        let u = TokenUsage { sessions: 2, unknown_cost_sessions: 2, ..Default::default() };
        assert_eq!(fmt_cost(&u), "unknown");
    }

    #[test]
    fn fmt_cost_partial_unknown() {
        let u = TokenUsage { sessions: 3, cost_usd: 5.0, unknown_cost_sessions: 1, ..Default::default() };
        assert_eq!(fmt_cost(&u), "$5.00*");
    }

    #[test]
    fn fmt_cost_no_sessions() {
        let u = TokenUsage::default();
        assert_eq!(fmt_cost(&u), "—");
    }
}
