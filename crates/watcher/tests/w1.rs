//! W1 open orders and the two-round rule (C-WATCH part 5, U19a5), on the
//! engine rig: the model venue, the test journal and a manual clock. W1
//! rounds are every 1 s, `min_age_ms` is 2 s.

#![allow(clippy::unwrap_used)]

mod common;

use common::rig::*;
use watcher::engine::{Action, Check};
use watcher::w1::PATH;

const CLOSED: &str = "venue open, journal closed";

/// The rounds at `T0 + k s` for each `k`.
fn rounds(r: &mut Rig, ks: std::ops::Range<u64>) {
    for k in ks {
        r.at(T0 + k * 1000);
    }
}

fn detail(kind: &str, subject: &str, what: &str, age: &str) -> String {
    format!("W1 {kind} {subject}: {what}; seen 2 rounds, {age}")
}

#[test]
fn a_one_round_difference_does_not_alarm() {
    let mut r = Rig::new("w1-one-round");
    r.write(T0 - 10_000, &open(1));
    r.write(T0 - 9_000, &[of("order_closed", 1)]);
    r.venue.orders = vec![
        ours(1, "BTCUSDT", "Buy"),
        row("x", "v900", "BTCUSDT", "Buy"),
    ];
    r.at(T0);
    // Two first sightings: pending, and the round succeeds.
    assert_eq!(r.gauge("watcher_differences", "W1"), Some(2.0));
    assert_eq!(r.engine.success(Check::W1), Some(T0));
    r.venue.orders.clear();
    rounds(&mut r, 1..3);
    assert_eq!(r.gauge("watcher_differences", "W1"), Some(0.0));
    // Seen, not seen, seen: each sighting is a first one again.
    for k in 3..9 {
        r.venue.orders = if k % 2 == 1 {
            vec![ours(1, "BTCUSDT", "Buy")]
        } else {
            vec![]
        };
        r.at(T0 + k * 1000);
    }
    assert!(r.mismatches.is_empty(), "{:?}", r.mismatches);
    assert_eq!(r.gauge("watcher_mismatches_total", "W1"), None);
    assert_eq!(r.engine.success(Check::W1), Some(T0 + 8000));
}

#[test]
fn a_two_round_difference_older_than_2_s_alarms() {
    let mut r = Rig::new("w1-two-rounds");
    r.write(T0 - 10_000, &open(1));
    // Young at the first round (1.5 s), old at the second (2.5 s): the
    // age is judged at the confirming round.
    r.write(T0 - 1_500, &[of("order_closed", 1)]);
    r.venue.orders = vec![ours(1, "BTCUSDT", "Buy")];
    r.at(T0);
    assert!(r.mismatches.is_empty());
    r.at(T0 + 1000);
    let want = detail(
        "closed_in_journal",
        &link(1),
        CLOSED,
        "journal entry 2.5 s old",
    );
    assert_eq!(r.mismatches, vec![(T0 + 1000, Check::W1, want.clone())]);
    assert_eq!(r.gauge("watcher_mismatches_total", "W1"), Some(1.0));
    assert_eq!(r.gauge("watcher_differences", "W1"), Some(0.0));
    assert_eq!(
        r.lines_with(&format!("error: confirmed difference: {want}")),
        1
    );
    assert_eq!(r.engine.success(Check::W1), Some(T0 + 1000));
}

#[test]
fn a_journal_entry_younger_than_2_s_does_not_alarm() {
    let mut r = Rig::new("w1-young");
    let orders = [&open(1)[..], &open(2)[..], &[accepted(3, "BTCUSDT", "buy")]];
    r.write(T0 - 10_000, &orders.concat());
    // Order 2 closed at T0, order 1 a microsecond later; order 3, not at
    // the venue, acked a microsecond later too (`missing_at_venue`).
    r.write(T0, &[of("order_closed", 2)]);
    r.journal
        .append(T0 * 1000 + 1, &[of("order_closed", 1), acked(3)]);
    r.venue.orders = vec![ours(1, "BTCUSDT", "Buy"), ours(2, "BTCUSDT", "Buy")];
    rounds(&mut r, 0..2);
    assert!(r.mismatches.is_empty());
    // At T0 + 2 s order 2 is exactly 2 s old; orders 1 and 3 are not yet.
    r.at(T0 + 2000);
    let old = "journal entry 2.0 s old";
    let want = detail("closed_in_journal", &link(2), CLOSED, old);
    assert_eq!(r.mismatches, vec![(T0 + 2000, Check::W1, want)]);
    assert_eq!(r.engine.success(Check::W1), Some(T0));
    r.at(T0 + 3000);
    let want = detail(
        "closed_in_journal",
        &link(1),
        CLOSED,
        "journal entry 2.9 s old",
    );
    let missing = detail(
        "missing_at_venue",
        &link(3),
        "venue not open, journal open and acked",
        "journal entry 2.9 s old",
    );
    // Confirmed at T0 + 3 s; their send waits for the hold the first send
    // set (U19a7).
    assert_eq!(r.confirmed[1..], [(T0 + 3000, want), (T0 + 3000, missing)]);
    assert_eq!(r.mismatches.len(), 1);
}

#[test]
fn an_unacked_order_missing_at_the_venue_is_not_compared() {
    let mut r = Rig::new("w1-unacked");
    r.write(T0 - 10_000, &[accepted(1, "BTCUSDT", "buy")]);
    r.write(
        T0 - 10_000,
        &[&open(2)[..], &[of("order_in_doubt", 2)]].concat(),
    );
    r.write(T0 - 10_000, &open(3));
    rounds(&mut r, 0..5);
    let what = "venue not open, journal open and acked";
    let want = detail(
        "missing_at_venue",
        &link(3),
        what,
        "journal entry 11.0 s old",
    );
    assert_eq!(r.mismatches, vec![(T0 + 1000, Check::W1, want)]);
    assert_eq!(r.gauge("watcher_differences", "W1"), Some(0.0));
    assert_eq!(r.gauge("watcher_mismatch_active", "W1"), Some(1.0));
    assert_eq!(r.engine.success(Check::W1), Some(T0 + 4000));
}

#[test]
fn a_foreign_order_at_the_venue_is_a_mismatch() {
    let mut r = Rig::new("w1-foreign");
    r.write(T0 - 10_000, &open(1));
    let other = watcher::wire_id::encode(8, 1);
    let unknown = link(99);
    r.venue.orders = vec![
        ours(1, "BTCUSDT", "Buy"),
        row("", "v901", "BTCUSDT", "Buy"),
        row("manual-1", "v902", "ETHUSDT", "Sell"),
        row(&other, "v903", "BTCUSDT", "Buy"),
        row(&unknown, "v904", "BTCUSDT", "Buy"),
    ];
    rounds(&mut r, 0..2);
    // Unknown to the journal: the age is the first sighting's, 1 s here.
    assert!(r.mismatches.is_empty());
    assert_eq!(r.engine.success(Check::W1), Some(T0));
    assert_eq!(r.gauge("watcher_differences", "W1"), Some(4.0));
    r.at(T0 + 2000);
    let what = "venue BTCUSDT Buy orderLinkId \"\", journal no such order";
    let first = detail("foreign", "v901", what, "first seen 2.0 s ago");
    assert_eq!(
        r.mismatches,
        vec![(T0 + 2000, Check::W1, format!("{first}; and 3 more"))]
    );
    assert_eq!(r.gauge("watcher_mismatches_total", "W1"), Some(4.0));
    for s in ["v902", "v903", "v904"] {
        assert_eq!(r.lines_with(&format!("W1 foreign {s}: venue")), 1, "{s}");
    }
}

#[test]
fn an_order_closed_in_the_journal_and_open_at_the_venue_is_a_mismatch() {
    let mut r = Rig::new("w1-closed");
    r.write(
        T0 - 10_000,
        &[&open(1)[..], &open(2)[..], &open(3)[..]].concat(),
    );
    r.write(T0 - 10_000, &[of("order_closed", 1)]);
    r.venue.orders = vec![
        ours(1, "BTCUSDT", "Buy"),
        ours(2, "ETHUSDT", "Buy"),
        ours(3, "BTCUSDT", "Sell"),
    ];
    rounds(&mut r, 0..2);
    let age = "journal entry 11.0 s old";
    let first = detail("closed_in_journal", &link(1), CLOSED, age);
    let want = format!("{first}; and 2 more");
    assert_eq!(r.mismatches, vec![(T0 + 1000, Check::W1, want)]);
    for (id, venue) in [(2, "ETHUSDT Buy"), (3, "BTCUSDT Sell")] {
        let what = format!("venue {venue}, journal BTCUSDT Buy");
        let line = detail("details", &link(id), &what, age);
        assert_eq!(r.lines_with(&line), 1, "{line}");
    }
}

#[test]
fn the_venue_is_read_before_the_journal() {
    let mut r = Rig::new("w1-venue-first");
    let a = r.engine.poll(T0);
    let Some(Action::Venue {
        req_id,
        path,
        params,
    }) = a.first().cloned()
    else {
        panic!("{a:?}");
    };
    assert_eq!(path, PATH);
    assert!(!a.iter().any(|x| matches!(x, Action::Status { .. })));
    // The OMS journals 51 orders and sends them while the venue is read.
    for id in 1..=51 {
        r.write(T0, &open(id));
        r.venue.orders.push(ours(id, "BTCUSDT", "Buy"));
    }
    let answer = r.venue.answer(path, &params);
    let a = r.engine.on_venue(T0, req_id, answer);
    // A second page first: no status request before the last venue answer.
    let [
        Action::Venue {
            req_id,
            path,
            params,
        },
    ] = &a[..]
    else {
        panic!("{a:?}");
    };
    assert_eq!(params.last(), Some(&("cursor", "p50".to_string())));
    let answer = r.venue.answer(path, params);
    let a = r.engine.on_venue(T0, *req_id, answer);
    assert!(matches!(a[..], [Action::Status { .. }]), "{a:?}");
    r.run(T0, a);
    assert_eq!(r.engine.state().last_seq, 51);
    assert_eq!(r.gauge("watcher_differences", "W1"), Some(0.0));
    assert_eq!(r.engine.success(Check::W1), Some(T0));
}

#[test]
fn a_difference_whose_order_keeps_receiving_events_is_confirmed_by_its_fact_age() {
    let mut r = Rig::new("w1-fact-age");
    r.write(T0 - 10_000, &open(1));
    r.write(T0 - 3_000, &[of("order_closed", 1)]);
    r.venue.orders = vec![ours(1, "BTCUSDT", "Buy")];
    // A fill after the close every 250 ms: later events of the order that
    // do not change what W1 compares.
    let mut t = T0 - 2_750;
    let mut fills = |r: &mut Rig, until: u64| {
        while t <= until {
            let f = serde_json::json!({"key": format!("BTCUSDT:{t}"), "order_id": 1,
                "qty": "0.1", "venue_ts": t * 1000});
            r.write(t, &[common::ev("fill_booked", f)]);
            t += 250;
        }
    };
    fills(&mut r, T0);
    r.at(T0);
    fills(&mut r, T0 + 1000);
    r.at(T0 + 1000);
    let want = detail(
        "closed_in_journal",
        &link(1),
        CLOSED,
        "journal entry 4.0 s old",
    );
    assert_eq!(r.mismatches, vec![(T0 + 1000, Check::W1, want)]);
}

#[test]
fn a_young_difference_seen_twice_defers_the_round() {
    let mut r = Rig::new("w1-defer");
    r.write(T0 - 10_000, &open(1));
    r.write(T0 - 500, &[of("order_closed", 1)]);
    r.venue.orders = vec![ours(1, "BTCUSDT", "Buy")];
    r.at(T0);
    assert_eq!(r.engine.success(Check::W1), Some(T0));
    r.at(T0 + 1000);
    // Seen twice, 1.5 s old: deferred, not a success.
    assert_eq!(r.engine.success(Check::W1), Some(T0));
    assert_eq!(r.gauge("watcher_differences", "W1"), Some(1.0));
    assert!(r.mismatches.is_empty());
    r.at(T0 + 1500);
    assert_eq!(r.gauge("watcher_check_age_s", "W1"), Some(1.5));
    r.at(T0 + 2000);
    assert_eq!(r.engine.success(Check::W1), Some(T0 + 2000));
    assert_eq!(r.mismatches.len(), 1);
    // The heartbeat carries the last success, not the deferred round.
    r.venue.orders.clear();
    r.write(T0 + 2100, &open(2));
    r.write(T0 + 2100, &[of("order_closed", 2)]);
    r.venue.orders = vec![ours(2, "BTCUSDT", "Buy")];
    rounds(&mut r, 3..6);
    let hb = r.heartbeats.last().unwrap();
    assert_eq!(hb.0, T0 + 5000);
    assert_eq!(hb.1[0], (Check::W1, Some(T0 + 3000)));
}

#[test]
fn a_full_page_without_a_cursor_or_an_unchanged_cursor_fails_the_round() {
    let mut r = Rig::new("w1-paging");
    let rows = |n: usize| -> Vec<serde_json::Value> {
        (0..n)
            .map(|i| row("", &format!("v{i}"), "BTCUSDT", "Buy"))
            .collect()
    };
    let unreadable = |r: &Rig| {
        let l = [("endpoint", PATH), ("class", "unreadable")];
        r.metrics.get("watcher_request_errors_total", &l)
    };
    // A full page without a cursor may not be the last.
    r.venue.script.push_back(page(rows(50), ""));
    r.at(T0);
    assert_eq!(r.engine.success(Check::W1), None);
    assert_eq!(unreadable(&r), Some(1.0));
    assert_eq!(
        r.lines_with("unreadable: a full page without nextPageCursor"),
        1
    );
    // The cursor just sent comes back.
    r.venue
        .script
        .extend([page(rows(1), "c1"), page(rows(1), "c1")]);
    r.at(T0 + 1000);
    assert_eq!(
        r.venue.requests[2].1.last(),
        Some(&("cursor", "c1".to_string()))
    );
    assert_eq!(
        r.lines_with("unreadable: nextPageCursor is the cursor just sent"),
        1
    );
    // A row without orderId; a list that is not a list.
    r.venue
        .script
        .push_back(Ok(serde_json::json!({"list": [{"orderLinkId": "",
        "symbol": "BTCUSDT", "side": "Buy", "qty": "1", "orderStatus": "New"}],
        "nextPageCursor": ""})));
    r.at(T0 + 2000);
    r.venue
        .script
        .push_back(Ok(serde_json::json!({"list": {}, "nextPageCursor": ""})));
    r.at(T0 + 3000);
    assert_eq!(unreadable(&r), Some(4.0));
    assert_eq!(r.engine.success(Check::W1), None);
    // 21 pages: the 20th still has a cursor.
    let before = r.venue.requests.len();
    for i in 0..21 {
        r.venue.script.push_back(page(vec![], &format!("k{i}")));
    }
    r.at(T0 + 4000);
    assert_eq!(r.venue.requests.len() - before, 20);
    assert_eq!(
        r.lines_with("round failed: /v5/order/realtime: more than 1000 open orders"),
        1
    );
    r.venue.script.clear();
    // An empty page with a cursor is followed; a short one without ends it.
    r.venue
        .script
        .extend([page(vec![], "c2"), page(rows(3), "")]);
    r.at(T0 + 5000);
    assert_eq!(r.engine.success(Check::W1), Some(T0 + 5000));
    assert_eq!(r.gauge("watcher_differences", "W1"), Some(3.0));
    assert_eq!(unreadable(&r), Some(4.0));
}

#[test]
fn an_order_ended_at_the_venue_in_the_journal_and_not_listed_agrees() {
    let mut r = Rig::new("w1-terminal-agrees");
    r.write(T0 - 10_000, &open(1));
    r.write(T0 - 9_000, &[of("venue_terminal", 1)]);
    rounds(&mut r, 0..4);
    assert!(r.mismatches.is_empty());
    assert_eq!(r.gauge("watcher_differences", "W1"), Some(0.0));
    assert_eq!(r.engine.success(Check::W1), Some(T0 + 3000));
    assert!(r.engine.state().open.contains(&1));
}

#[test]
fn an_order_ended_in_the_journal_and_still_open_at_the_venue_is_open_after_terminal() {
    let mut r = Rig::new("w1-open-after-terminal");
    r.write(T0 - 10_000, &open(1));
    r.write(T0 - 500, &[of("venue_terminal", 1)]);
    r.venue.orders = vec![ours(1, "BTCUSDT", "Buy")];
    rounds(&mut r, 0..2);
    assert!(r.mismatches.is_empty());
    assert_eq!(r.engine.success(Check::W1), Some(T0));
    r.at(T0 + 2000);
    let what = "venue open, journal ended at the venue (venue_terminal)";
    let want = detail(
        "open_after_terminal",
        &link(1),
        what,
        "journal entry 2.5 s old",
    );
    assert_eq!(r.mismatches, vec![(T0 + 2000, Check::W1, want)]);
}

#[test]
fn an_order_listed_on_two_pages_is_one_difference() {
    let mut r = Rig::new("w1-two-pages");
    for id in 1..=49 {
        r.write(T0 - 10_000, &open(id));
    }
    // Orders move between pages while W1 pages: the foreign order is the
    // first row of page 1 and the row of page 2.
    let foreign = row("x", "v900", "BTCUSDT", "Buy");
    r.venue.orders = vec![foreign.clone()];
    r.venue
        .orders
        .extend((1..=49).map(|id| ours(id, "BTCUSDT", "Buy")));
    r.venue.orders.push(foreign);
    r.at(T0);
    assert_eq!(r.venue.requests.len(), 2);
    assert_eq!(r.gauge("watcher_differences", "W1"), Some(1.0));
    rounds(&mut r, 1..3);
    let what = "venue BTCUSDT Buy orderLinkId \"x\", journal no such order";
    let want = detail("foreign", "v900", what, "first seen 2.0 s ago");
    assert_eq!(r.mismatches, vec![(T0 + 2000, Check::W1, want)]);
    assert_eq!(r.gauge("watcher_mismatches_total", "W1"), Some(1.0));
    assert_eq!(r.gauge("watcher_mismatch_active", "W1"), Some(1.0));
    assert_eq!(r.gauge("watcher_differences", "W1"), Some(0.0));
}

/// `hold_set` or `hold_cleared` of the kill latch, as the OMS writes them.
fn kill(set: bool) -> serde_json::Value {
    if set {
        common::ev(
            "hold_set",
            serde_json::json!({"hold": "kill", "source": "stream", "note": "private stream down"}),
        )
    } else {
        common::ev(
            "hold_cleared",
            serde_json::json!({"hold": "kill", "note": "venue checked"}),
        )
    }
}

const MISSING: &str = "venue not open, journal open and acked";

#[test]
fn a_missing_order_during_a_kill_is_not_reported() {
    let mut r = Rig::new("w1-kill-missing");
    // Two acked orders; the OMS latches its kill and its cancel-all empties
    // the venue, but it journals no close while its link is down.
    r.write(T0 - 10_000, &[&open(1)[..], &open(2)[..]].concat());
    r.write(T0 - 5_000, &[kill(true)]);
    rounds(&mut r, 0..6);
    assert!(r.mismatches.is_empty(), "{:?}", r.mismatches);
    assert!(r.confirmed.is_empty(), "{:?}", r.confirmed);
    assert_eq!(r.gauge("watcher_differences", "W1"), Some(0.0));
    assert_eq!(r.engine.success(Check::W1), Some(T0 + 5000));
}

#[test]
fn a_missing_order_is_reported_again_after_the_kill_is_cleared() {
    let mut r = Rig::new("w1-kill-cleared");
    r.write(T0 - 10_000, &open(1));
    r.write(T0 - 5_000, &[kill(true)]);
    rounds(&mut r, 0..3);
    assert!(r.mismatches.is_empty(), "{:?}", r.mismatches);
    // The latch is released while the journal still holds the order open
    // (the OMS refuses that, decide.rs; the watcher does not rely on it):
    // the difference is a first sighting again, confirmed one round later.
    r.write(T0 + 3_000, &[kill(false)]);
    r.at(T0 + 3_000);
    assert!(r.mismatches.is_empty(), "{:?}", r.mismatches);
    assert_eq!(r.gauge("watcher_differences", "W1"), Some(1.0));
    r.at(T0 + 4_000);
    let want = detail("missing_at_venue", &link(1), MISSING, "journal entry 14.0 s old");
    assert_eq!(r.mismatches, vec![(T0 + 4000, Check::W1, want)]);
}

#[test]
fn a_foreign_order_during_a_kill_is_still_reported() {
    let mut r = Rig::new("w1-kill-foreign");
    r.write(T0 - 10_000, &open(1));
    r.write(T0 - 5_000, &[kill(true)]);
    // Order 1 is gone (the kill's cancel-all); an order of another key
    // appears at the venue: still a difference during the kill.
    r.venue.orders = vec![row("manual-1", "v900", "BTCUSDT", "Buy")];
    rounds(&mut r, 0..3);
    let what = "venue BTCUSDT Buy orderLinkId \"manual-1\", journal no such order";
    let want = detail("foreign", "v900", what, "first seen 2.0 s ago");
    assert_eq!(r.mismatches, vec![(T0 + 2000, Check::W1, want)]);
    assert_eq!(r.gauge("watcher_mismatches_total", "W1"), Some(1.0));
}

#[test]
fn the_venue_side_kinds_during_a_kill_are_still_reported() {
    let mut r = Rig::new("w1-kill-venue-side");
    r.write(
        T0 - 10_000,
        &[&open(1)[..], &open(2)[..], &open(3)[..]].concat(),
    );
    r.write(
        T0 - 10_000,
        &[of("order_closed", 1), of("venue_terminal", 2)],
    );
    r.write(T0 - 5_000, &[kill(true)]);
    // Under the latch the venue still lists three of our orders: one closed
    // in the journal, one ended at the venue, one with another symbol. The
    // kill exemption covers only missing_at_venue.
    r.venue.orders = vec![
        ours(1, "BTCUSDT", "Buy"),
        ours(2, "BTCUSDT", "Buy"),
        ours(3, "ETHUSDT", "Buy"),
    ];
    rounds(&mut r, 0..2);
    let age = "journal entry 11.0 s old";
    let first = detail("closed_in_journal", &link(1), CLOSED, age);
    let want = format!("{first}; and 2 more");
    assert_eq!(r.mismatches, vec![(T0 + 1000, Check::W1, want)]);
    let terminal = "venue open, journal ended at the venue (venue_terminal)";
    let details = "venue ETHUSDT Buy, journal BTCUSDT Buy";
    for line in [
        detail("open_after_terminal", &link(2), terminal, age),
        detail("details", &link(3), details, age),
    ] {
        assert_eq!(r.lines_with(&line), 1, "{line}");
    }
}
