//! The export (U15) of a real journal, written by `oms-journal` from
//! records the core decided. The status answer is injected.

#![allow(clippy::unwrap_used)]

#[path = "../../oms-core/tests/common/mod.rs"]
mod common;

use common::*;
use oms_core::*;
use oms_export::rows::INSTRUMENT_PATHS;
use oms_export::{Config, ExportError, Settings, export};
use oms_journal::{Options, Writer, recover};
use oms_projector::{OmsStatus, StatusSource};
use parquet::file::reader::{FileReader, SerializedFileReader};
use parquet::record::Field;
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// 2026-01-02T00:00:00Z, µs.
const DAY1: u64 = 1_767_312_000 * SEC;
const DAY: u64 = 86_400 * SEC;
const INSTANCE: u64 = 7;

fn tmp(name: &str) -> PathBuf {
    let base = std::env::var("OMS_TEST_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir());
    let d = base.join(format!("oms-export-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(d.join("data")).unwrap();
    d
}

fn opts(record_schema: u16) -> Options {
    Options {
        segment_max_bytes: 4096,
        truncate_torn_tail: true,
        record_schema,
        readable_schemas: vec![1, 2],
        instance_id: INSTANCE,
        writer: "test".into(),
    }
}

/// A journal of the records in `log`, every one synced, in segments of
/// 4 KiB. The writer (and so the journal lock) is returned.
fn journal(dir: &Path, log: &[Record], record_schema: u16) -> Writer {
    fs::write(dir.join("INSTANCE"), format!("{INSTANCE:016x}\n")).unwrap();
    let (mut w, _) = recover(dir, &opts(record_schema), 0, |_, _, _| Ok(())).unwrap();
    for rec in log {
        w.append(rec.seq, &serde_json::to_vec(rec).unwrap())
            .unwrap();
        w.sync(rec.at).unwrap();
    }
    w
}

struct St(Option<OmsStatus>);
impl StatusSource for St {
    fn poll(&mut self) -> Option<OmsStatus> {
        self.0.clone()
    }
}

fn alive(durable_seq: u64) -> St {
    St(Some(OmsStatus {
        durable_seq,
        as_of_us: DAY1,
        wire_id_form: Some(WireIdForm::Base32Pair),
        instance_id: Some(INSTANCE),
    }))
}

fn settings(root: &Path) -> Settings {
    Settings {
        data_dir: root.join("data"),
        lake_root: root.join("lake"),
        venue: "bybit".into(),
        market: "linear".into(),
        batch: 3,
    }
}

/// `config()` on instrument BTCUSDT for strategy "golden", prices without
/// a significant-figure rule.
fn btc_config() -> CoreConfig {
    let mut cfg = config();
    let mut spec = cfg.instruments.remove(&iid("X")).unwrap();
    spec.px_max_sig_figs = None;
    cfg.instruments.insert(iid("BTCUSDT"), spec);
    let mut strat = cfg.strategies.remove(&sid("a")).unwrap();
    let lim = strat.limits.remove(&iid("X")).unwrap();
    strat.limits.insert(iid("BTCUSDT"), lim);
    cfg.strategies.clear();
    cfg.strategies.insert(sid("golden"), strat);
    cfg
}

/// A harness whose next input is recorded at `at`.
fn at(h: &mut H, at: u64) {
    h.now = at - 1_000;
}

fn h_at(start: u64) -> H {
    let mut h = H::new();
    h.now = start;
    h.inp(Input::Boot {
        binary: "test".into(),
    });
    h.inp(Input::Config(Box::new(btc_config())));
    h.link_up_and_sync();
    h
}

fn mref(update_id: Option<i64>, venue_ts_ns: i64, recv_ns: i64) -> MarketRef {
    MarketRef {
        venue: RefName::new("bybit").unwrap(),
        market: RefName::new("linear").unwrap(),
        symbol: RefName::new("BTCUSDT").unwrap(),
        channel: RefName::new("orderbook.50").unwrap(),
        update_id,
        venue_ts_ns,
        recv_ns,
    }
}

fn order(
    h: &mut H,
    key: &str,
    side: Side,
    qty: &str,
    price: &str,
    tif: Tif,
    market_ref: Option<MarketRef>,
) -> OrderId {
    let r = NewOrderReq {
        strategy: sid("golden"),
        client_order_id: cid(key),
        instrument: iid("BTCUSDT"),
        side,
        qty: q(qty),
        px: px(price),
        tif,
        reduce_only: false,
        expires_at: h.now + 5 * SEC,
        market_ref,
    };
    match h.new_order(r) {
        NewOrderReply::Accepted { order_id } => order_id,
        other => panic!("not accepted: {other:?}"),
    }
}

fn fill(h: &mut H, id: Option<OrderId>, key: &str, side: Side, qty: &str, price: &str, fee: &str) {
    let venue_ts = h.now - 2_000;
    let venue_oid = id.and_then(|i| h.order(i).venue_oid.clone());
    h.venue(VenueInput::Fill(FillIn {
        key: fkey(key),
        order_id: id,
        venue_oid,
        instrument: iid("BTCUSDT"),
        side,
        qty: q(qty),
        px: px(price),
        fee: amt(fee),
        venue_ts,
    }));
}

/// The contract's golden day (2026-01-02): a GTC buy without `market_ref`
/// left open; an IOC buy with `market_ref`, filled as taker; a post-only
/// sell whose `market_ref` has no `update_id`, filled as maker with a
/// rebate; a fill of no order of ours. One record on 2026-01-03 closes the
/// day.
fn golden_day() -> H {
    let mut h = h_at(DAY1);
    at(&mut h, DAY1 + 5 * SEC);
    let o1 = order(
        &mut h,
        "c-0001",
        Side::Buy,
        "0.010",
        "41990.0",
        Tif::Gtc,
        None,
    );
    at(&mut h, DAY1 + 5_200_000);
    h.placed_open(o1, "v-0001");
    at(&mut h, DAY1 + 7_100_000);
    let o2 = order(
        &mut h,
        "c-0002",
        Side::Buy,
        "0.010",
        "42000.2",
        Tif::Ioc,
        Some(mref(
            Some(106),
            1_767_312_006_997_000_000,
            1_767_312_007_000_000_000,
        )),
    );
    at(&mut h, DAY1 + 7_200_000);
    h.placed_open(o2, "v-0002");
    at(&mut h, DAY1 + 7_300_000);
    fill(
        &mut h,
        Some(o2),
        "e-0001",
        Side::Buy,
        "0.010",
        "42000.2",
        "0.0042",
    );
    at(&mut h, DAY1 + 7_400_000);
    h.status(o2, VenueStatus::Filled, Some("0.010"));
    at(&mut h, DAY1 + 10_200_200_000);
    let o3 = order(
        &mut h,
        "c-0003",
        Side::Sell,
        "0.005",
        "41999.9",
        Tif::Alo,
        Some(mref(
            None,
            1_767_322_199_997_000_000,
            1_767_322_200_000_000_000,
        )),
    );
    at(&mut h, DAY1 + 10_200_300_000);
    h.placed_open(o3, "v-0003");
    at(&mut h, DAY1 + 10_200_400_000);
    fill(
        &mut h,
        Some(o3),
        "e-0002",
        Side::Sell,
        "0.005",
        "41999.9",
        "-0.0010",
    );
    at(&mut h, DAY1 + 10_200_500_000);
    h.status(o3, VenueStatus::Filled, Some("0.005"));
    at(&mut h, DAY1 + 10_800_000_000);
    fill(
        &mut h,
        None,
        "e-0099",
        Side::Buy,
        "0.001",
        "42010.0",
        "0.0004",
    );
    at(&mut h, DAY1 + DAY + 60 * SEC);
    h.ref_price("BTCUSDT", "42100.0");
    h
}

/// The rows of a Parquet file as JSON objects, in file order.
fn read_rows(path: &Path) -> Vec<Map<String, Value>> {
    let r = SerializedFileReader::new(fs::File::open(path).unwrap()).unwrap();
    r.get_row_iter(None)
        .unwrap()
        .map(|row| {
            row.unwrap()
                .get_column_iter()
                .map(|(name, f)| {
                    let v = match f {
                        Field::Null => Value::Null,
                        Field::Bool(b) => Value::from(*b),
                        Field::Byte(i) => Value::from(*i),
                        Field::Short(i) => Value::from(*i),
                        Field::Int(i) => Value::from(*i),
                        Field::Long(i) => Value::from(*i),
                        Field::Str(s) => Value::from(s.as_str()),
                        other => panic!("not a trading column type: {other:?}"),
                    };
                    (name.clone(), v)
                })
                .collect()
        })
        .collect()
}

fn metadata(path: &Path) -> BTreeMap<String, String> {
    let r = SerializedFileReader::new(fs::File::open(path).unwrap()).unwrap();
    r.metadata()
        .file_metadata()
        .key_value_metadata()
        .unwrap()
        .iter()
        .map(|kv| (kv.key.clone(), kv.value.clone().unwrap_or_default()))
        .collect()
}

/// Every trading file under the lake root: relative path → rows.
fn lake(root: &Path) -> BTreeMap<String, Vec<Map<String, Value>>> {
    let mut out = BTreeMap::new();
    let base = root.join("lake");
    for table in ["orders", "fills", "events"] {
        let dir = base.join("crypto/trading").join(table);
        let Ok(dates) = fs::read_dir(&dir) else {
            continue;
        };
        for d in dates {
            let d = d.unwrap().path();
            for f in fs::read_dir(&d).unwrap() {
                let f = f.unwrap().path();
                let rel = f
                    .strip_prefix(&base)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned();
                out.insert(rel, read_rows(&f));
            }
        }
    }
    out
}

fn golden(table: &str) -> BTreeMap<String, Vec<Map<String, Value>>> {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../contracts/lake/golden/trading")
        .join(format!("{table}.json"));
    serde_json::from_slice(&fs::read(p).unwrap()).unwrap()
}

fn tree_bytes(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    fs::read_dir(dir)
        .unwrap()
        .map(|e| {
            let p = e.unwrap().path();
            (
                p.file_name().unwrap().to_string_lossy().into_owned(),
                fs::read(&p).unwrap(),
            )
        })
        .collect()
}

#[test]
fn the_golden_day_exports_as_the_contract_golden_rows() {
    let root = tmp("golden");
    let h = golden_day();
    let _w = journal(&root.join("data"), &h.log, 2);
    let sum = export(&settings(&root), &mut alive(h.log.len() as u64)).unwrap();
    assert_eq!(sum.open_date.as_deref(), Some("2026-01-03"));
    let got = lake(&root);
    // Kept for comparing by hand when the golden rows change.
    fs::write(
        root.join("actual.json"),
        serde_json::to_vec_pretty(&got).unwrap(),
    )
    .unwrap();
    for table in ["orders", "fills", "events"] {
        let want = golden(table);
        let mine: BTreeMap<_, _> = got
            .iter()
            .filter(|(k, _)| k.starts_with(&format!("crypto/trading/{table}/")))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        assert_eq!(
            mine,
            want,
            "{table} (actual rows in {})",
            root.join("actual.json").display()
        );
    }
}

#[test]
fn every_file_carries_the_contract_metadata_and_types() {
    let root = tmp("meta");
    let h = golden_day();
    let _w = journal(&root.join("data"), &h.log, 2);
    let sum = export(&settings(&root), &mut alive(h.log.len() as u64)).unwrap();
    assert_eq!(sum.written.len(), 3);
    for rel in &sum.written {
        let table = rel.split('/').nth(2).unwrap();
        let path = root.join("lake").join(rel);
        let m = metadata(&path);
        assert_eq!(
            m.get("contract_ver").map(String::as_str),
            Some("1"),
            "{rel}"
        );
        assert_eq!(
            m.get("export_ver").map(String::as_str),
            Some("oms-export-1"),
            "{rel}"
        );
        assert_eq!(m.get("table").map(String::as_str), Some(table), "{rel}");
        let r = SerializedFileReader::new(fs::File::open(&path).unwrap()).unwrap();
        let cols: Vec<(String, bool)> = r
            .metadata()
            .file_metadata()
            .schema_descr()
            .columns()
            .iter()
            .map(|c| (c.name().to_string(), c.self_type().is_optional()))
            .collect();
        let schema: Value = serde_json::from_str(match table {
            "orders" => oms_export::tables::ORDERS_JSON,
            "fills" => oms_export::tables::FILLS_JSON,
            _ => oms_export::tables::EVENTS_JSON,
        })
        .unwrap();
        let want: Vec<(String, bool)> = schema["columns"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| {
                (
                    c["name"].as_str().unwrap().to_string(),
                    c["nullable"].as_bool().unwrap(),
                )
            })
            .collect();
        assert_eq!(cols, want, "{rel}");
    }
}

#[test]
fn a_second_run_changes_no_file() {
    let root = tmp("rerun");
    let h = golden_day();
    let _w = journal(&root.join("data"), &h.log, 2);
    let first = export(&settings(&root), &mut alive(h.log.len() as u64)).unwrap();
    let before: Vec<_> = first
        .written
        .iter()
        .map(|r| {
            let p = root.join("lake").join(r);
            (
                fs::read(&p).unwrap(),
                fs::metadata(&p).unwrap().modified().unwrap(),
            )
        })
        .collect();
    let second = export(&settings(&root), &mut alive(h.log.len() as u64)).unwrap();
    assert_eq!(second.written, Vec::<String>::new());
    assert_eq!(second.unchanged, 3);
    assert_eq!(second.rows, first.rows);
    for (r, (bytes, mtime)) in first.written.iter().zip(before) {
        let p = root.join("lake").join(r);
        assert_eq!(fs::read(&p).unwrap(), bytes, "{r}");
        assert_eq!(fs::metadata(&p).unwrap().modified().unwrap(), mtime, "{r}");
        let leftovers: Vec<_> = fs::read_dir(p.parent().unwrap()).unwrap().collect();
        assert_eq!(
            leftovers.len(),
            1,
            "only day.parquet in {}",
            p.parent().unwrap().display()
        );
    }
}

#[test]
fn a_changed_file_is_replaced() {
    let root = tmp("replace");
    let h = golden_day();
    let _w = journal(&root.join("data"), &h.log, 2);
    let first = export(&settings(&root), &mut alive(h.log.len() as u64)).unwrap();
    let p = root.join("lake").join(&first.written[0]);
    let good = fs::read(&p).unwrap();
    fs::write(&p, b"stale").unwrap();
    let second = export(&settings(&root), &mut alive(h.log.len() as u64)).unwrap();
    assert_eq!(second.written, vec![first.written[0].clone()]);
    assert_eq!(fs::read(&p).unwrap(), good);
}

#[test]
fn the_date_of_the_last_durable_record_is_not_written() {
    let root = tmp("open");
    let h = golden_day();
    let _w = journal(&root.join("data"), &h.log, 2);
    // Durable up to the external fill: the last record read is on the
    // golden day, so that day is open.
    let n = h.log.len() as u64 - 1;
    let sum = export(&settings(&root), &mut alive(n)).unwrap();
    assert_eq!(sum.read_to_seq, n);
    assert_eq!(sum.open_date.as_deref(), Some("2026-01-02"));
    assert_eq!(sum.written, Vec::<String>::new());
    assert!(lake(&root).is_empty());
}

#[test]
fn nothing_past_the_durable_sequence_is_read() {
    let root = tmp("cap");
    let mut h = golden_day();
    let durable = h.log.len() as u64;
    // On disk but not durable: a record on 2026-01-03 and one on 01-04.
    // Read, they would close 01-03 and write its events.
    at(&mut h, DAY1 + DAY + 3_600 * SEC);
    h.ref_price("BTCUSDT", "42050.0");
    at(&mut h, DAY1 + 2 * DAY + SEC);
    h.ref_price("BTCUSDT", "42000.0");
    let _w = journal(&root.join("data"), &h.log, 2);
    let sum = export(&settings(&root), &mut alive(durable)).unwrap();
    assert_eq!(sum.read_to_seq, durable);
    assert_eq!(sum.open_date.as_deref(), Some("2026-01-03"));
    assert!(lake(&root).keys().all(|k| k.contains("date=2026-01-02")));
}

#[test]
fn without_an_answer_of_the_owning_oms_nothing_is_written() {
    let root = tmp("instance");
    let h = golden_day();
    let _w = journal(&root.join("data"), &h.log, 2);
    let n = h.log.len() as u64;
    let other = St(Some(OmsStatus {
        durable_seq: n,
        as_of_us: DAY1,
        wire_id_form: Some(WireIdForm::Base32Pair),
        instance_id: Some(INSTANCE + 1),
    }));
    let no_id = St(Some(OmsStatus {
        instance_id: None,
        ..alive(n).0.unwrap()
    }));
    for mut st in [other, no_id, St(None)] {
        match export(&settings(&root), &mut st) {
            Err(ExportError::NoReadLimit(_)) => {}
            other => panic!("expected no read limit, got {other:?}"),
        }
    }
    assert!(!root.join("lake").exists());
}

#[test]
fn a_fill_finds_its_order_from_an_earlier_date() {
    let root = tmp("earlier");
    let mut h = h_at(DAY1);
    let o = order(&mut h, "k", Side::Sell, "0.002", "42000.0", Tif::Gtc, None);
    h.placed_open(o, "v");
    at(&mut h, DAY1 + DAY + SEC);
    fill(
        &mut h,
        Some(o),
        "late",
        Side::Sell,
        "0.002",
        "42000.0",
        "0.0001",
    );
    at(&mut h, DAY1 + 2 * DAY + SEC);
    h.ref_price("BTCUSDT", "42000.0");
    let _w = journal(&root.join("data"), &h.log, 2);
    export(&settings(&root), &mut alive(h.log.len() as u64)).unwrap();
    let got = lake(&root);
    let fills = &got["crypto/trading/fills/date=2026-01-03/day.parquet"];
    assert_eq!(fills.len(), 1);
    assert_eq!(fills[0]["symbol"], "BTCUSDT");
    assert_eq!(fills[0]["side"], -1);
    assert_eq!(fills[0]["order_id"], o.0.to_string());
    assert!(!got.contains_key("crypto/trading/orders/date=2026-01-03/day.parquet"));
}

#[test]
fn a_schema_1_journal_exports_with_its_schema() {
    let root = tmp("schema1");
    let h = golden_day();
    let _w = journal(&root.join("data"), &h.log, 1);
    export(&settings(&root), &mut alive(h.log.len() as u64)).unwrap();
    let got = lake(&root);
    for rows in got.values() {
        for r in rows {
            if let Some(s) = r.get("record_schema") {
                assert_eq!(*s, 1);
            }
        }
    }
    assert!(!got.is_empty());
}

#[test]
fn the_journal_is_read_without_its_lock_and_left_unchanged() {
    let root = tmp("untouched");
    let h = golden_day();
    let data = root.join("data");
    // The writer holds the lock for the whole run.
    let w = journal(&data, &h.log, 2);
    let before = tree_bytes(&data);
    export(&settings(&root), &mut alive(h.log.len() as u64)).unwrap();
    assert_eq!(tree_bytes(&data), before);
    drop(w);
}

#[test]
fn the_instrument_paths_cover_every_instrument_field_of_c_tail() {
    let doc = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/contracts/tail.md"),
    )
    .unwrap();
    let mut seen = std::collections::BTreeSet::new();
    for line in doc.lines() {
        let cols: Vec<&str> = line.split('|').map(str::trim).collect();
        if cols.len() < 4 {
            continue;
        }
        let path = cols[2].trim_matches('`');
        // A list (`report.positions[].instrument`) names many instruments:
        // such an event has no one symbol.
        if path.contains("[]") {
            continue;
        }
        if path == "instrument" || path.ends_with(".instrument") {
            seen.insert(path.to_string());
        }
    }
    assert!(!seen.is_empty());
    for p in &seen {
        assert!(
            INSTRUMENT_PATHS.contains(&p.as_str()),
            "C-TAIL field {p} is not in INSTRUMENT_PATHS"
        );
    }
}

#[test]
fn the_config_refuses_a_remote_status_url_and_unknown_keys() {
    let base = "data_dir = \"/d\"\nlake_root = \"/l\"\nvenue = \"bybit\"\nmarket = \"linear\"\n";
    assert!(
        Config::parse(&format!(
            "{base}status_url = \"http://127.0.0.1:9/api/status\"\n"
        ))
        .is_ok()
    );
    assert!(
        Config::parse(&format!(
            "{base}status_url = \"http://10.0.0.2:9/api/status\"\n"
        ))
        .is_err()
    );
    assert!(
        Config::parse(&format!(
            "{base}status_url = \"http://127.0.0.1:9/\"\nextra = 1\n"
        ))
        .is_err()
    );
}

#[test]
fn the_binary_exits_3_when_the_oms_does_not_answer() {
    let root = tmp("bin");
    let h = golden_day();
    let _w = journal(&root.join("data"), &h.log, 2);
    // A port nobody listens on.
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let cfg = root.join("export.toml");
    fs::write(
        &cfg,
        format!(
            "data_dir = {:?}\nlake_root = {:?}\nstatus_url = \"http://127.0.0.1:{port}/api/status\"\nvenue = \"bybit\"\nmarket = \"linear\"\n",
            root.join("data"),
            root.join("lake")
        ),
    )
    .unwrap();
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_oms-export"))
        .arg("--config")
        .arg(&cfg)
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(3),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!root.join("lake").exists());
}
