//! The check engine (C-WATCH part 5, U19a5): the scheduler, request
//! deadlines and stray answers, status matching and the journal read per
//! round, and the bookkeeping of confirmed differences. On the engine rig
//! (model venue, test journal, manual clock) or driven by hand.

#![allow(clippy::unwrap_used)]

mod common;

use common::rig::*;
use serde_json::json;
use watcher::engine::{Action, Check, DEADLINE_EXTRA_MS, cut};
use watcher::w2::W2;

fn venue_ids(a: &[Action]) -> Vec<u64> {
    a.iter()
        .filter_map(|x| match x {
            Action::Venue { req_id, .. } => Some(*req_id),
            _ => None,
        })
        .collect()
}

fn status_id(a: &[Action]) -> u64 {
    match a {
        [Action::Status { req_id }] => *req_id,
        _ => panic!("{a:?}"),
    }
}

/// Answers the venue request `id` with an empty page; returns the status
/// request it brings.
fn empty_page(r: &mut Rig, now: u64, id: u64) -> u64 {
    status_id(&r.engine.on_venue(now, id, page(vec![], "")))
}

#[test]
fn the_first_rounds_and_heartbeat_are_due_at_start() {
    let mut r = Rig::new("engine-first");
    assert!(r.engine.poll(T0 - 1).is_empty());
    let a = r.engine.poll(T0);
    let want = vec![
        ("category", "linear".to_string()),
        ("settleCoin", "USDT".to_string()),
        ("limit", "50".to_string()),
    ];
    assert_eq!(
        a,
        vec![
            Action::Venue {
                req_id: 1,
                path: watcher::w1::PATH,
                params: want
            },
            Action::Heartbeat {
                checks: Check::ALL.map(|c| (c, None)).to_vec()
            },
        ]
    );
    // Before the first success the age counts from the config load.
    assert_eq!(r.gauge("watcher_check_age_s", "W1"), Some(5.0));
    r.run(T0, a);
    assert!(
        !r.engine
            .poll(T0 + 4999)
            .iter()
            .any(|x| matches!(x, Action::Heartbeat { .. }))
    );
    let a = r.engine.poll(T0 + 5000);
    let hb = a
        .iter()
        .find(|x| matches!(x, Action::Heartbeat { .. }))
        .unwrap();
    let want = [
        (Check::W1, Some(T0)),
        (Check::W2, None),
        (Check::W3, None),
        (Check::W4, None),
    ];
    assert_eq!(
        hb,
        &Action::Heartbeat {
            checks: want.to_vec()
        }
    );
}

#[test]
fn a_late_round_runs_once_and_rounds_do_not_overlap() {
    let mut r = Rig::new("engine-late");
    let [id] = venue_ids(&r.engine.poll(T0))[..] else {
        panic!()
    };
    for t in [T0 + 1000, T0 + 2000, T0 + 3500] {
        assert!(venue_ids(&r.engine.poll(t)).is_empty(), "overlap at {t}");
    }
    let s = empty_page(&mut r, T0 + 3600, id);
    r.engine.on_status(T0 + 3600, s, Ok(r.status()));
    assert_eq!(r.engine.success(Check::W1), Some(T0 + 3600));
    // Three intervals were missed: one round now, not three.
    let [id] = venue_ids(&r.engine.poll(T0 + 3600))[..] else {
        panic!()
    };
    assert!(venue_ids(&r.engine.poll(T0 + 3600)).is_empty());
    let s = empty_page(&mut r, T0 + 3600, id);
    r.engine.on_status(T0 + 3600, s, Ok(r.status()));
    assert!(venue_ids(&r.engine.poll(T0 + 4599)).is_empty());
    assert_eq!(venue_ids(&r.engine.poll(T0 + 4600)).len(), 1);
}

#[test]
fn a_request_without_an_answer_fails_its_round_at_the_deadline_and_a_late_answer_is_ignored() {
    let mut r = Rig::new("engine-deadline");
    let deadline = 2000 + DEADLINE_EXTRA_MS;
    let [lost] = venue_ids(&r.engine.poll(T0))[..] else {
        panic!()
    };
    assert!(venue_ids(&r.engine.poll(T0 + deadline - 1)).is_empty());
    // At the deadline the round fails and the next one starts.
    let [id] = venue_ids(&r.engine.poll(T0 + deadline))[..] else {
        panic!()
    };
    assert_eq!(
        r.lines_with("W1 round failed: no answer before the request's deadline"),
        1
    );
    assert!(
        r.engine
            .on_venue(T0 + deadline, lost, page(vec![], ""))
            .is_empty()
    );
    assert_eq!(
        r.lines_with(&format!("debug: an answer to request {lost}")),
        1
    );
    // A lost status answer: the same, from its own emission.
    let t1 = T0 + deadline + 10;
    let lost = empty_page(&mut r, t1, id);
    assert!(venue_ids(&r.engine.poll(t1 + deadline - 1)).is_empty());
    let [id] = venue_ids(&r.engine.poll(t1 + deadline))[..] else {
        panic!()
    };
    assert!(
        r.engine
            .on_status(t1 + deadline, lost, Ok(r.status()))
            .is_empty()
    );
    assert_eq!(
        r.lines_with(&format!("debug: an answer to request {lost}")),
        1
    );
    assert_eq!(r.engine.success(Check::W1), None);
    // A status answer given as a venue answer is stray too.
    let s = empty_page(&mut r, t1 + deadline, id);
    assert!(
        r.engine
            .on_venue(t1 + deadline, s, page(vec![], ""))
            .is_empty()
    );
    r.engine.on_status(t1 + deadline, s, Ok(r.status()));
    assert_eq!(r.engine.success(Check::W1), Some(t1 + deadline));
}

#[test]
fn status_answers_in_reverse_order_are_each_used_by_their_own_round() {
    let mut r = Rig::new("engine-reverse");
    r.engine.add_check(Box::new(W2::default()), 1000);
    r.write(T0 - 1000, &open(1));
    r.write(T0 - 1000, &open(2));
    let [w1, w2] = venue_ids(&r.engine.poll(T0))[..] else {
        panic!()
    };
    let s1 = empty_page(&mut r, T0, w1);
    r.write(T0, &open(3));
    let cap1 = r.status();
    r.write(T0, &open(4));
    let s2 = empty_page(&mut r, T0, w2);
    // The later-sampled cap (4) arrives first.
    r.engine.on_status(T0 + 10, s2, Ok(r.status()));
    assert_eq!(r.engine.success(Check::W2), Some(T0 + 10));
    assert_eq!(r.engine.success(Check::W1), None);
    assert_eq!(r.engine.state().last_seq, 4);
    // Cap 3 is below what the state holds, not below what it held when
    // W1 asked: nothing to read, no error.
    r.engine.on_status(T0 + 20, s1, Ok(cap1));
    assert_eq!(r.engine.success(Check::W1), Some(T0 + 20));
    assert_eq!(r.metrics.get("watcher_journal_error", &[]), Some(0.0));
    assert_eq!(r.lines_with("error"), 0);
    // W1 compared against the newer state: order 4 is missing there.
    assert_eq!(r.gauge("watcher_differences", "W1"), Some(4.0));
}

#[test]
fn a_status_of_another_instance_or_a_cap_below_the_read_before_the_request_stops_the_reader() {
    let mut r = Rig::new("engine-instance");
    r.write(T0 - 1000, &open(1));
    // Unusable answers fail the round and latch nothing.
    for (t, bad) in [
        (T0, Err("connection refused".to_string())),
        (
            T0 + 1000,
            Ok(json!({"core": {"instance_id": "0000000000000007"}})),
        ),
        (
            T0 + 2000,
            Ok(json!({"journal": {"durable_seq": 1}, "core": {"instance_id": "7"}})),
        ),
    ] {
        let [id] = venue_ids(&r.engine.poll(t))[..] else {
            panic!()
        };
        let s = empty_page(&mut r, t, id);
        r.engine.on_status(t, s, bad);
    }
    assert_eq!(
        r.lines_with("round failed: /api/status: connection refused"),
        1
    );
    assert_eq!(
        r.lines_with("round failed: /api/status: journal.durable_seq is not a u64"),
        1
    );
    assert_eq!(r.lines_with("core.instance_id is not 16 hex digits"), 1);
    assert_eq!(r.metrics.get("watcher_journal_error", &[]), None);
    r.instance = 8;
    r.at(T0 + 3000);
    assert_eq!(
        r.lines_with("error: a segment of instance 0000000000000007"),
        1
    );
    assert_eq!(r.metrics.get("watcher_journal_error", &[]), Some(1.0));
    // Latched: later rounds ask nothing and never succeed.
    rounds_ask_nothing(&mut r, T0 + 4000);

    let mut r = Rig::new("engine-went-back");
    for id in 1..=5 {
        r.write(T0 - 1000, &open(id));
    }
    r.at(T0);
    assert_eq!(r.engine.success(Check::W1), Some(T0));
    r.write(T0, &open(6));
    r.cap = Some(3);
    r.at(T0 + 1000);
    assert_eq!(
        r.lines_with("error: the OMS's durable sequence 3 is below record 5"),
        1
    );
    assert_eq!(r.engine.success(Check::W1), Some(T0));
    r.cap = None;
    rounds_ask_nothing(&mut r, T0 + 2000);
}

/// Five rounds from `t`: no venue request, no success, one latch line.
fn rounds_ask_nothing(r: &mut Rig, t: u64) {
    let (asked, success) = (r.venue.requests.len(), r.engine.success(Check::W1));
    for k in 0..5 {
        r.at(t + k * 1000);
    }
    assert_eq!(r.venue.requests.len(), asked);
    assert_eq!(r.engine.success(Check::W1), success);
    assert_eq!(r.metrics.get("watcher_journal_error", &[]), Some(1.0));
    assert_eq!(r.lines_with("stops until the watcher restarts"), 1);
}

#[test]
fn a_retried_journal_error_heals_and_a_latched_one_fails_every_round() {
    let mut r = Rig::new("engine-retried");
    for id in 1..=3 {
        r.write(T0 - 1000, &open(id));
    }
    // The OMS says 5 is durable; the files hold 3.
    r.cap = Some(5);
    r.at(T0);
    assert_eq!(r.lines_with("read stopped at record 3, below the cap 5"), 1);
    for k in 1..4 {
        r.at(T0 + k * 1000);
    }
    assert_eq!(r.metrics.get("watcher_journal_error", &[]), Some(1.0));
    assert_eq!(
        r.lines_with("missing segment"),
        2,
        "one error and one round line"
    );
    assert_eq!(r.engine.success(Check::W1), None);
    r.write(T0 + 3000, &open(4));
    r.write(T0 + 3000, &open(5));
    r.at(T0 + 4000);
    assert_eq!(r.metrics.get("watcher_journal_error", &[]), Some(0.0));
    assert_eq!(r.metrics.get("watcher_journal_seq", &[]), Some(5.0));
    assert_eq!(r.lines_with("the journal is read again without error"), 1);
    assert_eq!(r.engine.success(Check::W1), Some(T0 + 4000));
    // An unknown event latches: it does not heal by reading again.
    r.cap = None;
    r.write(T0 + 4000, &[common::ev("no_such_event", json!({}))]);
    r.at(T0 + 5000);
    assert_eq!(r.lines_with("unknown event \"no_such_event\""), 2);
    rounds_ask_nothing(&mut r, T0 + 6000);
}

#[test]
fn the_detail_is_at_most_1024_bytes() {
    let mut r = Rig::new("engine-detail");
    let long = "é".repeat(600);
    r.venue.orders = vec![row(&long, "v0", "BTCUSDT", "Buy")];
    r.venue
        .orders
        .extend((1..30).map(|i| row("", &format!("v{i}"), "BTCUSDT", "Buy")));
    for k in 0..3 {
        r.at(T0 + k * 1000);
    }
    let [(_, Check::W1, d)] = &r.mismatches[..] else {
        panic!("{:?}", r.mismatches)
    };
    let full = format!("W1 foreign v0: venue BTCUSDT Buy orderLinkId \"{long}\", journal");
    assert!(d.len() <= 1024 && d.len() >= 1023, "{}", d.len());
    assert!(full.starts_with(d.as_str()) || d.starts_with(&full));
    // Many short ones: the first and the count.
    let mut r = Rig::new("engine-detail-many");
    r.venue.orders = (0..30)
        .map(|i| row("", &format!("v{i}"), "BTCUSDT", "Buy"))
        .collect();
    for k in 0..3 {
        r.at(T0 + k * 1000);
    }
    let d = &r.mismatches[0].2;
    assert!(
        d.starts_with("W1 foreign v0: ") && d.ends_with("; and 29 more"),
        "{d}"
    );
    assert_eq!(cut(format!("a{long}"), 1024).len(), 1023);
    assert_eq!(cut("abc".to_string(), 1024), "abc");
}

/// Order `id` closed long ago and still listed at the venue.
fn closed_but_listed(r: &mut Rig, ids: &[u64]) {
    for &id in ids {
        r.write(T0 - 10_000, &open(id));
        r.write(T0 - 10_000, &[of("order_closed", id)]);
        r.venue.orders.push(ours(id, "BTCUSDT", "Buy"));
    }
}

#[test]
fn a_confirmed_identity_is_counted_once() {
    let mut r = Rig::new("engine-once");
    closed_but_listed(&mut r, &[1]);
    for k in 0..6 {
        r.at(T0 + k * 1000);
    }
    assert_eq!(r.mismatches.len(), 1);
    assert_eq!(r.gauge("watcher_mismatches_total", "W1"), Some(1.0));
    assert_eq!(r.lines_with("error: confirmed difference"), 1);
    // Gone for a round, then back: a new difference, two rounds again.
    // (Its send waits for the hold the first one set, U19a7.)
    let listed = std::mem::take(&mut r.venue.orders);
    r.at(T0 + 6000);
    r.venue.orders = listed;
    r.at(T0 + 7000);
    assert_eq!(r.confirmed.len(), 1);
    r.at(T0 + 8000);
    assert_eq!(r.confirmed.len(), 2);
    assert_eq!(r.confirmed[1].0, T0 + 8000);
    assert_eq!(r.mismatches.len(), 1);
    assert_eq!(r.gauge("watcher_mismatches_total", "W1"), Some(2.0));
}

#[test]
fn mismatch_active_counts_the_confirmed_differences_a_round_still_sees() {
    let mut r = Rig::new("engine-active");
    closed_but_listed(&mut r, &[1, 2]);
    r.at(T0);
    assert_eq!(r.gauge("watcher_mismatch_active", "W1"), Some(0.0));
    r.at(T0 + 1000);
    assert_eq!(r.gauge("watcher_mismatch_active", "W1"), Some(2.0));
    // A failed round changes nothing.
    r.venue
        .script
        .push_back(Err(watcher::rest::RestError::NoAnswer("timeout".into())));
    r.at(T0 + 2000);
    assert_eq!(r.gauge("watcher_mismatch_active", "W1"), Some(2.0));
    r.venue.orders.remove(0);
    r.at(T0 + 3000);
    assert_eq!(r.gauge("watcher_mismatch_active", "W1"), Some(1.0));
    r.venue.orders.clear();
    r.at(T0 + 4000);
    assert_eq!(r.gauge("watcher_mismatch_active", "W1"), Some(0.0));
    assert_eq!(r.gauge("watcher_mismatches_total", "W1"), Some(2.0));
}

#[test]
fn a_failed_round_keeps_the_sightings_of_the_last_completed_round() {
    let mut r = Rig::new("engine-failed-keeps");
    closed_but_listed(&mut r, &[1]);
    r.at(T0);
    assert_eq!(r.gauge("watcher_differences", "W1"), Some(1.0));
    // A venue failure, then an OMS that does not answer.
    r.venue
        .script
        .push_back(Err(watcher::rest::RestError::NoAnswer("timeout".into())));
    r.at(T0 + 1000);
    let [id] = venue_ids(&r.engine.poll(T0 + 2000))[..] else {
        panic!()
    };
    let answer = r.venue.answer(watcher::w1::PATH, &[]);
    let s = status_id(&r.engine.on_venue(T0 + 2000, id, answer));
    r.engine
        .on_status(T0 + 2000, s, Err("connection refused".to_string()));
    assert!(r.mismatches.is_empty());
    assert_eq!(r.engine.success(Check::W1), Some(T0));
    assert_eq!(r.gauge("watcher_differences", "W1"), Some(1.0));
    // The sighting of T0 is still the last completed round's: this
    // completed round confirms.
    r.at(T0 + 3000);
    let [(at, Check::W1, d)] = &r.mismatches[..] else {
        panic!("{:?}", r.mismatches)
    };
    assert_eq!(*at, T0 + 3000);
    assert!(
        d.starts_with("W1 closed_in_journal ") && d.ends_with("journal entry 13.0 s old"),
        "{d}"
    );
    assert_eq!(r.gauge("watcher_mismatches_total", "W1"), Some(1.0));
}
