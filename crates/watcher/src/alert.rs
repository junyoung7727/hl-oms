//! The watcher's own Telegram sender (C-WATCH part 10, [ADR-032]): written
//! anew from the delivery rules of draft PR #3, as `alert-direct` implements
//! them for the OMS, without linking that crate (independence, part 1).
//! U19b1a holds the pure parts: the bot token, the message and its split
//! into parts, the outbox, the outcome of an answer and the repeat rule.
//! U19b1b the sender task ([`spawn`]): two routes (DNS, then the host
//! pinned to Telegram's address), woken by a `Notify` (a new message, or
//! the runtime's tick when the retry flush is due on the injected clock),
//! the alert metrics and the shutdown flush. U19b1c the P1 conditions
//! ([`Alerts`]: the mismatch messages, the key stop, the public IP, the
//! start refusal) and their texts; the runtime starts the sender before
//! the start checks and drives `Alerts` at every tick.
//!
//! The token is never written to an error, a log or `Debug`.
//!
//! [ADR-032]: ../../docs/adr/ADR-032-watcher-kill-and-alerts.md

use std::collections::{BTreeMap, VecDeque};
use std::fmt;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::Notify;

use crate::clock::Clock;
use crate::engine::{Action, Check};
use crate::log::Log;
use crate::metrics::Metrics;
use crate::rest::{AuthStop, Client, OWN_RULE_CODE};
use crate::start::PublicIp;

/// The prefix of every message of the watcher (design §7.7, Q18: the
/// same bot as the OMS, told apart by the prefix).
pub const PREFIX: &str = "[WATCH P1]";
/// The venue label of the header; the watcher watches Bybit only.
pub const VENUE: &str = "bybit";
/// Telegram's limit is 4,096 UTF-16 units per message; this leaves room
/// (PR #3).
pub const MSG_LIMIT: usize = 3500;
/// Parts kept for delivery (PR #3).
pub const OUTBOX_LIMIT: usize = 50;
/// The repeat period of a lasting condition (design §7.7).
pub const REPEAT_MS: u64 = 300_000;

/// The bot token and the chat of the token file (PR #3 format,
/// `{"token": "<digits>:<chars>", "chat_id": …}`; read by
/// [`crate::creds::load_token`]).
#[derive(Clone, PartialEq, Eq)]
pub struct Token {
    token: String,
    chat_id: String,
}

impl fmt::Debug for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Token {{ chat_id: {:?}, token: <hidden> }}",
            self.chat_id
        )
    }
}

/// `<digits>:<[A-Za-z0-9_-]+>`, both sides non-empty.
fn token_ok(t: &str) -> bool {
    match t.split_once(':') {
        Some((id, key)) => {
            !id.is_empty()
                && id.bytes().all(|b| b.is_ascii_digit())
                && !key.is_empty()
                && key
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        }
        None => false,
    }
}

impl Token {
    /// A token of the form `<digits>:<[A-Za-z0-9_-]+>` and a non-empty
    /// chat id. The refusal does not quote the token.
    pub fn new(token: &str, chat_id: &str) -> Result<Token, String> {
        if !token_ok(token) {
            return Err("the token is not of the form <digits>:<letters, digits, _ or ->".into());
        }
        if chat_id.is_empty() {
            return Err("chat_id must be a non-empty string or an integer".into());
        }
        Ok(Token {
            token: token.to_string(),
            chat_id: chat_id.to_string(),
        })
    }

    pub fn chat_id(&self) -> &str {
        &self.chat_id
    }
}

/// `MM-DD HH:MM:SS KST` of a Unix time in ms (seconds cut).
pub fn kst(ms: u64) -> String {
    let s = ms / 1000 + 9 * 3600;
    let days = (s / 86_400) as i64;
    let sec = s % 86_400;
    // Civil date from days since 1970-01-01 (H. Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    format!(
        "{m:02}-{d:02} {:02}:{:02}:{:02} KST",
        sec / 3600,
        sec / 60 % 60,
        sec % 60
    )
}

/// `[WATCH P1] bybit {name} {MM-DD HH:MM:SS KST}\n{body}`: `name` is the
/// watcher's config name, the time the injected clock's.
pub fn message(name: &str, now_ms: u64, body: &str) -> String {
    format!("{PREFIX} {VENUE} {name} {}\n{body}", kst(now_ms))
}

fn units(s: &str) -> usize {
    s.encode_utf16().count()
}

/// Cuts `text` at line ends so that each part is within `limit` UTF-16
/// units; one overlong line is cut by characters (PR #3 `split_message`).
/// No part is empty unless the text is.
pub fn split(text: &str, limit: usize) -> Vec<String> {
    let limit = limit.max(2);
    let mut parts = Vec::new();
    let mut cur = String::new();
    for line in text.split('\n') {
        let mut line = line;
        while units(line) > limit {
            let mut n = 0;
            let mut used = 0;
            for c in line.chars() {
                if used + c.len_utf16() > limit {
                    break;
                }
                used += c.len_utf16();
                n += c.len_utf8();
            }
            if !cur.is_empty() {
                parts.push(std::mem::take(&mut cur));
            }
            parts.push(line[..n].to_string());
            line = &line[n..];
        }
        if cur.is_empty() {
            cur = line.to_string();
        } else if units(&cur) + 1 + units(line) > limit {
            parts.push(std::mem::replace(&mut cur, line.to_string()));
        } else {
            cur.push('\n');
            cur.push_str(line);
        }
    }
    if !cur.is_empty() || parts.is_empty() {
        parts.push(cur);
    }
    parts
}

/// The outcome of one send to Telegram.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Delivered,
    /// Telegram refused this message for good (400, 413: it is about this
    /// message): drop the part and count it.
    Refused(String),
    /// Anything else, and a network error: keep the part and end this
    /// flush.
    Retry(String),
}

/// The outcome of Telegram's answer `status` with `body` (PR #3):
/// `Delivered` for a 2xx answer whose body is JSON with `"ok": true`,
/// `Refused` for 400 and 413, `Retry` for everything else. The text names
/// the status and Telegram's `description` in one line of at most 200
/// characters; it never holds the request, so never the token.
pub fn outcome(status: u16, body: &[u8]) -> Outcome {
    let data = serde_json::from_slice::<serde_json::Value>(body).unwrap_or_default();
    if (200..300).contains(&status) && data["ok"].as_bool() == Some(true) {
        return Outcome::Delivered;
    }
    let description: String = data["description"]
        .as_str()
        .unwrap_or("")
        .chars()
        .take(200)
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let text = format!("telegram answered {status}: {description}");
    match status {
        400 | 413 => Outcome::Refused(text),
        _ => Outcome::Retry(text),
    }
}

/// One part handed out for sending; `id = None` is the drop notice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Part {
    pub id: Option<u64>,
    pub text: String,
    /// The drops a notice reports.
    pub count: u64,
}

/// Parts not yet delivered (PR #3 `Outbox`). Overflow drops the oldest
/// and counts it; once the queue is empty, one notice reports the count.
#[derive(Debug)]
pub struct Outbox {
    queue: VecDeque<(u64, String)>,
    next_id: u64,
    limit: usize,
    dropped: u64,
    dropped_total: u64,
}

impl Outbox {
    pub fn new(limit: usize) -> Outbox {
        Outbox {
            queue: VecDeque::new(),
            next_id: 0,
            limit: limit.max(1),
            dropped: 0,
            dropped_total: 0,
        }
    }

    /// Adds a message, split into parts of at most [`MSG_LIMIT`] units.
    pub fn add(&mut self, msg: &str) {
        for p in split(msg, MSG_LIMIT) {
            self.queue.push_back((self.next_id, p));
            self.next_id += 1;
        }
        while self.queue.len() > self.limit {
            self.queue.pop_front();
            self.count_drop();
        }
    }

    fn count_drop(&mut self) {
        self.dropped += 1;
        self.dropped_total += 1;
    }

    pub fn len(&self) -> usize {
        self.queue.len()
    }

    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    /// Drops not yet reported by a notice.
    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    pub fn dropped_total(&self) -> u64 {
        self.dropped_total
    }

    /// Nothing to send: no part and no notice owed.
    pub fn settled(&self) -> bool {
        self.queue.is_empty() && self.dropped == 0
    }

    /// The next part to send, or the drop notice
    /// `[WATCH P1] 전송 실패로 알림 N개를 버림` when the queue is empty.
    pub fn next(&self) -> Option<Part> {
        match self.queue.front() {
            Some((id, t)) => Some(Part {
                id: Some(*id),
                text: t.clone(),
                count: 0,
            }),
            None if self.dropped > 0 => Some(Part {
                id: None,
                text: format!("{PREFIX} 전송 실패로 알림 {}개를 버림", self.dropped),
                count: self.dropped,
            }),
            None => None,
        }
    }

    /// Applies the outcome of sending `part`; `false` ends the flush. A
    /// part that overflow dropped while it was being sent is not removed
    /// twice nor counted twice.
    pub fn done(&mut self, part: &Part, outcome: &Outcome) -> bool {
        let front = part.id.is_some() && self.queue.front().map(|(id, _)| *id) == part.id;
        match (outcome, part.id) {
            (Outcome::Retry(_), _) => false,
            (Outcome::Delivered, Some(_)) => {
                if front {
                    self.queue.pop_front();
                }
                true
            }
            (Outcome::Refused(_), Some(_)) => {
                if front {
                    self.queue.pop_front();
                    self.count_drop();
                }
                true
            }
            // The notice: delivered or refused, its count is told or
            // cannot be; drops after it was handed out stay owed.
            (_, None) => {
                self.dropped = self.dropped.saturating_sub(part.count);
                true
            }
        }
    }
}

#[derive(Debug, Clone)]
struct Cond {
    first_text: String,
    last_sent_ms: u64,
}

/// The repeat rule of design §7.7: once at start, every `period_ms` while
/// the condition lasts (with the latest text), once at clear
/// (`해소: <start text>`). In memory only: a restart sends a lasting
/// condition's start text once more. Elapsed time is `saturating_sub`, so
/// a clock that steps back holds the repeat.
#[derive(Debug)]
pub struct Repeat {
    period_ms: u64,
    conds: BTreeMap<String, Cond>,
}

impl Repeat {
    pub fn new(period_ms: u64) -> Repeat {
        Repeat {
            period_ms: period_ms.max(1),
            conds: BTreeMap::new(),
        }
    }

    /// The condition `key` holds (`Some(text)`) or not (`None`) at
    /// `now_ms`; the body to send now, if any.
    pub fn set(&mut self, key: &str, text: Option<&str>, now_ms: u64) -> Option<String> {
        match (text, self.conds.get_mut(key)) {
            (Some(t), None) => {
                self.conds.insert(
                    key.to_string(),
                    Cond {
                        first_text: t.to_string(),
                        last_sent_ms: now_ms,
                    },
                );
                Some(t.to_string())
            }
            (Some(t), Some(c)) => {
                if now_ms.saturating_sub(c.last_sent_ms) >= self.period_ms {
                    c.last_sent_ms = now_ms;
                    Some(t.to_string())
                } else {
                    None
                }
            }
            (None, Some(_)) => self
                .conds
                .remove(key)
                .map(|c| format!("해소: {}", c.first_text)),
            (None, None) => None,
        }
    }
}

// ---------------------------------------------------------------- sender (U19b1b)

/// The Bot API base of the production routes (PR #3).
pub const TELEGRAM_BASE: &str = "https://api.telegram.org/";
/// The host the second production route pins (SNI and certificate).
pub const TELEGRAM_HOST: &str = "api.telegram.org";
/// The address the second production route pins the host to, when DNS
/// fails (PR #3).
pub const TELEGRAM_PINNED: SocketAddr =
    SocketAddr::new(IpAddr::V4(Ipv4Addr::new(149, 154, 167, 220)), 443);
/// Per request, real time (PR #3, design value).
pub const REQUEST_TIMEOUT_MS: u64 = 10_000;
/// Per connect and route, real time (PR #3, design value): a blocked first
/// route does not use up the shutdown flush.
pub const CONNECT_TIMEOUT_MS: u64 = 3_000;
/// A flush that left parts is tried again this long after it ended, on the
/// injected clock (PR #3, design value).
pub const FLUSH_EVERY_MS: u64 = 5_000;
/// The bound of the shutdown flush, real time (판단(모델 아님)).
pub const FINISH_MS: u64 = 5_000;

/// One way to reach the Bot API: a base URL, and optionally a host name
/// resolved to a fixed address (the URL's host still gives SNI and the
/// certificate). It holds no token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Route {
    base: String,
    resolve: Option<(String, SocketAddr)>,
}

impl Route {
    /// `base` resolved by DNS.
    pub fn direct(base: &str) -> Route {
        Route {
            base: with_slash(base),
            resolve: None,
        }
    }

    /// `base` whose `host` is resolved to `addr`.
    pub fn pinned(base: &str, host: &str, addr: SocketAddr) -> Route {
        Route {
            base: with_slash(base),
            resolve: Some((host.to_string(), addr)),
        }
    }

    /// The two production routes: DNS, then `api.telegram.org` pinned to
    /// 149.154.167.220:443.
    pub fn telegram() -> Vec<Route> {
        vec![
            Route::direct(TELEGRAM_BASE),
            Route::pinned(TELEGRAM_BASE, TELEGRAM_HOST, TELEGRAM_PINNED),
        ]
    }
}

fn with_slash(base: &str) -> String {
    if base.ends_with('/') {
        base.to_string()
    } else {
        format!("{base}/")
    }
}

/// The alert series (C-WATCH part 10).
const QUEUED: &str = "watcher_alerts_queued";
const DELIVERED: &str = "watcher_alerts_delivered_total";
const DROPPED: &str = "watcher_alerts_dropped_total";
const FAILURES: &str = "watcher_alert_failures_total";

struct Inner {
    outbox: Outbox,
    repeat: Repeat,
    /// When the runtime's tick wakes the task again: set when a flush ended
    /// with parts left, cleared when a flush starts.
    next_flush: Option<u64>,
    stop: bool,
}

struct Shared {
    name: String,
    clock: Arc<dyn Clock>,
    metrics: Arc<Metrics>,
    inner: Mutex<Inner>,
    wake: Notify,
}

impl Shared {
    /// The gauge and the drop counter follow the outbox.
    fn publish(&self, i: &Inner) {
        self.metrics.set(QUEUED, &[], i.outbox.len() as f64);
        self.metrics
            .set(DROPPED, &[], i.outbox.dropped_total() as f64);
    }

    fn add(&self, i: &mut Inner, body: &str) {
        let msg = message(&self.name, self.clock.now_ms(), body);
        i.outbox.add(&msg);
        self.publish(i);
        self.wake.notify_one();
    }
}

/// What callers use: each call only takes a lock; nothing that calls it
/// waits for the network. Clones share the one sender.
#[derive(Clone)]
pub struct Handle(Arc<Shared>);

impl Handle {
    /// Queues one message with the `[WATCH P1]` header and wakes the
    /// sender.
    pub fn once(&self, body: &str) {
        if let Ok(mut i) = self.0.inner.lock() {
            self.0.add(&mut i, body);
        }
    }

    /// The condition `key` holds (`Some(text)`) or not (`None`) now: what
    /// the repeat rule gives is queued (start, every 5 min, `해소:`).
    pub fn set(&self, key: &str, text: Option<&str>) {
        if let Ok(mut i) = self.0.inner.lock() {
            let now = self.0.clock.now_ms();
            if let Some(body) = i.repeat.set(key, text, now) {
                self.0.add(&mut i, &body);
            }
        }
    }

    /// The instant on the injected clock at which the next retry flush is
    /// due; `None` when no flush left parts.
    pub fn next_flush(&self) -> Option<u64> {
        self.0.inner.lock().ok().and_then(|i| i.next_flush)
    }

    /// The runtime's tick: wakes the sender when [`Handle::next_flush`] is
    /// due at `now_ms`.
    pub fn tick(&self, now_ms: u64) {
        if self.next_flush().is_some_and(|t| now_ms >= t) {
            self.0.wake.notify_one();
        }
    }
}

/// The sender task; [`Task::finish`] ends it.
pub struct Task {
    shared: Arc<Shared>,
    join: tokio::task::JoinHandle<()>,
    log: Log,
}

impl Task {
    /// The task ended (before [`Task::finish`]: a bug).
    pub fn ended(&self) -> bool {
        self.join.is_finished()
    }

    /// Shutdown: one last flush, bounded by `bound_ms` of real time; what
    /// is left is dropped and counted in one log line. Returns the parts
    /// left.
    pub async fn finish(mut self, bound_ms: u64) -> usize {
        if let Ok(mut i) = self.shared.inner.lock() {
            i.stop = true;
        }
        self.shared.wake.notify_one();
        let ended = tokio::time::timeout(Duration::from_millis(bound_ms), &mut self.join)
            .await
            .is_ok();
        if !ended {
            self.join.abort();
        }
        let (left, notice) = match self.shared.inner.lock() {
            Ok(i) => (i.outbox.len(), i.outbox.dropped()),
            Err(_) => (0, 0),
        };
        if left == 0 && notice == 0 {
            (self.log)("alert sender stopped: nothing left to send");
        } else {
            (self.log)(&format!(
                "warn: alert sender stopped{}: {left} parts not delivered are dropped{}",
                if ended { "" } else { " at its bound" },
                if notice > 0 {
                    format!(" (and the notice of {notice} earlier drops)")
                } else {
                    String::new()
                }
            ));
        }
        left
    }
}

/// Builds one client per route and starts the sender task on the current
/// tokio runtime. `name` is the watcher's config name (the header).
pub fn spawn(
    name: &str,
    routes: Vec<Route>,
    token: Token,
    clock: Arc<dyn Clock>,
    metrics: Arc<Metrics>,
    log: Log,
) -> Result<(Handle, Task), String> {
    if routes.is_empty() {
        return Err("no route to Telegram".into());
    }
    let mut clients = Vec::new();
    for r in &routes {
        let mut b = reqwest::Client::builder()
            .timeout(Duration::from_millis(REQUEST_TIMEOUT_MS))
            .connect_timeout(Duration::from_millis(CONNECT_TIMEOUT_MS))
            // A redirect would carry the token elsewhere.
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy();
        if let Some((host, addr)) = &r.resolve {
            b = b.resolve(host, *addr);
        }
        let c = b
            .build()
            .map_err(|e| format!("cannot build the Telegram client: {}", chain(e)))?;
        clients.push((c, format!("{}bot{}/sendMessage", r.base, token.token)));
    }
    metrics.add(DELIVERED, &[], 0.0);
    metrics.add(FAILURES, &[], 0.0);
    let shared = Arc::new(Shared {
        name: name.to_string(),
        clock,
        metrics,
        inner: Mutex::new(Inner {
            outbox: Outbox::new(OUTBOX_LIMIT),
            repeat: Repeat::new(REPEAT_MS),
            next_flush: None,
            stop: false,
        }),
        wake: Notify::new(),
    });
    if let Ok(i) = shared.inner.lock() {
        shared.publish(&i);
    }
    let sender = Sender {
        shared: shared.clone(),
        clients,
        order: (0..routes.len()).collect(),
        chat: token.chat_id.clone(),
        log: log.clone(),
    };
    let join = tokio::spawn(sender.run());
    Ok((Handle(shared.clone()), Task { shared, join, log }))
}

/// The error and its causes in one line, without the URL (it holds the
/// token).
fn chain(e: reqwest::Error) -> String {
    let e = e.without_url();
    let mut text = e.to_string();
    let mut cause = std::error::Error::source(&e);
    while let Some(c) = cause {
        text.push_str(": ");
        text.push_str(&c.to_string());
        cause = c.source();
    }
    text
}

struct Sender {
    shared: Arc<Shared>,
    /// Per route: the client and the `sendMessage` URL (with the token).
    clients: Vec<(reqwest::Client, String)>,
    /// Route indexes in the order the next send tries them.
    order: Vec<usize>,
    chat: String,
    log: Log,
}

impl Sender {
    async fn run(mut self) {
        loop {
            self.shared.wake.notified().await;
            let stop = match self.shared.inner.lock() {
                Ok(mut i) => {
                    i.next_flush = None;
                    i.stop
                }
                Err(_) => return,
            };
            self.flush().await;
            if stop {
                return;
            }
        }
    }

    /// Sends parts in order, one at a time, until the outbox is settled or
    /// a part is kept (`Retry`); then the next flush is due
    /// [`FLUSH_EVERY_MS`] after this one ended.
    async fn flush(&mut self) {
        loop {
            let part = match self.shared.inner.lock() {
                Ok(i) => match i.outbox.next() {
                    Some(p) => p,
                    None => return,
                },
                Err(_) => return,
            };
            let (outcome, errors) = self.send(&part.text).await;
            for e in &errors {
                (self.log)(&format!("warn: telegram send failed: {e}"));
            }
            let m = &self.shared.metrics;
            m.add(FAILURES, &[], errors.len() as f64);
            if outcome == Outcome::Delivered {
                m.add(DELIVERED, &[], 1.0);
            }
            let Ok(mut i) = self.shared.inner.lock() else {
                return;
            };
            let go_on = i.outbox.done(&part, &outcome);
            self.shared.publish(&i);
            if !go_on {
                i.next_flush = Some(self.shared.clock.now_ms().saturating_add(FLUSH_EVERY_MS));
                return;
            }
        }
    }

    /// Tries the routes in order: a network error moves to the next route,
    /// an HTTP answer decides, and the route that answered goes first next
    /// time. The errors name the route, never the token.
    async fn send(&mut self, text: &str) -> (Outcome, Vec<String>) {
        let body = serde_json::json!({
            "chat_id": self.chat,
            "text": text,
            "disable_web_page_preview": true,
        })
        .to_string();
        let mut errors = Vec::new();
        for n in 0..self.order.len() {
            let r = self.order[n];
            let Some((client, url)) = self.clients.get(r) else {
                continue;
            };
            // A timeout after delivery can send twice: a duplicate is
            // better than a lost P1 (PR #3).
            let sent = client
                .post(url.as_str())
                .header("content-type", "application/json")
                .body(body.clone())
                .send()
                .await;
            let answer = match sent {
                Ok(resp) => {
                    let status = resp.status().as_u16();
                    resp.bytes().await.map(|b| (status, b))
                }
                Err(e) => Err(e),
            };
            let (status, bytes) = match answer {
                Ok(a) => a,
                Err(e) => {
                    errors.push(format!("route {r}: {}", chain(e)));
                    continue;
                }
            };
            self.order.remove(n);
            self.order.insert(0, r);
            let out = outcome(status, &bytes);
            match &out {
                Outcome::Delivered => {}
                Outcome::Refused(t) => errors.push(format!("route {r}: {t} (part dropped)")),
                Outcome::Retry(t) => errors.push(format!("route {r}: {t}")),
            }
            return (out, errors);
        }
        let last = errors.last().cloned().unwrap_or_else(|| "no route".into());
        (Outcome::Retry(last), errors)
    }
}

// ---------------------------------------------------------------- P1 conditions (U19b1c)

/// At most one mismatch message per check this often, on the raw injected
/// clock (판단(모델 아님)).
pub const MISMATCH_PACE_MS: u64 = 60_000;
/// A detail in a mismatch message is cut to this many UTF-16 units, so
/// that five of them and the header fit one part (판단(모델 아님)).
pub const DETAIL_UNITS: usize = 600;
/// The details a mismatch message shows; the rest are counted.
pub const DETAILS_SHOWN: usize = 5;

/// `text` cut to at most `max` UTF-16 units at a character boundary.
fn cut_units(text: &str, max: usize) -> String {
    let mut used = 0;
    text.chars()
        .take_while(|c| {
            used += c.len_utf16();
            used <= max
        })
        .collect()
}

/// The body of a mismatch message: `k` identities of `check` confirmed,
/// `details` the first ones (at most [`DETAILS_SHOWN`]).
pub fn mismatch_text(check: Check, k: u64, details: &[String]) -> String {
    let mut text = format!(
        "워처 {} 불일치 확정 {k}건. OMS에 일반 주문 차단(watcher_mismatch)을 보내기 시작함(실패하면 다시 보냄; 결과는 OMS의 hold 알림). {}",
        check.name(),
        details.first().map(String::as_str).unwrap_or("")
    );
    if k > 1 {
        let more: Vec<&str> = details.iter().skip(1).map(String::as_str).collect();
        text.push_str("\n그 밖에: ");
        text.push_str(&more.join("; "));
        let rest = k.saturating_sub(details.len() as u64);
        if rest > 0 {
            text.push_str(&format!("; 그리고 {rest}건 더"));
        }
    }
    text
}

#[derive(Debug, Default)]
struct Collected {
    last_sent_ms: Option<u64>,
    /// The first [`DETAILS_SHOWN`] details since the last message.
    details: Vec<String>,
    /// Identities confirmed since the last message.
    count: u64,
}

/// The mismatch P1 (C-WATCH part 10): one message per confirmation of an
/// identity ([`crate::engine::Action::Confirmed`]), at once for the first,
/// then at most one per check per [`MISMATCH_PACE_MS`]; what is confirmed
/// meanwhile is collected into the next. Sans-IO; elapsed time is
/// `saturating_sub`, so a clock that steps back holds the pace. Memory
/// only.
#[derive(Debug, Default)]
pub struct MismatchNotices {
    checks: BTreeMap<Check, Collected>,
}

impl MismatchNotices {
    /// Identities of `check` confirmed, one detail each.
    pub fn add(&mut self, check: Check, details: &[String]) {
        let c = self.checks.entry(check).or_default();
        for d in details {
            c.count += 1;
            if c.details.len() < DETAILS_SHOWN {
                c.details.push(cut_units(d, DETAIL_UNITS));
            }
        }
    }

    /// The message bodies due at `now_ms`.
    pub fn poll(&mut self, now_ms: u64) -> Vec<String> {
        let mut out = Vec::new();
        for (check, c) in &mut self.checks {
            if c.count == 0
                || c.last_sent_ms
                    .is_some_and(|t| now_ms.saturating_sub(t) < MISMATCH_PACE_MS)
            {
                continue;
            }
            out.push(mismatch_text(*check, c.count, &c.details));
            c.last_sent_ms = Some(now_ms);
            c.count = 0;
            c.details.clear();
        }
        out
    }

    /// At shutdown (review 1 S2): the bodies of everything still collected,
    /// whatever the pace, so that no confirmation goes untold.
    pub fn drain(&mut self) -> Vec<String> {
        let mut out = Vec::new();
        for (check, c) in &mut self.checks {
            if c.count > 0 {
                out.push(mismatch_text(*check, c.count, &c.details));
            }
            c.count = 0;
            c.details.clear();
        }
        out
    }
}

/// The repeat-rule key of the key stop.
pub const AUTH: &str = "auth";
/// The repeat-rule key of the public IP outside the key's list.
pub const PUBLIC_IP: &str = "public_ip";

/// What the watcher can no longer do while its key is stopped (the tail
/// of both `auth` texts).
const KEY_TAIL: &str = "워처의 Bybit 요청이 모두 멈춤(재시작 전까지): W1~W4는 성공하지 않아 OMS가 일반 주문을 막고, 워처의 일괄 취소(W5)도 나갈 수 없음. 키·IP 목록을 확인하고 워처를 다시 시작";

fn chars(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

/// The body of the `auth` P1: a rejected key (`retCode {code}` or
/// `HTTP 401`) or the watcher's own rule stop (code −1).
pub fn auth_text(s: &AuthStop) -> String {
    let text = chars(&s.text, 200);
    if s.code == OWN_RULE_CODE {
        format!("워처 키가 시작 규칙을 더는 지키지 않음({text}). {KEY_TAIL}")
    } else if s.http {
        format!("워처 키가 거부됨(HTTP {}: {text}). {KEY_TAIL}", s.code)
    } else {
        format!("워처 키가 거부됨(retCode {}: {text}). {KEY_TAIL}", s.code)
    }
}

/// The body of the `public_ip` P1; `why` as the start checks give it.
pub fn public_ip_text(why: &str) -> String {
    format!(
        "워처의 공인 IP가 키의 IP 목록에 없음({why}). Bybit가 10010으로 거부할 수 있음(그러면 워처 키 거부 P1이 따름). 이 호스트의 공인 IP와 키 설정을 확인"
    )
}

/// The body of the start-refusal P1 (sent once, before exit 2).
pub fn refusal_text(reason: &str) -> String {
    format!(
        "워처 시작 거부: {}. 워처가 꺼짐: W1~W4와 W5가 돌지 않음",
        chars(reason, 200)
    )
}

enum Out {
    Telegram(Handle),
    /// Testnet without `[telegram]`: every P1 is one `warn` line, with the
    /// repeat rule of its own.
    Log {
        log: Log,
        clock: Arc<dyn Clock>,
        repeat: Repeat,
    },
}

/// The watcher's P1 conditions as the runtime drives them (C-WATCH part
/// 10): the confirmations of the engine ([`Alerts::on_action`]), the key
/// stop and the public IP read at every tick ([`Alerts::poll`]), and the
/// one-off messages ([`Alerts::once`]). Each call only takes locks.
pub struct Alerts {
    out: Out,
    notices: MismatchNotices,
}

impl Alerts {
    /// P1s go to the sender.
    pub fn telegram(handle: Handle) -> Alerts {
        Alerts {
            out: Out::Telegram(handle),
            notices: MismatchNotices::default(),
        }
    }

    /// No `[telegram]` (testnet only): P1s go to the log as
    /// `warn: [WATCH P1] (no [telegram]: not sent) <first line>`.
    pub fn log_only(log: Log, clock: Arc<dyn Clock>) -> Alerts {
        Alerts {
            out: Out::Log {
                log,
                clock,
                repeat: Repeat::new(REPEAT_MS),
            },
            notices: MismatchNotices::default(),
        }
    }

    /// One message now.
    pub fn once(&mut self, body: &str) {
        match &self.out {
            Out::Telegram(h) => h.once(body),
            Out::Log { log, .. } => log_p1(log, body),
        }
    }

    /// The lasting condition `key` holds (`Some(text)`) or not, under the
    /// repeat rule.
    pub fn set(&mut self, key: &str, text: Option<&str>) {
        match &mut self.out {
            Out::Telegram(h) => h.set(key, text),
            Out::Log { log, clock, repeat } => {
                if let Some(body) = repeat.set(key, text, clock.now_ms()) {
                    log_p1(log, &body);
                }
            }
        }
    }

    /// An action of the engine: a confirmation is collected for the
    /// mismatch P1. Nothing else is (a resend is not a new confirmation).
    pub fn on_action(&mut self, a: &Action) {
        if let Action::Confirmed { check, details } = a {
            self.notices.add(*check, details);
        }
    }

    /// The runtime's tick at `now_ms`: the `auth` and `public_ip`
    /// conditions from their accessors, the mismatch messages that are due,
    /// and the sender's retry flush.
    pub fn poll(&mut self, now_ms: u64, client: &Client, ip: &PublicIp) {
        let auth = client.auth_stop();
        self.set(AUTH, auth.as_ref().map(auth_text).as_deref());
        let why = ip.get();
        self.set(PUBLIC_IP, why.as_deref().map(public_ip_text).as_deref());
        for body in self.notices.poll(now_ms) {
            self.once(&body);
        }
        if let Out::Telegram(h) = &self.out {
            h.tick(now_ms);
        }
    }

    /// Shutdown, before the sender's last flush: the mismatch messages
    /// still collected inside the pace are queued now.
    pub fn drain(&mut self) {
        for body in self.notices.drain() {
            self.once(&body);
        }
    }
}

fn log_p1(log: &Log, body: &str) {
    log(&format!(
        "warn: {PREFIX} (no [telegram]: not sent) {}",
        body.lines().next().unwrap_or("")
    ));
}
