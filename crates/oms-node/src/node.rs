//! Startup, wiring and shutdown of the OMS process.
//!
//! Startup order, each step finishing before the next begins:
//!
//! 1. Recover the state from snapshot and journal (no sends are possible).
//! 2. Start the core thread and the commit stage.
//! 3. Submit `Boot`, then the configuration from the file. Wait until both
//!    are durable. A configuration the core refuses stops the process.
//! 4. Start the venue gateway, the reconciler, the timer and the storage
//!    monitor. The link is down until the gateway reports it up, and new
//!    orders are held until reconciliation has confirmed the state.
//! 5. Open the local sockets.
//!
//! Shutdown on SIGINT/SIGTERM: close the sockets, close the input queues,
//! let the core and the commit stage drain, write a final snapshot, exit 0.
//! Open orders are left at the venue. A fatal condition (journal failure,
//! core fault) releases nothing further and exits with status 2, also when
//! it happens during the drain. A panic aborts the process (SIGABRT).

use crate::config::{NodeConfig, VenueConfig};
use crate::pipeline::{self, PipelineConfig, STATE_SCHEMA, Shared, SnapshotState};
use crate::published::Published;
use crate::queues::{CoreMsg, InputQueues, Lane};
use crate::server::{self, ApiCtx};
use crate::status::NodeInfo;
use crate::util::{Fatal, wall_us};
use crate::venue::{self, GatewayPorts, VenueInputTx};
use crate::{error, info, recovery, warn};
use oms_core::*;
use oms_journal::SnapshotInfo;
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

    let code = runtime.block_on(async {
        // Step 3: boot and configuration, in that order, both durable.
        if queues
            .try_push(Lane::Control, CoreMsg::new(Input::Boot { binary: writer_name() }))
            .is_err()
        {
            return 3;
        }
        let (msg, rx) = CoreMsg::with_reply(Input::Config(Box::new(cfg.core.clone())));
        if queues.try_push(Lane::Control, msg).is_err() {
            return 3;
        }
        match rx.await {
            Ok(Reply::Config(Ok(version))) => info!("configuration active", "version": version),
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

        // Step 4: gateway, reconciler, timer, storage monitor.
        let ports = GatewayPorts {
            commands: cmd_rx,
            queries: query_rx,
            input: VenueInputTx::new(queues.clone()),
            instance_id,
            boot,
        };
        let venue_name = match &cfg.venue {
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
                "sim".to_string()
            }
            VenueConfig::Hyperliquid(h) => {
                match venue::hyperliquid::spawn(
                    h.clone(),
                    cfg.core.instruments.clone(),
                    ports,
                    policy.max_inflight_sends as usize + 16,
                ) {
                    Ok(name) => name,
                    Err(e) => {
                        error!("cannot start the Hyperliquid gateway", "error": e);
                        return 3;
                    }
                }
            }
        };
        tokio::spawn(venue::reconciler::run(
            published.clone(),
            query_tx,
            Duration::from_millis(cfg.runtime.reconcile_ms),
            Duration::from_millis(cfg.runtime.sweep_ms),
            Duration::from_millis(cfg.runtime.margin_poll_ms),
            Duration::from_millis(cfg.runtime.venue_setting_poll_ms),
        ));
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
            data_dir: cfg.data_dir.display().to_string(),
            recovery: serde_json::to_value(&report).unwrap_or_default(),
        };
        let own_uid = rustix::process::getuid().as_raw();
        let ctx = Arc::new(ApiCtx::new(
            cfg.api.clone(),
            cfg.limits.clone(),
            shared.clone(),
            node_info,
            own_uid,
        ));
        let trading_mode = if cfg.api.allowed_uids.is_empty() { 0o600 } else { 0o666 };
        let trading = match server::bind(&cfg.socket_path(), trading_mode) {
            Ok(l) => l,
            Err(e) => {
                error!("cannot open the trading socket", "error": e);
                return 3;
            }
        };
        let admin = match server::bind(&cfg.admin_socket_path(), 0o600) {
            Ok(l) => l,
            Err(e) => {
                error!("cannot open the admin socket", "error": e);
                return 3;
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
                return 3;
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
    // are committed). That is not a clean stop.
    let code = if code == 0 && fatal.is_tripped() { 2 } else { code };

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
    runtime.shutdown_timeout(Duration::from_millis(200));
    code
}
