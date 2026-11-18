//! The lake contract's golden L0 frames must give the golden L1 rows through
//! this parser: the archiver (U7) copies these messages into rows, so a
//! difference here is a wrong row in the lake.

mod common;

use std::collections::BTreeMap;
use std::fs;

use common::qraw::{self, Record};
use md_bybit::{BybitAdapter, Market};
use md_core::adapter::{MarketDataAdapter, Route};
use md_core::{Normalized, Side, StreamKey};
use serde_json::Value;

/// Rows of one golden L1 table, over all its files.
fn rows(table: &str) -> Vec<Value> {
    let path = qraw::golden_dir().join("l1").join(format!("{table}.json"));
    let files: BTreeMap<String, Vec<Value>> =
        serde_json::from_slice(&fs::read(&path).expect("read rows")).expect("rows json");
    files.into_values().flatten().collect()
}

fn i(row: &Value, col: &str) -> Option<i64> {
    row[col].as_i64()
}

fn s<'a>(row: &'a Value, col: &str) -> &'a str {
    row[col]
        .as_str()
        .unwrap_or_else(|| panic!("{col} is not text"))
}

fn ints(row: &Value, col: &str) -> Vec<i64> {
    row[col]
        .as_array()
        .unwrap_or_else(|| panic!("{col} is not a list"))
        .iter()
        .map(|v| v.as_i64().expect("int"))
        .collect()
}

struct Golden {
    files: BTreeMap<String, Vec<Record>>,
}

impl Golden {
    fn new() -> Self {
        Self {
            files: BTreeMap::new(),
        }
    }

    fn record(&mut self, row: &Value) -> Record {
        let id = s(row, "l0_file_id").to_string();
        let rec_no = usize::try_from(i(row, "rec_no").expect("rec_no")).expect("rec_no");
        let file = self
            .files
            .entry(id.clone())
            .or_insert_with(|| qraw::read_file(&id));
        file.get(rec_no)
            .unwrap_or_else(|| panic!("{id} has no record {rec_no}"))
            .clone()
    }
}

fn adapter(golden: &mut Golden) -> BybitAdapter {
    let rows = rows("instrument");
    assert!(!rows.is_empty());
    let mut all = Vec::new();
    for row in &rows {
        let r = golden.record(row);
        assert_eq!(r.src, 1, "instruments come from REST");
        let parsed = BybitAdapter::new(Market::Linear, &[])
            .parse_instruments(&r.payload, r.recv_ns)
            .expect("golden instruments");
        let naming = rows.iter().filter(|x| lineage(x) == lineage(row)).count();
        assert_eq!(parsed.len(), naming, "instrument rows of the record");
        let inst = parsed
            .iter()
            .find(|x| x.symbol == s(row, "symbol"))
            .expect("the row's symbol")
            .clone();
        assert_eq!(
            (
                inst.status.as_str(),
                inst.tick_size.as_str(),
                inst.lot_size.as_str()
            ),
            (s(row, "status"), s(row, "tick_size"), s(row, "lot_size"))
        );
        assert_eq!(
            (
                i64::from(inst.px_exp),
                i64::from(inst.qty_exp),
                inst.venue_ts_ns
            ),
            (
                i(row, "px_exp").expect("px_exp"),
                i(row, "qty_exp").expect("qty_exp"),
                i(row, "venue_ts_ns")
            )
        );
        assert_eq!(inst.contract_type.as_deref(), row["contract_type"].as_str());
        assert_eq!(inst.settle_coin.as_deref(), row["settle_coin"].as_str());
        all.push(inst);
    }
    BybitAdapter::new(Market::Linear, &all)
}

/// Parses the record of a row through `route` and `parse`, as the archiver will.
fn parse(a: &BybitAdapter, row: &Value, r: &Record) -> Vec<Normalized> {
    let key = StreamKey::new(s(row, "channel"), s(row, "symbol"));
    assert_eq!(r.src, 0, "a WebSocket record");
    assert_eq!(a.route(&r.payload), Route::Stream(key.clone()));
    a.parse(&key, &r.payload, r.recv_ns)
        .unwrap_or_else(|e| panic!("{}: {}", String::from_utf8_lossy(&r.payload), e.reason))
}

fn lineage(row: &Value) -> (String, i64) {
    (
        s(row, "l0_file_id").to_string(),
        i(row, "rec_no").expect("rec_no"),
    )
}

#[test]
fn golden_l0_frames_give_the_golden_l1_rows() {
    let mut golden = Golden::new();
    let a = adapter(&mut golden);
    // (l0_file_id, rec_no) → (messages parsed, rows naming it)
    let mut counts: BTreeMap<(String, i64), (usize, usize)> = BTreeMap::new();

    let book_rows: Vec<Value> = rows("book_msg")
        .into_iter()
        .filter(|r| i(r, "kind") != Some(2))
        .collect();
    assert!(book_rows.len() >= 30, "{} book rows", book_rows.len());
    for row in &book_rows {
        let r = golden.record(row);
        assert_eq!(
            (r.recv_ns, i64::from(r.conn_id)),
            (
                i(row, "recv_ns").expect("recv"),
                i(row, "conn_id").expect("conn")
            )
        );
        let msgs = parse(&a, row, &r);
        counts.entry(lineage(row)).or_insert((msgs.len(), 0)).1 += 1;
        let (kind, u) = match msgs.as_slice() {
            [Normalized::BookSnapshot(u)] => (0, u),
            [Normalized::BookDelta(u)] => (1, u),
            other => panic!("not one book message: {other:?}"),
        };
        let levels = |side: &[md_core::Level]| -> (Vec<i64>, Vec<i64>) {
            (
                side.iter().map(|l| l.px).collect(),
                side.iter().map(|l| l.qty).collect(),
            )
        };
        assert_eq!(Some(kind), i(row, "kind"));
        assert_eq!(
            (
                Some(u.venue_ts_ns),
                Some(u.recv_ns),
                u.update_id,
                u.prev_update_id,
                u.first_update_id,
                u.venue_seq
            ),
            (
                i(row, "venue_ts_ns"),
                i(row, "recv_ns"),
                i(row, "update_id"),
                i(row, "prev_update_id"),
                i(row, "first_update_id"),
                i(row, "venue_seq")
            )
        );
        assert_eq!(levels(&u.bids), (ints(row, "bid_px"), ints(row, "bid_qty")));
        assert_eq!(levels(&u.asks), (ints(row, "ask_px"), ints(row, "ask_qty")));
        assert_eq!(
            (Some(i64::from(u.px_exp)), Some(i64::from(u.qty_exp))),
            (i(row, "px_exp"), i(row, "qty_exp"))
        );
        let depth = s(row, "channel").rsplit('.').next().expect("depth");
        assert_eq!(depth.parse::<i64>().ok(), i(row, "depth"));
    }

    let trade_rows = rows("trade");
    assert!(!trade_rows.is_empty());
    for row in &trade_rows {
        let r = golden.record(row);
        let msgs = parse(&a, row, &r);
        let data_len = serde_json::from_slice::<Value>(&r.payload).expect("json")["data"]
            .as_array()
            .expect("trade list")
            .len();
        assert_eq!(msgs.len(), data_len, "one Trade per element");
        counts.entry(lineage(row)).or_insert((msgs.len(), 0)).1 += 1;
        let idx = usize::try_from(i(row, "msg_idx").expect("msg_idx")).expect("idx");
        let Normalized::Trade(t) = &msgs[idx] else {
            panic!("not a trade")
        };
        let side = match t.side {
            Side::Buy => 1,
            Side::Sell => -1,
        };
        assert_eq!(
            (
                t.trade_id.as_str(),
                Some(side),
                Some(t.px),
                Some(t.qty),
                t.venue_seq,
                Some(i64::from(t.flags))
            ),
            (
                s(row, "trade_id"),
                i(row, "side"),
                i(row, "px"),
                i(row, "qty"),
                i(row, "venue_seq"),
                i(row, "flags")
            )
        );
        assert_eq!(
            (
                Some(t.venue_ts_ns),
                Some(t.recv_ns),
                Some(i64::from(t.px_exp)),
                Some(i64::from(t.qty_exp))
            ),
            (
                i(row, "venue_ts_ns"),
                i(row, "recv_ns"),
                i(row, "px_exp"),
                i(row, "qty_exp")
            )
        );
    }

    let ticker_rows = rows("ticker");
    assert!(!ticker_rows.is_empty());
    for row in &ticker_rows {
        let r = golden.record(row);
        let msgs = parse(&a, row, &r);
        counts.entry(lineage(row)).or_insert((msgs.len(), 0)).1 += 1;
        let [Normalized::Ticker(t)] = msgs.as_slice() else {
            panic!("not one ticker")
        };
        assert_eq!(Some(i64::from(!t.is_snapshot)), i(row, "kind"));
        assert_eq!(
            (Some(t.venue_ts_ns), Some(t.recv_ns)),
            (i(row, "venue_ts_ns"), i(row, "recv_ns"))
        );
        assert_eq!(
            [
                t.last_px,
                t.mark_px,
                t.index_px,
                t.funding_rate,
                t.next_funding_ns,
                t.open_interest,
                t.best_bid_px,
                t.best_bid_qty,
                t.best_ask_px,
                t.best_ask_qty
            ],
            [
                "last_px",
                "mark_px",
                "index_px",
                "funding_rate",
                "next_funding_ns",
                "open_interest",
                "best_bid_px",
                "best_bid_qty",
                "best_ask_px",
                "best_ask_qty"
            ]
            .map(|c| i(row, c))
        );
        assert_eq!(
            [t.px_exp, t.ref_px_exp, t.qty_exp].map(|e| Some(i64::from(e))),
            ["px_exp", "ref_px_exp", "qty_exp"].map(|c| i(row, c))
        );
    }

    for ((id, rec_no), (parsed, named)) in &counts {
        let is_trade = id.contains("/channel=publicTrade/");
        if is_trade {
            assert!(
                named <= parsed,
                "{id} {rec_no}: {named} rows for {parsed} trades"
            );
        } else {
            assert_eq!(parsed, named, "{id} {rec_no}: messages vs rows");
        }
    }

    // The golden gap table has no parse_error row: every WebSocket record of
    // every golden topic file parses.
    let gaps = rows("gap");
    assert!(!gaps.iter().any(|g| s(g, "kind") == "parse_error"));
    let mut checked = 0;
    for id in golden.files.keys().cloned().collect::<Vec<_>>() {
        if id.contains("/channel=instruments/") {
            continue;
        }
        for r in golden.files[&id].iter().filter(|r| r.src == 0) {
            let Route::Stream(key) = a.route(&r.payload) else {
                panic!("{id}: a topic record routes to Control")
            };
            assert!(a.parse(&key, &r.payload, r.recv_ns).is_ok(), "{id}");
            checked += 1;
        }
    }
    assert!(checked >= book_rows.len());
}

#[test]
fn golden_reader_refuses_a_torn_record() {
    let mut seg = qraw::MAGIC.to_vec();
    seg.extend_from_slice(&2u32.to_le_bytes());
    seg.extend_from_slice(b"{}");
    let mut rec = Vec::new();
    rec.extend_from_slice(&5u64.to_le_bytes());
    rec.push(0);
    rec.extend_from_slice(&1u32.to_le_bytes());
    rec.extend_from_slice(&0u64.to_le_bytes());
    rec.extend_from_slice(&3u32.to_le_bytes());
    rec.extend_from_slice(b"abc");
    let mut whole = seg.clone();
    whole.extend_from_slice(&rec);
    assert_eq!(
        qraw::read_segments(&[whole.clone()]).map(|r| r.len()),
        Ok(1)
    );
    for cut in [1, 10, 26] {
        let mut torn = whole.clone();
        torn.truncate(whole.len() - cut);
        assert!(qraw::read_segments(&[torn]).is_err(), "cut {cut}");
    }
    let mut bad_src = whole.clone();
    bad_src[seg.len() + 8] = 3;
    assert!(qraw::read_segments(&[bad_src]).is_err());
}
