use chrono::{DateTime, Utc};
use std::path::{Path, PathBuf};

use crate::usage::{DailyUsageReport, TokenUsage};
use crate::watch::AgentSnapshot;

/// Shared behavior for a usage source such as Claude Code or Codex.
pub trait Agent {
    /// Human-readable agent name used in CLI output.
    fn name(&self) -> &'static str;

    /// Resolve the agent-specific data directory from the provided home path.
    fn data_dir(&self, home: &Path) -> PathBuf;

    /// Collect aggregate usage for the agent.
    fn collect_usage(&self, data_dir: &Path, since: Option<DateTime<Utc>>) -> TokenUsage;

    /// Collect daily usage for the agent.
    fn collect_daily_usage(
        &self,
        data_dir: &Path,
        since: Option<DateTime<Utc>>,
    ) -> DailyUsageReport;

    /// Collect usage snapshots keyed by logical session identifier.
    fn collect_snapshot(&self, data_dir: &Path, since: Option<DateTime<Utc>>) -> AgentSnapshot;
}
