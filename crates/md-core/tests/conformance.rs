//! Every venue adapter must pass the same conformance checks (design §6.4).
//! This test runs the harness on a made-up venue format, and on two broken
//! adapters that the harness must reject.

use md_core::adapter::{
    ConnPlan, Instrument, Keepalive, MarketDataAdapter, ParseError, PlanError, RestRequest, Resync,
    Route,
};
use md_core::conformance::{Fixture, run};
use md_core::decimal::to_int;
use md_core::{BookUpdate, GapEvent, Level, Normalized, SeqRule, StreamKey};
use serde_json::Value;

/// Frames look like `{"topic":"book.BTC","type":"snapshot","u":1,"ts":5,"b":[["1.0","2"]],"a":[]}`.
#[derive(Default)]
struct Fake {
    drop_zero_levels: bool,
    ignore_sequence: bool,
}

impl MarketDataAdapter for Fake {
    fn venue(&self) -> &'static str {
        "fake"
    }
    fn plan(
        &self,
        symbols: &[String],
        channels: &[String],
        depth: u32,
    ) -> Result<Vec<ConnPlan>, PlanError> {
        let topics = symbols
            .iter()
            .flat_map(|s| {
                channels
                    .iter()
                    .map(move |c| StreamKey::new(c.clone(), s.clone()))
            })
            .collect();
        Ok(vec![ConnPlan {
            conn_index: 0,
            topics,
            depth,
        }])
    }
    fn subscribe_frames(&self, plan: &ConnPlan) -> Vec<String> {
        plan.topics
            .iter()
            .map(|k| format!("sub {}.{}", k.channel, k.symbol))
            .collect()
    }
    fn keepalive(&self) -> Keepalive {
        Keepalive {
            interval_ms: 20_000,
            frame: Some("ping".into()),
        }
    }
    fn route(&self, frame: &[u8]) -> Route {
        let topic = serde_json::from_slice::<Value>(frame)
            .ok()
            .and_then(|v| v["topic"].as_str().map(str::to_owned));
        match topic.as_deref().and_then(|t| t.split_once('.')) {
            Some((channel, symbol)) => Route::Stream(StreamKey::new(channel, symbol)),
            None => Route::Control,
        }
    }
    fn parse(
        &self,
        key: &StreamKey,
        frame: &[u8],
        recv_ns: i64,
    ) -> Result<Vec<Normalized>, ParseError> {
        let v: Value = serde_json::from_slice(frame).map_err(|e| ParseError::new(e.to_string()))?;
        let side = |name: &str| -> Result<Vec<Level>, ParseError> {
            let mut out = Vec::new();
            for pair in v[name]
                .as_array()
                .ok_or_else(|| ParseError::new("levels"))?
            {
                let px = to_int(pair[0].as_str().unwrap_or(""), -1)
                    .map_err(|e| ParseError::new(format!("{e:?}")))?;
                let qty = to_int(pair[1].as_str().unwrap_or(""), 0)
                    .map_err(|e| ParseError::new(format!("{e:?}")))?;
                if !(self.drop_zero_levels && qty == 0) {
                    out.push(Level { px, qty });
                }
            }
            Ok(out)
        };
        let update = BookUpdate {
            key: key.clone(),
            venue_ts_ns: v["ts"].as_i64().ok_or_else(|| ParseError::new("ts"))? * 1_000_000,
            recv_ns,
            update_id: if self.ignore_sequence {
                None
            } else {
                v["u"].as_i64()
            },
            prev_update_id: None,
            first_update_id: None,
            venue_seq: None,
            bids: side("b")?,
            asks: side("a")?,
            px_exp: -1,
            qty_exp: 0,
        };
        Ok(vec![match v["type"].as_str() {
            Some("snapshot") => Normalized::BookSnapshot(update),
            _ => Normalized::BookDelta(update),
        }])
    }
    fn seq_rule(&self, _key: &StreamKey) -> SeqRule {
        SeqRule::PlusOne
    }
    fn on_gap(&self, _key: &StreamKey, _gap: &GapEvent) -> Resync {
        Resync::Resubscribe
    }
    fn instruments_request(&self) -> RestRequest {
        RestRequest {
            method: "GET",
            path: "/instruments".into(),
            query: vec![],
        }
    }
    fn parse_instruments(
        &self,
        _body: &[u8],
        _recv_ns: i64,
    ) -> Result<Vec<Instrument>, ParseError> {
        Ok(vec![])
    }
    fn venue_ts_field(&self, _channel: &str) -> &'static str {
        "ts"
    }
}

fn fixture() -> Fixture {
    Fixture {
        key: StreamKey::new("book", "BTC"),
        snapshot: br#"{"topic":"book.BTC","type":"snapshot","u":10,"ts":5,"b":[["100.0","2"],["99.9","3"]],"a":[["100.1","4"]]}"#.to_vec(),
        deltas: vec![
            br#"{"topic":"book.BTC","type":"delta","u":11,"ts":6,"b":[["99.9","0"]],"a":[["100.2","1"]]}"#.to_vec(),
            br#"{"topic":"book.BTC","type":"delta","u":12,"ts":7,"b":[["100.0","5"]],"a":[]}"#.to_vec(),
        ],
        expected_bids: vec![Level { px: 1000, qty: 5 }],
        expected_asks: vec![Level { px: 1001, qty: 4 }, Level { px: 1002, qty: 1 }],
        deleted_px: 999,
        expected_rule: SeqRule::PlusOne,
        gap_delta: Some(br#"{"topic":"book.BTC","type":"delta","u":14,"ts":8,"b":[],"a":[]}"#.to_vec()),
        expected_resync: Resync::Resubscribe,
        recv_ns: 1_000,
        expected_venue_ts_ns: 5_000_000,
        garbage: vec![b"".to_vec(), vec![0xff, 0xfe], br#"{"op":"pong"}"#.to_vec(), b"ping".to_vec()],
    }
}

#[test]
fn fake_adapter_passes_the_conformance_harness() {
    assert_eq!(run(&Fake::default(), &fixture()), Vec::<String>::new());
}

#[test]
fn harness_fails_an_adapter_that_drops_zero_quantity_levels() {
    let failures = run(
        &Fake {
            drop_zero_levels: true,
            ..Fake::default()
        },
        &fixture(),
    );
    assert!(
        failures.iter().any(|f| f.contains("deleted level")),
        "{failures:?}"
    );
}

#[test]
fn harness_fails_an_adapter_that_ignores_the_sequence_field() {
    let failures = run(
        &Fake {
            ignore_sequence: true,
            ..Fake::default()
        },
        &fixture(),
    );
    assert!(failures.iter().any(|f| f.contains("gap")), "{failures:?}");
}

#[test]
fn route_never_fails_on_garbage_frames() {
    let fake = Fake::default();
    for frame in fixture().garbage {
        assert_eq!(fake.route(&frame), Route::Control);
    }
}
