#[derive(Default, Debug)]
pub struct TokenUsage {
    pub input_tokens: u64,
    pub cached_input_tokens: u64,
    pub output_tokens: u64,
    pub sessions: u32,
}

impl TokenUsage {
    pub fn add(&mut self, other: &TokenUsage) {
        self.input_tokens += other.input_tokens;
        self.cached_input_tokens += other.cached_input_tokens;
        self.output_tokens += other.output_tokens;
        self.sessions += other.sessions;
    }

    pub fn total_tokens(&self) -> u64 {
        self.input_tokens + self.output_tokens
    }

    pub fn net_input_tokens(&self) -> u64 {
        self.input_tokens.saturating_sub(self.cached_input_tokens)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add() {
        let mut a = TokenUsage { input_tokens: 100, cached_input_tokens: 80, output_tokens: 20, sessions: 1 };
        let b = TokenUsage { input_tokens: 50, cached_input_tokens: 10, output_tokens: 5, sessions: 1 };
        a.add(&b);
        assert_eq!(a.input_tokens, 150);
        assert_eq!(a.cached_input_tokens, 90);
        assert_eq!(a.output_tokens, 25);
        assert_eq!(a.sessions, 2);
    }

    #[test]
    fn total_and_net() {
        let u = TokenUsage { input_tokens: 1000, cached_input_tokens: 800, output_tokens: 200, sessions: 1 };
        assert_eq!(u.total_tokens(), 1200);
        assert_eq!(u.net_input_tokens(), 200);
    }

    #[test]
    fn net_no_underflow() {
        let u = TokenUsage { input_tokens: 10, cached_input_tokens: 20, output_tokens: 0, sessions: 1 };
        assert_eq!(u.net_input_tokens(), 0);
    }
}
