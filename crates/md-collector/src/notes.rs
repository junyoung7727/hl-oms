//! Note records of the collector (lake contract 2.3, `l0/note.schema.json`).
//! Payload keys in the order md-lake writes them (golden notes, U4 F9).

use md_lake::qraw::{Record, Src};
use md_lake::seal::Topic;
use serde_json::Value;

pub struct Note<'a> {
    pub note: &'a str,
    pub conn_id: Option<u32>,
    pub topics: Option<&'a [Topic]>,
    pub start_recv_ns: i64,
    pub end_recv_ns: Option<i64>,
    pub detail: Value,
}

pub fn payload(writer: &str, n: &Note<'_>) -> Vec<u8> {
    let conn = n.conn_id.map_or("null".to_string(), |c| c.to_string());
    let end = n.end_recv_ns.map_or("null".to_string(), |e| e.to_string());
    let topics = match n.topics {
        None => "null".to_string(),
        Some(ts) => {
            let items: Vec<String> = ts
                .iter()
                .map(|t| {
                    format!(
                        "{{\"market\":{},\"channel\":{},\"symbol\":{}}}",
                        Value::from(t.market.as_str()),
                        Value::from(t.channel.as_str()),
                        Value::from(t.symbol.as_str())
                    )
                })
                .collect();
            format!("[{}]", items.join(","))
        }
    };
    format!(
        "{{\"note\":{},\"writer\":{},\"conn_id\":{conn},\"topics\":{topics},\"start_recv_ns\":{},\"end_recv_ns\":{end},\"detail\":{}}}",
        Value::from(n.note),
        Value::from(writer),
        n.start_recv_ns,
        n.detail
    )
    .into_bytes()
}

/// The note as an L0 record written at `now_ns`.
pub fn record(writer: &str, n: &Note<'_>, now_ns: i64) -> Record {
    Record {
        recv_ns: now_ns,
        src: Src::Note,
        conn_id: n.conn_id.unwrap_or(0),
        frame_no: 0,
        payload: payload(writer, n),
    }
}
