//! The Bybit gateway behind `VenueGateway` (ADR-028 §7c, U16d): the boot
//! checks and the IP loop, the private stream, the queries, and the pause
//! after an IP ban. Orders and cancels are not sent before U17.
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

use super::checks::{self, Block, Expected, Health};
use super::query::{Queries, QueryFail};
use super::rest::{Credentials, IP_BAN_PAUSE, Rest, RestError};
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
    /// Boot checks that did not pass are run again this often.
    pub boot_retry: Duration,
    /// The IP check (`ip_check_secs`).
    pub ip_check: Duration,
    /// No request goes out for this long after HTTP 403.
    pub ban_pause: Duration,
    /// How often an established stream looks at `Health`.
    pub health_poll: Duration,
    /// Longest wait between reconnects.
    pub backoff_max: Duration,
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
            backoff_max: Duration::from_secs(10),
        }
    }
}

/// Where the gateway connects. Built from the network (`Endpoints`); tests
/// give loopback addresses.
#[derive(Clone, Debug)]
pub struct Wiring {
    pub rest: String,
    pub private_ws: String,
    pub ip_echo_url: String,
}

pub(crate) struct Gateway {
    rest: Rest,
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
}

fn wall_ms() -> u64 {
    wall_us() / 1000
}

fn wall_ns() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as i64)
        .unwrap_or(0)
}

/// Why a stream ended. `Rejected` stops the reconnects.
enum End {
    Retry(String),
    Rejected(String),
}

impl Gateway {
    pub(crate) fn new(
        cfg: &BybitConfig,
        creds: Arc<Credentials>,
        wiring: Wiring,
        symbols: Vec<String>,
        input: VenueInputTx,
        instance_id: u64,
        timing: Timing,
    ) -> Result<Gateway, String> {
        let timeout = Duration::from_millis(cfg.request_timeout_ms);
        let rest = || Rest::new(&wiring.rest, creds.clone(), cfg.recv_window_ms, timeout);
        let health = Arc::new(Health::default());
        // Nothing has been checked yet: the link stays down until the boot
        // checks pass.
        health.set_boot(Some(Block::Unchecked(
            "the boot checks have not run yet".into(),
        )));
        Ok(Gateway {
            queries: Queries::new(rest()?, instance_id, cfg.fills_lag_ms),
            rest: rest()?,
            echo: checks::echo_client(timeout)?,
            expected: Expected {
                sub_account_uid: cfg.sub_account_uid,
                symbols: symbols.clone(),
            },
            symbols: symbols.into_iter().collect(),
            recv_window_ms: cfg.recv_window_ms,
            wiring,
            creds,
            input,
            instance_id,
            session: AtomicU64::new(0),
            health,
            banned_until: Mutex::new(None),
            timing,
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

    /// The boot checks until they pass, then the IP check on its timer.
    /// Stops for good once the key is rejected.
    async fn checks_loop(self: Arc<Self>) {
        let mut booted = false;
        loop {
            if self.health.auth_rejected() {
                return;
            }
            if !self.quiet() {
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
                    }
                    self.record(r, Health::set_boot);
                }
            }
            let wait = if booted {
                self.timing.ip_check
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
        let mut backoff = Duration::from_millis(500);
        loop {
            if self.health.auth_rejected() {
                warn!("the private stream stays closed: the key was rejected");
                return;
            }
            // Connect only when the link may come up.
            if self.quiet() || self.health.block().is_some() {
                tokio::time::sleep(self.timing.health_poll).await;
                continue;
            }
            match self.run_stream(boot, &mut connection).await {
                Ok(()) => backoff = Duration::from_millis(500),
                Err(End::Retry(e)) => warn!("the private stream ended", "error": e),
                Err(End::Rejected(e)) => {
                    error!("the private stream's auth was refused; no request goes out until the restart", "error": e.clone());
                    self.health.reject_auth(format!("private stream auth: {e}"));
                    return;
                }
            }
            tokio::time::sleep(backoff).await;
            backoff = (backoff * 2).min(self.timing.backoff_max);
        }
    }

    /// Connects over IPv4, as every other request does (`LOCAL_ADDRESS`).
    async fn connect(
        &self,
    ) -> Result<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
        String,
    > {
        let url = reqwest::Url::parse(&self.wiring.private_ws).map_err(|e| e.to_string())?;
        let host = url.host_str().ok_or("the stream URL has no host")?;
        let port = url
            .port_or_known_default()
            .ok_or("the stream URL has no port")?;
        let addrs: Vec<SocketAddr> = tokio::net::lookup_host((host, port))
            .await
            .map_err(|e| e.to_string())?
            .filter(SocketAddr::is_ipv4)
            .collect();
        let addr = addrs.first().ok_or("the stream host has no IPv4 address")?;
        let tcp = tokio::net::TcpStream::connect(addr)
            .await
            .map_err(|e| e.to_string())?;
        let (ws, _) = tokio_tungstenite::client_async_tls(self.wiring.private_ws.as_str(), tcp)
            .await
            .map_err(|e| e.to_string())?;
        Ok(ws)
    }

    async fn run_stream(&self, boot: u32, connection: &mut u32) -> Result<(), End> {
        let ws = tokio::time::timeout(self.timing.handshake, self.connect())
            .await
            .map_err(|_| End::Retry("connect timed out".into()))?
            .map_err(End::Retry)?;
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

        *connection += 1;
        let session = session_number(boot, *connection);
        if let Some(b) = self.health.block() {
            return Err(End::Retry(format!("blocked before the link came up: {b}")));
        }
        self.session.store(session, Ordering::SeqCst);
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
        let mut awaiting_pong = false;
        let reason = loop {
            let msg = tokio::select! {
                m = read.next() => match m {
                    None => break "closed by the venue".to_string(),
                    Some(Err(e)) => break format!("read failed: {e}"),
                    Some(Ok(m)) => m,
                },
                _ = ping.tick() => {
                    // A quiet stream and a half-open connection look the
                    // same; only the pong tells them apart.
                    if awaiting_pong {
                        break format!("no pong within {} ms of a ping", self.timing.ping.as_millis());
                    }
                    pings += 1;
                    if let Err(e) = write.send(send(stream::ping_message(pings))).await {
                        break format!("ping failed: {e}");
                    }
                    awaiting_pong = true;
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
            match frame {
                stream::Frame::Pong => awaiting_pong = false,
                stream::Frame::Orders(rows) => {
                    if let Err(e) = self.orders(&rows, session).await {
                        break e;
                    }
                }
                stream::Frame::Executions(rows) => {
                    if let Err(e) = self.executions(&rows, recv_ns).await {
                        break e;
                    }
                }
                stream::Frame::Auth(_) | stream::Frame::Subscribe(Err(_)) => {
                    break "an unexpected auth or subscribe answer".to_string();
                }
                stream::Frame::Subscribe(Ok(())) | stream::Frame::Other => {}
            }
        };
        self.input
            .send(VenueInput::Link {
                up: false,
                session,
                reason: reason.clone(),
            })
            .await;
        Err(End::Retry(reason))
    }

    fn traded(&self, row: &serde_json::Value) -> bool {
        stream::symbol(row).is_some_and(|s| self.symbols.contains(s))
    }

    /// Delivers the readable rows; an unreadable row ends the stream (the
    /// queries after the reconnect read what it held, G-9).
    async fn orders(&self, rows: &[serde_json::Value], session: u64) -> Result<(), String> {
        for row in rows.iter().filter(|r| self.traded(r)) {
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
        for row in rows.iter().filter(|r| self.traded(r)) {
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

    async fn send_order(&self, cmd: VenueCommand) {
        let VenueCommand::SendOrder {
            order_id,
            instance_id,
            ..
        } = cmd
        else {
            return;
        };
        let at = wall_us();
        self.input
            .send(VenueInput::SendResult {
                order_id,
                outcome: SendOutcome::NotSent {
                    reason: "the Bybit gateway sends no orders before U17".into(),
                },
                timing: GatewayTiming {
                    started_at: at,
                    finished_at: at,
                    wire_id: exec::WIRE_ID.encode(VenueClientId {
                        instance_id,
                        order_id,
                    }),
                },
            })
            .await;
    }

    async fn send_cancel(&self, cmd: VenueCommand) {
        let VenueCommand::SendCancel {
            order_id,
            instance_id,
            attempt,
            ..
        } = cmd
        else {
            return;
        };
        let at = wall_us();
        self.input
            .send(VenueInput::CancelResult {
                order_id,
                attempt,
                outcome: CancelOutcome::NotSent {
                    reason: "the Bybit gateway sends no cancels before U17".into(),
                },
                timing: GatewayTiming {
                    started_at: at,
                    finished_at: at,
                    wire_id: exec::WIRE_ID.encode(VenueClientId {
                        instance_id,
                        order_id,
                    }),
                },
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

    fn spawn_tasks(self: &Arc<Self>, boot: u32) {
        tokio::spawn(self.clone().checks_loop());
        tokio::spawn(self.clone().stream_loop(boot));
    }
}

/// What the status API shows of the gateway: why the link may not come up.
pub type BlockView = Arc<dyn Fn() -> Option<String> + Send + Sync>;

/// Starts the gateway on the configured network. The credentials are read
/// once, here.
pub fn spawn(
    cfg: BybitConfig,
    symbols: Vec<String>,
    ports: GatewayPorts,
    max_concurrent: usize,
) -> Result<(String, BlockView), String> {
    let ep =
        Endpoints::of(&cfg.network).ok_or_else(|| format!("unknown network {:?}", cfg.network))?;
    let wiring = Wiring {
        rest: ep.rest,
        private_ws: ep.private_ws,
        ip_echo_url: cfg.ip_echo_url.clone(),
    };
    let creds = Arc::new(Credentials::load(&cfg.credentials_file)?);
    spawn_with(cfg, creds, wiring, symbols, ports, max_concurrent, None)
}

/// `spawn` with given credentials and endpoints (tests: loopback).
pub(crate) fn spawn_with(
    cfg: BybitConfig,
    creds: Arc<Credentials>,
    wiring: Wiring,
    symbols: Vec<String>,
    ports: GatewayPorts,
    max_concurrent: usize,
    timing: Option<Timing>,
) -> Result<(String, BlockView), String> {
    let timing = timing.unwrap_or_else(|| Timing::of(&cfg));
    let gw = Arc::new(Gateway::new(
        &cfg,
        creds,
        wiring,
        symbols,
        ports.input.clone(),
        ports.instance_id,
        timing,
    )?);
    info!("bybit gateway starting", "network": cfg.network.clone(), "sub_account_uid": cfg.sub_account_uid);
    let health = gw.health();
    let view: BlockView = Arc::new(move || health.block().map(|b| b.to_string()));
    start(gw, ports, max_concurrent);
    Ok((format!("bybit:{}", cfg.network), view))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::queues::{CoreMsg, InputQueues};
    use http_fake::{Answer as Http, FakeHttp};
    use serde_json::{Value, json};
    use tokio::net::TcpListener;
    use tokio::sync::mpsc;

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
            [ok(json!({"category": "linear", "list": [{"symbol": "BTCUSDT", "settleCoin": "USDT", "status": "Trading"}], "nextPageCursor": ""}))],
        );
        api.route("/v5/position/list", [positions()]);
        api.route("/ip", [Http::Reply(200, "127.0.0.1\n".into())]);
        api
    }

    #[derive(Clone, Copy, PartialEq)]
    enum Reply {
        Ok,
        Refuse,
    }

    #[derive(Clone, Copy)]
    struct Mode {
        auth: Reply,
        subscribe: Reply,
        pong: bool,
    }

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
        conns: Arc<Mutex<Vec<Arc<Mutex<Vec<String>>>>>>,
        push: Arc<Mutex<Option<mpsc::UnboundedSender<Ctl>>>>,
    }

    impl FakeWs {
        async fn start(mode: Mode) -> FakeWs {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let url = format!("ws://{}/v5/private", listener.local_addr().unwrap());
            let fake = FakeWs {
                url,
                mode: Arc::new(Mutex::new(mode)),
                conns: Arc::default(),
                push: Arc::default(),
            };
            let (mode, conns, push) = (fake.mode.clone(), fake.conns.clone(), fake.push.clone());
            tokio::spawn(async move {
                while let Ok((tcp, _)) = listener.accept().await {
                    let Ok(ws) = tokio_tungstenite::accept_async(tcp).await else {
                        continue;
                    };
                    let log: Arc<Mutex<Vec<String>>> = Arc::default();
                    conns.lock().unwrap().push(log.clone());
                    let (tx, rx) = mpsc::unbounded_channel();
                    *push.lock().unwrap() = Some(tx);
                    tokio::spawn(serve(ws, mode.clone(), log, rx));
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
    }

    async fn serve(
        ws: tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>,
        mode: Arc<Mutex<Mode>>,
        log: Arc<Mutex<Vec<String>>>,
        mut ctl: mpsc::UnboundedReceiver<Ctl>,
    ) {
        let (mut write, mut read) = ws.split();
        loop {
            tokio::select! {
                m = read.next() => {
                    let Some(Ok(Message::Text(t))) = m else { return };
                    log.lock().unwrap().push(t.to_string());
                    let v: Value = serde_json::from_str(t.as_str()).unwrap_or_default();
                    let m = *mode.lock().unwrap();
                    let answer = match v["op"].as_str() {
                        Some("auth") => Some(json!({"success": m.auth == Reply::Ok, "ret_msg": if m.auth == Reply::Ok { "" } else { "error sign" }, "op": "auth", "conn_id": "c"})),
                        Some("subscribe") => Some(json!({"success": m.subscribe == Reply::Ok, "ret_msg": "", "op": "subscribe", "conn_id": "c"})),
                        Some("ping") if m.pong => Some(json!({"req_id": v["req_id"], "op": "pong", "args": ["1"], "conn_id": "c"})),
                        _ => None,
                    };
                    if let Some(a) = answer {
                        if write.send(Message::text(a.to_string())).await.is_err() {
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
            backoff_max: Duration::from_millis(50),
        }
    }

    /// A running gateway and what it delivered to the core.
    struct Rig {
        gw: Arc<Gateway>,
        inputs: Arc<Mutex<Vec<VenueInput>>>,
        commands: mpsc::Sender<VenueCommand>,
        queries: mpsc::Sender<QueryReq>,
        queues: Arc<InputQueues>,
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
        let queues = Arc::new(InputQueues::new(256, 256, 256));
        let wiring = Wiring {
            rest: api.url(),
            private_ws: ws.url.clone(),
            ip_echo_url: format!("{}/ip", api.url()),
        };
        let gw = Arc::new(
            Gateway::new(
                &cfg(),
                Arc::new(Credentials::new("KEY", "SECRET")),
                wiring,
                vec!["BTCUSDT".into()],
                VenueInputTx::new(queues.clone()),
                INSTANCE,
                timing(),
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
        let (q, sink) = (queues.clone(), inputs.clone());
        std::thread::spawn(move || {
            let mut batch: Vec<CoreMsg> = Vec::new();
            while q.pop_batch(64, &mut batch) {
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

        // The boot checks ran before the stream: four reads, then the stream.
        let paths: Vec<String> = api
            .requests()
            .iter()
            .map(|q| q.path().to_string())
            .collect();
        assert_eq!(
            &paths[..5],
            [
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
        api.route("/ip", [Http::Reply(200, "127.0.0.1\n".into())]);
        tokio::time::sleep(Duration::from_millis(400)).await;
        assert_eq!(r.links().len(), 2, "the right IP again does not lift it");
        assert_eq!(ws.accepted(), 1);
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

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn orders_and_cancels_are_answered_not_sent_once_each() {
        let api = rest_venue("REGULAR_MARGIN");
        let ws = FakeWs::start(GOOD).await;
        let r = rig(&api, &ws);
        r.until("link up", up).await;
        let reads = api.requests().len();
        r.commands
            .send(VenueCommand::SendOrder {
                order_id: OrderId(5),
                instance_id: INSTANCE,
                instrument: InstrumentId::new("BTCUSDT").unwrap(),
                side: Side::Buy,
                qty: Qty(1),
                px: Px(1),
                tif: Tif::Gtc,
                reduce_only: false,
                expires_at: u64::MAX,
            })
            .await
            .unwrap();
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
        r.until("two results", |r| {
            r.inputs()
                .iter()
                .filter(|i| {
                    matches!(
                        i,
                        VenueInput::SendResult { .. } | VenueInput::CancelResult { .. }
                    )
                })
                .count()
                == 2
        })
        .await;
        tokio::time::sleep(Duration::from_millis(100)).await;
        let results: Vec<VenueInput> = r
            .inputs()
            .into_iter()
            .filter(|i| {
                matches!(
                    i,
                    VenueInput::SendResult { .. } | VenueInput::CancelResult { .. }
                )
            })
            .collect();
        assert_eq!(results.len(), 2);
        // The two run as their own tasks, in either order.
        assert!(results.iter().any(|i| matches!(i, VenueInput::SendResult { outcome: SendOutcome::NotSent { .. }, timing, .. } if timing.wire_id == link(5))), "{results:?}");
        assert!(
            results.iter().any(|i| matches!(
                i,
                VenueInput::CancelResult {
                    outcome: CancelOutcome::NotSent { .. },
                    attempt: 1,
                    ..
                }
            )),
            "{results:?}"
        );
        let order_paths = api
            .requests()
            .iter()
            .skip(reads)
            .filter(|q| q.method != "GET")
            .count();
        assert_eq!(order_paths, 0, "nothing but reads");
        assert_eq!(
            ws.texts(0)
                .iter()
                .filter(|t| !t.contains("\"ping\""))
                .count(),
            2,
            "auth and subscribe only"
        );
    }
}
