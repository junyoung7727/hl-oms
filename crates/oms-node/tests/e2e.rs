//! Process-level acceptance tests.
//!
//! Real components: the `oms-node` binary with its threads, queues, local
//! sockets and journal files; the `simvenue` binary in a second process.
//! Simulated: the venue's behaviour. A crash is a SIGKILL of the node
//! process, which loses whatever was only in its memory; it does not lose
//! data the kernel had accepted, so it says nothing about power loss.

#![allow(clippy::unwrap_used)]

mod common;
use common::*;
use oms_api::client::ClientError;
use oms_api::*;
use oms_core::*;
use serde_json::json;
use std::time::Duration;

fn s(v: &serde_json::Value) -> String {
    v.as_str().unwrap_or_default().to_string()
}

/// The first vertical slice: new order, partial fill, cancel of the rest,
/// journal, clean restart, same state.
#[test]
fn vertical_slice_new_partial_fill_cancel_restart() {
    let mut env = Env::started("slice");
    let mut c = env.client("a");
    c.call(Op::Subscribe).unwrap();

    // Durably accepted.
    let id = accept(&mut c, "k1", Side::Buy, "1", "100");
    // Acknowledged by the venue.
    assert!(env.wait(Duration::from_secs(5), || phase_of(&mut c, id) == Phase::Working));
    let o = c.get_order(id).unwrap().unwrap();
    assert!(o.venue_oid.is_some());
    assert_eq!(o.reserved, Qty::parse("1").unwrap());

    // Partial fill of 0.4 at the venue.
    let cloid = env.cloid(id);
    env.ctl(json!({"fill": {"client": cloid, "qty": "0.4"}}));
    assert!(env.wait(Duration::from_secs(5), || {
        c.get_order(id).unwrap().unwrap().cum_qty == Qty::parse("0.4").unwrap()
    }));

    // Cancel the rest: the reservation stays until the venue confirms.
    assert_eq!(c.cancel(id).unwrap(), CancelReply::Requested { order_id: id });
    assert!(env.wait(Duration::from_secs(5), || phase_of(&mut c, id) == Phase::Closed));
    let o = c.get_order(id).unwrap().unwrap();
    assert_eq!(o.cum_qty, Qty::parse("0.4").unwrap());
    assert_eq!(o.reserved, Qty::ZERO);
    assert!(matches!(o.close.unwrap().reason, CloseReason::Canceled { .. }));

    let st = env.status();
    assert_eq!(s(&st["account"]["X"]["pos"]), "0.4");
    assert_eq!(s(&st["account"]["X"]["buy_open"]), "0");
    assert_eq!(s(&st["strategies"]["a"]["books"]["X"]["pos"]), "0.4");

    // The subscriber saw the order's life in journal order.
    let mut names = Vec::new();
    let mut last_seq = 0;
    while names.last().map(String::as_str) != Some("order_closed") {
        let ev = c.next_event().unwrap();
        assert!(ev.seq > last_seq || names.is_empty() || ev.seq == last_seq);
        last_seq = ev.seq;
        assert_eq!(ev.order_id, id);
        names.extend(ev.events.iter().map(|e| e.name().to_string()));
    }
    for expected in ["order_accepted", "order_acked", "fill_booked", "cancel_requested", "cancel_sent", "venue_terminal", "order_closed"] {
        assert!(names.contains(&expected.to_string()), "{expected} missing from {names:?}");
    }
    assert_eq!(names[0], "order_accepted");

    // The venue saw exactly one placement.
    let dump = env.venue_dump();
    assert_eq!(dump["place_requests"], 1);
    assert_eq!(s(&dump["positions"]["X"]), "0.4");

    // Clean stop, restart: the state is the same and nothing is re-sent.
    let digest_before = env.status()["node"]["recovery"]["state_digest"].clone();
    drop(c);
    assert_eq!(env.stop_node(), 0);
    let (code, out, err) = env.tool(&["verify", "--re-decide"]);
    assert_eq!(code, 0, "verify failed: {err}");
    let report: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(report["re_decided"], true);

    env.start_node();
    env.wait_ready();
    let st = env.status();
    assert_ne!(st["node"]["recovery"]["state_digest"], digest_before);
    assert!(st["node"]["recovery"]["snapshot_seq"].is_u64(), "the clean stop left a snapshot");
    assert_eq!(st["core"]["boot"], 2);
    let mut c = env.client("a");
    let o = c.get_order(id).unwrap().unwrap();
    assert_eq!((o.phase, o.cum_qty), (Phase::Closed, Qty::parse("0.4").unwrap()));
    assert_eq!(s(&st["account"]["X"]["pos"]), "0.4");
    assert_eq!(env.venue_dump()["place_requests"], 1);

    // The offline trace shows the order's records.
    let (code, out, _) = env.tool(&["trace", "--order", &id.to_string()]);
    assert_eq!(code, 0);
    assert!(out.lines().count() >= 4);
    assert!(out.lines().next().unwrap().contains("order_accepted"));
}

/// Idempotency over the real socket, concurrently and across a restart.
#[test]
fn a_repeated_request_never_creates_a_second_order() {
    let mut env = Env::started("idem");
    let op = order_op("same-key", Side::Buy, "1", "100", Tif::Gtc, 20_000);

    // Eight connections race the same request.
    let sock = env.sock();
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let (op, sock) = (op.clone(), sock.clone());
            std::thread::spawn(move || {
                let mut c = oms_api::client::Client::connect(&sock, Some("a")).unwrap();
                c.new_order(op).unwrap()
            })
        })
        .collect();
    let replies: Vec<NewOrderReply> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    let accepted: Vec<_> = replies.iter().filter(|r| matches!(r, NewOrderReply::Accepted { .. })).collect();
    assert_eq!(accepted.len(), 1, "{replies:?}");
    let NewOrderReply::Accepted { order_id } = accepted[0] else { unreachable!() };
    assert!(replies.iter().all(|r| match r {
        NewOrderReply::Accepted { order_id: o } | NewOrderReply::Duplicate { order_id: o, .. } => o == order_id,
        _ => false,
    }));

    // A different request under the same key is refused as a conflict.
    let mut c = env.client("a");
    let Op::NewOrder { client_order_id, instrument, side, px, tif, reduce_only, expires_at, .. } = op.clone() else {
        unreachable!()
    };
    let other = Op::NewOrder {
        client_order_id,
        instrument,
        side,
        qty: Qty::parse("2").unwrap(),
        px,
        tif,
        reduce_only,
        expires_at,
    };
    assert_eq!(c.new_order(other).unwrap(), NewOrderReply::Conflict { order_id: *order_id });

    // After a crash the same request is still recognised.
    assert!(env.wait(Duration::from_secs(5), || phase_of(&mut c, *order_id) == Phase::Working));
    drop(c);
    env.kill_node();
    env.start_node();
    env.wait_ready();
    let mut c = env.client("a");
    assert!(matches!(c.new_order(op).unwrap(), NewOrderReply::Duplicate { order_id: o, .. } if o == *order_id));
    assert_eq!(env.venue_dump()["place_requests"], 1);
    assert_eq!(env.status()["core"]["orders_in_memory"], 1);
}

/// The response to a placement is lost, but the venue's event stream reports
/// the order. That evidence is enough: the order is acknowledged, the late
/// "no answer" from the gateway changes nothing, and nothing is sent again.
#[test]
fn a_lost_response_is_covered_by_stream_evidence() {
    let mut env = Env::started("lostresp");
    let mut c = env.client("a");
    env.ctl(json!({"drop_responses": {"n": 1}}));
    let id = accept(&mut c, "k1", Side::Buy, "1", "100");
    assert!(env.wait(Duration::from_secs(5), || phase_of(&mut c, id) == Phase::Working));
    // Let the gateway's timeout (300 ms) pass and its result be processed.
    std::thread::sleep(Duration::from_millis(600));
    let o = c.get_order(id).unwrap().unwrap();
    assert_eq!((o.phase, o.doubt), (Phase::Working, None));
    let (_, out, _) = env.tool(&["trace", "--order", &id.to_string()]);
    assert!(out.contains(r#""via":"stream""#), "{out}");
    assert!(!out.contains("order_in_doubt"));
    assert_eq!(env.venue_dump()["place_requests"], 1);
    env.wait_ready();
}

/// The request itself is lost: nothing exists at the venue. The order stays
/// in doubt, holding its reservation and blocking new exposure, until the
/// send window plus margin has passed and the venue still does not know it.
#[test]
fn a_lost_request_is_closed_only_after_its_send_window() {
    let mut env = Env::started("lostreq");
    let mut c = env.client("a");
    env.ctl(json!({"drop_requests": {"n": 1}}));
    let t0 = std::time::Instant::now();
    let reply = c.new_order(order_op("k1", Side::Buy, "1", "100", Tif::Gtc, 1_500)).unwrap();
    let NewOrderReply::Accepted { order_id: id } = reply else { panic!("{reply:?}") };

    // In doubt after the gateway timeout.
    assert!(env.wait(Duration::from_secs(3), || c.get_order(id).unwrap().unwrap().doubt.is_some()));
    let st = env.status();
    assert_eq!(st["core"]["holds"], json!(["in_doubt"]));
    assert_eq!(s(&st["account"]["X"]["buy_open"]), "1", "the reservation is kept while the outcome is unknown");
    // New exposure is refused meanwhile.
    let r = c.new_order(order_op("k2", Side::Buy, "1", "100", Tif::Gtc, 5_000)).unwrap();
    assert_eq!(r, NewOrderReply::Rejected { reasons: vec![RejectReason::Hold(HoldReason::InDoubt)] });

    // Closed as not placed, but not before expiry (1.5 s) + margin (0.3 s).
    assert!(env.wait(Duration::from_secs(8), || phase_of(&mut c, id) == Phase::Closed));
    assert!(t0.elapsed() >= Duration::from_millis(1_800), "closed after only {:?}", t0.elapsed());
    let o = c.get_order(id).unwrap().unwrap();
    assert!(matches!(o.close.unwrap().reason, CloseReason::NotPlaced { .. }));
    assert_eq!(env.venue_dump()["place_requests"], 0);
    env.wait_ready();
    assert_eq!(s(&env.status()["account"]["X"]["buy_open"]), "0");
}

/// The node is killed while a placement is on the wire. After restart the
/// order is in doubt, is found at the venue, and was placed exactly once.
#[test]
fn a_kill_during_send_does_not_duplicate_the_order() {
    let mut env = Env::started("killsend");
    let mut c = env.client("a");
    env.ctl(json!({"delay_ms": {"ms": 400}}));
    let id = accept(&mut c, "k1", Side::Buy, "1", "100");
    assert_eq!(phase_of(&mut c, id), Phase::PendingSend);
    drop(c);
    env.kill_node(); // the venue is still holding the request
    env.ctl(json!({"delay_ms": {"ms": 0}}));
    std::thread::sleep(Duration::from_millis(500)); // the venue processes it

    env.start_node();
    env.wait_ready();
    let mut c = env.client("a");
    let o = c.get_order(id).unwrap().unwrap();
    assert_eq!(o.phase, Phase::Working);
    let (_, out, _) = env.tool(&["trace", "--order", &id.to_string()]);
    assert!(out.contains(r#""reason":"restart""#), "restart must put the outstanding send in doubt");
    let dump = env.venue_dump();
    assert_eq!((dump["place_requests"].as_u64(), dump["repeated_place_requests"].as_u64()), (Some(1), Some(0)));
}

/// While the node is down the venue fills one order, cancels another and a
/// third stays open. After restart the node learns all of it from the venue.
#[test]
fn events_missed_while_down_are_recovered_from_the_venue() {
    let mut env = Env::started("missed");
    let mut c = env.client("a");
    let filled = accept(&mut c, "k1", Side::Buy, "1", "100");
    let cancelled = accept(&mut c, "k2", Side::Sell, "2", "101");
    let resting = accept(&mut c, "k3", Side::Buy, "3", "99");
    assert!(env.wait(Duration::from_secs(5), || env.status()["core"]["working"] == 3));
    drop(c);
    env.kill_node();

    let (f, x, r) = (env.cloid(filled), env.cloid(cancelled), env.cloid(resting));
    env.ctl(json!({"fill": {"client": f}}));
    env.ctl(json!({"fill": {"client": x, "qty": "0.5"}}));
    env.ctl(json!({"venue_cancel": {"client": x}}));
    env.ctl(json!({"fill": {"client": r, "qty": "1"}}));

    env.start_node();
    env.wait_ready();
    let mut c = env.client("a");
    let o = c.get_order(filled).unwrap().unwrap();
    assert_eq!((o.phase, o.cum_qty), (Phase::Closed, Qty::parse("1").unwrap()));
    let o = c.get_order(cancelled).unwrap().unwrap();
    assert_eq!((o.phase, o.cum_qty), (Phase::Closed, Qty::parse("0.5").unwrap()));
    let o = c.get_order(resting).unwrap().unwrap();
    assert_eq!((o.phase, o.cum_qty, o.reserved), (Phase::Working, Qty::parse("1").unwrap(), Qty::parse("2").unwrap()));
    let st = env.status();
    // +1 (k1) -0.5 (k2) +1 (k3) = 1.5
    assert_eq!(s(&st["account"]["X"]["pos"]), "1.5");
    assert_eq!(s(&env.venue_dump()["positions"]["X"]), "1.5");
    assert_eq!(st["core"]["mismatches_unacked"], json!([]));
}

/// Link loss blocks new orders; when the stream returns, trading resumes only
/// after the state was confirmed again.
#[test]
fn link_loss_holds_new_orders_until_resynchronised() {
    let mut env = Env::started("link");
    let mut c = env.client("a");
    let id = accept(&mut c, "k1", Side::Buy, "1", "100");
    assert!(env.wait(Duration::from_secs(5), || phase_of(&mut c, id) == Phase::Working));

    env.ctl(json!({"refuse_streams": {"on": true}}));
    env.ctl(json!("kick_streams"));
    assert!(env.wait(Duration::from_secs(5), || env.status()["core"]["link"]["up"] == false));
    let r = c.new_order(order_op("k2", Side::Buy, "1", "100", Tif::Gtc, 5_000)).unwrap();
    assert_eq!(r, NewOrderReply::Rejected { reasons: vec![RejectReason::Hold(HoldReason::LinkDown)] });
    // A cancel is still accepted and remembered.
    assert_eq!(c.cancel(id).unwrap(), CancelReply::Requested { order_id: id });
    // A fill that happens meanwhile is not seen on the stream.
    let cloid = env.cloid(id);
    env.ctl(json!({"fill": {"client": cloid, "qty": "0.25"}}));

    env.ctl(json!({"refuse_streams": {"on": false}}));
    env.wait_ready();
    // The pending cancel was sent after the link returned, and the missed
    // fill was recovered by backfill.
    assert!(env.wait(Duration::from_secs(5), || phase_of(&mut c, id) == Phase::Closed));
    let o = c.get_order(id).unwrap().unwrap();
    assert_eq!(o.cum_qty, Qty::parse("0.25").unwrap());
    accept(&mut c, "k3", Side::Buy, "1", "100");
}

/// A trade the OMS did not send is booked, flagged and holds new exposure
/// until an operator acknowledges it.
#[test]
fn external_activity_is_detected_and_needs_an_operator() {
    let mut env = Env::started("external");
    let mut c = env.client("a");
    env.ctl(json!({"external_fill": {"instrument": "X", "side": "sell", "qty": "0.3", "px": "100"}}));
    assert!(env.wait(Duration::from_secs(5), || {
        env.status()["core"]["mismatches_unacked"].as_array().is_some_and(|m| !m.is_empty())
    }));
    let st = env.status();
    assert_eq!(s(&st["account"]["X"]["pos"]), "-0.3");
    assert_eq!(s(&st["unattributed"]["X"]["pos"]), "-0.3");
    let r = c.new_order(order_op("k1", Side::Buy, "1", "100", Tif::Gtc, 5_000)).unwrap();
    assert_eq!(r, NewOrderReply::Rejected { reasons: vec![RejectReason::Hold(HoldReason::Mismatch)] });

    // Operator commands are refused on the trading socket.
    let id = st["core"]["mismatches_unacked"][0]["id"].as_u64().unwrap() as u32;
    assert_eq!(
        api_error(c.call(Op::AckMismatch { mismatch_id: id, note: "x".into() })),
        ErrorCode::Forbidden
    );
    let mut admin = env.admin();
    admin.call(Op::AckMismatch { mismatch_id: id, note: "manual trade, confirmed on the venue UI".into() }).unwrap();
    env.wait_ready();
    accept(&mut c, "k2", Side::Buy, "1", "100");
}

/// Strategies are isolated from each other on the trading socket.
#[test]
fn a_strategy_sees_and_cancels_only_its_own_orders() {
    let env = Env::started("isolation");
    let mut a = env.client("a");
    let mut b = env.client("b");
    let id = accept(&mut a, "k1", Side::Buy, "1", "100");
    assert!(b.get_order(id).unwrap().is_none());
    assert_eq!(b.cancel(id).unwrap(), CancelReply::NotOwner);
    assert_eq!(b.call(Op::ListOpen).unwrap(), json!([]));
    assert!(matches!(
        oms_api::client::Client::connect(&env.sock(), Some("nobody")),
        Err(ClientError::Api(e)) if e.code == ErrorCode::Unauthorized
    ));
    // Malformed input is refused without disturbing the connection.
    assert_eq!(
        api_error(a.call(Op::Cancel { order_id: None, client_order_id: None })),
        ErrorCode::BadRequest
    );
    assert_eq!(a.call(Op::Ping).unwrap(), json!("pong"));
}

/// More requests than the queue can hold: the excess is refused immediately,
/// is not recorded, and cancels still get through.
#[test]
fn overload_is_refused_at_the_door_and_cancels_still_work() {
    let mut env = Env::new("overload");
    env.extra_runtime = "orders_queue = 8\nmax_batch = 4".into();
    // A wide window lets one connection outrun the small queue.
    env.extra_api = "window = 512".into();
    env.start_node();
    env.wait_ready();
    let mut c = env.client("a");
    let resting = accept(&mut c, "rest", Side::Buy, "1", "90");
    assert!(env.wait(Duration::from_secs(5), || phase_of(&mut c, resting) == Phase::Working));
    let before = env.status();
    let seq_before = before["journal"]["durable_seq"].as_u64().unwrap();
    let stat = |st: &serde_json::Value, name: &str| st["core"]["stats"][name].as_u64().unwrap();

    // Pipeline 600 requests without reading responses.
    let n = 600;
    for i in 0..n {
        c.send(order_op(&format!("f{i}"), Side::Buy, "0.001", "100", Tif::Gtc, 5_000)).unwrap();
    }
    let (mut accepted, mut rejected, mut overloaded) = (0, 0, 0);
    for _ in 0..n {
        let r = c.read_response().unwrap();
        match (r.ok, r.error) {
            (true, _) => match serde_json::from_value::<NewOrderReply>(r.result.unwrap()).unwrap() {
                NewOrderReply::Accepted { .. } => accepted += 1,
                _ => rejected += 1,
            },
            (false, Some(e)) if e.code == ErrorCode::Overloaded => overloaded += 1,
            (false, e) => panic!("{e:?}"),
        }
    }
    assert_eq!(accepted + rejected + overloaded, n);
    assert!(overloaded > 0, "the flood never filled the queue");
    assert!(accepted > 0);
    // A cancel is accepted right after the flood.
    assert_eq!(c.cancel(resting).unwrap(), CancelReply::Requested { order_id: resting });

    let st = env.status();
    println!(
        "accepted={accepted} rejected={rejected} overloaded={overloaded} queues={}",
        st["pipeline"]["queues"]
    );
    // Whatever was refused as overloaded left no record: the records written
    // are at most one per accepted or core-rejected request, plus what the
    // venue and the cancel produced.
    let hw = st["pipeline"]["queues"]["high_water"][2].as_u64().unwrap();
    assert!(hw <= 8, "orders queue grew to {hw}");
    assert_eq!(
        st["pipeline"]["ingress_refused_overload"].as_u64().unwrap(),
        overloaded as u64
    );
    assert!(st["journal"]["durable_seq"].as_u64().unwrap() > seq_before);
    // The core saw exactly the requests that were answered by it. A request
    // refused as overloaded never reached it, so it has no record: every
    // acceptance and every rejection the core decides is journaled, and
    // these counters are rebuilt from the journal.
    assert_eq!(stat(&st, "accepted") - stat(&before, "accepted"), accepted as u64);
    assert_eq!(stat(&st, "rejected") - stat(&before, "rejected"), rejected as u64);
}

/// An incomplete record at the end of the journal is preserved and removed;
/// damage in the middle stops the node.
#[test]
fn journal_damage_is_handled_by_position() {
    let mut env = Env::started("damage");
    let mut c = env.client("a");
    for i in 0..5 {
        accept(&mut c, &format!("k{i}"), Side::Buy, "1", "100");
    }
    assert!(env.wait(Duration::from_secs(5), || env.status()["core"]["working"] == 5));
    drop(c);
    env.kill_node();
    let seg = env.journal_segments().pop().unwrap();
    let good = std::fs::read(&seg).unwrap();

    // 1. Incomplete tail.
    let mut torn = good.clone();
    torn.extend_from_slice(b"\x40\x00\x00\x00half-written record");
    std::fs::write(&seg, &torn).unwrap();
    env.start_node();
    env.wait_ready();
    let st = env.status();
    let tail = &st["node"]["recovery"]["torn_tail"];
    assert!(tail["bytes"].as_u64().unwrap() > 0, "{tail}");
    assert!(exists(std::path::Path::new(tail["saved_to"].as_str().unwrap())));
    assert_eq!(st["core"]["working"], 5);
    env.kill_node();

    // 2. One flipped bit in the middle.
    let mut damaged = std::fs::read(&seg).unwrap();
    let mid = damaged.len() / 2;
    damaged[mid] ^= 0x04;
    std::fs::write(&seg, &damaged).unwrap();
    env.start_node();
    assert_eq!(env.wait_exit(Duration::from_secs(10)), Some(3), "the node must refuse to start");
    assert!(env.log_tail(5).contains("recovery failed"));
    assert_eq!(std::fs::read(&seg).unwrap(), damaged, "the damaged file is left as found");
    let (code, _, err) = env.tool(&["verify"]);
    assert_eq!(code, 1);
    assert!(err.contains("corrupt"), "{err}");
}

/// A second node on the same data directory is refused.
#[test]
fn only_one_node_can_own_a_data_directory() {
    let mut env = Env::started("lock");
    let second = std::process::Command::new(env!("CARGO_BIN_EXE_oms-node"))
        .arg("run")
        .arg("--config")
        .arg(env.config_path())
        .output()
        .unwrap();
    assert_eq!(second.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&second.stderr).contains("locked by another process"));
    // The first one is unaffected.
    let mut c = env.client("a");
    accept(&mut c, "k1", Side::Buy, "1", "100");
    assert_eq!(env.stop_node(), 0);
}

/// A changed configuration needs a new version; the same version with other
/// content is refused at startup.
#[test]
fn configuration_changes_need_a_new_version() {
    let mut env = Env::started("config");
    assert_eq!(env.stop_node(), 0);
    env.extra_policy = "cancel_max_attempts = 7".into();
    env.start_node();
    assert_eq!(env.wait_exit(Duration::from_secs(10)), Some(3));
    assert!(env.log_tail(5).contains("must exceed the active version"));
}

/// Random kills under load, repeatedly. After each restart the node's books
/// must agree with the venue's, and no order may have been placed twice.
#[test]
fn random_kills_under_load_keep_books_consistent() {
    let rounds: u32 = std::env::var("OMS_KILL_ROUNDS").ok().and_then(|v| v.parse().ok()).unwrap_or(12);
    let mut env = Env::new("kills");
    env.extra_runtime = "snapshot_every = 150".into();
    env.start_node();
    env.wait_ready();
    env.ctl(json!({"auto_fill": {"mode": "half"}}));
    let mut seed = 0x9e37_79b9_7f4a_7c15u64 ^ u64::from(std::process::id());
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let mut key = 0u64;
    let mut total_accepted = 0u64;
    for round in 0..rounds {
        // Load: two strategies send as fast as replies come back.
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let mut workers = Vec::new();
        for (w, strat) in ["a", "b"].into_iter().enumerate() {
            let (sock, stop) = (env.sock(), stop.clone());
            let base = key + (w as u64) * 1_000_000;
            workers.push(std::thread::spawn(move || {
                let Ok(mut c) = oms_api::client::Client::connect(&sock, Some(strat)) else { return 0u64 };
                let mut n = 0u64;
                let mut i = 0u64;
                // Each worker cancels its older orders, so that open orders
                // stay bounded and every round has room for new ones.
                let mut mine = std::collections::VecDeque::new();
                while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                    i += 1;
                    let side = if (i + w as u64).is_multiple_of(2) { Side::Buy } else { Side::Sell };
                    let op = order_op(&format!("r{}", base + i), side, "0.01", "100", Tif::Gtc, 700);
                    match c.new_order(op) {
                        Ok(NewOrderReply::Accepted { order_id }) => {
                            n += 1;
                            mine.push_back(order_id);
                        }
                        Ok(_) => {}
                        Err(_) => break, // the node died
                    }
                    if mine.len() > 4 {
                        let Some(old) = mine.pop_front() else { continue };
                        if c.cancel(old).is_err() {
                            break;
                        }
                    }
                }
                n
            }));
        }
        key += 10_000_000;
        std::thread::sleep(Duration::from_millis(60 + next() % 400));
        env.kill_node();
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        total_accepted += workers.into_iter().map(|h| h.join().unwrap()).sum::<u64>();

        env.start_node();
        env.wait_ready_for(Duration::from_secs(40));

        // Orders left open by the killed workers are cancelled, so the next
        // round starts with room. The comparison below still sees them.
        env.admin().call(Op::CancelAll { strategy: None, note: "between rounds".into() }).unwrap();
        env.wait_ready_for(Duration::from_secs(40));
        assert!(env.wait(Duration::from_secs(20), || env.status()["core"]["open_orders"] == 0), "round {round}: cancels did not complete");

        // Compare with the venue's own view.
        let dump = env.venue_dump();
        assert_eq!(dump["repeated_place_requests"], 0, "round {round}: an order was placed twice");
        assert_eq!(dump["duplicate_placements"], 0);
        let st = env.status();
        assert_eq!(st["core"]["mismatches_unacked"], json!([]), "round {round}");
        let venue_pos = dump["positions"]["X"].as_str().unwrap_or("0").to_string();
        let oms_pos = st["account"]["X"]["pos"].as_str().unwrap_or("0").to_string();
        assert_eq!(oms_pos, venue_pos, "round {round}: position differs");
        let mut admin = env.admin();
        let instance = env.instance_id();
        for vo in dump["orders"].as_array().unwrap() {
            let client = VenueClientId::parse_hex(vo["client"].as_str().unwrap()).unwrap();
            assert_eq!(client.instance_id, instance);
            let o = admin.get_order(client.order_id).unwrap().unwrap_or_else(|| panic!("round {round}: venue order {vo} unknown"));
            assert_eq!(o.cum_qty.to_string(), vo["filled"].as_str().unwrap(), "round {round}: order {}", o.id);
            match vo["status"].as_str().unwrap() {
                "open" => assert_eq!(o.phase, Phase::Working, "order {}", o.id),
                _ => assert_eq!(o.phase, Phase::Closed, "order {}", o.id),
            }
        }
    }
    println!("rounds={rounds} accepted={total_accepted} venue_orders={}", env.venue_dump()["orders"].as_array().unwrap().len());
    assert!(total_accepted > 0);
    assert_eq!(env.stop_node(), 0);
    let (code, out, err) = env.tool(&["verify", "--re-decide"]);
    assert_eq!(code, 0, "{err}");
    println!("verify: {}", out.replace('\n', " "));
}
