//! Adversarial review tests for the execution layer (process level).
//!
//! Each test asserts the behaviour the documents promise. A test that FAILS
//! demonstrates a defect; a test that passes records something the reviewer
//! tried to break and could not.

#![allow(clippy::unwrap_used)]

mod common;

use common::*;
use oms_api::*;
use oms_core::*;
use serde_json::json;
use std::os::unix::net::UnixStream;
use std::time::Duration;

// ---- API server -------------------------------------------------------------

/// The trading socket and the admin socket draw from one connection pool.
/// Idle trading connections (no hello, nothing sent) must not be able to keep
/// the operator out: halt and cancel-all exist only on the admin socket.
#[test]
fn idle_trading_connections_do_not_lock_the_operator_out() {
    let mut env = Env::new("review-idle");
    env.extra_api = "max_connections = 4".into();
    env.start_node();
    env.wait_ready();
    std::thread::sleep(Duration::from_millis(200));

    let idle: Vec<UnixStream> = (0..4).map(|_| UnixStream::connect(env.sock()).unwrap()).collect();
    std::thread::sleep(Duration::from_millis(200));
    let reachable = env.wait(Duration::from_secs(2), || env.try_status().is_some());
    drop(idle);
    assert!(reachable, "the admin socket refused the operator while 4 idle trading connections were open");
}

/// A client that subscribed and then disconnected must give its connection
/// slot back. (api.md: "연결이 끊김 → 큐에 들어간 요청은 처리된다. 응답은 버려진다".)
#[test]
fn a_departed_subscriber_gives_its_connection_slot_back() {
    let mut env = Env::new("review-subleak");
    env.extra_api = "max_connections = 4".into();
    env.start_node();
    env.wait_ready();
    std::thread::sleep(Duration::from_millis(200));

    for _ in 0..4 {
        let mut c = env.client("b");
        c.call(Op::Subscribe).unwrap();
        drop(c);
        std::thread::sleep(Duration::from_millis(50));
    }
    // Every client is gone. Nothing happens for strategy b, so no event flows.
    let reachable = env.wait(Duration::from_secs(3), || env.try_status().is_some());
    assert!(
        reachable,
        "all 4 connection slots are still held by subscribers that disconnected; nobody can connect, the operator included"
    );
}

/// overload.md lists a bound for everything a connection can hold. The number
/// of subscriptions one connection can register is not among them.
#[test]
fn one_connection_cannot_register_thousands_of_subscriptions() {
    let env = Env::started("review-subs");
    let mut greedy = env.client("a");
    for _ in 0..3000 {
        greedy.call(Op::Subscribe).unwrap();
    }
    let before = env.status()["pipeline"]["stats"]["events_published"].as_u64().unwrap();
    let mut c = env.client("a");
    accept(&mut c, "k1", Side::Buy, "1", "100");
    assert!(env.wait(Duration::from_secs(5), || env.status()["core"]["working"] == 1));
    std::thread::sleep(Duration::from_millis(300));
    let after = env.status()["pipeline"]["stats"]["events_published"].as_u64().unwrap();
    // One order, a handful of records. One subscription would receive a
    // handful of events.
    assert!(
        after - before < 100,
        "one connection made the commit thread fan out {} event copies for a single order",
        after - before
    );
}

// ---- recovery ---------------------------------------------------------------

/// recovery.md step 2: a snapshot that fails a check (length, checksum, state
/// schema, instance id, state digest) is skipped and reported; with no usable
/// snapshot the journal is replayed from the start. The journal here is
/// complete, so the node must start.
#[test]
fn a_snapshot_whose_digest_does_not_match_is_skipped_not_fatal() {
    let mut env = Env::started("review-snapdigest");
    let mut c = env.client("a");
    accept(&mut c, "k1", Side::Buy, "1", "100");
    assert!(env.wait(Duration::from_secs(5), || env.status()["core"]["working"] == 1));
    drop(c);
    assert_eq!(env.stop_node(), 0);

    // What a build with a changed state layout (and an unchanged
    // STATE_SCHEMA) would find: a well-formed snapshot whose recorded digest
    // differs from the digest it computes.
    let (snap, _) = oms_journal::load_latest_snapshot(&env.data_dir()).unwrap();
    let (mut info, payload) = snap.expect("the clean stop wrote a snapshot");
    info.state_digest ^= 1;
    oms_journal::write_snapshot(&env.data_dir(), info, &payload).unwrap();
    let (code, _, err) = env.tool(&["verify", "--re-decide"]);
    assert_eq!(code, 0, "the journal itself is complete and consistent: {err}");

    env.start_node();
    let exited = env.wait_exit(Duration::from_secs(3));
    assert!(
        exited.is_none(),
        "the node refused to start (exit {exited:?}) instead of skipping the snapshot:\n{}",
        env.log_tail(3)
    );
    env.wait_ready();
    assert_eq!(env.status()["core"]["working"], 1);
}

/// `verify --re-decide` must notice a record whose events are not what the
/// recorded input decides. The record is rewritten with a valid checksum, so
/// only the re-decision can catch it.
#[test]
fn re_decide_detects_a_record_whose_events_differ_from_the_decision() {
    let mut env = Env::started("review-redecide");
    let mut admin = env.admin();
    admin
        .call(Op::Halt {
            strategy: None,
            note: "review-note".into(),
        })
        .unwrap();
    drop(admin);
    assert_eq!(env.stop_node(), 0);

    let src = env.data_dir();
    let dst = env.dir.join("tampered");
    std::fs::create_dir_all(&dst).unwrap();
    std::fs::copy(src.join("INSTANCE"), dst.join("INSTANCE")).unwrap();
    let mut records: Vec<(u64, Vec<u8>)> = Vec::new();
    oms_journal::verify_all(&src, |seq, _, payload| {
        records.push((seq, payload.to_vec()));
        Ok(())
    })
    .unwrap();
    let mut tampered = 0;
    for (_, payload) in records.iter_mut() {
        let mut v: serde_json::Value = serde_json::from_slice(payload).unwrap();
        if let Some(note) = v.pointer_mut("/events/0/operator_halted/note") {
            *note = json!("something else");
            *payload = serde_json::to_vec(&v).unwrap();
            tampered += 1;
        }
    }
    assert_eq!(tampered, 1);
    {
        let opts = oms_node::recovery::journal_options(env.instance_id(), 1 << 30, "review");
        let (mut w, _) = oms_journal::recover(&dst, &opts, 0, |_, _, _| Ok(())).unwrap();
        for (seq, payload) in &records {
            w.append(*seq, payload).unwrap();
        }
        w.sync(1).unwrap();
    }
    // Replay alone accepts it (the events are self-consistent) ...
    assert!(oms_node::recovery::verify(&dst, false).is_ok());
    // ... the re-decision must not.
    let r = oms_node::recovery::verify(&dst, true);
    assert!(r.is_err(), "a divergent record passed --re-decide");
}

/// recovery.md lists process-level recovery over several segments as not
/// checked. Small segments, periodic snapshots, a SIGKILL, a clean stop.
#[test]
fn many_small_segments_survive_a_kill_and_a_clean_restart() {
    let mut env = Env::new("review-segments");
    env.extra_runtime = "segment_max_bytes = 4096\nsnapshot_every = 25".into();
    env.start_node();
    env.wait_ready();
    let mut c = env.client("a");
    for i in 0..60 {
        accept(&mut c, &format!("k{i}"), Side::Buy, "1", "100");
    }
    assert!(env.wait(Duration::from_secs(10), || env.status()["core"]["working"] == 60));
    drop(c);
    env.kill_node();
    assert!(env.journal_segments().len() > 5, "{} segments", env.journal_segments().len());

    env.start_node();
    env.wait_ready();
    let st = env.status();
    assert_eq!(st["core"]["working"], 60);
    assert!(st["node"]["recovery"]["snapshot_seq"].as_u64().unwrap_or(0) > 0, "a snapshot was used");
    assert_eq!(env.stop_node(), 0);
    let (code, _, err) = env.tool(&["verify", "--re-decide"]);
    assert_eq!(code, 0, "{err}");

    env.start_node();
    env.wait_ready();
    assert_eq!(env.status()["core"]["working"], 60);
    let dump = env.venue_dump();
    assert_eq!(dump["place_requests"], 60, "each order reached the venue exactly once");
    assert_eq!(dump["repeated_place_requests"], 0);
    assert_eq!(dump["duplicate_placements"], 0);
}

// ---- Hyperliquid gateway ------------------------------------------------------

/// `sync_fills` (hyperliquid/mod.rs) returns without declaring fills complete
/// when one fill cannot be translated, and the next attempt starts from the
/// same time and meets the same fill. So every fill the account can have must
/// be translatable. Spot fills carry coins such as "@107" and "PURR/USDC".
/// The positions and open-orders paths map such names; the fill path does not.
#[test]
fn hyperliquid_fills_on_spot_coins_are_translatable() {
    use oms_node::venue::hyperliquid::wire::{HlFill, fill_input};
    for coin in ["BTC", "@107", "PURR/USDC"] {
        let f: HlFill = serde_json::from_value(json!({
            "coin": coin, "px": "25.1", "sz": "1.5", "side": "B", "time": 1700000000123u64,
            "oid": 91490942u64, "tid": 118906512037719u64, "fee": "0.01", "feeToken": "USDC"
        }))
        .unwrap();
        assert!(
            fill_input(&f, 7).is_some(),
            "a fill on {coin} cannot be translated: fill synchronisation would never complete again"
        );
    }
}

// ---- shutdown -----------------------------------------------------------------

/// node.rs: "A fatal condition (journal failure, core fault) ... exits with
/// status 2." Here the journal fails while the node is draining after
/// SIGTERM: requests are already queued, the signal arrives, and the next
/// segment rotation cannot create its file (the data directory was made
/// read-only, standing in for any write error). The exit status and the log
/// must not call that a clean stop.
#[test]
fn a_journal_failure_during_the_shutdown_drain_is_not_reported_as_a_clean_stop() {
    use std::os::unix::fs::PermissionsExt;
    let mut env = Env::new("review-drainfail");
    env.extra_runtime = "segment_max_bytes = 4096".into();
    env.start_node();
    env.wait_ready();
    let mut c = env.client("a");
    let data = env.data_dir();
    std::fs::set_permissions(&data, std::fs::Permissions::from_mode(0o555)).unwrap();
    for i in 0..30 {
        c.send(order_op(&format!("k{i}"), Side::Buy, "1", "100", Tif::Gtc, 5_000)).unwrap();
    }
    let code = env.stop_node();
    std::fs::set_permissions(&data, std::fs::Permissions::from_mode(0o755)).unwrap();
    let log = env.log_tail(12);
    assert!(log.contains("\"msg\":\"fatal\""), "inconclusive: the journal did not fail in this run\n{log}");
    assert!(
        code == 2 && !log.contains("stopped cleanly"),
        "exit status {code} after a journal failure during the drain:\n{log}"
    );
}
