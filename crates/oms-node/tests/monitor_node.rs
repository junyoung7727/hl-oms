//! The monitoring gate through a real node (C-LIVE part 2, U18b4): the
//! watcher's heartbeats arrive on `monitor.sock`, are journaled, and open
//! or close the gate; resting plain orders are cancelled at the venue once
//! the account has been unmonitored for `unmonitored_cancel_us`. Real
//! `oms-node` and `simvenue` processes, real time.

#![allow(clippy::unwrap_used)]

mod common;
use common::*;
use oms_api::client::{Client, now_us};
use oms_api::*;
use oms_core::*;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;

fn monitor_sock(env: &Env) -> std::path::PathBuf {
    env.data_dir().join("monitor.sock")
}

/// A node with the gate (`extra` lines in `[core.policy.monitor]`), link up
/// and synced, unmonitored since its start (a first start: the gate comes
/// on with the configuration record).
fn gated_node(name: &str, extra: &str) -> Env {
    let mut env = Env::new(name);
    env.extra_policy = format!("\n[core.policy.monitor]\n{extra}\n");
    env.start_node();
    let ok = env.wait(Duration::from_secs(20), || {
        env.try_status().is_some_and(|s| {
            s["core"]["holds"] == json!(["unmonitored"])
                && s["core"]["needs"].as_array().is_some_and(|n| n.is_empty())
                && s["core"]["boot"].as_u64().unwrap_or(0) > 0
        })
    });
    assert!(
        ok,
        "the node did not come up unmonitored: {}",
        env.log_tail(30)
    );
    env
}

fn monitor(env: &Env) -> Value {
    env.status()["core"]["monitor"].clone()
}

/// Waits until the venue's open orders are exactly `want`.
fn wait_open_at_venue(env: &mut Env, want: &BTreeSet<String>, within: Duration) {
    let deadline = std::time::Instant::now() + within;
    let mut open = open_at_venue(env);
    while open != *want && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
        open = open_at_venue(env);
    }
    assert_eq!(open, *want, "open at the venue");
}

fn now_ms() -> u64 {
    now_us() / 1_000
}

/// A heartbeat whose W1, W2 and W3 last succeeded at `w1`, `w2`, now.
fn heartbeat(w1: u64, w2: u64) -> Op {
    let mut checks = BTreeMap::new();
    checks.insert("W1".to_string(), Some(w1));
    checks.insert("W2".to_string(), Some(w2));
    checks.insert("W3".to_string(), Some(now_ms()));
    checks.insert("W4".to_string(), None);
    Op::Heartbeat {
        watcher: "watcher-1".into(),
        checks,
    }
}

fn fresh() -> Op {
    let t = now_ms();
    heartbeat(t, t)
}

/// Sends raw lines and reads `n` answers.
fn raw(path: &std::path::Path, lines: &str, n: usize) -> Vec<Value> {
    let mut s = UnixStream::connect(path).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    s.write_all(lines.as_bytes()).unwrap();
    let mut r = BufReader::new(s.try_clone().unwrap()).lines();
    (0..n)
        .map(|_| serde_json::from_str(&r.next().unwrap().unwrap()).unwrap())
        .collect()
}

/// Done criterion (U18b): heartbeats over `monitor.sock` open the gate,
/// and every other op there is forbidden; `heartbeat` on the other sockets
/// is forbidden. The node starts unmonitored and refuses plain orders until
/// the first fresh heartbeat; the exact line of the contract is taken; a
/// stale W2 closes the gate again, a check from the future is refused and
/// recorded nowhere; more than two heartbeats in a second on one
/// connection are refused; the socket has its own pool of two.
#[test]
fn heartbeats_over_monitor_sock_open_the_gate_and_other_ops_are_forbidden() {
    let env = gated_node("monitor-gate", "");
    use std::os::unix::fs::PermissionsExt;
    let mode = std::fs::metadata(monitor_sock(&env))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
    let m = monitor(&env);
    assert_eq!(
        (
            &m["gate"],
            &m["monitored"],
            &m["reason"],
            &m["cancel_stage"],
            &m["watcher"]
        ),
        (
            &json!(true),
            &json!(false),
            &json!("no heartbeat since the gate came on"),
            &json!("waiting"),
            &Value::Null
        ),
        "{m}"
    );
    let mut a = env.client("a");
    let r = a
        .new_order(order_op("before", Side::Buy, "1", "90", Tif::Gtc, 5_000))
        .unwrap();
    assert_eq!(
        r,
        NewOrderReply::Rejected {
            reasons: vec![RejectReason::Hold(HoldReason::Unmonitored)]
        }
    );

    // The pool of two, and the ops the socket refuses.
    let mut w = Client::connect(&monitor_sock(&env), None).unwrap();
    let spare = Client::connect(&monitor_sock(&env), None).unwrap();
    assert!(
        Client::connect(&monitor_sock(&env), None).is_err(),
        "a third connection"
    );
    drop(spare);
    assert_eq!(w.call(Op::Ping).unwrap(), json!("pong"));
    for op in [
        Op::Status,
        Op::ListOpen,
        order_op("m", Side::Buy, "1", "90", Tif::Gtc, 5_000),
        Op::Halt {
            strategy: None,
            note: "n".into(),
        },
        Op::WatcherKill { detail: "d".into() },
        Op::Snapshot,
    ] {
        assert_eq!(
            api_error(w.call(op.clone())),
            ErrorCode::Forbidden,
            "{op:?}"
        );
    }
    let hello_a = raw(
        &monitor_sock(&env),
        "{\"id\":1,\"op\":\"hello\",\"strategy\":\"a\"}\n",
        1,
    );
    assert_eq!(
        hello_a[0]["error"]["code"], "unauthorized",
        "{}",
        hello_a[0]
    );
    // `heartbeat` on the trading and the admin socket.
    assert_eq!(api_error(a.call(fresh())), ErrorCode::Forbidden);
    assert_eq!(api_error(env.admin().call(fresh())), ErrorCode::Forbidden);
    assert_eq!(env.journal_count("watcher_heartbeat"), 0);

    // The contract's line, with times of now: recorded after durable, the
    // gate opens and a plain order is taken.
    let t = now_ms();
    let line = format!(
        "{{\"id\":1,\"op\":\"hello\",\"strategy\":null}}\n{{\"id\": 7, \"op\": \"heartbeat\", \"watcher\": \"watcher-1\", \"checks\": {{\"W1\": {t}, \"W2\": {t}, \"W3\": {}, \"W4\": null}}}}\n",
        t - 1_000
    );
    let answers = raw(&monitor_sock(&env), &line, 2);
    assert_eq!(
        answers[1],
        json!({"id": 7, "ok": true, "result": "recorded"})
    );
    assert_eq!(
        env.journal_count("watcher_heartbeat"),
        1,
        "durable before the answer"
    );
    let m = monitor(&env);
    assert_eq!(
        (&m["monitored"], &m["watcher"], &m["cancel_stage"]),
        (&json!(true), &json!("watcher-1"), &json!("none")),
        "{m}"
    );
    assert!(m["checks"]["W3"].as_u64().unwrap() >= 1_000, "{m}");
    assert_eq!(m["checks"]["W4"], Value::Null);
    assert_eq!(env.status()["core"]["holds"], json!([]));
    accept(&mut a, "after", Side::Buy, "1", "90");

    // A check from the future: refused, recorded nowhere.
    let ahead = now_ms() + 60_000;
    assert_eq!(
        api_error(w.call(heartbeat(ahead, now_ms()))),
        ErrorCode::BadRequest
    );
    assert_eq!(env.journal_count("watcher_heartbeat"), 1);
    // A stale W2 closes the gate at once.
    std::thread::sleep(Duration::from_millis(1_100));
    let t = now_ms();
    assert_eq!(w.call(heartbeat(t, t - 11_000)).unwrap(), json!("recorded"));
    let m = monitor(&env);
    assert_eq!(m["monitored"], false, "{m}");
    assert_eq!(
        m["reason"],
        "W2 last succeeded more than 10000 ms before the heartbeat"
    );
    assert_eq!(env.status()["core"]["holds"], json!(["unmonitored"]));
    assert_eq!(env.journal_count("monitor_changed"), 2);

    // Two heartbeats a second per connection: of four sent at once, two
    // are recorded and two refused `overloaded`.
    std::thread::sleep(Duration::from_millis(1_100));
    let before = env.journal_count("watcher_heartbeat");
    let mut ids = Vec::new();
    for _ in 0..4 {
        ids.push(w.send(fresh()).unwrap());
    }
    let mut got = BTreeMap::new();
    while got.len() < 4 {
        let r = w.read_response().unwrap();
        if ids.contains(&r.id) {
            got.insert(r.id, r.error.map(|e| e.code));
        }
    }
    let refused = got
        .values()
        .filter(|c| **c == Some(ErrorCode::Overloaded))
        .count();
    assert_eq!((got.len() - refused, refused), (2, 2), "{got:?}");
    assert_eq!(env.journal_count("watcher_heartbeat"), before + 2);
    assert_eq!(monitor(&env)["monitored"], true);
}

/// Stage 2 through the node, with the shortest values the bounds allow
/// (10 s): a stale W2 unmonitors at once; the resting plain order is
/// cancelled at the venue only after 10 s of that, by the gate's rule; a
/// reduce-only order is not (margin policy set, so it rests).
#[test]
fn a_stale_heartbeat_unmonitors_and_the_node_cancels_resting_plain_orders_later() {
    let mut env = Env::new("monitor-stage2");
    env.extra_runtime = "margin_poll_ms = 100".into();
    env.extra_policy = "\n[core.policy.margin]\naccount_modes_allowed = [\"standard\"]\n\n[core.policy.monitor]\nheartbeat_max_age_us = 60000000\nunmonitored_cancel_us = 10000000\n".into();
    env.ctl(json!({"account_value": {"value": "100000"}}));
    env.start_node();
    let mut w = None;
    let ok = env.wait(Duration::from_secs(20), || {
        if w.is_none() {
            w = Client::connect(&monitor_sock(&env), None).ok();
        }
        if let Some(c) = w.as_mut() {
            let _ = c.call(fresh());
        }
        env.try_status().is_some_and(|s| {
            s["core"]["holds"] == json!([])
                && s["core"]["needs"].as_array().is_some_and(|n| n.is_empty())
                && s["core"]["margin"]["account_value"] == "100000"
        })
    });
    assert!(ok, "the node did not open the gate: {}", env.log_tail(30));
    let mut w = w.unwrap();
    env.ctl(json!({"ref_price": {"instrument": "X", "px": "100"}}));
    let mut a = env.client("a");
    let plain = rest(&mut env, &mut a, "plain", Side::Buy, "90", false);
    let reduce = rest(&mut env, &mut a, "reduce", Side::Sell, "120", true);
    let both: BTreeSet<String> = [env.cloid(plain), env.cloid(reduce)].into();
    wait_open_at_venue(&mut env, &both, Duration::from_secs(10));

    // The wait above sent heartbeats as fast as it polled.
    std::thread::sleep(Duration::from_millis(1_100));
    let t = now_ms();
    assert_eq!(w.call(heartbeat(t, t - 11_000)).unwrap(), json!("recorded"));
    let since = std::time::Instant::now();
    assert_eq!(monitor(&env)["cancel_stage"], "waiting");
    std::thread::sleep(Duration::from_secs(8));
    assert_eq!(
        open_at_venue(&mut env),
        both,
        "nothing cancelled before 10 s"
    );
    let left: BTreeSet<String> = [env.cloid(reduce)].into();
    wait_open_at_venue(&mut env, &left, Duration::from_secs(10));
    assert!(
        since.elapsed() >= Duration::from_secs(10),
        "{:?}",
        since.elapsed()
    );
    assert_eq!(monitor(&env)["cancel_stage"], "cancelling");
    let by_rule: Vec<OrderId> = env
        .journal_records()
        .iter()
        .flat_map(|r| r.events.iter())
        .filter_map(|e| match e {
            Event::CancelRequested {
                order_id,
                by: Actor::Rule(RuleId::Unmonitored),
            } => Some(*order_id),
            _ => None,
        })
        .collect();
    assert_eq!(by_rule, [plain]);
    // A restart starts unmonitored again, whatever the last heartbeat was.
    assert_eq!(w.call(fresh()).unwrap(), json!("recorded"));
    assert_eq!(monitor(&env)["monitored"], true);
    drop(w);
    assert_eq!(env.stop_node(), 0);
    env.start_node();
    let ok = env.wait(Duration::from_secs(20), || {
        env.try_status().is_some_and(|s| {
            s["core"]["monitor"]["reason"] == "no heartbeat since the start"
                && s["core"]["monitor"]["monitored"] == false
                && s["core"]["holds"]
                    .as_array()
                    .is_some_and(|h| h.contains(&json!("unmonitored")))
        })
    });
    assert!(ok, "after the restart: {}", monitor(&env));
}

/// Without the gate there is no monitor socket, no hold and no `monitor`
/// state beyond `gate: false`; `heartbeat` on the admin socket is
/// forbidden.
#[test]
fn without_the_gate_there_is_no_monitor_socket() {
    let env = Env::started("monitor-none");
    assert!(!monitor_sock(&env).exists());
    assert_eq!(monitor(&env), json!({"gate": false}));
    assert_eq!(api_error(env.admin().call(fresh())), ErrorCode::Forbidden);
    assert_eq!(env.journal_count("watcher_heartbeat"), 0);
}
