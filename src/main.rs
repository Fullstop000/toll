mod agent;
mod claude;
mod codex;
mod display;
mod gemini;
mod kimi;
mod output;
mod pricing;
mod usage;
mod watch;

use agent::Agent;
use chrono::{DateTime, Local, TimeZone, Utc};
use clap::Parser;
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

use claude::ClaudeAgent;
use codex::CodexAgent;
use display::{
    NumberFormat, print_daily_table, print_multi_table, print_single, render_daily_table,
    render_multi_table, render_single_table,
};
use gemini::GeminiAgent;
use kimi::KimiAgent;
use output::{
    OutputFilters, OutputMode, render_daily_csv, render_daily_json, render_summary_csv,
    render_summary_json,
};
use pricing::list_prices;
use signal_hook::{consts::SIGINT, flag as signal_flag};
use usage::{DailyUsage, TokenUsage, add_daily_usage};
use watch::{AgentSnapshot, diff_snapshot};

const WATCH_REFRESH_INTERVAL: Duration = Duration::from_secs(2);

#[derive(Parser)]
#[command(
    name = "toll",
    about = "Token usage statistics for Claude Code, Codex CLI, Kimi Code, and Gemini"
)]
#[command(after_help = "Examples:
  toll              # all-time stats
  toll --today      # today only
  toll --days 7     # last 7 days
  toll --by-day --days 7  # daily summary table
  toll --watch            # live delta stats until Ctrl-C
  toll --watch --by-day   # live per-day delta stats
  toll --claude     # Claude only
  toll --codex      # Codex only
  toll --kimi       # Kimi Code only
  toll --gemini     # Gemini only
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

    #[arg(
        long,
        conflicts_with = "today",
        conflicts_with = "days",
        help = "Watch usage deltas from now until interrupted"
    )]
    watch: bool,

    #[arg(long, help = "Show Claude stats only")]
    claude: bool,

    #[arg(long, help = "Show Codex stats only")]
    codex: bool,

    #[arg(long, help = "Show Kimi Code stats only")]
    kimi: bool,

    #[arg(long, help = "Show Gemini stats only")]
    gemini: bool,

    #[arg(long, help = "List all supported models and their prices, then exit")]
    list_prices: bool,

    #[arg(long, help = "Show full token counts instead of compact b/m/k units")]
    detail: bool,

    #[arg(long, help = "Show usage aggregated by day")]
    by_day: bool,

    #[arg(long, conflicts_with = "csv", help = "Emit JSON to stdout")]
    json: bool,

    #[arg(long, conflicts_with = "json", help = "Emit CSV to stdout")]
    csv: bool,
}

fn home_dir() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/root"))
}

fn version_text() -> String {
    format!("toll {}", env!("CARGO_PKG_VERSION"))
}

/// Select the output mode from mutually-exclusive CLI flags.
fn output_mode(args: &Args) -> OutputMode {
    if args.json {
        OutputMode::Json
    } else if args.csv {
        OutputMode::Csv
    } else {
        OutputMode::Table
    }
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

struct AgentSnapshotEntry<'a> {
    name: &'a str,
    snapshot: AgentSnapshot,
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

fn collect_snapshot_for_agent(
    agent: &dyn Agent,
    home: &std::path::Path,
    since: Option<DateTime<Utc>>,
) -> AgentSnapshot {
    let data_dir = agent.data_dir(home);
    agent.collect_snapshot(&data_dir, since)
}

fn collect_selected_snapshots<'a>(
    agents: &[&'a dyn Agent],
    home: &std::path::Path,
    since: Option<DateTime<Utc>>,
) -> Vec<AgentSnapshotEntry<'a>> {
    agents
        .iter()
        .map(|agent| AgentSnapshotEntry {
            name: agent.name(),
            snapshot: collect_snapshot_for_agent(*agent, home, since),
        })
        .collect()
}

fn diff_selected_snapshot_usage<'a>(
    baseline: &[AgentSnapshotEntry<'a>],
    current: &[AgentSnapshotEntry<'a>],
) -> Vec<AgentUsage<'a>> {
    baseline
        .iter()
        .zip(current.iter())
        .map(|(baseline_entry, current_entry)| AgentUsage {
            name: current_entry.name,
            usage: diff_snapshot(&baseline_entry.snapshot, &current_entry.snapshot).total,
        })
        .collect()
}

fn diff_selected_snapshot_daily_usage(
    baseline: &[AgentSnapshotEntry<'_>],
    current: &[AgentSnapshotEntry<'_>],
) -> DailyUsage {
    let mut by_day = DailyUsage::default();

    for (baseline_entry, current_entry) in baseline.iter().zip(current.iter()) {
        let delta = diff_snapshot(&baseline_entry.snapshot, &current_entry.snapshot);
        for (date, usage) in delta.by_day {
            add_daily_usage(&mut by_day, date, &usage);
        }
    }

    by_day
}

fn watch_sessions_scanned(usages: &[AgentUsage<'_>]) -> u32 {
    usages.iter().map(|entry| entry.usage.sessions).sum()
}

fn render_watch_table_frame(
    args: &Args,
    number_format: NumberFormat,
    started_at: chrono::DateTime<Local>,
    refreshed_at: chrono::DateTime<Local>,
    summary_rows: &[AgentUsage<'_>],
    by_day: &DailyUsage,
    final_frame: bool,
) -> String {
    let mut out = String::new();
    out.push_str("\x1B[2J\x1B[H");
    out.push_str("\nWatch usage — session delta\n");
    out.push_str(&format!(
        "Started: {}\nRefreshed: {}\n",
        started_at.format("%Y-%m-%d %H:%M:%S %Z"),
        refreshed_at.format("%Y-%m-%d %H:%M:%S %Z")
    ));

    if args.by_day {
        out.push_str(&render_daily_table("watch session", by_day, number_format));
    } else {
        match summary_rows {
            [single] => out.push_str(&render_single_table(single.name, &single.usage, number_format)),
            _ => {
                let display_rows: Vec<(&str, &TokenUsage)> = summary_rows
                    .iter()
                    .map(|entry| (entry.name, &entry.usage))
                    .collect();
                out.push_str(&render_multi_table(&display_rows, number_format));
            }
        }
    }

    out.push_str(&format!(
        "  Scanned {} session(s)\n",
        watch_sessions_scanned(summary_rows)
    ));
    if final_frame {
        out.push_str("  Watch stopped.\n");
    } else {
        out.push_str("  Press Ctrl-C to stop.\n");
    }

    out
}

fn run_watch(
    args: &Args,
    agents: &[&dyn Agent],
    home: &std::path::Path,
    number_format: NumberFormat,
    output_mode: OutputMode,
    filters: OutputFilters,
) {
    let started_at = Local::now();
    let started = Instant::now();
    let baseline = collect_selected_snapshots(agents, home, None);
    let interrupted = Arc::new(AtomicBool::new(false));

    signal_flag::register(SIGINT, Arc::clone(&interrupted))
        .expect("ctrl-c handler should install");

    loop {
        let current = collect_selected_snapshots(agents, home, None);
        let summary_rows = diff_selected_snapshot_usage(&baseline, &current);
        let by_day = diff_selected_snapshot_daily_usage(&baseline, &current);
        let collected_at = Local::now();

        if output_mode == OutputMode::Table {
            let frame = render_watch_table_frame(
                args,
                number_format,
                started_at,
                collected_at,
                &summary_rows,
                &by_day,
                interrupted.load(Ordering::SeqCst),
            );
            print!("{frame}");
            io::stdout().flush().expect("watch output should flush");
        }

        if interrupted.load(Ordering::SeqCst) {
            let display_rows: Vec<(&str, &TokenUsage)> = summary_rows
                .iter()
                .map(|entry| (entry.name, &entry.usage))
                .collect();

            match output_mode {
                OutputMode::Table => {
                    println!();
                }
                OutputMode::Csv => {
                    if args.by_day {
                        println!("{}", render_daily_csv(&by_day, number_format));
                    } else {
                        println!("{}", render_summary_csv(&display_rows, number_format));
                    }
                }
                OutputMode::Json => {
                    let elapsed_seconds = started.elapsed().as_secs_f64();
                    let sessions_total = watch_sessions_scanned(&summary_rows);
                    if args.by_day {
                        println!(
                            "{}",
                            render_daily_json(
                                "watch session",
                                &collected_at.to_rfc3339(),
                                filters,
                                elapsed_seconds,
                                sessions_total,
                                &by_day,
                            )
                            .expect("daily JSON output should serialize")
                        );
                    } else {
                        println!(
                            "{}",
                            render_summary_json(
                                "watch session",
                                &collected_at.to_rfc3339(),
                                filters,
                                elapsed_seconds,
                                sessions_total,
                                &display_rows,
                            )
                            .expect("summary JSON output should serialize")
                        );
                    }
                }
            }
            return;
        }

        std::thread::sleep(WATCH_REFRESH_INTERVAL);
    }
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

    if args.watch {
        since = None;
        period = "watch session".to_string();
    } else if args.today {
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

    let output_mode = output_mode(&args);
    let collected_at = Local::now();

    if output_mode == OutputMode::Table && !args.by_day {
        println!("\nToken usage — {}", period);
    }
    if output_mode == OutputMode::Table {
        println!(
            "{}Collected: {}",
            if args.by_day { "\n" } else { "" },
            collected_at.format("%Y-%m-%d %H:%M:%S %Z")
        );
    }
    let number_format = if args.detail {
        NumberFormat::Full
    } else {
        NumberFormat::Compact
    };

    let show_all = !args.claude && !args.codex && !args.kimi && !args.gemini;
    let show_claude = show_all || args.claude;
    let show_codex = show_all || args.codex;
    let show_kimi = show_all || args.kimi;
    let show_gemini = show_all || args.gemini;

    let home = home_dir();
    let claude_agent = ClaudeAgent::new();
    let codex_agent = CodexAgent::new();
    let kimi_agent = KimiAgent::new();
    let gemini_agent = GeminiAgent::new();
    let agents: Vec<&dyn Agent> = [
        (show_claude, &claude_agent as &dyn Agent),
        (show_codex, &codex_agent as &dyn Agent),
        (show_kimi, &kimi_agent as &dyn Agent),
        (show_gemini, &gemini_agent as &dyn Agent),
    ]
    .into_iter()
    .filter_map(|(show, a)| show.then_some(a))
    .collect();
    let filters = OutputFilters {
        watch: args.watch,
        today: args.today,
        days: args.days,
        claude: args.claude,
        codex: args.codex,
        kimi: args.kimi,
        gemini: args.gemini,
        by_day: args.by_day,
        detail: args.detail,
    };

    if args.watch {
        run_watch(&args, &agents, &home, number_format, output_mode, filters);
        return;
    }

    let t0 = std::time::Instant::now();

    if args.by_day {
        let (by_day, sessions_total) = collect_selected_daily_usage(&agents, &home, since);
        let elapsed = t0.elapsed();
        match output_mode {
            OutputMode::Table => {
                print_daily_table(&period, &by_day, number_format);
                println!(
                    "  Scanned {} session(s) in {:.2}s",
                    sessions_total,
                    elapsed.as_secs_f64()
                );
                println!();
            }
            OutputMode::Csv => {
                println!("{}", render_daily_csv(&by_day, number_format));
            }
            OutputMode::Json => {
                println!(
                    "{}",
                    render_daily_json(
                        &period,
                        &collected_at.to_rfc3339(),
                        filters,
                        elapsed.as_secs_f64(),
                        sessions_total,
                        &by_day,
                    )
                    .expect("daily JSON output should serialize")
                );
            }
        }
    } else {
        let usages = collect_selected_usage(&agents, &home, since);
        let elapsed = t0.elapsed();
        let display_rows: Vec<(&str, &TokenUsage)> = usages
            .iter()
            .map(|entry| (entry.name, &entry.usage))
            .collect();
        let sessions_total: u32 = usages.iter().map(|entry| entry.usage.sessions).sum();

        match output_mode {
            OutputMode::Table => match usages.as_slice() {
                [single] => {
                    print_single(single.name, &single.usage, number_format);
                    println!(
                        "  Scanned {} session(s) in {:.2}s",
                        single.usage.sessions,
                        elapsed.as_secs_f64()
                    );
                    println!();
                }
                _ => {
                    print_multi_table(&display_rows, number_format);
                    println!(
                        "  Scanned {} session(s) in {:.2}s",
                        sessions_total,
                        elapsed.as_secs_f64()
                    );
                    println!();
                }
            },
            OutputMode::Csv => {
                println!("{}", render_summary_csv(&display_rows, number_format));
            }
            OutputMode::Json => {
                println!(
                    "{}",
                    render_summary_json(
                        &period,
                        &collected_at.to_rfc3339(),
                        filters,
                        elapsed.as_secs_f64(),
                        sessions_total,
                        &display_rows,
                    )
                    .expect("summary JSON output should serialize")
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use watch::{AgentSnapshot, SessionUsage};

    fn snapshot(entries: [(&str, SessionUsage); 1]) -> AgentSnapshot {
        entries
            .into_iter()
            .map(|(session_id, usage)| (session_id.to_string(), usage))
            .collect()
    }

    fn session_usage(input_tokens: u64, cached_input_tokens: u64, output_tokens: u64) -> SessionUsage {
        SessionUsage {
            totals: TokenUsage {
                input_tokens,
                cached_input_tokens,
                output_tokens,
                sessions: 1,
                user_queries: 1,
                ..Default::default()
            },
            by_day: DailyUsage::default(),
        }
    }

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
    fn parses_watch_flag() {
        let args = Args::try_parse_from(["toll", "--watch"]).expect("should parse");
        assert!(args.watch);
    }

    #[test]
    fn parses_watch_with_by_day() {
        let args = Args::try_parse_from(["toll", "--watch", "--by-day"]).expect("should parse");
        assert!(args.watch);
        assert!(args.by_day);
    }

    #[test]
    fn rejects_watch_with_today() {
        assert!(Args::try_parse_from(["toll", "--watch", "--today"]).is_err());
    }

    #[test]
    fn rejects_watch_with_days() {
        assert!(Args::try_parse_from(["toll", "--watch", "--days", "7"]).is_err());
    }

    #[test]
    fn parses_json_flag() {
        let args = Args::try_parse_from(["toll", "--json"]).expect("should parse");
        assert!(args.json);
        assert_eq!(output_mode(&args), OutputMode::Json);
    }

    #[test]
    fn parses_csv_flag() {
        let args = Args::try_parse_from(["toll", "--csv"]).expect("should parse");
        assert!(args.csv);
        assert_eq!(output_mode(&args), OutputMode::Csv);
    }

    #[test]
    fn diff_selected_snapshot_usage_uses_delta_rows() {
        let baseline = vec![AgentSnapshotEntry {
            name: "Codex",
            snapshot: snapshot([("session-a", session_usage(100, 40, 10))]),
        }];
        let current = vec![AgentSnapshotEntry {
            name: "Codex",
            snapshot: snapshot([("session-a", session_usage(160, 70, 16))]),
        }];

        let rows = diff_selected_snapshot_usage(&baseline, &current);

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].usage.input_tokens, 60);
        assert_eq!(rows[0].usage.cached_input_tokens, 30);
        assert_eq!(rows[0].usage.output_tokens, 6);
        assert_eq!(rows[0].usage.user_queries, 0);
    }

    #[test]
    fn agents_expose_distinct_names_and_data_dirs() {
        let claude = claude::ClaudeAgent::new();
        let codex = codex::CodexAgent::new();
        let kimi = kimi::KimiAgent::new();
        let gemini = gemini::GeminiAgent::new();
        let agents: [&dyn agent::Agent; 4] = [&claude, &codex, &kimi, &gemini];

        assert_eq!(agents[0].name(), "Claude Code");
        assert_eq!(agents[1].name(), "Codex");
        assert_eq!(agents[2].name(), "Kimi Code");
        assert_eq!(agents[3].name(), "Gemini");
        assert_eq!(
            agents[0].data_dir(Path::new("/tmp")),
            PathBuf::from("/tmp/.claude/projects")
        );
        assert_eq!(
            agents[1].data_dir(Path::new("/tmp")),
            PathBuf::from("/tmp/.codex/sessions")
        );
        assert_eq!(
            agents[2].data_dir(Path::new("/tmp")),
            PathBuf::from("/tmp/.kimi/sessions")
        );
        assert_eq!(
            agents[3].data_dir(Path::new("/tmp")),
            PathBuf::from("/tmp/.gemini/tmp")
        );
    }

    #[test]
    fn agent_filter_selects_correct_subset() {
        let claude = claude::ClaudeAgent::new();
        let codex = codex::CodexAgent::new();
        let kimi = kimi::KimiAgent::new();
        let gemini = gemini::GeminiAgent::new();

        fn filter(flags: [bool; 4], agents: [&dyn agent::Agent; 4]) -> Vec<&dyn agent::Agent> {
            flags
                .into_iter()
                .zip(agents)
                .filter_map(|(show, a)| show.then_some(a))
                .collect()
        }

        let all: [&dyn agent::Agent; 4] = [&claude, &codex, &kimi, &gemini];

        let claude_only = filter([true, false, false, false], all);
        assert_eq!(claude_only.len(), 1);
        assert_eq!(claude_only[0].name(), "Claude Code");

        let gemini_only = filter([false, false, false, true], all);
        assert_eq!(gemini_only.len(), 1);
        assert_eq!(gemini_only[0].name(), "Gemini");

        let all_four = filter([true, true, true, true], all);
        assert_eq!(all_four.len(), 4);
        assert_eq!(all_four[0].name(), "Claude Code");
        assert_eq!(all_four[3].name(), "Gemini");
    }
}
