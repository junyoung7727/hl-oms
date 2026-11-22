//! Startup, wiring and shutdown of the OMS process.
//!
//! Startup order, each step finishing before the next begins:
//!
//! 1. Recover the state from snapshot and journal (no sends are possible).
//! 2. Start the direct alerts, if `[alert]` is configured (C-ALERT), then
//!    the core thread and the commit stage.
//! 3. Look at the kill marker, submit `Boot` with what was found there, then
//!    the configuration from the file. Wait until both are durable. A
//!    configuration the core refuses stops the process.
//! 4. Start the venue gateway, the reconciler, the timer and the storage
//!    monitor. The link is down until the gateway reports it up, and new
//!    orders are held until reconciliation has confirmed the state.
//! 5. Open the local sockets.
//!
//! Shutdown on SIGINT/SIGTERM: close the sockets, close the input queues,
//! let the core and the commit stage drain, write a final snapshot, exit 0.
//! Open orders are left at the venue. A fatal condition (journal failure,
//! core fault) releases nothing further and exits with status 2, also when
//! it happens during the drain or before the configuration is active. A
//! panic aborts the process (SIGABRT). Before either exit the alerts queue
//! the stop notice or the fatal message and wait up to `exit_flush_ms` for
//! delivery. A start-up failure (exit 3) after the configuration is active
//! queues a start-failure notice with the error kind and waits the same.

use crate::alerts::{self, Alerts};
use crate::config::{NodeConfig, VenueConfig};
use crate::pipeline::{self, PipelineConfig, STATE_SCHEMA, Shared, SnapshotState};
use crate::published::Published;
use crate::queues::{CoreMsg, InputQueues, Lane};
use crate::server::{self, ApiCtx};
use crate::status::{KillMarkerInfo, NodeInfo};
use crate::util::{Fatal, wall_us};
use crate::venue::{self, GatewayPorts, VenueGateway, VenueInputTx};
use crate::{error, info, recovery, warn};
use oms_core::*;
use oms_journal::SnapshotInfo;
use std::io::Read;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::time::Duration;
use tokio::sync::mpsc;

pub fn writer_name() -> String {
    format!("oms-node {} src:{}", crate::VERSION, crate::SOURCE_DIGEST)
}

/// Runs the node until it is told to stop. Returns the process exit code.
pub fn run(cfg: NodeConfig) -> i32 {
    let recovered = match recovery::recover(&cfg.data_dir, cfg.runtime.segment_max_bytes, &writer_name()) {
        Ok(r) => r,
        Err(e) => {
            error!("recovery failed; not starting", "error": e, "data_dir": cfg.data_dir.display().to_string());
            return 3;
        }
    };
    info!("recovered", "report": serde_json::to_value(&recovered.report).unwrap_or_default());

    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .worker_threads(cfg.runtime.worker_threads.max(1))
        .thread_name("oms-io")
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            error!("cannot start the async runtime", "error": e.to_string());
            return 3;
        }
    };

    let report = recovered.report.clone();
    let queues = Arc::new(InputQueues::new(
        cfg.runtime.venue_queue,
        cfg.runtime.control_queue,
        cfg.runtime.orders_queue,
    ));
    let published = Arc::new(Published::new(recovered.core.clone(), report.last_seq));
    published.write().stats.last_snapshot_seq = report.snapshot_seq.unwrap_or(0);
    let fatal = Arc::new(Fatal::default());
    let shared = Arc::new(Shared {
        queues: queues.clone(),
        published: published.clone(),
        fatal: fatal.clone(),
        snapshot_requested: AtomicBool::new(false),
        durable_seq: AtomicU64::new(report.last_seq),
        #[cfg(feature = "failpoints")]
        fail_next_sync: AtomicBool::new(false),
    });

    // The direct alerts start before the gateway, so that every later
    // failure, a fatal one included, can be told (C-ALERT).
    let alerts = match start_alerts(&cfg, &published, &fatal) {
        Ok(a) => a,
        Err(e) => {
            error!("cannot start the direct alerts; not starting", "error": e);
            return 3;
        }
    };

    let policy = &cfg.core.policy;
    let command_capacity = (policy.max_inflight_sends + policy.max_open_orders) as usize + 64;
    let (cmd_tx, cmd_rx) = mpsc::channel::<VenueCommand>(command_capacity);
    let (query_tx, query_rx) = mpsc::channel::<venue::QueryReq>(256);

    let pipe = pipeline::spawn(
        recovered.core,
        recovered.writer,
        report.last_at,
        shared.clone(),
        cmd_tx,
        PipelineConfig {
            max_batch: cfg.runtime.max_batch,
            snapshot_every: cfg.runtime.snapshot_every,
            inline_commit: cfg.runtime.inline_commit,
            data_dir: cfg.data_dir.clone(),
            writer_name: writer_name(),
        },
    );

    // The kill marker is read once, before the boot record (C-HOLD).
    let kill_marker = match &cfg.kill {
        Some(k) => {
            let found = read_kill_marker(&k.marker);
            if let Some(text) = &found {
                warn!("kill marker present: the kill switch latches", "path": k.marker.display().to_string(), "marker": text);
            }
            found
        }
        None => {
            warn!("no [kill] section: no kill marker is looked for");
            None
        }
    };
    let marker_info = KillMarkerInfo {
        configured: cfg.kill.is_some(),
        path: cfg.kill.as_ref().map(|k| k.marker.display().to_string()),
        present_at_boot: kill_marker.is_some(),
        file: cfg.kill.as_ref().map(|k| k.marker.clone()),
    };

    let code = runtime.block_on(async {
        // Step 3: boot and configuration, in that order, both durable.
        if queues
            .try_push(Lane::Control, CoreMsg::new(Input::Boot { binary: writer_name(), kill_marker }))
            .is_err()
        {
            return 3;
        }
        let (msg, rx) = CoreMsg::with_reply(Input::Config(Box::new(cfg.core.clone())));
        if queues.try_push(Lane::Control, msg).is_err() {
            return 3;
        }
        match rx.await {
            Ok(Reply::Config(Ok(version))) => {
                info!("configuration active", "version": version);
                if let Some(a) = &alerts {
                    a.activate();
                }
            }
            Ok(Reply::Config(Err(errs))) => {
                error!("the configuration was refused; not starting", "errors": errs);
                return 3;
            }
            other => {
                error!("no configuration reply", "reply": format!("{other:?}"), "fatal": fatal.reason());
                return 3;
            }
        }
        let (boot, instance_id) = {
            let p = published.read();
            (p.core.boot, p.core.instance_id)
        };
        // A start-up failure from here on is told (exit 3, the error kind
        // only: no path, no token).
        let start_failed = |kind: &'static str| {
            if let Some(a) = &alerts {
                a.start_failed(kind);
            }
            3
        };

        // Step 4: gateway, reconciler, timer, storage monitor.
        let ports = GatewayPorts {
            commands: cmd_rx,
            queries: query_rx,
            input: VenueInputTx::new(queues.clone()),
            instance_id,
            boot,
        };
        let mut gateway_kill: Option<venue::bybit::gateway::KillStatusView> = None;
        let (venue_name, wire_id_form, gateway_block): (String, WireIdForm, Option<venue::bybit::gateway::BlockView>) = match &cfg.venue {
            VenueConfig::Sim {
                socket,
                request_timeout_ms,
            } => {
                let socket = if socket.is_relative() {
                    cfg.data_dir.join(socket)
                } else {
                    socket.clone()
                };
                venue::simnet::spawn(
                    socket,
                    Duration::from_millis(*request_timeout_ms),
                    ports,
                    policy.max_inflight_sends as usize + 16,
                );
                ("sim".to_string(), <venue::simnet::Gateway as VenueGateway>::WIRE_ID, None)
            }
            VenueConfig::Hyperliquid(h) => {
                match venue::hyperliquid::spawn(
                    h.clone(),
                    cfg.core.instruments.clone(),
                    ports,
                    policy.max_inflight_sends as usize + 16,
                ) {
                    Ok(name) => (name, <venue::hyperliquid::Gateway as VenueGateway>::WIRE_ID, None),
                    Err(e) => {
                        error!("cannot start the Hyperliquid gateway", "error": e);
                        return start_failed("hyperliquid_gateway");
                    }
                }
            }
            VenueConfig::Bybit(b) => {
                let symbols = cfg
                    .core
                    .instruments
                    .iter()
                    .map(|(id, spec)| venue::bybit::checks::SymbolSpec {
                        name: id.to_string(),
                        px_tick: spec.px_tick,
                        qty_step: spec.qty_step,
                    })
                    .collect();
                // The kill loop's notices (C-KILL): through the direct
                // alerts, or a `warn` line each without `[alert]`.
                let notice: venue::bybit::gateway::NoticeSink = match &alerts {
                    Some(a) => {
                        let a = a.clone();
                        Arc::new(move |text: &str| a.notice(text))
                    }
                    None => Arc::new(|text: &str| warn!("kill loop notice (no [alert]: not sent)", "text": text)),
                };
                match venue::bybit::gateway::spawn(b.clone(), symbols, ports, policy.max_inflight_sends as usize + 16, notice) {
                    Ok(started) => {
                        if let Some(a) = &alerts {
                            a.set_gateway_block(started.block.clone());
                        }
                        gateway_kill = Some(started.kill);
                        (started.name, <venue::bybit::gateway::Gateway as VenueGateway>::WIRE_ID, Some(started.block))
                    }
                    Err(e) => {
                        error!("cannot start the Bybit gateway", "error": e);
                        return start_failed("bybit_gateway");
                    }
                }
            }
        };
        let pace = venue::reconciler::Pace {
            round: Duration::from_millis(cfg.runtime.reconcile_ms),
            sweep: Duration::from_millis(cfg.runtime.sweep_ms),
            margin_poll: Duration::from_millis(cfg.runtime.margin_poll_ms),
            venue_setting_poll: Duration::from_millis(cfg.runtime.venue_setting_poll_ms),
        };
        tokio::spawn(venue::reconciler::run(published.clone(), query_tx, pace));
        let tick_queues = queues.clone();
        let tick_ms = cfg.runtime.tick_ms.max(10);
        tokio::spawn(async move {
            let mut t = tokio::time::interval(Duration::from_millis(tick_ms));
            loop {
                t.tick().await;
                if tick_queues.is_closed() {
                    return;
                }
                tick_queues.push_tick();
            }
        });
        let (storage_queues, storage_dir, low_bytes) = (queues.clone(), cfg.data_dir.clone(), cfg.runtime.storage_low_bytes);
        tokio::spawn(async move {
            let mut t = tokio::time::interval(Duration::from_secs(5));
            loop {
                t.tick().await;
                if storage_queues.is_closed() {
                    return;
                }
                match rustix::fs::statvfs(&storage_dir) {
                    Ok(s) => {
                        let free_bytes = s.f_bavail.saturating_mul(s.f_frsize);
                        let input = Input::Storage {
                            low: free_bytes < low_bytes,
                            free_bytes,
                        };
                        let _ = storage_queues.try_push(Lane::Control, CoreMsg::new(input));
                    }
                    Err(e) => warn!("statvfs failed", "error": e.to_string()),
                }
            }
        });

        // Step 5: sockets.
        let node_info = NodeInfo {
            version: crate::VERSION.to_string(),
            source_digest: crate::SOURCE_DIGEST.to_string(),
            pid: std::process::id(),
            started_at: wall_us(),
            venue: venue_name,
            wire_id_form,
            data_dir: cfg.data_dir.display().to_string(),
            recovery: serde_json::to_value(&report).unwrap_or_default(),
            kill_marker: marker_info,
        };
        // Strategy liveness (C-LIVE): armed at the start only for listed
        // strategies with open plain orders in the durable state.
        let mut liveness = crate::liveness::Liveness::new(&cfg.liveness_ms());
        if cfg.liveness.is_none() {
            warn!("no [liveness] section: no strategy has liveness, and nothing cancels the orders of a strategy that dies");
        }
        let unlisted = cfg.strategies_without_liveness();
        if cfg.liveness.is_some() && !unlisted.is_empty() {
            warn!("enabled strategies without liveness: nothing cancels their orders when they die", "strategies": unlisted.join(", "));
        }
        {
            let p = published.read();
            let core = &p.core;
            let has_open_plain_orders = |s: &StrategyId| {
                core.open
                    .iter()
                    .filter_map(|id| core.order(*id))
                    .any(|o| o.strategy == *s && !o.reduce_only)
            };
            liveness.start(std::time::Instant::now(), has_open_plain_orders, |s| core.strategy_link.contains_key(s));
        }
        let own_uid = rustix::process::getuid().as_raw();
        let ctx = Arc::new(ApiCtx::new(
            cfg.api.clone(),
            cfg.limits.clone(),
            shared.clone(),
            node_info,
            own_uid,
            gateway_block,
            gateway_kill,
            alerts.clone(),
            liveness,
        ));
        let (live_ctx, live_queues) = (ctx.clone(), queues.clone());
        tokio::spawn(async move {
            let mut t = tokio::time::interval(Duration::from_millis(crate::liveness::TICK_MS));
            loop {
                t.tick().await;
                if live_queues.is_closed() {
                    return;
                }
                live_ctx.liveness_tick();
            }
        });
        let trading_mode = if cfg.api.allowed_uids.is_empty() { 0o600 } else { 0o666 };
        let trading = match server::bind(&cfg.socket_path(), trading_mode) {
            Ok(l) => l,
            Err(e) => {
                error!("cannot open the trading socket", "error": e);
                return start_failed("trading_socket");
            }
        };
        let admin = match server::bind(&cfg.admin_socket_path(), 0o600) {
            Ok(l) => l,
            Err(e) => {
                error!("cannot open the admin socket", "error": e);
                return start_failed("admin_socket");
            }
        };
        if !cfg.http.listen.is_empty() {
            // Validated as a loopback address when the configuration was loaded.
            if let Ok(addr) = cfg.http.listen.parse::<std::net::SocketAddr>() {
                let static_dir = cfg.http.static_dir.clone();
                let http_ctx = ctx.clone();
                tokio::spawn(async move {
                    if let Err(e) = crate::http::serve(addr, static_dir, Some(http_ctx)).await {
                        error!("dashboard stopped", "error": e);
                    }
                });
                info!("dashboard listening", "url": format!("http://{addr}/"));
            }
        }
        let trading_task = tokio::spawn(server::serve(trading, false, ctx.clone()));
        let admin_task = tokio::spawn(server::serve(admin, true, ctx.clone()));
        info!("ready", "socket": cfg.socket_path().display().to_string(), "admin_socket": cfg.admin_socket_path().display().to_string(), "boot": boot);

        let mut term = match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(s) => s,
            Err(e) => {
                error!("cannot install the signal handler", "error": e.to_string());
                return start_failed("signal_handler");
            }
        };
        let code = tokio::select! {
            _ = tokio::signal::ctrl_c() => 0,
            _ = term.recv() => 0,
            _ = fatal.wait() => 2,
        };
        trading_task.abort();
        admin_task.abort();
        let _ = std::fs::remove_file(cfg.socket_path());
        let _ = std::fs::remove_file(cfg.admin_socket_path());
        code
    });

    // Drain: no more input is accepted; what is queued is still processed
    // and made durable.
    queues.close();
    let _ = pipe.core_thread.join();
    if let Some(h) = pipe.commit_thread {
        let _ = h.join();
    }
    let _ = pipe.snapshot_thread.join();
    // The drain itself can fail (a write error while the last queued inputs
    // are committed), and so can the start (the Boot or Config flush, seen
    // as a missing reply, code 3). Any fatal stop exits with 2.
    let code = if fatal.is_tripped() { 2 } else { code };

    if code == 0 {
        // A final snapshot of the durable state shortens the next recovery.
        let (seq, core) = {
            let p = published.read();
            (p.durable_seq, p.core.clone())
        };
        let last_at = published
            .read()
            .recent
            .back()
            .map(|e| e.record.at)
            .unwrap_or(report.last_at);
        let digest = core.digest();
        match serde_json::to_vec(&SnapshotState { core, last_at }) {
            Ok(payload) => {
                let info = SnapshotInfo {
                    format: 0,
                    state_schema: STATE_SCHEMA,
                    seq,
                    len: 0,
                    crc32: 0,
                    created_at_us: wall_us(),
                    writer: writer_name(),
                    state_digest: digest,
                };
                match oms_journal::write_snapshot(&cfg.data_dir, info, &payload) {
                    Ok(_) => info!("final snapshot written", "seq": seq),
                    Err(e) => warn!("final snapshot failed", "error": e.to_string()),
                }
            }
            Err(e) => warn!("final snapshot failed", "error": e.to_string()),
        }
        info!("stopped cleanly", "durable_seq": seq);
    } else {
        error!("stopped after a fatal condition", "reason": fatal.reason(), "code": code);
    }
    // The stop notice, the fatal message or the start-failure notice, then
    // up to exit_flush_ms for delivery. `/api/status` still answers
    // meanwhile, with `fatal` set.
    if let Some(a) = &alerts {
        a.finish(code);
    }
    runtime.shutdown_timeout(Duration::from_millis(200));
    code
}

/// Bytes of the marker's content that go into the boot record. Judgment,
/// no model: enough for the watcher's reason, small in the journal.
const MARKER_TEXT_BYTES: u64 = 512;

/// What is at the kill marker path, as `Boot.kill_marker` (C-HOLD): `None`
/// only when nothing is there. Anything there, of any file type, latches;
/// so does a path that cannot be examined, which cannot prove that the
/// watcher did not kill.
pub fn read_kill_marker(path: &Path) -> Option<String> {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
        Err(e) => return Some(format!("marker unreadable: {:?}", e.kind())),
    };
    if !meta.file_type().is_file() {
        let kind = if meta.file_type().is_symlink() {
            "a symlink"
        } else if meta.is_dir() {
            "a directory"
        } else {
            "not a regular file"
        };
        return Some(format!("marker: ({kind} at the marker path)"));
    }
    let mut bytes = Vec::new();
    let read = std::fs::File::open(path).and_then(|f| f.take(MARKER_TEXT_BYTES).read_to_end(&mut bytes));
    if let Err(e) = read {
        return Some(format!("marker: (content unreadable: {:?})", e.kind()));
    }
    let text: String = String::from_utf8_lossy(&bytes)
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    Some(format!("marker: {}", text.trim()).trim_end().to_string())
}

/// Starts the direct alerts of `[alert]`; `None` without the section.
fn start_alerts(cfg: &NodeConfig, published: &Arc<Published>, fatal: &Arc<Fatal>) -> Result<Option<Arc<Alerts>>, String> {
    let Some(a) = &cfg.alert else {
        warn!("no [alert] section: direct [OMS P1] alerts are off");
        return Ok(None);
    };
    let Some(tg) = a.loaded.clone() else {
        return Err("[alert] telegram was not loaded".into());
    };
    let mut ac = alert_direct::AlertConfig::new(tg, alerts::PREFIX, &cfg.venue_label());
    if let Some(base) = &a.test_api {
        warn!("the direct alerts use the test endpoint", "url": base.clone());
        ac.routes = vec![alert_direct::Route::direct(base)?];
    }
    ac.repeat_ms = a.repeat_ms as i64;
    ac.second_after_ms = a.second_after_ms as i64;
    let settings = alerts::Settings {
        grace_ms: a.grace_ms as i64,
        eval_ms: a.eval_ms as i64,
        poll: Duration::from_millis(100),
        exit_flush: Duration::from_millis(a.exit_flush_ms),
    };
    // second_channel is "none" until D3 names a service (validated).
    let started = Alerts::start(ac, Arc::new(alert_direct::SystemClock), published.clone(), fatal.clone(), settings, None)?;
    info!("direct alerts on", "venue": cfg.venue_label(), "grace_ms": a.grace_ms, "repeat_ms": a.repeat_ms);
    Ok(Some(Arc::new(started)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn dir(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("marker-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// C-HOLD. Nothing at the path: no marker. A file: its first 512
    /// bytes, control characters replaced, after `marker:`. Any other file
    /// type latches too.
    #[test]
    fn the_kill_marker_is_whatever_is_at_its_path() {
        let d = dir("kinds");
        let m = d.join("kill.marker");
        assert_eq!(read_kill_marker(&m), None);

        std::fs::write(&m, "W5 killed\nat 12:00\t(OMS silent)\r\n").unwrap();
        assert_eq!(read_kill_marker(&m).as_deref(), Some("marker: W5 killed at 12:00 (OMS silent)"));
        std::fs::write(&m, "").unwrap();
        assert_eq!(read_kill_marker(&m).as_deref(), Some("marker:"));
        std::fs::write(&m, "x".repeat(2000)).unwrap();
        let text = read_kill_marker(&m).unwrap();
        assert_eq!(text, format!("marker: {}", "x".repeat(512)));
        // Not UTF-8: read lossy.
        std::fs::write(&m, [b'k', 0xff, b'!']).unwrap();
        assert_eq!(read_kill_marker(&m).as_deref(), Some("marker: k\u{fffd}!"));

        std::fs::remove_file(&m).unwrap();
        std::fs::create_dir(&m).unwrap();
        assert_eq!(read_kill_marker(&m).as_deref(), Some("marker: (a directory at the marker path)"));
        std::fs::remove_dir(&m).unwrap();
        // A symlink, even one that points nowhere.
        std::os::unix::fs::symlink(d.join("nowhere"), &m).unwrap();
        assert_eq!(read_kill_marker(&m).as_deref(), Some("marker: (a symlink at the marker path)"));
        let _ = std::fs::remove_dir_all(&d);
    }

    /// C-HOLD. A path the node cannot examine latches: it cannot prove that
    /// the watcher did not kill. (A test run as root sees through the
    /// permission and is skipped.)
    #[test]
    fn a_marker_that_cannot_be_examined_latches() {
        if rustix::process::getuid().is_root() {
            return;
        }
        let d = dir("unreadable");
        let shut = d.join("shut");
        std::fs::create_dir(&shut).unwrap();
        std::fs::set_permissions(&shut, std::fs::Permissions::from_mode(0o000)).unwrap();
        let got = read_kill_marker(&shut.join("kill.marker"));
        std::fs::set_permissions(&shut, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(got.as_deref(), Some("marker unreadable: PermissionDenied"));
        // Not a directory on the way is not "nothing there" either.
        std::fs::write(d.join("file"), "x").unwrap();
        assert_eq!(read_kill_marker(&d.join("file/kill.marker")).as_deref(), Some("marker unreadable: NotADirectory"));
        let _ = std::fs::remove_dir_all(&d);
    }
}
