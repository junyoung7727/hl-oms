//! Ticket 04: the writer beat (heartbeat value and `WATCHDOG=1`) follows the
//! writer loop, not the venue (U6 spec D5, R5, R10).

mod common;
#[path = "common/fake_bybit.rs"]
mod fake;
#[path = "common/run.rs"]
mod run;
#[path = "common/slowfs.rs"]
mod slowfs;

use std::os::unix::net::UnixDatagram;
use std::sync::Arc;
use std::time::{Duration, Instant};

use common::*;
use fake::Behaviour;
use md_collector::collector::Collector;
use md_collector::ingest::{self, Limits};
use md_collector::notify::{Notifier, SdNotify};
use md_collector::registry;
use md_feed::{FrameSink, RawFrame, Src};
use md_lake::fs::RealFs;
use md_lake::writer::WriterSettings;
use run::*;
use slowfs::{Gate, SlowFs};

/// A notify socket in the test directory; collects what arrives.
struct Socket {
    sock: UnixDatagram,
    path: String,
}

impl Socket {
    fn new(dir: &std::path::Path) -> Socket {
        let path = dir.join("notify.sock");
        let _ = std::fs::remove_file(&path);
        let sock = UnixDatagram::bind(&path).expect("bind");
        sock.set_nonblocking(true).expect("nonblocking");
        Socket {
            sock,
            path: path.to_string_lossy().into_owned(),
        }
    }

    fn drain(&self) -> Vec<String> {
        let mut out = Vec::new();
        let mut buf = [0u8; 256];
        while let Ok(n) = self.sock.recv(&mut buf) {
            out.push(String::from_utf8_lossy(&buf[..n]).into_owned());
        }
        out
    }

    fn notifier(&self) -> Arc<dyn Notifier> {
        Arc::new(SdNotify::new(&self.path).expect("notifier"))
    }
}

fn settings(t: i64) -> WriterSettings {
    WriterSettings::new("bybit", "md_collector", "0.1.0", "test", t)
}

fn limits() -> Limits {
    Limits {
        queue_bytes: 1 << 20,
        total_bytes: 1 << 30,
    }
}

fn bybit() -> &'static dyn registry::Venue {
    registry::venue("bybit").expect("bybit")
}

fn frame(recv_ns: i64, conn_id: u32, frame_no: u64, payload: &str) -> RawFrame {
    RawFrame {
        recv_ns,
        src: Src::Ws,
        conn_id,
        frame_no,
        conn_index: 0,
        topic: None,
        for_conn_id: None,
        payload: payload.as_bytes().to_vec(),
    }
}

fn watchdogs(v: &[String]) -> usize {
    v.iter().filter(|m| m.as_str() == "WATCHDOG=1").count()
}

#[test]
fn heartbeat_and_watchdog_without_frames() {
    let dir = test_dir("hb-quiet");
    let sock = Socket::new(&dir);
    let (ing, writer) = ingest::start_with(
        RealFs::new(&dir),
        settings(md_collector::wall_ns()),
        bybit(),
        limits(),
        Some(sock.notifier()),
    )
    .expect("start");
    let t0 = md_collector::wall_ns() / 1_000_000_000;
    std::thread::sleep(Duration::from_millis(3300));
    let got = sock.drain();
    let hb = ing.state().heartbeat_s.expect("a beat");
    assert!(hb >= t0 + 2, "the value advances: {hb} vs start {t0}");
    let n = watchdogs(&got);
    assert!(
        (2..=4).contains(&n),
        "about one WATCHDOG=1 per second: {got:?}"
    );
    assert!(got.iter().all(|m| m == "WATCHDOG=1"), "{got:?}");
    writer.stop().expect("stop");
}

#[test]
fn stalled_writer_stops_beats() {
    let dir = test_dir("hb-stall");
    let sock = Socket::new(&dir);
    let gate = Arc::new(Gate::default());
    let fs = Arc::new(SlowFs {
        inner: RealFs::new(&dir),
        gate: gate.clone(),
    });
    let (ing, writer) = ingest::start_with(
        fs,
        settings(md_collector::wall_ns()),
        bybit(),
        limits(),
        Some(sock.notifier()),
    )
    .expect("start");
    std::thread::sleep(Duration::from_millis(1200));
    assert!(watchdogs(&sock.drain()) >= 1, "beats before the stall");
    gate.close();
    // Frames keep coming; the next flush blocks inside a write.
    let b = book(50, "BTCUSDT", "snapshot", 1, 1);
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut i = 0;
    while gate.waited.load(std::sync::atomic::Ordering::SeqCst) == 0 {
        assert!(
            Instant::now() < deadline,
            "the writer never reached the gate"
        );
        ing.frame(frame(md_collector::wall_ns(), 5, i, &b))
            .expect("accepted");
        i += 1;
        std::thread::sleep(Duration::from_millis(20));
    }
    let _ = sock.drain();
    let hb = ing.state().heartbeat_s;
    for _ in 0..50 {
        let _ = ing.frame(frame(md_collector::wall_ns(), 5, i, &b));
        i += 1;
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(
        ing.state().heartbeat_s,
        hb,
        "the value stops while the writer is stuck"
    );
    assert_eq!(watchdogs(&sock.drain()), 0, "no WATCHDOG=1 while stuck");
    gate.open();
    std::thread::sleep(Duration::from_millis(1500));
    assert!(watchdogs(&sock.drain()) >= 1, "beats again after the stall");
    writer.stop().expect("stop");
}

#[test]
fn failed_write_stops_beats() {
    let dir = test_dir("hb-fail");
    let sock = Socket::new(&dir);
    let gate = Arc::new(Gate::default());
    let fs = Arc::new(SlowFs {
        inner: RealFs::new(&dir),
        gate: gate.clone(),
    });
    let (ing, writer) = ingest::start_with(
        fs,
        settings(md_collector::wall_ns()),
        bybit(),
        limits(),
        Some(sock.notifier()),
    )
    .expect("start");
    gate.fail_with(28);
    let b = book(50, "BTCUSDT", "snapshot", 1, 1);
    ing.frame(frame(md_collector::wall_ns(), 5, 0, &b))
        .expect("accepted");
    let deadline = Instant::now() + Duration::from_secs(5);
    while writer.failed().is_none() {
        assert!(Instant::now() < deadline, "the write error never came");
        std::thread::sleep(Duration::from_millis(20));
    }
    let failed = ing.state().failed_os_error;
    assert_eq!(failed.as_deref(), Some("ENOSPC"));
    let _ = sock.drain();
    let hb = ing.state().heartbeat_s;
    std::thread::sleep(Duration::from_millis(2200));
    assert_eq!(ing.state().heartbeat_s, hb);
    assert_eq!(watchdogs(&sock.drain()), 0, "no beat after a write error");
    assert!(writer.stop().is_err());
}

#[test]
fn abstract_socket_address() {
    use std::os::linux::net::SocketAddrExt;
    let name = format!("md-collector-test-{}", std::process::id());
    let addr = std::os::unix::net::SocketAddr::from_abstract_name(name.as_bytes()).expect("addr");
    let server = UnixDatagram::bind_addr(&addr).expect("bind abstract");
    server
        .set_read_timeout(Some(Duration::from_secs(2)))
        .expect("timeout");
    let n = SdNotify::new(&format!("@{name}")).expect("notifier");
    n.notify("READY=1");
    let mut buf = [0u8; 64];
    let len = server.recv(&mut buf).expect("received");
    assert_eq!(&buf[..len], b"READY=1");
    assert_eq!(n.failed.load(std::sync::atomic::Ordering::SeqCst), 0);
}

#[test]
fn full_socket_never_blocks() {
    let dir = test_dir("hb-full");
    let sock = Socket::new(&dir);
    let n = SdNotify::new(&sock.path).expect("notifier");
    let t0 = Instant::now();
    // Nobody reads: the receive queue fills, sends fail and are counted.
    for _ in 0..10_000 {
        n.notify("WATCHDOG=1");
    }
    assert!(t0.elapsed() < Duration::from_secs(1), "{:?}", t0.elapsed());
    assert!(n.failed.load(std::sync::atomic::Ordering::SeqCst) > 0);
}

/// The done criterion: a venue that sends nothing for 60 s keeps the beat
/// (spec R5: the one test allowed to run 60 s).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn quiet_venue_keeps_heartbeat_60s() {
    let s = setup(
        "hb-quiet-60s",
        Behaviour::default(),
        pages(&[("linear", &["BTCUSDT"])]),
    )
    .await;
    let sock = Socket::new(&s.dir);
    let cfg = s.config(
        &[(
            "linear",
            "symbols = [\"BTCUSDT\"]\nchannels = [\"orderbook.50\"]\nconn_budget = 2",
        )],
        "",
    );
    let mut deps = s.deps();
    deps.notifier = Some(sock.notifier());
    let h = Collector::start(cfg, deps).await.expect("start");
    eventually("the fake's frames are written", || written(&h) >= 4).await;
    let first = sock.drain();
    let ready = first.iter().position(|m| m == "READY=1");
    assert!(ready.is_some(), "READY=1 after the start: {first:?}");
    assert_eq!(
        first.iter().filter(|m| m.as_str() == "READY=1").count(),
        1,
        "{first:?}"
    );
    eventually("the first beat", || h.state().ingest.heartbeat_s.is_some()).await;
    let sent = s.ws.sent_by_topic().values().map(Vec::len).sum::<usize>();
    let mut last = h.state().ingest.heartbeat_s.expect("beat");
    for _ in 0..12 {
        tokio::time::sleep(Duration::from_secs(5)).await;
        let got = sock.drain();
        assert!(watchdogs(&got) >= 3, "WATCHDOG=1 keeps coming: {got:?}");
        let hb = h.state().ingest.heartbeat_s.expect("beat");
        assert!(hb >= last + 3, "the value advances: {hb} after {last}");
        last = hb;
    }
    assert_eq!(
        s.ws.sent_by_topic().values().map(Vec::len).sum::<usize>(),
        sent,
        "the venue was quiet for 60 s"
    );
    h.shutdown().await.expect("shutdown");
}
