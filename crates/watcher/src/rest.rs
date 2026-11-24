//! The watcher's own Bybit v5 REST client (C-WATCH part 3), written from
//! the venue's documentation, not from the OMS's code: signed GETs, the one
//! signed POST (the cancel-all of the kill loop, C-WATCH part 11), the
//! answer classes, the never-retried key rejection, the 403 pause, the
//! request guard and the request counters. The checks call it; it never
//! retries inside a call (the next round is the retry).

use std::collections::VecDeque;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::Value;

use crate::clock::Clock;
use crate::log::Log;
use crate::metrics::Metrics;
use crate::sign::{self, Signer};

/// At most this many requests in any [`GUARD_WINDOW_MS`], all Bybit paths
/// and the IP echo together (판단(모델 아님)).
pub const GUARD_MAX: usize = 60;
pub const GUARD_WINDOW_MS: u64 = 10_000;

/// The request guard: a sliding window on the injected clock.
pub struct Guard {
    max: usize,
    window_ms: u64,
    sent: VecDeque<u64>,
}

impl Default for Guard {
    fn default() -> Guard {
        Guard::new(GUARD_MAX, GUARD_WINDOW_MS)
    }
}

impl Guard {
    pub fn new(max: usize, window_ms: u64) -> Guard {
        Guard {
            max,
            window_ms,
            sent: VecDeque::new(),
        }
    }

    /// Records a request at `now_ms` and lets it go, or returns the
    /// earliest instant (ms) at which it may go; a refused request is not
    /// recorded. A request leaves the window `window_ms` after it went.
    pub fn admit(&mut self, now_ms: u64) -> Result<(), u64> {
        while let Some(&t) = self.sent.front() {
            if now_ms.saturating_sub(t) >= self.window_ms {
                self.sent.pop_front();
            } else {
                break;
            }
        }
        match self.sent.front() {
            Some(&first) if self.sent.len() >= self.max => Err(first + self.window_ms),
            _ => {
                self.sent.push_back(now_ms);
                Ok(())
            }
        }
    }
}

/// The API key and its signing key: an HMAC secret or an RSA key
/// (ADR-034). `Debug` never prints the secret or the key.
pub struct Credentials {
    api_key: String,
    signer: Signer,
}

impl Credentials {
    /// An HMAC key (`api_secret`).
    pub fn new(api_key: &str, api_secret: &str) -> Credentials {
        Credentials {
            api_key: api_key.to_string(),
            signer: Signer::hmac(api_secret),
        }
    }

    /// An RSA key (`rsa_private_key`, the PKCS#8 PEM text), parsed here;
    /// the error does not quote the text ([`Signer::rsa`]).
    pub fn rsa(api_key: &str, pem: &str) -> Result<Credentials, String> {
        Ok(Credentials {
            api_key: api_key.to_string(),
            signer: Signer::rsa(pem)?,
        })
    }

    pub fn api_key(&self) -> &str {
        &self.api_key
    }

    pub fn is_rsa(&self) -> bool {
        self.signer.is_rsa()
    }

    /// `X-BAPI-SIGN` of `prehash` ([`Signer::sign`]).
    pub fn sign(&self, prehash: &str) -> Result<String, String> {
        self.signer.sign(prehash)
    }
}

impl std::fmt::Debug for Credentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let field = if self.signer.is_rsa() {
            "rsa_private_key"
        } else {
            "api_secret"
        };
        write!(
            f,
            "Credentials {{ api_key: {:?}, {field}: <hidden> }}",
            self.api_key
        )
    }
}

/// An error of a request, by class (C-WATCH part 3). Every class fails
/// the round that asked; `Auth` also stops every later Bybit request and
/// `IpBanned` pauses every request for [`IP_BAN_PAUSE_MS`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RestError {
    /// Nothing was sent: the connection failed, or a query value is not
    /// sent as is.
    NotSent(String),
    /// Sent, but no usable answer: timeout, HTTP 5xx, unreadable body, no
    /// retCode, retCode 10016.
    NoAnswer(String),
    /// HTTP 401 or one of [`AUTH_CODES`]: the key is not accepted.
    Auth { code: i64, msg: String },
    /// HTTP 429 or retCode 10006.
    RateLimited(String),
    /// HTTP 403: the venue bans this IP.
    IpBanned(String),
    /// retCode 10002: the timestamp is outside the venue's window.
    Clock(String),
    /// Any other non-zero retCode.
    Refused { code: i64, msg: String },
    /// retCode 0, but the check cannot read the rows (made by the check).
    Unreadable(String),
}

impl RestError {
    /// The `class` label of `watcher_request_errors_total`.
    pub fn class(&self) -> &'static str {
        match self {
            RestError::NotSent(_) => "not_sent",
            RestError::NoAnswer(_) => "no_answer",
            RestError::Auth { .. } => "auth",
            RestError::RateLimited(_) => "rate_limited",
            RestError::IpBanned(_) => "ip_banned",
            RestError::Clock(_) => "clock",
            RestError::Refused { .. } => "refused",
            RestError::Unreadable(_) => "unreadable",
        }
    }
}

impl std::fmt::Display for RestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RestError::NotSent(e) => write!(f, "not sent: {e}"),
            RestError::NoAnswer(e) => write!(f, "no answer: {e}"),
            RestError::Auth { code, msg } => write!(f, "key rejected ({code}): {msg}"),
            RestError::RateLimited(e) => write!(f, "rate limited: {e}"),
            RestError::IpBanned(e) => write!(f, "IP banned: {e}"),
            RestError::Clock(e) => write!(f, "timestamp outside the venue's window: {e}"),
            RestError::Refused { code, msg } => write!(f, "refused ({code}): {msg}"),
            RestError::Unreadable(e) => write!(f, "unreadable rows: {e}"),
        }
    }
}

/// retCodes after which no Bybit request is sent again until the process
/// restarts: the key, its signature, its permissions or its bound IPs are
/// not accepted, and retrying does not change that.
pub const AUTH_CODES: [i64; 6] = [10003, 10004, 10005, 10007, 10010, 33004];

/// The code of a key stop that no venue answer made: the IP recheck found
/// that the key no longer passes the start rules ([`Client::stop_key`]).
pub const OWN_RULE_CODE: i64 = -1;

/// How long no request (Bybit or the IP echo) goes out after HTTP 403
/// (F18 L186: "wait for at least 10 minutes").
pub const IP_BAN_PAUSE_MS: u64 = 600_000;

/// A message of the venue as it goes into an error and a log line: at
/// most 200 characters (not bytes), control characters (line breaks) and
/// the Unicode line and paragraph separators as spaces, so one error line
/// stays one line.
fn one_line(text: &str) -> String {
    text.chars()
        .take(200)
        .map(|c| {
            if c.is_control() || matches!(c, '\u{2028}' | '\u{2029}') {
                ' '
            } else {
                c
            }
        })
        .collect()
}

/// Classifies an answer: the `result` of retCode 0 (`null` when absent),
/// or the error.
pub fn classify(status: u16, body: &str) -> Result<Value, RestError> {
    let short = one_line(body);
    match status {
        200..=299 => {}
        401 => {
            return Err(RestError::Auth {
                code: 401,
                msg: format!("HTTP 401: {short}"),
            });
        }
        403 => return Err(RestError::IpBanned(format!("HTTP 403: {short}"))),
        429 => return Err(RestError::RateLimited(format!("HTTP 429: {short}"))),
        _ => return Err(RestError::NoAnswer(format!("HTTP {status}: {short}"))),
    }
    let v: Value = serde_json::from_str(body)
        .map_err(|e| RestError::NoAnswer(format!("unreadable body: {e}")))?;
    let code = v
        .get("retCode")
        .and_then(Value::as_i64)
        .ok_or_else(|| RestError::NoAnswer(format!("no retCode: {short}")))?;
    let msg = one_line(v.get("retMsg").and_then(Value::as_str).unwrap_or(""));
    match code {
        0 => Ok(v.get("result").cloned().unwrap_or(Value::Null)),
        c if AUTH_CODES.contains(&c) => Err(RestError::Auth { code: c, msg }),
        10006 => Err(RestError::RateLimited(msg)),
        10002 => Err(RestError::Clock(msg)),
        10016 => Err(RestError::NoAnswer(format!("venue error 10016: {msg}"))),
        c => Err(RestError::Refused { code: c, msg }),
    }
}

/// What the client needs from the config (U19a4).
#[derive(Debug, Clone)]
pub struct ClientConfig {
    /// `https://api.bybit.com` or `https://api-testnet.bybit.com`.
    pub base: String,
    pub recv_window_ms: u64,
    pub request_timeout_ms: u64,
}

/// The local address of every connection: IPv4 only, so the IP echo sees
/// the address Bybit sees.
pub const LOCAL_ADDRESS: IpAddr = IpAddr::V4(Ipv4Addr::UNSPECIFIED);

/// The path of the watcher's one signed POST (U19b2a): cancel every open
/// order of the account in linear/USDT (gateway facts Q11).
pub const CANCEL_ALL: &str = "/v5/order/cancel-all";

/// Its body: the signed string and the body sent are these bytes. No
/// `orderFilter`, so every kind of order is cancelled (Q11.2). There is no
/// other body and no generic POST: no path of the watcher can create or
/// amend an order.
pub const CANCEL_ALL_BODY: &str = r#"{"category":"linear","settleCoin":"USDT"}"#;

/// A cancel-all that Bybit accepted: the length of `result.list` (at most
/// 500 per call, Q11.3; the cancel runs asynchronously, Q11.5, so only a
/// count of the open orders afterwards tells what is left).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CancelAll {
    pub cancelled: usize,
}

/// The `result` of a cancel-all: `success` absent or `"1"` with a list is
/// accepted; `"0"` is refused; anything else is unreadable.
fn read_cancel_all(v: &Value) -> Result<CancelAll, RestError> {
    match v.get("success") {
        None => {}
        Some(Value::String(s)) if s == "1" => {}
        Some(Value::String(s)) if s == "0" => {
            return Err(RestError::Refused {
                code: 0,
                msg: "cancel-all answered success 0".into(),
            });
        }
        Some(other) => {
            return Err(RestError::Unreadable(format!(
                "cancel-all: success is {}",
                one_line(&other.to_string())
            )));
        }
    }
    let list = v
        .get("list")
        .and_then(Value::as_array)
        .ok_or_else(|| RestError::Unreadable("cancel-all: result.list is not a list".into()))?;
    Ok(CancelAll {
        cancelled: list.len(),
    })
}

/// The `endpoint` label of the IP echo (its URL is config, not a label).
pub const ECHO: &str = "ip_echo";

/// How long the client sleeps at most before it reads the clock again
/// while it waits at the guard (판단(모델 아님)).
const GUARD_POLL_MS: u64 = 100;

/// The signed client. One per process: the guard, the key rejection
/// and the 403 pause hold for every request it sends, from any task.
pub struct Client {
    base: String,
    http: reqwest::Client,
    creds: Credentials,
    recv_window_ms: u64,
    clock: Arc<dyn Clock>,
    metrics: Arc<Metrics>,
    log: Log,
    stop: Mutex<Stop>,
}

/// What stopped every Bybit request until restart (U19b1c, the `auth`
/// P1): the code ([`OWN_RULE_CODE`] for [`Client::stop_key`]), whether it
/// was HTTP 401, and the text of the first answer that latched it (its
/// `retMsg`, the 401's body, or the rule's reason), one line of at most 200
/// characters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthStop {
    pub code: i64,
    pub http: bool,
    pub text: String,
}

#[derive(Default)]
struct Stop {
    guard: Guard,
    /// The code that stopped every Bybit request until restart.
    auth: Option<i64>,
    /// The same stop with its text, set with `auth` and never changed.
    auth_stop: Option<AuthStop>,
    /// The end of the last 403 pause (the clock at the answer + 600 s).
    paused_until: Option<u64>,
}

fn error_chain(e: &dyn std::error::Error) -> String {
    let mut text = e.to_string();
    let mut cause = e.source();
    while let Some(c) = cause {
        text.push_str(": ");
        text.push_str(&c.to_string());
        cause = c.source();
    }
    text
}

impl Client {
    pub fn new(
        cfg: &ClientConfig,
        creds: Credentials,
        clock: Arc<dyn Clock>,
        metrics: Arc<Metrics>,
        log: Log,
    ) -> Result<Client, String> {
        let http = reqwest::Client::builder()
            .local_address(LOCAL_ADDRESS)
            .connect_timeout(Duration::from_secs(3))
            .timeout(Duration::from_millis(cfg.request_timeout_ms))
            // A redirect would re-send a signed request.
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .build()
            .map_err(|e| e.to_string())?;
        metrics.set("watcher_auth_rejected", &[], 0.0);
        Ok(Client {
            base: cfg.base.trim_end_matches('/').to_string(),
            http,
            creds,
            recv_window_ms: cfg.recv_window_ms,
            clock,
            metrics,
            log,
            stop: Mutex::new(Stop::default()),
        })
    }

    /// The error of a check that cannot read the rows of a retCode 0
    /// answer, counted under `endpoint`.
    pub fn unreadable(&self, endpoint: &str, why: &str) -> RestError {
        let e = RestError::Unreadable(why.to_string());
        self.count_error(endpoint, &e);
        e
    }

    /// The code of the answer that stopped every Bybit request, if any.
    pub fn auth_rejected(&self) -> Option<i64> {
        self.stop.lock().ok()?.auth
    }

    /// The stop of every Bybit request with its first text, if any
    /// (U19b1c).
    pub fn auth_stop(&self) -> Option<AuthStop> {
        self.stop.lock().ok()?.auth_stop.clone()
    }

    /// Stops every later Bybit request until restart, as a rejected key
    /// does, for a rule of the watcher's own: the IP recheck found that the
    /// key no longer passes the start rules (U19a4b). The code is
    /// [`OWN_RULE_CODE`]; `watcher_auth_rejected` is 1 and one error line
    /// names `reason`, the first time only (a key stop that came first
    /// stays as it is).
    pub fn stop_key(&self, reason: &str) {
        let Ok(mut s) = self.stop.lock() else {
            return;
        };
        if s.auth.is_none() {
            s.auth = Some(OWN_RULE_CODE);
            s.auth_stop = Some(AuthStop {
                code: OWN_RULE_CODE,
                http: false,
                text: one_line(reason),
            });
            self.metrics.set("watcher_auth_rejected", &[], 1.0);
            (self.log)(&format!(
                "error: {}; every Bybit request stops until the watcher restarts",
                one_line(reason)
            ));
        }
    }

    /// The end (ms) of the last 403 pause, if there was one. The start
    /// checks (U19a4b) do not read it: they wait 600 s from the 403 answer
    /// they got, which is this instant for a 403 they sent.
    pub fn paused_until(&self) -> Option<u64> {
        self.stop.lock().ok()?.paused_until
    }

    /// An unsigned GET of the IP echo (U19a4b); returns the body, trimmed.
    /// It waits at the guard and the 403 pause like every Bybit request,
    /// but a rejected key does not stop it: the echo is not a Bybit
    /// request, and a changed public IP is a usual cause of 10010. Any
    /// HTTP status but 2xx is `no_answer` (the echo's 403 is no venue ban).
    pub async fn echo(&self, url: &str) -> Result<String, RestError> {
        self.turn(false).await?;
        self.metrics
            .add("watcher_requests_total", &[("endpoint", ECHO)], 1.0);
        let res = match self.send(self.http.get(url)).await {
            Ok((200..=299, body)) => Ok(body.trim().to_string()),
            Ok((status, _)) => Err(RestError::NoAnswer(format!("IP echo HTTP {status}"))),
            Err(e) => Err(e),
        };
        if let Err(e) = &res {
            self.count_error(ECHO, e);
        }
        res
    }

    /// After an answer of Bybit: a rejected key stops every later Bybit
    /// request (one error line); a 403 pauses every request for
    /// [`IP_BAN_PAUSE_MS`] from the clock at the answer.
    fn after(&self, path: &str, e: &RestError) {
        let Ok(mut s) = self.stop.lock() else {
            return;
        };
        match e {
            RestError::Auth { code, msg } if s.auth.is_none() => {
                s.auth = Some(*code);
                let http = *code == 401;
                s.auth_stop = Some(AuthStop {
                    code: *code,
                    http,
                    text: match msg.strip_prefix("HTTP 401: ") {
                        Some(body) if http => body.to_string(),
                        _ => msg.clone(),
                    },
                });
                self.metrics.set("watcher_auth_rejected", &[], 1.0);
                (self.log)(&format!(
                    "error: Bybit rejected the key ({code}) at {path}: {msg}; every Bybit request stops until the watcher restarts"
                ));
            }
            RestError::IpBanned(_) => {
                let until = self.clock.now_ms() + IP_BAN_PAUSE_MS;
                s.paused_until = Some(until);
                (self.log)(&format!(
                    "error: Bybit answered HTTP 403 at {path}: every request (and the IP echo) pauses for {} s, until {until} ms",
                    IP_BAN_PAUSE_MS / 1000
                ));
            }
            _ => {}
        }
    }

    fn count_error(&self, endpoint: &str, e: &RestError) {
        self.metrics.add(
            "watcher_request_errors_total",
            &[("endpoint", endpoint), ("class", e.class())],
            1.0,
        );
    }

    /// Refuses a request after a rejected key (`bybit` only) or during a
    /// 403 pause, without sending or counting it; otherwise waits until the
    /// guard lets it go. The stop and the pause are looked at again after
    /// every wait.
    async fn turn(&self, bybit: bool) -> Result<(), RestError> {
        loop {
            let now = self.clock.now_ms();
            let at = {
                let mut s = self
                    .stop
                    .lock()
                    .map_err(|_| RestError::NotSent("client state poisoned".into()))?;
                if let (true, Some(code)) = (bybit, s.auth) {
                    return Err(RestError::Auth {
                        code,
                        msg: "not sent: the key was rejected earlier; every Bybit request stops until restart".into(),
                    });
                }
                if let Some(until) = s.paused_until.filter(|&u| now < u) {
                    return Err(RestError::IpBanned(format!(
                        "not sent: paused after HTTP 403 until {until} ms"
                    )));
                }
                match s.guard.admit(now) {
                    Ok(()) => return Ok(()),
                    Err(at) => at,
                }
            };
            let wait = at.saturating_sub(now).clamp(1, GUARD_POLL_MS);
            tokio::time::sleep(Duration::from_millis(wait)).await;
        }
    }

    /// A signed GET of `path` with `params`; returns `result`.
    pub async fn get(&self, path: &str, params: &[(&str, String)]) -> Result<Value, RestError> {
        let q = match sign::query_string(params) {
            Ok(q) => q,
            Err(e) => {
                let e = RestError::NotSent(e);
                self.count_error(path, &e);
                return Err(e);
            }
        };
        self.turn(true).await?;
        let ts = self.clock.now_ms();
        let sig = self.sign(
            path,
            &sign::prehash(ts, &self.creds.api_key, self.recv_window_ms, &q),
        )?;
        self.metrics
            .add("watcher_requests_total", &[("endpoint", path)], 1.0);
        let url = if q.is_empty() {
            format!("{}{path}", self.base)
        } else {
            format!("{}{path}?{q}", self.base)
        };
        let rb = self.signed(self.http.get(url), ts, sig);
        let res = match self.send(rb).await {
            Ok((status, body)) => classify(status, &body),
            Err(e) => Err(e),
        };
        if let Err(e) = &res {
            self.count_error(path, e);
            self.after(path, e);
        }
        res
    }

    /// The cancel-all (U19b2a): the only signed request that is not a GET
    /// and the only POST to the REST base. The path and the body are the
    /// constants [`CANCEL_ALL`] and [`CANCEL_ALL_BODY`]; the signature is
    /// over the body sent. It waits at the guard and the 403 pause and is
    /// refused after a rejected key like every Bybit request, and counts in
    /// `watcher_requests_total{endpoint="/v5/order/cancel-all"}`.
    pub async fn cancel_all(&self) -> Result<CancelAll, RestError> {
        self.turn(true).await?;
        let ts = self.clock.now_ms();
        let sig = self.sign(
            CANCEL_ALL,
            &sign::post_prehash(ts, &self.creds.api_key, self.recv_window_ms, CANCEL_ALL_BODY),
        )?;
        self.metrics
            .add("watcher_requests_total", &[("endpoint", CANCEL_ALL)], 1.0);
        let rb = self
            .http
            .post(format!("{}{CANCEL_ALL}", self.base))
            .header("Content-Type", "application/json")
            .body(CANCEL_ALL_BODY);
        let res = match self.send(self.signed(rb, ts, sig)).await {
            Ok((status, body)) => classify(status, &body).and_then(|v| read_cancel_all(&v)),
            Err(e) => Err(e),
        };
        if let Err(e) = &res {
            self.count_error(CANCEL_ALL, e);
            self.after(CANCEL_ALL, e);
        }
        res
    }

    /// The signature of `prehash` for a request to `endpoint`. A
    /// signature that cannot be made (not expected for a parsed key) is a
    /// request not sent, counted under `endpoint`: nothing goes out
    /// unsigned or signed another way (ADR-034, spec D7).
    fn sign(&self, endpoint: &str, prehash: &str) -> Result<String, RestError> {
        self.creds.sign(prehash).map_err(|e| {
            let e = RestError::NotSent(e);
            self.count_error(endpoint, &e);
            e
        })
    }

    /// The five headers of a signed request (four with an RSA key: the
    /// measured RSA request had no `X-BAPI-SIGN-TYPE`, ADR-034).
    fn signed(&self, rb: reqwest::RequestBuilder, ts: u64, sig: String) -> reqwest::RequestBuilder {
        let rb = rb
            .header("X-BAPI-API-KEY", &self.creds.api_key)
            .header("X-BAPI-TIMESTAMP", ts.to_string())
            .header("X-BAPI-RECV-WINDOW", self.recv_window_ms.to_string())
            .header("X-BAPI-SIGN", sig);
        if self.creds.is_rsa() {
            rb
        } else {
            rb.header("X-BAPI-SIGN-TYPE", "2")
        }
    }

    /// The HTTP status and body; `NotSent` when the connection failed,
    /// `NoAnswer` when no answer came. A status other than 2xx keeps its
    /// status when its body is cut, so that a 401 or 403 still stops or
    /// pauses (`classify` uses the body of such a status only in the
    /// message; review 2 S1).
    async fn send(&self, rb: reqwest::RequestBuilder) -> Result<(u16, String), RestError> {
        let resp = rb.send().await.map_err(|e| {
            let text = error_chain(&e);
            if e.is_connect() || e.is_builder() {
                RestError::NotSent(text)
            } else {
                RestError::NoAnswer(text)
            }
        })?;
        let status = resp.status().as_u16();
        match resp.text().await {
            Ok(text) => Ok((status, text)),
            Err(e) if !(200..=299).contains(&status) => {
                Ok((status, format!("(body cut: {})", error_chain(&e))))
            }
            Err(e) => Err(RestError::NoAnswer(error_chain(&e))),
        }
    }
}
