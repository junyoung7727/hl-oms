//! Gaps and the Resubscribe resync (ticket 03).

mod common;

use std::sync::Arc;
use std::time::Duration;

use common::{FakeWs, Step, book, bybit, config, eventually, key, plan_of, wait_event};
use md_core::adapter::Resync;
use md_core::{BookSide, Level};
use md_feed::{ConnIds, Feed, FeedEvent};

fn lv(px: i64, qty: i64) -> Level {
    Level { px, qty }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn missing_u_gives_one_gap_and_resync() {
    let fake = FakeWs::start(vec![vec![
        Step::WaitFor("subscribe".into()),
        Step::Send(book(
            "BTCUSDT",
            "snapshot",
            100,
            1_000,
            r#"["42000.0","1.000"]"#,
            r#"["42000.1","1.000"]"#,
        )),
        Step::Send(book(
            "BTCUSDT",
            "delta",
            101,
            1_020,
            r#"["41999.0","1.000"]"#,
            "",
        )),
        // u 102 is missing.
        Step::Send(book(
            "BTCUSDT",
            "delta",
            103,
            1_060,
            r#"["41998.0","1.000"]"#,
            "",
        )),
        Step::Send(book(
            "BTCUSDT",
            "delta",
            104,
            1_080,
            r#"["41997.0","1.000"]"#,
            "",
        )),
        Step::WaitFor(r#""op":"unsubscribe""#.into()),
        Step::WaitFor("c0-rsubscribe".into()),
        // Bybit answers a resubscribe with a new snapshot (u may restart).
        Step::Send(book(
            "BTCUSDT",
            "snapshot",
            7,
            2_000,
            r#"["41990.0","5.000"]"#,
            r#"["41990.5","2.000"]"#,
        )),
        Step::Send(book(
            "BTCUSDT",
            "delta",
            8,
            2_020,
            r#"["41989.0","1.000"]"#,
            "",
        )),
    ]])
    .await;
    let mut feed = Feed::start(
        bybit(),
        config(&fake.url, Arc::new(ConnIds::new())),
        plan_of(&["BTCUSDT"], &["orderbook.50"]),
        None,
    )
    .expect("start");
    let mut ev = feed.take_events().expect("events");
    let g = wait_event(&mut ev, |e| matches!(e, FeedEvent::Gap { .. })).await;
    let FeedEvent::Gap { gap, resync, .. } = g else {
        unreachable!()
    };
    assert_eq!(
        (gap.last_good, gap.next, resync),
        (Some(101), Some(103), Resync::Resubscribe)
    );
    let k = key("orderbook.50", "BTCUSDT");
    eventually(|| feed.market_ref(&k).and_then(|r| r.update_id) == Some(8)).await;
    let (bids, asks, stale) = feed
        .read_book(&k, |v| {
            (
                v.book.levels(BookSide::Bid),
                v.book.levels(BookSide::Ask),
                v.stale,
            )
        })
        .expect("book");
    assert_eq!(
        bids,
        vec![lv(419900, 5000), lv(419890, 1000)],
        "the fake's book after the resync"
    );
    assert_eq!(asks, vec![lv(419905, 2000)]);
    assert!(!stale);
    let texts = fake.texts(0);
    assert!(
        texts
            .iter()
            .any(|t| t.contains(r#""op":"unsubscribe""#) && t.contains("orderbook.50.BTCUSDT"))
    );
    // Exactly one gap event, on the same connection.
    tokio::time::sleep(Duration::from_millis(200)).await;
    while let Ok(e) = ev.try_recv() {
        assert!(!matches!(e, FeedEvent::Gap { .. }), "second gap: {e:?}");
        assert!(!matches!(e, FeedEvent::Disconnected { .. }), "{e:?}");
    }
    assert_eq!(fake.accepted.load(std::sync::atomic::Ordering::SeqCst), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn resync_timeout_reconnects() {
    let first = vec![
        Step::WaitFor("subscribe".into()),
        Step::Send(book(
            "BTCUSDT",
            "snapshot",
            100,
            1_000,
            r#"["42000.0","1.000"]"#,
            "",
        )),
        Step::Send(book("BTCUSDT", "delta", 105, 1_020, "", "")),
        // Ignores the resubscribe.
    ];
    let second = vec![
        Step::WaitFor("subscribe".into()),
        Step::Send(book(
            "BTCUSDT",
            "snapshot",
            1,
            3_000,
            r#"["43000.0","1.000"]"#,
            "",
        )),
    ];
    let fake = FakeWs::start(vec![first, second]).await;
    let mut feed = Feed::start(
        bybit(),
        config(&fake.url, Arc::new(ConnIds::new())),
        plan_of(&["BTCUSDT"], &["orderbook.50"]),
        None,
    )
    .expect("start");
    let mut ev = feed.take_events().expect("events");
    let FeedEvent::Connected { conn_id: c1, .. } =
        wait_event(&mut ev, |e| matches!(e, FeedEvent::Connected { .. })).await
    else {
        unreachable!()
    };
    let started = std::time::Instant::now();
    let d = wait_event(&mut ev, |e| matches!(e, FeedEvent::Disconnected { .. })).await;
    assert!(
        matches!(&d, FeedEvent::Disconnected { reason, .. } if reason.contains("resync")),
        "{d:?}"
    );
    assert!(
        started.elapsed() >= Duration::from_millis(700),
        "after resync_timeout (800 ms)"
    );
    let FeedEvent::Connected { conn_id: c2, .. } =
        wait_event(&mut ev, |e| matches!(e, FeedEvent::Connected { .. })).await
    else {
        unreachable!()
    };
    assert_ne!(c1, c2, "a reconnect is a new connection");
    let k = key("orderbook.50", "BTCUSDT");
    eventually(|| feed.market_ref(&k).and_then(|r| r.update_id) == Some(1)).await;
    assert!(!feed.is_stale(&k));
}
