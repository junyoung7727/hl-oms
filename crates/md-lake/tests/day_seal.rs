//! The day seal (lake contract 2.7): only when every hour is sealed and no
//! collector still holds a file of the date; then rename, sidecar and one
//! manifest line per sealed file, idempotent, never changing a sealed file.

mod common;

use std::sync::Arc;

use common::kit::{self, FixedClock, S, T14, classify, md};
use common::sim::SimFs;
use md_lake::fs::{Fs, RealFs};
use md_lake::qraw::{Record, Src};
use md_lake::seal::{SealSettings, Sealer};
use md_lake::time;
use md_lake::writer::{SessionWriter, WriterSettings};
use md_lake::zst::Contexts;
use sha2::{Digest, Sha256};

const D: &str = "2026-01-02";
/// 2026-01-03 00:10, after every hour of D has closed.
const NEXT_DAY: i64 = T14 + 10 * time::HOUR_NS + 10 * 60 * S;

fn settings() -> SealSettings {
    SealSettings::new("bybit", "md_sealer", "0.1.0", "test", T14)
}

fn seal(fs: &Arc<dyn Fs>, now: i64) -> Result<md_lake::seal::SealReport, md_lake::seal::SealError> {
    let clock = FixedClock::at(now);
    Sealer::new(fs.clone(), settings()).seal(clock.as_ref(), &classify)
}

const OB: &str = "orderbook.50.BTCUSDT";

/// Two sessions on D (hours 14 and 16), and the first session's own session
/// file, closed.
fn day(fs: &Arc<dyn Fs>) -> (i64, i64) {
    let (s1, s2) = (T14 - S, T14 + 2 * time::HOUR_NS);
    kit::write_open(
        fs,
        "md1",
        s1,
        1,
        // Records in hours 14 and 15: the session's two runs are joined.
        &[
            md(T14 + S, 1, 0, OB),
            md(T14 + 2 * S, 1, 1, OB),
            md(T14 + time::HOUR_NS + S, 1, 3, OB),
        ],
        false,
    );
    kit::write_open(fs, "md1", s2, 1, &[md(s2 + S, 1, 0, OB)], false);
    let mut w = SessionWriter::new(
        fs.clone(),
        WriterSettings::new("bybit", "md1", "0.1.0", "test", s1),
        &Contexts::default(),
    );
    w.append(&kit::control(T14 + 3 * S, 1, 2)).expect("control");
    w.close().expect("close");
    (s1, s2)
}

fn manifest(fs: &dyn Fs) -> Vec<serde_json::Value> {
    let p = format!("crypto/l0/_manifest/venue=bybit/date={D}.jsonl");
    if !fs.exists(&p) {
        return Vec::new();
    }
    String::from_utf8(kit::read_all(fs, &p))
        .expect("utf8")
        .lines()
        .map(|l| serde_json::from_str(l).expect("line"))
        .collect()
}

fn partials(dir: &std::path::Path) -> Vec<String> {
    let mut out = Vec::new();
    fn walk(d: &std::path::Path, out: &mut Vec<String>) {
        for e in std::fs::read_dir(d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out);
            } else if p.to_string_lossy().ends_with(".partial") {
                out.push(p.to_string_lossy().into_owned());
            }
        }
    }
    walk(dir, &mut out);
    out
}

#[test]
fn day_seal_waits_for_every_hour_and_every_lock() {
    let dir = common::test_dir("day-waits");
    let fs = RealFs::new(&dir);
    day(&fs);
    let p14 = format!(
        "crypto/l0/_open/venue=bybit/date={D}/hour=14/{}",
        kit::open_files(fs.as_ref(), D, 14)[0]
    );
    {
        let _held = common::lock_holder::hold(&dir.join(&p14));
        let r = seal(&fs, NEXT_DAY).expect("seal");
        assert!(r.sealed.is_empty());
        assert!(r.days_sealed.is_empty(), "the day waits for hour 14");
        assert!(!partials(&dir).is_empty());
        assert!(manifest(fs.as_ref()).is_empty());
    }
    let r = seal(&fs, NEXT_DAY).expect("seal");
    assert_eq!(r.sealed.first(), Some(&(D.to_string(), 14)));
    assert_eq!(r.sealed.last(), Some(&(D.to_string(), 23)));
    assert_eq!(r.days_sealed, vec![D.to_string()]);
    assert!(partials(&dir).is_empty(), "{:?}", partials(&dir));

    // A collector that still holds its session file of the date: the day waits.
    let dir = common::test_dir("day-waits-session");
    let fs = RealFs::new(&dir);
    day(&fs);
    let mut w = SessionWriter::new(
        fs.clone(),
        WriterSettings::new("bybit", "md2", "0.1.0", "test", T14),
        &Contexts::default(),
    );
    w.append(&kit::control(T14 + 5 * S, 1, 0)).expect("control");
    let r = seal(&fs, NEXT_DAY).expect("seal");
    assert_eq!(r.sealed.len(), 10, "hours 14-23 are sealed");
    assert!(
        r.days_sealed.is_empty(),
        "md2 still writes its session file"
    );
    w.close().expect("close");
    let r = seal(&fs, NEXT_DAY).expect("seal");
    assert_eq!(r.days_sealed, vec![D.to_string()]);
}

#[test]
fn sidecar_and_manifest_line_validate() {
    let dir = common::test_dir("day-manifest");
    let fs = RealFs::new(&dir);
    let (s1, s2) = day(&fs);
    seal(&fs, NEXT_DAY).expect("seal");
    let lines = manifest(fs.as_ref());
    let ids: Vec<&str> = lines
        .iter()
        .map(|l| l["l0_file_id"].as_str().expect("id"))
        .collect();
    assert_eq!(lines.len(), 3, "{ids:?}");
    for l in &lines {
        common::schema::assert_valid("xrepo/manifest_line.schema.json", l);
        let path = l["path"].as_str().expect("path");
        let bytes = kit::read_all(fs.as_ref(), path);
        let sha = hex::encode(Sha256::digest(&bytes));
        assert_eq!(l["sha256"], sha);
        assert_eq!(l["bytes"], bytes.len());
        let name = path.rsplit('/').next().expect("name");
        let sidecar =
            String::from_utf8(kit::read_all(fs.as_ref(), &format!("{path}.sha256"))).expect("utf8");
        assert_eq!(sidecar, format!("{sha}  {name}\n"));
    }
    let ob = lines
        .iter()
        .find(|l| {
            l["l0_file_id"]
                == format!("venue=bybit/market=linear/channel=orderbook.50/date={D}/BTCUSDT")
        })
        .expect("ob line");
    assert_eq!(ob["layout"], "by_symbol");
    assert_eq!(ob["records"], 4);
    assert_eq!(ob["first_recv_ns"], T14 + S);
    assert_eq!(ob["last_recv_ns"], s2 + S);
    assert_eq!(
        ob["sessions"],
        serde_json::json!([
            {"collector": "md1", "session_ns": s1, "first_rec_no": 0, "records": 3},
            {"collector": "md1", "session_ns": s2, "first_rec_no": 3, "records": 1}
        ])
    );
    let coll = lines
        .iter()
        .find(|l| l["l0_file_id"] == format!("venue=bybit/_session/date={D}/md1-{s1}"))
        .expect("collector session file line");
    assert_eq!(
        (coll["layout"].as_str(), coll["records"].as_u64()),
        (Some("session"), Some(1))
    );
    assert_eq!(coll["sessions"], serde_json::Value::Null);
}

#[test]
fn step0_closes_a_crashed_session_file_once() {
    // A collector's session file without a frame end (the collector crashed).
    let (sim, fs) = SimFs::new();
    day(&fs);
    let mut w = SessionWriter::new(
        fs.clone(),
        WriterSettings::new("bybit", "md3", "0.1.0", "test", T14 + S),
        &Contexts::default(),
    );
    w.append(&kit::control(T14 + 4 * S, 1, 0)).expect("control");
    w.tick(T14 + 6 * S).expect("flush");
    drop(w);
    sim.persist_all();
    // Crash at each step after the hour seals until the day seal is done.
    let clean = {
        let (sim2, fs2) = SimFs::new();
        day(&fs2);
        let mut w = SessionWriter::new(
            fs2.clone(),
            WriterSettings::new("bybit", "md3", "0.1.0", "test", T14 + S),
            &Contexts::default(),
        );
        w.append(&kit::control(T14 + 4 * S, 1, 0)).expect("control");
        w.tick(T14 + 6 * S).expect("flush");
        drop(w);
        sim2.persist_all();
        sim2.fail_after(usize::MAX);
        seal(&fs2, NEXT_DAY).expect("clean");
        sim2.calls()
    };
    for n in (clean.saturating_sub(40)).max(1)..=clean {
        let (sim, fs) = SimFs::new();
        day(&fs);
        let mut w = SessionWriter::new(
            fs.clone(),
            WriterSettings::new("bybit", "md3", "0.1.0", "test", T14 + S),
            &Contexts::default(),
        );
        w.append(&kit::control(T14 + 4 * S, 1, 0)).expect("control");
        w.tick(T14 + 6 * S).expect("flush");
        drop(w);
        sim.persist_all();
        sim.fail_after(n);
        assert!(seal(&fs, NEXT_DAY).is_err(), "step {n}");
        sim.reboot(n as u64);
        seal(&fs, NEXT_DAY).expect("after the crash");
        let path = format!(
            "crypto/l0/venue=bybit/_session/date={D}/md3-{}.qraw.zst",
            T14 + S
        );
        let (records, frames) = kit::day_records(fs.as_ref(), &path);
        assert_eq!(frames, 1, "step {n}");
        let notes: Vec<&Record> = records.iter().filter(|r| r.src == Src::Note).collect();
        assert_eq!(notes.len(), 1, "step {n}: one crash_truncate note");
        assert_eq!(
            records.last().map(|r| r.src),
            Some(Src::Note),
            "step {n}: the note is last"
        );
        let note: serde_json::Value = serde_json::from_slice(&notes[0].payload).expect("note");
        assert_eq!(note["note"], "crash_truncate");
        assert_eq!(
            note["detail"]["file"],
            format!("venue=bybit/_session/date={D}/md3-{}", T14 + S)
        );
        assert_eq!(note["start_recv_ns"], T14 + 4 * S);
        let lines = manifest(fs.as_ref());
        let mut ids: Vec<&str> = lines
            .iter()
            .map(|l| l["l0_file_id"].as_str().expect("id"))
            .collect();
        let n_ids = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), n_ids, "step {n}: one line per file");
        assert!(
            !sim.files()
                .iter()
                .any(|f| f.ends_with(".partial") || f.ends_with(".tmp")),
            "step {n}: {:?}",
            sim.files()
        );
    }
    let _ = sim;
}

#[test]
fn day_seal_rerun_is_idempotent() {
    let dir = common::test_dir("day-rerun");
    let fs = RealFs::new(&dir);
    day(&fs);
    seal(&fs, NEXT_DAY).expect("seal");
    let before = manifest(fs.as_ref());
    let ob = format!(
        "crypto/l0/venue=bybit/market=linear/channel=orderbook.50/date={D}/BTCUSDT.qraw.zst"
    );
    let bytes = kit::read_all(fs.as_ref(), &ob);
    let mtime = std::fs::metadata(dir.join(&ob))
        .expect("meta")
        .modified()
        .expect("mtime");
    // A torn last manifest line (a crash while appending) and a missing sidecar.
    let mp = format!("crypto/l0/_manifest/venue=bybit/date={D}.jsonl");
    let mut f = fs.open(&mp).expect("manifest");
    f.write_all(b"{\"l0_file_id\":\"venue=by").expect("torn");
    drop(f);
    std::fs::remove_file(dir.join(format!("{ob}.sha256"))).expect("rm sidecar");
    seal(&fs, NEXT_DAY + time::HOUR_NS).expect("rerun");
    assert_eq!(
        manifest(fs.as_ref()),
        before,
        "torn line cut, no duplicate line"
    );
    assert!(fs.exists(&format!("{ob}.sha256")), "sidecar written again");
    assert_eq!(
        kit::read_all(fs.as_ref(), &ob),
        bytes,
        "a sealed file is never changed"
    );
    assert_eq!(
        std::fs::metadata(dir.join(&ob))
            .expect("meta")
            .modified()
            .expect("mtime"),
        mtime
    );
}

/// Run by `common::lock_holder::hold` in a child process only.
#[test]
#[ignore]
fn lock_holder_child() {
    common::lock_holder::child_main();
}

fn partials_of(dir: &std::path::Path, date: &str) -> Vec<String> {
    partials(dir)
        .into_iter()
        .filter(|p| p.contains(&format!("date={date}")))
        .collect()
}

#[test]
fn hours_before_first_hour_do_not_block_the_day() {
    // An open file of hour 13 is from before this sealer (F7): never sealed,
    // never touched, and the date still day-seals.
    let dir = common::test_dir("day-before-first-hour");
    let fs = RealFs::new(&dir);
    day(&fs);
    let early = T14 - time::HOUR_NS;
    kit::write_open(&fs, "md0", early - S, 1, &[md(early + S, 1, 0, OB)], false);
    let r = seal(&fs, NEXT_DAY).expect("seal");
    assert_eq!(r.days_sealed, vec![D.to_string()]);
    assert_eq!(kit::open_files(fs.as_ref(), D, 13).len(), 1);
}

#[test]
fn a_failed_hour_does_not_block_an_earlier_date() {
    // Hour 00 of the next date stops on the unroutable guard; the complete
    // date before it is day-sealed all the same, then the error is returned.
    let dir = common::test_dir("day-after-failed-hour");
    let fs = RealFs::new(&dir);
    day(&fs);
    let next = T14 + 10 * time::HOUR_NS;
    let junk: Vec<Record> = (0..5)
        .map(|i| kit::control(next + i * S, 1, i as u64))
        .collect();
    kit::write_open(&fs, "md1", next - S, 1, &junk, false);
    let r = seal(&fs, next + time::HOUR_NS + S);
    assert!(
        matches!(r, Err(md_lake::seal::SealError::Unroutable { .. })),
        "{r:?}"
    );
    assert!(
        partials_of(&dir, D).is_empty(),
        "{:?}",
        partials_of(&dir, D)
    );
    assert!(!manifest(fs.as_ref()).is_empty());
}

#[test]
fn a_date_older_than_the_lookback_seals_after_an_outage() {
    // No seal run for nine days: the run that seals the hours also
    // day-seals their dates, however old.
    let dir = common::test_dir("day-after-outage");
    let fs = RealFs::new(&dir);
    day(&fs);
    let r = seal(&fs, T14 + 9 * time::DAY_NS).expect("seal");
    assert_eq!(r.days_sealed.first(), Some(&D.to_string()));
    assert!(partials_of(&dir, D).is_empty());
}

#[test]
fn seal_days_since_reaches_any_date() {
    // A date that becomes sealable only after the lookback (here: a
    // collector held its session file for nine days) is sealed on demand.
    let dir = common::test_dir("day-since");
    let fs = RealFs::new(&dir);
    day(&fs);
    let mut w = SessionWriter::new(
        fs.clone(),
        WriterSettings::new("bybit", "md2", "0.1.0", "test", T14),
        &Contexts::default(),
    );
    w.append(&kit::control(T14 + 5 * S, 1, 0)).expect("control");
    let later = T14 + 9 * time::DAY_NS;
    let r = seal(&fs, later).expect("seal");
    assert!(!r.days_sealed.contains(&D.to_string()));
    w.close().expect("close");
    let clock = FixedClock::at(later + time::HOUR_NS);
    let r = Sealer::new(fs.clone(), settings())
        .seal_days_since(clock.as_ref(), T14)
        .expect("seal days");
    assert_eq!(r.days_sealed.first(), Some(&D.to_string()));
    assert!(partials_of(&dir, D).is_empty());
}
