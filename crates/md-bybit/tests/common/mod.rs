//! Synthetic instruments and frames in Bybit's documented shapes. No captured
//! venue data (the repository is public; Bybit API terms 6.7).
#![allow(dead_code)]

use md_bybit::{BybitAdapter, Market};
use md_core::adapter::Instrument;

pub fn instrument(symbol: &str, tick: &str, lot: &str, px_exp: i8, qty_exp: i8) -> Instrument {
    Instrument {
        symbol: symbol.into(),
        status: "Trading".into(),
        tick_size: tick.into(),
        lot_size: lot.into(),
        px_exp,
        qty_exp,
        contract_type: Some("LinearPerpetual".into()),
        settle_coin: Some("USDT".into()),
        venue_ts_ns: None,
    }
}

/// BTCUSDT with tick 0.10 and step 0.001, as in the golden data.
pub fn linear() -> BybitAdapter {
    BybitAdapter::new(
        Market::Linear,
        &[instrument("BTCUSDT", "0.10", "0.001", -1, -3)],
    )
}

pub fn book(kind: &str, u: i64, ts: i64, bids: &str, asks: &str) -> Vec<u8> {
    format!(
        r#"{{"topic":"orderbook.50.BTCUSDT","type":"{kind}","ts":{ts},"data":{{"s":"BTCUSDT","b":[{bids}],"a":[{asks}],"u":{u},"seq":{seq}}},"cts":{cts}}}"#,
        seq = 9000 + u,
        cts = ts - 2
    )
    .into_bytes()
}
