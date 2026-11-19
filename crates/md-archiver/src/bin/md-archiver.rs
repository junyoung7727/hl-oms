//! `md-archiver`: builds the L1 tables of a venue from its sealed L0 files
//! (lake contract 3, spec D1).
//!
//! ```text
//! md-archiver run --lake DIR --venue V [--small-channels publicTrade,tickers]
//!     [--rebuild YYYY-MM-DD..YYYY-MM-DD] [--retain-l1-days N]
//!     [--disk-floor-bytes B] [--threads N] [--now-ns N]
//! md-archiver version
//! ```
//!
//! Exit codes: 0 done (also with nothing to build), 1 a failure a later run
//! may cure (I/O, disk below the floor, a held lock, a `--rebuild` date not
//! day-sealed), 2 a refusal (unknown venue, bad arguments, a newer contract,
//! layout `by_connection`); a refused run writes nothing.
//!
//! `--now-ns` replaces the wall clock for retention; a value in the future is
//! refused unless `MD_ARCHIVER_TEST_ALLOW_FUTURE_NOW=1`.

use std::path::PathBuf;

use md_archiver::build::Settings;
use md_archiver::run::{RunArgs, run, statvfs_free};

/// Design 9.4: below 20 GB free, the lake is in trouble.
const DEFAULT_DISK_FLOOR: u64 = 20_000_000_000;

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn usage() -> i32 {
    eprintln!(
        "usage:\n  md-archiver run --lake DIR --venue V [--small-channels a,b] [--rebuild FROM..TO] [--retain-l1-days N] [--disk-floor-bytes B] [--threads N] [--now-ns N]\n  md-archiver version"
    );
    2
}

fn wall_ns() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_nanos()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

const KNOWN: [&str; 8] = [
    "--lake",
    "--venue",
    "--small-channels",
    "--rebuild",
    "--retain-l1-days",
    "--disk-floor-bytes",
    "--threads",
    "--now-ns",
];

fn main_run(args: &[String]) -> i32 {
    let mut i = 0;
    while i < args.len() {
        if !KNOWN.contains(&args[i].as_str()) || i + 1 >= args.len() {
            eprintln!("unknown or incomplete argument {}", args[i]);
            return usage();
        }
        i += 2;
    }
    let (Some(lake), Some(venue)) = (arg(args, "--lake"), arg(args, "--venue")) else {
        return usage();
    };
    let num = |name: &str| -> Result<Option<i64>, ()> {
        match arg(args, name) {
            None => Ok(None),
            Some(s) => s.parse::<i64>().map(Some).map_err(|_| ()),
        }
    };
    let (Ok(retain), Ok(floor), Ok(threads), Ok(now)) = (
        num("--retain-l1-days"),
        num("--disk-floor-bytes"),
        num("--threads"),
        num("--now-ns"),
    ) else {
        eprintln!("a number argument is not a number");
        return usage();
    };
    if retain.is_some_and(|n| n < 0)
        || floor.is_some_and(|n| n < 0)
        || threads.is_some_and(|n| n < 1)
    {
        eprintln!("a number argument is out of range");
        return usage();
    }
    if now.is_some_and(|n| n > wall_ns())
        && std::env::var("MD_ARCHIVER_TEST_ALLOW_FUTURE_NOW").as_deref() != Ok("1")
    {
        eprintln!("--now-ns is in the future");
        return 2;
    }
    let rebuild = match arg(args, "--rebuild") {
        None => None,
        Some(r) => match r.split_once("..") {
            Some((a, b)) => Some((a.to_string(), b.to_string())),
            None => {
                eprintln!("--rebuild is FROM..TO");
                return usage();
            }
        },
    };
    let mut settings = Settings::default();
    if let Some(c) = arg(args, "--small-channels") {
        settings.small_channels = c
            .split(',')
            .filter(|s| !s.is_empty())
            .map(String::from)
            .collect();
    }
    if let Some(n) = threads {
        settings.threads = n as usize;
    }
    let args = RunArgs {
        lake: PathBuf::from(lake),
        venue,
        settings,
        rebuild,
        retain_l1_days: retain,
        disk_floor_bytes: floor.map_or(DEFAULT_DISK_FLOOR, |f| f as u64),
        now_ns: now.unwrap_or_else(wall_ns),
        free: statvfs_free(),
    };
    match run(&args) {
        Ok(_) => 0,
        Err(e) => {
            eprintln!("md-archiver: {}", e.message);
            e.code
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match args.first().map(String::as_str) {
        Some("version") => {
            println!(
                "md-archiver {} parser_ver {}",
                env!("CARGO_PKG_VERSION"),
                md_archiver::PARSER_VER_BYBIT
            );
            0
        }
        Some("run") => main_run(&args[1..]),
        _ => usage(),
    };
    std::process::exit(code);
}
