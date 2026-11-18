//! Bybit order book frames: what the strategy trades on and what the archiver
//! writes as `book_msg` rows.

mod common;

use common::{book, linear};
use md_core::adapter::{MarketDataAdapter, Resync};
use md_core::conformance::{Fixture, run};
use md_core::{
    BookSide, BookUpdate, GapDetector, Level, Normalized, OrderBook, SeqRule, StreamKey,
};

fn key() -> StreamKey {
    StreamKey::new("orderbook.50", "BTCUSDT")
}

fn lv(px: i64, qty: i64) -> Level {
    Level { px, qty }
}

fn one(frame: &[u8]) -> (bool, BookUpdate) {
    match linear().parse(&key(), frame, 7).expect("parse").as_slice() {
        [Normalized::BookSnapshot(u)] => (true, u.clone()),
        [Normalized::BookDelta(u)] => (false, u.clone()),
        other => panic!("not one book message: {other:?}"),
    }
}

#[test]
fn bybit_adapter_passes_the_conformance_harness() {
    let fixture = Fixture {
        key: key(),
        snapshot: book(
            "snapshot",
            100,
            1_000,
            r#"["42000.0","1.000"],["41999.9","2.000"]"#,
            r#"["42000.1","1.500"]"#,
        ),
        deltas: vec![
            book("delta", 101, 1_020, r#"["41999.9","0"]"#, ""),
            book("delta", 102, 1_040, "", r#"["42000.2","3.000"]"#),
        ],
        expected_bids: vec![lv(420000, 1000)],
        expected_asks: vec![lv(420001, 1500), lv(420002, 3000)],
        deleted_px: 419999,
        expected_rule: SeqRule::PlusOne,
        gap_delta: Some(book("delta", 104, 1_060, "", "")),
        expected_resync: Resync::Resubscribe,
        recv_ns: 7,
        expected_venue_ts_ns: 1_000_000_000,
        expected_delta_venue_ts_ns: vec![1_020_000_000, 1_040_000_000],
        expected_px_exp: -1,
        expected_qty_exp: -3,
        garbage: vec![
            b"".to_vec(),
            br#"{"op":"pong"}"#.to_vec(),
            br#"{"success":true,"op":"subscribe","req_id":"c0-0"}"#.to_vec(),
        ],
    };
    assert_eq!(run(&linear(), &fixture), Vec::<String>::new());
}

#[test]
fn snapshot_fields_are_copied_exactly() {
    let (is_snapshot, u) = one(&book(
        "snapshot",
        100,
        1_767_312_002_997,
        r#"["41999.9","2.000"],["41999.8","4.000"]"#,
        r#"["42000.2","3.000"]"#,
    ));
    assert!(is_snapshot);
    assert_eq!(u.key, key());
    assert_eq!((u.venue_ts_ns, u.recv_ns), (1_767_312_002_997_000_000, 7));
    assert_eq!(
        (
            u.update_id,
            u.venue_seq,
            u.prev_update_id,
            u.first_update_id
        ),
        (Some(100), Some(9100), None, None)
    );
    assert_eq!(u.bids, vec![lv(419999, 2000), lv(419998, 4000)]);
    assert_eq!(u.asks, vec![lv(420002, 3000)]);
    assert_eq!((u.px_exp, u.qty_exp), (-1, -3));
}

#[test]
fn missing_u_gives_one_gap_and_resubscribe() {
    let a = linear();
    let mut gaps = GapDetector::new(a.seq_rule(&key()));
    let (_, s) = one(&book("snapshot", 100, 1, r#"["1.0","1"]"#, ""));
    gaps.snapshot(s.update_id, s.recv_ns);
    let mut events = Vec::new();
    for u in [101, 103, 104] {
        let (_, d) = one(&book("delta", u, 1, "", ""));
        events.extend(gaps.delta(d.update_id, d.prev_update_id, d.recv_ns));
    }
    assert_eq!(events.len(), 1);
    assert_eq!(
        (events[0].last_good, events[0].next),
        (Some(101), Some(103))
    );
    assert_eq!(a.on_gap(&key(), &events[0]), Resync::Resubscribe);
    assert_eq!(
        a.seq_rule(&StreamKey::new("publicTrade", "BTCUSDT")),
        SeqRule::None
    );
}

#[test]
fn every_snapshot_starts_a_new_run() {
    // u = 1 after a Bybit service restart; u below the last delta after a
    // resubscribe on the same connection (measured 4 of 4).
    for restart_u in [1, 480] {
        let mut gaps = GapDetector::new(SeqRule::PlusOne);
        let mut ob = OrderBook::new(None);
        let (_, s) = one(&book("snapshot", 400, 1, r#"["1.0","1.000"]"#, ""));
        ob.apply_snapshot(&s);
        gaps.snapshot(s.update_id, s.recv_ns);
        for u in 401..=500 {
            let (_, d) = one(&book("delta", u, 1, r#"["0.9","1.000"]"#, ""));
            assert_eq!(gaps.delta(d.update_id, d.prev_update_id, d.recv_ns), None);
            ob.apply_delta(&d);
        }
        let (is_snapshot, s) = one(&book("snapshot", restart_u, 1, r#"["2.0","5.000"]"#, ""));
        assert!(is_snapshot);
        ob.apply_snapshot(&s);
        gaps.snapshot(s.update_id, s.recv_ns);
        let (_, d) = one(&book("delta", restart_u + 1, 1, r#"["1.9","1.000"]"#, ""));
        assert_eq!(
            gaps.delta(d.update_id, d.prev_update_id, d.recv_ns),
            None,
            "u {restart_u}"
        );
        ob.apply_delta(&d);
        assert_eq!(ob.levels(BookSide::Bid), vec![lv(20, 5000), lv(19, 1000)]);
    }
}

#[test]
fn inexact_or_mismatched_book_frames_are_refused() {
    let a = linear();
    let bad: Vec<Vec<u8>> = vec![
        book("delta", 1, 1, r#"["42000.15","1.000"]"#, ""),
        book("delta", 1, 1, r#"["42000.1","1.0001"]"#, ""),
        book("delta", 1, 1, r#"["42000.1","99999999999999999.000"]"#, ""),
        book("delta", 1, 1, r#"["42000.1"]"#, ""),
        book("partial", 1, 1, "", ""),
        br#"{"topic":"orderbook.50.BTCUSDT","type":"delta","ts":1,"data":{"s":"ETHUSDT","b":[],"a":[],"u":1,"seq":1}}"#.to_vec(),
        br#"{"topic":"orderbook.200.BTCUSDT","type":"delta","ts":1,"data":{"s":"BTCUSDT","b":[],"a":[],"u":1,"seq":1}}"#.to_vec(),
        br#"{"topic":"orderbook.50.BTCUSDT","type":"delta","ts":1,"data":{"s":"BTCUSDT","b":[],"a":[],"seq":1}}"#.to_vec(),
        br#"{"topic":"orderbook.50.BTCUSDT","type":"delta","data":{"s":"BTCUSDT","b":[],"a":[],"u":1,"seq":1}}"#.to_vec(),
        b"not json".to_vec(),
    ];
    for frame in &bad {
        assert!(
            a.parse(&key(), frame, 7).is_err(),
            "{}",
            String::from_utf8_lossy(frame)
        );
    }
    let unknown = StreamKey::new("orderbook.50", "ETHUSDT");
    let eth = br#"{"topic":"orderbook.50.ETHUSDT","type":"delta","ts":1,"data":{"s":"ETHUSDT","b":[],"a":[],"u":1,"seq":1}}"#;
    assert!(a.parse(&unknown, eth, 7).is_err(), "no instrument row");
}
