//! A day file is a concatenation of zstd frames; an open file of a crashed
//! collector ends inside a frame. Reading either wrong loses or invents records.

mod common;

use md_lake::qraw::{self, Header, Record, Src};
use md_lake::zst::{self, Contexts, FrameEncoder};

fn rec(i: u64) -> Record {
    Record {
        recv_ns: 1_000 + i as i64,
        src: Src::WebSocket,
        conn_id: 1,
        frame_no: i,
        payload: format!(
            "{{\"topic\":\"t\",\"n\":{i},\"pad\":\"{}\"}}",
            "x".repeat((i % 97) as usize)
        )
        .into_bytes(),
    }
}

fn header() -> Header {
    Header::session("bybit", "2026-01-02", "md1", "0.1.0", "host", 5)
}

/// QRAW1 bytes of records `range`, with the header when `first`.
fn content(range: std::ops::Range<u64>, first: bool) -> Vec<u8> {
    let mut out = if first {
        qraw::encode_header(&header())
    } else {
        Vec::new()
    };
    for i in range {
        qraw::encode_record(&rec(i), &mut out).expect("encode");
    }
    out
}

#[test]
fn concatenated_frames_decode_to_one_stream() {
    let contexts = Contexts::default();
    let mut file = Vec::new();
    for (i, range) in [0..40, 40..41, 41..300].into_iter().enumerate() {
        let body = content(range, i == 0);
        let mut enc = FrameEncoder::new(9, Some(body.len() as u64), &contexts).expect("encoder");
        enc.write(&body, &mut file).expect("write");
        enc.finish(&mut file).expect("finish");
    }
    let scan = zst::scan(&file).expect("scan");
    assert_eq!(scan.frames.len(), 3);
    assert!(scan.unfinished.is_none());
    let segments = zst::decode_frames(&file, &scan).expect("decode");
    let refs: Vec<&[u8]> = segments.iter().map(Vec::as_slice).collect();
    let d = qraw::decode(&refs).expect("qraw");
    assert!(!d.truncated_tail);
    let want: Vec<Record> = (0..300).map(rec).collect();
    assert_eq!(d.records, want, "rec_no runs 0..300 without a break");
    assert_eq!(contexts.live(), 0, "every context released");
}

#[test]
fn scanner_finds_frame_ends_and_the_last_complete_block() {
    let contexts = Contexts::default();
    // One open file: level 3, a block flush after every 20 records, no frame end.
    let mut file = Vec::new();
    let mut enc = FrameEncoder::new(3, None, &contexts).expect("encoder");
    enc.write(&content(0..0, true), &mut file).expect("header");
    let mut flushed_at = vec![];
    for chunk in 0..10u64 {
        enc.write(&content(chunk * 20..chunk * 20 + 20, false), &mut file)
            .expect("write");
        enc.flush(&mut file).expect("flush");
        flushed_at.push(file.len());
    }
    drop(enc);
    // Each flush ends at least one block: the file grows at every flush.
    assert!(flushed_at.windows(2).all(|w| w[0] < w[1]), "{flushed_at:?}");
    // Block ends: after the 6-byte frame header (magic, descriptor, window;
    // checksum flag, no content size), and at every flush (each flush here
    // holds well under one 128 KiB block).
    let mut boundaries = vec![6];
    boundaries.extend(&flushed_at);
    let all: Vec<Record> = (0..200).map(rec).collect();
    // Every cut after the frame header: the last complete block is <= the cut,
    // and decoding up to it gives a prefix of the records, at least every
    // record flushed before the cut.
    for cut in 6..=file.len() {
        let part = &file[..cut];
        let scan = zst::scan(part).expect("scan");
        assert!(scan.frames.is_empty());
        let u = scan.unfinished.expect("unfinished frame");
        assert!(u.complete_blocks_end <= cut);
        let segments = zst::decode_frames(part, &scan).expect("decode");
        let refs: Vec<&[u8]> = segments.iter().map(Vec::as_slice).collect();
        let records = match qraw::decode(&refs) {
            Ok(d) => d.records,
            Err(qraw::FramingError::NoMagic | qraw::FramingError::TornHeader) => vec![],
            Err(e) => panic!("cut {cut}: {e:?}"),
        };
        assert_eq!(records[..], all[..records.len()], "cut {cut}: a prefix");
        let want = boundaries
            .iter()
            .copied()
            .filter(|&e| e <= cut)
            .max()
            .unwrap_or(0);
        assert_eq!(
            u.complete_blocks_end, want,
            "cut {cut}: the last block end at or before it"
        );
        let again = zst::scan(&file[..u.complete_blocks_end]).expect("rescan");
        assert_eq!(
            again.unfinished.map(|x| x.complete_blocks_end),
            Some(u.complete_blocks_end),
            "cut {cut}: the cut-back file ends at a block boundary"
        );
        let flushed = flushed_at.iter().filter(|&&f| f <= cut).count() * 20;
        assert!(
            records.len() >= flushed,
            "cut {cut}: {} < {flushed}",
            records.len()
        );
    }
    // A finished frame is reported as a frame, with its end offset.
    let mut done = Vec::new();
    let mut enc = FrameEncoder::new(3, None, &contexts).expect("encoder");
    enc.write(&content(0..5, true), &mut done).expect("write");
    enc.finish(&mut done).expect("finish");
    let scan = zst::scan(&done).expect("scan");
    assert_eq!(scan.frames.len(), 1);
    assert_eq!(scan.frames[0].end, done.len());
    assert!(zst::scan(b"not zstd").is_err());
}

#[test]
fn pledged_size_and_live_contexts_are_counted() {
    let contexts = Contexts::default();
    let a = FrameEncoder::new(9, Some(1_000), &contexts).expect("a");
    let b = FrameEncoder::new(9, Some(5_000_000), &contexts).expect("b");
    assert_eq!(contexts.live(), 2);
    assert_eq!(contexts.pledged(), vec![Some(1_000), Some(5_000_000)]);
    drop(a);
    assert_eq!(contexts.live(), 1);
    assert_eq!(contexts.max_live(), 2);
    drop(b);
    // A small pledged size gives a smaller level-9 context (measured, cached).
    let small = zst::context_bytes(9, Some(64 * 1024));
    let big = zst::context_bytes(9, Some(64 * 1024 * 1024));
    assert!(small < big, "{small} vs {big}");
    assert!(small > 0);
}

#[test]
fn a_record_across_a_frame_boundary_is_refused() {
    let contexts = Contexts::default();
    let body = content(0..3, true);
    let split = body.len() - 10; // inside the last record
    let mut file = Vec::new();
    for part in [&body[..split], &body[split..]] {
        let mut enc = FrameEncoder::new(9, Some(part.len() as u64), &contexts).expect("encoder");
        enc.write(part, &mut file).expect("write");
        enc.finish(&mut file).expect("finish");
    }
    let scan = zst::scan(&file).expect("scan");
    let segments = zst::decode_frames(&file, &scan).expect("decode");
    let refs: Vec<&[u8]> = segments.iter().map(Vec::as_slice).collect();
    assert!(matches!(
        qraw::decode(&refs),
        Err(qraw::FramingError::RecordCrossesSegment { segment: 0 })
    ));
}
