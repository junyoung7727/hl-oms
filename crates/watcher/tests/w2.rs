//! W2 positions (C-WATCH part 6, U19a6), on the engine rig: the model
//! venue, the test journal and a manual clock. W1 and W2 rounds are every
//! 1 s, `min_age_ms` is 2 s.

#![allow(clippy::unwrap_used)]

mod common;

use common::ev;
use common::rig::*;
use serde_json::{Value, json};
use watcher::engine::Check;
use watcher::rest::RestError;
use watcher::w2::{PATH, W2};

/// A rig that runs W2 beside W1.
fn rig(name: &str) -> Rig {
    let mut r = Rig::new(name);
    r.engine.add_check(Box::new(W2::default()), 1000);
    r
}

/// `position_adjusted` of `instrument` by `delta`.
fn adj(instrument: &str, delta: &str) -> Value {
    ev(
        "position_adjusted",
        json!({"delta": delta, "instrument": instrument, "ledger": "unattributed", "note": "t"}),
    )
}

/// `external_fill_booked` of one buy of `qty`.
fn ext(key: &str, instrument: &str, qty: &str, venue_ts_us: u64) -> Value {
    ev(
        "external_fill_booked",
        json!({"fill": {"fee": "0", "instrument": instrument, "key": key, "order_id": null,
            "px": "100", "qty": qty, "side": "buy", "venue_oid": null, "venue_ts": venue_ts_us}}),
    )
}

fn detail(symbol: &str, venue: &str, journal: &str, age: &str) -> String {
    format!("W2 position {symbol}: venue {venue}, journal {journal}; seen 2 rounds, {age}")
}

fn w2_mismatches(r: &Rig) -> Vec<(u64, String)> {
    r.mismatches
        .iter()
        .filter(|m| m.1 == Check::W2)
        .map(|m| (m.0, m.2.clone()))
        .collect()
}

fn unreadable(r: &Rig) -> Option<f64> {
    let l = [("endpoint", PATH), ("class", "unreadable")];
    r.metrics.get("watcher_request_errors_total", &l)
}

#[test]
fn a_venue_position_in_a_symbol_the_journal_never_changed_is_confirmed_on_the_second_round() {
    let mut r = rig("w2-never-changed");
    // No journal change time and no venue time (no updatedTime, the size
    // found at the first round): both unknown, taken as old.
    r.venue.positions = vec![pos("ETHUSDT", "Sell", "0.5", None)];
    r.at(T0);
    assert_eq!(r.engine.success(Check::W2), Some(T0));
    assert_eq!(r.gauge("watcher_differences", "W2"), Some(1.0));
    assert!(w2_mismatches(&r).is_empty());
    r.at(T0 + 1000);
    let want = detail("ETHUSDT", "-0.5", "0", "change time unknown");
    assert_eq!(w2_mismatches(&r), vec![(T0 + 1000, want)]);
    assert_eq!(r.gauge("watcher_mismatches_total", "W2"), Some(1.0));
    assert_eq!(r.gauge("watcher_mismatch_active", "W2"), Some(1.0));
    assert_eq!(r.engine.success(Check::W2), Some(T0 + 1000));
    // The request: linear USDT, 200 a page.
    let (path, params) = r.venue.requests.iter().find(|q| q.0 == PATH).unwrap();
    assert_eq!(*path, "/v5/position/list");
    let want: Vec<(&str, String)> = vec![
        ("category", "linear".into()),
        ("settleCoin", "USDT".into()),
        ("limit", "200".into()),
    ];
    assert_eq!(params, &want);
}

#[test]
fn a_changing_difference_restarts_the_count() {
    let mut r = rig("w2-changing");
    // ETHUSDT: a journal position the venue never lists.
    r.write(T0 - 60_000, &[adj("BTCUSDT", "1"), adj("ETHUSDT", "2")]);
    r.venue.positions = vec![pos("BTCUSDT", "Buy", "2", Some(T0 - 60_000))];
    r.at(T0);
    // Another value at the next round: a new identity, pending again.
    r.venue.positions = vec![pos("BTCUSDT", "Buy", "3", Some(T0 - 10_000))];
    r.at(T0 + 1000);
    let eth = detail("ETHUSDT", "0", "2", "journal entry 61.0 s old");
    assert_eq!(w2_mismatches(&r), vec![(T0 + 1000, eth.clone())]);
    assert_eq!(r.engine.success(Check::W2), Some(T0 + 1000));
    r.at(T0 + 2000);
    let btc = detail("BTCUSDT", "3", "1", "journal entry 12.0 s old");
    assert_eq!(
        w2_mismatches(&r),
        vec![(T0 + 1000, eth), (T0 + 2000, btc)]
    );
    // Agreement ends both.
    r.write(T0 + 2100, &[adj("BTCUSDT", "2"), adj("ETHUSDT", "-2")]);
    r.at(T0 + 3000);
    assert_eq!(r.gauge("watcher_differences", "W2"), Some(0.0));
    assert_eq!(r.gauge("watcher_mismatch_active", "W2"), Some(0.0));
    assert_eq!(r.gauge("watcher_mismatches_total", "W2"), Some(2.0));
    assert_eq!(r.engine.success(Check::W2), Some(T0 + 3000));
}

#[test]
fn hedge_mode_rows_are_unreadable() {
    let mut r = rig("w2-hedge");
    // Two rows of one symbol (hedge mode), on one page and across pages.
    r.venue.positions = vec![
        pos("BTCUSDT", "Buy", "1", None),
        pos("BTCUSDT", "Sell", "1", None),
    ];
    r.at(T0);
    assert_eq!(r.engine.success(Check::W2), None);
    assert_eq!(unreadable(&r), Some(1.0));
    assert_eq!(
        r.lines_with("round failed: /v5/position/list: unreadable: two rows for BTCUSDT"),
        1
    );
    let mut rows: Vec<Value> = (0..200)
        .map(|i| pos(&format!("S{i}USDT"), "Buy", "1", None))
        .collect();
    rows.push(pos("S7USDT", "Sell", "2", None));
    r.venue.positions = rows;
    r.at(T0 + 1000);
    assert_eq!(unreadable(&r), Some(2.0));
    assert_eq!(r.engine.success(Check::W2), None);
    assert!(r.mismatches.is_empty());
    assert_eq!(r.engine.success(Check::W1), Some(T0 + 1000));
}

#[test]
fn bad_pages_and_rows_fail_the_round() {
    let mut r = rig("w2-paging");
    let rows = |n: usize| -> Vec<Value> {
        (0..n)
            .map(|i| pos(&format!("S{i}USDT"), "Buy", "1", None))
            .collect()
    };
    // A full page without a cursor; the cursor just sent; a side that is
    // not Buy, Sell or ""; a size that is not a decimal; no symbol.
    r.venue.pos_script.push_back(page(rows(200), ""));
    r.venue
        .pos_script
        .extend([page(rows(1), "c1"), page(vec![], "c1")]);
    r.venue
        .pos_script
        .push_back(page(vec![pos("BTCUSDT", "Long", "1", None)], ""));
    r.venue
        .pos_script
        .push_back(page(vec![pos("BTCUSDT", "Buy", "1e3", None)], ""));
    r.venue
        .pos_script
        .push_back(page(vec![json!({"side": "Buy", "size": "1"})], ""));
    for k in 0..5 {
        r.at(T0 + k * 1000);
    }
    assert_eq!(unreadable(&r), Some(5.0));
    for why in [
        "a full page without nextPageCursor",
        "nextPageCursor is the cursor just sent",
        "side \"Long\"",
        "size \"1e3\"",
        "a row without symbol",
    ] {
        assert_eq!(r.lines_with(why), 1, "{why}");
    }
    // 6 pages: the 5th still has a cursor.
    let before = r.venue.requests.iter().filter(|q| q.0 == PATH).count();
    for i in 0..6 {
        r.venue.pos_script.push_back(page(vec![], &format!("k{i}")));
    }
    r.at(T0 + 5000);
    let after = r.venue.requests.iter().filter(|q| q.0 == PATH).count();
    assert_eq!(after - before, 5);
    assert_eq!(
        r.lines_with("round failed: /v5/position/list: more than 1000 positions"),
        1
    );
    assert_eq!(r.engine.success(Check::W2), None);
    // A venue error fails the round, as for W1.
    r.venue.pos_script.clear();
    r.venue
        .pos_script
        .push_back(Err(RestError::NoAnswer("timeout".into())));
    r.at(T0 + 6000);
    assert_eq!(r.engine.success(Check::W2), None);
    // An empty side is no position; an empty page with a cursor is
    // followed; a short page without one ends the read.
    r.venue.pos_script.extend([
        page(vec![pos("BTCUSDT", "", "0", None)], "c2"),
        page(vec![], "c3"),
        page(vec![], ""),
    ]);
    r.at(T0 + 7000);
    assert_eq!(r.engine.success(Check::W2), Some(T0 + 7000));
    assert_eq!(r.gauge("watcher_differences", "W2"), Some(0.0));
}

#[test]
fn a_young_journal_change_defers_even_at_the_first_sighting() {
    let mut r = rig("w2-young-first");
    // The venue's time is old, the journal's is 0.5 s old.
    r.write(T0 - 500, &[adj("BTCUSDT", "1")]);
    r.venue.positions = vec![pos("BTCUSDT", "Buy", "2", Some(T0 - 60_000))];
    r.at(T0);
    assert_eq!(r.engine.success(Check::W2), None);
    assert_eq!(r.gauge("watcher_differences", "W2"), Some(1.0));
    r.at(T0 + 1000);
    assert_eq!(r.engine.success(Check::W2), None);
    assert!(w2_mismatches(&r).is_empty());
    r.at(T0 + 2000);
    let want = detail("BTCUSDT", "2", "1", "journal entry 2.5 s old");
    assert_eq!(w2_mismatches(&r), vec![(T0 + 2000, want)]);
    assert_eq!(r.engine.success(Check::W2), Some(T0 + 2000));
    // A known time exactly 2 s old is old: a first sighting is pending.
    r.write(T0 + 1000, &[adj("ETHUSDT", "1")]);
    r.at(T0 + 3000);
    assert_eq!(r.engine.success(Check::W2), Some(T0 + 3000));
    assert_eq!(r.gauge("watcher_differences", "W2"), Some(1.0));
}

#[test]
fn a_deferred_round_is_not_a_success() {
    let mut r = rig("w2-deferred");
    r.at(T0);
    assert_eq!(r.engine.success(Check::W2), Some(T0));
    // The venue moves 100 ms before every round and the journal never
    // follows: every round is deferred, none confirms.
    for k in 1..=5 {
        let size = k.to_string();
        let t = T0 + k * 1000;
        r.venue.positions = vec![pos("BTCUSDT", "Buy", &size, Some(t - 100))];
        r.at(t);
        assert_eq!(r.engine.success(Check::W2), Some(T0), "round {k}");
    }
    assert!(r.mismatches.is_empty());
    assert_eq!(r.gauge("watcher_check_age_s", "W2"), Some(5.0));
    assert_eq!(r.gauge("watcher_differences", "W2"), Some(1.0));
    // The heartbeat carries W2's last success, not the deferred rounds.
    let (at, checks) = r.heartbeats.last().unwrap();
    assert_eq!(*at, T0 + 5000);
    assert!(checks.contains(&(Check::W2, Some(T0))));
    assert!(checks.contains(&(Check::W1, Some(T0 + 4000))));
}
