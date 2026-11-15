//! Journal behaviour at its failure boundaries.
//!
//! These tests manipulate real files, but they simulate a crash by editing the
//! file afterwards. They show how recovery treats each on-disk shape; they do
//! not show which shapes a power loss can actually produce on a given device.

#![allow(clippy::unwrap_used)]

use oms_journal::*;
use proptest::prelude::*;
use std::fs;
use std::path::{Path, PathBuf};

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("oms-journal-{name}-{}", std::process::id()));
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
        writer: "test".into(),
    }
}

fn payload(seq: u64) -> Vec<u8> {
    format!("record-{seq}-{}", "x".repeat((seq % 7) as usize)).into_bytes()
}

/// Opens the journal and returns the writer and every record visited.
fn open(dir: &Path, o: &Options, after: u64) -> Result<(Writer, Recovery, Vec<(u64, Vec<u8>)>), Error> {
    let mut seen = Vec::new();
    let (w, r) = recover(dir, o, after, |seq, _schema, p| {
        seen.push((seq, p.to_vec()));
        Ok(())
    })?;
    Ok((w, r, seen))
}

fn write_n(dir: &Path, o: &Options, from: u64, to: u64) {
    let (mut w, _, _) = open(dir, o, 0).unwrap();
    for seq in from..=to {
        w.append(seq, &payload(seq)).unwrap();
        w.sync(seq).unwrap();
    }
}

fn only_segment(dir: &Path) -> PathBuf {
    let segs = list_segments(dir).unwrap();
    assert_eq!(segs.len(), 1);
    segs[0].1.clone()
}

#[test]
fn records_are_durable_only_after_sync() {
    let d = tmp("durable");
    {
        let (mut w, r, seen) = open(&d, &opts(), 0).unwrap();
        assert_eq!((r.last_seq, seen.len()), (0, 0));
        w.append(1, b"one").unwrap();
        w.append(2, b"two").unwrap();
        assert_eq!((w.last_seq(), w.durable_seq()), (2, 0));
        assert_eq!(w.sync(10).unwrap(), 2);
        w.append(3, b"three").unwrap();
        // Dropped without sync: record 3 was never handed to the file.
    }
    let (w, r, seen) = open(&d, &opts(), 0).unwrap();
    assert_eq!(r.last_seq, 2);
    assert_eq!(r.torn_tail, None);
    assert_eq!(seen, vec![(1, b"one".to_vec()), (2, b"two".to_vec())]);
    assert_eq!(w.durable_seq(), 2);
    fs::remove_dir_all(&d).unwrap();
}

#[test]
fn a_second_writer_is_refused() {
    let d = tmp("lock");
    let (_w, _, _) = open(&d, &opts(), 0).unwrap();
    assert!(matches!(open(&d, &opts(), 0), Err(Error::Locked(_))));
    drop(_w);
    assert!(open(&d, &opts(), 0).is_ok());
    fs::remove_dir_all(&d).unwrap();
}

#[test]
fn appends_must_be_consecutive() {
    let d = tmp("seq");
    let (mut w, _, _) = open(&d, &opts(), 0).unwrap();
    assert!(matches!(w.append(2, b"x"), Err(Error::Usage(_))));
    w.append(1, b"x").unwrap();
    assert!(matches!(w.append(1, b"x"), Err(Error::Usage(_))));
    fs::remove_dir_all(&d).unwrap();
}

/// Cutting the file anywhere inside the last record leaves exactly the
/// earlier records; the cut-off bytes are preserved; writing resumes.
#[test]
fn every_truncation_of_the_last_record_recovers_the_prefix() {
    let d = tmp("torn");
    write_n(&d, &opts(), 1, 5);
    let seg = only_segment(&d);
    let full = fs::read(&seg).unwrap();
    let last_len = FRAME_HEADER_LEN + payload(5).len();
    let start_of_last = full.len() - last_len;

    for cut in start_of_last..full.len() {
        fs::write(&seg, &full[..cut]).unwrap();
        let _ = fs::remove_dir_all(d.join("torn"));
        let (mut w, r, seen) = open(&d, &opts(), 0).unwrap();
        assert_eq!(r.last_seq, 4, "cut at {cut}");
        assert_eq!(seen.len(), 4);
        if cut == start_of_last {
            assert_eq!(r.torn_tail, None);
        } else {
            let t = r.torn_tail.expect("torn tail reported");
            assert_eq!(t.offset as usize, start_of_last);
            assert_eq!(fs::read(&t.saved_to).unwrap(), &full[start_of_last..cut]);
        }
        assert_eq!(fs::metadata(&seg).unwrap().len() as usize, start_of_last);
        // The journal continues with the sequence that was lost.
        w.append(5, b"again").unwrap();
        w.sync(1).unwrap();
        drop(w);
        let (_, r2, seen2) = open(&d, &opts(), 0).unwrap();
        assert_eq!((r2.last_seq, r2.torn_tail), (5, None));
        assert_eq!(seen2[4], (5, b"again".to_vec()));
    }
    fs::remove_dir_all(&d).unwrap();
}

#[test]
fn garbage_and_zero_tails_are_truncated() {
    for (name, tail) in [("zeros", vec![0u8; 300]), ("ones", vec![0xffu8; 300])] {
        let d = tmp(name);
        write_n(&d, &opts(), 1, 3);
        let seg = only_segment(&d);
        let mut bytes = fs::read(&seg).unwrap();
        let good = bytes.len();
        bytes.extend_from_slice(&tail);
        fs::write(&seg, &bytes).unwrap();
        let (_, r, seen) = open(&d, &opts(), 0).unwrap();
        assert_eq!((r.last_seq, seen.len()), (3, 3));
        assert_eq!(r.torn_tail.unwrap().bytes, 300);
        assert_eq!(fs::metadata(&seg).unwrap().len() as usize, good);
        fs::remove_dir_all(&d).unwrap();
    }
}

/// Damage with valid records after it is not an incomplete tail. Recovery
/// stops and changes nothing.
#[test]
fn damage_in_the_middle_is_refused_and_nothing_is_modified() {
    let d = tmp("mid");
    write_n(&d, &opts(), 1, 6);
    let seg = only_segment(&d);
    let full = fs::read(&seg).unwrap();
    // Flip one bit in the payload of record 3.
    let tail_len: usize = (4..=6).map(|s| FRAME_HEADER_LEN + payload(s).len()).sum();
    let pos = full.len() - tail_len - 2;
    let mut damaged = full.clone();
    damaged[pos] ^= 0x10;
    fs::write(&seg, &damaged).unwrap();

    match open(&d, &opts(), 0) {
        Err(Error::Corrupt { detail, .. }) => assert!(detail.contains("valid record 4 follows"), "{detail}"),
        other => panic!("expected corruption, got {:?}", other.map(|x| x.1)),
    }
    assert_eq!(fs::read(&seg).unwrap(), damaged, "the file must be left as found");
    assert!(!d.join("torn").exists());
    fs::remove_dir_all(&d).unwrap();
}

/// Known limit: damage confined to the very last record cannot be told apart
/// from an incomplete write, so it is treated as one.
#[test]
fn damage_in_the_last_record_is_treated_as_an_incomplete_tail() {
    let d = tmp("lastflip");
    write_n(&d, &opts(), 1, 4);
    let seg = only_segment(&d);
    let mut bytes = fs::read(&seg).unwrap();
    let n = bytes.len();
    bytes[n - 1] ^= 1;
    fs::write(&seg, &bytes).unwrap();
    let (_, r, _) = open(&d, &opts(), 0).unwrap();
    assert_eq!(r.last_seq, 3);
    assert!(r.torn_tail.is_some());
    fs::remove_dir_all(&d).unwrap();
}

#[test]
fn truncation_can_be_disabled() {
    let d = tmp("notrunc");
    write_n(&d, &opts(), 1, 2);
    let seg = only_segment(&d);
    let mut bytes = fs::read(&seg).unwrap();
    bytes.extend_from_slice(b"partial");
    fs::write(&seg, &bytes).unwrap();
    let mut o = opts();
    o.truncate_torn_tail = false;
    assert!(matches!(open(&d, &o, 0), Err(Error::Corrupt { .. })));
    assert_eq!(fs::read(&seg).unwrap(), bytes);
    fs::remove_dir_all(&d).unwrap();
}

#[test]
fn rotation_keeps_sequences_consecutive_across_segments() {
    let d = tmp("rotate");
    let mut o = opts();
    o.segment_max_bytes = 200;
    write_n(&d, &o, 1, 40);
    let segs = list_segments(&d).unwrap();
    assert!(segs.len() > 3, "expected several segments, got {}", segs.len());
    let (_, r, seen) = open(&d, &o, 0).unwrap();
    assert_eq!(r.last_seq, 40);
    assert_eq!(seen.iter().map(|(s, _)| *s).collect::<Vec<_>>(), (1..=40).collect::<Vec<_>>());

    // Recovery after a snapshot at 25 visits only later records and does not
    // need the older segments.
    let (_, r, seen) = open(&d, &o, 25).unwrap();
    assert_eq!(seen.first().map(|(s, _)| *s), Some(26));
    assert_eq!(r.records_visited, 15);
    let first_needed = segs.iter().rposition(|(f, _)| *f <= 26).unwrap();
    for (_, p) in &segs[..first_needed] {
        fs::remove_file(p).unwrap();
    }
    let (_, r, seen) = open(&d, &o, 25).unwrap();
    assert_eq!((r.last_seq, seen.len()), (40, 15));

    // Without the snapshot the same directory has a gap at the start.
    assert!(matches!(open(&d, &o, 0), Err(Error::Gap { .. })));
    fs::remove_dir_all(&d).unwrap();
}

#[test]
fn a_missing_middle_segment_is_a_gap() {
    let d = tmp("gap");
    let mut o = opts();
    o.segment_max_bytes = 200;
    write_n(&d, &o, 1, 40);
    let segs = list_segments(&d).unwrap();
    fs::remove_file(&segs[2].1).unwrap();
    assert!(matches!(open(&d, &o, 0), Err(Error::Gap { .. })));
    fs::remove_dir_all(&d).unwrap();
}

#[test]
fn an_incomplete_newest_segment_is_removed_and_preserved() {
    let d = tmp("newseg");
    let mut o = opts();
    o.segment_max_bytes = 200;
    write_n(&d, &o, 1, 10);
    let last = list_segments(&d).unwrap().last().unwrap().0;
    // A crash while creating the next segment: only part of its header exists.
    let partial = d.join(format!("journal-{:020}.oj", 11));
    assert!(last < 11);
    fs::write(&partial, b"OMSJ").unwrap();
    let (mut w, r, _) = open(&d, &o, 0).unwrap();
    assert_eq!(r.last_seq, 10);
    assert!(r.torn_tail.is_some());
    assert!(!partial.exists());
    w.append(11, b"next").unwrap();
    w.sync(1).unwrap();
    drop(w);
    assert_eq!(open(&d, &o, 0).unwrap().1.last_seq, 11);
    fs::remove_dir_all(&d).unwrap();
}

#[test]
fn a_failed_sync_is_final() {
    let d = tmp("failsync");
    let (mut w, _, _) = open(&d, &opts(), 0).unwrap();
    w.append(1, b"one").unwrap();
    w.sync(1).unwrap();
    w.append(2, b"two").unwrap();
    w.inject_sync_failure();
    assert!(matches!(w.sync(2), Err(Error::Io { .. })));
    // No retry is possible: the writer refuses everything from now on.
    assert!(matches!(w.sync(3), Err(Error::Failed(_))));
    assert!(matches!(w.append(3, b"three"), Err(Error::Failed(_))));
    assert_eq!(w.durable_seq(), 1);
    drop(w);
    let (_, r, seen) = open(&d, &opts(), 0).unwrap();
    assert_eq!((r.last_seq, seen.len()), (1, 1));
    fs::remove_dir_all(&d).unwrap();
}

#[test]
fn a_journal_of_another_instance_is_refused() {
    let d = tmp("instance");
    write_n(&d, &opts(), 1, 2);
    let mut o = opts();
    o.instance_id = 78;
    assert!(matches!(open(&d, &o, 0), Err(Error::Usage(_))));
    fs::remove_dir_all(&d).unwrap();
}

/// A build that writes a newer record schema reads the old segments and
/// starts a new segment for its own records; a build that cannot read a
/// segment's schema refuses to start.
#[test]
fn a_schema_change_starts_a_new_segment() {
    let d = tmp("schema");
    write_n(&d, &opts(), 1, 3);
    let mut newer = opts();
    newer.record_schema = 2;
    newer.readable_schemas = vec![1, 2];
    {
        let mut schemas = Vec::new();
        let (mut w, r) = recover(&d, &newer, 0, |seq, schema, _| {
            schemas.push((seq, schema));
            Ok(())
        })
        .unwrap();
        assert_eq!(r.last_seq, 3);
        assert_eq!(schemas, vec![(1, 1), (2, 1), (3, 1)]);
        w.append(4, b"v2").unwrap();
        w.sync(1).unwrap();
    }
    assert_eq!(list_segments(&d).unwrap().len(), 2);
    let mut schemas = Vec::new();
    recover(&d, &newer, 0, |seq, schema, _| {
        schemas.push((seq, schema));
        Ok(())
    })
    .unwrap();
    assert_eq!(schemas, vec![(1, 1), (2, 1), (3, 1), (4, 2)]);
    // The older build cannot read schema 2.
    assert!(matches!(open(&d, &opts(), 0), Err(Error::Usage(_))));
    fs::remove_dir_all(&d).unwrap();
}

#[test]
fn verify_all_reads_without_modifying() {
    let d = tmp("verify");
    write_n(&d, &opts(), 1, 5);
    let seg = only_segment(&d);
    let mut bytes = fs::read(&seg).unwrap();
    bytes.extend_from_slice(b"junk");
    fs::write(&seg, &bytes).unwrap();
    let mut n = 0;
    let r = verify_all(&d, |_, _, _| {
        n += 1;
        Ok(())
    })
    .unwrap();
    assert_eq!((r.last_seq, n), (5, 5));
    assert_eq!(r.torn_tail.unwrap().bytes, 4);
    assert_eq!(fs::read(&seg).unwrap(), bytes);
    fs::remove_dir_all(&d).unwrap();
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// For arbitrary record sizes, batch boundaries and cut points, recovery
    /// returns exactly the records whose frames lie wholly before the cut.
    #[test]
    fn recovery_returns_the_longest_valid_prefix(
        sizes in proptest::collection::vec(0usize..600, 1..40),
        batch in 1usize..6,
        cut_back in 0usize..2000,
        case in 0u64..1_000_000,
    ) {
        let d = tmp(&format!("prop-{case}"));
        let o = opts();
        let mut ends = Vec::new();
        {
            let (mut w, _, _) = open(&d, &o, 0).unwrap();
            for (i, n) in sizes.iter().enumerate() {
                let seq = i as u64 + 1;
                w.append(seq, &vec![(seq % 251) as u8; *n]).unwrap();
                if (i + 1) % batch == 0 {
                    w.sync(seq).unwrap();
                }
            }
            w.sync(0).unwrap();
        }
        let seg = only_segment(&d);
        let full = fs::read(&seg).unwrap();
        let mut off = full.len() - sizes.iter().map(|n| FRAME_HEADER_LEN + n).sum::<usize>();
        for n in &sizes {
            off += FRAME_HEADER_LEN + n;
            ends.push(off);
        }
        let header_len = full.len() - sizes.iter().map(|n| FRAME_HEADER_LEN + n).sum::<usize>();
        let cut = full.len().saturating_sub(cut_back).max(header_len);
        fs::write(&seg, &full[..cut]).unwrap();

        let expected = ends.iter().filter(|e| **e <= cut).count() as u64;
        let (_, r, seen) = open(&d, &o, 0).unwrap();
        prop_assert_eq!(r.last_seq, expected);
        prop_assert_eq!(seen.len() as u64, expected);
        for (seq, p) in &seen {
            prop_assert_eq!(p.len(), sizes[*seq as usize - 1]);
        }
        fs::remove_dir_all(&d).unwrap();
    }
}
