//! Loopback fakes for md-feed tests: a scripted WebSocket server and a minimal
//! HTTP/1.1 responder, both on `127.0.0.1:0`, plus synthetic Bybit frames in
//! the documented shapes. No captured venue data (the repository is public;
//! Bybit API terms 6.7). Nothing here opens a non-loopback socket.
#![allow(dead_code)]

pub mod rest_adapter;

use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use md_bybit::{BybitAdapter, Market};
use md_core::adapter::Instrument;
use md_feed::{ConnIds, FeedConfig, FeedEvent, FrameSink, Overflow, RawFrame};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::{Notify, mpsc};
use tokio_tungstenite::tungstenite::Message;

/// One step of the script of an accepted connection.
pub enum Step {
    /// Send a text frame.
    Send(String),
    /// Wait until the client has sent a text frame containing this text
    /// (counted from the start of the connection), at most 5 s.
    WaitFor(String),
    Pause(Duration),
    /// Send a close frame and end the connection.
    Close,
    /// Drop the socket without a close frame.
    Drop,
}

/// What a fake connection received from the client.
#[derive(Debug, Default)]
pub struct ConnLog {
    pub texts: Vec<String>,
}

pub struct FakeWs {
    pub url: String,
    /// The client frames of each accepted connection, in accept order.
    pub conns: Arc<Mutex<Vec<Arc<Mutex<ConnLog>>>>>,
    pub accepted: Arc<AtomicUsize>,
}

impl FakeWs {
    /// The i-th accepted connection runs `scripts[i]`; later connections run
    /// the last script. After its script a connection stays open, recording
    /// client frames, until the client closes it.
    pub async fn start(scripts: Vec<Vec<Step>>) -> FakeWs {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let url = format!("ws://{}/", listener.local_addr().expect("addr"));
        let conns: Arc<Mutex<Vec<Arc<Mutex<ConnLog>>>>> = Arc::default();
        let accepted = Arc::new(AtomicUsize::new(0));
        let scripts: Arc<Mutex<VecDeque<Vec<Step>>>> = Arc::new(Mutex::new(scripts.into()));
        let (c2, a2) = (conns.clone(), accepted.clone());
        tokio::spawn(async move {
            loop {
                let Ok((tcp, _)) = listener.accept().await else {
                    return;
                };
                a2.fetch_add(1, Ordering::SeqCst);
                let script = {
                    let mut s = scripts.lock().expect("scripts");
                    if s.len() > 1 {
                        s.pop_front().unwrap_or_default()
                    } else {
                        s.front().map(|v| clone_steps(v)).unwrap_or_default()
                    }
                };
                let log = Arc::new(Mutex::new(ConnLog::default()));
                c2.lock().expect("conns").push(log.clone());
                tokio::spawn(serve(tcp, script, log));
            }
        });
        FakeWs {
            url,
            conns,
            accepted,
        }
    }

    pub fn conn(&self, i: usize) -> Option<Arc<Mutex<ConnLog>>> {
        self.conns.lock().expect("conns").get(i).cloned()
    }

    pub fn texts(&self, i: usize) -> Vec<String> {
        self.conn(i)
            .map(|c| c.lock().expect("log").texts.clone())
            .unwrap_or_default()
    }
}

fn clone_steps(steps: &[Step]) -> Vec<Step> {
    steps
        .iter()
        .map(|s| match s {
            Step::Send(t) => Step::Send(t.clone()),
            Step::WaitFor(t) => Step::WaitFor(t.clone()),
            Step::Pause(d) => Step::Pause(*d),
            Step::Close => Step::Close,
            Step::Drop => Step::Drop,
        })
        .collect()
}

async fn serve(tcp: tokio::net::TcpStream, script: Vec<Step>, log: Arc<Mutex<ConnLog>>) {
    let Ok(ws) = tokio_tungstenite::accept_async(tcp).await else {
        return;
    };
    let (mut write, mut read) = ws.split();
    let got = Arc::new(Notify::new());
    let (log2, got2) = (log.clone(), got.clone());
    let reader = tokio::spawn(async move {
        while let Some(Ok(m)) = read.next().await {
            if let Message::Text(t) = m {
                log2.lock().expect("log").texts.push(t.as_str().to_string());
                got2.notify_waiters();
            }
        }
    });
    for step in script {
        match step {
            Step::Send(t) => {
                if write.send(Message::text(t)).await.is_err() {
                    return;
                }
            }
            Step::WaitFor(t) => {
                let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
                loop {
                    let seen = log
                        .lock()
                        .expect("log")
                        .texts
                        .iter()
                        .any(|x| x.contains(&t));
                    if seen {
                        break;
                    }
                    let wait = got.notified();
                    if tokio::time::timeout_at(deadline, wait).await.is_err() {
                        break;
                    }
                }
            }
            Step::Pause(d) => tokio::time::sleep(d).await,
            Step::Close => {
                let _ = write.send(Message::Close(None)).await;
                reader.abort();
                return;
            }
            Step::Drop => {
                reader.abort();
                return;
            }
        }
    }
    let _ = reader.await;
}

/// A minimal HTTP/1.1 server: every request gets the next body of `bodies`
/// (the last one repeats) with status 200. Records the request lines.
pub struct FakeRest {
    pub base: String,
    pub requests: Arc<Mutex<Vec<String>>>,
}

impl FakeRest {
    pub async fn start(bodies: Vec<String>) -> FakeRest {
        Self::start_with_delay(bodies, Duration::ZERO).await
    }

    /// Each answer waits `delay` after the request has arrived.
    pub async fn start_with_delay(bodies: Vec<String>, delay: Duration) -> FakeRest {
        Self::start_full(bodies.into_iter().map(|b| (200, b)).collect(), delay).await
    }

    /// Answers with these statuses and bodies, in order (the last repeats).
    pub async fn start_full(bodies: Vec<(u16, String)>, delay: Duration) -> FakeRest {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let base = format!("http://{}", listener.local_addr().expect("addr"));
        let requests: Arc<Mutex<Vec<String>>> = Arc::default();
        let bodies = Arc::new(Mutex::new(VecDeque::from(bodies)));
        let r2 = requests.clone();
        tokio::spawn(async move {
            loop {
                let Ok((mut tcp, _)) = listener.accept().await else {
                    return;
                };
                let (r3, b3) = (r2.clone(), bodies.clone());
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 8192];
                    let mut have = 0;
                    loop {
                        let Ok(n) = tcp.read(&mut buf[have..]).await else {
                            return;
                        };
                        if n == 0 {
                            return;
                        }
                        have += n;
                        if buf[..have].windows(4).any(|w| w == b"\r\n\r\n") {
                            break;
                        }
                    }
                    let head = String::from_utf8_lossy(&buf[..have]).to_string();
                    r3.lock()
                        .expect("requests")
                        .push(head.lines().next().unwrap_or("").to_string());
                    let (status, body) = {
                        let mut b = b3.lock().expect("bodies");
                        if b.len() > 1 {
                            b.pop_front().unwrap_or((200, String::new()))
                        } else {
                            b.front().cloned().unwrap_or((200, String::new()))
                        }
                    };
                    tokio::time::sleep(delay).await;
                    let resp = format!(
                        "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = tcp.write_all(resp.as_bytes()).await;
                    let _ = tcp.shutdown().await;
                });
            }
        });
        FakeRest { base, requests }
    }
}

/// A sink that keeps every frame.
#[derive(Default)]
pub struct VecSink(pub Mutex<Vec<RawFrame>>);

impl FrameSink for VecSink {
    fn frame(&self, f: RawFrame) -> Result<(), Overflow> {
        self.0.lock().expect("sink").push(f);
        Ok(())
    }
}

impl VecSink {
    pub fn frames(&self) -> Vec<RawFrame> {
        self.0.lock().expect("sink").clone()
    }
}

pub fn instrument(symbol: &str) -> Instrument {
    Instrument {
        symbol: symbol.into(),
        status: "Trading".into(),
        tick_size: "0.10".into(),
        lot_size: "0.001".into(),
        px_exp: -1,
        qty_exp: -3,
        contract_type: Some("LinearPerpetual".into()),
        settle_coin: Some("USDT".into()),
        venue_ts_ns: None,
    }
}

/// Bybit linear with BTCUSDT and ETHUSDT (tick 0.10, step 0.001).
pub fn bybit() -> Arc<BybitAdapter> {
    Arc::new(BybitAdapter::new(
        Market::Linear,
        &[instrument("BTCUSDT"), instrument("ETHUSDT")],
    ))
}

/// A Bybit `orderbook.50` frame.
pub fn book(symbol: &str, kind: &str, u: i64, ts: i64, bids: &str, asks: &str) -> String {
    format!(
        r#"{{"topic":"orderbook.50.{symbol}","type":"{kind}","ts":{ts},"data":{{"s":"{symbol}","b":[{bids}],"a":[{asks}],"u":{u},"seq":{seq}}},"cts":{cts}}}"#,
        seq = 9000 + u,
        cts = ts - 2
    )
}

pub fn trade(symbol: &str, id: &str, ts: i64) -> String {
    format!(
        r#"{{"topic":"publicTrade.{symbol}","type":"snapshot","ts":{ts},"data":[{{"T":{ts},"s":"{symbol}","S":"Buy","v":"0.010","p":"42000.0","L":"PlusTick","i":"{id}","BT":false,"RPI":false,"seq":1}}]}}"#
    )
}

/// Test config: loopback only, short timers.
pub fn config(url: &str, ids: Arc<ConnIds>) -> FeedConfig {
    let mut cfg = FeedConfig::new("linear", url, ids);
    cfg.loopback_only = true;
    cfg.backoff_min = Duration::from_millis(50);
    cfg.backoff_max = Duration::from_millis(400);
    cfg.stale_after = Duration::from_millis(300);
    cfg.resync_timeout = Duration::from_millis(800);
    cfg
}

/// Waits up to 5 s for an event that `pred` accepts; returns it.
pub async fn wait_event(
    rx: &mut mpsc::Receiver<FeedEvent>,
    mut pred: impl FnMut(&FeedEvent) -> bool,
) -> FeedEvent {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        match tokio::time::timeout_at(deadline, rx.recv()).await {
            Ok(Some(e)) if pred(&e) => return e,
            Ok(Some(_)) => continue,
            Ok(None) => panic!("event channel closed"),
            Err(_) => panic!("no matching event within 5 s"),
        }
    }
}

/// Polls `f` every 10 ms for up to 5 s until it is true.
pub async fn eventually(mut f: impl FnMut() -> bool) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while !f() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "condition not met within 5 s"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

pub fn ticker(symbol: &str, kind: &str, ts: i64, fields: &str) -> String {
    format!(
        r#"{{"topic":"tickers.{symbol}","type":"{kind}","ts":{ts},"cs":1,"data":{{"symbol":"{symbol}"{fields}}}}}"#
    )
}

/// One connection with these channels for each symbol.
pub fn plan_of(symbols: &[&str], channels: &[&str]) -> Vec<md_core::adapter::ConnPlan> {
    use md_core::adapter::MarketDataAdapter;
    let symbols: Vec<String> = symbols.iter().map(|s| s.to_string()).collect();
    let channels: Vec<String> = channels.iter().map(|s| s.to_string()).collect();
    bybit().plan(&symbols, &channels, 50).expect("plan")
}

pub fn key(channel: &str, symbol: &str) -> md_core::StreamKey {
    md_core::StreamKey::new(channel, symbol)
}

/// Keeps every forwarded message.
#[derive(Default)]
pub struct VecMsgs(pub Mutex<Vec<(u32, md_core::Normalized)>>);

impl md_feed::MessageSink for VecMsgs {
    fn message(&self, conn_id: u32, msg: &md_core::Normalized) {
        self.0.lock().expect("msgs").push((conn_id, msg.clone()));
    }
}
