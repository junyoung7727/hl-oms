//! The Bybit gateway behind `VenueGateway` (ADR-028 §7c–§7f): the boot
//! checks and the IP loop, the private stream, the queries, the pause after
//! an IP ban, orders and cancels (`orders`) over WS Trade (`trade`) or
//! REST, and the reference price from the public stream (`ticker`).
//!
//! The link is up only while a stream is authenticated and subscribed and
//! `Health` has no block. Every reconnect is a new link session, so the
//! core re-reads fills, open orders and positions over the queries before
//! it trades again (C-GW, `needs_resync`). The gateway does no re-read of
//! its own.

use std::collections::HashSet;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use oms_core::*;
use tokio_tungstenite::tungstenite::Message;

use super::checks::{self, Block, Expected, Health, SymbolSpec};
use super::kill::{self, CancelAnswer, KillHost, KillTiming, KillView, NoticeContext, Report};
use super::orders::{Create, Effect, Orders};
use super::query::{Queries, QueryFail};
use super::rest::{Credentials, IP_BAN_PAUSE, Rest, RestError};
use super::ticker;
use super::trade::{self, Trade};
use super::{Endpoints, exec, stream};
use crate::config::BybitConfig;
use crate::util::wall_us;
use crate::venue::{GatewayPorts, QueryReq, VenueGateway, VenueInputTx, session_number, start};
use crate::{error, info, warn};

/// The times the gateway keeps. Tests shorten them.
#[derive(Clone, Copy, Debug)]
pub struct Timing {
    /// A ping goes out this often, and a pong must come back within the
    /// same time, or the stream is down (`ping_interval_ms`).
    pub ping: Duration,
    /// Auth and subscribe must both be confirmed within this time.
    pub handshake: Duration,
    /// Boot checks that did not pass, and a clock check that blocks, are
    /// run again this often.
    pub boot_retry: Duration,
    /// The IP check (`ip_check_secs`).
    pub ip_check: Duration,
    /// No request goes out for this long after HTTP 403.
    pub ban_pause: Duration,
    /// How often an established stream looks at `Health`.
    pub health_poll: Duration,
    /// The first wait before a reconnect, and the wait again after a link
    /// that came up.
    pub backoff_min: Duration,
    /// Longest wait between reconnects.
    pub backoff_max: Duration,
    /// A request written to WS Trade with no answer within this time is
    /// unknown (`request_timeout_ms`, as on REST).
    pub request: Duration,
    /// At most one reference price per symbol this often
    /// (`ref_price_interval_ms`).
    pub ref_price: Duration,
    /// The kill trigger: the private stream down this long after its last
    /// proof of life (`kill_after_ms`, C-KILL).
    pub kill_after: Duration,
    /// The kill loop's times (`kill_*_ms`).
    pub kill: KillTiming,
}

impl Timing {
    pub fn of(cfg: &BybitConfig) -> Timing {
        Timing {
            ping: Duration::from_millis(cfg.ping_interval_ms),
            handshake: Duration::from_secs(10),
            // Judgment: a check that failed for want of an answer is worth
            // repeating sooner than the IP loop.
            boot_retry: Duration::from_secs(10).min(Duration::from_secs(cfg.ip_check_secs)),
            ip_check: Duration::from_secs(cfg.ip_check_secs),
            ban_pause: IP_BAN_PAUSE,
            health_poll: Duration::from_millis(250),
            backoff_min: Duration::from_millis(500),
            backoff_max: Duration::from_secs(10),
            request: Duration::from_millis(cfg.request_timeout_ms),
            ref_price: Duration::from_millis(cfg.ref_price_interval_ms),
            kill_after: Duration::from_millis(cfg.kill_after_ms),
            kill: KillTiming {
                round_ms: cfg.kill_round_ms,
                watch_ms: cfg.kill_watch_ms,
                watch_every_ms: cfg.kill_watch_every_ms,
                notice_ms: cfg.kill_notice_ms,
            },
        }
    }
}

/// Where the gateway connects. Built from the network (`Endpoints`); tests
/// give loopback addresses.
#[derive(Clone, Debug)]
pub struct Wiring {
    pub rest: String,
    pub private_ws: String,
    pub trade_ws: String,
    pub public_ws: String,
    pub ip_echo_url: String,
}

impl Wiring {
    /// The endpoints of the network, and the configured IP echo service.
    pub fn of(cfg: &BybitConfig) -> Result<Wiring, String> {
        let ep = Endpoints::of(&cfg.network)
            .ok_or_else(|| format!("unknown network {:?}", cfg.network))?;
        Ok(Wiring {
            rest: ep.rest,
            private_ws: ep.private_ws,
            trade_ws: ep.trade_ws,
            public_ws: ep.public_ws,
            ip_echo_url: cfg.ip_echo_url.clone(),
        })
    }

    /// Test-only: the wiring in `text` (JSON with `rest`, `private_ws`,
    /// `trade_ws`, `public_ws` and `ip_echo_url`), refused unless every host
    /// is `127.0.0.1`: a test build never reaches the network through it.
    #[cfg(any(test, feature = "failpoints"))]
    pub fn of_test(text: &str) -> Result<Wiring, String> {
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct W {
            rest: String,
            private_ws: String,
            trade_ws: String,
            public_ws: String,
            ip_echo_url: String,
        }
        let w: W = serde_json::from_str(text).map_err(|e| format!("{TEST_WIRING}: {e}"))?;
        for (name, url) in [
            ("rest", &w.rest),
            ("private_ws", &w.private_ws),
            ("trade_ws", &w.trade_ws),
            ("public_ws", &w.public_ws),
            ("ip_echo_url", &w.ip_echo_url),
        ] {
            let host = reqwest::Url::parse(url)
                .map_err(|e| format!("{TEST_WIRING}: {name}: {e}"))?
                .host_str()
                .map(str::to_string);
            if host.as_deref() != Some("127.0.0.1") {
                return Err(format!(
                    "{TEST_WIRING}: {name} must be on 127.0.0.1, not {host:?}"
                ));
            }
        }
        Ok(Wiring {
            rest: w.rest,
            private_ws: w.private_ws,
            trade_ws: w.trade_ws,
            public_ws: w.public_ws,
            ip_echo_url: w.ip_echo_url,
        })
    }
}

/// The environment variable that replaces the network's endpoints, read
/// only by a build with the `failpoints` feature (the node tests).
#[cfg(any(test, feature = "failpoints"))]
pub const TEST_WIRING: &str = "OMS_TEST_BYBIT_WIRING";

pub(crate) struct Gateway {
    rest: Rest,
    orders: Orders,
    /// The WS Trade connection's handle, shared with `orders`.
    trade: Arc<Trade>,
    queries: Queries,
    echo: reqwest::Client,
    wiring: Wiring,
    creds: Arc<Credentials>,
    expected: Expected,
    symbols: HashSet<String>,
    recv_window_ms: u64,
    input: VenueInputTx,
    instance_id: u64,
    session: AtomicU64,
    health: Arc<Health>,
    /// The end of the pause after HTTP 403.
    banned_until: Mutex<Option<Instant>>,
    timing: Timing,
    /// The kill trigger's view of the private stream (C-KILL).
    proof: Mutex<Proof>,
    /// Set by `Kill`, cleared by `KillStop`: no create leaves meanwhile.
    kill_active: std::sync::atomic::AtomicBool,
    /// `Kill` and `KillStop` to the kill task.
    kill_tx: tokio::sync::mpsc::UnboundedSender<VenueCommand>,
    kill_rx: Mutex<Option<tokio::sync::mpsc::UnboundedReceiver<VenueCommand>>>,
    kill_view: Arc<Mutex<KillView>>,
    notice: NoticeSink,
}

/// Where the kill-loop notices go: the direct alerts, or a `warn` line
/// without `[alert]` (the node decides).
pub type NoticeSink = Arc<dyn Fn(&str) + Send + Sync>;

/// The private stream as the kill trigger sees it (C-KILL).
struct Proof {
    /// The last proof of life: the link came up, a pong, or the boot
    /// checks first passed. Moved on by the time spent waiting on the
    /// core's queue, as the pong deadline is.
    at: Option<tokio::time::Instant>,
    /// The boot checks passed once: the account is the configured one.
    armed: bool,
    link_up: bool,
    /// The trigger fired for this outage; a new proof re-arms it.
    fired: bool,
    /// Why the stream last ended, or why it does not connect.
    last_end: String,
}

/// Longest stream end reason passed to the core (journaled).
const STREAM_DETAIL_MAX: usize = 512;

fn clipped(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &text[..end])
}

pub(super) fn wall_ms() -> u64 {
    wall_us() / 1000
}

/// A client WebSocket.
pub(super) type Ws =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// Connects to `url` over IPv4, as every other request does
/// (`LOCAL_ADDRESS`). An upgrade answered with HTTP 403 is an IP ban.
pub(super) async fn connect(url: &str) -> Result<Ws, End> {
    let retry = |e: String| End::Retry(e);
    let parsed = reqwest::Url::parse(url).map_err(|e| retry(e.to_string()))?;
    let host = parsed
        .host_str()
        .ok_or_else(|| retry("the stream URL has no host".into()))?;
    let port = parsed
        .port_or_known_default()
        .ok_or_else(|| retry("the stream URL has no port".into()))?;
    let addrs: Vec<SocketAddr> = tokio::net::lookup_host((host, port))
        .await
        .map_err(|e| retry(e.to_string()))?
        .filter(SocketAddr::is_ipv4)
        .collect();
    let addr = addrs
        .first()
        .ok_or_else(|| retry("the stream host has no IPv4 address".into()))?;
    let tcp = tokio::net::TcpStream::connect(addr)
        .await
        .map_err(|e| retry(e.to_string()))?;
    match tokio_tungstenite::client_async_tls(url, tcp).await {
        Ok((ws, _)) => Ok(ws),
        Err(tokio_tungstenite::tungstenite::Error::Http(r)) if r.status().as_u16() == 403 => {
            Err(End::IpBanned("HTTP 403 on the upgrade".into()))
        }
        Err(e) => Err(retry(e.to_string())),
    }
}

fn wall_ns() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as i64)
        .unwrap_or(0)
}

/// Why a stream (the private one or WS Trade) ended. `Rejected` stops the
/// reconnects; `IpBanned` starts the pause.
pub(super) enum End {
    Retry(String),
    Rejected(String),
    IpBanned(String),
}

impl Gateway {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        cfg: &BybitConfig,
        creds: Arc<Credentials>,
        wiring: Wiring,
        symbols: Vec<SymbolSpec>,
        input: VenueInputTx,
        instance_id: u64,
        timing: Timing,
        notice: NoticeSink,
    ) -> Result<Gateway, String> {
        let timeout = Duration::from_millis(cfg.request_timeout_ms);
        let rest = || Rest::new(&wiring.rest, creds.clone(), cfg.recv_window_ms, timeout);
        let health = Arc::new(Health::default());
        // Nothing has been checked yet: the link stays down until the boot
        // checks pass.
        health.set_boot(Some(Block::Unchecked(
            "the boot checks have not run yet".into(),
        )));
        let trade = Arc::new(Trade::default());
        let (kill_tx, kill_rx) = tokio::sync::mpsc::unbounded_channel();
        Ok(Gateway {
            queries: Queries::new(rest()?, instance_id, cfg.fills_lag_ms),
            orders: Orders::new(rest()?).with_trade(trade.clone()),
            trade,
            rest: rest()?,
            echo: checks::echo_client(timeout)?,
            expected: Expected {
                sub_account_uid: cfg.sub_account_uid,
                symbols: symbols.clone(),
            },
            symbols: symbols.into_iter().map(|s| s.name).collect(),
            recv_window_ms: cfg.recv_window_ms,
            wiring,
            creds,
            input,
            instance_id,
            session: AtomicU64::new(0),
            health,
            banned_until: Mutex::new(None),
            timing,
            proof: Mutex::new(Proof {
                at: None,
                armed: false,
                link_up: false,
                fired: false,
                last_end: "the private stream has not come up since the boot checks passed".into(),
            }),
            kill_active: std::sync::atomic::AtomicBool::new(false),
            kill_tx,
            kill_rx: Mutex::new(Some(kill_rx)),
            kill_view: Arc::default(),
            notice,
        })
    }

    pub(crate) fn health(&self) -> Arc<Health> {
        self.health.clone()
    }

    // ---- the block and the pause --------------------------------------------

    /// True while no request may go out: the key was rejected, or the IP
    /// is banned. Lifts an ended ban.
    fn quiet(&self) -> bool {
        if self.health.auth_rejected() {
            return true;
        }
        let mut until = self.banned_until.lock().unwrap_or_else(|e| e.into_inner());
        match *until {
            Some(t) if Instant::now() < t => true,
            Some(_) => {
                *until = None;
                self.health.set_ban(None);
                info!("the pause after the IP ban ended; requests resume");
                false
            }
            None => false,
        }
    }

    fn ban(&self, why: String) {
        *self.banned_until.lock().unwrap_or_else(|e| e.into_inner()) =
            Some(Instant::now() + self.timing.ban_pause);
        error!("the venue banned this IP (HTTP 403); no request goes out for the pause", "pause_s": self.timing.ban_pause.as_secs(), "error": why.clone());
        self.health.set_ban(Some(Block::IpBanned(why)));
    }

    /// Records what a request's error says about the key or the IP.
    fn note(&self, e: &RestError) {
        match e {
            RestError::Auth { .. } => {
                error!("the venue rejected the key; no request goes out until the restart", "error": e.to_string());
                self.health.reject_auth(e.to_string());
            }
            RestError::IpBanned(_) => self.ban(e.to_string()),
            _ => {}
        }
    }

    /// Records a check's result in its slot. An IP ban goes to the pause.
    fn record(&self, r: Result<(), Block>, slot: fn(&Health, Option<Block>)) {
        match r {
            Err(Block::IpBanned(why)) => self.ban(why),
            Err(b) => {
                warn!("a check blocks the gateway", "block": b.to_string());
                slot(&self.health, Some(b));
            }
            Ok(()) => slot(&self.health, None),
        }
    }

    // ---- checks ---------------------------------------------------------------

    /// The boot checks until they pass, then the IP check on its timer; the
    /// clock check on both, and every `boot_retry` while it blocks.
    /// Stops for good once the key is rejected.
    async fn checks_loop(self: Arc<Self>) {
        let mut booted = false;
        let mut ip_due = tokio::time::Instant::now();
        loop {
            if self.health.auth_rejected() {
                return;
            }
            if !self.quiet() {
                // The clock first: its slot is set before a passing boot run
                // clears the boot slot, so no order leaves unchecked.
                // Before the boot checks pass a missing or slow answer
                // blocks; after, the last result stands (as for the IP).
                match checks::check_clock(&self.rest).await {
                    Err(Block::Unchecked(e)) if booted => {
                        warn!("the clock check proved nothing; the last result stands", "error": e)
                    }
                    r => self.record(r, Health::set_clock),
                }
            }
            // A ban met by the clock check ends this round.
            if !self.quiet() && (!booted || tokio::time::Instant::now() >= ip_due) {
                if booted {
                    match checks::recheck_ip(
                        &self.rest,
                        &self.echo,
                        &self.wiring.ip_echo_url,
                        &self.expected,
                    )
                    .await
                    {
                        // A missing answer proves no change: the last result
                        // stands (judgment: a flaky echo service must not
                        // take the link down every minute).
                        Err(Block::Unchecked(e)) => {
                            warn!("the IP check got no answer; the last result stands", "error": e)
                        }
                        r => self.record(r, Health::set_ip),
                    }
                } else {
                    let r = checks::boot(
                        &self.rest,
                        &self.echo,
                        &self.wiring.ip_echo_url,
                        &self.expected,
                    )
                    .await;
                    booted = r.is_ok();
                    if booted {
                        info!("the Bybit boot checks passed");
                        // The boot run read the IP too.
                        self.health.set_ip(None);
                        self.arm_kill();
                    }
                    self.record(r, Health::set_boot);
                }
                ip_due = tokio::time::Instant::now() + self.timing.ip_check;
            }
            // During a pause no check ran and `ip_due` may be past: wait the
            // short round, not zero.
            let wait = if booted && self.health.clock().is_none() && !self.quiet() {
                ip_due.saturating_duration_since(tokio::time::Instant::now())
            } else {
                self.timing.boot_retry
            };
            tokio::time::sleep(wait).await;
        }
    }

    // ---- queries ----------------------------------------------------------------

    async fn query(&self, q: QueryReq) {
        if self.quiet() {
            return;
        }
        let session = self.session.load(Ordering::SeqCst);
        let answer = self.queries.answer(q, session).await;
        for i in answer.inputs {
            if !self.input.send(i).await {
                return;
            }
        }
        match answer.failed {
            None => {}
            Some(QueryFail::Rest(e)) => {
                self.note(&e);
                warn!("a query gave no answer", "error": e.to_string());
            }
            Some(e @ QueryFail::Unreadable(_)) => {
                warn!("a query answer could not be read", "error": e.to_string())
            }
        }
    }

    // ---- the stream ---------------------------------------------------------------

    async fn stream_loop(self: Arc<Self>, boot: u32) {
        let mut connection = 0u32;
        let mut backoff = self.timing.backoff_min;
        loop {
            if self.health.auth_rejected() {
                warn!("the private stream stays closed: the key was rejected");
                return;
            }
            // Connect only when the link may come up.
            if self.quiet() || self.health.block().is_some() {
                if let Err(b) = self.allowed() {
                    self.stream_ended(&format!("not connected: {b}"));
                }
                tokio::time::sleep(self.timing.health_poll).await;
                continue;
            }
            let mut alive = false;
            let end = self.run_stream(boot, &mut connection, &mut alive).await;
            if alive {
                // The link was up and answered a ping: the waits start over.
                // A link that fails before its first pong keeps them growing,
                // so a fault that recurs on every connection does not
                // reconnect every half second.
                backoff = self.timing.backoff_min;
            }
            let why = match &end {
                End::Retry(e) | End::Rejected(e) | End::IpBanned(e) => e.clone(),
            };
            self.stream_ended(&why);
            match end {
                End::Retry(e) => warn!("the private stream ended", "error": e),
                End::Rejected(e) => {
                    error!("the private stream's auth was refused; no request goes out until the restart", "error": e.clone());
                    self.health.reject_auth(format!("private stream auth: {e}"));
                    return;
                }
                // The upgrade is an HTTP request too (§7c).
                End::IpBanned(e) => self.ban(format!("private stream upgrade: {e}")),
            }
            tokio::time::sleep(backoff).await;
            backoff = (backoff * 2).min(self.timing.backoff_max);
        }
    }

    /// One connection, from the upgrade to its end. `connection` grows only
    /// when the link came up; `alive` is set at the first pong.
    async fn run_stream(&self, boot: u32, connection: &mut u32, alive: &mut bool) -> End {
        match self.stream_once(boot, connection, alive).await {
            Ok(never) => match never {},
            Err(e) => e,
        }
    }

    async fn stream_once(
        &self,
        boot: u32,
        connection: &mut u32,
        alive: &mut bool,
    ) -> Result<std::convert::Infallible, End> {
        let ws = tokio::time::timeout(self.timing.handshake, connect(&self.wiring.private_ws))
            .await
            .map_err(|_| End::Retry("connect timed out".into()))??;
        let (mut write, mut read) = ws.split();
        let send = |m: String| Message::text(m);

        // Auth, then subscribe, each confirmed before the next (pybit does
        // not wait, F20 Q2.8; waiting tells a refused key from a refused
        // topic).
        let deadline = tokio::time::Instant::now() + self.timing.handshake;
        let expires = wall_ms() + self.recv_window_ms;
        write
            .send(send(stream::auth_message(&self.creds, expires)))
            .await
            .map_err(|e| End::Retry(format!("auth not sent: {e}")))?;
        let mut authed = false;
        loop {
            let m = tokio::time::timeout_at(deadline, read.next())
                .await
                .map_err(|_| End::Retry("auth and subscribe not confirmed in time".into()))?;
            let text = match m {
                None => return Err(End::Retry("closed during the handshake".into())),
                Some(Err(e)) => return Err(End::Retry(format!("read failed: {e}"))),
                Some(Ok(Message::Text(t))) => t,
                Some(Ok(_)) => continue,
            };
            match stream::parse(text.as_str()).map_err(End::Retry)? {
                stream::Frame::Auth(Ok(())) if !authed => {
                    authed = true;
                    write
                        .send(send(stream::subscribe_message()))
                        .await
                        .map_err(|e| End::Retry(format!("subscribe not sent: {e}")))?;
                }
                stream::Frame::Auth(Err(e)) => return Err(End::Rejected(e)),
                stream::Frame::Subscribe(Ok(())) if authed => break,
                stream::Frame::Subscribe(Err(e)) => {
                    return Err(End::Retry(format!("subscribe refused: {e}")));
                }
                _ => {}
            }
        }

        if let Some(b) = self.health.block() {
            return Err(End::Retry(format!("blocked before the link came up: {b}")));
        }
        *connection += 1;
        let session = session_number(boot, *connection);
        self.session.store(session, Ordering::SeqCst);
        self.proof_of_life(true);
        info!("the private stream is subscribed", "session": session);
        self.input
            .send(VenueInput::Link {
                up: true,
                session,
                reason: "private stream authenticated and subscribed".into(),
            })
            .await;

        let mut ping = tokio::time::interval(self.timing.ping);
        ping.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        ping.tick().await;
        let mut health = tokio::time::interval(self.timing.health_poll);
        let mut pings = 0u64;
        // When the pong to the last ping is due. Time spent waiting on the
        // core's queue moves it on: a pong read late because the gateway
        // was waiting is not missing.
        let mut pong_due: Option<tokio::time::Instant> = None;
        let reason = loop {
            let due = pong_due.unwrap_or_else(tokio::time::Instant::now);
            let msg = tokio::select! {
                // What the socket already holds is read before the deadline
                // is judged: a pong waiting in the buffer is not missing.
                biased;
                m = read.next() => match m {
                    None => break "closed by the venue".to_string(),
                    Some(Err(e)) => break format!("read failed: {e}"),
                    Some(Ok(m)) => m,
                },
                // A quiet stream and a half-open connection look the same;
                // only the pong tells them apart.
                _ = tokio::time::sleep_until(due), if pong_due.is_some() => {
                    break format!("no pong within {} ms of a ping", self.timing.ping.as_millis());
                }
                _ = ping.tick(), if pong_due.is_none() => {
                    pings += 1;
                    if let Err(e) = write.send(send(stream::ping_message(pings))).await {
                        break format!("ping failed: {e}");
                    }
                    pong_due = Some(tokio::time::Instant::now() + self.timing.ping);
                    continue;
                }
                _ = health.tick() => {
                    match self.health.block() {
                        Some(b) => break format!("blocked: {b}"),
                        None => continue,
                    }
                }
            };
            let Message::Text(text) = msg else { continue };
            let recv_ns = wall_ns();
            let frame = match stream::parse(text.as_str()) {
                Ok(f) => f,
                Err(e) => break format!("a stream message could not be read: {e}"),
            };
            let delivering = tokio::time::Instant::now();
            let delivered = match frame {
                stream::Frame::Pong => {
                    pong_due = None;
                    *alive = true;
                    self.proof_of_life(true);
                    Ok(())
                }
                stream::Frame::Orders(rows) => self.orders(&rows, session).await,
                stream::Frame::Executions(rows) => self.executions(&rows, recv_ns).await,
                stream::Frame::Auth(_) | stream::Frame::Subscribe(Err(_)) => {
                    Err("an unexpected auth or subscribe answer".to_string())
                }
                stream::Frame::Subscribe(Ok(())) | stream::Frame::Other => Ok(()),
            };
            let waited = delivering.elapsed();
            if let Some(d) = pong_due.as_mut() {
                *d += waited;
            }
            self.proof_moved_on(waited);
            if let Err(e) = delivered {
                break e;
            }
        };
        self.stream_ended(&reason);
        self.input
            .send(VenueInput::Link {
                up: false,
                session,
                reason: reason.clone(),
            })
            .await;
        Err(End::Retry(reason))
    }

    /// Whether a row is about a traded symbol. A row without a symbol
    /// cannot be placed, and ends the stream like any unreadable row.
    fn traded(&self, row: &serde_json::Value) -> Result<bool, String> {
        stream::symbol(row)
            .map(|s| self.symbols.contains(s))
            .ok_or_else(|| "a row has no symbol".to_string())
    }

    /// Delivers the readable rows; an unreadable row ends the stream (the
    /// queries after the reconnect read what it held, G-9).
    async fn orders(&self, rows: &[serde_json::Value], session: u64) -> Result<(), String> {
        for row in rows {
            if !self.traded(row)? {
                continue;
            }
            match stream::order_input(row, session, self.instance_id) {
                Ok(Some(i)) => {
                    self.input.send(i).await;
                }
                Ok(None) => {
                    warn!("an order status the OMS never produces; nothing is concluded", "status": row.get("orderStatus").cloned())
                }
                Err(e) => return Err(format!("an order row could not be read: {e}")),
            }
        }
        Ok(())
    }

    async fn executions(&self, rows: &[serde_json::Value], recv_ns: i64) -> Result<(), String> {
        for row in rows {
            if !self.traded(row)? {
                continue;
            }
            match exec::classify(row, self.instance_id, Some(recv_ns)) {
                Ok(i) => {
                    self.input.send(i).await;
                }
                Err(e) => return Err(format!("an execution row could not be read: {e}")),
            }
        }
        Ok(())
    }

    // ---- commands -------------------------------------------------------------------

    /// Whether a cancel may leave now: no block (the clock does not count).
    fn allowed(&self) -> Result<(), String> {
        if self.quiet() {
            return Err(self
                .health
                .block()
                .map_or_else(|| "requests are paused".to_string(), |b| b.to_string()));
        }
        match self.health.block() {
            Some(b) => Err(b.to_string()),
            None => Ok(()),
        }
    }

    /// Whether a new order may leave now: `allowed`, the clock, and no
    /// kill loop (C-KILL C1: a create released before the latch stays
    /// home).
    fn create_allowed(&self) -> Result<(), String> {
        self.allowed()?;
        if self.kill_active.load(Ordering::SeqCst) {
            return Err("kill switch active".into());
        }
        match self.health.create_block() {
            Some(b) => Err(b.to_string()),
            None => Ok(()),
        }
    }

    fn apply(&self, fx: Effect) {
        match fx {
            Effect::None => {}
            Effect::Ban(why) => self.ban(why),
            Effect::RejectKey(why) => {
                error!("the venue rejected the key; no request goes out until the restart", "error": why.clone());
                self.health.reject_auth(why);
            }
        }
    }

    async fn send_order(&self, cmd: VenueCommand) {
        let VenueCommand::SendOrder {
            order_id,
            instance_id,
            instrument,
            side,
            qty,
            px,
            tif,
            reduce_only,
            expires_at,
        } = cmd
        else {
            return;
        };
        let started_at = wall_us();
        let wire_id = exec::WIRE_ID.encode(VenueClientId {
            instance_id,
            order_id,
        });
        let (outcome, fx) = if self.symbols.contains(instrument.as_str()) {
            let c = Create {
                symbol: instrument.as_str(),
                side,
                qty,
                px,
                tif,
                reduce_only,
                link: &wire_id,
                expires_at,
            };
            self.orders.create(&c, || self.create_allowed()).await
        } else {
            (
                SendOutcome::NotSent {
                    reason: format!("{instrument} is not a traded symbol of this gateway"),
                },
                Effect::None,
            )
        };
        self.apply(fx);
        self.input
            .send(VenueInput::SendResult {
                order_id,
                outcome,
                timing: GatewayTiming {
                    started_at,
                    finished_at: wall_us(),
                    wire_id,
                },
            })
            .await;
    }

    async fn send_cancel(&self, cmd: VenueCommand) {
        let VenueCommand::SendCancel {
            order_id,
            instance_id,
            attempt,
            instrument,
            ..
        } = cmd
        else {
            return;
        };
        let started_at = wall_us();
        let wire_id = exec::WIRE_ID.encode(VenueClientId {
            instance_id,
            order_id,
        });
        let (outcome, fx) = self
            .orders
            .cancel(instrument.as_str(), &wire_id, attempt, || self.allowed())
            .await;
        self.apply(fx);
        self.input
            .send(VenueInput::CancelResult {
                order_id,
                attempt,
                outcome,
                timing: GatewayTiming {
                    started_at,
                    finished_at: wall_us(),
                    wire_id,
                },
            })
            .await;
    }
}

// ---- the kill switch (C-KILL) ---------------------------------------------------

impl Gateway {
    fn proof(&self) -> std::sync::MutexGuard<'_, Proof> {
        self.proof.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The boot checks passed for the first time: the account is the
    /// configured sub-account, so the trigger may fire from now on. The
    /// pass counts as a proof of life.
    fn arm_kill(&self) {
        let mut p = self.proof();
        if !p.armed {
            p.armed = true;
            p.at = Some(tokio::time::Instant::now());
            p.fired = false;
        }
    }

    /// The link came up, or a pong: a new proof of life. `up` marks the
    /// link up.
    fn proof_of_life(&self, up: bool) {
        let mut p = self.proof();
        p.at = Some(tokio::time::Instant::now());
        p.link_up = up;
        p.fired = false;
    }

    /// Time spent waiting on the core's queue does not count: a slow core
    /// is not an outage.
    fn proof_moved_on(&self, waited: Duration) {
        if let Some(at) = self.proof().at.as_mut() {
            *at += waited;
        }
    }

    fn stream_ended(&self, why: &str) {
        let mut p = self.proof();
        p.link_up = false;
        p.last_end = why.to_string();
    }

    /// The watchdog (every `health_poll`): once per outage, when the link
    /// is down and the last proof is older than `kill_after`, it sends
    /// `StreamLost`. While the link is up the stream itself ends at a
    /// missing pong, so only a down link is judged here.
    async fn kill_watch(self: Arc<Self>) {
        let mut tick = tokio::time::interval(self.timing.health_poll);
        loop {
            tick.tick().await;
            let lost = {
                let mut p = self.proof();
                match p.at {
                    Some(at) if p.armed && !p.link_up && !p.fired && at.elapsed() > self.timing.kill_after => {
                        p.fired = true;
                        let since = wall_us().saturating_sub(at.elapsed().as_micros() as u64);
                        Some((since, clipped(&p.last_end, STREAM_DETAIL_MAX)))
                    }
                    _ => None,
                }
            };
            if let Some((since, detail)) = lost {
                error!("the private stream is down past kill_after: the kill switch latches", "kill_after_ms": self.timing.kill_after.as_millis() as u64, "detail": detail.clone());
                if !self.input.send(VenueInput::StreamLost { since, detail }).await {
                    return;
                }
            }
        }
    }

    /// Runs a kill loop for each `Kill`, one at a time. A `KillStop` while
    /// idle is ignored; a `Kill` while a loop runs is ignored by the loop.
    async fn kill_task(self: Arc<Self>, mut rx: tokio::sync::mpsc::UnboundedReceiver<VenueCommand>) {
        while let Some(cmd) = rx.recv().await {
            let VenueCommand::Kill { source, note, .. } = cmd else {
                continue;
            };
            warn!("kill latched: the kill loop starts", "source": format!("{source:?}"), "note": note.clone());
            let ctx = NoticeContext {
                source,
                note,
                timing: self.timing.kill,
            };
            let clock = kill::TokioClock::new();
            kill::run(&*self, &clock, self.timing.kill, &ctx, &mut rx, &self.kill_view).await;
        }
    }

    /// `status.gateway` of the kill switch: process state.
    fn kill_status(&self) -> serde_json::Value {
        let view = self.kill_view.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let p = self.proof();
        serde_json::json!({
            "kill_loop": view,
            "kill_armed": p.armed,
            "last_proof_age_ms": p.at.map(|at| at.elapsed().as_millis() as u64),
            "source": "gateway process state",
        })
    }
}

/// The venue calls of the kill loop are the gateway's, with the same block,
/// pause and key handling as every other request.
impl KillHost for Gateway {
    fn allowed(&self) -> Result<(), String> {
        Gateway::allowed(self)
    }

    fn quiet(&self) -> bool {
        Gateway::quiet(self)
    }

    async fn cancel_all(&self) -> CancelAnswer {
        let r = super::cancel_all(&self.rest).await;
        if let Err(e) = &r {
            self.note(e);
            warn!("a kill-loop cancel-all failed", "error": e.to_string());
        }
        kill::cancel_answer(r)
    }

    async fn count(&self) -> Result<usize, String> {
        self.queries.kill_count().await.map_err(|e| {
            if let QueryFail::Rest(r) = &e {
                self.note(r);
            }
            e.to_string()
        })
    }

    async fn positions(&self) -> Result<Vec<serde_json::Value>, String> {
        self.queries.position_rows().await.map_err(|e| {
            if let QueryFail::Rest(r) = &e {
                self.note(r);
            }
            e.to_string()
        })
    }

    fn notice(&self, text: &str) {
        (self.notice)(text);
    }

    async fn ended(&self, r: Report) {
        self.input
            .send(VenueInput::KillLoopEnded {
                outcome: KillOutcome::Confirmed,
                rounds: r.rounds,
                cancel_all_calls: r.cancel_all_calls,
                detail: r.detail,
            })
            .await;
    }
}

impl VenueGateway for Gateway {
    const QUERY_CONCURRENCY: usize = 4;
    const WIRE_ID: WireIdForm = exec::WIRE_ID;

    fn send_order(&self, cmd: VenueCommand) -> impl Future<Output = ()> + Send {
        Gateway::send_order(self, cmd)
    }

    fn send_cancel(&self, cmd: VenueCommand) -> impl Future<Output = ()> + Send {
        Gateway::send_cancel(self, cmd)
    }

    fn query(&self, q: QueryReq) -> impl Future<Output = ()> + Send {
        Gateway::query(self, q)
    }

    /// C-KILL. The flag is set here, before the kill task sees the
    /// command, so that a create already released stays home.
    fn kill(&self, cmd: VenueCommand) {
        match &cmd {
            VenueCommand::Kill { .. } => self.kill_active.store(true, Ordering::SeqCst),
            VenueCommand::KillStop => self.kill_active.store(false, Ordering::SeqCst),
            VenueCommand::SendOrder { .. } | VenueCommand::SendCancel { .. } => return,
        }
        let _ = self.kill_tx.send(cmd);
    }

    fn spawn_tasks(self: &Arc<Self>, boot: u32) {
        tokio::spawn(self.clone().checks_loop());
        tokio::spawn(self.clone().stream_loop(boot));
        tokio::spawn(self.clone().kill_watch());
        let rx = self
            .kill_rx
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        if let Some(rx) = rx {
            tokio::spawn(self.clone().kill_task(rx));
        }
        let conn = trade::Conn {
            url: self.wiring.trade_ws.clone(),
            creds: self.creds.clone(),
            recv_window_ms: self.recv_window_ms,
            timing: self.timing,
        };
        tokio::spawn(trade::run(self.trade.clone(), self.clone(), conn));
        let mut symbols: Vec<String> = self.symbols.iter().cloned().collect();
        symbols.sort();
        let conn = ticker::Conn {
            url: self.wiring.public_ws.clone(),
            symbols,
            timing: self.timing,
            input: self.input.clone(),
        };
        tokio::spawn(ticker::run(self.clone(), conn));
    }
}

/// What the public stream asks of the gateway: it connects when the private
/// stream would.
impl ticker::Host for Gateway {
    fn block(&self) -> Option<String> {
        self.allowed().err()
    }

    fn ban(&self, why: String) {
        Gateway::ban(self, why);
    }
}

/// What WS Trade asks of the gateway: the same blocks, pause and key
/// rejection as every other request.
impl trade::Host for Gateway {
    fn block(&self) -> Option<String> {
        self.allowed().err()
    }

    fn key_rejected(&self) -> bool {
        self.health.auth_rejected()
    }

    fn ban(&self, why: String) {
        Gateway::ban(self, why);
    }

    fn reject_key(&self, why: String) {
        self.apply(Effect::RejectKey(why));
    }
}

/// What the status API shows of the gateway: why the link may not come up.
pub type BlockView = Arc<dyn Fn() -> Option<String> + Send + Sync>;

/// What the status API shows of the kill switch (`status.gateway`).
pub type KillStatusView = Arc<dyn Fn() -> serde_json::Value + Send + Sync>;

/// A started Bybit gateway, as the node sees it.
pub struct Started {
    pub name: String,
    pub block: BlockView,
    pub kill: KillStatusView,
}

/// Starts the gateway on the configured network. The credentials are read
/// once, here.
pub fn spawn(
    cfg: BybitConfig,
    symbols: Vec<SymbolSpec>,
    ports: GatewayPorts,
    max_concurrent: usize,
    notice: NoticeSink,
) -> Result<Started, String> {
    let wiring = Wiring::of(&cfg)?;
    // Only a build with the failpoints feature (the node tests) reads it;
    // the shipped binary has no such code.
    #[cfg(feature = "failpoints")]
    let wiring = match std::env::var(TEST_WIRING) {
        Ok(text) => {
            let w = Wiring::of_test(&text)?;
            warn!("the Bybit gateway uses the test endpoints", "wiring": format!("{w:?}"));
            w
        }
        Err(_) => wiring,
    };
    let creds = Arc::new(Credentials::load(&cfg.credentials_file)?);
    spawn_with(cfg, creds, wiring, symbols, ports, max_concurrent, None, notice)
}

/// `spawn` with given credentials and endpoints (tests: loopback).
#[allow(clippy::too_many_arguments)]
pub(crate) fn spawn_with(
    cfg: BybitConfig,
    creds: Arc<Credentials>,
    wiring: Wiring,
    symbols: Vec<SymbolSpec>,
    ports: GatewayPorts,
    max_concurrent: usize,
    timing: Option<Timing>,
    notice: NoticeSink,
) -> Result<Started, String> {
    let timing = timing.unwrap_or_else(|| Timing::of(&cfg));
    let gw = Arc::new(Gateway::new(
        &cfg,
        creds,
        wiring,
        symbols,
        ports.input.clone(),
        ports.instance_id,
        timing,
        notice,
    )?);
    info!("bybit gateway starting", "network": cfg.network.clone(), "sub_account_uid": cfg.sub_account_uid);
    let health = gw.health();
    let view: BlockView = Arc::new(move || health.create_block().map(|b| b.to_string()));
    let status = gw.clone();
    let kill: KillStatusView = Arc::new(move || status.kill_status());
    start(gw, ports, max_concurrent);
    Ok(Started {
        name: format!("bybit:{}", cfg.network),
        block: view,
        kill,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::queues::{CoreMsg, InputQueues};
    use crate::venue::bybit::orders;
    use http_fake::{Answer as Http, FakeHttp};
    use serde_json::{Value, json};
    use tokio::net::TcpListener;
    use tokio::sync::mpsc;
    use tokio_tungstenite::tungstenite::handshake::server::{ErrorResponse, Request};
    use tokio_tungstenite::tungstenite::http::StatusCode;

    const INSTANCE: u64 = 7;
    const BOOT: u32 = 3;

    fn ok(result: Value) -> Http {
        Http::Reply(
            200,
            json!({"retCode": 0, "retMsg": "OK", "result": result, "time": 1}).to_string(),
        )
    }

    fn key() -> Value {
        json!({
            "isMaster": false, "userID": 4242, "parentUid": "1000", "readOnly": 0,
            "ips": ["127.0.0.1", "203.0.113.7"],
            "permissions": {"ContractTrade": ["Order", "Position"], "Spot": []}
        })
    }

    fn positions() -> Http {
        ok(
            json!({"category": "linear", "list": [{"symbol": "BTCUSDT", "positionIdx": 0, "size": "0", "side": ""}], "nextPageCursor": ""}),
        )
    }

    /// The REST venue and the IP echo on one loopback server.
    fn rest_venue(margin_mode: &str) -> FakeHttp {
        let api = FakeHttp::start(Http::Reply(404, "{}".into()));
        api.route("/v5/user/query-api", [ok(key())]);
        api.route(
            "/v5/account/info",
            [ok(
                json!({"marginMode": margin_mode, "unifiedMarginStatus": 5, "updatedTime": "1"}),
            )],
        );
        api.route(
            "/v5/market/instruments-info",
            [ok(json!({"category": "linear", "list": [{"symbol": "BTCUSDT", "settleCoin": "USDT", "status": "Trading",
                "priceFilter": {"tickSize": "0.10"}, "lotSizeFilter": {"qtyStep": "0.001"}}], "nextPageCursor": ""}))],
        );
        api.route("/v5/position/list", [positions()]);
        // A venue clock far ahead (year 2096): the local clock is behind, so
        // the clock check passes whenever the test runs.
        api.route(
            "/v5/market/time",
            [ok(json!({"timeSecond": "4000000000", "timeNano": "4000000000000000000"}))],
        );
        api.route("/ip", [Http::Reply(200, "127.0.0.1\n".into())]);
        api
    }

    #[derive(Clone, Copy, PartialEq)]
    enum Reply {
        Ok,
        Refuse,
        Silent,
    }

    #[derive(Clone, Copy)]
    struct Mode {
        auth: Reply,
        subscribe: Reply,
        pong: bool,
        /// Answer the upgrade with HTTP 403.
        forbid: bool,
        /// Send a frame that is not JSON right after confirming subscribe
        /// (WS Trade: auth).
        garbage: bool,
        /// Speak WS Trade: auth and pong answers carry retCode, and order
        /// requests are answered as the script says.
        trade: bool,
        /// Speak the public stream: no auth, subscribe answers echo the
        /// `req_id`, and the pong is the public linear shape (`op = "ping"`,
        /// `ret_msg = "pong"`, F20 2.10).
        public: bool,
        /// The public stream answers only this many subscribe requests of a
        /// connection; the rest get no answer.
        subscribe_answers: usize,
    }

    /// How the fake WS Trade answers the next order request. With an empty
    /// script it answers retCode 0.
    #[derive(Clone, Copy, Debug)]
    enum Script {
        Code(i64),
        Silent,
        /// Close the connection after reading the request.
        Drop,
    }

    /// What one fake connection received.
    type ConnLog = Arc<Mutex<Vec<String>>>;

    enum Ctl {
        Text(String),
        Drop,
    }

    /// A loopback private stream: answers auth, subscribe and ping as the
    /// mode says, records what each connection received, and sends what a
    /// test pushes to the newest connection.
    struct FakeWs {
        url: String,
        mode: Arc<Mutex<Mode>>,
        conns: Arc<Mutex<Vec<ConnLog>>>,
        /// TCP connections, upgraded or not.
        attempts: Arc<AtomicU64>,
        /// Pings that got no pong.
        unanswered: Arc<AtomicU64>,
        /// When the last pong was written.
        last_pong: Arc<Mutex<Option<Instant>>>,
        push: Arc<Mutex<Option<mpsc::UnboundedSender<Ctl>>>>,
        script: Arc<Mutex<std::collections::VecDeque<Script>>>,
    }

    impl FakeWs {
        async fn start(mode: Mode) -> FakeWs {
            FakeWs::start_at(mode, "/v5/private").await
        }

        /// A loopback WS Trade.
        async fn start_trade(mode: Mode) -> FakeWs {
            FakeWs::start_at(Mode { trade: true, ..mode }, "/v5/trade").await
        }

        /// A loopback public stream of category linear.
        async fn start_public(mode: Mode) -> FakeWs {
            FakeWs::start_at(Mode { public: true, ..mode }, "/v5/public/linear").await
        }

        async fn start_at(mode: Mode, path: &str) -> FakeWs {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let url = format!("ws://{}{path}", listener.local_addr().unwrap());
            let fake = FakeWs {
                url,
                mode: Arc::new(Mutex::new(mode)),
                conns: Arc::default(),
                attempts: Arc::default(),
                unanswered: Arc::default(),
                last_pong: Arc::default(),
                push: Arc::default(),
                script: Arc::default(),
            };
            let (mode, conns, push) = (fake.mode.clone(), fake.conns.clone(), fake.push.clone());
            let script = fake.script.clone();
            let (attempts, unanswered) = (fake.attempts.clone(), fake.unanswered.clone());
            let last_pong = fake.last_pong.clone();
            tokio::spawn(async move {
                while let Ok((tcp, _)) = listener.accept().await {
                    attempts.fetch_add(1, Ordering::SeqCst);
                    let forbid = mode.lock().unwrap().forbid;
                    // The handshake callback's error type is tungstenite's (not ours).
                    #[allow(clippy::result_large_err)]
                    let answer = move |_: &Request, resp| {
                        if forbid {
                            let mut r = ErrorResponse::new(Some("forbidden".into()));
                            *r.status_mut() = StatusCode::FORBIDDEN;
                            return Err(r);
                        }
                        Ok(resp)
                    };
                    let Ok(ws) = tokio_tungstenite::accept_hdr_async(tcp, answer).await else {
                        continue;
                    };
                    let log: Arc<Mutex<Vec<String>>> = Arc::default();
                    conns.lock().unwrap().push(log.clone());
                    let (tx, rx) = mpsc::unbounded_channel();
                    *push.lock().unwrap() = Some(tx);
                    tokio::spawn(serve(
                        ws,
                        mode.clone(),
                        log,
                        rx,
                        unanswered.clone(),
                        script.clone(),
                        last_pong.clone(),
                    ));
                }
            });
            fake
        }

        fn set(&self, f: impl FnOnce(&mut Mode)) {
            f(&mut self.mode.lock().unwrap());
        }

        fn accepted(&self) -> usize {
            self.conns.lock().unwrap().len()
        }

        fn attempts(&self) -> u64 {
            self.attempts.load(Ordering::SeqCst)
        }

        fn unanswered(&self) -> u64 {
            self.unanswered.load(Ordering::SeqCst)
        }

        fn texts(&self, i: usize) -> Vec<String> {
            self.conns.lock().unwrap()[i].lock().unwrap().clone()
        }

        fn send(&self, text: String) {
            let _ = self
                .push
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .send(Ctl::Text(text));
        }

        fn drop_conn(&self) {
            let _ = self.push.lock().unwrap().as_ref().unwrap().send(Ctl::Drop);
        }

        fn then(&self, s: impl IntoIterator<Item = Script>) {
            self.script.lock().unwrap().extend(s);
        }

        /// Every order request received, over all connections.
        fn requests(&self, op: &str) -> Vec<Value> {
            let conns = self.conns.lock().unwrap().clone();
            conns
                .iter()
                .flat_map(|c| c.lock().unwrap().clone())
                .filter_map(|t| serde_json::from_str::<Value>(&t).ok())
                .filter(|v| v["op"] == op)
                .collect()
        }
    }

    /// The fake WS Trade's answer to an order request.
    fn trade_answer(v: &Value, code: i64, n: usize) -> Value {
        let data = if code == 0 {
            json!({"orderId": format!("o-ws-{n}"), "orderLinkId": v["args"][0]["orderLinkId"]})
        } else {
            json!({})
        };
        json!({"reqId": v["reqId"], "retCode": code, "retMsg": if code == 0 { "OK" } else { "refused" },
               "op": v["op"], "data": data, "header": {"X-Bapi-Limit": "10"}, "connId": "c"})
    }

    async fn serve(
        ws: tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>,
        mode: Arc<Mutex<Mode>>,
        log: Arc<Mutex<Vec<String>>>,
        mut ctl: mpsc::UnboundedReceiver<Ctl>,
        unanswered: Arc<AtomicU64>,
        script: Arc<Mutex<std::collections::VecDeque<Script>>>,
        last_pong: Arc<Mutex<Option<Instant>>>,
    ) {
        let (mut write, mut read) = ws.split();
        let mut subscribes = 0usize;
        loop {
            tokio::select! {
                m = read.next() => {
                    let Some(Ok(Message::Text(t))) = m else { return };
                    let n = {
                        let mut l = log.lock().unwrap();
                        l.push(t.to_string());
                        l.len()
                    };
                    let v: Value = serde_json::from_str(t.as_str()).unwrap_or_default();
                    let m = *mode.lock().unwrap();
                    let answer = if m.trade {
                        match v["op"].as_str() {
                            Some("auth") if m.auth != Reply::Silent => Some(if m.auth == Reply::Ok {
                                json!({"retCode": 0, "retMsg": "OK", "op": "auth", "connId": "c"})
                            } else {
                                json!({"retCode": 10004, "retMsg": "Invalid sign", "op": "auth", "connId": "c"})
                            }),
                            Some("ping") if m.pong => Some(json!({"reqId": v["req_id"], "retCode": 0, "retMsg": "OK", "op": "pong", "data": ["1"], "connId": "c"})),
                            Some("ping") => {
                                unanswered.fetch_add(1, Ordering::SeqCst);
                                None
                            }
                            Some(op) if op == trade::CREATE_OP || op == trade::CANCEL_OP => {
                                let next = script.lock().unwrap().pop_front();
                                match next.unwrap_or(Script::Code(0)) {
                                    Script::Code(c) => Some(trade_answer(&v, c, n)),
                                    Script::Silent => None,
                                    Script::Drop => return,
                                }
                            }
                            _ => None,
                        }
                    } else if m.public {
                        if v["op"] == "subscribe" {
                            subscribes += 1;
                        }
                        match v["op"].as_str() {
                            Some("subscribe") if m.subscribe != Reply::Silent && subscribes <= m.subscribe_answers => Some(json!({"success": m.subscribe == Reply::Ok, "ret_msg": if m.subscribe == Reply::Ok { "" } else { "error:handler not found" }, "conn_id": "c", "req_id": v["req_id"], "op": "subscribe"})),
                            Some("ping") if m.pong => Some(json!({"success": true, "ret_msg": "pong", "conn_id": "c", "req_id": "", "op": "ping"})),
                            Some("ping") => {
                                unanswered.fetch_add(1, Ordering::SeqCst);
                                None
                            }
                            _ => None,
                        }
                    } else { match v["op"].as_str() {
                        Some("auth") if m.auth != Reply::Silent => Some(json!({"success": m.auth == Reply::Ok, "ret_msg": if m.auth == Reply::Ok { "" } else { "error sign" }, "op": "auth", "conn_id": "c"})),
                        Some("subscribe") if m.subscribe != Reply::Silent => Some(json!({"success": m.subscribe == Reply::Ok, "ret_msg": "", "op": "subscribe", "conn_id": "c"})),
                        Some("ping") if m.pong => Some(json!({"req_id": v["req_id"], "op": "pong", "args": ["1"], "conn_id": "c"})),
                        Some("ping") => {
                            unanswered.fetch_add(1, Ordering::SeqCst);
                            None
                        }
                        _ => None,
                    } };
                    if let Some(a) = answer {
                        if write.send(Message::text(a.to_string())).await.is_err() {
                            return;
                        }
                        if v["op"] == "ping" {
                            *last_pong.lock().unwrap() = Some(Instant::now());
                        }
                    }
                    let confirmed = if m.trade { "auth" } else { "subscribe" };
                    let ok = if m.trade { m.auth } else { m.subscribe };
                    if v["op"] == confirmed && ok == Reply::Ok && m.garbage {
                        if write.send(Message::text("not json")).await.is_err() {
                            return;
                        }
                    }
                }
                c = ctl.recv() => match c {
                    Some(Ctl::Text(t)) => {
                        if write.send(Message::text(t)).await.is_err() {
                            return;
                        }
                    }
                    Some(Ctl::Drop) | None => return,
                },
            }
        }
    }

    const GOOD: Mode = Mode {
        auth: Reply::Ok,
        subscribe: Reply::Ok,
        pong: true,
        forbid: false,
        garbage: false,
        trade: false,
        public: false,
        subscribe_answers: usize::MAX,
    };

    fn cfg() -> BybitConfig {
        BybitConfig {
            network: "testnet".into(),
            credentials_file: "/etc/hl-oms/bybit.toml".into(),
            sub_account_uid: 4242,
            ip_echo_url: String::new(),
            ip_check_secs: 60,
            recv_window_ms: 5000,
            request_timeout_ms: 500,
            mainnet_confirmed: false,
            fills_lag_ms: 2000,
            ping_interval_ms: 5000,
            ref_price_interval_ms: 5000,
            kill_after_ms: 10_000,
            kill_round_ms: 1_000,
            kill_watch_ms: 30_000,
            kill_watch_every_ms: 5_000,
            kill_notice_ms: 60_000,
        }
    }

    fn timing() -> Timing {
        Timing {
            ping: Duration::from_millis(150),
            handshake: Duration::from_secs(1),
            boot_retry: Duration::from_millis(50),
            ip_check: Duration::from_millis(50),
            ban_pause: Duration::from_millis(600),
            health_poll: Duration::from_millis(10),
            backoff_min: Duration::from_millis(20),
            backoff_max: Duration::from_millis(50),
            request: Duration::from_millis(300),
            ref_price: Duration::from_millis(300),
            // Far: the tests before U18b2 never see the trigger.
            kill_after: Duration::from_secs(60),
            kill: KillTiming {
                round_ms: 100,
                watch_ms: 300,
                watch_every_ms: 100,
                notice_ms: 60_000,
            },
        }
    }

    /// A running gateway and what it delivered to the core.
    struct Rig {
        gw: Arc<Gateway>,
        inputs: Arc<Mutex<Vec<VenueInput>>>,
        commands: mpsc::Sender<VenueCommand>,
        queries: mpsc::Sender<QueryReq>,
        queues: Arc<InputQueues>,
        /// While set, the core takes nothing from the queues.
        hold: Arc<std::sync::atomic::AtomicBool>,
        /// Bound ports that do not listen: WS Trade or the public stream of
        /// a rig without one.
        _held: Vec<tokio::net::TcpSocket>,
        /// The kill loop's notices.
        notices: Arc<Mutex<Vec<String>>>,
    }

    impl Drop for Rig {
        fn drop(&mut self) {
            self.queues.close();
        }
    }

    impl Rig {
        fn inputs(&self) -> Vec<VenueInput> {
            self.inputs.lock().unwrap().clone()
        }

        fn links(&self) -> Vec<(bool, u64, String)> {
            self.inputs()
                .into_iter()
                .filter_map(|i| match i {
                    VenueInput::Link {
                        up,
                        session,
                        reason,
                    } => Some((up, session, reason)),
                    _ => None,
                })
                .collect()
        }

        fn block(&self) -> Option<String> {
            self.gw.health().block().map(|b| b.to_string())
        }

        async fn until(&self, what: &str, f: impl Fn(&Rig) -> bool) {
            for _ in 0..400 {
                if f(self) {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            panic!("timed out waiting for {what}; inputs {:?}", self.inputs());
        }
    }

    fn rig(api: &FakeHttp, ws: &FakeWs) -> Rig {
        rig_with(api, ws, timing(), 256)
    }

    /// A rig whose WS Trade and public stream never connect: orders go
    /// over REST.
    fn rig_with(api: &FakeHttp, ws: &FakeWs, timing: Timing, venue_lane: usize) -> Rig {
        rig_at(api, ws, None, None, timing, venue_lane)
    }

    fn rig_trade(api: &FakeHttp, ws: &FakeWs, trade: &FakeWs, timing: Timing) -> Rig {
        rig_at(api, ws, Some(trade), None, timing, 256)
    }

    /// A rig with a public stream and no WS Trade.
    fn rig_public(api: &FakeHttp, ws: &FakeWs, public: &FakeWs, timing: Timing) -> Rig {
        rig_at(api, ws, None, Some(public), timing, 256)
    }

    fn rig_at(
        api: &FakeHttp,
        ws: &FakeWs,
        trade: Option<&FakeWs>,
        public: Option<&FakeWs>,
        timing: Timing,
        venue_lane: usize,
    ) -> Rig {
        let mut ports = Vec::new();
        // Bound, not listening: the port stays ours and a connect is
        // refused.
        let mut url = |fake: Option<&FakeWs>, path: &str| match fake {
            Some(f) => f.url.clone(),
            None => {
                let s = tokio::net::TcpSocket::new_v4().unwrap();
                s.bind("127.0.0.1:0".parse().unwrap()).unwrap();
                let url = format!("ws://{}{path}", s.local_addr().unwrap());
                ports.push(s);
                url
            }
        };
        let trade_ws = url(trade, "/v5/trade");
        let public_ws = url(public, "/v5/public/linear");
        let queues = Arc::new(InputQueues::new(venue_lane, 256, 256));
        let notices: Arc<Mutex<Vec<String>>> = Arc::default();
        let wiring = Wiring {
            rest: api.url(),
            private_ws: ws.url.clone(),
            trade_ws,
            public_ws,
            ip_echo_url: format!("{}/ip", api.url()),
        };
        let gw = Arc::new(
            Gateway::new(
                &cfg(),
                Arc::new(Credentials::new("KEY", "SECRET")),
                wiring,
                vec![SymbolSpec {
                    name: "BTCUSDT".into(),
                    px_tick: Px::parse("0.1").unwrap(),
                    qty_step: Qty::parse("0.001").unwrap(),
                }],
                VenueInputTx::new(queues.clone()),
                INSTANCE,
                timing,
                {
                    let n = notices.clone();
                    Arc::new(move |t: &str| n.lock().unwrap().push(t.to_string()))
                },
            )
            .unwrap(),
        );
        let (cmd_tx, cmd_rx) = mpsc::channel(16);
        let (q_tx, q_rx) = mpsc::channel(16);
        start(
            gw.clone(),
            GatewayPorts {
                commands: cmd_rx,
                queries: q_rx,
                input: VenueInputTx::new(queues.clone()),
                instance_id: INSTANCE,
                boot: BOOT,
            },
            8,
        );
        let inputs: Arc<Mutex<Vec<VenueInput>>> = Arc::default();
        let hold: Arc<std::sync::atomic::AtomicBool> = Arc::default();
        let (q, sink, held) = (queues.clone(), inputs.clone(), hold.clone());
        std::thread::spawn(move || {
            let mut batch: Vec<CoreMsg> = Vec::new();
            loop {
                while held.load(Ordering::SeqCst) {
                    std::thread::sleep(Duration::from_millis(5));
                }
                if !q.pop_batch(1, &mut batch) {
                    return;
                }
                for m in batch.drain(..) {
                    if let Input::Venue(v) = m.input {
                        sink.lock().unwrap().push(v);
                    }
                }
            }
        });
        Rig {
            gw,
            inputs,
            commands: cmd_tx,
            queries: q_tx,
            queues,
            hold,
            _held: ports,
            notices,
        }
    }

    fn up(r: &Rig) -> bool {
        r.links().last().is_some_and(|l| l.0)
    }

    fn link(order: u64) -> String {
        exec::WIRE_ID.encode(VenueClientId {
            instance_id: INSTANCE,
            order_id: OrderId(order),
        })
    }

    fn order_msg(symbol: &str, link: &str, status: &str) -> String {
        json!({"id": "1", "topic": "order.linear", "creationTime": 1, "data": [{
            "category": "linear", "symbol": symbol, "orderId": "o-1", "orderLinkId": link,
            "side": "Buy", "orderStatus": status, "cancelType": "UNKNOWN", "rejectReason": "EC_NoError",
            "cumExecQty": "0.001", "leavesQty": "0.002", "updatedTime": "1700000000123"
        }]})
        .to_string()
    }

    fn execution(id: &str, qty: &str) -> Value {
        json!({
            "category": "linear", "symbol": "BTCUSDT", "orderId": "o-1", "orderLinkId": link(42),
            "side": "Buy", "execFee": "0.01", "execId": id, "execPrice": "30000", "execQty": qty,
            "execType": "Trade", "execTime": "1700000000456", "isMaker": true, "feeRate": "0.0001", "seq": 1
        })
    }

    fn execution_msg(rows: Vec<Value>) -> String {
        json!({"id": "2", "topic": "execution.linear", "creationTime": 1, "data": rows}).to_string()
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn the_link_comes_up_after_the_checks_auth_and_subscribe_and_stream_rows_become_inputs() {
        let api = rest_venue("REGULAR_MARGIN");
        let ws = FakeWs::start(GOOD).await;
        let before = wall_ms();
        let r = rig(&api, &ws);
        r.until("link up", up).await;
        let (_, session, _) = r.links()[0].clone();
        assert_eq!(session, session_number(BOOT, 1));
        assert_eq!(r.block(), None);

        // The clock and the boot checks ran before the stream.
        let paths: Vec<String> = api
            .requests()
            .iter()
            .map(|q| q.path().to_string())
            .collect();
        assert_eq!(
            &paths[..6],
            [
                "/v5/market/time",
                "/v5/user/query-api",
                "/ip",
                "/v5/account/info",
                "/v5/market/instruments-info",
                "/v5/position/list"
            ]
        );
        assert!(
            api.requests().iter().all(|q| q.method == "GET"),
            "read-only"
        );

        // Auth first, signed over GET/realtime + expires; then the topics.
        let sent = ws.texts(0);
        let auth: Value = serde_json::from_str(&sent[0]).unwrap();
        assert_eq!(auth["op"], "auth");
        assert_eq!(auth["args"][0], "KEY");
        let expires = auth["args"][1].as_u64().unwrap();
        assert!(
            expires >= before + 5000 && expires <= wall_ms() + 5000,
            "expires = now + recv_window"
        );
        assert_eq!(
            auth["args"][2],
            super::super::sign::ws_auth_signature("SECRET", expires)
        );
        let sub: Value = serde_json::from_str(&sent[1]).unwrap();
        assert_eq!(sub["args"], json!(["order.linear", "execution.linear"]));

        ws.send(order_msg("ETHUSDT", &link(41), "New"));
        ws.send(order_msg("BTCUSDT", &link(42), "PartiallyFilled"));
        ws.send(execution_msg(vec![execution("e-1", "0.001")]));
        r.until("a status and a fill", |r| {
            r.inputs().iter().any(|i| matches!(i, VenueInput::Fill(_)))
        })
        .await;
        let rest: Vec<VenueInput> = r.inputs().into_iter().skip(1).collect();
        assert_eq!(
            rest.len(),
            2,
            "the ETHUSDT row is not a traded symbol: {rest:?}"
        );
        let VenueInput::OrderStatus(s) = &rest[0] else {
            panic!("{rest:?}")
        };
        assert_eq!(s.order_id, Some(OrderId(42)));
        assert_eq!(s.session, session);
        assert_eq!(s.source, EvidenceSource::Stream);
        assert_eq!(s.status.status, VenueStatus::Open);
        let VenueInput::Fill(f) = &rest[1] else {
            panic!("{rest:?}")
        };
        assert_eq!(f.order_id, Some(OrderId(42)));
        assert_eq!(f.key.as_str(), "BTCUSDT:e-1");
        assert_eq!(f.is_maker, Some(true));
        assert!(
            f.recv_ns.is_some_and(|ns| ns > 0),
            "a stream fill carries its receive time"
        );

        // Pings go out and are answered: the link stays up.
        tokio::time::sleep(Duration::from_millis(500)).await;
        assert!(up(&r));
        assert!(
            ws.texts(0)
                .iter()
                .filter(|t| t.contains("\"ping\""))
                .count()
                >= 2
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn the_stream_does_not_connect_while_a_boot_check_fails() {
        let api = rest_venue("ISOLATED_MARGIN");
        let ws = FakeWs::start(GOOD).await;
        let r = rig(&api, &ws);
        r.until("the block", |r| {
            r.block().is_some_and(|b| b.starts_with("account:"))
        })
        .await;
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert_eq!(ws.accepted(), 0);
        assert!(r.links().is_empty());
        // Fixed at the venue: the next boot run passes and the link comes up.
        api.route("/v5/account/info", [ok(json!({"marginMode": "REGULAR_MARGIN", "unifiedMarginStatus": 5, "updatedTime": "1"}))]);
        r.until("link up", up).await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_missing_pong_takes_the_link_down_and_the_reconnect_is_a_new_session() {
        let api = rest_venue("REGULAR_MARGIN");
        let ws = FakeWs::start(Mode {
            pong: false,
            ..GOOD
        })
        .await;
        let r = rig(&api, &ws);
        r.until("link down", |r| r.links().len() >= 2).await;
        let links = r.links();
        assert!(links[0].0 && !links[1].0);
        assert!(links[1].2.contains("no pong"), "{links:?}");
        ws.set(|m| m.pong = true);
        r.until("a second link up", |r| {
            r.links().iter().filter(|l| l.0).count() >= 2
        })
        .await;
        let ups: Vec<u64> = r.links().iter().filter(|l| l.0).map(|l| l.1).collect();
        assert!(ups[1] > ups[0], "a new session: {ups:?}");
        assert!(ws.accepted() >= 2);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn an_unreadable_row_or_a_closed_socket_takes_the_link_down_and_it_comes_back() {
        let api = rest_venue("REGULAR_MARGIN");
        let ws = FakeWs::start(GOOD).await;
        let r = rig(&api, &ws);
        r.until("link up", up).await;
        ws.send(execution_msg(vec![
            execution("e-1", "0.001"),
            execution("e-2", "x"),
        ]));
        r.until("link down", |r| r.links().len() >= 2).await;
        let links = r.links();
        assert!(
            links[1].2.contains("execution row could not be read"),
            "{links:?}"
        );
        let fills = r
            .inputs()
            .iter()
            .filter(|i| matches!(i, VenueInput::Fill(_)))
            .count();
        assert_eq!(
            fills, 1,
            "the readable row before it is delivered, the unreadable one is not"
        );
        r.until("link up again", |r| r.links().len() >= 3 && up(r))
            .await;
        ws.drop_conn();
        r.until("down after the drop", |r| r.links().len() >= 4)
            .await;
        r.until("up after the drop", |r| r.links().len() >= 5 && up(r))
            .await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_refused_stream_auth_blocks_until_the_restart_and_nothing_is_sent_after() {
        let api = rest_venue("REGULAR_MARGIN");
        let ws = FakeWs::start(Mode {
            auth: Reply::Refuse,
            ..GOOD
        })
        .await;
        let r = rig(&api, &ws);
        r.until("auth block", |r| {
            r.block().is_some_and(|b| b.starts_with("auth_rejected:"))
        })
        .await;
        let sent = api.requests().len();
        tokio::time::sleep(Duration::from_millis(400)).await;
        assert_eq!(ws.accepted(), 1, "no second connection");
        assert_eq!(api.requests().len(), sent, "no request after the rejection");
        assert!(r.links().is_empty());
        // A query is not sent either.
        r.queries.send(QueryReq::Positions).await.unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(api.requests().len(), sent);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_refused_subscription_never_brings_the_link_up_and_is_retried() {
        let api = rest_venue("REGULAR_MARGIN");
        let ws = FakeWs::start(Mode {
            subscribe: Reply::Refuse,
            ..GOOD
        })
        .await;
        let r = rig(&api, &ws);
        for _ in 0..200 {
            if ws.accepted() >= 2 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(
            ws.accepted() >= 2,
            "a refused topic is retried, unlike a refused key"
        );
        assert!(
            r.links().is_empty(),
            "no link without the subscription: {:?}",
            r.links()
        );
        assert_eq!(r.block(), None);
        ws.set(|m| m.subscribe = Reply::Ok);
        r.until("link up", up).await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn an_ip_change_while_up_takes_the_link_down_for_good() {
        let api = rest_venue("REGULAR_MARGIN");
        let ws = FakeWs::start(GOOD).await;
        let r = rig(&api, &ws);
        r.until("link up", up).await;
        api.route("/ip", [Http::Reply(200, "198.51.100.9\n".into())]);
        r.until("link down", |r| r.links().len() >= 2).await;
        assert!(r.links()[1].2.contains("auth_rejected"), "{:?}", r.links());
        let sent = api.requests().len();
        api.route("/ip", [Http::Reply(200, "127.0.0.1\n".into())]);
        r.queries.send(QueryReq::Positions).await.unwrap();
        tokio::time::sleep(Duration::from_millis(400)).await;
        assert_eq!(r.links().len(), 2, "the right IP again does not lift it");
        assert_eq!(ws.accepted(), 1);
        assert_eq!(
            api.requests().len(),
            sent,
            "no IP check and no query after the change"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_403_on_the_stream_upgrade_pauses_every_request_then_the_link_comes_up() {
        let api = rest_venue("REGULAR_MARGIN");
        let ws = FakeWs::start(Mode {
            forbid: true,
            ..GOOD
        })
        .await;
        let r = rig(&api, &ws);
        r.until("the ban", |r| {
            r.block().is_some_and(|b| b.starts_with("ip_banned:"))
        })
        .await;
        let banned_at = Instant::now();
        ws.set(|m| m.forbid = false);
        let sent = api.requests().len();
        r.queries.send(QueryReq::Positions).await.unwrap();
        tokio::time::sleep(Duration::from_millis(400)).await;
        assert!(banned_at.elapsed() < timing().ban_pause);
        assert_eq!(ws.attempts(), 1, "no upgrade during the pause");
        assert_eq!(api.requests().len(), sent, "no request during the pause");
        assert!(r.links().is_empty());
        r.until("link up after the pause", up).await;
        assert!(banned_at.elapsed() >= Duration::from_millis(500));
        assert_eq!(r.block(), None);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn no_subscription_goes_out_before_the_auth_answer_and_a_silent_auth_is_retried() {
        let api = rest_venue("REGULAR_MARGIN");
        let ws = FakeWs::start(Mode {
            auth: Reply::Silent,
            ..GOOD
        })
        .await;
        let r = rig(&api, &ws);
        for _ in 0..300 {
            if ws.accepted() >= 2 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(ws.accepted() >= 2, "an unanswered auth is retried");
        let first = ws.texts(0);
        assert_eq!(first.len(), 1, "auth only: {first:?}");
        assert!(first[0].contains("\"auth\""));
        assert!(r.links().is_empty());
        assert_eq!(r.block(), None, "silence is not a refusal");
        ws.set(|m| m.auth = Reply::Ok);
        r.until("link up", up).await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_row_without_a_symbol_takes_the_link_down() {
        let api = rest_venue("REGULAR_MARGIN");
        let ws = FakeWs::start(GOOD).await;
        let r = rig(&api, &ws);
        r.until("link up", up).await;
        let mut msg: Value = serde_json::from_str(&order_msg("BTCUSDT", &link(42), "New")).unwrap();
        msg["data"][0].as_object_mut().unwrap().remove("symbol");
        ws.send(msg.to_string());
        r.until("link down", |r| r.links().len() >= 2).await;
        assert!(r.links()[1].2.contains("no symbol"), "{:?}", r.links());
        assert!(
            !r.inputs()
                .iter()
                .any(|i| matches!(i, VenueInput::OrderStatus(_))),
            "nothing delivered from the row"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn time_spent_waiting_on_a_full_core_queue_does_not_count_against_the_pong() {
        let api = rest_venue("REGULAR_MARGIN");
        let ws = FakeWs::start(GOOD).await;
        let r = rig_with(&api, &ws, timing(), 1);
        r.until("link up", up).await;
        // The next ping is not answered by the fake. Rows come that the
        // gateway cannot deliver while the core holds, longer than the
        // ping interval; the test answers the ping just after the hold.
        ws.set(|m| m.pong = false);
        for _ in 0..100 {
            if ws.unanswered() > 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        assert_eq!(ws.unanswered(), 1);
        r.hold.store(true, Ordering::SeqCst);
        let row: Value = serde_json::from_str::<Value>(&order_msg("BTCUSDT", &link(42), "New"))
            .unwrap()["data"][0]
            .clone();
        ws.send(
            json!({"id": "1", "topic": "order.linear", "creationTime": 1, "data": vec![row; 6]})
                .to_string(),
        );
        tokio::time::sleep(timing().ping * 3).await;
        ws.set(|m| m.pong = true);
        r.hold.store(false, Ordering::SeqCst);
        tokio::time::sleep(Duration::from_millis(20)).await;
        ws.send(json!({"req_id": "late", "op": "pong", "args": ["1"], "conn_id": "c"}).to_string());
        r.until("the rows", |r| {
            r.inputs()
                .iter()
                .filter(|i| matches!(i, VenueInput::OrderStatus(_)))
                .count()
                == 6
        })
        .await;
        tokio::time::sleep(timing().ping * 2).await;
        assert_eq!(r.links().len(), 1, "the link stays up: {:?}", r.links());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn the_reconnect_waits_start_over_after_a_link_that_came_up() {
        let api = rest_venue("REGULAR_MARGIN");
        let ws = FakeWs::start(Mode {
            subscribe: Reply::Refuse,
            ..GOOD
        })
        .await;
        let t = Timing {
            backoff_max: Duration::from_millis(1000),
            ..timing()
        };
        let r = rig_with(&api, &ws, t, 256);
        // 20, 40, … 640 ms: the next wait is the longest.
        for _ in 0..300 {
            if ws.accepted() >= 7 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(ws.accepted() >= 7);
        ws.set(|m| m.subscribe = Reply::Ok);
        r.until("link up", up).await;
        r.until("a pong", |_| {
            ws.texts(ws.accepted() - 1)
                .iter()
                .any(|t| t.contains("\"ping\""))
        })
        .await;
        tokio::time::sleep(Duration::from_millis(20)).await;
        ws.drop_conn();
        let dropped = Instant::now();
        r.until("up again", |r| r.links().len() >= 3 && up(r)).await;
        assert!(
            dropped.elapsed() < Duration::from_millis(500),
            "waited {:?} after a link that came up",
            dropped.elapsed()
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn an_ip_ban_stops_every_request_for_the_pause_then_the_link_comes_back() {
        let api = rest_venue("REGULAR_MARGIN");
        let ws = FakeWs::start(GOOD).await;
        let r = rig(&api, &ws);
        r.until("link up", up).await;
        api.route(
            "/v5/position/list",
            [Http::Reply(403, "access too frequent".into()), positions()],
        );
        r.queries.send(QueryReq::Positions).await.unwrap();
        r.until("link down", |r| r.links().len() >= 2).await;
        let banned_at = Instant::now();
        assert!(r.links()[1].2.contains("ip_banned"), "{:?}", r.links());
        let sent = api.requests().len();
        // A query during the pause is not sent.
        r.queries.send(QueryReq::OpenOrders).await.unwrap();
        tokio::time::sleep(Duration::from_millis(400)).await;
        assert!(banned_at.elapsed() < timing().ban_pause);
        assert_eq!(api.requests().len(), sent, "no request during the pause");
        assert_eq!(ws.accepted(), 1, "no stream connection during the pause");
        r.until("link up after the pause", |r| r.links().len() >= 3 && up(r))
            .await;
        assert!(banned_at.elapsed() >= Duration::from_millis(500));
        assert_eq!(r.block(), None);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_ban_met_by_the_clock_check_ends_the_round() {
        let api = rest_venue("REGULAR_MARGIN");
        let ws = FakeWs::start(GOOD).await;
        let r = rig(&api, &ws);
        r.until("link up", up).await;
        api.route("/v5/market/time", [Http::Reply(403, "access too frequent".into())]);
        r.until("the ban", |r| r.block().is_some_and(|b| b.starts_with("ip_banned:")))
            .await;
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(r.block().is_some_and(|b| b.starts_with("ip_banned:")), "still in the pause");
        let reqs = api.requests();
        let last = reqs.last().unwrap();
        assert_eq!(last.path(), "/v5/market/time", "a request followed the ban");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_key_rejected_by_a_query_takes_the_link_down_for_good() {
        let api = rest_venue("REGULAR_MARGIN");
        let ws = FakeWs::start(GOOD).await;
        let r = rig(&api, &ws);
        r.until("link up", up).await;
        api.route(
            "/v5/position/list",
            [Http::Reply(
                200,
                json!({"retCode": 33004, "retMsg": "api key expired", "result": {}, "time": 1})
                    .to_string(),
            )],
        );
        r.queries.send(QueryReq::Positions).await.unwrap();
        r.until("link down", |r| r.links().len() >= 2).await;
        assert!(r.block().is_some_and(|b| b.starts_with("auth_rejected:")));
        let sent = api.requests().len();
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert_eq!(api.requests().len(), sent, "no request after the rejection");
        assert_eq!(ws.accepted(), 1);
    }

    fn order_cmd(order: u64, symbol: &str) -> VenueCommand {
        VenueCommand::SendOrder {
            order_id: OrderId(order),
            instance_id: INSTANCE,
            instrument: InstrumentId::new(symbol).unwrap(),
            side: Side::Buy,
            qty: Qty::parse("0.001").unwrap(),
            px: Px::parse("30000").unwrap(),
            tif: Tif::Gtc,
            reduce_only: false,
            expires_at: u64::MAX,
        }
    }

    fn cancel_cmd(order: u64, symbol: &str) -> VenueCommand {
        VenueCommand::SendCancel {
            order_id: OrderId(order),
            instance_id: INSTANCE,
            attempt: 1,
            instrument: InstrumentId::new(symbol).unwrap(),
            venue_oid: None,
        }
    }

    fn results(r: &Rig) -> Vec<VenueInput> {
        r.inputs()
            .into_iter()
            .filter(|i| {
                matches!(
                    i,
                    VenueInput::SendResult { .. } | VenueInput::CancelResult { .. }
                )
            })
            .collect()
    }

    fn posts(api: &FakeHttp) -> Vec<String> {
        api.requests()
            .iter()
            .filter(|q| q.method == "POST")
            .map(|q| q.path().to_string())
            .collect()
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn an_order_and_a_cancel_each_go_out_once_over_rest_and_get_one_result() {
        let api = rest_venue("REGULAR_MARGIN");
        api.route(
            orders::CREATE_PATH,
            [ok(json!({"orderId": "o-5", "orderLinkId": link(5)}))],
        );
        api.route(
            orders::CANCEL_PATH,
            [ok(json!({"orderId": "o-5", "orderLinkId": link(5)}))],
        );
        let ws = FakeWs::start(GOOD).await;
        let r = rig(&api, &ws);
        r.until("link up", up).await;
        r.commands.send(order_cmd(5, "BTCUSDT")).await.unwrap();
        r.until("the send result", |r| results(r).len() == 1).await;
        r.commands
            .send(VenueCommand::SendCancel {
                order_id: OrderId(5),
                instance_id: INSTANCE,
                attempt: 1,
                instrument: InstrumentId::new("BTCUSDT").unwrap(),
                venue_oid: None,
            })
            .await
            .unwrap();
        r.until("two results", |r| results(r).len() == 2).await;
        tokio::time::sleep(Duration::from_millis(100)).await;
        let res = results(&r);
        assert_eq!(res.len(), 2, "{res:?}");
        assert!(
            matches!(&res[0], VenueInput::SendResult { order_id: OrderId(5), outcome: SendOutcome::Placed { venue_oid, .. }, timing }
                if venue_oid.as_str() == "o-5" && timing.wire_id == link(5) && timing.started_at <= timing.finished_at),
            "{res:?}"
        );
        assert!(
            matches!(
                &res[1],
                VenueInput::CancelResult {
                    outcome: CancelOutcome::Confirmed,
                    attempt: 1,
                    ..
                }
            ),
            "{res:?}"
        );
        assert_eq!(posts(&api), [orders::CREATE_PATH, orders::CANCEL_PATH]);
        let create = api
            .requests()
            .into_iter()
            .find(|q| q.path() == orders::CREATE_PATH)
            .unwrap();
        assert!(create.body_str().contains(&format!("\"orderLinkId\":\"{}\"", link(5))));
        assert_eq!(
            ws.texts(0)
                .iter()
                .filter(|t| !t.contains("\"ping\""))
                .count(),
            2,
            "auth and subscribe only on the private stream"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_local_clock_ahead_of_the_venue_keeps_orders_home_but_not_cancels_or_the_link() {
        let api = rest_venue("REGULAR_MARGIN");
        // The venue says 2023: the local clock is years ahead.
        api.route(
            "/v5/market/time",
            [ok(json!({"timeSecond": "1700000000", "timeNano": "1700000000000000000"}))],
        );
        api.route(orders::CREATE_PATH, [ok(json!({"orderId": "o-1"}))]);
        api.route(orders::CANCEL_PATH, [ok(json!({}))]);
        let ws = FakeWs::start(GOOD).await;
        // The IP timer is long: the clock recheck runs on its own.
        let slow_ip = Timing {
            ip_check: Duration::from_secs(60),
            ..timing()
        };
        let r = rig_with(&api, &ws, slow_ip, 256);
        r.until("the link", up).await;
        assert!(r.gw.health().clock().is_some(), "the clock blocks");
        r.commands.send(order_cmd(9, "BTCUSDT")).await.unwrap();
        r.until("one result", |r| results(r).len() == 1).await;
        assert!(matches!(&results(&r)[0], VenueInput::SendResult { outcome: SendOutcome::NotSent { reason }, .. } if reason.starts_with("clock:")));
        assert!(posts(&api).is_empty());
        r.commands.send(cancel_cmd(9, "BTCUSDT")).await.unwrap();
        r.until("the cancel result", |r| results(r).len() == 2).await;
        assert!(matches!(&results(&r)[1], VenueInput::CancelResult { outcome: CancelOutcome::Confirmed, .. }));
        assert_eq!(posts(&api), vec![orders::CANCEL_PATH.to_string()]);
        // Rechecked every boot_retry while it blocks.
        let reads = || api.requests().iter().filter(|q| q.path() == "/v5/market/time").count();
        let n = reads();
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(reads() >= n + 3, "{} → {}", n, reads());
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(up(&r), "the link stays up with the clock ahead");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn nothing_goes_out_while_blocked_or_for_an_untraded_symbol() {
        let api = rest_venue("REGULAR_MARGIN");
        api.route(
            orders::CREATE_PATH,
            [Http::Reply(403, "access too frequent".into())],
        );
        let ws = FakeWs::start(GOOD).await;
        let r = rig(&api, &ws);
        r.until("link up", up).await;
        r.commands.send(order_cmd(6, "ETHUSDT")).await.unwrap();
        r.until("one result", |r| results(r).len() == 1).await;
        assert!(matches!(&results(&r)[0], VenueInput::SendResult { outcome: SendOutcome::NotSent { .. }, .. }));
        assert!(posts(&api).is_empty(), "an untraded symbol went out");
        // A 403 on a create: the answer is unknown, and the pause starts.
        r.commands.send(order_cmd(7, "BTCUSDT")).await.unwrap();
        r.until("two results", |r| results(r).len() == 2).await;
        assert!(matches!(&results(&r)[1], VenueInput::SendResult { outcome: SendOutcome::Unknown { .. }, .. }));
        assert!(r.block().is_some_and(|b| b.starts_with("ip_banned:")), "{:?}", r.block());
        r.commands.send(order_cmd(8, "BTCUSDT")).await.unwrap();
        r.commands
            .send(VenueCommand::SendCancel {
                order_id: OrderId(7),
                instance_id: INSTANCE,
                attempt: 1,
                instrument: InstrumentId::new("BTCUSDT").unwrap(),
                venue_oid: None,
            })
            .await
            .unwrap();
        r.until("four results", |r| results(r).len() == 4).await;
        let res = results(&r);
        assert!(res[2..].iter().all(|i| matches!(i,
            VenueInput::SendResult { outcome: SendOutcome::NotSent { reason }, .. }
            | VenueInput::CancelResult { outcome: CancelOutcome::NotSent { reason }, .. } if reason.starts_with("ip_banned:"))),
            "{res:?}");
        assert_eq!(posts(&api), [orders::CREATE_PATH], "only the first create left");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_link_that_fails_before_its_first_pong_keeps_the_waits_growing() {
        let api = rest_venue("REGULAR_MARGIN");
        let ws = FakeWs::start(Mode {
            garbage: true,
            ..GOOD
        })
        .await;
        let t = Timing {
            backoff_max: Duration::from_millis(1000),
            ..timing()
        };
        let r = rig_with(&api, &ws, t, 256);
        r.until("link up", up).await;
        let first = Instant::now();
        for _ in 0..300 {
            if ws.accepted() >= 6 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(ws.accepted() >= 6);
        // Waits of 20, 40, 80, 160, 320 ms between the six links.
        assert!(
            first.elapsed() >= Duration::from_millis(500),
            "six links in {:?}: the waits started over each time",
            first.elapsed()
        );
        assert!(
            r.links()
                .iter()
                .filter(|l| !l.0)
                .all(|l| l.2.contains("could not be read")),
            "{:?}",
            r.links()
        );
    }

    // ---- WS Trade (U17b) ----------------------------------------------------------

    fn trade_up(r: &Rig) -> bool {
        r.gw.trade.is_up()
    }

    /// The REST venue with answers to both order paths.
    fn order_venue() -> FakeHttp {
        let api = rest_venue("REGULAR_MARGIN");
        api.route(
            orders::CREATE_PATH,
            [ok(json!({"orderId": "o-rest", "orderLinkId": "x"}))],
        );
        api.route(orders::CANCEL_PATH, [ok(json!({"orderId": "o-rest"}))]);
        api
    }

    fn rest_creates(api: &FakeHttp) -> usize {
        posts(api)
            .iter()
            .filter(|p| *p == orders::CREATE_PATH)
            .count()
    }

    fn ws_links(trade: &FakeWs, op: &str) -> Vec<String> {
        trade
            .requests(op)
            .iter()
            .map(|v| v["args"][0]["orderLinkId"].as_str().unwrap().to_string())
            .collect()
    }

    fn send_outcome(r: &Rig, order: u64) -> Option<SendOutcome> {
        results(r).into_iter().find_map(|i| match i {
            VenueInput::SendResult {
                order_id, outcome, ..
            } if order_id == OrderId(order) => Some(outcome),
            _ => None,
        })
    }

    async fn outcome_of(r: &Rig, order: u64) -> SendOutcome {
        r.until("the send result", |r| send_outcome(r, order).is_some())
            .await;
        send_outcome(r, order).unwrap()
    }

    async fn rig_up(api: &FakeHttp, ws: &FakeWs, trade: &FakeWs, t: Timing) -> Rig {
        let r = rig_trade(api, ws, trade, t);
        r.until("link and WS Trade up", |r| up(r) && trade_up(r))
            .await;
        r
    }

    /// Up: the create and the cancel go over WS Trade, each once, with its
    /// reqId. Down: REST takes the next create. Back up: WS Trade again.
    /// WS Trade does not move the link.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn orders_go_over_ws_trade_when_it_is_up_over_rest_while_it_is_down_and_back() {
        let api = order_venue();
        let ws = FakeWs::start(GOOD).await;
        let trade = FakeWs::start_trade(GOOD).await;
        let r = rig_up(&api, &ws, &trade, timing()).await;
        // Auth: the private stream's signature over GET/realtime + expires.
        let auth: Value = serde_json::from_str(&trade.texts(0)[0]).unwrap();
        assert_eq!(auth["op"], "auth");
        assert_eq!(auth["args"][0], "KEY");
        let expires = auth["args"][1].as_u64().unwrap();
        assert_eq!(
            auth["args"][2],
            super::super::sign::ws_auth_signature("SECRET", expires)
        );

        r.commands.send(order_cmd(1, "BTCUSDT")).await.unwrap();
        let out = outcome_of(&r, 1).await;
        assert!(
            matches!(&out, SendOutcome::Placed { venue_oid, .. } if venue_oid.as_str().starts_with("o-ws-")),
            "{out:?}"
        );
        r.commands.send(cancel_cmd(1, "BTCUSDT")).await.unwrap();
        r.until("the cancel result", |r| results(r).len() == 2).await;
        assert!(matches!(
            &results(&r)[1],
            VenueInput::CancelResult {
                outcome: CancelOutcome::Confirmed,
                ..
            }
        ));
        assert!(posts(&api).is_empty(), "REST was used: {:?}", posts(&api));
        let create = &trade.requests(trade::CREATE_OP)[0];
        assert_eq!(create["reqId"], format!("c{}", link(1)));
        assert_eq!(create["header"]["X-BAPI-RECV-WINDOW"], "5000");
        assert_eq!(create["args"][0]["orderLinkId"], link(1));
        assert_eq!(create["args"][0]["positionIdx"], 0);
        assert_eq!(
            trade.requests(trade::CANCEL_OP)[0]["reqId"],
            format!("x1{}", link(1))
        );

        // Down, and kept down: the next create goes over REST.
        trade.set(|m| m.auth = Reply::Silent);
        trade.drop_conn();
        r.until("WS Trade down", |r| !trade_up(r)).await;
        r.commands.send(order_cmd(2, "BTCUSDT")).await.unwrap();
        let out = outcome_of(&r, 2).await;
        assert!(
            matches!(&out, SendOutcome::Placed { venue_oid, .. } if venue_oid.as_str() == "o-rest"),
            "{out:?}"
        );

        // Back: WS Trade again.
        trade.set(|m| m.auth = Reply::Ok);
        r.until("WS Trade up again", trade_up).await;
        r.commands.send(order_cmd(3, "BTCUSDT")).await.unwrap();
        let out = outcome_of(&r, 3).await;
        assert!(
            matches!(&out, SendOutcome::Placed { venue_oid, .. } if venue_oid.as_str().starts_with("o-ws-")),
            "{out:?}"
        );
        assert_eq!(ws_links(&trade, trade::CREATE_OP), [link(1), link(3)]);
        assert_eq!(rest_creates(&api), 1);
        let create = api
            .requests()
            .into_iter()
            .find(|q| q.path() == orders::CREATE_PATH)
            .unwrap();
        assert!(create.body_str().contains(&link(2)));
        assert_eq!(r.links().len(), 1, "WS Trade moved the link: {:?}", r.links());
    }

    /// Requests the venue never answers are unknown after the request
    /// timeout, all of them, and none is sent again over either path.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_silent_ws_trade_makes_every_unanswered_request_unknown_and_nothing_is_sent_again() {
        let api = order_venue();
        let ws = FakeWs::start(GOOD).await;
        let trade = FakeWs::start_trade(GOOD).await;
        let r = rig_up(&api, &ws, &trade, timing()).await;
        trade.then([Script::Silent; 3]);
        let sent = Instant::now();
        for o in 1..=3 {
            r.commands.send(order_cmd(o, "BTCUSDT")).await.unwrap();
        }
        for o in 1..=3 {
            let out = outcome_of(&r, o).await;
            assert!(
                matches!(&out, SendOutcome::Unknown { reason } if reason.contains("no answer within")),
                "{o}: {out:?}"
            );
        }
        let took = sent.elapsed();
        assert!(
            took >= timing().request && took < timing().request + timing().ping,
            "unknown after {took:?}"
        );
        tokio::time::sleep(Duration::from_millis(400)).await;
        let mut sent = ws_links(&trade, trade::CREATE_OP);
        sent.sort();
        assert_eq!(sent, [link(1), link(2), link(3)], "one create each");
        assert_eq!(rest_creates(&api), 0);
        assert_eq!(results(&r).len(), 3);
        assert!(trade_up(&r), "the connection stays up: its pings are answered");
    }

    /// A WS Trade that stops answering pings goes down within two ping
    /// intervals, and the request it held is unknown then, not after the
    /// (long) request timeout. It is not sent again after the reconnect.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_ws_trade_that_stops_answering_pings_makes_its_pending_request_unknown() {
        let api = order_venue();
        let ws = FakeWs::start(GOOD).await;
        let trade = FakeWs::start_trade(GOOD).await;
        let t = Timing {
            request: Duration::from_secs(5),
            ..timing()
        };
        let r = rig_up(&api, &ws, &trade, t).await;
        trade.set(|m| m.pong = false);
        trade.then([Script::Silent]);
        let sent = Instant::now();
        r.commands.send(order_cmd(1, "BTCUSDT")).await.unwrap();
        let out = outcome_of(&r, 1).await;
        let took = sent.elapsed();
        assert!(
            matches!(&out, SendOutcome::Unknown { reason } if reason.contains("no pong")),
            "{out:?}"
        );
        assert!(took < t.ping * 3, "unknown after {took:?}");
        trade.set(|m| m.pong = true);
        r.until("a new connection", |_| trade.accepted() >= 2).await;
        r.until("WS Trade up again", trade_up).await;
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert_eq!(ws_links(&trade, trade::CREATE_OP), [link(1)]);
        assert_eq!(rest_creates(&api), 0);
    }

    /// The connection drops right after the venue read the request: unknown
    /// at once, never sent over REST.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_connection_that_drops_after_reading_the_request_makes_it_unknown() {
        let api = order_venue();
        let ws = FakeWs::start(GOOD).await;
        let trade = FakeWs::start_trade(GOOD).await;
        let t = Timing {
            request: Duration::from_secs(5),
            ..timing()
        };
        let r = rig_up(&api, &ws, &trade, t).await;
        trade.then([Script::Drop]);
        let sent = Instant::now();
        r.commands.send(order_cmd(1, "BTCUSDT")).await.unwrap();
        let out = outcome_of(&r, 1).await;
        assert!(
            matches!(&out, SendOutcome::Unknown { reason } if reason.contains("went down")),
            "{out:?}"
        );
        assert!(sent.elapsed() < Duration::from_secs(1), "{:?}", sent.elapsed());
        r.until("WS Trade up again", |r| trade.accepted() >= 2 && trade_up(r))
            .await;
        // A cancel dropped the same way is unknown too.
        trade.then([Script::Drop]);
        r.commands.send(cancel_cmd(1, "BTCUSDT")).await.unwrap();
        r.until("the cancel result", |r| results(r).len() == 2).await;
        assert!(matches!(
            &results(&r)[1],
            VenueInput::CancelResult {
                outcome: CancelOutcome::Unknown { .. },
                ..
            }
        ));
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert_eq!(ws_links(&trade, trade::CREATE_OP), [link(1)]);
        assert_eq!(ws_links(&trade, trade::CANCEL_OP), [link(1)]);
        assert!(posts(&api).is_empty(), "{:?}", posts(&api));
    }

    /// 20006 (duplicate reqId) and 10019 (WS Trade restarting) are unknown;
    /// 10019 also closes the connection, and the next create goes over the
    /// new one.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn answers_20006_and_10019_are_unknown_and_10019_reopens_the_connection() {
        let api = order_venue();
        let ws = FakeWs::start(GOOD).await;
        let trade = FakeWs::start_trade(GOOD).await;
        let r = rig_up(&api, &ws, &trade, timing()).await;
        trade.then([Script::Code(20006), Script::Code(10019)]);
        r.commands.send(order_cmd(1, "BTCUSDT")).await.unwrap();
        assert!(matches!(outcome_of(&r, 1).await, SendOutcome::Unknown { .. }));
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(trade.accepted(), 1, "20006 closed the connection");
        r.commands.send(order_cmd(2, "BTCUSDT")).await.unwrap();
        let out = outcome_of(&r, 2).await;
        assert!(
            matches!(&out, SendOutcome::Unknown { reason } if reason.contains("10019")),
            "{out:?}"
        );
        r.until("a new connection", |r| trade.accepted() >= 2 && trade_up(r))
            .await;
        r.commands.send(order_cmd(3, "BTCUSDT")).await.unwrap();
        assert!(matches!(outcome_of(&r, 3).await, SendOutcome::Placed { .. }));
        assert_eq!(trade.texts(1).iter().filter(|t| t.contains(&link(3))).count(), 1);
        assert_eq!(ws_links(&trade, trade::CREATE_OP), [link(1), link(2), link(3)]);
        assert_eq!(rest_creates(&api), 0);
        assert_eq!(r.block(), None);
    }

    /// A late answer to a request already reported unknown is dropped: no
    /// second result, and the connection stays.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_late_answer_is_dropped() {
        let api = order_venue();
        let ws = FakeWs::start(GOOD).await;
        let trade = FakeWs::start_trade(GOOD).await;
        let r = rig_up(&api, &ws, &trade, timing()).await;
        trade.then([Script::Silent]);
        r.commands.send(order_cmd(1, "BTCUSDT")).await.unwrap();
        assert!(matches!(outcome_of(&r, 1).await, SendOutcome::Unknown { .. }));
        let req = trade.requests(trade::CREATE_OP)[0].clone();
        trade.send(trade_answer(&req, 0, 99).to_string());
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert_eq!(results(&r).len(), 1, "{:?}", results(&r));
        assert!(trade_up(&r));
        assert_eq!(trade.accepted(), 1);
        r.commands.send(order_cmd(2, "BTCUSDT")).await.unwrap();
        assert!(matches!(outcome_of(&r, 2).await, SendOutcome::Placed { .. }));
    }

    /// A refused WS Trade auth rejects the key until the restart: no order
    /// leaves over either path, and WS Trade does not reconnect.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_refused_ws_trade_auth_rejects_the_key_until_the_restart() {
        let api = order_venue();
        let ws = FakeWs::start(GOOD).await;
        let trade = FakeWs::start_trade(Mode {
            auth: Reply::Refuse,
            ..GOOD
        })
        .await;
        let r = rig_trade(&api, &ws, &trade, timing());
        r.until("the key block", |r| {
            r.block().is_some_and(|b| b.starts_with("auth_rejected: WS Trade auth"))
        })
        .await;
        r.commands.send(order_cmd(1, "BTCUSDT")).await.unwrap();
        let out = outcome_of(&r, 1).await;
        assert!(
            matches!(&out, SendOutcome::NotSent { reason } if reason.starts_with("auth_rejected:")),
            "{out:?}"
        );
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert_eq!(trade.accepted(), 1, "WS Trade reconnected");
        assert!(trade.requests(trade::CREATE_OP).is_empty());
        assert!(posts(&api).is_empty());
    }

    /// 10403 (the WS IP limit) starts the ban pause: the request is unknown,
    /// WS Trade goes down, and nothing leaves during the pause.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_ws_ip_limit_answer_starts_the_ban_pause() {
        let api = order_venue();
        let ws = FakeWs::start(GOOD).await;
        let trade = FakeWs::start_trade(GOOD).await;
        let r = rig_up(&api, &ws, &trade, timing()).await;
        trade.then([Script::Code(trade::IP_LIMIT)]);
        r.commands.send(order_cmd(1, "BTCUSDT")).await.unwrap();
        assert!(matches!(outcome_of(&r, 1).await, SendOutcome::Unknown { .. }));
        assert!(r.block().is_some_and(|b| b.starts_with("ip_banned:")), "{:?}", r.block());
        r.until("WS Trade down", |r| !trade_up(r)).await;
        r.commands.send(order_cmd(2, "BTCUSDT")).await.unwrap();
        assert!(matches!(outcome_of(&r, 2).await, SendOutcome::NotSent { reason } if reason.starts_with("ip_banned:")));
        assert_eq!(ws_links(&trade, trade::CREATE_OP), [link(1)]);
        assert!(posts(&api).is_empty());
        // After the pause WS Trade comes back.
        r.until("WS Trade up after the pause", trade_up).await;
    }

    /// HTTP 403 on the WS Trade upgrade is a ban like any other.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_403_on_the_ws_trade_upgrade_pauses_every_request() {
        let api = order_venue();
        let ws = FakeWs::start(GOOD).await;
        let trade = FakeWs::start_trade(Mode {
            forbid: true,
            ..GOOD
        })
        .await;
        let r = rig_trade(&api, &ws, &trade, timing());
        r.until("the ban", |r| {
            r.block().is_some_and(|b| b.starts_with("ip_banned: WS Trade upgrade"))
        })
        .await;
        let attempts = trade.attempts();
        r.commands.send(order_cmd(1, "BTCUSDT")).await.unwrap();
        assert!(matches!(outcome_of(&r, 1).await, SendOutcome::NotSent { .. }));
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert_eq!(trade.attempts(), attempts, "a connect during the pause");
        assert!(posts(&api).is_empty());
    }

    /// The private stream's reconnect waits: they grow while connections
    /// fail before their first pong, and start over after one that answered.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn ws_trade_reconnect_waits_grow_until_a_pong_then_start_over() {
        let api = order_venue();
        let ws = FakeWs::start(GOOD).await;
        let trade = FakeWs::start_trade(Mode {
            garbage: true,
            ..GOOD
        })
        .await;
        let t = Timing {
            backoff_max: Duration::from_millis(1000),
            ..timing()
        };
        let r = rig_trade(&api, &ws, &trade, t);
        r.until("a first connection", |_| trade.accepted() >= 1).await;
        let first = Instant::now();
        r.until("six connections", |_| trade.accepted() >= 6).await;
        // Waits of 20, 40, 80, 160, 320 ms between the six.
        assert!(
            first.elapsed() >= Duration::from_millis(500),
            "six connections in {:?}",
            first.elapsed()
        );
        trade.set(|m| m.garbage = false);
        r.until("up and a ping answered", |r| {
            trade_up(r)
                && trade
                    .texts(trade.accepted() - 1)
                    .iter()
                    .any(|t| t.contains("\"ping\""))
        })
        .await;
        let n = trade.accepted();
        tokio::time::sleep(Duration::from_millis(20)).await;
        trade.drop_conn();
        let dropped = Instant::now();
        r.until("up again", |r| trade.accepted() > n && trade_up(r))
            .await;
        assert!(
            dropped.elapsed() < Duration::from_millis(500),
            "waited {:?} after a connection that answered a ping",
            dropped.elapsed()
        );
    }

    /// A host whose block the test sets.
    #[derive(Default)]
    struct TestHost {
        blocked: std::sync::atomic::AtomicBool,
        /// The bans and key rejections the task applied, in order.
        effects: Mutex<Vec<String>>,
    }

    impl trade::Host for TestHost {
        fn block(&self) -> Option<String> {
            self.blocked
                .load(Ordering::SeqCst)
                .then(|| "ip_banned: test".to_string())
        }
        fn key_rejected(&self) -> bool {
            false
        }
        /// A ban sets the block, as the gateway's does.
        fn ban(&self, why: String) {
            self.blocked.store(true, Ordering::SeqCst);
            self.effects.lock().unwrap().push(format!("ban: {why}"));
        }
        fn reject_key(&self, why: String) {
            self.blocked.store(true, Ordering::SeqCst);
            self.effects.lock().unwrap().push(format!("reject: {why}"));
        }
    }

    /// The connection task alone, with a block that only the write sees
    /// (the health poll is slow): a block set before the write keeps the
    /// request off the wire (`NotWritten`), and a reqId already pending is
    /// never written again.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn the_task_writes_nothing_under_a_block_and_no_reqid_twice() {
        let fake = FakeWs::start_trade(GOOD).await;
        let start = |host: Arc<TestHost>| {
            let t = Arc::new(Trade::default());
            let conn = trade::Conn {
                url: fake.url.clone(),
                creds: Arc::new(Credentials::new("KEY", "SECRET")),
                recv_window_ms: 5000,
                timing: Timing {
                    health_poll: Duration::from_secs(30),
                    request: Duration::from_secs(5),
                    ..timing()
                },
            };
            tokio::spawn(trade::run(t.clone(), host, conn));
            t
        };
        let host = Arc::new(TestHost::default());
        let t = start(host.clone());
        for _ in 0..200 {
            if t.is_up() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(t.is_up());
        let msg = |id: &str| {
            trade::request_message(id, trade::CREATE_OP, 1, 5000, &json!({"orderLinkId": id}))
        };

        // The same reqId while the first waits for its answer.
        fake.then([Script::Silent]);
        let first = {
            let t = t.clone();
            let m = msg("cA");
            tokio::spawn(async move { t.request("cA", m).await })
        };
        for _ in 0..200 {
            if !fake.requests(trade::CREATE_OP).is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        let again = t.request("cA", msg("cA")).await;
        assert!(
            matches!(&again, trade::Reply::NoAnswer(e) if e.contains("already pending")),
            "{again:?}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(fake.requests(trade::CREATE_OP).len(), 1, "written twice");
        assert!(!first.is_finished());

        // A block the task sees before the write.
        host.blocked.store(true, Ordering::SeqCst);
        let r = t.request("cB", msg("cB")).await;
        assert!(matches!(&r, trade::Reply::NotWritten(e) if e.starts_with("ip_banned")), "{r:?}");
        let first = first.await.unwrap();
        assert!(
            matches!(&first, trade::Reply::NoAnswer(e) if e.contains("went down")),
            "{first:?}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(fake.requests(trade::CREATE_OP).len(), 1, "written under a block");
        assert!(!t.is_up());
    }

    /// What an answer says about the IP or the key takes effect in the
    /// connection task before its next write, so the requests queued
    /// behind it stay home; also for an answer that matches no request.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn the_task_applies_a_ban_or_key_answer_before_its_next_write() {
        let fake = FakeWs::start_trade(GOOD).await;
        let start = |host: Arc<TestHost>| {
            let t = Arc::new(Trade::default());
            let conn = trade::Conn {
                url: fake.url.clone(),
                creds: Arc::new(Credentials::new("KEY", "SECRET")),
                recv_window_ms: 5000,
                timing: Timing {
                    health_poll: Duration::from_secs(30),
                    request: Duration::from_secs(5),
                    ..timing()
                },
            };
            tokio::spawn(trade::run(t.clone(), host, conn));
            t
        };
        let up = |t: Arc<Trade>| async move {
            for _ in 0..200 {
                if t.is_up() {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            panic!("WS Trade not up");
        };
        let msg = |id: &str| {
            trade::request_message(id, trade::CREATE_OP, 1, 5000, &json!({"orderLinkId": id}))
        };

        // 10403 to the first request: the ban is set when its caller sees
        // the answer, and the next request is not written.
        let host = Arc::new(TestHost::default());
        let t = start(host.clone());
        up(t.clone()).await;
        fake.then([Script::Code(trade::IP_LIMIT)]);
        let r = t.request("cA", msg("cA")).await;
        assert!(
            matches!(
                &r,
                trade::Reply::Answered {
                    code: Some(trade::IP_LIMIT),
                    ..
                }
            ),
            "{r:?}"
        );
        assert_eq!(host.effects.lock().unwrap().len(), 1, "{:?}", host.effects);
        assert!(host.effects.lock().unwrap()[0].starts_with("ban:"));
        let r = t.request("cB", msg("cB")).await;
        assert!(matches!(&r, trade::Reply::NotWritten(_)), "{r:?}");
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(
            fake.requests(trade::CREATE_OP).len(),
            1,
            "written after the ban"
        );

        // A key-rejection answer no request waits for still rejects the key.
        let host = Arc::new(TestHost::default());
        let n = fake.accepted();
        let t = start(host.clone());
        for _ in 0..200 {
            if fake.accepted() > n && t.is_up() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(t.is_up());
        fake.send(
            json!({"reqId": "late", "retCode": 10003, "retMsg": "Invalid apikey", "op": trade::CREATE_OP})
                .to_string(),
        );
        for _ in 0..200 {
            if !host.effects.lock().unwrap().is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        let fx = host.effects.lock().unwrap().clone();
        assert!(fx.len() == 1 && fx[0].starts_with("reject:"), "{fx:?}");
    }

    // ---- the node test hook (U17c) -------------------------------------------------

    /// `OMS_TEST_BYBIT_WIRING` names every endpoint, and each must be on
    /// 127.0.0.1: another host, a name, IPv6 loopback, a user part that
    /// hides the host, an unknown or a missing key are refused.
    #[test]
    fn the_test_wiring_takes_only_hosts_on_127_0_0_1() {
        let keys = ["rest", "private_ws", "trade_ws", "public_ws", "ip_echo_url"];
        let good = json!({
            "rest": "http://127.0.0.1:1",
            "private_ws": "ws://127.0.0.1:2/v5/private",
            "trade_ws": "ws://127.0.0.1:3/v5/trade",
            "public_ws": "ws://127.0.0.1:4/v5/public/linear",
            "ip_echo_url": "http://127.0.0.1:5/ip",
        });
        let w = Wiring::of_test(&good.to_string()).unwrap();
        assert_eq!(
            [w.rest, w.private_ws, w.trade_ws, w.public_ws, w.ip_echo_url],
            keys.map(|k| good[k].as_str().unwrap().to_string())
        );
        for key in keys {
            for bad in [
                "https://api.bybit.com",
                "wss://stream.bybit.com/v5/public/linear",
                "http://localhost:1",
                "http://127.0.0.2:1",
                "http://[::1]:1",
                "http://127.0.0.1@api.bybit.com/",
                "http://127.0.0.1.example:1",
                "not a url",
            ] {
                let mut v = good.clone();
                v[key] = json!(bad);
                let e = Wiring::of_test(&v.to_string()).expect_err(&format!("{key} = {bad} taken"));
                assert!(e.starts_with(TEST_WIRING), "{e}");
            }
            let mut v = good.clone();
            v.as_object_mut().unwrap().remove(key);
            assert!(Wiring::of_test(&v.to_string()).is_err(), "{key} missing");
        }
        let mut v = good.clone();
        v["extra"] = json!("http://127.0.0.1:6");
        assert!(Wiring::of_test(&v.to_string()).is_err(), "an unknown key");
        assert_eq!(TEST_WIRING, "OMS_TEST_BYBIT_WIRING");
    }

    // ---- the reference price (U17c) ----------------------------------------------

    fn ref_prices(r: &Rig) -> Vec<(String, Px, Ts)> {
        r.inputs()
            .into_iter()
            .filter_map(|i| match i {
                VenueInput::RefPrice {
                    instrument,
                    px,
                    venue_ts,
                } => Some((instrument.to_string(), px, venue_ts)),
                _ => None,
            })
            .collect()
    }

    fn px(s: &str) -> Px {
        Px::parse(s).unwrap()
    }

    fn ticker_msg(kind: &str, data: Value, ts: u64) -> String {
        let topic = format!("tickers.{}", data["symbol"].as_str().unwrap_or("BTCUSDT"));
        json!({"topic": topic, "type": kind, "data": data, "cs": 7, "ts": ts}).to_string()
    }

    /// The `n`-th public connection sent its subscribe request.
    fn subscribed(public: &FakeWs, n: usize) -> bool {
        public.accepted() > n && public.texts(n).iter().any(|t| t.contains("\"subscribe\""))
    }

    /// The newest public connection sent a ping (so it lived a ping
    /// interval).
    fn pinged(public: &FakeWs) -> bool {
        public.accepted() > 0
            && public
                .texts(public.accepted() - 1)
                .iter()
                .any(|t| t.contains("\"ping\""))
    }

    /// The snapshot's mark goes to the core at once, then at most once per
    /// interval. A delta before the snapshot gives nothing; a delta with
    /// `markPrice` moves the held mark; a delta without it gives the held
    /// mark as of its `ts`. Other symbols are ignored, nothing is sent
    /// again without a frame, and the public stream does not move the link.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn the_ticker_mark_becomes_the_reference_price_at_most_once_per_interval() {
        let api = rest_venue("REGULAR_MARGIN");
        let ws = FakeWs::start(GOOD).await;
        let public = FakeWs::start_public(GOOD).await;
        let r = rig_public(&api, &ws, &public, timing());
        r.until("subscribed", |_| subscribed(&public, 0)).await;
        let sub: Value = serde_json::from_str(&public.texts(0)[0]).unwrap();
        assert_eq!(sub["op"], "subscribe");
        assert_eq!(sub["args"], json!(["tickers.BTCUSDT"]));

        public.send(ticker_msg("delta", json!({"symbol": "BTCUSDT", "markPrice": "29999"}), 1));
        public.send(ticker_msg("snapshot", json!({"symbol": "ETHUSDT", "markPrice": "2000"}), 2));
        public.send(ticker_msg(
            "snapshot",
            json!({"symbol": "BTCUSDT", "markPrice": "30000.5", "lastPrice": "30001", "indexPrice": "29990"}),
            1_700_000_000_000,
        ));
        r.until("a reference price", |r| !ref_prices(r).is_empty()).await;
        let first = Instant::now();
        assert_eq!(
            ref_prices(&r),
            [("BTCUSDT".to_string(), px("30000.5"), 1_700_000_000_000_000)]
        );
        // Within the interval: nothing goes out, the new mark is held.
        public.send(ticker_msg("delta", json!({"symbol": "BTCUSDT", "markPrice": "30001"}), 1_700_000_000_100));
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(ref_prices(&r).len(), 1, "{:?}", ref_prices(&r));
        // After the interval, a delta without markPrice: the held mark.
        tokio::time::sleep((first + timing().ref_price + Duration::from_millis(20)).saturating_duration_since(Instant::now())).await;
        public.send(ticker_msg("delta", json!({"symbol": "BTCUSDT", "fundingRate": "0.0001"}), 1_700_000_000_400));
        r.until("a second reference price", |r| ref_prices(r).len() >= 2).await;
        assert_eq!(
            ref_prices(&r)[1],
            ("BTCUSDT".to_string(), px("30001"), 1_700_000_000_400_000)
        );
        // No timer sends the held price again.
        tokio::time::sleep(timing().ref_price * 3).await;
        assert_eq!(ref_prices(&r).len(), 2, "{:?}", ref_prices(&r));
        assert_eq!(public.accepted(), 1);
        assert_eq!(r.links().len(), 1, "the public stream moved the link: {:?}", r.links());
    }

    /// An unreadable ticker frame ends the connection. The next connection
    /// knows no mark until its own snapshot: a delta before it gives
    /// nothing, even after the interval.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn an_unreadable_ticker_frame_ends_the_connection_and_the_next_starts_with_no_mark() {
        let api = rest_venue("REGULAR_MARGIN");
        let ws = FakeWs::start(GOOD).await;
        let public = FakeWs::start_public(GOOD).await;
        let r = rig_public(&api, &ws, &public, timing());
        r.until("subscribed", |_| subscribed(&public, 0)).await;
        public.send(ticker_msg("snapshot", json!({"symbol": "BTCUSDT", "markPrice": "30000"}), 1));
        r.until("a reference price", |r| ref_prices(r).len() == 1).await;
        public.send(ticker_msg("delta", json!({"symbol": "BTCUSDT", "markPrice": "x"}), 2));
        r.until("a second connection", |_| subscribed(&public, 1)).await;
        tokio::time::sleep(timing().ref_price + Duration::from_millis(50)).await;
        public.send(ticker_msg("delta", json!({"symbol": "BTCUSDT", "fundingRate": "0.0001"}), 3));
        public.send(ticker_msg("delta", json!({"symbol": "BTCUSDT", "markPrice": "30005"}), 4));
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert_eq!(ref_prices(&r).len(), 1, "a mark before the snapshot: {:?}", ref_prices(&r));
        public.send(ticker_msg("snapshot", json!({"symbol": "BTCUSDT", "markPrice": "30010"}), 5));
        r.until("the new snapshot's price", |r| ref_prices(r).len() == 2).await;
        assert_eq!(ref_prices(&r)[1], ("BTCUSDT".to_string(), px("30010"), 5_000));
        assert_eq!(r.links().len(), 1, "{:?}", r.links());
    }

    /// A refused subscription is retried (it is not a key rejection), and
    /// the connection comes up once the venue accepts.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_refused_ticker_subscription_is_retried() {
        let api = rest_venue("REGULAR_MARGIN");
        let ws = FakeWs::start(GOOD).await;
        let public = FakeWs::start_public(Mode {
            subscribe: Reply::Refuse,
            ..GOOD
        })
        .await;
        let r = rig_public(&api, &ws, &public, timing());
        r.until("three connections", |_| public.accepted() >= 3).await;
        assert!(
            (0..3).all(|i| public.texts(i).iter().all(|t| !t.contains("\"ping\""))),
            "a refused connection lived a ping interval"
        );
        assert_eq!(r.block(), None);
        public.set(|m| m.subscribe = Reply::Ok);
        r.until("a ping on a subscribed connection", |_| pinged(&public)).await;
        let n = public.accepted();
        public.send(ticker_msg("snapshot", json!({"symbol": "BTCUSDT", "markPrice": "30000"}), 1));
        r.until("a reference price", |r| ref_prices(r).len() == 1).await;
        tokio::time::sleep(timing().ping * 3).await;
        assert_eq!(public.accepted(), n);
    }

    /// A subscribe request with no answer ends the connection at the end of
    /// the handshake time, even while pings are answered: a connection
    /// without its subscription would never bring a price.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn an_unanswered_ticker_subscription_ends_the_connection_after_the_handshake_time() {
        let api = rest_venue("REGULAR_MARGIN");
        let ws = FakeWs::start(GOOD).await;
        let public = FakeWs::start_public(Mode {
            subscribe: Reply::Silent,
            ..GOOD
        })
        .await;
        let r = rig_public(&api, &ws, &public, timing());
        r.until("the first connection", |_| public.accepted() >= 1).await;
        let first = Instant::now();
        r.until("a second connection", |_| public.accepted() >= 2).await;
        assert!(
            first.elapsed() >= timing().handshake - Duration::from_millis(100),
            "the first connection ended after {:?}, before the handshake time",
            first.elapsed()
        );
        assert!(
            public.texts(0).iter().any(|t| t.contains("\"ping\"")),
            "the first connection did not live past a ping"
        );
        assert_eq!(r.block(), None);
    }

    /// Twelve symbols: the first request of ten is confirmed, the second
    /// gets no answer. Answered pings do not keep the connection.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn one_unanswered_subscribe_request_of_two_ends_the_connection() {
        let public = FakeWs::start_public(Mode {
            subscribe_answers: 1,
            ..GOOD
        })
        .await;
        let symbols: Vec<String> = (0..12).map(|i| format!("S{i:02}USDT")).collect();
        let queues = Arc::new(InputQueues::new(16, 16, 16));
        let conn = ticker::Conn {
            url: public.url.clone(),
            symbols,
            timing: timing(),
            input: VenueInputTx::new(queues.clone()),
        };
        tokio::spawn(ticker::run(Arc::new(OpenHost), conn));
        let start = Instant::now();
        while public.accepted() < 2 && start.elapsed() < Duration::from_secs(4) {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(public.accepted() >= 2, "the connection stayed with one subscribe unanswered");
        assert_eq!(public.texts(0).iter().filter(|t| t.contains("\"subscribe\"")).count(), 2);
        assert!(public.texts(0).iter().any(|t| t.contains("\"ping\"")));
        queues.close();
    }

    /// A ticker connection that fails before its first pong keeps the
    /// reconnect waits growing, as on the private stream.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_ticker_connection_that_fails_before_its_first_pong_keeps_the_waits_growing() {
        let api = rest_venue("REGULAR_MARGIN");
        let ws = FakeWs::start(GOOD).await;
        let public = FakeWs::start_public(Mode {
            garbage: true,
            ..GOOD
        })
        .await;
        let t = Timing {
            backoff_max: Duration::from_millis(1000),
            ..timing()
        };
        let _r = rig_public(&api, &ws, &public, t);
        let start = Instant::now();
        while public.accepted() < 1 && start.elapsed() < Duration::from_secs(4) {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        let first = Instant::now();
        while public.accepted() < 6 && first.elapsed() < Duration::from_secs(4) {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        assert!(public.accepted() >= 6);
        // Waits of 20, 40, 80, 160, 320 ms between the six connections.
        assert!(
            first.elapsed() >= Duration::from_millis(500),
            "six connections in {:?}: the waits started over each time",
            first.elapsed()
        );
    }

    /// No pong within the ping interval ends the connection; the public
    /// pong shape (`op = "ping"`, `ret_msg = "pong"`) keeps it.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_missing_ticker_pong_ends_the_connection_and_the_public_pong_keeps_it() {
        let api = rest_venue("REGULAR_MARGIN");
        let ws = FakeWs::start(GOOD).await;
        let public = FakeWs::start_public(Mode {
            pong: false,
            ..GOOD
        })
        .await;
        let r = rig_public(&api, &ws, &public, timing());
        r.until("a second connection", |_| public.accepted() >= 2).await;
        assert!(public.unanswered() >= 1);
        public.set(|m| m.pong = true);
        // A second ping goes out only after the pong to the first.
        let pings = |p: &FakeWs| p.texts(p.accepted() - 1).iter().filter(|t| t.contains("\"ping\"")).count();
        r.until("a ping answered", |_| pings(&public) >= 2).await;
        let n = public.accepted();
        tokio::time::sleep(timing().ping * 4).await;
        assert_eq!(public.accepted(), n, "the public pong was not taken as one");
        assert!(pings(&public) >= 4, "{:?}", public.texts(n - 1));
        drop(r);
    }

    /// HTTP 403 on the public upgrade is a ban like any other: every
    /// request pauses, and nothing connects during the pause.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_403_on_the_public_upgrade_pauses_every_request() {
        let api = rest_venue("REGULAR_MARGIN");
        let ws = FakeWs::start(GOOD).await;
        let public = FakeWs::start_public(Mode {
            forbid: true,
            ..GOOD
        })
        .await;
        let r = rig_public(&api, &ws, &public, timing());
        r.until("the ban", |r| {
            r.block().is_some_and(|b| b.starts_with("ip_banned: public stream upgrade"))
        })
        .await;
        let (attempts, sent) = (public.attempts(), api.requests().len());
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert_eq!(public.attempts(), attempts, "a connect during the pause");
        assert_eq!(api.requests().len(), sent, "a request during the pause");
    }

    /// The public stream connects only when the private stream would: not
    /// while a boot check fails, and a block while it is up takes it down
    /// and keeps it down.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn the_ticker_stream_connects_only_without_a_block() {
        let api = rest_venue("ISOLATED_MARGIN");
        let ws = FakeWs::start(GOOD).await;
        let public = FakeWs::start_public(GOOD).await;
        let r = rig_public(&api, &ws, &public, timing());
        r.until("the block", |r| r.block().is_some_and(|b| b.starts_with("account:")))
            .await;
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert_eq!(public.attempts(), 0);
        api.route("/v5/account/info", [ok(json!({"marginMode": "REGULAR_MARGIN", "unifiedMarginStatus": 5, "updatedTime": "1"}))]);
        r.until("subscribed", |_| subscribed(&public, 0)).await;
        // The IP changes: the key is rejected until the restart.
        api.route("/ip", [Http::Reply(200, "198.51.100.9\n".into())]);
        r.until("the key block", |r| r.block().is_some_and(|b| b.starts_with("auth_rejected:")))
            .await;
        tokio::time::sleep(Duration::from_millis(100)).await;
        public.send(ticker_msg("snapshot", json!({"symbol": "BTCUSDT", "markPrice": "30000"}), 1));
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(ref_prices(&r).is_empty(), "read under a block: {:?}", ref_prices(&r));
        assert_eq!(public.accepted(), 1);
    }

    /// A host that never blocks.
    struct OpenHost;

    impl ticker::Host for OpenHost {
        fn block(&self) -> Option<String> {
            None
        }
        fn ban(&self, _: String) {}
    }

    /// Twelve symbols: two subscribe requests of ten and two topics, both
    /// confirmed, and the connection stays.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn the_ticker_stream_subscribes_ten_topics_per_request() {
        let public = FakeWs::start_public(GOOD).await;
        let symbols: Vec<String> = (0..12).map(|i| format!("S{i:02}USDT")).collect();
        let queues = Arc::new(InputQueues::new(16, 16, 16));
        let conn = ticker::Conn {
            url: public.url.clone(),
            symbols: symbols.clone(),
            timing: timing(),
            input: VenueInputTx::new(queues.clone()),
        };
        tokio::spawn(ticker::run(Arc::new(OpenHost), conn));
        for _ in 0..200 {
            if pinged(&public) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(pinged(&public), "{:?}", public.accepted());
        let subs: Vec<Value> = public
            .requests("subscribe")
            .into_iter()
            .collect();
        let args: Vec<Vec<String>> = subs
            .iter()
            .map(|m| m["args"].as_array().unwrap().iter().map(|a| a.as_str().unwrap().to_string()).collect())
            .collect();
        let want: Vec<String> = symbols.iter().map(|s| format!("tickers.{s}")).collect();
        assert_eq!(args, [want[..10].to_vec(), want[10..].to_vec()]);
        assert_eq!(public.accepted(), 1);
        queues.close();
    }

    // ---- the kill switch (U18b2, C-KILL) ------------------------------------------

    fn kill_after(ms: u64) -> Timing {
        Timing {
            kill_after: Duration::from_millis(ms),
            ..timing()
        }
    }

    fn stream_lost(r: &Rig) -> Vec<(Ts, String)> {
        r.inputs()
            .into_iter()
            .filter_map(|i| match i {
                VenueInput::StreamLost { since, detail } => Some((since, detail)),
                _ => None,
            })
            .collect()
    }

    /// The first time `f` holds, polled every 2 ms, within `limit`.
    async fn when(r: &Rig, limit: Duration, f: impl Fn(&Rig) -> bool) -> Option<Instant> {
        let end = Instant::now() + limit;
        while Instant::now() < end {
            if f(r) {
                return Some(Instant::now());
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        None
    }

    /// The network goes half-open: no pong, and no reconnect comes up (the
    /// fake answers nothing). Returns the time of the last pong.
    async fn go_half_open(ws: &FakeWs) -> Instant {
        ws.set(|m| {
            m.pong = false;
            m.auth = Reply::Silent;
        });
        for _ in 0..200 {
            if ws.unanswered() > 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        assert!(ws.unanswered() > 0, "no ping went unanswered");
        ws.last_pong.lock().unwrap().expect("a pong before")
    }

    fn kill_cmd() -> VenueCommand {
        VenueCommand::Kill {
            since: 1,
            source: HoldSource::Stream,
            note: "private stream down".into(),
        }
    }

    fn ended(r: &Rig) -> Vec<VenueInput> {
        r.inputs()
            .into_iter()
            .filter(|i| matches!(i, VenueInput::KillLoopEnded { .. }))
            .collect()
    }

    fn cancel_alls(api: &FakeHttp) -> usize {
        posts(api).iter().filter(|p| *p == "/v5/order/cancel-all").count()
    }

    /// The REST venue with cancel-all, the open-order view (`open` orders
    /// open) and the order paths.
    fn kill_venue(open: usize) -> FakeHttp {
        let api = order_venue();
        api.route(
            "/v5/order/cancel-all",
            [ok(json!({"list": [{"orderId": "o1", "orderLinkId": ""}], "success": "1"}))],
        );
        let rows: Vec<Value> = (0..open)
            .map(|i| json!({"orderId": format!("o{i}"), "orderLinkId": "", "symbol": "BTCUSDT", "orderStatus": "New"}))
            .collect();
        api.route(
            "/v5/order/realtime",
            [ok(json!({"category": "linear", "list": rows, "nextPageCursor": ""}))],
        );
        api
    }

    /// Done criterion (gateway part): the private stream goes half-open.
    /// The link goes down at the missing pong; `StreamLost` comes once,
    /// `kill_after` after the last pong and not before, with the wall time
    /// of that pong and the stream's end reason.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn the_kill_fires_kill_after_the_last_pong_and_not_before() {
        let api = rest_venue("REGULAR_MARGIN");
        let ws = FakeWs::start(GOOD).await;
        let r = rig_with(&api, &ws, kill_after(1_000), 256);
        r.until("link up", up).await;
        tokio::time::sleep(Duration::from_millis(400)).await;
        let last = go_half_open(&ws).await;
        let fired = when(&r, Duration::from_secs(3), |r| !stream_lost(r).is_empty())
            .await
            .expect("no StreamLost");
        let after = fired - last;
        assert!(after >= Duration::from_millis(1_000), "fired {after:?} after the last pong");
        assert!(after < Duration::from_millis(1_200), "fired {after:?} after the last pong");
        assert!(r.links().last().is_some_and(|l| !l.0), "the link is down: {:?}", r.links());
        let (since, detail) = stream_lost(&r)[0].clone();
        assert!(detail.contains("no pong within 150 ms"), "{detail}");
        let age = Duration::from_micros(wall_us() - since);
        let real = last.elapsed();
        assert!(age.abs_diff(real) < Duration::from_millis(50), "since is {age:?} ago, the pong {real:?}");
        // Once per outage.
        tokio::time::sleep(Duration::from_millis(1_500)).await;
        assert_eq!(stream_lost(&r).len(), 1, "{:?}", stream_lost(&r));
        let s = r.gw.kill_status();
        assert_eq!(s["kill_armed"], true);
        assert!(s["last_proof_age_ms"].as_u64().unwrap() >= 2_500, "{s}");
    }

    /// A quiet stream that answers every ping never fires, however long.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_quiet_stream_that_answers_pings_never_fires_the_kill() {
        let api = rest_venue("REGULAR_MARGIN");
        let ws = FakeWs::start(GOOD).await;
        let r = rig_with(&api, &ws, kill_after(400), 256);
        r.until("link up", up).await;
        tokio::time::sleep(Duration::from_millis(2_000)).await;
        assert!(stream_lost(&r).is_empty(), "{:?}", stream_lost(&r));
        assert_eq!(r.links().len(), 1, "{:?}", r.links());
        assert!(r.gw.kill_status()["last_proof_age_ms"].as_u64().unwrap() < 400);
    }

    /// The socket closes; the reconnect is subscribed well before the
    /// deadline and answers pings: no kill.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_reconnect_before_the_deadline_does_not_fire() {
        let api = rest_venue("REGULAR_MARGIN");
        let ws = FakeWs::start(GOOD).await;
        let r = rig_with(&api, &ws, kill_after(1_000), 256);
        r.until("link up", up).await;
        for _ in 0..3 {
            ws.drop_conn();
            tokio::time::sleep(Duration::from_millis(400)).await;
        }
        tokio::time::sleep(Duration::from_millis(1_500)).await;
        assert!(r.links().iter().filter(|l| l.0).count() >= 4, "{:?}", r.links());
        assert!(stream_lost(&r).is_empty(), "{:?}", stream_lost(&r));
    }

    /// Before the boot checks pass the account is not verified: the trigger
    /// is not armed, however long the stream is down. The first pass arms
    /// it, and a stream that never comes up fires `kill_after` later.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn the_kill_is_not_armed_before_the_boot_checks_pass() {
        let api = rest_venue("REGULAR_MARGIN");
        let mut wrong = key();
        wrong["userID"] = json!(9999);
        api.route("/v5/user/query-api", [ok(wrong)]);
        let ws = FakeWs::start(Mode {
            auth: Reply::Silent,
            ..GOOD
        })
        .await;
        let r = rig_with(&api, &ws, kill_after(300), 256);
        r.until("the account block", |r| r.block().is_some_and(|b| b.starts_with("account:")))
            .await;
        tokio::time::sleep(Duration::from_millis(1_000)).await;
        assert!(stream_lost(&r).is_empty(), "{:?}", stream_lost(&r));
        let s = r.gw.kill_status();
        assert_eq!((s["kill_armed"].clone(), s["last_proof_age_ms"].clone()), (json!(false), Value::Null));

        api.route("/v5/user/query-api", [ok(key())]);
        let passed = when(&r, Duration::from_secs(3), |r| r.gw.kill_status()["kill_armed"] == true)
            .await
            .expect("never armed");
        let fired = when(&r, Duration::from_secs(3), |r| !stream_lost(r).is_empty())
            .await
            .expect("no StreamLost after the boot checks passed");
        let after = fired - passed;
        assert!(after >= Duration::from_millis(250) && after < Duration::from_millis(500), "{after:?}");
        assert!(r.links().is_empty(), "the link never came up: {:?}", r.links());
    }

    /// Time the gateway spends waiting on a full core queue moves the last
    /// proof on, as it moves the pong deadline: a link that goes down
    /// after such a wait does not fire at once, only `kill_after` after
    /// the moved proof.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn time_waiting_on_the_core_queue_does_not_fire_the_kill() {
        let api = rest_venue("REGULAR_MARGIN");
        let ws = FakeWs::start(GOOD).await;
        let r = rig_with(&api, &ws, kill_after(600), 1);
        r.until("link up", up).await;
        tokio::time::sleep(Duration::from_millis(400)).await;
        go_half_open(&ws).await;
        r.hold.store(true, Ordering::SeqCst);
        let row: Value = serde_json::from_str::<Value>(&order_msg("BTCUSDT", &link(42), "New"))
            .unwrap()["data"][0]
            .clone();
        ws.send(
            json!({"id": "1", "topic": "order.linear", "creationTime": 1, "data": vec![row; 6]})
                .to_string(),
        );
        // Longer than kill_after while the link is up and the core holds.
        tokio::time::sleep(Duration::from_millis(1_500)).await;
        r.hold.store(false, Ordering::SeqCst);
        let down = when(&r, Duration::from_secs(3), |r| r.links().iter().any(|l| !l.0))
            .await
            .expect("the link never went down");
        let fired = when(&r, Duration::from_secs(3), |r| !stream_lost(r).is_empty())
            .await
            .expect("no StreamLost");
        let gap = fired - down;
        assert!(gap >= Duration::from_millis(150), "fired {gap:?} after the link went down");
    }

    /// C1: a `Kill` keeps every create home from that moment, the loop
    /// confirms zero and tells the core, and creates stay home after it
    /// (the latch stays). Only cancel-alls go out on the order path.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn no_create_leaves_while_the_kill_loop_runs() {
        let api = kill_venue(0);
        let ws = FakeWs::start(GOOD).await;
        let r = rig(&api, &ws);
        r.until("link up", up).await;
        r.commands.send(kill_cmd()).await.unwrap();
        r.commands.send(order_cmd(5, "BTCUSDT")).await.unwrap();
        r.until("the send result", |r| results(r).len() == 1).await;
        assert!(
            matches!(&results(&r)[0], VenueInput::SendResult { outcome: SendOutcome::NotSent { reason }, .. } if reason == "kill switch active"),
            "{:?}",
            results(&r)
        );
        r.until("the loop's end", |r| ended(r).len() == 1).await;
        assert!(
            matches!(&ended(&r)[0], VenueInput::KillLoopEnded { outcome: KillOutcome::Confirmed, rounds: 1, cancel_all_calls: 1, .. }),
            "{:?}",
            ended(&r)
        );
        r.commands.send(order_cmd(6, "BTCUSDT")).await.unwrap();
        r.until("two results", |r| results(r).len() == 2).await;
        assert!(matches!(&results(&r)[1], VenueInput::SendResult { outcome: SendOutcome::NotSent { .. }, .. }));
        assert_eq!(posts(&api), ["/v5/order/cancel-all"]);
        let s = r.gw.kill_status();
        assert_eq!((s["kill_loop"]["phase"].clone(), s["kill_loop"]["remaining"].clone()), (json!("done"), json!(0)));
        let notices = r.notices.lock().unwrap().clone();
        assert_eq!(notices.len(), 2, "start and final: {notices:#?}");
        assert!(notices[0].starts_with("킬 루프 시작(개인 스트림 끊김: private stream down)"), "{}", notices[0]);
    }

    /// C2 under a block: no cancel-all leaves, no count follows, and the
    /// round says why (status and the repeat notice). Nothing is confirmed.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_blocked_gateway_sends_no_cancel_all_and_says_why() {
        let api = kill_venue(0);
        let mut wrong = key();
        wrong["userID"] = json!(9999);
        api.route("/v5/user/query-api", [ok(wrong)]);
        let ws = FakeWs::start(GOOD).await;
        let mut t = timing();
        t.kill.notice_ms = 250;
        let r = rig_with(&api, &ws, t, 256);
        r.until("the account block", |r| r.block().is_some_and(|b| b.starts_with("account:")))
            .await;
        r.commands.send(kill_cmd()).await.unwrap();
        tokio::time::sleep(Duration::from_millis(700)).await;
        assert_eq!(cancel_alls(&api), 0);
        let gets = |p: &str| api.requests().iter().filter(|q| q.path() == p).count();
        assert_eq!(gets("/v5/order/realtime"), 0, "a count after a round that sent nothing");
        assert!(ended(&r).is_empty());
        let s = r.gw.kill_status();
        assert!(s["kill_loop"]["round"].as_u64().unwrap() >= 5, "{s}");
        assert!(s["kill_loop"]["last_error"].as_str().unwrap().starts_with("account:"), "{s}");
        assert_eq!(s["kill_loop"]["remaining"], Value::Null);
        let notices = r.notices.lock().unwrap().clone();
        assert!(
            notices.iter().skip(1).any(|n| n.contains("남은 미체결 모름: account:") && n.contains("마지막 오류: account:")),
            "{notices:#?}"
        );
    }

    /// `KillStop` ends a running loop (no more cancel-alls) and creates go
    /// out again.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn kill_stop_ends_the_loop_and_creates_flow_again() {
        let api = kill_venue(1);
        let ws = FakeWs::start(GOOD).await;
        let r = rig(&api, &ws);
        r.until("link up", up).await;
        r.commands.send(kill_cmd()).await.unwrap();
        r.until("three rounds", |_| cancel_alls(&api) >= 3).await;
        // A second Kill while the loop runs changes nothing.
        r.commands.send(kill_cmd()).await.unwrap();
        r.commands.send(VenueCommand::KillStop).await.unwrap();
        r.until("stopped", |r| r.gw.kill_status()["kill_loop"]["phase"] == "stopped")
            .await;
        let n = cancel_alls(&api);
        tokio::time::sleep(Duration::from_millis(400)).await;
        assert_eq!(cancel_alls(&api), n, "a cancel-all after the stop");
        assert!(ended(&r).is_empty());
        r.commands.send(order_cmd(5, "BTCUSDT")).await.unwrap();
        r.until("the send result", |r| results(r).len() == 1).await;
        assert!(
            matches!(&results(&r)[0], VenueInput::SendResult { outcome: SendOutcome::Placed { .. }, .. }),
            "{:?}",
            results(&r)
        );
        assert_eq!(rest_creates(&api), 1);
    }
}
