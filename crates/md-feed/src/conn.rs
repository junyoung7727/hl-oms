//! One connection: connect, subscribe, read, apply, resync. The task owns the
//! writes to its `ConnState`; readers take the per-topic locks briefly.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use md_core::adapter::{Bridge, MarketDataAdapter, RestRequest, Resync, Route};
use md_core::{BookUpdate, GapEvent, Normalized, StreamKey};
use tokio::sync::{mpsc, watch};
use tokio_tungstenite::tungstenite::Message;

use crate::config::FeedConfig;
use crate::event::FeedEvent;
use crate::net::{self, RestClient};
use crate::sink::{FrameSink, MessageSink, RawFrame, Src};
use crate::state::{ConnState, MarketRef, RestSync, TopicState};

/// What every connection task of one feed shares.
pub(crate) struct Ctx<A> {
    pub adapter: Arc<A>,
    pub cfg: FeedConfig,
    pub sink: Option<Arc<dyn FrameSink>>,
    pub msgs: Option<Arc<dyn MessageSink>>,
    pub events: mpsc::Sender<FeedEvent>,
    pub dropped_events: AtomicU64,
    pub rest: Option<RestClient>,
    pub shutdown: watch::Receiver<bool>,
}

impl<A> Ctx<A> {
    pub fn emit(&self, e: FeedEvent) {
        if self.events.try_send(e).is_err() {
            self.dropped_events.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// Resolves when the flag becomes true (or its sender is gone).
async fn stopped(rx: &mut watch::Receiver<bool>) {
    let _ = rx.wait_for(|s| *s).await;
}

/// Why a connection ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum End {
    /// Asked to stop (switch, shutdown).
    Stopped,
    /// The connect failed or the connection broke.
    Failed(String),
    /// The feed closed it: silence, a resync that needs a new connection.
    Reconnect(String),
}

impl End {
    pub fn reason(&self) -> String {
        match self {
            End::Stopped => "stopped".into(),
            End::Failed(r) | End::Reconnect(r) => r.clone(),
        }
    }
}

/// What a received frame asks the connection to do.
enum Action {
    Send(Vec<String>),
    Rest(StreamKey, RestRequest),
    Reconnect(String),
}

/// A REST answer: `(recv_ns, frame_no, status, body)` (already in the sink),
/// or a transport error; `true` when the sink overflowed.
type RestResult = (StreamKey, Result<(i64, u64, u16, Vec<u8>), String>, bool);

pub(crate) struct Conn<A> {
    ctx: Arc<Ctx<A>>,
    state: Arc<ConnState>,
    rest_tx: mpsc::Sender<RestResult>,
}

impl<A: MarketDataAdapter + Send + Sync + 'static> Conn<A> {
    /// Runs the connection until it ends or `stop` becomes true. The
    /// connection is marked closed (its books stale) before this returns.
    pub async fn run(ctx: Arc<Ctx<A>>, state: Arc<ConnState>, stop: watch::Receiver<bool>) -> End {
        let sink = ctx.sink.clone();
        let end = Self::run_inner(ctx, state.clone(), stop).await;
        state.closed.store(true, Ordering::Release);
        if let Some(sink) = sink {
            sink.conn_ended(state.conn_id);
        }
        end
    }

    async fn run_inner(
        ctx: Arc<Ctx<A>>,
        state: Arc<ConnState>,
        mut stop: watch::Receiver<bool>,
    ) -> End {
        let conn_id = state.conn_id;
        let conn_index = state.plan.conn_index;
        let mut shutdown = ctx.shutdown.clone();
        let ws = tokio::select! {
            r = net::connect_ws(&ctx.cfg.ws_url, ctx.cfg.loopback_only, ctx.cfg.connect_timeout) => r,
            _ = stopped(&mut stop) => return End::Stopped,
            _ = stopped(&mut shutdown) => return End::Stopped,
        };
        let ws = match ws {
            Ok(ws) => ws,
            Err(e) => return End::Failed(e),
        };
        ctx.emit(FeedEvent::Connected {
            conn_index,
            conn_id,
        });
        let (mut write, mut read) = ws.split();
        for f in ctx.adapter.subscribe_frames(&state.plan) {
            if let Err(e) = write.send(Message::text(f)).await {
                return End::Failed(format!("subscribe failed: {e}"));
            }
        }
        if let Ok(mut t) = state.subscribed_at.lock() {
            *t = Some(Instant::now());
        }
        state.connected.store(true, Ordering::Release);
        state.ready.notify_one();
        let keep = ctx.adapter.keepalive();
        let interval = ctx
            .cfg
            .keepalive_interval
            .unwrap_or(Duration::from_millis(keep.interval_ms.max(1)));
        let silence = ctx
            .cfg
            .silence_timeout
            .unwrap_or(interval.saturating_mul(2) + Duration::from_secs(5));
        let mut ping = tokio::time::interval_at(tokio::time::Instant::now() + interval, interval);
        ping.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut check = tokio::time::interval(Duration::from_millis(50));
        let (rest_tx, mut rest_rx) = mpsc::channel::<RestResult>(64);
        let me = Conn {
            ctx: ctx.clone(),
            state: state.clone(),
            rest_tx,
        };
        let mut frame_no: u64 = 0;
        let mut last_rx = Instant::now();
        loop {
            let actions = tokio::select! {
                _ = stopped(&mut stop) => {
                    let _ = write.send(Message::Close(None)).await;
                    return End::Stopped;
                }
                _ = stopped(&mut shutdown) => {
                    let _ = write.send(Message::Close(None)).await;
                    return End::Stopped;
                }
                _ = ping.tick() => {
                    if let Some(f) = &keep.frame {
                        if let Err(e) = write.send(Message::text(f.clone())).await {
                            return End::Failed(format!("keepalive failed: {e}"));
                        }
                    }
                    continue;
                }
                _ = check.tick() => {
                    if last_rx.elapsed() > silence {
                        return End::Reconnect(format!("no message for {silence:?}"));
                    }
                    if let Some(reason) = me.resync_timed_out() {
                        return End::Reconnect(reason);
                    }
                    continue;
                }
                Some((key, res, overflow)) = rest_rx.recv() => {
                    if overflow {
                        state.closed.store(true, Ordering::Release);
                        let _ = write.send(Message::Close(None)).await;
                        return End::Reconnect("overflow".into());
                    }
                    me.on_rest(&key, res)
                }
                m = read.next() => {
                    let msg = match m {
                        None => return End::Failed("closed by the venue".into()),
                        Some(Err(e)) => return End::Failed(format!("read failed: {e}")),
                        Some(Ok(msg)) => msg,
                    };
                    let recv_ns = net::now_ns();
                    last_rx = Instant::now();
                    let payload = match msg {
                        Message::Text(t) => t.as_bytes().to_vec(),
                        Message::Binary(b) => b.to_vec(),
                        // tungstenite queued the pong; a flush sends it.
                        Message::Ping(_) => {
                            if let Err(e) = write.flush().await {
                                return End::Failed(format!("pong failed: {e}"));
                            }
                            continue;
                        }
                        Message::Pong(_) | Message::Frame(_) => continue,
                        Message::Close(_) => return End::Failed("close frame from the venue".into()),
                    };
                    let no = frame_no;
                    frame_no += 1;
                    // The sink first (spec F5): L0 keeps the frame even if
                    // parsing it goes wrong.
                    if let Some(sink) = &ctx.sink {
                        let f = RawFrame {
                            recv_ns,
                            src: Src::Ws,
                            conn_id,
                            frame_no: no,
                            conn_index,
                            topic: None,
                            for_conn_id: None,
                            payload: payload.clone(),
                        };
                        if sink.frame(f).is_err() {
                            state.closed.store(true, Ordering::Release);
                            let _ = write.send(Message::Close(None)).await;
                            return End::Reconnect("overflow".into());
                        }
                    }
                    me.on_frame(&payload, recv_ns, no)
                }
            };
            for a in actions {
                match a {
                    Action::Send(frames) => {
                        for f in frames {
                            if let Err(e) = write.send(Message::text(f)).await {
                                return End::Failed(format!("resubscribe failed: {e}"));
                            }
                        }
                    }
                    Action::Rest(key, req) => me.spawn_rest(key, req),
                    Action::Reconnect(reason) => {
                        let _ = write.send(Message::Close(None)).await;
                        return End::Reconnect(reason);
                    }
                }
            }
        }
    }

    /// A resync, or the first snapshot after subscribe (spec F3), that took
    /// longer than `resync_timeout`.
    fn resync_timed_out(&self) -> Option<String> {
        let limit = self.ctx.cfg.resync_timeout;
        let subscribed = self.state.subscribed_at.lock().ok().and_then(|t| *t);
        self.state.books.iter().find_map(|(key, st)| {
            let st = st.read().ok()?;
            match (st.resyncing, st.resync_started) {
                (true, Some(t)) if t.elapsed() > limit => Some(format!(
                    "resync of {}.{} took longer than {limit:?}",
                    key.channel, key.symbol
                )),
                (false, _) if !st.has_book && subscribed.is_some_and(|t| t.elapsed() > limit) => {
                    Some(format!(
                        "no snapshot of {}.{} within {limit:?} of subscribing",
                        key.channel, key.symbol
                    ))
                }
                _ => None,
            }
        })
    }

    fn market_ref(&self, u: &BookUpdate) -> MarketRef {
        MarketRef {
            venue: self.ctx.adapter.venue().to_string(),
            market: self.ctx.cfg.market.clone(),
            symbol: u.key.symbol.clone(),
            channel: u.key.channel.clone(),
            update_id: u.update_id,
            venue_ts_ns: u.venue_ts_ns,
            recv_ns: u.recv_ns,
        }
    }

    fn on_frame(&self, payload: &[u8], recv_ns: i64, frame_no: u64) -> Vec<Action> {
        let key = match self.ctx.adapter.route(payload) {
            Route::Control => return Vec::new(),
            Route::Stream(key) => key,
        };
        let is_book = self.state.books.contains_key(&key);
        if !is_book {
            let Some(seen) = self.state.seen.get(&key) else {
                // A topic this connection did not subscribe: kept in the sink only.
                return Vec::new();
            };
            if !seen.swap(true, Ordering::AcqRel) {
                self.state.ready.notify_one();
            }
        }
        let msgs = match self.ctx.adapter.parse(&key, payload, recv_ns) {
            Ok(msgs) => msgs,
            Err(e) => {
                self.ctx.emit(FeedEvent::ParseError {
                    conn_id: self.state.conn_id,
                    key: Some(key.clone()),
                    frame_no,
                    reason: e.reason,
                });
                if !is_book {
                    return Vec::new();
                }
                // The book may now be wrong: handle it as a gap.
                let Some(lock) = self.state.books.get(&key) else {
                    return Vec::new();
                };
                let Ok(mut st) = lock.write() else {
                    return Vec::new();
                };
                if st.resyncing || !st.has_book {
                    return Vec::new();
                }
                let gap = GapEvent {
                    last_good: st.last_ref.as_ref().and_then(|r| r.update_id),
                    next: None,
                    recv_ns,
                };
                return self.gap(&key, &mut st, gap, None).into_iter().collect();
            }
        };
        let mut actions = Vec::new();
        for m in msgs {
            match m {
                Normalized::BookSnapshot(u) => self.snapshot(&u),
                Normalized::BookDelta(u) => actions.extend(self.delta(u)),
                Normalized::Ticker(t) => {
                    if let Some(slot) = self.state.tickers.get(&t.key) {
                        if let Ok(mut cur) = slot.lock() {
                            let merged = match cur.as_mut() {
                                Some(c) if !t.is_snapshot => c.merge(&t),
                                _ => false,
                            };
                            if !merged {
                                *cur = Some(t.clone());
                            }
                        }
                    }
                    self.forward(&Normalized::Ticker(t));
                }
                m @ Normalized::Trade(_) => self.forward(&m),
                Normalized::Control(_) => {}
            }
        }
        actions
    }

    fn forward(&self, m: &Normalized) {
        if let Some(msgs) = &self.ctx.msgs {
            if self.state.active.load(Ordering::Acquire) {
                msgs.message(self.state.conn_id, m);
            }
        }
    }

    fn applied(&self, st: &mut TopicState, last: &BookUpdate) {
        st.last_ref = Some(self.market_ref(last));
        st.last_applied = Some(Instant::now());
    }

    fn snapshot(&self, u: &BookUpdate) {
        let Some(lock) = self.state.books.get(&u.key) else {
            return;
        };
        let Ok(mut st) = lock.write() else { return };
        if st.has_book {
            self.ctx.emit(FeedEvent::Resnapshot {
                conn_id: self.state.conn_id,
                key: u.key.clone(),
                recv_ns: u.recv_ns,
            });
        }
        st.book.apply_snapshot(u);
        st.detector.snapshot(u.update_id, u.recv_ns);
        st.has_book = true;
        st.resyncing = false;
        st.resync_started = None;
        st.rest = None;
        self.applied(&mut st, u);
        drop(st);
        self.state.applied_any.store(true, Ordering::Release);
        self.state.ready.notify_one();
    }

    fn delta(&self, u: BookUpdate) -> Option<Action> {
        let lock = self.state.books.get(&u.key)?;
        let mut st = lock.write().ok()?;
        if st.rest.is_some() {
            return self.rest_delta(&mut st, u);
        }
        if st.resyncing {
            // Until a snapshot ends the resync nothing is applied and no
            // further gap is reported.
            return None;
        }
        match st.detector.delta(u.update_id, u.prev_update_id, u.recv_ns) {
            Some(gap) => {
                let resync = self.ctx.adapter.on_gap(&u.key, &gap);
                if gap.last_good.is_none()
                    && !st.has_book
                    && resync == Resync::RestSnapshotThenReplay
                {
                    // The initial sync of a venue that starts with deltas: no
                    // run was broken, so this is not a gap.
                    let key = u.key.clone();
                    st.resyncing = true;
                    st.resync_started = Some(Instant::now());
                    return self.start_rest(&key, &mut st, vec![u]);
                }
                let key = u.key.clone();
                self.gap(&key, &mut st, gap, Some(u))
            }
            None => {
                if st.has_book {
                    st.book.apply_delta(&u);
                    self.applied(&mut st, &u);
                }
                None
            }
        }
    }

    /// One `Gap` event and the adapter's resync.
    /// `pending` is the delta that showed the gap; a REST resync keeps it,
    /// because it is newer than the book and may bridge the snapshot.
    fn gap(
        &self,
        key: &StreamKey,
        st: &mut TopicState,
        gap: GapEvent,
        pending: Option<BookUpdate>,
    ) -> Option<Action> {
        let resync = self.ctx.adapter.on_gap(key, &gap);
        self.ctx.emit(FeedEvent::Gap {
            conn_id: self.state.conn_id,
            key: key.clone(),
            gap,
            resync,
        });
        let now = Instant::now();
        st.resyncing = true;
        st.resync_started = Some(now);
        let window = self.ctx.cfg.resync_window;
        while st
            .resyncs
            .front()
            .is_some_and(|t| now.saturating_duration_since(*t) > window)
        {
            st.resyncs.pop_front();
        }
        st.resyncs.push_back(now);
        if st.resyncs.len() > self.ctx.cfg.max_resyncs as usize {
            return Some(Action::Reconnect(format!(
                "more than {} resyncs of {}.{} within {window:?}",
                self.ctx.cfg.max_resyncs, key.channel, key.symbol
            )));
        }
        match resync {
            Resync::Resubscribe => {
                let frames = self.ctx.adapter.resubscribe_frames(&self.state.plan, key);
                if frames.is_empty() {
                    Some(Action::Reconnect(format!(
                        "{}.{} cannot be resubscribed alone",
                        key.channel, key.symbol
                    )))
                } else {
                    Some(Action::Send(frames))
                }
            }
            Resync::Reconnect => Some(Action::Reconnect(format!(
                "gap on {}.{}",
                key.channel, key.symbol
            ))),
            Resync::RestSnapshotThenReplay => {
                self.start_rest(key, st, pending.into_iter().collect())
            }
        }
    }

    fn start_rest(
        &self,
        key: &StreamKey,
        st: &mut TopicState,
        buffered: Vec<BookUpdate>,
    ) -> Option<Action> {
        let req = match (
            &self.ctx.rest,
            self.ctx
                .adapter
                .snapshot_request(key, self.state.plan.depth),
        ) {
            (Some(_), Some(req)) => req,
            _ => {
                return Some(Action::Reconnect(format!(
                    "{}.{} needs a REST snapshot, which is not available",
                    key.channel, key.symbol
                )));
            }
        };
        let attempts = st.rest.as_ref().map_or(0, |r| r.attempts) + 1;
        st.rest = Some(RestSync {
            buffer: buffered.into(),
            attempts,
            snapshot: None,
        });
        Some(Action::Rest(key.clone(), req))
    }

    /// Fetches on its own task and sinks every body it receives, also a
    /// non-2xx one and also when the connection has ended meanwhile (F6).
    fn spawn_rest(&self, key: StreamKey, req: RestRequest) {
        let Some(rest) = self.ctx.rest.clone() else {
            return;
        };
        let tx = self.rest_tx.clone();
        let ctx = self.ctx.clone();
        let (conn_index, for_conn_id) = (self.state.plan.conn_index, self.state.conn_id);
        tokio::spawn(async move {
            let mut overflow = false;
            let res = match rest.get_any(&req).await {
                Ok((recv_ns, status, body)) => {
                    let frame_no = ctx.cfg.conn_ids.next_rest_frame_no();
                    if let Some(sink) = &ctx.sink {
                        let f = RawFrame {
                            recv_ns,
                            src: Src::Rest,
                            conn_id: 0,
                            frame_no,
                            conn_index,
                            topic: Some(key.clone()),
                            for_conn_id: Some(for_conn_id),
                            payload: body.clone(),
                        };
                        overflow = sink.frame(f).is_err();
                    }
                    Ok((recv_ns, frame_no, status, body))
                }
                Err(e) => Err(e),
            };
            let _ = tx.send((key, res, overflow)).await;
        });
    }

    /// A delta while a REST resync runs: buffered until the snapshot is in,
    /// then bridged.
    fn rest_delta(&self, st: &mut TopicState, u: BookUpdate) -> Option<Action> {
        let rest = st.rest.as_mut()?;
        let attempts = rest.attempts;
        let Some(s) = rest.snapshot else {
            rest.buffer.push_back(u);
            if rest.buffer.len() > self.ctx.cfg.max_buffered_deltas {
                return Some(Action::Reconnect(format!(
                    "more than {} deltas buffered during a REST resync",
                    self.ctx.cfg.max_buffered_deltas
                )));
            }
            return None;
        };
        let key = u.key.clone();
        match self.ctx.adapter.bridge(&key, s, &u) {
            Bridge::Drop => return None,
            Bridge::Bridges => {
                st.book.apply_delta(&u);
                st.detector.snapshot(u.update_id, u.recv_ns);
                st.rest = None;
                st.resyncing = false;
                st.resync_started = None;
                st.has_book = true;
                self.applied(st, &u);
                self.state.applied_any.store(true, Ordering::Release);
                self.state.ready.notify_one();
                self.ctx.emit(FeedEvent::Resynced {
                    conn_id: self.state.conn_id,
                    key,
                    recv_ns: u.recv_ns,
                });
                return None;
            }
            Bridge::Fails => {}
        }
        // The snapshot is older than the first delta after it: fetch again,
        // keeping this delta. Two attempts, then a new connection.
        if attempts >= 2 {
            return Some(Action::Reconnect(format!(
                "REST snapshot of {}.{} did not bridge twice",
                key.channel, key.symbol
            )));
        }
        self.start_rest(&key, st, vec![u])
    }

    fn on_rest(
        &self,
        key: &StreamKey,
        res: Result<(i64, u64, u16, Vec<u8>), String>,
    ) -> Vec<Action> {
        let Some(lock) = self.state.books.get(key) else {
            return Vec::new();
        };
        let failed = |reason: String, frame_no: u64| -> Vec<Action> {
            let Ok(mut st) = lock.write() else {
                return Vec::new();
            };
            if st.rest.is_none() {
                return Vec::new();
            }
            self.ctx.emit(FeedEvent::ParseError {
                conn_id: 0,
                key: Some(key.clone()),
                frame_no,
                reason,
            });
            self.retry_rest(key, &mut st).into_iter().collect()
        };
        let (recv_ns, frame_no, body) = match res {
            Ok((r, n, status, body)) if (200..300).contains(&status) => (r, n, body),
            Ok((_, n, status, _)) => return failed(format!("REST snapshot answered {status}"), n),
            Err(e) => return failed(e, 0),
        };
        let Ok(mut st) = lock.write() else {
            return Vec::new();
        };
        if st.rest.is_none() {
            // A WebSocket snapshot ended the resync first.
            return Vec::new();
        }
        let snap = match self.ctx.adapter.parse_snapshot(key, &body, recv_ns) {
            Ok(s) if s.update_id.is_some() => s,
            Ok(_) | Err(_) => {
                self.ctx.emit(FeedEvent::ParseError {
                    conn_id: 0,
                    key: Some(key.clone()),
                    frame_no,
                    reason: "REST snapshot without an update id or unreadable".into(),
                });
                return self.retry_rest(key, &mut st).into_iter().collect();
            }
        };
        let s = snap.update_id.unwrap_or_default();
        st.book.apply_snapshot(&snap);
        self.applied(&mut st, &snap);
        let buffered = match st.rest.as_mut() {
            Some(r) => {
                r.snapshot = Some(s);
                std::mem::take(&mut r.buffer)
            }
            None => Default::default(),
        };
        let mut actions = Vec::new();
        let mut rest_of = buffered.into_iter();
        while let Some(d) = rest_of.next() {
            let action = if st.rest.is_none() {
                // Bridged: the rest are ordinary deltas.
                match st.detector.delta(d.update_id, d.prev_update_id, d.recv_ns) {
                    Some(gap) => {
                        let k = d.key.clone();
                        self.gap(&k, &mut st, gap, Some(d))
                    }
                    None => {
                        st.book.apply_delta(&d);
                        self.applied(&mut st, &d);
                        None
                    }
                }
            } else {
                self.rest_delta(&mut st, d)
            };
            if let Some(a) = action {
                // A new REST fetch keeps the deltas not yet replayed.
                if let Some(r) = st.rest.as_mut().filter(|r| r.snapshot.is_none()) {
                    r.buffer.extend(rest_of);
                }
                actions.push(a);
                break;
            }
        }
        actions
    }

    fn retry_rest(&self, key: &StreamKey, st: &mut TopicState) -> Option<Action> {
        let attempts = st.rest.as_ref().map_or(0, |r| r.attempts);
        if attempts >= 2 {
            return Some(Action::Reconnect(format!(
                "REST snapshot of {}.{} failed twice",
                key.channel, key.symbol
            )));
        }
        let buffered: Vec<BookUpdate> = st
            .rest
            .as_mut()
            .map(|r| r.buffer.drain(..).collect())
            .unwrap_or_default();
        self.start_rest(key, st, buffered)
    }
}
