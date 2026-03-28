use std::collections::BTreeMap;

use chrono::NaiveDate;

#[derive(Clone, Default, Debug)]
pub struct TokenUsage {
    /// Total input tokens sent (= pure_input + cache_write + cache_read)
    pub input_tokens: u64,
    /// Cache read tokens only (served from cache, discounted)
    pub cached_input_tokens: u64,
    /// Cache write tokens (Claude only; tracked separately for cost calculation)
    pub cache_write_tokens: u64,
    pub output_tokens: u64,
    pub sessions: u32,
    pub user_queries: u32,
    /// Estimated cost in USD (0.0 if unknown)
    pub cost_usd: f64,
    /// Sessions whose model pricing was not found
    pub unknown_cost_sessions: u32,
    pub processing_time_ms: u64,
    /// Per-model breakdown (model name → usage); inner entries keep by_model empty.
    pub by_model: BTreeMap<String, TokenUsage>,
}

/// Aggregated token usage keyed by local calendar date.
pub type DailyUsage = BTreeMap<NaiveDate, TokenUsage>;

/// Result of a daily aggregation pass plus the number of scanned sessions.
#[derive(Default, Debug)]
pub struct DailyUsageReport {
    pub by_day: DailyUsage,
    pub sessions_scanned: u32,
}

impl TokenUsage {
    pub fn add(&mut self, other: &TokenUsage) {
        self.input_tokens += other.input_tokens;
        self.cached_input_tokens += other.cached_input_tokens;
        self.cache_write_tokens += other.cache_write_tokens;
        self.output_tokens += other.output_tokens;
        self.sessions += other.sessions;
        self.user_queries += other.user_queries;
        self.cost_usd += other.cost_usd;
        self.unknown_cost_sessions += other.unknown_cost_sessions;
        self.processing_time_ms += other.processing_time_ms;
        for (model, usage) in &other.by_model {
            self.by_model.entry(model.clone()).or_default().add(usage);
        }
    }

    /// Merge a single-model observation into by_model, without recursing.
    pub fn record_model(
        &mut self,
        model: &str,
        inp: u64,
        cache_write: u64,
        cache_read: u64,
        out: u64,
        cost: f64,
        processing_time_ms: u64,
    ) {
        let e = self.by_model.entry(model.to_string()).or_default();
        e.input_tokens += inp + cache_write + cache_read;
        e.cached_input_tokens += cache_read;
        e.cache_write_tokens += cache_write;
        e.output_tokens += out;
        e.sessions += 1;
        e.cost_usd += cost;
        e.processing_time_ms += processing_time_ms;
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

    /// Tokens per second = output_tokens / processing_time_ms * 1000
    /// Returns None if processing_time_ms is 0.
    #[allow(dead_code)]
    pub fn tps(&self) -> Option<f64> {
        if self.processing_time_ms == 0 {
            return None;
        }
        Some(self.output_tokens as f64 / self.processing_time_ms as f64 * 1000.0)
    }

    pub fn saturating_sub(&self, baseline: &TokenUsage) -> TokenUsage {
        let mut by_model = BTreeMap::new();

        for (model, usage) in &self.by_model {
            let baseline_usage = baseline.by_model.get(model).cloned().unwrap_or_default();
            let delta = usage.saturating_sub(&baseline_usage);
            if delta.total_tokens() > 0
                || delta.sessions > 0
                || delta.user_queries > 0
                || delta.cost_usd > 0.0
                || delta.unknown_cost_sessions > 0
            {
                by_model.insert(model.clone(), delta);
            }
        }

        TokenUsage {
            input_tokens: self.input_tokens.saturating_sub(baseline.input_tokens),
            cached_input_tokens: self
                .cached_input_tokens
                .saturating_sub(baseline.cached_input_tokens),
            cache_write_tokens: self
                .cache_write_tokens
                .saturating_sub(baseline.cache_write_tokens),
            output_tokens: self.output_tokens.saturating_sub(baseline.output_tokens),
            sessions: self.sessions.saturating_sub(baseline.sessions),
            user_queries: self.user_queries.saturating_sub(baseline.user_queries),
            cost_usd: (self.cost_usd - baseline.cost_usd).max(0.0),
            unknown_cost_sessions: self
                .unknown_cost_sessions
                .saturating_sub(baseline.unknown_cost_sessions),
            processing_time_ms: self
                .processing_time_ms
                .saturating_sub(baseline.processing_time_ms),
            by_model,
        }
    }
}

/// Merge a usage snapshot into the given local-date bucket.
pub fn add_daily_usage(by_day: &mut DailyUsage, date: NaiveDate, usage: &TokenUsage) {
    by_day.entry(date).or_default().add(usage);
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    #[test]
    fn add_scalars() {
        let mut a = TokenUsage {
            input_tokens: 100,
            cached_input_tokens: 80,
            cache_write_tokens: 10,
            output_tokens: 20,
            sessions: 1,
            user_queries: 2,
            cost_usd: 1.5,
            unknown_cost_sessions: 0,
            ..Default::default()
        };
        let b = TokenUsage {
            input_tokens: 50,
            cached_input_tokens: 10,
            cache_write_tokens: 5,
            output_tokens: 5,
            sessions: 1,
            user_queries: 1,
            cost_usd: 0.5,
            unknown_cost_sessions: 1,
            ..Default::default()
        };
        a.add(&b);
        assert_eq!(a.input_tokens, 150);
        assert_eq!(a.cached_input_tokens, 90);
        assert_eq!(a.cache_write_tokens, 15);
        assert_eq!(a.output_tokens, 25);
        assert_eq!(a.sessions, 2);
        assert_eq!(a.user_queries, 3);
        assert!((a.cost_usd - 2.0).abs() < 1e-9);
        assert_eq!(a.unknown_cost_sessions, 1);
    }

    #[test]
    fn add_merges_by_model() {
        let mut a = TokenUsage::default();
        a.record_model("model-a", 100, 0, 0, 10, 1.0, 0);

        let mut b = TokenUsage::default();
        b.record_model("model-a", 200, 0, 0, 20, 2.0, 0);
        b.record_model("model-b", 50, 0, 0, 5, 0.5, 0);

        a.add(&b);

        assert_eq!(a.by_model.len(), 2);
        assert_eq!(a.by_model["model-a"].input_tokens, 300);
        assert_eq!(a.by_model["model-a"].sessions, 2);
        assert!((a.by_model["model-a"].cost_usd - 3.0).abs() < 1e-9);
        assert_eq!(a.by_model["model-b"].input_tokens, 50);
    }

    #[test]
    fn record_model_accumulates() {
        let mut u = TokenUsage::default();
        u.record_model("gpt-5", 100, 0, 50, 10, 1.0, 1000);
        u.record_model("gpt-5", 200, 0, 80, 20, 2.0, 2000);
        let m = &u.by_model["gpt-5"];
        assert_eq!(m.input_tokens, 430); // (100+0+50)+(200+0+80)
        assert_eq!(m.cached_input_tokens, 130);
        assert_eq!(m.sessions, 2);
        assert_eq!(m.processing_time_ms, 3000);
        assert!((m.cost_usd - 3.0).abs() < 1e-9);
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

    #[test]
    fn saturating_sub_subtracts_scalars() {
        let current = TokenUsage {
            input_tokens: 120,
            cached_input_tokens: 90,
            cache_write_tokens: 10,
            output_tokens: 20,
            sessions: 3,
            user_queries: 4,
            cost_usd: 7.0,
            unknown_cost_sessions: 1,
            ..Default::default()
        };
        let baseline = TokenUsage {
            input_tokens: 100,
            cached_input_tokens: 70,
            cache_write_tokens: 5,
            output_tokens: 12,
            sessions: 1,
            user_queries: 2,
            cost_usd: 2.5,
            ..Default::default()
        };

        let delta = current.saturating_sub(&baseline);

        assert_eq!(delta.input_tokens, 20);
        assert_eq!(delta.cached_input_tokens, 20);
        assert_eq!(delta.cache_write_tokens, 5);
        assert_eq!(delta.output_tokens, 8);
        assert_eq!(delta.sessions, 2);
        assert_eq!(delta.user_queries, 2);
        assert!((delta.cost_usd - 4.5).abs() < 1e-9);
        assert_eq!(delta.unknown_cost_sessions, 1);
    }

    #[test]
    fn saturating_sub_clamps_models() {
        let mut current = TokenUsage::default();
        current.record_model("gpt-5", 100, 0, 40, 20, 3.0, 0);

        let mut baseline = TokenUsage::default();
        baseline.record_model("gpt-5", 150, 0, 60, 25, 4.0, 0);

        let delta = current.saturating_sub(&baseline);

        assert!(!delta.by_model.contains_key("gpt-5"));
    }

    #[test]
    fn add_daily_usage_merges_same_day() {
        let mut by_day = DailyUsage::default();
        let date = NaiveDate::from_ymd_opt(2026, 3, 12).expect("valid date");

        add_daily_usage(
            &mut by_day,
            date,
            &TokenUsage {
                input_tokens: 100,
                sessions: 1,
                ..Default::default()
            },
        );
        add_daily_usage(
            &mut by_day,
            date,
            &TokenUsage {
                output_tokens: 25,
                sessions: 1,
                ..Default::default()
            },
        );

        assert_eq!(by_day.len(), 1);
        assert_eq!(by_day[&date].input_tokens, 100);
        assert_eq!(by_day[&date].output_tokens, 25);
        assert_eq!(by_day[&date].sessions, 2);
    }

    #[test]
    fn tps_returns_tokens_per_second() {
        let usage = TokenUsage {
            input_tokens: 1000,
            output_tokens: 500,
            processing_time_ms: 1000,
            ..Default::default()
        };
        assert!((usage.tps().unwrap() - 1500.0).abs() < 1e-6);
    }

    #[test]
    fn tps_returns_none_when_no_processing_time() {
        let usage = TokenUsage {
            input_tokens: 1000,
            output_tokens: 500,
            processing_time_ms: 0,
            ..Default::default()
        };
        assert!(usage.tps().is_none());
    }
}
