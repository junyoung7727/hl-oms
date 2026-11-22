//! Replay speed of the watcher (C-WATCH part 2): the watcher keeps no state
//! and reads the journal from record 1 at every start, and no heartbeat is
//! sent while it does (owner question 9). A measurement, reported and not
//! asserted:
//!
//! ```text
//! cargo test --release -p watcher --test replay -- --ignored --nocapture
//! ```

#![allow(clippy::unwrap_used)]

mod common;

use common::*;
use serde_json::{Value, json};
use std::time::Instant;
use watcher::journal::{MAX_RECORDS, Tail};
use watcher::state::JournalState;

const RECORDS: u64 = 100_000;

/// Record `seq` of a journal shaped like a quiet trading day: in every ten
/// records one order's life (accepted, acked, filled and closed), a
/// reference price, a margin report and five heartbeats (the heartbeat every
/// 5 s is the most frequent record of a running OMS; the mix itself is
/// 판단(모델 아님)). Every 10,000 records one heartbeat is a purge of the
/// orders closed since the last purge, as the OMS's own purge keeps its
/// memory bounded. `unpurged` is the first order not purged yet.
fn events(seq: u64, at: u64, unpurged: &mut u64) -> Vec<Value> {
    let id = (seq - 1) / 10 + 1;
    let side = if id % 2 == 0 { "buy" } else { "sell" };
    if seq % 10_000 == 6 && seq > 10 {
        let ids: Vec<u64> = (*unpurged..=id).collect();
        *unpurged = id + 1;
        return vec![ev(
            "purged",
            json!({"cutoff": 0, "fill_floor": at - 3_600_000_000, "order_ids": ids}),
        )];
    }
    match (seq - 1) % 10 {
        0 => vec![ev(
            "order_accepted",
            json!({"config_version": 1, "order_id": id, "ref_px": "70000", "request": {
                "client_order_id": format!("c{id}"), "expires_at": at + 5_000_000,
                "instrument": "BTCUSDT", "px": "70000.5", "qty": "0.001",
                "reduce_only": false, "side": side, "strategy": "mm", "tif": "gtc"}}),
        )],
        1 => vec![ev(
            "order_acked",
            json!({"order_id": id, "session": 1, "venue_oid": format!("v{id}"), "via": "send_result"}),
        )],
        2 => vec![
            ev(
                "fill_booked",
                json!({"fee": "0.0007", "key": format!("BTCUSDT:e{id}"), "order_id": id,
                    "px": "70000.5", "qty": "0.001", "venue_ts": at}),
            ),
            ev("order_closed", json!({"order_id": id, "reason": "filled"})),
        ],
        3 => vec![ev(
            "ref_price_updated",
            json!({"instrument": "BTCUSDT", "px": "70000.1", "venue_ts": at}),
        )],
        4 => vec![ev(
            "margin_reported",
            json!({"report": {"account_value": "12600.12", "positions": [{
                "instrument": "BTCUSDT", "leverage_type": "cross", "leverage_value": 2,
                "margin_used": "35", "max_leverage": 100, "position_value": "70",
                "szi": "0.001"}], "queried_at": at, "seq": seq, "session": 1}}),
        )],
        _ => vec![ev(
            "watcher_heartbeat",
            json!({"watcher": "w", "checks": {"W1": at / 1000, "W2": at / 1000}}),
        )],
    }
}

#[test]
#[ignore = "measurement: cargo test --release -p watcher --test replay -- --ignored --nocapture"]
fn replay_speed_of_100000_records() {
    let dir = test_dir("replay");
    // Written in memory and stored once (the test writer's append opens the
    // file per record).
    let mut bytes = header_bytes(&header_json(1, 2, 7));
    let at0 = 1_800_000_000_000_000u64;
    let mut unpurged = 1;
    for seq in 1..=RECORDS {
        let at = at0 + seq * 1_000_000;
        let evs = events(seq, at, &mut unpurged);
        bytes.extend_from_slice(&frame_bytes(seq, &payload(seq, at, &evs)));
    }
    std::fs::write(segment_path(&dir, 1), &bytes).unwrap();

    let started = Instant::now();
    let mut tail = Tail::open(&dir);
    let mut st = JournalState::default();
    let mut calls = 0;
    while st.last_seq < RECORDS {
        st.read_from(&mut tail, RECORDS, MAX_RECORDS).unwrap();
        calls += 1;
        assert!(calls <= RECORDS as usize / MAX_RECORDS + 1);
    }
    let elapsed = started.elapsed();

    // What the replay built (so that the measurement measured the work).
    assert_eq!(st.last_seq, RECORDS);
    assert!(st.open.is_empty(), "{:?}", st.open);
    assert!(st.orders.len() < 1_100, "{}", st.orders.len());
    let per_s = RECORDS as f64 / elapsed.as_secs_f64();
    println!(
        "{}",
        json!({
            "label": "watcher-replay",
            "release_build": !cfg!(debug_assertions),
            "records": RECORDS,
            "journal_bytes": bytes.len(),
            "read_calls": calls,
            "elapsed_ms": elapsed.as_millis() as u64,
            "records_per_s": per_s.round() as u64,
            "s_per_million_records": (1_000_000.0 / per_s * 10.0).round() / 10.0,
            "orders_in_memory_after": st.orders.len(),
            "position_btcusdt": st.position("BTCUSDT").to_string(),
        })
    );
    let _ = std::fs::remove_dir_all(&dir);
}
