//! The feed: one supervisor task per plan slot (connect, reconnect, switch),
//! a stale checker, and the read API.

use std::collections::BTreeMap;
use std::sync::atomic::Ordering;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use md_core::adapter::{ConnPlan, MarketDataAdapter};
use md_core::{StreamKey, Ticker};
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;

use crate::config::FeedConfig;
use crate::conn::{Conn, Ctx, End};
use crate::event::FeedEvent;
use crate::net::{self, RestClient};
use crate::sink::{FrameSink, MessageSink};
use crate::state::{BookView, ConnState, MarketRef};

/// One plan slot: its plan and its active connection.
struct Slot {
    plan: ConnPlan,
    active: RwLock<Option<Arc<ConnState>>>,
    cmds: mpsc::Sender<SlotCmd>,
}

enum SlotCmd {
    Switch,
}

impl Slot {
    fn active(&self) -> Option<Arc<ConnState>> {
        self.active.read().ok().and_then(|a| a.clone())
    }

    fn set_active(&self, conn: &Arc<ConnState>) {
        if let Ok(mut a) = self.active.write() {
            if let Some(old) = a.as_ref() {
                old.active.store(false, Ordering::Release);
            }
            conn.active.store(true, Ordering::Release);
            *a = Some(conn.clone());
        }
    }
}

/// Starts feeds.
pub struct Feed;

/// A running feed. Dropping it stops every connection.
pub struct FeedHandle {
    slots: Arc<Vec<Arc<Slot>>>,
    topic_slot: BTreeMap<StreamKey, usize>,
    stale_cfg: FeedConfig,
    events: Option<mpsc::Receiver<FeedEvent>>,
    shutdown: watch::Sender<bool>,
    tasks: Vec<JoinHandle<()>>,
    dropped_events: Arc<dyn Fn() -> u64 + Send + Sync>,
}

impl Feed {
    /// Starts one supervisor per plan on the current tokio runtime.
    pub fn start<A: MarketDataAdapter + Send + Sync + 'static>(
        adapter: Arc<A>,
        cfg: FeedConfig,
        plans: Vec<ConnPlan>,
        sink: Option<Arc<dyn FrameSink>>,
    ) -> Result<FeedHandle, String> {
        Self::start_with(adapter, cfg, plans, sink, None)
    }

    /// As [`Feed::start`], with a sink for the parsed trades and tickers.
    pub fn start_with<A: MarketDataAdapter + Send + Sync + 'static>(
        adapter: Arc<A>,
        cfg: FeedConfig,
        plans: Vec<ConnPlan>,
        sink: Option<Arc<dyn FrameSink>>,
        msgs: Option<Arc<dyn MessageSink>>,
    ) -> Result<FeedHandle, String> {
        if let Some(m) = adapter.market_name() {
            if m != cfg.market {
                return Err(format!(
                    "the adapter is for market {m}, the config says {}",
                    cfg.market
                ));
            }
        }
        let mut topic_slot = BTreeMap::new();
        for (i, p) in plans.iter().enumerate() {
            for k in &p.topics {
                if topic_slot.insert(k.clone(), i).is_some() {
                    return Err(format!("topic {}.{} is in two plans", k.channel, k.symbol));
                }
            }
        }
        let rest = match &cfg.rest_base {
            Some(base) => Some(RestClient::new(base.clone(), cfg.connect_timeout, cfg.loopback_only)?),
            None => None,
        };
        let (tx, rx) = mpsc::channel(cfg.event_capacity.max(1));
        let (shutdown, shutdown_rx) = watch::channel(false);
        let ctx = Arc::new(Ctx {
            adapter,
            cfg: cfg.clone(),
            sink,
            msgs,
            events: tx,
            dropped_events: Default::default(),
            rest,
            shutdown: shutdown_rx,
        });
        let mut slots = Vec::new();
        let mut cmd_rxs = Vec::new();
        for p in plans {
            let (ctx_tx, ctx_rx) = mpsc::channel(4);
            slots.push(Arc::new(Slot {
                plan: p,
                active: RwLock::new(None),
                cmds: ctx_tx,
            }));
            cmd_rxs.push(ctx_rx);
        }
        let slots = Arc::new(slots);
        let mut tasks = Vec::new();
        for (pos, (slot, cmds)) in slots.iter().zip(cmd_rxs).enumerate() {
            tasks.push(tokio::spawn(supervise(ctx.clone(), slot.clone(), cmds, pos)));
        }
        tasks.push(tokio::spawn(stale_checker(ctx.clone(), slots.clone())));
        let c2 = ctx.clone();
        Ok(FeedHandle {
            slots,
            topic_slot,
            stale_cfg: cfg,
            events: Some(rx),
            shutdown,
            tasks,
            dropped_events: Arc::new(move || c2.dropped_events.load(Ordering::Relaxed)),
        })
    }
}

impl Drop for FeedHandle {
    fn drop(&mut self) {
        let _ = self.shutdown.send(true);
        for t in &self.tasks {
            t.abort();
        }
    }
}

impl FeedHandle {
    /// The event receiver; `None` after the first call.
    pub fn take_events(&mut self) -> Option<mpsc::Receiver<FeedEvent>> {
        self.events.take()
    }

    /// Events dropped because the event channel was full.
    pub fn dropped_events(&self) -> u64 {
        (self.dropped_events)()
    }

    fn active_conn(&self, key: &StreamKey) -> Option<Arc<ConnState>> {
        let slot = self.slots.get(*self.topic_slot.get(key)?)?;
        slot.active()
    }

    /// Reads one book: the book, its `market_ref` and its stale flag under one
    /// lock. `None` when the topic is not a book topic of this feed or its
    /// slot has no connection yet.
    pub fn read_book<R>(&self, key: &StreamKey, f: impl FnOnce(BookView<'_>) -> R) -> Option<R> {
        let conn = self.active_conn(key)?;
        let lock = conn.books.get(key)?;
        let st = lock.read().ok()?;
        let stale = conn.closed.load(Ordering::Acquire)
            || st.stale(Instant::now(), self.stale_cfg.stale_after(&key.channel));
        Some(f(BookView {
            book: &st.book,
            market_ref: st.last_ref.as_ref(),
            stale,
            conn_id: conn.conn_id,
        }))
    }

    /// The fields of the last applied message of the book.
    pub fn market_ref(&self, key: &StreamKey) -> Option<MarketRef> {
        self.read_book(key, |v| v.market_ref.cloned()).flatten()
    }

    /// Whether the book is stale. An unknown topic, or one without a
    /// connection, is stale.
    pub fn is_stale(&self, key: &StreamKey) -> bool {
        self.read_book(key, |v| v.stale).unwrap_or(true)
    }

    /// The merged ticker of a ticker topic.
    pub fn ticker(&self, key: &StreamKey) -> Option<Ticker> {
        let conn = self.active_conn(key)?;
        conn.tickers.get(key)?.lock().ok()?.clone()
    }

    /// The `conn_id` of the active connection of the slot with this `conn_index`.
    pub fn active_conn_id(&self, conn_index: u32) -> Option<u32> {
        self.slots
            .iter()
            .find(|s| s.plan.conn_index == conn_index)?
            .active()
            .map(|c| c.conn_id)
    }

    /// Opens a new connection for the topics of the slot with this
    /// `conn_index`, and makes it active when it has every book (spec
    /// "Overlapping switch"). The result comes as `SwitchDone` or
    /// `SwitchFailed`.
    pub fn switch(&self, conn_index: u32) -> Result<(), String> {
        let slot = self
            .slots
            .iter()
            .find(|s| s.plan.conn_index == conn_index)
            .ok_or_else(|| format!("no plan with conn_index {conn_index}"))?;
        slot.cmds
            .try_send(SlotCmd::Switch)
            .map_err(|e| format!("switch not queued: {e}"))
    }
}

/// A running connection.
struct Running {
    conn: Arc<ConnState>,
    stop: watch::Sender<bool>,
    done: JoinHandle<End>,
}

fn spawn_conn<A: MarketDataAdapter + Send + Sync + 'static>(
    ctx: &Arc<Ctx<A>>,
    slot: &Slot,
) -> Option<Running> {
    let conn_id = ctx.cfg.conn_ids.next()?;
    let a = ctx.adapter.clone();
    let b = ctx.adapter.clone();
    let conn = Arc::new(ConnState::new(
        conn_id,
        slot.plan.clone(),
        |k| a.is_book(k),
        |k| b.seq_rule(k),
    ));
    let (stop, stop_rx) = watch::channel(false);
    let done = tokio::spawn(Conn::run(ctx.clone(), conn.clone(), stop_rx));
    Some(Running { conn, stop, done })
}

/// Marks the connection closed (its books stale, R3) and reports it.
fn ended<A>(ctx: &Ctx<A>, r: &Running, end: Result<End, tokio::task::JoinError>) {
    r.conn.closed.store(true, Ordering::Release);
    let reason = match end {
        Ok(e) => e.reason(),
        Err(e) => format!("connection task failed: {e}"),
    };
    ctx.emit(FeedEvent::Disconnected {
        conn_index: r.conn.plan.conn_index,
        conn_id: r.conn.conn_id,
        reason,
    });
}

async fn wait_ready(c: &Option<(Running, tokio::time::Instant)>) {
    match c {
        Some((r, _)) => loop {
            if r.conn.all_books_ready() {
                return;
            }
            r.conn.ready.notified().await;
        },
        None => std::future::pending().await,
    }
}

async fn supervise<A: MarketDataAdapter + Send + Sync + 'static>(
    ctx: Arc<Ctx<A>>,
    slot: Arc<Slot>,
    mut cmds: mpsc::Receiver<SlotCmd>,
    pos: usize,
) {
    let cfg = &ctx.cfg;
    let spread = Duration::from_millis(100)
        .saturating_mul(u32::try_from(pos).unwrap_or(u32::MAX))
        .min(Duration::from_secs(2));
    let mut backoff = cfg.backoff_min;
    let mut current: Option<Running> = None;
    let mut candidate: Option<(Running, tokio::time::Instant)> = None;
    let mut first = true;
    loop {
        if current.is_none() {
            if !first {
                tokio::time::sleep(backoff + spread).await;
                backoff = (backoff.saturating_mul(2)).min(cfg.backoff_max);
            }
            first = false;
            let Some(r) = spawn_conn(&ctx, &slot) else {
                ctx.emit(FeedEvent::Disconnected {
                    conn_index: slot.plan.conn_index,
                    conn_id: 0,
                    reason: "no conn_id left in this session".into(),
                });
                return;
            };
            slot.set_active(&r.conn);
            current = Some(r);
        }
        let Some(cur) = current.as_mut() else { continue };
        tokio::select! {
            end = &mut cur.done => {
                let Some(r) = current.take() else { continue };
                ended(&ctx, &r, end);
                if r.conn.applied_any.load(Ordering::Acquire) {
                    backoff = cfg.backoff_min;
                }
                if let Some((c, _)) = candidate.take() {
                    // R7: the switch's connection takes over the slot.
                    slot.set_active(&c.conn);
                    ctx.emit(FeedEvent::SwitchDone {
                        conn_index: slot.plan.conn_index,
                        from_conn_id: r.conn.conn_id,
                        to_conn_id: c.conn.conn_id,
                    });
                    current = Some(c);
                }
            }
            cmd = cmds.recv() => match cmd {
                Some(SlotCmd::Switch) => {
                    if candidate.is_none() {
                        match spawn_conn(&ctx, &slot) {
                            Some(c) => {
                                let deadline = tokio::time::Instant::now() + cfg.resync_timeout.saturating_mul(2);
                                candidate = Some((c, deadline));
                            }
                            None => ctx.emit(FeedEvent::SwitchFailed {
                                conn_index: slot.plan.conn_index,
                                conn_id: 0,
                                reason: "no conn_id left in this session".into(),
                            }),
                        }
                    }
                }
                None => return,
            },
            _ = wait_ready(&candidate) => {
                let Some((c, _)) = candidate.take() else { continue };
                let Some(old) = current.replace(c) else { continue };
                if let Some(new) = current.as_ref() {
                    slot.set_active(&new.conn);
                    ctx.emit(FeedEvent::SwitchDone {
                        conn_index: slot.plan.conn_index,
                        from_conn_id: old.conn.conn_id,
                        to_conn_id: new.conn.conn_id,
                    });
                }
                let _ = old.stop.send(true);
                let ctx2 = ctx.clone();
                tokio::spawn(async move {
                    let mut old = old;
                    let end = (&mut old.done).await;
                    ended(&ctx2, &old, end);
                });
            }
            _ = async {
                match &candidate {
                    Some((_, deadline)) => tokio::time::sleep_until(*deadline).await,
                    None => std::future::pending().await,
                }
            } => {
                if let Some((c, _)) = candidate.take() {
                    let _ = c.stop.send(true);
                    let mut c = c;
                    let end = (&mut c.done).await;
                    c.conn.closed.store(true, Ordering::Release);
                    ctx.emit(FeedEvent::SwitchFailed {
                        conn_index: slot.plan.conn_index,
                        conn_id: c.conn.conn_id,
                        reason: match end {
                            Ok(End::Stopped) => "no snapshot of every book in time".into(),
                            Ok(e) => e.reason(),
                            Err(e) => e.to_string(),
                        },
                    });
                }
            }
        }
    }
}

/// Sends `StaleRaised` and `StaleCleared` on transitions of the active books.
/// A (connection, topic) is followed from its first fresh reading on, so a
/// new connection that has not had its snapshot yet sends nothing.
async fn stale_checker<A>(ctx: Arc<Ctx<A>>, slots: Arc<Vec<Arc<Slot>>>) {
    let min = ctx
        .cfg
        .stale_after_by_channel
        .values()
        .copied()
        .chain([ctx.cfg.stale_after])
        .min()
        .unwrap_or(ctx.cfg.stale_after);
    let every = (min / 4).max(Duration::from_millis(50));
    let mut tick = tokio::time::interval(every);
    let mut was: BTreeMap<(u32, StreamKey), bool> = BTreeMap::new();
    loop {
        tick.tick().await;
        let now = Instant::now();
        let mut seen = std::collections::BTreeSet::new();
        for slot in slots.iter() {
            let Some(conn) = slot.active() else { continue };
            let closed = conn.closed.load(Ordering::Acquire);
            for (key, lock) in &conn.books {
                let Ok(st) = lock.read() else { continue };
                let stale = closed || st.stale(now, ctx.cfg.stale_after(&key.channel));
                let last_recv_ns = st.last_ref.as_ref().map(|r| r.recv_ns);
                drop(st);
                let id = (conn.conn_id, key.clone());
                seen.insert(id.clone());
                match was.get(&id).copied() {
                    None if !stale => {
                        was.insert(id, false);
                    }
                    None => {}
                    Some(prev) if prev != stale => {
                        was.insert(id, stale);
                        ctx.emit(if stale {
                            FeedEvent::StaleRaised {
                                conn_id: conn.conn_id,
                                key: key.clone(),
                                last_recv_ns,
                                now_ns: net::now_ns(),
                            }
                        } else {
                            FeedEvent::StaleCleared {
                                conn_id: conn.conn_id,
                                key: key.clone(),
                                now_ns: net::now_ns(),
                            }
                        });
                    }
                    Some(_) => {}
                }
            }
        }
        was.retain(|k, _| seen.contains(k));
    }
}
