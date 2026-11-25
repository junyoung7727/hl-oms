//! The WS Trade connection (ADR-028 §7e, U17b): orders and cancels over
//! `/v5/trade`, the first path. REST takes a request only when this
//! connection wrote no byte of it (`Reply::NotWritten`).
//!
//! One connection task (`run`): connect over IPv4 as the private stream
//! does, auth, then up; ping and pong and the reconnect waits as on the
//! private stream. A written request waits in the pending map by its
//! `reqId` for its answer, the request timeout or the end of the
//! connection, whichever comes first; the last two leave it unknown.
//! Nothing is written twice, over this path or REST (G-3). The connection
//! does not drive the link: the private stream does.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::sync::{mpsc, oneshot};
use tokio::time::Instant;
use tokio_tungstenite::tungstenite::Message;

use super::answer::{self, Answer};
use super::gateway::{End, Timing, connect, wall_ms};
use super::orders::{self, Effect};
use super::rest::Credentials;
use super::stream;
use crate::{info, warn};

pub const CREATE_OP: &str = "order.create";
pub const CANCEL_OP: &str = "order.cancel";

/// The longest `reqId` the venue takes (F18 §3.1).
pub const REQ_ID_MAX: usize = 36;

/// "exceed IP rate limit" (WS Trade, F18 §3.1): the WS form of REST 10018.
pub const IP_LIMIT: i64 = 10403;

/// "ws trade service is restarting ... You can build new connection to be
/// routed to normal service" (F18 §3.1).
pub const RESTARTING: i64 = 10019;

/// The `reqId` of a create: `c` + the wire id (28 characters). One create
/// per order, so it is never reused.
pub fn create_req_id(wire_id: &str) -> String {
    format!("c{wire_id}")
}

/// The `reqId` of a cancel: `x` + the attempt in base 36 + the wire id (at
/// most 35 characters). One cancel per attempt. The wire id has a fixed
/// length, so the attempt's digits are never confused with it.
pub fn cancel_req_id(attempt: u32, wire_id: &str) -> String {
    const DIGITS: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut digits = Vec::new();
    let mut n = attempt;
    loop {
        digits.push(DIGITS[(n % 36) as usize]);
        n /= 36;
        if n == 0 {
            break;
        }
    }
    digits.reverse();
    format!("x{}{wire_id}", String::from_utf8_lossy(&digits))
}

/// One request: the REST body as the only item of `args`, with the request
/// time and the receive window in `header` (F18 §3.1).
pub fn request_message(
    req_id: &str,
    op: &str,
    ts_ms: u64,
    recv_window_ms: u64,
    body: &Value,
) -> String {
    json!({
        "reqId": req_id,
        "header": {
            "X-BAPI-TIMESTAMP": ts_ms.to_string(),
            "X-BAPI-RECV-WINDOW": recv_window_ms.to_string(),
        },
        "op": op,
        "args": [body],
    })
    .to_string()
}

/// The `auth` message: the private stream's signature over
/// `GET/realtime` + `expires`, in the shape of the WS Trade page. `Err`
/// when the signature cannot be made: the message is not sent.
pub fn auth_message(creds: &Credentials, expires_ms: u64) -> Result<String, String> {
    let sig = creds.ws_auth_signature(expires_ms)?;
    Ok(json!({
        "op": "auth",
        "args": [creds.api_key, expires_ms, sig],
    })
    .to_string())
}

/// One text frame of WS Trade.
#[derive(Debug, PartialEq)]
pub enum Frame {
    /// The answer to `auth`: retCode 0 is success, anything else holds what
    /// the venue said.
    Auth(Result<(), String>),
    /// `op = "pong"`, or `op = "ping"` with `retMsg` or `ret_msg` `pong`.
    Pong,
    /// The answer to a request, by its `reqId` (absent if the venue gave
    /// none), with its retCode.
    Answer {
        req_id: Option<String>,
        code: Option<i64>,
    },
    /// Anything else. Nothing is read from it.
    Other,
}

/// Reads one text frame. `Err` if it is not JSON.
pub fn parse(text: &str) -> Result<Frame, String> {
    let v: Value = serde_json::from_str(text).map_err(|e| format!("not JSON: {e}"))?;
    let s = |k: &str| v.get(k).and_then(Value::as_str);
    let code = v.get("retCode").and_then(Value::as_i64);
    Ok(match s("op") {
        Some("auth") => Frame::Auth(match code {
            Some(0) => Ok(()),
            _ => Err(format!(
                "retCode {code:?}, retMsg {:?}",
                s("retMsg")
                    .unwrap_or("")
                    .chars()
                    .take(200)
                    .collect::<String>()
            )),
        }),
        Some("pong") => Frame::Pong,
        Some("ping") if s("retMsg") == Some("pong") || s("ret_msg") == Some("pong") => Frame::Pong,
        Some("ping") => Frame::Other,
        _ if v.get("reqId").is_some() || code.is_some() => Frame::Answer {
            req_id: s("reqId").map(str::to_string),
            code,
        },
        _ => Frame::Other,
    })
}

/// What an answer on WS Trade says about the key or the IP: as on REST
/// (auth codes, 10018), and 10403 starts the ban pause too (judgment, the
/// safe side: whether a ban follows is undocumented).
pub fn effect(a: &Answer, code: Option<i64>) -> Effect {
    match code {
        Some(IP_LIMIT) => Effect::Ban(format!("IP rate limit (10403) on WS Trade: {a:?}")),
        _ => orders::effect(200, a),
    }
}

/// What became of one request handed to the connection.
#[derive(Debug)]
pub enum Reply {
    /// No byte of it was written (no authenticated connection, or a block
    /// before the write began): the caller may send it over REST.
    NotWritten(String),
    /// The venue's answer, the whole frame, and its retCode.
    Answered { text: String, code: Option<i64> },
    /// It was written and no answer came: the connection ended or the
    /// request timed out. It may have reached the venue.
    NoAnswer(String),
}

/// One request for the connection task.
pub struct Job {
    pub req_id: String,
    pub text: String,
    pub reply: oneshot::Sender<Reply>,
}

/// The handle of the connection: the order path hands requests through
/// it. Holds the current connection's queue while one is authenticated
/// and up, and nothing otherwise.
#[derive(Default)]
pub struct Trade {
    conn: Mutex<Option<mpsc::UnboundedSender<Job>>>,
}

impl Trade {
    fn slot(&self) -> std::sync::MutexGuard<'_, Option<mpsc::UnboundedSender<Job>>> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// A connection came up: requests go to the returned queue from now on.
    pub fn install(&self) -> mpsc::UnboundedReceiver<Job> {
        let (tx, rx) = mpsc::unbounded_channel();
        *self.slot() = Some(tx);
        rx
    }

    /// The connection went down: no request goes to it any more.
    pub fn uninstall(&self) {
        self.slot().take();
    }

    /// Whether a connection is authenticated and up.
    #[cfg(test)]
    pub fn is_up(&self) -> bool {
        self.slot().is_some()
    }

    /// Hands one request to the connection and waits for what became of it.
    pub async fn request(&self, req_id: &str, text: String) -> Reply {
        if req_id.len() > REQ_ID_MAX {
            return Reply::NotWritten(format!("reqId {req_id} is longer than {REQ_ID_MAX}"));
        }
        let Some(tx) = self.slot().clone() else {
            return Reply::NotWritten("WS Trade is not connected".into());
        };
        let (reply, answer) = oneshot::channel();
        let job = Job {
            req_id: req_id.to_string(),
            text,
            reply,
        };
        if tx.send(job).is_err() {
            return Reply::NotWritten("the WS Trade connection closed".into());
        }
        // The task answers every request it took. A task that ended
        // without answering may have written it.
        answer
            .await
            .unwrap_or_else(|_| Reply::NoAnswer("the WS Trade task ended".into()))
    }
}

/// What the connection asks of the gateway.
pub(crate) trait Host: Send + Sync + 'static {
    /// Why no request may go out now (key rejected, the pause after a ban,
    /// a failed check), or `None`. The connection opens only while there is
    /// none, and closes when one appears. The clock block is not one: it
    /// keeps creates home, and the order path asks for it.
    fn block(&self) -> Option<String>;
    /// The key was rejected: the connection stays closed until the restart.
    fn key_rejected(&self) -> bool;
    /// HTTP 403 on the upgrade.
    fn ban(&self, why: String);
    /// The venue refused the auth.
    fn reject_key(&self, why: String);
}

/// Where and how the connection connects.
pub(crate) struct Conn {
    pub url: String,
    pub creds: Arc<Credentials>,
    pub recv_window_ms: u64,
    pub timing: Timing,
}

/// The connection task: connects while no block is set, and again after
/// every end with the private stream's waits (they start over only after a
/// connection that answered a ping). Stops for good once the key is
/// rejected.
pub(crate) async fn run<H: Host>(trade: Arc<Trade>, host: Arc<H>, c: Conn) {
    let mut backoff = c.timing.backoff_min;
    loop {
        if host.key_rejected() {
            warn!("WS Trade stays closed: the key was rejected");
            return;
        }
        if host.block().is_some() {
            tokio::time::sleep(c.timing.health_poll).await;
            continue;
        }
        let mut alive = false;
        let end = once(&trade, &*host, &c, &mut alive).await;
        if alive {
            backoff = c.timing.backoff_min;
        }
        match end {
            End::Retry(e) => {
                warn!("WS Trade is down; orders and cancels go over REST until it is back", "error": e)
            }
            End::Rejected(e) => {
                host.reject_key(format!("WS Trade auth: {e}"));
                return;
            }
            // The upgrade is an HTTP request too (§7c).
            End::IpBanned(e) => host.ban(format!("WS Trade upgrade: {e}")),
        }
        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(c.timing.backoff_max);
    }
}

/// A request written and not yet answered.
struct Pending {
    reply: oneshot::Sender<Reply>,
    /// When it becomes unknown.
    due: Instant,
}

/// One connection, from the upgrade to its end. Every request it took is
/// answered before it returns: a written one with its answer or
/// `NoAnswer`, a queued one with `NotWritten`.
async fn once<H: Host>(trade: &Trade, host: &H, c: &Conn, alive: &mut bool) -> End {
    let t = &c.timing;
    let ws = match tokio::time::timeout(t.handshake, connect(&c.url)).await {
        Err(_) => return End::Retry("connect timed out".into()),
        Ok(Err(e)) => return e,
        Ok(Ok(ws)) => ws,
    };
    let (mut write, mut read) = ws.split();
    let expires = wall_ms() + c.recv_window_ms;
    let auth = match auth_message(&c.creds, expires) {
        Ok(m) => m,
        Err(e) => return End::Retry(format!("auth not sent: {e}")),
    };
    if let Err(e) = write.send(Message::text(auth)).await {
        return End::Retry(format!("auth not sent: {e}"));
    }
    let deadline = Instant::now() + t.handshake;
    loop {
        let m = match tokio::time::timeout_at(deadline, read.next()).await {
            Err(_) => return End::Retry("auth not confirmed in time".into()),
            Ok(None) => return End::Retry("closed during the auth".into()),
            Ok(Some(Err(e))) => return End::Retry(format!("read failed: {e}")),
            Ok(Some(Ok(m))) => m,
        };
        let Message::Text(text) = m else { continue };
        match parse(text.as_str()) {
            Err(e) => return End::Retry(e),
            Ok(Frame::Auth(Ok(()))) => break,
            Ok(Frame::Auth(Err(e))) => return End::Rejected(e),
            Ok(_) => {}
        }
    }
    if let Some(b) = host.block() {
        return End::Retry(format!("blocked before WS Trade came up: {b}"));
    }
    let mut jobs = trade.install();
    info!("WS Trade is up; orders and cancels go over it");

    let mut ping = tokio::time::interval(t.ping);
    ping.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    ping.tick().await;
    let mut health = tokio::time::interval(t.health_poll);
    let mut pings = 0u64;
    let mut pong_due: Option<Instant> = None;
    let mut pending: HashMap<String, Pending> = HashMap::new();
    let reason = loop {
        let next_due = pending.values().map(|p| p.due).min();
        tokio::select! {
            // What the socket already holds is read before any deadline is
            // judged: an answer or a pong in the buffer is not missing.
            biased;
            m = read.next() => {
                let text = match m {
                    None => break "closed by the venue".to_string(),
                    Some(Err(e)) => break format!("read failed: {e}"),
                    Some(Ok(Message::Text(t))) => t,
                    Some(Ok(_)) => continue,
                };
                match parse(text.as_str()) {
                    Err(e) => break format!("a WS Trade message could not be read: {e}"),
                    Ok(Frame::Pong) => {
                        pong_due = None;
                        *alive = true;
                    }
                    Ok(Frame::Answer { req_id, code }) => {
                        // What it says about the IP or the key takes effect
                        // here, before the next write: the requests queued
                        // behind it stay home. Also for an answer no
                        // request waits for.
                        match effect(&answer::classify(200, text.as_str()), code) {
                            Effect::Ban(why) => host.ban(why),
                            Effect::RejectKey(why) => host.reject_key(why),
                            Effect::None => {}
                        }
                        match req_id.as_ref().and_then(|id| pending.remove(id)) {
                            Some(p) => {
                                let _ = p.reply.send(Reply::Answered {
                                    text: text.to_string(),
                                    code,
                                });
                            }
                            // The request was already reported unknown; the
                            // core resolves it by its orderLinkId.
                            None => warn!("a WS Trade answer for no pending request (late or unknown); dropped", "req_id": req_id, "ret_code": code),
                        }
                        if code == Some(RESTARTING) {
                            break "WS Trade is restarting (10019); a new connection goes to a normal service".to_string();
                        }
                    }
                    Ok(Frame::Auth(_)) => break "an unexpected auth answer".to_string(),
                    Ok(Frame::Other) => {}
                }
            }
            _ = tokio::time::sleep_until(next_due.unwrap_or_else(Instant::now)), if next_due.is_some() => {
                let now = Instant::now();
                let late: Vec<String> = pending
                    .iter()
                    .filter(|(_, p)| p.due <= now)
                    .map(|(id, _)| id.clone())
                    .collect();
                for id in late {
                    if let Some(p) = pending.remove(&id) {
                        let _ = p.reply.send(Reply::NoAnswer(format!(
                            "no answer within {} ms",
                            t.request.as_millis()
                        )));
                    }
                }
            }
            // A quiet connection and a half-open one look the same; only
            // the pong tells them apart.
            _ = tokio::time::sleep_until(pong_due.unwrap_or_else(Instant::now)), if pong_due.is_some() => {
                break format!("no pong within {} ms of a ping", t.ping.as_millis());
            }
            job = jobs.recv() => {
                // The handle holds the sender while the connection is up.
                let Some(job) = job else { break "the queue closed".to_string() };
                if let Some(b) = host.block() {
                    let _ = job.reply.send(Reply::NotWritten(b.clone()));
                    break format!("blocked: {b}");
                }
                if pending.contains_key(&job.req_id) {
                    // Never written twice; the first one decides.
                    let _ = job.reply.send(Reply::NoAnswer(format!(
                        "reqId {} is already pending; not written again",
                        job.req_id
                    )));
                    continue;
                }
                // From here the request may leave: it is pending first, so
                // the end of the connection reports it unknown.
                pending.insert(
                    job.req_id,
                    Pending { reply: job.reply, due: Instant::now() + t.request },
                );
                match tokio::time::timeout(t.request, write.send(Message::text(job.text))).await {
                    Ok(Ok(())) => {}
                    Ok(Err(e)) => break format!("a request could not be written: {e}"),
                    Err(_) => break "a request write timed out".to_string(),
                }
            }
            _ = ping.tick(), if pong_due.is_none() => {
                pings += 1;
                match tokio::time::timeout(t.ping, write.send(Message::text(stream::ping_message(pings)))).await {
                    Ok(Ok(())) => {}
                    Ok(Err(e)) => break format!("ping failed: {e}"),
                    Err(_) => break "a ping write timed out".to_string(),
                }
                pong_due = Some(Instant::now() + t.ping);
            }
            _ = health.tick() => {
                if let Some(b) = host.block() {
                    break format!("blocked: {b}");
                }
            }
        }
    };
    // No request goes to this connection any more; the queued ones were
    // never written, the pending ones may have been.
    trade.uninstall();
    jobs.close();
    while let Some(job) = jobs.recv().await {
        let _ = job
            .reply
            .send(Reply::NotWritten(format!("WS Trade went down: {reason}")));
    }
    for (_, p) in pending.drain() {
        let _ = p.reply.send(Reply::NoAnswer(format!(
            "WS Trade went down before the answer: {reason}"
        )));
    }
    End::Retry(reason)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn req_ids_are_unique_per_request_and_fit_the_venue_limit() {
        let wire = "028q5cu4qnjff-000000000001a";
        assert_eq!(create_req_id(wire), format!("c{wire}"));
        assert_eq!(create_req_id(wire).len(), 28);
        assert_eq!(cancel_req_id(1, wire), format!("x1{wire}"));
        assert_eq!(cancel_req_id(36, wire), format!("x10{wire}"));
        assert_eq!(cancel_req_id(35, wire), format!("xz{wire}"));
        assert_eq!(cancel_req_id(0, wire), format!("x0{wire}"));
        assert!(cancel_req_id(u32::MAX, wire).len() <= REQ_ID_MAX);
        assert_eq!(cancel_req_id(u32::MAX, wire), format!("x1z141z3{wire}"));
        assert_ne!(cancel_req_id(1, wire), cancel_req_id(2, wire));
    }

    #[test]
    fn a_request_carries_its_id_time_window_op_and_the_rest_body() {
        let body = json!({"category": "linear", "symbol": "BTCUSDT", "orderLinkId": "L"});
        let m: Value = serde_json::from_str(&request_message(
            "cL",
            CREATE_OP,
            1_700_000_000_000,
            5000,
            &body,
        ))
        .unwrap();
        assert_eq!(
            m,
            json!({
                "reqId": "cL",
                "header": {"X-BAPI-TIMESTAMP": "1700000000000", "X-BAPI-RECV-WINDOW": "5000"},
                "op": "order.create",
                "args": [body],
            })
        );
    }

    #[test]
    fn the_auth_message_is_the_private_stream_signature() {
        let creds = super::super::rest::Credentials::new("KEY", "SECRET");
        let m: Value =
            serde_json::from_str(&auth_message(&creds, 1_662_350_400_000).unwrap()).unwrap();
        assert_eq!(
            m,
            json!({"op": "auth", "args": ["KEY", 1_662_350_400_000u64,
                super::super::sign::ws_auth_signature("SECRET", 1_662_350_400_000)]})
        );
    }

    /// RSA (ADR-034, spec T3, F2): the `auth` message signed with an RSA
    /// key carries the base64 signature of `GET/realtime{expires}`:
    /// openssl's value (`printf '%s' GET/realtime1662350400000 | openssl dgst
    /// -sha256 -sign key.pem | base64 -w0`, the test-only key), not hex.
    #[test]
    fn the_auth_message_with_an_rsa_key_carries_the_base64_signature() {
        let pem = crate::venue::bybit::sign::test_keys::pem_2048();
        let creds = super::super::rest::Credentials::rsa("KEY", &pem).unwrap();
        let m: Value =
            serde_json::from_str(&auth_message(&creds, 1_662_350_400_000).unwrap()).unwrap();
        assert_eq!(
            m,
            json!({"op": "auth", "args": ["KEY", 1_662_350_400_000u64,
                "HZKuzRsZu3GyRpRRU9GK/xpL4DUyjo4bd+uRkRedWmZwUUr1ye+RvVNtV+LpPmx5X+1iKtlWIJGe1FNJ/jW/A0q4e2ND7FcQy9zEgbRZwVO+sbUqoN0m69zQ+mhqR5VBKXTFIcbEhzFiQKFHSC/hK5OmdSX2aCU0e0ROiA5o5p5Vcvs6jF4U1G2EF4bq5LX5BPufvpLsI4HoqBcX5Oq/gIjTxAORH2YNwzeuGh1migMEAxTpp2lRXc27qaBGhbPmYURG3QvCguBp5ctIiP2T6RTeiPgfBSYxSZZESMOs/oFVhjeRJ226Nrj7tUqcA9Gn1P4B4Y4SpIzAJ0tWLizZWg=="]})
        );
    }

    #[test]
    fn frames_are_read_by_their_documented_fields() {
        // F18 §3.1: auth answers carry retCode, 0 = success.
        assert_eq!(
            parse(r#"{"retCode":0,"retMsg":"OK","op":"auth","connId":"c"}"#).unwrap(),
            Frame::Auth(Ok(()))
        );
        assert!(matches!(
            parse(r#"{"retCode":10004,"retMsg":"invalid sign","op":"auth"}"#).unwrap(),
            Frame::Auth(Err(m)) if m.contains("10004") && m.contains("invalid sign")
        ));
        assert!(matches!(
            parse(r#"{"retCode":20001,"retMsg":"repeat auth","op":"auth"}"#).unwrap(),
            Frame::Auth(Err(_))
        ));
        assert!(matches!(
            parse(r#"{"success":true,"op":"auth"}"#).unwrap(),
            Frame::Auth(Err(_))
        ));
        assert_eq!(
            parse(
                r#"{"reqId":"p","retCode":0,"retMsg":"OK","op":"pong","data":["1"],"connId":"c"}"#
            )
            .unwrap(),
            Frame::Pong
        );
        assert_eq!(
            parse(r#"{"op":"ping","retMsg":"pong"}"#).unwrap(),
            Frame::Pong
        );
        assert_eq!(
            parse(r#"{"op":"ping","ret_msg":"pong","success":true}"#).unwrap(),
            Frame::Pong
        );
        assert_eq!(parse(r#"{"op":"ping"}"#).unwrap(), Frame::Other);
        assert_eq!(
            parse(r#"{"reqId":"cX","retCode":0,"retMsg":"OK","op":"order.create","data":{"orderId":"1"}}"#)
                .unwrap(),
            Frame::Answer {
                req_id: Some("cX".into()),
                code: Some(0)
            }
        );
        assert_eq!(
            parse(
                r#"{"retCode":10019,"retMsg":"ws trade service is restarting","op":"order.create"}"#
            )
            .unwrap(),
            Frame::Answer {
                req_id: None,
                code: Some(10019)
            }
        );
        assert_eq!(
            parse(r#"{"reqId":"x1X","op":"order.cancel"}"#).unwrap(),
            Frame::Answer {
                req_id: Some("x1X".into()),
                code: None
            }
        );
        assert_eq!(parse(r#"{"op":"notice"}"#).unwrap(), Frame::Other);
        assert!(parse("not json").is_err());
    }
}
