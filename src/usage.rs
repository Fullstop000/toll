#[derive(Default, Debug)]
pub struct TokenUsage {
    /// Total input tokens sent (= pure_input + cache_write + cache_read)
    pub input_tokens: u64,
    /// Cache read tokens only (served from cache, discounted)
    pub cached_input_tokens: u64,
    /// Cache write tokens (Claude only; tracked separately for cost calculation)
    pub cache_write_tokens: u64,
    pub output_tokens: u64,
    pub sessions: u32,
    /// Estimated cost in USD (0.0 if unknown)
    pub cost_usd: f64,
    /// Sessions whose model pricing was not found
    pub unknown_cost_sessions: u32,
}

impl TokenUsage {
    pub fn add(&mut self, other: &TokenUsage) {
        self.input_tokens += other.input_tokens;
        self.cached_input_tokens += other.cached_input_tokens;
        self.cache_write_tokens += other.cache_write_tokens;
        self.output_tokens += other.output_tokens;
        self.sessions += other.sessions;
        self.cost_usd += other.cost_usd;
        self.unknown_cost_sessions += other.unknown_cost_sessions;
    }

    pub fn total_tokens(&self) -> u64 {
        self.input_tokens + self.output_tokens
    }

    /// Non-cached input (= pure_input + cache_write); what you pay full/write price for.
    pub fn net_input_tokens(&self) -> u64 {
        self.input_tokens.saturating_sub(self.cached_input_tokens)
    }

    pub fn has_unknown_cost(&self) -> bool {
        self.unknown_cost_sessions > 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add() {
        let mut a = TokenUsage {
            input_tokens: 100,
            cached_input_tokens: 80,
            cache_write_tokens: 10,
            output_tokens: 20,
            sessions: 1,
            cost_usd: 1.5,
            unknown_cost_sessions: 0,
        };
        let b = TokenUsage {
            input_tokens: 50,
            cached_input_tokens: 10,
            cache_write_tokens: 5,
            output_tokens: 5,
            sessions: 1,
            cost_usd: 0.5,
            unknown_cost_sessions: 1,
        };
        a.add(&b);
        assert_eq!(a.input_tokens, 150);
        assert_eq!(a.cached_input_tokens, 90);
        assert_eq!(a.cache_write_tokens, 15);
        assert_eq!(a.output_tokens, 25);
        assert_eq!(a.sessions, 2);
        assert!((a.cost_usd - 2.0).abs() < 1e-9);
        assert_eq!(a.unknown_cost_sessions, 1);
    }

    #[test]
    fn total_and_net() {
        let u = TokenUsage {
            input_tokens: 1000,
            cached_input_tokens: 800,
            output_tokens: 200,
            ..Default::default()
        };
        assert_eq!(u.total_tokens(), 1200);
        assert_eq!(u.net_input_tokens(), 200);
    }

    #[test]
    fn net_no_underflow() {
        let u = TokenUsage {
            input_tokens: 10,
            cached_input_tokens: 20,
            ..Default::default()
        };
        assert_eq!(u.net_input_tokens(), 0);
    }

    #[test]
    fn has_unknown_cost() {
        let mut u = TokenUsage::default();
        assert!(!u.has_unknown_cost());
        u.unknown_cost_sessions = 1;
        assert!(u.has_unknown_cost());
    }
}
