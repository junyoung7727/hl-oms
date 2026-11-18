//! The overlapping connection switch (ticket 07, spec R7).

mod common;

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use common::{FakeWs, Step, VecSink, book, bybit, config, eventually, key, plan_of, wait_event};
use md_feed::{ConnIds, Feed, FeedEvent, FrameSink};

fn snap(u: i64) -> Step {
    Step::Send(book("BTCUSDT", "snapshot", u, 1_000, r#"["42000.0","1.000"]"#, ""))
}

fn deltas(from: i64, n: i64) -> Vec<Step> {
    let mut v = Vec::new();
    for u in from..from + n {
        v.push(Step::Send(book("BTCUSDT", "delta", u, 1_000 + u, "", "")));
        v.push(Step::Pause(Duration::from_millis(40)));
    }
    v
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn switch_keeps_both_until_old_closed() {
    let mut old = vec![Step::WaitFor("subscribe".into()), snap(1)];
    old.extend(deltas(2, 60));
    let mut new = vec![Step::WaitFor("subscribe".into())];
    for _ in 0..4 {
        new.push(Step::Send(r#"{"op":"pong"}"#.into()));
        new.push(Step::Pause(Duration::from_millis(50)));
    }
    new.push(snap(1_000));
    new.extend(deltas(1_001, 60));
    let fake = FakeWs::start(vec![old, new]).await;
    let sink = Arc::new(VecSink::default());
    let mut feed = Feed::start(
        bybit(),
        config(&fake.url, Arc::new(ConnIds::new())),
        plan_of(&["BTCUSDT"], &["orderbook.50"]),
        Some(sink.clone() as Arc<dyn FrameSink>),
    )
    .expect("start");
    let k = key("orderbook.50", "BTCUSDT");
    let mut ev = feed.take_events().expect("events");
    eventually(|| feed.market_ref(&k).is_some()).await;
    let c1 = feed.active_conn_id(0).expect("active");
    feed.switch(0).expect("switch");
    let done = wait_event(&mut ev, |e| matches!(e, FeedEvent::SwitchDone { .. })).await;
    let FeedEvent::SwitchDone { from_conn_id, to_conn_id: c2, .. } = done else { unreachable!() };
    assert_eq!(from_conn_id, c1);
    assert_ne!(c1, c2);
    // Reads move to the new connection at once.
    assert_eq!(feed.active_conn_id(0), Some(c2));
    assert!(feed.market_ref(&k).and_then(|r| r.update_id).is_some_and(|u| u >= 1_000));
    let d = wait_event(&mut ev, |e| matches!(e, FeedEvent::Disconnected { .. })).await;
    assert!(matches!(d, FeedEvent::Disconnected { conn_id, .. } if conn_id == c1), "{d:?}");
    // Overlap: the old connection delivered frames after the new one's first.
    let frames = sink.frames();
    let first_new = frames.iter().find(|f| f.conn_id == c2).expect("new frames").recv_ns;
    assert!(frames.iter().any(|f| f.conn_id == c1 && f.recv_ns > first_new), "no overlap");
    // After the old one closed, only the new one delivers.
    let old_count = frames.iter().filter(|f| f.conn_id == c1).count();
    tokio::time::sleep(Duration::from_millis(300)).await;
    let later = sink.frames();
    assert_eq!(later.iter().filter(|f| f.conn_id == c1).count(), old_count);
    assert!(later.iter().filter(|f| f.conn_id == c2).count() > frames.iter().filter(|f| f.conn_id == c2).count());
    assert!(!feed.is_stale(&k));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn switch_fails_without_snapshot() {
    let mut old = vec![Step::WaitFor("subscribe".into()), snap(1)];
    old.extend(deltas(2, 80));
    let new = vec![Step::WaitFor("subscribe".into())];
    let fake = FakeWs::start(vec![old, new]).await;
    let mut feed = Feed::start(
        bybit(),
        config(&fake.url, Arc::new(ConnIds::new())),
        plan_of(&["BTCUSDT"], &["orderbook.50"]),
        None,
    )
    .expect("start");
    let k = key("orderbook.50", "BTCUSDT");
    let mut ev = feed.take_events().expect("events");
    eventually(|| feed.market_ref(&k).is_some()).await;
    let c1 = feed.active_conn_id(0).expect("active");
    feed.switch(0).expect("switch");
    let f = wait_event(&mut ev, |e| matches!(e, FeedEvent::SwitchFailed { .. })).await;
    assert!(matches!(f, FeedEvent::SwitchFailed { conn_id, .. } if conn_id != c1), "{f:?}");
    assert_eq!(feed.active_conn_id(0), Some(c1), "the old connection stays");
    let u = feed.market_ref(&k).and_then(|r| r.update_id).expect("u");
    eventually(|| feed.market_ref(&k).and_then(|r| r.update_id).is_some_and(|x| x > u)).await;
    assert!(!feed.is_stale(&k));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn old_connection_dies_during_switch() {
    let old = vec![
        Step::WaitFor("subscribe".into()),
        snap(1),
        Step::Pause(Duration::from_millis(400)),
        Step::Drop,
    ];
    let new = vec![
        Step::WaitFor("subscribe".into()),
        Step::Pause(Duration::from_millis(800)),
        snap(1_000),
    ];
    let fake = FakeWs::start(vec![old, new]).await;
    let mut cfg = config(&fake.url, Arc::new(ConnIds::new()));
    cfg.resync_timeout = Duration::from_secs(3);
    let mut feed = Feed::start(bybit(), cfg, plan_of(&["BTCUSDT"], &["orderbook.50"]), None)
        .expect("start");
    let k = key("orderbook.50", "BTCUSDT");
    let mut ev = feed.take_events().expect("events");
    eventually(|| feed.market_ref(&k).is_some()).await;
    let c1 = feed.active_conn_id(0).expect("active");
    feed.switch(0).expect("switch");
    let done = wait_event(&mut ev, |e| matches!(e, FeedEvent::SwitchDone { .. })).await;
    let FeedEvent::SwitchDone { from_conn_id, to_conn_id: c2, .. } = done else { unreachable!() };
    assert_eq!(from_conn_id, c1);
    // The switch's connection took over: stale until its snapshot, and no
    // third connection was opened.
    assert!(feed.is_stale(&k));
    eventually(|| feed.market_ref(&k).and_then(|r| r.update_id) == Some(1_000)).await;
    assert_eq!(feed.active_conn_id(0), Some(c2));
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(fake.accepted.load(Ordering::SeqCst), 2);
}
