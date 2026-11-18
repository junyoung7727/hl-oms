//! A frame routed to the wrong stream, or an instrument with the wrong
//! exponent, would put every later price of that symbol in the wrong place.

use md_bybit::{BybitAdapter, Market, next_page_cursor};
use md_core::StreamKey;
use md_core::adapter::{MarketDataAdapter, Route};

fn adapter(market: Market) -> BybitAdapter {
    BybitAdapter::new(market, &[])
}

#[test]
fn route_maps_each_topic_and_never_fails() {
    let a = adapter(Market::Linear);
    for (topic, channel, symbol) in [
        ("orderbook.50.BTCUSDT", "orderbook.50", "BTCUSDT"),
        ("orderbook.1000.ETHUSDT", "orderbook.1000", "ETHUSDT"),
        ("orderbook.1.SOLUSDT", "orderbook.1", "SOLUSDT"),
        ("orderbook.200.XRPUSDT", "orderbook.200", "XRPUSDT"),
        ("publicTrade.BTCUSDT", "publicTrade", "BTCUSDT"),
        ("tickers.1000PEPEUSDT", "tickers", "1000PEPEUSDT"),
    ] {
        let frame = format!(r#"{{"topic":"{topic}","type":"delta","ts":1,"data":{{}}}}"#);
        assert_eq!(
            a.route(frame.as_bytes()),
            Route::Stream(StreamKey::new(channel, symbol)),
            "{topic}"
        );
    }
    let garbage: [&[u8]; 10] = [
        b"",
        &[0xff, 0xfe, 0x00],
        br#"{"op":"pong","success":true}"#,
        br#"{"success":true,"ret_msg":"","conn_id":"x","req_id":"c0-0","op":"subscribe"}"#,
        br#"{"topic":"orderbook.7.BTCUSDT","type":"delta"}"#,
        br#"{"topic":"kline.1.BTCUSDT","type":"snapshot"}"#,
        br#"{"topic":"orderbook.50.","type":"delta"}"#,
        br#"{"topic":42}"#,
        b"[1,2,3]",
        br#"{"topic":"publicTrade.BTC.USDT"}"#,
    ];
    for frame in garbage {
        assert_eq!(a.route(frame), Route::Control, "{frame:?}");
    }
}

const LINEAR: &[u8] = br#"{"retCode":0,"retMsg":"OK","result":{"category":"linear","list":[
 {"symbol":"BTCUSDT","status":"Trading","contractType":"LinearPerpetual","settleCoin":"USDT",
  "priceFilter":{"tickSize":"0.10","minPrice":"0.10"},"lotSizeFilter":{"qtyStep":"0.001","minOrderQty":"0.001"}},
 {"symbol":"1000PEPEUSDT","status":"PreLaunch","contractType":"LinearPerpetual","settleCoin":"USDT",
  "priceFilter":{"tickSize":"0.0000010"},"lotSizeFilter":{"qtyStep":"100"}}],
 "nextPageCursor":"abc%3D"},"time":1767312000497}"#;

const SPOT: &[u8] = br#"{"retCode":0,"retMsg":"OK","result":{"category":"spot","list":[
 {"symbol":"BTCUSDT","status":"Trading","baseCoin":"BTC","quoteCoin":"USDT",
  "priceFilter":{"tickSize":"0.01"},"lotSizeFilter":{"basePrecision":"0.000001","quotePrecision":"0.00000001"}}]},
 "time":1767312000123}"#;

#[test]
fn instruments_give_exponents_for_linear_and_spot() {
    let rows = adapter(Market::Linear)
        .parse_instruments(LINEAR, 99)
        .expect("linear body");
    assert_eq!(rows.len(), 2);
    let btc = &rows[0];
    assert_eq!(
        (
            btc.symbol.as_str(),
            btc.status.as_str(),
            btc.tick_size.as_str(),
            btc.lot_size.as_str()
        ),
        ("BTCUSDT", "Trading", "0.10", "0.001")
    );
    assert_eq!((btc.px_exp, btc.qty_exp), (-1, -3));
    assert_eq!(btc.contract_type.as_deref(), Some("LinearPerpetual"));
    assert_eq!(btc.settle_coin.as_deref(), Some("USDT"));
    assert_eq!(btc.venue_ts_ns, Some(1_767_312_000_497_000_000));
    assert_eq!((rows[1].px_exp, rows[1].qty_exp), (-6, 0));

    let spot = adapter(Market::Spot)
        .parse_instruments(SPOT, 99)
        .expect("spot body");
    assert_eq!(
        (spot[0].px_exp, spot[0].qty_exp, spot[0].lot_size.as_str()),
        (-2, -6, "0.000001")
    );
    assert_eq!(spot[0].contract_type, None);

    let refused = br#"{"retCode":10001,"retMsg":"params error","result":{},"time":1}"#;
    assert!(
        adapter(Market::Linear)
            .parse_instruments(refused, 1)
            .is_err()
    );
    let bad_tick = br#"{"retCode":0,"result":{"list":[{"symbol":"X","status":"Trading","priceFilter":{"tickSize":"1e-1"},"lotSizeFilter":{"qtyStep":"1"}}]},"time":1}"#;
    assert!(
        adapter(Market::Linear)
            .parse_instruments(bad_tick, 1)
            .is_err()
    );
}

#[test]
fn instruments_request_pages_linear_but_not_spot() {
    let q = |pairs: &[(&str, &str)]| -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    };
    let linear = adapter(Market::Linear);
    let first = linear.instruments_request();
    assert_eq!(
        (first.method, first.path.as_str()),
        ("GET", "/v5/market/instruments-info")
    );
    assert_eq!(first.query, q(&[("category", "linear"), ("limit", "1000")]));
    let next = linear.instruments_request_page(Some("abc%3D"));
    assert_eq!(
        next.query,
        q(&[
            ("category", "linear"),
            ("limit", "1000"),
            ("cursor", "abc%3D")
        ])
    );
    assert_eq!(
        adapter(Market::Inverse).instruments_request().query,
        q(&[("category", "inverse"), ("limit", "1000")])
    );
    let spot = adapter(Market::Spot);
    assert_eq!(spot.instruments_request().query, q(&[("category", "spot")]));
    assert_eq!(
        spot.instruments_request_page(Some("x")).query,
        q(&[("category", "spot")])
    );

    assert_eq!(next_page_cursor(LINEAR).as_deref(), Some("abc%3D"));
    assert_eq!(next_page_cursor(SPOT), None);
    assert_eq!(
        next_page_cursor(br#"{"retCode":0,"result":{"list":[],"nextPageCursor":""}}"#),
        None
    );
}

#[test]
fn venue_and_time_fields() {
    let a = adapter(Market::Linear);
    assert_eq!(a.venue(), "bybit");
    assert_eq!(a.venue_ts_field("orderbook.50"), "ts");
    assert_eq!(a.venue_ts_field("tickers"), "ts");
    assert_eq!(a.venue_ts_field("publicTrade"), "T");
    assert_eq!(
        Market::Linear.ws_url(),
        "wss://stream.bybit.com/v5/public/linear"
    );
    assert_eq!(
        Market::Spot.ws_url(),
        "wss://stream.bybit.com/v5/public/spot"
    );
    assert_eq!(
        Market::Inverse.ws_url(),
        "wss://stream.bybit.com/v5/public/inverse"
    );
}
