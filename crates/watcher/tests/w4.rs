//! W4 executions (C-WATCH part 8, U19a8), on the engine rig: the model
//! venue, the test journal and a manual clock. W1 rounds are every 1 s,
//! W4 sweeps every 5 s (a sweep of one round each unless it says
//! otherwise), `min_age_ms` is 2 s, `w4_overlap_ms` 10 s, `w4_max_pages`
//! 10, `w4_initial_lookback_ms` 600 s.

#![allow(clippy::unwrap_used)]

mod common;

use common::ev;
use common::rig::*;
use serde_json::{Value, json};
use watcher::config::Config;
use watcher::engine::Check;
use watcher::rest::RestError;
use watcher::w2::W2;
use watcher::w4::{ExecTimes, PATH, W4};

const DAY: u64 = 86_400_000;

/// A rig that runs W4 (every 5 s) beside W1.
fn rig(name: &str) -> (Rig, ExecTimes) {
    rig_with(name, &config())
}

fn rig_with(name: &str, cfg: &Config) -> (Rig, ExecTimes) {
    let mut r = Rig::with_config(name, cfg);
    let w4 = W4::new(&cfg.rules, r.log.clone());
    let times = w4.exec_times();
    r.engine.add_check(Box::new(w4), 5000);
    (r, times)
}

/// The default config with `w4_max_pages = 1`.
fn one_page() -> Config {
    let mut cfg = config();
    cfg.rules.w4_max_pages = 1;
    cfg
}

/// `external_fill_booked` of the execution `<instrument>:<id>`.
fn ext(instrument: &str, id: &str, side: &str, qty: &str, at_ms: u64) -> Value {
    ev(
        "external_fill_booked",
        json!({"fill": {"fee": "0", "instrument": instrument,
            "key": format!("{instrument}:{id}"), "order_id": null, "px": "100", "qty": qty,
            "side": side, "venue_oid": null, "venue_ts": at_ms * 1000}}),
    )
}

/// `venue_position_changed` of the execution `<instrument>:<id>`.
fn change(instrument: &str, id: &str, kind: &str, qty: &str, at_ms: u64) -> Value {
    ev(
        "venue_position_changed",
        json!({"change": {"instrument": instrument, "key": format!("{instrument}:{id}"),
            "kind": kind, "px": "100", "qty": qty, "venue_ts": at_ms * 1000}}),
    )
}

/// `funding_booked` of the execution `<instrument>:<id>`.
fn funding(instrument: &str, id: &str, at_ms: u64) -> Value {
    ev(
        "funding_booked",
        json!({"funding": {"amount": "0.1", "instrument": instrument,
            "key": format!("{instrument}:{id}"), "rate": "0.0001", "venue_ts": at_ms * 1000}}),
    )
}

/// A BTCUSDT Trade buy of 1.
fn trade(id: &str, t: u64) -> Value {
    exec("BTCUSDT", id, "Trade", "Buy", "1", t)
}

/// A record at `t` ms, or at the journal's newest time if that is later
/// (the reader refuses a record time below the previous one).
fn rec(r: &mut Rig, t: u64, events: &[Value]) {
    let at = (t * 1000).max(r.journal.last_at);
    r.journal.append(at, events);
}

/// A BTCUSDT Trade buy of 1 at the venue and in the journal.
fn known(r: &mut Rig, id: &str, t: u64) {
    r.venue.executions.push(trade(id, t));
    rec(r, t, &[ext("BTCUSDT", id, "buy", "1", t)]);
}

fn w4_mismatches(r: &Rig) -> Vec<(u64, String)> {
    r.mismatches
        .iter()
        .filter(|m| m.1 == Check::W4)
        .map(|m| (m.0, m.2.clone()))
        .collect()
}

/// `startTime`, `endTime` and `cursor` of each execution request.
fn windows(r: &Rig) -> Vec<(u64, u64, Option<String>)> {
    r.venue
        .exec_requests()
        .iter()
        .map(|p| {
            (
                param(p, "startTime").unwrap().parse().unwrap(),
                param(p, "endTime").unwrap().parse().unwrap(),
                param(p, "cursor"),
            )
        })
        .collect()
}

fn missing_fill(id: &str, age: &str) -> String {
    format!(
        "W4 missing_fill BTCUSDT:{id}: venue Trade Buy 1, journal no fill; seen 2 sweeps, execTime {age} old"
    )
}

/// `n` rows 100 ms apart from `from`, all in the journal except the
/// oldest one (`m0`, the last page's), each booked in one record.
fn many(r: &mut Rig, n: u64, from: u64) {
    let mut booked = Vec::new();
    for i in 0..n {
        let t = from + i * 100;
        r.venue.executions.push(trade(&format!("m{i}"), t));
        if i > 0 {
            booked.push(ext("BTCUSDT", &format!("m{i}"), "buy", "1", t));
        }
    }
    rec(r, from, &booked);
}

#[test]
fn a_missing_trade_needs_two_sweeps() {
    let (mut r, _) = rig("w4-two-sweeps");
    // X is missing in the journal; Y, 25 s newer, agrees: a watermark
    // that passed X would leave it out of the next window.
    r.venue.executions.push(trade("x", T0 - 30_000));
    known(&mut r, "y", T0 - 5_000);
    r.at(T0);
    assert!(w4_mismatches(&r).is_empty());
    assert_eq!(r.engine.success(Check::W4), Some(T0));
    assert_eq!(r.gauge("watcher_differences", "W4"), Some(1.0));
    r.at(T0 + 5000);
    assert_eq!(
        w4_mismatches(&r),
        vec![(T0 + 5000, missing_fill("x", "35.0 s"))]
    );
    assert_eq!(r.gauge("watcher_mismatches_total", "W4"), Some(1.0));
    assert_eq!(r.gauge("watcher_differences", "W4"), Some(0.0));
    // The request: linear USDT, 100 a page, the window.
    let req = r.venue.exec_requests()[0].clone();
    let want: Vec<(&str, String)> = vec![
        ("category", "linear".into()),
        ("settleCoin", "USDT".into()),
        ("limit", "100".into()),
        ("startTime", (T0 - 610_000).to_string()),
        ("endTime", T0.to_string()),
    ];
    assert_eq!(req, want);
    assert_eq!(r.venue.requests.iter().filter(|q| q.0 == PATH).count(), 2);
    // The watermark did not pass X: the same start.
    assert_eq!(windows(&r)[1], (T0 - 610_000, T0 + 5000, None));
}

#[test]
fn the_sweep_starts_10_s_before_the_last_confirmed_exec_time() {
    let (mut r, _) = rig("w4-overlap");
    known(&mut r, "y", T0 - 5_000);
    r.at(T0);
    // The first window: the initial watermark (600 s back) − 10 s.
    assert_eq!(windows(&r)[0], (T0 - 610_000, T0, None));
    // X, 3 s before T0, is missing: pending, then confirmed.
    r.venue.executions.push(trade("x", T0 - 3_000));
    r.at(T0 + 5000);
    assert_eq!(windows(&r)[1], (T0 - 15_000, T0 + 5000, None));
    r.at(T0 + 10_000);
    assert_eq!(w4_mismatches(&r).len(), 1);
    // Confirmed, X no longer holds the watermark back.
    r.at(T0 + 15_000);
    assert_eq!(windows(&r)[2], (T0 - 15_000, T0 + 10_000, None));
    assert_eq!(windows(&r)[3], (T0 - 13_000, T0 + 15_000, None));
}

#[test]
fn a_round_of_250_executions_is_compared_in_full_over_3_pages() {
    let (mut r, _) = rig("w4-250");
    many(&mut r, 250, T0 - 60_000);
    r.at(T0);
    let cursors: Vec<Option<String>> = windows(&r).into_iter().map(|w| w.2).collect();
    assert_eq!(
        cursors,
        vec![None, Some("e100".into()), Some("e200".into())]
    );
    assert_eq!(r.gauge("watcher_differences", "W4"), Some(1.0));
    assert_eq!(r.engine.success(Check::W4), Some(T0));
    r.at(T0 + 5000);
    assert_eq!(windows(&r).len(), 6);
    assert_eq!(
        w4_mismatches(&r),
        vec![(T0 + 5000, missing_fill("m0", "65.0 s"))]
    );
}

#[test]
fn a_row_loaded_late_inside_the_overlap_is_compared() {
    let (mut r, _) = rig("w4-late-row");
    known(&mut r, "y", T0 - 2_000);
    r.at(T0);
    // L appears at the venue after the sweep that moved the watermark
    // past its time; it is inside the 10 s overlap.
    r.venue.executions.push(trade("l", T0 - 8_000));
    r.at(T0 + 5000);
    assert_eq!(windows(&r)[1].0, T0 - 12_000);
    r.at(T0 + 10_000);
    assert_eq!(
        w4_mismatches(&r),
        vec![(T0 + 10_000, missing_fill("l", "18.0 s"))]
    );
}

#[test]
fn the_watermark_never_moves_back() {
    let (mut r, _) = rig("w4-never-back");
    known(&mut r, "y", T0 - 1_000);
    known(&mut r, "z", T0 - 9_000);
    r.at(T0);
    // The venue no longer lists Y: the newest agreeing row is Z, older
    // than the watermark.
    r.venue.executions.retain(|e| e["execId"] != "y");
    r.at(T0 + 5000);
    r.at(T0 + 10_000);
    let starts: Vec<u64> = windows(&r).iter().map(|w| w.0).collect();
    assert_eq!(starts, vec![T0 - 610_000, T0 - 11_000, T0 - 11_000]);
}

#[test]
fn executions_1200_are_compared_over_two_rounds_and_the_w4_age_rises_between() {
    let (mut r, _) = rig("w4-1200");
    many(&mut r, 1200, T0 - 300_000);
    r.at(T0);
    // Round 1: 10 pages, the cap; no comparison, no success.
    assert_eq!(windows(&r).len(), 10);
    assert_eq!(r.engine.success(Check::W4), None);
    assert_eq!(r.gauge("watcher_check_age_s", "W4"), Some(5.0));
    r.at(T0 + 4000);
    assert_eq!(r.gauge("watcher_check_age_s", "W4"), Some(9.0));
    assert_eq!(r.gauge("watcher_differences", "W4"), Some(0.0));
    // Round 2 continues the sweep: same window, pages 11 and 12 (the
    // last one full, with an empty cursor).
    r.at(T0 + 5000);
    let w = windows(&r);
    assert_eq!(w.len(), 12);
    assert_eq!(
        w[10..].to_vec(),
        vec![
            (T0 - 610_000, T0, Some("e1000".into())),
            (T0 - 610_000, T0, Some("e1100".into()))
        ]
    );
    assert!(w.iter().all(|x| x.1 == T0));
    assert_eq!(r.engine.success(Check::W4), Some(T0 + 5000));
    assert_eq!(r.gauge("watcher_differences", "W4"), Some(1.0));
    r.at(T0 + 6000);
    assert_eq!(r.gauge("watcher_check_age_s", "W4"), Some(1.0));
}

#[test]
fn an_empty_page_ends_the_sweep() {
    let (mut r, _) = rig("w4-empty-page");
    r.venue
        .exec_script
        .push_back(page(vec![trade("x", T0 - 10_000)], "c1"));
    r.venue.exec_script.push_back(page(vec![], "c2"));
    r.at(T0);
    assert_eq!(windows(&r).len(), 2);
    assert_eq!(r.engine.success(Check::W4), Some(T0));
    assert_eq!(r.gauge("watcher_differences", "W4"), Some(1.0));
}

#[test]
fn an_unchanged_cursor_is_a_failed_page() {
    let (mut r, _) = rig("w4-same-cursor");
    r.venue
        .exec_script
        .push_back(page(vec![trade("x", T0 - 10_000)], "c1"));
    r.venue
        .exec_script
        .push_back(page(vec![trade("y", T0 - 20_000)], "c1"));
    r.at(T0);
    assert_eq!(r.engine.success(Check::W4), None);
    let l = [("endpoint", PATH), ("class", "unreadable")];
    assert_eq!(r.metrics.get("watcher_request_errors_total", &l), Some(1.0));
    assert_eq!(r.lines_with("nextPageCursor is the cursor just sent"), 1);
    // The next round retries that page.
    r.at(T0 + 5000);
    let w = windows(&r);
    assert_eq!(w[2], (T0 - 610_000, T0, Some("c1".into())));
    assert_eq!(r.engine.success(Check::W4), Some(T0 + 5000));
}

#[test]
fn a_failed_page_three_rounds_in_a_row_restarts_the_sweep_with_a_new_end() {
    let (mut r, _) = rig("w4-restart");
    r.venue
        .exec_script
        .push_back(page(vec![trade("x", T0 - 10_000)], "c1"));
    for _ in 0..3 {
        r.venue.exec_script.push_back(Err(RestError::Refused {
            code: 10001,
            msg: "cursor".into(),
        }));
    }
    for k in 0..4 {
        r.at(T0 + k * 5000);
    }
    let w = windows(&r);
    let start = T0 - 610_000;
    assert_eq!(
        w,
        vec![
            (start, T0, None),
            (start, T0, Some("c1".into())),
            (start, T0, Some("c1".into())),
            (start, T0, Some("c1".into())),
            (start, T0 + 15_000, None),
        ]
    );
    assert_eq!(r.lines_with("rounds in a row; the sweep restarts"), 1);
    assert_eq!(r.engine.success(Check::W4), Some(T0 + 15_000));
}

#[test]
fn a_sweep_not_complete_after_50_rounds_restarts() {
    let (mut r, _) = rig_with("w4-50-rounds", &one_page());
    for k in 0..50 {
        r.venue.exec_script.push_back(page(
            vec![trade(&format!("r{k}"), T0 - 1000)],
            &format!("c{k}"),
        ));
    }
    for k in 0..51 {
        r.at(T0 + k * 5000);
    }
    let w = windows(&r);
    assert_eq!(w.len(), 51);
    assert_eq!(w[49], (T0 - 610_000, T0, Some("c48".into())));
    assert_eq!(w[50], (T0 - 610_000, T0 + 250_000, None));
    assert_eq!(r.lines_with("not complete after 50 rounds"), 1);
    assert_eq!(r.engine.success(Check::W4), Some(T0 + 250_000));
}

#[test]
fn an_adl_trade_the_journal_lacks_is_a_mismatch() {
    let (mut r, _) = rig("w4-adl");
    // A1 is in the journal as a venue change; A2 is not.
    r.venue
        .executions
        .push(exec("BTCUSDT", "a1", "AdlTrade", "Sell", "1", T0 - 20_000));
    rec(
        &mut r,
        T0 - 20_000,
        &[change("BTCUSDT", "a1", "adl", "-1", T0 - 20_000)],
    );
    r.venue
        .executions
        .push(exec("BTCUSDT", "a2", "AdlTrade", "Sell", "1", T0 - 10_000));
    r.at(T0);
    r.at(T0 + 5000);
    let want = "W4 missing_venue_change BTCUSDT:a2: venue AdlTrade adl -1, journal no venue change; seen 2 sweeps, execTime 15.0 s old";
    assert_eq!(w4_mismatches(&r), vec![(T0 + 5000, want.to_string())]);
    assert_eq!(r.gauge("watcher_mismatches_total", "W4"), Some(1.0));
}

#[test]
fn a_trade_or_venue_change_with_other_details_is_a_difference() {
    let (mut r, _) = rig("w4-details");
    r.venue
        .executions
        .push(exec("BTCUSDT", "t", "Trade", "Buy", "2", T0 - 20_000));
    rec(
        &mut r,
        T0 - 20_000,
        &[ext("BTCUSDT", "t", "buy", "1", T0 - 20_000)],
    );
    r.venue
        .executions
        .push(exec("BTCUSDT", "b", "BustTrade", "Buy", "1", T0 - 30_000));
    rec(
        &mut r,
        T0 - 30_000,
        &[change("BTCUSDT", "b", "adl", "1", T0 - 30_000)],
    );
    r.at(T0);
    r.at(T0 + 5000);
    assert_eq!(r.confirmed.len(), 2);
    let want = [
        "W4 fill_details BTCUSDT:t: venue Buy 2, journal Buy 1; seen 2 sweeps, execTime 25.0 s old",
        "W4 venue_change_details BTCUSDT:b: venue takeover 1, journal adl 1; seen 2 sweeps, execTime 35.0 s old",
    ];
    for w in want {
        assert!(
            r.confirmed.iter().any(|c| c.1 == w),
            "{w}: {:?}",
            r.confirmed
        );
    }
}

#[test]
fn funding_rows_beside_known_fills_are_no_mismatch_while_a_position_is_held() {
    let (mut r, _) = rig("w4-funding");
    r.engine.add_check(Box::new(W2::default()), 1000);
    known(&mut r, "t1", T0 - 50_000);
    known(&mut r, "t2", T0 - 40_000);
    r.venue.positions = vec![pos("BTCUSDT", "Buy", "2", Some(T0 - 40_000))];
    // Funding rows with execQty and side present; the journal booked one.
    r.venue
        .executions
        .push(exec("BTCUSDT", "f1", "Funding", "Buy", "2", T0 - 30_000));
    rec(
        &mut r,
        T0 - 30_000,
        &[funding("BTCUSDT", "f1", T0 - 30_000)],
    );
    r.venue
        .executions
        .push(exec("BTCUSDT", "f2", "Funding", "Sell", "2", T0 - 20_000));
    for t in (T0..=T0 + 30_000).step_by(1000) {
        r.at(t);
    }
    assert!(r.mismatches.is_empty(), "{:?}", r.mismatches);
    assert!(r.confirmed.is_empty());
    assert_eq!(r.engine.success(Check::W2), Some(T0 + 30_000));
    assert_eq!(r.engine.success(Check::W4), Some(T0 + 30_000));
    assert_eq!(r.gauge("watcher_differences", "W4"), Some(0.0));
    assert_eq!(windows(&r).len(), 7);
}

#[test]
fn an_unknown_exec_type_is_a_mismatch_at_once() {
    let (mut r, _) = rig("w4-unknown");
    // Young too: no age rule.
    r.venue
        .executions
        .push(exec("BTCUSDT", "u", "MovePosition", "Buy", "1", T0 - 500));
    r.at(T0);
    let want = "W4 unknown_exec_type BTCUSDT:u: venue execType \"MovePosition\", journal not compared; seen 1 sweep, execTime 0.5 s old";
    assert_eq!(w4_mismatches(&r), vec![(T0, want.to_string())]);
    assert_eq!(r.gauge("watcher_mismatches_total", "W4"), Some(1.0));
    r.at(T0 + 5000);
    assert_eq!(r.gauge("watcher_mismatches_total", "W4"), Some(1.0));
}

#[test]
fn an_unknown_exec_type_in_a_multi_round_sweep_is_confirmed_when_the_sweep_completes() {
    let (mut r, _) = rig_with("w4-unknown-multi", &one_page());
    r.venue.exec_script.push_back(page(
        vec![exec("BTCUSDT", "u", "MovePosition", "Buy", "1", T0 - 3000)],
        "c1",
    ));
    r.venue.exec_script.push_back(page(vec![], ""));
    r.at(T0);
    assert!(r.confirmed.is_empty());
    assert_eq!(r.engine.success(Check::W4), None);
    r.at(T0 + 5000);
    assert_eq!(r.confirmed.len(), 1);
    assert_eq!(w4_mismatches(&r).len(), 1);
    assert_eq!(w4_mismatches(&r)[0].0, T0 + 5000);
}

#[test]
fn a_funding_row_without_side_or_qty_is_ignored() {
    let (mut r, _) = rig("w4-funding-bare");
    let bare = |id: &str, t: u64| {
        json!({"symbol": "BTCUSDT", "execId": id, "execType": "Funding",
            "execFee": "0.1", "execTime": t.to_string()})
    };
    r.venue.executions.push(bare("f1", T0 - 20_000));
    r.venue.executions.push(bare("f2", T0 - 4_000));
    r.at(T0);
    r.at(T0 + 5000);
    assert!(r.confirmed.is_empty());
    assert_eq!(r.gauge("watcher_differences", "W4"), Some(0.0));
    // Ignored rows let the watermark pass.
    assert_eq!(windows(&r)[1].0, T0 - 14_000);
}

#[test]
fn a_trade_row_without_qty_is_an_unreadable_row_and_needs_two_sweeps() {
    let (mut r, _) = rig("w4-unreadable");
    let mut row = trade("q", T0 - 10_000);
    row.as_object_mut().unwrap().remove("execQty");
    r.venue.executions.push(row);
    r.at(T0);
    assert!(r.confirmed.is_empty());
    assert_eq!(r.gauge("watcher_differences", "W4"), Some(1.0));
    r.at(T0 + 5000);
    let want = "W4 unreadable_row BTCUSDT:q: venue row without execQty, journal not compared; seen 2 sweeps, execTime 15.0 s old";
    assert_eq!(w4_mismatches(&r), vec![(T0 + 5000, want.to_string())]);
}

#[test]
fn a_row_without_a_key_is_named_by_its_text() {
    let (mut r, _) = rig("w4-no-key");
    let row = json!({"execType": "Trade", "side": "Buy", "execQty": "1",
        "execTime": (T0 - 10_000).to_string(), "note": "x".repeat(300)});
    r.venue.executions.push(row.clone());
    r.at(T0);
    r.at(T0 + 5000);
    let text = row.to_string();
    let want = format!(
        "W4 unreadable_row {}: venue row without execId,symbol, journal not compared; seen 2 sweeps, execTime 15.0 s old",
        &text[..200]
    );
    assert_eq!(w4_mismatches(&r), vec![(T0 + 5000, want)]);
}

#[test]
fn an_unconfirmed_row_without_a_readable_exec_time_holds_the_watermark() {
    let (mut r, _) = rig("w4-no-time");
    let mut row = trade("n", 0);
    row.as_object_mut().unwrap().remove("execTime");
    r.venue.executions.push(row);
    for k in 0..4 {
        known(&mut r, &format!("a{k}"), T0 - 30_000 + k * 10_000);
    }
    r.at(T0);
    assert!(r.confirmed.is_empty());
    r.at(T0 + 5000);
    // The watermark did not move: the same start; and N is confirmed.
    assert_eq!(windows(&r)[1].0, T0 - 610_000);
    let want = "W4 unreadable_row BTCUSDT:n: venue row without execTime, journal not compared; seen 2 sweeps, execTime unknown";
    assert_eq!(w4_mismatches(&r), vec![(T0 + 5000, want.to_string())]);
    // Confirmed, it no longer holds the watermark.
    r.at(T0 + 10_000);
    assert_eq!(windows(&r)[2].0, T0 - 10_000);
}

#[test]
fn a_trade_at_the_fill_floor_millisecond_is_ignored() {
    let (mut r, _) = rig("w4-floor");
    // The floor is 500 µs into the millisecond T0 − 20 s.
    let floor_us = (T0 - 20_000) * 1000 + 500;
    rec(
        &mut r,
        T0 - 30_000,
        &[ev("booted", json!({"fill_floor": floor_us}))],
    );
    // The venue lists a row in that millisecond (below the floor) and one
    // in the next; the journal has neither.
    for _ in 0..2 {
        r.venue.exec_script.push_back(page(
            vec![trade("f1", T0 - 19_999), trade("f0", T0 - 20_000)],
            "",
        ));
    }
    r.at(T0);
    r.at(T0 + 5000);
    // The start: the floor rounded up (the first sweep began before the
    // journal was read).
    assert_eq!(windows(&r)[1].0, T0 - 19_999);
    assert_eq!(
        w4_mismatches(&r),
        vec![(T0 + 5000, missing_fill("f1", "24.9 s"))]
    );
}

#[test]
fn an_empty_window_logs_once_per_state_change() {
    let (mut r, _) = rig("w4-cut");
    r.at(T0);
    r.at(T0 + 5000);
    // An empty sweep leaves the watermark where it is.
    assert_eq!(windows(&r)[1].0, T0 - 610_000);
    let cut = "longer than 7 days";
    assert_eq!(r.lines_with(cut), 0);
    let end = T0 + 7 * DAY;
    r.at(end);
    assert_eq!(windows(&r)[2], (end - 7 * DAY + 1, end, None));
    r.at(end + 5000);
    assert_eq!(r.lines_with(cut), 1);
    assert_eq!(windows(&r)[3].0, end + 5000 - 7 * DAY + 1);
    known(&mut r, "y", end + 6000);
    r.at(end + 10_000);
    r.at(end + 15_000);
    r.at(end + 20_000);
    assert_eq!(windows(&r)[5].0, end - 4000);
    assert_eq!(r.lines_with(cut), 1);
    assert_eq!(r.lines_with("7 days or shorter again"), 1);
}

#[test]
fn the_newest_agreeing_exec_time_is_kept_per_symbol() {
    let (mut r, times) = rig("w4-times");
    known(&mut r, "t1", T0 - 30_000);
    known(&mut r, "t2", T0 - 20_000);
    r.venue
        .executions
        .push(exec("ETHUSDT", "a", "AdlTrade", "Buy", "1", T0 - 25_000));
    rec(
        &mut r,
        T0 - 25_000,
        &[change("ETHUSDT", "a", "adl", "1", T0 - 25_000)],
    );
    // Newer, but not agreeing.
    r.venue.executions.push(trade("x", T0 - 10_000));
    r.venue
        .executions
        .push(exec("BTCUSDT", "f", "Funding", "Buy", "1", T0 - 5_000));
    r.at(T0);
    let want = [
        ("BTCUSDT".to_string(), T0 - 20_000),
        ("ETHUSDT".to_string(), T0 - 25_000),
    ];
    assert_eq!(times.lock().unwrap().clone(), want.into_iter().collect());
}

/// Sweeps every 5 s from `from` to `to` with W1 rounds between.
fn run(r: &mut Rig, from: u64, to: u64) {
    for t in (from..=to).step_by(1000) {
        r.at(t);
    }
}

#[test]
fn a_confirmed_row_is_not_sent_again_after_clear_hold_in_a_quiet_account() {
    let (mut r, _) = rig("w4-quiet");
    let x = T0 - 30_000;
    r.venue.executions.push(trade("x", x));
    run(&mut r, T0, T0 + 5000);
    assert_eq!(w4_mismatches(&r).len(), 1);
    assert!(r.held);
    assert_eq!(r.gauge("watcher_mismatch_active", "W4"), Some(1.0));
    r.clear(T0 + 5500);
    // No newer execution; 60 s pass.
    run(&mut r, T0 + 6000, T0 + 66_000);
    assert_eq!(w4_mismatches(&r).len(), 1);
    assert_eq!(r.gauge("watcher_mismatch_active", "W4"), Some(0.0));
    assert_eq!(r.gauge("watcher_differences", "W4"), Some(0.0));
    assert_eq!(r.gauge("watcher_mismatches_total", "W4"), Some(1.0));
    // Every sweep reads X: the watermark is X.
    let w = windows(&r);
    assert_eq!(w.len(), 14);
    assert!(w[2..].iter().all(|w| w.0 == x - 10_000), "{w:?}");
}

#[test]
fn a_dismissed_row_that_leaves_the_window_and_a_changed_value_are_sent_anew() {
    let (mut r, _) = rig("w4-anew");
    let x = T0 - 30_000;
    r.venue.executions.push(trade("x", x));
    run(&mut r, T0, T0 + 5000);
    r.clear(T0 + 5500);
    run(&mut r, T0 + 6000, T0 + 10_000);
    assert_eq!(w4_mismatches(&r).len(), 1);
    // A sweep no longer sees X: forgotten. Read again, it is new: two
    // sweeps, then sent anew.
    let row = r.venue.executions.pop().unwrap();
    run(&mut r, T0 + 11_000, T0 + 15_000);
    r.venue.executions.push(row);
    run(&mut r, T0 + 16_000, T0 + 20_000);
    assert_eq!(w4_mismatches(&r).len(), 1);
    run(&mut r, T0 + 21_000, T0 + 25_000);
    assert_eq!(
        w4_mismatches(&r)[1],
        (T0 + 25_000, missing_fill("x", "55.0 s"))
    );
    r.clear(T0 + 25_500);
    run(&mut r, T0 + 26_000, T0 + 35_000);
    assert_eq!(w4_mismatches(&r).len(), 2);
    // The venue changes X's quantity: another identity.
    r.venue.executions[0]["execQty"] = json!("2");
    run(&mut r, T0 + 36_000, T0 + 40_000);
    assert_eq!(w4_mismatches(&r).len(), 2);
    run(&mut r, T0 + 41_000, T0 + 45_000);
    let want = "W4 missing_fill BTCUSDT:x: venue Trade Buy 2, journal no fill; seen 2 sweeps, execTime 75.0 s old";
    assert_eq!(w4_mismatches(&r)[2], (T0 + 45_000, want.to_string()));
    assert_eq!(w4_mismatches(&r).len(), 3);
}

#[test]
fn a_hold_seen_at_a_cap_sampled_before_the_answer_does_not_dismiss() {
    let (mut r, _) = rig("w4-early-cap");
    r.oms = Oms::Silent;
    r.venue.executions.push(trade("x", T0 - 30_000));
    run(&mut r, T0, T0 + 5000);
    assert_eq!(w4_mismatches(&r).len(), 1);
    let send_id = *r.send_ids.last().unwrap();
    let detail = w4_mismatches(&r)[0].1.clone();
    // A W1 round asks for the status before the answer comes.
    let cap_before = r.journal.next_seq - 1;
    let acts = r.engine.poll(T0 + 6000);
    let watcher::engine::Action::Venue {
        req_id,
        path,
        params,
    } = &acts[0]
    else {
        panic!("{acts:?}");
    };
    let answer = r.venue.answer(path, params);
    let acts = r.engine.on_venue(T0 + 6000, *req_id, answer);
    let [watcher::engine::Action::Status { req_id: status_req }] = acts[..] else {
        panic!("{acts:?}");
    };
    // The answer "held" (the OMS wrote hold_set), then an operator clears
    // the hold; the status answer has the cap sampled before both.
    r.hold(Check::W4, &detail).unwrap();
    let more = r
        .engine
        .on_admin(T0 + 6000, Check::W4, send_id, Ok("held".into()));
    assert!(more.is_empty());
    r.clear(T0 + 6000);
    r.cap = Some(cap_before);
    let s = r.status();
    let more = r.engine.on_status(T0 + 6000, status_req, Ok(s));
    assert!(more.is_empty());
    assert!(!r.engine.state().holds.watcher_mismatch);
    assert_eq!(r.gauge("watcher_mismatch_active", "W4"), Some(1.0));
    // A cap sampled after the answer shows the clear: dismissed.
    r.cap = None;
    run(&mut r, T0 + 7000, T0 + 7000);
    assert_eq!(r.gauge("watcher_mismatch_active", "W4"), Some(0.0));
    run(&mut r, T0 + 8000, T0 + 30_000);
    assert_eq!(w4_mismatches(&r).len(), 1);
}

#[test]
fn a_row_confirmed_under_an_older_hold_is_sent_once_after_that_hold_is_cleared() {
    let (mut r, _) = rig("w4-older-hold");
    // A W1 difference sets the hold.
    r.venue.orders = vec![row("foreign-1", "o1", "BTCUSDT", "Buy")];
    run(&mut r, T0, T0 + 2000);
    assert_eq!(r.mismatches.len(), 1);
    assert_eq!(r.mismatches[0].1, Check::W1);
    r.venue.orders.clear();
    // A W4 row is confirmed in a round whose journal read shows that hold.
    r.venue
        .executions
        .push(exec("BTCUSDT", "a", "AdlTrade", "Buy", "1", T0 - 20_000));
    run(&mut r, T0 + 3000, T0 + 10_000);
    assert!(r.engine.state().holds.watcher_mismatch);
    assert_eq!(r.confirmed.len(), 2);
    assert!(w4_mismatches(&r).is_empty());
    r.clear(T0 + 10_500);
    run(&mut r, T0 + 11_000, T0 + 11_000);
    let want = "W4 missing_venue_change BTCUSDT:a: venue AdlTrade adl 1, journal no venue change; seen 2 sweeps, execTime 30.0 s old";
    assert_eq!(w4_mismatches(&r), vec![(T0 + 11_000, want.to_string())]);
    // Answered "held"; after a second clear it is dismissed.
    r.clear(T0 + 11_500);
    run(&mut r, T0 + 12_000, T0 + 40_000);
    assert_eq!(w4_mismatches(&r).len(), 1);
    assert_eq!(r.mismatches.len(), 2);
    assert_eq!(r.gauge("watcher_mismatch_active", "W4"), Some(0.0));
}

#[test]
fn a_young_difference_seen_twice_does_not_fail_the_sweep() {
    let mut cfg = config();
    cfg.rules.min_age_ms = 10_000;
    let (mut r, _) = rig_with("w4-young", &cfg);
    r.venue.executions.push(trade("x", T0 - 1000));
    r.at(T0);
    // Seen twice, 6 s old: not confirmed, and the sweep is a success.
    r.at(T0 + 5000);
    assert!(r.confirmed.is_empty());
    assert_eq!(r.engine.success(Check::W4), Some(T0 + 5000));
    assert_eq!(r.gauge("watcher_differences", "W4"), Some(1.0));
    // The watermark did not pass it: read again, and confirmed.
    r.at(T0 + 10_000);
    assert_eq!(
        w4_mismatches(&r),
        vec![(T0 + 10_000, missing_fill("x", "11.0 s"))]
    );
}

#[test]
fn a_trade_at_the_floor_itself_is_compared() {
    let (mut r, _) = rig("w4-floor-equal");
    let floor_us = (T0 - 20_000) * 1000;
    rec(
        &mut r,
        T0 - 30_000,
        &[ev("booted", json!({"fill_floor": floor_us}))],
    );
    r.venue.executions.push(trade("f", T0 - 20_000));
    r.at(T0);
    r.at(T0 + 5000);
    assert_eq!(windows(&r)[1].0, T0 - 20_000);
    assert_eq!(
        w4_mismatches(&r),
        vec![(T0 + 5000, missing_fill("f", "25.0 s"))]
    );
}

#[test]
fn a_round_that_reads_a_page_ends_the_run_of_failed_rounds() {
    let (mut r, _) = rig_with("w4-failed-run", &one_page());
    let fail = || Err(RestError::NoAnswer("timeout".into()));
    let s = &mut r.venue.exec_script;
    s.push_back(page(vec![trade("a", T0 - 9000)], "c1"));
    s.push_back(fail());
    s.push_back(fail());
    s.push_back(page(vec![trade("b", T0 - 8000)], "c2"));
    s.push_back(fail());
    for k in 0..6 {
        r.at(T0 + k * 5000);
    }
    // Failed, failed, read, failed: no run of 3, so the sweep goes on.
    let cursors: Vec<Option<String>> = windows(&r).into_iter().map(|w| w.2).collect();
    let c = |s: &str| Some(s.to_string());
    assert_eq!(
        cursors,
        vec![None, c("c1"), c("c1"), c("c1"), c("c2"), c("c2")]
    );
    assert_eq!(r.lines_with("rounds in a row; the sweep restarts"), 0);
}
