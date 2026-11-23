//! W3 wallet and margin (C-WATCH part 9, U19a9), on the engine rig: the
//! model venue's wallet, the test journal and a manual clock. W1 rounds
//! are every 1 s, W3 rounds every 2 s; the limits are the defaults
//! (`account_mm_rate_max_bps` 5000, `daily_loss_bps` 500).

#![allow(clippy::unwrap_used)]

mod common;

use common::ev;
use common::rig::*;
use serde_json::{Value, json};
use watcher::engine::Check;
use watcher::rest::RestError;
use watcher::w3::{PATH, W3};

const DAY: u64 = 86_400_000;

/// A rig that runs W3 (every 2 s) beside W1.
fn rig(name: &str) -> Rig {
    let mut r = Rig::new(name);
    r.engine.add_check(Box::new(W3::new(&config().w3)), 2000);
    r
}

fn w3_mismatches(r: &Rig) -> Vec<(u64, String)> {
    r.mismatches
        .iter()
        .filter(|m| m.1 == Check::W3)
        .map(|m| (m.0, m.2.clone()))
        .collect()
}

/// `day_base_set` of UTC day `day` with `value`.
fn base(day: u64, value: &str) -> Value {
    ev(
        "day_base_set",
        json!({"day": day, "set_by": "report", "value": value}),
    )
}

fn rate_detail(maintenance: &str, balance: &str) -> String {
    format!(
        "W3 account_mm_rate account: venue totalMaintenanceMargin {maintenance}, totalMarginBalance {balance}, at or above 5000 bps; seen 2 rounds"
    )
}

fn loss_detail(equity: &str, day: u64, value: &str) -> String {
    format!(
        "W3 daily_loss account: venue totalEquity {equity}, journal day base {value} of day {day}, more than 500 bps below; seen 2 rounds"
    )
}

fn unreadable(r: &Rig) -> Option<f64> {
    let l = [("endpoint", PATH), ("class", "unreadable")];
    r.metrics.get("watcher_request_errors_total", &l)
}

#[test]
fn a_margin_rate_at_the_limit_on_two_rounds_alarms() {
    let mut r = rig("w3-rate");
    // One unit of 10^-8 below the limit: no violation (the OMS's formula
    // `maintenance × 10 000 ≥ max_bps × margin_balance`).
    r.venue.wallet = vec![account("1000", "499.99999999", "1000")];
    r.at(T0);
    r.at(T0 + 2000);
    assert_eq!(r.gauge("watcher_differences", "W3"), Some(0.0));
    assert_eq!(r.engine.success(Check::W3), Some(T0 + 2000));
    // At the limit, one round only: pending, never an alarm.
    r.venue.wallet = vec![account("1000", "500", "1000")];
    r.at(T0 + 4000);
    assert_eq!(r.gauge("watcher_differences", "W3"), Some(1.0));
    assert_eq!(r.engine.success(Check::W3), Some(T0 + 4000));
    r.venue.wallet = vec![account("1000", "0", "1000")];
    r.at(T0 + 6000);
    assert_eq!(r.gauge("watcher_differences", "W3"), Some(0.0));
    assert!(w3_mismatches(&r).is_empty());
    // At the limit on two rounds in a row: confirmed at the second. The
    // identity is the kind alone: another maintenance is the same one.
    r.venue.wallet = vec![account("1000", "500", "1000")];
    r.at(T0 + 8000);
    assert!(w3_mismatches(&r).is_empty());
    r.venue.wallet = vec![account("1000", "600", "1000")];
    r.at(T0 + 10_000);
    assert_eq!(
        w3_mismatches(&r),
        vec![(T0 + 10_000, rate_detail("600", "1000"))]
    );
    assert_eq!(r.gauge("watcher_mismatches_total", "W3"), Some(1.0));
    assert_eq!(r.gauge("watcher_mismatch_active", "W3"), Some(1.0));
    assert_eq!(r.engine.success(Check::W3), Some(T0 + 10_000));
    // A margin balance at or below 0 with a maintenance above 0 is at
    // every limit; a maintenance of 0 is at none, whatever the balance.
    let mut r = rig("w3-rate-edges");
    r.venue.wallet = vec![account("0", "0", "-5")];
    r.at(T0);
    r.at(T0 + 2000);
    assert_eq!(r.gauge("watcher_differences", "W3"), Some(0.0));
    r.venue.wallet = vec![account("0", "0.00000001", "0")];
    r.at(T0 + 4000);
    r.at(T0 + 6000);
    assert_eq!(
        w3_mismatches(&r),
        vec![(T0 + 6000, rate_detail("0.00000001", "0"))]
    );
    // The request: the unified account.
    let (path, params) = r.venue.requests.iter().find(|q| q.0 == PATH).unwrap();
    assert_eq!(*path, "/v5/account/wallet-balance");
    assert_eq!(params, &vec![("accountType", "UNIFIED".to_string())]);
}

#[test]
fn a_daily_loss_beyond_todays_base_on_two_rounds_alarms() {
    let mut r = rig("w3-loss");
    let today = T0 / DAY;
    r.write(T0 - 60_000, &[base(today, "100")]);
    // 95.00 of 100 is 500 bps below: not beyond (the OMS's `>`).
    r.venue.wallet = vec![account("95", "0", "95")];
    r.at(T0);
    r.at(T0 + 2000);
    assert_eq!(r.gauge("watcher_differences", "W3"), Some(0.0));
    // 94.99 is beyond: pending at the first round, confirmed at the next.
    r.venue.wallet = vec![account("94.99", "0", "94.99")];
    r.at(T0 + 4000);
    assert_eq!(r.gauge("watcher_differences", "W3"), Some(1.0));
    assert!(w3_mismatches(&r).is_empty());
    r.at(T0 + 6000);
    let d = loss_detail("94.99", today, "100");
    assert_eq!(w3_mismatches(&r), vec![(T0 + 6000, d.clone())]);
    assert!(r.held);
    // The operator clears the watcher hold while the loss lasts: W3 sends
    // it again once `mismatch_resend_ms` (10 s) has passed since the last
    // send, and the hold is back.
    r.clear(T0 + 7000);
    for k in 4..=7 {
        r.at(T0 + k * 2000);
    }
    assert_eq!(w3_mismatches(&r).len(), 1);
    r.at(T0 + 16_000);
    assert_eq!(
        w3_mismatches(&r),
        vec![(T0 + 6000, d.clone()), (T0 + 16_000, d)]
    );
    assert!(r.held);
    // A new base of today (the operator's rebase) ends the difference;
    // after the next clear nothing more is sent.
    r.write(T0 + 19_000, &[base(today, "94.99")]);
    r.at(T0 + 20_000);
    assert_eq!(r.gauge("watcher_differences", "W3"), Some(0.0));
    assert_eq!(r.gauge("watcher_mismatch_active", "W3"), Some(0.0));
    r.clear(T0 + 21_000);
    for k in 11..=20 {
        r.at(T0 + k * 2000);
    }
    assert_eq!(w3_mismatches(&r).len(), 2);
}

#[test]
fn without_todays_base_only_the_margin_rate_is_judged() {
    let today = T0 / DAY;
    for (name, records) in [
        ("w3-no-base", vec![]),
        ("w3-old-base", vec![base(today - 1, "1000")]),
    ] {
        let mut r = rig(name);
        if !records.is_empty() {
            r.write(T0 - 60_000, &records);
        }
        // Half the base lost, and the margin rate at the limit.
        r.venue.wallet = vec![account("500", "250", "500")];
        r.at(T0);
        assert_eq!(r.gauge("watcher_differences", "W3"), Some(1.0), "{name}");
        r.at(T0 + 2000);
        assert_eq!(
            w3_mismatches(&r),
            vec![(T0 + 2000, rate_detail("250", "500"))],
            "{name}"
        );
        assert_eq!(r.lines_with("W3 daily_loss"), 0, "{name}");
    }
    // A base of today is judged until the UTC day ends; after midnight
    // the old base is not judged.
    let mut r = rig("w3-midnight");
    r.write(T0 - 60_000, &[base(today, "1000")]);
    r.venue.wallet = vec![account("500", "0", "500")];
    r.at(T0);
    assert_eq!(r.gauge("watcher_differences", "W3"), Some(1.0));
    let midnight = (today + 1) * DAY;
    r.at(midnight);
    assert_eq!(r.gauge("watcher_differences", "W3"), Some(0.0));
    r.at(midnight + 2000);
    assert!(w3_mismatches(&r).is_empty());
}

#[test]
fn an_unreadable_wallet_fails_the_round() {
    let mut r = rig("w3-unreadable");
    r.venue.wallet = vec![account("1000", "0", "1000")];
    r.at(T0);
    assert_eq!(r.engine.success(Check::W3), Some(T0));
    let other = json!({"accountType": "CONTRACT", "totalEquity": "1",
        "totalMaintenanceMargin": "0", "totalMarginBalance": "1"});
    let mut no_equity = account("1000", "0", "1000");
    no_equity["totalEquity"] = json!("");
    let pages = [
        json!({"list": [other]}),
        json!({"list": [account("1", "0", "1"), account("1", "0", "1")]}),
        json!({"list": [no_equity]}),
        json!({"list": [account("1000", "-1", "1000")]}),
        json!({"list": [account("1e3", "0", "1000")]}),
        json!({"list": "x"}),
    ];
    let n = pages.len() as u64;
    for p in pages {
        r.venue.wallet_script.push_back(Ok(p));
    }
    r.venue
        .wallet_script
        .push_back(Err(RestError::NoAnswer("timeout".into())));
    for k in 1..=n + 1 {
        r.at(T0 + k * 2000);
    }
    assert_eq!(unreadable(&r), Some(n as f64));
    assert_eq!(r.engine.success(Check::W3), Some(T0));
    assert_eq!(r.lines_with("warn: W3 round failed"), (n + 1) as usize);
    // A readable answer again: a success.
    r.at(T0 + (n + 2) * 2000);
    assert_eq!(r.engine.success(Check::W3), Some(T0 + (n + 2) * 2000));
}
