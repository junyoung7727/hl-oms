//! The watcher's own Bybit v5 REST client (C-WATCH part 3), written from
//! the venue's documentation, not from the OMS's code: signed GETs, the
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
use crate::sign;

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

/// The API key and secret. `Debug` never prints the secret.
pub struct Credentials {
    api_key: String,
    api_secret: String,
}

impl Credentials {
    pub fn new(api_key: &str, api_secret: &str) -> Credentials {
        Credentials {
            api_key: api_key.to_string(),
            api_secret: api_secret.to_string(),
        }
    }

    pub fn api_key(&self) -> &str {
        &self.api_key
    }
}

impl std::fmt::Debug for Credentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Credentials {{ api_key: {:?}, api_secret: <hidden> }}",
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

/// How long no request (Bybit or the IP echo) goes out after HTTP 403
/// (F18 L186: "wait for at least 10 minutes").
pub const IP_BAN_PAUSE_MS: u64 = 600_000;

/// A message of the venue as it goes into an error and a log line: at
/// most 200 characters, control characters (line breaks) as spaces, so one
/// error line stays one line.
fn one_line(text: &str) -> String {
    text.chars()
        .take(200)
        .map(|c| if c.is_control() { ' ' } else { c })
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

/// The `endpoint` label of the IP echo (its URL is config, not a label).
pub const ECHO: &str = "ip_echo";

/// How long the client sleeps at most before it reads the clock again
/// while it waits at the guard (판단(모델 아님)).
const GUARD_POLL_MS: u64 = 100;

/// The signed GET client. One per process: the guard, the key rejection
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

#[derive(Default)]
struct Stop {
    guard: Guard,
    /// The code that stopped every Bybit request until restart.
    auth: Option<i64>,
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

    /// The end (ms) of the last 403 pause, if there was one. The start
    /// checks wait for it before their retry (U19a4b).
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
        self.metrics
            .add("watcher_requests_total", &[("endpoint", path)], 1.0);
        let url = if q.is_empty() {
            format!("{}{path}", self.base)
        } else {
            format!("{}{path}?{q}", self.base)
        };
        let ts = self.clock.now_ms();
        let sig = sign::signature(
            &self.creds.api_secret,
            ts,
            &self.creds.api_key,
            self.recv_window_ms,
            &q,
        );
        let rb = self
            .http
            .get(url)
            .header("X-BAPI-API-KEY", &self.creds.api_key)
            .header("X-BAPI-TIMESTAMP", ts.to_string())
            .header("X-BAPI-RECV-WINDOW", self.recv_window_ms.to_string())
            .header("X-BAPI-SIGN", sig)
            .header("X-BAPI-SIGN-TYPE", "2");
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

    /// The HTTP status and body; `NotSent` when the connection failed,
    /// `NoAnswer` when no answer came.
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
        let text = resp
            .text()
            .await
            .map_err(|e| RestError::NoAnswer(error_chain(&e)))?;
        Ok((status, text))
    }
}
