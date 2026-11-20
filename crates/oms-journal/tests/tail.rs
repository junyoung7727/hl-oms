//! The read-only follower (U14a): it never returns a record above the
//! durable sequence the caller gives, follows segment rotation, never takes
//! the lock, and stops on damage without moving.

#![allow(clippy::unwrap_used)]

use oms_journal::*;
use std::fs;
use std::path::{Path, PathBuf};

fn tmp(name: &str) -> PathBuf {
    let base = std::env::var("OMS_TEST_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir());
    let d = base.join(format!("oms-tail-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

fn opts(segment_max_bytes: u64) -> Options {
    Options {
        segment_max_bytes,
        truncate_torn_tail: true,
        record_schema: 2,
        readable_schemas: vec![1, 2],
        instance_id: 77,
        writer: "test".into(),
    }
}

fn payload(seq: u64) -> Vec<u8> {
    format!("record-{seq}-{}", "x".repeat((seq % 7) as usize)).into_bytes()
}

fn writer(dir: &Path, segment_max_bytes: u64) -> Writer {
    recover(dir, &opts(segment_max_bytes), 0, |_, _, _| Ok(()))
        .unwrap()
        .0
}

/// Appends `from..=to` and syncs them.
fn append(w: &mut Writer, from: u64, to: u64) {
    for seq in from..=to {
        w.append(seq, &payload(seq)).unwrap();
        w.sync(seq).unwrap();
    }
}

fn seqs(records: &[TailRecord]) -> Vec<u64> {
    records.iter().map(|r| r.seq).collect()
}

#[test]
fn the_tail_never_returns_a_record_above_the_durable_cap() {
    let dir = tmp("cap");
    let mut w = writer(&dir, 1 << 20);
    append(&mut w, 1, 10);
    let mut t = Tail::new(&dir, 0);
    // Ten records are on disk; the caller says five are durable.
    let got = t.read(5, 100).unwrap();
    assert_eq!(seqs(&got), vec![1, 2, 3, 4, 5]);
    assert_eq!(got[2].payload, payload(3));
    assert_eq!(got[0].record_schema, 2);
    assert!(t.read(5, 100).unwrap().is_empty());
    assert_eq!(t.next_seq(), 6);
    // max bounds one call.
    assert_eq!(seqs(&t.read(10, 2).unwrap()), vec![6, 7]);
    assert_eq!(seqs(&t.read(10, 100).unwrap()), vec![8, 9, 10]);
    // A cap beyond what is on disk returns what is there and waits.
    assert!(t.read(20, 100).unwrap().is_empty());
    append(&mut w, 11, 12);
    assert_eq!(seqs(&t.read(20, 100).unwrap()), vec![11, 12]);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn the_tail_follows_segment_rotation_and_starts_after_a_cursor() {
    let dir = tmp("rotation");
    let mut w = writer(&dir, 256);
    append(&mut w, 1, 60);
    assert!(
        list_segments(&dir).unwrap().len() >= 3,
        "small segments rotate"
    );
    let mut t = Tail::new(&dir, 0);
    let mut all = Vec::new();
    loop {
        let got = t.read(60, 7).unwrap();
        if got.is_empty() {
            break;
        }
        all.extend(got);
    }
    assert_eq!(seqs(&all), (1..=60).collect::<Vec<_>>());
    for r in &all {
        assert_eq!(r.payload, payload(r.seq));
    }
    // A follower restarted from a cursor in the middle of a segment.
    let mut t = Tail::new(&dir, 33);
    assert_eq!(
        seqs(&t.read(40, 100).unwrap()),
        (34..=40).collect::<Vec<_>>()
    );
    // Rotation while following.
    append(&mut w, 61, 90);
    let mut t2 = Tail::new(&dir, 60);
    let mut more = Vec::new();
    loop {
        let got = t2.read(90, 100).unwrap();
        if got.is_empty() {
            break;
        }
        more.extend(got);
    }
    assert_eq!(seqs(&more), (61..=90).collect::<Vec<_>>());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn the_tail_never_takes_the_lock() {
    let dir = tmp("lock");
    let mut w = writer(&dir, 1 << 20);
    append(&mut w, 1, 3);
    // The writer holds the lock; the follower still reads.
    let mut t = Tail::new(&dir, 0);
    assert_eq!(seqs(&t.read(3, 10).unwrap()), vec![1, 2, 3]);
    drop(w);
    // With the follower alive, a new writer can take the directory.
    let mut w = writer(&dir, 1 << 20);
    append(&mut w, 4, 4);
    assert_eq!(seqs(&t.read(4, 10).unwrap()), vec![4]);
    let _ = fs::remove_dir_all(&dir);
}

/// Flips one payload byte of record `seq` in the only segment.
fn damage(dir: &Path, seq: u64) {
    let (_, path) = list_segments(dir).unwrap().pop().unwrap();
    let mut bytes = fs::read(&path).unwrap();
    let needle = payload(seq);
    let at = bytes
        .windows(needle.len())
        .position(|w| w == needle.as_slice())
        .unwrap();
    bytes[at] ^= 0xff;
    fs::write(&path, bytes).unwrap();
}

#[test]
fn a_damaged_durable_frame_stops_the_tail_without_moving_it() {
    let dir = tmp("crc");
    let mut w = writer(&dir, 1 << 20);
    append(&mut w, 1, 6);
    drop(w);
    damage(&dir, 4);
    let mut t = Tail::new(&dir, 0);
    // Records before the damage come out; the damaged one is an error.
    assert_eq!(seqs(&t.read(6, 100).unwrap()), vec![1, 2, 3]);
    for _ in 0..2 {
        let err = t.read(6, 100).unwrap_err();
        assert!(matches!(err, Error::Corrupt { .. }), "{err}");
        assert_eq!(t.next_seq(), 4, "the position does not move");
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn peek_beyond_shows_valid_frames_past_the_cap_without_moving() {
    let dir = tmp("peek");
    let mut w = writer(&dir, 1 << 20);
    append(&mut w, 1, 8);
    drop(w);
    let mut t = Tail::new(&dir, 0);
    assert_eq!(seqs(&t.read(5, 100).unwrap()), vec![1, 2, 3, 4, 5]);
    assert_eq!(seqs(&t.peek_beyond(100).unwrap()), vec![6, 7, 8]);
    assert_eq!(t.next_seq(), 6, "peeking does not move");
    // A torn last frame ends the peek; it is not an error past the cap.
    let (_, path) = list_segments(&dir).unwrap().pop().unwrap();
    let len = fs::metadata(&path).unwrap().len();
    let f = fs::OpenOptions::new().write(true).open(&path).unwrap();
    f.set_len(len - 3).unwrap();
    assert_eq!(seqs(&t.peek_beyond(100).unwrap()), vec![6, 7]);
    // Below the cap the same torn frame is an error.
    assert!(t.read(8, 100).is_ok_and(|r| seqs(&r) == vec![6, 7]));
    assert!(matches!(t.read(8, 100), Err(Error::Corrupt { .. })));
    let _ = fs::remove_dir_all(&dir);
}

/// A frame as the journal writes it (frame.rs), with any sequence number.
fn raw_frame(seq: u64, payload: &[u8]) -> Vec<u8> {
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

/// Review 2026-11-20. A valid frame with the wrong sequence is damage: the
/// records before it come out first, then the error; a peek stops there.
#[test]
fn a_frame_with_the_wrong_sequence_comes_after_the_good_records() {
    let dir = tmp("seq");
    let mut w = writer(&dir, 1 << 20);
    append(&mut w, 1, 3);
    drop(w);
    let (_, path) = list_segments(&dir).unwrap().pop().unwrap();
    let mut bytes = fs::read(&path).unwrap();
    bytes.extend(raw_frame(9, b"stray"));
    fs::write(&path, bytes).unwrap();
    let mut t = Tail::new(&dir, 0);
    assert_eq!(seqs(&t.peek_beyond(100).unwrap()), vec![1, 2, 3]);
    assert_eq!(seqs(&t.read(10, 100).unwrap()), vec![1, 2, 3]);
    assert!(matches!(t.read(10, 100), Err(Error::Corrupt { .. })));
    assert_eq!(t.next_seq(), 4);
    assert!(t.peek_beyond(100).unwrap().is_empty());
    let _ = fs::remove_dir_all(&dir);
}

/// Review 2026-11-20. Damage before the cursor is an error, not a wait.
#[test]
fn damage_before_the_cursor_is_an_error() {
    let dir = tmp("before-cursor");
    let mut w = writer(&dir, 1 << 20);
    append(&mut w, 1, 10);
    drop(w);
    damage(&dir, 3);
    let mut t = Tail::new(&dir, 5);
    assert!(matches!(t.read(10, 100), Err(Error::Corrupt { .. })));
    // A frame being written at the end, before the cursor, is a wait.
    let dir2 = tmp("before-cursor-torn");
    let mut w = writer(&dir2, 1 << 20);
    append(&mut w, 1, 3);
    drop(w);
    let (_, path) = list_segments(&dir2).unwrap().pop().unwrap();
    let len = fs::metadata(&path).unwrap().len();
    fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .unwrap()
        .set_len(len - 2)
        .unwrap();
    let mut t = Tail::new(&dir2, 4);
    assert!(t.read(10, 100).unwrap().is_empty());
    let _ = fs::remove_dir_all(&dir);
    let _ = fs::remove_dir_all(&dir2);
}

fn writer_with_schema(dir: &Path, schema: u16) -> Writer {
    let mut o = opts(1 << 20);
    o.record_schema = schema;
    recover(dir, &o, 0, |_, _, _| Ok(())).unwrap().0
}

/// Review 2026-11-20. A new record schema opens a new segment; the tail
/// moves to it and reports each record's schema.
#[test]
fn the_tail_follows_a_schema_change_to_a_new_segment() {
    let dir = tmp("schema");
    let mut w = writer_with_schema(&dir, 1);
    append(&mut w, 1, 3);
    drop(w);
    let mut t = Tail::new(&dir, 0);
    assert_eq!(seqs(&t.read(3, 100).unwrap()), vec![1, 2, 3]);
    let mut w = writer_with_schema(&dir, 2);
    append(&mut w, 4, 6);
    assert_eq!(list_segments(&dir).unwrap().len(), 2);
    let got = t.read(6, 100).unwrap();
    assert_eq!(seqs(&got), vec![4, 5, 6]);
    assert!(got.iter().all(|r| r.record_schema == 2));
    let all = Tail::new(&dir, 0).read(6, 100).unwrap();
    let schemas: Vec<u16> = all.iter().map(|r| r.record_schema).collect();
    assert_eq!(schemas, vec![1, 1, 1, 2, 2, 2]);
    let _ = fs::remove_dir_all(&dir);
}

/// Review 2026-11-20. A segment that holds only its header is rewritten
/// with the new schema's header (C-JOURNAL); a tail waiting on it reads the
/// new header.
#[test]
fn the_tail_reads_a_header_only_segment_rewritten_with_a_new_schema() {
    let dir = tmp("header-rewrite");
    let mut w = writer_with_schema(&dir, 1);
    append(&mut w, 1, 3);
    drop(w);
    // A header-only segment of schema 1 that starts at 4, as rotation
    // leaves it.
    let (_, first) = list_segments(&dir).unwrap().pop().unwrap();
    let bytes = fs::read(&first).unwrap();
    let header_len = 12 + u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize + 4;
    let json: serde_json::Value = serde_json::from_slice(&bytes[12..header_len - 4]).unwrap();
    let mut h = json.clone();
    h["first_seq"] = 4.into();
    let hj = serde_json::to_vec(&h).unwrap();
    let mut header = b"OMSJRNL\x01".to_vec();
    header.extend_from_slice(&(hj.len() as u32).to_le_bytes());
    header.extend_from_slice(&hj);
    header.extend_from_slice(&crc32fast::hash(&hj).to_le_bytes());
    fs::write(dir.join("journal-00000000000000000004.oj"), header).unwrap();
    let mut t = Tail::new(&dir, 3);
    assert!(t.read(3, 100).unwrap().is_empty());
    // A writer of schema 2 rewrites that header and appends.
    let mut w = writer_with_schema(&dir, 2);
    append(&mut w, 4, 5);
    assert_eq!(
        list_segments(&dir).unwrap().len(),
        2,
        "the header-only segment was reused"
    );
    let got = t.read(5, 100).unwrap();
    assert_eq!(seqs(&got), vec![4, 5]);
    assert!(got.iter().all(|r| r.record_schema == 2));
    let _ = fs::remove_dir_all(&dir);
}
