//! QRAW1 is the only original of the lake: a byte the codec changes, or a
//! record it drops or invents, is lost data.

mod common;

use md_lake::qraw::{self, Decoded, Header, Record, Src};

#[test]
fn golden_segments_round_trip_byte_exact() {
    let ids = common::golden_file_ids();
    assert_eq!(
        ids.len(),
        9,
        "golden L0 files (one directory of segments each)"
    );
    for id in ids {
        let segments = common::golden_segments(&id);
        let refs: Vec<&[u8]> = segments.iter().map(Vec::as_slice).collect();
        let Decoded {
            header,
            records,
            truncated_tail,
        } = qraw::decode(&refs).unwrap_or_else(|e| panic!("{id}: {e:?}"));
        assert!(!truncated_tail, "{id}");
        // Re-encode segment by segment: the first holds magic and header.
        let mut at = 0;
        for (i, seg) in segments.iter().enumerate() {
            let mut out = if i == 0 {
                qraw::encode_header(&header)
            } else {
                Vec::new()
            };
            while out.len() < seg.len() {
                qraw::encode_record(&records[at], &mut out).expect("encode");
                at += 1;
            }
            assert_eq!(&out, seg, "{id} segment {i}");
        }
        assert_eq!(at, records.len(), "{id}");
    }
}

fn rec(recv_ns: i64, payload: &[u8]) -> Record {
    Record {
        recv_ns,
        src: Src::WebSocket,
        conn_id: 1,
        frame_no: 7,
        payload: payload.to_vec(),
    }
}

fn session_header() -> Header {
    Header::session("bybit", "2026-01-02", "md1", "0.1.0", "host", 5)
}

#[test]
fn framing_errors_and_truncated_tail() {
    let mut first = qraw::encode_header(&session_header());
    qraw::encode_record(&rec(1, b"abc"), &mut first).expect("encode");
    let mut second = Vec::new();
    qraw::encode_record(&rec(2, b"defgh"), &mut second).expect("encode");

    // A record across a segment end.
    let mut a = first.clone();
    a.extend_from_slice(&second[..10]);
    assert!(qraw::decode(&[&a, &second[10..]]).is_err());

    // A cut inside the last record of the last segment: a truncated tail.
    let cut = &second[..second.len() - 2];
    let d = qraw::decode(&[&first, cut]).expect("tail");
    assert!(d.truncated_tail);
    assert_eq!(d.records, vec![rec(1, b"abc")]);
    let d = qraw::decode(&[&first[..first.len() - 1]]).expect("tail");
    assert!(d.truncated_tail && d.records.is_empty());

    // src 3 and an oversized len are framing errors.
    let mut bad_src = second.clone();
    bad_src[8] = 3;
    assert!(qraw::decode(&[&first, &bad_src]).is_err());
    let mut big = second.clone();
    big[21..25].copy_from_slice(&(16_777_217u32).to_le_bytes());
    assert!(qraw::decode(&[&first, &big]).is_err());
    assert!(qraw::encode_record(&rec(1, &vec![0; 16_777_217]), &mut Vec::new()).is_err());

    // A header longer than the segment, and a missing magic.
    let h = qraw::encode_header(&session_header());
    assert!(qraw::decode(&[&h[..h.len() - 3]]).is_err());
    assert!(qraw::decode(&[b"QRAW2\0\0\0\0\0"]).is_err());
    // A negative recv_ns cannot be written (the format holds u64 ≤ 2^63 − 1).
    assert!(qraw::encode_record(&rec(-1, b""), &mut Vec::new()).is_err());
}

#[test]
fn header_json_is_compact_with_sorted_keys() {
    let h = Header::topic(
        "bybit",
        "2026-01-02",
        "md1",
        "0.1.0",
        "golden",
        "linear",
        "orderbook.50",
        "BTCUSDT",
    );
    let bytes = qraw::encode_header(&h);
    let body = std::str::from_utf8(&bytes[10..]).expect("utf8");
    assert_eq!(
        body,
        r#"{"channel":"orderbook.50","collector":"md1","collector_ver":"0.1.0","contract_ver":1,"date":"2026-01-02","form":"topic","host":"golden","market":"linear","symbol":"BTCUSDT","venue":"bybit"}"#
    );
}
