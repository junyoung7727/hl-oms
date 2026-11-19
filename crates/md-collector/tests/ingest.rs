//! Ticket 03: sink, bounded queues, writer thread, notes (spec D4).

mod common;
#[path = "common/slowfs.rs"]
mod slowfs;

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use common::*;
use md_collector::ingest;
use md_collector::registry;
use md_feed::{FrameSink, RawFrame, Src};
use md_lake::fs::{Fs, RealFs};
use md_lake::qraw::{Record, Src as LakeSrc};
use md_lake::writer::WriterSettings;
use slowfs::{Gate, SlowFs};

const SPOT: u32 = (2 << 28) + 1;

fn frame(recv_ns: i64, src: Src, conn_id: u32, frame_no: u64, payload: &str) -> RawFrame {
    // Slot = conn_id in these tests, unless a test sets another.
    RawFrame {
        recv_ns,
        src,
        conn_id,
        frame_no,
        conn_index: conn_id,
        topic: None,
        for_conn_id: None,
        payload: payload.as_bytes().to_vec(),
    }
}

fn settings(session_ns: i64) -> WriterSettings {
    WriterSettings::new("bybit", "md_collector", "0.1.0", "test", session_ns)
}

/// Records of every open file of one connection, in hour order.
fn open_records(fs: &dyn Fs, dir: &Path, conn_id: u32) -> Vec<Record> {
    let mut files: Vec<String> = files_with(dir, "crypto/l0/_open/")
        .into_iter()
        .filter(|p| p.ends_with(&format!("-c{conn_id}.qraw.zst.partial")))
        .collect();
    files.sort();
    files.iter().flat_map(|p| records(fs, p)).collect()
}

fn session_records(fs: &dyn Fs, dir: &Path) -> Vec<Record> {
    let mut files = files_with(dir, "crypto/l0/venue=bybit/_session/");
    files.sort();
    files.iter().flat_map(|p| records(fs, p)).collect()
}

fn payloads(rs: &[Record]) -> Vec<String> {
    rs.iter()
        .map(|r| String::from_utf8_lossy(&r.payload).to_string())
        .collect()
}

fn notes(rs: &[Record]) -> Vec<serde_json::Value> {
    rs.iter()
        .filter(|r| r.src == LakeSrc::Note)
        .map(|r| serde_json::from_slice(&r.payload).expect("note json"))
        .collect()
}

fn eventually(mut f: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !f() {
        assert!(Instant::now() < deadline, "condition not met within 10 s");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn limits(queue_bytes: usize, total_bytes: usize) -> ingest::Limits {
    ingest::Limits {
        queue_bytes,
        total_bytes,
    }
}

fn bybit() -> &'static dyn registry::Venue {
    registry::venue("bybit").expect("bybit")
}

#[test]
fn routes_market_data_and_control() {
    let dir = test_dir("ingest-routes");
    let fs = RealFs::new(&dir);
    let t = md_collector::wall_ns();
    let (ing, writer) =
        ingest::start(fs.clone(), settings(t), bybit(), limits(1 << 20, 1 << 30)).expect("start");
    let b = book(50, "BTCUSDT", "snapshot", 1, 1);
    let tr = trade("BTCUSDT", "t1", 2);
    let inst = instruments_body("linear", &["BTCUSDT"], "");
    let failed = r#"{"retCode":10006,"retMsg":"Too many visits"}"#;
    let pong = r#"{"op":"pong","success":true}"#;
    ing.frame(frame(t + 1, Src::Rest, 0, 0, &inst))
        .expect("rest");
    ing.frame(frame(t + 2, Src::Ws, 5, 0, &b)).expect("book");
    ing.frame(frame(t + 3, Src::Ws, 5, 1, pong)).expect("pong");
    ing.frame(frame(t + 4, Src::Ws, 5, 2, &tr)).expect("trade");
    ing.frame(frame(t + 5, Src::Ws, SPOT, 0, &b)).expect("spot");
    ing.frame(frame(t + 6, Src::Rest, 0, 1, failed))
        .expect("failed rest");
    writer.stop().expect("stop");

    let lin = open_records(fs.as_ref(), &dir, 5);
    assert_eq!(payloads(&lin), vec![b.clone(), tr.clone()]);
    assert_eq!(
        lin.iter()
            .map(|r| (r.conn_id, r.frame_no, r.recv_ns))
            .collect::<Vec<_>>(),
        vec![(5, 0, t + 2), (5, 2, t + 4)]
    );
    assert_eq!(
        payloads(&open_records(fs.as_ref(), &dir, SPOT)),
        vec![b.clone()]
    );
    let rest = open_records(fs.as_ref(), &dir, 0);
    assert_eq!(payloads(&rest), vec![inst.clone()]);
    assert_eq!(rest[0].src, LakeSrc::Rest);
    let sess = session_records(fs.as_ref(), &dir);
    assert_eq!(payloads(&sess), vec![pong.to_string(), failed.to_string()]);
    assert_eq!((sess[0].conn_id, sess[0].frame_no), (5, 1));
}

#[test]
fn clock_step_note() {
    let dir = test_dir("ingest-clock-step");
    let fs = RealFs::new(&dir);
    let t = md_collector::wall_ns();
    let (ing, writer) =
        ingest::start(fs.clone(), settings(t), bybit(), limits(1 << 20, 1 << 30)).expect("start");
    let a = book(50, "BTCUSDT", "snapshot", 1, 1);
    let b = book(50, "BTCUSDT", "delta", 2, 2);
    let c = book(50, "BTCUSDT", "delta", 3, 3);
    ing.frame(frame(t + 5_000, Src::Ws, 5, 0, &a)).expect("a");
    ing.frame(frame(t + 4_000, Src::Ws, 5, 1, &b)).expect("b");
    ing.frame(frame(t + 4_500, Src::Ws, 5, 2, &c)).expect("c");
    // Another connection's earlier time is not a step of connection 5.
    ing.frame(frame(t + 100, Src::Ws, 6, 0, &a)).expect("other");
    writer.stop().expect("stop");
    assert_eq!(payloads(&open_records(fs.as_ref(), &dir, 5)), vec![a, b, c]);
    let n = notes(&session_records(fs.as_ref(), &dir));
    assert_eq!(n.len(), 1, "{n:?}");
    assert_eq!(n[0]["note"], "clock_step");
    assert_eq!(n[0]["writer"], "md_collector");
    assert_eq!(n[0]["conn_id"], 5);
    assert_eq!(n[0]["topics"], serde_json::Value::Null);
    assert_eq!(n[0]["start_recv_ns"], t + 4_000);
    assert_eq!(n[0]["end_recv_ns"], serde_json::Value::Null);
    assert_eq!(n[0]["detail"]["back_ns"], 1_000);
    assert_eq!(ing.state().clock_steps, 1);
}

#[test]
fn overflow_refuses_and_notes() {
    let dir = test_dir("ingest-overflow");
    let real = RealFs::new(&dir);
    let gate = Arc::new(Gate::default());
    gate.close();
    let fs: Arc<dyn Fs> = Arc::new(SlowFs {
        inner: real.clone(),
        gate: gate.clone(),
    });
    let t = md_collector::wall_ns();
    let (ing, writer) =
        ingest::start(fs, settings(t), bybit(), limits(4096, 1 << 30)).expect("start");
    let mut sent = Vec::new();
    let mut refused_at = None;
    for i in 0..200u64 {
        let p = book(50, "BTCUSDT", "delta", i64::try_from(i).expect("i") + 1, 1);
        let at = t + i64::try_from(i).expect("i");
        match ing.frame(frame(at, Src::Ws, 5, i, &p)) {
            Ok(()) => sent.push(p),
            Err(_) => {
                refused_at = Some(at);
                break;
            }
        }
    }
    let refused_at = refused_at.expect("the queue never filled");
    assert!(sent.len() > 2, "accepted {}", sent.len());
    // The refused frame and later ones of the connection are refused; the
    // note is written once.
    assert!(
        ing.frame(frame(refused_at + 1, Src::Ws, 5, 999, "{}"))
            .is_err()
    );
    // Another connection has its own queue.
    ing.frame(frame(
        t,
        Src::Ws,
        6,
        0,
        &book(50, "ETHUSDT", "snapshot", 1, 1),
    ))
    .expect("other connection");
    assert_eq!(ing.state().conns[&5].overflows, 1);
    // The reconnect (conn_id 7) of the same slot inherits the backlog: it is
    // refused while the slot is full, with its own note (spec R1).
    let mut again = frame(
        refused_at + 2,
        Src::Ws,
        7,
        0,
        &book(50, "BTCUSDT", "snapshot", 1, 1),
    );
    again.conn_index = 5;
    assert!(ing.frame(again).is_err());
    ing.conn_ended(5);
    ing.conn_ended(7);
    gate.open();
    writer.stop().expect("stop");
    assert_eq!(payloads(&open_records(real.as_ref(), &dir, 5)), sent);
    let n = notes(&session_records(real.as_ref(), &dir));
    assert_eq!(n.len(), 2, "{n:?}");
    assert_eq!(n[0]["note"], "overflow");
    assert_eq!(n[0]["conn_id"], 5);
    assert_eq!(n[0]["start_recv_ns"], refused_at);
    assert_eq!(n[0]["end_recv_ns"], serde_json::Value::Null);
    assert_eq!(
        (n[1]["note"].clone(), n[1]["conn_id"].clone()),
        ("overflow".into(), 7.into())
    );
}

#[test]
fn total_cap_refuses() {
    let dir = test_dir("ingest-total-cap");
    let real = RealFs::new(&dir);
    let gate = Arc::new(Gate::default());
    gate.close();
    let fs: Arc<dyn Fs> = Arc::new(SlowFs {
        inner: real.clone(),
        gate: gate.clone(),
    });
    let t = md_collector::wall_ns();
    // Each slot may hold 1 MiB, all slots together 2 KiB.
    let (ing, writer) =
        ingest::start(fs, settings(t), bybit(), limits(1 << 20, 2048)).expect("start");
    let p = book(50, "BTCUSDT", "snapshot", 1, 1);
    let mut refused = None;
    for c in 1..100u32 {
        if ing.frame(frame(t, Src::Ws, c, 0, &p)).is_err() {
            refused = Some(c);
            break;
        }
    }
    let refused = refused.expect("the total cap never refused");
    assert!(refused > 2, "refused at connection {refused}");
    // The collector's own instrument bodies are never refused (spec R6).
    let inst = instruments_body("linear", &["BTCUSDT"], "");
    ing.rest_body(t + 1, 0, inst.clone().into_bytes());
    gate.open();
    writer.stop().expect("stop");
    assert_eq!(payloads(&open_records(real.as_ref(), &dir, 0)), vec![inst]);
}

#[test]
fn close_marker_unlocks() {
    let dir = test_dir("ingest-close");
    let fs = RealFs::new(&dir);
    let t = md_collector::wall_ns();
    let (ing, writer) =
        ingest::start(fs.clone(), settings(t), bybit(), limits(1 << 20, 1 << 30)).expect("start");
    ing.frame(frame(
        t,
        Src::Ws,
        5,
        0,
        &book(50, "BTCUSDT", "snapshot", 1, 1),
    ))
    .expect("frame");
    eventually(|| {
        ing.state()
            .conns
            .get(&5)
            .is_some_and(|c| c.frames_written == 1)
    });
    let path = files_with(&dir, "-c5.qraw.zst.partial");
    assert_eq!(path.len(), 1);
    let locked = |p: &str| {
        let f = std::fs::File::open(dir.join(p)).expect("open");
        matches!(f.try_lock(), Err(std::fs::TryLockError::WouldBlock))
    };
    assert!(locked(&path[0]), "the writer holds the open file");
    ing.conn_ended(5);
    eventually(|| !locked(&path[0]));
    let bytes = std::fs::read(dir.join(&path[0])).expect("read");
    let scan = md_lake::zst::scan(&bytes).expect("scan");
    assert!(scan.unfinished.is_none(), "the file ends with a frame end");
    // A frame of a closed connection is kept, in the session file.
    ing.frame(frame(t + 1, Src::Ws, 5, 1, &trade("BTCUSDT", "late", 1)))
        .expect("late");
    writer.stop().expect("stop");
    assert_eq!(open_records(fs.as_ref(), &dir, 5).len(), 1);
    let sess = session_records(fs.as_ref(), &dir);
    assert_eq!(payloads(&sess), vec![trade("BTCUSDT", "late", 1)]);
    assert_eq!(ing.state().late_frames, 1);
}

#[test]
fn a_queued_frame_never_opens_a_past_hour() {
    // Spec R2: a frame stamped in an hour that has passed by the time the
    // writer takes it goes to the current hour's file.
    let dir = test_dir("ingest-past-hour");
    let fs = RealFs::new(&dir);
    let t = md_collector::wall_ns();
    let (ing, writer) =
        ingest::start(fs.clone(), settings(t), bybit(), limits(1 << 20, 1 << 30)).expect("start");
    ing.frame(frame(
        t - 2 * HOUR,
        Src::Ws,
        5,
        0,
        &book(50, "BTCUSDT", "snapshot", 1, 1),
    ))
    .expect("frame");
    writer.stop().expect("stop");
    let files = files_with(&dir, "-c5.qraw.zst.partial");
    assert_eq!(files.len(), 1, "{files:?}");
    let now_hour = md_lake::time::hour(md_collector::wall_ns());
    let file_hour = md_lake::time::hour(t);
    assert!(
        files[0].contains(&format!("/hour={file_hour:02}/"))
            || files[0].contains(&format!("/hour={now_hour:02}/")),
        "{files:?}"
    );
}
