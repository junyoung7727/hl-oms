//! Synthetic scenarios for the replay and gap rules (spec D4, D5, R1–R3,
//! R8, R9; tickets 03–05).

mod common;

use std::collections::BTreeMap;
use std::path::Path;

use common::synth::{H_NS, MIN_NS, SEC_NS, Synth, book_frame, note, trade_frame, ws};
use md_archiver::build::{Settings, build_date};
use md_archiver::commit::{commit, l1_files};
use md_archiver::registry;
use md_lake::fs::RealFs;
use md_lake::time::date_start;
use serde_json::{Map, Value, json};

type Rows = Vec<Map<String, Value>>;
/// `bid_px`, `bid_qty`, `ask_px`, `ask_qty`.
type Sides = (Vec<i64>, Vec<i64>, Vec<i64>, Vec<i64>);
type Levels<'a> = &'a [(i64, i64)];

fn build(lake: &Path, dates: &[&str]) -> BTreeMap<String, Rows> {
    let fs = RealFs::new(lake);
    let venue = registry::venue("bybit").expect("bybit");
    let mut out = BTreeMap::new();
    for d in dates {
        let built = build_date(lake, fs.as_ref(), venue, d, &Settings::default())
            .unwrap_or_else(|e| panic!("{d}: {e}"));
        commit(lake, "bybit", &built, None).expect("commit");
        for p in l1_files(lake, "bybit", d).expect("list") {
            out.insert(p.clone(), common::read_rows(&lake.join(&p)));
        }
    }
    out
}

fn table<'a>(all: &'a BTreeMap<String, Rows>, t: &str) -> Vec<(&'a str, &'a Map<String, Value>)> {
    all.iter()
        .filter(|(p, _)| p.starts_with(&format!("crypto/l1/{t}/")))
        .flat_map(|(p, rows)| rows.iter().map(move |r| (p.as_str(), r)))
        .collect()
}

fn gaps<'a>(all: &'a BTreeMap<String, Rows>, kind: &str) -> Vec<&'a Map<String, Value>> {
    table(all, "gap")
        .into_iter()
        .map(|(_, r)| r)
        .filter(|r| r["kind"] == kind)
        .collect()
}

fn checkpoints(all: &BTreeMap<String, Rows>) -> Vec<(String, &Map<String, Value>)> {
    table(all, "book_msg")
        .into_iter()
        .filter(|(_, r)| r["kind"] == 2)
        .map(|(p, r)| (p.rsplit('/').next().expect("name").to_string(), r))
        .collect()
}

const D: &str = "2026-02-02";
fn t0() -> i64 {
    date_start(D).expect("date")
}
const S: i64 = 1_770_000_000_000_000_000;

/// The session of the scenario starts before the date; instruments first.
fn base() -> Synth {
    let mut s = Synth::default();
    s.instruments(S, t0() + 100_000_000);
    s
}

#[test]
fn one_seq_gap_one_row() {
    let dir = common::test_dir("one_seq_gap");
    let mut s = base();
    let mut recs = vec![ws(
        t0() + 10 * MIN_NS,
        1,
        1,
        book_frame(
            "snapshot",
            t0() + 10 * MIN_NS,
            1,
            &[(4200, 1000)],
            &[(4201, 1000)],
        ),
    )];
    let mut u = 1;
    let mut fno = 2;
    // Deltas every minute until 00:40; u = 30 is missing.
    for m in 11..40 {
        u += 1;
        if u == 30 {
            u += 1;
        }
        let t = t0() + m * MIN_NS;
        recs.push(ws(
            t,
            1,
            fno,
            book_frame("delta", t, u, &[(4200, 1000 + m)], &[]),
        ));
        fno += 1;
    }
    // Resubscribe on the same connection: a snapshot at 03:00:30, deltas to 05:00.
    let t = t0() + 3 * H_NS + 30 * SEC_NS;
    recs.push(ws(
        t,
        1,
        fno,
        book_frame("snapshot", t, 500, &[(4100, 5)], &[(4300, 5)]),
    ));
    fno += 1;
    for (k, m) in (0..120).step_by(10).enumerate() {
        let t = t0() + 3 * H_NS + (m + 1) * MIN_NS;
        recs.push(ws(
            t,
            1,
            fno,
            book_frame("delta", t, 501 + k as i64, &[], &[(4300, 6 + k as i64)]),
        ));
        fno += 1;
    }
    s.topic("orderbook.50", S, recs);
    s.write(&dir);
    let all = build(&dir, &[D]);
    let g = gaps(&all, "seq_gap");
    assert_eq!(g.len(), 1, "{g:?}");
    assert_eq!(g[0]["last_good_update_id"], 29);
    assert_eq!(g[0]["next_update_id"], 31);
    assert!(
        gaps(&all, "resnapshot").is_empty(),
        "a snapshot after a break is no resnapshot (R8)"
    );
    let hours: Vec<String> = checkpoints(&all).into_iter().map(|(h, _)| h).collect();
    // The gap at 00:29 ends the state until the 03:00:30 snapshot; the
    // connection's last record is at 04:51, so 05:00 has none either.
    assert_eq!(
        hours,
        ["hour=04.parquet"],
        "no checkpoint between the gap and the snapshot"
    );
}

#[test]
fn overlap_keeps_both_connections() {
    let dir = common::test_dir("overlap");
    let mut s = base();
    let mut recs = Vec::new();
    // conn 1: snapshot at 00:00:01, deltas every 10 s until 00:10.
    recs.push(ws(
        t0() + SEC_NS,
        1,
        1,
        book_frame(
            "snapshot",
            t0() + SEC_NS,
            1,
            &[(4200, 1000)],
            &[(4201, 1000)],
        ),
    ));
    for k in 1..60i64 {
        let t = t0() + SEC_NS + k * 10 * SEC_NS;
        recs.push(ws(
            t,
            1,
            1 + k as u64,
            book_frame("delta", t, 1 + k, &[(4200, 1000 + k)], &[]),
        ));
    }
    // conn 2: snapshot at 00:05 (u = 31, the same book), deltas until 01:30.
    let t = t0() + 5 * MIN_NS + 500_000_000;
    recs.push(ws(
        t,
        2,
        1,
        book_frame("snapshot", t, 31, &[(4200, 1030)], &[(4201, 1000)]),
    ));
    for k in 1..=90i64 {
        let t = t0() + 5 * MIN_NS + k * MIN_NS;
        recs.push(ws(
            t,
            2,
            1 + k as u64,
            book_frame("delta", t, 31 + k, &[(4199, k)], &[]),
        ));
    }
    recs.sort_by_key(|r| (r.recv_ns, r.conn_id));
    s.topic("orderbook.50", S, recs);
    s.write(&dir);
    let all = build(&dir, &[D]);
    let book = table(&all, "book_msg");
    let per_conn = |c: i64| {
        book.iter()
            .filter(|(_, r)| r["kind"] != 2 && r["conn_id"] == c)
            .count()
    };
    assert_eq!(
        (per_conn(1), per_conn(2)),
        (60, 91),
        "every row of both connections"
    );
    let h = gaps(&all, "handover");
    assert_eq!(h.len(), 1, "{h:?}");
    assert_eq!(h[0]["conn_id"], 2);
    assert_eq!(h[0]["detail"], r#"{"from_conn_id":1,"to_conn_id":2}"#);
    assert!(gaps(&all, "reconnect").is_empty());
    let cp = checkpoints(&all);
    assert_eq!(cp.len(), 1, "01:00 only; conn 2 ends at 01:35");
    assert_eq!(
        cp[0].1["conn_id"], 2,
        "the book of the connection replay moved to"
    );
    assert_eq!(cp[0].1["update_id"], 31 + 54);
}

#[test]
fn trade_on_two_connections() {
    let dir = common::test_dir("trade_two_conns");
    let mut s = base();
    let mut recs = Vec::new();
    for (k, id) in ["t1", "t2", "t3", "t4", "t5"].iter().enumerate() {
        let t = t0() + SEC_NS + k as i64 * SEC_NS;
        recs.push(ws(t, 1, 1 + k as u64, trade_frame(t, &[id])));
    }
    // conn 2 opens at 00:00:03.5 and repeats t3 and t4 (later).
    recs.push(ws(
        t0() + 3 * SEC_NS + 500_000_000,
        2,
        1,
        trade_frame(t0() + 3 * SEC_NS, &["t3"]),
    ));
    recs.push(ws(
        t0() + 4 * SEC_NS + 500_000_000,
        2,
        2,
        trade_frame(t0() + 4 * SEC_NS, &["t4", "t6"]),
    ));
    recs.sort_by_key(|r| (r.recv_ns, r.conn_id));
    let l0 = 5 + 1 + 2; // trades in the L0 records
    s.topic("publicTrade", S, recs);
    s.write(&dir);
    let all = build(&dir, &[D]);
    let trades = table(&all, "trade");
    let ids: Vec<&str> = trades
        .iter()
        .map(|(_, r)| r["trade_id"].as_str().expect("id"))
        .collect();
    // Sorted by recv_ns: t6 came at 00:00:04.5, t5 at 00:00:05.
    assert_eq!(ids, ["t1", "t2", "t3", "t4", "t6", "t5"]);
    let by_id = |id: &str| {
        trades
            .iter()
            .find(|(_, r)| r["trade_id"] == id)
            .expect("row")
            .1
    };
    assert_eq!(by_id("t3")["conn_id"], 1, "the earliest recv_ns is kept");
    let h = gaps(&all, "handover");
    assert_eq!(h.len(), 1);
    assert_eq!(
        h[0]["detail"],
        r#"{"dropped_trade_rows":2,"from_conn_id":1,"to_conn_id":2}"#
    );
    // Contract 7: L0 trades = L1 rows + dropped.
    assert_eq!(l0, trades.len() + 2);
}

#[test]
fn trade_dedup_against_the_date_before() {
    let dir = common::test_dir("trade_prev_date");
    let mut s = base();
    s.instruments(S, t0() - 2 * H_NS);
    let before = t0() - SEC_NS;
    s.topic(
        "publicTrade",
        S,
        vec![ws(before, 1, 1, trade_frame(before, &["x1"]))],
    );
    s.topic(
        "publicTrade",
        S,
        vec![ws(
            t0() + SEC_NS,
            1,
            2,
            trade_frame(t0() + SEC_NS, &["x1", "x2"]),
        )],
    );
    s.write(&dir);
    let prev = "2026-02-01";
    let all = build(&dir, &[prev, D]);
    let d: Vec<&str> = table(&all, "trade")
        .iter()
        .filter(|(p, _)| p.contains(&format!("date={D}")))
        .map(|(_, r)| r["trade_id"].as_str().expect("id"))
        .collect();
    assert_eq!(d, ["x2"], "x1 was kept on the date before");
}

#[test]
fn parse_error_rows() {
    let dir = common::test_dir("parse_error");
    let mut s = base();
    let t = |m: i64| t0() + m * MIN_NS;
    let mut bad_topic = book_frame("delta", t(30), 3, &[], &[]);
    bad_topic = String::from_utf8(bad_topic)
        .expect("utf8")
        .replace("orderbook.50.BTCUSDT", "orderbook.50.ETHUSDT")
        .into_bytes();
    let recs = vec![
        ws(
            t(10),
            1,
            1,
            book_frame("snapshot", t(10), 1, &[(4200, 1000)], &[(4201, 1000)]),
        ),
        // 420.05 is not exact at tick 0.10.
        ws(
            t(20),
            1,
            2,
            String::from_utf8(book_frame("delta", t(20), 2, &[(4200, 5)], &[]))
                .expect("utf8")
                .replace("\"420.0\"", "\"420.05\"")
                .into_bytes(),
        ),
        ws(t(30), 1, 3, bad_topic),
        ws(t(70), 1, 4, book_frame("delta", t(70), 3, &[], &[])),
    ];
    s.topic("orderbook.50", S, recs);
    s.write(&dir);
    let all = build(&dir, &[D]);
    let pe = gaps(&all, "parse_error");
    assert_eq!(pe.len(), 2, "{pe:?}");
    let rows = table(&all, "book_msg");
    assert_eq!(
        rows.len(),
        2,
        "no data row for the two bad records, no checkpoint (R3)"
    );
}

#[test]
fn checkpoint_equals_reference_replay() {
    let dir = common::test_dir("checkpoint_reference");
    let mut s = base();
    let mut x: u64 = 7;
    let mut rnd = |n: u64| {
        x = x
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (x >> 33) % n
    };
    let bids: Vec<(i64, i64)> = (0..50).map(|i| (4200 - i, 100 + i)).collect();
    let asks: Vec<(i64, i64)> = (0..50).map(|i| (4201 + i, 100 + i)).collect();
    let mut recs = vec![ws(
        t0() + SEC_NS,
        1,
        1,
        book_frame("snapshot", t0() + SEC_NS, 1, &bids, &asks),
    )];
    let mut reference: BTreeMap<(bool, i64), i64> = BTreeMap::new();
    for (p, q) in &bids {
        reference.insert((true, *p), *q);
    }
    for (p, q) in &asks {
        reference.insert((false, *p), *q);
    }
    let mut want: BTreeMap<i64, Sides> = BTreeMap::new();
    // The last record applied before each hour: (update_id, recv_ns, rec_no).
    let mut last_applied: BTreeMap<i64, (i64, i64, i64)> = BTreeMap::new();
    let mut last = (1i64, t0() + SEC_NS, 0i64);
    let n = 6000i64;
    let mut next_hour = 1;
    for k in 1..=n {
        let t = t0() + SEC_NS + k * (24 * H_NS - 2 * SEC_NS) / n;
        while next_hour < 24 && t >= t0() + next_hour * H_NS {
            let side = |b: bool| -> (Vec<i64>, Vec<i64>) {
                let mut v: Vec<(i64, i64)> = reference
                    .iter()
                    .filter(|((s, _), _)| *s == b)
                    .map(|((_, p), q)| (*p, *q))
                    .collect();
                if b {
                    v.reverse();
                }
                (
                    v.iter().map(|x| x.0).collect(),
                    v.iter().map(|x| x.1).collect(),
                )
            };
            let (bp, bq) = side(true);
            let (ap, aq) = side(false);
            want.insert(next_hour, (bp, bq, ap, aq));
            last_applied.insert(next_hour, last);
            next_hour += 1;
        }
        let bid = rnd(2) == 0;
        let px = if bid {
            4150 + rnd(51) as i64
        } else {
            4201 + rnd(51) as i64
        };
        let qty = if rnd(4) == 0 { 0 } else { 1 + rnd(5000) as i64 };
        if qty == 0 {
            reference.remove(&(bid, px));
        } else {
            reference.insert((bid, px), qty);
        }
        let l = [(px, qty)];
        let (b, a): (Levels, Levels) = if bid { (&l, &[]) } else { (&[], &l) };
        recs.push(ws(t, 1, 1 + k as u64, book_frame("delta", t, 1 + k, b, a)));
        last = (1 + k, t, k);
    }
    s.topic("orderbook.50", S, recs);
    s.write(&dir);
    let all = build(&dir, &[D]);
    let cp = checkpoints(&all);
    assert_eq!(cp.len(), 23, "hours 01..23");
    for (name, row) in cp {
        let h: i64 = name
            .trim_start_matches("hour=")
            .trim_end_matches(".parquet")
            .parse()
            .expect("hour");
        let (bp, bq, ap, aq) = &want[&h];
        let ints = |v: &Value| {
            v.as_array()
                .expect("list")
                .iter()
                .map(|x| x.as_i64().expect("i64"))
                .collect::<Vec<_>>()
        };
        assert_eq!(&ints(&row["bid_px"]), bp, "hour {h} bid_px");
        assert_eq!(&ints(&row["bid_qty"]), bq, "hour {h} bid_qty");
        assert_eq!(&ints(&row["ask_px"]), ap, "hour {h} ask_px");
        assert_eq!(&ints(&row["ask_qty"]), aq, "hour {h} ask_qty");
        assert_eq!(row["recv_ns"], t0() + h * H_NS);
        // Final review F7: the lineage and ids of the last record applied.
        let (u, t, rec) = last_applied[&h];
        assert_eq!(row["update_id"], u, "hour {h} update_id");
        assert_eq!(row["venue_seq"], 1000 + u, "hour {h} venue_seq");
        assert_eq!(
            row["venue_ts_ns"],
            (t / 1_000_000 - 3) * 1_000_000,
            "hour {h} venue_ts_ns"
        );
        assert_eq!(row["rec_no"], rec, "hour {h} rec_no");
        assert_eq!(row["frame_no"], rec + 1, "hour {h} frame_no");
    }
}

#[test]
fn collector_down_one_row_per_topic() {
    let dir = common::test_dir("collector_down_topics");
    let mut s = base();
    let t = |m: i64| t0() + m * MIN_NS;
    // Topics with records on the date before (orderbook) and on the date (trades).
    s.instruments(S, t0() - 3 * H_NS);
    s.topic(
        "orderbook.50",
        S,
        vec![ws(
            t0() - H_NS,
            1,
            1,
            book_frame("snapshot", t0() - H_NS, 1, &[(4200, 1)], &[(4201, 1)]),
        )],
    );
    s.topic(
        "publicTrade",
        S,
        vec![ws(t(5), 2, 1, trade_frame(t(5), &["a"]))],
    );
    s.session(
        D,
        "md_lake",
        t(65),
        vec![note(
            t(65),
            json!({
                "note": "collector_down", "writer": "md_lake", "conn_id": null, "topics": null,
                "start_recv_ns": t(60), "end_recv_ns": t(120),
                "detail": {"prev_session_ns": S, "next_session_ns": t(120)}
            }),
        )],
    );
    s.write(&dir);
    let all = build(&dir, &["2026-02-01", D]);
    let mut topics: Vec<String> = gaps(&all, "collector_down")
        .iter()
        .map(|r| {
            format!(
                "{}/{}",
                r["channel"].as_str().expect("c"),
                r["symbol"].as_str().expect("s")
            )
        })
        .collect();
    topics.sort();
    assert_eq!(
        topics,
        [
            "instruments/_all",
            "orderbook.50/BTCUSDT",
            "publicTrade/BTCUSDT"
        ]
    );
}

#[test]
fn note_topics_and_state_ends() {
    let dir = common::test_dir("note_topics");
    let mut s = base();
    let t = |m: i64| t0() + m * MIN_NS;
    let mut recs = vec![ws(
        t(1),
        1,
        1,
        book_frame("snapshot", t(1), 1, &[(4200, 1)], &[(4201, 1)]),
    )];
    for k in 1..300i64 {
        recs.push(ws(
            t(1 + k),
            1,
            1 + k as u64,
            book_frame("delta", t(1 + k), 1 + k, &[(4200, k)], &[]),
        ));
    }
    s.topic("orderbook.50", S, recs);
    s.topic(
        "publicTrade",
        S,
        vec![ws(t(2), 2, 1, trade_frame(t(2), &["a"]))],
    );
    let topics = json!([{"market": "linear", "channel": "orderbook.50", "symbol": "BTCUSDT"}]);
    let n = |kind: &str, at: i64, conn: Value, topics: Value| {
        note(
            t(at),
            json!({"note": kind, "writer": "md1", "conn_id": conn, "topics": topics,
                           "start_recv_ns": t(at), "end_recv_ns": t(at) + SEC_NS, "detail": {}}),
        )
    };
    s.session(
        D,
        "md1",
        S,
        vec![
            n("clock_step", 30, json!(1), Value::Null),
            n("overflow", 40, json!(2), Value::Null),
            // disk_floor at 02:10 ends the book state: no checkpoint at 03:00.
            n("disk_floor", 130, Value::Null, topics.clone()),
            // stale at 04:30 alone would leave 03:00 and 04:00 checkpoints.
            n("stale", 270, Value::Null, topics),
        ],
    );
    s.write(&dir);
    let all = build(&dir, &[D]);
    let row = |k: &str| {
        let g = gaps(&all, k);
        assert_eq!(g.len(), 1, "{k}: {g:?}");
        g[0].clone()
    };
    assert_eq!(
        row("clock_step")["channel"],
        "orderbook.50",
        "null topics + conn_id: the connection's topics"
    );
    assert_eq!(row("overflow")["channel"], "publicTrade");
    assert_eq!(row("disk_floor")["symbol"], "BTCUSDT");
    assert_eq!(row("stale")["conn_id"], Value::Null);
    let hours: Vec<String> = checkpoints(&all).into_iter().map(|(h, _)| h).collect();
    assert_eq!(
        hours,
        ["hour=01.parquet", "hour=02.parquet"],
        "disk_floor at 02:10 ends the state"
    );
}

/// R2: a day-change connection subscribed while the old one was alive, but
/// whose snapshot came after the old one's last record, is a handover.
#[test]
fn late_snapshot_is_still_a_handover() {
    let dir = common::test_dir("late_handover");
    let mut s = base();
    let mut recs = vec![ws(
        t0() + SEC_NS,
        1,
        1,
        book_frame("snapshot", t0() + SEC_NS, 1, &[(4200, 1)], &[(4201, 1)]),
    )];
    for k in 1..10i64 {
        let t = t0() + SEC_NS + k * SEC_NS;
        recs.push(ws(
            t,
            1,
            1 + k as u64,
            book_frame("delta", t, 1 + k, &[(4200, 1 + k)], &[]),
        ));
    }
    let snap = t0() + 12 * SEC_NS;
    recs.push(ws(
        snap,
        2,
        2,
        book_frame("snapshot", snap, 10, &[(4200, 10)], &[(4201, 1)]),
    ));
    for k in 1..=80i64 {
        let t = snap + k * MIN_NS;
        recs.push(ws(
            t,
            2,
            2 + k as u64,
            book_frame("delta", t, 10 + k, &[], &[(4201, k)]),
        ));
    }
    s.topic("orderbook.50", S, recs);
    // conn 2's subscribe answer at 00:00:05, in the collector's session file.
    let ans = br#"{"success":true,"ret_msg":"","conn_id":"x","op":"subscribe"}"#.to_vec();
    s.session(D, "md1", S, vec![ws(t0() + 5 * SEC_NS, 2, 1, ans)]);
    s.write(&dir);
    let all = build(&dir, &[D]);
    assert_eq!(
        gaps(&all, "handover").len(),
        1,
        "a handover, not a reconnect"
    );
    assert!(gaps(&all, "reconnect").is_empty());
    let cp = checkpoints(&all);
    assert_eq!(cp.len(), 1);
    assert_eq!(cp[0].1["conn_id"], 2);
}

/// R9: when the instrument row of the book changes exponents, the book
/// state ends; levels of two exponents never mix.
#[test]
fn exponent_change_ends_the_book_state() {
    let dir = common::test_dir("exponent_change");
    let mut s = base();
    let finer = String::from_utf8(common::synth::instruments_body())
        .expect("utf8")
        .replace(r#""tickSize":"0.10""#, r#""tickSize":"0.01""#)
        .into_bytes();
    s.instruments_body(S, t0() + 12 * H_NS, finer);
    let mut recs = vec![ws(
        t0() + SEC_NS,
        1,
        1,
        book_frame("snapshot", t0() + SEC_NS, 1, &[(4200, 1)], &[(4201, 1)]),
    )];
    for k in 1..=20i64 {
        let t = t0() + k * H_NS + MIN_NS;
        recs.push(ws(
            t,
            1,
            1 + k as u64,
            book_frame("delta", t, 1 + k, &[(4200, k)], &[]),
        ));
    }
    s.topic("orderbook.50", S, recs);
    s.write(&dir);
    let all = build(&dir, &[D]);
    let hours: Vec<String> = checkpoints(&all).into_iter().map(|(h, _)| h).collect();
    let want: Vec<String> = (1..=12).map(|h| format!("hour={h:02}.parquet")).collect();
    assert_eq!(
        hours, want,
        "the 12:01 delta has px_exp -2: no checkpoint after it"
    );
}

/// Final review F1: when the followed connection ends while another
/// connection already sends the topic (its deltas are not applied to this
/// book), the book state ends; a book never freezes while rows go missing.
#[test]
fn state_ends_when_the_other_connection_already_sends() {
    let dir = common::test_dir("chain_of_overlaps");
    let mut s = base();
    let t = |m: i64| t0() + m * MIN_NS;
    let mut recs = vec![ws(
        t(1),
        1,
        1,
        book_frame("snapshot", t(1), 1, &[(4200, 1)], &[(4201, 1)]),
    )];
    for k in 1..30i64 {
        recs.push(ws(
            t(1 + k),
            1,
            1 + k as u64,
            book_frame("delta", t(1 + k), 1 + k, &[(4200, k)], &[]),
        ));
    }
    // conn 2 sends deltas only (no snapshot) from 00:20 to 00:25.
    for k in 0..=5i64 {
        recs.push(ws(
            t(20 + k),
            2,
            1 + k as u64,
            book_frame("delta", t(20 + k), 900 + k, &[], &[(4201, k)]),
        ));
    }
    recs.sort_by_key(|r| (r.recv_ns, r.conn_id));
    s.topic("orderbook.50", S, recs);
    // conn 2 stays alive (pongs in the session file) until 02:30, but its
    // deltas of 00:20–00:25 were never applied to conn 1's book.
    let pongs = (0..=12)
        .map(|n| {
            ws(
                t(30 + 10 * n),
                2,
                200 + n as u64,
                br#"{"op":"pong"}"#.to_vec(),
            )
        })
        .collect();
    s.session(D, "md1", S, pongs);
    s.write(&dir);
    let all = build(&dir, &[D]);
    let cp = checkpoints(&all);
    let hours: Vec<&str> = cp.iter().map(|(h, _)| h.as_str()).collect();
    assert!(hours.is_empty(), "no frozen checkpoint: {hours:?}");
}

/// Final review F1: a pending connection that sends a delta before any
/// snapshot ends the book state (the book would miss that message).
#[test]
fn delta_on_pending_connection_ends_the_state() {
    let dir = common::test_dir("pending_delta");
    let mut s = base();
    let t = |m: i64| t0() + m * MIN_NS;
    let mut recs = vec![ws(
        t(1),
        1,
        1,
        book_frame("snapshot", t(1), 1, &[(4200, 1)], &[(4201, 1)]),
    )];
    for k in 1..30i64 {
        recs.push(ws(
            t(1 + k),
            1,
            1 + k as u64,
            book_frame("delta", t(1 + k), 1 + k, &[(4200, k)], &[]),
        ));
    }
    for k in 0..=120i64 {
        recs.push(ws(
            t(31 + k),
            2,
            2 + k as u64,
            book_frame("delta", t(31 + k), 900 + k, &[], &[(4201, k)]),
        ));
    }
    s.topic("orderbook.50", S, recs);
    let ans = br#"{"success":true,"op":"subscribe"}"#.to_vec();
    s.session(D, "md1", S, vec![ws(t(20), 2, 1, ans)]);
    s.write(&dir);
    let all = build(&dir, &[D]);
    let hours: Vec<String> = checkpoints(&all).into_iter().map(|(h, _)| h).collect();
    assert!(hours.is_empty(), "no frozen checkpoint: {hours:?}");
}
