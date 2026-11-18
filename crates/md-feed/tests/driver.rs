//! The connection driver: subscribe, deliver every frame to the sink with its
//! connection number and position, and the loopback guard (ticket 01).

mod common;

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use common::{FakeWs, Step, VecSink, book, bybit, config, eventually, wait_event};
use md_core::adapter::MarketDataAdapter;
use md_feed::{ConnIds, Feed, FeedEvent, FrameSink, Src};

fn plans(symbols: &[&str], per_conn: usize) -> Vec<md_core::adapter::ConnPlan> {
    let a = md_bybit::BybitAdapter::new(
        md_bybit::Market::Linear,
        &[common::instrument("BTCUSDT"), common::instrument("ETHUSDT")],
    )
    .with_max_topics_per_conn(per_conn);
    let symbols: Vec<String> = symbols.iter().map(|s| s.to_string()).collect();
    a.plan(&symbols, &["orderbook.50".to_string()], 50)
        .expect("plan")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn frames_reach_sink_in_order() {
    let n = 20;
    let mut frames = vec![Step::WaitFor("subscribe".into())];
    for i in 0..n {
        frames.push(Step::Send(format!(r#"{{"op":"pong","n":{i}}}"#)));
    }
    let fake = FakeWs::start(vec![frames]).await;
    let sink = Arc::new(VecSink::default());
    let feed = Feed::start(
        bybit(),
        config(&fake.url, Arc::new(ConnIds::new())),
        plans(&["BTCUSDT", "ETHUSDT"], 1),
        Some(sink.clone() as Arc<dyn FrameSink>),
    )
    .expect("start");
    eventually(|| sink.frames().len() == 2 * n).await;
    let mut by_conn: BTreeMap<u32, Vec<u64>> = BTreeMap::new();
    let mut last_ns: BTreeMap<u32, i64> = BTreeMap::new();
    for f in sink.frames() {
        assert_eq!(f.src, Src::Ws);
        assert!(f.conn_id >= 1, "conn_id 0 is for REST and notes");
        let prev = last_ns.insert(f.conn_id, f.recv_ns).unwrap_or(0);
        assert!(f.recv_ns >= prev);
        by_conn.entry(f.conn_id).or_default().push(f.frame_no);
    }
    assert_eq!(by_conn.len(), 2, "two connections");
    for nos in by_conn.values() {
        assert_eq!(*nos, (0..n as u64).collect::<Vec<_>>());
    }
    drop(feed);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn subscribe_frames_are_sent() {
    let fake = FakeWs::start(vec![vec![]]).await;
    let plans = plans(&["BTCUSDT", "ETHUSDT"], 200);
    let want = bybit().subscribe_frames(&plans[0]);
    let feed = Feed::start(
        bybit(),
        config(&fake.url, Arc::new(ConnIds::new())),
        plans,
        None,
    )
    .expect("start");
    eventually(|| fake.texts(0).len() >= want.len()).await;
    assert_eq!(fake.texts(0)[..want.len()], want[..]);
    drop(feed);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn conn_ids_shared_across_feeds() {
    let fake = FakeWs::start(vec![vec![Step::Send(book(
        "BTCUSDT",
        "snapshot",
        1,
        1_000,
        r#"["42000.0","1.000"]"#,
        "",
    ))]])
    .await;
    let ids = Arc::new(ConnIds::new());
    let sink = Arc::new(VecSink::default());
    let a = Feed::start(
        bybit(),
        config(&fake.url, ids.clone()),
        plans(&["BTCUSDT", "ETHUSDT"], 1),
        Some(sink.clone() as Arc<dyn FrameSink>),
    )
    .expect("start");
    let b = Feed::start(
        bybit(),
        config(&fake.url, ids.clone()),
        plans(&["BTCUSDT", "ETHUSDT"], 1),
        Some(sink.clone() as Arc<dyn FrameSink>),
    )
    .expect("start");
    eventually(|| sink.frames().len() == 4).await;
    let mut ids_seen: Vec<u32> = sink.frames().iter().map(|f| f.conn_id).collect();
    ids_seen.sort();
    ids_seen.dedup();
    assert_eq!(ids_seen.len(), 4, "four connections, four conn_ids");
    drop((a, b));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn non_loopback_refused() {
    // TEST-NET-3 (RFC 5737): never routed. The guard refuses it before any
    // socket is opened.
    let mut feed = Feed::start(
        bybit(),
        config("ws://203.0.113.1:9/", Arc::new(ConnIds::new())),
        plans(&["BTCUSDT"], 200),
        None,
    )
    .expect("start");
    let mut events = feed.take_events().expect("events");
    let start = std::time::Instant::now();
    let e = wait_event(&mut events, |e| matches!(e, FeedEvent::Disconnected { .. })).await;
    let FeedEvent::Disconnected { reason, .. } = e else {
        unreachable!()
    };
    assert!(reason.contains("not loopback"), "{reason}");
    assert!(start.elapsed() < Duration::from_secs(1));
}

/// A sink whose queue is full after `cap` frames.
struct FullSink {
    cap: usize,
    n: std::sync::atomic::AtomicUsize,
}

impl FrameSink for FullSink {
    fn frame(&self, _f: md_feed::RawFrame) -> Result<(), md_feed::Overflow> {
        if self.n.fetch_add(1, std::sync::atomic::Ordering::SeqCst) < self.cap {
            Ok(())
        } else {
            Err(md_feed::Overflow)
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sink_overflow_closes_the_connection() {
    let mut frames = vec![Step::WaitFor("subscribe".into())];
    for _ in 0..10 {
        frames.push(Step::Send(r#"{"op":"pong"}"#.into()));
    }
    let fake = FakeWs::start(vec![frames]).await;
    let sink = Arc::new(FullSink {
        cap: 3,
        n: Default::default(),
    });
    let mut feed = Feed::start(
        bybit(),
        config(&fake.url, Arc::new(ConnIds::new())),
        plans(&["BTCUSDT"], 200),
        Some(sink as Arc<dyn FrameSink>),
    )
    .expect("start");
    let mut events = feed.take_events().expect("events");
    let e = wait_event(&mut events, |e| matches!(e, FeedEvent::Disconnected { .. })).await;
    assert!(
        matches!(&e, FeedEvent::Disconnected { reason, .. } if reason == "overflow"),
        "{e:?}"
    );
}
