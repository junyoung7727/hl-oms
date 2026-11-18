//! Helpers for seal tests: a Bybit-like classifier, open files written by the
//! real stage-1 writer, a fixed clock, and readers of what the seal wrote.

use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use md_lake::fs::Fs;
use md_lake::qraw::{self, Record, Src};
use md_lake::seal::{Clock, Topic};
use md_lake::writer::{OpenWriter, WriterSettings};
use md_lake::zst::{self, Contexts};

pub const S: i64 = 1_000_000_000;
/// 2026-01-02 14:00:00 UTC.
pub const T14: i64 = 1_767_362_400_000_000_000;

/// A clock the test moves.
#[derive(Debug)]
pub struct FixedClock(pub AtomicI64);

impl FixedClock {
    pub fn at(ns: i64) -> Arc<FixedClock> {
        Arc::new(FixedClock(AtomicI64::new(ns)))
    }
    pub fn set(&self, ns: i64) {
        self.0.store(ns, Ordering::SeqCst);
    }
}

impl Clock for FixedClock {
    fn now_ns(&self) -> i64 {
        self.0.load(Ordering::SeqCst)
    }
}

/// Topic from `{"topic":"<channel>.<symbol>",...}`; `None` for anything else
/// (as a venue adapter's `route` would give `Control`).
pub fn classify(r: &Record) -> Option<Topic> {
    if r.src == Src::Note {
        return None;
    }
    let v: serde_json::Value = serde_json::from_slice(&r.payload).ok()?;
    let topic = v.get("topic")?.as_str()?;
    let (channel, symbol) = topic.rsplit_once('.')?;
    Some(Topic {
        market: "linear".into(),
        channel: channel.into(),
        symbol: symbol.into(),
    })
}

pub fn md(t: i64, conn_id: u32, frame_no: u64, topic: &str) -> Record {
    Record {
        recv_ns: t,
        src: Src::WebSocket,
        conn_id,
        frame_no,
        payload: format!("{{\"topic\":\"{topic}\",\"f\":{frame_no},\"c\":{conn_id}}}").into_bytes(),
    }
}

pub fn control(t: i64, conn_id: u32, frame_no: u64) -> Record {
    Record {
        recv_ns: t,
        src: Src::WebSocket,
        conn_id,
        frame_no,
        payload: br#"{"op":"pong"}"#.to_vec(),
    }
}

/// Writes `records` (all of one connection) as the open files of one
/// collector session; `crash` leaves the last file without a frame end.
pub fn write_open(
    fs: &Arc<dyn Fs>,
    collector: &str,
    session_ns: i64,
    conn_id: u32,
    records: &[Record],
    crash: bool,
) {
    let settings = WriterSettings::new("bybit", collector, "0.1.0", "test", session_ns);
    let mut w = OpenWriter::new(fs.clone(), settings, conn_id, &Contexts::default());
    for r in records {
        w.append(r).expect("append");
    }
    if crash {
        // A block flush, then the process dies: no frame end, lock released.
        if let Some(last) = records.last() {
            w.tick(last.recv_ns + 2 * S).expect("flush");
        }
        drop(w);
    } else {
        w.close().expect("close");
    }
}

pub fn read_all(fs: &dyn Fs, path: &str) -> Vec<u8> {
    let mut f = fs.open(path).expect("open");
    let mut bytes = vec![0; f.len().expect("len") as usize];
    let mut at = 0;
    while at < bytes.len() {
        at += f.read_at(at as u64, &mut bytes[at..]).expect("read");
    }
    bytes
}

/// Records and frame count of a day or session file.
pub fn day_records(fs: &dyn Fs, path: &str) -> (Vec<Record>, usize) {
    let bytes = read_all(fs, path);
    let scan = zst::scan(&bytes).expect("scan");
    assert!(scan.unfinished.is_none(), "{path} ends inside a frame");
    let segs = zst::decode_frames(&bytes, &scan).expect("decode");
    let refs: Vec<&[u8]> = segs.iter().map(Vec::as_slice).collect();
    (
        qraw::decode(&refs).expect("qraw").records,
        scan.frames.len(),
    )
}

pub fn day_path(channel: &str, symbol: &str, date: &str) -> String {
    format!(
        "crypto/l0/venue=bybit/market=linear/channel={channel}/date={date}/{symbol}.qraw.zst.partial"
    )
}

pub fn seal_log(fs: &dyn Fs, date: &str, hour: u8) -> Vec<serde_json::Value> {
    let path = format!("crypto/l0/_seal/venue=bybit/date={date}/hour={hour:02}.json");
    if !fs.exists(&path) {
        return Vec::new();
    }
    String::from_utf8(read_all(fs, &path))
        .expect("utf8")
        .lines()
        .map(|l| serde_json::from_str(l).expect("json line"))
        .collect()
}

pub fn open_files(fs: &dyn Fs, date: &str, hour: u8) -> Vec<String> {
    fs.list(&format!(
        "crypto/l0/_open/venue=bybit/date={date}/hour={hour:02}"
    ))
    .expect("list")
}
