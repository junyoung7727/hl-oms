//! The golden L0 fixtures of the contract give exactly the golden L1 rows
//! (spec Testing Decisions, R4).

mod common;

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use common::lake::golden_lake;
use md_archiver::build::{Settings, build_date};
use md_archiver::commit::{commit, l1_files};
use md_archiver::registry;
use md_lake::fs::RealFs;
use serde_json::{Map, Value};

const DATES: [&str; 3] = ["2026-01-01", "2026-01-02", "2026-01-05"];

fn golden(table: &str) -> BTreeMap<String, Vec<Map<String, Value>>> {
    let v: Value = serde_json::from_slice(
        &fs::read(common::golden_dir().join(format!("l1/{table}.json"))).expect("golden"),
    )
    .expect("json");
    v.as_object()
        .expect("object")
        .iter()
        .map(|(k, rows)| {
            (
                k.clone(),
                rows.as_array()
                    .expect("rows")
                    .iter()
                    .map(|r| r.as_object().expect("row").clone())
                    .collect(),
            )
        })
        .collect()
}

pub fn build_all(lake: &Path) {
    let fs = RealFs::new(lake);
    let venue = registry::venue("bybit").expect("bybit");
    for d in DATES {
        let built = build_date(lake, fs.as_ref(), venue, d, &Settings::default())
            .unwrap_or_else(|e| panic!("{d}: {e}"));
        commit(lake, "bybit", &built, None).expect("commit");
    }
}

/// Every L1 file of the golden dates, read back.
fn all_l1(lake: &Path) -> BTreeMap<String, Vec<Map<String, Value>>> {
    let mut out = BTreeMap::new();
    for d in DATES {
        for p in l1_files(lake, "bybit", d).expect("list") {
            out.insert(p.clone(), common::read_rows(&lake.join(&p)));
        }
    }
    out
}

#[test]
fn golden_l1_rows() {
    let dir = common::test_dir("golden_l1");
    golden_lake(&dir);
    build_all(&dir);
    let mut got = all_l1(&dir);
    let mut diffs = Vec::new();
    for table in ["book_msg", "trade", "ticker", "gap", "instrument"] {
        for (path, want) in golden(table) {
            match got.remove(&path) {
                None => diffs.push(format!("missing {path}")),
                Some(rows) => {
                    if rows.len() != want.len() {
                        diffs.push(format!(
                            "{path}: {} rows, golden {}",
                            rows.len(),
                            want.len()
                        ));
                    }
                    for (i, (g, w)) in rows.iter().zip(&want).enumerate() {
                        if g != w {
                            let cols: Vec<String> = w
                                .keys()
                                .filter(|k| g.get(*k) != w.get(*k))
                                .map(|k| format!("{k}: got {:?} want {:?}", g.get(k), w.get(k)))
                                .collect();
                            diffs.push(format!("{path} row {i}: {}", cols.join("; ")));
                        }
                        if g.len() != w.len() {
                            diffs.push(format!(
                                "{path} row {i}: {} columns, golden {}",
                                g.len(),
                                w.len()
                            ));
                        }
                    }
                }
            }
        }
    }
    // What the fixtures do not hold (R4, spec Testing Decisions): the empty
    // gap file of the date before and the instrument file of the synthetic
    // 2026-01-01 response.
    let gap0101 = "crypto/l1/gap/venue=bybit/date=2026-01-01/day.parquet";
    assert_eq!(
        got.remove(gap0101).map(|r| r.len()),
        Some(0),
        "empty gap file of 2026-01-01"
    );
    let inst0101 = "crypto/l1/instrument/venue=bybit/market=linear/date=2026-01-01/day.parquet";
    let extra = got.remove(inst0101).expect("instrument file of 2026-01-01");
    assert_eq!(extra.len(), 1);
    assert_eq!(extra[0]["recv_ns"], common::lake::EXTRA_INSTRUMENTS_NS);
    for p in got.keys() {
        diffs.push(format!("unexpected {p}"));
    }
    assert!(
        diffs.is_empty(),
        "{} differences:\n{}",
        diffs.len(),
        diffs.join("\n")
    );
}

/// Contract 7: no two rows of a table share its key, over all files.
#[test]
fn golden_keys_are_unique() {
    let dir = common::test_dir("golden_keys");
    golden_lake(&dir);
    build_all(&dir);
    let keys: [(&str, &[&str]); 5] = [
        (
            "book_msg",
            &[
                "venue",
                "market",
                "symbol",
                "channel",
                "l0_file_id",
                "rec_no",
                "kind",
                "recv_ns",
            ],
        ),
        ("trade", &["venue", "market", "symbol", "trade_id"]),
        (
            "ticker",
            &["venue", "market", "symbol", "l0_file_id", "rec_no"],
        ),
        (
            "gap",
            &[
                "venue",
                "market",
                "symbol",
                "channel",
                "kind",
                "start_recv_ns",
                "conn_id",
            ],
        ),
        ("instrument", &["venue", "market", "symbol", "recv_ns"]),
    ];
    let all = all_l1(&dir);
    for (table, cols) in keys {
        let mut seen = std::collections::BTreeSet::new();
        for (p, rows) in all
            .iter()
            .filter(|(p, _)| p.starts_with(&format!("crypto/l1/{table}/")))
        {
            for r in rows {
                let k: Vec<String> = cols.iter().map(|c| r[*c].to_string()).collect();
                assert!(
                    seen.insert(k.clone()),
                    "{table}: duplicate key {k:?} in {p}"
                );
            }
        }
        assert!(!seen.is_empty(), "{table} has rows");
    }
}
