//! The monitoring gate in the core (C-LIVE part 2, U18b4): the watcher's
//! journaled heartbeats, the verdict at record time, the hold `Unmonitored`
//! (stage 1) and the cancel requests for resting plain orders 60 s later
//! (stage 2). Time is the record time; ticks come every 250 ms as the
//! node's tick task sends them.

#![allow(clippy::unwrap_used)]

mod common;
use common::*;
use oms_core::*;
use std::collections::BTreeMap;

const TICK: u64 = 250_000;
const MS: u64 = 1_000;

fn names(out: &Output) -> Vec<&'static str> {
    out.events.iter().map(Event::name).collect()
}

/// `config()` with the gate at its design values (30 s, 10 s, 60 s).
fn gated() -> CoreConfig {
    let mut cfg = config();
    cfg.policy.monitor = Some(MonitorPolicy::default());
    cfg
}

/// Runs `input` at exactly record time `at` (`H::inp` adds 1 ms first).
fn at(h: &mut H, t: Ts, input: Input) -> Output {
    assert!(t > h.now, "time goes forward");
    h.now = t - MS;
    h.inp(input)
}

fn checks(w1: Option<Ts>, w2: Option<Ts>, w3: Option<Ts>) -> BTreeMap<String, Option<Ts>> {
    let mut c = BTreeMap::new();
    c.insert("W1".to_string(), w1);
    c.insert("W2".to_string(), w2);
    c.insert("W3".to_string(), w3);
    c.insert("W4".to_string(), None);
    c
}

/// A heartbeat at record time `t` with these check times; the reply.
fn heartbeat(
    h: &mut H,
    t: Ts,
    c: BTreeMap<String, Option<Ts>>,
) -> (Output, Result<String, String>) {
    let out = at(
        h,
        t,
        Input::Watcher(WatcherInput::Heartbeat {
            watcher: "watcher-1".into(),
            checks: c,
        }),
    );
    let reply = match &out.reply {
        Some(Reply::Operator(r)) => r.clone(),
        other => panic!("unexpected reply {other:?}"),
    };
    (out, reply)
}

/// A heartbeat at `t` whose four checks all succeeded at `t`.
fn fresh(h: &mut H, t: Ts) -> Output {
    let (out, reply) = heartbeat(h, t, checks(Some(t), Some(t), Some(t)));
    assert_eq!(reply, Ok("recorded".to_string()));
    out
}

fn unmonitored(h: &H) -> bool {
    h.core.holds(None).contains(&HoldReason::Unmonitored)
}

fn rule_cancels(h: &H) -> Vec<OrderId> {
    h.core
        .orders
        .values()
        .filter(|o| {
            o.cancel
                .as_ref()
                .is_some_and(|c| c.by == Actor::Rule(RuleId::Unmonitored))
        })
        .map(|o| o.id)
        .collect()
}

/// An order of `a` resting at the venue.
fn resting(h: &mut H, key: &str, side: Side, reduce_only: bool) -> OrderId {
    let r = if reduce_only {
        h.reduce_only_req("a", key, side, "0.5", "100")
    } else {
        h.req("a", key, side, "0.5", "100")
    };
    let id = match h.new_order(r) {
        NewOrderReply::Accepted { order_id } => order_id,
        other => panic!("{key}: {other:?}"),
    };
    h.placed_open(id, key);
    id
}

/// Gated, link up and synced, monitored by one fresh heartbeat; a long of
/// 2, a resting plain buy and a resting reduce-only sell of `a`. Returns
/// (plain, reduce-only).
fn gated_with_orders() -> (H, OrderId, OrderId) {
    let mut h = H::configured(gated());
    h.link_up_and_sync();
    assert_eq!(h.core.holds(None), vec![HoldReason::Unmonitored]);
    let t = h.now + SEC;
    fresh(&mut h, t);
    assert_eq!(h.core.holds(None), vec![]);
    h.long("a", "k0", "2");
    let plain = resting(&mut h, "p", Side::Buy, false);
    let reduce = resting(&mut h, "r", Side::Sell, true);
    (h, plain, reduce)
}

/// Ticks every 250 ms from the last whole tick after `from` up to and
/// including `to`; returns the record times at which the gate changed and
/// those at which a cancel was asked for.
fn ticks(h: &mut H, from: Ts, to: Ts) -> (Vec<(Ts, bool)>, Vec<Ts>) {
    let (mut changes, mut cancels) = (Vec::new(), Vec::new());
    let mut t = from + TICK;
    // Bounded: (to - from) / 250 ms ticks.
    while t <= to {
        let out = at(h, t, Input::Tick);
        for e in &out.events {
            match e {
                Event::MonitorChanged { monitored, .. } => changes.push((t, *monitored)),
                Event::CancelRequested {
                    by: Actor::Rule(RuleId::Unmonitored),
                    ..
                } => cancels.push(t),
                _ => {}
            }
        }
        t += TICK;
    }
    (changes, cancels)
}

/// Done criterion (U18b): without heartbeats the gate is monitored at
/// 29.75 s and 30.0 s and unmonitored at 30.25 s after the last one; no
/// cancel up to 60 s after that record (90.25 s), the cancel of the
/// resting plain order on the next tick (90.5 s); the reduce-only order
/// stays.
#[test]
fn missing_heartbeats_unmonitor_at_30_s_and_cancel_plain_orders_at_90_s() {
    let (mut h, plain, reduce) = gated_with_orders();
    let t0 = h.now + SEC;
    fresh(&mut h, t0);
    let (changes, cancels) = ticks(&mut h, t0, t0 + 30 * SEC);
    assert_eq!(
        changes,
        Vec::<(Ts, bool)>::new(),
        "monitored up to and at 30 s"
    );
    assert!(!unmonitored(&h));
    let (changes, cancels_30) = ticks(&mut h, t0 + 30 * SEC, t0 + 30 * SEC + TICK);
    assert_eq!(
        changes,
        vec![(t0 + 30 * SEC + TICK, false)],
        "unmonitored at 30.25 s"
    );
    assert!(unmonitored(&h));
    let m = h.core.monitor.clone().unwrap();
    assert_eq!((m.monitored, m.since), (false, t0 + 30 * SEC + TICK));
    assert_eq!(m.reason, "no heartbeat for more than 30000 ms");
    let (changes, cancels_90) = ticks(&mut h, t0 + 30 * SEC + TICK, t0 + 90 * SEC + TICK);
    assert_eq!(changes, Vec::<(Ts, bool)>::new());
    assert!(
        cancels.is_empty() && cancels_30.is_empty() && cancels_90.is_empty(),
        "no cancel up to 90.25 s"
    );
    assert_eq!(rule_cancels(&h), Vec::<OrderId>::new());
    let (_, cancels) = ticks(&mut h, t0 + 90 * SEC + TICK, t0 + 90 * SEC + 2 * TICK);
    assert_eq!(
        cancels,
        vec![t0 + 90 * SEC + 2 * TICK],
        "the cancel at 90.5 s"
    );
    assert_eq!(rule_cancels(&h), vec![plain]);
    assert!(
        h.order(reduce).cancel.is_none(),
        "the reduce-only order stays"
    );
    // One request per order: later ticks ask for nothing more.
    let (_, cancels) = ticks(&mut h, t0 + 90 * SEC + 2 * TICK, t0 + 95 * SEC);
    assert_eq!(cancels, Vec::<Ts>::new());
}

/// Done criterion (U18b): heartbeats every 5 s with W2 (then W1) frozen
/// at `t0`. The heartbeat 10 s after it keeps the gate (not older than
/// 10 s), the one at 15 s unmonitors; the cancel follows on the first tick
/// more than 60 s after that record. W3 being old changes nothing.
#[test]
fn a_heartbeat_with_w1_or_w2_older_than_10_s_unmonitors_and_cancels_60_s_later() {
    for frozen in ["W1", "W2"] {
        let (mut h, plain, reduce) = gated_with_orders();
        let t0 = h.now + SEC;
        fresh(&mut h, t0);
        let mut t = t0;
        let mut changed_at = None;
        for _ in 0..3 {
            t += 5 * SEC;
            let mut c = checks(Some(t), Some(t), Some(t0 - 100 * SEC));
            c.insert(frozen.to_string(), Some(t0));
            let (out, reply) = heartbeat(&mut h, t, c);
            assert_eq!(reply, Ok("recorded".to_string()));
            if names(&out).contains(&"monitor_changed") {
                changed_at.get_or_insert(t);
            }
        }
        assert_eq!(
            changed_at,
            Some(t0 + 15 * SEC),
            "{frozen}: not at 10 s, at 15 s"
        );
        let m = h.core.monitor.clone().unwrap();
        assert_eq!(
            m.reason,
            format!("{frozen} last succeeded more than 10000 ms before the heartbeat")
        );
        let since = t0 + 15 * SEC;
        let (_, cancels) = ticks(&mut h, since, since + 60 * SEC);
        assert_eq!(cancels, Vec::<Ts>::new(), "{frozen}: no cancel up to 60 s");
        let (_, cancels) = ticks(&mut h, since + 60 * SEC, since + 60 * SEC + TICK);
        assert_eq!(cancels, vec![since + 60 * SEC + TICK], "{frozen}");
        assert_eq!(rule_cancels(&h), vec![plain]);
        assert!(h.order(reduce).cancel.is_none());
    }
}

/// The exact edge of the check age: 10 s old at the heartbeat keeps the
/// gate, 1 µs more does not; `null` does not.
#[test]
fn the_check_age_edge_and_a_check_without_success() {
    let (mut h, _, _) = gated_with_orders();
    let t = h.now + 20 * SEC;
    assert_eq!(
        heartbeat(&mut h, t, checks(Some(t), Some(t - 10 * SEC), None)).1,
        Ok("recorded".to_string())
    );
    assert!(!unmonitored(&h), "exactly 10 s");
    let t = t + SEC;
    assert_eq!(
        heartbeat(&mut h, t, checks(Some(t), Some(t - 10 * SEC - 1), None)).1,
        Ok("recorded".to_string())
    );
    assert!(unmonitored(&h), "10 s and 1 µs");
    let t = t + SEC;
    assert_eq!(
        heartbeat(&mut h, t, checks(Some(t), Some(t), None)).1,
        Ok("recorded".to_string())
    );
    assert!(!unmonitored(&h));
    let t = t + SEC;
    assert_eq!(
        heartbeat(&mut h, t, checks(None, Some(t), None)).1,
        Ok("recorded".to_string())
    );
    assert_eq!(
        h.core.monitor.as_ref().unwrap().reason,
        "W1 has not succeeded yet"
    );
    assert!(unmonitored(&h));
}

/// W3 and W4 are recorded and shown, not gated (the plan gates W1, W2).
#[test]
fn w3_and_w4_are_recorded_and_not_gated() {
    let (mut h, _, _) = gated_with_orders();
    let t = h.now + SEC;
    let (out, _) = heartbeat(&mut h, t, checks(Some(t), Some(t), Some(t - 100 * SEC)));
    assert_eq!(names(&out), ["watcher_heartbeat"]);
    assert!(!unmonitored(&h), "an old W3 does not unmonitor");
    let last = h.core.monitor.as_ref().unwrap().last.clone().unwrap();
    assert_eq!(last.at, t);
    assert_eq!(last.watcher, "watcher-1");
    assert_eq!(last.checks.get("W3"), Some(&Some(t - 100 * SEC)));
    assert_eq!(last.checks.get("W4"), Some(&None));
}

/// The boot rule: a start with the gate is unmonitored from the boot
/// record until the first fresh heartbeat; a plain order is refused until
/// then, and 60 s after the start resting plain orders are cancelled. The
/// state from before the restart does not count.
#[test]
fn the_gate_starts_unmonitored_at_boot_until_the_first_fresh_heartbeat() {
    let (mut h, plain, _) = gated_with_orders();
    let t = h.now + SEC;
    fresh(&mut h, t);
    let boot_at = t + SEC;
    let out = at(
        &mut h,
        boot_at,
        Input::Boot {
            binary: "test".into(),
            kill_marker: None,
        },
    );
    assert_eq!(names(&out)[0], "booted");
    let m = h.core.monitor.clone().unwrap();
    assert_eq!(
        (m.monitored, m.since, m.reason.as_str(), m.last.is_none()),
        (false, boot_at, "no heartbeat since the start", true)
    );
    h.link_up_and_sync();
    assert!(unmonitored(&h));
    let refused = h.reject("a", "after-boot", Side::Buy, "0.1", "100");
    assert!(
        refused.contains(&RejectReason::Hold(HoldReason::Unmonitored)),
        "{refused:?}"
    );
    // No heartbeat: cancels 60 s after the boot record.
    let from = h.now;
    let (changes, cancels) = ticks(&mut h, boot_at, boot_at + 60 * SEC);
    assert!(from < boot_at + 60 * SEC);
    assert!(changes.is_empty() && cancels.is_empty());
    let (_, cancels) = ticks(&mut h, boot_at + 60 * SEC, boot_at + 60 * SEC + TICK);
    assert_eq!(cancels, vec![boot_at + 60 * SEC + TICK]);
    assert_eq!(rule_cancels(&h), vec![plain]);
    // The first fresh heartbeat opens the gate.
    let t = h.now + SEC;
    let out = fresh(&mut h, t);
    assert_eq!(names(&out), ["watcher_heartbeat", "monitor_changed"]);
    assert!(!unmonitored(&h));
}

/// Back to monitored: the first heartbeat with W1 and W2 fresh. Stage 2
/// stops: a plain order rested after that gets no request; the request
/// made before stands.
#[test]
fn a_fresh_heartbeat_monitors_again_and_stops_the_cancels() {
    let (mut h, plain, _) = gated_with_orders();
    let t0 = h.now + SEC;
    fresh(&mut h, t0);
    ticks(&mut h, t0, t0 + 91 * SEC);
    assert_eq!(rule_cancels(&h), vec![plain]);
    let t = h.now + SEC;
    let out = fresh(&mut h, t);
    assert_eq!(names(&out), ["watcher_heartbeat", "monitor_changed"]);
    assert_eq!(
        out.events[1],
        Event::MonitorChanged {
            monitored: true,
            reason: "W1 and W2 fresh (watcher-1)".into()
        }
    );
    let later = resting(&mut h, "later", Side::Buy, false);
    let t = h.now;
    for i in 1..=8 {
        fresh(&mut h, t + i * 5 * SEC);
        ticks(&mut h, t + i * 5 * SEC, t + i * 5 * SEC + 4 * SEC);
    }
    assert!(h.order(later).cancel.is_none(), "stage 2 stopped");
    assert!(
        h.order(plain).cancel.is_some(),
        "the earlier request stands"
    );
}

/// A check time more than 1 s after the record time is refused: no event,
/// no record, the gate as it was. Exactly 1 s is taken.
#[test]
fn a_heartbeat_from_the_future_is_refused() {
    let (mut h, _, _) = gated_with_orders();
    let records = h.log.len();
    let before = h.core.monitor.clone();
    let t = h.now + SEC;
    let (out, reply) = heartbeat(&mut h, t, checks(Some(t), Some(t + SEC + 1), None));
    let e = reply.unwrap_err();
    assert!(e.starts_with("bad_request: W2 succeeded at"), "{e}");
    assert!(out.events.is_empty());
    assert_eq!(h.log.len(), records);
    assert_eq!(h.core.monitor, before);
    let t = t + SEC;
    let (_, reply) = heartbeat(&mut h, t, checks(Some(t + SEC), Some(t), None));
    assert_eq!(
        reply,
        Ok("recorded".to_string()),
        "exactly 1 s ahead is taken"
    );
}

/// The shape checks: W1 and W2 present, keys W1..W9 only, a label of 1 to
/// 64 characters; a heartbeat without a gate. Each refused, nothing
/// recorded.
#[test]
fn a_heartbeat_of_the_wrong_shape_or_without_a_gate_records_nothing() {
    let (mut h, _, _) = gated_with_orders();
    let t = h.now + SEC;
    let mut no_w2 = checks(Some(t), Some(t), None);
    no_w2.remove("W2");
    let mut w10 = checks(Some(t), Some(t), None);
    w10.insert("W10".into(), Some(t));
    let records = h.log.len();
    for (label, c, because) in [
        ("watcher-1", no_w2, "W1 and W2"),
        ("watcher-1", w10, "W1..W9"),
        ("", checks(Some(t), Some(t), None), "label"),
        (&"x".repeat(65), checks(Some(t), Some(t), None), "label"),
    ] {
        let out = h.inp(Input::Watcher(WatcherInput::Heartbeat {
            watcher: label.to_string(),
            checks: c,
        }));
        match out.reply {
            Some(Reply::Operator(Err(e))) => assert!(
                e.starts_with("bad_request") && e.contains(because),
                "{because}: {e}"
            ),
            other => panic!("{because}: {other:?}"),
        }
        assert!(out.events.is_empty());
    }
    assert_eq!(h.log.len(), records);
    // 64 characters are taken.
    let out = h.inp(Input::Watcher(WatcherInput::Heartbeat {
        watcher: "é".repeat(64),
        checks: checks(Some(h.now), Some(h.now), None),
    }));
    assert_eq!(out.reply, Some(Reply::Operator(Ok("recorded".into()))));
    // Without a gate.
    let mut h = H::ready();
    let t = h.now + SEC;
    let (out, reply) = heartbeat(&mut h, t, checks(Some(t), Some(t), None));
    assert!(reply.unwrap_err().contains("no monitoring gate"));
    assert!(out.events.is_empty());
    assert_eq!(h.core.monitor, None);
    assert_eq!(h.core.holds(None), vec![]);
}

/// Stage 2 asks only while the link is up; the ticks after the link comes
/// back ask then.
#[test]
fn stage_two_asks_only_while_the_link_is_up() {
    let (mut h, plain, _) = gated_with_orders();
    let t0 = h.now + SEC;
    fresh(&mut h, t0);
    let session = h.session;
    at(
        &mut h,
        t0 + SEC,
        Input::Venue(VenueInput::Link {
            up: false,
            session,
            reason: "test".into(),
        }),
    );
    let (_, cancels) = ticks(&mut h, t0 + SEC, t0 + 100 * SEC);
    assert_eq!(cancels, Vec::<Ts>::new(), "link down: nothing asked");
    h.link_up_and_sync();
    let from = h.now;
    let (_, cancels) = ticks(&mut h, from, from + SEC);
    assert_eq!(cancels.len(), 1);
    assert_eq!(rule_cancels(&h), vec![plain]);
}

/// A configuration that turns the gate on starts unmonitored at its
/// activation; one that turns it off clears the gate and its hold; one
/// that keeps it keeps the state.
#[test]
fn a_configuration_that_turns_the_gate_on_or_off() {
    let mut h = H::ready();
    assert_eq!(h.core.monitor, None);
    let mut cfg = gated();
    cfg.version = 2;
    let out = h.inp(Input::Config(Box::new(cfg.clone())));
    assert_eq!(names(&out), ["config_activated"]);
    let m = h.core.monitor.clone().unwrap();
    assert_eq!(
        (m.monitored, m.since, m.reason.as_str()),
        (false, h.now, "no heartbeat since the gate came on")
    );
    assert_eq!(h.core.holds(None), vec![HoldReason::Unmonitored]);
    let t = h.now + SEC;
    fresh(&mut h, t);
    cfg.version = 3;
    cfg.policy.monitor.as_mut().unwrap().unmonitored_cancel_us = 120 * SEC;
    h.inp(Input::Config(Box::new(cfg.clone())));
    assert!(
        h.core.monitor.as_ref().unwrap().monitored,
        "the gate stays as it was"
    );
    cfg.version = 4;
    cfg.policy.monitor = None;
    h.inp(Input::Config(Box::new(cfg)));
    assert_eq!(h.core.monitor, None);
    assert_eq!(h.core.holds(None), vec![]);
}

/// The gate is in the journal: replay gives the live state (H's shadow
/// core checks every input), and the next boot starts unmonitored again.
#[test]
fn the_gate_survives_replay_and_a_boot_starts_unmonitored() {
    let (mut h, _, _) = gated_with_orders();
    let t0 = h.now + SEC;
    fresh(&mut h, t0);
    ticks(&mut h, t0, t0 + 31 * SEC);
    assert!(unmonitored(&h));
    let mut replayed = Core::new(7);
    for r in &h.log {
        for e in &r.events {
            replayed.apply(r.at, e).unwrap();
        }
    }
    assert_eq!(replayed, h.core);
    assert_eq!(replayed.monitor, h.core.monitor);
    let t = h.now + SEC;
    fresh(&mut h, t);
    assert!(!unmonitored(&h));
    h.inp(Input::Boot {
        binary: "test".into(),
        kill_marker: None,
    });
    assert!(h.core.holds(None).contains(&HoldReason::Unmonitored));
}

/// The policy's bounds (판단(모델 아님)): each refused one step outside
/// and taken at the edge.
#[test]
fn the_gate_values_are_bounded() {
    let with = |f: fn(&mut MonitorPolicy)| {
        let mut cfg = gated();
        f(cfg.policy.monitor.as_mut().unwrap());
        cfg.validate()
    };
    assert_eq!(
        MonitorPolicy::default(),
        MonitorPolicy {
            heartbeat_max_age_us: 30 * SEC,
            check_max_age_us: 10 * SEC,
            unmonitored_cancel_us: 60 * SEC,
        }
    );
    assert_eq!(gated().validate(), Ok(()));
    let edges: [fn(&mut MonitorPolicy); 6] = [
        |m| m.heartbeat_max_age_us = 10 * SEC,
        |m| m.heartbeat_max_age_us = 300 * SEC,
        |m| m.check_max_age_us = 2 * SEC,
        |m| m.check_max_age_us = 60 * SEC,
        |m| m.unmonitored_cancel_us = 10 * SEC,
        |m| m.unmonitored_cancel_us = 600 * SEC,
    ];
    for f in edges {
        assert_eq!(with(f), Ok(()));
    }
    let outside: [(fn(&mut MonitorPolicy), &str); 6] = [
        (
            |m| m.heartbeat_max_age_us = 10 * SEC - 1,
            "heartbeat_max_age_us",
        ),
        (
            |m| m.heartbeat_max_age_us = 300 * SEC + 1,
            "heartbeat_max_age_us",
        ),
        (|m| m.check_max_age_us = 2 * SEC - 1, "check_max_age_us"),
        (|m| m.check_max_age_us = 60 * SEC + 1, "check_max_age_us"),
        (
            |m| m.unmonitored_cancel_us = 10 * SEC - 1,
            "unmonitored_cancel_us",
        ),
        (
            |m| m.unmonitored_cancel_us = 600 * SEC + 1,
            "unmonitored_cancel_us",
        ),
    ];
    for (f, because) in outside {
        let e = with(f).unwrap_err().join("; ");
        assert!(e.contains(because), "{because}: {e}");
    }
}
