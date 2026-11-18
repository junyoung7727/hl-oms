//! Test helpers. Data directories are under `OMS_TEST_DIR`, else
//! `CARGO_TARGET_TMPDIR` (as md-lake): never `std::env::temp_dir()`, because
//! `/tmp` on the owner's PC is a tmpfs. Nothing here opens a non-loopback
//! socket, and every frame is synthetic (public repository; Bybit API terms
//! 6.7).
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use md_lake::fs::Fs;
use md_lake::qraw::{self, Record};
use md_lake::zst;

pub const S: i64 = 1_000_000_000;
pub const HOUR: i64 = 3600 * S;
pub const DAY: i64 = 24 * HOUR;

/// A fresh directory for one test.
pub fn test_dir(name: &str) -> PathBuf {
    let base = std::env::var("OMS_TEST_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_TARGET_TMPDIR")));
    let dir = base
        .join("md-collector")
        .join(format!("{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create test dir");
    dir
}

pub fn read_all(fs: &dyn Fs, path: &str) -> Vec<u8> {
    let mut f = fs.open(path).expect("open");
    let mut bytes = vec![0; f.len().expect("len") as usize];
    let mut at = 0;
    while at < bytes.len() {
        at += f.read_at(at as u64, &mut bytes[at..]).expect("read");
    }
    bytes
}

/// The records of an L0 file (a finished or an unfinished zstd stream).
pub fn records(fs: &dyn Fs, path: &str) -> Vec<Record> {
    let bytes = read_all(fs, path);
    let scan = zst::scan(&bytes).expect("scan");
    let segs = zst::decode_frames(&bytes, &scan).expect("decode");
    let refs: Vec<&[u8]> = segs.iter().map(Vec::as_slice).collect();
    qraw::decode(&refs).expect("qraw").records
}

/// Every file below `dir`, relative to it, with its bytes.
pub fn tree(dir: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn walk(root: &Path, d: &Path, out: &mut BTreeMap<PathBuf, Vec<u8>>) {
        let Ok(rd) = fs::read_dir(d) else { return };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                out.insert(p.strip_prefix(root).expect("prefix").join(""), Vec::new());
                walk(root, &p, out);
            } else {
                let rel = p.strip_prefix(root).expect("prefix").to_path_buf();
                out.insert(rel, fs::read(&p).expect("read"));
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(dir, dir, &mut out);
    out
}

/// Every file below `dir` whose relative path contains `part`.
pub fn files_with(dir: &Path, part: &str) -> Vec<String> {
    tree(dir)
        .keys()
        .map(|p| p.to_string_lossy().to_string())
        .filter(|p| p.contains(part) && !p.ends_with('/'))
        .collect()
}

/// Runs the md-lake binary; returns (exit code, stdout, stderr).
pub fn md_lake(args: &[&str]) -> (i32, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_md-lake"))
        .args(args)
        .output()
        .expect("run md-lake");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
    )
}

/// The sealed or still partial day file of a topic.
pub fn day_file(
    dir: &Path,
    market: &str,
    channel: &str,
    date: &str,
    symbol: &str,
) -> Option<String> {
    let base = format!(
        "crypto/l0/venue=bybit/market={market}/channel={channel}/date={date}/{symbol}.qraw.zst"
    );
    [base.clone(), format!("{base}.partial")]
        .into_iter()
        .find(|p| dir.join(p).is_file())
}

/// A Bybit `orderbook.<depth>` frame (documented v5 shape, synthetic values).
pub fn book(depth: u32, symbol: &str, kind: &str, u: i64, ts: i64) -> String {
    format!(
        r#"{{"topic":"orderbook.{depth}.{symbol}","type":"{kind}","ts":{ts},"data":{{"s":"{symbol}","b":[["42000.0","1.000"]],"a":[["42000.5","2.000"]],"u":{u},"seq":{seq}}},"cts":{cts}}}"#,
        seq = 9000 + u,
        cts = ts - 2
    )
}

pub fn trade(symbol: &str, id: &str, ts: i64) -> String {
    format!(
        r#"{{"topic":"publicTrade.{symbol}","type":"snapshot","ts":{ts},"data":[{{"T":{ts},"s":"{symbol}","S":"Buy","v":"0.010","p":"42000.0","L":"PlusTick","i":"{id}","BT":false,"RPI":false,"seq":1}}]}}"#
    )
}

/// One page of `GET /v5/market/instruments-info` (documented shape,
/// synthetic rows).
pub fn instruments_body(category: &str, symbols: &[&str], cursor: &str) -> String {
    let rows: Vec<String> = symbols
        .iter()
        .map(|s| {
            let lot = if category == "spot" {
                r#""lotSizeFilter":{"basePrecision":"0.001","minOrderQty":"0.001"}"#
            } else {
                r#""lotSizeFilter":{"qtyStep":"0.001","minOrderQty":"0.001"}"#
            };
            format!(
                r#"{{"symbol":"{s}","contractType":"LinearPerpetual","status":"Trading","baseCoin":"BTC","quoteCoin":"USDT","settleCoin":"USDT","priceFilter":{{"tickSize":"0.10"}},{lot}}}"#
            )
        })
        .collect();
    format!(
        r#"{{"retCode":0,"retMsg":"OK","result":{{"category":"{category}","list":[{}],"nextPageCursor":"{cursor}"}},"retExtInfo":{{}},"time":1767362400000}}"#,
        rows.join(",")
    )
}
