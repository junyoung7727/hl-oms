//! `md-lake`: stage 2 of the lake (contract 2.5–2.7, 5.2) for a registered
//! venue (spec D9).
//!
//! ```text
//! md-lake seal      --venue V --lake DIR --first-hour YYYY-MM-DDTHH [--layout by_symbol|by_connection] [--now-ns N] [--telegram FILE]
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
use alert_direct::{AlertConfig, AlertDirect, Route, TelegramConfig};
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
    // U6 D9, R4: the alert config is checked before the lake is touched.
    let alert = match alert_config(args, venue.name()) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}");
            return 2;
        }
    };
    let mut s = SealSettings::new(venue.name(), SEALER, ver, &host, first_hour);
    s.layout = match arg(args, "--layout").as_deref() {
        None | Some("by_symbol") => Layout::BySymbol,
        Some("by_connection") => Layout::ByConnection,
        Some(_) => return usage(),
    };
    let sealer = Sealer::new(RealFs::new(&lake), s);
    let classify = |r: &md_lake::qraw::Record| venue.classify(r);
    let mut report = SealReport::default();
    let result = if cmd == "seal" {
        sealer.seal_into(&clock, &classify, &mut report)
    } else {
        let Some(from) = arg(args, "--from")
            .as_deref()
            .and_then(md_lake::time::date_start)
        else {
            return usage();
        };
        sealer.seal_days_since(&clock, from).map(|r| report = r)
    };
    // R12: hours noted before an error are alerted too.
    send_down_alert(alert, &report.collector_down_hours);
    match result {
        Ok(()) => {
            println!("{}", report_json(&report));
            0
        }
        Err(e) => seal_exit(&e),
    }
}

/// `--telegram FILE` (the token file) and `--telegram-api URL` (tests
/// only: a loopback Bot API; the token file may then sit in a checkout).
fn alert_config(args: &[String], venue: &str) -> Result<Option<AlertConfig>, String> {
    let api = arg(args, "--telegram-api");
    if let Some(u) = &api {
        let host = u
            .strip_prefix("http://")
            .and_then(|r| r.split('/').next())
            .and_then(|hp| hp.rsplit_once(':').map(|(h, _)| h.to_string()));
        let loopback = host
            .as_deref()
            .and_then(|h| h.parse::<std::net::IpAddr>().ok())
            .is_some_and(|ip| ip.is_loopback());
        if !loopback {
            return Err(format!("--telegram-api {u}: tests only, a loopback http://IP:PORT"));
        }
    }
    let Some(path) = arg(args, "--telegram") else {
        if api.is_some() {
            return Err("--telegram-api needs --telegram".into());
        }
        return Ok(None);
    };
    let tg = TelegramConfig::load_with(std::path::Path::new(&path), api.is_none())
        .map_err(|e| format!("--telegram: {e}"))?;
    let mut c = AlertConfig::new(tg, "[MD P1]", venue);
    if let Some(u) = &api {
        c.routes = vec![Route::direct(u)?];
    }
    Ok(Some(c))
}

/// One message per run for the hours with no open file (spec D9): a long
/// outage gives one message, not one per hour.
fn send_down_alert(alert: Option<AlertConfig>, hours: &[(String, u8)]) {
    if hours.is_empty() {
        return;
    }
    let Some(cfg) = alert else {
        eprintln!("collector_down hours {hours:?}: no --telegram, no direct alert");
        return;
    };
    let shown: Vec<String> = hours.iter().take(24).map(|(d, h)| format!("{d}T{h:02}")).collect();
    let more = hours.len().saturating_sub(shown.len());
    let mut text = format!(
        "수집기 정지: 수집기 파일이 하나도 없는 시간 {}개 (UTC)\n{}",
        hours.len(),
        shown.join(", ")
    );
    if more > 0 {
        text.push_str(&format!(" 외 {more}개"));
    }
    match AlertDirect::start(cfg, std::sync::Arc::new(alert_direct::SystemClock)) {
        Ok(h) => {
            h.once(&text);
            // R2: a blocked first route plus the pinned one fit in 20 s.
            if !h.flush_for(std::time::Duration::from_secs(20)) {
                let st = h.state();
                eprintln!("the [MD P1] collector_down alert was not delivered: {:?}", st.last_errors);
            }
        }
        Err(e) => eprintln!("cannot start the alert sender: {e}"),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(run(&args));
}
