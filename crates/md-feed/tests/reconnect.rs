//! Keepalive, silence timeout, reconnect and backoff (ticket 04).

mod common;

use std::sync::Arc;
use std::time::{Duration, Instant};

use common::{FakeWs, Step, VecSink, book, bybit, config, eventually, plan_of, wait_event};
use md_feed::{ConnIds, Feed, FeedEvent, FrameSink};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn keepalive_frames_sent() {
    let fake = FakeWs::start(vec![vec![]]).await;
    let mut cfg = config(&fake.url, Arc::new(ConnIds::new()));
    cfg.keepalive_interval = Some(Duration::from_millis(100));
    let feed =
        Feed::start(bybit(), cfg, plan_of(&["BTCUSDT"], &["orderbook.50"]), None).expect("start");
    eventually(|| {
        fake.texts(0)
            .iter()
            .filter(|t| *t == r#"{"op":"ping"}"#)
            .count()
            >= 3
    })
    .await;
    drop(feed);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn silence_reconnects() {
    let script = vec![
        Step::WaitFor("subscribe".into()),
        Step::Send(book(
            "BTCUSDT",
            "snapshot",
            1,
            1_000,
            r#"["42000.0","1.000"]"#,
            "",
        )),
        Step::Send(book("BTCUSDT", "delta", 2, 1_020, "", "")),
        // Then silent, connection left open.
    ];
    let fake = FakeWs::start(vec![script]).await;
    let mut cfg = config(&fake.url, Arc::new(ConnIds::new()));
    cfg.silence_timeout = Some(Duration::from_millis(300));
    let sink = Arc::new(VecSink::default());
    let mut feed = Feed::start(
        bybit(),
        cfg,
        plan_of(&["BTCUSDT"], &["orderbook.50"]),
        Some(sink.clone() as Arc<dyn FrameSink>),
    )
    .expect("start");
    let mut ev = feed.take_events().expect("events");
    let d = wait_event(&mut ev, |e| matches!(e, FeedEvent::Disconnected { .. })).await;
    assert!(
        matches!(&d, FeedEvent::Disconnected { reason, .. } if reason.contains("no message")),
        "{d:?}"
    );
    eventually(|| {
        let ids: std::collections::BTreeSet<u32> =
            sink.frames().iter().map(|f| f.conn_id).collect();
        ids.len() == 2
    })
    .await;
    let frames = sink.frames();
    let second = frames.iter().map(|f| f.conn_id).max().expect("frames");
    let nos: Vec<u64> = frames
        .iter()
        .filter(|f| f.conn_id == second)
        .map(|f| f.frame_no)
        .collect();
    assert_eq!(nos.first(), Some(&0), "frame_no counts again from 0");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn server_close_reconnects_with_backoff() {
    let fake = FakeWs::start(vec![vec![Step::Close]]).await;
    let cfg = config(&fake.url, Arc::new(ConnIds::new()));
    let min = cfg.backoff_min;
    let mut feed =
        Feed::start(bybit(), cfg, plan_of(&["BTCUSDT"], &["orderbook.50"]), None).expect("start");
    let mut ev = feed.take_events().expect("events");
    let mut at = Vec::new();
    while at.len() < 3 {
        wait_event(&mut ev, |e| matches!(e, FeedEvent::Connected { .. })).await;
        at.push(Instant::now());
    }
    // Waits min, then 2 × min (no snapshot was applied, so no reset).
    assert!(at[1] - at[0] >= min, "{:?}", at[1] - at[0]);
    assert!(at[2] - at[1] >= min * 2, "{:?}", at[2] - at[1]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn busy_connection_is_not_silent() {
    // A snapshot first: without one the book would time out (spec F3).
    let mut script = vec![
        Step::WaitFor("subscribe".into()),
        Step::Send(book(
            "BTCUSDT",
            "snapshot",
            1,
            1_000,
            r#"["42000.0","1.000"]"#,
            "",
        )),
    ];
    for _ in 0..10 {
        script.push(Step::Send(r#"{"op":"pong"}"#.into()));
        script.push(Step::Pause(Duration::from_millis(100)));
    }
    let fake = FakeWs::start(vec![script]).await;
    let mut cfg = config(&fake.url, Arc::new(ConnIds::new()));
    cfg.silence_timeout = Some(Duration::from_millis(300));
    let mut feed =
        Feed::start(bybit(), cfg, plan_of(&["BTCUSDT"], &["orderbook.50"]), None).expect("start");
    let mut ev = feed.take_events().expect("events");
    tokio::time::sleep(Duration::from_millis(900)).await;
    while let Ok(e) = ev.try_recv() {
        assert!(!matches!(e, FeedEvent::Disconnected { .. }), "{e:?}");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn backoff_not_reset_by_a_short_lived_connection() {
    // F2: a snapshot then a close at once must not reset the backoff.
    let fake = FakeWs::start(vec![vec![
        Step::WaitFor("subscribe".into()),
        Step::Send(book(
            "BTCUSDT",
            "snapshot",
            1,
            1_000,
            r#"["42000.0","1.000"]"#,
            "",
        )),
        Step::Pause(Duration::from_millis(20)),
        Step::Close,
    ]])
    .await;
    let mut cfg = config(&fake.url, Arc::new(ConnIds::new()));
    cfg.backoff_reset_after = Duration::from_secs(10);
    let min = cfg.backoff_min;
    let mut feed =
        Feed::start(bybit(), cfg, plan_of(&["BTCUSDT"], &["orderbook.50"]), None).expect("start");
    let mut ev = feed.take_events().expect("events");
    let mut at = Vec::new();
    while at.len() < 3 {
        wait_event(&mut ev, |e| matches!(e, FeedEvent::Connected { .. })).await;
        at.push(Instant::now());
    }
    assert!(at[2] - at[1] >= min * 2, "{:?}", at[2] - at[1]);
}

/// Records frames and `conn_ended` calls in one sequence.
#[derive(Default)]
struct EndSink(std::sync::Mutex<Vec<(u32, Option<u64>)>>);

impl FrameSink for EndSink {
    fn frame(&self, f: md_feed::RawFrame) -> Result<(), md_feed::Overflow> {
        self.0.lock().expect("sink").push((f.conn_id, Some(f.frame_no)));
        Ok(())
    }
    fn conn_ended(&self, conn_id: u32) {
        self.0.lock().expect("sink").push((conn_id, None));
    }
}

/// U5b spec R5: the connection's own task tells the sink it has ended, after
/// its last frame, once per connection.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn conn_ended_follows_last_frame() {
    let script = vec![
        Step::WaitFor("subscribe".into()),
        Step::Send(book("BTCUSDT", "snapshot", 1, 1_000, r#"["42000.0","1.000"]"#, "")),
        Step::Send(book("BTCUSDT", "delta", 2, 1_020, "", "")),
        Step::Drop,
    ];
    let fake = FakeWs::start(vec![script]).await;
    let sink = Arc::new(EndSink::default());
    let feed = Feed::start(
        bybit(),
        config(&fake.url, Arc::new(ConnIds::new())),
        plan_of(&["BTCUSDT"], &["orderbook.50"]),
        Some(sink.clone() as Arc<dyn FrameSink>),
    )
    .expect("start");
    eventually(|| {
        let s = sink.0.lock().expect("sink");
        s.iter().filter(|(_, f)| f.is_none()).count() >= 2
    })
    .await;
    drop(feed);
    let s = sink.0.lock().expect("sink").clone();
    for id in [1u32, 2] {
        let mine: Vec<Option<u64>> = s.iter().filter(|(c, _)| *c == id).map(|(_, f)| *f).collect();
        assert_eq!(mine, vec![Some(0), Some(1), None], "conn {id}: {s:?}");
    }
}
