//! Adversarial review tests for the journal crate.
//!
//! Each test asserts the behaviour the documents promise. A test that FAILS
//! demonstrates a defect; a test that passes records something the reviewer
//! tried to break and could not.

#![allow(clippy::unwrap_used)]

use oms_journal::*;
use std::fs;
use std::path::{Path, PathBuf};

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("oms-review-journal-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    d
}

fn opts() -> Options {
    Options {
        segment_max_bytes: 1 << 20,
        truncate_torn_tail: true,
        record_schema: 1,
        readable_schemas: vec![1],
        instance_id: 77,
        writer: "review".into(),
    }
}

fn open(dir: &Path, o: &Options, after: u64) -> Result<(Writer, Recovery, Vec<u64>), Error> {
    let mut seen = Vec::new();
    let (w, r) = recover(dir, o, after, |seq, _schema, _p| {
        seen.push(seq);
        Ok(())
    })?;
    Ok((w, r, seen))
}

fn write_n(dir: &Path, o: &Options, from: u64, to: u64) {
    let (mut w, _, _) = open(dir, o, from - 1).unwrap();
    for seq in from..=to {
        w.append(seq, format!("record-{seq}").as_bytes()).unwrap();
        w.sync(seq).unwrap();
    }
}

fn dir_listing(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    v.sort();
    v
}

/// recovery.md: "중간 손상, 빈틈, 다른 인스턴스, 읽을 수 없는 스키마 → 시작 거부. 파일을
/// 건드리지 않는다." A journal of another instance that happens to end in an
/// incomplete record must be refused without being modified.
#[test]
fn a_foreign_instance_journal_is_refused_without_being_modified() {
    let d = tmp("foreign");
    write_n(&d, &opts(), 1, 3);
    let seg = list_segments(&d).unwrap().pop().unwrap().1;
    let mut bytes = fs::read(&seg).unwrap();
    bytes.extend_from_slice(b"\x40\x00\x00\x00half-written record");
    fs::write(&seg, &bytes).unwrap();
    let before = dir_listing(&d);

    let mut other = opts();
    other.instance_id = 78;
    assert!(matches!(open(&d, &other, 0), Err(Error::Usage(_))), "must be refused");

    assert_eq!(
        fs::read(&seg).unwrap().len(),
        bytes.len(),
        "the refused journal was truncated before the instance check"
    );
    assert_eq!(dir_listing(&d), before, "the refused directory gained files");
    fs::remove_dir_all(&d).unwrap();
}

/// ADR-011 "버전 호환": a build that writes a newer record schema starts a new
/// segment. If the previous run ended right after a rotation, the newest
/// segment is header-only and already has the name the new segment needs.
#[test]
fn a_schema_change_right_after_a_rotation_can_still_write() {
    let d = tmp("schema-rot");
    let mut o = opts();
    o.segment_max_bytes = 200;
    let last = {
        let (mut w, _, _) = open(&d, &o, 0).unwrap();
        let mut seq = 0;
        // Stop at the first sync that rotated: the newest segment then holds
        // a header and no record. This is the state after any restart that
        // follows a rotation.
        while w.stats().rotations == 0 {
            seq += 1;
            w.append(seq, format!("record-{seq}").as_bytes()).unwrap();
            w.sync(seq).unwrap();
        }
        seq
    };
    let mut newer = o.clone();
    newer.record_schema = 2;
    newer.readable_schemas = vec![1, 2];
    let (mut w, r, _) = open(&d, &newer, 0).unwrap();
    assert_eq!(r.last_seq, last);
    w.append(last + 1, b"v2").unwrap();
    let result = w.sync(1);
    assert!(result.is_ok(), "the upgraded build cannot write its first record: {result:?}");
    fs::remove_dir_all(&d).unwrap();
}

/// A snapshot at 5 with every segment gone: records 6..10 existed and were
/// durable. Recovery must not present this as a healthy journal.
#[test]
fn a_snapshot_without_any_segment_is_not_accepted_silently() {
    let d = tmp("nosegs");
    write_n(&d, &opts(), 1, 10);
    for (_, p) in list_segments(&d).unwrap() {
        fs::remove_file(p).unwrap();
    }
    let result = open(&d, &opts(), 5);
    assert!(
        result.is_err(),
        "recovery accepted a data directory whose journal is missing entirely (last_seq {:?})",
        result.as_ref().ok().map(|(_, r, _)| r.last_seq)
    );
    fs::remove_dir_all(&d).unwrap();
}

/// The gap check that follows a snapshot newer than the journal runs after
/// the torn tail was already cut. Refusal should leave the files as found.
#[test]
fn a_snapshot_newer_than_the_journal_is_refused_without_modifying_it() {
    let d = tmp("snapnewer");
    write_n(&d, &opts(), 1, 4);
    let seg = list_segments(&d).unwrap().pop().unwrap().1;
    let mut bytes = fs::read(&seg).unwrap();
    bytes.extend_from_slice(&[0u8; 40]);
    fs::write(&seg, &bytes).unwrap();
    assert!(matches!(open(&d, &opts(), 9), Err(Error::Gap { .. })));
    assert_eq!(fs::read(&seg).unwrap().len(), bytes.len(), "refused, yet truncated");
    fs::remove_dir_all(&d).unwrap();
}

// ---- attempts that did not find a defect -----------------------------------

/// A header-only newest segment (the state right after a rotation) is a clean
/// journal: nothing is reported as torn, and writing continues in it.
#[test]
fn header_only_newest_segment_is_clean() {
    let d = tmp("hdronly");
    let mut o = opts();
    o.segment_max_bytes = 200;
    let last = {
        let (mut w, _, _) = open(&d, &o, 0).unwrap();
        let mut seq = 0;
        while w.stats().rotations == 0 {
            seq += 1;
            w.append(seq, format!("record-{seq}").as_bytes()).unwrap();
            w.sync(seq).unwrap();
        }
        seq
    };
    let n = list_segments(&d).unwrap().len();
    // With a snapshot exactly at the rotation boundary, too.
    for after in [0, last] {
        let (_, r, _) = open(&d, &o, after).unwrap();
        assert_eq!((r.last_seq, r.torn_tail.is_none()), (last, true));
    }
    let (mut w, _, _) = open(&d, &o, last).unwrap();
    w.append(last + 1, b"next").unwrap();
    w.sync(1).unwrap();
    drop(w);
    assert_eq!(list_segments(&d).unwrap().len(), n, "no extra segment");
    assert_eq!(open(&d, &o, 0).unwrap().1.last_seq, last + 1);
    fs::remove_dir_all(&d).unwrap();
}

/// A torn tail that happens to contain an old, valid frame (stale bytes with a
/// sequence that is not newer) is still a tail, and a valid frame with the
/// wrong sequence directly after the good prefix is refused, not truncated.
#[test]
fn stale_and_out_of_order_frames_are_classified_conservatively() {
    let d = tmp("stale");
    write_n(&d, &opts(), 1, 4);
    let seg = list_segments(&d).unwrap().pop().unwrap().1;
    let good = fs::read(&seg).unwrap();
    // The last frame is 16 + len("record-4") bytes.
    let frame4 = good[good.len() - (FRAME_HEADER_LEN + 8)..].to_vec();

    // Garbage, then a copy of frame 4 (not newer than the last good record).
    let mut bytes = good.clone();
    bytes.extend_from_slice(&[0xAB; 9]);
    bytes.extend_from_slice(&frame4);
    fs::write(&seg, &bytes).unwrap();
    let (_, r, _) = open(&d, &opts(), 0).unwrap();
    assert_eq!(r.last_seq, 4);
    assert!(r.torn_tail.is_some());

    // A duplicate of frame 4 directly after frame 4: valid checksum, wrong
    // sequence. Must be refused.
    let mut bytes = good.clone();
    bytes.extend_from_slice(&frame4);
    fs::write(&seg, &bytes).unwrap();
    assert!(matches!(open(&d, &opts(), 0), Err(Error::Corrupt { .. })));
    fs::remove_dir_all(&d).unwrap();
}

/// A crash while the segment after a rotation was being created leaves a
/// newest segment without a usable header. With a snapshot that covers every
/// earlier record, that file is the only segment recovery would look at; it
/// must be set aside and writing must continue in the segment before it.
#[test]
fn an_incomplete_newest_segment_after_a_snapshot_at_the_boundary_is_set_aside() {
    let d = tmp("hdrtorn");
    let mut o = opts();
    o.segment_max_bytes = 200;
    let last = {
        let (mut w, _, _) = open(&d, &o, 0).unwrap();
        let mut seq = 0;
        while w.stats().rotations == 0 {
            seq += 1;
            w.append(seq, format!("record-{seq}").as_bytes()).unwrap();
            w.sync(seq).unwrap();
        }
        seq
    };
    let newest = list_segments(&d).unwrap().pop().unwrap().1;
    let header = fs::read(&newest).unwrap();
    fs::write(&newest, &header[..5]).unwrap();

    let (mut w, r, seen) = open(&d, &o, last).unwrap();
    assert_eq!(r.last_seq, last);
    assert!(seen.is_empty(), "the snapshot covers every record");
    assert_eq!(r.torn_tail.as_ref().map(|t| t.bytes), Some(5));
    assert!(!newest.exists());
    w.append(last + 1, b"next").unwrap();
    w.sync(1).unwrap();
    drop(w);
    assert_eq!(open(&d, &o, 0).unwrap().1.last_seq, last + 1);
    fs::remove_dir_all(&d).unwrap();
}
