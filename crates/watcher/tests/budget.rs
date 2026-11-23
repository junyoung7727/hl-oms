//! The request budget (C-WATCH part 9, U19a9), measured, not enforced:
//! W1, W2, W3 and W4 at their design intervals (1 s, 1 s, 2 s, 5 s) on the
//! engine rig and a manual clock, for one simulated minute. Every venue
//! request passes the real request guard (`rest::Guard`, 60 in any 10 s)
//! before the model venue answers it; a request the guard holds back
//! waits, first come first served, and asks again at every step of 10 ms
//! (the client asks again at least every 100 ms; 판단(모델 아님)). Status
//! requests (the OMS, not Bybit) are answered at once. The design budget
//! is 1 + 1 + 0.5 + 0.2 = 2.7 requests per second (§7.5).

#![allow(clippy::unwrap_used)]

mod common;

use common::ev;
use common::rig::*;
use serde_json::{Value, json};
use std::collections::VecDeque;
use watcher::engine::{Action, Check};
use watcher::rest::Guard;
use watcher::w2::W2;
use watcher::w3::W3;
use watcher::w4::W4;

/// One simulated minute.
const MINUTE: u64 = 60_000;
/// The step of the manual clock (판단(모델 아님)).
const STEP: u64 = 10;

const W1_PATH: &str = "/v5/order/realtime";
const W2_PATH: &str = "/v5/position/list";
const W3_PATH: &str = "/v5/account/wallet-balance";
const W4_PATH: &str = "/v5/execution/list";

/// A venue request at the guard: its id, path and params.
type Waiting = (u64, &'static str, Vec<(&'static str, String)>);

/// The engine with all four checks, the guard and the requests that wait
/// at it.
struct Sim {
    r: Rig,
    guard: Guard,
    waiting: VecDeque<Waiting>,
    /// When each venue request was answered, and its path.
    sent: Vec<(u64, &'static str)>,
    /// When the current W1 round asked its first page.
    w1_start: Option<u64>,
    /// Each W1 round that succeeded: its first page's emission and its
    /// success.
    w1_rounds: Vec<(u64, u64)>,
    /// Each W4 sweep that completed: its window `(startTime, endTime)`
    /// and the clock of its comparison.
    w4_sweeps: Vec<(u64, u64, u64)>,
    w4_window: Option<(u64, u64)>,
}

impl Sim {
    fn new(name: &str) -> Sim {
        let mut r = Rig::new(name);
        let cfg = config();
        let w4 = W4::new(&cfg.rules, r.log.clone());
        r.engine
            .add_check(Box::new(W2::with_exec_times(w4.exec_times())), 1000);
        r.engine.add_check(Box::new(W3::new(&cfg.w3)), 2000);
        r.engine.add_check(Box::new(w4), 5000);
        r.venue.wallet = vec![account("1000", "0", "1000")];
        Sim {
            r,
            guard: Guard::default(),
            waiting: VecDeque::new(),
            sent: Vec::new(),
            w1_start: None,
            w1_rounds: Vec::new(),
            w4_sweeps: Vec::new(),
            w4_window: None,
        }
    }

    /// Polls at `now`, then lets through every waiting request the guard
    /// admits, answering it and what its answer brings.
    fn step(&mut self, now: u64) {
        let (w1, w4) = (
            self.r.engine.success(Check::W1),
            self.r.engine.success(Check::W4),
        );
        let mut q: VecDeque<Action> = self.r.engine.poll(now).into();
        loop {
            while let Some(a) = q.pop_front() {
                match a {
                    Action::Venue {
                        req_id,
                        path,
                        params,
                    } => {
                        if path == W1_PATH && param(&params, "cursor").is_none() {
                            self.w1_start = Some(now);
                        }
                        self.waiting.push_back((req_id, path, params));
                    }
                    // A status answer brings no venue request: a status
                    // ends a round.
                    other => self.r.run(now, vec![other]),
                }
            }
            if self.waiting.is_empty() || self.guard.admit(now).is_err() {
                break;
            }
            let (req_id, path, params) = self.waiting.pop_front().unwrap();
            if path == W4_PATH && param(&params, "cursor").is_none() {
                let p = |k| param(&params, k).unwrap().parse::<u64>().unwrap();
                self.w4_window = Some((p("startTime"), p("endTime")));
            }
            self.sent.push((now, path));
            let answer = self.r.venue.answer(path, &params);
            let more = self.r.engine.on_venue(now, req_id, answer);
            // The next page waits at the guard; a status request (and the
            // sends its answer brings) goes to the rig at once.
            q.extend(more);
        }
        let s = self.r.engine.success(Check::W1);
        if s != w1 {
            let start = self.w1_start.take().unwrap();
            self.w1_rounds.push((start, s.unwrap()));
        }
        if self.r.engine.success(Check::W4) != w4 {
            let (from, to) = self.w4_window.unwrap();
            self.w4_sweeps.push((from, to, now));
        }
    }

    /// Requests answered in `[from, to)` on `path` (all paths: `None`).
    fn count(&self, path: Option<&str>, from: u64, to: u64) -> usize {
        self.sent
            .iter()
            .filter(|(t, p)| *t >= from && *t < to && path.is_none_or(|x| x == *p))
            .count()
    }
}

/// `external_fill_booked` of the BTCUSDT buy `BTCUSDT:<id>`.
fn ext(id: &str, at_ms: u64) -> Value {
    ev(
        "external_fill_booked",
        json!({"fill": {"fee": "0", "instrument": "BTCUSDT",
            "key": format!("BTCUSDT:{id}"), "order_id": null, "px": "100", "qty": "1",
            "side": "buy", "venue_oid": null, "venue_ts": at_ms * 1000}}),
    )
}

/// One minute with `n` executions every 5 s spread evenly (execution `i`
/// at `T0 + i × 5000 / n` ms); each is booked by the journal at the step
/// it happens in, and the venue's position follows it.
fn minute(name: &str, n: u64) -> Sim {
    let mut s = Sim::new(name);
    let mut i = 0u64;
    let mut t = T0;
    while t < T0 + MINUTE {
        let mut events = Vec::new();
        while T0 + i * 5000 / n <= t {
            let at = T0 + i * 5000 / n;
            let id = format!("e{i}");
            s.r.venue
                .executions
                .push(exec("BTCUSDT", &id, "Trade", "Buy", "1", at));
            events.push(ext(&id, at));
            i += 1;
            s.r.venue.positions = vec![pos("BTCUSDT", "Buy", &i.to_string(), Some(at))];
        }
        if !events.is_empty() {
            s.r.write(t, &events);
        }
        s.step(t);
        t += STEP;
    }
    s
}

/// Requests per second in the minute, and the lines for the report.
fn report(s: &Sim, label: &str) -> f64 {
    let total = s.count(None, T0, T0 + MINUTE);
    let rate = total as f64 / 60.0;
    println!(
        "{label}: requests in [0, 60 s): {total} = W1 {} + W2 {} + W3 {} + W4 {} ({rate:.3} per second); W4 sweeps {}",
        s.count(Some(W1_PATH), T0, T0 + MINUTE),
        s.count(Some(W2_PATH), T0, T0 + MINUTE),
        s.count(Some(W3_PATH), T0, T0 + MINUTE),
        s.count(Some(W4_PATH), T0, T0 + MINUTE),
        s.w4_sweeps.len(),
    );
    rate
}

#[test]
fn at_30_executions_per_5_s_the_reconciliation_rate_in_a_simulated_minute_stays_at_or_below_2_7() {
    let s = minute("budget-30", 30);
    let rate = report(&s, "30 per 5 s");
    // One page for each W1, W2 and W3 round; W4 one page a sweep: a 15 s
    // window holds about 90 rows.
    assert_eq!(s.count(Some(W1_PATH), T0, T0 + MINUTE), 60);
    assert_eq!(s.count(Some(W2_PATH), T0, T0 + MINUTE), 60);
    assert_eq!(s.count(Some(W3_PATH), T0, T0 + MINUTE), 30);
    assert_eq!(s.count(Some(W4_PATH), T0, T0 + MINUTE), 12);
    assert!(s.count(None, T0, T0 + MINUTE) <= 162);
    assert!(rate <= 2.7, "{rate}");
    // Every check kept up and nothing differed.
    assert_eq!(s.w4_sweeps.len(), 12);
    assert!(s.r.mismatches.is_empty(), "{:?}", s.r.mismatches);
    assert!(s.r.confirmed.is_empty());
    for c in ["W1", "W2", "W3", "W4"] {
        assert_eq!(s.r.lines_with(&format!("{c} round failed")), 0, "{c}");
    }
}

#[test]
fn at_100_executions_per_5_s_the_rate_is_about_3_1_and_w4_keeps_up() {
    let s = minute("budget-100", 100);
    let rate = report(&s, "100 per 5 s");
    assert_eq!(s.count(Some(W1_PATH), T0, T0 + MINUTE), 60);
    assert_eq!(s.count(Some(W2_PATH), T0, T0 + MINUTE), 60);
    assert_eq!(s.count(Some(W3_PATH), T0, T0 + MINUTE), 30);
    // Above the design's 2.7: the scope's W4 rules (10 s overlap, every
    // cursor) read a 15 s window of about 300 rows, 3 or 4 pages (owner
    // question 1).
    assert!(rate > 2.7, "{rate}");
    assert!(rate <= 3.4, "{rate}");
    // W4 keeps up: every sweep completes in its own round, and every
    // window after the first starts at most overlap + interval + 1 s
    // before its end.
    assert_eq!(s.w4_sweeps.len(), 12);
    for (k, (from, to, at)) in s.w4_sweeps.iter().enumerate() {
        assert_eq!(*at, T0 + k as u64 * 5000, "sweep {k}");
        assert_eq!(*to, *at, "sweep {k}");
        if k > 0 {
            assert!(to - from <= 16_000, "sweep {k}: {from}..{to}");
        }
    }
    assert!(s.r.mismatches.is_empty(), "{:?}", s.r.mismatches);
    assert!(s.r.confirmed.is_empty());
    for c in ["W1", "W2", "W3", "W4"] {
        assert_eq!(s.r.lines_with(&format!("{c} round failed")), 0, "{c}");
    }
}

#[test]
fn at_500_open_orders_the_guard_stretches_w1_rounds_below_10_s() {
    let mut s = Sim::new("budget-500");
    // 500 acked journal orders, all open at the venue: 11 W1 pages a round
    // (10 full ones and an empty last one), 110 a second at 1 s rounds.
    for id in 1..=500 {
        s.r.write(T0 - 60_000, &open(id));
        s.r.venue.orders.push(ours(id, "BTCUSDT", "Buy"));
    }
    let mut t = T0;
    while t < T0 + MINUTE {
        s.step(t);
        t += STEP;
    }
    // The guard held: never more than 60 requests in any 10 s.
    for (k, (at, _)) in s.sent.iter().enumerate() {
        let n = s.sent[k..]
            .iter()
            .take_while(|(u, _)| *u < at + 10_000)
            .count();
        assert!(n <= 60, "{n} requests in the 10 s from {at}");
    }
    // The W1 and W2 ages at every heartbeat after the first stay below
    // the OMS gate's 10 s.
    let beats = &s.r.heartbeats;
    assert_eq!(beats.len(), 12);
    assert!(beats[0].1.iter().all(|(_, t)| t.is_none()));
    let mut worst = 0;
    for (at, checks) in &beats[1..] {
        for c in [Check::W1, Check::W2] {
            let t = checks.iter().find(|x| x.0 == c).unwrap().1;
            let age = at - t.unwrap_or_else(|| panic!("{c:?} null at {at}"));
            worst = worst.max(age);
            assert!(age < 10_000, "{c:?} {age} ms old at {at}");
        }
    }
    let rounds: Vec<u64> = s.w1_rounds.iter().map(|(a, b)| b - a).collect();
    let mean = rounds.iter().sum::<u64>() / rounds.len() as u64;
    println!(
        "500 open orders: {} W1 rounds in 60 s, round time mean {mean} ms, max {} ms; W1/W2 worst age at a heartbeat {worst} ms; requests W1 {} W2 {} W3 {} W4 {}",
        rounds.len(),
        rounds.iter().max().unwrap(),
        s.count(Some(W1_PATH), T0, T0 + MINUTE),
        s.count(Some(W2_PATH), T0, T0 + MINUTE),
        s.count(Some(W3_PATH), T0, T0 + MINUTE),
        s.count(Some(W4_PATH), T0, T0 + MINUTE),
    );
    assert!(s.r.mismatches.is_empty(), "{:?}", s.r.mismatches);
    for c in ["W1", "W2"] {
        assert_eq!(s.r.lines_with(&format!("{c} round failed")), 0, "{c}");
    }
}
