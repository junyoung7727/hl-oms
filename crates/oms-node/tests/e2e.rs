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
        market_ref: None,
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

/// S11 at the process level. With a margin policy the node asks the venue
/// for a margin report on a timer, each report reaches the core with a
/// rising number, and `status` shows the account value and its age. Without
/// a margin policy the node never asks.
#[test]
fn node_polls_margin_and_status_shows_value_and_age() {
    let mut env = Env::new("margin-poll");
    env.extra_runtime = "margin_poll_ms = 100".into();
    env.extra_policy = "\n[core.policy.margin]\naccount_modes_allowed = [\"standard\"]\n".into();
    env.ctl(json!({"account_value": {"value": "250.5"}}));
    env.start_node();
    env.wait_ready();

    // The venue's value arrives in the durable state.
    let shown = |env: &Env, value: &str| {
        env.wait(Duration::from_secs(5), || {
            env.try_status().is_some_and(|st| st["core"]["margin"]["account_value"] == value)
        })
    };
    assert!(shown(&env, "250.5"), "status never showed the account value: {}", env.status()["core"]["margin"]);
    let first = env.status()["core"]["margin"].clone();
    assert_eq!(first["fresh"], true);
    let age = first["age_us"].as_u64().expect("age_us is a number");
    assert!(age < 3_000_000, "the report shown is {age} us old with a 100 ms poll");
    assert_eq!(first["session"], env.status()["core"]["link"]["session"]);

    // The poll repeats: the number of the report shown keeps rising and its
    // query time moves forward.
    let seq0 = first["seq"].as_u64().expect("seq is a number");
    assert!(seq0 >= 1);
    assert!(
        env.wait(Duration::from_secs(5), || {
            env.status()["core"]["margin"]["seq"].as_u64().is_some_and(|n| n >= seq0 + 3)
        }),
        "the margin poll did not repeat: {}",
        env.status()["core"]["margin"]
    );
    let later = env.status()["core"]["margin"].clone();
    assert!(later["queried_at"].as_u64().unwrap() > first["queried_at"].as_u64().unwrap());
    // With a 100 ms poll the age shown stays far below the 30 s limit.
    assert!(later["age_us"].as_u64().unwrap() < 3_000_000);

    // A change at the venue is seen by the next poll.
    env.ctl(json!({"account_value": {"value": "249"}}));
    assert!(shown(&env, "249"), "status did not follow the venue: {}", env.status()["core"]["margin"]);

    // The report is what a plain order needs, with a fresh reference price
    // for the leverage gate: it is accepted. The price reaches the core
    // through the venue stream, so the order is tried until it has arrived.
    env.ctl(json!({"ref_price": {"instrument": "X", "px": "100"}}));
    let mut c = env.client("a");
    let accepted = (0..50).any(|n| {
        let reply = c.new_order(order_op(&format!("k{n}"), Side::Buy, "1", "100", Tif::Gtc, 5_000)).unwrap();
        let ok = matches!(reply, NewOrderReply::Accepted { .. });
        if !ok {
            std::thread::sleep(Duration::from_millis(100));
        }
        ok
    });
    assert!(accepted, "a plain order was never accepted: {}", env.status()["core"]["margin"]);
    assert_eq!(env.status()["core"]["margin"]["last_rejection"], serde_json::Value::Null);
    assert!(env.venue_dump()["margin_queries"].as_u64().unwrap() >= 4);
    drop(c);
    drop(env);

    // No margin policy: the node never sends the query, and status says so.
    let mut env = Env::new("margin-poll-off");
    env.extra_runtime = "margin_poll_ms = 100".into();
    env.start_node();
    env.wait_ready();
    std::thread::sleep(Duration::from_millis(600));
    assert_eq!(env.venue_dump()["margin_queries"], 0);
    assert_eq!(env.status()["core"]["margin"], serde_json::Value::Null);
}

/// The operator sees the account exposure the leverage gate judges with:
/// `status` shows it under `core.margin`, it follows an accepted order, and
/// an order the gate refuses leaves it as it was.
#[test]
fn status_shows_account_exposure() {
    let mut env = Env::new("margin-exposure");
    env.extra_runtime = "margin_poll_ms = 100".into();
    env.extra_policy = "\n[core.policy.margin]\naccount_modes_allowed = [\"standard\"]\n".into();
    env.ctl(json!({"account_value": {"value": "100"}}));
    env.start_node();
    env.wait_ready();
    let shown = |env: &Env, value: &str| {
        env.wait(Duration::from_secs(5), || {
            env.try_status().is_some_and(|st| st["core"]["margin"]["account_exposure"] == value)
        })
    };
    // No position and no order: the exposure is zero, not absent.
    assert!(shown(&env, "0"), "status never showed the exposure: {}", env.status()["core"]["margin"]);

    // Account value 100, limit 2.0, reference price 100: a buy of 1.5 is an
    // exposure of 150. The price reaches the core through the venue stream,
    // so the order is tried until it has arrived.
    env.ctl(json!({"ref_price": {"instrument": "X", "px": "100"}}));
    let mut c = env.client("a");
    let accepted = (0..50).any(|n| {
        let reply = c.new_order(order_op(&format!("k{n}"), Side::Buy, "1.5", "100", Tif::Gtc, 5_000)).unwrap();
        let ok = matches!(reply, NewOrderReply::Accepted { .. });
        if !ok {
            std::thread::sleep(Duration::from_millis(100));
        }
        ok
    });
    assert!(accepted, "a plain order was never accepted: {}", env.status()["core"]["margin"]);
    assert!(shown(&env, "150"), "status did not follow the order: {}", env.status()["core"]["margin"]);

    // One more would make 250 against a limit of 200: refused by the gate,
    // and the exposure shown stays 150.
    match c.new_order(order_op("over", Side::Buy, "1", "100", Tif::Gtc, 5_000)).unwrap() {
        NewOrderReply::Rejected { reasons } => assert_eq!(reasons, vec![RejectReason::AccountLeverage]),
        other => panic!("expected rejection, got {other:?}"),
    }
    assert_eq!(env.status()["core"]["margin"]["account_exposure"], "150");
}

/// C-MARGIN: the venue process answers the margin query from its margin
/// model, positions included, and `status` shows them. Hand-worked, venue
/// fee 2 bps, venue setting isolated at leverage 2, maximum leverage 40:
///   account 100, buy 1 at 100 with the mark at 100: fee 0.02 -> 99.98,
///     position value 100, margin 100 / 2 = 50
///   mark 100 -> 101: account 100.98, position value 101, margin 50.5
/// The setting of an instrument with a position reaches the core through
/// the report: after the venue is switched to cross margin at leverage 3
/// the report says so and the instrument is held.
#[test]
fn margin_report_of_the_venue_process_carries_its_positions() {
    let mut env = margin_node("margin-positions", "100");
    let mut c = env.client("a");
    let id = rest(&mut env, &mut c, "p", Side::Buy, "100", false);
    assert!(env.wait(Duration::from_secs(5), || phase_of(&mut c, id) == Phase::Working));
    let cloid = env.cloid(id);
    env.ctl(json!({"fill": {"client": cloid}}));
    let position = |value: &str, margin: &str, isolated: &str, leverage: u32| {
        json!([{
            "instrument": "X",
            "szi": "1",
            "position_value": value,
            "margin_used": margin,
            "leverage_type": isolated,
            "leverage_value": leverage,
            "max_leverage": 40,
        }])
    };
    let first = position("100", "50", "isolated", 2);
    wait_margin(&env, "the position", |m| m["positions"] == first && m["account_value"] == "99.98");

    env.ctl(json!({"ref_price": {"instrument": "X", "px": "101"}}));
    let moved = position("101", "50.5", "isolated", 2);
    wait_margin(&env, "the price move", |m| m["positions"] == moved && m["account_value"] == "100.98");
    assert_eq!(env.status()["core"]["holds"], json!([]));

    env.ctl(json!({"venue_setting": {"instrument": "X", "isolated": false, "leverage": 3}}));
    let cross = position("101", "33.66666666", "cross", 3);
    wait_margin(&env, "the changed setting", |m| {
        m["positions"] == cross && m["venue_settings"] == json!({"X": {"isolated": false, "leverage": 3, "as_configured": false}})
    });
    assert_eq!(env.status()["core"]["holds"], json!([{"venue_setting": "X"}]));
}

/// C-MARGIN: the venue process has a control for the margin it states for
/// a position, so the MM-buffer rule can be reached through the node
/// process. Hand-worked, position value 100, maximum leverage 40, buffer
/// 25 bps: the line is 100 x (1/80 + 0.0025) = 1.5. A stated margin of 1.5
/// is on the line and does not trip; 1.49 enters REDUCING by `mm_buffer`.
#[test]
fn venue_process_states_the_margin_a_control_sets_and_the_mm_rule_latches_on_it() {
    let mut env = margin_node("margin-used-knob", "100");
    let mut c = env.client("a");
    let id = rest(&mut env, &mut c, "p", Side::Buy, "100", false);
    assert!(env.wait(Duration::from_secs(5), || phase_of(&mut c, id) == Phase::Working));
    let cloid = env.cloid(id);
    env.ctl(json!({"fill": {"client": cloid}}));
    // The model's own figure: value 100 at leverage 2.
    wait_margin(&env, "the position", |m| m["positions"][0]["margin_used"] == "50");

    env.ctl(json!({"margin_used": {"instrument": "X", "value": "1.5"}}));
    wait_margin(&env, "the stated margin", |m| m["positions"][0]["margin_used"] == "1.5");
    assert_eq!(env.status()["core"]["margin"]["reducing"], serde_json::Value::Null);

    env.ctl(json!({"margin_used": {"instrument": "X", "value": "1.49"}}));
    wait_margin(&env, "REDUCING by the MM buffer", |m| {
        m["positions"][0]["margin_used"] == "1.49" && m["reducing"]["rule"] == "mm_buffer"
    });
    assert_eq!(env.status()["core"]["holds"], json!(["reducing"]));
}

/// C-MARGIN: the operator sees the daily-loss rule in `status`. The first
/// report the node polls becomes the day base. When the venue's account
/// value falls more than 5 % below it, status shows REDUCING with the rule,
/// the hold, and the unchanged day base, and a plain order is refused.
#[test]
fn status_shows_reducing_its_rule_and_the_day_base() {
    let mut env = Env::new("margin-reducing-status");
    env.extra_runtime = "margin_poll_ms = 100".into();
    env.extra_policy = "\n[core.policy.margin]\naccount_modes_allowed = [\"standard\"]\n".into();
    env.ctl(json!({"account_value": {"value": "100"}}));
    env.start_node();
    env.wait_ready();
    assert!(
        env.wait(Duration::from_secs(5), || {
            env.try_status().is_some_and(|st| st["core"]["margin"]["account_value"] == "100")
        }),
        "status never showed the account value: {}",
        env.status()["core"]["margin"]
    );
    let before = env.status()["core"]["margin"].clone();
    assert_eq!(before["day_base"]["value"], "100", "no day base in status: {before}");
    assert_eq!(before["day_base"]["set_by"], "report");
    // The base's day is the UTC day of the first report's query time: today,
    // or yesterday if this test runs across midnight.
    let today = oms_api::client::now_us() / 86_400_000_000;
    let day = before["day_base"]["day"].as_u64().expect("day is a number");
    assert!(day == today || day + 1 == today, "day base of day {day}, today is {today}");
    assert_eq!(before["reducing"], serde_json::Value::Null);

    // The leverage gate needs a fresh reference price for X, or a plain
    // order is refused with `MarginUnavailable` as well. The price reaches
    // the core through the venue stream, so a small plain order is tried
    // until one is accepted: from then on the price is there.
    env.ctl(json!({"ref_price": {"instrument": "X", "px": "100"}}));
    let mut c = env.client("a");
    let priced = (0..50).any(|n| {
        let reply = c.new_order(order_op(&format!("p{n}"), Side::Buy, "0.1", "100", Tif::Gtc, 5_000)).unwrap();
        let ok = matches!(reply, NewOrderReply::Accepted { .. });
        if !ok {
            std::thread::sleep(Duration::from_millis(100));
        }
        ok
    });
    assert!(priced, "a plain order was never accepted: {}", env.status()["core"]["margin"]);

    // The account loses 5.01 % of the base.
    env.ctl(json!({"account_value": {"value": "94.99"}}));
    assert!(
        env.wait(Duration::from_secs(5), || env.status()["core"]["margin"]["reducing"]["rule"] == "daily_loss"),
        "status never showed REDUCING: {}",
        env.status()["core"]["margin"]
    );
    let st = env.status();
    let after = &st["core"]["margin"];
    assert_eq!(after["reducing"]["detail"], "account value 94.99 is more than 500 bps below the day base 100");
    assert!(after["reducing"]["since"].as_u64().is_some_and(|t| t > 0));
    assert_eq!(after["day_base"], before["day_base"], "entering REDUCING changed the day base");
    assert_eq!(st["core"]["holds"], json!(["reducing"]));

    // Exposure 10 + 100 is far below 2 x 94.99: only the hold answers.
    let r = c.new_order(order_op("k1", Side::Buy, "1", "100", Tif::Gtc, 5_000)).unwrap();
    assert_eq!(r, NewOrderReply::Rejected { reasons: vec![RejectReason::Hold(HoldReason::Reducing)] });
}

/// S17, S18 at the process level. With a margin policy the node asks the
/// venue for the setting of every instrument and for the account mode when
/// the link comes up, asks again for the settings on a timer, and `status`
/// shows what the venue told and whether it is what the policy asks for. A
/// setting changed at the venue holds the instrument after the next poll.
/// The account mode is read once per link session. The node only reads:
/// the gateway has no request that changes either.
#[test]
fn node_reads_venue_setting_and_account_mode_and_status_shows_them() {
    let mut env = Env::new("venue-setting");
    env.extra_runtime = "margin_poll_ms = 100\nvenue_setting_poll_ms = 200".into();
    env.extra_policy = "\n[core.policy.margin]\naccount_modes_allowed = [\"standard\"]\n".into();
    env.start_node();
    env.wait_ready();

    let margin = |env: &Env| env.status()["core"]["margin"].clone();
    let shown = |env: &Env, key: &str, want: serde_json::Value| {
        env.wait(Duration::from_secs(5), || {
            env.try_status().is_some_and(|st| st["core"]["margin"][key] == want)
        })
    };
    // What the simulated venue starts with: isolated at leverage 2, mode
    // "standard". Both are what the policy asks for.
    let as_configured = json!({"X": {"isolated": true, "leverage": 2, "as_configured": true}});
    assert_eq!(margin(&env)["venue_settings"], as_configured);
    assert_eq!(margin(&env)["account_mode"], json!({"mode": "standard", "allowed": true}));

    // The setting poll repeats while the instrument has no position.
    let asked = env.venue_dump()["venue_setting_queries"].as_u64().expect("venue_setting_queries");
    assert!(asked >= 1);
    let repeated = (0..50).any(|_| {
        std::thread::sleep(Duration::from_millis(100));
        env.venue_dump()["venue_setting_queries"].as_u64().unwrap() >= asked + 3
    });
    assert!(repeated, "the venue-setting poll did not repeat");

    // The operator changes the instrument to cross margin at leverage 3 at
    // the venue. The next poll shows it and the instrument is held.
    env.ctl(json!({"venue_setting": {"instrument": "X", "isolated": false, "leverage": 3}}));
    let differs = json!({"X": {"isolated": false, "leverage": 3, "as_configured": false}});
    assert!(shown(&env, "venue_settings", differs), "status did not follow the venue: {}", margin(&env));
    assert_eq!(env.status()["core"]["holds"], json!([{"venue_setting": "X"}]));
    env.ctl(json!({"ref_price": {"instrument": "X", "px": "100"}}));
    let mut c = env.client("a");
    let reply = c.new_order(order_op("k1", Side::Buy, "1", "100", Tif::Gtc, 5_000)).unwrap();
    match reply {
        NewOrderReply::Rejected { reasons } => {
            assert!(reasons.contains(&RejectReason::Hold(HoldReason::VenueSetting(InstrumentId::new("X").unwrap()))), "{reasons:?}")
        }
        other => panic!("a plain order passed a differing venue setting: {other:?}"),
    }

    // Changed back at the venue: the hold goes with the next poll.
    env.ctl(json!({"venue_setting": {"instrument": "X", "isolated": true, "leverage": 2}}));
    assert!(shown(&env, "venue_settings", as_configured.clone()), "status did not follow the venue: {}", margin(&env));
    assert_eq!(env.status()["core"]["holds"], json!([]));

    // The account mode is read when the link comes up, not on a timer: one
    // query so far, and a change at the venue is seen in the next session.
    assert_eq!(env.venue_dump()["account_mode_queries"], 1);
    env.ctl(json!({"account_mode": {"mode": "unifiedAccount"}}));
    std::thread::sleep(Duration::from_millis(600));
    assert_eq!(margin(&env)["account_mode"], json!({"mode": "standard", "allowed": true}));
    let session = env.status()["core"]["link"]["session"].clone();
    env.ctl(json!("kick_streams"));
    let unified = json!({"mode": "unifiedAccount", "allowed": false});
    assert!(shown(&env, "account_mode", unified), "status did not show the new mode: {}", margin(&env));
    assert_ne!(env.status()["core"]["link"]["session"], session);
    // The settings were asked again in the new session, and the account is
    // held by its mode alone.
    assert!(shown(&env, "venue_settings", as_configured), "{}", margin(&env));
    assert!(
        env.wait(Duration::from_secs(10), || env.status()["core"]["holds"] == json!(["account_mode"])),
        "holds: {}",
        env.status()["core"]["holds"]
    );
    assert_eq!(env.venue_dump()["account_mode_queries"], 2);

    // Back to the listed mode, next session: ready to trade again.
    env.ctl(json!({"account_mode": {"mode": "standard"}}));
    env.ctl(json!("kick_streams"));
    assert!(shown(&env, "account_mode", json!({"mode": "standard", "allowed": true})), "{}", margin(&env));
    env.wait_ready();
    drop(c);
    drop(env);

    // No margin policy: the node asks for neither.
    let mut env = Env::new("venue-setting-off");
    env.extra_runtime = "venue_setting_poll_ms = 100".into();
    env.start_node();
    env.wait_ready();
    std::thread::sleep(Duration::from_millis(600));
    assert_eq!(env.venue_dump()["venue_setting_queries"], 0);
    assert_eq!(env.venue_dump()["account_mode_queries"], 0);
}

// ---- C-MARGIN: operator commands, restart (ticket 05) ---------------------------

/// S14. `clear-reduce` and `rebase-day` exist only on the admin socket. From
/// a strategy connection both are refused with `forbidden`, nothing reaches
/// the core, and the latch and the day base stay as they were: a strategy
/// cannot release itself.
#[test]
fn strategy_socket_cannot_clear_reducing_or_rebase() {
    let mut env = margin_node("margin-strategy-socket", "100");
    lose_to(&mut env, "94");
    let before = env.status()["core"]["margin"].clone();
    assert_eq!(before["day_base"]["value"], "100");
    let records = env.status()["core"]["stats"].clone();

    let mut c = env.client("a");
    assert_eq!(api_error(c.call(Op::ClearReduce { note: "let me trade".into() })), ErrorCode::Forbidden);
    assert_eq!(api_error(c.call(Op::RebaseDay { note: "let me trade".into() })), ErrorCode::Forbidden);

    let st = env.status();
    assert_eq!(st["core"]["margin"]["reducing"], before["reducing"]);
    assert_eq!(st["core"]["margin"]["day_base"], before["day_base"]);
    assert_eq!(st["core"]["holds"], json!(["reducing"]));
    assert_eq!(st["core"]["stats"], records);
}

/// Runs the operator tool's own code (`omsctl`) with these arguments on a
/// socket of the node.
fn omsctl(socket: &std::path::Path, args: &[&str]) -> Result<String, String> {
    let mut all = vec!["--socket".to_string(), socket.display().to_string()];
    all.extend(args.iter().map(|a| a.to_string()));
    oms_api::ctl::run(&all, &mut std::io::sink())
}

/// The `core.margin` part of what `omsctl status` prints.
fn omsctl_margin(env: &Env) -> serde_json::Value {
    let text = omsctl(&env.admin_sock(), &["status"]).expect("omsctl status");
    let st: serde_json::Value = serde_json::from_str(&text).expect("omsctl status prints JSON");
    json!({"margin": st["core"]["margin"], "holds": st["core"]["holds"]})
}

/// S14, S15. The operator tool end to end: `omsctl clear-reduce --note` and
/// `omsctl rebase-day --note` on the admin socket reach the core of a
/// running node, the answers come back, and `omsctl status` shows the state
/// (NORMAL or REDUCING), the rule and the day base before and after.
#[test]
fn omsctl_clear_reduce_and_rebase_day_work_end_to_end() {
    let mut env = margin_node("margin-omsctl", "100");
    let admin = env.admin_sock();
    let shown = omsctl_margin(&env);
    assert_eq!(shown["margin"]["reducing"], serde_json::Value::Null);
    assert_eq!(shown["margin"]["day_base"]["value"], "100");
    assert_eq!(shown["margin"]["day_base"]["set_by"], "report");

    // NORMAL: there is nothing to clear, and the tool says why.
    let err = omsctl(&admin, &["clear-reduce", "--note", "nothing to clear"]).unwrap_err();
    assert_eq!(err, "BadRequest: not_reducing: the account is not REDUCING");

    lose_to(&mut env, "94");
    let shown = omsctl_margin(&env);
    assert_eq!(shown["margin"]["reducing"]["rule"], "daily_loss");
    assert_eq!(shown["margin"]["day_base"]["value"], "100");
    assert_eq!(shown["holds"], json!(["reducing"]));

    // The note is part of the command: without it the tool sends nothing,
    // and a blank one is refused by the node.
    assert_eq!(omsctl(&admin, &["clear-reduce"]).unwrap_err(), "missing --note");
    let err = omsctl(&admin, &["clear-reduce", "--note", " "]).unwrap_err();
    assert_eq!(err, "BadRequest: note_required: the command needs a note");
    assert_eq!(omsctl_margin(&env)["holds"], json!(["reducing"]));

    // The release. The account value 94 is the new day base.
    assert_eq!(omsctl(&admin, &["clear-reduce", "--note", "loss understood"]).unwrap(), "\"cleared\"");
    let shown = omsctl_margin(&env);
    assert_eq!(shown["margin"]["reducing"], serde_json::Value::Null);
    assert_eq!(shown["margin"]["day_base"]["value"], "94");
    assert_eq!(shown["margin"]["day_base"]["set_by"], "operator");
    assert_eq!(shown["holds"], json!([]));
    // The next reports at 94 do not latch again.
    let seq = shown["margin"]["seq"].as_u64().expect("seq");
    wait_margin(&env, "three more reports", |m| m["seq"].as_u64().is_some_and(|n| n >= seq + 3));
    assert_eq!(omsctl_margin(&env)["margin"]["reducing"], serde_json::Value::Null);

    // A deposit of 26, then the rebase: the base follows, the state does not
    // change.
    env.ctl(json!({"account_value": {"value": "120"}}));
    wait_margin(&env, "the deposit", |m| m["account_value"] == "120");
    assert_eq!(omsctl(&admin, &["rebase-day"]).unwrap_err(), "missing --note");
    assert_eq!(omsctl(&admin, &["rebase-day", "--note", "deposit of 26"]).unwrap(), "\"rebased\"");
    let shown = omsctl_margin(&env);
    assert_eq!(shown["margin"]["day_base"]["value"], "120");
    assert_eq!(shown["margin"]["day_base"]["set_by"], "operator");
    assert_eq!(shown["margin"]["reducing"], serde_json::Value::Null);

    // The same tool on the trading socket is refused.
    let err = omsctl(&env.sock(), &["--strategy", "a", "clear-reduce", "--note", "x"]).unwrap_err();
    assert_eq!(err, "Forbidden: operator operations need the admin socket");
}

/// S13. REDUCING and the day base are journal state. The node process is
/// killed (SIGKILL) while REDUCING and started again: `status` shows the
/// same latch (rule, detail, time of entry) and the same day base, although
/// the venue's account value is what a fresh start would take as a new
/// base. Plain orders stay refused. A clear survives a kill in the same
/// way: after the next restart the account is NORMAL with the operator's
/// base.
#[test]
fn reducing_and_day_base_survive_a_node_kill() {
    let mut env = margin_node("margin-kill", "100");
    lose_to(&mut env, "94");
    let before = env.status()["core"]["margin"].clone();
    assert_eq!(before["reducing"]["rule"], "daily_loss");
    assert_eq!(before["day_base"]["value"], "100");

    env.kill_node();
    env.start_node();
    // The restarted node is in session again and has taken new reports.
    let seq_seen = |m: &serde_json::Value| m["seq"].as_u64().is_some_and(|n| n >= 3);
    assert!(
        env.wait(Duration::from_secs(20), || {
            env.try_status().is_some_and(|st| {
                st["core"]["boot"] == 2 && st["core"]["holds"] == json!(["reducing"]) && seq_seen(&st["core"]["margin"])
            })
        }),
        "the restarted node did not come back REDUCING: {:?}\n{}",
        env.try_status().map(|st| st["core"].to_string()),
        env.log_tail(20)
    );
    let after = env.status()["core"]["margin"].clone();
    assert_eq!(after["reducing"], before["reducing"], "the latch changed across the kill");
    assert_eq!(after["day_base"], before["day_base"], "the day base changed across the kill");
    assert_eq!(after["account_value"], "94");
    let mut c = env.client("a");
    match c.new_order(order_op("k1", Side::Buy, "0.1", "100", Tif::Gtc, 5_000)).unwrap() {
        NewOrderReply::Rejected { reasons } => {
            assert!(reasons.contains(&RejectReason::Hold(HoldReason::Reducing)), "{reasons:?}")
        }
        other => panic!("a plain order was not refused after the restart: {other:?}"),
    }
    drop(c);

    // The operator clears, and the node is killed again.
    assert_eq!(env.admin().call(Op::ClearReduce { note: "after the restart".into() }).unwrap(), json!("cleared"));
    let cleared = env.status()["core"]["margin"].clone();
    assert_eq!(cleared["day_base"]["value"], "94");
    env.kill_node();
    env.start_node();
    env.wait_ready();
    wait_margin(&env, "reports after the second restart", seq_seen);
    let st = env.status();
    assert_eq!(st["core"]["boot"], 3);
    assert_eq!(st["core"]["margin"]["reducing"], serde_json::Value::Null);
    assert_eq!(st["core"]["margin"]["day_base"], cleared["day_base"]);
    assert_eq!(st["core"]["margin"]["day_base"]["set_by"], "operator");
}

/// S8, G7. Entry into REDUCING through a real node with many orders resting
/// at the simulated venue: 300 plain orders of two strategies and 3
/// reduce-only orders. After the loss report every plain order is cancelled
/// at the venue and the three reduce-only orders are still open there. The
/// burst of 300 cancels leaves the node working: margin reports keep
/// arriving.
#[test]
fn reducing_cancels_every_resting_plain_order_through_the_node() {
    let mut env = margin_node("margin-burst", "100000");
    env.ctl(json!({"ref_price": {"instrument": "X", "px": "100"}}));
    let mut a = env.client("a");
    let mut b = env.client("b");
    // Exposure: 150 buys at max(90, 100) = 15 000 against 150 sells at 110
    // plus 3 at 120 = 16 860. Far below 2 x 100 000.
    let mut plain = std::collections::BTreeSet::new();
    for n in 0..150 {
        let id = rest(&mut env, &mut a, &format!("bid{n}"), Side::Buy, "90", false);
        plain.insert(env.cloid(id));
        let id = rest(&mut env, &mut b, &format!("ask{n}"), Side::Sell, "110", false);
        plain.insert(env.cloid(id));
    }
    let mut flagged = std::collections::BTreeSet::new();
    for n in 0..3 {
        let id = rest(&mut env, &mut a, &format!("flagged{n}"), Side::Sell, "120", true);
        flagged.insert(env.cloid(id));
    }
    assert_eq!((plain.len(), flagged.len()), (300, 3));
    assert!(
        env.wait(Duration::from_secs(10), || env.status()["core"]["working"] == 303),
        "the orders never rested: {}",
        env.status()["core"]
    );
    let all: std::collections::BTreeSet<String> = plain.union(&flagged).cloned().collect();
    assert_eq!(open_at_venue(&mut env), all);

    // The account loses 5.001 % of the base.
    lose_to(&mut env, "94999");
    let mut left = open_at_venue(&mut env);
    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    while left != flagged && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
        left = open_at_venue(&mut env);
    }
    let plain_left: Vec<&String> = left.intersection(&plain).collect();
    assert!(plain_left.is_empty(), "{} plain orders are still open at the venue after entry", plain_left.len());
    assert_eq!(left, flagged, "a reduce-only order was cancelled");
    // The node agrees with the venue once the cancels are confirmed.
    assert!(
        env.wait(Duration::from_secs(10), || env.status()["core"]["open_orders"] == 3),
        "the node still counts {} open orders",
        env.status()["core"]["open_orders"]
    );
    let st = env.status();
    assert_eq!(st["core"]["holds"], json!(["reducing"]));
    assert_eq!(st["core"]["mismatches_unacked"], json!([]));
    // After the burst the margin poll still runs.
    let seq = st["core"]["margin"]["seq"].as_u64().expect("seq");
    wait_margin(&env, "reports after the cancel burst", |m| m["seq"].as_u64().is_some_and(|n| n >= seq + 3));
}
