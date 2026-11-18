//! A loopback fake of Bybit's public WebSocket and of its instruments REST
//! endpoint, on `127.0.0.1:0`. Frames follow the documented v5 shapes with
//! synthetic values (public repository; Bybit API terms 6.7). Each accepted
//! connection answers its subscribe frames, then sends per subscribed topic a
//! snapshot and deltas (books) or trades, and answers `ping`. It records the
//! market (URL path) and every frame it sent.
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::handshake::server::{Request, Response};

#[derive(Debug, Clone)]
pub struct Behaviour {
    /// Deltas (books) or trades per topic after its first frame.
    pub per_topic: usize,
    /// Drop the i-th accepted connection after this many sent data frames.
    pub drop_after: BTreeMap<usize, usize>,
    /// After the first frames, keep sending one delta of every book topic
    /// every `stream_every` until the connection ends.
    pub stream_every: Option<Duration>,
    /// Pause between frames.
    pub pace: Duration,
}

impl Default for Behaviour {
    fn default() -> Self {
        Self {
            per_topic: 3,
            drop_after: BTreeMap::new(),
            stream_every: None,
            pace: Duration::from_millis(1),
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct FakeConn {
    /// The market of the URL path (`/linear`, `/spot`, ...).
    pub market: String,
    /// Frames the client sent.
    pub got: Vec<String>,
    /// Data frames this connection sent, in order (book, trade, ticker).
    pub data: Vec<String>,
    /// Control frames this connection sent (subscribe answers, pongs).
    pub control: Vec<String>,
    /// The connection has ended.
    pub ended: bool,
}

pub struct FakeBybit {
    /// `ws://127.0.0.1:port` (append `/<market>`).
    pub ws_base: String,
    pub conns: Arc<Mutex<Vec<FakeConn>>>,
    pub accepted: Arc<AtomicUsize>,
}

impl FakeBybit {
    pub async fn start(b: Behaviour) -> FakeBybit {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let ws_base = format!("ws://{}", listener.local_addr().expect("addr"));
        let conns: Arc<Mutex<Vec<FakeConn>>> = Arc::default();
        let accepted = Arc::new(AtomicUsize::new(0));
        let (c2, a2) = (conns.clone(), accepted.clone());
        tokio::spawn(async move {
            loop {
                let Ok((tcp, _)) = listener.accept().await else {
                    return;
                };
                let i = a2.fetch_add(1, Ordering::SeqCst);
                c2.lock().expect("conns").push(FakeConn::default());
                tokio::spawn(serve(tcp, i, c2.clone(), b.clone()));
            }
        });
        FakeBybit {
            ws_base,
            conns,
            accepted,
        }
    }

    pub fn ws_url(&self, market: &str) -> String {
        format!("{}/{market}", self.ws_base)
    }

    pub fn conns(&self) -> Vec<FakeConn> {
        self.conns.lock().expect("conns").clone()
    }

    /// Data frames sent per (market, channel, symbol), in order, over all
    /// connections in accept order.
    pub fn sent_by_topic(&self) -> BTreeMap<(String, String, String), Vec<String>> {
        let mut out: BTreeMap<(String, String, String), Vec<String>> = BTreeMap::new();
        for c in self.conns() {
            for f in &c.data {
                let (ch, sym) = topic_of(f);
                out.entry((c.market.clone(), ch, sym))
                    .or_default()
                    .push(f.clone());
            }
        }
        out
    }
}

/// (channel, symbol) of a data frame.
pub fn topic_of(frame: &str) -> (String, String) {
    let v: serde_json::Value = serde_json::from_str(frame).expect("json");
    let t = v["topic"].as_str().expect("topic");
    let (c, s) = t.rsplit_once('.').expect("channel.symbol");
    (c.to_string(), s.to_string())
}

fn book(topic: &str, kind: &str, u: i64, ts: i64) -> String {
    let (_, symbol) = topic.rsplit_once('.').expect("topic");
    format!(
        r#"{{"topic":"{topic}","type":"{kind}","ts":{ts},"data":{{"s":"{symbol}","b":[["42000.0","{q}.000"]],"a":[["42000.5","2.000"]],"u":{u},"seq":{seq}}},"cts":{cts}}}"#,
        q = 1 + u % 7,
        seq = 90_000 + u,
        cts = ts - 2
    )
}

fn trade(topic: &str, n: i64, ts: i64) -> String {
    let (_, symbol) = topic.rsplit_once('.').expect("topic");
    format!(
        r#"{{"topic":"{topic}","type":"snapshot","ts":{ts},"data":[{{"T":{ts},"s":"{symbol}","S":"Buy","v":"0.010","p":"42000.0","L":"PlusTick","i":"t{n}","BT":false,"RPI":false,"seq":{n}}}]}}"#
    )
}

fn ticker(topic: &str, ts: i64) -> String {
    let (_, symbol) = topic.rsplit_once('.').expect("topic");
    format!(
        r#"{{"topic":"{topic}","type":"snapshot","ts":{ts},"cs":1,"data":{{"symbol":"{symbol}","lastPrice":"42000.0"}}}}"#
    )
}

async fn serve(
    tcp: tokio::net::TcpStream,
    i: usize,
    conns: Arc<Mutex<Vec<FakeConn>>>,
    b: Behaviour,
) {
    let path = Arc::new(Mutex::new(String::new()));
    let p2 = path.clone();
    let cb = move |req: &Request, resp: Response| {
        *p2.lock().expect("path") = req.uri().path().trim_start_matches('/').to_string();
        Ok(resp)
    };
    let Ok(ws) = tokio_tungstenite::accept_hdr_async(tcp, cb).await else {
        return;
    };
    let market = path.lock().expect("path").clone();
    let set = |f: &dyn Fn(&mut FakeConn)| {
        if let Some(c) = conns.lock().expect("conns").get_mut(i) {
            f(c);
        }
    };
    set(&|c| c.market = market.clone());
    let (mut write, mut read) = ws.split();
    let (out_tx, mut out_rx) = tokio::sync::mpsc::unbounded_channel::<(bool, String)>();
    let stop = Arc::new(AtomicBool::new(false));
    // Reader: subscribe answers and the topics to send; pongs.
    let (topics_tx, mut topics_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let (conns_r, otx, stop_r) = (conns.clone(), out_tx.clone(), stop.clone());
    let reader = tokio::spawn(async move {
        while let Some(Ok(m)) = read.next().await {
            let Message::Text(t) = m else { continue };
            let t = t.as_str().to_string();
            if let Some(c) = conns_r.lock().expect("conns").get_mut(i) {
                c.got.push(t.clone());
            }
            let v: serde_json::Value = serde_json::from_str(&t).unwrap_or_default();
            match v["op"].as_str() {
                Some("subscribe") => {
                    let req_id = v["req_id"].as_str().unwrap_or("");
                    let _ = otx.send((
                        false,
                        format!(
                            r#"{{"success":true,"ret_msg":"","conn_id":"fake-{i}","req_id":"{req_id}","op":"subscribe"}}"#
                        ),
                    ));
                    for a in v["args"].as_array().cloned().unwrap_or_default() {
                        if let Some(a) = a.as_str() {
                            let _ = topics_tx.send(a.to_string());
                        }
                    }
                }
                Some("ping") => {
                    let _ = otx.send((
                        false,
                        format!(r#"{{"success":true,"ret_msg":"pong","conn_id":"fake-{i}","op":"ping"}}"#),
                    ));
                }
                _ => {}
            }
        }
        stop_r.store(true, Ordering::SeqCst);
    });
    // Producer: frames of each subscribed topic.
    let (otx, stop_p, b2) = (out_tx.clone(), stop.clone(), b.clone());
    let producer = tokio::spawn(async move {
        let base = 1_000 * i64::try_from(i + 1).unwrap_or(1);
        let mut books: Vec<(String, i64)> = Vec::new();
        let mut ts = 1_767_362_400_000i64;
        // Topics arrive in subscribe order; wait briefly for all of them.
        let mut topics = Vec::new();
        while let Ok(Some(t)) =
            tokio::time::timeout(Duration::from_millis(150), topics_rx.recv()).await
        {
            topics.push(t);
        }
        for t in &topics {
            ts += 1;
            if t.starts_with("orderbook.") {
                let _ = otx.send((true, book(t, "snapshot", base, ts)));
                for k in 1..=b2.per_topic {
                    ts += 1;
                    let _ = otx.send((true, book(t, "delta", base + k as i64, ts)));
                }
                books.push((t.clone(), base + b2.per_topic as i64));
            } else if t.starts_with("publicTrade.") {
                for k in 0..=b2.per_topic {
                    ts += 1;
                    let _ = otx.send((true, trade(t, base + k as i64, ts)));
                }
            } else {
                let _ = otx.send((true, ticker(t, ts)));
            }
        }
        if let Some(every) = b2.stream_every {
            while !stop_p.load(Ordering::SeqCst) {
                for (t, u) in books.iter_mut() {
                    *u += 1;
                    ts += 1;
                    if otx.send((true, book(t, "delta", *u, ts))).is_err() {
                        return;
                    }
                }
                tokio::time::sleep(every).await;
            }
        }
    });
    // Only the reader and the producer hold senders: the loop below ends
    // when both have ended.
    drop(out_tx);
    let mut sent_data = 0usize;
    let limit = b.drop_after.get(&i).copied();
    while let Some((is_data, f)) = out_rx.recv().await {
        if limit.is_some_and(|l| is_data && sent_data >= l) {
            break;
        }
        if write.send(Message::text(f.clone())).await.is_err() {
            break;
        }
        if let Some(c) = conns.lock().expect("conns").get_mut(i) {
            if is_data {
                c.data.push(f);
            } else {
                c.control.push(f);
            }
        }
        if is_data {
            sent_data += 1;
        }
        if !b.pace.is_zero() {
            tokio::time::sleep(b.pace).await;
        }
        if stop.load(Ordering::SeqCst) {
            break;
        }
    }
    stop.store(true, Ordering::SeqCst);
    reader.abort();
    producer.abort();
    set(&|c| c.ended = true);
}

/// A minimal HTTP/1.1 server for `GET /v5/market/instruments-info`: the
/// answer comes from `pages` by `(category, cursor)` (cursor "" = first
/// page); unknown requests get 404. Records the request lines.
pub struct FakeRest {
    pub base: String,
    pub requests: Arc<Mutex<Vec<String>>>,
    /// Bodies served, in order.
    pub served: Arc<Mutex<Vec<(u16, String)>>>,
}

pub type Pages = BTreeMap<(String, String), (u16, String)>;

fn query_value(line: &str, key: &str) -> String {
    let q = line.split_whitespace().nth(1).unwrap_or("");
    let q = q.split_once('?').map(|(_, q)| q).unwrap_or("");
    q.split('&')
        .filter_map(|kv| kv.split_once('='))
        .find(|(k, _)| *k == key)
        .map(|(_, v)| v.to_string())
        .unwrap_or_default()
}

impl FakeRest {
    pub async fn start(pages: Pages) -> FakeRest {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let base = format!("http://{}", listener.local_addr().expect("addr"));
        let requests: Arc<Mutex<Vec<String>>> = Arc::default();
        let served: Arc<Mutex<Vec<(u16, String)>>> = Arc::default();
        let pages = Arc::new(pages);
        let (r2, s2) = (requests.clone(), served.clone());
        tokio::spawn(async move {
            loop {
                let Ok((mut tcp, _)) = listener.accept().await else {
                    return;
                };
                let (r3, s3, p3) = (r2.clone(), s2.clone(), pages.clone());
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
                    let line = head.lines().next().unwrap_or("").to_string();
                    r3.lock().expect("requests").push(line.clone());
                    let key = (query_value(&line, "category"), query_value(&line, "cursor"));
                    let (status, body) = p3
                        .get(&key)
                        .cloned()
                        .unwrap_or((404, r#"{"retCode":10001,"retMsg":"not found"}"#.into()));
                    s3.lock().expect("served").push((status, body.clone()));
                    let resp = format!(
                        "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = tcp.write_all(resp.as_bytes()).await;
                    let _ = tcp.shutdown().await;
                });
            }
        });
        FakeRest {
            base,
            requests,
            served,
        }
    }
}
