//! The runtime (C-WATCH part 7, U19a7): the single-threaded tokio loop
//! that carries out what the sans-IO parts ask. Boot: credentials → the
//! metrics push timer (from here on) → the start checks ([`Start`]) → the
//! wait for `/api/status` → the replay of the journal from record 1 to its
//! cap → the engine (W1, W2 and W4). The IP recheck ([`IpCheck`]) runs from
//! the end of the start on, also while the watcher waits for the OMS and
//! replays. Every timer reads the injected clock; the loop wakes every
//! [`TICK_MS`] of real time (and at every answer) and asks each part what
//! is due. Each request runs on its own task and its answer comes back to
//! the loop. Shutdown: no new round, the requests in flight get at most
//! `request_timeout_ms`, one last metrics push, the monitor connection
//! closes, exit 0. Nothing is persisted.

use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use tokio::sync::mpsc;
use tokio::task::JoinSet;

use crate::clock::Clock;
use crate::config::Config;
use crate::engine::{Action, Check, Engine};
use crate::journal::{MAX_RECORDS, Tail};
use crate::log::Log;
use crate::metrics::{Exporter, Metrics};
use crate::oms::{Monitor, Status, admin_send};
use crate::rest::{Client, ClientConfig, RestError};
use crate::start::{self, IpCheck, Start, Step};
use crate::state::JournalState;
use crate::w2::W2;
use crate::w4::W4;

/// The loop asks every part what is due at least this often (real time,
/// 판단(모델 아님)).
pub const TICK_MS: u64 = 100;
/// The wait for `/api/status` at boot asks again this long after a failed
/// answer (판단(모델 아님)).
pub const STATUS_RETRY_MS: u64 = 1_000;
/// ... and writes a line at most this often (판단(모델 아님)).
pub const STATUS_LOG_MS: u64 = 10_000;

/// Where the watcher sends: the binary takes both from the config (the
/// REST base from `network` only, no override); tests give loopback fakes.
#[derive(Debug, Clone)]
pub struct Endpoints {
    pub rest_base: String,
    pub ip_echo_url: String,
}

impl Endpoints {
    pub fn of(cfg: &Config) -> Endpoints {
        Endpoints {
            rest_base: cfg.network.rest_base().to_string(),
            ip_echo_url: cfg.ip_echo_url.clone(),
        }
    }
}

/// An answer coming back to the loop.
enum Msg {
    Start(Result<Value, RestError>),
    Recheck(Result<Value, RestError>),
    BootStatus(Result<Value, String>),
    Venue(u64, Result<Value, RestError>),
    Status(u64, Result<Value, String>),
    Admin(Check, u64, Result<String, String>),
    Pushed,
}

enum Phase {
    Start(Start),
    /// Waiting for a usable `/api/status` at boot.
    Wait {
        next_try: u64,
        asking: bool,
        last_log: Option<u64>,
    },
    /// The boot replay up to `cap`.
    Replay {
        tail: Tail,
        state: JournalState,
        cap: u64,
        not_before: u64,
        failing: bool,
    },
    Run(Engine),
}

/// The cap of a boot status answer, if it is usable.
fn boot_cap(v: &Value) -> Result<u64, String> {
    let cap = v
        .pointer("/journal/durable_seq")
        .and_then(Value::as_u64)
        .ok_or("journal.durable_seq is not a u64")?;
    v.pointer("/core/instance_id")
        .and_then(Value::as_str)
        .filter(|s| s.len() == 16 && s.bytes().all(|b| b.is_ascii_hexdigit()))
        .ok_or("core.instance_id is not 16 hex digits")?;
    Ok(cap)
}

struct Runtime {
    cfg: Config,
    echo_url: String,
    clock: Arc<dyn Clock>,
    log: Log,
    metrics: Arc<Metrics>,
    client: Arc<Client>,
    status: Arc<Status>,
    exporter: Option<Arc<Exporter>>,
    boot_ms: u64,
    tx: mpsc::UnboundedSender<Msg>,
    tasks: JoinSet<()>,
    beats: mpsc::Sender<Vec<(Check, Option<u64>)>>,
    phase: Phase,
    ip: Option<IpCheck>,
    next_push: u64,
    pushing: bool,
    /// Set by a start refusal: the exit code 2.
    refused: bool,
}

/// Runs the watcher until `shutdown` resolves; the exit code (0, or 2 for
/// a start refusal, with one error line).
pub async fn run(
    cfg: Config,
    ep: Endpoints,
    clock: Arc<dyn Clock>,
    log: Log,
    shutdown: impl Future<Output = ()>,
) -> u8 {
    let boot_ms = clock.now_ms();
    let creds = match crate::creds::load(&cfg.credentials_file, &cfg.data_dir) {
        Ok(c) => c,
        Err(e) => {
            log(&format!("error: {e}"));
            return 2;
        }
    };
    let metrics = Arc::new(Metrics::new(&cfg.host, &cfg.name));
    let timeout = cfg.http.request_timeout_ms;
    let exporter = match &cfg.telemetry.vm_url {
        None => None,
        Some(url) => match Exporter::new(url, timeout, log.clone()) {
            Ok(e) => Some(Arc::new(e)),
            Err(e) => {
                log(&format!("error: the metrics exporter: {e}"));
                return 2;
            }
        },
    };
    let client_cfg = ClientConfig {
        base: ep.rest_base.clone(),
        ..cfg.client_config()
    };
    let parts = Client::new(
        &client_cfg,
        creds,
        clock.clone(),
        metrics.clone(),
        log.clone(),
    )
    .and_then(|c| Ok((c, Status::new(&cfg.status_url, timeout)?)));
    let (client, status) = match parts {
        Ok(p) => p,
        Err(e) => {
            log(&format!("error: the HTTP clients: {e}"));
            return 2;
        }
    };
    let (tx, mut rx) = mpsc::unbounded_channel();
    // At most one heartbeat waits: a slow monitor link drops beats, it
    // does not pile them up.
    let (beats, beats_rx) = mpsc::channel(1);
    let monitor = Monitor::new(
        &cfg.monitor_socket,
        &cfg.name,
        timeout,
        metrics.clone(),
        log.clone(),
    );
    let heartbeat = tokio::spawn(heartbeats(monitor, beats_rx, clock.clone()));
    let start = Start::new(&cfg, metrics.clone(), log.clone(), boot_ms);
    let mut rt = Runtime {
        echo_url: ep.ip_echo_url,
        clock,
        log,
        metrics,
        client: Arc::new(client),
        status: Arc::new(status),
        exporter,
        boot_ms,
        tx,
        tasks: JoinSet::new(),
        beats,
        phase: Phase::Start(start),
        ip: None,
        next_push: boot_ms,
        pushing: false,
        refused: false,
        cfg,
    };
    let mut shutdown = std::pin::pin!(shutdown);
    loop {
        let wait = if rt.busy() { 0 } else { TICK_MS };
        tokio::select! {
            biased;
            _ = &mut shutdown => break,
            Some(m) = rx.recv() => rt.on(m),
            _ = tokio::time::sleep(Duration::from_millis(wait)) => {}
        }
        rt.tick();
        while rt.tasks.try_join_next().is_some() {}
        if rt.refused {
            rt.tasks.abort_all();
            return 2;
        }
    }
    rt.stop(heartbeat).await;
    0
}

/// The heartbeat task: one beat at a time over the persistent connection;
/// it ends (and the connection closes) when the loop drops its sender.
async fn heartbeats(
    mut monitor: Monitor,
    mut rx: mpsc::Receiver<Vec<(Check, Option<u64>)>>,
    clock: Arc<dyn Clock>,
) {
    while let Some(checks) = rx.recv().await {
        let _ = monitor.beat(clock.now_ms(), &checks).await;
    }
    monitor.close();
}

impl Runtime {
    /// The boot replay reads on without waiting for a tick.
    fn busy(&self) -> bool {
        matches!(self.phase, Phase::Replay { not_before, .. } if self.clock.now_ms() >= not_before)
    }

    fn spawn(&mut self, f: impl Future<Output = Msg> + Send + 'static) {
        let tx = self.tx.clone();
        self.tasks.spawn(async move {
            let _ = tx.send(f.await);
        });
    }

    /// An answer.
    fn on(&mut self, m: Msg) {
        let now = self.clock.now_ms();
        match m {
            Msg::Start(a) => {
                let Phase::Start(s) = &mut self.phase else {
                    return;
                };
                let step = s.on_answer(now, a);
                self.after_start(now, step);
            }
            Msg::Recheck(a) => {
                if let Some(ip) = &mut self.ip {
                    let step = ip.on_answer(now, a);
                    start::apply(&self.client, &step);
                }
            }
            Msg::BootStatus(a) => self.on_boot_status(now, a),
            Msg::Venue(id, a) => {
                if let Phase::Run(e) = &mut self.phase {
                    let out = e.on_venue(now, id, a);
                    self.exec(out);
                }
            }
            Msg::Status(id, a) => {
                if let Phase::Run(e) = &mut self.phase {
                    let out = e.on_status(now, id, a);
                    self.exec(out);
                }
            }
            Msg::Admin(check, send_id, a) => {
                if let Phase::Run(e) = &mut self.phase {
                    let out = e.on_admin(now, check, send_id, a);
                    self.exec(out);
                }
            }
            Msg::Pushed => self.pushing = false,
        }
    }

    fn after_start(&mut self, now: u64, step: Step) {
        let Phase::Start(s) = &self.phase else {
            return;
        };
        match step {
            Step::Refuse { reason } => {
                (self.log)(&format!("error: start refused: {reason}"));
                self.refused = true;
            }
            Step::Done | Step::StayUpNoRequests => {
                self.ip = s.ip_check();
                self.phase = Phase::Wait {
                    next_try: now,
                    asking: false,
                    last_log: None,
                };
            }
            Step::Wait { .. } | Step::Next | Step::StopRequests { .. } => {}
        }
    }

    fn on_boot_status(&mut self, now: u64, a: Result<Value, String>) {
        let Phase::Wait { last_log, .. } = &self.phase else {
            return;
        };
        match a.and_then(|v| boot_cap(&v)) {
            Ok(cap) => {
                (self.log)(&format!(
                    "the OMS answers /api/status: replaying the journal up to record {cap}"
                ));
                self.phase = Phase::Replay {
                    tail: Tail::open(&self.cfg.data_dir),
                    state: JournalState::default(),
                    cap,
                    not_before: now,
                    failing: false,
                };
            }
            Err(why) => {
                let log_now = last_log.is_none_or(|t| now.saturating_sub(t) >= STATUS_LOG_MS);
                let last_log = if log_now {
                    (self.log)(&format!(
                        "warn: waiting for /api/status: {why}; asked every {} s",
                        STATUS_RETRY_MS / 1000
                    ));
                    Some(now)
                } else {
                    *last_log
                };
                self.phase = Phase::Wait {
                    next_try: now.saturating_add(STATUS_RETRY_MS),
                    asking: false,
                    last_log,
                };
            }
        }
    }

    /// What is due now.
    fn tick(&mut self) {
        let now = self.clock.now_ms();
        self.push_if_due(now);
        if let Some(req) = self.ip.as_mut().and_then(|ip| ip.poll(now)) {
            let (c, url) = (self.client.clone(), self.echo_url.clone());
            self.spawn(async move { Msg::Recheck(start::send(&c, &url, &req).await) });
        }
        match &mut self.phase {
            Phase::Start(s) => {
                if let Some(req) = s.poll(now) {
                    let (c, url) = (self.client.clone(), self.echo_url.clone());
                    self.spawn(async move { Msg::Start(start::send(&c, &url, &req).await) });
                }
            }
            Phase::Wait {
                next_try, asking, ..
            } => {
                if !*asking && now >= *next_try {
                    *asking = true;
                    let s = self.status.clone();
                    self.spawn(async move { Msg::BootStatus(s.get().await) });
                }
            }
            Phase::Replay { .. } => self.replay(now),
            Phase::Run(e) => {
                let out = e.poll(now);
                self.exec(out);
                return;
            }
        }
        // Before the engine runs, the ages count from the config load.
        let age = now.saturating_sub(self.boot_ms) as f64 / 1000.0;
        for c in [Check::W1, Check::W2, Check::W4] {
            self.metrics
                .set("watcher_check_age_s", &[("check", c.name())], age);
        }
    }

    /// One read of the boot replay; the engine starts at the cap.
    fn replay(&mut self, now: u64) {
        let Phase::Replay {
            tail,
            state,
            cap,
            not_before,
            failing,
        } = &mut self.phase
        else {
            return;
        };
        if now < *not_before {
            return;
        }
        let res = state.read_from(tail, *cap, MAX_RECORDS);
        self.metrics
            .set("watcher_journal_seq", &[], state.last_seq as f64);
        let at_cap = state.last_seq >= *cap;
        match res {
            Ok(_) if at_cap => {}
            Ok(n) => {
                if n == 0 {
                    if !*failing {
                        (self.log)(&format!(
                            "warn: boot replay: the journal ends at record {}, below the cap {cap}; read again in 1 s",
                            state.last_seq
                        ));
                    }
                    *failing = true;
                    *not_before = now.saturating_add(STATUS_RETRY_MS);
                }
                return;
            }
            Err(e) if e.is_retried() => {
                if !*failing {
                    (self.log)(&format!("error: boot replay: {e}; read again in 1 s"));
                }
                *failing = true;
                self.metrics.set("watcher_journal_error", &[], 1.0);
                *not_before = now.saturating_add(STATUS_RETRY_MS);
                return;
            }
            // A latched reader: the engine fails every round and says so.
            Err(e) => (self.log)(&format!("error: boot replay: {e}")),
        }
        let Phase::Replay { tail, state, .. } = std::mem::replace(
            &mut self.phase,
            Phase::Wait {
                next_try: u64::MAX,
                asking: true,
                last_log: None,
            },
        ) else {
            return;
        };
        (self.log)(&format!(
            "boot replay done at record {}; the checks start",
            state.last_seq
        ));
        let mut e = Engine::new(
            &self.cfg,
            tail,
            state,
            self.metrics.clone(),
            self.log.clone(),
            self.boot_ms,
            now,
        );
        e.add_check(Box::new(W2::default()), self.cfg.intervals.w2_ms);
        let w4 = W4::new(&self.cfg.rules, self.log.clone());
        e.add_check(Box::new(w4), self.cfg.intervals.w4_ms);
        self.phase = Phase::Run(e);
    }

    /// Carries out the engine's actions.
    fn exec(&mut self, actions: Vec<Action>) {
        for a in actions {
            match a {
                Action::Venue {
                    req_id,
                    path,
                    params,
                } => {
                    let c = self.client.clone();
                    self.spawn(async move { Msg::Venue(req_id, c.get(path, &params).await) });
                }
                Action::Status { req_id } => {
                    let s = self.status.clone();
                    self.spawn(async move { Msg::Status(req_id, s.get().await) });
                }
                Action::Mismatch {
                    check,
                    send_id,
                    detail,
                } => {
                    let path = self.cfg.admin_socket.clone();
                    let name = self.cfg.name.clone();
                    let timeout = self.cfg.http.request_timeout_ms;
                    self.spawn(async move {
                        let r = admin_send(&path, &name, check, &detail, timeout).await;
                        Msg::Admin(check, send_id, r)
                    });
                }
                Action::Heartbeat { checks } => {
                    if self.beats.try_send(checks).is_err() {
                        (self.log)(
                            "debug: a heartbeat is dropped: the previous one is still being sent",
                        );
                    }
                }
            }
        }
    }

    fn push_if_due(&mut self, now: u64) {
        let Some(x) = &self.exporter else {
            return;
        };
        if self.pushing || now < self.next_push {
            return;
        }
        self.pushing = true;
        self.next_push = now.saturating_add(self.cfg.intervals.export_ms);
        let (x, m) = (x.clone(), self.metrics.clone());
        self.spawn(async move {
            let _ = x.push(&m, now).await;
            Msg::Pushed
        });
    }

    /// Shutdown: the requests in flight get at most `request_timeout_ms`,
    /// one last push, the monitor connection closes.
    async fn stop(mut self, heartbeat: tokio::task::JoinHandle<()>) {
        (self.log)("shutdown: no new round; finishing the requests in flight");
        let timeout = Duration::from_millis(self.cfg.http.request_timeout_ms);
        let mut tasks = std::mem::take(&mut self.tasks);
        let _ = tokio::time::timeout(timeout, async {
            while tasks.join_next().await.is_some() {}
        })
        .await;
        tasks.abort_all();
        if let Some(x) = &self.exporter {
            let _ = x.push(&self.metrics, self.clock.now_ms()).await;
        }
        drop(self.beats);
        if tokio::time::timeout(timeout, heartbeat).await.is_err() {
            (self.log)("warn: shutdown: the heartbeat in flight did not end in time");
        }
        (self.log)("shutdown: done");
    }
}
