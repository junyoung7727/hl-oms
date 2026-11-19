//! A sealed lake built from the golden L0 segments of the contract (spec
//! Testing Decisions): each segment is one zstd frame of its sealed file,
//! with sidecars and manifest lines (`sessions` from `session_runs.json`).
//! One synthetic instruments response is added for 2026-01-01, the date
//! before, which has no instruments file in the fixtures.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use md_lake::paths::L0Path;
use md_lake::qraw::{self, Header, Record, Src};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::golden_dir;

/// The `recv_ns` of the synthetic 2026-01-01 instruments response: after the
/// golden collector session started (22:59:58) and before its first order
/// book record (23:00:00).
pub const EXTRA_INSTRUMENTS_NS: i64 = 1_767_308_398_500_000_000;
pub const EXTRA_INSTRUMENTS_ID: &str =
    "venue=bybit/market=linear/channel=instruments/date=2026-01-01/_all";

/// The segments of a golden L0 file, in name order.
pub fn golden_segments(l0_file_id: &str) -> Vec<Vec<u8>> {
    let dir = golden_dir().join("l0").join(l0_file_id);
    let mut names: Vec<PathBuf> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .map(|e| e.expect("entry").path())
        .filter(|p| p.extension().is_some_and(|x| x == "qraw"))
        .collect();
    names.sort();
    names
        .iter()
        .map(|p| fs::read(p).expect("segment"))
        .collect()
}

/// Every golden L0 `l0_file_id`.
pub fn golden_file_ids() -> Vec<String> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) {
        let mut has_segment = false;
        for e in fs::read_dir(dir).expect("read_dir") {
            let p = e.expect("entry").path();
            if p.is_dir() {
                walk(root, &p, out);
            } else if p.extension().is_some_and(|x| x == "qraw") {
                has_segment = true;
            }
        }
        if has_segment {
            let rel = dir.strip_prefix(root).expect("under root");
            let parts: Vec<_> = rel
                .components()
                .map(|c| c.as_os_str().to_string_lossy())
                .collect();
            out.push(parts.join("/"));
        }
    }
    let root = golden_dir().join("l0");
    let mut out = Vec::new();
    walk(&root, &root, &mut out);
    out.sort();
    out
}

fn date_of(l0_file_id: &str) -> String {
    let at = l0_file_id.find("date=").expect("date") + 5;
    l0_file_id[at..at + 10].to_string()
}

/// A sealed file of `segments` (each one zstd frame) at its place in the
/// lake, with its sidecar; returns its manifest line.
pub fn write_sealed(lake: &Path, l0_file_id: &str, segments: &[Vec<u8>], sessions: Value) -> Value {
    let rel = format!("crypto/l0/{l0_file_id}.qraw.zst");
    let path = lake.join(&rel);
    fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
    let mut bytes = Vec::new();
    for s in segments {
        bytes.extend(zstd::encode_all(s.as_slice(), 9).expect("zstd"));
    }
    fs::write(&path, &bytes).expect("write");
    let sha = hex::encode(Sha256::digest(&bytes));
    let name = path
        .file_name()
        .expect("name")
        .to_string_lossy()
        .to_string();
    fs::write(
        lake.join(format!("{rel}.sha256")),
        format!("{sha}  {name}\n"),
    )
    .expect("sidecar");
    let refs: Vec<&[u8]> = segments.iter().map(Vec::as_slice).collect();
    let decoded = qraw::decode(&refs).expect("golden segments decode");
    let layout = match L0Path::parse(&rel).expect("lake path") {
        L0Path::Session { .. } => "session",
        _ => "by_symbol",
    };
    json!({
        "l0_file_id": l0_file_id,
        "path": rel,
        "sha256": sha,
        "bytes": bytes.len(),
        "records": decoded.records.len(),
        "first_recv_ns": decoded.records.first().map(|r| r.recv_ns),
        "last_recv_ns": decoded.records.last().map(|r| r.recv_ns),
        "sealed_at": 1_767_999_000_000_000_000i64,
        "contract_ver": 1,
        "layout": layout,
        "sessions": if layout == "session" { Value::Null } else { sessions },
    })
}

/// Appends manifest lines, grouped by date.
pub fn write_manifest(lake: &Path, venue: &str, lines: &[Value]) {
    let mut by_date: BTreeMap<String, Vec<&Value>> = BTreeMap::new();
    for l in lines {
        by_date
            .entry(date_of(l["l0_file_id"].as_str().expect("id")))
            .or_default()
            .push(l);
    }
    for (date, lines) in by_date {
        let dir = lake.join(format!("crypto/l0/_manifest/venue={venue}"));
        fs::create_dir_all(&dir).expect("mkdir");
        let mut text = String::new();
        for l in lines {
            text.push_str(&l.to_string());
            text.push('\n');
        }
        let path = dir.join(format!("date={date}.jsonl"));
        let mut old = fs::read_to_string(&path).unwrap_or_default();
        old.push_str(&text);
        fs::write(path, old).expect("manifest");
    }
}

/// The golden lake: every golden L0 file sealed, plus the synthetic
/// 2026-01-01 instruments response. Returns the manifest lines written.
pub fn golden_lake(lake: &Path) -> Vec<Value> {
    let runs: Value = serde_json::from_slice(
        &fs::read(golden_dir().join("l0/session_runs.json")).expect("session_runs"),
    )
    .expect("json");
    let mut lines = Vec::new();
    for id in golden_file_ids() {
        let sessions = runs.get(&id).cloned().unwrap_or(Value::Null);
        lines.push(write_sealed(lake, &id, &golden_segments(&id), sessions));
    }
    // The instruments body of the fixture date, received on the date before.
    let body = qraw::decode(&[golden_segments(
        "venue=bybit/market=linear/channel=instruments/date=2026-01-02/_all",
    )[0]
    .as_slice()])
    .expect("instruments")
    .records[0]
        .payload
        .clone();
    let header = Header::topic(
        "bybit",
        "2026-01-01",
        "md1",
        "0.1.0",
        "test",
        "linear",
        "instruments",
        "_all",
    );
    let mut seg = qraw::encode_header(&header);
    qraw::encode_record(
        &Record {
            recv_ns: EXTRA_INSTRUMENTS_NS,
            src: Src::Rest,
            conn_id: 0,
            frame_no: 0,
            payload: body,
        },
        &mut seg,
    )
    .expect("record");
    lines.push(write_sealed(
        lake,
        EXTRA_INSTRUMENTS_ID,
        &[seg],
        json!([{"collector": "md1", "session_ns": 1_767_308_398_000_000_000i64, "first_rec_no": 0, "records": 1}]),
    ));
    write_manifest(lake, "bybit", &lines);
    lines
}
