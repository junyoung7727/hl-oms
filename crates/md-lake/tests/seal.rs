//! The hour seal, layout by_symbol (lake contract 2.5, 2.6). Every record of
//! an open file must appear exactly once in the day files, in the order the
//! contract fixes, and nothing may be touched that a collector still holds.

mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use sha2::{Digest, Sha256};

use common::kit::{self, FixedClock, S, T14, classify, md};
use md_lake::fs::{Fs, RealFs};
use md_lake::qraw::{Record, Src};
use md_lake::seal::{SealError, SealSettings, Sealer};
use md_lake::time;

const D: &str = "2026-01-02";
const OB: &str = "orderbook.50.BTCUSDT";
const TR: &str = "publicTrade.BTCUSDT";

fn settings() -> SealSettings {
    SealSettings::new("bybit", "md_sealer", "0.1.0", "test", T14)
}

fn lake(name: &str) -> Arc<dyn Fs> {
    RealFs::new(common::test_dir(name))
}

/// Seals with the clock at `now`.
fn seal(
    fs: &Arc<dyn Fs>,
    s: SealSettings,
    now: i64,
) -> Result<md_lake::seal::SealReport, SealError> {
    let clock = FixedClock::at(now);
    Sealer::new(fs.clone(), s).seal(clock.as_ref(), &classify)
}

#[test]
fn lossless_move_and_session_runs() {
    let fs = lake("seal-lossless");
    let s1 = T14 - 2 * time::HOUR_NS;
    let s2 = T14 + 20 * 60 * S;
    // Session 1: two connections; conn 2 has a clock step back (recv_ns 14:00:05
    // after 14:00:07). Session 2 (a restart at 14:20) reconnects conn 1.
    let c1 = vec![
        md(T14 + S, 1, 0, OB),
        md(T14 + 3 * S, 1, 1, TR),
        md(T14 + 6 * S, 1, 2, OB),
    ];
    let c2 = vec![
        md(T14 + 2 * S, 2, 0, OB),
        md(T14 + 7 * S, 2, 1, OB),
        md(T14 + 5 * S, 2, 2, OB),
        md(T14 + 9 * S, 2, 3, TR),
    ];
    let c1b = vec![
        md(T14 + 21 * 60 * S, 1, 0, OB),
        md(T14 + 22 * 60 * S, 1, 1, TR),
    ];
    kit::write_open(&fs, "md1", s1, 1, &c1, false);
    kit::write_open(&fs, "md1", s1, 2, &c2, false);
    kit::write_open(&fs, "md1", s2, 1, &c1b, false);
    // An older session s0 that still ran (a fault the contract keeps): its
    // record at 14:00:04 lies between session 1's, but its run comes first.
    let s0 = s1 - time::HOUR_NS;
    let c0 = vec![md(T14 + 4 * S, 1, 0, OB)];
    kit::write_open(&fs, "md0", s0, 1, &c0, false);

    let report = seal(&fs, settings(), T14 + time::HOUR_NS + 60 * S).expect("seal");
    assert_eq!(report.sealed, vec![(D.to_string(), 14)]);
    assert!(
        kit::open_files(fs.as_ref(), D, 14).is_empty(),
        "open files deleted"
    );

    let (ob, frames) = kit::day_records(fs.as_ref(), &kit::day_path("orderbook.50", "BTCUSDT", D));
    assert_eq!(frames, 1);
    // Run of session 1 merged by recv_ns, conn 2 keeping its own order
    // (14:00:07 before 14:00:05), then the run of session 2.
    let want_ob = vec![
        c0[0].clone(),
        c1[0].clone(),
        c2[0].clone(),
        c1[2].clone(),
        c2[1].clone(),
        c2[2].clone(),
        c1b[0].clone(),
    ];
    assert_eq!(ob, want_ob);
    let (tr, _) = kit::day_records(fs.as_ref(), &kit::day_path("publicTrade", "BTCUSDT", D));
    assert_eq!(tr, vec![c1[1].clone(), c2[3].clone(), c1b[1].clone()]);

    let log = kit::seal_log(fs.as_ref(), D, 14);
    assert_eq!(log.len(), 2);
    assert_eq!(
        (log[0]["entry"].as_str(), log[1]["entry"].as_str()),
        (Some("begin"), Some("done"))
    );
    let done = &log[1];
    assert_eq!(done["records"], 10);
    assert_eq!(done["unroutable_records"], 0);
    let ob_target = done["targets"]
        .as_array()
        .expect("targets")
        .iter()
        .find(|t| {
            t["path"]
                .as_str()
                .is_some_and(|p| p.contains("orderbook.50"))
        })
        .expect("ob target");
    assert_eq!(
        ob_target["sessions"],
        serde_json::json!([
            {"collector": "md0", "session_ns": s0, "records": 1},
            {"collector": "md1", "session_ns": s1, "records": 5},
            {"collector": "md1", "session_ns": s2, "records": 1}
        ])
    );
    common::schema::assert_valid("l0/seal_log_entry.schema.json", &log[0]);
    common::schema::assert_valid("l0/seal_log_entry.schema.json", &log[1]);
}

#[test]
fn golden_restart_hour_is_reproduced() {
    // 2026-01-05 09: session A's file ends without a frame end (A crashed at
    // 09:30), session B's file is complete. Rebuild both open files from the
    // golden sealed records and the golden session runs.
    let fs = lake("seal-golden");
    let id = "venue=bybit/market=linear/channel=orderbook.50/date=2026-01-05/BTCUSDT";
    let segments = common::golden_segments(id);
    let refs: Vec<&[u8]> = segments.iter().map(Vec::as_slice).collect();
    let records = md_lake::qraw::decode(&refs).expect("golden").records;
    let runs: serde_json::Value = serde_json::from_slice(
        &std::fs::read(common::golden_dir().join("l0/session_runs.json")).expect("runs"),
    )
    .expect("json");
    let hour9 = time::date_start("2026-01-05").expect("date") + 9 * time::HOUR_NS;
    let mut by_session: BTreeMap<i64, Vec<Record>> = BTreeMap::new();
    for run in runs[id].as_array().expect("runs of the file") {
        let first = run["first_rec_no"].as_u64().expect("first") as usize;
        let n = run["records"].as_u64().expect("records") as usize;
        let ns = run["session_ns"].as_i64().expect("session");
        for r in &records[first..first + n] {
            if time::hour_start(r.recv_ns) == hour9 {
                by_session.entry(ns).or_default().push(r.clone());
            }
        }
    }
    let sessions: Vec<i64> = by_session.keys().copied().collect();
    assert_eq!(
        sessions,
        vec![1_767_604_198_000_000_000, 1_767_605_403_000_000_000]
    );
    kit::write_open(&fs, "md1", sessions[0], 1, &by_session[&sessions[0]], true);
    kit::write_open(&fs, "md1", sessions[1], 1, &by_session[&sessions[1]], false);

    let mut s = SealSettings::new("bybit", "md_sealer", "0.1.0", "golden", hour9);
    s.sealer_start_ns = Some(1_767_607_440_000_000_000); // golden sealer process start
    let clock = FixedClock::at(1_767_607_500_000_000_000); // 10:05:00, the golden note time
    let report = Sealer::new(fs.clone(), s)
        .seal(clock.as_ref(), &classify)
        .expect("seal");
    assert!(report.sealed.contains(&("2026-01-05".to_string(), 9)));

    let golden: Vec<serde_json::Value> = std::fs::read_to_string(
        common::golden_dir().join("xrepo/seal_log/date=2026-01-05/hour=09.json"),
    )
    .expect("golden log")
    .lines()
    .map(|l| serde_json::from_str(l).expect("line"))
    .collect();
    let ours = kit::seal_log(fs.as_ref(), "2026-01-05", 9);
    let done = |log: &[serde_json::Value]| log.last().expect("done").clone();
    let pick = |d: &serde_json::Value| -> Vec<(
        String,
        serde_json::Value,
        serde_json::Value,
        serde_json::Value,
    )> {
        d["targets"]
            .as_array()
            .expect("targets")
            .iter()
            .map(|t| {
                (
                    t["path"].as_str().expect("path").to_string(),
                    t["records"].clone(),
                    t["sha256"].clone(),
                    t["sessions"].clone(),
                )
            })
            .collect()
    };
    assert_eq!(pick(&done(&ours)), pick(&done(&golden)));
    assert_eq!(done(&ours)["records"], done(&golden)["records"]);
    // The crash_truncate note, byte for byte, against the golden session file.
    let note_path = "crypto/l0/venue=bybit/_session/date=2026-01-05/md_sealer-1767607440000000000.qraw.zst.partial";
    let (notes, _) = kit::day_records(fs.as_ref(), note_path);
    let gseg = common::golden_segments(
        "venue=bybit/_session/date=2026-01-05/md_sealer-1767607440000000000",
    );
    let grefs: Vec<&[u8]> = gseg.iter().map(Vec::as_slice).collect();
    let gnotes = md_lake::qraw::decode(&grefs).expect("golden notes").records;
    assert_eq!(notes, gnotes);
    for line in &ours {
        common::schema::assert_valid("l0/seal_log_entry.schema.json", line);
    }
}

#[test]
fn sealer_session_file_is_always_a_target() {
    let fs = lake("seal-session-target");
    kit::write_open(&fs, "md1", T14 - S, 1, &[md(T14 + S, 1, 0, OB)], false);
    seal(&fs, settings(), T14 + time::HOUR_NS + S).expect("seal");
    let log = kit::seal_log(fs.as_ref(), D, 14);
    let sess = |e: &serde_json::Value| -> serde_json::Value {
        e["targets"]
            .as_array()
            .expect("targets")
            .iter()
            .find(|t| t["path"].as_str().is_some_and(|p| p.contains("/_session/")))
            .cloned()
            .expect("the sealer's session file is a target")
    };
    let (b, d) = (sess(&log[0]), sess(&log[1]));
    assert_eq!(b["offset_before"], 0);
    assert_eq!(
        (d["offset_after"].as_i64(), d["records"].as_i64()),
        (Some(0), Some(0))
    );
    // The SHA-256 of no bytes (a literal would trip the secret scan).
    assert_eq!(d["sha256"], hex::encode(Sha256::digest(b"")));
    assert!(
        !fs.exists(d["path"].as_str().expect("path")),
        "no file for nothing"
    );
    // A new day file's first frame holds the header too: it decodes.
    let (ob, _) = kit::day_records(fs.as_ref(), &kit::day_path("orderbook.50", "BTCUSDT", D));
    assert_eq!(ob.len(), 1);
}

#[test]
fn unroutable_guard() {
    // 2 % unroutable market data: the run stops before writing anything.
    let fs = lake("seal-unroutable-2");
    let mut recs: Vec<Record> = (0..98).map(|i| md(T14 + i * S, 1, i as u64, OB)).collect();
    recs.push(kit::control(T14 + 98 * S, 1, 98));
    recs.push(kit::control(T14 + 99 * S, 1, 99));
    kit::write_open(&fs, "md1", T14 - S, 1, &recs, false);
    let before = kit::open_files(fs.as_ref(), D, 14);
    let err = seal(&fs, settings(), T14 + time::HOUR_NS + S).expect_err("guard");
    assert!(matches!(err, SealError::Unroutable { .. }), "{err:?}");
    assert_eq!(kit::open_files(fs.as_ref(), D, 14), before);
    assert!(kit::seal_log(fs.as_ref(), D, 14).is_empty());
    assert!(!fs.exists(&kit::day_path("orderbook.50", "BTCUSDT", D)));

    // 0.5 %: sealed; the record goes to the sealer's session file and is counted.
    let fs = lake("seal-unroutable-05");
    let mut recs: Vec<Record> = (0..199)
        .map(|i| md(T14 + i * S / 10, 1, i as u64, OB))
        .collect();
    recs.push(kit::control(T14 + 20 * S, 1, 199));
    kit::write_open(&fs, "md1", T14 - S, 1, &recs, false);
    seal(&fs, settings(), T14 + time::HOUR_NS + S).expect("seal");
    let log = kit::seal_log(fs.as_ref(), D, 14);
    assert_eq!(log[1]["unroutable_records"], 1);
    assert_eq!(log[1]["records"], 200);
    let sess = log[1]["targets"]
        .as_array()
        .expect("targets")
        .iter()
        .find(|t| t["path"].as_str().is_some_and(|p| p.contains("/_session/")))
        .expect("session target")
        .clone();
    let (records, _) = kit::day_records(fs.as_ref(), sess["path"].as_str().expect("path"));
    assert_eq!(records, vec![kit::control(T14 + 20 * S, 1, 199)]);
}

#[test]
fn hour_without_open_files_and_session_gap() {
    let fs = lake("seal-empty-and-gap");
    // Hour 14: nothing. Hour 15: session 1 ends 15:59:50; session 2 starts
    // 16:00:20 (30 s later, across the hour boundary).
    let h15 = T14 + time::HOUR_NS;
    let h16 = T14 + 2 * time::HOUR_NS;
    let (s1, s2) = (h15 - 5 * S, h16 + 19 * S);
    kit::write_open(
        &fs,
        "md1",
        s1,
        1,
        &[md(h15 + 10 * S, 1, 0, OB), md(h16 - 10 * S, 1, 1, OB)],
        false,
    );
    kit::write_open(&fs, "md1", s2, 1, &[md(h16 + 20 * S, 1, 0, OB)], false);
    let report = seal(&fs, settings(), h16 + time::HOUR_NS + S).expect("seal");
    assert_eq!(
        report.sealed,
        vec![
            (D.to_string(), 14),
            (D.to_string(), 15),
            (D.to_string(), 16)
        ]
    );

    // U6 D9: the whole-hour note is listed for the seal's alert; the
    // session-gap note is not a whole hour.
    assert_eq!(report.collector_down_hours, vec![(D.to_string(), 14)]);
    let empty = kit::seal_log(fs.as_ref(), D, 14);
    assert_eq!(empty[1]["records"], 0);
    assert_eq!(empty[0]["sources"], serde_json::json!([]));
    let notes = sealer_notes(fs.as_ref());
    let down: Vec<&serde_json::Value> = notes
        .iter()
        .filter(|n| n["note"] == "collector_down")
        .collect();
    assert_eq!(down.len(), 2, "{notes:?}");
    // The empty hour's note, in the golden form (hour 20 of 2026-01-01).
    assert_eq!(down[0]["start_recv_ns"], T14);
    assert_eq!(down[0]["end_recv_ns"], serde_json::Value::Null);
    assert_eq!(
        down[0]["detail"],
        serde_json::json!({"text": "no open file in the hour"})
    );
    assert_eq!(down[1]["start_recv_ns"], h16 - 10 * S);
    assert_eq!(down[1]["end_recv_ns"], h16 + 20 * S);
    assert_eq!(
        down[1]["detail"],
        serde_json::json!({"prev_session_ns": s1, "next_session_ns": s2})
    );
}

/// The notes of the sealer's session file of `D`.
fn sealer_notes(fs: &dyn Fs) -> Vec<serde_json::Value> {
    let dir = format!("crypto/l0/venue=bybit/_session/date={D}");
    let mut out = Vec::new();
    for name in fs.list(&dir).expect("list") {
        if name.starts_with("md_sealer-") {
            let (records, _) = kit::day_records(fs, &format!("{dir}/{name}"));
            for r in records.iter().filter(|r| r.src == Src::Note) {
                out.push(serde_json::from_slice(&r.payload).expect("note json"));
            }
        }
    }
    out
}

#[test]
fn recovery_of_an_unlocked_crashed_file() {
    use md_lake::writer::{OpenWriter, WriterSettings};
    // 4,000 small records: a block flush after record 1,000, then enough
    // input that zstd emits blocks without a flush; then the process dies.
    let recs: Vec<Record> = (0..4000)
        .map(|i| md(T14 + i * S / 2000, 1, i as u64, OB))
        .collect();
    let fs0 = lake("seal-recover-src");
    let mut w = OpenWriter::new(
        fs0.clone(),
        WriterSettings::new("bybit", "md1", "0.1.0", "t", T14 - S),
        1,
        &Default::default(),
    );
    for r in &recs[..1000] {
        w.append(r).expect("append");
    }
    w.tick(T14 + 2 * S).expect("flush");
    let open = w.current_path().expect("open").to_string();
    let flushed = kit::read_all(fs0.as_ref(), &open).len();
    for r in &recs[1000..] {
        w.append(r).expect("append");
    }
    drop(w); // crash: no frame end
    let crashed = kit::read_all(fs0.as_ref(), &open);
    assert!(
        crashed.len() > flushed + 1000,
        "zstd emitted blocks after the flush"
    );
    for seed in [1usize, 7, 42, 99, 500] {
        let fs = lake(&format!("seal-recover-{seed}"));
        let cut = flushed + (seed * 7919) % (crashed.len() - flushed);
        let mut f = fs.create_new(&open).expect("create");
        f.write_all(&crashed[..cut]).expect("write");
        drop(f);
        seal(&fs, settings(), T14 + time::HOUR_NS + S).expect("seal");
        let (got, _) = kit::day_records(fs.as_ref(), &kit::day_path("orderbook.50", "BTCUSDT", D));
        assert!(
            got.len() >= 1000,
            "seed {seed}: no flushed record lost ({})",
            got.len()
        );
        assert_eq!(
            got[..],
            recs[..got.len()],
            "seed {seed}: a prefix, nothing invented"
        );
        let notes = sealer_notes(fs.as_ref());
        let ct: Vec<_> = notes
            .iter()
            .filter(|n| n["note"] == "crash_truncate")
            .collect();
        assert_eq!(ct.len(), 1, "seed {seed}");
        assert_eq!(ct[0]["start_recv_ns"], got.last().expect("records").recv_ns);
        assert_eq!(ct[0]["detail"]["file"], open);
    }
}

#[test]
fn locked_file_is_never_touched() {
    let dir = common::test_dir("seal-locked");
    let fs = RealFs::new(&dir);
    let h15 = T14 + time::HOUR_NS;
    kit::write_open(&fs, "md1", T14 - S, 1, &[md(T14 + S, 1, 0, OB)], false);
    kit::write_open(&fs, "md1", T14 - S, 1, &[md(h15 + S, 1, 1, OB)], false);
    let p14 = format!(
        "crypto/l0/_open/venue=bybit/date={D}/hour=14/{}",
        kit::open_files(fs.as_ref(), D, 14)[0]
    );
    let bytes_before = kit::read_all(fs.as_ref(), &p14);
    let holder = common::lock_holder::hold(&dir.join(&p14));
    let (cfs, counts) = common::counting::CountingFs::open_at(&dir);
    let report = seal(&cfs, settings(), h15 + time::HOUR_NS + S).expect("seal");
    assert!(report.sealed.is_empty(), "14 waits, so 15 waits too");
    assert_eq!(report.waiting_on.as_deref(), Some(p14.as_str()));
    assert!(
        !counts
            .reads
            .lock()
            .expect("reads")
            .iter()
            .any(|p| p == &p14),
        "never read"
    );
    assert_eq!(kit::read_all(fs.as_ref(), &p14), bytes_before, "unchanged");
    drop(holder);
    let report = seal(&fs, settings(), h15 + time::HOUR_NS + S).expect("seal");
    assert_eq!(
        report.sealed,
        vec![(D.to_string(), 14), (D.to_string(), 15)]
    );
}

#[test]
fn one_seal_run_per_venue() {
    let dir = common::test_dir("seal-one-run");
    let fs = RealFs::new(&dir);
    let _held = common::lock_holder::hold(&dir.join("crypto/l0/_seal/venue=bybit"));
    let err = seal(&fs, settings(), T14 + time::HOUR_NS + S).expect_err("busy");
    assert!(matches!(err, SealError::Busy), "{err:?}");
}

#[test]
fn first_hour_setting() {
    let fs = lake("seal-first-hour");
    let mut s = settings();
    s.first_hour_ns = T14 + time::HOUR_NS;
    let report = seal(&fs, s, T14 + 3 * time::HOUR_NS).expect("seal");
    assert_eq!(
        report.sealed,
        vec![(D.to_string(), 15), (D.to_string(), 16)]
    );
    assert!(kit::seal_log(fs.as_ref(), D, 14).is_empty());
}

#[test]
fn grouping_respects_the_budget_and_slots() {
    let fs = lake("seal-groups");
    // Two connections (two slots), 6 topics each, about 64 KiB per topic.
    let pad = "x".repeat(600);
    for conn in 1..=2u32 {
        let mut recs = Vec::new();
        for i in 0..600u64 {
            let sym = format!("S{conn}X{}", i % 6);
            let mut r = md(
                T14 + i as i64 * S / 100,
                conn,
                i,
                &format!("orderbook.50.{sym}"),
            );
            r.payload =
                format!("{{\"topic\":\"orderbook.50.{sym}\",\"pad\":\"{pad}\"}}").into_bytes();
            recs.push(r);
        }
        kit::write_open(&fs, "md1", T14 - S, conn, &recs, false);
    }
    let mut s = settings();
    // Each topic holds about 65 KB: its context is of the 128 KiB class.
    let one = md_lake::zst::context_bytes(9, Some(100_000));
    s.context_budget = 4 * one;
    let report = seal(&fs, s, T14 + time::HOUR_NS + S).expect("seal");
    assert!(
        report.max_live_context_bytes <= 4 * one,
        "{} > {}",
        report.max_live_context_bytes,
        4 * one
    );
    for g in &report.groups {
        assert!(!g.topics.is_empty());
        let slots: std::collections::BTreeSet<_> =
            g.topics.iter().map(|t| t.symbol.as_bytes()[1]).collect();
        assert_eq!(slots.len(), 1, "a group of one slot: {g:?}");
    }
    assert!(report.groups.len() >= 4);
    // The pledged size is the frame's uncompressed size: the topic's record
    // bytes, plus magic and header in the first frame of a new day file.
    assert_eq!(report.pledged.len(), 12);
    for (topic, pledged) in &report.pledged {
        let records = report.topic_bytes[topic];
        let header = pledged - records;
        assert!(
            header == 0 || (100..400).contains(&header),
            "{topic:?}: {pledged} vs {records}"
        );
        assert!(header > 0, "every day file here is new");
    }
}

/// Run by `common::lock_holder::hold` in a child process only.
#[test]
#[ignore]
fn lock_holder_child() {
    common::lock_holder::child_main();
}

#[test]
fn check_catches_a_lost_or_duplicated_record() {
    use md_lake::seal::Fault;
    // A dropped or repeated record is caught at the frame end already (the
    // pledged size); a byte changed on disk only by the read-back check.
    for fault in [
        Fault::DropRecord(3),
        Fault::RepeatRecord(3),
        Fault::CorruptAppend,
    ] {
        let fs = lake(&format!("seal-check-{fault:?}"));
        let recs: Vec<Record> = (0..10).map(|i| md(T14 + i * S, 1, i as u64, OB)).collect();
        kit::write_open(&fs, "md1", T14 - S, 1, &recs, false);
        let mut s = settings();
        s.fault = Some(fault);
        let err = seal(&fs, s, T14 + time::HOUR_NS + S).expect_err("check");
        match fault {
            Fault::CorruptAppend => assert!(matches!(err, SealError::Check { .. }), "{err:?}"),
            _ => assert!(
                format!("{err:?}").contains("Src size is incorrect"),
                "{fault:?}: {err:?}"
            ),
        }
        let log = kit::seal_log(fs.as_ref(), D, 14);
        assert_eq!(log.len(), 1, "begin only, no done entry");
        assert_eq!(
            kit::open_files(fs.as_ref(), D, 14).len(),
            1,
            "open file kept"
        );
        // The next run without the fault redoes the hour and seals it.
        seal(&fs, settings(), T14 + time::HOUR_NS + S).expect("redo");
        let (ob, frames) =
            kit::day_records(fs.as_ref(), &kit::day_path("orderbook.50", "BTCUSDT", D));
        assert_eq!((ob, frames), (recs, 1));
        let log = kit::seal_log(fs.as_ref(), D, 14);
        assert_eq!(log.last().expect("done")["attempt"], 2);
    }
}

#[test]
fn a_run_does_not_reread_sealed_history() {
    // Two days of sealed hours; the next run seals one hour of the third day
    // and reads no lake data of the earlier days.
    let dir = common::test_dir("seal-history");
    let (fs, counts) = common::counting::CountingFs::open_at(&dir);
    let s1 = T14 - 5 * S;
    let last = T14 + 44 * time::HOUR_NS; // 2026-01-04 10:00
    let mut h = T14;
    while h < last {
        kit::write_open(&fs, "md1", s1, 1, &[md(h + 10 * S, 1, 0, OB)], false);
        h += time::HOUR_NS;
    }
    seal(&fs, settings(), last + S).expect("first run");
    kit::write_open(&fs, "md1", s1, 1, &[md(last + 10 * S, 1, 0, OB)], false);
    counts.reads.lock().expect("reads").clear();
    let report = seal(&fs, settings(), last + time::HOUR_NS + S).expect("second run");
    assert_eq!(report.sealed, vec![("2026-01-04".to_string(), 10)]);
    let reads = counts.reads.lock().expect("reads").clone();
    let old: BTreeSet<&String> = reads
        .iter()
        .filter(|p| p.contains(".qraw") && !p.contains("date=2026-01-04"))
        .collect();
    assert!(old.is_empty(), "read sealed history: {old:?}");
}

/// The `collector_down` notes between two sessions when session 1 ends in
/// hour 15 and session 2 starts in hour 16; `split` seals the two hours in
/// two runs.
fn gap_notes(layout: md_lake::seal::Layout, split: bool) -> Vec<serde_json::Value> {
    let fs = lake(&format!("seal-gap-{layout:?}-{split}"));
    let h15 = T14 + time::HOUR_NS;
    let h16 = T14 + 2 * time::HOUR_NS;
    let (s1, s2) = (h15 - 5 * S, h16 + 19 * S);
    let note = Record {
        recv_ns: h16 - 2 * S,
        src: Src::Note,
        conn_id: 1,
        frame_no: 0,
        payload: br#"{"note":"x"}"#.to_vec(),
    };
    kit::write_open(
        &fs,
        "md1",
        s1,
        1,
        &[md(h15 + 10 * S, 1, 0, OB), md(h16 - 10 * S, 1, 1, OB), note],
        false,
    );
    kit::write_open(&fs, "md1", s2, 1, &[md(h16 + 20 * S, 1, 0, OB)], false);
    let mut s = settings();
    s.layout = layout;
    if split {
        seal(&fs, s.clone(), h16 + S).expect("run 1");
    }
    seal(&fs, s, h16 + time::HOUR_NS + S).expect("run 2");
    sealer_notes(fs.as_ref())
        .into_iter()
        .filter(|n| n["note"] == "collector_down" && n["detail"]["prev_session_ns"] == s1)
        .collect()
}

#[test]
fn session_gap_across_runs_is_the_same_as_in_one_run() {
    use md_lake::seal::Layout;
    let h16 = T14 + 2 * time::HOUR_NS;
    // by_symbol: the tail is the session's last routed record (the day
    // files hold no notes or unroutable records); by_connection: its last
    // record, notes included.
    for (layout, end) in [
        (Layout::BySymbol, h16 - 10 * S),
        (Layout::ByConnection, h16 - 2 * S),
    ] {
        let one = gap_notes(layout, false);
        assert_eq!(one.len(), 1, "{layout:?} one run: {one:?}");
        assert_eq!(one[0]["start_recv_ns"], end, "{layout:?}");
        assert_eq!(one[0]["end_recv_ns"], h16 + 20 * S);
        assert_eq!(gap_notes(layout, true), one, "{layout:?} two runs");
    }
}

#[test]
fn instruments_topics_must_use_the_all_symbol() {
    // The contract names one instruments file per market (symbol `_all`); a
    // classifier that gives a symbol is refused before anything is written.
    let fs = lake("seal-instruments-symbol");
    kit::write_open(
        &fs,
        "md1",
        T14 - S,
        1,
        &[md(T14 + S, 1, 0, "instruments.BTCUSDT")],
        false,
    );
    let clock = FixedClock::at(T14 + time::HOUR_NS + S);
    let r = Sealer::new(fs.clone(), settings()).seal(clock.as_ref(), &classify);
    assert!(
        matches!(&r, Err(SealError::Io(e)) if e.to_string().contains("_all")),
        "{r:?}"
    );
    assert!(kit::seal_log(fs.as_ref(), D, 14).is_empty());
}
