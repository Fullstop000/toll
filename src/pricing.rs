/// Pricing per million tokens for a model.
pub struct ModelPricing {
    /// Non-cached, non-write input tokens (standard input rate)
    pub input_per_m: f64,
    /// Cache write tokens (Claude only; 0.0 → not applicable, use input_per_m)
    pub cache_write_per_m: f64,
    /// Cache read tokens (discounted rate)
    pub cache_read_per_m: f64,
    /// Output tokens
    pub output_per_m: f64,
}

impl ModelPricing {
    /// Compute USD cost from raw token counts.
    ///
    /// For Claude: pure_input = inp (excluding cache_write and cache_read).
    /// For OpenAI: pure_input = input - cached, cache_write = 0.
    pub fn cost(&self, pure_input: u64, cache_write: u64, cache_read: u64, output: u64) -> f64 {
        let cw = if self.cache_write_per_m > 0.0 {
            self.cache_write_per_m
        } else {
            self.input_per_m
        };
        (pure_input as f64 * self.input_per_m
            + cache_write as f64 * cw
            + cache_read as f64 * self.cache_read_per_m
            + output as f64 * self.output_per_m)
            / 1_000_000.0
    }
}

/// Pricing table — ordered most-specific first so prefix matching works correctly
/// (e.g. "gpt-4o-mini" must appear before "gpt-4o").
///
/// Sources (March 2026):
///   Anthropic: https://www.anthropic.com/pricing
///   OpenAI:    https://openai.com/api/pricing/
static PRICING: &[(&str, ModelPricing)] = &[
    // ── Anthropic Claude ──────────────────────────────────────────────────────
    // Opus 4.6 / 4.5  $5 in / $25 out  cache_write $6.25  cache_read $0.50
    (
        "claude-opus-4-6",
        ModelPricing {
            input_per_m: 5.0,
            cache_write_per_m: 6.25,
            cache_read_per_m: 0.50,
            output_per_m: 25.0,
        },
    ),
    (
        "claude-opus-4-5",
        ModelPricing {
            input_per_m: 5.0,
            cache_write_per_m: 6.25,
            cache_read_per_m: 0.50,
            output_per_m: 25.0,
        },
    ),
    // Opus 4.1 / 4  $15 in / $75 out  cache_write $18.75  cache_read $1.50
    (
        "claude-opus-4-1",
        ModelPricing {
            input_per_m: 15.0,
            cache_write_per_m: 18.75,
            cache_read_per_m: 1.50,
            output_per_m: 75.0,
        },
    ),
    (
        "claude-opus-4",
        ModelPricing {
            input_per_m: 15.0,
            cache_write_per_m: 18.75,
            cache_read_per_m: 1.50,
            output_per_m: 75.0,
        },
    ),
    // Opus 3  $15 in / $75 out
    (
        "claude-opus-3",
        ModelPricing {
            input_per_m: 15.0,
            cache_write_per_m: 18.75,
            cache_read_per_m: 1.50,
            output_per_m: 75.0,
        },
    ),
    // Sonnet 4.x / 3.7  $3 in / $15 out  cache_write $3.75  cache_read $0.30
    (
        "claude-sonnet-4",
        ModelPricing {
            input_per_m: 3.0,
            cache_write_per_m: 3.75,
            cache_read_per_m: 0.30,
            output_per_m: 15.0,
        },
    ),
    (
        "claude-sonnet-3",
        ModelPricing {
            input_per_m: 3.0,
            cache_write_per_m: 3.75,
            cache_read_per_m: 0.30,
            output_per_m: 15.0,
        },
    ),
    // Haiku 4.5  $1 in / $5 out  cache_write $1.25  cache_read $0.10
    (
        "claude-haiku-4",
        ModelPricing {
            input_per_m: 1.0,
            cache_write_per_m: 1.25,
            cache_read_per_m: 0.10,
            output_per_m: 5.0,
        },
    ),
    // Haiku 3.5  $0.80 in / $4 out  cache_write $1.00  cache_read $0.08
    (
        "claude-haiku-3-5",
        ModelPricing {
            input_per_m: 0.80,
            cache_write_per_m: 1.0,
            cache_read_per_m: 0.08,
            output_per_m: 4.0,
        },
    ),
    // Haiku 3  $0.25 in / $1.25 out  cache_write $0.30  cache_read $0.03
    (
        "claude-haiku-3",
        ModelPricing {
            input_per_m: 0.25,
            cache_write_per_m: 0.30,
            cache_read_per_m: 0.03,
            output_per_m: 1.25,
        },
    ),
    // ── OpenAI (cache_write_per_m = 0.0; no separate write charge) ────────────
    // GPT-5.2 Pro  $21 in / $168 out  cache_read $2.10
    (
        "gpt-5.2-pro",
        ModelPricing {
            input_per_m: 21.0,
            cache_write_per_m: 0.0,
            cache_read_per_m: 2.10,
            output_per_m: 168.0,
        },
    ),
    // GPT-5.2  $1.75 in / $14 out  cache_read $0.175
    (
        "gpt-5.2",
        ModelPricing {
            input_per_m: 1.75,
            cache_write_per_m: 0.0,
            cache_read_per_m: 0.175,
            output_per_m: 14.0,
        },
    ),
    // GPT-5.1 / GPT-5  $1.25 in / $10 out  cache_read $0.125
    (
        "gpt-5.1",
        ModelPricing {
            input_per_m: 1.25,
            cache_write_per_m: 0.0,
            cache_read_per_m: 0.125,
            output_per_m: 10.0,
        },
    ),
    (
        "gpt-5",
        ModelPricing {
            input_per_m: 1.25,
            cache_write_per_m: 0.0,
            cache_read_per_m: 0.125,
            output_per_m: 10.0,
        },
    ),
    // GPT-4.1-mini  $0.40 in / $1.60 out  cache_read $0.10
    (
        "gpt-4.1-mini",
        ModelPricing {
            input_per_m: 0.40,
            cache_write_per_m: 0.0,
            cache_read_per_m: 0.10,
            output_per_m: 1.60,
        },
    ),
    // GPT-4.1  $2 in / $8 out  cache_read $0.20
    (
        "gpt-4.1",
        ModelPricing {
            input_per_m: 2.00,
            cache_write_per_m: 0.0,
            cache_read_per_m: 0.20,
            output_per_m: 8.0,
        },
    ),
    // GPT-4o-mini  $0.15 in / $0.60 out  cache_read $0.075
    (
        "gpt-4o-mini",
        ModelPricing {
            input_per_m: 0.15,
            cache_write_per_m: 0.0,
            cache_read_per_m: 0.075,
            output_per_m: 0.60,
        },
    ),
    // GPT-4o  $2.50 in / $10 out  cache_read $1.25
    (
        "gpt-4o",
        ModelPricing {
            input_per_m: 2.50,
            cache_write_per_m: 0.0,
            cache_read_per_m: 1.25,
            output_per_m: 10.0,
        },
    ),
    // o3-pro  $20 in / $80 out
    (
        "o3-pro",
        ModelPricing {
            input_per_m: 20.0,
            cache_write_per_m: 0.0,
            cache_read_per_m: 5.0,
            output_per_m: 80.0,
        },
    ),
    // o3  $2 in / $8 out  cache_read $0.50
    (
        "o3",
        ModelPricing {
            input_per_m: 2.00,
            cache_write_per_m: 0.0,
            cache_read_per_m: 0.50,
            output_per_m: 8.0,
        },
    ),
    // o4-mini  $1.10 in / $4.40 out  cache_read $0.275
    (
        "o4-mini",
        ModelPricing {
            input_per_m: 1.10,
            cache_write_per_m: 0.0,
            cache_read_per_m: 0.275,
            output_per_m: 4.40,
        },
    ),
    // o1  $15 in / $60 out  cache_read $7.50
    (
        "o1",
        ModelPricing {
            input_per_m: 15.0,
            cache_write_per_m: 0.0,
            cache_read_per_m: 7.50,
            output_per_m: 60.0,
        },
    ),
];

/// Print all known model prices as a formatted table.
pub fn list_prices() {
    let col = 14usize;
    let label_w = 28usize;
    let total_w = label_w + 2 + (col + 2) * 4;

    println!();
    println!(
        "  {:<lw$} {:>cw$}  {:>cw$}  {:>cw$}  {:>cw$}",
        "Model",
        "Input/M",
        "CacheWrite/M",
        "CacheRead/M",
        "Output/M",
        lw = label_w,
        cw = col
    );
    println!("  {}", "═".repeat(total_w));

    for (name, p) in PRICING {
        let cache_write = if p.cache_write_per_m > 0.0 {
            format!("${:.3}", p.cache_write_per_m)
        } else {
            "—".to_string()
        };
        println!(
            "  {:<lw$} {:>cw$}  {:>cw$}  {:>cw$}  {:>cw$}",
            name,
            format!("${:.3}", p.input_per_m),
            cache_write,
            format!("${:.3}", p.cache_read_per_m),
            format!("${:.3}", p.output_per_m),
            lw = label_w,
            cw = col
        );
    }
    println!("  {}", "─".repeat(total_w));
    println!("  All prices are USD per 1 million tokens.");
    println!("  CacheWrite/M: — means cache writes are billed at the standard input rate.");
    println!();
}

/// Look up pricing for a model by name.
///
/// Matching rules (in order):
/// 1. Exact match (case-insensitive)
/// 2. Prefix match — longest key wins (table is already most-specific-first)
///    e.g. "gpt-5.4" → matches "gpt-5" prefix → uses GPT-5 pricing
///    e.g. "claude-sonnet-4-6" → matches "claude-sonnet-4" prefix
pub fn lookup(model: &str) -> Option<&'static ModelPricing> {
    let m = model.to_lowercase();
    // Exact match first
    if let Some((_, p)) = PRICING.iter().find(|(k, _)| *k == m.as_str()) {
        return Some(p);
    }
    // Prefix match — PRICING is ordered most-specific first, so first hit is best
    PRICING
        .iter()
        .find(|(k, _)| m.starts_with(k))
        .map(|(_, p)| p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_match() {
        let p = lookup("claude-sonnet-4-6").expect("should find");
        assert_eq!(p.input_per_m, 3.0);
        assert_eq!(p.output_per_m, 15.0);
        assert_eq!(p.cache_write_per_m, 3.75);
        assert_eq!(p.cache_read_per_m, 0.30);
    }

    #[test]
    fn prefix_match_claude_with_snapshot_date() {
        // "claude-sonnet-4-5-20250929" should match "claude-sonnet-4" prefix
        let p = lookup("claude-sonnet-4-5-20250929").expect("should find via prefix");
        assert_eq!(p.input_per_m, 3.0);
    }

    #[test]
    fn prefix_match_gpt5_variant() {
        // "gpt-5.4" not in table → falls back to "gpt-5" prefix
        let p = lookup("gpt-5.4").expect("should find via gpt-5 prefix");
        assert_eq!(p.input_per_m, 1.25);
    }

    #[test]
    fn gpt4o_mini_before_gpt4o() {
        let mini = lookup("gpt-4o-mini").expect("should find");
        assert_eq!(mini.input_per_m, 0.15);
        let full = lookup("gpt-4o").expect("should find");
        assert_eq!(full.input_per_m, 2.50);
    }

    #[test]
    fn unknown_model_returns_none() {
        assert!(lookup("unknown-model-xyz").is_none());
    }

    #[test]
    fn cost_calculation() {
        let p = lookup("claude-sonnet-4-6").unwrap();
        // 1M pure input + 0 cache_write + 0 cache_read + 1M output
        let cost = p.cost(1_000_000, 0, 0, 1_000_000);
        assert!((cost - 18.0).abs() < 0.001); // $3 + $15 = $18
    }

    #[test]
    fn cost_with_cache() {
        let p = lookup("claude-sonnet-4-6").unwrap();
        // 0 pure input + 1M cache_write + 1M cache_read + 0 output
        let cost = p.cost(0, 1_000_000, 1_000_000, 0);
        assert!((cost - (3.75 + 0.30)).abs() < 0.001);
    }

    #[test]
    fn openai_cost_no_cache_write_charge() {
        // OpenAI: cache_write goes through at input_per_m rate (not extra charge)
        let p = lookup("gpt-4o").unwrap();
        let cost_no_write = p.cost(1_000_000, 0, 0, 0);
        let cost_with_write = p.cost(0, 1_000_000, 0, 0);
        // Both should be 2.50 (cache_write_per_m == 0 → falls back to input_per_m)
        assert!((cost_no_write - 2.50).abs() < 0.001);
        assert!((cost_with_write - 2.50).abs() < 0.001);
    }
}
