//! The watcher's links to the OMS (C-WATCH part 7, U19a7), written from
//! C-API and C-LIVE part 2, not from `oms-api`: `/api/status` on the
//! loopback dashboard, `watcher_mismatch` on `admin.sock` (one connection
//! per send) and the `heartbeat` on `monitor.sock` (one persistent
//! connection). Each line is one JSON object; a reply is
//! `{"id","ok","result"}` or `{"id","ok":false,"error":{"code","message"}}`.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};

use crate::engine::Check;
use crate::log::Log;
use crate::metrics::Metrics;

/// A line of the OMS's sockets is at most this many bytes (C-API).
pub const LINE_MAX: u64 = 64 * 1024;

fn quote(s: &str) -> String {
    Value::String(s.to_string()).to_string()
}

fn hello_line(id: u64, name: &str) -> String {
    format!(
        r#"{{"id":{id},"op":"hello","client":{}}}"#,
        quote(&format!("watcher/{name}"))
    )
}

/// Why a request has no `result`.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Fail {
    /// An error reply: the connection is still usable.
    Refused { code: String, message: String },
    /// No reply (closed, timeout, not JSON, too long): the connection is
    /// not usable.
    Link(String),
}

impl std::fmt::Display for Fail {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Fail::Refused { code, message } => write!(f, "{code}: {message}"),
            Fail::Link(why) => f.write_str(why),
        }
    }
}

/// The `result` of the reply with `id` as text (a JSON string as is, any
/// other value as JSON), or the error reply. Lines with another id are
/// skipped.
async fn reply(rd: &mut BufReader<OwnedReadHalf>, id: u64) -> Result<String, Fail> {
    loop {
        let mut line = String::new();
        let n = (&mut *rd)
            .take(LINE_MAX + 1)
            .read_line(&mut line)
            .await
            .map_err(|e| Fail::Link(format!("read: {e}")))?;
        if n == 0 {
            return Err(Fail::Link("the OMS closed the connection".into()));
        }
        if n as u64 > LINE_MAX {
            return Err(Fail::Link("a reply line longer than 64 KiB".into()));
        }
        let v: Value = serde_json::from_str(line.trim_end())
            .map_err(|e| Fail::Link(format!("a reply that is not JSON: {e}")))?;
        if v.get("id").and_then(Value::as_u64) != Some(id) {
            continue;
        }
        if v.get("ok").and_then(Value::as_bool) == Some(true) {
            return Ok(match v.get("result") {
                Some(Value::String(s)) => s.clone(),
                Some(other) => other.to_string(),
                None => String::new(),
            });
        }
        let text = |p: &str, or: &str| {
            v.pointer(p)
                .and_then(Value::as_str)
                .unwrap_or(or)
                .to_string()
        };
        return Err(Fail::Refused {
            code: text("/error/code", "unknown"),
            message: text("/error/message", ""),
        });
    }
}

/// One request and its reply within `timeout_ms`.
async fn ask(
    rd: &mut BufReader<OwnedReadHalf>,
    wr: &mut OwnedWriteHalf,
    id: u64,
    line: &str,
    timeout_ms: u64,
) -> Result<String, Fail> {
    let exchange = async {
        wr.write_all(format!("{line}\n").as_bytes())
            .await
            .map_err(|e| Fail::Link(format!("write: {e}")))?;
        reply(rd, id).await
    };
    match tokio::time::timeout(Duration::from_millis(timeout_ms), exchange).await {
        Ok(r) => r,
        Err(_) => Err(Fail::Link(format!(
            "no reply to request {id} within {timeout_ms} ms"
        ))),
    }
}

type Conn = (BufReader<OwnedReadHalf>, OwnedWriteHalf);

async fn connect(path: &Path) -> Result<Conn, String> {
    let s = UnixStream::connect(path)
        .await
        .map_err(|e| format!("not sent: {}: {e}", path.display()))?;
    let (rd, wr) = s.into_split();
    Ok((BufReader::new(rd), wr))
}

/// One `watcher_mismatch` send to `admin.sock` at `path`: connect,
/// `hello` (client `watcher/<name>`), then the request; each reply within
/// `timeout_ms` and the whole send within 2 × `timeout_ms`; the connection
/// closes after it. The `result` (`"held"`, `"already held"`), or why
/// there is none (`<code>: <message>` for an error reply).
pub async fn admin_send(
    path: &Path,
    name: &str,
    check: Check,
    detail: &str,
    timeout_ms: u64,
) -> Result<String, String> {
    let send = async {
        let (mut rd, mut wr) = connect(path).await?;
        ask(&mut rd, &mut wr, 1, &hello_line(1, name), timeout_ms)
            .await
            .map_err(|e| format!("hello: {e}"))?;
        let line = format!(
            r#"{{"id":2,"op":"watcher_mismatch","check":"{}","detail":{}}}"#,
            check.name(),
            quote(detail)
        );
        ask(&mut rd, &mut wr, 2, &line, timeout_ms)
            .await
            .map_err(|e| e.to_string())
    };
    let whole = 2 * timeout_ms;
    match tokio::time::timeout(Duration::from_millis(whole), send).await {
        Ok(r) => r,
        Err(_) => Err(format!("no reply within {whole} ms")),
    }
}

/// A `watcher_kill` reply that halted the OMS: `"latched"`, or one that
/// starts with `"already latched"` (C-HOLD "워처의 입력"; this covers the
/// re-arm reply after a confirmed loop).
pub fn latched(reply: &str) -> bool {
    reply == "latched" || reply.starts_with("already latched")
}

/// How often an error answer to a heartbeat is logged at most.
pub const REFUSAL_LOG_MS: u64 = 60_000;

/// The `monitor.sock` heartbeat client: one persistent connection, opened
/// with `hello` (no strategy) at a heartbeat when there is none, and
/// dropped when a heartbeat gets no reply within `timeout_ms` or the link
/// fails, so the next heartbeat reopens it. It therefore tries to connect
/// at most once per heartbeat, also when the socket does not exist. One
/// line per change of state (connected / not connected, with the reason);
/// an error answer (`bad_request`, `overloaded`, …) keeps the connection
/// and is logged at most once a minute. A `"recorded"` reply sets
/// `watcher_heartbeat_ok_ts` (epoch seconds).
pub struct Monitor {
    path: PathBuf,
    name: String,
    timeout_ms: u64,
    metrics: Arc<Metrics>,
    log: Log,
    conn: Option<Conn>,
    next_id: u64,
    tries: u64,
    /// The state last logged.
    connected: Option<bool>,
    last_refusal_log: Option<u64>,
}

impl Monitor {
    pub fn new(
        path: &Path,
        name: &str,
        timeout_ms: u64,
        metrics: Arc<Metrics>,
        log: Log,
    ) -> Monitor {
        Monitor {
            path: path.to_path_buf(),
            name: name.to_string(),
            timeout_ms,
            metrics,
            log,
            conn: None,
            next_id: 1,
            tries: 0,
            connected: None,
            last_refusal_log: None,
        }
    }

    /// Connection attempts so far.
    pub fn tries(&self) -> u64 {
        self.tries
    }

    fn id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id - 1
    }

    fn state(&mut self, connected: bool, why: &str) {
        if self.connected == Some(connected) {
            return;
        }
        self.connected = Some(connected);
        let at = format!("monitor.sock {}", self.path.display());
        if connected {
            (self.log)(&format!("{at} connected"));
        } else {
            (self.log)(&format!(
                "warn: {at} not connected: {why}; tried again at the next heartbeat"
            ));
        }
    }

    /// Opens the connection and has `hello` answered, if there is none.
    async fn open(&mut self) -> Result<(), String> {
        if self.conn.is_some() {
            return Ok(());
        }
        self.tries += 1;
        let (mut rd, mut wr) = connect(&self.path).await?;
        let id = self.id();
        let hello = hello_line(id, &self.name);
        ask(&mut rd, &mut wr, id, &hello, self.timeout_ms)
            .await
            .map_err(|e| format!("hello: {e}"))?;
        self.conn = Some((rd, wr));
        Ok(())
    }

    /// The heartbeat line: W1..W4, each the time in `checks` (ms) or `null`.
    fn line(&self, id: u64, checks: &[(Check, Option<u64>)]) -> String {
        let fields: Vec<String> = Check::ALL
            .iter()
            .map(|c| {
                let t = checks.iter().find(|(k, _)| k == c).and_then(|(_, t)| *t);
                let v = t.map_or("null".to_string(), |t| t.to_string());
                format!("\"{}\":{v}", c.name())
            })
            .collect();
        format!(
            r#"{{"id":{id},"op":"heartbeat","watcher":{},"checks":{{{}}}}}"#,
            quote(&self.name),
            fields.join(",")
        )
    }

    /// One heartbeat at `now_ms` with each check's last success.
    pub async fn beat(
        &mut self,
        now_ms: u64,
        checks: &[(Check, Option<u64>)],
    ) -> Result<(), String> {
        if let Err(e) = self.open().await {
            self.state(false, &e);
            return Err(e);
        }
        self.state(true, "");
        let id = self.id();
        let line = self.line(id, checks);
        let timeout = self.timeout_ms;
        let Some((rd, wr)) = self.conn.as_mut() else {
            return Err("no connection".into());
        };
        match ask(rd, wr, id, &line, timeout).await {
            Ok(r) if r == "recorded" => {
                let s = now_ms as f64 / 1000.0;
                self.metrics.set("watcher_heartbeat_ok_ts", &[], s);
                Ok(())
            }
            Ok(other) => {
                let why = format!("unexpected answer {other:?}");
                self.refused(now_ms, &why);
                Err(why)
            }
            Err(e @ Fail::Refused { .. }) => {
                let why = e.to_string();
                self.refused(now_ms, &why);
                Err(why)
            }
            Err(Fail::Link(why)) => {
                self.conn = None;
                self.state(false, &why);
                Err(why)
            }
        }
    }

    /// Closes the connection (shutdown).
    pub fn close(&mut self) {
        self.conn = None;
    }

    fn refused(&mut self, now_ms: u64, why: &str) {
        let due = self
            .last_refusal_log
            .is_none_or(|t| now_ms.saturating_sub(t) >= REFUSAL_LOG_MS);
        if due {
            self.last_refusal_log = Some(now_ms);
            (self.log)(&format!(
                "error: monitor.sock heartbeat refused ({why}); logged at most once a minute"
            ));
        }
    }
}

/// The `/api/status` client: GET of `status_url` (loopback only, U19a4),
/// so the `Host` header is `127.0.0.1:<port>` or `[::1]:<port>`, as the
/// dashboard requires; no redirect, no proxy, `request_timeout_ms`. The
/// body as JSON; the engine reads `journal.durable_seq` and
/// `core.instance_id` from it.
pub struct Status {
    url: String,
    http: reqwest::Client,
}

impl Status {
    pub fn new(url: &str, timeout_ms: u64) -> Result<Status, String> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_millis(timeout_ms))
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Status {
            url: url.to_string(),
            http,
        })
    }

    pub async fn get(&self) -> Result<Value, String> {
        let r = self
            .http
            .get(&self.url)
            .send()
            .await
            .map_err(|e| format!("no answer: {e}"))?;
        let status = r.status().as_u16();
        if !(200..=299).contains(&status) {
            return Err(format!("HTTP {status}"));
        }
        let body = r.text().await.map_err(|e| format!("no answer: {e}"))?;
        serde_json::from_str(&body).map_err(|e| format!("not JSON: {e}"))
    }
}
