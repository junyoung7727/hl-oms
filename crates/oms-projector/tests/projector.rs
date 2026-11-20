//! The projector (U14b) against a real journal written by `oms-journal`
//! from records the core decided, with loopback fake stores. Status, lock
//! and disk are injected; time is given to each step.

#![allow(clippy::unwrap_used)]

#[path = "../../oms-core/tests/common/mod.rs"]
mod common;

use common::*;
use http_fake::{Answer, FakeHttp};
use obs_push::{ObsConfig, ObsHandle, ObsPush};
use oms_core::*;
use oms_journal::{Options, Writer, recover};
use oms_projector::*;
use serde_json::Value;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const MS: u64 = 1000;

fn tmp(name: &str) -> PathBuf {
    let base = std::env::var("OMS_TEST_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir());
    let d = base.join(format!("oms-projector-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("data")).unwrap();
    d
}

fn opts(segment_max_bytes: u64) -> Options {
    Options {
        segment_max_bytes,
        truncate_torn_tail: true,
        record_schema: 2,
        readable_schemas: vec![1, 2],
        instance_id: 7,
        writer: "test".into(),
    }
}

/// A journal of the records in `h.log`, every one synced. The writer (and
/// so the journal lock) is returned.
fn journal(dir: &Path, h: &H, segment_max_bytes: u64) -> Writer {
    std::fs::write(dir.join("INSTANCE"), format!("{:016x}\n", 7)).unwrap();
    let (mut w, _) = recover(dir, &opts(segment_max_bytes), 0, |_, _, _| Ok(())).unwrap();
    for rec in &h.log {
        w.append(rec.seq, &serde_json::to_vec(rec).unwrap())
            .unwrap();
        w.sync(rec.at).unwrap();
    }
    w
}

/// Buy 2 at 100 (filled), sell 1 at 110 (filled), reference price 105:
/// position 1, realised 10, unrealised 5.
fn trades() -> (H, OrderId, OrderId) {
    let mut h = H::ready();
    let buy = h.accept("a", "k1", Side::Buy, "2", "100");
    h.placed_open(buy, "v1");
    h.fill(buy, "t1", "2", "100");
    h.status(buy, VenueStatus::Filled, Some("2"));
    let sell = h.accept("a", "k2", Side::Sell, "1", "110");
    h.placed_open(sell, "v2");
    h.fill(sell, "t2", "1", "110");
    h.status(sell, VenueStatus::Filled, Some("1"));
    h.ref_price("X", "105");
    (h, buy, sell)
}

#[derive(Clone, Default)]
struct Fakes {
    status: Arc<Mutex<Option<OmsStatus>>>,
    held: Arc<AtomicBool>,
    free: Arc<AtomicU64>,
    lock_probes: Arc<AtomicU64>,
}

struct St(Arc<Mutex<Option<OmsStatus>>>);
impl StatusSource for St {
    fn poll(&mut self) -> Option<OmsStatus> {
        self.0.lock().unwrap().clone()
    }
}
struct Lk(Arc<AtomicBool>, Arc<AtomicU64>);
impl LockProbe for Lk {
    fn held(&self) -> io::Result<bool> {
        self.1.fetch_add(1, Ordering::SeqCst);
        Ok(self.0.load(Ordering::SeqCst))
    }
}
struct Dk(Arc<AtomicU64>);
impl FreeSpace for Dk {
    fn free_bytes(&self) -> io::Result<u64> {
        Ok(self.0.load(Ordering::SeqCst))
    }
}

impl Fakes {
    fn alive(&self, durable_seq: u64) {
        *self.status.lock().unwrap() = Some(OmsStatus {
            durable_seq,
            as_of_us: 1_700_000_000_000_000,
            wire_id_form: Some(WireIdForm::Base32Pair),
        });
    }
    fn silent(&self) {
        *self.status.lock().unwrap() = None;
    }
}

struct Stores {
    vm: FakeHttp,
    vl: FakeHttp,
}

fn stores() -> Stores {
    Stores {
        vm: FakeHttp::start(Answer::Reply(204, String::new())),
        vl: FakeHttp::start(Answer::Reply(200, String::new())),
    }
}

fn obs(s: &Stores) -> ObsHandle {
    let mut c = ObsConfig::new(Some(s.vm.url()), Some(s.vl.url()));
    c.flush_every = Duration::from_millis(10);
    c.backoff_min = Duration::from_millis(10);
    c.backoff_max = Duration::from_millis(20);
    c.timeout = Duration::from_millis(300);
    c.self_every = Duration::from_secs(3600);
    ObsPush::start(c).unwrap()
}

fn settings(root: &Path) -> Settings {
    Settings {
        data_dir: root.join("data"),
        cursor_file: root.join("cursor"),
        venue: "bybit".into(),
        host: "h1".into(),
        max_records: 1000,
        max_pending_lines: 10_000,
        dead_after_us: 10_000 * MS,
        disk_every_us: 10_000 * MS,
        logs_on: true,
    }
}

fn projector(root: &Path, f: &Fakes, o: Option<ObsHandle>, now: u64) -> Projector {
    Projector::new(
        settings(root),
        Box::new(St(f.status.clone())),
        Box::new(Lk(f.held.clone(), f.lock_probes.clone())),
        Box::new(Dk(f.free.clone())),
        o,
        now,
    )
    .unwrap()
}

/// Every VictoriaLogs line received so far, parsed.
fn vl_lines(s: &Stores) -> Vec<Value> {
    s.vl.requests()
        .iter()
        .filter(|r| r.path() == "/insert/jsonline")
        .flat_map(|r| {
            r.body_str()
                .lines()
                .map(|l| serde_json::from_str::<Value>(l).unwrap())
                .collect::<Vec<_>>()
        })
        .collect()
}

fn raw_vl_lines(s: &Stores) -> Vec<String> {
    s.vl.requests()
        .iter()
        .filter(|r| r.path() == "/insert/jsonline")
        .flat_map(|r| r.body_str().lines().map(str::to_string).collect::<Vec<_>>())
        .collect()
}

fn vm_lines(s: &Stores) -> Vec<String> {
    s.vm.requests()
        .iter()
        .filter(|r| r.path() == "/api/v1/import")
        .flat_map(|r| r.body_str().lines().map(str::to_string).collect::<Vec<_>>())
        .collect()
}

/// The last pushed value of a series with exactly these labels.
fn last_value(s: &Stores, name: &str, labels: &[(&str, &str)]) -> Option<f64> {
    vm_lines(s)
        .iter()
        .map(|l| serde_json::from_str::<Value>(l).unwrap())
        .filter(|v| {
            let m = v["metric"].as_object().unwrap();
            m["__name__"] == name
                && m.len() == labels.len() + 2
                && labels
                    .iter()
                    .all(|(k, val)| m.get(*k).and_then(Value::as_str) == Some(*val))
        })
        .last()
        .map(|v| v["values"][0].as_f64().unwrap())
}

/// Steps until `done` holds, at most 3 s.
fn step_until(p: &mut Projector, now: &mut u64, mut done: impl FnMut(&Projector) -> bool) {
    let start = Instant::now();
    loop {
        p.step(*now);
        *now += 100 * MS;
        if done(p) {
            return;
        }
        assert!(
            start.elapsed() < Duration::from_secs(3),
            "condition not reached; cursor {}",
            p.cursor()
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn seqs_of(lines: &[Value]) -> Vec<u64> {
    let mut v: Vec<u64> = lines
        .iter()
        .filter(|l| l["service"] == "oms")
        .map(|l| l["seq"].as_u64().unwrap())
        .collect();
    v.dedup();
    v
}

#[test]
fn event_lines_and_metric_lines_have_the_documented_shape() {
    let root = tmp("shape");
    let (h, buy, _) = trades();
    let _w = journal(&root.join("data"), &h, 1 << 20);
    let (f, s) = (Fakes::default(), stores());
    f.free.store(123_456, Ordering::SeqCst);
    let last = h.log.len() as u64;
    f.alive(last);
    let mut now = 1_800_000_000_000_000;
    let mut p = projector(&root, &f, Some(obs(&s)), now);
    step_until(&mut p, &mut now, |p| p.cursor() == last);

    // The fill of the buy order: exact line.
    let fill_rec = h
        .log
        .iter()
        .find(|r| {
            r.events
                .iter()
                .any(|e| matches!(e, Event::FillBooked { order_id, .. } if *order_id == buy))
        })
        .unwrap();
    let idx = fill_rec
        .events
        .iter()
        .position(|e| matches!(e, Event::FillBooked { .. }))
        .unwrap();
    let wire = WireIdForm::Base32Pair.encode(VenueClientId {
        instance_id: 7,
        order_id: buy,
    });
    let expected = format!(
        concat!(
            r#"{{"_time":"{}","_msg":"fill_booked seq={} order={} X buy qty=2 px=100","service":"oms","host":"h1","venue":"bybit","#,
            r#""event_index":{},"fill_key":"t1","kind":"fill_booked","order_id":{},"phase":"closed","px":"100","qty":"2","seq":{},"side":"buy","symbol":"X","venue_ts_ns":{},"wire_id":"{}"}}"#
        ),
        obs_push::rfc3339_ns(fill_rec.at as i64 * 1000),
        fill_rec.seq,
        buy.0,
        idx,
        buy.0,
        fill_rec.seq,
        match &fill_rec.events[idx] {
            Event::FillBooked { venue_ts, .. } => venue_ts * 1000,
            _ => unreachable!(),
        },
        wire,
    );
    let raw = raw_vl_lines(&s);
    assert!(
        raw.contains(&expected),
        "expected line\n{expected}\nnot among\n{}",
        raw.join("\n")
    );
    // Every event of every record has one line, in order.
    let n_events: usize = h.log.iter().map(|r| r.events.len()).sum();
    let lines = vl_lines(&s);
    assert_eq!(
        lines.iter().filter(|l| l["service"] == "oms").count(),
        n_events
    );
    assert_eq!(seqs_of(&lines), (1..=last).collect::<Vec<_>>());
    assert!(lines.iter().all(|l| l.get("unconfirmed").is_none()));
    // The request URL names the stream fields.
    assert!(
        s.vl.requests()
            .iter()
            .all(|r| r.query() == "_stream_fields=service,host,venue")
    );

    // Metrics: values and exact line shape.
    let v = |name: &str, labels: &[(&str, &str)]| last_value(&s, name, labels);
    assert_eq!(v("oms_position_qty", &[("symbol", "X")]), Some(1.0));
    assert_eq!(v("oms_realized_pnl", &[("symbol", "X")]), Some(10.0));
    assert_eq!(v("oms_upnl", &[("symbol", "X")]), Some(5.0));
    assert_eq!(v("oms_up", &[]), Some(1.0));
    assert_eq!(v("oms_journal_durable_seq", &[]), Some(last as f64));
    assert_eq!(v("oms_open_orders", &[]), Some(0.0));
    assert_eq!(v("oms_in_doubt_orders", &[]), Some(0.0));
    assert_eq!(v("oms_halt", &[]), Some(0.0));
    assert_eq!(v("oms_heartbeat_ts", &[]), Some(1_700_000_000.0));
    assert_eq!(v("projector_lag_records", &[]), Some(0.0));
    assert_eq!(v("journal_disk_free_bytes", &[]), Some(123_456.0));
    assert_eq!(
        v("oms_limit", &[("kind", "position_max"), ("symbol", "X")]),
        Some(10.0)
    );
    assert_eq!(
        v(
            "oms_limit_distance",
            &[("kind", "position_max"), ("symbol", "X")]
        ),
        Some(9.0)
    );
    assert_eq!(
        v(
            "oms_limit_distance",
            &[("kind", "position_min"), ("symbol", "X")]
        ),
        Some(11.0)
    );
    assert_eq!(
        v("oms_limit_distance", &[("kind", "open_orders")]),
        Some(500.0)
    );
    let pos_line = vm_lines(&s)
        .into_iter()
        .find(|l| l.contains(r#""__name__":"oms_position_qty""#))
        .unwrap();
    let ts = serde_json::from_str::<Value>(&pos_line).unwrap()["timestamps"][0]
        .as_i64()
        .unwrap();
    assert_eq!(
        pos_line,
        format!(
            r#"{{"metric":{{"__name__":"oms_position_qty","symbol":"X","venue":"bybit"}},"values":[1.0],"timestamps":[{ts}]}}"#
        )
    );
    // No order id is a label.
    assert!(vm_lines(&s).iter().all(|l| !l.contains("order_id")));
    // The replayed state is the core's.
    assert_eq!(p.core(), &h.core);
}

#[test]
fn nothing_past_the_durable_sequence_while_the_oms_holds_the_lock() {
    let root = tmp("cap");
    let (h, _, _) = trades();
    let _w = journal(&root.join("data"), &h, 1 << 20);
    let (f, s) = (Fakes::default(), stores());
    f.alive(3);
    let mut now = 1_800_000_000_000_000;
    let mut p = projector(&root, &f, Some(obs(&s)), now);
    step_until(&mut p, &mut now, |p| p.cursor() == 3);
    // The OMS stops answering but still holds the lock: not dead.
    f.silent();
    f.held.store(true, Ordering::SeqCst);
    for _ in 0..5 {
        let rep = p.step(now);
        assert!(!rep.dead);
        assert_eq!(rep.unconfirmed_lines, 0);
        now += 5_000 * MS;
    }
    assert!(
        f.lock_probes.load(Ordering::SeqCst) > 0,
        "the lock was never probed"
    );
    std::thread::sleep(Duration::from_millis(150));
    assert_eq!(seqs_of(&vl_lines(&s)), vec![1, 2, 3]);
    assert_eq!(p.applied_seq(), 3);
    assert_eq!(last_value(&s, "oms_up", &[]), Some(0.0));
}

#[test]
fn a_dead_oms_shows_unconfirmed_lines_that_are_emitted_again_confirmed() {
    let root = tmp("dead");
    let (h, _, _) = trades();
    // Small segments: the records span several segments.
    let _w = journal(&root.join("data"), &h, 1500);
    assert!(
        oms_journal::list_segments(&root.join("data"))
            .unwrap()
            .len()
            > 2
    );
    let last = h.log.len() as u64;
    let (f, s) = (Fakes::default(), stores());
    f.alive(4);
    let mut now = 1_800_000_000_000_000;
    let mut p = projector(&root, &f, Some(obs(&s)), now);
    step_until(&mut p, &mut now, |p| p.cursor() == 4);
    // No answer and no lock holder, but not yet for 10 s: not dead.
    f.silent();
    let rep = p.step(now + 9_000 * MS);
    assert!(!rep.dead);
    assert_eq!(rep.unconfirmed_lines, 0);
    // 10 s without an answer: dead. Records 5.. are shown unconfirmed, once.
    now += 10_000 * MS;
    let rep = p.step(now);
    assert!(rep.dead);
    let n_beyond: usize = h.log[4..].iter().map(|r| r.events.len()).sum();
    assert_eq!(rep.unconfirmed_lines, n_beyond);
    assert_eq!(
        p.step(now + 1000 * MS).unconfirmed_lines,
        0,
        "shown twice in one dead period"
    );
    assert_eq!(p.cursor(), 4, "the cursor moved past the durable sequence");
    assert_eq!(
        p.applied_seq(),
        4,
        "an unconfirmed record changed the state"
    );
    std::thread::sleep(Duration::from_millis(100));
    let unconfirmed: Vec<Value> = vl_lines(&s)
        .into_iter()
        .filter(|l| l.get("unconfirmed").is_some())
        .collect();
    assert_eq!(unconfirmed.len(), n_beyond);
    assert!(
        unconfirmed
            .iter()
            .all(|l| l["unconfirmed"] == true && l.get("phase").is_none())
    );
    assert_eq!(seqs_of(&unconfirmed), (5..=last).collect::<Vec<_>>());
    assert!(
        unconfirmed
            .iter()
            .all(|l| l["_msg"].as_str().unwrap().ends_with("(unconfirmed)"))
    );
    // The OMS is back and its durable sequence passes them: emitted again
    // without the flag.
    f.alive(last);
    now += 2000 * MS;
    step_until(&mut p, &mut now, |p| p.cursor() == last);
    let confirmed: Vec<Value> = vl_lines(&s)
        .into_iter()
        .filter(|l| {
            l["service"] == "oms"
                && l.get("unconfirmed").is_none()
                && l["seq"].as_u64().unwrap() > 4
        })
        .collect();
    assert_eq!(confirmed.len(), n_beyond);
    assert_eq!(seqs_of(&confirmed), (5..=last).collect::<Vec<_>>());
    assert_eq!(p.core(), &h.core);
}

#[test]
fn after_a_crash_the_projector_resumes_from_its_cursor_with_the_same_seqs() {
    let root = tmp("crash");
    let (h, _, _) = trades();
    let _w = journal(&root.join("data"), &h, 1 << 20);
    let last = h.log.len() as u64;
    let (f, s) = (Fakes::default(), stores());
    f.alive(5);
    let mut now = 1_800_000_000_000_000;
    {
        let mut p = projector(&root, &f, Some(obs(&s)), now);
        step_until(&mut p, &mut now, |p| p.cursor() == 5);
        // The store stops answering; records 6.. are pushed but never
        // taken, then the projector dies.
        s.vl.set_default(Answer::Hang);
        f.alive(last);
        for _ in 0..3 {
            p.step(now);
            now += 100 * MS;
        }
        assert_eq!(p.applied_seq(), last);
        assert_eq!(p.cursor(), 5);
    }
    let cursor_text = std::fs::read_to_string(root.join("cursor")).unwrap();
    assert_eq!(
        cursor_text,
        "{\"instance_id\":\"0000000000000007\",\"seq\":5}\n"
    );
    // A new process with a store that answers: lines from 6, none before.
    let s2 = stores();
    let mut p = projector(&root, &f, Some(obs(&s2)), now);
    step_until(&mut p, &mut now, |p| p.cursor() == last);
    assert_eq!(seqs_of(&vl_lines(&s2)), (6..=last).collect::<Vec<_>>());
    // State rebuilt from the first record: same as the core and as a run
    // without the crash.
    assert_eq!(p.core(), &h.core);
    assert_eq!(
        last_value(&s2, "oms_realized_pnl", &[("symbol", "X")]),
        Some(10.0)
    );
}

#[test]
fn a_store_that_refuses_keeps_the_cursor_until_it_takes_the_lines() {
    let root = tmp("vldown");
    let (h, _, _) = trades();
    let _w = journal(&root.join("data"), &h, 1 << 20);
    let last = h.log.len() as u64;
    let (f, s) = (Fakes::default(), stores());
    s.vl.set_default(Answer::Reply(503, String::new()));
    f.alive(last);
    let mut now = 1_800_000_000_000_000;
    let mut p = projector(&root, &f, Some(obs(&s)), now);
    for _ in 0..10 {
        p.step(now);
        now += 100 * MS;
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(p.applied_seq(), last);
    assert_eq!(p.cursor(), 0);
    assert!(!root.join("cursor").exists());
    s.vl.set_default(Answer::Reply(200, String::new()));
    step_until(&mut p, &mut now, |p| p.cursor() == last);
}

#[test]
fn damage_at_or_below_the_durable_sequence_stops_without_moving_the_cursor() {
    let root = tmp("crc");
    let (h, _, _) = trades();
    let data = root.join("data");
    drop(journal(&data, &h, 1 << 20));
    let last = h.log.len() as u64;
    // Flip one byte in the middle of the segment: some record ≤ last breaks.
    let (_, seg) = oms_journal::list_segments(&data).unwrap().pop().unwrap();
    let mut bytes = std::fs::read(&seg).unwrap();
    let at = bytes.len() * 2 / 3;
    bytes[at] ^= 0xff;
    std::fs::write(&seg, &bytes).unwrap();
    let (f, s) = (Fakes::default(), stores());
    f.alive(last);
    let mut now = 1_800_000_000_000_000;
    let mut p = projector(&root, &f, Some(obs(&s)), now);
    let mut rep = StepReport::default();
    for _ in 0..20 {
        rep = p.step(now);
        now += 100 * MS;
        std::thread::sleep(Duration::from_millis(10));
    }
    let err = rep.tail_error.expect("no tail error");
    assert!(p.applied_seq() < last, "read past the damage: {err}");
    assert_eq!(p.cursor(), p.applied_seq());
    // The good records before the damage were emitted; nothing after it.
    std::thread::sleep(Duration::from_millis(100));
    let lines = vl_lines(&s);
    assert_eq!(seqs_of(&lines), (1..=p.applied_seq()).collect::<Vec<_>>());
    let errors: Vec<&Value> = lines
        .iter()
        .filter(|l| l["service"] == "oms-projector")
        .collect();
    assert_eq!(errors.len(), 1, "the stop is logged once: {errors:?}");
    assert_eq!(errors[0]["level"], "error");
    assert_eq!(last_value(&s, "projector_tail_error", &[]), Some(1.0));
}

#[test]
fn the_projector_never_takes_the_journal_lock() {
    let root = tmp("lock");
    let (h, _, _) = trades();
    let data = root.join("data");
    drop(journal(&data, &h, 1 << 20));
    let f = Fakes::default();
    f.alive(h.log.len() as u64);
    let mut now = 1_800_000_000_000_000;
    let mut p = projector(&root, &f, None, now);
    for _ in 0..3 {
        p.step(now);
        now += 100 * MS;
    }
    assert_eq!(p.applied_seq(), h.log.len() as u64);
    // While the projector is alive, an OMS can open (and lock) the journal.
    let (w, _) = recover(&data, &opts(1 << 20), 0, |_, _, _| Ok(())).unwrap();
    // The real probe sees that lock, and the projector still reads.
    assert!(ProcLocks::new(&data).held().unwrap());
    p.step(now);
    drop(w);
    assert!(!ProcLocks::new(&data).held().unwrap());
    // Without telemetry the cursor follows the applied records.
    assert_eq!(p.cursor(), h.log.len() as u64);
}

#[test]
fn the_lock_probe_reads_proc_locks_by_inode() {
    let text = "1: FLOCK  ADVISORY  WRITE 4242 103:02:5245009 0 EOF\n\
                1: -> FLOCK  ADVISORY  WRITE 4243 103:02:5245009 0 EOF\n\
                2: POSIX  ADVISORY  READ 77 00:1a:12 128 128\n";
    assert_eq!(
        oms_projector::probe::locked_inodes(text),
        vec![5245009, 5245009, 12]
    );
    let root = tmp("probe");
    let data = root.join("data");
    let probe = ProcLocks::new(&data);
    assert!(!probe.held().unwrap(), "no LOCK file");
    let f = std::fs::File::create(data.join("LOCK")).unwrap();
    assert!(!probe.held().unwrap(), "LOCK exists but nobody holds it");
    f.try_lock().unwrap();
    assert!(probe.held().unwrap());
    f.unlock().unwrap();
    assert!(!probe.held().unwrap());
}

#[test]
fn the_http_status_source_reads_the_oms_status_answer() {
    let fake = FakeHttp::start(Answer::Reply(
        200,
        r#"{"as_of":1700000000000001,"node":{"wire_id_form":"base32_pair"},"journal":{"durable_seq":42}}"#.into(),
    ));
    let mut src = HttpStatus::new(
        &format!("{}/api/status", fake.url()),
        Duration::from_millis(500),
    )
    .unwrap();
    assert_eq!(
        src.poll(),
        Some(OmsStatus {
            durable_seq: 42,
            as_of_us: 1_700_000_000_000_001,
            wire_id_form: Some(WireIdForm::Base32Pair),
        })
    );
    fake.set_default(Answer::Reply(500, "{}".into()));
    assert_eq!(src.poll(), None);
    fake.set_default(Answer::Hang);
    assert_eq!(src.poll(), None);
    // The names are the ones the OMS writes.
    for form in [WireIdForm::Hex128, WireIdForm::Base32Pair] {
        let name = serde_json::to_value(form).unwrap();
        assert_eq!(
            oms_projector::probe::wire_id_form(name.as_str().unwrap()),
            Some(form)
        );
    }
}

#[test]
fn the_cursor_of_another_journal_is_refused() {
    let root = tmp("other");
    let (h, _, _) = trades();
    drop(journal(&root.join("data"), &h, 1 << 20));
    oms_projector::cursor::store(&root.join("cursor"), 8, 3).unwrap();
    let f = Fakes::default();
    let err = Projector::new(
        settings(&root),
        Box::new(St(f.status.clone())),
        Box::new(Lk(f.held.clone(), f.lock_probes.clone())),
        Box::new(Dk(f.free.clone())),
        None,
        0,
    )
    .err()
    .unwrap();
    assert!(err.contains("instance 0000000000000008"), "{err}");
}

#[test]
fn the_config_refuses_a_status_url_that_is_not_loopback_and_unknown_keys() {
    let base = "data_dir = \"/d\"\nvenue = \"bybit\"\ncursor_file = \"/c\"\n";
    assert!(
        Config::parse(&format!(
            "{base}status_url = \"http://127.0.0.1:8080/api/status\"\n"
        ))
        .is_ok()
    );
    let err = Config::parse(&format!(
        "{base}status_url = \"http://10.0.0.2:8080/api/status\"\n"
    ))
    .unwrap_err();
    assert!(err.contains("loopback"), "{err}");
    assert!(
        Config::parse(&format!(
            "{base}status_url = \"http://127.0.0.1:1/\"\nfoo = 1\n"
        ))
        .is_err()
    );
    let err = Config::parse(&format!(
        "{base}status_url = \"http://127.0.0.1:1/\"\nmax_pending_lines = 200000\n[telemetry]\nvl_url = \"http://127.0.0.1:9428\"\n"
    ))
    .unwrap_err();
    assert!(err.contains("queue_lines"), "{err}");
}

#[test]
fn lines_the_push_queue_dropped_are_pushed_again_before_the_cursor_moves() {
    let root = tmp("dropped");
    let (h, _, _) = trades();
    let _w = journal(&root.join("data"), &h, 1 << 20);
    let last = h.log.len() as u64;
    let (f, s) = (Fakes::default(), stores());
    s.vl.set_default(Answer::Reply(503, String::new()));
    f.alive(last);
    // A queue of 4 lines: the first pass overflows it while the store
    // refuses.
    let mut c = ObsConfig::new(Some(s.vm.url()), Some(s.vl.url()));
    c.queue_lines = 4;
    // (Below the pending limit on purpose: a config refuses that.)
    c.flush_every = Duration::from_millis(10);
    c.backoff_min = Duration::from_millis(10);
    c.backoff_max = Duration::from_millis(20);
    c.timeout = Duration::from_millis(300);
    let o = ObsPush::start(c).unwrap();
    let mut now = 1_800_000_000_000_000;
    let mut p = projector(&root, &f, Some(o), now);
    p.step(now);
    std::thread::sleep(Duration::from_millis(50));
    s.vl.set_default(Answer::Reply(200, String::new()));
    step_until(&mut p, &mut now, |p| p.cursor() == last);
    // Every record reached the store at least once.
    let got = seqs_of(&vl_lines(&s));
    let mut uniq = got.clone();
    uniq.sort_unstable();
    uniq.dedup();
    assert_eq!(uniq, (1..=last).collect::<Vec<_>>());
    // One rewind: after it the records go one at a time and the queue does
    // not overflow again.
    let rewinds = vl_lines(&s)
        .into_iter()
        .filter(|l| l["service"] == "oms-projector" && l["level"] == "warn")
        .count();
    assert_eq!(rewinds, 1);
}
