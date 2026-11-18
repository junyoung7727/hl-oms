//! The stale flag and its events (ticket 05, spec R3).

mod common;

use std::sync::Arc;
use std::time::Duration;

use common::{FakeWs, Step, book, bybit, config, eventually, key, plan_of, wait_event};
use md_feed::{ConnIds, Feed, FeedEvent};

fn snap(u: i64) -> Step {
    Step::Send(book(
        "BTCUSDT",
        "snapshot",
        u,
        1_000,
        r#"["42000.0","1.000"]"#,
        "",
    ))
}

fn delta(u: i64) -> Step {
    Step::Send(book("BTCUSDT", "delta", u, 1_000 + u, "", ""))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stale_before_first_snapshot() {
    let fake = FakeWs::start(vec![vec![
        Step::WaitFor("subscribe".into()),
        Step::Pause(Duration::from_millis(300)),
        snap(1),
    ]])
    .await;
    let feed = Feed::start(
        bybit(),
        config(&fake.url, Arc::new(ConnIds::new())),
        plan_of(&["BTCUSDT"], &["orderbook.50"]),
        None,
    )
    .expect("start");
    let k = key("orderbook.50", "BTCUSDT");
    eventually(|| fake.accepted.load(std::sync::atomic::Ordering::SeqCst) == 1).await;
    assert!(feed.is_stale(&k));
    eventually(|| !feed.is_stale(&k)).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stale_on_silence_and_cleared_by_next_message() {
    let fake = FakeWs::start(vec![vec![
        Step::WaitFor("subscribe".into()),
        snap(1),
        Step::Pause(Duration::from_millis(700)),
        delta(2),
    ]])
    .await;
    let mut feed = Feed::start(
        bybit(),
        config(&fake.url, Arc::new(ConnIds::new())),
        plan_of(&["BTCUSDT"], &["orderbook.50"]),
        None,
    )
    .expect("start");
    let k = key("orderbook.50", "BTCUSDT");
    let mut ev = feed.take_events().expect("events");
    eventually(|| !feed.is_stale(&k)).await;
    let r = wait_event(&mut ev, |e| matches!(e, FeedEvent::StaleRaised { .. })).await;
    assert!(
        matches!(&r, FeedEvent::StaleRaised { key, last_recv_ns: Some(_), .. } if *key == k),
        "{r:?}"
    );
    assert!(feed.is_stale(&k));
    wait_event(&mut ev, |e| matches!(e, FeedEvent::StaleCleared { .. })).await;
    assert!(!feed.is_stale(&k));
    assert_eq!(feed.market_ref(&k).and_then(|r| r.update_id), Some(2));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stale_during_resync() {
    let fake = FakeWs::start(vec![vec![
        Step::WaitFor("subscribe".into()),
        snap(1),
        delta(2),
        delta(4),
        Step::WaitFor("c0-rsubscribe".into()),
        // Fresh deltas, but no snapshot yet: still stale.
        delta(5),
        Step::Pause(Duration::from_millis(100)),
        delta(6),
        Step::Pause(Duration::from_millis(300)),
        snap(10),
    ]])
    .await;
    let mut feed = Feed::start(
        bybit(),
        config(&fake.url, Arc::new(ConnIds::new())),
        plan_of(&["BTCUSDT"], &["orderbook.50"]),
        None,
    )
    .expect("start");
    let k = key("orderbook.50", "BTCUSDT");
    let mut ev = feed.take_events().expect("events");
    wait_event(&mut ev, |e| matches!(e, FeedEvent::Gap { .. })).await;
    for _ in 0..20 {
        if feed.market_ref(&k).and_then(|r| r.update_id) == Some(10) {
            break;
        }
        assert!(feed.is_stale(&k), "stale while the resync runs");
        tokio::time::sleep(Duration::from_millis(15)).await;
    }
    eventually(|| feed.market_ref(&k).and_then(|r| r.update_id) == Some(10)).await;
    assert!(!feed.is_stale(&k));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn closed_connection_is_stale_at_once() {
    let fake = FakeWs::start(vec![
        vec![
            Step::WaitFor("subscribe".into()),
            snap(1),
            Step::Pause(Duration::from_millis(100)),
            Step::Drop,
        ],
        vec![],
    ])
    .await;
    let mut cfg = config(&fake.url, Arc::new(ConnIds::new()));
    cfg.stale_after = Duration::from_secs(10);
    cfg.backoff_min = Duration::from_secs(2);
    let mut feed =
        Feed::start(bybit(), cfg, plan_of(&["BTCUSDT"], &["orderbook.50"]), None).expect("start");
    let k = key("orderbook.50", "BTCUSDT");
    let mut ev = feed.take_events().expect("events");
    eventually(|| !feed.is_stale(&k)).await;
    wait_event(&mut ev, |e| matches!(e, FeedEvent::Disconnected { .. })).await;
    // stale_after is 10 s and the reconnect waits 2 s: only R3 makes it stale.
    assert!(feed.is_stale(&k));
}
