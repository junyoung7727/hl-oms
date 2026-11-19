//! The collector run (spec D5–D8): instruments, one feed per plan group,
//! events, the instrument refresh, the day change and the disk floor.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use md_core::adapter::Instrument;
use md_feed::{
    ConnIds, FeedConfig, FeedEvent, FeedHandle, FrameSink, Overflow, RawFrame, RestClient,
};
use md_lake::fs::{Fs, RealFs};
use md_lake::seal::Topic;
use md_lake::writer::WriterSettings;
use serde_json::json;
use tokio::task::JoinHandle;

use crate::config::{Config, MarketConfig, Symbols};
use crate::disk::{FreeSpace, StatVfs};
use crate::ingest::{self, Ingest, IngestSnapshot, Limits, WriterThread};
use crate::notes::Note;
use crate::notify::{Notifier, SdNotify};
use crate::obs::{Chrony, ClockOffset, LagSink, LogSink, SERVICE};
use crate::telemetry::Telemetry;
use alert_direct::{AlertConfig, AlertDirect, AlertHandle, Route};
use obs_push::{ObsConfig, ObsHandle, ObsPush};
use crate::plan::{self, Group};
use crate::registry::{self, Venue};

/// Wall clock of the day-change scheduler and the disk checker (spec R7).
pub trait WallClock: Send + Sync {
    fn now_ns(&self) -> i64;
}

pub struct SystemClock;

impl WallClock for SystemClock {
    fn now_ns(&self) -> i64 {
        crate::wall_ns()
    }
}

/// What the collector touches, injected so that tests can slow the disk,
/// set the free space and move the day change.
pub struct Deps {
    pub fs: Arc<dyn Fs>,
    pub free_space: Arc<dyn FreeSpace>,
    pub clock: Arc<dyn WallClock>,
    /// systemd notify (`READY=1`, `WATCHDOG=1`; U6 D5).
    pub notifier: Option<Arc<dyn Notifier>>,
    /// The clock offset reader (U6 D6).
    pub clock_offset: Option<Arc<dyn ClockOffset>>,
}

impl Deps {
    pub fn real(cfg: &Config) -> Deps {
        Deps {
            fs: RealFs::new(&cfg.lake_root),
            free_space: Arc::new(StatVfs(cfg.lake_root.clone())),
            clock: Arc::new(SystemClock),
            notifier: SdNotify::from_env().map(|n| Arc::new(n) as Arc<dyn Notifier>),
            clock_offset: Some(Arc::new(Chrony)),
        }
    }
}

/// The collector's outputs, made before the writer starts (U6).
pub(crate) struct Out {
    pub logs: Arc<LogSink>,
    pub telemetry: Arc<Telemetry>,
    pub obs: Option<Arc<ObsHandle>>,
    pub alert: Option<Arc<AlertHandle>>,
}

impl Out {
    fn start(cfg: &Config) -> Result<Out, StartError> {
        let obs = match &cfg.telemetry {
            Some(t) if t.vm_url.is_some() || t.vl_url.is_some() => {
                let mut oc = ObsConfig::new(t.vm_url.clone(), t.vl_url.clone());
                oc.queue_lines = t.queue_lines;
                oc.self_labels = [("service", SERVICE), ("venue", cfg.venue.as_str())]
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect();
                Some(Arc::new(ObsPush::start(oc).map_err(StartError::Failed)?))
            }
            _ => None,
        };
        let logs = Arc::new(LogSink {
            obs: obs.clone(),
            host: cfg.host.clone(),
            venue: cfg.venue.clone(),
        });
        let alert = match &cfg.alert {
            None => {
                logs.log(
                    "warn",
                    "no [alert] section: direct [MD P1] alerts are off",
                    json!({}),
                );
                None
            }
            Some(a) => {
                let tg = a
                    .load_telegram(cfg.loopback_only)
                    .map_err(StartError::Refused)?;
                let mut ac = AlertConfig::new(tg, "[MD P1]", &cfg.venue);
                if let Some(base) = &a.api_base {
                    ac.routes = vec![Route::direct(base).map_err(StartError::Refused)?];
                }
                ac.state_file = a.state_file.clone().or_else(|| {
                    std::env::var_os("RUNTIME_DIRECTORY")
                        .map(|d| std::path::PathBuf::from(d).join("alert-state.json"))
                });
                if ac.state_file.is_none() {
                    logs.log(
                        "warn",
                        "no alert state file: the repeat rule holds only within this process",
                        json!({}),
                    );
                }
                let h = AlertDirect::start(ac, Arc::new(alert_direct::SystemClock))
                    .map_err(StartError::Failed)?;
                Some(Arc::new(h))
            }
        };
        Ok(Out {
            logs,
            telemetry: Arc::new(Telemetry::new(&cfg.venue)),
            obs,
            alert,
        })
    }
}

/// The sink of one feed: forwards to the ingest until the feed is stopped
/// (spec R5), and remembers the feed's connections.
pub(crate) struct FeedSink {
    inner: Arc<Ingest>,
    open: AtomicBool,
    active: AtomicUsize,
    seen: Mutex<BTreeSet<u32>>,
}

impl FeedSink {
    fn new(inner: Arc<Ingest>) -> Arc<FeedSink> {
        Arc::new(FeedSink {
            inner,
            open: AtomicBool::new(true),
            active: AtomicUsize::new(0),
            seen: Mutex::new(BTreeSet::new()),
        })
    }

    /// Closes the gate and waits until no call is in progress.
    async fn close(&self) {
        self.open.store(false, Ordering::SeqCst);
        while self.active.load(Ordering::SeqCst) > 0 {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    }

    fn seen(&self) -> Vec<u32> {
        self.seen
            .lock()
            .map(|s| s.iter().copied().collect())
            .unwrap_or_default()
    }
}

impl FrameSink for FeedSink {
    fn frame(&self, f: RawFrame) -> Result<(), Overflow> {
        self.active.fetch_add(1, Ordering::SeqCst);
        // A frame after the stop decision is not written: the stop is the
        // start of the gap (its note, or the end of the session).
        let r = if self.open.load(Ordering::SeqCst) {
            if f.conn_id != 0 {
                if let Ok(mut s) = self.seen.lock() {
                    s.insert(f.conn_id);
                }
            }
            self.inner.frame(f)
        } else {
            Ok(())
        };
        self.active.fetch_sub(1, Ordering::SeqCst);
        r
    }

    fn conn_ended(&self, conn_id: u32) {
        self.inner.conn_ended(conn_id);
    }
}

/// Counters for U6 (spec D10).
#[derive(Debug, Default)]
pub(crate) struct Counters {
    pub events: Mutex<BTreeMap<&'static str, u64>>,
    pub instruments_ok: AtomicU64,
    pub instruments_failed: AtomicU64,
    pub instruments_last_recv_ns: AtomicU64,
    pub switches_done: AtomicU64,
    pub switches_failed: AtomicU64,
    pub disk_floor: AtomicBool,
    pub free_bytes: AtomicU64,
    /// A free-space reading exists (U6: `lake_disk_free_bytes`).
    pub free_known: AtomicBool,
}

/// What the collector reports to U6.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StateSnapshot {
    pub ingest: IngestSnapshot,
    pub events: BTreeMap<&'static str, u64>,
    pub instruments_ok: u64,
    pub instruments_failed: u64,
    pub instruments_last_recv_ns: u64,
    pub switches_done: u64,
    pub switches_failed: u64,
    pub disk_floor: bool,
    pub free_bytes: u64,
    /// `conn_id` of the active connection of every running slot, by
    /// (market, conn_index).
    pub active: BTreeMap<(String, u32), u32>,
}

fn event_kind(e: &FeedEvent) -> &'static str {
    match e {
        FeedEvent::Connected { .. } => "connected",
        FeedEvent::Disconnected { .. } => "disconnected",
        FeedEvent::Gap { .. } => "gap",
        FeedEvent::Resnapshot { .. } => "resnapshot",
        FeedEvent::Resynced { .. } => "resynced",
        FeedEvent::ParseError { .. } => "parse_error",
        FeedEvent::StaleRaised { .. } => "stale_raised",
        FeedEvent::StaleCleared { .. } => "stale_cleared",
        FeedEvent::SwitchDone { .. } => "switch_done",
        FeedEvent::SwitchFailed { .. } => "switch_failed",
    }
}

/// A plan group and its running feed.
pub(crate) struct Running {
    pub group: Group,
    pub handle: Option<FeedHandle>,
    pub sink: Arc<FeedSink>,
    pub events: Option<JoinHandle<()>>,
}

/// The markets' fixed inputs for starting feeds again.
pub(crate) struct MarketRun {
    pub cfg: MarketConfig,
    pub ids: Arc<ConnIds>,
    pub instruments: Vec<Instrument>,
    pub rest_base: String,
}

pub(crate) struct Inner {
    pub venue: &'static dyn Venue,
    pub cfg: Config,
    pub ingest: Arc<Ingest>,
    pub markets: Vec<MarketRun>,
    pub groups: tokio::sync::Mutex<Vec<Running>>,
    pub counters: Arc<Counters>,
    pub clock: Arc<dyn WallClock>,
    pub free_space: Arc<dyn FreeSpace>,
    pub session_ids: Arc<ConnIds>,
    pub fs: Arc<dyn Fs>,
    pub logs: Arc<LogSink>,
    pub telemetry: Arc<Telemetry>,
    pub obs: Option<Arc<ObsHandle>>,
    pub alert: Option<Arc<AlertHandle>>,
    pub clock_offset: Option<Arc<dyn ClockOffset>>,
    /// `(px_exp, qty_exp)` by (market, symbol).
    pub exps: BTreeMap<(String, String), (i8, i8)>,
}

/// A running collector.
pub struct CollectorHandle {
    inner: Arc<Inner>,
    writer: Option<WriterThread>,
    tasks: Vec<JoinHandle<()>>,
}

pub struct Collector;

/// Why the collector did not start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartError {
    /// The config or the venue's instrument list cannot give the plan; a
    /// restart cannot help (`md-collector` exits 2, systemd does not restart).
    Refused(String),
    /// Anything else (network, the writer); a restart may help (exit 1).
    Failed(String),
}

impl std::fmt::Display for StartError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StartError::Refused(e) | StartError::Failed(e) => f.write_str(e),
        }
    }
}

impl From<String> for StartError {
    fn from(e: String) -> Self {
        StartError::Failed(e)
    }
}

/// Fetches every page of one market's instrument list; every body goes to
/// L0. `Err` when an answer failed or a page did not parse.
pub(crate) async fn fetch_instruments(
    venue: &dyn Venue,
    market: &str,
    rest_base: &str,
    loopback_only: bool,
    ids: &ConnIds,
    ingest: &Ingest,
) -> Result<Vec<Instrument>, String> {
    let client = RestClient::new(rest_base, Duration::from_secs(10), loopback_only)?;
    let mut out = Vec::new();
    let mut cursor: Option<String> = None;
    // Judgement: a venue list of more than 100 pages is a fault.
    for _ in 0..100 {
        let req = venue
            .instruments_request(market, cursor.as_deref())
            .ok_or_else(|| format!("no instruments request for {market}"))?;
        let (recv_ns, status, body) = client.get_any(&req).await?;
        ingest.rest_body(recv_ns, ids.next_rest_frame_no(), body.clone());
        if !(200..300).contains(&status) {
            return Err(format!("instruments of {market}: status {status}"));
        }
        out.extend(venue.parse_instruments(market, &body, recv_ns)?);
        cursor = venue.next_page_cursor(&body);
        if cursor.is_none() {
            return Ok(out);
        }
    }
    Err(format!("instruments of {market}: more than 100 pages"))
}

fn conn_index_base(cfg: &Config, market: usize) -> u32 {
    cfg.markets[..market].iter().map(|m| m.conn_budget).sum()
}

impl Inner {
    fn feed_config(&self, m: &MarketRun) -> FeedConfig {
        let ws_url = m
            .cfg
            .ws_url
            .clone()
            .or_else(|| self.venue.default_ws_url(&m.cfg.name).map(str::to_string))
            .unwrap_or_default();
        let mut fc = FeedConfig::new(m.cfg.name.clone(), ws_url, m.ids.clone());
        fc.rest_base = Some(m.rest_base.clone());
        fc.loopback_only = self.cfg.loopback_only;
        fc
    }

    fn market(&self, name: &str) -> Option<&MarketRun> {
        self.markets.iter().find(|m| m.cfg.name == name)
    }

    /// Starts the feed of a group.
    pub(crate) fn start_group(&self, group: Group) -> Result<Running, String> {
        let m = self
            .market(&group.market)
            .ok_or_else(|| format!("no market {}", group.market))?;
        let sink = FeedSink::new(self.ingest.clone());
        let now = crate::wall_ns();
        for p in &group.plans {
            self.telemetry
                .register_slot(&group.market, p.conn_index, &p.topics, group.traded, now);
        }
        let lag: Arc<dyn md_feed::MessageSink> = Arc::new(LagSink {
            market: group.market.clone(),
            telemetry: self.telemetry.clone(),
        });
        let mut handle = self.venue.start_feed(
            &m.cfg.name,
            &m.instruments,
            m.cfg.max_topics_per_conn,
            self.feed_config(m),
            group.plans.clone(),
            registry::Sinks {
                frames: Some(sink.clone() as Arc<dyn FrameSink>),
                msgs: Some(lag),
            },
        )?;
        let events = handle.take_events().map(|mut rx| {
            let (counters, market) = (self.counters.clone(), group.market.clone());
            let (telemetry, logs) = (self.telemetry.clone(), self.logs.clone());
            tokio::spawn(async move {
                while let Some(e) = rx.recv().await {
                    telemetry.event(&market, &e);
                    let kind = event_kind(&e);
                    if let Ok(mut m) = counters.events.lock() {
                        *m.entry(kind).or_default() += 1;
                    }
                    match &e {
                        FeedEvent::SwitchDone { .. } => {
                            counters.switches_done.fetch_add(1, Ordering::Relaxed);
                        }
                        FeedEvent::SwitchFailed { .. } => {
                            counters.switches_failed.fetch_add(1, Ordering::Relaxed);
                        }
                        _ => {}
                    }
                    let level = match &e {
                        FeedEvent::Disconnected { .. }
                        | FeedEvent::Gap { .. }
                        | FeedEvent::ParseError { .. }
                        | FeedEvent::SwitchFailed { .. }
                        | FeedEvent::StaleRaised { .. } => "warn",
                        _ => "info",
                    };
                    logs.log(
                        level,
                        kind,
                        json!({"market": market, "event": format!("{e:?}")}),
                    );
                }
            })
        });
        Ok(Running {
            group,
            handle: Some(handle),
            sink,
            events,
        })
    }

    /// Stops a group's feed (spec R5): gate, drop, close markers.
    pub(crate) async fn stop_group(&self, r: &mut Running) {
        r.sink.close().await;
        drop(r.handle.take());
        for id in r.sink.seen() {
            self.ingest.conn_ended(id);
        }
        if let Some(t) = r.events.take() {
            t.abort();
        }
    }

    pub(crate) fn note(&self, n: &Note<'_>) {
        self.ingest.note(n);
    }

    /// One log line to stderr and VictoriaLogs (U6 D10).
    pub(crate) fn log(&self, level: &str, msg: &str, fields: serde_json::Value) {
        self.logs.log(level, msg, fields);
    }

    pub(crate) fn topics_of(group: &Group) -> Vec<Topic> {
        group
            .topics()
            .into_iter()
            .map(|k| Topic {
                market: group.market.clone(),
                channel: k.channel,
                symbol: k.symbol,
            })
            .collect()
    }
}

/// Every page of every market, again (D5): bodies recorded, list logged.
async fn refresh_instruments(inner: Arc<Inner>) {
    let every = Duration::from_millis(inner.cfg.instruments.refresh_ms.max(1000));
    let mut t = tokio::time::interval(every);
    t.tick().await;
    loop {
        t.tick().await;
        for m in &inner.markets {
            let r = fetch_instruments(
                inner.venue,
                &m.cfg.name,
                &m.rest_base,
                inner.cfg.loopback_only,
                &inner.session_ids,
                &inner.ingest,
            )
            .await;
            match r {
                Ok(list) => {
                    inner
                        .counters
                        .instruments_ok
                        .fetch_add(1, Ordering::Relaxed);
                    let now = u64::try_from(crate::wall_ns()).unwrap_or(0);
                    inner
                        .counters
                        .instruments_last_recv_ns
                        .store(now, Ordering::Relaxed);
                    let had: BTreeSet<&str> =
                        m.instruments.iter().map(|i| i.symbol.as_str()).collect();
                    let new: Vec<&str> = list
                        .iter()
                        .filter(|i| inner.venue.is_trading(i) && !had.contains(i.symbol.as_str()))
                        .map(|i| i.symbol.as_str())
                        .collect();
                    if !new.is_empty() {
                        inner.log(
                            "info",
                            "new symbols are collected from the next start",
                            json!({"market": m.cfg.name, "symbols": new}),
                        );
                    }
                }
                Err(e) => {
                    inner
                        .counters
                        .instruments_failed
                        .fetch_add(1, Ordering::Relaxed);
                    inner.log(
                        "warn",
                        "instruments refresh failed",
                        json!({"market": m.cfg.name, "error": e}),
                    );
                }
            }
        }
    }
}

impl Collector {
    /// Starts the collector on the current tokio runtime.
    pub async fn start(cfg: Config, deps: Deps) -> Result<CollectorHandle, StartError> {
        let venue = registry::venue(&cfg.venue)
            .ok_or_else(|| StartError::Refused(format!("venue {} is not registered", cfg.venue)))?;
        let session_ns = crate::wall_ns();
        let mut ws = WriterSettings::new(
            &cfg.venue,
            &cfg.collector,
            env!("CARGO_PKG_VERSION"),
            &cfg.host,
            session_ns,
        );
        ws.flush_ns = i64::try_from(cfg.write.flush_ms).unwrap_or(1000) * 1_000_000;
        ws.fsync_ns = i64::try_from(cfg.write.fsync_ms).unwrap_or(5000) * 1_000_000;
        let notifier = deps.notifier.clone();
        let out = Out::start(&cfg)?;
        let (ingest, writer) = ingest::start_with(
            deps.fs.clone(),
            ws,
            venue,
            Limits {
                queue_bytes: cfg.write.queue_bytes,
                total_bytes: cfg.write.total_queue_bytes.max(cfg.write.queue_bytes),
            },
            ingest::Hooks {
                notifier: notifier.clone(),
                log: Some(out.logs.clone()),
                telemetry: Some(out.telemetry.clone()),
            },
        )?;
        match Self::start_inner(cfg, deps, venue, ingest, out).await {
            Ok((inner, tasks)) => {
                if let Some(n) = &notifier {
                    n.notify("READY=1");
                }
                Ok(CollectorHandle {
                    inner,
                    writer: Some(writer),
                    tasks,
                })
            }
            Err(e) => {
                // The bodies fetched so far stay in L0.
                let _ = writer.stop();
                Err(e)
            }
        }
    }

    async fn start_inner(
        cfg: Config,
        deps: Deps,
        venue: &'static dyn Venue,
        ingest: Arc<Ingest>,
        out: Out,
    ) -> Result<(Arc<Inner>, Vec<JoinHandle<()>>), StartError> {
        let session_ids = Arc::new(ConnIds::new());
        let counters = Arc::new(Counters::default());
        let mut markets = Vec::new();
        let mut all_groups = Vec::new();
        for (i, m) in cfg.markets.iter().enumerate() {
            let slot = venue
                .market_slot(&m.name)
                .ok_or_else(|| StartError::Refused(format!("no market {}", m.name)))?;
            let (start, end) = registry::conn_range(slot);
            let ids = session_ids.range(start, end);
            let rest_base = m
                .rest_base
                .clone()
                .unwrap_or_else(|| venue.default_rest_base().to_string());
            let instruments =
                fetch_instruments(venue, &m.name, &rest_base, cfg.loopback_only, &ids, &ingest)
                    .await?;
            counters.instruments_ok.fetch_add(1, Ordering::Relaxed);
            counters.instruments_last_recv_ns.store(
                u64::try_from(crate::wall_ns()).unwrap_or(0),
                Ordering::Relaxed,
            );
            let symbols: Vec<String> = match &m.symbols {
                Symbols::List(l) => l.clone(),
                Symbols::All => instruments
                    .iter()
                    .filter(|i| venue.is_trading(i))
                    .map(|i| i.symbol.clone())
                    .collect(),
            };
            let groups = plan::groups(
                venue,
                m,
                conn_index_base(&cfg, i),
                &cfg.traded_channels,
                &symbols,
            )
            .map_err(StartError::Refused)?;
            // R4: every traded topic of this market is planned.
            let planned: BTreeSet<(String, String)> = groups
                .iter()
                .flat_map(|g| g.topics())
                .map(|k| (k.symbol, k.channel))
                .collect();
            for t in cfg.traded_channels.iter().filter(|t| t.market == m.name) {
                if !planned.contains(&(t.symbol.clone(), t.channel.clone())) {
                    return Err(StartError::Refused(format!(
                        "traded channel {}/{}/{} is not collected (not trading at the venue?)",
                        t.market, t.symbol, t.channel
                    )));
                }
            }
            all_groups.extend(groups);
            markets.push(MarketRun {
                cfg: m.clone(),
                ids,
                instruments,
                rest_base,
            });
        }
        let exps = markets
            .iter()
            .flat_map(|m| {
                m.instruments.iter().map(|i| {
                    (
                        (m.cfg.name.clone(), i.symbol.clone()),
                        (i.px_exp, i.qty_exp),
                    )
                })
            })
            .collect();
        let inner = Arc::new(Inner {
            venue,
            cfg,
            ingest,
            markets,
            groups: tokio::sync::Mutex::new(Vec::new()),
            counters,
            clock: deps.clock,
            free_space: deps.free_space,
            session_ids,
            fs: deps.fs,
            logs: out.logs,
            telemetry: out.telemetry,
            obs: out.obs,
            alert: out.alert,
            clock_offset: deps.clock_offset,
            exps,
        });
        {
            let mut running = inner.groups.lock().await;
            for g in all_groups {
                running.push(inner.start_group(g)?);
            }
        }
        let mut tasks = vec![
            tokio::spawn(refresh_instruments(inner.clone())),
            tokio::spawn(crate::daychange::run(inner.clone())),
            tokio::spawn(crate::disk::run(inner.clone())),
            tokio::spawn(crate::obs::run(inner.clone())),
        ];
        if let Some(a) = inner.alert.clone() {
            tasks.push(tokio::spawn(crate::obs::alerts(inner.clone(), a)));
        }
        Ok((inner, tasks))
    }
}

impl CollectorHandle {
    pub fn state(&self) -> StateSnapshot {
        let c = &self.inner.counters;
        let active = match self.inner.groups.try_lock() {
            Ok(groups) => groups
                .iter()
                .filter_map(|r| r.handle.as_ref().map(|h| (r, h)))
                .flat_map(|(r, h)| {
                    r.group.plans.iter().filter_map(move |p| {
                        h.active_conn_id(p.conn_index)
                            .map(|id| ((r.group.market.clone(), p.conn_index), id))
                    })
                })
                .collect(),
            Err(_) => BTreeMap::new(),
        };
        StateSnapshot {
            ingest: self.inner.ingest.state(),
            events: c.events.lock().map(|m| m.clone()).unwrap_or_default(),
            instruments_ok: c.instruments_ok.load(Ordering::Relaxed),
            instruments_failed: c.instruments_failed.load(Ordering::Relaxed),
            instruments_last_recv_ns: c.instruments_last_recv_ns.load(Ordering::Relaxed),
            switches_done: c.switches_done.load(Ordering::Relaxed),
            switches_failed: c.switches_failed.load(Ordering::Relaxed),
            disk_floor: c.disk_floor.load(Ordering::Relaxed),
            free_bytes: c.free_bytes.load(Ordering::Relaxed),
            active,
        }
    }

    /// The writer's error, if it stopped on one (the process must exit).
    pub fn failed(&self) -> Option<String> {
        self.writer.as_ref().and_then(WriterThread::failed)
    }

    /// Before an exit on a writer error (U6 D8, R2, R3): the `l0_write`
    /// condition is set and the alert outbox flushed for up to `wait`.
    /// `true` when everything was delivered (or no alert is configured).
    pub async fn report_write_failure(&self, wait: Duration) -> bool {
        let Some(a) = self.inner.alert.clone() else {
            return true;
        };
        let st = self.inner.ingest.state();
        let os = st.failed_os_error.clone().unwrap_or_default();
        let e = st.failed.clone().unwrap_or_default();
        a.set("l0_write", Some(crate::obs::write_failure_text(&os, &e)));
        tokio::task::spawn_blocking(move || a.flush_for(wait))
            .await
            .unwrap_or(false)
    }

    /// What the pushes and the alerts have done (tests, status).
    pub fn out_state(&self) -> (Option<obs_push::ObsState>, Option<alert_direct::AlertState>) {
        (
            self.inner.obs.as_ref().map(|o| o.state()),
            self.inner.alert.as_ref().map(|a| a.state()),
        )
    }

    /// Stops every feed, then closes every file (frame end, fsync, unlock).
    pub async fn shutdown(mut self) -> Result<(), String> {
        for t in self.tasks.drain(..) {
            t.abort();
        }
        {
            let mut groups = self.inner.groups.lock().await;
            for r in groups.iter_mut() {
                self.inner.stop_group(r).await;
            }
        }
        match self.writer.take() {
            Some(w) => tokio::task::spawn_blocking(move || w.stop())
                .await
                .map_err(|e| e.to_string())?,
            None => Ok(()),
        }
    }
}
