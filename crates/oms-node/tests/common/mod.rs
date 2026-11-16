//! Harness for process-level tests: a real `oms-node` process, a real
//! `simvenue` process, real sockets and a real journal on disk.
//!
//! The data directory is under `CARGO_TARGET_TMPDIR`, which is on whatever
//! file system holds the build output. `OMS_TEST_DIR` overrides it. Note that
//! a tmpfs there would make every flush a no-op; the tests would still check
//! ordering and recovery logic but nothing about a device.

#![allow(dead_code, clippy::unwrap_used)]

use oms_api::client::{Client, ClientError, now_us};
use oms_api::*;
use oms_core::*;
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

pub struct Env {
    pub dir: PathBuf,
    pub venue: Option<Child>,
    pub node: Option<Child>,
    pub extra_runtime: String,
    pub extra_policy: String,
    pub extra_limits: String,
    pub extra_api: String,
    /// Environment variables for the node process (failpoints).
    pub node_env: Vec<(String, String)>,
    venue_id: u64,
}

fn base_dir() -> PathBuf {
    std::env::var("OMS_TEST_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_TARGET_TMPDIR")))
}

impl Env {
    /// Creates the directory and starts the venue. The node is not started.
    pub fn new(name: &str) -> Env {
        let dir = base_dir().join(format!("e2e-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut env = Env {
            dir,
            venue: None,
            node: None,
            extra_runtime: String::new(),
            extra_policy: String::new(),
            extra_limits: String::new(),
            extra_api: String::new(),
            node_env: Vec::new(),
            venue_id: 0,
        };
        env.start_venue();
        env
    }

    /// Venue and node both running and the node ready to trade.
    pub fn started(name: &str) -> Env {
        let mut env = Env::new(name);
        env.start_node();
        env.wait_ready();
        env
    }

    pub fn venue_sock(&self) -> PathBuf {
        self.dir.join("venue.sock")
    }
    pub fn data_dir(&self) -> PathBuf {
        self.dir.join("data")
    }
    pub fn sock(&self) -> PathBuf {
        self.data_dir().join("oms.sock")
    }
    pub fn admin_sock(&self) -> PathBuf {
        self.data_dir().join("admin.sock")
    }
    pub fn config_path(&self) -> PathBuf {
        self.dir.join("oms.toml")
    }

    pub fn start_venue(&mut self) {
        let log = std::fs::File::create(self.dir.join("venue.log")).unwrap();
        let child = Command::new(env!("CARGO_BIN_EXE_simvenue"))
            .arg(self.venue_sock())
            .stderr(log)
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        self.venue = Some(child);
        let deadline = Instant::now() + Duration::from_secs(5);
        while UnixStream::connect(self.venue_sock()).is_err() {
            assert!(Instant::now() < deadline, "simvenue did not start");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    pub fn write_config(&self) {
        let text = format!(
            r#"
data_dir = "{data}"

[api]
{extra_api}

[runtime]
tick_ms = 20
reconcile_ms = 50
sweep_ms = 500
{extra_runtime}

[limits]
new_orders_per_sec = 100000
new_orders_burst = 100000
cancels_per_sec = 100000
cancels_burst = 100000
{extra_limits}

[venue]
kind = "sim"
socket = "{venue}"
request_timeout_ms = 300

[core]
version = 1

[core.instruments.X]
qty_step = "0.001"
px_tick = "0.1"
min_notional = "0"
max_order_qty = "100"
max_order_notional = "100000000"
account_pos_min = "-1000"
account_pos_max = "1000"

[core.strategies.a]
enabled = true
max_open_orders = 400
[core.strategies.a.limits.X]
min = "-1000"
max = "1000"

[core.strategies.b]
enabled = true
max_open_orders = 400
[core.strategies.b.limits.X]
min = "-1000"
max = "1000"

[core.policy]
max_open_orders = 400
max_inflight_sends = 64
absence_margin_us = 300000
cancel_retry_backoff_us = 100000
max_expiry_horizon_us = 60000000
{extra_policy}
"#,
            data = self.data_dir().display(),
            venue = self.venue_sock().display(),
            extra_runtime = self.extra_runtime,
            extra_api = self.extra_api,
            extra_limits = self.extra_limits,
            extra_policy = self.extra_policy,
        );
        std::fs::write(self.config_path(), text).unwrap();
    }

    pub fn start_node(&mut self) {
        self.write_config();
        let log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.dir.join("node.log"))
            .unwrap();
        let child = Command::new(env!("CARGO_BIN_EXE_oms-node"))
            .arg("run")
            .arg("--config")
            .arg(self.config_path())
            .envs(self.node_env.iter().cloned())
            .stderr(log)
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        self.node = Some(child);
    }

    /// SIGKILL: the process gets no chance to flush or clean up.
    pub fn kill_node(&mut self) {
        if let Some(mut c) = self.node.take() {
            c.kill().unwrap();
            c.wait().unwrap();
        }
    }

    /// SIGTERM and wait for a clean exit. Returns the exit code.
    pub fn stop_node(&mut self) -> i32 {
        let mut c = self.node.take().expect("node running");
        let pid = rustix::process::Pid::from_child(&c);
        rustix::process::kill_process(pid, rustix::process::Signal::TERM).unwrap();
        c.wait().unwrap().code().unwrap_or(-1)
    }

    /// Waits for the node process to exit by itself.
    pub fn wait_exit(&mut self, timeout: Duration) -> Option<i32> {
        let deadline = Instant::now() + timeout;
        let c = self.node.as_mut()?;
        loop {
            if let Some(st) = c.try_wait().unwrap() {
                self.node = None;
                return st.code();
            }
            if Instant::now() > deadline {
                return None;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    pub fn client(&self, strategy: &str) -> Client {
        Client::connect(&self.sock(), Some(strategy)).unwrap()
    }

    pub fn admin(&self) -> Client {
        Client::connect(&self.admin_sock(), None).unwrap()
    }

    pub fn try_status(&self) -> Option<Value> {
        let mut c = Client::connect(&self.admin_sock(), None).ok()?;
        c.call(Op::Status).ok()
    }

    pub fn status(&self) -> Value {
        self.try_status().expect("status")
    }

    /// Ready: link up, reconciled, nothing in doubt, no hold.
    pub fn wait_ready(&self) {
        self.wait_ready_for(Duration::from_secs(20));
    }

    pub fn wait_ready_for(&self, timeout: Duration) {
        let ok = self.wait(timeout, || {
            self.try_status().is_some_and(|s| {
                s["core"]["holds"].as_array().is_some_and(|h| h.is_empty())
                    && s["core"]["needs"].as_array().is_some_and(|n| n.is_empty())
                    && s["core"]["closing"] == 0
                    && s["core"]["boot"].as_u64().unwrap_or(0) > 0
            })
        });
        if !ok {
            panic!(
                "node not ready; status: {}\nlog tail:\n{}",
                self.try_status().map(|s| s["core"].to_string()).unwrap_or_default(),
                self.log_tail(30)
            );
        }
    }

    pub fn wait(&self, timeout: Duration, mut cond: impl FnMut() -> bool) -> bool {
        let deadline = Instant::now() + timeout;
        loop {
            if cond() {
                return true;
            }
            if Instant::now() > deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(15));
        }
    }

    pub fn log_tail(&self, lines: usize) -> String {
        let text = std::fs::read_to_string(self.dir.join("node.log")).unwrap_or_default();
        let all: Vec<&str> = text.lines().collect();
        all[all.len().saturating_sub(lines)..].join("\n")
    }

    pub fn instance_id(&self) -> u64 {
        let text = std::fs::read_to_string(self.data_dir().join("INSTANCE")).unwrap();
        u64::from_str_radix(text.trim(), 16).unwrap()
    }

    pub fn cloid(&self, order: OrderId) -> String {
        VenueClientId {
            instance_id: self.instance_id(),
            order_id: order,
        }
        .to_hex()
    }

    /// One request to the simulated venue, directly, bypassing the OMS.
    pub fn venue_call(&mut self, op: Value) -> Value {
        self.venue_id += 1;
        let mut req = op;
        req["id"] = json!(self.venue_id);
        let mut s = UnixStream::connect(self.venue_sock()).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let mut line = serde_json::to_vec(&req).unwrap();
        line.push(b'\n');
        s.write_all(&line).unwrap();
        let mut out = String::new();
        BufReader::new(s).read_line(&mut out).unwrap();
        serde_json::from_str(&out).unwrap_or_else(|e| panic!("venue answered {out:?}: {e}"))
    }

    pub fn ctl(&mut self, action: Value) -> Value {
        let r = self.venue_call(json!({"op": "ctl", "action": action}));
        assert_eq!(r["ok"], true, "venue ctl failed: {r}");
        r["result"].clone()
    }

    pub fn venue_dump(&mut self) -> Value {
        self.ctl(json!("dump"))
    }

    /// Runs an `oms-node` subcommand against this data directory.
    pub fn tool(&self, args: &[&str]) -> (i32, String, String) {
        let out = Command::new(env!("CARGO_BIN_EXE_oms-node"))
            .args(args)
            .arg("--data-dir")
            .arg(self.data_dir())
            .output()
            .unwrap();
        (
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )
    }

    /// Every record of the journal on disk, read without the lock.
    pub fn journal_records(&self) -> Vec<Record> {
        let mut records = Vec::new();
        oms_journal::verify_all(&self.data_dir(), |_, _, payload| {
            records.push(serde_json::from_slice::<Record>(payload).map_err(|e| e.to_string())?);
            Ok(())
        })
        .unwrap();
        records
    }

    /// How many events with this name the journal on disk holds.
    pub fn journal_count(&self, event: &str) -> usize {
        self.journal_records().iter().flat_map(|r| r.events.iter()).filter(|e| e.name() == event).count()
    }

    pub fn journal_segments(&self) -> Vec<PathBuf> {
        oms_journal::list_segments(&self.data_dir())
            .unwrap()
            .into_iter()
            .map(|(_, p)| p)
            .collect()
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        if let Some(mut c) = self.node.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
        if let Some(mut c) = self.venue.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
        if std::env::var("OMS_KEEP").is_err() && !std::thread::panicking() {
            let _ = std::fs::remove_dir_all(&self.dir);
        } else {
            eprintln!("test directory kept: {}", self.dir.display());
        }
    }
}

pub fn order_op(key: &str, side: Side, qty: &str, px: &str, tif: Tif, ttl_ms: u64) -> Op {
    Op::NewOrder {
        client_order_id: ClientOrderId::new(key).unwrap(),
        instrument: InstrumentId::new("X").unwrap(),
        side,
        qty: Qty::parse(qty).unwrap(),
        px: Px::parse(px).unwrap(),
        tif,
        reduce_only: false,
        expires_at: now_us() + ttl_ms * 1000,
    }
}

pub fn accept(c: &mut Client, key: &str, side: Side, qty: &str, px: &str) -> OrderId {
    match c.new_order(order_op(key, side, qty, px, Tif::Gtc, 5_000)).unwrap() {
        NewOrderReply::Accepted { order_id } => order_id,
        other => panic!("expected acceptance, got {other:?}"),
    }
}

pub fn api_error(r: Result<Value, ClientError>) -> ErrorCode {
    match r {
        Err(ClientError::Api(e)) => e.code,
        other => panic!("expected an API error, got {other:?}"),
    }
}

pub fn phase_of(c: &mut Client, id: OrderId) -> Phase {
    c.get_order(id).unwrap().expect("order exists").phase
}

pub fn exists(p: &Path) -> bool {
    p.exists()
}

// ---- C-MARGIN ------------------------------------------------------------------

/// A node with a margin policy at its defaults and a 100 ms margin poll,
/// ready, with the venue's account value already shown in `status`: the
/// first report became the day base.
pub fn margin_node(name: &str, account_value: &str) -> Env {
    margin_node_with(name, account_value, Vec::new())
}

/// `margin_node` with environment variables for the node process
/// (failpoints).
pub fn margin_node_with(name: &str, account_value: &str, node_env: Vec<(String, String)>) -> Env {
    let mut env = Env::new(name);
    env.node_env = node_env;
    env.extra_runtime = "margin_poll_ms = 100".into();
    env.extra_policy = "\n[core.policy.margin]\naccount_modes_allowed = [\"standard\"]\n".into();
    env.ctl(json!({"account_value": {"value": account_value}}));
    env.start_node();
    env.wait_ready();
    wait_margin(&env, "the account value", |m| m["account_value"] == account_value);
    env
}

/// Waits until `core.margin` of `status` satisfies `cond`.
pub fn wait_margin(env: &Env, what: &str, cond: impl Fn(&Value) -> bool) {
    let ok = env.wait(Duration::from_secs(5), || env.try_status().is_some_and(|st| cond(&st["core"]["margin"])));
    assert!(ok, "status never showed {what}: {}", env.status()["core"]["margin"]);
}

/// The venue's account value changes and the node enters REDUCING on it.
pub fn lose_to(env: &mut Env, account_value: &str) {
    env.ctl(json!({"account_value": {"value": account_value}}));
    wait_margin(env, "REDUCING", |m| m["reducing"]["rule"] == "daily_loss" && m["account_value"] == account_value);
}

/// Places one resting order and returns its id. A refusal that is only a
/// matter of pace (too many sends outstanding, the reference price not yet
/// arrived) is tried again.
pub fn rest(env: &mut Env, c: &mut Client, key: &str, side: Side, px: &str, reduce_only: bool) -> OrderId {
    for attempt in 0..100 {
        let Op::NewOrder { client_order_id, instrument, side, qty, px, tif, expires_at, .. } =
            order_op(&format!("{key}-{attempt}"), side, "1", px, Tif::Gtc, 60_000)
        else {
            unreachable!()
        };
        let op = Op::NewOrder { client_order_id, instrument, side, qty, px, tif, reduce_only, expires_at };
        match c.new_order(op).unwrap() {
            NewOrderReply::Accepted { order_id } => return order_id,
            NewOrderReply::Rejected { reasons }
                if reasons.iter().all(|r| matches!(r, RejectReason::GatewayBusy | RejectReason::MarginUnavailable)) =>
            {
                env.ctl(json!({"ref_price": {"instrument": "X", "px": "100"}}));
                std::thread::sleep(Duration::from_millis(20));
            }
            other => panic!("order {key} was refused: {other:?}"),
        }
    }
    panic!("order {key} was never accepted");
}

/// The venue's own list of orders that are open, by client id.
pub fn open_at_venue(env: &mut Env) -> std::collections::BTreeSet<String> {
    let dump = env.venue_dump();
    dump["orders"]
        .as_array()
        .expect("orders")
        .iter()
        .filter(|o| o["status"] == "open")
        .map(|o| o["client"].as_str().expect("client id").to_string())
        .collect()
}
