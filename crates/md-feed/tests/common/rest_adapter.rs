//! A test-only adapter for a venue that starts with deltas and syncs from a
//! REST snapshot (the Binance diff-depth pattern), in a made-up JSON shape:
//! WS `{"t":"d","s":SYM,"U":first,"u":last,"pu":prev,"b":[[px,qty]],"a":[]}`,
//! REST `{"lastUpdateId":S,"b":[[px,qty]],"a":[]}`. Integers only.

use md_core::adapter::{
    ConnPlan, Instrument, Keepalive, MarketDataAdapter, ParseError, PlanError, RestRequest, Resync,
    Route,
};
use md_core::{BookUpdate, GapEvent, Level, Normalized, SeqRule, StreamKey};
use serde_json::Value;

pub struct RestVenue;

pub const CHANNEL: &str = "depth";

fn levels(v: &Value, side: &str) -> Vec<Level> {
    v[side]
        .as_array()
        .map(|rows| {
            rows.iter()
                .filter_map(|r| Some(Level { px: r[0].as_i64()?, qty: r[1].as_i64()? }))
                .collect()
        })
        .unwrap_or_default()
}

fn update(key: &StreamKey, v: &Value, recv_ns: i64) -> BookUpdate {
    BookUpdate {
        key: key.clone(),
        venue_ts_ns: v["E"].as_i64().unwrap_or(0),
        recv_ns,
        update_id: v["u"].as_i64().or_else(|| v["lastUpdateId"].as_i64()),
        prev_update_id: v["pu"].as_i64(),
        first_update_id: v["U"].as_i64(),
        venue_seq: None,
        bids: levels(v, "b"),
        asks: levels(v, "a"),
        px_exp: 0,
        qty_exp: 0,
    }
}

impl MarketDataAdapter for RestVenue {
    fn venue(&self) -> &'static str {
        "restvenue"
    }
    fn plan(&self, symbols: &[String], _c: &[String], depth: u32) -> Result<Vec<ConnPlan>, PlanError> {
        Ok(vec![ConnPlan {
            conn_index: 0,
            topics: symbols.iter().map(|s| StreamKey::new(CHANNEL, s.clone())).collect(),
            depth,
        }])
    }
    fn subscribe_frames(&self, plan: &ConnPlan) -> Vec<String> {
        plan.topics.iter().map(|k| format!(r#"{{"sub":"{}"}}"#, k.symbol)).collect()
    }
    fn keepalive(&self) -> Keepalive {
        Keepalive { interval_ms: 20_000, frame: None }
    }
    fn route(&self, frame: &[u8]) -> Route {
        match serde_json::from_slice::<Value>(frame) {
            Ok(v) if v["t"] == "d" => match v["s"].as_str() {
                Some(s) => Route::Stream(StreamKey::new(CHANNEL, s)),
                None => Route::Control,
            },
            _ => Route::Control,
        }
    }
    fn parse(&self, key: &StreamKey, frame: &[u8], recv_ns: i64) -> Result<Vec<Normalized>, ParseError> {
        let v: Value = serde_json::from_slice(frame).map_err(|e| ParseError::new(e.to_string()))?;
        Ok(vec![Normalized::BookDelta(update(key, &v, recv_ns))])
    }
    fn seq_rule(&self, _key: &StreamKey) -> SeqRule {
        SeqRule::PrevEquals
    }
    fn on_gap(&self, _key: &StreamKey, _gap: &GapEvent) -> Resync {
        Resync::RestSnapshotThenReplay
    }
    fn instruments_request(&self) -> RestRequest {
        RestRequest { method: "GET", path: "/instruments".into(), query: vec![] }
    }
    fn parse_instruments(&self, _b: &[u8], _r: i64) -> Result<Vec<Instrument>, ParseError> {
        Ok(vec![])
    }
    fn venue_ts_field(&self, _channel: &str) -> &'static str {
        "E"
    }
    fn snapshot_request(&self, key: &StreamKey, depth: u32) -> Option<RestRequest> {
        Some(RestRequest {
            method: "GET",
            path: "/depth".into(),
            query: vec![("symbol".into(), key.symbol.clone()), ("limit".into(), depth.to_string())],
        })
    }
    fn parse_snapshot(&self, key: &StreamKey, body: &[u8], recv_ns: i64) -> Result<BookUpdate, ParseError> {
        let v: Value = serde_json::from_slice(body).map_err(|e| ParseError::new(e.to_string()))?;
        Ok(update(key, &v, recv_ns))
    }
}

/// A WS delta frame.
pub fn d(first: i64, last: i64, prev: i64, bids: &str) -> String {
    format!(r#"{{"t":"d","s":"BTC","E":{last},"U":{first},"u":{last},"pu":{prev},"b":[{bids}],"a":[]}}"#)
}

/// A REST snapshot body.
pub fn snapshot(s: i64, bids: &str) -> String {
    format!(r#"{{"lastUpdateId":{s},"b":[{bids}],"a":[[200,1]]}}"#)
}
