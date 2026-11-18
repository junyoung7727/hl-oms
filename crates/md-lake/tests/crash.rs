//! Hour-seal restart rules (lake contract 2.5): a crash at any step, with
//! torn writes of everything not yet fsynced, must end, after a restart,
//! with every record exactly once and no open file left.

mod common;

use std::sync::Arc;

use common::kit::{self, FixedClock, S, T14, classify, md};
use common::sim::SimFs;
use md_lake::fs::Fs;
use md_lake::qraw::{Record, Src};
use md_lake::seal::{Layout, SealError, SealSettings, Sealer};
use md_lake::time;

const D: &str = "2026-01-02";
const NOW: i64 = T14 + time::HOUR_NS + 60 * S;

fn settings(layout: Layout) -> SealSettings {
    let mut s = SealSettings::new("bybit", "md_sealer", "0.1.0", "test", T14);
    s.layout = layout;
    s
}

/// Two sessions, three connections, two topics, one unroutable record (of
/// 201), and one crashed file. Returns every record written.
fn scenario(fs: &Arc<dyn Fs>) -> Vec<Record> {
    let (s1, s2) = (T14 - S, T14 + 30 * 60 * S);
    let mut all = Vec::new();
    let mut c1: Vec<Record> = (0..80)
        .map(|i| {
            md(
                T14 + i * S,
                1,
                i as u64,
                if i % 3 == 0 {
                    "publicTrade.BTCUSDT"
                } else {
                    "orderbook.50.BTCUSDT"
                },
            )
        })
        .collect();
    c1.push(kit::control(T14 + 81 * S, 1, 80));
    let c2: Vec<Record> = (0..60)
        .map(|i| md(T14 + i * S + S / 2, 2, i as u64, "orderbook.50.ETHUSDT"))
        .collect();
    let c3: Vec<Record> = (0..60)
        .map(|i| md(s2 + S + i * S, 1, i as u64, "orderbook.50.BTCUSDT"))
        .collect();
    kit::write_open(fs, "md1", s1, 1, &c1, false);
    kit::write_open(fs, "md1", s1, 2, &c2, true);
    kit::write_open(fs, "md1", s2, 1, &c3, false);
    all.extend(c1);
    all.extend(c2);
    all.extend(c3);
    all
}

fn seal(fs: &Arc<dyn Fs>, layout: Layout) -> Result<md_lake::seal::SealReport, SealError> {
    let clock = FixedClock::at(NOW);
    Sealer::new(fs.clone(), settings(layout)).seal(clock.as_ref(), &classify)
}

/// Every record of the lake's sealed targets of the hour (day files and the
/// sealer's session file; notes left out), sorted.
fn lake_records(fs: &dyn Fs) -> Vec<Record> {
    let log = kit::seal_log(fs, D, 14);
    let done = log.last().expect("a done entry");
    assert_eq!(done["entry"], "done");
    let mut out = Vec::new();
    for t in done["targets"].as_array().expect("targets") {
        let p = t["path"].as_str().expect("path");
        if t["records"].as_u64() == Some(0) && !fs.exists(p) {
            continue;
        }
        let (records, _) = kit::day_records(fs, p);
        out.extend(records.into_iter().filter(|r| r.src != Src::Note));
    }
    out.sort_by(|a, b| {
        (a.conn_id, a.recv_ns, a.frame_no, &a.payload)
            .cmp(&(b.conn_id, b.recv_ns, b.frame_no, &b.payload))
    });
    out
}

fn sorted(mut v: Vec<Record>) -> Vec<Record> {
    v.sort_by(|a, b| {
        (a.conn_id, a.recv_ns, a.frame_no, &a.payload)
            .cmp(&(b.conn_id, b.recv_ns, b.frame_no, &b.payload))
    });
    v
}

fn crash_at_every_step(layout: Layout) {
    // The calls of one clean seal, to know how many steps there are.
    let (sim, fs) = SimFs::new();
    let want = sorted(scenario(&fs));
    sim.persist_all();
    sim.fail_after(usize::MAX);
    seal(&fs, layout).expect("clean seal");
    let steps = sim.calls();
    assert!(steps > 20, "{steps} steps");
    assert_eq!(lake_records(fs.as_ref()), want);

    for n in 1..=steps {
        let (sim, fs) = SimFs::new();
        scenario(&fs);
        sim.persist_all();
        sim.fail_after(n);
        let first = seal(&fs, layout);
        assert!(first.is_err(), "step {n}: the crash is reported");
        sim.reboot(n as u64);
        let again = seal(&fs, layout);
        assert!(again.is_ok(), "step {n}: restart: {again:?}");
        assert_eq!(
            lake_records(fs.as_ref()),
            want,
            "step {n}: every record exactly once"
        );
        assert!(
            kit::open_files(fs.as_ref(), D, 14).is_empty(),
            "step {n}: no open file left"
        );
        let notes: Vec<serde_json::Value> = sealer_notes(fs.as_ref());
        assert_eq!(
            notes
                .iter()
                .filter(|n| n["note"] == "crash_truncate")
                .count(),
            1,
            "step {n}: one crash_truncate note"
        );
    }
}

fn sealer_notes(fs: &dyn Fs) -> Vec<serde_json::Value> {
    let dir = format!("crypto/l0/venue=bybit/_session/date={D}");
    let mut out = Vec::new();
    for name in fs.list(&dir).expect("list") {
        let (records, _) = kit::day_records(fs, &format!("{dir}/{name}"));
        for r in records.iter().filter(|r| r.src == Src::Note) {
            out.push(serde_json::from_slice(&r.payload).expect("note"));
        }
    }
    out
}

#[test]
fn crash_at_every_step_ends_with_every_record_once() {
    crash_at_every_step(Layout::BySymbol);
}

#[test]
fn done_with_an_unknown_source_stops() {
    let (sim, fs) = SimFs::new();
    scenario(&fs);
    sim.persist_all();
    seal(&fs, Layout::BySymbol).expect("seal");
    // A file appears in the sealed hour that the done entry does not name.
    kit::write_open(
        &fs,
        "md9",
        T14,
        4,
        &[md(T14 + S, 4, 0, "orderbook.50.BTCUSDT")],
        false,
    );
    let before = sim.files();
    let err = seal(&fs, Layout::BySymbol).expect_err("unknown source");
    assert!(matches!(err, SealError::UnknownSource { .. }), "{err:?}");
    assert_eq!(sim.files(), before, "nothing changed");
}

#[test]
fn torn_last_seal_log_line_is_cut() {
    let (sim, fs) = SimFs::new();
    let want = sorted(scenario(&fs));
    sim.persist_all();
    // Crash inside the write of the begin entry: a torn line.
    sim.fail_after(1);
    let path = format!("crypto/l0/_seal/venue=bybit/date={D}/hour=14.json");
    drop(seal(&fs, Layout::BySymbol));
    sim.reboot(0);
    let mut f = fs.create_new(&path).expect("log");
    f.write_all(b"{\"entry\":\"begin\",\"att").expect("torn");
    f.sync().expect("sync");
    drop(f);
    sim.persist_all();
    seal(&fs, Layout::BySymbol).expect("seal after a torn line");
    let log = kit::seal_log(fs.as_ref(), D, 14);
    assert_eq!(log.len(), 2, "the torn line is gone, then begin and done");
    assert_eq!(lake_records(fs.as_ref()), want);
}

#[test]
fn crash_at_every_step_by_connection() {
    crash_at_every_step(Layout::ByConnection);
}
