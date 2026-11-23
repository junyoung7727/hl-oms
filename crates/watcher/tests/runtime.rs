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
use watcher::alert::Route;
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
    /// The Telegram Bot API (U19b1c); used when the config has
    /// `[telegram]`.
    telegram: FakeHttp,
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
        bybit.route("/v5/execution/list", [empty_page()]);
        bybit.route(
            "/v5/account/wallet-balance",
            [ok(json!({"list": [{"accountType": "UNIFIED", "totalEquity": "1000",
                "totalMaintenanceMargin": "0", "totalMarginBalance": "1000"}]}))],
        );
        let echo = FakeHttp::start(Answer::Reply(200, "127.0.0.1\n".into()));
        let status = FakeHttp::start(Answer::Reply(404, "{}".into()));
        let vm = FakeHttp::start(Answer::Reply(204, String::new()));
        let telegram = FakeHttp::start(Answer::Reply(200, r#"{"ok":true,"result":{}}"#.into()));
        let mut journal = TestJournal::new(name);
        journal.segment(2);
        // Short names: a socket path has at most 107 bytes (review 1 N2).
        let creds_dir = common::test_dir(&format!("{name}-c"));
        let creds = creds_dir.join("creds.toml");
        std::fs::write(&creds, "api_key = \"k\"\napi_secret = \"s\"\n").unwrap();
        std::fs::set_permissions(&creds, std::fs::Permissions::from_mode(0o600)).unwrap();
        let admin_path = creds_dir.join("a.sock");
        let monitor_path = creds_dir.join("m.sock");
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
            telegram,
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
            telegram: vec![Route::direct(&self.telegram.url())],
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

    /// The texts the fake Telegram got so far.
    fn sent(&self) -> Vec<String> {
        self.telegram
            .requests()
            .iter()
            .map(|r| {
                let v: Value = serde_json::from_slice(&r.body).unwrap();
                v["text"].as_str().unwrap().to_string()
            })
            .collect()
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

/// Walks the clock to the `k`-th IP recheck after the start (ip_check_ms =
/// 10 s). The recheck is due 10 s after the loop read the last echo answer,
/// and that answer can reach the loop after the fake saw the request and
/// after the walk went on (a loaded machine): none is sent in the first
/// 9.9 s, then the walk goes on in steps until query-api and the echo are
/// both asked, at most 10 s more (판단(모델 아님)). The exact pace is
/// start.rs's.
fn walk_to_recheck(w: &World, clock: &ManualClock, k: usize) {
    let asked = || {
        (
            w.requests("/v5/user/query-api").len(),
            w.echo.requests().len(),
        )
    };
    walk(clock, 9_900);
    assert_eq!(asked(), (k, k), "recheck {k} before 10 s");
    for _ in 0..100 {
        walk(clock, 100);
        if until(1, || asked() == (k + 1, k + 1)) {
            return;
        }
    }
    panic!("recheck {k}: {:?}", asked());
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
        "[intervals]\nw1_ms = 500\nw2_ms = 500\nw3_ms = 1000\nw4_ms = 1000\nheartbeat_ms = 1000\n\
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
    // The start checks, then rounds of W1, W2, W3 and W4, heartbeats with
    // the success times, the mismatch at admin.sock and the metrics pushes.
    assert!(
        until(20, || {
            let hb = heartbeats(&w.monitor);
            hb.iter().any(|h| {
                h["checks"]["W1"].is_u64()
                    && h["checks"]["W2"].is_u64()
                    && h["checks"]["W3"].is_u64()
                    && h["checks"]["W4"].is_u64()
            })
        }),
        "no heartbeat with W1, W2, W3 and W4: {:?}",
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
    // settle coin; the checks read open orders, positions, executions and
    // the wallet.
    for p in [
        "/v5/user/query-api",
        "/v5/market/instruments-info",
        "/v5/order/realtime",
        "/v5/position/list",
        "/v5/execution/list",
        "/v5/account/wallet-balance",
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
        walk_to_recheck(&w, &clock, k);
    }
    // No engine round and no heartbeat while the OMS is down.
    assert!(w.requests("/v5/order/realtime").is_empty());
    assert!(w.requests("/v5/position/list").is_empty());
    assert!(w.monitor.conns().is_empty());
    assert!(!w.status.requests().is_empty());
    assert_eq!(run.shutdown(), 0);
}

#[test]
fn the_ip_recheck_runs_during_the_boot_replay() {
    let mut w = World::new(
        "rt-recheck-replay",
        "[intervals]\nip_check_ms = 10000\n[http]\nrequest_timeout_ms = 1000\n",
    );
    w.journal.append(T0 * 1000 - 60_000_000, &[ev_filler()]);
    // The status cap is above the journal's last record: the boot replay
    // reads again every second (the record after the last is missing, a
    // retried journal error) and does not end.
    let cap = w.journal.next_seq + 5;
    w.status.route(
        "/api/status",
        [Answer::Reply(
            200,
            json!({"journal": {"durable_seq": cap},
                "core": {"instance_id": "0000000000000007"}})
            .to_string(),
        )],
    );
    let clock = Arc::new(ManualClock::new(T0));
    let mut run = w.start(clock.clone());
    assert!(until(10, || w.lines_with("read again in 1 s") == 1));
    assert_eq!(w.requests("/v5/user/query-api").len(), 1);
    assert_eq!(w.echo.requests().len(), 1);
    for k in 1..=3 {
        walk_to_recheck(&w, &clock, k);
    }
    // Still in the replay: no engine round, no heartbeat.
    assert_eq!(w.lines_with("read again in 1 s"), 1);
    assert_eq!(w.lines_with("boot replay done"), 0);
    assert!(w.requests("/v5/order/realtime").is_empty());
    assert!(w.requests("/v5/position/list").is_empty());
    assert!(w.monitor.conns().is_empty());
    assert_eq!(run.shutdown(), 0);
}

#[test]
fn a_recheck_that_fails_a_start_rule_stops_every_bybit_request() {
    let w = World::new(
        "rt-recheck-stop",
        "[intervals]\nip_check_ms = 10000\n[http]\nrequest_timeout_ms = 1000\n",
    );
    w.status_ok();
    let clock = Arc::new(ManualClock::new(T0));
    let mut run = w.start(clock.clone());
    // The checks run.
    assert!(until(10, || {
        walk(&clock, 1000);
        !w.requests("/v5/order/realtime").is_empty() && !w.requests("/v5/position/list").is_empty()
    }));
    // The key becomes the master account's: the next recheck stops every
    // Bybit request until restart.
    w.bybit.route(
        "/v5/user/query-api",
        [ok(
            json!({"isMaster": true, "userID": 1, "ips": ["127.0.0.1"]}),
        )],
    );
    let rejected = r#"{"metric":{"__name__":"watcher_auth_rejected","host":"h","venue":"bybit","watcher":"w-rt"},"values":[1.0]"#;
    assert!(
        until(20, || {
            walk(&clock, 1000);
            w.pushes().iter().any(|b| b.contains(rejected))
        }),
        "{:?}",
        w.lines.lock().unwrap()
    );
    assert_eq!(
        w.lines_with("error: the key no longer passes the start check: query-api"),
        1
    );
    // Without [telegram] the auth P1 is one warn line (U19b1c).
    assert_eq!(
        w.lines_with(
            "warn: [WATCH P1] (no [telegram]: not sent) 워처 키가 시작 규칙을 더는 지키지 않음(the key no longer passes the start check: query-api"
        ),
        1
    );
    // Requests already on the wire may end; after that nothing reaches
    // Bybit, through rounds, rechecks and 30 s of the clock.
    std::thread::sleep(Duration::from_millis(300));
    let seen = w.bybit.requests().len();
    walk(&clock, 30_000);
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(w.bybit.requests().len(), seen);
    assert!(w.lines_with("round failed") >= 1);
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
    // Before the engine runs, every check's age counts from the config
    // load (U19a7; W3 from U19a9).
    let last = w.pushes().pop().unwrap();
    for c in ["W1", "W2", "W3", "W4"] {
        let age = format!(r#"{{"metric":{{"__name__":"watcher_check_age_s","check":"{c}","#);
        assert!(last.contains(&age), "{c}: {last}");
    }
    drop(run);
}

#[test]
fn the_runtime_runs_w2_w3_and_w4_and_w2_reads_w4s_exec_times() {
    // The checks the runtime adds after the boot replay, on the engine rig
    // and a manual clock (default intervals: W2 1 s, W3 2 s, W4 5 s).
    let mut r = common::rig::Rig::new("rt-checks");
    let cfg = common::rig::config();
    for (logic, interval) in watcher::runtime::checks(&cfg, &r.log) {
        r.engine.add_check(logic, interval);
    }
    // A fill the journal booked 60 s ago, listed by the venue with an
    // execTime 100 ms before T0; the venue holds 2 without updatedTime,
    // the journal 1.
    let fill = json!({"external_fill_booked": {"fill": {"fee": "0", "instrument": "BTCUSDT",
        "key": "BTCUSDT:x", "order_id": null, "px": "100", "qty": "1", "side": "buy",
        "venue_oid": null, "venue_ts": (T0 - 100) * 1000}}});
    r.write(T0 - 60_000, &[fill]);
    r.venue.executions = vec![common::rig::exec("BTCUSDT", "x", "Trade", "Buy", "1", T0 - 100)];
    r.venue.positions = vec![common::rig::pos("BTCUSDT", "Buy", "2", None)];
    r.venue.wallet = vec![common::rig::account("1000", "0", "1000")];
    r.at(T0);
    for c in watcher::engine::Check::ALL {
        assert_eq!(r.engine.success(c), Some(T0), "{c:?}");
    }
    // W4's execTime defers W2 at +1 s; confirmed at +2 s. Each check at
    // its own interval (review 1 S1): W3 every 2 s, W4 every 5 s, neither
    // at another check's pace.
    use watcher::engine::Check::{W3, W4};
    let mut seen = Vec::new();
    for t in (1..=5).map(|s| T0 + s * 1000) {
        r.at(t);
        if t == T0 + 1000 {
            assert!(r.confirmed.is_empty(), "{:?}", r.confirmed);
        }
        seen.push((r.engine.success(W3), r.engine.success(W4)));
    }
    let d = "W2 position BTCUSDT: venue 2, journal 1; seen 2 rounds, journal entry 2.1 s old";
    assert_eq!(r.confirmed, vec![(T0 + 2000, d.to_string())]);
    let want = [
        (T0, T0),
        (T0 + 2000, T0),
        (T0 + 2000, T0),
        (T0 + 4000, T0),
        (T0 + 4000, T0 + 5000),
    ];
    let want: Vec<_> = want.iter().map(|&(a, b)| (Some(a), Some(b))).collect();
    assert_eq!(seen, want, "(W3, W4) success at +1..+5 s");
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
    // A redirect to a loopback target that would answer: not followed.
    let target = FakeHttp::start(Answer::Reply(
        200,
        r#"{"journal":{"durable_seq":9}}"#.into(),
    ));
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let redirect = format!("http://{}/api/status", l.local_addr().unwrap());
    let to = format!("{}/api/status", target.url());
    std::thread::spawn(move || {
        use std::io::{Read, Write};
        for c in l.incoming().take(4) {
            let Ok(mut c) = c else { return };
            let mut buf = [0u8; 4096];
            let _ = c.read(&mut buf);
            let _ = write!(
                c,
                "HTTP/1.1 302 Found\r\nLocation: {to}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            );
        }
    });
    let r = watcher::oms::Status::new(&redirect, 1000).unwrap();
    assert!(rt.block_on(r.get()).is_err());
    assert!(target.requests().is_empty(), "{:?}", target.requests());
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

#[test]
fn a_start_refusal_ends_the_run_with_exit_code_2() {
    // A good token file (U19b1a) is read and the start goes on to step 2.
    let tok_dir = common::test_dir("rt-refusal-t");
    let tok = tok_dir.join("t.json");
    std::fs::write(&tok, r#"{"token": "1:made-up", "chat_id": 1}"#).unwrap();
    std::fs::set_permissions(&tok, std::fs::Permissions::from_mode(0o600)).unwrap();
    let telegram = format!("[telegram]\ntoken_file = \"{}\"\n", tok.display());
    let w = World::new("rt-refusal", &telegram);
    // A master key: refused at step 2, nothing asked after it.
    w.bybit.route(
        "/v5/user/query-api",
        [ok(
            json!({"isMaster": true, "userID": 1, "ips": ["127.0.0.1"]}),
        )],
    );
    w.status_ok();
    let mut run = w.start(Arc::new(ManualClock::new(T0)));
    assert!(until(10, || !run.running()));
    assert_eq!(run.shutdown(), 2);
    assert_eq!(
        w.lines_with("error: start refused: query-api: the API key is the master account's"),
        1
    );
    assert_eq!(w.bybit.requests().len(), 1);
    assert!(w.echo.requests().is_empty());
    assert!(w.status.requests().is_empty());
    assert!(w.monitor.conns().is_empty());
    assert_eq!(w.lines_with("no [telegram]"), 0);
    // Testnet without `[telegram]`: one warn line at start, before the
    // start checks (U19b1a); the start goes on to step 2 as above.
    let w = World::new("rt-refusal-notg", "");
    w.bybit.route(
        "/v5/user/query-api",
        [ok(
            json!({"isMaster": true, "userID": 1, "ips": ["127.0.0.1"]}),
        )],
    );
    let mut run = w.start(Arc::new(ManualClock::new(T0)));
    assert!(until(10, || !run.running()));
    assert_eq!(run.shutdown(), 2);
    assert_eq!(
        w.lines_with(
            "warn: no [telegram]: every [WATCH P1] is written to this log only, not sent"
        ),
        1
    );
    // ... and, from U19b1c, the refusal P1 as a log line.
    assert_eq!(w.lines_with("no [telegram]"), 2);
    assert_eq!(w.lines_with("warn: [WATCH P1] (no [telegram]: not sent) 워처 시작 거부"), 1);
    assert_eq!(w.bybit.requests().len(), 1);
    // A credentials file that is not owner-only: refused before any request.
    let w = World::new("rt-refusal-creds", "");
    std::fs::set_permissions(
        &w.cfg.credentials_file,
        std::fs::Permissions::from_mode(0o644),
    )
    .unwrap();
    let mut run = w.start(Arc::new(ManualClock::new(T0)));
    assert!(until(10, || !run.running()));
    assert_eq!(run.shutdown(), 2);
    assert_eq!(w.lines_with("error: credentials file"), 1);
    assert!(w.bybit.requests().is_empty());
    assert!(w.vm.requests().is_empty());
    // A token file that is not owner-only: refused before any request,
    // the token not quoted.
    std::fs::set_permissions(&tok, std::fs::Permissions::from_mode(0o640)).unwrap();
    let w = World::new("rt-refusal-tok", &telegram);
    let mut run = w.start(Arc::new(ManualClock::new(T0)));
    assert!(until(10, || !run.running()));
    assert_eq!(run.shutdown(), 2);
    assert_eq!(w.lines_with("error: token file"), 1);
    assert_eq!(w.lines_with("must have mode 0600 or 0400"), 1);
    assert_eq!(w.lines_with("made-up"), 0);
    assert!(w.bybit.requests().is_empty());
    assert!(w.vm.requests().is_empty());
    let _ = std::fs::remove_dir_all(&tok_dir);
}

/// An owner-only token file in a fresh directory and the `[telegram]`
/// section that names it (U19b1c).
fn telegram_section(name: &str) -> (std::path::PathBuf, String) {
    let dir = common::test_dir(name);
    let tok = dir.join("t.json");
    std::fs::write(&tok, r#"{"token": "1:made-up", "chat_id": 1}"#).unwrap();
    std::fs::set_permissions(&tok, std::fs::Permissions::from_mode(0o600)).unwrap();
    let section = format!("[telegram]\ntoken_file = \"{}\"\n", tok.display());
    (dir, section)
}

const KEY_TAIL: &str = "워처의 Bybit 요청이 모두 멈춤(재시작 전까지): W1~W4는 성공하지 않아 OMS가 일반 주문을 막고, 워처의 일괄 취소(W5)도 나갈 수 없음. 키·IP 목록을 확인하고 워처를 다시 시작";

#[test]
fn a_key_rejected_at_start_is_told_on_telegram() {
    // U19b1c: the sender is up before the start checks, so a key rejected
    // at step 2 is told while the start stays there.
    let (dir, tg) = telegram_section("rt-key-tg-t");
    let w = World::new("rt-key-tg", &tg);
    w.bybit.route(
        "/v5/user/query-api",
        [Answer::Reply(
            200,
            r#"{"retCode":10010,"retMsg":"Unmatched IP, please check your API key's bound IP addresses."}"#.into(),
        )],
    );
    w.status
        .route("/api/status", [Answer::Reply(500, "{}".into())]);
    let clock = Arc::new(ManualClock::new(T0));
    let mut run = w.start(clock.clone());
    assert!(until(10, || !w.sent().is_empty()), "{:?}", w.lines.lock().unwrap());
    let body = format!(
        "워처 키가 거부됨(retCode 10010: Unmatched IP, please check your API key's bound IP addresses.). {KEY_TAIL}"
    );
    assert_eq!(w.sent(), vec![format!("[WATCH P1] bybit w-rt 11-15 07:13:20 KST\n{body}")]);
    // The start stays at step 2: nothing more at Bybit, the watcher up, no
    // second message before 5 min.
    walk(&clock, 10_000);
    assert_eq!(w.requests("/v5/user/query-api").len(), 1);
    assert_eq!(w.bybit.requests().len(), 1);
    assert!(run.running());
    assert_eq!(w.sent().len(), 1);
    // 5 min after the first: the repeat, from the runtime's tick.
    clock.advance(300_000);
    assert!(until(10, || w.sent().len() == 2));
    assert!(w.sent()[1].ends_with(&format!("\n{body}")));
    // Shutdown flushes the sender after U19a's steps.
    assert_eq!(run.shutdown(), 0);
    assert_eq!(w.lines_with("alert sender stopped: nothing left to send"), 1);
    assert_eq!(w.lines_with("no [telegram]"), 0);
    assert_eq!(w.lines_with("made-up"), 0);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_start_refusal_after_the_sender_is_up_sends_one_watch_p1() {
    let master = || {
        ok(json!({"isMaster": true, "userID": 1, "ips": ["127.0.0.1"]}))
    };
    let body = "워처 시작 거부: query-api: the API key is the master account's; the watcher needs a sub-account key. 워처가 꺼짐: W1~W4와 W5가 돌지 않음";
    let (dir, tg) = telegram_section("rt-refusal-tg-t");
    let w = World::new("rt-refusal-tg", &tg);
    w.bybit.route("/v5/user/query-api", [master()]);
    let mut run = w.start(Arc::new(ManualClock::new(T0)));
    assert!(until(10, || !run.running()));
    assert_eq!(run.shutdown(), 2);
    // One message reached the fake before the exit.
    let sent = w.sent();
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert!(sent[0].starts_with("[WATCH P1] bybit w-rt "), "{}", sent[0]);
    assert!(sent[0].ends_with(&format!("\n{body}")), "{}", sent[0]);
    assert_eq!(w.lines_with("error: start refused: query-api"), 1);
    assert_eq!(w.lines_with("alert sender stopped: nothing left to send"), 1);

    // Telegram does not answer: the exit waits for the flush at most 5 s.
    let w = World::new("rt-refusal-tg-hang", &tg);
    w.bybit.route("/v5/user/query-api", [master()]);
    w.telegram.script([Answer::Hang]);
    let started = Instant::now();
    let mut run = w.start(Arc::new(ManualClock::new(T0)));
    assert!(until(15, || !run.running()));
    let took = started.elapsed();
    assert_eq!(run.shutdown(), 2);
    assert!(
        took >= Duration::from_millis(4_500) && took < Duration::from_secs(9),
        "{took:?}"
    );
    assert_eq!(w.telegram.requests().len(), 1);
    assert_eq!(
        w.lines_with("warn: alert sender stopped at its bound: 1 parts not delivered are dropped"),
        1
    );

    // Without [telegram] (testnet): the same P1 is one warn line.
    let w = World::new("rt-refusal-notg-p1", "");
    w.bybit.route("/v5/user/query-api", [master()]);
    let mut run = w.start(Arc::new(ManualClock::new(T0)));
    assert!(until(10, || !run.running()));
    assert_eq!(run.shutdown(), 2);
    assert_eq!(
        w.lines_with(&format!("warn: [WATCH P1] (no [telegram]: not sent) {body}")),
        1
    );
    assert!(w.telegram.requests().is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_confirmed_mismatch_is_told_on_telegram_once() {
    // U19b1c, end to end through the runtime: a foreign order at the fake
    // venue is confirmed by W1 and sent to admin.sock; one [WATCH P1]
    // reaches the fake Telegram, and the rounds that still see it send no
    // more.
    let (dir, tg) = telegram_section("rt-mismatch-tg-t");
    let mut w = World::new(
        "rt-mismatch-tg",
        &format!(
            "[intervals]\nw1_ms = 500\nw2_ms = 500\nw3_ms = 1000\nw4_ms = 1000\nheartbeat_ms = 1000\n\
             [rules]\nmin_age_ms = 500\nmismatch_resend_ms = 1000\n\
             [http]\nrequest_timeout_ms = 1000\n{tg}"
        ),
    );
    let now_us = SystemClock.now_ms() * 1000;
    w.journal.append(now_us - 10_000_000, &[ev_filler()]);
    w.status_ok();
    w.bybit.route(
        "/v5/order/realtime",
        [ok(json!({"category": "linear", "nextPageCursor": "",
            "list": [row("x", "v900", "BTCUSDT", "Buy")]}))],
    );
    let mut run = w.start(Arc::new(SystemClock));
    assert!(until(20, || !mismatch_lines(&w.admin).is_empty()));
    assert!(until(10, || !w.sent().is_empty()));
    let detail = mismatch_lines(&w.admin)[0]["detail"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(detail.starts_with("W1 foreign v900: "), "{detail}");
    // Rounds go on for 3 s (6 W1 rounds) that still see it.
    std::thread::sleep(Duration::from_secs(3));
    let sent = w.sent();
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert!(sent[0].starts_with("[WATCH P1] bybit w-rt "), "{}", sent[0]);
    assert!(
        sent[0].contains("\n워처 W1 불일치 확정 1건. OMS에 일반 주문 차단(watcher_mismatch)을 보내기 시작함"),
        "{}",
        sent[0]
    );
    assert!(sent[0].ends_with(&detail), "{}", sent[0]);
    assert_eq!(run.shutdown(), 0);
    let _ = std::fs::remove_dir_all(&dir);
}
