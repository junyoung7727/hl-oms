//! Stage 1: what the collector writes is what the lake keeps. A file that is
//! unlocked too early can be sealed while it still grows.

mod common;

use std::sync::atomic::Ordering;

use common::counting::CountingFs;
use md_lake::fs::{Fs, RealFs};
use md_lake::qraw::{self, Record, Src};
use md_lake::time;
use md_lake::writer::{OpenWriter, SessionWriter, WriterSettings};
use md_lake::zst::{self, Contexts};

const T14: i64 = 1_767_362_400_000_000_000; // 2026-01-02 14:00:00 UTC
const S: i64 = 1_000_000_000;

fn settings() -> WriterSettings {
    WriterSettings::new("bybit", "md1", "0.1.0", "test", T14 - 3_600 * S)
}

fn rec(t: i64, n: u64) -> Record {
    Record {
        recv_ns: t,
        src: Src::WebSocket,
        conn_id: 3,
        frame_no: n,
        payload: format!("{{\"topic\":\"orderbook.50.BTCUSDT\",\"n\":{n}}}").into_bytes(),
    }
}

fn read_records(fs: &dyn Fs, path: &str) -> (Vec<Record>, bool) {
    let mut f = fs.open(path).expect("open");
    let mut bytes = vec![0; f.len().expect("len") as usize];
    let mut at = 0;
    while at < bytes.len() {
        at += f.read_at(at as u64, &mut bytes[at..]).expect("read");
    }
    let scan = zst::scan(&bytes).expect("scan");
    let finished = scan.unfinished.is_none() && scan.frames.len() == 1;
    let segs = zst::decode_frames(&bytes, &scan).expect("decode");
    let refs: Vec<&[u8]> = segs.iter().map(Vec::as_slice).collect();
    (qraw::decode(&refs).expect("qraw").records, finished)
}

#[test]
fn open_file_is_created_locked_under_a_tmp_name() {
    let dir = common::test_dir("writer-lock");
    let (fs, counts) = CountingFs::open_at(&dir);
    let mut w = OpenWriter::new(fs.clone(), settings(), 3, &Contexts::default());
    w.append(&rec(T14 + S, 0)).expect("append");
    let path = w.current_path().expect("open file").to_string();
    assert_eq!(
        path,
        "crypto/l0/_open/venue=bybit/date=2026-01-02/hour=14/md1-1767358800000000000-c3.qraw.zst.partial"
    );
    assert_eq!(
        *counts.link_locked.lock().expect("log"),
        vec![true],
        "locked before the link"
    );
    assert!(!fs.exists(&format!("{path}.tmp")), "the .tmp name is gone");
    assert!(
        !fs.open(&path).expect("open").try_lock().expect("try"),
        "held by the writer"
    );
    w.close().expect("close");
    assert!(
        fs.open(&path).expect("open").try_lock().expect("try"),
        "free after close"
    );

    // An existing open-file name is never replaced.
    let real = RealFs::new(&dir);
    let mut again = OpenWriter::new(real.clone(), settings(), 3, &Contexts::default());
    assert!(again.append(&rec(T14 + 2 * S, 1)).is_err());
    let (records, finished) = read_records(real.as_ref(), &path);
    assert_eq!(records, vec![rec(T14 + S, 0)]);
    assert!(finished);
}

#[test]
fn flush_every_second_and_fsync_every_five() {
    for (flush, fsync, want_flush, want_sync) in [(1, 5, 12, 2), (2, 3, 6, 4)] {
        let dir = common::test_dir(&format!("writer-timers-{flush}-{fsync}"));
        let (fs, counts) = CountingFs::open_at(&dir);
        let mut s = settings();
        s.flush_ns = flush * S;
        s.fsync_ns = fsync * S;
        let mut w = OpenWriter::new(fs, s, 3, &Contexts::default());
        for i in 0..=120 {
            let t = T14 + i * S / 10;
            w.append(&rec(t, i as u64)).expect("append");
            w.tick(t).expect("tick");
        }
        // At level 3 these small records stay in zstd's buffer until a flush,
        // so every write is a flush.
        assert_eq!(
            counts.writes.load(Ordering::SeqCst),
            want_flush,
            "flushes ({flush}, {fsync})"
        );
        assert_eq!(
            counts.syncs.load(Ordering::SeqCst),
            want_sync,
            "fsyncs ({flush}, {fsync})"
        );
        w.close().expect("close");
    }
}

#[test]
fn rotation_at_the_utc_hour() {
    let dir = common::test_dir("writer-rotate");
    let fs = RealFs::new(&dir);
    let contexts = Contexts::default();
    let mut w = OpenWriter::new(fs.clone(), settings(), 3, &contexts);
    w.append(&rec(T14 - S / 10, 0)).expect("13:59:59.9");
    let first = w.current_path().expect("13").to_string();
    w.append(&rec(T14, 1)).expect("14:00:00.0");
    let second = w.current_path().expect("14").to_string();
    assert!(first.contains("/hour=13/") && second.contains("/hour=14/"));
    let (r13, finished) = read_records(fs.as_ref(), &first);
    assert_eq!((r13, finished), (vec![rec(T14 - S / 10, 0)], true));
    assert!(
        fs.open(&first).expect("open").try_lock().expect("try"),
        "13 unlocked"
    );

    // tick closes a file whose hour has passed, without a record.
    w.tick(T14 + time::HOUR_NS).expect("tick at 15:00");
    assert_eq!(w.current_path(), None);
    assert!(
        fs.open(&second).expect("open").try_lock().expect("try"),
        "14 unlocked"
    );
    assert!(read_records(fs.as_ref(), &second).1, "14 finished");
}

#[test]
fn rotation_is_forward_only() {
    let dir = common::test_dir("writer-forward");
    let fs = RealFs::new(&dir);
    let mut w = OpenWriter::new(fs.clone(), settings(), 3, &Contexts::default());
    w.append(&rec(T14 + S, 0)).expect("14:00:01");
    w.append(&rec(T14 - S, 1))
        .expect("13:59:59, clock stepped back");
    let path = w.current_path().expect("14").to_string();
    w.close().expect("close");
    assert!(path.contains("/hour=14/"));
    assert_eq!(read_records(fs.as_ref(), &path).0.len(), 2);
    assert!(
        fs.list("crypto/l0/_open/venue=bybit/date=2026-01-02/hour=13")
            .expect("ls")
            .is_empty()
    );
}

#[test]
fn an_hour_closed_by_tick_is_not_reopened() {
    // A record of hour 14 that arrives after tick closed hour 14 (taken just
    // before the tick, or the clock stepped back) goes to the next hour's
    // file: hour 14 may be sealed already.
    let dir = common::test_dir("writer-closed-hour");
    let fs = RealFs::new(&dir);
    let mut w = OpenWriter::new(fs.clone(), settings(), 3, &Contexts::default());
    w.append(&rec(T14 + S, 0)).expect("14:00:01");
    w.tick(T14 + time::HOUR_NS).expect("tick at 15:00 closes hour 14");
    assert!(w.current_path().is_none());
    w.append(&rec(T14 + time::HOUR_NS - S, 1))
        .expect("14:59:59 after the close");
    let path = w.current_path().expect("15").to_string();
    w.close().expect("close");
    assert!(path.contains("/hour=15/"), "{path}");
    assert_eq!(read_records(fs.as_ref(), &path).0.len(), 1);
    let h14 = fs
        .list("crypto/l0/_open/venue=bybit/date=2026-01-02/hour=14")
        .expect("ls");
    assert_eq!(h14.len(), 1, "{h14:?}");
}

#[test]
fn one_level3_context_per_connection() {
    let dir = common::test_dir("writer-contexts");
    let fs = RealFs::new(&dir);
    let contexts = Contexts::default();
    let mut ws: Vec<OpenWriter> = (1..=3)
        .map(|c| OpenWriter::new(fs.clone(), settings(), c, &contexts))
        .collect();
    for h in 0..2 {
        for (i, w) in ws.iter_mut().enumerate() {
            w.append(&rec(T14 + h * time::HOUR_NS + i as i64, h as u64))
                .expect("append");
        }
    }
    assert_eq!(contexts.max_live(), 3);
    assert_eq!(contexts.live(), 3);
    for w in ws {
        w.close().expect("close");
    }
    assert_eq!(contexts.live(), 0);
}

#[test]
fn session_file_by_date_and_backward_clock() {
    let dir = common::test_dir("writer-session");
    let fs = RealFs::new(&dir);
    let mut w = SessionWriter::new(fs.clone(), settings(), &Contexts::default());
    let midnight = time::day_start(T14) + time::DAY_NS; // 2026-01-03 00:00
    w.append(&rec(T14, 0)).expect("day 1");
    let day1 = w.current_path().expect("day 1").to_string();
    w.tick(midnight).expect("tick at midnight");
    let day2 = w.current_path().expect("day 2").to_string();
    assert!(
        day1.contains("/date=2026-01-02/") && day2.contains("/date=2026-01-03/"),
        "{day1} {day2}"
    );
    assert!(
        fs.open(&day1).expect("open").try_lock().expect("try"),
        "day 1 closed and unlocked"
    );
    // A record of 2026-01-02 after its file is closed goes to the newest date.
    w.append(&rec(midnight - S, 1)).expect("backward");
    w.close().expect("close");
    assert_eq!(read_records(fs.as_ref(), &day1).0, vec![rec(T14, 0)]);
    assert_eq!(
        read_records(fs.as_ref(), &day2).0,
        vec![rec(midnight - S, 1)]
    );
    assert!(day2.ends_with("/md1-1767358800000000000.qraw.zst.partial"));
}

#[test]
fn the_real_fs_refuses_paths_outside_the_lake() {
    let dir = common::test_dir("writer-root");
    let fs = RealFs::new(&dir);
    for bad in ["../x", "/etc/x", "a/../../x", "a\\b"] {
        assert!(fs.create_new(bad).is_err(), "{bad}");
    }
}

#[test]
fn new_directories_are_found_outermost_first() {
    // RealFs fsyncs the parent of each of these after creating them.
    let dir = common::test_dir("writer-dirs");
    let d = dir.join("a/b/c");
    assert_eq!(
        md_lake::fs::missing_dirs(&d),
        vec![dir.join("a"), dir.join("a/b"), dir.join("a/b/c")]
    );
    std::fs::create_dir_all(dir.join("a")).expect("mkdir");
    assert_eq!(
        md_lake::fs::missing_dirs(&d),
        vec![dir.join("a/b"), dir.join("a/b/c")]
    );
}
