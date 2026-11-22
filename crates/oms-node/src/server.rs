//! The local socket API.
//!
//! Trust boundary: the sockets are files; who may open them is decided by
//! file permissions, and the peer's user id is checked again on accept. A
//! strategy connection may act only for the strategy it named in `hello` and
//! sees only that strategy's orders. Operator operations exist only on the
//! admin socket. The monitor socket (only with the monitoring gate, C-LIVE
//! part 2) takes `hello`, `ping` and the watcher's `heartbeat`, nothing
//! else, and `heartbeat` exists only there. Every request line is parsed into typed values before
//! anything else happens, and nothing a client sends is interpreted as code,
//! a path or a format string.
//!
//! Resource bounds per connection: one line is at most `MAX_LINE` bytes; at
//! most `window` requests are outstanding, and no further line is read until
//! one completes; replies and events wait in a bounded buffer; a connection
//! holds at most one subscription and must say `hello` within a time limit.
//! The number of connections is capped, separately for each socket. A request that cannot enter the core's queue is
//! refused at once with `overloaded` and leaves no trace in the journal.

use crate::config::{ApiConfig, IngressLimits};
use crate::liveness::{HelloOutcome, Liveness};
use crate::monitor::{HeartbeatPace, MONITOR_MAX_CONNECTIONS};
use crate::pipeline::Shared;
use crate::queues::{CoreMsg, Lane};
use crate::status;
use crate::{info, warn};
use oms_api::*;
use oms_core::*;
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{Notify, Semaphore, mpsc};

struct Bucket {
    tokens: f64,
    last: Instant,
}

impl Bucket {
    fn take(&mut self, per_sec: u32, burst: u32) -> bool {
        let now = Instant::now();
        let dt = now.duration_since(self.last).as_secs_f64();
        self.last = now;
        self.tokens = (self.tokens + dt * f64::from(per_sec)).min(f64::from(burst));
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

pub struct ApiCtx {
    pub cfg: ApiConfig,
    pub limits: IngressLimits,
    pub shared: Arc<Shared>,
    pub info: status::NodeInfo,
    pub own_uid: u32,
    /// Why the gateway keeps the link down, if it says (Bybit only).
    pub gateway_block: Option<crate::venue::bybit::gateway::BlockView>,
    /// The kill switch's process state (Bybit only, C-KILL).
    pub gateway_kill: Option<crate::venue::bybit::gateway::KillStatusView>,
    /// The direct alerts, if `[alert]` is configured (C-ALERT).
    pub alerts: Option<Arc<crate::alerts::Alerts>>,
    /// Strategy liveness (C-LIVE): process state of the trading
    /// connections.
    pub liveness: Mutex<Liveness>,
    connections: Arc<Semaphore>,
    admin_connections: Arc<Semaphore>,
    monitor_connections: Arc<Semaphore>,
    buckets: Mutex<HashMap<(StrategyId, bool), Bucket>>,
    pub rate_limited: std::sync::atomic::AtomicU64,
    pub refused_overload: std::sync::atomic::AtomicU64,
}

impl ApiCtx {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        cfg: ApiConfig,
        limits: IngressLimits,
        shared: Arc<Shared>,
        info: status::NodeInfo,
        own_uid: u32,
        gateway_block: Option<crate::venue::bybit::gateway::BlockView>,
        gateway_kill: Option<crate::venue::bybit::gateway::KillStatusView>,
        alerts: Option<Arc<crate::alerts::Alerts>>,
        liveness: Liveness,
    ) -> ApiCtx {
        ApiCtx {
            gateway_block,
            gateway_kill,
            alerts,
            liveness: Mutex::new(liveness),
            connections: Arc::new(Semaphore::new(cfg.max_connections)),
            admin_connections: Arc::new(Semaphore::new(cfg.admin_max_connections)),
            monitor_connections: Arc::new(Semaphore::new(MONITOR_MAX_CONNECTIONS)),
            cfg,
            limits,
            shared,
            info,
            own_uid,
            buckets: Mutex::new(HashMap::new()),
            rate_limited: Default::default(),
            refused_overload: Default::default(),
        }
    }

    /// Token bucket per strategy; cancels have their own bucket.
    fn allow(&self, strategy: &StrategyId, cancel: bool) -> bool {
        let (rate, burst) = if cancel {
            (self.limits.cancels_per_sec, self.limits.cancels_burst)
        } else {
            (self.limits.new_orders_per_sec, self.limits.new_orders_burst)
        };
        let mut g = self.buckets.lock().unwrap_or_else(|e| e.into_inner());
        let b = g.entry((strategy.clone(), cancel)).or_insert(Bucket {
            tokens: f64::from(burst),
            last: Instant::now(),
        });
        let ok = b.take(rate, burst);
        if !ok {
            self.rate_limited.fetch_add(1, Ordering::Relaxed);
        }
        ok
    }

    pub fn liveness(&self) -> std::sync::MutexGuard<'_, Liveness> {
        self.liveness.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The liveness timer's step (C-LIVE): waiting losses again, and the
    /// strategies silent for longer than their liveness time.
    pub fn liveness_tick(&self) {
        self.liveness().tick(Instant::now(), &mut |input| push_session(&self.shared, input));
    }
}

/// Hands a loss of a strategy's link to the core (C-LIVE): control lane,
/// no reply. False when the lane is full or closed; the table then offers
/// it again at its next tick.
pub fn push_session(shared: &Shared, input: Input) -> bool {
    let lost = match &input {
        Input::Session(SessionInput::Lost { strategy, why }) => Some((strategy.to_string(), why.clone())),
        _ => None,
    };
    let taken = shared.queues.try_push(Lane::Control, CoreMsg::new(input)).is_ok();
    if let (true, Some((strategy, why))) = (taken, lost) {
        warn!("strategy link lost: its plain orders are held and its resting plain orders cancelled", "strategy": strategy, "why": why);
    }
    taken
}

/// Binds a socket file with the given mode, replacing a stale file.
pub fn bind(path: &Path, mode: u32) -> Result<UnixListener, String> {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::remove_file(path);
    let listener = UnixListener::bind(path).map_err(|e| format!("bind {}: {e}", path.display()))?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
        .map_err(|e| format!("chmod {}: {e}", path.display()))?;
    Ok(listener)
}

/// Which socket a listener serves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SocketKind {
    Trading,
    Admin,
    /// The watcher's heartbeats (C-LIVE part 2): own uid only, its own pool.
    Monitor,
}

pub async fn serve(listener: UnixListener, kind: SocketKind, ctx: Arc<ApiCtx>) {
    let admin = kind == SocketKind::Admin;
    loop {
        let (stream, _) = match listener.accept().await {
            Ok(s) => s,
            Err(e) => {
                warn!("accept failed", "error": e.to_string());
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                continue;
            }
        };
        let uid = stream.peer_cred().map(|c| c.uid()).ok();
        let allowed = match uid {
            Some(u) if u == ctx.own_uid => true,
            Some(u) => kind == SocketKind::Trading && ctx.cfg.allowed_uids.contains(&u),
            None => false,
        };
        // The admin socket has its own pool: no number of trading connections
        // can keep the operator from halting or cancelling. So has the
        // monitor socket: nothing else can keep the watcher out.
        let pool = match kind {
            SocketKind::Trading => &ctx.connections,
            SocketKind::Admin => &ctx.admin_connections,
            SocketKind::Monitor => &ctx.monitor_connections,
        };
        let Ok(permit) = pool.clone().try_acquire_owned() else {
            reject(stream, "too many connections").await;
            continue;
        };
        if !allowed {
            warn!("connection refused", "peer_uid": uid, "admin": admin, "socket": format!("{kind:?}"));
            reject(stream, "this user may not use this socket").await;
            continue;
        }
        let ctx = ctx.clone();
        tokio::spawn(async move {
            connection(stream, kind, ctx).await;
            drop(permit);
        });
    }
}

async fn reject(mut stream: UnixStream, why: &str) {
    let resp = Response::err(0, ErrorCode::Unauthorized, why);
    if let Ok(mut line) = serde_json::to_vec(&resp) {
        line.push(b'\n');
        let _ = stream.write_all(&line).await;
    }
}

enum Line {
    Text,
    Eof,
    TooLong,
}

/// Reads one line into `buf` without ever buffering more than `max` bytes of
/// it.
async fn read_line<R: AsyncBufRead + Unpin>(r: &mut R, buf: &mut Vec<u8>, max: usize) -> std::io::Result<Line> {
    buf.clear();
    loop {
        let available = r.fill_buf().await?;
        if available.is_empty() {
            return Ok(Line::Eof);
        }
        if let Some(pos) = available.iter().position(|b| *b == b'\n') {
            if buf.len() + pos > max {
                r.consume(pos + 1);
                return Ok(Line::TooLong);
            }
            buf.extend_from_slice(&available[..pos]);
            r.consume(pos + 1);
            return Ok(Line::Text);
        }
        let n = available.len();
        if buf.len() + n > max {
            return Ok(Line::TooLong);
        }
        buf.extend_from_slice(available);
        r.consume(n);
    }
}

struct Session {
    admin: bool,
    /// The monitor socket (C-LIVE part 2).
    monitor: bool,
    /// Heartbeats of this connection in the last second.
    pace: HeartbeatPace,
    hello: bool,
    strategy: Option<StrategyId>,
    out: mpsc::Sender<ServerMsg>,
    window: Arc<Semaphore>,
    closing: Arc<Notify>,
    /// The event forwarder of this connection, if it subscribed. At most one.
    subscription: Option<tokio::task::JoinHandle<()>>,
    /// The strategy this connection counts for in the liveness table: a
    /// trading connection after its `hello` (C-LIVE).
    link: Option<StrategyId>,
}

async fn connection(stream: UnixStream, kind: SocketKind, ctx: Arc<ApiCtx>) {
    let (read, mut write) = stream.into_split();
    let (out, mut out_rx) = mpsc::channel::<ServerMsg>(ctx.cfg.window * 2 + 64);
    let writer = tokio::spawn(async move {
        while let Some(msg) = out_rx.recv().await {
            let Ok(mut line) = serde_json::to_vec(&msg) else { continue };
            line.push(b'\n');
            if write.write_all(&line).await.is_err() {
                break;
            }
        }
    });
    let mut s = Session {
        admin: kind == SocketKind::Admin,
        monitor: kind == SocketKind::Monitor,
        pace: HeartbeatPace::default(),
        hello: false,
        strategy: None,
        out,
        window: Arc::new(Semaphore::new(ctx.cfg.window)),
        closing: Arc::new(Notify::new()),
        subscription: None,
        link: None,
    };
    let hello_deadline = (ctx.cfg.hello_timeout_ms > 0)
        .then(|| tokio::time::Instant::now() + std::time::Duration::from_millis(ctx.cfg.hello_timeout_ms));
    let mut reader = BufReader::with_capacity(8 * 1024, read);
    let mut buf = Vec::with_capacity(1024);
    loop {
        // Until `hello`, the connection lives on borrowed time: a socket that
        // was opened and left silent gives its slot back.
        let waiting_for_hello = async {
            match hello_deadline {
                Some(d) if !s.hello => tokio::time::sleep_until(d).await,
                _ => std::future::pending().await,
            }
        };
        let line = tokio::select! {
            r = read_line(&mut reader, &mut buf, MAX_LINE) => r,
            _ = s.closing.notified() => break,
            _ = waiting_for_hello => {
                let _ = s
                    .out
                    .send(ServerMsg::Response(Response::err(0, ErrorCode::Unauthorized, "no hello within the time limit")))
                    .await;
                break;
            }
        };
        match line {
            Ok(Line::Text) => {}
            Ok(Line::Eof) | Err(_) => break,
            Ok(Line::TooLong) => {
                let _ = s
                    .out
                    .send(ServerMsg::Response(Response::err(0, ErrorCode::BadRequest, "line too long")))
                    .await;
                break;
            }
        }
        if buf.iter().all(|b| b.is_ascii_whitespace()) {
            continue;
        }
        let req: Request = match serde_json::from_slice(&buf) {
            Ok(r) => r,
            Err(e) => {
                let id = serde_json::from_slice::<serde_json::Value>(&buf)
                    .ok()
                    .and_then(|v| v.get("id").and_then(|i| i.as_u64()))
                    .unwrap_or(0);
                let resp = Response::err(id, ErrorCode::BadRequest, e.to_string());
                if s.out.send(ServerMsg::Response(resp)).await.is_err() {
                    break;
                }
                continue;
            }
        };
        // Every parsed request is a sign of life of the strategy, `ping`
        // included (C-LIVE).
        if let Some(st) = &s.link {
            ctx.liveness().request(st, Instant::now());
        }
        // No further line is read while `window` requests are outstanding.
        let Ok(permit) = s.window.clone().acquire_owned().await else {
            break;
        };
        if let Some(resp) = dispatch(&mut s, &ctx, req, permit).await {
            if s.out.send(ServerMsg::Response(resp)).await.is_err() {
                break;
            }
        }
        if ctx.shared.fatal.is_tripped() {
            break;
        }
    }
    // The forwarder holds a sender of the output channel and waits for
    // events that may never come; end it, or the writer below never finishes
    // and the connection slot is never returned.
    if let Some(h) = s.subscription.take() {
        h.abort();
    }
    if let Some(st) = s.link.take() {
        ctx.liveness().closed(&st, &mut |input| push_session(&ctx.shared, input));
    }
    drop(s);
    let _ = writer.await;
}

/// Submits an input to the core and answers when its record is durable. The
/// waiting task holds the window permit, so outstanding requests stay
/// bounded.
fn submit(
    s: &Session,
    ctx: &Arc<ApiCtx>,
    lane: Lane,
    id: u64,
    input: Input,
    permit: tokio::sync::OwnedSemaphorePermit,
) -> Option<Response> {
    submit_then(s, ctx, lane, id, input, permit, |r| r)
}

/// Why a watcher input's `detail` is refused before the core sees it, if it
/// is: empty, or longer than `WATCHER_DETAIL_MAX` (C-HOLD). The core refuses
/// both again.
fn watcher_detail_refusal(detail: &str) -> Option<String> {
    if detail.trim().is_empty() {
        return Some("the detail is empty; nothing was recorded".into());
    }
    if detail.len() > WATCHER_DETAIL_MAX {
        return Some(format!("the detail is longer than {WATCHER_DETAIL_MAX} bytes; nothing was recorded"));
    }
    None
}

/// `submit`, with `after` applied to an operator reply once its record is
/// durable, before the answer leaves.
fn submit_then(
    s: &Session,
    ctx: &Arc<ApiCtx>,
    lane: Lane,
    id: u64,
    input: Input,
    permit: tokio::sync::OwnedSemaphorePermit,
    after: impl FnOnce(Result<String, String>) -> Result<String, String> + Send + 'static,
) -> Option<Response> {
    let (msg, rx) = CoreMsg::with_reply(input);
    if ctx.shared.queues.try_push(lane, msg).is_err() {
        ctx.refused_overload.fetch_add(1, Ordering::Relaxed);
        let (code, why) = if ctx.shared.queues.is_closed() {
            (ErrorCode::Unavailable, "the OMS is stopping")
        } else {
            (ErrorCode::Overloaded, "input queue is full; nothing was recorded")
        };
        return Some(Response::err(id, code, why));
    }
    let out = s.out.clone();
    tokio::spawn(async move {
        let resp = match rx.await {
            Ok(Reply::NewOrder(r)) => Response::ok(id, r),
            Ok(Reply::Cancel(r)) => Response::ok(id, r),
            Ok(Reply::Operator(r)) => match after(r) {
                Ok(text) => Response::ok(id, text),
                Err(text) => Response::err(id, ErrorCode::BadRequest, text),
            },
            Ok(Reply::Config(_)) => Response::err(id, ErrorCode::Internal, "unexpected reply"),
            // The reply channel was dropped: the process is stopping before
            // the outcome could be confirmed. The request may or may not be
            // recorded; repeating it with the same key is safe.
            Err(_) => Response::err(
                id,
                ErrorCode::Unavailable,
                "outcome not confirmed; repeat the request with the same key",
            ),
        };
        let _ = out.send(ServerMsg::Response(resp)).await;
        drop(permit);
    });
    None
}

fn hello_answer(ctx: &ApiCtx) -> serde_json::Value {
    let p = ctx.shared.published.read();
    serde_json::json!({
        "instance_id": format!("{:016x}", p.core.instance_id),
        "boot": p.core.boot,
        "durable_seq": p.durable_seq,
        "server_time": crate::util::wall_us(),
        "version": ctx.info.version,
    })
}

/// `hello` for a strategy on the trading socket (C-LIVE). The connection
/// counts for the strategy from here. After a loss reported since the last
/// restore, or while the core holds the strategy, the hello submits
/// `Input::Session(Hello)` and is answered only once that is durable; a
/// loss still waiting for room in the queue goes in first. Both go on the
/// control lane, so the core applies the loss before the restore. While a
/// restore is in flight, a further hello restores too, so that its answer
/// also comes after the first restore is durable. A full queue refuses the
/// hello and the connection does not count. The table's `hello` does all
/// this (C-LIVE L5).
fn trading_hello(
    s: &mut Session,
    ctx: &Arc<ApiCtx>,
    id: u64,
    strategy: StrategyId,
    client: String,
    permit: tokio::sync::OwnedSemaphorePermit,
) -> Option<Response> {
    let held = ctx.shared.published.read().core.strategy_link.contains_key(&strategy);
    let mut restored = None;
    let outcome = ctx.liveness().hello(
        &strategy,
        held,
        Instant::now(),
        s.link.as_ref(),
        &mut |input| push_session(&ctx.shared, input),
        &mut |input| {
            let (msg, rx) = CoreMsg::with_reply(input);
            let taken = ctx.shared.queues.try_push(Lane::Control, msg).is_ok();
            if taken {
                restored = Some(rx);
            }
            taken
        },
    );
    if outcome == HelloOutcome::Refused {
        ctx.refused_overload.fetch_add(1, Ordering::Relaxed);
        let (code, why) = if ctx.shared.queues.is_closed() {
            (ErrorCode::Unavailable, "the OMS is stopping")
        } else {
            (ErrorCode::Overloaded, "input queue is full; say hello again")
        };
        return Some(Response::err(id, code, why));
    }
    s.hello = true;
    s.strategy = Some(strategy.clone());
    s.link = Some(strategy.clone());
    info!("client connected", "strategy": strategy.to_string(), "admin": false, "client": client, "restore": restored.is_some());
    let Some(rx) = restored else {
        return Some(Response::ok(id, hello_answer(ctx)));
    };
    let out = s.out.clone();
    let ctx = ctx.clone();
    tokio::spawn(async move {
        let reply = rx.await;
        // Before the answer leaves: a later hello is answered at once again
        // only after this one's restore is (review 1 S1).
        ctx.liveness().restore_answered(&strategy);
        let resp = match reply {
            Ok(Reply::Operator(Ok(_))) => Response::ok(id, hello_answer(&ctx)),
            Ok(Reply::Operator(Err(text))) => Response::err(id, ErrorCode::BadRequest, text),
            Ok(_) => Response::err(id, ErrorCode::Internal, "unexpected reply"),
            Err(_) => Response::err(
                id,
                ErrorCode::Unavailable,
                "the restore of the strategy's link was not confirmed; say hello again",
            ),
        };
        let _ = out.send(ServerMsg::Response(resp)).await;
        drop(permit);
    });
    None
}

async fn dispatch(
    s: &mut Session,
    ctx: &Arc<ApiCtx>,
    req: Request,
    permit: tokio::sync::OwnedSemaphorePermit,
) -> Option<Response> {
    let id = req.id;
    if !s.hello && !matches!(req.op, Op::Hello { .. } | Op::Ping) {
        return Some(Response::err(id, ErrorCode::Unauthorized, "send hello first"));
    }
    // The monitor socket takes `hello` without a strategy, `ping` and
    // `heartbeat`; `heartbeat` is taken nowhere else (C-LIVE part 2).
    match (&req.op, s.monitor) {
        (Op::Ping, _) | (Op::Hello { strategy: None, .. }, true) | (Op::Heartbeat { .. }, true) => {}
        (Op::Hello { .. }, true) => {
            return Some(Response::err(id, ErrorCode::Unauthorized, "the monitor socket takes a hello without a strategy"));
        }
        (_, true) => {
            return Some(Response::err(id, ErrorCode::Forbidden, "the monitor socket takes only hello, ping and heartbeat"));
        }
        (Op::Heartbeat { .. }, false) => {
            return Some(Response::err(id, ErrorCode::Forbidden, "heartbeat needs the monitor socket"));
        }
        _ => {}
    }
    let operator_only = |s: &Session| -> Option<Response> {
        (!s.admin).then(|| Response::err(id, ErrorCode::Forbidden, "operator operations need the admin socket"))
    };
    let scope_of = |strategy: Option<StrategyId>| match strategy {
        Some(st) => Scope::Strategy(st),
        None => Scope::Account,
    };
    match req.op {
        Op::Ping => Some(Response::ok(id, "pong")),
        Op::Hello { strategy, client } => {
            match (&strategy, s.admin || s.monitor) {
                (None, false) => {
                    return Some(Response::err(id, ErrorCode::Unauthorized, "name a strategy on the trading socket"));
                }
                (Some(st), _) => {
                    if !ctx.shared.published.read().core.config.strategies.contains_key(st) {
                        return Some(Response::err(id, ErrorCode::Unauthorized, "unknown strategy"));
                    }
                }
                (None, true) => {}
            }
            if let (Some(st), false) = (&strategy, s.admin) {
                return trading_hello(s, ctx, id, st.clone(), client, permit);
            }
            s.hello = true;
            s.strategy = strategy.clone();
            info!("client connected", "strategy": strategy.as_ref().map(|x| x.to_string()), "admin": s.admin, "client": client);
            Some(Response::ok(id, hello_answer(ctx)))
        }
        Op::NewOrder {
            client_order_id,
            instrument,
            side,
            qty,
            px,
            tif,
            reduce_only,
            expires_at,
            market_ref,
        } => {
            let Some(strategy) = s.strategy.clone() else {
                return Some(Response::err(id, ErrorCode::Forbidden, "orders need a strategy connection"));
            };
            if !ctx.allow(&strategy, false) {
                return Some(Response::err(id, ErrorCode::Overloaded, "new-order rate limit; nothing was recorded"));
            }
            let input = Input::NewOrder(NewOrderReq {
                strategy,
                client_order_id,
                instrument,
                side,
                qty,
                px,
                tif,
                reduce_only,
                expires_at,
                market_ref,
            });
            submit(s, ctx, Lane::Orders, id, input, permit)
        }
        Op::Cancel {
            order_id,
            client_order_id,
        } => {
            let actor = match &s.strategy {
                Some(st) => Actor::Strategy(st.clone()),
                None => Actor::Operator,
            };
            let target = match (order_id, client_order_id, &s.strategy) {
                (Some(o), _, _) => CancelTarget::Order(o),
                (None, Some(c), Some(st)) => CancelTarget::Client {
                    strategy: st.clone(),
                    client_order_id: c,
                },
                _ => return Some(Response::err(id, ErrorCode::BadRequest, "name an order")),
            };
            if let Some(st) = &s.strategy {
                if !ctx.allow(st, true) {
                    return Some(Response::err(id, ErrorCode::Overloaded, "cancel rate limit; nothing was recorded"));
                }
            }
            submit(s, ctx, Lane::Control, id, Input::Cancel(CancelReq { actor, target }), permit)
        }
        Op::GetOrder {
            order_id,
            client_order_id,
        } => {
            let p = ctx.shared.published.read();
            let found = match (order_id, client_order_id, &s.strategy) {
                (Some(o), _, _) => p.core.order(o),
                (None, Some(c), Some(st)) => p.core.lookup_client(st, &c).and_then(|o| p.core.order(o)),
                _ => None,
            };
            match found {
                Some(o) if s.strategy.as_ref().is_none_or(|st| *st == o.strategy) => Some(Response::ok(id, o)),
                _ => Some(Response::err(id, ErrorCode::NotFound, "no such order")),
            }
        }
        Op::ListOpen => {
            let p = ctx.shared.published.read();
            let list: Vec<&Order> = p
                .core
                .open
                .iter()
                .filter_map(|o| p.core.order(*o))
                .filter(|o| s.strategy.as_ref().is_none_or(|st| *st == o.strategy))
                .collect();
            Some(Response::ok(id, list))
        }
        Op::Status => Some(Response::ok(id, status::status(ctx, s.strategy.as_ref()))),
        Op::Subscribe => {
            if s.subscription.as_ref().is_some_and(|h| !h.is_finished()) {
                return Some(Response::ok(id, "already subscribed"));
            }
            let mut rx = ctx.shared.published.subscribe(s.strategy.clone(), ctx.cfg.subscriber_buffer);
            let out = s.out.clone();
            let closing = s.closing.clone();
            s.subscription = Some(tokio::spawn(async move {
                while let Some(ev) = rx.recv().await {
                    if out.send(ServerMsg::Event(ev)).await.is_err() {
                        return;
                    }
                }
                // The publisher dropped this subscriber because its buffer
                // was full. Say so and end the connection; the client must
                // reconnect and query to resynchronise.
                let _ = out
                    .send(ServerMsg::Response(Response::err(
                        0,
                        ErrorCode::Overloaded,
                        "subscriber too slow: events were dropped; reconnect and query",
                    )))
                    .await;
                closing.notify_one();
            }));
            Some(Response::ok(id, "subscribed"))
        }
        Op::Halt { strategy, note } => operator_only(s).or_else(|| {
            let input = Input::Operator(OperatorCmd::Halt {
                scope: scope_of(strategy),
                note,
            });
            submit(s, ctx, Lane::Control, id, input, permit)
        }),
        Op::Resume { strategy, note } => operator_only(s).or_else(|| {
            let input = Input::Operator(OperatorCmd::Resume {
                scope: scope_of(strategy),
                note,
            });
            submit(s, ctx, Lane::Control, id, input, permit)
        }),
        Op::AckMismatch { mismatch_id, note } => operator_only(s).or_else(|| {
            let input = Input::Operator(OperatorCmd::AckMismatch { id: mismatch_id, note });
            submit(s, ctx, Lane::Control, id, input, permit)
        }),
        Op::ResolveOrder { order_id, note } => operator_only(s).or_else(|| {
            let input = Input::Operator(OperatorCmd::ResolveOrder { order_id, note });
            submit(s, ctx, Lane::Control, id, input, permit)
        }),
        Op::CancelAll { strategy, note } => operator_only(s).or_else(|| {
            let input = Input::Operator(OperatorCmd::CancelAll {
                scope: scope_of(strategy),
                note,
            });
            submit(s, ctx, Lane::Control, id, input, permit)
        }),
        Op::AdjustPosition {
            instrument,
            strategy,
            delta,
            note,
        } => operator_only(s).or_else(|| {
            let ledger = match strategy {
                Some(st) => Ledger::Strategy(st),
                None => Ledger::Unattributed,
            };
            let input = Input::Operator(OperatorCmd::AdjustPosition {
                instrument,
                ledger,
                delta,
                note,
            });
            submit(s, ctx, Lane::Control, id, input, permit)
        }),
        Op::ClearReduce { note } => operator_only(s).or_else(|| {
            let input = Input::Operator(OperatorCmd::ClearReduce { note });
            submit(s, ctx, Lane::Control, id, input, permit)
        }),
        Op::RebaseDay { note } => operator_only(s).or_else(|| {
            let input = Input::Operator(OperatorCmd::RebaseDay { note });
            submit(s, ctx, Lane::Control, id, input, permit)
        }),
        Op::ClearHold { reason, note } => operator_only(s).or_else(|| {
            let input = Input::Operator(OperatorCmd::ClearHold { reason, note });
            // The marker goes only after the clear is durable: a crash in
            // between latches the next start again (C-HOLD).
            let marker = ctx.info.kill_marker.file.clone().filter(|_| reason == HoldKind::Kill);
            submit_then(s, ctx, Lane::Control, id, input, permit, move |r| match (r, marker) {
                (Ok(text), Some(path)) => Ok(format!("{text}; {}", remove_kill_marker(&path))),
                (r, _) => r,
            })
        }),
        Op::WatcherMismatch { check, detail } => operator_only(s).or_else(|| {
            if let Some(why) = watcher_detail_refusal(&detail) {
                return Some(Response::err(id, ErrorCode::BadRequest, why));
            }
            let input = Input::Watcher(WatcherInput::Mismatch { check, detail });
            submit(s, ctx, Lane::Control, id, input, permit)
        }),
        Op::WatcherKill { detail } => operator_only(s).or_else(|| {
            if let Some(why) = watcher_detail_refusal(&detail) {
                return Some(Response::err(id, ErrorCode::BadRequest, why));
            }
            // Answered after the latch is durable: the watcher then knows
            // that the OMS sends no new order (C-HOLD).
            let input = Input::Watcher(WatcherInput::Kill { detail });
            submit(s, ctx, Lane::Control, id, input, permit)
        }),
        Op::Heartbeat { watcher, checks } => {
            // A runaway watcher cannot flood the journal: the rest of a
            // second's heartbeats are refused and recorded nowhere.
            if !s.pace.take(Instant::now()) {
                ctx.refused_overload.fetch_add(1, Ordering::Relaxed);
                return Some(Response::err(
                    id,
                    ErrorCode::Overloaded,
                    format!("more than {} heartbeats in a second; nothing was recorded", crate::monitor::HEARTBEATS_PER_SEC),
                ));
            }
            match crate::monitor::heartbeat_input(watcher, checks) {
                // Answered `recorded` after the record is durable.
                Ok(input) => submit(s, ctx, Lane::Control, id, input, permit),
                Err(why) => Some(Response::err(id, ErrorCode::BadRequest, why)),
            }
        }
        Op::Snapshot => operator_only(s).or_else(|| {
            ctx.shared.snapshot_requested.store(true, Ordering::SeqCst);
            Some(Response::ok(id, "snapshot requested"))
        }),
    }
}

/// Removes the kill marker after a durable `clear_hold kill` (C-HOLD) and
/// says how that went. A marker that is not there counts as removed.
fn remove_kill_marker(path: &Path) -> String {
    match std::fs::remove_file(path) {
        Ok(()) => "kill marker removed".to_string(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => "kill marker removed".to_string(),
        Err(e) => {
            warn!("the kill marker could not be removed: the next start latches again", "path": path.display().to_string(), "error": e.to_string());
            format!("kill marker NOT removed ({:?}): the next start latches again", e.kind())
        }
    }
}
