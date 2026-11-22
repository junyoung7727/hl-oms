//! The watcher's journal state (C-WATCH part 2): every rule is a row of the
//! C-TAIL table "이벤트가 바꾸는 상태" (`docs/contracts/tail.md`). The
//! records are built in memory as the reader returns them.

#![allow(clippy::unwrap_used)]

use serde_json::{Value, json};
use watcher::decimal::Dec;
use watcher::journal::{JournalError, Record};
use watcher::state::{JournalState, Side};

fn d(s: &str) -> Dec {
    Dec::parse(s).unwrap()
}

/// Appends records to a state, numbered from 1, and keeps the record time.
struct S {
    st: JournalState,
    seq: u64,
    at: u64,
}

impl S {
    fn new() -> S {
        S {
            st: JournalState::default(),
            seq: 0,
            at: 1_000_000,
        }
    }

    fn rec(&self, events: &[(&str, Value)]) -> Record {
        Record {
            seq: self.seq + 1,
            at: self.at,
            record_schema: 2,
            events: events
                .iter()
                .map(|(n, f)| (n.to_string(), f.clone()))
                .collect(),
        }
    }

    /// One record at `at` + 1 s; returns its record time.
    fn try_apply(&mut self, events: &[(&str, Value)]) -> Result<u64, JournalError> {
        self.at += 1_000_000;
        let r = self.rec(events);
        self.st.apply(&r)?;
        self.seq += 1;
        Ok(r.at)
    }

    fn apply(&mut self, events: &[(&str, Value)]) -> u64 {
        self.try_apply(events).unwrap()
    }

    fn refused(&mut self, events: &[(&str, Value)]) -> String {
        match self.try_apply(events) {
            Err(JournalError::State { seq, why }) => {
                assert_eq!(seq, self.seq + 1);
                why
            }
            other => panic!("expected a state error, got {other:?}"),
        }
    }
}

fn accepted(id: u64, side: &str, qty: &str) -> (&'static str, Value) {
    (
        "order_accepted",
        json!({
            "config_version": 1,
            "order_id": id,
            "ref_px": null,
            "request": {
                "client_order_id": format!("k{id}"),
                "expires_at": 9,
                "instrument": "BTCUSDT",
                "px": "100",
                "qty": qty,
                "reduce_only": false,
                "side": side,
                "strategy": "a",
                "tif": "gtc",
            },
        }),
    )
}

fn acked(id: u64, venue_oid: Value) -> (&'static str, Value) {
    (
        "order_acked",
        json!({"order_id": id, "session": 1, "venue_oid": venue_oid, "via": "stream"}),
    )
}

fn fill(id: u64, key: &str, qty: &str, venue_ts: u64) -> (&'static str, Value) {
    (
        "fill_booked",
        json!({"fee": "0", "key": key, "order_id": id, "px": "100", "qty": qty, "venue_ts": venue_ts}),
    )
}

fn external(
    key: &str,
    instrument: &str,
    side: &str,
    qty: &str,
    venue_ts: u64,
) -> (&'static str, Value) {
    (
        "external_fill_booked",
        json!({"fill": {
            "fee": "0", "instrument": instrument, "key": key, "order_id": null, "px": "100",
            "qty": qty, "side": side, "venue_oid": null, "venue_ts": venue_ts,
        }}),
    )
}

fn closed(id: u64) -> (&'static str, Value) {
    ("order_closed", json!({"order_id": id, "reason": "filled"}))
}

fn change(
    key: &str,
    instrument: &str,
    qty: &str,
    kind: &str,
    venue_ts: u64,
) -> (&'static str, Value) {
    (
        "venue_position_changed",
        json!({"change": {
            "instrument": instrument, "key": key, "kind": kind, "px": "100", "qty": qty,
            "venue_ts": venue_ts,
        }}),
    )
}

fn adjusted(instrument: &str, delta: &str) -> (&'static str, Value) {
    (
        "position_adjusted",
        json!({"delta": delta, "instrument": instrument, "ledger": "unattributed", "note": "n"}),
    )
}

fn funding(key: &str, instrument: &str, venue_ts: u64) -> (&'static str, Value) {
    (
        "funding_booked",
        json!({"funding": {
            "amount": "-1.5", "instrument": instrument, "key": key, "rate": "0.0001",
            "venue_ts": venue_ts,
        }}),
    )
}

fn terminal(id: u64) -> (&'static str, Value) {
    (
        "venue_terminal",
        json!({"final_cum": "0", "kind": "canceled", "order_id": id, "reason": "r", "venue_ts": 5}),
    )
}

fn purged(ids: &[u64], fill_floor: u64) -> (&'static str, Value) {
    (
        "purged",
        json!({"cutoff": 0, "fill_floor": fill_floor, "order_ids": ids}),
    )
}

fn booted(fill_floor: u64) -> (&'static str, Value) {
    (
        "booted",
        json!({"binary": "b", "boot": 2, "fill_floor": fill_floor}),
    )
}

fn other(name: &'static str, fields: Value) -> (&'static str, Value) {
    (name, fields)
}

#[test]
fn the_sets_follow_the_field_table() {
    let mut s = S::new();
    let t1 = s.apply(&[accepted(1, "buy", "2"), accepted(2, "sell", "1.5")]);
    let o = &s.st.orders[&1];
    assert_eq!(
        (o.strategy.as_str(), o.instrument.as_str(), o.side, o.qty),
        ("a", "BTCUSDT", Side::Buy, d("2"))
    );
    assert_eq!(o.client_order_id, "k1");
    assert_eq!(
        (
            o.venue_oid.clone(),
            o.filled,
            o.open,
            o.acked,
            o.in_doubt,
            o.terminal,
            o.fact_at
        ),
        (None, Dec::ZERO, true, false, false, false, t1)
    );
    assert_eq!(s.st.open.iter().copied().collect::<Vec<_>>(), vec![1, 2]);

    // Ack with and without a venue order id; a venue id learned later.
    s.apply(&[acked(1, json!("v1")), acked(2, Value::Null)]);
    assert_eq!(s.st.orders[&1].venue_oid.as_deref(), Some("v1"));
    assert!(s.st.orders[&1].acked && s.st.orders[&2].acked);
    assert_eq!(s.st.orders[&2].venue_oid, None);
    s.apply(&[other(
        "venue_oid_learned",
        json!({"order_id": 2, "venue_oid": "v2"}),
    )]);
    assert_eq!(s.st.orders[&2].venue_oid.as_deref(), Some("v2"));
    // An ack without a venue order id keeps the known one (review 1 N3).
    s.apply(&[acked(2, Value::Null)]);
    assert_eq!(s.st.orders[&2].venue_oid.as_deref(), Some("v2"));

    // In doubt (cleared by an ack or venue_terminal, see the tests below).
    s.apply(&[other(
        "order_in_doubt",
        json!({"detail": "d", "order_id": 2, "reason": "transport"}),
    )]);
    assert!(s.st.orders[&2].in_doubt);
    // An ack clears the doubt (an order in doubt before its ack).
    s.apply(&[accepted(3, "buy", "1")]);
    s.apply(&[other(
        "order_in_doubt",
        json!({"detail": "d", "order_id": 3, "reason": "transport"}),
    )]);
    assert!(s.st.orders[&3].in_doubt);
    s.apply(&[acked(3, json!("v3"))]);
    assert!(!s.st.orders[&3].in_doubt && s.st.orders[&3].acked);

    // Fills: the order's side and instrument; buy +, sell −.
    let tf = s.apply(&[
        fill(1, "BTCUSDT:e1", "0.5", 10),
        fill(2, "BTCUSDT:e2", "1.5", 11),
    ]);
    assert_eq!(s.st.orders[&1].filled, d("0.5"));
    let f = &s.st.fills["BTCUSDT:e2"];
    assert_eq!(
        (f.qty, f.side, f.instrument.as_str(), f.venue_ts),
        (d("1.5"), Side::Sell, "BTCUSDT", 11)
    );
    assert_eq!(s.st.position("BTCUSDT"), d("-1"));
    assert_eq!(s.st.last_change_at.get("BTCUSDT"), Some(&tf));

    // External fill on another instrument.
    s.apply(&[external("ETHUSDT:e3", "ETHUSDT", "sell", "3", 12)]);
    assert_eq!(s.st.position("ETHUSDT"), d("-3"));
    let f = &s.st.fills["ETHUSDT:e3"];
    assert_eq!((f.qty, f.side, f.venue_ts), (d("3"), Side::Sell, 12));

    // Venue change (already signed) and an adjustment.
    s.apply(&[change("ETHUSDT:e4", "ETHUSDT", "1.25", "adl", 13)]);
    assert_eq!(s.st.position("ETHUSDT"), d("-1.75"));
    let c = &s.st.venue_changes["ETHUSDT:e4"];
    assert_eq!(
        (c.instrument.as_str(), c.qty, c.kind.as_str(), c.venue_ts),
        ("ETHUSDT", d("1.25"), "adl", 13)
    );
    s.apply(&[adjusted("ETHUSDT", "-0.25")]);
    assert_eq!(s.st.position("ETHUSDT"), d("-2"));

    // Close: out of the open set, the info stays.
    s.apply(&[closed(2)]);
    assert_eq!(s.st.open.iter().copied().collect::<Vec<_>>(), vec![1, 3]);
    assert!(!s.st.orders[&2].open);
    // A close is not an end at the venue (review 1 N3).
    assert!(!s.st.orders[&2].terminal);

    // Holds and the day base.
    s.apply(&[other(
        "hold_set",
        json!({"hold": "watcher_mismatch", "source": "W1", "note": "n"}),
    )]);
    assert!(s.st.holds.watcher_mismatch && !s.st.holds.kill);
    s.apply(&[other(
        "hold_set",
        json!({"hold": "kill", "source": "stream", "note": "n"}),
    )]);
    assert!(s.st.holds.watcher_mismatch && s.st.holds.kill);
    s.apply(&[other(
        "hold_cleared",
        json!({"hold": "venue_change", "note": "n"}),
    )]);
    assert!(s.st.holds.watcher_mismatch && s.st.holds.kill);
    s.apply(&[other(
        "hold_cleared",
        json!({"hold": "watcher_mismatch", "note": "n"}),
    )]);
    assert!(!s.st.holds.watcher_mismatch && s.st.holds.kill);
    s.apply(&[other("hold_cleared", json!({"hold": "kill", "note": "n"}))]);
    assert!(!s.st.holds.watcher_mismatch && !s.st.holds.kill);
    assert_eq!(s.st.day_base, None);
    s.apply(&[other(
        "day_base_set",
        json!({"day": 20_000, "set_by": "report", "value": "12600.5"}),
    )]);
    let b = s.st.day_base.unwrap();
    assert_eq!((b.day, b.value), (20_000, d("12600.5")));

    // Events of no meaning for the sets change nothing but the last record.
    let before = s.st.clone();
    let tn = s.apply(&[
        other("storage_low_changed", json!({"low": true, "free_bytes": 1})),
        other(
            "ref_price_updated",
            json!({"instrument": "SOLUSDT", "px": "1", "venue_ts": 1}),
        ),
        other("cancel_requested", json!({"by": "operator", "order_id": 1})),
        other(
            "status_noted",
            json!({"order_id": 1, "reported_cum": "1", "session": 1, "venue_ts": 1}),
        ),
    ]);
    let mut expected = before;
    expected.last_seq = s.seq;
    expected.last_at = tn;
    assert_eq!(s.st, expected);
    assert_eq!((s.st.last_seq, s.st.last_at), (s.seq, tn));
}

#[test]
fn a_fill_after_close_uses_the_kept_order_info() {
    let mut s = S::new();
    s.apply(&[accepted(1, "sell", "1")]);
    s.apply(&[acked(1, json!("v1"))]);
    s.apply(&[closed(1)]);
    assert!(s.st.open.is_empty());
    // The late fill: the side and the instrument come from the kept info.
    s.apply(&[fill(1, "BTCUSDT:late", "1", 20)]);
    assert_eq!(s.st.position("BTCUSDT"), d("-1"));
    assert_eq!(s.st.fills["BTCUSDT:late"].side, Side::Sell);
    assert_eq!(s.st.orders[&1].filled, d("1"));
    assert!(s.st.open.is_empty());
    // An order the journal never accepted: the side cannot be known.
    let why = s.refused(&[fill(9, "BTCUSDT:x", "1", 21)]);
    assert!(why.contains("order 9"), "{why}");
}

#[test]
fn purged_forgets_closed_orders_and_old_keys() {
    let mut s = S::new();
    s.apply(&[accepted(1, "buy", "1"), accepted(2, "buy", "1")]);
    s.apply(&[fill(1, "BTCUSDT:a", "1", 100), closed(1)]);
    s.apply(&[
        external("BTCUSDT:b", "BTCUSDT", "buy", "1", 199),
        change("BTCUSDT:c", "BTCUSDT", "-1", "takeover", 150),
        funding("BTCUSDT:d", "BTCUSDT", 120),
        funding("BTCUSDT:e", "BTCUSDT", 200),
        external("BTCUSDT:f", "BTCUSDT", "sell", "1", 200),
        change("BTCUSDT:g", "BTCUSDT", "2", "settle", 201),
    ]);
    assert_eq!(s.st.position("BTCUSDT"), d("2"));
    s.apply(&[purged(&[1], 200)]);
    // The closed order's info is gone; the open one stays.
    assert!(!s.st.orders.contains_key(&1));
    assert!(s.st.orders.contains_key(&2) && s.st.open.contains(&2));
    // Keys below the floor are forgotten, keys at or above it stay.
    let keys = |m: Vec<&String>| m.into_iter().cloned().collect::<Vec<_>>();
    assert_eq!(keys(s.st.fills.keys().collect()), vec!["BTCUSDT:f"]);
    assert_eq!(keys(s.st.venue_changes.keys().collect()), vec!["BTCUSDT:g"]);
    assert_eq!(keys(s.st.funding.keys().collect()), vec!["BTCUSDT:e"]);
    assert_eq!(s.st.fill_floor, 200);
    // Forgetting a key never changes a position.
    assert_eq!(s.st.position("BTCUSDT"), d("2"));
    // An open order is never purged, nor an unknown one.
    let why = s.refused(&[purged(&[2], 200)]);
    assert!(why.contains("open order 2"), "{why}");
}

#[test]
fn purging_an_unknown_order_stops_the_state() {
    let mut s = S::new();
    let why = s.refused(&[purged(&[5], 0)]);
    assert!(why.contains("order 5"), "{why}");
}

#[test]
fn the_fill_floor_never_goes_down() {
    let mut s = S::new();
    s.apply(&[
        accepted(1, "buy", "1"),
        fill(1, "BTCUSDT:f", "1", 100),
        change("BTCUSDT:c", "BTCUSDT", "1", "adl", 100),
        funding("BTCUSDT:p", "BTCUSDT", 100),
    ]);
    s.apply(&[booted(500)]);
    assert_eq!(s.st.fill_floor, 500);
    // booted raises the floor and forgets no key; only purged forgets
    // (review 1 S2).
    assert!(s.st.fills.contains_key("BTCUSDT:f"));
    assert!(s.st.venue_changes.contains_key("BTCUSDT:c"));
    assert!(s.st.funding.contains_key("BTCUSDT:p"));
    s.apply(&[booted(300)]);
    assert_eq!(s.st.fill_floor, 500);
    s.apply(&[
        funding("BTCUSDT:k", "BTCUSDT", 450),
        funding("BTCUSDT:m", "BTCUSDT", 520),
    ]);
    // A purge with a lower floor keeps the higher one, and forgets by it.
    s.apply(&[purged(&[], 400)]);
    assert_eq!(s.st.fill_floor, 500);
    assert!(s.st.fills.is_empty() && s.st.venue_changes.is_empty());
    assert_eq!(
        s.st.funding.keys().cloned().collect::<Vec<_>>(),
        vec!["BTCUSDT:m"]
    );
    s.apply(&[purged(&[], 600)]);
    assert_eq!(s.st.fill_floor, 600);
    s.apply(&[booted(0)]);
    assert_eq!(s.st.fill_floor, 600);
    assert!(s.st.funding.is_empty());
}

#[test]
fn the_fact_time_moves_only_on_the_compared_order_events() {
    let mut s = S::new();
    let mut fact = s.apply(&[accepted(1, "buy", "2")]);
    assert_eq!(s.st.orders[&1].fact_at, fact);
    // Each of these moves it.
    let moving: Vec<(&str, Value)> = vec![
        acked(1, json!("v1")),
        other(
            "order_in_doubt",
            json!({"detail": "d", "order_id": 1, "reason": "no_result"}),
        ),
        terminal(1),
        closed(1),
    ];
    // Each of these does not.
    let still: Vec<(&str, Value)> = vec![
        other("cancel_requested", json!({"by": "operator", "order_id": 1})),
        other("cancel_sent", json!({"attempt": 1, "order_id": 1})),
        other(
            "cancel_outcome_noted",
            json!({"attempt": 1, "order_id": 1, "outcome": "confirmed"}),
        ),
        other(
            "status_noted",
            json!({"order_id": 1, "reported_cum": "1", "session": 1, "venue_ts": 1}),
        ),
        other("order_recheck", json!({"order_id": 1})),
        other(
            "venue_oid_learned",
            json!({"order_id": 1, "venue_oid": "v9"}),
        ),
        fill(1, "BTCUSDT:f1", "1", 7),
        other("missing_fills_flagged", json!({"order_id": 1})),
        other("absence_noted", json!({"order_id": 1, "queried_at": 3})),
    ];
    for m in moving {
        for st in &still {
            s.apply(std::slice::from_ref(st));
            assert_eq!(s.st.orders[&1].fact_at, fact, "{} moved it", st.0);
        }
        let name = m.0;
        fact = s.apply(&[m]);
        assert_eq!(s.st.orders[&1].fact_at, fact, "{name} did not move it");
    }
}

#[test]
fn funding_never_changes_a_position() {
    let mut s = S::new();
    s.apply(&[external("BTCUSDT:e1", "BTCUSDT", "buy", "2", 1)]);
    let t = s.st.last_change_at["BTCUSDT"];
    s.apply(&[
        funding("BTCUSDT:f1", "BTCUSDT", 2),
        funding("ETHUSDT:f2", "ETHUSDT", 3),
    ]);
    assert_eq!(s.st.position("BTCUSDT"), d("2"));
    assert_eq!(s.st.position("ETHUSDT"), Dec::ZERO);
    assert!(!s.st.positions.contains_key("ETHUSDT"));
    // Nor its change time: funding is cash only.
    assert_eq!(s.st.last_change_at["BTCUSDT"], t);
    assert!(!s.st.last_change_at.contains_key("ETHUSDT"));
    let f = &s.st.funding["ETHUSDT:f2"];
    assert_eq!((f.instrument.as_str(), f.venue_ts), ("ETHUSDT", 3));
    assert!(!s.st.fills.contains_key("BTCUSDT:f1"));
}

#[test]
fn venue_terminal_marks_the_order_terminal_and_moves_its_fact_time() {
    let mut s = S::new();
    s.apply(&[accepted(1, "buy", "1")]);
    s.apply(&[other(
        "order_in_doubt",
        json!({"detail": "d", "order_id": 1, "reason": "restart"}),
    )]);
    assert!(s.st.orders[&1].in_doubt);
    let t = s.apply(&[terminal(1)]);
    let o = &s.st.orders[&1];
    assert!(o.terminal && !o.in_doubt);
    assert_eq!(o.fact_at, t);
    // The core keeps it open until order_closed (C-TERM).
    assert!(o.open && s.st.open.contains(&1));
    let tc = s.apply(&[closed(1)]);
    assert!(!s.st.open.contains(&1));
    assert!(s.st.orders[&1].terminal);
    assert_eq!(s.st.orders[&1].fact_at, tc);
}

#[test]
fn an_instrument_the_journal_never_changed_has_no_change_time() {
    let mut s = S::new();
    s.apply(&[
        accepted(1, "buy", "1"),
        other(
            "ref_price_updated",
            json!({"instrument": "ETHUSDT", "px": "1", "venue_ts": 1}),
        ),
        funding("SOLUSDT:f", "SOLUSDT", 1),
        other(
            "venue_setting_noted",
            json!({"instrument": "XRPUSDT", "isolated": false, "leverage": 2, "session": 1}),
        ),
    ]);
    for i in ["BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT"] {
        assert_eq!(s.st.last_change_at.get(i), None, "{i}");
    }
    // Each of the four position events sets it, to its own record time.
    let t1 = s.apply(&[fill(1, "BTCUSDT:a", "1", 1)]);
    let t2 = s.apply(&[external("ETHUSDT:b", "ETHUSDT", "buy", "1", 1)]);
    let t3 = s.apply(&[change("SOLUSDT:c", "SOLUSDT", "1", "delivery", 1)]);
    let t4 = s.apply(&[adjusted("XRPUSDT", "1")]);
    assert_eq!(s.st.last_change_at.get("BTCUSDT"), Some(&t1));
    assert_eq!(s.st.last_change_at.get("ETHUSDT"), Some(&t2));
    assert_eq!(s.st.last_change_at.get("SOLUSDT"), Some(&t3));
    assert_eq!(s.st.last_change_at.get("XRPUSDT"), Some(&t4));
    // A later change of one moves only that one.
    let t5 = s.apply(&[adjusted("BTCUSDT", "-1")]);
    assert_eq!(s.st.last_change_at.get("BTCUSDT"), Some(&t5));
    assert_eq!(s.st.last_change_at.get("ETHUSDT"), Some(&t2));
}

#[test]
fn records_that_break_the_rules_stop_the_state() {
    // Each case on a fresh state after one accepted order; each is refused
    // with a state error, and the refusal latches: a good record after it
    // is refused with the same error.
    let cases: Vec<(&str, Vec<(&str, Value)>)> = vec![
        ("order accepted twice", vec![accepted(1, "buy", "1")]),
        ("ack of an unknown order", vec![acked(7, json!("v"))]),
        ("close of an unknown order", vec![closed(7)]),
        ("terminal of an unknown order", vec![terminal(7)]),
        (
            "a side that is neither buy nor sell",
            vec![external("k", "BTCUSDT", "long", "1", 1)],
        ),
        (
            "a quantity with an exponent",
            vec![external("k", "BTCUSDT", "buy", "1e3", 1)],
        ),
        (
            "a missing key",
            vec![(
                "funding_booked",
                json!({"funding": {"amount": "1", "instrument": "BTCUSDT", "rate": "0", "venue_ts": 1}}),
            )],
        ),
        (
            "a venue change of an unknown kind",
            vec![change("k", "BTCUSDT", "1", "liquidation", 1)],
        ),
        (
            "a hold of an unknown kind",
            vec![other(
                "hold_set",
                json!({"hold": "venue_change", "source": "venue", "note": "n"}),
            )],
        ),
        (
            "a position overflow",
            vec![
                adjusted("BTCUSDT", &"9".repeat(30)),
                adjusted("BTCUSDT", &"9".repeat(30)),
            ],
        ),
    ];
    for (label, events) in cases {
        let mut s = S::new();
        s.apply(&[accepted(1, "buy", "1")]);
        let why = s.refused(&events);
        assert!(!why.is_empty(), "{label}");
        let first = JournalError::State {
            seq: 2,
            why: why.clone(),
        };
        let good = s.rec(&[other("order_recheck", json!({"order_id": 1}))]);
        assert_eq!(
            s.st.apply(&good),
            Err(first),
            "{label}: the refusal did not latch"
        );
    }

    // Records out of order: applied twice or with a gap.
    let mut s = S::new();
    s.apply(&[external("k1", "BTCUSDT", "buy", "1", 1)]);
    let mut again = s.rec(&[external("k1", "BTCUSDT", "buy", "1", 1)]);
    again.seq = 1;
    assert!(matches!(
        s.st.apply(&again),
        Err(JournalError::State { seq: 1, .. })
    ));
    let mut s = S::new();
    let mut gap = s.rec(&[external("k1", "BTCUSDT", "buy", "1", 1)]);
    gap.seq = 2;
    assert!(matches!(
        s.st.apply(&gap),
        Err(JournalError::State { seq: 2, .. })
    ));
    assert_eq!(s.st.position("BTCUSDT"), Dec::ZERO);
}
