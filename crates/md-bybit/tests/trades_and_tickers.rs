//! Trades and tickers: the `trade` and `ticker` L1 rows.

mod common;

use common::{instrument, linear};
use md_bybit::{BybitAdapter, Market};
use md_core::adapter::MarketDataAdapter;
use md_core::{Normalized, Side, StreamKey, Ticker, Trade};

fn trades(frame: &str) -> Vec<Trade> {
    let key = StreamKey::new("publicTrade", "BTCUSDT");
    linear()
        .parse(&key, frame.as_bytes(), 5)
        .expect("trade frame")
        .into_iter()
        .map(|m| match m {
            Normalized::Trade(t) => t,
            other => panic!("not a trade: {other:?}"),
        })
        .collect()
}

fn ticker(adapter: &BybitAdapter, frame: &str) -> Result<Ticker, String> {
    let key = StreamKey::new("tickers", "BTCUSDT");
    match adapter.parse(&key, frame.as_bytes(), 5) {
        Ok(msgs) => match msgs.as_slice() {
            [Normalized::Ticker(t)] => Ok(t.clone()),
            other => panic!("not one ticker: {other:?}"),
        },
        Err(e) => Err(e.reason),
    }
}

#[test]
fn trades_keep_order_side_and_flags() {
    let got = trades(
        r#"{"topic":"publicTrade.BTCUSDT","type":"snapshot","ts":1767313799997,"data":[
        {"T":1767313799996,"s":"BTCUSDT","S":"Sell","v":"0.100","p":"41999.9","L":"MinusTick","i":"t-2","BT":false,"RPI":false,"seq":9101},
        {"T":1767313799996,"s":"BTCUSDT","S":"Buy","v":"0.025","p":"42000.0","L":"PlusTick","i":"t-3","BT":true,"RPI":false,"seq":9102},
        {"T":1767313799997,"s":"BTCUSDT","S":"Buy","v":"1","p":"42000.1","L":"ZeroPlusTick","i":"t-4","BT":true,"RPI":true,"seq":9103}]}"#,
    );
    let rows: Vec<_> = got
        .iter()
        .map(|t| {
            (
                t.trade_id.as_str(),
                t.side,
                t.px,
                t.qty,
                t.venue_seq,
                t.flags,
                t.venue_ts_ns,
            )
        })
        .collect();
    assert_eq!(
        rows,
        vec![
            (
                "t-2",
                Side::Sell,
                419999,
                100,
                Some(9101),
                0,
                1_767_313_799_996_000_000
            ),
            (
                "t-3",
                Side::Buy,
                420000,
                25,
                Some(9102),
                1,
                1_767_313_799_996_000_000
            ),
            (
                "t-4",
                Side::Buy,
                420001,
                1000,
                Some(9103),
                3,
                1_767_313_799_997_000_000
            ),
        ]
    );
    for t in &got {
        assert_eq!(
            (t.recv_ns, t.px_exp, t.qty_exp, &t.key),
            (5, -1, -3, &StreamKey::new("publicTrade", "BTCUSDT"))
        );
    }

    let key = StreamKey::new("publicTrade", "BTCUSDT");
    for bad in [
        r#"{"topic":"publicTrade.BTCUSDT","type":"snapshot","ts":1,"data":[{"T":1,"s":"BTCUSDT","S":"None","v":"1","p":"1","i":"x","BT":false,"seq":1}]}"#,
        r#"{"topic":"publicTrade.BTCUSDT","type":"snapshot","ts":1,"data":[{"T":1,"s":"BTCUSDT","S":"Buy","v":"1","p":"1.05","i":"x","BT":false,"seq":1}]}"#,
        r#"{"topic":"publicTrade.BTCUSDT","type":"snapshot","ts":1,"data":[{"T":1,"s":"ETHUSDT","S":"Buy","v":"1","p":"1","i":"x","BT":false,"seq":1}]}"#,
        r#"{"topic":"publicTrade.BTCUSDT","type":"snapshot","ts":1,"data":[{"s":"BTCUSDT","S":"Buy","v":"1","p":"1","i":"x","BT":false,"seq":1}]}"#,
        r#"{"topic":"publicTrade.BTCUSDT","type":"snapshot","ts":1,"data":{}}"#,
    ] {
        assert!(linear().parse(&key, bad.as_bytes(), 5).is_err(), "{bad}");
    }
}

#[test]
fn ticker_delta_carries_only_its_fields_and_merges() {
    let a = linear();
    let mut state = ticker(
        &a,
        r#"{"topic":"tickers.BTCUSDT","type":"snapshot","cs":70001,"ts":1767312006497,"data":{"symbol":"BTCUSDT",
        "lastPrice":"42000.1","markPrice":"42000.0","indexPrice":"41999.95","openInterest":"1234.567",
        "fundingRate":"0.0001","nextFundingTime":"1767340800000","bid1Price":"41999.9","bid1Size":"2.000",
        "ask1Price":"42000.2","ask1Size":"3.000","volume24h":"5.5","tickDirection":"PlusTick"}}"#,
    )
    .expect("snapshot");
    assert!(state.is_snapshot);
    assert_eq!(
        (state.px_exp, state.ref_px_exp, state.qty_exp),
        (-1, -3, -3)
    );
    assert_eq!(
        (
            state.last_px,
            state.mark_px,
            state.index_px,
            state.open_interest,
            state.funding_rate,
            state.next_funding_ns
        ),
        (
            Some(420001),
            Some(42000000),
            Some(41999950),
            Some(1234567),
            Some(10000),
            Some(1_767_340_800_000_000_000)
        )
    );
    assert_eq!(
        (
            state.best_bid_px,
            state.best_bid_qty,
            state.best_ask_px,
            state.best_ask_qty
        ),
        (Some(419999), Some(2000), Some(420002), Some(3000))
    );
    assert_eq!(state.venue_ts_ns, 1_767_312_006_497_000_000);

    let delta = ticker(
        &a,
        r#"{"topic":"tickers.BTCUSDT","type":"delta","cs":70002,"ts":1767312299997,"data":{"symbol":"BTCUSDT","lastPrice":"42000.2","bid1Size":"2.500","markPrice":"42000.15"}}"#,
    )
    .expect("delta");
    assert!(!delta.is_snapshot);
    assert_eq!(
        (delta.last_px, delta.best_bid_qty, delta.mark_px),
        (Some(420002), Some(2500), Some(42000150))
    );
    assert_eq!(
        (
            delta.index_px,
            delta.open_interest,
            delta.funding_rate,
            delta.best_ask_px
        ),
        (None, None, None, None)
    );
    assert!(state.merge(&delta));
    assert_eq!(
        (
            state.last_px,
            state.best_bid_qty,
            state.mark_px,
            state.index_px
        ),
        (Some(420002), Some(2500), Some(42000150), Some(41999950))
    );

    // lastPrice is a traded price: it must be at the tick.
    let fine_last = r#"{"topic":"tickers.BTCUSDT","type":"delta","cs":1,"ts":1,"data":{"symbol":"BTCUSDT","lastPrice":"42000.15"}}"#;
    assert!(ticker(&a, fine_last).is_err());
    let too_fine_mark = r#"{"topic":"tickers.BTCUSDT","type":"delta","cs":1,"ts":1,"data":{"symbol":"BTCUSDT","markPrice":"42000.1555"}}"#;
    assert!(ticker(&a, too_fine_mark).is_err());
    let wrong_symbol = r#"{"topic":"tickers.BTCUSDT","type":"delta","cs":1,"ts":1,"data":{"symbol":"ETHUSDT","lastPrice":"1"}}"#;
    assert!(ticker(&a, wrong_symbol).is_err());
}

#[test]
fn spot_ticker_is_a_snapshot_without_book_fields() {
    let spot = BybitAdapter::new(
        Market::Spot,
        &[instrument("BTCUSDT", "0.01", "0.000001", -2, -6)],
    );
    let t = ticker(
        &spot,
        r#"{"topic":"tickers.BTCUSDT","ts":1767312006497,"type":"snapshot","cs":1,"data":{"symbol":"BTCUSDT",
        "lastPrice":"42000.01","highPrice24h":"42100","lowPrice24h":"41000","prevPrice24h":"41500",
        "volume24h":"10.5","turnover24h":"440000","price24hPcnt":"0.012","usdIndexPrice":"42000.0123456"}}"#,
    )
    .expect("spot snapshot");
    assert!(t.is_snapshot);
    assert_eq!(t.last_px, Some(4200001));
    assert_eq!(
        (
            t.mark_px,
            t.index_px,
            t.best_bid_px,
            t.open_interest,
            t.funding_rate
        ),
        (None, None, None, None, None)
    );
    let delta = r#"{"topic":"tickers.BTCUSDT","ts":1,"type":"delta","cs":1,"data":{"symbol":"BTCUSDT","lastPrice":"1"}}"#;
    assert!(
        ticker(&spot, delta).is_err(),
        "spot tickers are snapshot only"
    );
}
