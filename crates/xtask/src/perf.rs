//! End-to-end performance run: a real `oms-node`, a real `simvenue`, a real
//! journal on the directory under test, driven by the open-loop generator.
//!
//! The result file records the conditions next to the numbers: hardware,
//! kernel, storage device and mount options, free space, build, durability
//! setting (always `fdatasync` per commit; there is no switch to turn it
//! off), queue sizes, offered load and sample counts.

use crate::loadgen::{self, LoadCfg};
use oms_api::Op;
use oms_api::client::Client;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

fn sh(cmd: &str) -> String {
    Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

pub fn environment(dir: &Path) -> serde_json::Value {
    serde_json::json!({
        "cpu": sh("lscpu | grep 'Model name' | sed 's/.*: *//'"),
        "cpus_online": sh("nproc"),
        "governor": sh("cat /sys/devices/system/cpu/cpu0/cpufreq/scaling_governor 2>/dev/null"),
        "energy_pref": sh("cat /sys/devices/system/cpu/cpu0/cpufreq/energy_performance_preference 2>/dev/null"),
        "kernel": sh("uname -r"),
        "os": sh(". /etc/os-release; echo $PRETTY_NAME"),
        "memory": sh("free -h | awk '/Mem:/ {print $2}'"),
        "rustc": sh("rustc -V"),
        "mount": sh(&format!("findmnt -no SOURCE,FSTYPE,OPTIONS -T {}", dir.display())),
        "device_model": sh(&format!(
            "lsblk -no MODEL $(findmnt -no SOURCE -T {} | sed 's/p[0-9]*$//') 2>/dev/null | head -1",
            dir.display()
        )),
        "write_cache": sh(&format!(
            "cat /sys/block/$(basename $(findmnt -no SOURCE -T {} | sed 's/p[0-9]*$//'))/queue/write_cache 2>/dev/null",
            dir.display()
        )),
        "df": sh(&format!("df -h {} | tail -1", dir.display())),
        "loadavg": sh("cut -d' ' -f1-3 /proc/loadavg"),
        // What else the host was doing: a number measured next to another
        // heavy job is a number about both.
        "busiest_processes": sh("ps -eo pcpu,comm --sort=-pcpu | sed -n '2,4p' | tr -s ' ' | tr '\n' ';'"),
    })
}

struct Procs {
    venue: Child,
    node: Option<Child>,
}

impl Drop for Procs {
    fn drop(&mut self) {
        if let Some(mut n) = self.node.take() {
            let _ = n.kill();
            let _ = n.wait();
        }
        let _ = self.venue.kill();
        let _ = self.venue.wait();
    }
}

fn venue_ctl(sock: &Path, action: serde_json::Value) -> Result<(), String> {
    let mut s = UnixStream::connect(sock).map_err(|e| e.to_string())?;
    let line = serde_json::json!({"id": 1, "op": "ctl", "action": action}).to_string();
    s.write_all(format!("{line}\n").as_bytes()).map_err(|e| e.to_string())?;
    let mut out = String::new();
    BufReader::new(s).read_line(&mut out).map_err(|e| e.to_string())?;
    if out.contains("\"ok\":true") { Ok(()) } else { Err(out) }
}

/// The node configuration of a measurement.
///
/// The default form lifts every admission limit far out of the way (window,
/// per-strategy rates, open orders, outstanding sends, memory), because the
/// question is what the pipeline itself can carry. `default_limits` keeps the
/// shipped limits instead, to show what an overload looks like to a client.
/// The text is stored in the report, so a number can always be read together
/// with the configuration that produced it.
fn config_text(dir: &Path, inline: bool, extra_runtime: &str, default_limits: bool) -> String {
    if default_limits {
        return format!(
            r#"
data_dir = "{data}"

[runtime]
inline_commit = {inline}
tick_ms = 100
reconcile_ms = 200
sweep_ms = 5000
{extra_runtime}

[venue]
kind = "sim"
socket = "{venue}"
request_timeout_ms = 2000

[core]
version = 1

[core.instruments.X]
qty_step = "0.001"
px_tick = "0.1"
min_notional = "0"
max_order_qty = "100"
max_order_notional = "100000000"
account_pos_min = "-100000000"
account_pos_max = "100000000"

[core.strategies.a]
enabled = true
max_open_orders = 500
[core.strategies.a.limits.X]
min = "-100000000"
max = "100000000"

# Every policy value at its shipped default.
[core.policy]
"#,
            data = dir.join("data").display(),
            venue = dir.join("venue.sock").display(),
        );
    }
    format!(
        r#"
data_dir = "{data}"

[api]
window = 512

[runtime]
inline_commit = {inline}
tick_ms = 100
reconcile_ms = 200
sweep_ms = 5000
snapshot_every = 100000000
{extra_runtime}

[limits]
new_orders_per_sec = 1000000
new_orders_burst = 1000000
cancels_per_sec = 1000000
cancels_burst = 1000000

[venue]
kind = "sim"
socket = "{venue}"
request_timeout_ms = 2000

[core]
version = 1

[core.instruments.X]
qty_step = "0.001"
px_tick = "0.1"
min_notional = "0"
max_order_qty = "100"
max_order_notional = "100000000"
account_pos_min = "-100000000"
account_pos_max = "100000000"

[core.strategies.a]
enabled = true
max_open_orders = 100000
[core.strategies.a.limits.X]
min = "-100000000"
max = "100000000"

[core.policy]
max_open_orders = 100000
max_inflight_sends = 4096
max_orders_in_memory = 4000000
closed_retention_us = 5000000
dedup_grace_us = 1000000
purge_interval_us = 1000000
"#,
        data = dir.join("data").display(),
        venue = dir.join("venue.sock").display(),
    )
}

fn wait_ready(admin: &Path) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Ok(mut c) = Client::connect(admin, None) {
            if let Ok(s) = c.call(Op::Status) {
                if s["core"]["holds"].as_array().is_some_and(|h| h.is_empty()) {
                    return Ok(());
                }
            }
        }
        if Instant::now() > deadline {
            return Err("node did not become ready".into());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

pub struct PerfCfg {
    pub dir: PathBuf,
    pub bin_dir: PathBuf,
    pub rates: Vec<u64>,
    pub seconds: u64,
    pub connections: u64,
    pub inline: bool,
    pub label: String,
    pub out: PathBuf,
    pub extra_runtime: String,
    pub default_limits: bool,
}

pub fn run(cfg: PerfCfg) -> i32 {
    match run_inner(&cfg) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("perf: {e}");
            1
        }
    }
}

fn run_inner(cfg: &PerfCfg) -> Result<(), String> {
    let _ = std::fs::remove_dir_all(&cfg.dir);
    std::fs::create_dir_all(&cfg.dir).map_err(|e| e.to_string())?;
    // Absolute paths: the node resolves relative ones against its config.
    let abs = cfg.dir.canonicalize().map_err(|e| e.to_string())?;
    let cfg = &PerfCfg {
        dir: abs,
        bin_dir: cfg.bin_dir.clone(),
        rates: cfg.rates.clone(),
        seconds: cfg.seconds,
        connections: cfg.connections,
        inline: cfg.inline,
        label: cfg.label.clone(),
        out: cfg.out.clone(),
        extra_runtime: cfg.extra_runtime.clone(),
        default_limits: cfg.default_limits,
    };
    let venue_sock = cfg.dir.join("venue.sock");
    let cfg_path = cfg.dir.join("oms.toml");
    let node_config = config_text(&cfg.dir, cfg.inline, &cfg.extra_runtime, cfg.default_limits);
    std::fs::write(&cfg_path, &node_config).map_err(|e| e.to_string())?;
    let node_bin = cfg.bin_dir.join("oms-node");
    let version = sh(&format!("{} version", node_bin.display()));

    let venue = Command::new(cfg.bin_dir.join("simvenue"))
        .arg(&venue_sock)
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("start simvenue: {e}"))?;
    let mut procs = Procs { venue, node: None };
    let deadline = Instant::now() + Duration::from_secs(5);
    while UnixStream::connect(&venue_sock).is_err() {
        if Instant::now() > deadline {
            return Err("simvenue did not start".into());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    venue_ctl(&venue_sock, serde_json::json!({"auto_fill": {"mode": "full"}}))?;

    let env = environment(&cfg.dir);
    println!("# environment: {env}");
    println!("# build: {version}; commit layout: {}", if cfg.inline { "inline" } else { "commit thread" });

    let mut runs = Vec::new();
    for (i, rate) in cfg.rates.iter().enumerate() {
        // A fresh node per rate, so the stage statistics belong to one run.
        let log = std::fs::File::create(cfg.dir.join(format!("node-{rate}.log"))).map_err(|e| e.to_string())?;
        let node = Command::new(&node_bin)
            .arg("run")
            .arg("--config")
            .arg(&cfg_path)
            .stderr(log)
            .spawn()
            .map_err(|e| format!("start oms-node: {e}"))?;
        procs.node = Some(node);
        let admin = cfg.dir.join("data/admin.sock");
        wait_ready(&admin)?;

        let result = loadgen::run(LoadCfg {
            socket: cfg.dir.join("data/oms.sock").display().to_string(),
            rate: *rate,
            seconds: cfg.seconds,
            connections: cfg.connections,
            strategy: "a".into(),
            run_id: format!("p{i}r{rate}"),
            duplicate_every: 0,
            ttl_ms: 10_000,
        })?;
        // Let the venue's answers and fills drain before reading statistics.
        std::thread::sleep(Duration::from_millis(1500));
        let status = Client::connect(&admin, None)
            .and_then(|mut c| c.call(Op::Status))
            .map_err(|e| e.to_string())?;
        let stage = &status["pipeline"]["stage_us"];
        println!(
            "rate={rate} answered={} unanswered={} errors={} accepted={} | intended->resp p50={} p99={} p999={} max={} us | queue p99={} core p99={} persist p99={} sync p50={} p99={} us | batch p50={} max={}",
            result["answered"],
            result["unanswered"],
            result["errors"],
            result["accepted"],
            result["latency_intended_to_response"]["p50"],
            result["latency_intended_to_response"]["p99"],
            result["latency_intended_to_response"]["p999"],
            result["latency_intended_to_response"]["max"],
            stage["queue"]["p99_us"],
            stage["core"]["p99_us"],
            stage["persist"]["p99_us"],
            stage["sync"]["p50_us"],
            stage["sync"]["p99_us"],
            stage["batch"]["p50_us"],
            stage["batch"]["max_us"],
        );
        runs.push(serde_json::json!({
            "load": result,
            "stage_us": stage,
            "queues": status["pipeline"]["queues"],
            "pipeline": status["pipeline"]["stats"],
            "journal": status["journal"],
            "core_stats": status["core"]["stats"],
            "holds_at_end": status["core"]["holds"],
            "ingress_rate_limited": status["pipeline"]["ingress_rate_limited"],
            "ingress_refused_overload": status["pipeline"]["ingress_refused_overload"],
        }));
        if let Some(mut n) = procs.node.take() {
            let _ = Command::new("kill").arg("-TERM").arg(n.id().to_string()).status();
            let _ = n.wait();
        }
    }
    let report = serde_json::json!({
        "label": cfg.label,
        "taken_at_us": oms_api::client::now_us(),
        "build": version,
        "commit_layout": if cfg.inline { "inline" } else { "commit_thread" },
        "durability": "fdatasync after every commit batch",
        "environment": env,
        "venue": "simvenue (auto-fill full), same host, Unix socket",
        "seconds_per_rate": cfg.seconds,
        "connections": cfg.connections,
        "limits": if cfg.default_limits { "shipped defaults" } else { "admission limits lifted (see node_config)" },
        "node_config": node_config,
        "source_digest": crate::digest::source_digest(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")),
        "runs": runs,
    });
    if let Some(parent) = cfg.out.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(&cfg.out, serde_json::to_string_pretty(&report).unwrap_or_default()).map_err(|e| e.to_string())?;
    println!("# written {}", cfg.out.display());
    Ok(())
}

/// Builds a journal by running load, kills the node so that no final snapshot
/// exists, restarts it and reads how long the replay took.
pub fn recovery_time(dir: PathBuf, orders: u64) -> i32 {
    match recovery_inner(&dir, orders) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("recovery-time: {e}");
            1
        }
    }
}

fn recovery_inner(dir: &Path, orders: u64) -> Result<(), String> {
    let _ = std::fs::remove_dir_all(dir);
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let dir = dir.canonicalize().map_err(|e| e.to_string())?;
    let bin_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .ok_or("cannot locate the binaries")?;
    let venue_sock = dir.join("venue.sock");
    let cfg_path = dir.join("oms.toml");
    std::fs::write(&cfg_path, config_text(&dir, false, "", false)).map_err(|e| e.to_string())?;
    let node_bin = bin_dir.join("oms-node");
    let version = sh(&format!("{} version", node_bin.display()));
    let venue = Command::new(bin_dir.join("simvenue"))
        .arg(&venue_sock)
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("start simvenue: {e}"))?;
    let mut procs = Procs { venue, node: None };
    let deadline = Instant::now() + Duration::from_secs(5);
    while UnixStream::connect(&venue_sock).is_err() {
        if Instant::now() > deadline {
            return Err("simvenue did not start".into());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    venue_ctl(&venue_sock, serde_json::json!({"auto_fill": {"mode": "full"}}))?;
    let start_node = |procs: &mut Procs, name: &str| -> Result<(), String> {
        let log = std::fs::File::create(dir.join(name)).map_err(|e| e.to_string())?;
        procs.node = Some(
            Command::new(&node_bin)
                .arg("run")
                .arg("--config")
                .arg(&cfg_path)
                .stderr(log)
                .spawn()
                .map_err(|e| format!("start oms-node: {e}"))?,
        );
        Ok(())
    };
    let admin = dir.join("data/admin.sock");
    start_node(&mut procs, "node-load.log")?;
    wait_ready(&admin)?;
    let seconds = 8;
    loadgen::run(LoadCfg {
        socket: dir.join("data/oms.sock").display().to_string(),
        rate: orders / seconds,
        seconds,
        connections: 4,
        strategy: "a".into(),
        run_id: "rec".into(),
        duplicate_every: 0,
        ttl_ms: 10_000,
    })?;
    std::thread::sleep(Duration::from_millis(1500));
    let before = Client::connect(&admin, None)
        .and_then(|mut c| c.call(Op::Status))
        .map_err(|e| e.to_string())?;
    let durable = before["journal"]["durable_seq"].as_u64().unwrap_or(0);
    // SIGKILL: no drain, no final snapshot.
    if let Some(mut n) = procs.node.take() {
        let _ = n.kill();
        let _ = n.wait();
    }
    let journal_bytes: u64 = std::fs::read_dir(dir.join("data"))
        .map_err(|e| e.to_string())?
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with("journal-"))
        .filter_map(|e| e.metadata().ok())
        .map(|m| m.len())
        .sum();
    start_node(&mut procs, "node-recover.log")?;
    wait_ready(&admin)?;
    let after = Client::connect(&admin, None)
        .and_then(|mut c| c.call(Op::Status))
        .map_err(|e| e.to_string())?;
    let rec = &after["node"]["recovery"];
    let report = serde_json::json!({
        "label": "recovery",
        "taken_at_us": oms_api::client::now_us(),
        "build": version,
        "environment": environment(&dir),
        "how": "load, SIGKILL, restart; no snapshot existed, so the whole journal was replayed",
        "durable_seq_before_kill": durable,
        "journal_bytes": journal_bytes,
        "records_replayed": rec["records_replayed"],
        "elapsed_ms": rec["elapsed_ms"],
        "snapshot_seq": rec["snapshot_seq"],
        "orders_in_memory_after": after["core"]["orders_in_memory"],
    });
    println!("{}", serde_json::to_string_pretty(&report).unwrap_or_default());
    std::fs::create_dir_all("evidence/perf").map_err(|e| e.to_string())?;
    std::fs::write("evidence/perf/recovery.json", serde_json::to_string_pretty(&report).unwrap_or_default())
        .map_err(|e| e.to_string())?;
    Ok(())
}
