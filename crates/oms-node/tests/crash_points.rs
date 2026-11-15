//! The node is stopped at one named boundary of the commit sequence
//! (docs/contracts/commit.md, "경계별 장애 결과") and restarted. Each test
//! checks the row of that table: what the journal holds, what the strategy
//! saw, what reached the venue, what the restarted node does, and what a
//! repeated request gets.
//!
//! The stop is `abort()` from inside the commit stage, so the operating
//! system keeps what was written: this is a process crash, not a power loss.
//! Needs `--features failpoints`; the shipped binary has no such code.

#![allow(clippy::unwrap_used)]

mod common;

use common::*;
use oms_api::client::ClientError;
use oms_core::*;
use std::time::Duration;

/// Starts a node that aborts at `point` when it commits a new order, sends
/// one order, waits for the process to die, and restarts it without the
/// failpoint. Returns the environment, the request (for repeating it
/// unchanged) and what the strategy saw.
fn crash_then_restart(name: &str, point: &str) -> (Env, oms_api::Op, Result<NewOrderReply, ClientError>) {
    let mut env = Env::new(name);
    env.node_env = vec![("OMS_CRASH_AT".into(), point.into())];
    env.start_node();
    env.wait_ready();
    let op = order_op("k1", Side::Buy, "1", "100", Tif::Gtc, 1_500);
    let mut c = env.client("a");
    let seen = c.new_order(op.clone());
    let exit = env.wait_exit(Duration::from_secs(5));
    assert!(env.node.is_none(), "the node did not stop at {point}");
    assert_ne!(exit, Some(0));
    assert!(env.log_tail(5).contains(point), "{}", env.log_tail(5));
    // Nothing reached the venue at any of these boundaries.
    assert_eq!(env.venue_dump()["place_requests"], 0);

    env.node_env.clear();
    env.start_node();
    (env, op, seen)
}

fn settle_as_not_placed(env: &mut Env, id: OrderId) {
    // The restarted node knows the order, does not know whether it was sent,
    // and holds new orders until it is resolved.
    assert!(env.wait(Duration::from_secs(5), || env.try_status().is_some()));
    let mut c = env.client("a");
    assert!(env.wait(Duration::from_secs(5), || c.get_order(id).is_ok_and(|o| o.is_some())));
    // After the send window plus the margin, a query that finds nothing
    // closes it. It was never sent again.
    assert!(
        env.wait(Duration::from_secs(10), || c
            .get_order(id)
            .unwrap()
            .is_some_and(|o| matches!(o.close, Some(Close { reason: CloseReason::NotPlaced { .. }, .. })))),
        "order: {:?}",
        c.get_order(id)
    );
    env.wait_ready();
    let dump = env.venue_dump();
    assert_eq!(dump["place_requests"], 0, "the order was sent after the restart");
    assert_eq!(env.status()["core"]["stats"]["accepted"], 1);
}

/// ④~⑤: stopped before anything was written.
#[test]
fn stopped_before_the_write_the_request_never_happened() {
    let (mut env, op, seen) = crash_then_restart("crash-before-write", "before_write");
    assert!(seen.is_err(), "no reply may leave before the record is durable: {seen:?}");
    env.wait_ready();
    let st = env.status();
    assert_eq!(st["core"]["orders_in_memory"], 0);
    assert_eq!(st["core"]["stats"]["accepted"], 0);
    // The strategy repeats the same request: it is evaluated afresh, once.
    let mut c = env.client("a");
    let again = order_op("k1", Side::Buy, "1", "100", Tif::Gtc, 5_000);
    let _ = op;
    assert!(matches!(c.new_order(again).unwrap(), NewOrderReply::Accepted { .. }));
    assert!(env.wait(Duration::from_secs(5), || env.status()["core"]["working"] == 1));
    assert_eq!(env.venue_dump()["place_requests"], 1);
}

/// ⑤~⑥: the bytes were written, the flush was not confirmed. The process
/// died, the operating system did not, so the record is there.
#[test]
fn stopped_after_the_write_the_order_exists_and_is_resolved_without_a_send() {
    let (mut env, op, seen) = crash_then_restart("crash-after-write", "after_write");
    assert!(seen.is_err(), "no reply may leave before the flush is confirmed: {seen:?}");
    assert!(env.wait(Duration::from_secs(5), || env.try_status().is_some()));
    let mut c = env.client("a");
    // The same request again is the same order, not a second one.
    let NewOrderReply::Duplicate { order_id, .. } = c.new_order(op).unwrap() else {
        panic!("the repeated request was not recognised as a duplicate")
    };
    let o = c.get_order(order_id).unwrap().unwrap();
    assert!(o.close.is_some() || o.doubt.as_ref().is_some_and(|d| d.reason == DoubtReason::Restart), "{o:?}");
    settle_as_not_placed(&mut env, order_id);
}

/// ⑥~⑧: durable, nothing released.
#[test]
fn stopped_after_the_flush_the_order_is_in_doubt_and_never_sent() {
    let (mut env, op, seen) = crash_then_restart("crash-after-sync", "after_sync");
    assert!(seen.is_err(), "the reply is released after the read model, which did not happen: {seen:?}");
    assert!(env.wait(Duration::from_secs(5), || env.try_status().is_some()));
    let mut c = env.client("a");
    let NewOrderReply::Duplicate { order_id, .. } = c.new_order(op).unwrap() else {
        panic!("the repeated request was not recognised as a duplicate")
    };
    settle_as_not_placed(&mut env, order_id);
}

/// ⑧~⑨: the strategy has its answer, the gateway never got the command.
#[test]
fn stopped_after_the_reply_the_accepted_order_is_never_sent_and_closes_as_not_placed() {
    let (mut env, op, seen) = crash_then_restart("crash-after-reply", "after_reply");
    let NewOrderReply::Accepted { order_id } = seen.expect("the reply left before the stop") else {
        panic!("not accepted")
    };
    assert!(env.wait(Duration::from_secs(5), || env.try_status().is_some()));
    let mut c = env.client("a");
    assert!(matches!(c.new_order(op).unwrap(), NewOrderReply::Duplicate { order_id: again, .. } if again == order_id));
    settle_as_not_placed(&mut env, order_id);
}

/// The flush reports an error and the process lives: nothing of the batch is
/// released, the node stops with status 2, and the restart finds the record
/// that did reach the file.
#[test]
fn a_flush_error_after_the_write_stops_the_node_and_the_record_may_survive() {
    let mut env = Env::new("crash-sync-error");
    env.node_env = vec![("OMS_CRASH_AT".into(), "sync_error_after_write".into())];
    env.start_node();
    env.wait_ready();
    let op = order_op("k1", Side::Buy, "1", "100", Tif::Gtc, 1_500);
    let mut c = env.client("a");
    let seen = c.new_order(op.clone());
    assert!(!matches!(seen, Ok(NewOrderReply::Accepted { .. })), "accepted without a confirmed flush: {seen:?}");
    assert_eq!(env.wait_exit(Duration::from_secs(5)), Some(2), "{}", env.log_tail(5));
    assert_eq!(env.venue_dump()["place_requests"], 0);

    env.node_env.clear();
    env.start_node();
    assert!(env.wait(Duration::from_secs(5), || env.try_status().is_some()));
    let mut c = env.client("a");
    let NewOrderReply::Duplicate { order_id, .. } = c.new_order(op).unwrap() else {
        panic!("the record written before the failed flush was not found")
    };
    settle_as_not_placed(&mut env, order_id);
}
