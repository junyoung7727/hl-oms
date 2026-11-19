//! Ticket 02: the seal's classifier and the md-lake binary (spec D1, D9).

mod common;

use std::sync::Arc;

use common::*;
use md_collector::registry;
use md_lake::fs::{Fs, RealFs};
use md_lake::qraw::{Record, Src};
use md_lake::seal::Topic;
use md_lake::writer::{OpenWriter, WriterSettings};
use md_lake::zst::Contexts;

/// 2026-01-02 14:00:00 UTC.
const T14: i64 = 1_767_362_400_000_000_000;
const SPOT_CONN: u32 = (2 << 28) + 5;

fn ws(t: i64, conn_id: u32, frame_no: u64, payload: &str) -> Record {
    Record {
        recv_ns: t,
        src: Src::WebSocket,
        conn_id,
        frame_no,
        payload: payload.as_bytes().to_vec(),
    }
}

fn topic(market: &str, channel: &str, symbol: &str) -> Option<Topic> {
    Some(Topic {
        market: market.into(),
        channel: channel.into(),
        symbol: symbol.into(),
    })
}

#[test]
fn classify_ws_by_conn_slot() {
    let v = registry::venue("bybit").expect("bybit");
    let f = book(50, "BTCUSDT", "snapshot", 1, 1);
    assert_eq!(
        v.classify(&ws(1, 5, 0, &f)),
        topic("linear", "orderbook.50", "BTCUSDT")
    );
    assert_eq!(
        v.classify(&ws(1, (1 << 28) + 5, 0, &f)),
        topic("inverse", "orderbook.50", "BTCUSDT")
    );
    assert_eq!(
        v.classify(&ws(1, SPOT_CONN, 0, &f)),
        topic("spot", "orderbook.50", "BTCUSDT")
    );
    // A slot no market owns.
    assert_eq!(v.classify(&ws(1, 3 << 28, 0, &f)), None);
    // Control frames and notes are never routed.
    assert_eq!(v.classify(&ws(1, 5, 1, r#"{"op":"pong"}"#)), None);
    let mut note = ws(1, 0, 0, r#"{"note":"overflow"}"#);
    note.src = Src::Note;
    assert_eq!(v.classify(&note), None);
}

#[test]
fn classify_rest_by_category() {
    let v = registry::venue("bybit").expect("bybit");
    let mut r = ws(1, 0, 0, &instruments_body("inverse", &["BTCUSD"], ""));
    r.src = Src::Rest;
    assert_eq!(v.classify(&r), topic("inverse", "instruments", "_all"));
    r.payload = br#"{"retCode":10006,"retMsg":"Too many visits"}"#.to_vec();
    assert_eq!(v.classify(&r), None);
    r.payload = instruments_body("option", &["BTC-X"], "").into_bytes();
    assert_eq!(v.classify(&r), None);
}

#[test]
fn md_lake_unknown_venue() {
    let dir = test_dir("unknown-venue");
    std::fs::create_dir_all(dir.join("crypto/l0/_open")).expect("mkdir");
    std::fs::write(dir.join("crypto/l0/_open/keep.txt"), b"keep").expect("write");
    let before = tree(&dir);
    let lake = dir.to_string_lossy().to_string();
    for args in [
        vec![
            "seal",
            "--venue",
            "nope",
            "--lake",
            &lake,
            "--first-hour",
            "2026-01-02T14",
        ],
        vec![
            "seal-days",
            "--venue",
            "nope",
            "--lake",
            &lake,
            "--first-hour",
            "2026-01-02T14",
            "--from",
            "2026-01-02",
        ],
        vec![
            "verify",
            "--venue",
            "nope",
            "--lake",
            &lake,
            "--date",
            "2026-01-02",
            "--json",
        ],
    ] {
        let (code, _, err) = md_lake(&args);
        assert_ne!(code, 0, "{args:?}");
        assert!(err.contains("nope"), "{err}");
    }
    assert_eq!(tree(&dir), before, "a file changed");
    // A lake root that does not exist is not created either.
    let missing = dir.join("absent").to_string_lossy().to_string();
    let (code, _, _) = md_lake(&[
        "seal",
        "--venue",
        "nope",
        "--lake",
        &missing,
        "--first-hour",
        "2026-01-02T14",
    ]);
    assert_ne!(code, 0);
    assert!(!dir.join("absent").exists());
}

#[test]
fn md_lake_usage_errors() {
    for args in [
        vec![],
        vec!["seal"],
        vec!["seal", "--venue", "bybit"],
        vec!["frobnicate"],
    ] {
        let (code, _, _) = md_lake(&args);
        assert_eq!(code, 2, "{args:?}");
    }
}

fn write_open(fs: &Arc<dyn Fs>, session_ns: i64, conn_id: u32, records: &[Record]) {
    let settings = WriterSettings::new("bybit", "md_collector", "0.1.0", "test", session_ns);
    let mut w = OpenWriter::new(fs.clone(), settings, conn_id, &Contexts::default());
    for r in records {
        w.append(r).expect("append");
    }
    w.close().expect("close");
}

#[test]
fn md_lake_seal_and_verify() {
    let dir = test_dir("seal-verify");
    let fs = RealFs::new(&dir);
    let session = T14 - 10 * S;
    let lin = [
        ws(T14 + S, 5, 0, &book(50, "BTCUSDT", "snapshot", 1, 1)),
        ws(T14 + 2 * S, 5, 1, &trade("BTCUSDT", "t1", 2)),
        ws(T14 + 3 * S, 5, 2, &book(50, "BTCUSDT", "delta", 2, 3)),
    ];
    let spot = [ws(
        T14 + S + 1,
        SPOT_CONN,
        0,
        &book(50, "BTCUSDT", "snapshot", 7, 1),
    )];
    let mut rest = ws(T14 + 1, 0, 0, &instruments_body("linear", &["BTCUSDT"], ""));
    rest.src = Src::Rest;
    write_open(&fs, session, 5, &lin);
    write_open(&fs, session, SPOT_CONN, &spot);
    write_open(&fs, session, 0, std::slice::from_ref(&rest));
    let lake = dir.to_string_lossy().to_string();
    let now = (T14 - 14 * HOUR + DAY + HOUR + S).to_string();
    let (code, out, err) = md_lake(&[
        "seal",
        "--venue",
        "bybit",
        "--lake",
        &lake,
        "--first-hour",
        "2026-01-02T14",
        "--now-ns",
        &now,
    ]);
    assert_eq!(code, 0, "{out}\n{err}");
    let report: serde_json::Value = serde_json::from_str(&out).expect("report json");
    assert!(
        report["sealed"].as_array().is_some_and(|a| !a.is_empty()),
        "{report}"
    );

    let date = "2026-01-02";
    let read = |market: &str, channel: &str, symbol: &str| -> Vec<Vec<u8>> {
        let p = day_file(&dir, market, channel, date, symbol)
            .unwrap_or_else(|| panic!("no day file {market}/{channel}/{symbol}"));
        records(fs.as_ref(), &p)
            .into_iter()
            .map(|r| r.payload)
            .collect()
    };
    assert_eq!(
        read("linear", "orderbook.50", "BTCUSDT"),
        vec![lin[0].payload.clone(), lin[2].payload.clone()]
    );
    assert_eq!(
        read("linear", "publicTrade", "BTCUSDT"),
        vec![lin[1].payload.clone()]
    );
    assert_eq!(
        read("spot", "orderbook.50", "BTCUSDT"),
        vec![spot[0].payload.clone()]
    );
    assert_eq!(
        read("linear", "instruments", "_all"),
        vec![rest.payload.clone()]
    );
    assert!(day_file(&dir, "spot", "publicTrade", date, "BTCUSDT").is_none());

    let (code, out, err) = md_lake(&[
        "verify", "--venue", "bybit", "--lake", &lake, "--date", date, "--json", "--now-ns", &now,
    ]);
    assert_eq!(code, 0, "{err}");
    let v: serde_json::Value = serde_json::from_str(&out).expect("verify json");
    assert_eq!(v["venue"], "bybit");
    assert_eq!(v["date"], date);
    assert!(v["files"].is_array(), "{v}");

    // A second seal run with the same clock has nothing left to do.
    let (code, out, _) = md_lake(&[
        "seal",
        "--venue",
        "bybit",
        "--lake",
        &lake,
        "--first-hour",
        "2026-01-02T14",
        "--now-ns",
        &now,
    ]);
    assert_eq!(code, 0, "{out}");
}

#[test]
fn md_lake_future_now_refused() {
    // Spec R8: a clock in the future could seal hours that are not closed.
    let dir = test_dir("future-now");
    let lake = dir.to_string_lossy().to_string();
    let future = (md_collector::wall_ns() + 2 * HOUR).to_string();
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_md-lake"))
        .args([
            "seal",
            "--venue",
            "bybit",
            "--lake",
            &lake,
            "--first-hour",
            "2026-01-02T14",
            "--now-ns",
            &future,
        ])
        .env_remove("MD_LAKE_TEST_ALLOW_FUTURE_NOW")
        .output()
        .expect("run");
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("future"));
    assert!(tree(&dir).is_empty(), "nothing was written");
}
