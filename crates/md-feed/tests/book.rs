//! The in-process book, tickers, `market_ref` and the read API (ticket 02).

mod common;

use std::sync::Arc;

use common::{
    FakeWs, Step, VecMsgs, VecSink, book, bybit, config, eventually, key, plan_of, ticker, trade,
    wait_event,
};
use md_core::{BookSide, Level, Normalized};
use md_feed::{ConnIds, Feed, FeedEvent, FrameSink, MessageSink};

fn lv(px: i64, qty: i64) -> Level {
    Level { px, qty }
}

fn snap() -> Step {
    Step::Send(book(
        "BTCUSDT",
        "snapshot",
        100,
        1_000,
        r#"["42000.0","1.000"],["41999.9","2.000"]"#,
        r#"["42000.1","1.500"]"#,
    ))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn book_matches_fake_after_snapshot_and_deltas() {
    let fake = FakeWs::start(vec![vec![
        Step::WaitFor("subscribe".into()),
        snap(),
        Step::Send(book(
            "BTCUSDT",
            "delta",
            101,
            1_020,
            r#"["41999.9","0"]"#,
            "",
        )),
        Step::Send(book(
            "BTCUSDT",
            "delta",
            102,
            1_040,
            "",
            r#"["42000.2","3.000"]"#,
        )),
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
    eventually(|| feed.market_ref(&k).and_then(|r| r.update_id) == Some(102)).await;
    let (bids, asks, stale) = feed
        .read_book(&k, |v| {
            (
                v.book.levels(BookSide::Bid),
                v.book.levels(BookSide::Ask),
                v.stale,
            )
        })
        .expect("book");
    assert_eq!(bids, vec![lv(420000, 1000)]);
    assert_eq!(asks, vec![lv(420001, 1500), lv(420002, 3000)]);
    assert!(!stale);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn market_ref_is_last_applied_message() {
    let fake = FakeWs::start(vec![vec![
        Step::WaitFor("subscribe".into()),
        snap(),
        Step::Send(book(
            "BTCUSDT",
            "delta",
            101,
            1_020,
            r#"["41999.9","0"]"#,
            "",
        )),
    ]])
    .await;
    let sink = Arc::new(VecSink::default());
    let feed = Feed::start(
        bybit(),
        config(&fake.url, Arc::new(ConnIds::new())),
        plan_of(&["BTCUSDT"], &["orderbook.50"]),
        Some(sink.clone() as Arc<dyn FrameSink>),
    )
    .expect("start");
    let k = key("orderbook.50", "BTCUSDT");
    eventually(|| feed.market_ref(&k).and_then(|r| r.update_id) == Some(101)).await;
    let r = feed.market_ref(&k).expect("ref");
    let last = sink
        .frames()
        .into_iter()
        .find(|f| String::from_utf8_lossy(&f.payload).contains(r#""u":101"#))
        .expect("delta frame in the sink");
    assert_eq!(
        (
            r.venue.as_str(),
            r.market.as_str(),
            r.symbol.as_str(),
            r.channel.as_str()
        ),
        ("bybit", "linear", "BTCUSDT", "orderbook.50")
    );
    assert_eq!(r.venue_ts_ns, 1_020 * 1_000_000);
    assert_eq!(r.recv_ns, last.recv_ns);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ticker_merges_and_trades_reach_message_sink() {
    let fake = FakeWs::start(vec![vec![
        Step::WaitFor("subscribe".into()),
        Step::Send(ticker(
            "BTCUSDT",
            "snapshot",
            1_000,
            r#","lastPrice":"42000.0","bid1Price":"41999.9""#,
        )),
        Step::Send(ticker(
            "BTCUSDT",
            "delta",
            1_100,
            r#","lastPrice":"42001.0""#,
        )),
        Step::Send(trade("BTCUSDT", "t-1", 1_200)),
    ]])
    .await;
    let msgs = Arc::new(VecMsgs::default());
    let feed = Feed::start_with(
        bybit(),
        config(&fake.url, Arc::new(ConnIds::new())),
        plan_of(&["BTCUSDT"], &["publicTrade", "tickers"]),
        None,
        Some(msgs.clone() as Arc<dyn MessageSink>),
    )
    .expect("start");
    eventually(|| msgs.0.lock().expect("msgs").len() == 3).await;
    let t = feed.ticker(&key("tickers", "BTCUSDT")).expect("ticker");
    assert_eq!(t.last_px, Some(420010));
    assert_eq!(t.best_bid_px, Some(419999), "kept from the snapshot");
    assert_eq!(t.venue_ts_ns, 1_100 * 1_000_000);
    let got = msgs.0.lock().expect("msgs");
    assert!(matches!(&got[2].1, Normalized::Trade(tr) if tr.trade_id == "t-1"));
    // Trades and tickers are not books: no book, always stale for reads.
    assert!(
        feed.read_book(&key("publicTrade", "BTCUSDT"), |_| ())
            .is_none()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn resnapshot_event() {
    let fake = FakeWs::start(vec![vec![
        Step::WaitFor("subscribe".into()),
        snap(),
        Step::Send(book(
            "BTCUSDT",
            "snapshot",
            1,
            2_000,
            r#"["41000.0","1.000"]"#,
            "",
        )),
        Step::Send(book(
            "BTCUSDT",
            "delta",
            2,
            2_020,
            r#"["41000.1","1.000"]"#,
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
    let e = wait_event(&mut ev, |e| matches!(e, FeedEvent::Resnapshot { .. })).await;
    assert!(matches!(e, FeedEvent::Resnapshot { ref key, .. } if key.symbol == "BTCUSDT"));
    let k = key("orderbook.50", "BTCUSDT");
    eventually(|| feed.market_ref(&k).and_then(|r| r.update_id) == Some(2)).await;
    let bids = feed
        .read_book(&k, |v| v.book.levels(BookSide::Bid))
        .expect("book");
    assert_eq!(
        bids,
        vec![lv(410001, 1000), lv(410000, 1000)],
        "restarted from the new snapshot"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn parse_error_event() {
    let bad = r#"{"topic":"orderbook.50.BTCUSDT","type":"delta","ts":1020,"data":{"s":"BTCUSDT","b":[["x","1"]],"a":[],"u":101,"seq":1}}"#;
    let fake = FakeWs::start(vec![vec![
        Step::WaitFor("subscribe".into()),
        snap(),
        Step::Send(bad.into()),
        Step::Send(book("BTCUSDT", "delta", 102, 1_040, "", "")),
    ]])
    .await;
    let sink = Arc::new(VecSink::default());
    let mut feed = Feed::start(
        bybit(),
        config(&fake.url, Arc::new(ConnIds::new())),
        plan_of(&["BTCUSDT"], &["orderbook.50"]),
        Some(sink.clone() as Arc<dyn FrameSink>),
    )
    .expect("start");
    let mut ev = feed.take_events().expect("events");
    let e = wait_event(&mut ev, |e| matches!(e, FeedEvent::ParseError { .. })).await;
    let FeedEvent::ParseError { frame_no, .. } = e else {
        unreachable!()
    };
    // The broken frame is still in the sink (L0 keeps everything).
    eventually(|| sink.frames().iter().any(|f| f.frame_no == frame_no)).await;
    // The book may be wrong: handled as one gap, and stale until a snapshot.
    let g = wait_event(&mut ev, |e| matches!(e, FeedEvent::Gap { .. })).await;
    // From the parse error itself (`next` unknown), not from the next delta.
    assert!(
        matches!(g, FeedEvent::Gap { ref gap, .. } if gap.last_good == Some(100) && gap.next.is_none()),
        "{g:?}"
    );
    assert!(feed.is_stale(&key("orderbook.50", "BTCUSDT")));
    // No second gap from the next delta (R5).
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    while let Ok(e) = ev.try_recv() {
        assert!(!matches!(e, FeedEvent::Gap { .. }), "second gap: {e:?}");
    }
}
