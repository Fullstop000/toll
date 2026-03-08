mod claude;
mod codex;
mod display;
mod pricing;
mod usage;

use chrono::{DateTime, Local, TimeZone, Utc};
use clap::Parser;
use std::path::PathBuf;

use claude::collect_claude_usage;
use codex::collect_codex_usage;
use display::{print_single, print_table};
use pricing::list_prices;
use usage::TokenUsage;

#[derive(Parser)]
#[command(name = "toll", about = "Token usage statistics for Claude Code and Codex CLI")]
#[command(after_help = "Examples:
  toll              # all-time stats
  toll --today      # today only
  toll --days 7     # last 7 days
  toll --claude     # Claude only
  toll --codex      # Codex only")]
struct Args {
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
}

fn home_dir() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/root"))
}

fn main() {
    let args = Args::parse();

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

    println!("\nToken usage — {}", period);
    println!("Collected: {}", Local::now().format("%Y-%m-%d %H:%M:%S %Z"));

    let show_claude = !args.codex;
    let show_codex = !args.claude;

    let home = home_dir();

    let t0 = std::time::Instant::now();

    let claude_usage = if show_claude {
        collect_claude_usage(&home.join(".claude").join("projects"), since)
    } else {
        TokenUsage::default()
    };

    let codex_usage = if show_codex {
        collect_codex_usage(&home.join(".codex").join("sessions"), since)
    } else {
        TokenUsage::default()
    };

    let elapsed = t0.elapsed();

    if !show_claude {
        print_single("Codex", &codex_usage);
    } else if !show_codex {
        print_single("Claude Code", &claude_usage);
    } else {
        print_table(&claude_usage, &codex_usage);
    }

    let sessions_total = claude_usage.sessions + codex_usage.sessions;
    println!(
        "  Scanned {} session(s) in {:.2}s",
        sessions_total,
        elapsed.as_secs_f64()
    );
    println!();
}
