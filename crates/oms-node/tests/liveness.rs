//! Strategy liveness through a real node (C-LIVE, U18b3): a strategy whose
//! last trading connection closes, or that sends no request for its
//! liveness time, loses its link; the node cancels its resting plain orders
//! at the venue and holds its plain orders until it says `hello` again.
//! Real `oms-node` and `simvenue` processes, real time.

#![allow(clippy::unwrap_used)]

mod common;
use common::*;
use oms_api::client::Client;
use oms_api::*;
use oms_core::*;
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::time::{Duration, Instant};

/// A node with a margin policy (so that a reduce-only order passes a
/// plain-only hold), strategies `a` and `b` listed in `[liveness]` at
/// 5,000 ms, ready, with the reference price set.
fn live_node(name: &str) -> Env {
    let mut env = Env::new(name);
    env.extra_runtime = "margin_poll_ms = 100".into();
    env.extra_policy = "\n[core.policy.margin]\naccount_modes_allowed = [\"standard\"]\n".into();
    env.extra_tables = "[liveness]\nstrategies = { a = 5000, b = 5000 }\n".into();
    env.ctl(json!({"account_value": {"value": "100000"}}));
    env.start_node();
    env.wait_ready();
    wait_margin(&env, "the account value", |m| {
        m["account_value"] == "100000"
    });
    env.ctl(json!({"ref_price": {"instrument": "X", "px": "100"}}));
    env
}

fn ping(c: &mut Client) {
    assert_eq!(c.call(Op::Ping).unwrap(), json!("pong"));
}

fn set(ids: &[&str]) -> BTreeSet<String> {
    ids.iter().map(|s| s.to_string()).collect()
}

/// Waits until the venue's open orders are exactly `want`.
fn wait_open_at_venue(env: &mut Env, want: &BTreeSet<String>, within: Duration) {
    let deadline = Instant::now() + within;
    let mut open = open_at_venue(env);
    while open != *want && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
        open = open_at_venue(env);
    }
    assert_eq!(open, *want, "open at the venue");
}

fn link(env: &Env, strategy: &str) -> Value {
    env.status()["strategies"][strategy]["link"].clone()
}

fn holds(env: &Env, strategy: &str) -> Value {
    env.status()["strategies"][strategy]["holds"].clone()
}

/// The `cancel_requested` events of the journal asked by the liveness
/// rule, by order.
fn rule_cancels(env: &Env) -> Vec<OrderId> {
    env.journal_records()
        .iter()
        .flat_map(|r| r.events.iter())
        .filter_map(|e| match e {
            Event::CancelRequested {
                order_id,
                by: Actor::Rule(RuleId::StrategyLink),
            } => Some(*order_id),
            _ => None,
        })
        .collect()
}

/// Done criterion (U18b): closing a strategy's last connection sends
/// cancel requests for that strategy's plain orders only. `a`'s plain order
/// is cancelled at the venue; `a`'s reduce-only order and `b`'s plain order
/// stay. `a` is held, `b` trades.
#[test]
fn closing_the_last_connection_cancels_that_strategys_plain_orders_only() {
    let mut env = live_node("live-close");
    let mut a = env.client("a");
    let mut b = env.client("b");
    let a_plain = rest(&mut env, &mut a, "a-plain", Side::Buy, "90", false);
    let a_reduce = rest(&mut env, &mut a, "a-reduce", Side::Sell, "120", true);
    let b_plain = rest(&mut env, &mut b, "b-plain", Side::Buy, "90", false);
    let all = set(&[
        &env.cloid(a_plain),
        &env.cloid(a_reduce),
        &env.cloid(b_plain),
    ]);
    wait_open_at_venue(&mut env, &all, Duration::from_secs(10));
    let l = link(&env, "a");
    assert_eq!(
        (
            l["liveness_ms"].as_u64(),
            l["connections"].as_u64(),
            l["held"].as_bool()
        ),
        (Some(5_000), Some(1), Some(false)),
        "{l}"
    );

    drop(a);
    let left = set(&[&env.cloid(a_reduce), &env.cloid(b_plain)]);
    wait_open_at_venue(&mut env, &left, Duration::from_secs(10));
    assert_eq!(rule_cancels(&env), [a_plain]);
    assert_eq!(env.journal_count("strategy_link_lost"), 1);
    let l = link(&env, "a");
    assert_eq!(l["held"], true, "{l}");
    assert_eq!(l["why"], "last trading connection closed");
    assert_eq!(l["connections"], 0);
    assert!(l["since"].is_u64(), "{l}");
    assert!(
        holds(&env, "a")
            .as_array()
            .unwrap()
            .contains(&json!("strategy_link"))
    );
    assert!(
        !holds(&env, "b")
            .as_array()
            .unwrap()
            .contains(&json!("strategy_link"))
    );
    assert!(
        !env.status()["core"]["holds"]
            .as_array()
            .unwrap()
            .contains(&json!("strategy_link"))
    );
    // `b` still trades.
    rest(&mut env, &mut b, "b-after", Side::Buy, "90", false);
    ping(&mut b);
}

/// Done criterion (U18b): 5 s without any request sends cancel requests
/// for that strategy's plain orders only; `ping` counts as a request. On
/// the connection that stayed open the strategy's plain orders are refused
/// and its reduce-only orders taken until it says `hello` again.
#[test]
fn five_seconds_without_a_request_cancel_and_a_ping_keeps_it_alive() {
    let mut env = live_node("live-silent");
    let mut a = env.client("a");
    let mut b = env.client("b");
    let a_plain = rest(&mut env, &mut a, "a-plain", Side::Buy, "90", false);
    let a_reduce = rest(&mut env, &mut a, "a-reduce", Side::Sell, "120", true);
    let b_plain = rest(&mut env, &mut b, "b-plain", Side::Buy, "90", false);
    let all = set(&[
        &env.cloid(a_plain),
        &env.cloid(a_reduce),
        &env.cloid(b_plain),
    ]);
    wait_open_at_venue(&mut env, &all, Duration::from_secs(10));

    // Pings only, every second, for 8 s: longer than the liveness time.
    for _ in 0..8 {
        std::thread::sleep(Duration::from_secs(1));
        ping(&mut a);
        ping(&mut b);
    }
    assert_eq!(
        env.journal_count("strategy_link_lost"),
        0,
        "{}",
        env.log_tail(10)
    );
    assert_eq!(open_at_venue(&mut env), all);

    // `a` falls silent; `b` keeps pinging.
    let last = Instant::now();
    let mut lost_after = None;
    while last.elapsed() < Duration::from_secs(8) {
        std::thread::sleep(Duration::from_millis(500));
        ping(&mut b);
        if lost_after.is_none() && env.journal_count("strategy_link_lost") > 0 {
            lost_after = Some(last.elapsed());
        }
    }
    let lost_after = lost_after.expect("a was never lost");
    assert!(
        lost_after > Duration::from_millis(5_000) && lost_after < Duration::from_millis(7_000),
        "lost {lost_after:?} after a's last request"
    );
    let left = set(&[&env.cloid(a_reduce), &env.cloid(b_plain)]);
    wait_open_at_venue(&mut env, &left, Duration::from_secs(10));
    assert_eq!(rule_cancels(&env), [a_plain]);
    let l = link(&env, "a");
    assert_eq!(
        (l["why"].as_str(), l["connections"].as_u64()),
        (Some("no request for 5000 ms"), Some(1)),
        "{l}"
    );

    // The connection is still open: plain refused, reduce-only taken.
    let reasons = match a
        .new_order(order_op(
            "a-plain-2",
            Side::Buy,
            "1",
            "90",
            Tif::Gtc,
            60_000,
        ))
        .unwrap()
    {
        NewOrderReply::Rejected { reasons } => reasons,
        other => panic!("{other:?}"),
    };
    assert!(
        reasons.contains(&RejectReason::Hold(HoldReason::StrategyLink)),
        "{reasons:?}"
    );
    rest(&mut env, &mut a, "a-reduce-2", Side::Sell, "120", true);
    // A hello on the same connection restores it.
    a.call(Op::Hello {
        strategy: Some(StrategyId::new("a").unwrap()),
        client: "test".into(),
    })
    .unwrap();
    assert_eq!(env.journal_count("strategy_link_restored"), 1);
    assert_eq!(
        link(&env, "a")["connections"],
        1,
        "the same connection counts once"
    );
    rest(&mut env, &mut a, "a-plain-3", Side::Buy, "90", false);
}

/// A second connection of the strategy keeps it alive when the first one
/// closes; its last close loses it.
#[test]
fn a_second_connection_keeps_the_strategy_alive() {
    let mut env = live_node("live-second");
    let mut a1 = env.client("a");
    let mut a2 = env.client("a");
    let id = rest(&mut env, &mut a1, "a-plain", Side::Buy, "90", false);
    let one = set(&[&env.cloid(id)]);
    wait_open_at_venue(&mut env, &one, Duration::from_secs(10));
    assert_eq!(link(&env, "a")["connections"], 2);
    drop(a1);
    for _ in 0..6 {
        std::thread::sleep(Duration::from_secs(1));
        ping(&mut a2);
    }
    assert_eq!(env.journal_count("strategy_link_lost"), 0);
    assert_eq!(open_at_venue(&mut env), one);
    assert_eq!(link(&env, "a")["connections"], 1);
    drop(a2);
    wait_open_at_venue(&mut env, &BTreeSet::new(), Duration::from_secs(10));
    assert_eq!(rule_cancels(&env), [id]);
}

/// A `hello` after a loss is answered only once the restore is durable:
/// the moment the answer arrives, the published state (durable records
/// only) has no hold and the journal files hold the restore. A hold
/// replayed after a restart is released by the next hello too.
#[test]
fn hello_after_a_loss_is_answered_after_the_restore_is_durable() {
    let mut env = live_node("live-hello");
    let mut a = env.client("a");
    let id = rest(&mut env, &mut a, "a-plain", Side::Buy, "90", false);
    let one = set(&[&env.cloid(id)]);
    wait_open_at_venue(&mut env, &one, Duration::from_secs(10));
    drop(a);
    assert!(
        env.wait(Duration::from_secs(10), || link(&env, "a")["held"] == true),
        "never held: {}",
        env.log_tail(10)
    );

    let mut a = env.client("a");
    // The same connection, at once: the published state is durable only.
    let st = a.call(Op::Status).unwrap();
    assert_eq!(
        st["strategies"]["a"]["link"]["held"], false,
        "{}",
        st["strategies"]["a"]
    );
    assert!(
        !st["strategies"]["a"]["holds"]
            .as_array()
            .unwrap()
            .contains(&json!("strategy_link"))
    );
    assert_eq!(env.journal_count("strategy_link_restored"), 1);
    let id = rest(&mut env, &mut a, "a-plain-2", Side::Buy, "90", false);
    let one = set(&[&env.cloid(id)]);
    wait_open_at_venue(&mut env, &one, Duration::from_secs(10));

    // Lost again, then a restart: the hold is replayed, and the next hello
    // releases it before it is answered.
    drop(a);
    assert!(env.wait(Duration::from_secs(10), || link(&env, "a")["held"] == true));
    assert_eq!(env.stop_node(), 0);
    env.start_node();
    assert!(
        env.wait(Duration::from_secs(20), || env.try_status().is_some_and(
            |s| s["strategies"]["a"]["holds"]
                .as_array()
                .is_some_and(|h| h.contains(&json!("strategy_link")))
        )),
        "the hold was not replayed: {}",
        env.log_tail(20)
    );
    let mut a = env.client("a");
    let st = a.call(Op::Status).unwrap();
    assert_eq!(
        st["strategies"]["a"]["link"]["held"], false,
        "{}",
        st["strategies"]["a"]
    );
    assert_eq!(env.journal_count("strategy_link_restored"), 2);
    assert_eq!(env.journal_count("strategy_link_lost"), 2);
}
