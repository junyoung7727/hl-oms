//! The watcher's own journal reader (C-WATCH part 1) against journals
//! written by the test writer of `common/`, which follows ADR-011 and
//! C-TAIL, not `oms-journal`.

#![allow(clippy::unwrap_used)]

mod common;

use common::*;
use serde_json::json;
use std::path::Path;
use watcher::journal::{JournalError, MAX_RECORDS, Record, Tail};

fn damaged_at(r: Result<Vec<Record>, JournalError>, seq: u64) -> String {
    match r {
        Err(JournalError::Damaged { seq: s, why }) if s == seq => why,
        other => panic!("expected damage at {seq}, got {other:?}"),
    }
}

fn flip(path: &Path, at: usize, mask: u8) {
    let mut b = std::fs::read(path).unwrap();
    b[at] ^= mask;
    std::fs::write(path, b).unwrap();
}

/// Byte offset of frame `seq` in a segment of filler records written by
/// `TestJournal::fill` (every frame there has the same length).
fn frame_offset(path: &Path, first_seq: u64, seq: u64) -> usize {
    let b = std::fs::read(path).unwrap();
    let mut off = 12 + u32::from_le_bytes([b[8], b[9], b[10], b[11]]) as usize + 4;
    for _ in first_seq..seq {
        off += 16 + u32::from_le_bytes([b[off], b[off + 1], b[off + 2], b[off + 3]]) as usize;
    }
    off
}

#[test]
fn frames_and_headers_follow_adr_011() {
    // One segment written byte by byte from ADR-011 §7, with CRC-32 values
    // computed by an independent tool:
    // python3 -c "import zlib,struct; print(hex(zlib.crc32(H)), hex(zlib.crc32(struct.pack('<Q',1)+P)))"
    // -> 0x4b0926f2 0x925731cf
    let h = br#"{"format":1,"record_schema":2,"instance_id":7,"first_seq":1,"created_at_us":0,"writer":"t"}"#;
    let p = br#"{"seq":1,"at":5,"input":{},"events":[{"booted":{"session":1}}]}"#;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"OMSJRNL\x01");
    bytes.extend_from_slice(&91u32.to_le_bytes());
    bytes.extend_from_slice(h);
    bytes.extend_from_slice(&0x4b09_26f2u32.to_le_bytes());
    bytes.extend_from_slice(&63u32.to_le_bytes());
    bytes.extend_from_slice(&0x9257_31cfu32.to_le_bytes());
    bytes.extend_from_slice(&1u64.to_le_bytes());
    bytes.extend_from_slice(p);
    assert_eq!((h.len(), p.len()), (91, 63));
    // The test writer produces the same bytes.
    let mut mine = header_bytes(h);
    mine.extend_from_slice(&frame_bytes(1, p));
    assert_eq!(mine, bytes);

    let dir = test_dir("adr011");
    std::fs::write(segment_path(&dir, 1), &bytes).unwrap();
    let mut t = Tail::open(&dir);
    let recs = t.read(1, 10).unwrap();
    assert_eq!(
        recs,
        vec![Record {
            seq: 1,
            at: 5,
            record_schema: 2,
            events: vec![("booted".to_string(), json!({"session": 1}))],
        }]
    );
    assert_eq!(t.instance_id(), Some(7));
    assert_eq!(t.last_seq(), 1);

    // Header refusals: each is damage at record 1 (the segment is retried).
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("bad magic", {
            let mut b = bytes.clone();
            b[7] = 0x02;
            b
        }),
        ("header longer than 64 KiB", {
            let mut b = bytes.clone();
            b[8..12].copy_from_slice(&(64 * 1024 + 1u32).to_le_bytes());
            b
        }),
        ("header checksum", {
            let mut b = bytes.clone();
            b[12 + 91] ^= 1;
            b
        }),
        ("header JSON", header_bytes(b"{not json")),
        (
            "header without first_seq",
            header_bytes(br#"{"format":1,"record_schema":2,"instance_id":7}"#),
        ),
        ("short header", bytes[..20].to_vec()),
        // Complete, with a valid checksum: only the 64 KiB bound refuses it.
        ("header JSON above 64 KiB", {
            let big = String::from_utf8(header_json(1, 2, 7))
                .unwrap()
                .replace("watcher-test", &"w".repeat(64 * 1024));
            header_bytes(big.as_bytes())
        }),
        (
            "header first_seq is not the file's",
            header_bytes(&header_json(2, 2, 7)),
        ),
    ];
    for (label, b) in cases {
        let d = test_dir("adr011-header");
        std::fs::write(segment_path(&d, 1), &b).unwrap();
        let why = damaged_at(Tail::open(&d).read(1, 10), 1);
        assert!(!why.is_empty(), "{label}");
    }
    let d = test_dir("adr011-format");
    let json = String::from_utf8(header_json(1, 2, 7))
        .unwrap()
        .replace("\"format\":1", "\"format\":2");
    std::fs::write(segment_path(&d, 1), header_bytes(json.as_bytes())).unwrap();
    assert_eq!(
        Tail::open(&d).read(1, 10),
        Err(JournalError::Format {
            segment: 1,
            format: 2
        })
    );

    // Frame refusals, after one good record.
    let frame_cases: Vec<(&str, Vec<u8>)> = vec![
        // A complete record with a valid checksum: only the 16 MiB bound
        // refuses it.
        ("complete frame above 16 MiB", {
            let pad = "p".repeat(16 * 1024 * 1024);
            let big = format!(r#"{{"seq":2,"at":6,"input":{{"pad":"{pad}"}},"events":[]}}"#);
            frame_bytes(2, big.as_bytes())
        }),
        ("length above 16 MiB", {
            let mut f = frame_bytes(2, &payload(2, 6, &[filler()]));
            f[0..4].copy_from_slice(&(16 * 1024 * 1024 + 1u32).to_le_bytes());
            f
        }),
        ("payload not JSON", frame_bytes(2, b"{\"seq\":2,")),
        (
            "payload seq differs from the frame's",
            frame_bytes(2, &payload(3, 6, &[filler()])),
        ),
        (
            "record time goes back",
            frame_bytes(2, &payload(2, 4, &[filler()])),
        ),
        (
            "no events",
            frame_bytes(2, br#"{"seq":2,"at":6,"input":{}}"#),
        ),
        (
            "event of two keys",
            frame_bytes(
                2,
                br#"{"seq":2,"at":6,"input":{},"events":[{"booted":{},"purged":{}}]}"#,
            ),
        ),
        (
            "event that is not an object",
            frame_bytes(2, br#"{"seq":2,"at":6,"input":{},"events":["booted"]}"#),
        ),
    ];
    for (label, f) in frame_cases {
        let d = test_dir("adr011-frame");
        let mut b = bytes.clone();
        b.extend_from_slice(&f);
        std::fs::write(segment_path(&d, 1), &b).unwrap();
        let mut t = Tail::open(&d);
        assert_eq!(seqs(&t.read(2, 10).unwrap()), vec![1], "{label}");
        let why = damaged_at(t.read(2, 10), 2);
        assert!(!why.is_empty(), "{label}");
    }
}

#[test]
fn schema_1_and_2_are_read() {
    let mut j = TestJournal::new("schemas");
    j.segment(1);
    j.fill(2, 1_000);
    j.segment(2);
    j.fill(2, 5_000);
    let mut t = Tail::open(&j.dir);
    let recs = t.read(4, 10).unwrap();
    assert_eq!(
        recs.iter()
            .map(|r| (r.seq, r.record_schema))
            .collect::<Vec<_>>(),
        vec![(1, 1), (2, 1), (3, 2), (4, 2)]
    );
    for schema in [0, 3] {
        let mut j = TestJournal::new("schemas-other");
        j.segment(schema);
        j.fill(1, 1_000);
        assert_eq!(
            Tail::open(&j.dir).read(1, 10),
            Err(JournalError::Schema { segment: 1, schema })
        );
    }
}

#[test]
fn the_reader_never_returns_a_record_above_the_durable_cap() {
    let mut j = TestJournal::new("cap");
    j.segment(2);
    j.fill(10, 1_000);
    let mut t = Tail::open(&j.dir);
    assert_eq!(t.read(0, 100).unwrap(), vec![]);
    assert_eq!(seqs(&t.read(5, 100).unwrap()), vec![1, 2, 3, 4, 5]);
    assert_eq!(t.read(5, 100).unwrap(), vec![]);
    assert_eq!(seqs(&t.read(7, 100).unwrap()), vec![6, 7]);
    assert_eq!(t.last_seq(), 7);
    // Beyond the cap the OMS may still be writing: a half frame and then
    // garbage after record 10 are not looked at while the cap is 10.
    let half = frame_bytes(11, &payload(11, 99_000, &[filler()]));
    let mut b = std::fs::read(j.last_segment()).unwrap();
    b.extend_from_slice(&half[..half.len() / 2]);
    std::fs::write(j.last_segment(), &b).unwrap();
    assert_eq!(seqs(&t.read(10, 100).unwrap()), vec![8, 9, 10]);
    assert_eq!(t.read(10, 100).unwrap(), vec![]);
    // A fresh reader with a low cap stops there too.
    assert_eq!(
        seqs(&Tail::open(&j.dir).read(3, 100).unwrap()),
        vec![1, 2, 3]
    );
    // Once the cap covers the half frame, it is damage.
    damaged_at(t.read(11, 100), 11);
}

#[test]
fn the_reader_follows_segment_rotation_and_a_schema_change() {
    let mut j = TestJournal::new("rotation");
    j.segment(1);
    j.fill(3, 1_000);
    j.segment(1);
    j.fill(3, 10_000);
    j.segment(2);
    j.fill(3, 20_000);
    // In one call.
    let recs = Tail::open(&j.dir).read(9, 100).unwrap();
    assert_eq!(seqs(&recs), (1..=9).collect::<Vec<_>>());
    assert_eq!(
        recs.iter().map(|r| r.record_schema).collect::<Vec<_>>(),
        vec![1, 1, 1, 1, 1, 1, 2, 2, 2]
    );
    // In calls that end at, before and after each boundary.
    let mut t = Tail::open(&j.dir);
    let mut got = Vec::new();
    for cap in [2, 3, 4, 4, 6, 8, 9, 9] {
        got.extend(seqs(&t.read(cap, 100).unwrap()));
    }
    assert_eq!(got, (1..=9).collect::<Vec<_>>());
    // And by `max_records`.
    let mut t = Tail::open(&j.dir);
    let mut got = Vec::new();
    for _ in 0..5 {
        got.extend(seqs(&t.read(9, 2).unwrap()));
    }
    assert_eq!(got, (1..=9).collect::<Vec<_>>());
}

#[test]
fn a_header_only_segment_rewritten_with_a_new_schema_is_read_again() {
    let mut j = TestJournal::new("header-only");
    j.segment(1);
    j.fill(3, 1_000);
    let b = j.segment(1);
    let mut t = Tail::open(&j.dir);
    assert_eq!(seqs(&t.read(3, 100).unwrap()), vec![1, 2, 3]);
    // A cap that names record 4 while its segment holds only a header.
    damaged_at(t.read(4, 100), 4);
    // The OMS rewrites the header-only segment with a new schema (and a
    // header of another length) and writes record 4 into it.
    let json = String::from_utf8(header_json(4, 2, 7))
        .unwrap()
        .replace("watcher-test", "watcher-test-upgraded");
    std::fs::write(&b, header_bytes(json.as_bytes())).unwrap();
    j.append(9_000, &[filler()]);
    let recs = t.read(4, 100).unwrap();
    assert_eq!(
        recs.iter()
            .map(|r| (r.seq, r.record_schema))
            .collect::<Vec<_>>(),
        vec![(4, 2)]
    );
}

/// Damage planted into a test journal.
type Plant = Box<dyn Fn(&TestJournal)>;

#[test]
fn a_damaged_frame_at_or_below_the_cap_stops_without_moving() {
    // Each plant damages record 4 of 1..=5 (or its segment); the good
    // records before it come first, then the same error at every call, and
    // once the files are mended the reader goes on from record 4.
    let plants: Vec<(&str, Plant)> = vec![
        (
            "short frame",
            Box::new(|j: &TestJournal| {
                let p = j.last_segment();
                let at = frame_offset(&p, 1, 4);
                let b = std::fs::read(&p).unwrap();
                std::fs::write(&p, &b[..at + 20]).unwrap();
            }),
        ),
        (
            "short frame header",
            Box::new(|j: &TestJournal| {
                let p = j.last_segment();
                let at = frame_offset(&p, 1, 4);
                let b = std::fs::read(&p).unwrap();
                std::fs::write(&p, &b[..at + 7]).unwrap();
            }),
        ),
        (
            "bit flip in the checksum field",
            Box::new(|j: &TestJournal| {
                let p = j.last_segment();
                flip(&p, frame_offset(&p, 1, 4) + 5, 0x10);
            }),
        ),
        (
            "bit flip in a digit of the payload (still JSON)",
            Box::new(|j: &TestJournal| {
                let p = j.last_segment();
                let at = frame_offset(&p, 1, 4);
                let b = std::fs::read(&p).unwrap();
                // `"free_bytes":1` -> `"free_bytes":0`
                let needle = b"\"free_bytes\":1";
                let i = b[at..]
                    .windows(needle.len())
                    .position(|w| w == needle)
                    .unwrap()
                    + at
                    + needle.len()
                    - 1;
                flip(&p, i, 0x01);
            }),
        ),
        (
            "bit flip in the sequence field",
            Box::new(|j: &TestJournal| {
                let p = j.last_segment();
                flip(&p, frame_offset(&p, 1, 4) + 8, 0x02);
            }),
        ),
        (
            "wrong frame sequence while the payload says 4",
            Box::new(|j: &TestJournal| {
                let p = j.last_segment();
                let at = frame_offset(&p, 1, 4);
                let mut b = std::fs::read(&p).unwrap();
                b.truncate(at);
                b.extend_from_slice(&frame_bytes(5, &payload(4, 4_000, &[filler()])));
                std::fs::write(&p, &b).unwrap();
            }),
        ),
        (
            "wrong sequence with a valid checksum",
            Box::new(|j: &TestJournal| {
                let p = j.last_segment();
                let at = frame_offset(&p, 1, 4);
                let mut b = std::fs::read(&p).unwrap();
                b.truncate(at);
                b.extend_from_slice(&frame_bytes(5, &payload(5, 4_000, &[filler()])));
                std::fs::write(&p, &b).unwrap();
            }),
        ),
    ];
    for (label, plant) in &plants {
        let mut j = TestJournal::new("damage");
        j.segment(2);
        j.fill(5, 1_000);
        let good = std::fs::read(j.last_segment()).unwrap();
        plant(&j);
        let mut t = Tail::open(&j.dir);
        assert_eq!(seqs(&t.read(5, 100).unwrap()), vec![1, 2, 3], "{label}");
        let first = t.read(5, 100);
        damaged_at(first.clone(), 4);
        for _ in 0..3 {
            assert_eq!(
                t.read(5, 100),
                first,
                "{label}: the same error until the files change"
            );
        }
        assert_eq!(t.last_seq(), 3, "{label}");
        std::fs::write(j.last_segment(), &good).unwrap();
        assert_eq!(seqs(&t.read(5, 100).unwrap()), vec![4, 5], "{label}");
    }

    // A missing segment.
    let mut j = TestJournal::new("damage-missing");
    j.segment(2);
    j.fill(3, 1_000);
    let second = j.segment(2);
    j.fill(2, 10_000);
    let kept = std::fs::read(&second).unwrap();
    std::fs::remove_file(&second).unwrap();
    let mut t = Tail::open(&j.dir);
    assert_eq!(seqs(&t.read(5, 100).unwrap()), vec![1, 2, 3]);
    let first = t.read(5, 100);
    damaged_at(first.clone(), 4);
    assert_eq!(t.read(5, 100), first);
    std::fs::write(&second, &kept).unwrap();
    assert_eq!(seqs(&t.read(5, 100).unwrap()), vec![4, 5]);

    // A segment cut below the read position.
    let mut j = TestJournal::new("damage-shrunk");
    j.segment(2);
    j.fill(5, 1_000);
    let mut t = Tail::open(&j.dir);
    assert_eq!(seqs(&t.read(3, 100).unwrap()), vec![1, 2, 3]);
    let p = j.last_segment();
    let at = frame_offset(&p, 1, 3);
    let b = std::fs::read(&p).unwrap();
    std::fs::write(&p, &b[..at]).unwrap();
    let why = damaged_at(t.read(5, 100), 4);
    assert!(why.contains("shorter than the read position"), "{why}");
}

#[test]
fn an_unknown_event_stops_the_reader() {
    let mut j = TestJournal::new("unknown");
    j.segment(2);
    j.fill(2, 1_000);
    j.append(5_000, &[filler(), ev("made_up_event", json!({"x": 1}))]);
    j.fill(2, 6_000);
    let good_three = {
        let d = test_dir("unknown-good");
        let mut k = TestJournal::at(&d);
        k.segment(2);
        k.fill(5, 1_000);
        let b = std::fs::read(k.last_segment()).unwrap();
        std::fs::remove_dir_all(&d).unwrap();
        b
    };
    let mut t = Tail::open(&j.dir);
    assert_eq!(seqs(&t.read(5, 100).unwrap()), vec![1, 2]);
    let e = JournalError::UnknownEvent {
        seq: 3,
        name: "made_up_event".into(),
    };
    assert_eq!(t.read(5, 100), Err(e.clone()));
    assert!(!e.is_retried());
    // Mending the file does not help this reader: it is latched until a
    // restart. A new reader (a restart) reads the mended journal.
    std::fs::write(j.last_segment(), &good_three).unwrap();
    assert_eq!(t.read(5, 100), Err(e.clone()));
    assert_eq!(t.latched(), Some(&e));
    assert_eq!(
        seqs(&Tail::open(&j.dir).read(5, 100).unwrap()),
        vec![1, 2, 3, 4, 5]
    );
}

#[test]
fn io_damage_and_no_start_are_retried_and_the_other_errors_latch() {
    // Retried: Io (the directory is missing), Damaged (a short frame),
    // NoStart (the first segment starts at 5). Each heals once the files
    // are right, without a new reader.
    let j = TestJournal::new("io");
    std::fs::remove_dir_all(&j.dir).unwrap();
    let mut t = Tail::open(&j.dir);
    let e = t.read(2, 100).unwrap_err();
    assert_eq!(e, JournalError::Io(std::io::ErrorKind::NotFound));
    assert!(e.is_retried());
    assert_eq!(t.read(2, 100), Err(e));
    let mut j = TestJournal::new("io");
    j.segment(2);
    j.fill(2, 1_000);
    assert_eq!(seqs(&t.read(2, 100).unwrap()), vec![1, 2]);
    assert_eq!(t.latched(), None);

    let mut j = TestJournal::new("retried-damage");
    j.segment(2);
    j.fill(2, 1_000);
    let good = std::fs::read(j.last_segment()).unwrap();
    std::fs::write(j.last_segment(), &good[..good.len() - 3]).unwrap();
    let mut t = Tail::open(&j.dir);
    assert_eq!(seqs(&t.read(2, 100).unwrap()), vec![1]);
    let e = t.read(2, 100).unwrap_err();
    assert!(
        e.is_retried() && matches!(e, JournalError::Damaged { seq: 2, .. }),
        "{e:?}"
    );
    std::fs::write(j.last_segment(), &good).unwrap();
    assert_eq!(seqs(&t.read(2, 100).unwrap()), vec![2]);

    let mut j = TestJournal::new("nostart");
    j.next_seq = 5;
    j.segment(2);
    j.fill(2, 1_000);
    let mut t = Tail::open(&j.dir);
    assert_eq!(t.read(6, 100), Err(JournalError::NoStart));
    assert!(JournalError::NoStart.is_retried());
    assert_eq!(t.read(6, 100), Err(JournalError::NoStart));
    std::fs::remove_file(j.last_segment()).unwrap();
    let mut k = TestJournal::at(&j.dir);
    k.segment(2);
    k.fill(6, 1_000);
    assert_eq!(seqs(&t.read(6, 100).unwrap()), (1..=6).collect::<Vec<_>>());

    // Latched: UnknownEvent (own test), Schema, Instance, Format. Mending
    // the files does not make the same reader read again.
    let mut j = TestJournal::new("latch-schema");
    j.segment(3);
    j.fill(2, 1_000);
    let mut t = Tail::open(&j.dir);
    let e = JournalError::Schema {
        segment: 1,
        schema: 3,
    };
    assert_eq!(t.read(2, 100), Err(e.clone()));
    assert!(!e.is_retried());
    let mut k = TestJournal::at(&j.dir);
    k.segment(2);
    k.fill(2, 1_000);
    assert_eq!(t.read(2, 100), Err(e));
    assert_eq!(seqs(&Tail::open(&j.dir).read(2, 100).unwrap()), vec![1, 2]);

    let mut j = TestJournal::new("latch-instance");
    j.segment(2);
    j.fill(2, 1_000);
    let other = j.segment_of(2, 8);
    j.fill(2, 10_000);
    let mut t = Tail::open(&j.dir);
    assert_eq!(seqs(&t.read(4, 100).unwrap()), vec![1, 2]);
    let e = JournalError::Instance {
        expected: 7,
        found: 8,
    };
    assert_eq!(t.read(4, 100), Err(e.clone()));
    assert!(!e.is_retried());
    std::fs::remove_file(&other).unwrap();
    j.next_seq = 3;
    j.segments.pop();
    j.segment(2);
    j.fill(2, 10_000);
    assert_eq!(t.read(4, 100), Err(e));

    let d = test_dir("latch-format");
    let json = String::from_utf8(header_json(1, 2, 7))
        .unwrap()
        .replace("\"format\":1", "\"format\":9");
    std::fs::write(segment_path(&d, 1), header_bytes(json.as_bytes())).unwrap();
    let mut t = Tail::open(&d);
    let e = JournalError::Format {
        segment: 1,
        format: 9,
    };
    assert_eq!(t.read(1, 100), Err(e.clone()));
    assert!(!e.is_retried());
    let mut k = TestJournal::at(&d);
    k.segment(2);
    k.fill(1, 1_000);
    assert_eq!(t.read(1, 100), Err(e));
}

#[test]
fn a_large_segment_is_read_frame_by_frame_in_bounded_calls() {
    // 25,000 records in one segment, with damage in the last one: the
    // first calls return MAX_RECORDS each without seeing the damage, which
    // they would if they loaded or checked the whole segment.
    let n = 25_000u64;
    let mut j = TestJournal::new("large");
    j.segment(2);
    let mut bytes = std::fs::read(j.last_segment()).unwrap();
    for seq in 1..=n {
        bytes.extend_from_slice(&frame_bytes(seq, &payload(seq, 1_000 + seq, &[filler()])));
    }
    std::fs::write(j.last_segment(), &bytes).unwrap();
    let p = j.last_segment();
    flip(&p, frame_offset(&p, 1, n) + 4, 0x01);
    assert_eq!(MAX_RECORDS, 10_000);
    let mut t = Tail::open(&j.dir);
    let a = t.read(n, MAX_RECORDS).unwrap();
    assert_eq!((a.len(), a[0].seq, a[a.len() - 1].seq), (10_000, 1, 10_000));
    let b = t.read(n, MAX_RECORDS).unwrap();
    assert_eq!(
        (b.len(), b[0].seq, b[b.len() - 1].seq),
        (10_000, 10_001, 20_000)
    );
    let c = t.read(n, MAX_RECORDS).unwrap();
    assert_eq!(
        (c.len(), c[0].seq, c[c.len() - 1].seq),
        (4_999, 20_001, 24_999)
    );
    damaged_at(t.read(n, MAX_RECORDS), n);
}
