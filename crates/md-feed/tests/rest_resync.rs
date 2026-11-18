//! REST snapshot then replay (ticket 06).

mod common;

use std::sync::Arc;
use std::sync::atomic::Ordering;

use common::rest_adapter::{CHANNEL, RestVenue, d, snapshot};
use common::{FakeRest, FakeWs, Step, VecSink, config, eventually, key, wait_event};
use md_core::adapter::{MarketDataAdapter, Resync};
use md_core::{BookSide, Level};
use md_feed::{ConnIds, Feed, FeedEvent, FrameSink, Src};

fn lv(px: i64, qty: i64) -> Level {
    Level { px, qty }
}

fn plans() -> Vec<md_core::adapter::ConnPlan> {
    RestVenue
        .plan(&["BTC".to_string()], &[], 100)
        .expect("plan")
}

fn k() -> md_core::StreamKey {
    key(CHANNEL, "BTC")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn initial_rest_sync_no_gap_event() {
    let fake = FakeWs::start(vec![vec![
        Step::WaitFor("sub".into()),
        // u 5 is already in the snapshot (S = 7): dropped; 6..10 bridges.
        Step::Send(d(1, 5, 0, "[90,9]")),
        Step::Send(d(6, 10, 5, "[100,2]")),
        Step::Send(d(11, 15, 10, "[101,3]")),
    ]])
    .await;
    let rest = FakeRest::start(vec![snapshot(7, "[100,1],[99,1]")]).await;
    let mut cfg = config(&fake.url, Arc::new(ConnIds::new()));
    cfg.rest_base = Some(rest.base.clone());
    let sink = Arc::new(VecSink::default());
    let mut feed = Feed::start(
        Arc::new(RestVenue),
        cfg,
        plans(),
        Some(sink.clone() as Arc<dyn FrameSink>),
    )
    .expect("start");
    let mut ev = feed.take_events().expect("events");
    eventually(|| feed.market_ref(&k()).and_then(|r| r.update_id) == Some(15)).await;
    let bids = feed
        .read_book(&k(), |v| v.book.levels(BookSide::Bid))
        .expect("book");
    assert_eq!(
        bids,
        vec![lv(101, 3), lv(100, 2), lv(99, 1)],
        "snapshot + 6..10 + 11..15; u 5 dropped"
    );
    assert!(!feed.is_stale(&k()));
    while let Ok(e) = ev.try_recv() {
        assert!(
            !matches!(e, FeedEvent::Gap { .. }),
            "initial sync is not a gap: {e:?}"
        );
    }
    assert_eq!(rest.requests.lock().expect("req").len(), 1);
    assert!(rest.requests.lock().expect("req")[0].starts_with("GET /depth?symbol=BTC&limit=100 "));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn gap_rest_resync_book_equals_fake() {
    let fake = FakeWs::start(vec![vec![
        Step::WaitFor("sub".into()),
        Step::Send(d(6, 10, 5, "[100,2]")),
        Step::Pause(std::time::Duration::from_millis(200)),
        Step::Send(d(11, 15, 10, "[101,1]")),
        // pu 22 != 15: a gap.
        Step::Send(d(25, 30, 22, "[102,1]")),
        Step::Pause(std::time::Duration::from_millis(200)),
        Step::Send(d(31, 35, 30, "[103,1]")),
    ]])
    .await;
    let rest = FakeRest::start(vec![snapshot(7, "[100,1]"), snapshot(27, "[150,5]")]).await;
    let mut cfg = config(&fake.url, Arc::new(ConnIds::new()));
    cfg.rest_base = Some(rest.base.clone());
    let sink = Arc::new(VecSink::default());
    let mut feed = Feed::start(
        Arc::new(RestVenue),
        cfg,
        plans(),
        Some(sink.clone() as Arc<dyn FrameSink>),
    )
    .expect("start");
    let mut ev = feed.take_events().expect("events");
    let g = wait_event(&mut ev, |e| matches!(e, FeedEvent::Gap { .. })).await;
    assert!(
        matches!(&g, FeedEvent::Gap { gap, resync: Resync::RestSnapshotThenReplay, .. } if gap.last_good == Some(15) && gap.next == Some(30)),
        "{g:?}"
    );
    eventually(|| feed.market_ref(&k()).and_then(|r| r.update_id) == Some(35)).await;
    let bids = feed
        .read_book(&k(), |v| v.book.levels(BookSide::Bid))
        .expect("book");
    assert_eq!(
        bids,
        vec![lv(150, 5), lv(103, 1), lv(102, 1)],
        "second snapshot + 25..30 + 31..35"
    );
    // Both REST bodies are in the sink, with conn_id 0 and the connection they start.
    let conn = feed.read_book(&k(), |v| v.conn_id).expect("conn");
    let rest_frames: Vec<_> = sink
        .frames()
        .into_iter()
        .filter(|f| f.src == Src::Rest)
        .collect();
    let mut evs = Vec::new();
    while let Ok(e) = ev.try_recv() {
        evs.push(e);
    }
    assert_eq!(
        rest_frames.len(),
        2,
        "{:?} {evs:?}",
        rest_frames
            .iter()
            .map(|f| String::from_utf8_lossy(&f.payload).to_string())
            .collect::<Vec<_>>()
    );
    for (i, f) in rest_frames.iter().enumerate() {
        assert_eq!(
            (f.conn_id, f.frame_no, f.for_conn_id),
            (0, i as u64, Some(conn))
        );
        assert_eq!(f.topic.as_ref(), Some(&k()));
    }
    assert_eq!(fake.accepted.load(Ordering::SeqCst), 1, "no reconnect");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn non_bridging_snapshot_retries_then_reconnects() {
    let fake = FakeWs::start(vec![vec![
        Step::WaitFor("sub".into()),
        Step::Send(d(10, 15, 9, "[100,1]")),
    ]])
    .await;
    // S = 3 is older than U = 10: never bridges.
    let rest = FakeRest::start(vec![snapshot(3, "[100,1]")]).await;
    let mut cfg = config(&fake.url, Arc::new(ConnIds::new()));
    cfg.rest_base = Some(rest.base.clone());
    let mut feed = Feed::start(Arc::new(RestVenue), cfg, plans(), None).expect("start");
    let mut ev = feed.take_events().expect("events");
    let e = wait_event(&mut ev, |e| matches!(e, FeedEvent::Disconnected { .. })).await;
    assert!(
        matches!(&e, FeedEvent::Disconnected { reason, .. } if reason.contains("did not bridge twice")),
        "{e:?}"
    );
    assert_eq!(rest.requests.lock().expect("req").len(), 2, "one retry");
    assert!(feed.is_stale(&k()));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn gap_inside_the_buffer_keeps_the_later_deltas() {
    let fake = FakeWs::start(vec![vec![
        Step::WaitFor("sub".into()),
        Step::Send(d(1, 5, 0, "[90,9]")),
        Step::Send(d(6, 10, 5, "[100,2]")),
        // pu 14 != 10: a gap inside the buffer.
        Step::Send(d(16, 20, 14, "[102,1]")),
        Step::Send(d(21, 25, 20, "[103,1]")),
    ]])
    .await;
    // Slow answers, so that all four deltas are buffered before the snapshot.
    let rest = FakeRest::start_with_delay(
        vec![snapshot(7, "[100,1]"), snapshot(17, "[150,5]")],
        std::time::Duration::from_millis(300),
    )
    .await;
    let mut cfg = config(&fake.url, Arc::new(ConnIds::new()));
    cfg.rest_base = Some(rest.base.clone());
    cfg.resync_timeout = std::time::Duration::from_secs(3);
    let mut feed = Feed::start(Arc::new(RestVenue), cfg, plans(), None).expect("start");
    let mut ev = feed.take_events().expect("events");
    wait_event(&mut ev, |e| matches!(e, FeedEvent::Gap { .. })).await;
    eventually(|| feed.market_ref(&k()).and_then(|r| r.update_id) == Some(25)).await;
    let bids = feed
        .read_book(&k(), |v| v.book.levels(BookSide::Bid))
        .expect("book");
    assert_eq!(bids, vec![lv(150, 5), lv(103, 1), lv(102, 1)]);
    assert_eq!(fake.accepted.load(Ordering::SeqCst), 1, "no reconnect");
    assert_eq!(rest.requests.lock().expect("req").len(), 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn failed_rest_answer_body_is_sunk() {
    // F6: L0 keeps every REST body, also a 5xx one.
    let fake = FakeWs::start(vec![vec![
        Step::WaitFor("sub".into()),
        Step::Send(d(6, 10, 5, "[100,2]")),
        Step::Send(d(11, 15, 10, "[101,3]")),
    ]])
    .await;
    let rest = FakeRest::start_full(
        vec![
            (503, r#"{"err":"busy"}"#.into()),
            (200, snapshot(7, "[100,1]")),
        ],
        std::time::Duration::ZERO,
    )
    .await;
    let mut cfg = config(&fake.url, Arc::new(ConnIds::new()));
    cfg.rest_base = Some(rest.base.clone());
    let sink = Arc::new(VecSink::default());
    let feed = Feed::start(
        Arc::new(RestVenue),
        cfg,
        plans(),
        Some(sink.clone() as Arc<dyn FrameSink>),
    )
    .expect("start");
    eventually(|| feed.market_ref(&k()).and_then(|r| r.update_id) == Some(15)).await;
    let bodies: Vec<String> = sink
        .frames()
        .into_iter()
        .filter(|f| f.src == Src::Rest)
        .map(|f| String::from_utf8_lossy(&f.payload).to_string())
        .collect();
    assert_eq!(bodies.len(), 2, "{bodies:?}");
    assert_eq!(bodies[0], r#"{"err":"busy"}"#);
}
