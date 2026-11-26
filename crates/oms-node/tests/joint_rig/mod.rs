//! The joint rig: the real `oms-node` (a `failpoints` build, Bybit
//! gateway) and the real watcher (its `test-endpoints` build) as child
//! processes, against one stateful fake Bybit ([`FakeBybit`]), a fake
//! Telegram, a fake VictoriaMetrics and a fake `systemctl`, loopback only.
//! The tests see both programs only through their real interfaces: the
//! journal files, `/api/status`, the admin and trading sockets (`omsctl`
//! too), the watcher's stderr and metrics, the kill marker and the fakes'
//! logs.
//!
//! The two binaries run on their system clocks, so every wait here is a
//! poll with its own timeout inside the test's budget. A failed wait
//! panics with the tails of both logs and of the fake's request log. A
//! precondition of the rig itself (a base directory inside a git work
//! tree, a socket path too long, a missing or wrong binary, a port still in
//! use) panics with a message that starts [`PRECONDITION`], so it is never
//! read as a scenario result.
//!
//! Each child has a reaper thread that keeps its exit status and time;
//! dropping the rig sends SIGCONT then SIGKILL to every live child and
//! waits for it.

#![allow(dead_code)]

use crate::bybit_fake::book::{Book, OKEY, OSECRET, WKEY, WSECRET};
use crate::bybit_fake::journal::records;
use crate::bybit_fake::venue::FakeBybit;
use http_fake::{Answer, FakeHttp};
use oms_api::client::{Client, now_us};
use oms_api::*;
use oms_core::*;
use rustix::process::{Pid, Signal};
use serde_json::Value;
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// The start of every panic about the rig's own preconditions.
pub const PRECONDITION: &str = "joint rig precondition:";
/// The OMS's systemd unit as the watcher names it; the fake `systemctl`
/// stops only this one.
pub const UNIT: &str = "oms-node-joint.service";
/// The watcher's `name` (its heartbeat label).
pub const WATCHER_NAME: &str = "joint";
/// A Unix socket path of this many bytes or more is refused (SUN_LEN is
/// 108; the margin is 판단(모델 아님)).
pub const SOCKET_PATH_MAX: usize = 100;
/// The fake bot token, in both programs' token file (made up).
const TOKEN: &str = r#"{"token":"123456:ABCdef_ghi-JKL","chat_id":"42"}"#;

fn precondition(what: impl std::fmt::Display) -> ! {
    panic!("{PRECONDITION} {what}")
}

/// A path from the environment, or a precondition failure.
fn env_path(name: &str) -> PathBuf {
    match std::env::var_os(name) {
        Some(v) if !v.is_empty() => PathBuf::from(v),
        _ => precondition(format!(
            "{name} is not set (run the suite with `cargo run -p xtask -- joint`)"
        )),
    }
}

/// The git work tree `p` lies in: the first of `p` and its ancestors that
/// holds `.git`, whether `p` exists or not.
pub fn git_work_tree(p: &Path) -> Option<PathBuf> {
    p.ancestors()
        .find(|a| a.join(".git").exists())
        .map(Path::to_path_buf)
}

/// The rig's rules on the base directory and a test directory under it:
/// absolute, outside any git work tree (the watcher refuses a token file
/// there), and every socket path below [`SOCKET_PATH_MAX`] bytes.
pub fn check_base(base: &Path, dir: &Path) -> Result<(), String> {
    if !base.is_absolute() {
        return Err(format!("OMS_TEST_DIR {} is not absolute", base.display()));
    }
    if let Some(tree) = git_work_tree(base) {
        return Err(format!(
            "OMS_TEST_DIR {} is inside the git work tree {}: the watcher refuses a token file there",
            base.display(),
            tree.display()
        ));
    }
    for sock in ["oms.sock", "admin.sock", "monitor.sock"] {
        let p = dir.join("o/d").join(sock);
        let n = p.as_os_str().len();
        if n >= SOCKET_PATH_MAX {
            return Err(format!(
                "the socket path {} has {n} bytes, at most {} allowed",
                p.display(),
                SOCKET_PATH_MAX - 1
            ));
        }
    }
    Ok(())
}

fn mode(path: &Path, mode: u32) {
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
}

fn write_mode(path: &Path, text: &str, m: u32) {
    std::fs::write(path, text).unwrap();
    mode(path, m);
}

fn send_signal(pid: u32, sig: Signal) {
    if let Some(p) = i32::try_from(pid).ok().and_then(Pid::from_raw) {
        let _ = rustix::process::kill_process(p, sig);
    }
}

/// A loopback port nobody listens on (bind and release).
fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// The timers of a test (spec "Timers"): the code defaults, or the short
/// values of every scenario but J1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Timers {
    Deployed,
    Short,
}

impl Timers {
    /// The OMS's `[runtime]`, `[venue]` timer lines, `[core.policy]` and
    /// `[core.policy.monitor]` bodies.
    fn oms(self) -> (&'static str, &'static str, &'static str, &'static str) {
        match self {
            Timers::Deployed => (
                "",
                "",
                "absence_not_found_needed = 2\nvenue_enforces_expiry = true\n",
                "",
            ),
            Timers::Short => (
                "tick_ms = 50\nreconcile_ms = 100\nsweep_ms = 60000\n",
                "ping_interval_ms = 1000\nrequest_timeout_ms = 1000\nrecv_window_ms = 1000\nref_price_interval_ms = 1000\nkill_after_ms = 10000\nkill_watch_ms = 10000\nkill_watch_every_ms = 1000\n",
                "absence_margin_us = 300000\nabsence_not_found_needed = 2\nvenue_enforces_expiry = true\nmax_expiry_horizon_us = 60000000\n",
                "heartbeat_max_age_us = 10000000\ncheck_max_age_us = 5000000\nunmonitored_cancel_us = 10000000\n",
            ),
        }
    }

    /// The watcher's `[intervals]`, `[rules]`, `[http]`, `[w5]` and `[kill]`
    /// loop lines.
    fn watcher(self) -> [&'static str; 5] {
        match self {
            Timers::Deployed => ["", "", "", "", ""],
            Timers::Short => [
                "w1_ms = 500\nw2_ms = 500\nw3_ms = 1000\nw4_ms = 1000\nheartbeat_ms = 1000\n",
                "min_age_ms = 2000\nmismatch_resend_ms = 2000\n",
                "request_timeout_ms = 1000\n",
                "probe_ms = 500\nsilence_ms = 10000\n",
                "round_ms = 1000\nwatch_ms = 10000\nwatch_every_ms = 1000\nnotice_ms = 60000\nstop_wait_ms = 5000\n",
            ],
        }
    }

    /// The watcher's `w4_ms` (its sweep period).
    pub fn w4_ms(self) -> u64 {
        match self {
            Timers::Deployed => 5_000,
            Timers::Short => 1_000,
        }
    }

    pub fn min_age_ms(self) -> u64 {
        2_000
    }
}

/// A child process with a reaper thread: the reaper `try_wait`s every
/// 20 ms and keeps the exit status and time; `on_exit` runs before the
/// exit is recorded. Dropping it sends SIGCONT then SIGKILL to a live child
/// and waits for it.
struct Proc {
    pid: u32,
    state: Arc<Mutex<ProcState>>,
}

struct ProcState {
    child: Option<Child>,
    exit: Option<(ExitStatus, Instant)>,
    on_exit: Option<Box<dyn FnOnce() + Send>>,
}

impl ProcState {
    fn exited(&mut self, st: ExitStatus) {
        self.child = None;
        if let Some(f) = self.on_exit.take() {
            f();
        }
        self.exit = Some((st, Instant::now()));
    }
}

impl Proc {
    fn spawn(child: Child, on_exit: Box<dyn FnOnce() + Send>) -> Proc {
        let pid = child.id();
        let state = Arc::new(Mutex::new(ProcState {
            child: Some(child),
            exit: None,
            on_exit: Some(on_exit),
        }));
        let s = state.clone();
        std::thread::spawn(move || {
            loop {
                {
                    let mut st = s.lock().unwrap();
                    let Some(c) = st.child.as_mut() else { return };
                    if let Ok(Some(status)) = c.try_wait() {
                        st.exited(status);
                        return;
                    }
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        });
        Proc { pid, state }
    }

    fn exit(&self) -> Option<(ExitStatus, Instant)> {
        self.state.lock().unwrap().exit
    }
}

impl Drop for Proc {
    fn drop(&mut self) {
        let mut st = self.state.lock().unwrap();
        if let Some(mut c) = st.child.take() {
            send_signal(self.pid, Signal::CONT);
            let _ = c.kill();
            if let Ok(status) = c.wait() {
                st.exited(status);
            }
        }
    }
}

/// One test's world: its directory `$OMS_TEST_DIR/<tag>-<pid>/`, the fakes
/// and the OMS configuration. The watcher's configuration is written only
/// after its binary passed the version guard ([`World::watcher_with`]).
pub struct World {
    pub tag: String,
    pub dir: PathBuf,
    pub timers: Timers,
    pub fake: FakeBybit,
    pub telegram: FakeHttp,
    pub vm: FakeHttp,
    /// The OMS's `[http] listen` port; a start that finds it taken moves
    /// it once ([`Oms::start`]).
    port: Arc<Mutex<u16>>,
    deadline: Instant,
    budget: Duration,
}

impl World {
    /// Checks the base directory and the socket paths, makes the
    /// directories, starts the fakes and writes the OMS's configuration.
    /// `budget` is the whole test's.
    pub fn new(tag: &str, timers: Timers, budget: Duration) -> World {
        assert!(tag.len() <= 3, "a tag has at most 3 bytes");
        let base = env_path("OMS_TEST_DIR");
        let dir = base.join(format!("{tag}-{}", std::process::id()));
        if let Err(e) = check_base(&base, &dir) {
            precondition(e);
        }
        let _ = std::fs::remove_dir_all(&dir);
        for d in ["o", "w", "s", "bin"] {
            std::fs::create_dir_all(dir.join(d)).unwrap();
        }
        let telegram = FakeHttp::start(Answer::Reply(200, r#"{"ok":true,"result":{}}"#.into()));
        let vm = FakeHttp::start(Answer::Reply(204, String::new()));
        let w = World {
            tag: tag.into(),
            dir,
            timers,
            fake: FakeBybit::start(),
            telegram,
            vm,
            port: Arc::new(Mutex::new(free_port())),
            deadline: Instant::now() + budget,
            budget,
        };
        write_mode(&w.dir.join("telegram.json"), TOKEN, 0o600);
        write_mode(
            &w.dir.join("o/bybit.toml"),
            &format!("api_key = \"{OKEY}\"\napi_secret = \"{OSECRET}\"\n"),
            0o600,
        );
        write_mode(
            &w.dir.join("w/bybit.toml"),
            &format!("api_key = \"{WKEY}\"\napi_secret = \"{WSECRET}\"\n"),
            0o600,
        );
        write_mode(&w.systemctl(), &systemctl_script(&w.dir), 0o700);
        w.write_oms_config();
        w
    }

    pub fn data(&self) -> PathBuf {
        self.dir.join("o/d")
    }

    pub fn marker(&self) -> PathBuf {
        self.dir.join("s/kill.marker")
    }

    pub fn systemctl(&self) -> PathBuf {
        self.dir.join("bin/systemctl")
    }

    pub fn port(&self) -> u16 {
        *self.port.lock().unwrap()
    }

    fn write_oms_config(&self) {
        let (runtime, venue, policy, monitor) = self.timers.oms();
        let config = format!(
            r#"data_dir = "{data}"

[runtime]
{runtime}
[http]
listen = "127.0.0.1:{port}"

[limits]
new_orders_per_sec = 1000
new_orders_burst = 1000
cancels_per_sec = 1000
cancels_burst = 1000

[venue]
kind = "bybit"
network = "testnet"
credentials_file = "{creds}"
sub_account_uid = 4242
ip_echo_url = "https://ip.example/"
{venue}
[core]
version = 1

[core.instruments.BTCUSDT]
qty_step = "0.001"
px_tick = "0.1"
min_notional = "0"
max_order_qty = "100"
max_order_notional = "100000000"
account_pos_min = "-1000"
account_pos_max = "1000"
price_band_bps = 500

[core.strategies.a]
enabled = true
max_open_orders = 400
[core.strategies.a.limits.BTCUSDT]
min = "-1000"
max = "1000"

[core.policy]
max_open_orders = 400
max_inflight_sends = 64
{policy}
[core.policy.monitor]
{monitor}
[alert]
telegram = "{token}"

[kill]
marker = "{marker}"
"#,
            data = self.data().display(),
            port = self.port(),
            creds = self.dir.join("o/bybit.toml").display(),
            token = self.dir.join("telegram.json").display(),
            marker = self.marker().display(),
        );
        std::fs::write(self.dir.join("o/oms.toml"), config).unwrap();
    }

    fn write_watcher_config(&self) {
        let [intervals, rules, http, w5, kill] = self.timers.watcher();
        let config = format!(
            r#"name = "{WATCHER_NAME}"
network = "testnet"
credentials_file = "{creds}"
sub_account_uid = 4242
symbols = ["BTCUSDT"]
ip_echo_url = "https://ip.example/"
data_dir = "{data}"
status_url = "http://127.0.0.1:{port}/api/status"

[intervals]
{intervals}
[rules]
{rules}
[http]
{http}
[telemetry]
vm_url = "{vm}"

[telegram]
token_file = "{token}"

[kill]
marker = "{marker}"
oms_unit = "{UNIT}"
{kill}
[w5]
{w5}"#,
            creds = self.dir.join("w/bybit.toml").display(),
            data = self.data().display(),
            port = self.port(),
            vm = self.vm.url(),
            token = self.dir.join("telegram.json").display(),
            marker = self.marker().display(),
        );
        std::fs::write(self.dir.join("w/watcher.toml"), config).unwrap();
    }

    /// The OMS of this world (not started).
    pub fn oms(&self) -> Oms {
        Oms {
            dir: self.dir.clone(),
            wiring: self.fake.wiring(),
            telegram: self.telegram.url(),
            port: self.port.clone(),
            proc: None,
        }
    }

    /// The watcher of `JOINT_WATCHER_BIN` (not started); a binary that
    /// fails the version guard is a precondition failure.
    pub fn watcher(&self) -> Watcher {
        let bin = env_path("JOINT_WATCHER_BIN");
        self.watcher_with(&bin).unwrap_or_else(|e| precondition(e))
    }

    /// The watcher of `bin`, after the version guard: `bin --version` must
    /// end in ` test-endpoints`. Only then is its configuration written,
    /// so a binary built without the test hook never gets one.
    pub fn watcher_with(&self, bin: &Path) -> Result<Watcher, String> {
        version_guard(bin)?;
        self.write_watcher_config();
        let wiring = serde_json::json!({
            "rest": self.fake.api.url(),
            "ip_echo_url": format!("{}/ip", self.fake.api.url()),
            "telegram": self.telegram.url(),
            "systemctl": self.systemctl(),
        })
        .to_string();
        Ok(Watcher {
            dir: self.dir.clone(),
            bin: bin.to_path_buf(),
            wiring,
            proc: None,
        })
    }

    pub fn budget_left(&self) -> Duration {
        self.deadline.saturating_duration_since(Instant::now())
    }

    /// Polls `cond` every 25 ms until it holds; panics after `timeout` or
    /// at the end of the budget, whichever is first.
    pub fn wait(&self, what: &str, timeout: Duration, mut cond: impl FnMut() -> bool) {
        let end = (Instant::now() + timeout).min(self.deadline);
        loop {
            if cond() {
                return;
            }
            if Instant::now() >= end {
                self.fail(&format!(
                    "waited {timeout:?} (budget {:?}) for: {what}",
                    self.budget
                ));
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    }

    /// Polls `bad` every 25 ms for `window`; panics as soon as it holds.
    pub fn never(&self, what: &str, window: Duration, mut bad: impl FnMut() -> bool) {
        let end = Instant::now() + window;
        while Instant::now() < end {
            if bad() {
                self.fail(&format!("within {window:?}: {what}"));
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    }

    /// Panics with `what` and the tails of both logs and the fake's log.
    pub fn fail(&self, what: &str) -> ! {
        panic!(
            "{what}\n--- node.log\n{}\n--- watcher.log\n{}\n--- fake requests\n{}",
            tail(&self.dir.join("o/node.log"), 40),
            tail(&self.dir.join("w/watcher.log"), 40),
            self.fake_log_tail(30)
        )
    }

    fn fake_log_tail(&self, n: usize) -> String {
        self.fake.with(|b, _| {
            let skip = b.log.len().saturating_sub(n);
            b.log[skip..]
                .iter()
                .map(|l| {
                    format!(
                        "{} {} {} {}?{} {}",
                        l.at_ms, l.key, l.method, l.path, l.query, l.answer
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")
        })
    }

    /// The fake Telegram's messages as `(prefix, body)`: the prefix is the
    /// text up to its first `]` (`[OMS P1]`, `[WATCH P1]`).
    pub fn telegram_messages(&self) -> Vec<(String, String)> {
        self.telegram
            .requests()
            .iter()
            .filter(|r| r.path().ends_with("/sendMessage"))
            .filter_map(|r| serde_json::from_str::<Value>(&r.body_str()).ok())
            .filter_map(|v| v["text"].as_str().map(str::to_string))
            .map(|t| match t.find(']') {
                Some(i) => (t[..=i].to_string(), t[i + 1..].trim_start().to_string()),
                None => (String::new(), t),
            })
            .collect()
    }

    /// The times (ms) of the requests `key` sent to `path` at the fake.
    pub fn requests(&self, key: &str, path: &str) -> Vec<u64> {
        self.fake.with(|b, _| {
            b.log
                .iter()
                .filter(|l| l.key == key && l.path == path)
                .map(|l| l.at_ms)
                .collect()
        })
    }

    /// The venue invariants every test ends with: the watcher sent no
    /// create, amend or cancel (REST or WS Trade); no `orderLinkId` was
    /// created twice; at most 2 cancel-alls per second per key.
    pub fn assert_venue_invariants(&self) {
        let problems = self.fake.with(|b: &mut Book, _| {
            let mut p = Vec::new();
            for l in &b.log {
                if l.key == WKEY
                    && (l.method == "WS"
                        || ["/v5/order/create", "/v5/order/amend", "/v5/order/cancel"]
                            .contains(&l.path.as_str()))
                {
                    p.push(format!("the watcher's key sent {} {}", l.method, l.path));
                }
            }
            let links: std::collections::BTreeSet<String> = b
                .log
                .iter()
                .filter(|l| l.path == "order.create" || l.path == "/v5/order/create")
                .filter_map(|l| serde_json::from_str::<Value>(&l.body).ok())
                .filter_map(|v| v["orderLinkId"].as_str().map(str::to_string))
                .collect();
            for link in links {
                let n = b.creates_for(&link);
                if n > 1 {
                    p.push(format!("{n} creates for {link}"));
                }
            }
            for key in [OKEY, WKEY] {
                let t = b.cancel_alls(key);
                for (i, a) in t.iter().enumerate() {
                    let within = t[i..].iter().take_while(|b| **b < a + 1_000).count();
                    if within > 2 {
                        p.push(format!("{within} cancel-alls from {key} within 1 s of {a}"));
                    }
                }
            }
            p
        });
        if !problems.is_empty() {
            self.fail(&format!("venue invariants: {problems:?}"));
        }
    }
}

impl Drop for World {
    fn drop(&mut self) {
        if std::env::var("OMS_KEEP").is_err() && !std::thread::panicking() {
            let _ = std::fs::remove_dir_all(&self.dir);
        } else {
            eprintln!("test directory kept: {}", self.dir.display());
        }
    }
}

fn tail(path: &Path, n: usize) -> String {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let all: Vec<&str> = text.lines().collect();
    all[all.len().saturating_sub(n)..].join("\n")
}

/// The rig's guard: `bin --version` must print a line ending in
/// ` test-endpoints` (the watcher's test build).
pub fn version_guard(bin: &Path) -> Result<(), String> {
    let out = Command::new(bin)
        .arg("--version")
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("{} --version: {e}", bin.display()))?;
    let line = String::from_utf8_lossy(&out.stdout).trim_end().to_string();
    if out.status.success() && line.starts_with("watcher ") && line.ends_with(" test-endpoints") {
        Ok(())
    } else {
        Err(format!(
            "{} --version printed {line:?}: not the watcher's test build (`--features test-endpoints`)",
            bin.display()
        ))
    }
}

/// The fake `systemctl` (POSIX sh). One line per call in `systemctl.log`:
/// the argv, whether the marker exists, the time in ms. For
/// `--no-ask-password stop -- <UNIT>` it signals the pid of `o/oms.pid`
/// only if that process's command line holds the rig's `o/oms.toml`:
/// SIGTERM then SIGCONT (systemd follows the stop signal with SIGCONT,
/// systemd.kill(5)); then it waits up to 5,000 ms (판단(모델 아님)) for the
/// process to be gone, a zombie or another program. Any other call: exit 1.
fn systemctl_script(dir: &Path) -> String {
    let d = dir.display();
    format!(
        r#"#!/bin/sh
marker=absent
[ -e "{d}/s/kill.marker" ] && marker=present
echo "$* marker=$marker $(date +%s%3N)" >> "{d}/systemctl.log"
[ "$*" = "--no-ask-password stop -- {UNIT}" ] || {{ echo "systemctl: not the rig's stop: $*" >&2; exit 1; }}
[ -f "{d}/o/oms.pid" ] || {{ echo "systemctl: no o/oms.pid" >&2; exit 1; }}
pid=$(cat "{d}/o/oms.pid")
ours() {{ tr '\0' ' ' < /proc/$pid/cmdline 2>/dev/null | grep -qF "{d}/o/oms.toml"; }}
ours || {{ echo "systemctl: pid $pid is not the rig's OMS" >&2; exit 1; }}
kill -TERM "$pid"; kill -CONT "$pid"
i=0
while [ $i -lt 250 ]; do
  [ -d /proc/$pid ] || exit 0
  state=$(sed 's/.*) //' /proc/$pid/stat 2>/dev/null | cut -c1)
  [ "$state" = Z ] && exit 0
  ours || exit 0
  sleep 0.02; i=$((i+1))
done
echo "systemctl: pid $pid still runs after 5000 ms" >&2
exit 1
"#
    )
}

/// The OMS: `start` is the only place that builds an `oms-node` command.
pub struct Oms {
    dir: PathBuf,
    wiring: String,
    telegram: String,
    port: Arc<Mutex<u16>>,
    proc: Option<Proc>,
}

impl Oms {
    pub fn data(&self) -> PathBuf {
        self.dir.join("o/d")
    }

    fn config(&self) -> PathBuf {
        self.dir.join("o/oms.toml")
    }

    /// Starts `oms-node run --config o/oms.toml` with the two wiring
    /// variables, then `extra_env` (a name that begins `OMS_TEST_` is a
    /// test error: the wiring cannot be replaced or cleared here). Stderr
    /// goes to `o/node.log`; the pid to `o/oms.pid`, removed by the reaper
    /// before it records the exit. Waits until the admin socket answers or
    /// the process ended; if it ended because the `[http]` port was taken,
    /// moves the port and starts once more.
    pub fn start(&mut self, extra_env: &[(&str, &str)]) {
        if let Some((name, _)) = extra_env.iter().find(|(n, _)| n.starts_with("OMS_TEST_")) {
            panic!("Oms::start: {name} is set by the rig only");
        }
        assert!(
            self.proc.as_ref().is_none_or(|p| p.exit().is_some()),
            "the OMS is still running"
        );
        for attempt in 0..2 {
            let from = std::fs::metadata(self.dir.join("o/node.log")).map_or(0, |m| m.len());
            self.spawn(extra_env);
            let end = Instant::now() + Duration::from_secs(20);
            while Instant::now() < end && self.exit().is_none() && self.status_admin().is_none() {
                std::thread::sleep(Duration::from_millis(25));
            }
            let log = std::fs::read_to_string(self.dir.join("o/node.log")).unwrap_or_default();
            let new = log.get(from as usize..).unwrap_or("");
            if self.exit().is_none() || !new.contains("Address already in use") {
                return;
            }
            if attempt == 1 {
                precondition(format!(
                    "the [http] port {} is still in use after a retry",
                    self.port()
                ));
            }
            let mut p = self.port.lock().unwrap();
            let old = *p;
            *p = free_port();
            let cfg = std::fs::read_to_string(self.config()).unwrap();
            std::fs::write(
                self.config(),
                cfg.replace(
                    &format!("127.0.0.1:{old}\""),
                    &format!("127.0.0.1:{}\"", *p),
                ),
            )
            .unwrap();
        }
    }

    fn spawn(&mut self, extra_env: &[(&str, &str)]) {
        let log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.dir.join("o/node.log"))
            .unwrap();
        let child = Command::new(env!("CARGO_BIN_EXE_oms-node"))
            .arg("run")
            .arg("--config")
            .arg(self.config())
            .env("OMS_TEST_BYBIT_WIRING", &self.wiring)
            .env("OMS_TEST_TELEGRAM_API", &self.telegram)
            .envs(extra_env.iter().copied())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(log)
            .spawn()
            .unwrap();
        let pid_file = self.dir.join("o/oms.pid");
        std::fs::write(&pid_file, child.id().to_string()).unwrap();
        self.proc = Some(Proc::spawn(
            child,
            Box::new(move || {
                let _ = std::fs::remove_file(pid_file);
            }),
        ));
    }

    pub fn port(&self) -> u16 {
        *self.port.lock().unwrap()
    }

    pub fn pid(&self) -> Option<u32> {
        self.proc.as_ref().map(|p| p.pid)
    }

    /// The exit status and time, once the process ended.
    pub fn exit(&self) -> Option<(ExitStatus, Instant)> {
        self.proc.as_ref().and_then(Proc::exit)
    }

    pub fn log(&self) -> String {
        std::fs::read_to_string(self.dir.join("o/node.log")).unwrap_or_default()
    }

    /// `GET /api/status` over HTTP, as the watcher asks it.
    pub fn status_http(&self) -> Option<Value> {
        let port = self.port();
        let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).ok()?;
        s.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
        write!(
            s,
            "GET /api/status HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
        )
        .ok()?;
        let mut text = String::new();
        s.read_to_string(&mut text).ok()?;
        let (head, body) = text.split_once("\r\n\r\n")?;
        if !head.starts_with("HTTP/1.1 200") {
            return None;
        }
        serde_json::from_str(body).ok()
    }

    /// Admin `Status`.
    pub fn status_admin(&self) -> Option<Value> {
        let mut c = Client::connect(&self.data().join("admin.sock"), None).ok()?;
        c.call(Op::Status).ok()
    }

    /// A client of strategy `a` on the trading socket.
    pub fn client(&self) -> Client {
        Client::connect(&self.data().join("oms.sock"), Some("a")).unwrap()
    }

    /// A GTC limit order of `BTCUSDT`, valid for 30 s.
    pub fn new_order(
        &self,
        key: &str,
        side: Side,
        qty: &str,
        px: &str,
        reduce_only: bool,
    ) -> NewOrderReply {
        self.client()
            .new_order(Op::NewOrder {
                client_order_id: ClientOrderId::new(key).unwrap(),
                instrument: InstrumentId::new("BTCUSDT").unwrap(),
                side,
                qty: Qty::parse(qty).unwrap(),
                px: Px::parse(px).unwrap(),
                tif: Tif::Gtc,
                reduce_only,
                expires_at: now_us() + 30_000_000,
                market_ref: None,
            })
            .unwrap()
    }

    pub fn cancel(&self, id: OrderId) -> CancelReply {
        self.client().cancel(id).unwrap()
    }

    pub fn order(&self, id: OrderId) -> Order {
        self.client()
            .get_order(id)
            .unwrap()
            .expect("the order exists")
    }

    /// The order's `orderLinkId` at the venue.
    pub fn link(&self, id: OrderId) -> String {
        let instance = std::fs::read_to_string(self.data().join("INSTANCE")).unwrap();
        WireIdForm::Base32Pair.encode(VenueClientId {
            instance_id: u64::from_str_radix(instance.trim(), 16).unwrap(),
            order_id: id,
        })
    }

    /// `JOINT_OMSCTL_BIN --socket <admin.sock> args…`: exit code and stdout.
    pub fn omsctl(&self, args: &[&str]) -> (Option<i32>, String) {
        let out = Command::new(env_path("JOINT_OMSCTL_BIN"))
            .arg("--socket")
            .arg(self.data().join("admin.sock"))
            .args(args)
            .stdin(Stdio::null())
            .output()
            .unwrap();
        (
            out.status.code(),
            String::from_utf8_lossy(&out.stdout).into_owned(),
        )
    }

    pub fn records(&self) -> Vec<Record> {
        records(&self.data())
    }

    /// Every event named `name` with its record time.
    pub fn events(&self, name: &str) -> Vec<(Ts, Event)> {
        self.records()
            .into_iter()
            .flat_map(|r| r.events.into_iter().map(move |e| (r.at, e)))
            .filter(|(_, e)| e.name() == name)
            .collect()
    }

    fn signal(&self, sig: Signal) {
        let p = self.proc.as_ref().expect("the OMS was started");
        if p.exit().is_none() {
            send_signal(p.pid, sig);
        }
    }

    pub fn sigstop(&self) {
        self.signal(Signal::STOP)
    }
    pub fn sigcont(&self) {
        self.signal(Signal::CONT)
    }
    pub fn sigterm(&self) {
        self.signal(Signal::TERM)
    }
    pub fn sigkill(&self) {
        self.signal(Signal::KILL)
    }
}

/// The watcher of a world.
pub struct Watcher {
    dir: PathBuf,
    bin: PathBuf,
    wiring: String,
    proc: Option<Proc>,
}

impl Watcher {
    /// Starts `watcher --config w/watcher.toml` with
    /// `WATCHER_TEST_ENDPOINTS`; stderr to `w/watcher.log`.
    pub fn start(&mut self) {
        let wiring = self.wiring.clone();
        self.spawn(Some(&wiring));
    }

    /// Starts it without `WATCHER_TEST_ENDPOINTS` (its test build refuses).
    pub fn start_unwired(&mut self) {
        self.spawn(None);
    }

    fn spawn(&mut self, wiring: Option<&str>) {
        assert!(
            self.proc.as_ref().is_none_or(|p| p.exit().is_some()),
            "the watcher is still running"
        );
        let log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.dir.join("w/watcher.log"))
            .unwrap();
        let mut cmd = Command::new(&self.bin);
        cmd.arg("--config").arg(self.dir.join("w/watcher.toml"));
        match wiring {
            Some(w) => cmd.env("WATCHER_TEST_ENDPOINTS", w),
            None => cmd.env_remove("WATCHER_TEST_ENDPOINTS"),
        };
        let child = cmd
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(log)
            .spawn()
            .unwrap();
        self.proc = Some(Proc::spawn(child, Box::new(|| {})));
    }

    pub fn exit(&self) -> Option<(ExitStatus, Instant)> {
        self.proc.as_ref().and_then(Proc::exit)
    }

    /// Its stderr lines without the `watcher: ` prefix that start with
    /// `prefix`.
    pub fn log_lines(&self, prefix: &str) -> Vec<String> {
        std::fs::read_to_string(self.dir.join("w/watcher.log"))
            .unwrap_or_default()
            .lines()
            .filter_map(|l| l.strip_prefix("watcher: "))
            .filter(|l| l.starts_with(prefix))
            .map(str::to_string)
            .collect()
    }

    /// Every stderr line holding `text`.
    pub fn log_containing(&self, text: &str) -> Vec<String> {
        std::fs::read_to_string(self.dir.join("w/watcher.log"))
            .unwrap_or_default()
            .lines()
            .filter(|l| l.contains(text))
            .map(str::to_string)
            .collect()
    }

    fn signal(&self, sig: Signal) {
        let p = self.proc.as_ref().expect("the watcher was started");
        if p.exit().is_none() {
            send_signal(p.pid, sig);
        }
    }

    pub fn sigterm(&self) {
        self.signal(Signal::TERM)
    }
    pub fn sigkill(&self) {
        self.signal(Signal::KILL)
    }
}

/// The last value of the metric `name` (any labels) that the watcher
/// imported into the fake VictoriaMetrics.
pub fn metric(vm: &FakeHttp, name: &str) -> Option<f64> {
    vm.requests()
        .iter()
        .filter(|r| r.path() == "/api/v1/import")
        .flat_map(|r| {
            r.body_str()
                .lines()
                .filter_map(|l| serde_json::from_str::<Value>(l).ok())
                .collect::<Vec<_>>()
        })
        .filter(|v| v["metric"]["__name__"] == name)
        .filter_map(|v| {
            v["values"]
                .as_array()
                .and_then(|a| a.last())
                .and_then(Value::as_f64)
        })
        .next_back()
}
