//! The runtime (C-WATCH part 7, U19a7): `runtime::run` against loopback
//! fakes only: `http-fake` for Bybit, the IP echo, `/api/status` and the
//! VictoriaMetrics import, fake `admin.sock` and `monitor.sock`, a test
//! journal. No test starts the binary with a config and no test sends a
//! signal; the shutdown input of `run` stands in for SIGTERM/SIGINT.

#![allow(clippy::unwrap_used)]

mod common;

use common::TestJournal;
use common::rig::{accepted, acked, of, ours, row};
use common::sock::{self, FakeSock, oms};
use http_fake::{Answer, FakeHttp, Request};
use serde_json::{Value, json};
use std::os::unix::fs::PermissionsExt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use watcher::alert::Route;
use watcher::clock::{Clock, ManualClock, SystemClock};
use watcher::config::Config;
use watcher::rest::CANCEL_ALL;
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
    /// The `systemctl` the runtime may run (U19b3c): under the test base
    /// directory only; a missing file unless a test writes a fake.
    systemctl: std::path::PathBuf,
}

fn key() -> Answer {
    ok(json!({"isMaster": false, "userID": 1, "ips": ["127.0.0.1"],
        "readOnly": 1, "expiredAt": "1970-01-01T00:00:00Z"}))
}

/// Sets this process's soft limit on open files to its hard limit, once.
/// A `World` holds about 40 sockets (five http fakes, two socket fakes,
/// the runtime's clients), a test may hold up to four, and libtest starts
/// as many tests at once as the machine has cores: on 32 cores the peak was
/// 1086 open files, over the soft limit 1024 a systemd service starts with
/// ("Too many open files" in 9 tests, U19b3c gate impl_1). The hard limit
/// stays as it is; if the call fails, the limit stays too.
fn raise_open_files_limit() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        use rustix::process::{Resource, Rlimit, getrlimit, setrlimit};
        let hard = getrlimit(Resource::Nofile).maximum;
        let _ = setrlimit(
            Resource::Nofile,
            Rlimit {
                current: hard,
                maximum: hard,
            },
        );
    });
}

impl World {
    /// The fakes answering as a healthy OMS and Bybit would; `extra` is
    /// appended to the config.
    fn new(name: &str, extra: &str) -> World {
        raise_open_files_limit();
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
        let systemctl = creds_dir.join("no-systemctl");
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
            systemctl,
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

    /// The fakes' endpoints. A `systemctl` outside the test base directory
    /// panics: no test may run the real one (review 3 N5).
    fn endpoints(&self) -> Endpoints {
        let base = std::env::var("OMS_TEST_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| std::env::temp_dir());
        assert!(
            self.systemctl.starts_with(&base),
            "systemctl {} is outside the test directory {}",
            self.systemctl.display(),
            base.display()
        );
        Endpoints {
            rest_base: self.bybit.url(),
            ip_echo_url: format!("{}/", self.echo.url()),
            telegram: vec![Route::direct(&self.telegram.url())],
            systemctl: self.systemctl.clone(),
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
    // ... and, from U19b3a, one warn line for no [kill] (testnet only:
    // W5 is off).
    assert_eq!(
        w.lines_with(
            "warn: no [kill]: W5 is off; when the OMS is silent the watcher neither stops it nor cancels its orders, and no [WATCH P1] tells it"
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
    // Telegram fails the first attempt: the part is kept.
    w.telegram
        .script([Answer::Reply(500, r#"{"ok":false}"#.into())]);
    let clock = Arc::new(ManualClock::new(T0));
    let mut run = w.start(clock.clone());
    assert!(until(10, || !w.sent().is_empty()), "{:?}", w.lines.lock().unwrap());
    let body = format!(
        "워처 키가 거부됨(retCode 10010: Unmatched IP, please check your API key's bound IP addresses.). {KEY_TAIL}"
    );
    let first = format!("[WATCH P1] bybit w-rt 11-15 07:13:20 KST\n{body}");
    assert_eq!(w.sent(), vec![first.clone()]);
    // The retry is due 5,000 ms later on the injected clock; the runtime's
    // tick wakes the sender for it, not real time.
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(w.sent().len(), 1);
    walk(&clock, 5_100);
    assert!(until(10, || w.sent().len() == 2));
    assert_eq!(w.sent()[1], first);
    // The start stays at step 2: nothing more at Bybit, the watcher up, no
    // further message before 5 min.
    walk(&clock, 10_000);
    assert_eq!(w.requests("/v5/user/query-api").len(), 1);
    assert_eq!(w.bybit.requests().len(), 1);
    assert!(run.running());
    assert_eq!(w.sent().len(), 2);
    // 5 min after the first: the repeat, from the runtime's tick.
    clock.advance(300_000);
    assert!(until(10, || w.sent().len() == 3));
    assert!(w.sent()[2].ends_with(&format!("\n{body}")));
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

#[test]
fn a_mismatch_collected_inside_the_pace_is_sent_at_shutdown() {
    // U19b1c review 1 S2: W1 confirms v900 (told at once), then v901 inside
    // the 60 s pace (collected); the shutdown queues what is collected
    // before the sender's last flush. Review 1 S1: without [telegram] the
    // same two P1s are one warn line each.
    let (dir, tg) = telegram_section("rt-mm-drain-t");
    let log_head = "warn: [WATCH P1] (no [telegram]: not sent) 워처 W1 불일치 확정 1건. OMS에 일반 주문 차단(watcher_mismatch)을 보내기 시작함(실패하면 다시 보냄; 결과는 OMS의 hold 알림).";
    for with_tg in [true, false] {
        let mut w = World::new(
            if with_tg { "rt-mm-drain-tg" } else { "rt-mm-drain-log" },
            &format!(
                "[intervals]\nw1_ms = 500\nw2_ms = 500\nw3_ms = 1000\nw4_ms = 1000\nheartbeat_ms = 1000\n\
                 [rules]\nmin_age_ms = 500\nmismatch_resend_ms = 1000\n\
                 [http]\nrequest_timeout_ms = 1000\n{}",
                if with_tg { tg.as_str() } else { "" }
            ),
        );
        let now_us = SystemClock.now_ms() * 1000;
        w.journal.append(now_us - 10_000_000, &[ev_filler()]);
        w.status_ok();
        let page = |rows: Vec<Value>| {
            ok(json!({"category": "linear", "nextPageCursor": "", "list": rows}))
        };
        w.bybit.route(
            "/v5/order/realtime",
            [page(vec![row("x", "v900", "BTCUSDT", "Buy")])],
        );
        let mut run = w.start(Arc::new(SystemClock));
        let told = |w: &World| {
            if with_tg {
                w.sent().len()
            } else {
                w.lines_with(log_head)
            }
        };
        assert!(until(20, || told(&w) == 1), "{:?}", w.lines.lock().unwrap());
        w.bybit.route(
            "/v5/order/realtime",
            [page(vec![
                row("x", "v900", "BTCUSDT", "Buy"),
                row("y", "v901", "BTCUSDT", "Buy"),
            ])],
        );
        assert!(until(20, || w
            .lines_with("error: confirmed difference: W1 foreign v901: ")
            == 1));
        // Inside the 60 s pace: collected, not told.
        std::thread::sleep(Duration::from_millis(500));
        assert_eq!(told(&w), 1);
        assert_eq!(run.shutdown(), 0);
        assert_eq!(told(&w), 2, "{:?}", w.lines.lock().unwrap());
        if with_tg {
            let sent = w.sent();
            assert!(sent[0].contains("W1 foreign v900: "), "{}", sent[0]);
            assert!(
                sent[1].contains("\n워처 W1 불일치 확정 1건. OMS에 일반 주문 차단(watcher_mismatch)을 보내기 시작함"),
                "{}",
                sent[1]
            );
            assert!(sent[1].contains("W1 foreign v901: "), "{}", sent[1]);
            assert_eq!(w.lines_with("alert sender stopped: nothing left to send"), 1);
            assert_eq!(w.lines_with("no [telegram]"), 0);
        } else {
            assert_eq!(w.lines_with(&format!("{log_head} W1 foreign v900: ")), 1);
            assert_eq!(w.lines_with(&format!("{log_head} W1 foreign v901: ")), 1);
            assert!(w.telegram.requests().is_empty());
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

// ---- U19b3c: W5 and the kill sequence in the runtime ----
//
// Every kill test keeps `2 × probe_ms + request_timeout_ms` = 4,000 ms (the
// defaults), walks the clock with `walk` only until the kill fired (the
// error line of the fire) and then waits for each kill effect in real time
// (`walk_kill`), so no K step and no position read runs on a clock the test
// moves faster than real time; after the start notice reached the fake
// Telegram it walks again (the loop's waits are on the clock).

/// The unit name of every runtime test (review 3 N5).
const UNIT: &str = "watcher-test-no-such-unit.service";
/// The error line of a fire, as far as a test reads it.
const FIRED: &str = "error: W5: /api/status gave no good answer for";

/// A key that may cancel (U19b2a): read-write with `ContractTrade: Order`.
fn cancel_key() -> Answer {
    ok(json!({"isMaster": false, "userID": 1, "ips": ["127.0.0.1"],
        "readOnly": 0, "permissions": {"ContractTrade": ["Order", "Position"], "Wallet": []},
        "expiredAt": "1970-01-01T00:00:00Z"}))
}

/// A world with `[kill]` (`kill` holds its further fields): the marker in
/// its own test directory, the key one that may cancel. `extra` as for
/// `World::new`. Also the marker's directory.
fn kill_world(name: &str, extra: &str, kill: &str) -> (World, std::path::PathBuf) {
    let dir = common::test_dir(&format!("{name}-k"));
    let marker = dir.join("kill.marker");
    let w = World::new(
        name,
        &format!(
            "{extra}\n[kill]\nmarker = \"{}\"\noms_unit = \"{UNIT}\"\n{kill}",
            marker.display()
        ),
    );
    w.bybit.route("/v5/user/query-api", [cancel_key()]);
    w.bybit.route(CANCEL_ALL, [cancel_ok()]);
    (w, dir)
}

/// A cancel-all Bybit accepted.
fn cancel_ok() -> Answer {
    ok(json!({"list": [], "success": "1"}))
}

/// `node.kill_marker` of an OMS that reads `path` (`None`: no `[kill]`).
fn marker_node(path: Option<&std::path::Path>) -> Value {
    json!({"kill_marker": {"configured": path.is_some(),
        "path": path.map(|p| p.display().to_string()), "present_at_boot": false}})
}

impl World {
    /// `/api/status` with the journal's cap and `node`.
    fn status_with(&self, node: Value) {
        let cap = self.journal.next_seq - 1;
        self.status.route(
            "/api/status",
            [Answer::Reply(
                200,
                json!({"journal": {"durable_seq": cap},
                    "core": {"instance_id": "0000000000000007"}, "node": node})
                .to_string(),
            )],
        );
    }

    /// The OMS answers and reads the watcher's marker.
    fn status_marked(&self) {
        self.status_with(marker_node(Some(&self.cfg.kill.as_ref().unwrap().marker)));
    }

    /// The OMS stops answering (`/api/status` 503).
    fn status_down(&self) {
        self.status
            .route("/api/status", [Answer::Reply(503, "{}".into())]);
    }

    /// Every line of the log so far.
    fn log(&self) -> Vec<String> {
        self.lines.lock().unwrap().clone()
    }

    /// The last value of `series` in the VM pushes so far.
    fn pushed(&self, series: &str) -> Option<f64> {
        let head = format!(r#"{{"metric":{{"__name__":"{series}""#);
        self.pushes().iter().rev().find_map(|b| {
            b.lines().find(|l| l.starts_with(&head)).map(|l| {
                let v: Value = serde_json::from_str(l).unwrap();
                v["values"][0].as_f64().unwrap()
            })
        })
    }
}

/// A fake `systemctl` in `dir`: writes its arguments, one per line, to
/// `<dir>/argv` and exits `code`.
fn fake_systemctl(dir: &std::path::Path, code: i32) -> std::path::PathBuf {
    let p = dir.join("systemctl");
    std::fs::write(
        &p,
        format!("#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}/argv'\nexit {code}\n", dir.display()),
    )
    .unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o700)).unwrap();
    p
}

/// `walk` until `cond` holds after a step (at most `max_ms`).
fn walk_until(clock: &ManualClock, max_ms: u64, what: &str, cond: impl Fn() -> bool) {
    for _ in 0..max_ms / 100 {
        if cond() {
            return;
        }
        walk(clock, 100);
    }
    assert!(cond(), "{what}: not within {max_ms} ms of walking");
}

/// Waits in real time (at most 2,000 ms per step) for a kill effect; when
/// it does not come, moves the clock one 100 ms step and waits again, at
/// most 300 steps (review 4 S2). A K step thus gets at most one 100 ms
/// step of the manual clock while it runs.
fn walk_kill(clock: &ManualClock, what: &str, effect: impl Fn() -> bool) {
    for _ in 0..300 {
        let end = Instant::now() + Duration::from_millis(2000);
        while Instant::now() < end {
            if effect() {
                // One tick more, so the runtime acted on it.
                std::thread::sleep(Duration::from_millis(150));
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        clock.advance(100);
    }
    panic!("walk_kill: {what} did not come within 300 steps");
}

#[test]
fn a_status_with_another_marker_path_refuses_the_start() {
    let why = |oms: &str, ours: &std::path::Path| {
        format!(
            "the OMS reads its kill marker at {oms}; the watcher writes {}: they must be the same file",
            ours.display()
        )
    };
    for (case, node) in [
        ("other", marker_node(Some(std::path::Path::new("/elsewhere/kill.marker")))),
        ("none", marker_node(None)),
    ] {
        let (w, dir) = kill_world(&format!("rt-mpath-{case}"), "", "");
        let marker = w.cfg.kill.as_ref().unwrap().marker.clone();
        w.status_with(node);
        let mut run = w.start(Arc::new(ManualClock::new(T0)));
        assert!(until(10, || !run.running()), "{case}: {:?}", w.log());
        assert_eq!(run.shutdown(), 2, "{case}");
        let oms = if case == "none" { "none" } else { "/elsewhere/kill.marker" };
        let reason = why(oms, &marker);
        assert_eq!(w.lines_with(&format!("error: start refused: {reason}")), 1, "{case}: {:?}", w.log());
        // The start-refusal P1 (U19b1c), here as a log line.
        assert_eq!(
            w.lines_with(&format!("warn: [WATCH P1] (no [telegram]: not sent) 워처 시작 거부: {reason}")),
            1,
            "{case}"
        );
        // Nothing was replayed: no check ran.
        assert!(w.monitor.conns().is_empty(), "{case}");
        assert!(!marker.exists(), "{case}");
        let _ = std::fs::remove_dir_all(&dir);
    }
    // The same file through a symlinked directory is the same marker: the
    // watcher goes on to the checks.
    let (w, dir) = kill_world("rt-mpath-link", "", "");
    let link = dir.with_extension("link");
    let _ = std::fs::remove_file(&link);
    std::os::unix::fs::symlink(&dir, &link).unwrap();
    w.status_with(marker_node(Some(&link.join("kill.marker"))));
    let mut run = w.start(Arc::new(ManualClock::new(T0)));
    assert!(until(10, || !heartbeats(&w.monitor).is_empty()), "{:?}", w.log());
    assert_eq!(w.lines_with("start refused"), 0);
    assert_eq!(run.shutdown(), 0);
    let _ = std::fs::remove_file(&link);
    let _ = std::fs::remove_dir_all(&dir);
}

/// The `watcher_kill` lines `admin.sock` got.
fn kill_lines(a: &FakeSock) -> Vec<Value> {
    a.conns()
        .iter()
        .flatten()
        .filter(|v| v["op"] == "watcher_kill")
        .cloned()
        .collect()
}

/// An `admin.sock` at `path` that answers `watcher_kill` with `reply` (and
/// every other op as the OMS); at each `watcher_kill` it notes whether the
/// marker was there, before it answers.
fn admin_answering(
    path: &std::path::Path,
    marker: &std::path::Path,
    reply: &'static str,
) -> (FakeSock, Arc<Mutex<Vec<bool>>>) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let (s, m) = (seen.clone(), marker.to_path_buf());
    let sock = FakeSock::serve(path, move |_, _, v| {
        if v["op"] == "watcher_kill" {
            s.lock().unwrap().push(m.exists());
            oms(v, reply)
        } else {
            oms(v, "held")
        }
    });
    (sock, seen)
}

/// The fake Telegram's texts that hold `pat`.
fn sent_with(w: &World, pat: &str) -> Vec<String> {
    w.sent().into_iter().filter(|t| t.contains(pat)).collect()
}

const START_NOTICE: &str = "워처 킬 시작(W5: OMS /api/status 무응답 ";

#[test]
fn oms_silence_writes_the_marker_and_halts_the_oms_before_the_first_cancel_all() {
    let (tg_dir, tg) = telegram_section("rt-k-halt-t");
    let (mut w, dir) = kill_world("rt-k-halt", &tg, "");
    let marker = w.cfg.kill.as_ref().unwrap().marker.clone();
    let (admin, at_kill) = admin_answering(&w.cfg.admin_socket, &marker, "latched");
    w.admin = admin;
    w.systemctl = fake_systemctl(&dir, 0);
    // At the first cancel-all: had admin.sock answered watcher_kill?
    let at_cancel = Arc::new(Mutex::new(Vec::new()));
    let (a, replied) = (at_cancel.clone(), at_kill.clone());
    w.bybit.on_request(move |r| {
        if r.path() == CANCEL_ALL {
            a.lock().unwrap().push(!replied.lock().unwrap().is_empty());
        }
    });
    w.status_marked();
    let clock = Arc::new(ManualClock::new(T0));
    let mut run = w.start(clock.clone());
    assert!(until(10, || !heartbeats(&w.monitor).is_empty()), "{:?}", w.log());
    walk(&clock, 2000);
    // The OMS stops answering: W5 fires after the silence, then K1, K2 and
    // only then the loop.
    w.status_down();
    walk_until(&clock, 20_000, "the fire", || w.lines_with(FIRED) == 1);
    walk_kill(&clock, "the marker", || marker.exists());
    walk_kill(&clock, "watcher_kill at admin.sock", || {
        kill_lines(&w.admin).len() == 1
    });
    walk_kill(&clock, "the first cancel-all", || {
        !w.requests(CANCEL_ALL).is_empty()
    });
    assert_eq!(*at_kill.lock().unwrap(), vec![true], "the marker was there at watcher_kill");
    assert!(at_cancel.lock().unwrap()[0], "the halt was answered before the first cancel-all");
    assert!(!dir.join("argv").exists(), "the halt latched: systemctl is not run");
    let k = &kill_lines(&w.admin)[0];
    let detail = k["detail"].as_str().unwrap();
    assert!(detail.starts_with("W5: /api/status gave no good answer for "), "{detail}");
    assert!(detail.ends_with(" s; watcher w-rt"), "{detail}");
    walk_kill(&clock, "the start notice", || !sent_with(&w, START_NOTICE).is_empty());
    let start = &sent_with(&w, START_NOTICE)[0];
    assert!(start.starts_with("[WATCH P1] bybit w-rt "), "{start}");
    assert!(
        start.contains("킬 마커: 씀. OMS 정지: admin.sock watcher_kill → latched). 워처 키로 미체결 0건 확인까지 1초마다 일괄 취소"),
        "{start}"
    );
    assert!(start.contains("\n포지션(거래소 /v5/position/list, "), "{start}");
    assert!(start.ends_with("\n포지션 없음"), "{start}");
    assert_eq!(run.shutdown(), 0);
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&tg_dir);
}

#[test]
fn when_watcher_kill_fails_the_oms_unit_is_stopped_before_the_first_cancel_all() {
    let (tg_dir, tg) = telegram_section("rt-k-stop-t");
    let (mut w, dir) = kill_world("rt-k-stop", &tg, "");
    // No admin.sock at the configured path.
    w.cfg.admin_socket = w.cfg.admin_socket.with_file_name("none.sock");
    w.systemctl = fake_systemctl(&dir, 0);
    let argv = dir.join("argv");
    let at_cancel = Arc::new(Mutex::new(Vec::new()));
    let (a, file) = (at_cancel.clone(), argv.clone());
    w.bybit.on_request(move |r| {
        if r.path() == CANCEL_ALL {
            a.lock().unwrap().push(file.exists());
        }
    });
    w.status_marked();
    let clock = Arc::new(ManualClock::new(T0));
    let mut run = w.start(clock.clone());
    assert!(until(10, || !heartbeats(&w.monitor).is_empty()), "{:?}", w.log());
    walk(&clock, 2000);
    w.status_down();
    walk_until(&clock, 20_000, "the fire", || w.lines_with(FIRED) == 1);
    walk_kill(&clock, "the fake systemctl", || argv.exists());
    walk_kill(&clock, "the first cancel-all", || {
        !w.requests(CANCEL_ALL).is_empty()
    });
    assert!(at_cancel.lock().unwrap()[0], "systemctl ran before the first cancel-all");
    assert_eq!(
        std::fs::read_to_string(&argv).unwrap(),
        format!("--no-ask-password\nstop\n--\n{UNIT}\n")
    );
    walk_kill(&clock, "the start notice", || !sent_with(&w, START_NOTICE).is_empty());
    let start = &sent_with(&w, START_NOTICE)[0];
    assert!(start.contains("킬 마커: 씀. OMS 정지: watcher_kill 실패(not sent: "), "{start}");
    assert!(start.contains(&format!("→ systemctl stop {UNIT}: 정지됨). ")), "{start}");
    assert!(!start.contains("X8"), "{start}");
    assert_eq!(run.shutdown(), 0);
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&tg_dir);
}

/// A watcher started while the OMS does not answer `/api/status`: W5 is
/// armed at the end of the start (owner question 1) and fires after the
/// silence; `admin.sock` latches. Returns once `watcher_kill` arrived.
fn kill_with_the_oms_down(name: &str) -> (World, std::path::PathBuf, std::path::PathBuf, Arc<ManualClock>, Running) {
    let (tg_dir, tg) = telegram_section(&format!("{name}-t"));
    let (mut w, dir) = kill_world(name, &tg, "");
    let marker = w.cfg.kill.as_ref().unwrap().marker.clone();
    w.admin = admin_answering(&w.cfg.admin_socket, &marker, "latched").0;
    w.systemctl = fake_systemctl(&dir, 0);
    let clock = Arc::new(ManualClock::new(T0));
    let run = w.start(clock.clone());
    // The start ends with the public IP echo.
    assert!(until(10, || !w.echo.requests().is_empty()), "{:?}", w.log());
    std::thread::sleep(Duration::from_millis(300));
    // A gap only makes the fire later, never earlier.
    walk(&clock, 9000);
    assert_eq!(w.lines_with(FIRED), 0, "{:?}", w.log());
    walk_until(&clock, 10_000, "the fire", || w.lines_with(FIRED) == 1);
    walk_kill(&clock, "the marker", || marker.exists());
    walk_kill(&clock, "watcher_kill at admin.sock", || {
        kill_lines(&w.admin).len() == 1
    });
    (w, dir, tg_dir, clock, run)
}

#[test]
fn a_watcher_started_with_the_oms_down_kills_after_the_silence() {
    let (w, dir, tg_dir, clock, mut run) = kill_with_the_oms_down("rt-k-down");
    walk_kill(&clock, "the start notice", || !sent_with(&w, START_NOTICE).is_empty());
    let start = &sent_with(&w, START_NOTICE)[0];
    assert!(
        start.contains("초, 마지막 정상 응답 이 프로세스에서 없음. 킬 마커: 씀(OMS가 읽는 경로인지 확인 못 함). OMS 정지: admin.sock watcher_kill → latched)"),
        "{start}"
    );
    // The OMS never answered: no replay, no check, no heartbeat.
    assert!(w.monitor.conns().is_empty());
    assert!(w.requests("/v5/account/wallet-balance").is_empty());
    assert_eq!(run.shutdown(), 0);
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&tg_dir);
}

#[test]
fn a_marker_written_by_the_watcher_is_read_by_a_marker_rule() {
    use std::os::unix::fs::MetadataExt;
    let (w, dir, tg_dir, _clock, mut run) = kill_with_the_oms_down("rt-k-file");
    assert_eq!(run.shutdown(), 0);
    // The file the runtime left, as the OMS's marker rule reads it
    // (C-HOLD: anything at the path latches; the first 512 bytes, control
    // characters as spaces, are the note).
    let marker = &w.cfg.kill.as_ref().unwrap().marker;
    let meta = std::fs::symlink_metadata(marker).unwrap();
    assert!(meta.file_type().is_file());
    assert_eq!(meta.mode() & 0o777, 0o600);
    let bytes = std::fs::read(marker).unwrap();
    assert!(bytes.len() <= 512, "{}", bytes.len());
    let text: String = String::from_utf8(bytes)
        .unwrap()
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    assert!(text.starts_with("watcher w-rt W5 2023-11-14T22:13:"), "{text}");
    assert!(
        text.contains(" s (last good answer none in this process); the watcher stops the OMS and cancels every order with its own key. Clear with omsctl clear-hold --reason kill."),
        "{text}"
    );
    // Only the marker and the fake systemctl are in its directory: no
    // temporary file is left.
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(names, vec!["kill.marker".to_string(), "systemctl".to_string()]);
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&tg_dir);
}

/// One page of `/v5/order/realtime` with `n` open orders.
fn orders(n: usize) -> Answer {
    let list: Vec<Value> = (0..n)
        .map(|i| json!({"orderStatus": "New", "orderLinkId": "", "orderId": format!("o{i}"),
            "symbol": "BTCUSDT", "side": "Buy", "qty": "1"}))
        .collect();
    ok(json!({"category": "linear", "list": list, "nextPageCursor": ""}))
}

/// The manual clock and the path of each request at the fake Bybit.
type Seen = Arc<Mutex<Vec<(u64, String)>>>;

/// Records the manual clock and the path of every request at the fake
/// Bybit.
fn record_at(w: &World, clock: &Arc<ManualClock>) -> Seen {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let (s, c) = (seen.clone(), clock.clone());
    w.bybit.on_request(move |r| s.lock().unwrap().push((c.now_ms(), r.path().to_string())));
    seen
}

/// Requests to `path` from the `from`-th request at the fake on (by order:
/// the clock stands still while the K steps run, so instants can tie).
fn since(seen: &Mutex<Vec<(u64, String)>>, path: &str, from: usize) -> Vec<(usize, u64)> {
    seen.lock()
        .unwrap()
        .iter()
        .enumerate()
        .skip(from)
        .filter(|(_, (_, p))| p == path)
        .map(|(i, (t, _))| (i, *t))
        .collect()
}

/// A kill of an answering OMS that went silent: `admin.sock` latches. Walks
/// until the start notice reached the fake Telegram. `kill` holds the
/// further `[kill]` fields. `at_fire` changes the fakes once the fire line
/// is logged, before the loop's first request: the `watcher_kill` reply
/// waits for it (at most 10 s of real time), and K1, K2 and the first
/// cancel-all and count otherwise all fit in one 15 ms pause of the walk.
fn kill_after_silence(
    name: &str,
    extra: &str,
    kill: &str,
    at_fire: impl Fn(&World),
) -> (World, std::path::PathBuf, std::path::PathBuf, Arc<ManualClock>, Running, Seen) {
    let (tg_dir, tg) = telegram_section(&format!("{name}-t"));
    let (mut w, dir) = kill_world(name, &format!("{extra}\n{tg}"), kill);
    // The manual clock stands still while the reply waits: K2's deadline
    // does not pass.
    let ready = Arc::new(AtomicBool::new(false));
    let r = ready.clone();
    w.admin = FakeSock::serve(&w.cfg.admin_socket, move |_, _, v| {
        if v["op"] == "watcher_kill" {
            let end = Instant::now() + Duration::from_secs(10);
            while !r.load(Ordering::SeqCst) && Instant::now() < end {
                std::thread::sleep(Duration::from_millis(5));
            }
            oms(v, "latched")
        } else {
            oms(v, "held")
        }
    });
    w.systemctl = fake_systemctl(&dir, 0);
    w.status_marked();
    let clock = Arc::new(ManualClock::new(T0));
    let seen = record_at(&w, &clock);
    let run = w.start(clock.clone());
    assert!(until(10, || !heartbeats(&w.monitor).is_empty()), "{:?}", w.log());
    walk(&clock, 2000);
    w.status_down();
    walk_until(&clock, 20_000, "the fire", || w.lines_with(FIRED) == 1);
    at_fire(&w);
    ready.store(true, Ordering::SeqCst);
    walk_kill(&clock, "watcher_kill at admin.sock", || {
        kill_lines(&w.admin).len() == 1
    });
    walk_kill(&clock, "the first cancel-all", || {
        !w.requests(CANCEL_ALL).is_empty()
    });
    walk_kill(&clock, "the start notice", || !sent_with(&w, START_NOTICE).is_empty());
    (w, dir, tg_dir, clock, run, seen)
}

const FINAL_NOTICE: &str = "워처 킬 정리 완료: 미체결 0건 확인 뒤 30초 동안 늦은 접수 없음(";

#[test]
fn the_engine_is_paused_during_a_kill_and_resumes_after() {
    let (w, dir, tg_dir, clock, mut run, seen) = kill_after_silence(
        "rt-k-pause",
        "[intervals]\nw1_ms = 500\nw2_ms = 500\nw3_ms = 1000\nw4_ms = 1000\nheartbeat_ms = 1000\n\
         [rules]\nmin_age_ms = 500\n",
        "",
        // One order stays open: the loop cannot end while the test looks.
        |w| w.bybit.route("/v5/order/realtime", [orders(1)]),
    );
    // Every request from here on was sent after the fire (the start
    // notice needs K1, K2 and a position read).
    let from = seen.lock().unwrap().len();
    let beats = heartbeats(&w.monitor).len();
    let age = |w: &World| w.pushed(r#"watcher_check_age_s","check":"W1"#).unwrap();
    std::thread::sleep(Duration::from_millis(300));
    let (age0, t0) = (age(&w), clock.now_ms());
    // Until 5 cancel-alls (one per round_ms of 1,000 if the machine keeps
    // the pace; bounded).
    walk_until(&clock, 60_000, "5 cancel-alls", || since(&seen, CANCEL_ALL, from).len() >= 5);
    std::thread::sleep(Duration::from_millis(300));
    // The kill ran the whole window (an order was open): no check round
    // (W3, W4; W1's list is the loop's count: no more of it than
    // cancel-alls), no heartbeat; the check ages kept growing.
    assert!(since(&seen, "/v5/account/wallet-balance", from).is_empty());
    assert!(since(&seen, "/v5/execution/list", from).is_empty());
    let cancels = since(&seen, CANCEL_ALL, from).len();
    assert!(cancels >= 5, "{cancels}");
    assert!(since(&seen, "/v5/order/realtime", from).len() <= cancels + 1);
    let to = seen.lock().unwrap().len();
    assert_eq!(heartbeats(&w.monitor).len(), beats);
    let grown = age(&w) - age0;
    assert!(grown >= (clock.now_ms() - t0) as f64 / 1000.0 - 2.0, "{grown}");
    assert_eq!(w.pushed("watcher_kill_active"), Some(1.0));
    // Zero open orders: the loop ends, the checks and heartbeats resume.
    w.bybit.route("/v5/order/realtime", [empty_page()]);
    walk_until(&clock, 60_000, "the final notice", || !sent_with(&w, FINAL_NOTICE).is_empty());
    walk(&clock, 3000);
    std::thread::sleep(Duration::from_millis(300));
    assert!(until(10, || heartbeats(&w.monitor).len() > beats));
    assert!(!since(&seen, "/v5/account/wallet-balance", to).is_empty());
    assert!(!since(&seen, "/v5/order/realtime", to).is_empty());
    assert_eq!(w.pushed("watcher_kill_active"), Some(0.0));
    // W5 watches again after one good answer (the halt latched: not owed).
    assert_eq!(w.pushed("watcher_w5_armed"), Some(0.0));
    w.status_marked();
    walk_until(&clock, 10_000, "W5 armed again", || w.pushed("watcher_w5_armed") == Some(1.0));
    assert_eq!(w.pushed("watcher_kill_halt_owed"), Some(0.0));
    assert_eq!(run.shutdown(), 0);
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&tg_dir);
}

#[test]
fn the_watchers_loop_repeats_until_zero_at_no_more_than_2_cancel_alls_per_second() {
    // From the fire on: the first count finds one order left, the second
    // cancel-all and its count answer 503, the third round counts zero.
    let (w, dir, tg_dir, clock, mut run, seen) = kill_after_silence("rt-k-rate", "", "", |w| {
        w.bybit.route(
            "/v5/order/realtime",
            [orders(1), Answer::Reply(503, "{}".into()), empty_page()],
        );
        w.bybit.route(
            CANCEL_ALL,
            [cancel_ok(), Answer::Reply(503, "{}".into()), cancel_ok()],
        );
    });
    walk_until(&clock, 120_000, "the final notice", || !sent_with(&w, FINAL_NOTICE).is_empty());
    let all = since(&seen, CANCEL_ALL, 0);
    let cancels: Vec<u64> = all.iter().map(|(_, t)| *t).collect();
    assert_eq!(cancels.len(), 3, "{cancels:?}");
    for &t in &cancels {
        let n = cancels.iter().filter(|&&u| u >= t && u < t + 1000).count();
        assert!(n <= 2, "{n} cancel-alls in [{t}, {t} + 1,000): {cancels:?}");
    }
    // The round wait (round_ms 1,000) holds between them.
    assert!(cancels.windows(2).all(|p| p[1] - p[0] >= 1000), "{cancels:?}");
    // A count after every cancel-all (the failed one too); after the third
    // the watch counts every 5 s, the last at 30 s (z is the third count's
    // answer, within a tick of the third cancel-all; the engine runs again
    // only after the last).
    let counts = |a: usize, b: usize| {
        since(&seen, "/v5/order/realtime", a)
            .into_iter()
            .filter(|(i, _)| *i < b)
            .count()
    };
    assert_eq!(counts(all[0].0, all[1].0), 1);
    assert_eq!(counts(all[1].0, all[2].0), 1);
    let watch = since(&seen, "/v5/order/realtime", all[2].0)
        .into_iter()
        .filter(|(_, t)| *t <= cancels[2] + 29_000)
        .count();
    assert_eq!(watch, 6);
    let fin = &sent_with(&w, FINAL_NOTICE)[0];
    assert!(
        fin.contains("(3회차, 일괄 취소 3회). OMS는 래치됨(watcher_kill)."),
        "{fin}"
    );
    assert!(fin.contains("\n포지션(거래소 /v5/position/list, "), "{fin}");
    assert_eq!(w.lines_with("the kill ended: zero open orders confirmed (3 rounds, 3 cancel-alls); the halt is not owed"), 1);
    assert_eq!(run.shutdown(), 0);
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&tg_dir);
}

const LATE_HALT: &str = "늦은 정지: OMS가 다시 답해 watcher_kill로 래치함(latched)";

/// A kill whose halt and stop both fail: no `admin.sock` at the configured
/// path and a `systemctl` that exits 1; one order stays open at the fake.
/// `before` runs once the checks run, before the OMS goes silent. Returns
/// once the start notice reached the fake Telegram, with the path where
/// `admin.sock` may appear.
fn kill_unhalted(
    name: &str,
    before: impl Fn(&World, &ManualClock),
) -> (World, std::path::PathBuf, std::path::PathBuf, Arc<ManualClock>, Running, std::path::PathBuf) {
    let (tg_dir, tg) = telegram_section(&format!("{name}-t"));
    let (mut w, dir) = kill_world(name, &tg, "");
    let sock = w.cfg.admin_socket.with_file_name("late.sock");
    w.cfg.admin_socket = sock.clone();
    w.systemctl = fake_systemctl(&dir, 1);
    w.status_marked();
    let clock = Arc::new(ManualClock::new(T0));
    let run = w.start(clock.clone());
    assert!(until(10, || !heartbeats(&w.monitor).is_empty()), "{:?}", w.log());
    before(&w, &clock);
    walk(&clock, 2000);
    w.bybit.route("/v5/order/realtime", [orders(1)]);
    w.status_down();
    walk_until(&clock, 20_000, "the fire", || w.lines_with(FIRED) == 1);
    walk_kill(&clock, "the fake systemctl", || dir.join("argv").exists());
    walk_kill(&clock, "the start notice", || !sent_with(&w, START_NOTICE).is_empty());
    let start = &sent_with(&w, START_NOTICE)[0];
    assert!(
        start.contains(&format!("→ systemctl stop {UNIT}: 실패(exit 1). OMS 정지 실패: OMS가 새 주문을 낼 수 있음. OMS 키 폐기(X8 절차)를 검토). 워처 키로")),
        "{start}"
    );
    (w, dir, tg_dir, clock, run, sock)
}

#[test]
fn an_oms_that_comes_back_during_the_loop_is_latched_before_the_loop_ends() {
    // The OMS comes back unlatched 5 s into the loop (stalled, or restarted
    // by systemd): the halt retried every 5 s reaches it while the kill
    // runs (review 3 M1).
    let (w, dir, tg_dir, clock, mut run, sock) = kill_unhalted("rt-k-late", |_, _| {});
    walk(&clock, 5000);
    let admin = admin_answering(&sock, &w.cfg.kill.as_ref().unwrap().marker, "latched").0;
    walk_until(&clock, 20_000, "watcher_kill at the new admin.sock", || {
        kill_lines(&admin).len() == 1
    });
    assert!(sent_with(&w, FINAL_NOTICE).is_empty());
    assert_eq!(w.pushed("watcher_kill_active"), Some(1.0));
    let detail = kill_lines(&admin)[0]["detail"].as_str().unwrap().to_string();
    assert_eq!(detail, "W5: 늦은 정지: watcher w-rt의 킬이 OMS 래치나 정지를 확인하지 못함");
    walk_until(&clock, 20_000, "the late halt notice", || {
        !sent_with(&w, LATE_HALT).is_empty()
    });
    assert!(sent_with(&w, LATE_HALT)[0].contains("\n포지션(거래소 /v5/position/list, "));
    assert!(sent_with(&w, FINAL_NOTICE).is_empty());
    // The order goes: the final notice names the latch; no further halt.
    w.bybit.route("/v5/order/realtime", [empty_page()]);
    walk_until(&clock, 60_000, "the final notice", || !sent_with(&w, FINAL_NOTICE).is_empty());
    let fin = &sent_with(&w, FINAL_NOTICE)[0];
    assert!(fin.contains("OMS는 래치됨(watcher_kill)."), "{fin}");
    walk(&clock, 10_000);
    assert_eq!(kill_lines(&admin).len(), 1);
    assert_eq!(sent_with(&w, LATE_HALT).len(), 1);
    assert_eq!(w.lines_with("the halt is not owed"), 1);
    assert_eq!(run.shutdown(), 0);
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&tg_dir);

    // The watcher's key was latched by a 10010 answer before the kill
    // (scenario C of review 3 M1): no cancel-all can be sent, the loop
    // cannot end, and the halt retries still reach the OMS.
    let (w, dir, tg_dir, clock, mut run, sock) = kill_unhalted("rt-k-late-key", |w, clock| {
        w.bybit.route(
            "/v5/account/wallet-balance",
            [Answer::Reply(200, r#"{"retCode":10010,"retMsg":"Unmatched IP"}"#.into())],
        );
        walk_until(clock, 10_000, "the key's latch", || {
            !sent_with(w, "워처 키가 거부됨(retCode 10010").is_empty()
        });
    });
    walk(&clock, 5000);
    let admin = admin_answering(&sock, &w.cfg.kill.as_ref().unwrap().marker, "latched").0;
    walk_until(&clock, 20_000, "watcher_kill at the new admin.sock", || {
        kill_lines(&admin).len() == 1
    });
    walk_until(&clock, 20_000, "the late halt notice", || {
        !sent_with(&w, LATE_HALT).is_empty()
    });
    walk(&clock, 10_000);
    std::thread::sleep(Duration::from_millis(300));
    // U19b3c's shutdown is U19a's path (U19b3d adds the abort notice).
    assert_eq!(run.shutdown(), 0);
    assert_eq!(kill_lines(&admin).len(), 1);
    assert_eq!(sent_with(&w, LATE_HALT).len(), 1);
    assert!(w.requests(CANCEL_ALL).is_empty());
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&tg_dir);
}

#[test]
fn a_clock_step_back_during_a_kill_does_not_hold_the_loop() {
    // One order stays open; the wall clock steps back 60 s during the loop
    // (review 4 S3): the kill path reads clock::Steady, so the next
    // cancel-all comes round_ms of further progress after the last one,
    // not 60 s later.
    let (w, dir, tg_dir, clock, mut run, seen) = kill_after_silence("rt-k-back", "", "", |w| {
        w.bybit.route("/v5/order/realtime", [orders(1)])
    });
    walk(&clock, 3000);
    let cancels = || w.requests(CANCEL_ALL).len();
    // Settle: the last cancel-all was counted.
    std::thread::sleep(Duration::from_millis(300));
    let k = cancels();
    let last = since(&seen, CANCEL_ALL, 0).last().unwrap().0;
    assert!(until(5, || !since(&seen, "/v5/order/realtime", last).is_empty()));
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(cancels(), k);
    clock.set(clock.now_ms() - 60_000);
    // round_ms (1,000) plus one step: each step waits for the runtime's
    // ticks before the next.
    let mut steps = 0;
    while cancels() == k {
        steps += 1;
        assert!(steps <= 11, "no cancel-all within round_ms + 100 ms after the step back");
        clock.advance(100);
        until(1, || cancels() > k);
    }
    assert_eq!(cancels(), k + 1);
    assert_eq!(run.shutdown(), 0);
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&tg_dir);
}

#[test]
fn a_halt_without_a_reply_is_aborted_at_its_deadline() {
    // admin.sock takes watcher_kill and never answers. The K2 deadline (2 ×
    // request_timeout_ms + 1,000 = 11,000 ms of the kill's clock) passes
    // while the connection hangs, so walking the clock through it does not
    // depend on the machine: the runtime aborts the task (the connection
    // closes) long before admin_kill's own timeout of 5,000 ms of real time
    // for the reply, and K3 runs (review 2 S1, S2).
    let (tg_dir, tg) = telegram_section("rt-k-hang-t");
    let (mut w, dir) = kill_world(
        "rt-k-hang",
        &format!("[http]\nrequest_timeout_ms = 5000\n[w5]\nsilence_ms = 19000\n{tg}"),
        "",
    );
    w.admin = FakeSock::serve(&w.cfg.admin_socket, |_, _, v| {
        if v["op"] == "watcher_kill" {
            sock::Reply::Silent
        } else {
            oms(v, "held")
        }
    });
    w.systemctl = fake_systemctl(&dir, 0);
    w.status_marked();
    let clock = Arc::new(ManualClock::new(T0));
    let mut run = w.start(clock.clone());
    assert!(until(10, || !heartbeats(&w.monitor).is_empty()), "{:?}", w.log());
    walk(&clock, 2000);
    w.status_down();
    walk_until(&clock, 40_000, "the fire", || w.lines_with(FIRED) == 1);
    walk_kill(&clock, "watcher_kill at admin.sock", || {
        kill_lines(&w.admin).len() == 1
    });
    let asked = Instant::now();
    let conn = w.admin.conns().len() - 1;
    assert_eq!(w.admin.conns()[conn].last().unwrap()["op"], "watcher_kill");
    walk(&clock, 11_100);
    assert!(w.admin.wait(Duration::from_secs(10), |s| s.closed[conn]));
    let took = asked.elapsed();
    assert!(took < Duration::from_millis(4000), "the halt task was not aborted at its deadline: {took:?}");
    walk_kill(&clock, "the start notice", || !sent_with(&w, START_NOTICE).is_empty());
    let start = &sent_with(&w, START_NOTICE)[0];
    assert!(
        start.contains(&format!("OMS 정지: watcher_kill 실패(정지 응답 없음(시간 초과)) → systemctl stop {UNIT}: 정지됨). ")),
        "{start}"
    );
    assert!(dir.join("argv").exists());
    assert_eq!(run.shutdown(), 0);
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&tg_dir);
}
