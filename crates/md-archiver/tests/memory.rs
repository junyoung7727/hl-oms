//! Peak memory of a large book day (spec R6), measured once by hand in
//! release mode and recorded in the PR; not part of the gate:
//!
//! ```text
//! MD_ARCHIVER_RSS_ROWS=3200000 cargo test --release -p md-archiver --test memory -- --ignored --nocapture
//! ```

mod common;

use std::process::Command;

use common::synth::{SEC_NS, Synth, book_frame, ws};
use md_lake::time::date_start;

#[test]
#[ignore = "large; run by hand"]
fn large_book_day_peak_rss() {
    let rows: i64 = std::env::var("MD_ARCHIVER_RSS_ROWS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(3_200_000);
    let dir = common::test_dir("large_book_day");
    let t0 = date_start("2026-02-02").expect("date");
    let s0 = t0 - 10 * SEC_NS;
    let mut s = Synth::default();
    s.instruments(s0, t0 + 1);
    let bids: Vec<(i64, i64)> = (0..50).map(|i| (42_000 - i, 100 + i)).collect();
    let asks: Vec<(i64, i64)> = (0..50).map(|i| (42_001 + i, 100 + i)).collect();
    let mut recs = vec![ws(
        t0 + SEC_NS,
        1,
        1,
        book_frame("snapshot", t0 + SEC_NS, 1, &bids, &asks),
    )];
    let step = (86_000 * SEC_NS) / rows;
    let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
    for k in 1..rows {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        let t = t0 + 2 * SEC_NS + k * step;
        let lv: Vec<(i64, i64)> = (0..4)
            .map(|j| {
                (
                    41_960 + ((x >> (j * 8)) % 80) as i64,
                    ((x >> 40) % 5_000) as i64,
                )
            })
            .collect();
        let (b, a) = lv.split_at(2);
        recs.push(ws(t, 1, 1 + k as u64, book_frame("delta", t, 1 + k, b, a)));
    }
    s.topic("orderbook.50", s0, recs);
    s.write(&dir);
    let out = Command::new(env!("CARGO_BIN_EXE_md-archiver"))
        .args([
            "run",
            "--venue",
            "bybit",
            "--threads",
            "1",
            "--disk-floor-bytes",
            "1",
            "--lake",
        ])
        .arg(&dir)
        .output()
        .expect("run");
    let log = String::from_utf8_lossy(&out.stderr);
    eprintln!("{log}");
    assert!(out.status.success());
    let peak: u64 = log
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .find_map(|v| v["peak_rss_kb"].as_u64())
        .expect("peak_rss_kb");
    eprintln!("rows {rows}: peak RSS {} MB", peak / 1024);
}
