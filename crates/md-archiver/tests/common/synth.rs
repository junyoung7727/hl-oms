//! Synthetic sealed lakes for scenario tests: Bybit v5 frames built from the
//! documented message shapes (no venue bytes), one zstd frame per hour.

use std::collections::BTreeMap;
use std::path::Path;

use md_lake::qraw::{self, Header, Record, Src};
use md_lake::time::{HOUR_NS, date, hour_start};
use serde_json::{Value, json};

use super::lake::{golden_segments, write_manifest, write_sealed};

pub const COLLECTOR: &str = "md1";

/// The golden instruments body (BTCUSDT, tick 0.10, lot 0.001).
pub fn instruments_body() -> Vec<u8> {
    let segs =
        golden_segments("venue=bybit/market=linear/channel=instruments/date=2026-01-02/_all");
    qraw::decode(&[segs[0].as_slice()])
        .expect("instruments")
        .records[0]
        .payload
        .clone()
}

fn ms(ns: i64) -> i64 {
    ns / 1_000_000
}

pub fn levels(l: &[(i64, i64)]) -> Value {
    // Prices in units of 0.1, quantities in units of 0.001.
    Value::Array(
        l.iter()
            .map(|(p, q)| {
                json!([
                    format!("{}.{}", p / 10, p % 10),
                    format!("{}.{:03}", q / 1000, q % 1000)
                ])
            })
            .collect(),
    )
}

pub fn book_frame(kind: &str, t: i64, u: i64, bids: &[(i64, i64)], asks: &[(i64, i64)]) -> Vec<u8> {
    json!({
        "topic": "orderbook.50.BTCUSDT",
        "type": kind,
        "ts": ms(t) - 3,
        "data": {"s": "BTCUSDT", "b": levels(bids), "a": levels(asks), "u": u, "seq": 1000 + u},
        "cts": ms(t) - 4,
    })
    .to_string()
    .into_bytes()
}

pub fn trade_frame(t: i64, ids: &[&str]) -> Vec<u8> {
    let data: Vec<Value> = ids
        .iter()
        .map(|id| {
            json!({"T": ms(t) - 2, "s": "BTCUSDT", "S": "Buy", "v": "0.001", "p": "42000.0",
                   "L": "PlusTick", "i": id, "BT": false, "seq": 7})
        })
        .collect();
    json!({"topic": "publicTrade.BTCUSDT", "type": "snapshot", "ts": ms(t), "data": data})
        .to_string()
        .into_bytes()
}

pub fn ws(t: i64, conn_id: u32, frame_no: u64, payload: Vec<u8>) -> Record {
    Record {
        recv_ns: t,
        src: Src::WebSocket,
        conn_id,
        frame_no,
        payload,
    }
}

pub fn note(t: i64, body: Value) -> Record {
    Record {
        recv_ns: t,
        src: Src::Note,
        conn_id: 0,
        frame_no: 0,
        payload: body.to_string().into_bytes(),
    }
}

/// A synthetic lake: add records per file, then `write`.
#[derive(Default)]
pub struct Synth {
    /// topic file id → session runs (session_ns, records) in order.
    topics: BTreeMap<String, Vec<(i64, Vec<Record>)>>,
    sessions: BTreeMap<String, (String, i64, Vec<Record>)>,
}

impl Synth {
    /// Records of one collector session for `channel` of `BTCUSDT` on the
    /// date of the first record. Runs must be added in session order.
    pub fn topic(&mut self, channel: &str, session_ns: i64, records: Vec<Record>) {
        let d = date(records[0].recv_ns);
        let symbol = if channel == "instruments" {
            "_all"
        } else {
            "BTCUSDT"
        };
        let id = format!("venue=bybit/market=linear/channel={channel}/date={d}/{symbol}");
        self.topics
            .entry(id)
            .or_default()
            .push((session_ns, records));
    }

    pub fn instruments(&mut self, session_ns: i64, recv_ns: i64) {
        self.instruments_body(session_ns, recv_ns, instruments_body());
    }

    pub fn instruments_body(&mut self, session_ns: i64, recv_ns: i64, body: Vec<u8>) {
        self.topic(
            "instruments",
            session_ns,
            vec![Record {
                recv_ns,
                src: Src::Rest,
                conn_id: 0,
                frame_no: 0,
                payload: body,
            }],
        );
    }

    /// A session file of `writer` started at `start_ns`, on `d`.
    pub fn session(&mut self, d: &str, writer: &str, start_ns: i64, records: Vec<Record>) {
        let id = format!("venue=bybit/_session/date={d}/{writer}-{start_ns}");
        self.sessions
            .insert(id, (writer.to_string(), start_ns, records));
    }

    pub fn write(&self, lake: &Path) -> Vec<Value> {
        let mut lines = Vec::new();
        for (id, runs) in &self.topics {
            let parts: Vec<&str> = id.split('/').collect();
            let channel = parts[2].trim_start_matches("channel=");
            let d = parts[3].trim_start_matches("date=");
            let symbol = parts[4];
            // One segment per hour; inside an hour the runs follow each other.
            let mut by_hour: BTreeMap<i64, Vec<(i64, Vec<&Record>)>> = BTreeMap::new();
            for (s, recs) in runs {
                for r in recs {
                    let h = hour_start(r.recv_ns);
                    let e = by_hour.entry(h).or_default();
                    match e.last_mut() {
                        Some((ls, v)) if *ls == *s => v.push(r),
                        _ => e.push((*s, vec![r])),
                    }
                }
            }
            let mut segs = Vec::new();
            let mut sessions: Vec<Value> = Vec::new();
            let mut rec_no = 0u64;
            for (_, hruns) in by_hour {
                let mut seg = if segs.is_empty() {
                    qraw::encode_header(&Header::topic(
                        "bybit", d, COLLECTOR, "0.1.0", "test", "linear", channel, symbol,
                    ))
                } else {
                    Vec::new()
                };
                for (s, recs) in hruns {
                    for r in &recs {
                        qraw::encode_record(r, &mut seg).expect("record");
                    }
                    let n = recs.len() as u64;
                    match sessions.last_mut() {
                        Some(last) if last["session_ns"] == s => {
                            last["records"] = json!(last["records"].as_u64().expect("n") + n);
                        }
                        _ => sessions.push(json!({"collector": COLLECTOR, "session_ns": s,
                                                  "first_rec_no": rec_no, "records": n})),
                    }
                    rec_no += n;
                }
                segs.push(seg);
            }
            lines.push(write_sealed(lake, id, &segs, Value::Array(sessions)));
        }
        for (id, (writer, start_ns, recs)) in &self.sessions {
            let d = id
                .split('/')
                .nth(2)
                .expect("date")
                .trim_start_matches("date=");
            let mut seg = qraw::encode_header(&Header::session(
                "bybit", d, writer, "0.1.0", "test", *start_ns,
            ));
            for r in recs {
                qraw::encode_record(r, &mut seg).expect("record");
            }
            lines.push(write_sealed(lake, id, &[seg], Value::Null));
        }
        write_manifest(lake, "bybit", &lines);
        lines
    }
}

pub const MIN_NS: i64 = 60_000_000_000;
pub const SEC_NS: i64 = 1_000_000_000;
pub const H_NS: i64 = HOUR_NS;
