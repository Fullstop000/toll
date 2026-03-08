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

pub fn print_table(claude: &TokenUsage, codex: &TokenUsage) {
    let combined = TokenUsage {
        input_tokens: claude.input_tokens + codex.input_tokens,
        cached_input_tokens: claude.cached_input_tokens + codex.cached_input_tokens,
        output_tokens: claude.output_tokens + codex.output_tokens,
        sessions: claude.sessions + codex.sessions,
    };

    let col_w = 15usize;
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
        &fmt_num(claude.input_tokens),
        &fmt_num(codex.input_tokens),
        &fmt_num(combined.input_tokens),
    );
    row(
        "  ↳ cached",
        &format!(
            "{} ({})",
            fmt_num(claude.cached_input_tokens),
            fmt_pct(claude.cached_input_tokens, claude.input_tokens)
        ),
        &format!(
            "{} ({})",
            fmt_num(codex.cached_input_tokens),
            fmt_pct(codex.cached_input_tokens, codex.input_tokens)
        ),
        &format!(
            "{} ({})",
            fmt_num(combined.cached_input_tokens),
            fmt_pct(combined.cached_input_tokens, combined.input_tokens)
        ),
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
    println!();
}

pub fn print_single(label: &str, usage: &TokenUsage) {
    let col_w = 15usize;
    let label_w = 28usize;
    let total_w = label_w + 2 + col_w;
    println!();
    println!("  {:<lw$} {:>cw$}", "", label, lw = label_w, cw = col_w);
    println!("  {}", "═".repeat(total_w));
    println!(
        "  {:<lw$} {:>cw$}",
        "Sessions",
        fmt_num(usage.sessions as u64),
        lw = label_w,
        cw = col_w
    );
    println!("  {}", "─".repeat(total_w));
    println!(
        "  {:<lw$} {:>cw$}",
        "Input tokens",
        fmt_num(usage.input_tokens),
        lw = label_w,
        cw = col_w
    );
    println!(
        "  {:<lw$} {:>cw$} ({})",
        "  ↳ cached",
        fmt_num(usage.cached_input_tokens),
        fmt_pct(usage.cached_input_tokens, usage.input_tokens),
        lw = label_w,
        cw = col_w
    );
    println!(
        "  {:<lw$} {:>cw$}",
        "  ↳ net (non-cached)",
        fmt_num(usage.net_input_tokens()),
        lw = label_w,
        cw = col_w
    );
    println!(
        "  {:<lw$} {:>cw$}",
        "Output tokens",
        fmt_num(usage.output_tokens),
        lw = label_w,
        cw = col_w
    );
    println!("  {}", "─".repeat(total_w));
    println!(
        "  {:<lw$} {:>cw$}",
        "Total tokens",
        fmt_num(usage.total_tokens()),
        lw = label_w,
        cw = col_w
    );
    println!();
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
