//! The watcher's test journal writer, built from ADR-011 §7 and C-TAIL
//! "기록 형식" (not from `oms-journal`): segment headers, frames and record
//! payloads as bytes, so that a test can also write what the OMS never
//! writes (damage, a foreign instance, another schema).

#![allow(dead_code, clippy::unwrap_used)]

pub mod kill_rig;
pub mod rig;
pub mod sock;

use serde_json::{Value, json};
use std::path::{Path, PathBuf};

pub const MAGIC: &[u8; 8] = b"OMSJRNL\x01";

/// A fresh directory under `OMS_TEST_DIR` (the system temp dir if unset).
pub fn test_dir(name: &str) -> PathBuf {
    let base = std::env::var("OMS_TEST_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir());
    let d = base.join(format!("watcher-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

pub fn segment_path(dir: &Path, first_seq: u64) -> PathBuf {
    dir.join(format!("journal-{first_seq:020}.oj"))
}

pub fn header_json(first_seq: u64, schema: u64, instance: u64) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "format": 1,
        "record_schema": schema,
        "instance_id": instance,
        "first_seq": first_seq,
        "created_at_us": 0,
        "writer": "watcher-test",
    }))
    .unwrap()
}

/// `magic | u32 LE length | JSON | u32 LE CRC-32 of the JSON`.
pub fn header_bytes(json: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&(json.len() as u32).to_le_bytes());
    out.extend_from_slice(json);
    out.extend_from_slice(&crc32fast::hash(json).to_le_bytes());
    out
}

/// `u32 LE payload length | u32 LE CRC-32 over sequence + payload | u64 LE sequence | payload`.
pub fn frame_bytes(seq: u64, payload: &[u8]) -> Vec<u8> {
    let mut h = crc32fast::Hasher::new();
    h.update(&seq.to_le_bytes());
    h.update(payload);
    let mut out = Vec::new();
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(&h.finalize().to_le_bytes());
    out.extend_from_slice(&seq.to_le_bytes());
    out.extend_from_slice(payload);
    out
}

pub fn payload(seq: u64, at: u64, events: &[Value]) -> Vec<u8> {
    serde_json::to_vec(&json!({"seq": seq, "at": at, "input": {"tick": {}}, "events": events}))
        .unwrap()
}

/// One event `{"<name>": fields}`.
pub fn ev(name: &str, fields: Value) -> Value {
    json!({ name: fields })
}

/// A known event without meaning for the sets, for filler records.
pub fn filler() -> Value {
    ev(
        "storage_low_changed",
        json!({"low": false, "free_bytes": 1}),
    )
}

/// A journal in a test directory: segments in order, records appended to
/// the last segment.
pub struct TestJournal {
    pub dir: PathBuf,
    pub instance: u64,
    pub next_seq: u64,
    /// First sequence of every segment, in order.
    pub segments: Vec<u64>,
    /// The `at` (µs) of the last record appended.
    pub last_at: u64,
}

impl TestJournal {
    pub fn new(name: &str) -> TestJournal {
        TestJournal {
            dir: test_dir(name),
            instance: 7,
            next_seq: 1,
            segments: Vec::new(),
            last_at: 0,
        }
    }

    /// A writer for an existing directory (to write it anew from record 1).
    pub fn at(dir: &Path) -> TestJournal {
        TestJournal {
            dir: dir.to_path_buf(),
            instance: 7,
            next_seq: 1,
            segments: Vec::new(),
            last_at: 0,
        }
    }

    /// Opens a new segment at the next sequence.
    pub fn segment(&mut self, schema: u64) -> PathBuf {
        self.segment_of(schema, self.instance)
    }

    pub fn segment_of(&mut self, schema: u64, instance: u64) -> PathBuf {
        let p = segment_path(&self.dir, self.next_seq);
        std::fs::write(
            &p,
            header_bytes(&header_json(self.next_seq, schema, instance)),
        )
        .unwrap();
        self.segments.push(self.next_seq);
        p
    }

    pub fn last_segment(&self) -> PathBuf {
        segment_path(&self.dir, *self.segments.last().unwrap())
    }

    /// Appends one frame with any payload at any sequence to the last segment.
    pub fn append_frame(&mut self, seq: u64, payload: &[u8]) {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(self.last_segment())
            .unwrap();
        f.write_all(&frame_bytes(seq, payload)).unwrap();
    }

    /// Appends the record `next_seq` with time `at`; returns its sequence.
    pub fn append(&mut self, at: u64, events: &[Value]) -> u64 {
        let seq = self.next_seq;
        self.append_frame(seq, &payload(seq, at, events));
        self.next_seq += 1;
        self.last_at = self.last_at.max(at);
        seq
    }

    /// `n` filler records, one millisecond apart from `at0` (µs).
    pub fn fill(&mut self, n: u64, at0: u64) {
        for i in 0..n {
            self.append(at0 + i * 1000, &[filler()]);
        }
    }
}

/// The sequences of the records.
pub fn seqs(recs: &[watcher::journal::Record]) -> Vec<u64> {
    recs.iter().map(|r| r.seq).collect()
}
