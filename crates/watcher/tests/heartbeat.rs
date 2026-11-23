//! The `monitor.sock` heartbeat client (C-WATCH part 7, U19a7): one
//! persistent connection, `hello` without a strategy first, a heartbeat
//! line with each check's last success, reopened at the next heartbeat
//! after a failure; against a fake socket. The engine's heartbeats carried
//! by the real client, on the engine rig and a manual clock.

#![allow(clippy::unwrap_used)]

mod common;

use common::rig::*;
use common::sock::{FakeSock, Reply, err, oms};
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;
use watcher::engine::{Action, Check};
use watcher::metrics::Metrics;
use watcher::oms::Monitor;
use watcher::rest::RestError;
use watcher::w2::W2;
use watcher::w3::W3;
use watcher::w4::W4;

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

fn monitor(
    path: &std::path::Path,
    timeout_ms: u64,
) -> (Monitor, Arc<Metrics>, Arc<std::sync::Mutex<Vec<String>>>) {
    let metrics = Arc::new(Metrics::new("h", "w"));
    let (log, lines) = watcher::log::collect();
    let m = Monitor::new(path, "w-1", timeout_ms, metrics.clone(), log);
    (m, metrics, lines)
}

fn count(lines: &std::sync::Mutex<Vec<String>>, pat: &str) -> usize {
    lines
        .lock()
        .unwrap()
        .iter()
        .filter(|l| l.contains(pat))
        .count()
}

const NONE: [(Check, Option<u64>); 4] = [
    (Check::W1, None),
    (Check::W2, None),
    (Check::W3, None),
    (Check::W4, None),
];

#[test]
fn the_heartbeat_says_hello_first_and_reconnects() {
    let dir = common::test_dir("hb-hello");
    let path = dir.join("monitor.sock");
    let (mut m, metrics, lines) = monitor(&path, 1000);
    let rt = rt();
    // No socket (an OMS without [core.policy.monitor]): one try per
    // heartbeat, one line for the change of state.
    for k in 0..3 {
        assert!(rt.block_on(m.beat(T0 + k * 5000, &NONE)).is_err());
    }
    assert_eq!(m.tries(), 3);
    assert_eq!(count(&lines, "monitor.sock not connected"), 1);
    assert_eq!(metrics.get("watcher_heartbeat_ok_ts", &[]), None);
    // The OMS closes the first connection after its second heartbeat; it
    // refuses the fourth heartbeat line of the second with bad_request.
    let fake = FakeSock::serve(&path, |conn, i, v| match (conn, i) {
        (0, 3) => Reply::Close,
        (1, 3) | (1, 4) => err(v, "bad_request", "W1 is ahead of the record time"),
        _ => oms(v, "recorded"),
    });
    let checks = [
        (Check::W1, Some(T0 + 14_000)),
        (Check::W2, Some(T0 + 13_000)),
        (Check::W3, None),
        (Check::W4, None),
    ];
    rt.block_on(m.beat(T0 + 15_000, &checks)).unwrap();
    assert_eq!(count(&lines, "monitor.sock connected"), 1);
    assert_eq!(
        metrics.get("watcher_heartbeat_ok_ts", &[]),
        Some((T0 + 15_000) as f64 / 1000.0)
    );
    rt.block_on(m.beat(T0 + 20_000, &checks)).unwrap();
    let c = fake.conns();
    assert_eq!(c.len(), 1);
    assert_eq!(
        c[0][0],
        json!({"id": 1, "op": "hello", "client": "watcher/w-1"})
    );
    assert_eq!(
        c[0][1],
        json!({"id": 2, "op": "heartbeat", "watcher": "w-1",
            "checks": {"W1": T0 + 14_000, "W2": T0 + 13_000, "W3": null, "W4": null}})
    );
    // Closed by the OMS: this heartbeat fails, the next one reopens with
    // hello first.
    assert!(rt.block_on(m.beat(T0 + 25_000, &checks)).is_err());
    assert_eq!(count(&lines, "monitor.sock not connected"), 2);
    rt.block_on(m.beat(T0 + 30_000, &checks)).unwrap();
    let c = fake.conns();
    assert_eq!(c.len(), 2);
    assert_eq!(c[1][0]["op"], "hello");
    assert_eq!(c[1][1]["op"], "heartbeat");
    assert_eq!(m.tries(), 5);
    // bad_request: one error line a minute, the connection kept.
    rt.block_on(m.beat(T0 + 35_000, &checks)).unwrap();
    assert!(rt.block_on(m.beat(T0 + 40_000, &checks)).is_err());
    assert!(rt.block_on(m.beat(T0 + 45_000, &checks)).is_err());
    assert_eq!(
        count(&lines, "error: monitor.sock heartbeat refused (bad_request"),
        1
    );
    rt.block_on(m.beat(T0 + 50_000, &checks)).unwrap();
    assert_eq!(fake.conns().len(), 2);
    assert_eq!(fake.conns()[1].len(), 6);
    assert_eq!(
        metrics.get("watcher_heartbeat_ok_ts", &[]),
        Some((T0 + 50_000) as f64 / 1000.0)
    );
}

#[test]
fn a_reply_that_does_not_come_in_time_reopens_the_connection() {
    let dir = common::test_dir("hb-timeout");
    let path = dir.join("monitor.sock");
    let (mut m, metrics, lines) = monitor(&path, 200);
    let fake = FakeSock::serve(&path, |conn, i, v| match (conn, i) {
        (0, 1) => Reply::Silent,
        (1, 2) => err(v, "overloaded", "2 per second"),
        _ => oms(v, "recorded"),
    });
    let rt = rt();
    let t = std::time::Instant::now();
    assert!(rt.block_on(m.beat(T0, &NONE)).is_err());
    assert!(
        t.elapsed() < Duration::from_millis(1000),
        "{:?}",
        t.elapsed()
    );
    assert_eq!(metrics.get("watcher_heartbeat_ok_ts", &[]), None);
    assert_eq!(count(&lines, "monitor.sock not connected"), 1);
    // The next heartbeat opens a new connection.
    rt.block_on(m.beat(T0 + 5000, &NONE)).unwrap();
    assert!(fake.wait(Duration::from_secs(5), |s| s.closed[0]));
    let c = fake.conns();
    assert_eq!(c.len(), 2);
    assert_eq!(c[1][0]["op"], "hello");
    // Another error answer: one error line, the connection kept.
    assert!(rt.block_on(m.beat(T0 + 10_000, &NONE)).is_err());
    assert_eq!(
        count(&lines, "error: monitor.sock heartbeat refused (overloaded"),
        1
    );
    rt.block_on(m.beat(T0 + 15_000, &NONE)).unwrap();
    assert_eq!(fake.conns().len(), 2);
}

/// The checks of a heartbeat line, as the OMS reads them.
fn checks_of(v: &Value) -> Vec<(String, Option<u64>)> {
    ["W1", "W2", "W3", "W4"]
        .iter()
        .map(|c| (c.to_string(), v["checks"][c].as_u64()))
        .collect()
}

#[test]
fn heartbeats_carry_the_success_time_of_each_check() {
    let mut r = Rig::new("hb-success");
    r.engine.add_check(Box::new(W2::default()), 1000);
    let path = r.journal.dir.join("monitor.sock");
    let fake = FakeSock::serve(&path, |_, _, v| oms(v, "recorded"));
    let (mut m, _, _) = monitor(&path, 1000);
    let rt = rt();
    let mut want = Vec::new();
    for k in 0..16 {
        let t = T0 + k * 1000;
        if k == 5 {
            // W2 fails from here on; W1 keeps succeeding.
            for _ in 0..20 {
                r.venue
                    .pos_script
                    .push_back(Err(RestError::NoAnswer("timeout".into())));
            }
        }
        let a = r.engine.poll(t);
        for x in &a {
            if let Action::Heartbeat { checks } = x {
                let s = |c| r.engine.success(c);
                want.push(vec![
                    ("W1".to_string(), s(Check::W1)),
                    ("W2".to_string(), s(Check::W2)),
                    ("W3".to_string(), None),
                    ("W4".to_string(), None),
                ]);
                rt.block_on(m.beat(t, checks)).unwrap();
            }
        }
        r.run(t, a);
    }
    let got: Vec<_> = fake.conns()[0]
        .iter()
        .filter(|v| v["op"] == "heartbeat")
        .map(checks_of)
        .collect();
    assert_eq!(got, want);
    // Null before the first success; both at T0 + 4 s; then W2 stays.
    let at = |k: u64| Some(T0 + k * 1000);
    let w = |w1, w2| {
        vec![
            ("W1".to_string(), w1),
            ("W2".to_string(), w2),
            ("W3".to_string(), None),
            ("W4".to_string(), None),
        ]
    };
    assert_eq!(
        got,
        vec![
            w(None, None),
            w(at(4), at(4)),
            w(at(9), at(4)),
            w(at(14), at(4))
        ]
    );
}

#[test]
fn w3_and_w4_success_times_reach_the_heartbeat() {
    // W1 and W2 every 1 s, W3 every 2 s, W4 every 5 s; a heartbeat every
    // 5 s, carried by the real client to a fake monitor.sock.
    let mut r = Rig::new("hb-w3-w4");
    let cfg = config();
    let w4 = W4::new(&cfg.rules, r.log.clone());
    r.engine
        .add_check(Box::new(W2::with_exec_times(w4.exec_times())), 1000);
    r.engine.add_check(Box::new(W3::new(&cfg.w3)), 2000);
    r.engine.add_check(Box::new(w4), 5000);
    r.venue.wallet = vec![account("1000", "0", "1000")];
    let path = r.journal.dir.join("monitor.sock");
    let fake = FakeSock::serve(&path, |_, _, v| oms(v, "recorded"));
    let (mut m, _, _) = monitor(&path, 1000);
    let rt = rt();
    let mut want = Vec::new();
    for k in 0..16 {
        let t = T0 + k * 1000;
        if k == 5 {
            // W3 fails from here on; W4 keeps succeeding.
            for _ in 0..20 {
                r.venue
                    .wallet_script
                    .push_back(Err(RestError::NoAnswer("timeout".into())));
            }
        }
        let a = r.engine.poll(t);
        for x in &a {
            if let Action::Heartbeat { checks } = x {
                let s = |c| r.engine.success(c);
                want.push(vec![
                    ("W1".to_string(), s(Check::W1)),
                    ("W2".to_string(), s(Check::W2)),
                    ("W3".to_string(), s(Check::W3)),
                    ("W4".to_string(), s(Check::W4)),
                ]);
                rt.block_on(m.beat(t, checks)).unwrap();
            }
        }
        r.run(t, a);
    }
    let got: Vec<_> = fake.conns()[0]
        .iter()
        .filter(|v| v["op"] == "heartbeat")
        .map(checks_of)
        .collect();
    assert_eq!(got, want);
    // Null before the first success; W3 stays at its last success (T0 +
    // 4 s) once its rounds fail; W4 moves with every sweep.
    let at = |k: u64| Some(T0 + k * 1000);
    let w34: Vec<_> = got.iter().map(|g| (g[2].1, g[3].1)).collect();
    assert_eq!(
        w34,
        vec![(None, None), (at(4), at(0)), (at(4), at(5)), (at(4), at(10))]
    );
    assert!(got[1..].iter().all(|g| g[0].1.is_some() && g[1].1.is_some()));
}
