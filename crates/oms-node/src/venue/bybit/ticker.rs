//! The reference price (ADR-028 §7f, U17c): the mark price of every traded
//! symbol from the public stream of category linear (`tickers.<symbol>`).
//!
//! One connection task (`run`): connect over IPv4 as the other two
//! connections do, subscribe, then read. Ping and pong, the reconnect
//! waits and the blocks as on the private stream; a public stream has no
//! auth. The connection does not drive the link: the private stream does.
//!
//! The venue sends a snapshot of each symbol, then deltas with the changed
//! fields only (F20 2.10, the 11-18 collector design). A delta without
//! `markPrice` means the mark is unchanged as of its `ts`. A delta before
//! the snapshot of its connection is ignored, and every connection starts
//! with no mark. A symbol's price goes to the core on a ticker frame, at
//! most once per `ref_price_interval_ms`; no timer sends a held price
//! again, so a symbol the venue stops sending goes stale in the core.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use oms_core::*;
use serde_json::{Value, json};
use tokio::time::Instant;
use tokio_tungstenite::tungstenite::Message;

use super::gateway::{End, Timing, connect};
use super::stream;
use crate::venue::VenueInputTx;
use crate::{info, warn};

/// Topics per subscribe request: the spot limit, used for every market as
/// in md-bybit.
pub const ARGS_PER_SUBSCRIBE: usize = 10;

pub fn topic(symbol: &str) -> String {
    format!("tickers.{symbol}")
}

/// The subscribe requests for `symbols`, at most `ARGS_PER_SUBSCRIBE`
/// topics each.
pub fn subscribe_messages(symbols: &[String]) -> Vec<String> {
    symbols
        .chunks(ARGS_PER_SUBSCRIBE)
        .enumerate()
        .map(|(n, chunk)| {
            let args: Vec<String> = chunk.iter().map(|s| topic(s)).collect();
            json!({"req_id": format!("tickers-{n}"), "op": "subscribe", "args": args}).to_string()
        })
        .collect()
}

/// Whether a ticker frame holds the whole state or the changed fields.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Snapshot,
    Delta,
}

/// One ticker frame of a traded symbol.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ticker {
    pub symbol: InstrumentId,
    pub kind: Kind,
    /// Always present in a snapshot.
    pub mark: Option<Px>,
    /// The frame's `ts`, in µs.
    pub venue_ts: Ts,
}

/// One text frame of the public stream.
#[derive(Debug, PartialEq)]
pub enum Frame {
    /// The answer to a subscribe request: anything other than
    /// `success: true` is a refusal.
    Subscribe(Result<(), String>),
    /// A pong in either documented shape: `op = "pong"`, or `op = "ping"`
    /// with `ret_msg` (or `retMsg`) `pong`, the public linear shape (F20
    /// 2.10).
    Pong,
    Ticker(Ticker),
    /// Another topic or operation, or a ticker of a symbol not traded.
    /// Nothing is read from it.
    Other,
}

/// Reads one text frame. `Err` if it is not JSON, or a ticker frame of a
/// traded symbol has a bad `type`, `symbol`, `markPrice` or `ts`, or is a
/// snapshot without `markPrice`: the stream cannot be trusted after it.
pub fn parse(text: &str, traded: &HashSet<String>) -> Result<Frame, String> {
    let v: Value = serde_json::from_str(text).map_err(|e| format!("not JSON: {e}"))?;
    if let Some(topic) = v.get("topic").and_then(Value::as_str) {
        let Some(symbol) = topic.strip_prefix("tickers.") else {
            return Ok(Frame::Other);
        };
        if !traded.contains(symbol) {
            return Ok(Frame::Other);
        }
        return ticker(&v, symbol).map(Frame::Ticker);
    }
    let op = v.get("op").and_then(Value::as_str);
    let pong = |k: &str| v.get(k).and_then(Value::as_str) == Some("pong");
    Ok(match op {
        Some("subscribe") => Frame::Subscribe(stream::outcome(&v)),
        Some("pong") => Frame::Pong,
        Some("ping") if pong("ret_msg") || pong("retMsg") => Frame::Pong,
        _ => Frame::Other,
    })
}

fn ticker(v: &Value, symbol: &str) -> Result<Ticker, String> {
    let kind = match v.get("type").and_then(Value::as_str) {
        Some("snapshot") => Kind::Snapshot,
        Some("delta") => Kind::Delta,
        other => return Err(format!("ticker {symbol}: type {other:?}")),
    };
    let data = v
        .get("data")
        .filter(|d| d.is_object())
        .ok_or_else(|| format!("ticker {symbol}: no data object"))?;
    if data.get("symbol").and_then(Value::as_str) != Some(symbol) {
        return Err(format!(
            "ticker {symbol}: data.symbol is {:?}",
            data.get("symbol")
        ));
    }
    let mark = match data.get("markPrice") {
        None => None,
        Some(m) => {
            let px = m
                .as_str()
                .and_then(|s| Px::parse(s).ok())
                .filter(|p| p.0 > 0)
                .ok_or_else(|| format!("ticker {symbol}: markPrice {m}"))?;
            Some(px)
        }
    };
    if kind == Kind::Snapshot && mark.is_none() {
        return Err(format!("ticker {symbol}: a snapshot without markPrice"));
    }
    let venue_ts = v
        .get("ts")
        .and_then(Value::as_u64)
        .and_then(|ms| ms.checked_mul(1000))
        .ok_or_else(|| format!("ticker {symbol}: ts {:?}", v.get("ts")))?;
    Ok(Ticker {
        symbol: InstrumentId::new(symbol).map_err(|e| format!("ticker {symbol}: {e:?}"))?,
        kind,
        mark,
        venue_ts,
    })
}

/// The marks of the current connection, and when each symbol's price last
/// went to the core.
pub struct Marks {
    interval: Duration,
    /// Per symbol, the mark since its snapshot on this connection.
    held: HashMap<InstrumentId, Px>,
    /// Per symbol, when its price last went to the core, over all
    /// connections: a reconnect does not send more often.
    sent: HashMap<InstrumentId, std::time::Instant>,
}

impl Marks {
    pub fn new(interval: Duration) -> Marks {
        Marks {
            interval,
            held: HashMap::new(),
            sent: HashMap::new(),
        }
    }

    /// A new connection: no mark is known until its snapshot.
    pub fn reset(&mut self) {
        self.held.clear();
    }

    /// Takes one ticker frame read at `now`. Returns the reference price
    /// for the core when the symbol has a mark on this connection and its
    /// last one went out at least one interval ago.
    pub fn on_ticker(&mut self, t: &Ticker, now: std::time::Instant) -> Option<VenueInput> {
        match (t.kind, t.mark) {
            (Kind::Snapshot, Some(px)) => {
                self.held.insert(t.symbol.clone(), px);
            }
            (Kind::Snapshot, None) => return None,
            (Kind::Delta, _) if !self.held.contains_key(&t.symbol) => return None,
            (Kind::Delta, Some(px)) => {
                self.held.insert(t.symbol.clone(), px);
            }
            (Kind::Delta, None) => {}
        }
        let px = *self.held.get(&t.symbol)?;
        let due = self
            .sent
            .get(&t.symbol)
            .is_none_or(|at| now.saturating_duration_since(*at) >= self.interval);
        if !due {
            return None;
        }
        self.sent.insert(t.symbol.clone(), now);
        Some(VenueInput::RefPrice {
            instrument: t.symbol.clone(),
            px,
            venue_ts: t.venue_ts,
        })
    }
}

/// What the connection asks of the gateway.
pub(crate) trait Host: Send + Sync + 'static {
    /// Why no request may go out now, or `None`: the connection opens only
    /// while there is none, and closes when one appears (as the private
    /// stream).
    fn block(&self) -> Option<String>;
    /// HTTP 403 on the upgrade.
    fn ban(&self, why: String);
}

/// Where the connection connects and what it reads.
pub(crate) struct Conn {
    pub url: String,
    /// The traded symbols, in the order they are subscribed.
    pub symbols: Vec<String>,
    pub timing: Timing,
    pub input: VenueInputTx,
}

/// The connection task: connects while no block is set, and again after
/// every end with the private stream's waits (they start over only after a
/// connection that answered a ping).
pub(crate) async fn run<H: Host>(host: Arc<H>, c: Conn) {
    let mut backoff = c.timing.backoff_min;
    let mut marks = Marks::new(c.timing.ref_price);
    loop {
        if host.block().is_some() {
            tokio::time::sleep(c.timing.health_poll).await;
            continue;
        }
        marks.reset();
        let mut alive = false;
        let end = once(&*host, &c, &mut marks, &mut alive).await;
        if alive {
            backoff = c.timing.backoff_min;
        }
        match end {
            End::Retry(e) | End::Rejected(e) => {
                warn!("the public ticker stream is down; no reference price reaches the core until it is back", "error": e)
            }
            // The upgrade is an HTTP request too (§7c).
            End::IpBanned(e) => host.ban(format!("public stream upgrade: {e}")),
        }
        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(c.timing.backoff_max);
    }
}

/// One connection, from the upgrade to its end.
async fn once<H: Host>(host: &H, c: &Conn, marks: &mut Marks, alive: &mut bool) -> End {
    let t = &c.timing;
    let ws = match tokio::time::timeout(t.handshake, connect(&c.url)).await {
        Err(_) => return End::Retry("connect timed out".into()),
        Ok(Err(e)) => return e,
        Ok(Ok(ws)) => ws,
    };
    let (mut write, mut read) = ws.split();
    if let Some(b) = host.block() {
        return End::Retry(format!("blocked before the subscription: {b}"));
    }
    let traded: HashSet<String> = c.symbols.iter().cloned().collect();
    let requests = subscribe_messages(&c.symbols);
    for m in &requests {
        if let Err(e) = write.send(Message::text(m.clone())).await {
            return End::Retry(format!("subscribe not sent: {e}"));
        }
    }
    // Every request must be confirmed within the handshake time. Tickers
    // read before that count: a snapshot is sent once per subscription.
    let mut confirmed = 0usize;
    let mut confirm_due = Some(Instant::now() + t.handshake);

    let mut ping = tokio::time::interval(t.ping);
    ping.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    ping.tick().await;
    let mut health = tokio::time::interval(t.health_poll);
    let mut pings = 0u64;
    let mut pong_due: Option<Instant> = None;
    loop {
        tokio::select! {
            // What the socket already holds is read before any deadline is
            // judged: a pong in the buffer is not missing.
            biased;
            m = read.next() => {
                let text = match m {
                    None => return End::Retry("closed by the venue".into()),
                    Some(Err(e)) => return End::Retry(format!("read failed: {e}")),
                    Some(Ok(Message::Text(t))) => t,
                    Some(Ok(_)) => continue,
                };
                match parse(text.as_str(), &traded) {
                    Err(e) => return End::Retry(format!("a ticker message could not be read: {e}")),
                    Ok(Frame::Pong) => {
                        pong_due = None;
                        *alive = true;
                    }
                    Ok(Frame::Subscribe(Err(e))) => return End::Retry(format!("subscribe refused: {e}")),
                    Ok(Frame::Subscribe(Ok(()))) => {
                        confirmed += 1;
                        if confirmed == requests.len() && confirm_due.take().is_some() {
                            info!("the public ticker stream is subscribed", "symbols": c.symbols.len());
                        }
                    }
                    Ok(Frame::Ticker(tk)) => {
                        if let Some(i) = marks.on_ticker(&tk, std::time::Instant::now()) {
                            // Time spent waiting on the core's queue does not
                            // count against the pong, as on the private stream.
                            let delivering = Instant::now();
                            if !c.input.send(i).await {
                                return End::Retry("the core is gone".into());
                            }
                            if let Some(d) = pong_due.as_mut() {
                                *d += delivering.elapsed();
                            }
                        }
                    }
                    Ok(Frame::Other) => {}
                }
            }
            _ = tokio::time::sleep_until(confirm_due.unwrap_or_else(Instant::now)), if confirm_due.is_some() => {
                return End::Retry("subscribe not confirmed in time".into());
            }
            // A quiet stream and a half-open connection look the same; only
            // the pong tells them apart.
            _ = tokio::time::sleep_until(pong_due.unwrap_or_else(Instant::now)), if pong_due.is_some() => {
                return End::Retry(format!("no pong within {} ms of a ping", t.ping.as_millis()));
            }
            _ = ping.tick(), if pong_due.is_none() => {
                pings += 1;
                match tokio::time::timeout(t.ping, write.send(Message::text(stream::ping_message(pings)))).await {
                    Ok(Ok(())) => {}
                    Ok(Err(e)) => return End::Retry(format!("ping failed: {e}")),
                    Err(_) => return End::Retry("a ping write timed out".into()),
                }
                pong_due = Some(Instant::now() + t.ping);
            }
            _ = health.tick() => {
                if let Some(b) = host.block() {
                    return End::Retry(format!("blocked: {b}"));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn traded() -> HashSet<String> {
        ["BTCUSDT".to_string()].into()
    }

    fn btc() -> InstrumentId {
        InstrumentId::new("BTCUSDT").unwrap()
    }

    fn frame(kind: &str, data: Value, ts: Value) -> String {
        json!({"topic": "tickers.BTCUSDT", "type": kind, "data": data, "cs": 1, "ts": ts})
            .to_string()
    }

    fn tick(kind: Kind, mark: Option<&str>, ts_ms: u64) -> Ticker {
        Ticker {
            symbol: btc(),
            kind,
            mark: mark.map(|m| Px::parse(m).unwrap()),
            venue_ts: ts_ms * 1000,
        }
    }

    fn refpx(px: &str, ts_ms: u64) -> Option<VenueInput> {
        Some(VenueInput::RefPrice {
            instrument: btc(),
            px: Px::parse(px).unwrap(),
            venue_ts: ts_ms * 1000,
        })
    }

    #[test]
    fn subscribe_requests_hold_at_most_ten_topics_each() {
        let symbols: Vec<String> = (0..25).map(|i| format!("S{i}USDT")).collect();
        let msgs: Vec<Value> = subscribe_messages(&symbols)
            .iter()
            .map(|m| serde_json::from_str(m).unwrap())
            .collect();
        assert_eq!(msgs.len(), 3);
        let sizes: Vec<usize> = msgs
            .iter()
            .map(|m| m["args"].as_array().unwrap().len())
            .collect();
        assert_eq!(sizes, [10, 10, 5]);
        assert!(msgs.iter().all(|m| m["op"] == "subscribe"));
        let all: Vec<String> = msgs
            .iter()
            .flat_map(|m| m["args"].as_array().unwrap().clone())
            .map(|a| a.as_str().unwrap().to_string())
            .collect();
        let want: Vec<String> = symbols.iter().map(|s| format!("tickers.{s}")).collect();
        assert_eq!(all, want, "every symbol once, in order");
        assert_eq!(subscribe_messages(&symbols[..10]).len(), 1);
        assert_eq!(subscribe_messages(&symbols[..11]).len(), 2);
    }

    #[test]
    fn frames_are_read_by_their_documented_fields() {
        let t = traded();
        // F20 2.10: the public linear pong; the private shape too.
        assert_eq!(
            parse(
                r#"{"success":true,"ret_msg":"pong","conn_id":"c","req_id":"","op":"ping"}"#,
                &t
            )
            .unwrap(),
            Frame::Pong
        );
        assert_eq!(
            parse(
                r#"{"req_id":"1","op":"pong","args":["1"],"conn_id":"c"}"#,
                &t
            )
            .unwrap(),
            Frame::Pong
        );
        assert_eq!(
            parse(r#"{"op":"ping","retMsg":"pong"}"#, &t).unwrap(),
            Frame::Pong
        );
        assert_eq!(parse(r#"{"op":"ping"}"#, &t).unwrap(), Frame::Other);
        assert_eq!(
            parse(
                r#"{"success":true,"ret_msg":"","conn_id":"c","req_id":"tickers-0","op":"subscribe"}"#,
                &t
            )
            .unwrap(),
            Frame::Subscribe(Ok(()))
        );
        assert!(matches!(
            parse(r#"{"success":false,"ret_msg":"error:handler not found","op":"subscribe"}"#, &t).unwrap(),
            Frame::Subscribe(Err(m)) if m.contains("handler not found")
        ));
        assert!(matches!(
            parse(r#"{"op":"subscribe"}"#, &t).unwrap(),
            Frame::Subscribe(Err(_))
        ));

        // A snapshot and a delta (the delta carries the changed fields only).
        let snap = json!({"symbol": "BTCUSDT", "markPrice": "30000.10", "lastPrice": "30001", "indexPrice": "29999"});
        assert_eq!(
            parse(&frame("snapshot", snap, json!(1_700_000_000_123u64)), &t).unwrap(),
            Frame::Ticker(tick(Kind::Snapshot, Some("30000.1"), 1_700_000_000_123))
        );
        assert_eq!(
            parse(
                &frame(
                    "delta",
                    json!({"symbol": "BTCUSDT", "markPrice": "30002.5"}),
                    json!(5)
                ),
                &t
            )
            .unwrap(),
            Frame::Ticker(tick(Kind::Delta, Some("30002.5"), 5))
        );
        assert_eq!(
            parse(
                &frame(
                    "delta",
                    json!({"symbol": "BTCUSDT", "fundingRate": "0.0001"}),
                    json!(6)
                ),
                &t
            )
            .unwrap(),
            Frame::Ticker(tick(Kind::Delta, None, 6))
        );
        // Not traded, or another topic: nothing is read, not even a bad field.
        let eth = json!({"topic": "tickers.ETHUSDT", "type": "snapshot", "data": {"symbol": "ETHUSDT", "markPrice": "x"}, "ts": 1});
        assert_eq!(parse(&eth.to_string(), &t).unwrap(), Frame::Other);
        assert_eq!(
            parse(r#"{"topic":"publicTrade.BTCUSDT","data":[]}"#, &t).unwrap(),
            Frame::Other
        );
    }

    #[test]
    fn an_unreadable_ticker_frame_is_an_error() {
        let t = traded();
        let good = || json!({"symbol": "BTCUSDT", "markPrice": "30000"});
        let bad = [
            "not json".to_string(),
            frame("full", good(), json!(1)),
            json!({"topic": "tickers.BTCUSDT", "data": good(), "ts": 1}).to_string(),
            frame("snapshot", json!([good()]), json!(1)),
            frame(
                "snapshot",
                json!({"symbol": "ETHUSDT", "markPrice": "1"}),
                json!(1),
            ),
            frame("snapshot", json!({"markPrice": "1"}), json!(1)),
            frame(
                "snapshot",
                json!({"symbol": "BTCUSDT", "markPrice": 30000}),
                json!(1),
            ),
            frame(
                "delta",
                json!({"symbol": "BTCUSDT", "markPrice": "x"}),
                json!(1),
            ),
            frame(
                "delta",
                json!({"symbol": "BTCUSDT", "markPrice": "0"}),
                json!(1),
            ),
            frame(
                "delta",
                json!({"symbol": "BTCUSDT", "markPrice": "-1"}),
                json!(1),
            ),
            frame(
                "delta",
                json!({"symbol": "BTCUSDT", "markPrice": "0.000000001"}),
                json!(1),
            ),
            frame(
                "snapshot",
                json!({"symbol": "BTCUSDT", "lastPrice": "1"}),
                json!(1),
            ),
            frame("snapshot", good(), json!("1700000000000")),
            frame("snapshot", good(), json!(-1)),
            frame("snapshot", good(), json!(u64::MAX)),
            json!({"topic": "tickers.BTCUSDT", "type": "snapshot", "data": good()}).to_string(),
        ];
        for b in bad {
            assert!(parse(&b, &t).is_err(), "read: {b}");
        }
    }

    #[test]
    fn a_snapshot_then_deltas_give_the_mark_at_most_once_per_interval() {
        let t0 = std::time::Instant::now();
        let at = |ms: u64| t0 + Duration::from_millis(ms);
        let mut m = Marks::new(Duration::from_millis(1000));
        // A delta before any snapshot gives nothing, and is not kept.
        assert_eq!(m.on_ticker(&tick(Kind::Delta, Some("1"), 1), at(0)), None);
        assert_eq!(m.on_ticker(&tick(Kind::Delta, None, 2), at(1500)), None);
        // The snapshot: its mark at once.
        assert_eq!(
            m.on_ticker(&tick(Kind::Snapshot, Some("100"), 10), at(2000)),
            refpx("100", 10)
        );
        // Within the interval: nothing, but a new mark is kept.
        assert_eq!(
            m.on_ticker(&tick(Kind::Delta, Some("101"), 11), at(2001)),
            None
        );
        assert_eq!(m.on_ticker(&tick(Kind::Delta, None, 12), at(2999)), None);
        // One interval after the last emission: the held mark, as of the
        // frame's ts, from a delta without markPrice.
        assert_eq!(
            m.on_ticker(&tick(Kind::Delta, None, 13), at(3000)),
            refpx("101", 13)
        );
        assert_eq!(
            m.on_ticker(&tick(Kind::Delta, Some("102"), 14), at(3999)),
            None
        );
        assert_eq!(
            m.on_ticker(&tick(Kind::Delta, Some("103"), 15), at(4000)),
            refpx("103", 15)
        );
        // Another symbol has its own interval.
        let eth = Ticker {
            symbol: InstrumentId::new("ETHUSDT").unwrap(),
            ..tick(Kind::Snapshot, Some("7"), 16)
        };
        assert!(m.on_ticker(&eth, at(4001)).is_some());
    }

    #[test]
    fn a_new_connection_knows_no_mark_until_its_snapshot_and_keeps_the_interval() {
        let t0 = std::time::Instant::now();
        let at = |ms: u64| t0 + Duration::from_millis(ms);
        let mut m = Marks::new(Duration::from_millis(1000));
        assert_eq!(
            m.on_ticker(&tick(Kind::Snapshot, Some("100"), 1), at(0)),
            refpx("100", 1)
        );
        m.reset();
        // The old connection's mark is gone: a delta gives nothing.
        assert_eq!(m.on_ticker(&tick(Kind::Delta, None, 2), at(5000)), None);
        assert_eq!(
            m.on_ticker(&tick(Kind::Delta, Some("1"), 3), at(5001)),
            None
        );
        // A reconnect does not send more often than the interval.
        let mut m = Marks::new(Duration::from_millis(1000));
        assert!(
            m.on_ticker(&tick(Kind::Snapshot, Some("100"), 1), at(0))
                .is_some()
        );
        m.reset();
        assert_eq!(
            m.on_ticker(&tick(Kind::Snapshot, Some("100"), 2), at(500)),
            None
        );
        assert_eq!(
            m.on_ticker(&tick(Kind::Delta, None, 3), at(1000)),
            refpx("100", 3)
        );
    }
}
