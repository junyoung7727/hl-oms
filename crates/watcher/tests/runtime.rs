//! The runtime (C-WATCH part 7, U19a7): `runtime::run` against loopback
//! fakes only: `http-fake` for Bybit, the IP echo, `/api/status` and the
//! VictoriaMetrics import, fake `admin.sock` and `monitor.sock`, a test
//! journal. No test starts the binary with a config and no test sends a
//! signal; the shutdown input of `run` stands in for SIGTERM/SIGINT.

#![allow(clippy::unwrap_used)]

mod common;

use common::TestJournal;
use common::rig::{accepted, acked, of, ours, row};
use common::sock::{FakeSock, oms};
use http_fake::{Answer, FakeHttp, Request};
use serde_json::{Value, json};
use std::os::unix::fs::PermissionsExt;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use watcher::clock::{Clock, ManualClock, SystemClock};
use watcher::config::Config;
use watcher::runtime::{Endpoints, run};

/// 2023-11-14T22:13:20Z.
const T0: u64 = 1_700_000_000_000;

fn ok(result: Value) -> Answer {
    Answer::Reply(
        200,
        json!({"retCode": 0, "retMsg": "OK", "result": result}).to_string(),
    )
}

fn empty_page() -> Answer {
    ok(json!({"category": "linear", "list": [], "nextPageCursor": ""}))
}

/// Every fake of one watcher, its config and its journal.
struct World {
    bybit: FakeHttp,
    echo: FakeHttp,
    status: FakeHttp,
    vm: FakeHttp,
    admin: FakeSock,
    monitor: FakeSock,
    journal: TestJournal,
    cfg: Config,
    lines: Arc<Mutex<Vec<String>>>,
}

fn key() -> Answer {
    ok(json!({"isMaster": false, "userID": 1, "ips": ["127.0.0.1"],
        "readOnly": 1, "expiredAt": "1970-01-01T00:00:00Z"}))
}

impl World {
    /// The fakes answering as a healthy OMS and Bybit would; `extra` is
    /// appended to the config.
    fn new(name: &str, extra: &str) -> World {
        let bybit = FakeHttp::start(Answer::Reply(404, "{}".into()));
        bybit.route("/v5/user/query-api", [key()]);
        bybit.route(
            "/v5/market/instruments-info",
            [ok(json!({"category": "linear",
                "list": [{"symbol": "BTCUSDT", "settleCoin": "USDT"}]}))],
        );
        bybit.route("/v5/order/realtime", [empty_page()]);
        bybit.route("/v5/position/list", [empty_page()]);
        let echo = FakeHttp::start(Answer::Reply(200, "127.0.0.1\n".into()));
        let status = FakeHttp::start(Answer::Reply(404, "{}".into()));
        let vm = FakeHttp::start(Answer::Reply(204, String::new()));
        let mut journal = TestJournal::new(name);
        journal.segment(2);
        let creds_dir = common::test_dir(&format!("{name}-creds"));
        let creds = creds_dir.join("creds.toml");
        std::fs::write(&creds, "api_key = \"k\"\napi_secret = \"s\"\n").unwrap();
        std::fs::set_permissions(&creds, std::fs::Permissions::from_mode(0o600)).unwrap();
        let admin_path = creds_dir.join("admin.sock");
        let monitor_path = creds_dir.join("monitor.sock");
        let admin = FakeSock::serve(&admin_path, |_, _, v| oms(v, "held"));
        let monitor = FakeSock::serve(&monitor_path, |_, _, v| oms(v, "recorded"));
        let text = format!(
            r#"name = "w-rt"
network = "testnet"
credentials_file = "{}"
sub_account_uid = 1
symbols = ["BTCUSDT"]
ip_echo_url = "https://ip.example/"
data_dir = "{}"
status_url = "{}/api/status"
admin_socket = "{}"
monitor_socket = "{}"
host = "h"
[telemetry]
vm_url = "{}"
{extra}"#,
            creds.display(),
            journal.dir.display(),
            status.url(),
            admin_path.display(),
            monitor_path.display(),
            vm.url(),
        );
        let cfg = Config::from_toml(&text).unwrap();
        let lines = Arc::new(Mutex::new(Vec::new()));
        World {
            bybit,
            echo,
            status,
            vm,
            admin,
            monitor,
            journal,
            cfg,
            lines,
        }
    }

    /// `/api/status` with the journal's last record as the cap.
    fn status_ok(&self) {
        let cap = self.journal.next_seq - 1;
        self.status.route(
            "/api/status",
            [Answer::Reply(
                200,
                json!({"journal": {"durable_seq": cap},
                    "core": {"instance_id": "0000000000000007"}})
                .to_string(),
            )],
        );
    }

    fn endpoints(&self) -> Endpoints {
        Endpoints {
            rest_base: self.bybit.url(),
            ip_echo_url: format!("{}/", self.echo.url()),
        }
    }

    /// `run` on its own thread; the sender is the shutdown input.
    fn start(&self, clock: Arc<dyn Clock>) -> Running {
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        let (cfg, ep) = (self.cfg.clone(), self.endpoints());
        let l = self.lines.clone();
        let log: watcher::log::Log = Arc::new(move |line: &str| {
            l.lock().unwrap().push(line.to_string());
        });
        let join = std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            rt.block_on(run(cfg, ep, clock, log, async {
                let _ = rx.await;
            }))
        });
        Running {
            stop: Some(tx),
            join: Some(join),
        }
    }

    fn requests(&self, path: &str) -> Vec<Request> {
        self.bybit
            .requests()
            .into_iter()
            .filter(|r| r.path() == path)
            .collect()
    }

    fn lines_with(&self, pat: &str) -> usize {
        self.lines
            .lock()
            .unwrap()
            .iter()
            .filter(|l| l.contains(pat))
            .count()
    }

    /// The VM import bodies so far.
    fn pushes(&self) -> Vec<String> {
        self.vm.requests().iter().map(|r| r.body_str()).collect()
    }
}

struct Running {
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    join: Option<std::thread::JoinHandle<u8>>,
}

impl Running {
    /// Sends the shutdown input; the exit code, within 10 s.
    fn shutdown(&mut self) -> u8 {
        let _ = self.stop.take().unwrap().send(());
        let join = self.join.take().unwrap();
        let end = Instant::now() + Duration::from_secs(10);
        while !join.is_finished() {
            assert!(Instant::now() < end, "run did not end within 10 s");
            std::thread::sleep(Duration::from_millis(10));
        }
        join.join().unwrap()
    }

    fn running(&self) -> bool {
        self.join.as_ref().is_some_and(|j| !j.is_finished())
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        if let Some(tx) = self.stop.take() {
            let _ = tx.send(());
        }
    }
}

/// Waits (real time, at most `secs`) until `f` holds.
fn until(secs: u64, f: impl Fn() -> bool) -> bool {
    let end = Instant::now() + Duration::from_secs(secs);
    loop {
        if f() {
            return true;
        }
        if Instant::now() >= end {
            return false;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Moves the manual clock `ms` in steps of 100 ms, letting the loop (it
/// polls every 100 ms of real time) see each step.
fn walk(clock: &ManualClock, ms: u64) {
    for _ in 0..ms / 100 {
        clock.advance(100);
        std::thread::sleep(Duration::from_millis(15));
    }
}

fn heartbeats(m: &FakeSock) -> Vec<Value> {
    m.conns()
        .iter()
        .flatten()
        .filter(|v| v["op"] == "heartbeat")
        .cloned()
        .collect()
}

fn mismatch_lines(a: &FakeSock) -> Vec<Value> {
    a.conns()
        .iter()
        .flatten()
        .filter(|v| v["op"] == "watcher_mismatch")
        .cloned()
        .collect()
}

#[test]
fn the_watcher_runs_against_loopback_fakes_end_to_end() {
    let mut w = World::new(
        "rt-e2e",
        "[intervals]\nw1_ms = 500\nw2_ms = 500\nheartbeat_ms = 1000\n\
         [rules]\nmin_age_ms = 500\nmismatch_resend_ms = 1000\n\
         [http]\nrequest_timeout_ms = 1000\n",
    );
    // An order closed in the journal 10 s ago that the venue still lists.
    let now_us = SystemClock.now_ms() * 1000;
    w.journal.append(
        now_us - 10_000_000,
        &[accepted(1, "BTCUSDT", "buy"), acked(1)],
    );
    w.journal
        .append(now_us - 10_000_000, &[of("order_closed", 1)]);
    w.status_ok();
    w.bybit.route(
        "/v5/order/realtime",
        [ok(json!({"category": "linear", "nextPageCursor": "",
            "list": [ours(1, "BTCUSDT", "Buy")]}))],
    );
    let mut run = w.start(Arc::new(SystemClock));
    // The start checks, then rounds of W1 and W2, heartbeats with both
    // success times, the mismatch at admin.sock and the metrics pushes.
    assert!(
        until(20, || {
            let hb = heartbeats(&w.monitor);
            hb.iter()
                .any(|h| h["checks"]["W1"].is_u64() && h["checks"]["W2"].is_u64())
        }),
        "no heartbeat with W1 and W2: {:?}",
        heartbeats(&w.monitor)
    );
    assert!(until(10, || !mismatch_lines(&w.admin).is_empty()));
    let m = &mismatch_lines(&w.admin)[0];
    assert_eq!(m["check"], "W1");
    let d = m["detail"].as_str().unwrap();
    assert!(
        d.starts_with(&format!(
            "W1 closed_in_journal {}: venue open, journal closed; seen 2 rounds",
            watcher::wire_id::encode(7, 1)
        )),
        "{d}"
    );
    let admin = w.admin.conns();
    assert_eq!(
        admin[0][0],
        json!({"id": 1, "op": "hello", "client": "watcher/w-rt"})
    );
    let mon = w.monitor.conns();
    assert_eq!(
        mon[0][0],
        json!({"id": 1, "op": "hello", "client": "watcher/w-rt"})
    );
    // The first heartbeat is all null; every one names the watcher.
    let hb = heartbeats(&w.monitor);
    assert_eq!(
        hb[0]["checks"],
        json!({"W1": null, "W2": null, "W3": null, "W4": null})
    );
    assert!(hb.iter().all(|h| h["watcher"] == "w-rt"));
    // Every Bybit request is signed; the start asked query-api and the
    // settle coin; the checks read open orders and positions.
    for p in [
        "/v5/user/query-api",
        "/v5/market/instruments-info",
        "/v5/order/realtime",
        "/v5/position/list",
    ] {
        let r = w.requests(p);
        assert!(!r.is_empty(), "{p}");
        for h in [
            "x-bapi-api-key",
            "x-bapi-timestamp",
            "x-bapi-recv-window",
            "x-bapi-sign",
        ] {
            assert!(r[0].header(h).is_some(), "{p} {h}");
        }
        assert_eq!(r[0].header("x-bapi-sign-type"), Some("2"));
    }
    assert!(!w.echo.requests().is_empty());
    assert!(until(5, || w
        .pushes()
        .iter()
        .any(|b| b.contains("watcher_heartbeat_ok_ts"))));
    let pushes = w.pushes();
    assert!(
        pushes
            .iter()
            .all(|b| b.contains(r#"{"metric":{"__name__":"watcher_up""#))
    );
    assert!(
        pushes
            .iter()
            .any(|b| b.contains("watcher_mismatches_total"))
    );
    assert_eq!(run.shutdown(), 0);
    assert!(w.monitor.wait(Duration::from_secs(5), |s| s.closed[0]));
}

#[test]
fn without_an_answering_oms_nothing_is_judged() {
    let mut w = World::new(
        "rt-no-oms",
        "[rules]\nmismatch_resend_ms = 1000\n[http]\nrequest_timeout_ms = 1000\n",
    );
    w.journal.append(T0 * 1000 - 60_000_000, &[ev_filler()]);
    w.status
        .route("/api/status", [Answer::Reply(500, "{}".into())]);
    let clock = Arc::new(ManualClock::new(T0));
    let mut run = w.start(clock.clone());
    // The start passes; without /api/status the watcher waits: no check
    // round, no heartbeat; a line every 10 s.
    assert!(until(10, || w
        .requests("/v5/market/instruments-info")
        .len()
        == 1));
    walk(&clock, 25_000);
    assert!(w.requests("/v5/order/realtime").is_empty());
    assert!(w.monitor.conns().is_empty());
    assert!(
        w.status.requests().len() >= 3,
        "{}",
        w.status.requests().len()
    );
    assert_eq!(w.lines_with("waiting for /api/status"), 3);
    // The OMS answers: the checks run and succeed.
    w.status_ok();
    assert!(until(10, || {
        walk(&clock, 1000);
        heartbeats(&w.monitor)
            .iter()
            .any(|h| h["checks"]["W1"].is_u64())
    }));
    // The OMS stops answering while the venue shows a foreign order: no
    // round is judged, nothing is sent, the success times stay.
    w.status
        .route("/api/status", [Answer::Reply(500, "{}".into())]);
    walk(&clock, 6000);
    let last = heartbeats(&w.monitor).last().unwrap()["checks"].clone();
    w.bybit.route(
        "/v5/order/realtime",
        [ok(json!({"category": "linear", "nextPageCursor": "",
            "list": [row("x", "v900", "BTCUSDT", "Buy")]}))],
    );
    walk(&clock, 15_000);
    assert!(mismatch_lines(&w.admin).is_empty());
    let hb = heartbeats(&w.monitor);
    let after = hb.last().unwrap()["checks"].clone();
    assert_eq!(after["W1"], last["W1"]);
    assert_eq!(after["W2"], last["W2"]);
    assert!(w.lines_with("round failed: /api/status") >= 1);
    // It answers again: the foreign order is judged and sent.
    w.status_ok();
    assert!(until(10, || {
        walk(&clock, 1000);
        !mismatch_lines(&w.admin).is_empty()
    }));
    assert_eq!(run.shutdown(), 0);
}

fn ev_filler() -> Value {
    common::filler()
}

#[test]
fn the_shutdown_input_ends_cleanly() {
    let w = World::new("rt-shutdown", "");
    w.status_ok();
    let clock = Arc::new(ManualClock::new(T0));
    let mut run = w.start(clock.clone());
    assert!(until(10, || !heartbeats(&w.monitor).is_empty()));
    walk(&clock, 3000);
    assert!(until(10, || !heartbeats(&w.monitor).is_empty()
        && w.pushes().len() >= 2));
    std::thread::sleep(Duration::from_millis(300));
    let rounds = w.requests("/v5/order/realtime").len();
    let pushes = w.pushes().len();
    assert!(rounds >= 2, "{rounds}");
    assert_eq!(run.shutdown(), 0);
    // One last push at the fake VM, the monitor connection closed, and no
    // round after the shutdown input.
    assert_eq!(w.pushes().len(), pushes + 1);
    assert!(w.monitor.wait(Duration::from_secs(5), |s| s.closed[0]));
    clock.advance(10_000);
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(w.requests("/v5/order/realtime").len(), rounds);
}

#[test]
fn the_ip_recheck_runs_while_the_oms_is_down() {
    let w = World::new(
        "rt-recheck",
        "[intervals]\nip_check_ms = 10000\n[http]\nrequest_timeout_ms = 1000\n",
    );
    w.status.route("/api/status", [Answer::Hang]);
    let clock = Arc::new(ManualClock::new(T0));
    let mut run = w.start(clock.clone());
    assert!(until(10, || w
        .requests("/v5/market/instruments-info")
        .len()
        == 1));
    assert_eq!(w.requests("/v5/user/query-api").len(), 1);
    assert_eq!(w.echo.requests().len(), 1);
    for k in 1..=3 {
        walk(&clock, 10_000);
        assert!(
            until(10, || w.requests("/v5/user/query-api").len() == 1 + k
                && w.echo.requests().len() == 1 + k),
            "recheck {k}"
        );
    }
    // No engine round and no heartbeat while the OMS is down.
    assert!(w.requests("/v5/order/realtime").is_empty());
    assert!(w.requests("/v5/position/list").is_empty());
    assert!(w.monitor.conns().is_empty());
    assert!(!w.status.requests().is_empty());
    assert_eq!(run.shutdown(), 0);
}

#[test]
fn a_key_rejected_at_start_is_exported() {
    let w = World::new("rt-key-rejected", "");
    w.bybit.route(
        "/v5/user/query-api",
        [Answer::Reply(401, "{\"retCode\":10003}".into())],
    );
    // The OMS is down too: nothing but the export shows the state.
    w.status
        .route("/api/status", [Answer::Reply(500, "{}".into())]);
    let clock = Arc::new(ManualClock::new(T0));
    let run = w.start(clock.clone());
    let rejected = r#"{"metric":{"__name__":"watcher_auth_rejected","host":"h","venue":"bybit","watcher":"w-rt"},"values":[1.0]"#;
    assert!(
        until(10, || {
            walk(&clock, 1000);
            w.pushes().iter().any(|b| b.contains(rejected))
        }),
        "{:?}",
        w.pushes()
    );
    let body = w
        .pushes()
        .into_iter()
        .find(|b| b.contains(rejected))
        .unwrap();
    assert!(body.contains(r#"{"metric":{"__name__":"watcher_up","host":"h","venue":"bybit","watcher":"w-rt"},"values":[1.0]"#));
    // The start stays in step 2: no settle coin request, query-api never
    // asked again, nothing else sent to Bybit; the watcher stays up.
    walk(&clock, 10_000);
    assert_eq!(w.requests("/v5/user/query-api").len(), 1);
    assert_eq!(w.bybit.requests().len(), 1);
    assert!(run.running());
    assert_eq!(w.lines_with("Bybit rejected the key (401)"), 1);
    drop(run);
}

#[test]
fn the_status_client_sends_the_loopback_host_and_follows_no_redirect() {
    let fake = FakeHttp::start(Answer::Reply(404, "{}".into()));
    fake.route(
        "/api/status",
        [Answer::Reply(
            200,
            r#"{"journal":{"durable_seq":3}}"#.into(),
        )],
    );
    let url = format!("{}/api/status", fake.url());
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let s = watcher::oms::Status::new(&url, 1000).unwrap();
    let v = rt.block_on(s.get()).unwrap();
    assert_eq!(v["journal"]["durable_seq"], 3);
    let r = &fake.requests()[0];
    assert_eq!(r.method, "GET");
    assert_eq!(
        r.header("host"),
        Some(format!("127.0.0.1:{}", fake.addr().port()).as_str())
    );
    for a in [
        Answer::Reply(302, String::new()),
        Answer::Reply(500, "{}".into()),
        Answer::Reply(200, "not json".into()),
        Answer::Close,
    ] {
        fake.route("/api/status", [a]);
        assert!(rt.block_on(s.get()).is_err());
    }
    assert!(fake.requests().iter().all(|r| r.path() == "/api/status"));
}
