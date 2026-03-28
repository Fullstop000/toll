use std::collections::BTreeMap;

use crate::usage::{DailyUsage, TokenUsage, add_daily_usage};

#[derive(Clone, Debug, Default)]
pub struct SessionUsage {
    pub totals: TokenUsage,
    pub by_day: DailyUsage,
}

pub type AgentSnapshot = BTreeMap<String, SessionUsage>;

#[derive(Debug, Default)]
pub struct SnapshotDelta {
    pub total: TokenUsage,
    pub by_day: DailyUsage,
}

pub fn diff_snapshot(baseline: &AgentSnapshot, current: &AgentSnapshot) -> SnapshotDelta {
    let mut delta = SnapshotDelta::default();

    for (session_id, session_usage) in current {
        let baseline_usage = baseline.get(session_id).cloned().unwrap_or_default();
        let total_delta = session_usage.totals.saturating_sub(&baseline_usage.totals);
        delta.total.add(&total_delta);

        for (date, usage) in &session_usage.by_day {
            let baseline_day = baseline_usage.by_day.get(date).cloned().unwrap_or_default();
            let usage_delta = usage.saturating_sub(&baseline_day);
            if usage_delta.total_tokens() > 0
                || usage_delta.sessions > 0
                || usage_delta.user_queries > 0
                || usage_delta.cost_usd > 0.0
                || usage_delta.unknown_cost_sessions > 0
            {
                add_daily_usage(&mut delta.by_day, *date, &usage_delta);
            }
        }
    }

    delta
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn usage(
        input_tokens: u64,
        cached_input_tokens: u64,
        cache_write_tokens: u64,
        output_tokens: u64,
        sessions: u32,
        user_queries: u32,
    ) -> SessionUsage {
        SessionUsage {
            totals: TokenUsage {
                input_tokens,
                cached_input_tokens,
                cache_write_tokens,
                output_tokens,
                sessions,
                user_queries,
                ..Default::default()
            },
            by_day: DailyUsage::from([(
                NaiveDate::from_ymd_opt(2026, 3, 28).expect("valid date"),
                TokenUsage {
                    input_tokens,
                    cached_input_tokens,
                    cache_write_tokens,
                    output_tokens,
                    sessions,
                    user_queries,
                    ..Default::default()
                },
            )]),
        }
    }

    fn snapshot(entries: [(&str, SessionUsage); 1]) -> AgentSnapshot {
        entries
            .into_iter()
            .map(|(session_id, usage)| (session_id.to_string(), usage))
            .collect()
    }

    #[test]
    fn diff_existing_session_uses_incremental_growth() {
        let baseline = snapshot([("session-a", usage(100, 40, 0, 10, 1, 2))]);
        let current = snapshot([("session-a", usage(160, 70, 0, 16, 1, 3))]);

        let delta = diff_snapshot(&baseline, &current);

        assert_eq!(delta.total.input_tokens, 60);
        assert_eq!(delta.total.cached_input_tokens, 30);
        assert_eq!(delta.total.output_tokens, 6);
        assert_eq!(delta.total.user_queries, 1);
    }

    #[test]
    fn diff_new_session_takes_full_current_usage() {
        let baseline = AgentSnapshot::default();
        let current = snapshot([("session-b", usage(80, 20, 0, 8, 1, 1))]);

        let delta = diff_snapshot(&baseline, &current);

        assert_eq!(delta.total.total_tokens(), 88);
    }

    #[test]
    fn diff_regression_clamps_to_zero() {
        let baseline = snapshot([("session-a", usage(100, 40, 0, 10, 1, 2))]);
        let current = snapshot([("session-a", usage(50, 10, 0, 5, 1, 1))]);

        let delta = diff_snapshot(&baseline, &current);

        assert_eq!(delta.total.total_tokens(), 0);
        assert_eq!(delta.total.user_queries, 0);
    }

    #[test]
    fn diff_snapshot_tracks_daily_deltas() {
        let baseline = snapshot([("session-a", usage(100, 40, 0, 10, 1, 2))]);
        let current = snapshot([("session-a", usage(160, 70, 0, 16, 1, 3))]);

        let delta = diff_snapshot(&baseline, &current);
        let date = NaiveDate::from_ymd_opt(2026, 3, 28).expect("valid date");

        assert_eq!(delta.by_day[&date].input_tokens, 60);
        assert_eq!(delta.by_day[&date].user_queries, 1);
    }
}
