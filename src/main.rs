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
use display::{NumberFormat, print_single, print_table};
use pricing::list_prices;
use usage::TokenUsage;

#[derive(Parser)]
#[command(
    name = "toll",
    about = "Token usage statistics for Claude Code and Codex CLI"
)]
#[command(after_help = "Examples:
  toll              # all-time stats
  toll --today      # today only
  toll --days 7     # last 7 days
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
}

fn home_dir() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/root"))
}

fn version_text() -> String {
    format!("toll {}", env!("CARGO_PKG_VERSION"))
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

    println!("\nToken usage — {}", period);
    println!("Collected: {}", Local::now().format("%Y-%m-%d %H:%M:%S %Z"));
    let number_format = if args.detail {
        NumberFormat::Full
    } else {
        NumberFormat::Compact
    };

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
        print_single("Codex", &codex_usage, number_format);
    } else if !show_codex {
        print_single("Claude Code", &claude_usage, number_format);
    } else {
        print_table(&claude_usage, &codex_usage, number_format);
    }

    let sessions_total = claude_usage.sessions + codex_usage.sessions;
    println!(
        "  Scanned {} session(s) in {:.2}s",
        sessions_total,
        elapsed.as_secs_f64()
    );
    println!();
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
