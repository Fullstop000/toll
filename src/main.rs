mod agent;
mod claude;
mod codex;
mod display;
mod pricing;
mod usage;

use agent::Agent;
use chrono::{DateTime, Local, TimeZone, Utc};
use clap::Parser;
use std::path::PathBuf;

use claude::ClaudeAgent;
use codex::CodexAgent;
use display::{NumberFormat, print_daily_table, print_single, print_table};
use pricing::list_prices;
use usage::{DailyUsage, TokenUsage, add_daily_usage};

#[derive(Parser)]
#[command(
    name = "toll",
    about = "Token usage statistics for Claude Code and Codex CLI"
)]
#[command(after_help = "Examples:
  toll              # all-time stats
  toll --today      # today only
  toll --days 7     # last 7 days
  toll --by-day --days 7  # daily summary table
  toll --claude     # Claude only
  toll --codex      # Codex only
  toll --detail     # full token counts")]
struct Args {
    #[arg(
        short = 'v',
        long = "version",
        help = "Show version information and exit"
    )]
    version: bool,

    #[arg(long, conflicts_with = "days", help = "Show today's usage only")]
    today: bool,

    #[arg(long, value_name = "N", help = "Show last N days")]
    days: Option<u32>,

    #[arg(long, conflicts_with = "codex", help = "Show Claude stats only")]
    claude: bool,

    #[arg(long, conflicts_with = "claude", help = "Show Codex stats only")]
    codex: bool,

    #[arg(long, help = "List all supported models and their prices, then exit")]
    list_prices: bool,

    #[arg(long, help = "Show full token counts instead of compact b/m/k units")]
    detail: bool,

    #[arg(long, help = "Show usage aggregated by day")]
    by_day: bool,
}

fn home_dir() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/root"))
}

fn version_text() -> String {
    format!("toll {}", env!("CARGO_PKG_VERSION"))
}

/// Collect usage for a single agent via the shared abstraction.
fn collect_usage_for_agent(
    agent: &dyn Agent,
    home: &std::path::Path,
    since: Option<DateTime<Utc>>,
) -> TokenUsage {
    let data_dir = agent.data_dir(home);
    agent.collect_usage(&data_dir, since)
}

/// Collect daily usage for a single agent via the shared abstraction.
fn collect_daily_usage_for_agent(
    agent: &dyn Agent,
    home: &std::path::Path,
    since: Option<DateTime<Utc>>,
) -> usage::DailyUsageReport {
    let data_dir = agent.data_dir(home);
    agent.collect_daily_usage(&data_dir, since)
}

/// Collected usage paired with the agent display name.
struct AgentUsage<'a> {
    name: &'a str,
    usage: TokenUsage,
}

/// Select enabled agents in display order based on CLI filters.
fn selected_agents<'a>(
    show_claude: bool,
    show_codex: bool,
    claude: &'a ClaudeAgent,
    codex: &'a CodexAgent,
) -> Vec<&'a dyn Agent> {
    let mut agents: Vec<&dyn Agent> = Vec::new();
    if show_claude {
        agents.push(claude);
    }
    if show_codex {
        agents.push(codex);
    }
    agents
}

/// Collect aggregate usage for all enabled agents.
fn collect_selected_usage<'a>(
    agents: &[&'a dyn Agent],
    home: &std::path::Path,
    since: Option<DateTime<Utc>>,
) -> Vec<AgentUsage<'a>> {
    agents
        .iter()
        .map(|agent| AgentUsage {
            name: agent.name(),
            usage: collect_usage_for_agent(*agent, home, since),
        })
        .collect()
}

/// Collect and merge daily usage for all enabled agents.
fn collect_selected_daily_usage(
    agents: &[&dyn Agent],
    home: &std::path::Path,
    since: Option<DateTime<Utc>>,
) -> (DailyUsage, u32) {
    let mut by_day = DailyUsage::default();
    let mut sessions_total = 0u32;

    for agent in agents {
        let report = collect_daily_usage_for_agent(*agent, home, since);
        sessions_total += report.sessions_scanned;
        for (date, usage) in report.by_day {
            add_daily_usage(&mut by_day, date, &usage);
        }
    }

    (by_day, sessions_total)
}

fn main() {
    let args = Args::parse();

    if args.version {
        println!("{}", version_text());
        return;
    }

    if args.list_prices {
        list_prices();
        return;
    }

    let now = Utc::now();
    let since: Option<DateTime<Utc>>;
    let period: String;

    if args.today {
        let local_today = Local::now().date_naive().and_hms_opt(0, 0, 0).unwrap();
        since = Some(
            Local
                .from_local_datetime(&local_today)
                .unwrap()
                .with_timezone(&Utc),
        );
        period = "today".to_string();
    } else if let Some(days) = args.days {
        since = Some(now - chrono::Duration::days(days as i64));
        period = format!("last {} days", days);
    } else {
        since = None;
        period = "all time".to_string();
    }

    if !args.by_day {
        println!("\nToken usage — {}", period);
    }
    println!(
        "{}Collected: {}",
        if args.by_day { "\n" } else { "" },
        Local::now().format("%Y-%m-%d %H:%M:%S %Z")
    );
    let number_format = if args.detail {
        NumberFormat::Full
    } else {
        NumberFormat::Compact
    };

    let show_claude = !args.codex;
    let show_codex = !args.claude;

    let home = home_dir();
    let claude_agent = ClaudeAgent::new();
    let codex_agent = CodexAgent::new();
    let agents = selected_agents(show_claude, show_codex, &claude_agent, &codex_agent);

    let t0 = std::time::Instant::now();

    if args.by_day {
        let (by_day, sessions_total) = collect_selected_daily_usage(&agents, &home, since);
        let elapsed = t0.elapsed();
        print_daily_table(&period, &by_day, number_format);
        println!(
            "  Scanned {} session(s) in {:.2}s",
            sessions_total,
            elapsed.as_secs_f64()
        );
        println!();
    } else {
        let usages = collect_selected_usage(&agents, &home, since);
        let elapsed = t0.elapsed();
        match usages.as_slice() {
            [single] => {
                print_single(single.name, &single.usage, number_format);
                println!(
                    "  Scanned {} session(s) in {:.2}s",
                    single.usage.sessions,
                    elapsed.as_secs_f64()
                );
                println!();
            }
            [claude, codex] => {
                print_table(&claude.usage, &codex.usage, number_format);
                let sessions_total = claude.usage.sessions + codex.usage.sessions;
                println!(
                    "  Scanned {} session(s) in {:.2}s",
                    sessions_total,
                    elapsed.as_secs_f64()
                );
                println!();
            }
            _ => unreachable!("clap should ensure at least one agent is enabled"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn parses_detail_flag() {
        let args = Args::try_parse_from(["toll", "--detail"]).expect("should parse");
        assert!(args.detail);
    }

    #[test]
    fn parses_long_version_flag() {
        let args = Args::try_parse_from(["toll", "--version"]).expect("should parse");
        assert!(args.version);
    }

    #[test]
    fn parses_short_version_flag() {
        let args = Args::try_parse_from(["toll", "-v"]).expect("should parse");
        assert!(args.version);
    }

    #[test]
    fn formats_version_output() {
        assert_eq!(
            version_text(),
            format!("toll {}", env!("CARGO_PKG_VERSION"))
        );
    }

    #[test]
    fn parses_by_day_flag() {
        let args = Args::try_parse_from(["toll", "--by-day"]).expect("should parse");
        assert!(args.by_day);
    }

    #[test]
    fn agents_expose_distinct_names_and_data_dirs() {
        let claude = claude::ClaudeAgent::new();
        let codex = codex::CodexAgent::new();
        let agents: [&dyn agent::Agent; 2] = [&claude, &codex];

        assert_eq!(agents[0].name(), "Claude Code");
        assert_eq!(agents[1].name(), "Codex");
        assert_eq!(agents[0].data_dir(Path::new("/tmp")), PathBuf::from("/tmp/.claude/projects"));
        assert_eq!(agents[1].data_dir(Path::new("/tmp")), PathBuf::from("/tmp/.codex/sessions"));
    }

    #[test]
    fn selected_agents_follow_cli_filters() {
        let claude = claude::ClaudeAgent::new();
        let codex = codex::CodexAgent::new();

        let claude_only = selected_agents(true, false, &claude, &codex);
        assert_eq!(claude_only.len(), 1);
        assert_eq!(claude_only[0].name(), "Claude Code");

        let codex_only = selected_agents(false, true, &claude, &codex);
        assert_eq!(codex_only.len(), 1);
        assert_eq!(codex_only[0].name(), "Codex");

        let both = selected_agents(true, true, &claude, &codex);
        assert_eq!(both.len(), 2);
        assert_eq!(both[0].name(), "Claude Code");
        assert_eq!(both[1].name(), "Codex");
    }
}
