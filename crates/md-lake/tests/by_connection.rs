//! Layout by_connection (lake contract 2.5 step 4): each open file's records,
//! unchanged and unclassified, as one level-9 frame per hour of that
//! connection's day file; one header, at the start of the day file only.

mod common;

use std::sync::Arc;

use common::kit::{self, FixedClock, S, T14, classify, md};
use md_lake::fs::{Fs, RealFs};
use md_lake::qraw::{self, Record};
use md_lake::seal::{Layout, SealSettings, Sealer};
use md_lake::{time, zst};

const D: &str = "2026-01-02";

fn settings(layout: Layout, measure: bool) -> SealSettings {
    let mut s = SealSettings::new("bybit", "md_sealer", "0.1.0", "test", T14);
    s.layout = layout;
    s.measure_layouts = measure;
    s
}

fn seal(fs: &Arc<dyn Fs>, s: SealSettings, now: i64) {
    let clock = FixedClock::at(now);
    Sealer::new(fs.clone(), s)
        .seal(clock.as_ref(), &classify)
        .expect("seal");
}

#[test]
fn by_connection_is_lossless() {
    let fs: Arc<dyn Fs> = RealFs::new(common::test_dir("byconn-lossless"));
    let session = T14 - S;
    let h15 = T14 + time::HOUR_NS;
    // Records of hour 14 and 15 on one connection, a control record among them
    // (by_connection does not classify), and recv_ns that steps back.
    let r14 = vec![
        md(T14 + S, 1, 0, "orderbook.50.BTCUSDT"),
        kit::control(T14 + 3 * S, 1, 1),
        md(T14 + 2 * S, 1, 2, "publicTrade.ETHUSDT"),
    ];
    let r15 = vec![md(h15 + S, 1, 3, "tickers.BTCUSDT")];
    let mut all = r14.clone();
    all.extend(r15.clone());
    kit::write_open(&fs, "md1", session, 1, &all, false);
    seal(
        &fs,
        settings(Layout::ByConnection, false),
        h15 + time::HOUR_NS + S,
    );

    let path = format!("crypto/l0/venue=bybit/_conn/date={D}/md1-{session}-c1.qraw.zst.partial");
    let (records, frames) = kit::day_records(fs.as_ref(), &path);
    assert_eq!(frames, 2, "one frame per hour");
    assert_eq!(records, all, "unchanged, in file order");
    // One header, in the first frame only, of the connection form with the day's date.
    let bytes = kit::read_all(fs.as_ref(), &path);
    let scan = zst::scan(&bytes).expect("scan");
    let segs = zst::decode_frames(&bytes, &scan).expect("decode");
    assert!(segs[0].starts_with(qraw::MAGIC));
    assert!(!segs[1].starts_with(qraw::MAGIC), "no second header");
    let (h, _) = qraw::decode_header(&segs[0]).expect("header");
    assert_eq!(
        (
            h.get_str("form"),
            h.get_str("date"),
            h.get_i64("session_ns"),
            h.get_i64("conn_id")
        ),
        (Some("connection"), Some(D), Some(session), Some(1))
    );
    for hour in [14u8, 15] {
        let log = kit::seal_log(fs.as_ref(), D, hour);
        let done = log.last().expect("done");
        assert_eq!(done["layout"], "by_connection");
        for t in done["targets"].as_array().expect("targets") {
            assert_eq!(t["sessions"], serde_json::Value::Null);
        }
        common::schema::assert_valid("l0/seal_log_entry.schema.json", done);
    }
    assert_eq!(
        kit::seal_log(fs.as_ref(), D, 14).last().expect("done")["unroutable_records"],
        0
    );
    let _: Vec<Record> = records;
}

#[test]
fn measure_layouts_records_both_sizes() {
    for layout in [Layout::BySymbol, Layout::ByConnection] {
        let dir = common::test_dir(&format!("byconn-measure-{layout:?}"));
        let fs: Arc<dyn Fs> = RealFs::new(&dir);
        let recs: Vec<Record> = (0..300)
            .map(|i| {
                md(
                    T14 + i * S / 10,
                    1,
                    i as u64,
                    if i % 2 == 0 {
                        "orderbook.50.BTCUSDT"
                    } else {
                        "publicTrade.BTCUSDT"
                    },
                )
            })
            .collect();
        kit::write_open(&fs, "md1", T14 - S, 1, &recs, false);
        seal(&fs, settings(layout, true), T14 + time::HOUR_NS + S);
        let done = kit::seal_log(fs.as_ref(), D, 14).pop().expect("done");
        let m = &done["measure"];
        let (sym, conn) = (
            m["by_symbol_bytes"].as_u64().expect("sym"),
            m["by_connection_bytes"].as_u64().expect("conn"),
        );
        assert!(sym > 0 && conn > 0, "{m}");
        // The measured size of the layout used equals the bytes it appended.
        let appended: u64 = done["targets"]
            .as_array()
            .expect("targets")
            .iter()
            .filter(|t| !t["path"].as_str().unwrap_or("").contains("/_session/"))
            .map(|t| t["offset_after"].as_u64().expect("after"))
            .sum();
        let used = if layout == Layout::BySymbol {
            sym
        } else {
            conn
        };
        assert_eq!(used, appended, "{layout:?}");
        common::schema::assert_valid("l0/seal_log_entry.schema.json", &done);
        // No file of the other layout was written.
        let other = if layout == Layout::BySymbol {
            "crypto/l0/venue=bybit/_conn"
        } else {
            "crypto/l0/venue=bybit/market=linear"
        };
        assert!(!dir.join(other).exists(), "{layout:?}: {other} exists");
    }
}
