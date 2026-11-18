//! `verify` (lake contract 5.2): decode every sealed file of a date and say
//! what it holds; a framing error must be reported, never hidden.

mod common;

use std::sync::Arc;

use common::kit::{self, FixedClock};
use md_lake::fs::{Fs, RealFs};
use md_lake::verify::verify;
use md_lake::zst::{Contexts, FrameEncoder};

const D: &str = "2026-01-02";

/// Sealed files of D built from the golden segments: one zstd frame per
/// segment, as the hour seal writes them.
fn golden_lake(name: &str) -> Arc<dyn Fs> {
    let fs = RealFs::new(common::test_dir(name));
    let contexts = Contexts::default();
    for id in common::golden_file_ids() {
        if !id.contains(&format!("/date={D}/")) {
            continue;
        }
        let mut out = Vec::new();
        for seg in common::golden_segments(&id) {
            let mut enc = FrameEncoder::new(9, Some(seg.len() as u64), &contexts).expect("encoder");
            enc.write(&seg, &mut out).expect("write");
            enc.finish(&mut out).expect("finish");
        }
        let mut f = fs
            .create_new(&format!("crypto/l0/{id}.qraw.zst"))
            .expect("create");
        f.write_all(&out).expect("write");
    }
    fs
}

fn golden_result() -> serde_json::Value {
    serde_json::from_slice(
        &std::fs::read(common::golden_dir().join("xrepo/verify_result.json")).expect("golden"),
    )
    .expect("json")
}

fn by_id(v: &serde_json::Value) -> std::collections::BTreeMap<String, serde_json::Value> {
    v["files"]
        .as_array()
        .expect("files")
        .iter()
        .map(|f| (f["l0_file_id"].as_str().expect("id").to_string(), f.clone()))
        .collect()
}

#[test]
fn verify_matches_the_golden_counts() {
    let fs = golden_lake("verify-golden");
    let clock = FixedClock::at(1_767_399_060_000_000_000);
    let v = verify(fs.as_ref(), "bybit", D, "0.1.0", clock.as_ref()).expect("verify");
    common::schema::assert_valid("xrepo/verify_result.schema.json", &v);
    let (ours, golden) = (by_id(&v), by_id(&golden_result()));
    assert_eq!(
        ours.keys().collect::<Vec<_>>(),
        golden.keys().collect::<Vec<_>>()
    );
    for (id, g) in &golden {
        let o = &ours[id];
        for k in [
            "frames",
            "records",
            "first_recv_ns",
            "last_recv_ns",
            "framing_errors",
            "error",
        ] {
            assert_eq!(o[k], g[k], "{id} {k}");
        }
        // bytes and sha256 are of the file as read (ours are real zstd).
        let bytes = kit::read_all(fs.as_ref(), o["path"].as_str().expect("path"));
        assert_eq!(o["bytes"], bytes.len());
        use sha2::Digest;
        assert_eq!(o["sha256"], hex::encode(sha2::Sha256::digest(&bytes)));
    }
    // Written whole to its place.
    let p = format!("crypto/l0/_verify/venue=bybit/date={D}.json");
    let written: serde_json::Value =
        serde_json::from_slice(&kit::read_all(fs.as_ref(), &p)).expect("written");
    assert_eq!(written, v);
}

#[test]
fn verify_reports_framing_errors() {
    let fs = golden_lake("verify-errors");
    let ob = format!(
        "crypto/l0/venue=bybit/market=linear/channel=orderbook.50/date={D}/BTCUSDT.qraw.zst"
    );
    let tr = format!(
        "crypto/l0/venue=bybit/market=linear/channel=publicTrade/date={D}/BTCUSDT.qraw.zst"
    );
    // A corrupted byte in a frame, and a file cut inside its last frame.
    let mut b = kit::read_all(fs.as_ref(), &ob);
    let mid = b.len() / 2;
    b[mid] ^= 0x5a;
    let mut f = fs.open(&ob).expect("open");
    f.set_len(0).expect("truncate");
    f.write_all(&b).expect("write");
    let t = kit::read_all(fs.as_ref(), &tr);
    let mut f = fs.open(&tr).expect("open");
    f.set_len((t.len() - 7) as u64).expect("cut");
    drop(f);
    let clock = FixedClock::at(1);
    let v = verify(fs.as_ref(), "bybit", D, "0.1.0", clock.as_ref()).expect("verify");
    common::schema::assert_valid("xrepo/verify_result.schema.json", &v);
    let ours = by_id(&v);
    for id in [
        format!("venue=bybit/market=linear/channel=orderbook.50/date={D}/BTCUSDT"),
        format!("venue=bybit/market=linear/channel=publicTrade/date={D}/BTCUSDT"),
    ] {
        let o = &ours[&id];
        assert!(o["framing_errors"].as_u64().expect("n") >= 1, "{id}: {o}");
        assert!(o["error"].is_string(), "{id}: {o}");
    }
    let ok = &ours[&format!("venue=bybit/market=linear/channel=tickers/date={D}/BTCUSDT")];
    assert_eq!(
        (ok["framing_errors"].as_u64(), ok["error"].is_null()),
        (Some(0), true),
        "others unaffected"
    );
}
