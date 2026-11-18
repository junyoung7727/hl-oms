//! Topic routing. `route` reads only the topic and never fails.

use md_core::StreamKey;
use md_core::adapter::Route;
use serde_json::Value;

/// The channels this adapter parses, as they appear before the symbol.
pub(crate) const CHANNELS: [&str; 6] = [
    "orderbook.1",
    "orderbook.50",
    "orderbook.200",
    "orderbook.1000",
    "publicTrade",
    "tickers",
];

/// `orderbook.50.BTCUSDT` → `("orderbook.50", "BTCUSDT")`. The symbol must be
/// non-empty and hold no dot.
pub(crate) fn split_topic(topic: &str) -> Option<StreamKey> {
    CHANNELS.iter().find_map(|channel| {
        let symbol = topic.strip_prefix(channel)?.strip_prefix('.')?;
        (!symbol.is_empty() && !symbol.contains('.')).then(|| StreamKey::new(*channel, symbol))
    })
}

pub(crate) fn route(frame: &[u8]) -> Route {
    let topic = serde_json::from_slice::<Value>(frame)
        .ok()
        .and_then(|v| v.get("topic").and_then(Value::as_str).map(str::to_owned));
    match topic.as_deref().and_then(split_topic) {
        Some(key) => Route::Stream(key),
        None => Route::Control,
    }
}
