//! The input model (spec D2, D3) over the golden lake.

mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;

use common::lake::{EXTRA_INSTRUMENTS_ID, golden_lake, golden_segments};
use md_archiver::input::{
    ConnKey, DateState, InputError, InstrumentsInEffect, L0File, Pass1, RecRef, Response,
    date_state, for_each_record, has_l1, l0_dates,
};
use md_core::adapter::Instrument;
use md_lake::fs::RealFs;
use md_lake::qraw::{self, Src};
use serde_json::Value;

const S1: i64 = 1_767_308_398_000_000_000;
const S2: i64 = 1_767_317_429_000_000_000;
const OB_0102: &str = "venue=bybit/market=linear/channel=orderbook.50/date=2026-01-02/BTCUSDT";

fn sealed(st: DateState) -> Vec<L0File> {
    match st {
        DateState::Sealed(f) => f,
        other => panic!("not sealed: {other:?}"),
    }
}

#[test]
fn golden_dates_are_day_sealed() {
    let dir = common::test_dir("golden_sealed");
    golden_lake(&dir);
    let fs = RealFs::new(&dir);
    assert_eq!(
        l0_dates(fs.as_ref(), "bybit").expect("dates"),
        ["2026-01-01", "2026-01-02", "2026-01-05"]
    );
    let files = sealed(date_state(fs.as_ref(), "bybit", "2026-01-02").expect("state"));
    let ids: Vec<&str> = files.iter().map(|f| f.l0_file_id.as_str()).collect();
    assert_eq!(ids.len(), 6, "{ids:?}");
    assert!(ids.contains(&OB_0102));
    assert!(matches!(
        date_state(fs.as_ref(), "bybit", "2026-01-03").expect("state"),
        DateState::NoL0
    ));
    assert!(!has_l1(fs.as_ref(), "bybit", "2026-01-02"));
    for d in ["2026-01-01", "2026-01-05"] {
        sealed(date_state(fs.as_ref(), "bybit", d).expect("state"));
    }
}

/// Each change makes 2026-01-02 not day-sealed; the lake is rebuilt each time.
#[test]
fn day_sealed_rules() {
    let ob = format!("crypto/l0/{OB_0102}.qraw.zst");
    let manifest = "crypto/l0/_manifest/venue=bybit/date=2026-01-02.jsonl";
    type Change = Box<dyn Fn(&std::path::Path)>;
    let changes: Vec<(&str, Change)> = vec![
        (
            "a .partial file",
            Box::new(|d| {
                fs::write(
                    d.join("crypto/l0/venue=bybit/_session/date=2026-01-02/md1-1.qraw.zst.partial"),
                    b"x",
                )
                .expect("w")
            }),
        ),
        (
            "an open file",
            Box::new(|d| {
                let p = d.join("crypto/l0/_open/venue=bybit/date=2026-01-02/hour=23");
                fs::create_dir_all(&p).expect("mkdir");
                fs::write(p.join("md1-1-c1.qraw.zst.partial"), b"x").expect("w");
            }),
        ),
        (
            "no sidecar",
            Box::new(move |d| fs::remove_file(d.join(format!("{ob}.sha256"))).expect("rm")),
        ),
        (
            "a size mismatch",
            Box::new({
                let ob = format!("crypto/l0/{OB_0102}.qraw.zst");
                move |d| {
                    let mut b = fs::read(d.join(&ob)).expect("r");
                    b.push(0);
                    fs::write(d.join(&ob), b).expect("w");
                }
            }),
        ),
        (
            "a sealed file without its manifest line",
            Box::new(move |d| {
                let text = fs::read_to_string(d.join(manifest)).expect("r");
                let kept: String = text
                    .lines()
                    .filter(|l| !l.contains(OB_0102))
                    .map(|l| format!("{l}\n"))
                    .collect();
                fs::write(d.join(manifest), kept).expect("w");
            }),
        ),
        (
            "a cut last manifest line",
            Box::new(move |d| {
                let mut text = fs::read_to_string(d.join(manifest)).expect("r");
                text.truncate(text.len() - 5);
                fs::write(d.join(manifest), text).expect("w");
            }),
        ),
        (
            "a manifest line for a missing file",
            Box::new({
                let ob = format!("crypto/l0/{OB_0102}.qraw.zst");
                move |d| {
                    fs::remove_file(d.join(&ob)).expect("rm");
                    fs::remove_file(d.join(format!("{ob}.sha256"))).expect("rm");
                }
            }),
        ),
    ];
    for (name, change) in changes {
        let dir = common::test_dir("day_sealed_rules");
        golden_lake(&dir);
        change(&dir);
        let fs = RealFs::new(&dir);
        match date_state(fs.as_ref(), "bybit", "2026-01-02").expect("state") {
            DateState::NotSealed(_) => {}
            other => panic!("{name}: {other:?}"),
        }
    }
}

#[test]
fn refusals() {
    let manifest = "crypto/l0/_manifest/venue=bybit/date=2026-01-02.jsonl";
    for (name, from, to) in [
        ("contract_ver 2", "\"contract_ver\":1", "\"contract_ver\":2"),
        (
            "by_connection",
            "\"layout\":\"by_symbol\"",
            "\"layout\":\"by_connection\"",
        ),
    ] {
        let dir = common::test_dir("refusals");
        golden_lake(&dir);
        let text = fs::read_to_string(dir.join(manifest)).expect("r");
        assert!(text.contains(from), "{name}");
        fs::write(dir.join(manifest), text.replacen(from, to, 1)).expect("w");
        let fs = RealFs::new(&dir);
        match date_state(fs.as_ref(), "bybit", "2026-01-02") {
            Err(InputError::Refuse(_)) => {}
            other => panic!("{name}: {other:?}"),
        }
    }
}

#[test]
fn record_connections_from_sessions() {
    let dir = common::test_dir("record_connections");
    golden_lake(&dir);
    let fs = RealFs::new(&dir);
    let files = sealed(date_state(fs.as_ref(), "bybit", "2026-01-02").expect("state"));
    let ob = files.iter().find(|f| f.l0_file_id == OB_0102).expect("ob");
    let mut seen = Vec::new();
    for_each_record(fs.as_ref(), ob, |rec_no, conn, r| {
        seen.push((
            rec_no,
            conn.expect("a WebSocket record has a connection"),
            r.conn_id,
        ));
    })
    .expect("read");
    assert_eq!(seen.len(), 24);
    for (rec_no, conn, conn_id) in &seen {
        let want = if *rec_no < 15 { S1 } else { S2 };
        assert_eq!(conn.session_ns, want, "rec {rec_no}");
        assert_eq!(conn.collector, "md1");
        assert_eq!(conn.conn_id, *conn_id);
    }
    let files = sealed(date_state(fs.as_ref(), "bybit", "2026-01-05").expect("state"));
    let ob = files
        .iter()
        .find(|f| f.l0_file_id.contains("orderbook.50"))
        .expect("ob");
    let mut conns = BTreeSet::new();
    for_each_record(fs.as_ref(), ob, |_, conn, _| {
        conns.insert(conn.expect("conn"));
    })
    .expect("read");
    assert_eq!(conns.len(), 3, "three sessions: {conns:?}");
    assert!(conns.iter().all(|c| c.conn_id == 1));
}

#[test]
fn pass1_lifetimes() {
    let dir = common::test_dir("pass1");
    golden_lake(&dir);
    let fs = RealFs::new(&dir);
    let mut files = sealed(date_state(fs.as_ref(), "bybit", "2026-01-01").expect("state"));
    files.extend(sealed(
        date_state(fs.as_ref(), "bybit", "2026-01-02").expect("state"),
    ));
    let p = Pass1::scan(fs.as_ref(), &files).expect("pass 1");
    // Reference: every WebSocket record of the golden segments, with the
    // session from session_runs.json or from the session file's name.
    let runs: Value = serde_json::from_slice(
        &fs::read(common::golden_dir().join("l0/session_runs.json")).expect("runs"),
    )
    .expect("json");
    let mut want: BTreeMap<ConnKey, (i64, i64, BTreeSet<String>)> = BTreeMap::new();
    for f in &files {
        if f.l0_file_id == EXTRA_INSTRUMENTS_ID {
            continue;
        }
        let segs = golden_segments(&f.l0_file_id);
        let refs: Vec<&[u8]> = segs.iter().map(Vec::as_slice).collect();
        let recs = qraw::decode(&refs).expect("decode").records;
        for (i, r) in recs.iter().enumerate() {
            if r.src != Src::WebSocket || r.conn_id == 0 {
                continue;
            }
            let session_ns = match runs.get(&f.l0_file_id) {
                Some(list) => list
                    .as_array()
                    .expect("runs")
                    .iter()
                    .find(|x| {
                        let a = x["first_rec_no"].as_u64().expect("first");
                        a <= i as u64 && (i as u64) < a + x["records"].as_u64().expect("n")
                    })
                    .expect("run")["session_ns"]
                    .as_i64()
                    .expect("ns"),
                None => f
                    .l0_file_id
                    .rsplit('-')
                    .next()
                    .expect("start")
                    .parse()
                    .expect("ns"),
            };
            let key = ConnKey {
                collector: "md1".into(),
                session_ns,
                conn_id: r.conn_id,
            };
            let e = want
                .entry(key)
                .or_insert((r.recv_ns, r.recv_ns, BTreeSet::new()));
            e.0 = e.0.min(r.recv_ns);
            e.1 = e.1.max(r.recv_ns);
            if let Some(t) = f.topic().filter(|t| t.channel != "instruments") {
                e.2.insert(t.channel);
            }
        }
    }
    let got: BTreeMap<ConnKey, (i64, i64, BTreeSet<String>)> = p
        .conns
        .iter()
        .map(|(k, v)| {
            (
                k.clone(),
                (
                    v.first_ns,
                    v.last_ns,
                    v.topics.keys().map(|t| t.channel.clone()).collect(),
                ),
            )
        })
        .collect();
    assert_eq!(got, want);
    // The second session's connection 2 carries only publicTrade.
    let trade_only = &p.conns[&ConnKey {
        collector: "md1".into(),
        session_ns: S2,
        conn_id: 2,
    }];
    assert_eq!(
        trade_only
            .topics
            .keys()
            .map(|t| t.channel.as_str())
            .collect::<Vec<_>>(),
        ["publicTrade"]
    );
    // The collector_down note of the sealer's session file is collected.
    assert!(
        p.notes
            .iter()
            .any(|n| n.body["note"] == "collector_down" && n.writer == "md_sealer")
    );
}

fn inst(symbol: &str, tick: &str) -> Instrument {
    Instrument {
        symbol: symbol.into(),
        status: "Trading".into(),
        tick_size: tick.into(),
        lot_size: "0.001".into(),
        px_exp: md_core::decimal::exponent_of(tick).expect("tick"),
        qty_exp: -3,
        contract_type: None,
        settle_coin: None,
        venue_ts_ns: None,
    }
}

fn response(recv_ns: i64, rows: Vec<Instrument>) -> Response {
    Response {
        at: RecRef {
            l0_file_id: "x".into(),
            rec_no: 0,
            recv_ns,
        },
        rows,
    }
}

#[test]
fn instrument_in_effect() {
    let e = InstrumentsInEffect::new(vec![
        response(300, vec![inst("BTCUSDT", "0.01")]),
        response(100, vec![inst("BTCUSDT", "0.1"), inst("ETHUSDT", "0.01")]),
        response(200, vec![inst("SOLUSDT", "0.001")]),
    ]);
    let tick = |s: &str, t: i64| e.row(s, t).map(|i| i.tick_size.clone());
    assert_eq!(
        tick("BTCUSDT", 150).as_deref(),
        Some("0.1"),
        "latest at or before"
    );
    assert_eq!(
        tick("BTCUSDT", 300).as_deref(),
        Some("0.01"),
        "at counts as before"
    );
    assert_eq!(
        tick("BTCUSDT", 50).as_deref(),
        Some("0.1"),
        "none before: the earliest"
    );
    assert_eq!(
        tick("ETHUSDT", 400).as_deref(),
        Some("0.01"),
        "a later response that does not name it does not hide it"
    );
    assert_eq!(tick("SOLUSDT", 150).as_deref(), Some("0.001"));
    assert_eq!(tick("XRPUSDT", 150), None, "no row in the window");
    let rows: Vec<String> = e.rows_at(250).into_iter().map(|i| i.symbol).collect();
    assert_eq!(rows, ["BTCUSDT", "ETHUSDT", "SOLUSDT"]);
}
