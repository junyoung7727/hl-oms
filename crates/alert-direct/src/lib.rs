//! Telegram alerts sent directly (design §7.7, unit U6 spec D1), independent
//! of the observability stack. The delivery rules are those of draft PR #3
//! (`tools/alert/hl_alert.py`): parts of at most 3,500 UTF-16 units, an
//! outbox of 50 parts that drops the oldest and counts them, 400 and 413
//! drop the part (it is about this message), every other answer or a network
//! error keeps it for the next flush, and a second route to a pinned
//! Telegram IP with the certificate checked for `api.telegram.org`.
//!
//! The sender runs on its own thread: [`AlertHandle::set`] and
//! [`AlertHandle::once`] only take a lock, so the caller never waits for the
//! network. The token is never written to an error, a log or `Debug`.
//!
//! A [`SecondChannel`] (U18a1, design §7.7), when configured, gets a copy of
//! every message through its own bounded outbox and thread, and a notice
//! when Telegram has failed for longer than `second_after_ms`.

use std::collections::{BTreeMap, VecDeque};
use std::fmt;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use reqwest::Url;

/// Telegram's limit is 4,096 UTF-16 units per message; this leaves room (PR #3).
pub const MSG_LIMIT: usize = 3500;
/// Parts kept for delivery (PR #3).
pub const OUTBOX_LIMIT: usize = 50;
/// The repeat period of a lasting condition (design §7.7).
pub const REPEAT_MS: i64 = 5 * 60 * 1000;
pub const TELEGRAM_BASE: &str = "https://api.telegram.org/";
pub const TELEGRAM_HOST: &str = "api.telegram.org";
/// The fallback route when DNS or the usual path to Telegram is blocked (PR #3).
pub const TELEGRAM_FALLBACK_IP: [u8; 4] = [149, 154, 167, 220];
/// Telegram failing this long is told on the second channel (design §7.7).
pub const SECOND_AFTER_MS: i64 = 60 * 1000;
/// The condition key of the Telegram-down notice (second channel only).
pub const TELEGRAM_DOWN: &str = "telegram_down";

/// Milliseconds since the Unix epoch.
pub trait Clock: Send + Sync {
    fn now_ms(&self) -> i64;
}

pub struct SystemClock;

impl Clock for SystemClock {
    fn now_ms(&self) -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
            .unwrap_or(0)
    }
}

// ---------------------------------------------------------------- config

/// Why a Telegram config was refused. The text never holds the token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    Io(String),
    /// Not a regular file, readable by others, or inside a repository.
    Unsafe(String),
    Invalid(String),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::Io(e) | ConfigError::Unsafe(e) | ConfigError::Invalid(e) => f.write_str(e),
        }
    }
}

/// The bot token and the chat (PR #3 file format:
/// `{"token": "<digits>:<chars>", "chat_id": "..."}`, mode 0600).
#[derive(Clone)]
pub struct TelegramConfig {
    token: String,
    chat_id: String,
}

impl fmt::Debug for TelegramConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TelegramConfig")
            .field("token", &"<hidden>")
            .field("chat_id", &self.chat_id)
            .finish()
    }
}

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

/// The first ancestor directory of `path` that holds `.git`, if any.
fn repository_of(path: &Path) -> Option<std::path::PathBuf> {
    let abs = std::path::absolute(path).ok()?;
    abs.ancestors()
        .skip(1)
        .find(|d| d.join(".git").exists())
        .map(Path::to_path_buf)
}

impl TelegramConfig {
    pub fn new(token: &str, chat_id: &str) -> Result<TelegramConfig, ConfigError> {
        if !token_ok(token) {
            return Err(ConfigError::Invalid(
                "token is not of the form <digits>:<letters>".into(),
            ));
        }
        if chat_id.is_empty() {
            return Err(ConfigError::Invalid("needs token and chat_id".into()));
        }
        Ok(TelegramConfig {
            token: token.to_string(),
            chat_id: chat_id.to_string(),
        })
    }

    /// Loads the file with every rule, including "not inside a repository".
    pub fn load(path: &Path) -> Result<TelegramConfig, ConfigError> {
        Self::load_with(path, true)
    }

    /// As [`TelegramConfig::load`]; `refuse_in_repo = false` skips only the
    /// repository rule (tests whose directories sit inside a checkout).
    pub fn load_with(path: &Path, refuse_in_repo: bool) -> Result<TelegramConfig, ConfigError> {
        use std::os::unix::fs::PermissionsExt;

        let shown = path.display();
        let meta = std::fs::symlink_metadata(path)
            .map_err(|e| ConfigError::Io(format!("{shown}: {}", e.kind())))?;
        if !meta.file_type().is_file() || meta.permissions().mode() & 0o077 != 0 {
            return Err(ConfigError::Unsafe(format!(
                "{shown}: must be a regular file only its owner can read (0600)"
            )));
        }
        if refuse_in_repo {
            if let Some(repo) = repository_of(path) {
                return Err(ConfigError::Unsafe(format!(
                    "{shown}: is inside the repository {}; keep the token outside any repository",
                    repo.display()
                )));
            }
        }
        let raw =
            std::fs::read(path).map_err(|e| ConfigError::Io(format!("{shown}: {}", e.kind())))?;
        let v: serde_json::Value = serde_json::from_slice(&raw)
            .map_err(|_| ConfigError::Invalid(format!("{shown}: not JSON")))?;
        let token = v["token"].as_str().unwrap_or_default();
        let chat = match &v["chat_id"] {
            serde_json::Value::String(s) => s.clone(),
            serde_json::Value::Number(n) => n.to_string(),
            _ => String::new(),
        };
        Self::new(token, &chat).map_err(|e| ConfigError::Invalid(format!("{shown}: {e}")))
    }

    pub fn chat_id(&self) -> &str {
        &self.chat_id
    }
}

// ---------------------------------------------------------------- split

fn units(s: &str) -> usize {
    s.encode_utf16().count()
}

/// Cuts at line ends so that each part is within `limit` UTF-16 units; one
/// overlong line is cut by characters (PR #3 `split_message`).
pub fn split_message(text: &str, limit: usize) -> Vec<String> {
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
    parts.push(cur);
    parts
}

// ---------------------------------------------------------------- outbox

/// The result of one send.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SendOutcome {
    Delivered,
    /// Telegram refused this message for good (400, 413): drop and count it.
    Refused(String),
    /// Anything else: keep the part and stop this flush.
    Retry(String),
}

/// One part handed out for sending: `id = None` is the drop notice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Part {
    pub id: Option<u64>,
    pub text: String,
    /// The drops a notice reports.
    pub count: u64,
}

/// Parts not yet delivered (PR #3 `Outbox`). Overflow drops the oldest and
/// counts it; after everything was delivered, one notice reports the count.
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

    /// Adds a message, split into parts.
    pub fn add(&mut self, msg: &str) {
        for p in split_message(msg, MSG_LIMIT) {
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

    /// The next part to send, or the drop notice when the queue is empty.
    pub fn next(&self, prefix: &str) -> Option<Part> {
        match self.queue.front() {
            Some((id, t)) => Some(Part {
                id: Some(*id),
                text: t.clone(),
                count: 0,
            }),
            None if self.dropped > 0 => Some(Part {
                id: None,
                text: format!("{prefix} 전송 실패로 알림 {}개를 버림", self.dropped),
                count: self.dropped,
            }),
            None => None,
        }
    }

    /// Applies the outcome of sending `part`; `false` ends the flush. A part
    /// that overflow dropped while it was being sent is not removed twice.
    pub fn done(&mut self, part: &Part, outcome: &SendOutcome) -> bool {
        let front = self.queue.front().map(|(id, _)| *id) == part.id && part.id.is_some();
        match (outcome, part.id) {
            (SendOutcome::Retry(_), _) => false,
            (SendOutcome::Delivered, Some(_)) => {
                if front {
                    self.queue.pop_front();
                }
                true
            }
            (SendOutcome::Refused(_), Some(_)) => {
                if front {
                    self.queue.pop_front();
                    self.count_drop();
                }
                true
            }
            // The notice: delivered or refused, the count is reported or
            // cannot be; drops after it was made stay owed.
            (_, None) => {
                self.dropped = self.dropped.saturating_sub(part.count);
                true
            }
        }
    }

    /// Sends until empty or a retryable failure (the pure form of the loop).
    pub fn flush(&mut self, send: &mut dyn FnMut(&str) -> SendOutcome, prefix: &str) {
        while let Some(p) = self.next(prefix) {
            let o = send(&p.text);
            if !self.done(&p, &o) {
                return;
            }
        }
    }
}

// ---------------------------------------------------------------- repeat rule

#[derive(Debug, Clone)]
struct Cond {
    first_text: String,
    last_sent_ms: i64,
}

/// The repeat rule of design §7.7: once at start, every `period_ms` while
/// the condition lasts (the latest text), once at clear.
#[derive(Debug)]
pub struct Repeat {
    period_ms: i64,
    conds: BTreeMap<String, Cond>,
}

impl Repeat {
    pub fn new(period_ms: i64) -> Repeat {
        Repeat {
            period_ms: period_ms.max(1),
            conds: BTreeMap::new(),
        }
    }

    /// The condition `key` holds (`Some(text)`) or not (`None`) at `now_ms`;
    /// returns the message body to send now, if any.
    pub fn set(&mut self, key: &str, text: Option<&str>, now_ms: i64) -> Option<String> {
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
                if now_ms - c.last_sent_ms >= self.period_ms {
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

    /// Keys of the conditions that hold.
    pub fn active(&self) -> Vec<String> {
        self.conds.keys().cloned().collect()
    }

    /// Drops a condition without a message; `true` when it held.
    pub fn forget(&mut self, key: &str) -> bool {
        self.conds.remove(key).is_some()
    }

    /// The state as JSON (spec R3: kept across restarts).
    pub fn to_json(&self) -> serde_json::Value {
        let conds: serde_json::Map<String, serde_json::Value> = self
            .conds
            .iter()
            .map(|(k, c)| {
                (
                    k.clone(),
                    serde_json::json!({"first_text": c.first_text, "last_sent_ms": c.last_sent_ms}),
                )
            })
            .collect();
        serde_json::json!({"conds": conds})
    }

    /// Restores [`Repeat::to_json`]; entries that do not parse are skipped.
    pub fn restore(&mut self, v: &serde_json::Value) {
        if let Some(m) = v["conds"].as_object() {
            for (k, c) in m {
                if let (Some(t), Some(ms)) = (c["first_text"].as_str(), c["last_sent_ms"].as_i64())
                {
                    self.conds.insert(
                        k.clone(),
                        Cond {
                            first_text: t.to_string(),
                            last_sent_ms: ms,
                        },
                    );
                }
            }
        }
    }
}

/// Writes the repeat state: temp file, fsync, rename, fsync of the folder.
fn save_state(path: &Path, v: &serde_json::Value) -> std::io::Result<()> {
    use std::io::Write;
    let tmp = path.with_extension("tmp");
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(v.to_string().as_bytes())?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path)?;
    if let Some(d) = path.parent() {
        if let Ok(d) = std::fs::File::open(d) {
            let _ = d.sync_all();
        }
    }
    Ok(())
}

fn load_state(path: &Path) -> Option<serde_json::Value> {
    let raw = std::fs::read(path).ok()?;
    match serde_json::from_slice(&raw) {
        Ok(v) => Some(v),
        Err(_) => {
            eprintln!(
                "{}",
                serde_json::json!({"level": "warn", "msg": "alert state file not understood; starting empty", "fields": {"path": path.display().to_string()}})
            );
            None
        }
    }
}

// ---------------------------------------------------------------- routes

/// One way to reach the Bot API: a base URL, and optionally a host name
/// resolved to a fixed address (SNI and certificate still use the host).
#[derive(Clone)]
pub struct Route {
    pub base: Url,
    pub resolve: Option<(String, SocketAddr)>,
}

impl fmt::Debug for Route {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Route({} via {:?})",
            self.base.as_str(),
            self.resolve.as_ref().map(|r| r.1)
        )
    }
}

impl Route {
    pub fn direct(base: &str) -> Result<Route, String> {
        Ok(Route {
            base: parse_base(base)?,
            resolve: None,
        })
    }

    pub fn pinned(base: &str, host: &str, addr: SocketAddr) -> Result<Route, String> {
        Ok(Route {
            base: parse_base(base)?,
            resolve: Some((host.to_string(), addr)),
        })
    }

    /// The two production routes: DNS, then the pinned Telegram IP.
    pub fn telegram() -> Vec<Route> {
        let base = Url::parse(TELEGRAM_BASE).ok();
        base.map(|b| {
            vec![
                Route {
                    base: b.clone(),
                    resolve: None,
                },
                Route {
                    base: b,
                    resolve: Some((
                        TELEGRAM_HOST.to_string(),
                        SocketAddr::from((TELEGRAM_FALLBACK_IP, 443)),
                    )),
                },
            ]
        })
        .unwrap_or_default()
    }
}

fn parse_base(s: &str) -> Result<Url, String> {
    let s = if s.ends_with('/') {
        s.to_string()
    } else {
        format!("{s}/")
    };
    Url::parse(&s).map_err(|e| format!("bad url {s}: {e}"))
}

// ---------------------------------------------------------------- sender

/// A channel used besides Telegram, for when Telegram cannot be reached
/// (design §7.7; which service is owner decision D3).
pub trait SecondChannel: Send + Sync {
    fn name(&self) -> &str;
    /// Blocking; called only on the `alert-second` thread. Err = keep the part and retry.
    /// The error text is logged: it must not hold a secret.
    fn send(&self, text: &str) -> Result<(), String>;
}

#[derive(Clone)]
pub struct AlertConfig {
    pub telegram: TelegramConfig,
    /// `[MD P1]`, `[OMS P1]`, `[WATCH P1]`.
    pub prefix: String,
    pub venue: String,
    pub routes: Vec<Route>,
    /// Per request (PR #3: 10 s).
    pub timeout: Duration,
    /// Retry period of a flush that stopped on a failure.
    pub flush_every: Duration,
    pub outbox_limit: usize,
    pub repeat_ms: i64,
    /// Connect timeout per route (spec R2: a blocked first route must not
    /// use up the whole exit flush).
    pub connect_timeout: Duration,
    /// Where the repeat state is kept across restarts (spec R3); `None`:
    /// only within this process.
    pub state_file: Option<std::path::PathBuf>,
    /// Gets a copy of every message, and the Telegram-down notice; `None`:
    /// no second thread and no copy.
    pub second: Option<Arc<dyn SecondChannel>>,
    /// Telegram failing for longer than this starts the Telegram-down notice.
    pub second_after_ms: i64,
}

impl fmt::Debug for AlertConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AlertConfig")
            .field("telegram", &self.telegram)
            .field("prefix", &self.prefix)
            .field("venue", &self.venue)
            .field("routes", &self.routes)
            .field("timeout", &self.timeout)
            .field("flush_every", &self.flush_every)
            .field("outbox_limit", &self.outbox_limit)
            .field("repeat_ms", &self.repeat_ms)
            .field("connect_timeout", &self.connect_timeout)
            .field("state_file", &self.state_file)
            .field(
                "second",
                &self.second.as_ref().map(|s| s.name().to_string()),
            )
            .field("second_after_ms", &self.second_after_ms)
            .finish()
    }
}

impl AlertConfig {
    pub fn new(telegram: TelegramConfig, prefix: &str, venue: &str) -> AlertConfig {
        AlertConfig {
            telegram,
            prefix: prefix.to_string(),
            venue: venue.to_string(),
            routes: Route::telegram(),
            timeout: Duration::from_secs(10),
            flush_every: Duration::from_secs(5),
            outbox_limit: OUTBOX_LIMIT,
            repeat_ms: REPEAT_MS,
            connect_timeout: Duration::from_secs(3),
            state_file: None,
            second: None,
            second_after_ms: SECOND_AFTER_MS,
        }
    }
}

/// What the sender has done.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AlertState {
    pub queued: usize,
    pub delivered: u64,
    /// Parts dropped by overflow or refused by Telegram.
    pub dropped_total: u64,
    /// Failed attempts (every route tried counts).
    pub failures: u64,
    /// The last few failure texts (never the token).
    pub last_errors: Vec<String>,
    /// Route indexes in the order the next send tries them.
    pub route_order: Vec<usize>,
    /// Conditions that hold now.
    pub active: Vec<String>,
    /// The clock time of the first failed attempt since Telegram last
    /// answered (delivered or refused); `None` while it answers.
    pub failing_since_ms: Option<i64>,
    pub second: Option<SecondState>,
}

/// What the second channel's sender has done.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SecondState {
    pub name: String,
    pub queued: usize,
    pub delivered: u64,
    /// Parts dropped by overflow.
    pub dropped_total: u64,
    /// Failed sends.
    pub failures: u64,
}

struct Inner {
    outbox: Outbox,
    repeat: Repeat,
    in_flight: bool,
    delivered: u64,
    failures: u64,
    last_errors: VecDeque<String>,
    route_order: Vec<usize>,
    wake: bool,
    failing_since_ms: Option<i64>,
    second: Option<Second>,
}

/// The second channel's side: its own outbox, and the repeat rule of the
/// Telegram-down notice.
struct Second {
    name: String,
    outbox: Outbox,
    repeat: Repeat,
    in_flight: bool,
    delivered: u64,
    failures: u64,
    wake: bool,
}

impl Inner {
    /// Queues a message for Telegram and, whole, for the second channel.
    fn add(&mut self, msg: &str) {
        self.outbox.add(msg);
        self.wake = true;
        if let Some(s) = self.second.as_mut() {
            s.outbox.add(msg);
            s.wake = true;
        }
    }

    /// Both outboxes are empty and nothing is being sent.
    fn settled(&self) -> bool {
        self.outbox.settled()
            && !self.in_flight
            && self
                .second
                .as_ref()
                .is_none_or(|s| s.outbox.settled() && !s.in_flight)
    }

    /// The Telegram-down condition at `now`: it holds while Telegram has
    /// failed for more than `after_ms`; start, repeat and clear go to the
    /// second channel only. `true` when a notice was queued.
    fn telegram_down(&mut self, now: i64, after_ms: i64, prefix: &str, venue: &str) -> bool {
        if self.second.is_none() {
            return false;
        }
        let text = match self.failing_since_ms {
            Some(t) if now - t > after_ms => Some(format!(
                "Telegram 전송이 {}초째 실패함. 대기 {}개. 마지막 오류: {}",
                (now - t) / 1000,
                self.outbox.len(),
                self.last_errors.back().map(String::as_str).unwrap_or("")
            )),
            _ => None,
        };
        let Some(s) = self.second.as_mut() else {
            return false;
        };
        match s.repeat.set(TELEGRAM_DOWN, text.as_deref(), now) {
            Some(body) => {
                s.outbox.add(&message(prefix, venue, now, &body));
                s.wake = true;
                true
            }
            None => false,
        }
    }
}

struct Shared {
    inner: Mutex<Inner>,
    cv: Condvar,
    stop: AtomicBool,
}

/// The sender's handle. Dropping it stops the thread after its current send.
pub struct AlertHandle {
    shared: Arc<Shared>,
    clock: Arc<dyn Clock>,
    prefix: String,
    venue: String,
    state_file: Option<std::path::PathBuf>,
}

pub struct AlertDirect;

impl AlertDirect {
    /// Builds the clients and starts the sender thread.
    pub fn start(cfg: AlertConfig, clock: Arc<dyn Clock>) -> Result<AlertHandle, String> {
        if cfg.routes.is_empty() {
            return Err("no route to Telegram".into());
        }
        let mut clients = Vec::new();
        for r in &cfg.routes {
            let mut b = reqwest::Client::builder()
                .timeout(cfg.timeout)
                .connect_timeout(cfg.connect_timeout.min(cfg.timeout))
                .no_proxy();
            if let Some((host, addr)) = &r.resolve {
                b = b.resolve(host, *addr);
            }
            let c = b
                .build()
                .map_err(|e| format!("cannot build the Telegram client: {}", e.without_url()))?;
            let url = r
                .base
                .join(&format!("./bot{}/sendMessage", cfg.telegram.token))
                .map_err(|_| "cannot build the Telegram URL".to_string())?;
            clients.push((c, url));
        }
        let mut repeat = Repeat::new(cfg.repeat_ms);
        if let Some(v) = cfg.state_file.as_deref().and_then(load_state) {
            repeat.restore(&v);
        }
        let shared = Arc::new(Shared {
            inner: Mutex::new(Inner {
                outbox: Outbox::new(cfg.outbox_limit),
                repeat,
                in_flight: false,
                delivered: 0,
                failures: 0,
                last_errors: VecDeque::new(),
                route_order: (0..clients.len()).collect(),
                wake: false,
                failing_since_ms: None,
                second: cfg.second.as_ref().map(|c| Second {
                    name: c.name().to_string(),
                    outbox: Outbox::new(cfg.outbox_limit),
                    repeat: Repeat::new(cfg.repeat_ms),
                    in_flight: false,
                    delivered: 0,
                    failures: 0,
                    wake: false,
                }),
            }),
            cv: Condvar::new(),
            stop: AtomicBool::new(false),
        });
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| format!("cannot start the alert runtime: {e}"))?;
        let s = shared.clone();
        let sender = Sender {
            clients,
            chat: cfg.telegram.chat_id.clone(),
            prefix: cfg.prefix.clone(),
            venue: cfg.venue.clone(),
            every: cfg.flush_every,
            second_after_ms: cfg.second_after_ms,
            clock: clock.clone(),
        };
        std::thread::Builder::new()
            .name("alert-direct".into())
            .spawn(move || run(rt, s, sender))
            .map_err(|e| format!("cannot start the alert thread: {e}"))?;
        if let Some(channel) = cfg.second.clone() {
            let s = shared.clone();
            let (prefix, every) = (cfg.prefix.clone(), cfg.flush_every);
            let spawned = std::thread::Builder::new()
                .name("alert-second".into())
                .spawn(move || run_second(s, channel, prefix, every));
            if let Err(e) = spawned {
                shared.stop.store(true, Ordering::SeqCst);
                shared.cv.notify_all();
                return Err(format!("cannot start the second alert thread: {e}"));
            }
        }
        Ok(AlertHandle {
            shared,
            clock,
            prefix: cfg.prefix,
            venue: cfg.venue,
            state_file: cfg.state_file,
        })
    }
}

/// `MM-DD HH:MM:SS KST` of a Unix time in ms.
pub fn kst(ms: i64) -> String {
    let s = ms.div_euclid(1000) + 9 * 3600;
    let days = s.div_euclid(86_400);
    let sec = s.rem_euclid(86_400);
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

/// `{prefix} {venue} {KST}\n{body}`.
fn message(prefix: &str, venue: &str, now_ms: i64, body: &str) -> String {
    format!("{prefix} {venue} {}\n{body}", kst(now_ms))
}

impl AlertHandle {
    fn wake(&self, inner: &mut Inner) {
        inner.wake = true;
        if let Some(s) = inner.second.as_mut() {
            s.wake = true;
        }
        self.shared.cv.notify_all();
    }

    fn message(&self, body: &str) -> String {
        message(&self.prefix, &self.venue, self.clock.now_ms(), body)
    }

    /// Queues one message without a condition key.
    pub fn once(&self, body: &str) {
        let msg = self.message(body);
        if let Ok(mut i) = self.shared.inner.lock() {
            i.add(&msg);
            self.wake(&mut i);
        }
    }

    /// The condition `key` holds (`Some`) or not (`None`); call it on every
    /// check, the repeat rule decides what is sent.
    pub fn set(&self, key: &str, text: Option<String>) {
        let now = self.clock.now_ms();
        let mut changed = None;
        if let Ok(mut i) = self.shared.inner.lock() {
            if let Some(body) = i.repeat.set(key, text.as_deref(), now) {
                let msg = self.message(&body);
                i.add(&msg);
                self.wake(&mut i);
                if self.state_file.is_some() {
                    changed = Some(i.repeat.to_json());
                }
            }
        }
        // The state changes only when a message is made (start, repeat,
        // clear): the file is written then, outside the lock.
        if let (Some(v), Some(path)) = (changed, &self.state_file) {
            if let Err(e) = save_state(path, &v) {
                eprintln!(
                    "{}",
                    serde_json::json!({"level": "warn", "msg": "alert state not saved", "fields": {"error": e.to_string()}})
                );
            }
        }
    }

    /// Drops the condition `key` without a message (and from the state
    /// file): used when its start message was not delivered, so that the
    /// next process sends the start again.
    pub fn forget(&self, key: &str) {
        let mut changed = None;
        if let Ok(mut i) = self.shared.inner.lock() {
            if i.repeat.forget(key) && self.state_file.is_some() {
                changed = Some(i.repeat.to_json());
            }
        }
        if let (Some(v), Some(path)) = (changed, &self.state_file) {
            let _ = save_state(path, &v);
        }
    }

    /// Waits until everything is delivered (or the time is up); `true` when
    /// nothing is left to send, to Telegram and to the second channel.
    pub fn flush_for(&self, d: Duration) -> bool {
        let end = Instant::now() + d;
        let Ok(mut i) = self.shared.inner.lock() else {
            return false;
        };
        self.wake(&mut i);
        loop {
            if i.settled() {
                return true;
            }
            let now = Instant::now();
            if now >= end {
                return false;
            }
            match self.shared.cv.wait_timeout(i, end - now) {
                Ok((g, _)) => i = g,
                Err(_) => return false,
            }
        }
    }

    pub fn state(&self) -> AlertState {
        match self.shared.inner.lock() {
            Ok(i) => AlertState {
                queued: i.outbox.len(),
                delivered: i.delivered,
                dropped_total: i.outbox.dropped_total(),
                failures: i.failures,
                last_errors: i.last_errors.iter().cloned().collect(),
                route_order: i.route_order.clone(),
                active: i.repeat.active(),
                failing_since_ms: i.failing_since_ms,
                second: i.second.as_ref().map(|s| SecondState {
                    name: s.name.clone(),
                    queued: s.outbox.len(),
                    delivered: s.delivered,
                    dropped_total: s.outbox.dropped_total(),
                    failures: s.failures,
                }),
            },
            Err(_) => AlertState::default(),
        }
    }
}

impl Drop for AlertHandle {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        self.shared.cv.notify_all();
    }
}

/// What the Telegram sender thread needs.
struct Sender {
    clients: Vec<(reqwest::Client, Url)>,
    chat: String,
    prefix: String,
    venue: String,
    every: Duration,
    second_after_ms: i64,
    clock: Arc<dyn Clock>,
}

/// Waits until `woken` holds, the handle is dropped, or `every` passed.
/// Other notifies on the shared condvar (the other thread's sends,
/// `flush_for`) do not end the wait early.
fn wait_for_work<'a>(
    shared: &'a Shared,
    mut i: MutexGuard<'a, Inner>,
    every: Duration,
    woken: impl Fn(&Inner) -> bool,
) -> Option<MutexGuard<'a, Inner>> {
    let end = Instant::now() + every;
    while !woken(&i) && !shared.stop.load(Ordering::SeqCst) {
        let now = Instant::now();
        if now >= end {
            break;
        }
        i = shared.cv.wait_timeout(i, end - now).ok()?.0;
    }
    Some(i)
}

fn run(rt: tokio::runtime::Runtime, shared: Arc<Shared>, s: Sender) {
    loop {
        // Wait for work or the retry period.
        {
            let Ok(i) = shared.inner.lock() else {
                return;
            };
            let Some(mut i) = wait_for_work(&shared, i, s.every, |i| i.wake) else {
                return;
            };
            i.wake = false;
            // Every wake looks at how long Telegram has been failing.
            let now = s.clock.now_ms();
            if i.telegram_down(now, s.second_after_ms, &s.prefix, &s.venue) {
                shared.cv.notify_all();
            }
        }
        if shared.stop.load(Ordering::SeqCst) {
            return;
        }
        // One flush: send parts until empty or a retryable failure.
        loop {
            let (part, order) = {
                let Ok(mut i) = shared.inner.lock() else {
                    return;
                };
                match i.outbox.next(&s.prefix) {
                    Some(p) => {
                        i.in_flight = true;
                        (p, i.route_order.clone())
                    }
                    None => break,
                }
            };
            let started = s.clock.now_ms();
            let (outcome, errors, good) =
                rt.block_on(send(&s.clients, &order, &s.chat, &part.text));
            let Ok(mut i) = shared.inner.lock() else {
                return;
            };
            i.in_flight = false;
            i.failures += errors.len() as u64;
            for e in errors {
                eprintln!(
                    "{}",
                    serde_json::json!({"level": "warn", "msg": "telegram send failed", "fields": {"error": e}})
                );
                i.last_errors.push_back(e);
                while i.last_errors.len() > 5 {
                    i.last_errors.pop_front();
                }
            }
            if let Some(g) = good {
                i.route_order.retain(|&r| r != g);
                i.route_order.insert(0, g);
            }
            match outcome {
                SendOutcome::Delivered => {
                    i.delivered += 1;
                    i.failing_since_ms = None;
                }
                // Telegram answered: it is reachable.
                SendOutcome::Refused(_) => i.failing_since_ms = None,
                SendOutcome::Retry(_) => {
                    if i.failing_since_ms.is_none() {
                        i.failing_since_ms = Some(started);
                    }
                }
            }
            let go_on = i.outbox.done(&part, &outcome);
            let now = s.clock.now_ms();
            i.telegram_down(now, s.second_after_ms, &s.prefix, &s.venue);
            shared.cv.notify_all();
            if !go_on || shared.stop.load(Ordering::SeqCst) {
                break;
            }
        }
    }
}

/// The second channel's thread: flushes its outbox on notify and every
/// `every`; an error keeps the part and ends the flush.
fn run_second(
    shared: Arc<Shared>,
    channel: Arc<dyn SecondChannel>,
    prefix: String,
    every: Duration,
) {
    let woken = |i: &Inner| i.second.as_ref().is_some_and(|s| s.wake);
    loop {
        {
            let Ok(i) = shared.inner.lock() else {
                return;
            };
            let Some(mut i) = wait_for_work(&shared, i, every, woken) else {
                return;
            };
            let Some(s) = i.second.as_mut() else {
                return;
            };
            s.wake = false;
        }
        if shared.stop.load(Ordering::SeqCst) {
            return;
        }
        loop {
            let part = {
                let Ok(mut i) = shared.inner.lock() else {
                    return;
                };
                let Some(s) = i.second.as_mut() else {
                    return;
                };
                match s.outbox.next(&prefix) {
                    Some(p) => {
                        s.in_flight = true;
                        p
                    }
                    None => break,
                }
            };
            let outcome = match channel.send(&part.text) {
                Ok(()) => SendOutcome::Delivered,
                Err(e) => SendOutcome::Retry(e),
            };
            let Ok(mut i) = shared.inner.lock() else {
                return;
            };
            let Some(s) = i.second.as_mut() else {
                return;
            };
            s.in_flight = false;
            match &outcome {
                SendOutcome::Retry(e) => {
                    s.failures += 1;
                    eprintln!(
                        "{}",
                        serde_json::json!({"level": "warn", "msg": "second channel send failed", "fields": {"channel": s.name, "error": e}})
                    );
                }
                _ => s.delivered += 1,
            }
            let go_on = s.outbox.done(&part, &outcome);
            shared.cv.notify_all();
            if !go_on || shared.stop.load(Ordering::SeqCst) {
                break;
            }
        }
    }
}

/// Tries the routes in order. A network error moves to the next route; an
/// answer from Telegram decides (PR #3: 400 and 413 are about this message,
/// 401, 403 and the rest about the bot or the chat).
async fn send(
    clients: &[(reqwest::Client, Url)],
    order: &[usize],
    chat: &str,
    text: &str,
) -> (SendOutcome, Vec<String>, Option<usize>) {
    let body = serde_json::json!({"chat_id": chat, "text": text, "disable_web_page_preview": true})
        .to_string();
    let mut errors = Vec::new();
    for &i in order {
        let Some((client, url)) = clients.get(i) else {
            continue;
        };
        let r = client
            .post(url.clone())
            .header("content-type", "application/json")
            .body(body.clone())
            .send()
            .await;
        let resp = match r {
            Ok(r) => r,
            Err(e) => {
                // A timeout after delivery can send twice: a duplicate is
                // better than a lost alert (PR #3).
                errors.push(format!("route {i}: {}", e.without_url()));
                continue;
            }
        };
        let status = resp.status().as_u16();
        let data = match resp.bytes().await {
            Ok(b) => serde_json::from_slice::<serde_json::Value>(&b).unwrap_or_default(),
            Err(e) => {
                errors.push(format!("route {i}: {}", e.without_url()));
                continue;
            }
        };
        if data["ok"].as_bool() == Some(true) && (200..300).contains(&status) {
            return (SendOutcome::Delivered, errors, Some(i));
        }
        let msg = format!(
            "telegram answered {status}: {}",
            data["description"].as_str().unwrap_or("")
        );
        errors.push(msg.clone());
        return if status == 400 || status == 413 {
            (SendOutcome::Refused(msg), errors, Some(i))
        } else {
            (SendOutcome::Retry(msg), errors, Some(i))
        };
    }
    let last = errors.last().cloned().unwrap_or_else(|| "no route".into());
    (SendOutcome::Retry(last), errors, None)
}
