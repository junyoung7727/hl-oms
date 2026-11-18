//! `md-lake`: stage 2 of the lake (contract 2.5–2.7, 5.2) for a registered
//! venue (spec D9).
//!
//! ```text
//! md-lake seal      --venue V --lake DIR --first-hour YYYY-MM-DDTHH [--layout by_symbol|by_connection] [--now-ns N]
//! md-lake seal-days --venue V --lake DIR --first-hour YYYY-MM-DDTHH --from YYYY-MM-DD [--now-ns N]
//! md-lake verify    --venue V --lake DIR --date YYYY-MM-DD --json [--now-ns N]
//! ```
//!
//! Exit codes: 0 done (hours waiting on a locked file are reported, not an
//! error), 1 seal or I/O error, 2 usage error or unknown venue (nothing is
//! touched), 3 another run of the venue holds the seal lock.
//!
//! `--now-ns` replaces the wall clock (a person re-running a past state); a
//! value in the future is refused unless `MD_LAKE_TEST_ALLOW_FUTURE_NOW=1`.

use md_collector::registry;
use md_lake::fs::RealFs;
use md_lake::seal::{Clock, Layout, SealError, SealReport, SealSettings, Sealer};
use serde_json::json;

const SEALER: &str = "md_lake";

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn usage() -> i32 {
    eprintln!(
        "usage:\n  md-lake seal --venue V --lake DIR --first-hour YYYY-MM-DDTHH [--layout by_symbol|by_connection] [--now-ns N]\n  md-lake seal-days --venue V --lake DIR --first-hour YYYY-MM-DDTHH --from YYYY-MM-DD [--now-ns N]\n  md-lake verify --venue V --lake DIR --date YYYY-MM-DD --json [--now-ns N]"
    );
    2
}

/// The wall clock, or a fixed time (`--now-ns`: tests, and a person
/// re-running a past state; the lock rule still holds).
struct ArgClock(Option<i64>);

impl Clock for ArgClock {
    fn now_ns(&self) -> i64 {
        self.0.unwrap_or_else(md_collector::wall_ns)
    }
}

fn parse_hour(s: &str) -> Option<i64> {
    let (date, hh) = s.split_once('T')?;
    let h: i64 = hh
        .parse()
        .ok()
        .filter(|h| (0..24).contains(h) && hh.len() == 2)?;
    Some(md_lake::time::date_start(date)? + h * md_lake::time::HOUR_NS)
}

fn report_json(r: &SealReport) -> serde_json::Value {
    json!({
        "sealed": r.sealed.iter().map(|(d, h)| format!("{d}T{h:02}")).collect::<Vec<_>>(),
        "waiting_on": r.waiting_on,
        "days_sealed": r.days_sealed,
        "groups": r.groups.len(),
        "max_live_context_bytes": r.max_live_context_bytes,
    })
}

fn seal_exit(e: &SealError) -> i32 {
    eprintln!("seal failed: {e:?}");
    match e {
        SealError::Busy => 3,
        _ => 1,
    }
}

fn run(args: &[String]) -> i32 {
    let Some(cmd) = args.first().map(String::as_str) else {
        return usage();
    };
    if !matches!(cmd, "seal" | "seal-days" | "verify") {
        return usage();
    }
    let (Some(venue_name), Some(lake)) = (arg(args, "--venue"), arg(args, "--lake")) else {
        return usage();
    };
    // The venue first: an unknown one touches nothing.
    let Some(venue) = registry::venue(&venue_name) else {
        eprintln!("venue {venue_name} is not registered in this build");
        return 2;
    };
    let now = match arg(args, "--now-ns").map(|s| s.parse::<i64>()) {
        None => None,
        Some(Ok(n)) => Some(n),
        Some(Err(_)) => return usage(),
    };
    // A clock in the future could seal hours that are not closed yet (spec
    // R8): only tests may use one.
    if now.is_some_and(|n| n > md_collector::wall_ns())
        && std::env::var("MD_LAKE_TEST_ALLOW_FUTURE_NOW").as_deref() != Ok("1")
    {
        eprintln!("--now-ns is in the future");
        return 2;
    }
    let clock = ArgClock(now);
    let host = md_collector::host_name();
    let ver = env!("CARGO_PKG_VERSION");
    if cmd == "verify" {
        let Some(date) = arg(args, "--date") else {
            return usage();
        };
        if !args.iter().any(|a| a == "--json") || !md_lake::paths::valid_date(&date) {
            return usage();
        }
        let fs = RealFs::new(&lake);
        return match md_lake::verify::verify(fs.as_ref(), venue.name(), &date, ver, &clock) {
            Ok(v) => {
                println!("{v}");
                0
            }
            Err(e) => {
                eprintln!("verify failed: {e}");
                1
            }
        };
    }
    let Some(first_hour) = arg(args, "--first-hour").as_deref().and_then(parse_hour) else {
        return usage();
    };
    let mut s = SealSettings::new(venue.name(), SEALER, ver, &host, first_hour);
    s.layout = match arg(args, "--layout").as_deref() {
        None | Some("by_symbol") => Layout::BySymbol,
        Some("by_connection") => Layout::ByConnection,
        Some(_) => return usage(),
    };
    let sealer = Sealer::new(RealFs::new(&lake), s);
    let classify = |r: &md_lake::qraw::Record| venue.classify(r);
    let result = if cmd == "seal" {
        sealer.seal(&clock, &classify)
    } else {
        let Some(from) = arg(args, "--from")
            .as_deref()
            .and_then(md_lake::time::date_start)
        else {
            return usage();
        };
        sealer.seal_days_since(&clock, from)
    };
    match result {
        Ok(r) => {
            println!("{}", report_json(&r));
            0
        }
        Err(e) => seal_exit(&e),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(run(&args));
}
