//! The fake Bybit WebSockets: the private stream, WS Trade and the public
//! stream, loopback only.

use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

#[derive(Clone, Copy, PartialEq)]
pub enum Kind {
    Private,
    Trade,
    Public,
}

/// What the fake WS Trade answers to an order request (`None`: silence).
pub type OnOrder = Arc<dyn Fn(&Value) -> Option<Value> + Send + Sync>;

/// A loopback Bybit WebSocket of one kind. It answers auth, subscribe and
/// ping in the documented shapes, hands order requests to `on_order`, and
/// sends what a test pushes to the newest connection.
pub struct FakeWs {
    pub url: String,
    /// Every text received, over all connections.
    got: Arc<Mutex<Vec<Value>>>,
    push: Arc<Mutex<Option<mpsc::UnboundedSender<String>>>>,
    /// While set, the fake answers nothing (a half-open network): no
    /// pong, and no auth or subscribe for a reconnect.
    pub silent: Arc<AtomicBool>,
    /// When the last pong was written.
    pub last_pong: Arc<Mutex<Option<Instant>>>,
}

impl FakeWs {
    pub fn start(rt: &tokio::runtime::Runtime, kind: Kind, on_order: Option<OnOrder>) -> FakeWs {
        let listener = rt.block_on(TcpListener::bind("127.0.0.1:0")).unwrap();
        let path = match kind {
            Kind::Private => "/v5/private",
            Kind::Trade => "/v5/trade",
            Kind::Public => "/v5/public/linear",
        };
        let fake = FakeWs {
            url: format!("ws://{}{path}", listener.local_addr().unwrap()),
            got: Arc::default(),
            push: Arc::default(),
            silent: Arc::default(),
            last_pong: Arc::default(),
        };
        let (got, push) = (fake.got.clone(), fake.push.clone());
        let (silent, last_pong) = (fake.silent.clone(), fake.last_pong.clone());
        rt.spawn(async move {
            while let Ok((tcp, _)) = listener.accept().await {
                let Ok(ws) = tokio_tungstenite::accept_async(tcp).await else {
                    continue;
                };
                let (tx, rx) = mpsc::unbounded_channel();
                *push.lock().unwrap() = Some(tx);
                tokio::spawn(serve(
                    ws,
                    kind,
                    got.clone(),
                    rx,
                    on_order.clone(),
                    silent.clone(),
                    last_pong.clone(),
                ));
            }
        });
        fake
    }

    pub fn received(&self, op: &str) -> Vec<Value> {
        self.got
            .lock()
            .unwrap()
            .iter()
            .filter(|v| v["op"] == op)
            .cloned()
            .collect()
    }

    pub fn send(&self, text: String) {
        let _ = self.push.lock().unwrap().as_ref().unwrap().send(text);
    }

    /// [`FakeWs::send`] if a connection is there; `false` if none is.
    pub fn try_send(&self, text: String) -> bool {
        match self.push.lock().unwrap().as_ref() {
            Some(tx) => tx.send(text).is_ok(),
            None => false,
        }
    }

    /// Closes the newest connection (its task ends and drops the socket);
    /// a client reconnects to a new one.
    pub fn drop_connection(&self) {
        *self.push.lock().unwrap() = None;
    }
}

async fn serve(
    ws: tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>,
    kind: Kind,
    got: Arc<Mutex<Vec<Value>>>,
    mut ctl: mpsc::UnboundedReceiver<String>,
    on_order: Option<OnOrder>,
    silent: Arc<AtomicBool>,
    last_pong: Arc<Mutex<Option<Instant>>>,
) {
    let (mut write, mut read) = ws.split();
    loop {
        tokio::select! {
            m = read.next() => {
                let Some(Ok(Message::Text(t))) = m else { return };
                let v: Value = serde_json::from_str(t.as_str()).unwrap_or_default();
                got.lock().unwrap().push(v.clone());
                let answer = match (kind, v["op"].as_str().unwrap_or("")) {
                    (Kind::Private, "auth") => Some(json!({"success": true, "ret_msg": "", "op": "auth", "conn_id": "c"})),
                    (Kind::Private, "subscribe") => Some(json!({"success": true, "ret_msg": "", "op": "subscribe", "conn_id": "c"})),
                    (Kind::Private, "ping") => Some(json!({"req_id": v["req_id"], "op": "pong", "args": ["1"], "conn_id": "c"})),
                    (Kind::Trade, "auth") => Some(json!({"retCode": 0, "retMsg": "OK", "op": "auth", "connId": "c"})),
                    (Kind::Trade, "ping") => Some(json!({"reqId": v["req_id"], "retCode": 0, "retMsg": "OK", "op": "pong", "data": ["1"], "connId": "c"})),
                    (Kind::Trade, "order.create" | "order.cancel") => on_order.as_ref().and_then(|f| f(&v)),
                    (Kind::Public, "subscribe") => Some(json!({"success": true, "ret_msg": "", "conn_id": "c", "req_id": v["req_id"], "op": "subscribe"})),
                    (Kind::Public, "ping") => Some(json!({"success": true, "ret_msg": "pong", "conn_id": "c", "req_id": "", "op": "ping"})),
                    _ => None,
                };
                let answer = answer.filter(|_| !silent.load(Ordering::SeqCst));
                if let Some(a) = answer {
                    if write.send(Message::text(a.to_string())).await.is_err() {
                        return;
                    }
                    if v["op"] == "ping" {
                        *last_pong.lock().unwrap() = Some(Instant::now());
                    }
                }
            }
            c = ctl.recv() => match c {
                Some(t) => {
                    if write.send(Message::text(t)).await.is_err() {
                        return;
                    }
                }
                None => return,
            },
        }
    }
}

/// The answer of WS Trade to an accepted create.
pub fn placed(v: &Value) -> Value {
    json!({"reqId": v["reqId"], "retCode": 0, "retMsg": "OK", "op": v["op"],
           "data": {"orderId": "o-ws-1", "orderLinkId": v["args"][0]["orderLinkId"]},
           "header": {"X-Bapi-Limit": "10"}, "connId": "c"})
}
