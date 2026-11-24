//! The runtime (C-WATCH part 7, U19a7): the single-threaded tokio loop
//! that carries out what the sans-IO parts ask. Boot: credentials and the
//! token file → the metrics push timer and the alert sender (from here on;
//! U19b1c) → the start checks ([`Start`]) → the
//! wait for `/api/status` → the replay of the journal from record 1 to its
//! cap → the engine (W1, W2, W3 and W4; W2 reads W4's execution times). The IP recheck ([`IpCheck`]) runs from
//! the end of the start on, also while the watcher waits for the OMS and
//! replays. Every timer reads the injected clock; the loop wakes every
//! [`TICK_MS`] of real time (and at every answer) and asks each part what
//! is due. Each request runs on its own task and its answer comes back to
//! the loop. Every tick also drives the P1 conditions ([`Alerts`], C-WATCH
//! part 10): the key stop, the public IP and the mismatch confirmations; a
//! start refusal sends one P1 and flushes the sender before exit 2.
//! Shutdown: no new round, the requests in flight get at most
//! `request_timeout_ms`, one last metrics push, the monitor connection
//! closes, the sender flushes for at most [`FINISH_MS`], exit 0. Nothing is
//! persisted.

use std::collections::VecDeque;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use tokio::sync::mpsc;
use tokio::task::{AbortHandle, JoinSet};

use crate::alert::{self, Alerts, FINISH_MS, Route, Task, kst};
use crate::clock::{Clock, Steady};
use crate::config::{Config, resolved};
use crate::engine::{Action, Check, Engine, Logic};
use crate::journal::{MAX_RECORDS, Tail};
use crate::kill::{self, AbortWhy, Inflight, Kill, MarkerOutcome, Notice, Positions, Setup, Stage};
use crate::log::Log;
use crate::metrics::{Exporter, Metrics};
use crate::oms::{Monitor, Status, admin_kill, admin_send};
use crate::rest::{Client, ClientConfig, RestError};
use crate::start::{self, IpCheck, PublicIp, Start, Step};
use crate::state::JournalState;
use crate::stop::{self, Stopper};
use crate::w2::W2;
use crate::w3::W3;
use crate::w4::W4;
use crate::w5::{self, Fire, Timing, W5};

/// The loop asks every part what is due at least this often (real time,
/// 판단(모델 아님)).
pub const TICK_MS: u64 = 100;
/// The wait for `/api/status` at boot asks again this long after a failed
/// answer (판단(모델 아님)).
pub const STATUS_RETRY_MS: u64 = 1_000;
/// ... and writes a line at most this often (판단(모델 아님)).
pub const STATUS_LOG_MS: u64 = 10_000;
/// After the message of a turn the loop takes at most this many more that
/// already wait before it ticks (U19b3c, 판단(모델 아님)): a probe answer
/// in the channel reaches W5 before `fire`.
pub const QUEUED_MAX: usize = 1000;
/// At most this many kill notices wait for their position read; beyond
/// it the oldest waiting progress notice is dropped (판단(모델 아님)).
pub const NOTICES_MAX: usize = 16;
/// `main.rs` ends the tokio runtime with this `shutdown_timeout`
/// (판단(모델 아님), U19b3d): a dropped runtime would wait for every
/// `spawn_blocking` task, and a marker write hung on the disk must not hold
/// the exit.
pub const SHUTDOWN_MS: u64 = 1000;
/// Why the kill notices still waiting at the end are sent without their
/// position read (U19b3d).
pub const ENDING: &str = "워처 종료 중";

/// Where the watcher sends: the binary takes them from the config (the
/// REST base from `network` only), the two Telegram routes from
/// [`Route::telegram`] and the stopper's program [`stop::SYSTEMCTL`], with
/// no override; tests give loopback fakes and a script. The sender uses
/// `telegram` when the config has `[telegram]` (U19b1c); a kill runs
/// `systemctl` (U19b3c).
#[derive(Debug, Clone)]
pub struct Endpoints {
    pub rest_base: String,
    pub ip_echo_url: String,
    pub telegram: Vec<Route>,
    pub systemctl: PathBuf,
}

impl Endpoints {
    pub fn of(cfg: &Config) -> Endpoints {
        Endpoints {
            rest_base: cfg.network.rest_base().to_string(),
            ip_echo_url: cfg.ip_echo_url.clone(),
            telegram: Route::telegram(),
            systemctl: PathBuf::from(stop::SYSTEMCTL),
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
    /// W5's probe of `/api/status`.
    Probe(u64, Result<Value, String>),
    /// K1's outcome, of kill `kill_no`.
    Marker(u64, MarkerOutcome),
    /// The reply to halt attempt `id` of kill `kill_no`.
    Halt(u64, u64, Result<String, String>),
    /// The position read of notice `seq`.
    Positions(u64, Positions),
    /// The reply to owed halt `id` (W5's after a kill, or the one before
    /// a marker-path refusal; U19b3d).
    Owed(u64, Result<String, String>),
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

/// The checks the engine runs beside W1, each with its interval: W2 (it
/// reads W4's newest agreeing `execTime` per symbol), W3 and W4.
pub fn checks(cfg: &Config, log: &Log) -> Vec<(Box<dyn Logic>, u64)> {
    let w4 = W4::new(&cfg.rules, log.clone());
    let w2 = W2::with_exec_times(w4.exec_times());
    vec![
        (Box::new(w2), cfg.intervals.w2_ms),
        (Box::new(W3::new(&cfg.w3)), cfg.intervals.w3_ms),
        (Box::new(w4), cfg.intervals.w4_ms),
    ]
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

/// What the engine emitted from an answer while a kill runs (U19b3c,
/// review 1 S1): the mismatch P1 (`Confirmed`) is kept; every request
/// (a page, a status request, a `watcher_mismatch` send) is dropped and
/// named for the log.
fn paused(actions: Vec<Action>) -> (Vec<Action>, Vec<&'static str>) {
    let mut keep = Vec::new();
    let mut dropped = Vec::new();
    for a in actions {
        match a {
            Action::Confirmed { .. } => keep.push(a),
            Action::Venue { path, .. } => dropped.push(path),
            Action::Status { .. } => dropped.push("/api/status"),
            Action::Mismatch { .. } => dropped.push("watcher_mismatch"),
            Action::Heartbeat { .. } => dropped.push("heartbeat"),
        }
    }
    (keep, dropped)
}

/// The marker-path boot rule (C-WATCH part 12, U19b3c): with `[kill]` a
/// status answer is usable only if the OMS reads its kill marker at the
/// watcher's path (`node.kill_marker`, compared where both point). A marker
/// the OMS never reads would not keep a killed OMS halted at its next
/// start. The OMS config is not read (independence).
fn marker_rule(v: &Value, ours: &Path) -> Result<(), String> {
    let path = oms_marker(v);
    match path {
        Some(p) if resolved(Path::new(p)) == resolved(ours) => Ok(()),
        _ => Err(format!(
            "the OMS reads its kill marker at {}; the watcher writes {}: they must be the same file",
            path.map_or("none".to_string(), |p| kill::cut(p, kill::TEXT_MAX)),
            ours.display()
        )),
    }
}

/// The path where the OMS of a status answer reads its kill marker
/// (`node.kill_marker`), `None` when it reads none.
fn oms_marker(v: &Value) -> Option<&str> {
    let m = v.pointer("/node/kill_marker");
    let configured = m.and_then(|m| m.get("configured")).and_then(Value::as_bool) == Some(true);
    m.and_then(|m| m.get("path"))
        .and_then(Value::as_str)
        .filter(|_| configured)
}

/// How the refusal P1 names the outcome of the `watcher_kill` it sent
/// first (U19b3d): the reply that latched, or `실패(<why>)` for any other
/// reply or error.
fn halt_outcome(r: &Result<String, String>) -> String {
    match r {
        Ok(reply) if crate::oms::latched(reply) => kill::cut(reply, kill::TEXT_MAX),
        Ok(why) | Err(why) => format!("실패({})", kill::cut(why, kill::TEXT_MAX)),
    }
}

/// A running kill (C-WATCH part 12, U19b3c): the sans-IO sequence, the
/// tasks of its loop's requests and of its open halt attempt. Everything
/// of it reads `clock::Steady`.
struct KillRun {
    /// Tells this kill's outcomes from a former kill's late ones.
    kill_no: u64,
    kill: Kill,
    inflight: Inflight,
    /// The task of the open halt attempt (K2 or a retry): its id.
    halt: Option<(u64, AbortHandle)>,
    /// The loop's cancel-alls already counted in the metric.
    cancel_alls: u64,
    /// Late outcomes and stray answers already logged.
    ignored: u64,
    stray: u64,
}

/// A `watcher_kill` sent outside a kill's own attempts (U19b3d): W5's
/// owed halt at the OMS's first good answer after a kill, or the one before
/// a marker-path refusal. Its task is aborted at the K2 deadline.
struct Owed {
    id: u64,
    deadline: u64,
    task: AbortHandle,
}

/// A marker-path refusal that waits for one `watcher_kill` first
/// (U19b3d): the halt is owed and the OMS answers.
struct Refusing {
    reason: String,
    wait: RefusalWait,
}

enum RefusalWait {
    /// The running kill's own attempt (K2 or a retry) that was in flight.
    Kill { kill_no: u64, id: u64, deadline: u64 },
    /// The runtime's owed halt ([`Owed`]).
    Owed,
}

/// A kill notice waiting for its position block, with the header of its
/// kill (the start notice names it).
struct Waiting {
    notice: Notice,
    header: String,
}

/// The position read of the notice being sent.
struct Reading {
    seq: u64,
    w: Waiting,
    deadline: u64,
    task: AbortHandle,
}

/// The kill notices, sent beside the loop one at a time in their order,
/// each with a position block read just before (C-WATCH part 12).
#[derive(Default)]
struct Notices {
    waiting: VecDeque<Waiting>,
    reading: Option<Reading>,
    next_seq: u64,
    dropped: u64,
}

impl Notices {
    /// Queues `w`; beyond [`NOTICES_MAX`] waiting, the oldest waiting
    /// progress notice is dropped (never a start, a final or a late halt).
    /// Returns whether one was dropped.
    fn push(&mut self, w: Waiting) -> bool {
        let drop = if self.waiting.len() >= NOTICES_MAX {
            self.waiting
                .iter()
                .position(|w| matches!(w.notice, Notice::Progress { .. }))
        } else {
            None
        };
        if let Some(i) = drop {
            self.waiting.remove(i);
            self.dropped += 1;
        }
        self.waiting.push_back(w);
        drop.is_some()
    }
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
    /// The P1 conditions (U19b1c).
    alerts: Alerts,
    /// The sender task, `None` without `[telegram]`.
    alert_task: Option<Task>,
    /// Set once the sender task ended before the shutdown.
    sender_ended: bool,
    public_ip: PublicIp,
    /// A status answer in this process passed the marker-path rule.
    marker_checked: bool,
    /// The OMS gave a good status answer in this process.
    answered: bool,
    /// The clock of W5 and the kill path: it does not step back.
    steady: Arc<dyn Clock>,
    systemctl: PathBuf,
    w5: W5,
    kill: Option<KillRun>,
    kills: u64,
    /// The stoppers of K3, polled until their client is reaped (also
    /// after their kill ended), each with its kill's `kill_no`.
    stoppers: Vec<(u64, Stopper)>,
    notices: Notices,
    /// The owed halt in flight, and the ids given so far.
    owed: Option<Owed>,
    owed_ids: u64,
    /// A marker-path refusal waiting for its `watcher_kill`; the kill is
    /// held meanwhile.
    refusing: Option<Refusing>,
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
    // Read once, before the start checks (U19b1a).
    let token = match &cfg.telegram {
        Some(t) => match crate::creds::load_token(&t.token_file, &cfg.data_dir) {
            Ok(tok) => Some(tok),
            Err(e) => {
                log(&format!("error: {e}"));
                return 2;
            }
        },
        // Testnet only: the config refuses mainnet without it.
        None => {
            log("warn: no [telegram]: every [WATCH P1] is written to this log only, not sent");
            None
        }
    };
    // Testnet only: the config refuses mainnet without it (U19b3a).
    if cfg.kill.is_none() {
        log("warn: no [kill]: W5 is off; when the OMS is silent the watcher neither stops it nor cancels its orders, and no [WATCH P1] tells it");
    }
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
    // The sender is up before the start checks, so that a key rejected at
    // step 2 is told (U19b1c).
    let (alerts, alert_task) = match token {
        Some(tok) => match alert::spawn(
            &cfg.name,
            ep.telegram.clone(),
            tok,
            clock.clone(),
            metrics.clone(),
            log.clone(),
        ) {
            Ok((h, t)) => (Alerts::telegram(h), Some(t)),
            Err(e) => {
                log(&format!("error: the alert sender: {e}"));
                return 2;
            }
        },
        None => (Alerts::log_only(log.clone(), clock.clone()), None),
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
    let public_ip = start.public_ip();
    let steady: Arc<dyn Clock> = Arc::new(Steady::new(clock.clone()));
    let w5 = W5::new(Timing::of(&cfg), log.clone());
    metrics.set("watcher_w5_armed", &[], 0.0);
    if cfg.kill.is_some() {
        for series in [
            "watcher_kill_active",
            "watcher_kills_total",
            "watcher_kill_cancel_all_total",
            "watcher_kill_halt_owed",
        ] {
            metrics.set(series, &[], 0.0);
        }
    }
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
        alerts,
        alert_task,
        sender_ended: false,
        public_ip,
        marker_checked: false,
        answered: false,
        steady,
        systemctl: ep.systemctl,
        w5,
        kill: None,
        kills: 0,
        stoppers: Vec::new(),
        notices: Notices::default(),
        owed: None,
        owed_ids: 0,
        refusing: None,
        cfg,
    };
    let mut shutdown = std::pin::pin!(shutdown);
    loop {
        let wait = if rt.busy() { 0 } else { TICK_MS };
        tokio::select! {
            biased;
            _ = &mut shutdown => break,
            Some(m) = rx.recv() => {
                rt.on(m);
                // What already waits goes before the tick (review 2 S3).
                for _ in 0..QUEUED_MAX {
                    match rx.try_recv() {
                        Ok(m) => rt.on(m),
                        Err(_) => break,
                    }
                }
            }
            _ = tokio::time::sleep(Duration::from_millis(wait)) => {}
        }
        rt.tick();
        while rt.tasks.try_join_next().is_some() {}
        if rt.refused {
            rt.tasks.abort_all();
            // The refusal P1 is queued: flush it before the exit.
            finish_alerts(rt.alert_task.take()).await;
            return 2;
        }
    }
    // A running kill is aborted first, with one notice (U19b3d).
    rt.shutdown_kill(&mut rx).await;
    // What the 60 s mismatch pace still holds is queued before the flush.
    rt.alerts.drain();
    let task = rt.alert_task.take();
    rt.stop(heartbeat).await;
    finish_alerts(task).await;
    0
}

/// The sender's last flush, bounded by [`FINISH_MS`] of real time.
async fn finish_alerts(task: Option<Task>) {
    if let Some(t) = task {
        t.finish(FINISH_MS).await;
    }
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
    /// The boot replay reads on without waiting for a tick, and so does a
    /// kill whose loop got an answer.
    fn busy(&self) -> bool {
        matches!(self.phase, Phase::Replay { not_before, .. } if self.clock.now_ms() >= not_before)
            || self.kill.as_ref().is_some_and(|k| k.inflight.queued() > 0)
    }

    fn spawn(&mut self, f: impl Future<Output = Msg> + Send + 'static) -> AbortHandle {
        let tx = self.tx.clone();
        self.tasks.spawn(async move {
            let _ = tx.send(f.await);
        })
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
                    self.exec_answer(out);
                }
            }
            Msg::Status(id, a) => {
                if let Phase::Run(e) = &mut self.phase {
                    let out = e.on_status(now, id, a);
                    self.exec_answer(out);
                }
            }
            Msg::Admin(check, send_id, a) => {
                if let Phase::Run(e) = &mut self.phase {
                    let out = e.on_admin(now, check, send_id, a);
                    self.exec_answer(out);
                }
            }
            Msg::Pushed => self.pushing = false,
            Msg::Probe(id, a) => {
                let good = w5::good_answer(&a);
                self.answered |= good;
                if let (true, Ok(v)) = (good, &a) {
                    self.probe_marker(v);
                }
                let now = self.steady.now_ms();
                if self.w5.on_answer(now, id, good).is_some() {
                    (self.log)(
                        "warn: W5: the OMS answers again and the last kill owes the halt: watcher_kill",
                    );
                    self.start_owed(now);
                }
            }
            Msg::Marker(kill_no, o) => {
                let now = self.steady.now_ms();
                let steps = match self.kill.as_mut() {
                    Some(k) if k.kill_no == kill_no => k.kill.marker_done(now, o),
                    _ => {
                        (self.log)(&format!(
                            "debug: a late kill marker outcome is ignored: {o:?}"
                        ));
                        return;
                    }
                };
                self.kill_steps(now, steps);
            }
            Msg::Halt(kill_no, id, r) => {
                let now = self.steady.now_ms();
                if self.refusing.as_ref().is_some_and(|rf| {
                    matches!(rf.wait, RefusalWait::Kill { kill_no: k, id: i, .. } if (k, i) == (kill_no, id))
                }) {
                    if let Some(k) = self.kill.as_mut() {
                        k.halt = None;
                    }
                    return self.refusal_halt_done(r);
                }
                let steps = match self.kill.as_mut() {
                    Some(k) if k.kill_no == kill_no => {
                        if k.halt.as_ref().is_some_and(|(open, _)| *open == id) {
                            k.halt = None;
                        }
                        k.kill.halt_done(now, id, r)
                    }
                    _ => {
                        (self.log)(&format!(
                            "debug: a late watcher_kill reply is ignored: {r:?}"
                        ));
                        return;
                    }
                };
                self.kill_steps(now, steps);
            }
            Msg::Positions(seq, p) => self.positions_read(seq, p),
            Msg::Owed(id, r) => {
                if self.owed.as_ref().is_some_and(|o| o.id == id) {
                    self.owed = None;
                    self.owed_done(self.steady.now_ms(), r);
                }
            }
        }
    }

    fn after_start(&mut self, now: u64, step: Step) {
        let Phase::Start(s) = &self.phase else {
            return;
        };
        match step {
            Step::Refuse { reason } => {
                (self.log)(&format!("error: start refused: {reason}"));
                self.alerts.once(&alert::refusal_text(&reason));
                self.refused = true;
            }
            Step::Done | Step::StayUpNoRequests => {
                self.ip = s.ip_check();
                // The account is verified: W5 watches from here on, also
                // while the watcher waits for the OMS and replays.
                self.w5.arm(self.steady.now_ms());
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
        match a.and_then(|v| Ok((boot_cap(&v)?, v))) {
            Ok((cap, v)) => {
                if let Some(k) = &self.cfg.kill {
                    if let Err(reason) = marker_rule(&v, &k.marker) {
                        (self.log)(&format!("error: start refused: {reason}"));
                        self.refuse(reason);
                        return;
                    }
                    self.marker_checked = true;
                }
                self.answered = true;
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
        self.alerts.poll(now, &self.client, &self.public_ip);
        self.watch_sender();
        self.w5_tick();
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
            // A kill pauses the engine (U19b3c): no round, no
            // watcher_mismatch send, no heartbeat; answers in flight are
            // still taken, what they ask for is not sent (`exec_answer`),
            // their deadlines run at the polls after it.
            Phase::Run(e) if self.kill.is_some() => {
                e.export_ages(now);
                return;
            }
            Phase::Run(e) => {
                let out = e.poll(now);
                self.exec(out);
                return;
            }
        }
        // Before the engine runs, the ages count from the config load.
        let age = now.saturating_sub(self.boot_ms) as f64 / 1000.0;
        for c in Check::ALL {
            self.metrics
                .set("watcher_check_age_s", &[("check", c.name())], age);
        }
    }

    /// A sender task that ended before the shutdown (a bug: a panic or a
    /// poisoned lock) sends nothing more; one error line (U19b1b review 1
    /// N1).
    fn watch_sender(&mut self) {
        if !self.sender_ended && self.alert_task.as_ref().is_some_and(Task::ended) {
            self.sender_ended = true;
            (self.log)(
                "error: the alert sender task ended: no [WATCH P1] is sent until the watcher restarts",
            );
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
        for (logic, interval) in checks(&self.cfg, &self.log) {
            e.add_check(logic, interval);
        }
        self.phase = Phase::Run(e);
    }

    /// Carries out the engine's actions.
    /// What the engine makes of an answer. While a kill runs (U19b3c,
    /// review 1 S1) a round in flight at the fire goes no further: its next
    /// page, its status request and any `watcher_mismatch` send are not
    /// sent (one debug line each); the round closes at its deadline at a
    /// poll after the kill, and an owed send goes again after its own
    /// deadline. The mismatch P1 (`Confirmed`) still goes.
    fn exec_answer(&mut self, actions: Vec<Action>) {
        if self.kill.is_none() {
            return self.exec(actions);
        }
        let (keep, dropped) = paused(actions);
        for what in dropped {
            (self.log)(&format!(
                "debug: a kill runs: the engine's request is not sent ({what}); its round ends at its deadline after the kill"
            ));
        }
        self.exec(keep);
    }

    fn exec(&mut self, actions: Vec<Action>) {
        for a in actions {
            self.alerts.on_action(&a);
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
                // The mismatch P1, given to `alerts` above (U19b1c).
                Action::Confirmed { .. } => {}
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

/// W5 and the kill (C-WATCH part 12, U19b3c). W5, the kill sequence, its
/// loop, the loop's request tasks, the position reads and the K-step
/// deadlines read `clock::Steady`; the texts name instants of the raw clock.
impl Runtime {
    /// At every tick, in every phase: W5's probe and fire, the running
    /// kill, the notices, the W5 and kill metrics.
    fn w5_tick(&mut self) {
        let now = self.steady.now_ms();
        if let Some(p) = self.w5.poll(now) {
            let s = self.status.clone();
            self.spawn(async move { Msg::Probe(p.req_id, s.get().await) });
        }
        if let Some(f) = self.w5.fire(now) {
            self.fire(now, f);
        }
        if let Some(o) = self.owed.take_if(|o| now >= o.deadline) {
            o.task.abort();
            self.owed_done(now, Err(kill::HALT_TIMED_OUT.to_string()));
        }
        if let Some(Refusing {
            wait: RefusalWait::Kill { deadline, .. },
            ..
        }) = &self.refusing
            && now >= *deadline
        {
            if let Some((_, task)) = self.kill.as_mut().and_then(|k| k.halt.take()) {
                task.abort();
            }
            self.refusal_halt_done(Err(kill::HALT_TIMED_OUT.to_string()));
        }
        if self.refusing.is_some() {
            // The kill is held until the refusal's watcher_kill ended.
            self.notices_tick(now);
            self.kill_metrics(now);
            return;
        }
        // K3's exit before the kill judges its deadline: a stop that ended
        // between two ticks is reported as it ended (review 1 N1).
        self.poll_stopper(now);
        self.kill_tick(now);
        self.notices_tick(now);
        self.kill_metrics(now);
    }

    /// W5 fired: the kill starts with K1.
    fn fire(&mut self, now: u64, f: Fire) {
        let Some(k) = self.cfg.kill.clone() else {
            return;
        };
        self.kills += 1;
        self.metrics.add("watcher_kills_total", &[], 1.0);
        self.metrics.set("watcher_kill_active", &[], 1.0);
        for step in ["marker", "halt", "stop"] {
            self.metrics.remove("watcher_kill_step", &[("step", step)]);
        }
        (self.log)(&format!(
            "error: W5: /api/status gave no good answer for {} ms: the watcher kills (the marker {}, watcher_kill, systemctl stop {} if that fails) and cancels every order with its own key",
            f.silent_ms,
            k.marker.display(),
            k.oms_unit
        ));
        let setup = Setup {
            name: self.cfg.name.clone(),
            kill: k,
            request_timeout_ms: self.cfg.http.request_timeout_ms,
            marker_checked: self.marker_checked,
            answered: self.answered,
            raw_now_ms: self.clock.now_ms(),
        };
        let (kill, steps) = Kill::new(f, setup, now);
        let inflight = Inflight::new(
            self.client.clone(),
            self.steady.clone(),
            self.cfg.http.request_timeout_ms,
        );
        self.kill = Some(KillRun {
            kill_no: self.kills,
            kill,
            inflight,
            halt: None,
            cancel_alls: 0,
            ignored: 0,
            stray: 0,
        });
        self.kill_steps(now, steps);
    }

    /// The running kill at `now`: the halt task past its deadline and the
    /// loop's request past its own are aborted before the next input, then
    /// what the loop's tasks sent, then the deadlines and retries.
    fn kill_tick(&mut self, now: u64) {
        let Some(k) = self.kill.as_mut() else {
            return;
        };
        if let Some((id, task)) = &k.halt
            && k.kill
                .halt_open()
                .is_none_or(|(open, deadline)| open != *id || now >= deadline)
        {
            task.abort();
            k.halt = None;
        }
        k.inflight.abort_due(now, k.kill.open_request());
        for a in k.inflight.take() {
            let Some(k) = self.kill.as_mut() else {
                return;
            };
            let steps = k.kill.arrived(now, a);
            self.kill_steps(now, steps);
        }
        let Some(k) = self.kill.as_mut() else {
            return;
        };
        let steps = k.kill.poll(now);
        self.kill_steps(now, steps);
        if let Some(k) = self.kill.as_mut() {
            let (ignored, stray) = (k.kill.ignored(), k.inflight.stray());
            if (ignored, stray) != (k.ignored, k.stray) {
                (k.ignored, k.stray) = (ignored, stray);
                (self.log)(&format!(
                    "debug: kill: {ignored} late outcomes and {stray} answers of aborted requests ignored so far"
                ));
            }
        }
    }

    /// K3's clients: each outcome to the kill that started it; each is
    /// polled until it is reaped (a dropped `Child` is never reaped).
    fn poll_stopper(&mut self, now: u64) {
        let mut outcomes = Vec::new();
        self.stoppers.retain_mut(|(kill_no, s)| {
            if let Some(o) = s.poll(now) {
                outcomes.push((*kill_no, o));
            }
            !s.reaped()
        });
        for (kill_no, o) in outcomes {
            match self.kill.as_mut() {
                Some(k) if k.kill_no == kill_no => {
                    let steps = k.kill.stop_done(now, o);
                    self.kill_steps(now, steps);
                }
                _ => (self.log)(&format!(
                    "debug: a late systemctl outcome is ignored: {o:?}"
                )),
            }
        }
    }

    /// Carries out the kill's steps, in order. While a refusal waits for
    /// its `watcher_kill` the kill is held: what it asks is not done (no
    /// second `watcher_kill` connection; it is aborted next).
    fn kill_steps(&mut self, now: u64, steps: Vec<kill::Step>) {
        if self.refusing.is_some() {
            if !steps.is_empty() {
                (self.log)("debug: a start refusal waits for its watcher_kill: the kill is held");
            }
            return;
        }
        let mut steps: VecDeque<kill::Step> = steps.into();
        while let Some(step) = steps.pop_front() {
            let Some(kill_no) = self.kill.as_ref().map(|k| k.kill_no) else {
                return;
            };
            let Some(cfg) = self.cfg.kill.as_ref() else {
                return;
            };
            match step {
                kill::Step::WriteMarker { text } => {
                    let path = cfg.marker.clone();
                    self.spawn(async move {
                        let o = kill::spawn_marker(path, text)
                            .await
                            .unwrap_or_else(|e| MarkerOutcome::Failed(e.to_string()));
                        Msg::Marker(kill_no, o)
                    });
                }
                kill::Step::Halt { id, detail } => {
                    let (path, name) = (self.cfg.admin_socket.clone(), self.cfg.name.clone());
                    let timeout = self.cfg.http.request_timeout_ms;
                    let task = self.spawn(async move {
                        Msg::Halt(
                            kill_no,
                            id,
                            admin_kill(&path, &name, &detail, timeout).await,
                        )
                    });
                    if let Some(k) = self.kill.as_mut()
                        && let Some((_, old)) = k.halt.replace((id, task))
                    {
                        // Never two watcher_kill connections at once.
                        old.abort();
                    }
                }
                kill::Step::StopUnit { deadline } => {
                    match Stopper::start(&self.systemctl, cfg, deadline) {
                        Ok(s) => self.stoppers.push((kill_no, s)),
                        Err(e) => {
                            (self.log)(&format!("error: kill: systemctl stop did not start: {e}"));
                            if let Some(k) = self.kill.as_mut() {
                                steps.extend(k.kill.stop_done(now, stop::Outcome::Failed(e)));
                            }
                        }
                    }
                }
                kill::Step::Loop(kill::Action::Notice(n)) => {
                    let header = self
                        .kill
                        .as_ref()
                        .map(|k| k.kill.kill_loop().header().to_string())
                        .unwrap_or_default();
                    self.queue_notice(n, header);
                }
                kill::Step::Loop(a) => {
                    if let Some(k) = self.kill.as_mut() {
                        k.inflight.start(&a, now);
                    }
                }
                kill::Step::Ended { report, halt_owed } => self.kill_ended(now, &report, halt_owed),
            }
        }
    }

    /// The kill ended: its tasks are aborted (a halt retry in flight too:
    /// the halt stays owed for W5), the engine runs again and W5 waits for
    /// a good answer.
    fn kill_ended(&mut self, now: u64, report: &kill::Report, halt_owed: bool) {
        self.kill_metrics(now);
        if let Some(mut k) = self.kill.take() {
            if let Some((_, task)) = k.halt.take() {
                task.abort();
            }
            k.inflight.abort_all();
        }
        self.w5.kill_ended(halt_owed);
        self.metrics.set("watcher_kill_active", &[], 0.0);
        let how = if report.aborted {
            "aborted".to_string()
        } else {
            format!(
                "zero open orders confirmed ({} rounds, {} cancel-alls)",
                report.rounds, report.cancel_all_calls
            )
        };
        (self.log)(&format!(
            "the kill ended: {how}; the halt is {}",
            if halt_owed { "owed" } else { "not owed" }
        ));
    }

    /// The marker-path rule on a good probe answer after the boot (U19b3d,
    /// review 3 N6): a failure is the lasting P1 `marker_path` and makes
    /// `marker_checked` false while it lasts (a kill then owes the halt);
    /// an answer that passes clears both. Not a start refusal (the boot
    /// status answer passed the rule) and not a kill. In the boot wait the
    /// boot rule judges.
    fn probe_marker(&mut self, v: &Value) {
        let Some(k) = &self.cfg.kill else {
            return;
        };
        if matches!(self.phase, Phase::Start(_) | Phase::Wait { .. }) {
            return;
        }
        let text = marker_rule(v, &k.marker)
            .err()
            .map(|_| alert::marker_path_text(oms_marker(v), &k.marker));
        self.marker_checked = text.is_none();
        self.alerts.set(alert::MARKER_PATH, text.as_deref());
    }

    /// Sends one owed `watcher_kill` (U19b3d), unless one is in flight:
    /// W5's after a kill, or the one before a marker-path refusal. The K2
    /// deadline; the engine runs meanwhile.
    fn start_owed(&mut self, now: u64) {
        if self.owed.is_some() {
            return;
        }
        self.owed_ids += 1;
        let id = self.owed_ids;
        let (path, name) = (self.cfg.admin_socket.clone(), self.cfg.name.clone());
        let timeout = self.cfg.http.request_timeout_ms;
        let detail = kill::late_halt_detail(&name);
        let task = self.spawn(async move {
            Msg::Owed(id, admin_kill(&path, &name, &detail, timeout).await)
        });
        let deadline = now
            .saturating_add(timeout.saturating_mul(2))
            .saturating_add(kill::HALT_EXTRA_MS);
        self.owed = Some(Owed { id, deadline, task });
    }

    /// The owed halt's outcome (its reply, or its deadline): one log line,
    /// and W5's P1 (`늦은 정지` / `늦은 정지 실패` once), or the refusal
    /// that waited for it.
    fn owed_done(&mut self, now: u64, r: Result<String, String>) {
        if matches!(
            self.refusing,
            Some(Refusing {
                wait: RefusalWait::Owed,
                ..
            })
        ) {
            return self.refusal_halt_done(r);
        }
        self.log_owed(&r);
        if let Some(text) = self.w5.owed_halt_done(now, r) {
            self.alerts.once(&text);
        }
    }

    fn log_owed(&self, r: &Result<String, String>) {
        match r {
            Ok(reply) if crate::oms::latched(reply) => (self.log)(&format!(
                "W5: the owed watcher_kill latched the OMS: {}",
                kill::cut(reply, kill::TEXT_MAX)
            )),
            Ok(why) | Err(why) => (self.log)(&format!(
                "error: W5: the owed watcher_kill did not latch the OMS: {}",
                kill::cut(why, kill::TEXT_MAX)
            )),
        }
    }

    /// The marker-path boot rule refused the start (U19b3d). While the halt
    /// is owed (a running kill that owes it or has not reached its loop:
    /// nothing confirmed a latch yet; or W5's owed halt), the answer shows
    /// that the OMS answers, and a latch is the only thing that keeps an OMS
    /// reading another marker path from trading after the watcher exits: one
    /// `watcher_kill` first (the one in flight, the kill's or W5's, if there
    /// is one), the kill held meanwhile. Then [`Runtime::finish_refusal`].
    fn refuse(&mut self, reason: String) {
        let kill_owes = self
            .kill
            .as_ref()
            .is_some_and(|k| k.kill.halt_owed() || k.kill.stage() != Stage::Loop);
        if !kill_owes && !self.w5.halt_owed() {
            return self.finish_refusal(&reason, None);
        }
        let open = self.kill.as_ref().and_then(|k| {
            let (id, _) = k.halt.as_ref()?;
            let (open, deadline) = k.kill.halt_open()?;
            (open == *id).then_some((k.kill_no, *id, deadline))
        });
        let wait = match open {
            Some((kill_no, id, deadline)) => RefusalWait::Kill {
                kill_no,
                id,
                deadline,
            },
            None => {
                // A kill's halt task whose attempt already ended goes first:
                // never two watcher_kill connections at once.
                if let Some((_, task)) = self.kill.as_mut().and_then(|k| k.halt.take()) {
                    task.abort();
                }
                self.start_owed(self.steady.now_ms());
                RefusalWait::Owed
            }
        };
        (self.log)("the start is refused while the halt is owed: watcher_kill first");
        self.refusing = Some(Refusing { reason, wait });
    }

    /// The `watcher_kill` a refusal waited for ended (its reply or its
    /// deadline): logged, then the refusal with its outcome.
    fn refusal_halt_done(&mut self, r: Result<String, String>) {
        self.log_owed(&r);
        if let Some(rf) = self.refusing.take() {
            self.finish_refusal(&rf.reason, Some(r));
        }
    }

    /// The refusal path (U19b3d, review 1 S10): a running kill is aborted
    /// with its notice first, then the start-refusal P1 (with the owed
    /// halt's outcome, if one was sent); the loop then flushes the sender
    /// and exits 2.
    fn finish_refusal(&mut self, reason: &str, halt: Option<Result<String, String>>) {
        self.abort_kill(AbortWhy::StartRefused(reason.to_string()));
        let mut text = alert::refusal_text(reason);
        if let Some(r) = &halt {
            text.push_str(". 늦은 정지: ");
            text.push_str(&halt_outcome(r));
        }
        self.alerts.once(&text);
        self.refused = true;
    }

    /// Queues a notice (see [`Notices::push`]); a drop is logged.
    fn queue_notice(&mut self, notice: Notice, header: String) {
        if self.notices.push(Waiting { notice, header }) {
            (self.log)(&format!(
                "warn: kill: {NOTICES_MAX} notices wait for their position read: the oldest progress notice is dropped ({} so far)",
                self.notices.dropped
            ));
        }
    }

    /// The position read past its deadline is aborted and its notice goes
    /// with `시간 초과`; the next waiting notice starts its read.
    fn notices_tick(&mut self, now: u64) {
        if let Some(r) = &self.notices.reading
            && now >= r.deadline
        {
            r.task.abort();
            if let Some(r) = self.notices.reading.take() {
                self.send_notice(&r.w, &Positions::Failed(kill::TIMED_OUT.to_string()));
            }
        }
        while self.notices.reading.is_none() {
            let Some(w) = self.notices.waiting.pop_front() else {
                return;
            };
            // The abort notices already say that nothing was read.
            if matches!(w.notice, Notice::Abort { .. } | Notice::AbortBefore { .. }) {
                (self.alerts).once(&kill::notice_text(&w.notice, &w.header));
                continue;
            }
            let seq = self.notices.next_seq;
            self.notices.next_seq += 1;
            let (c, clock) = (self.client.clone(), self.steady.clone());
            let timeout = self.cfg.http.request_timeout_ms;
            let task = self.spawn(async move {
                Msg::Positions(seq, kill::read_positions(&c, &*clock, timeout).await)
            });
            let deadline = now
                .saturating_add(timeout)
                .saturating_add(kill::DEADLINE_EXTRA_MS);
            self.notices.reading = Some(Reading {
                seq,
                w,
                deadline,
                task,
            });
        }
    }

    /// The positions of notice `seq`: an answer handled at or after the
    /// read's deadline is past it.
    fn positions_read(&mut self, seq: u64, p: Positions) {
        let now = self.steady.now_ms();
        if self.notices.reading.as_ref().is_none_or(|r| r.seq != seq) {
            return;
        }
        let Some(r) = self.notices.reading.take() else {
            return;
        };
        let p = if now >= r.deadline {
            Positions::Failed(kill::TIMED_OUT.to_string())
        } else {
            p
        };
        self.send_notice(&r.w, &p);
        self.notices_tick(now);
    }

    fn send_notice(&mut self, w: &Waiting, p: &Positions) {
        let body = format!(
            "{}\n{}",
            kill::notice_text(&w.notice, &w.header),
            kill::position_block(p, &kst(self.clock.now_ms()))
        );
        self.alerts.once(&body);
    }

    /// Shutdown (SIGTERM, SIGINT) during a kill (U19b3d): during K1 the
    /// marker's outcome is awaited up to K1's deadline (at most
    /// [`kill::MARKER_WAIT_MS`] of real time) so the notice can tell it;
    /// then `Kill::abort` in whatever step: the abort notice (no position
    /// read) and `Ended`. A halt task is aborted; K3's client, if it runs,
    /// is left to finish its job in systemd (a half-stopped OMS is worse
    /// than a stopped one); the marker stays. Every notice still waiting
    /// goes now, also when the kill had already ended (its final notice
    /// may still wait for its position read; review 1 M1).
    async fn shutdown_kill(&mut self, rx: &mut mpsc::UnboundedReceiver<Msg>) {
        // A refusal still waiting for its watcher_kill gives way: the
        // watcher ends (exit 0), its halt task is aborted with the rest.
        self.refusing = None;
        if let Some(k) = &self.kill
            && k.kill.stage() == Stage::Marker
        {
            let kill_no = k.kill_no;
            let left = k
                .kill
                .next_wake()
                .unwrap_or(0)
                .saturating_sub(self.steady.now_ms())
                .min(kill::MARKER_WAIT_MS);
            let end = tokio::time::Instant::now() + Duration::from_millis(left);
            loop {
                match tokio::time::timeout_at(end, rx.recv()).await {
                    Ok(Some(Msg::Marker(no, o))) if no == kill_no => {
                        let now = self.steady.now_ms();
                        if let Some(k) = self.kill.as_mut() {
                            // K2 is not started: the watcher ends.
                            let _ = k.kill.marker_done(now, o);
                        }
                        break;
                    }
                    Ok(Some(_)) => {}
                    _ => break,
                }
            }
        }
        self.abort_kill(AbortWhy::Shutdown);
    }

    /// Aborts a running kill, if one runs (shutdown or a start refusal,
    /// U19b3d): the abort notice and `Ended`; then every kill notice still
    /// waiting goes at once, in its order, the abort notice last.
    fn abort_kill(&mut self, why: AbortWhy) {
        let now = self.steady.now_ms();
        if let Some(k) = self.kill.as_mut() {
            let steps = k.kill.abort(now, why);
            self.kill_steps(now, steps);
        }
        self.flush_notices();
    }

    /// The kill notices still waiting, before the watcher ends (U19b3d): the
    /// one whose positions are being read (its read is aborted) and every
    /// waiting one go now with `포지션 조회 안 함(워처 종료 중)`; the abort
    /// notices as they are.
    fn flush_notices(&mut self) {
        let not_read = Positions::NotRead(ENDING.to_string());
        if let Some(r) = self.notices.reading.take() {
            r.task.abort();
            self.send_notice(&r.w, &not_read);
        }
        while let Some(w) = self.notices.waiting.pop_front() {
            if matches!(w.notice, Notice::Abort { .. } | Notice::AbortBefore { .. }) {
                self.alerts.once(&kill::notice_text(&w.notice, &w.header));
            } else {
                self.send_notice(&w, &not_read);
            }
        }
    }

    /// The W5 and kill series (C-WATCH part 12) from their accessors.
    fn kill_metrics(&mut self, now: u64) {
        let m = self.metrics.clone();
        m.set(
            "watcher_w5_armed",
            &[],
            f64::from(u8::from(self.w5.armed())),
        );
        match self.w5.silent_ms(now) {
            Some(ms) => m.set("watcher_oms_silence_s", &[], ms as f64 / 1000.0),
            None => m.remove("watcher_oms_silence_s", &[]),
        }
        if self.cfg.kill.is_none() {
            return;
        }
        let owed = self.w5.halt_owed() || self.kill.as_ref().is_some_and(|k| k.kill.halt_owed());
        m.set("watcher_kill_halt_owed", &[], f64::from(u8::from(owed)));
        let Some(k) = self.kill.as_mut() else {
            return;
        };
        let steps = k.kill.steps();
        for (step, done) in [
            ("marker", steps.marker),
            ("halt", steps.halt),
            ("stop", steps.stop),
        ] {
            if let Some(done) = done {
                m.set(
                    "watcher_kill_step",
                    &[("step", step)],
                    f64::from(u8::from(done)),
                );
            }
        }
        let lp = k.kill.kill_loop();
        m.set("watcher_kill_round", &[], lp.round() as f64);
        match lp.remaining() {
            Ok(n) => m.set("watcher_kill_remaining", &[], *n as f64),
            Err(_) => m.remove("watcher_kill_remaining", &[]),
        }
        let calls = lp.cancel_all_calls();
        if calls > k.cancel_alls {
            m.add(
                "watcher_kill_cancel_all_total",
                &[],
                (calls - k.cancel_alls) as f64,
            );
            k.cancel_alls = calls;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kill::StopOutcome;

    fn waiting(notice: Notice) -> Waiting {
        Waiting {
            notice,
            header: String::new(),
        }
    }

    fn progress(round: u64) -> Waiting {
        waiting(Notice::Progress {
            round,
            remaining: Ok(1),
            cancel_all_calls: round,
            last_error: None,
        })
    }

    fn last() -> Waiting {
        waiting(Notice::Final {
            watch_ms: 30_000,
            round: 99,
            cancel_all_calls: 99,
            outcome: StopOutcome::Latched,
        })
    }

    #[test]
    fn during_a_kill_an_answer_keeps_the_mismatch_p1_and_drops_every_request() {
        let p1 = Action::Confirmed {
            check: Check::W1,
            details: vec!["o1".into()],
        };
        let (keep, dropped) = paused(vec![
            Action::Venue {
                req_id: 1,
                path: "/v5/execution/list",
                params: Vec::new(),
            },
            p1.clone(),
            Action::Status { req_id: 2 },
            Action::Mismatch {
                check: Check::W1,
                send_id: 3,
                detail: "o1".into(),
            },
        ]);
        assert_eq!(keep, vec![p1]);
        assert_eq!(
            dropped,
            vec!["/v5/execution/list", "/api/status", "watcher_mismatch"]
        );
    }

    #[test]
    fn the_refusal_names_a_latch_only_for_a_reply_that_latched() {
        assert_eq!(halt_outcome(&Ok("latched".into())), "latched");
        let rearm = "already latched; the earlier confirmation is void and a new kill loop starts";
        assert_eq!(halt_outcome(&Ok(rearm.into())), rearm);
        assert_eq!(halt_outcome(&Ok("held".into())), "실패(held)");
        assert_eq!(halt_outcome(&Err("not sent: no socket".into())), "실패(not sent: no socket)");
        let long = "x".repeat(500);
        assert_eq!(halt_outcome(&Err(long)), format!("실패({})", "x".repeat(200)));
    }

    #[test]
    fn a_full_notice_queue_drops_the_oldest_progress_notice_never_a_start_or_a_final() {
        let mut q = Notices::default();
        assert!(!q.push(waiting(Notice::Start { round_ms: 1_000 })));
        for r in 1..NOTICES_MAX as u64 {
            assert!(!q.push(progress(r)), "round {r}");
        }
        assert_eq!(q.waiting.len(), NOTICES_MAX);
        assert!(q.push(last()), "the final makes room");
        assert_eq!((q.waiting.len(), q.dropped), (NOTICES_MAX, 1));
        let rounds: Vec<_> = q
            .waiting
            .iter()
            .map(|w| match w.notice {
                Notice::Start { .. } => "start".to_string(),
                Notice::Progress { round, .. } => round.to_string(),
                Notice::Final { .. } => "final".to_string(),
                _ => "other".to_string(),
            })
            .collect();
        assert_eq!(rounds.first().map(String::as_str), Some("start"));
        assert_eq!(rounds[1], "2", "the oldest progress notice went");
        assert_eq!(rounds.last().map(String::as_str), Some("final"));

        // Without a progress notice waiting nothing is dropped: the queue
        // grows past the bound rather than lose a late halt or the final.
        let mut q = Notices::default();
        for _ in 0..NOTICES_MAX {
            q.push(waiting(Notice::LateHalt {
                reply: "latched".into(),
            }));
        }
        assert!(!q.push(last()));
        assert_eq!((q.waiting.len(), q.dropped), (NOTICES_MAX + 1, 0));
    }
}
