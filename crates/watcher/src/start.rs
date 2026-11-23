//! The start checks and the IP recheck (C-WATCH part 4, U19a4b), as sans-IO
//! steps on the injected clock: [`Start`] runs steps 2–5 of the spec
//! (query-api, `watcher_api_key_days_left`, the public IP, the settle coin
//! of every symbol), [`IpCheck`] the recheck every `ip_check_ms` after it.
//! Neither sleeps nor sends: `poll(now)` names the next request, the caller
//! sends it ([`send`]) and gives the answer to `on_answer(now, …)`, which
//! says what comes next ([`Step`]). The U19a7 runtime drives both in its
//! loop; the tests drive them on a manual clock.
//!
//! Written from the venue's documentation (gateway facts Q4, Q8), not from
//! the OMS's start checks.

use std::net::IpAddr;
use std::sync::Arc;

use serde_json::Value;

use crate::config::Config;
use crate::log::Log;
use crate::metrics::Metrics;
use crate::rest::{Client, IP_BAN_PAUSE_MS, RestError};

pub const QUERY_API: &str = "/v5/user/query-api";
pub const INSTRUMENTS: &str = "/v5/market/instruments-info";

/// The settle coin of every traded symbol: the watcher's queries use
/// `category=linear&settleCoin=USDT`.
pub const SETTLE_COIN: &str = "USDT";

/// The pause before a start request that got no usable answer is asked
/// again (판단(모델 아님)); after HTTP 403 the 600 s pause instead.
pub const RETRY_MS: u64 = 5_000;

/// An `expiredAt` at or before this instant (1970-01-02T00:00:00Z) does
/// not apply: the venue's example for an IP-bound key is the epoch.
pub const NO_EXPIRY_UNTIL_MS: i64 = DAY_MS;
const DAY_MS: i64 = 86_400_000;

pub const DAYS_LEFT: &str = "watcher_api_key_days_left";
pub const PUBLIC_IP_OK: &str = "watcher_public_ip_ok";

/// A request of a start step or a recheck.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    /// A signed GET of Bybit.
    Venue {
        path: &'static str,
        params: Vec<(&'static str, String)>,
    },
    /// The unsigned GET of `ip_echo_url`. Its answer is the trimmed body
    /// as a JSON string ([`send`]).
    Echo,
}

/// What comes after an answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// The next request is due at `until_ms`; `poll` gives none before.
    Wait { until_ms: u64 },
    /// The next request is due now.
    Next,
    /// A start rule failed: the watcher exits with code 2 and this line.
    Refuse { reason: String },
    /// The key was rejected (`auth`) during the start: the client sends no
    /// Bybit request again, the watcher stays up and the start asks
    /// nothing more. After a rejection at step 5, [`Start::ip_check`] gives
    /// a recheck that asks the echo only.
    StayUpNoRequests,
    /// A recheck found that the key no longer passes the start rules:
    /// every Bybit request stops until restart, as for `auth`
    /// ([`apply`] tells the client), and `watcher_public_ip_ok` is 0. The
    /// recheck asks nothing more.
    StopRequests { reason: String },
    /// Every start check passed; [`Start::ip_check`] gives the recheck.
    Done,
}

/// Sends `req` with `client`; the echo's body comes back as a JSON string.
pub async fn send(client: &Client, echo_url: &str, req: &Request) -> Result<Value, RestError> {
    match req {
        Request::Venue { path, params } => client.get(path, params).await,
        Request::Echo => client.echo(echo_url).await.map(Value::String),
    }
}

/// What a step asks of the client: [`Step::StopRequests`] stops every
/// later Bybit request until restart.
pub fn apply(client: &Client, step: &Step) {
    if let Step::StopRequests { reason } = step {
        client.stop_key(reason);
    }
}

fn query_api() -> Request {
    Request::Venue {
        path: QUERY_API,
        params: vec![],
    }
}

/// A query-api answer, read against the start rules.
enum Key {
    /// The key passes; its bound IPs.
    Passes(Vec<String>),
    /// A readable answer that fails a rule.
    Fails(String),
    Unreadable(String),
}

/// Step 2's rules: a sub-account key (`isMaster == false`) of
/// `sub_account_uid`, bound to at least one IP address and not to `*`.
fn read_key(v: &Value, uid: u64) -> Key {
    let Some(master) = v.get("isMaster").and_then(Value::as_bool) else {
        return Key::Unreadable("isMaster is not a boolean".into());
    };
    if master {
        return Key::Fails(
            "the API key is the master account's; the watcher needs a sub-account key".into(),
        );
    }
    let Some(user) = v.get("userID").and_then(Value::as_u64) else {
        return Key::Unreadable("userID is not an integer".into());
    };
    if user != uid {
        return Key::Fails(format!(
            "the API key belongs to user {user}; the config says sub_account_uid {uid}"
        ));
    }
    let Some(ips) = v.get("ips").and_then(Value::as_array).and_then(|a| {
        a.iter()
            .map(|x| x.as_str().map(str::to_string))
            .collect::<Option<Vec<_>>>()
    }) else {
        return Key::Unreadable("ips is not a list of strings".into());
    };
    if ips.is_empty() || ips.iter().any(|s| s.trim() == "*") {
        return Key::Fails(
            "the API key is bound to no IP address (ips [] or \"*\"); bind it to this host's address"
                .into(),
        );
    }
    if !ips.iter().any(|s| address(s).is_some()) {
        return Key::Fails("the API key's ips hold no IP address".into());
    }
    Key::Passes(ips)
}

/// An IP address, IPv4-mapped IPv6 as IPv4.
fn address(s: &str) -> Option<IpAddr> {
    s.trim().parse::<IpAddr>().ok().map(|a| a.to_canonical())
}

/// Milliseconds since the epoch of an RFC 3339 UTC time
/// `YYYY-MM-DDTHH:MM:SSZ`, with fractional seconds accepted and dropped.
/// Nothing else is read (no offset, no lower-case letters, no space).
pub fn rfc3339_utc_ms(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() < 20 || b[b.len() - 1] != b'Z' {
        return None;
    }
    let digits = |from: usize, to: usize| -> Option<i64> {
        let part = b.get(from..to)?;
        if !part.iter().all(u8::is_ascii_digit) {
            return None;
        }
        part.iter()
            .try_fold(0i64, |n, d| Some(n * 10 + i64::from(d - b'0')))
    };
    let seps = [(4, b'-'), (7, b'-'), (10, b'T'), (13, b':'), (16, b':')];
    if seps.iter().any(|&(i, c)| b[i] != c) {
        return None;
    }
    let rest = &b[19..b.len() - 1];
    if let Some(frac) = rest.strip_prefix(b".") {
        if frac.is_empty() || !frac.iter().all(u8::is_ascii_digit) {
            return None;
        }
    } else if !rest.is_empty() {
        return None;
    }
    let (y, mo, d) = (digits(0, 4)?, digits(5, 7)?, digits(8, 10)?);
    let (h, mi, sec) = (digits(11, 13)?, digits(14, 16)?, digits(17, 19)?);
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let month_days = match mo {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return None,
    };
    if d < 1 || d > month_days || h > 23 || mi > 59 || sec > 59 {
        return None;
    }
    // Days from the civil date (H. Hinnant's algorithm).
    let y = if mo <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (mo + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(((days * 24 + h) * 60 + mi) * 60_000 + sec * 1000)
}

/// Step 3: `watcher_api_key_days_left` from `expiredAt`, rounded down;
/// removed when absent, `null`, at or before [`NO_EXPIRY_UNTIL_MS`], or
/// unreadable (one warn line).
fn export_days_left(v: &Value, now_ms: u64, metrics: &Metrics, log: &Log) {
    let at = match v.get("expiredAt") {
        None | Some(Value::Null) => None,
        Some(e) => match e.as_str().and_then(rfc3339_utc_ms) {
            Some(ms) => Some(ms),
            None => {
                metrics.remove(DAYS_LEFT, &[]);
                log(&format!(
                    "warn: query-api: expiredAt {} cannot be read; watcher_api_key_days_left is not exported",
                    short(e)
                ));
                return;
            }
        },
    };
    match at {
        Some(ms) if ms > NO_EXPIRY_UNTIL_MS => {
            let diff = ms - now_ms as i64;
            let days = diff.div_euclid(DAY_MS);
            metrics.set(DAYS_LEFT, &[], days as f64);
        }
        _ => metrics.remove(DAYS_LEFT, &[]),
    }
}

/// A venue value as it goes into a line: JSON, at most 60 characters.
fn short(v: &Value) -> String {
    v.to_string().chars().take(60).collect()
}

/// Step 4: `watcher_public_ip_ok` from the echo's answer: 1 if its address
/// is one of `ips`; 0 with one warn line if not, if the echo failed or if
/// its body is not an address (an unknown IP is not ok).
fn export_public_ip(
    answer: &Result<Value, RestError>,
    ips: &[String],
    metrics: &Metrics,
    log: &Log,
) {
    let why = match answer {
        Err(e) => format!("unknown: the IP echo failed ({}): {e}", e.class()),
        Ok(body) => match body.as_str().and_then(address) {
            None => format!(
                "unknown: the IP echo's body {} is not an address",
                short(body)
            ),
            Some(ip) if ips.iter().any(|s| address(s) == Some(ip)) => {
                metrics.set(PUBLIC_IP_OK, &[], 1.0);
                return;
            }
            Some(ip) => format!("{ip} is not among the key's bound IPs"),
        },
    };
    metrics.set(PUBLIC_IP_OK, &[], 0.0);
    log(&format!("warn: public IP {why}; watcher_public_ip_ok = 0"));
}

/// An instruments-info answer for `symbol`, read against step 5's rule.
fn read_settle(v: &Value, symbol: &str) -> Key {
    let Some(list) = v.get("list").and_then(Value::as_array) else {
        return Key::Unreadable("instruments-info: list is not a list".into());
    };
    let Some(row) = list
        .iter()
        .find(|r| r.get("symbol").and_then(Value::as_str) == Some(symbol))
    else {
        return Key::Fails(format!(
            "{symbol} is not in instruments-info for category linear; the watcher's scope is linear/USDT"
        ));
    };
    let coin = row.get("settleCoin");
    if coin.and_then(Value::as_str) == Some(SETTLE_COIN) {
        return Key::Passes(vec![]);
    }
    let coin = match coin {
        Some(Value::String(c))
            if !c.is_empty() && c.chars().all(|ch| ch.is_ascii_alphanumeric()) =>
        {
            c.clone()
        }
        Some(c) => short(c),
        None => "no settleCoin".into(),
    };
    Key::Fails(format!(
        "{symbol} settles in {coin}; the watcher's scope is linear/USDT"
    ))
}

/// The instant a start request is asked again after `e`: the end of the
/// 600 s pause after HTTP 403 (the clock at the answer + 600 s, as the
/// client's pause), else [`RETRY_MS`] after the answer.
fn retry_at(now_ms: u64, e: &RestError) -> u64 {
    match e {
        RestError::IpBanned(_) => now_ms.saturating_add(IP_BAN_PAUSE_MS),
        _ => now_ms.saturating_add(RETRY_MS),
    }
}

/// The error of a retCode 0 answer that the start rules cannot read,
/// counted under `endpoint` (by the check, not the client).
fn unreadable(metrics: &Metrics, endpoint: &str, why: String) -> RestError {
    let labels = [("endpoint", endpoint), ("class", "unreadable")];
    metrics.add("watcher_request_errors_total", &labels, 1.0);
    RestError::Unreadable(why)
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Phase {
    QueryApi,
    Echo,
    Symbol(usize),
    Done,
    StayedUp,
    Refused(String),
}

/// Steps 2–5 of the start, in order. The start goes on only after each
/// step has a readable answer (the echo excepted: its failure is a 0, not
/// a wait).
pub struct Start {
    uid: u64,
    symbols: Vec<String>,
    ip_check_ms: u64,
    metrics: Arc<Metrics>,
    log: Log,
    phase: Phase,
    /// A request was given by `poll` and has no answer yet.
    in_flight: bool,
    /// No request before this instant.
    not_before: u64,
    ips: Vec<String>,
    /// The clock at the echo's answer (step 4).
    echo_at: Option<u64>,
}

impl Start {
    /// The start at `now_ms`; the first request (query-api) is due at once.
    pub fn new(cfg: &Config, metrics: Arc<Metrics>, log: Log, now_ms: u64) -> Start {
        Start {
            uid: cfg.sub_account_uid,
            symbols: cfg.symbols.clone(),
            ip_check_ms: cfg.intervals.ip_check_ms,
            metrics,
            log,
            phase: Phase::QueryApi,
            in_flight: false,
            not_before: now_ms,
            ips: Vec::new(),
            echo_at: None,
        }
    }

    /// The request due at `now_ms`, or `None` while one is in flight,
    /// while it waits, and after the end.
    pub fn poll(&mut self, now_ms: u64) -> Option<Request> {
        if self.in_flight || now_ms < self.not_before {
            return None;
        }
        let req = match &self.phase {
            Phase::QueryApi => query_api(),
            Phase::Echo => Request::Echo,
            Phase::Symbol(i) => Request::Venue {
                path: INSTRUMENTS,
                params: vec![
                    ("category", "linear".to_string()),
                    ("symbol", self.symbols[*i].clone()),
                ],
            },
            Phase::Done | Phase::StayedUp | Phase::Refused(_) => return None,
        };
        self.in_flight = true;
        Some(req)
    }

    /// The answer at `now_ms` to the request in flight. An answer without
    /// a request in flight changes nothing.
    pub fn on_answer(&mut self, now_ms: u64, answer: Result<Value, RestError>) -> Step {
        if !self.in_flight {
            return self.current(now_ms);
        }
        self.in_flight = false;
        match self.phase.clone() {
            Phase::QueryApi => match answer {
                Ok(v) => match read_key(&v, self.uid) {
                    Key::Passes(ips) => {
                        export_days_left(&v, now_ms, &self.metrics, &self.log);
                        self.ips = ips;
                        self.phase = Phase::Echo;
                        Step::Next
                    }
                    Key::Fails(why) => self.refuse(format!("query-api: {why}")),
                    Key::Unreadable(why) => {
                        let e = unreadable(&self.metrics, QUERY_API, why);
                        self.retry(now_ms, "query-api", e)
                    }
                },
                Err(e) => self.retry(now_ms, "query-api", e),
            },
            Phase::Echo => {
                export_public_ip(&answer, &self.ips, &self.metrics, &self.log);
                self.echo_at = Some(now_ms);
                self.next_symbol(0)
            }
            Phase::Symbol(i) => {
                let what = format!("instruments-info {}", self.symbols[i]);
                let answer = answer.map(|v| read_settle(&v, &self.symbols[i]));
                match answer {
                    Ok(Key::Passes(_)) => self.next_symbol(i + 1),
                    Ok(Key::Fails(why)) => self.refuse(why),
                    Ok(Key::Unreadable(why)) => {
                        let e = unreadable(&self.metrics, INSTRUMENTS, why);
                        self.retry(now_ms, &what, e)
                    }
                    Err(e) => self.retry(now_ms, &what, e),
                }
            }
            Phase::Done | Phase::StayedUp | Phase::Refused(_) => self.current(now_ms),
        }
    }

    /// The recheck, once every start check passed: the first is due
    /// `ip_check_ms` after the echo of step 4 answered. After a key
    /// rejected at step 5 a recheck of the echo alone against step 2's
    /// IPs (review 1 M1), so that a changed public IP still shows.
    pub fn ip_check(&self) -> Option<IpCheck> {
        let key_stopped = match self.phase {
            Phase::Done => false,
            Phase::StayedUp => true,
            _ => return None,
        };
        let at = self.echo_at?;
        Some(IpCheck {
            uid: self.uid,
            interval_ms: self.ip_check_ms,
            metrics: self.metrics.clone(),
            log: self.log.clone(),
            next_due: at.saturating_add(self.ip_check_ms),
            phase: Recheck::Idle,
            ips: self.ips.clone(),
            key_stopped,
        })
    }

    fn next_symbol(&mut self, i: usize) -> Step {
        if i < self.symbols.len() {
            self.phase = Phase::Symbol(i);
            Step::Next
        } else {
            self.phase = Phase::Done;
            Step::Done
        }
    }

    fn refuse(&mut self, reason: String) -> Step {
        self.phase = Phase::Refused(reason.clone());
        Step::Refuse { reason }
    }

    /// `auth`: stay up without requests (the client latched the key stop
    /// and wrote the error line). Any other class: one line and a wait.
    fn retry(&mut self, now_ms: u64, what: &str, e: RestError) -> Step {
        if let RestError::Auth { .. } = e {
            self.phase = Phase::StayedUp;
            return Step::StayUpNoRequests;
        }
        let until = retry_at(now_ms, &e);
        self.not_before = until;
        (self.log)(&format!(
            "start: {what} failed ({}): {e}; asked again at {until} ms",
            e.class()
        ));
        Step::Wait { until_ms: until }
    }

    fn current(&self, now_ms: u64) -> Step {
        match &self.phase {
            Phase::Done => Step::Done,
            Phase::StayedUp => Step::StayUpNoRequests,
            Phase::Refused(reason) => Step::Refuse {
                reason: reason.clone(),
            },
            _ if now_ms < self.not_before => Step::Wait {
                until_ms: self.not_before,
            },
            _ => Step::Next,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Recheck {
    /// Waiting for `next_due`.
    Idle,
    /// query-api in flight.
    Query,
    /// query-api passed; the echo is due now.
    EchoDue,
    /// The echo in flight.
    Echo,
    /// The key no longer passes the start rules (the reason): nothing is
    /// asked until restart.
    Stopped(String),
}

/// Step 6: every `ip_check_ms`, query-api again (the IP list may change)
/// and then the echo, refreshing `watcher_api_key_days_left` and
/// `watcher_public_ip_ok`. The next recheck is due `ip_check_ms` after the
/// previous one's last answer. A failed recheck (any class but `auth`)
/// sets `watcher_public_ip_ok = 0` and keeps the days left. After `auth`
/// (the client latched the key stop) it asks the echo alone, at once and
/// then at the same pace, against the last IPs that passed: the echo is
/// not a Bybit request, and a changed public IP is a usual cause of 10010
/// (review 1 M1). A readable query-api answer that fails a start rule
/// stops every Bybit request until restart ([`Step::StopRequests`]) and
/// sets `watcher_public_ip_ok = 0`. These requests go through the
/// client's guard and are outside the reconciliation budget.
pub struct IpCheck {
    uid: u64,
    interval_ms: u64,
    metrics: Arc<Metrics>,
    log: Log,
    next_due: u64,
    phase: Recheck,
    /// The key's IPs of the last query-api that passed.
    ips: Vec<String>,
    /// The key was rejected: the echo alone.
    key_stopped: bool,
}

impl IpCheck {
    /// The request due at `now_ms`, or `None` while one is in flight,
    /// before the next recheck is due, and after a stop.
    pub fn poll(&mut self, now_ms: u64) -> Option<Request> {
        match self.phase {
            Recheck::Idle if now_ms >= self.next_due && self.key_stopped => {
                self.phase = Recheck::Echo;
                Some(Request::Echo)
            }
            Recheck::Idle if now_ms >= self.next_due => {
                self.phase = Recheck::Query;
                Some(query_api())
            }
            Recheck::EchoDue => {
                self.phase = Recheck::Echo;
                Some(Request::Echo)
            }
            _ => None,
        }
    }

    /// The answer at `now_ms` to the request in flight. An answer without
    /// a request in flight changes nothing.
    pub fn on_answer(&mut self, now_ms: u64, answer: Result<Value, RestError>) -> Step {
        match self.phase {
            Recheck::Query => match answer {
                Ok(v) => match read_key(&v, self.uid) {
                    Key::Passes(ips) => {
                        export_days_left(&v, now_ms, &self.metrics, &self.log);
                        self.ips = ips;
                        self.phase = Recheck::EchoDue;
                        Step::Next
                    }
                    Key::Fails(why) => {
                        let reason =
                            format!("the key no longer passes the start check: query-api: {why}");
                        self.metrics.set(PUBLIC_IP_OK, &[], 0.0);
                        self.phase = Recheck::Stopped(reason.clone());
                        Step::StopRequests { reason }
                    }
                    Key::Unreadable(why) => {
                        let e = unreadable(&self.metrics, QUERY_API, why);
                        self.failed(now_ms, e)
                    }
                },
                Err(RestError::Auth { .. }) => {
                    self.key_stopped = true;
                    self.phase = Recheck::EchoDue;
                    Step::Next
                }
                Err(e) => self.failed(now_ms, e),
            },
            Recheck::Echo => {
                export_public_ip(&answer, &self.ips, &self.metrics, &self.log);
                self.wait(now_ms)
            }
            Recheck::Stopped(ref reason) => Step::StopRequests {
                reason: reason.clone(),
            },
            Recheck::EchoDue => Step::Next,
            Recheck::Idle => Step::Wait {
                until_ms: self.next_due,
            },
        }
    }

    /// A failed query-api: `watcher_public_ip_ok = 0` with one warn line,
    /// the days left kept, no echo, the next recheck at the normal pace.
    fn failed(&mut self, now_ms: u64, e: RestError) -> Step {
        self.metrics.set(PUBLIC_IP_OK, &[], 0.0);
        let step = self.wait(now_ms);
        (self.log)(&format!(
            "warn: IP recheck: query-api failed ({}): {e}; the next recheck at {} ms; watcher_public_ip_ok = 0",
            e.class(),
            self.next_due
        ));
        step
    }

    fn wait(&mut self, now_ms: u64) -> Step {
        self.phase = Recheck::Idle;
        self.next_due = now_ms.saturating_add(self.interval_ms);
        Step::Wait {
            until_ms: self.next_due,
        }
    }
}
