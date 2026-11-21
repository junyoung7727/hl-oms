//! The node with `kind = "bybit"` end to end (U17c): the real binary in its
//! own process, against fake Bybit servers in this process (REST, the
//! private stream, WS Trade and the public stream), loopback only.
//!
//! The node reads the fakes' addresses from `OMS_TEST_BYBIT_WIRING`, which
//! only a build with the `failpoints` feature reads; the configuration file
//! still names the testnet and an https echo service. Needs
//! `--features failpoints`.

#![allow(clippy::unwrap_used)]

use futures_util::{SinkExt, StreamExt};
use http_fake::{Answer as Http, FakeHttp};
use oms_api::client::{Client, now_us};
use oms_api::*;
use oms_core::*;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

const CREATE_PATH: &str = "/v5/order/create";

fn ok(result: Value) -> Http {
    Http::Reply(
        200,
        json!({"retCode": 0, "retMsg": "OK", "result": result, "time": 1}).to_string(),
    )
}

fn list(rows: Vec<Value>) -> Http {
    ok(json!({"category": "linear", "list": rows, "nextPageCursor": ""}))
}

/// The REST venue and the IP echo on one loopback server: the boot checks
/// pass, and every query finds nothing.
fn rest_venue() -> Arc<FakeHttp> {
    let api = FakeHttp::start(Http::Reply(404, "{}".into()));
    api.route(
        "/v5/user/query-api",
        [ok(json!({
            "isMaster": false, "userID": 4242, "parentUid": "1000", "readOnly": 0,
            "ips": ["127.0.0.1"],
            "permissions": {"ContractTrade": ["Order", "Position"], "Spot": []}
        }))],
    );
    api.route(
        "/v5/account/info",
        [ok(
            json!({"marginMode": "REGULAR_MARGIN", "unifiedMarginStatus": 5, "updatedTime": "1"}),
        )],
    );
    api.route(
        "/v5/market/instruments-info",
        [ok(
            json!({"category": "linear", "list": [{"symbol": "BTCUSDT", "settleCoin": "USDT", "status": "Trading",
            "priceFilter": {"tickSize": "0.10"}, "lotSizeFilter": {"qtyStep": "0.001"}}], "nextPageCursor": ""}),
        )],
    );
    api.route(
        "/v5/position/list",
        [list(vec![
            json!({"symbol": "BTCUSDT", "positionIdx": 0, "size": "0", "side": ""}),
        ])],
    );
    // A venue clock far ahead (year 2096): the local clock is behind, so
    // the clock check passes whenever the test runs.
    api.route(
        "/v5/market/time",
        [ok(
            json!({"timeSecond": "4000000000", "timeNano": "4000000000000000000"}),
        )],
    );
    api.route("/ip", [Http::Reply(200, "127.0.0.1\n".into())]);
    api.route("/v5/execution/list", [list(vec![])]);
    api.route("/v5/order/realtime", [list(vec![])]);
    api.route("/v5/order/history", [list(vec![])]);
    Arc::new(api)
}

/// An open order of ours as the venue lists it.
fn open_order(link: &str) -> Value {
    json!({
        "orderId": "o-ws-1", "orderLinkId": link, "symbol": "BTCUSDT", "side": "Buy",
        "price": "30000", "qty": "0.001", "orderStatus": "New", "cancelType": "UNKNOWN",
        "rejectReason": "EC_NoError", "leavesQty": "0.001", "cumExecQty": "0",
        "updatedTime": "1700000000123"
    })
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Private,
    Trade,
    Public,
}

/// What the fake WS Trade answers to an order request (`None`: silence).
type OnOrder = Arc<dyn Fn(&Value) -> Option<Value> + Send + Sync>;

/// A loopback Bybit WebSocket of one kind. It answers auth, subscribe and
/// ping in the documented shapes, hands order requests to `on_order`, and
/// sends what a test pushes to the newest connection.
struct FakeWs {
    url: String,
    /// Every text received, over all connections.
    got: Arc<Mutex<Vec<Value>>>,
    push: Arc<Mutex<Option<mpsc::UnboundedSender<String>>>>,
}

impl FakeWs {
    fn start(rt: &tokio::runtime::Runtime, kind: Kind, on_order: Option<OnOrder>) -> FakeWs {
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
        };
        let (got, push) = (fake.got.clone(), fake.push.clone());
        rt.spawn(async move {
            while let Ok((tcp, _)) = listener.accept().await {
                let Ok(ws) = tokio_tungstenite::accept_async(tcp).await else {
                    continue;
                };
                let (tx, rx) = mpsc::unbounded_channel();
                *push.lock().unwrap() = Some(tx);
                tokio::spawn(serve(ws, kind, got.clone(), rx, on_order.clone()));
            }
        });
        fake
    }

    fn received(&self, op: &str) -> Vec<Value> {
        self.got
            .lock()
            .unwrap()
            .iter()
            .filter(|v| v["op"] == op)
            .cloned()
            .collect()
    }

    fn send(&self, text: String) {
        let _ = self.push.lock().unwrap().as_ref().unwrap().send(text);
    }
}

async fn serve(
    ws: tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>,
    kind: Kind,
    got: Arc<Mutex<Vec<Value>>>,
    mut ctl: mpsc::UnboundedReceiver<String>,
    on_order: Option<OnOrder>,
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
                if let Some(a) = answer {
                    if write.send(Message::text(a.to_string())).await.is_err() {
                        return;
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
fn placed(v: &Value) -> Value {
    json!({"reqId": v["reqId"], "retCode": 0, "retMsg": "OK", "op": v["op"],
           "data": {"orderId": "o-ws-1", "orderLinkId": v["args"][0]["orderLinkId"]},
           "header": {"X-Bapi-Limit": "10"}, "connId": "c"})
}

/// Every valid record in the journal files on disk, read without the lock
/// and without the node: a record being written is not one yet.
fn records(data: &Path) -> Vec<Record> {
    oms_journal::Tail::new(data, 0)
        .peek_beyond(usize::MAX)
        .unwrap_or_default()
        .into_iter()
        .map(|r| serde_json::from_slice(&r.payload).unwrap())
        .collect()
}

/// Whether the journal on disk holds the acceptance of the order whose
/// `orderLinkId` is `link`, for the instance in the data directory.
fn journal_holds(data: &Path, link: &str) -> bool {
    let Some(id) = WireIdForm::Base32Pair.decode(link) else {
        return false;
    };
    let instance = std::fs::read_to_string(data.join("INSTANCE"))
        .ok()
        .and_then(|t| u64::from_str_radix(t.trim(), 16).ok());
    instance == Some(id.instance_id)
        && records(data).iter().any(|r| {
            r.events.iter().any(
                |e| matches!(e, Event::OrderAccepted { order_id, .. } if *order_id == id.order_id),
            )
        })
}

/// The fake Bybit: REST, the three streams, and the tokio runtime they run
/// on.
struct Venue {
    api: Arc<FakeHttp>,
    private: FakeWs,
    trade: FakeWs,
    public: FakeWs,
    _rt: tokio::runtime::Runtime,
}

impl Venue {
    fn start(on_order: OnOrder) -> Venue {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        Venue {
            api: rest_venue(),
            private: FakeWs::start(&rt, Kind::Private, None),
            trade: FakeWs::start(&rt, Kind::Trade, Some(on_order)),
            public: FakeWs::start(&rt, Kind::Public, None),
            _rt: rt,
        }
    }

    fn wiring(&self) -> String {
        json!({
            "rest": self.api.url(),
            "private_ws": self.private.url,
            "trade_ws": self.trade.url,
            "public_ws": self.public.url,
            "ip_echo_url": format!("{}/ip", self.api.url()),
        })
        .to_string()
    }

    fn rest_creates(&self) -> usize {
        self.api
            .requests()
            .iter()
            .filter(|q| q.method == "POST" && q.path() == CREATE_PATH)
            .count()
    }

    fn ws_creates(&self) -> usize {
        self.trade.received("order.create").len()
    }

    fn posts(&self) -> usize {
        self.api
            .requests()
            .iter()
            .filter(|q| q.method == "POST")
            .count()
    }

    /// A snapshot of BTCUSDT on the newest public connection, once one has
    /// subscribed.
    fn ticker_snapshot(&self, mark: &str) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while self.public.received("subscribe").is_empty() {
            assert!(
                Instant::now() < deadline,
                "the node never subscribed the ticker"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        self.public.send(
            json!({"topic": "tickers.BTCUSDT", "type": "snapshot", "cs": 1, "ts": 1_700_000_000_000u64,
                   "data": {"symbol": "BTCUSDT", "markPrice": mark, "lastPrice": mark, "indexPrice": mark}})
            .to_string(),
        );
    }
}

fn base_dir() -> PathBuf {
    std::env::var("OMS_TEST_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_TARGET_TMPDIR")))
}

/// One node process with `kind = "bybit"` and its directory.
struct Node {
    dir: PathBuf,
    child: Option<Child>,
    wiring: String,
}

impl Node {
    fn new(name: &str, venue: &Venue) -> Node {
        let dir = base_dir().join(format!("bybit-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let creds = dir.join("bybit.toml");
        std::fs::write(&creds, "api_key = \"KEY\"\napi_secret = \"SECRET\"\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&creds, std::fs::Permissions::from_mode(0o600)).unwrap();
        let config = format!(
            r#"
data_dir = "{data}"

[runtime]
tick_ms = 20
reconcile_ms = 50
sweep_ms = 60000

[limits]
new_orders_per_sec = 1000
new_orders_burst = 1000
cancels_per_sec = 1000
cancels_burst = 1000

[venue]
kind = "bybit"
network = "testnet"
credentials_file = "{creds}"
sub_account_uid = 4242
ip_echo_url = "https://ip.example/"
recv_window_ms = 1000
request_timeout_ms = 500
ping_interval_ms = 1000
ref_price_interval_ms = 1000

[core]
version = 1

[core.instruments.BTCUSDT]
qty_step = "0.001"
px_tick = "0.1"
min_notional = "0"
max_order_qty = "100"
max_order_notional = "100000000"
account_pos_min = "-1000"
account_pos_max = "1000"
price_band_bps = 500

[core.strategies.a]
enabled = true
max_open_orders = 400
[core.strategies.a.limits.BTCUSDT]
min = "-1000"
max = "1000"

[core.policy]
max_open_orders = 400
max_inflight_sends = 64
absence_margin_us = 300000
cancel_retry_backoff_us = 100000
max_expiry_horizon_us = 60000000
venue_enforces_expiry = true
absence_not_found_needed = 2
"#,
            data = dir.join("data").display(),
            creds = creds.display(),
        );
        std::fs::write(dir.join("oms.toml"), config).unwrap();
        Node {
            dir,
            child: None,
            wiring: venue.wiring(),
        }
    }

    fn data(&self) -> PathBuf {
        self.dir.join("data")
    }

    fn start(&mut self, env: &[(&str, &str)]) {
        let log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.dir.join("node.log"))
            .unwrap();
        let child = Command::new(env!("CARGO_BIN_EXE_oms-node"))
            .arg("run")
            .arg("--config")
            .arg(self.dir.join("oms.toml"))
            .env("OMS_TEST_BYBIT_WIRING", &self.wiring)
            .envs(env.iter().copied())
            .stderr(log)
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        self.child = Some(child);
    }

    /// Waits for the process to exit by itself.
    fn wait_exit(&mut self, timeout: Duration) -> Option<i32> {
        let deadline = Instant::now() + timeout;
        let c = self.child.as_mut()?;
        loop {
            if let Some(st) = c.try_wait().unwrap() {
                self.child = None;
                return st.code();
            }
            if Instant::now() > deadline {
                return None;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn status(&self) -> Option<Value> {
        let mut c = Client::connect(&self.data().join("admin.sock"), None).ok()?;
        c.call(Op::Status).ok()
    }

    fn client(&self) -> Client {
        Client::connect(&self.data().join("oms.sock"), Some("a")).unwrap()
    }

    fn wait(&self, timeout: Duration, mut cond: impl FnMut() -> bool) -> bool {
        let deadline = Instant::now() + timeout;
        loop {
            if cond() {
                return true;
            }
            if Instant::now() > deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(15));
        }
    }

    fn log_tail(&self, lines: usize) -> String {
        let text = std::fs::read_to_string(self.dir.join("node.log")).unwrap_or_default();
        let all: Vec<&str> = text.lines().collect();
        all[all.len().saturating_sub(lines)..].join("\n")
    }

    /// Link up on the private stream, reconciled, nothing in doubt, no hold.
    fn wait_ready(&self) {
        let ok = self.wait(Duration::from_secs(20), || {
            self.status().is_some_and(|s| {
                s["core"]["link"]["up"] == true
                    && s["core"]["holds"].as_array().is_some_and(|h| h.is_empty())
                    && s["core"]["needs"].as_array().is_some_and(|n| n.is_empty())
                    && s["core"]["boot"].as_u64().unwrap_or(0) > 0
            })
        });
        assert!(
            ok,
            "node not ready; status: {}\nlog tail:\n{}",
            self.status()
                .map(|s| s["core"].to_string())
                .unwrap_or_default(),
            self.log_tail(30)
        );
    }

    fn ref_prices(&self) -> usize {
        records(&self.data())
            .iter()
            .flat_map(|r| r.events.iter())
            .filter(|e| e.name() == "ref_price_updated")
            .count()
    }

    fn order(&self, id: OrderId) -> Order {
        self.client()
            .get_order(id)
            .unwrap()
            .expect("the order exists")
    }
}

impl Drop for Node {
    fn drop(&mut self) {
        if let Some(mut c) = self.child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
        if std::env::var("OMS_KEEP").is_err() && !std::thread::panicking() {
            let _ = std::fs::remove_dir_all(&self.dir);
        } else {
            eprintln!("test directory kept: {}", self.dir.display());
        }
    }
}

/// A buy of 0.001 BTCUSDT at 30000, valid for `ttl_ms`. With the receive
/// window of 1 s the gateway sends it while `now + 2 s ≤ expires_at`.
fn order_op(key: &str, ttl_ms: u64) -> Op {
    Op::NewOrder {
        client_order_id: ClientOrderId::new(key).unwrap(),
        instrument: InstrumentId::new("BTCUSDT").unwrap(),
        side: Side::Buy,
        qty: Qty::parse("0.001").unwrap(),
        px: Px::parse("30000").unwrap(),
        tif: Tif::Gtc,
        reduce_only: false,
        expires_at: now_us() + ttl_ms * 1000,
        market_ref: None,
    }
}

/// Test 1. Before any ticker an order with a price band is refused
/// (`RefPriceUnavailable`) and no create leaves. After a ticker snapshot
/// the same request is accepted and goes over WS Trade; when the fake WS
/// Trade reads the `order.create`, the journal files on disk already hold
/// the order's acceptance. One create in total, and the order is working.
#[test]
fn an_order_waits_for_the_reference_price_and_leaves_only_after_its_record() {
    let seen: Arc<Mutex<Vec<(String, bool)>>> = Arc::default();
    let data: Arc<Mutex<Option<PathBuf>>> = Arc::default();
    let on_order: OnOrder = {
        let (seen, data) = (seen.clone(), data.clone());
        Arc::new(move |v: &Value| {
            let link = v["args"][0]["orderLinkId"]
                .as_str()
                .unwrap_or("")
                .to_string();
            let held = data
                .lock()
                .unwrap()
                .as_ref()
                .is_some_and(|d| journal_holds(d, &link));
            seen.lock().unwrap().push((link, held));
            Some(placed(v))
        })
    };
    let venue = Venue::start(on_order);
    let mut node = Node::new("refprice", &venue);
    *data.lock().unwrap() = Some(node.data());
    node.start(&[]);
    node.wait_ready();
    assert!(
        node.log_tail(200)
            .contains("the Bybit gateway uses the test endpoints")
    );

    let op = order_op("k1", 5_000);
    let mut c = node.client();
    let reply = c.new_order(op.clone()).unwrap();
    assert!(
        matches!(&reply, NewOrderReply::Rejected { reasons } if reasons.contains(&RejectReason::RefPriceUnavailable)),
        "{reply:?}"
    );
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(venue.ws_creates(), 0, "a create left before the price");
    assert_eq!(venue.posts(), 0, "a REST request left before the price");

    venue.ticker_snapshot("30000");
    assert!(
        node.wait(Duration::from_secs(5), || node.ref_prices() >= 1),
        "no reference price: {}",
        node.log_tail(20)
    );
    let NewOrderReply::Accepted { order_id } = c.new_order(op).unwrap() else {
        panic!("not accepted after the ticker")
    };
    assert!(
        node.wait(Duration::from_secs(5), || node.order(order_id).phase
            == Phase::Working),
        "{:?}\n{}",
        node.order(order_id),
        node.log_tail(20)
    );
    let seen = seen.lock().unwrap().clone();
    let link = WireIdForm::Base32Pair.encode(VenueClientId {
        instance_id: u64::from_str_radix(
            std::fs::read_to_string(node.data().join("INSTANCE"))
                .unwrap()
                .trim(),
            16,
        )
        .unwrap(),
        order_id,
    });
    assert_eq!(
        seen,
        [(link, true)],
        "the create left before its record was on disk"
    );
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!((venue.ws_creates(), venue.rest_creates()), (1, 0));
}

/// Test 2. The journal flush fails for the new order
/// (`OMS_CRASH_AT=sync_error_after_write`): the node stops with status 2
/// and no create reaches REST or WS Trade. Restarted without the
/// failpoint, it finds the record, asks for the order by its
/// `orderLinkId` (realtime and history miss, twice) and closes it as not
/// placed. Still no create.
#[test]
fn a_failed_flush_sends_nothing_and_the_restart_resolves_the_order_as_not_placed() {
    let venue = Venue::start(Arc::new(|v: &Value| Some(placed(v))));
    let mut node = Node::new("flush", &venue);
    node.start(&[("OMS_CRASH_AT", "sync_error_after_write")]);
    node.wait_ready();
    venue.ticker_snapshot("30000");
    assert!(node.wait(Duration::from_secs(5), || node.ref_prices() >= 1));
    let op = order_op("k1", 3_000);
    let seen = node.client().new_order(op.clone());
    assert!(
        !matches!(seen, Ok(NewOrderReply::Accepted { .. })),
        "accepted without a confirmed flush: {seen:?}"
    );
    assert_eq!(
        node.wait_exit(Duration::from_secs(5)),
        Some(2),
        "{}",
        node.log_tail(10)
    );
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!((venue.ws_creates(), venue.rest_creates()), (0, 0));

    node.start(&[]);
    assert!(node.wait(Duration::from_secs(10), || node.status().is_some()));
    let NewOrderReply::Duplicate { order_id, .. } = node.client().new_order(op).unwrap() else {
        panic!("the record written before the failed flush was not found")
    };
    assert!(
        node.wait(Duration::from_secs(15), || matches!(
            node.order(order_id).close,
            Some(Close {
                reason: CloseReason::NotPlaced { .. },
                ..
            })
        )),
        "{:?}\n{}",
        node.order(order_id),
        node.log_tail(20)
    );
    let link = |q: &http_fake::Request| q.query().contains("orderLinkId=");
    let asked = |path: &str| {
        venue
            .api
            .requests()
            .iter()
            .filter(|q| q.path() == path && link(q))
            .count()
    };
    assert!(
        asked("/v5/order/realtime") >= 2,
        "realtime asked {}",
        asked("/v5/order/realtime")
    );
    assert!(
        asked("/v5/order/history") >= 4,
        "history asked {}",
        asked("/v5/order/history")
    );
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!((venue.ws_creates(), venue.rest_creates()), (0, 0));
    node.wait_ready();
}

/// Test 3. WS Trade reads the create and never answers: the order is in
/// doubt after the request timeout, the realtime query by its
/// `orderLinkId` finds it open, and it is working. One create over both
/// paths.
#[test]
fn an_unanswered_create_is_found_open_by_its_order_link_id_and_never_sent_again() {
    let api: Arc<Mutex<Option<Arc<FakeHttp>>>> = Arc::default();
    let on_order: OnOrder = {
        let api = api.clone();
        Arc::new(move |v: &Value| {
            // The venue has the order from now on, and never says so here.
            if let Some(api) = api.lock().unwrap().as_ref() {
                let link = v["args"][0]["orderLinkId"].as_str().unwrap_or("");
                api.route("/v5/order/realtime", [list(vec![open_order(link)])]);
            }
            None
        })
    };
    let venue = Venue::start(on_order);
    *api.lock().unwrap() = Some(venue.api.clone());
    let mut node = Node::new("silent", &venue);
    node.start(&[]);
    node.wait_ready();
    venue.ticker_snapshot("30000");
    assert!(node.wait(Duration::from_secs(5), || node.ref_prices() >= 1));

    let NewOrderReply::Accepted { order_id } =
        node.client().new_order(order_op("k1", 5_000)).unwrap()
    else {
        panic!("not accepted")
    };
    assert!(node.wait(Duration::from_secs(5), || venue.ws_creates() == 1));
    let sent_at = venue.api.requests().len();
    // In doubt for a moment only: the status query follows within a
    // reconcile round, so the journal says it, not a status poll.
    let in_doubt = || {
        records(&node.data()).iter().flat_map(|r| r.events.clone()).any(|e| {
            matches!(e, Event::OrderInDoubt { order_id: id, reason: DoubtReason::Transport, .. } if id == order_id)
        })
    };
    assert!(
        node.wait(Duration::from_secs(5), in_doubt),
        "never in doubt: {:?}",
        node.order(order_id)
    );
    assert!(
        node.wait(Duration::from_secs(10), || {
            let o = node.order(order_id);
            o.phase == Phase::Working && o.doubt.is_none()
        }),
        "{:?}\n{}",
        node.order(order_id),
        node.log_tail(20)
    );
    let after: Vec<String> = venue.api.requests()[sent_at..]
        .iter()
        .filter(|q| q.path() == "/v5/order/realtime")
        .map(|q| q.query().to_string())
        .collect();
    assert!(
        !after.is_empty() && after.iter().all(|q| q.contains("orderLinkId=")),
        "the order was found by another query: {after:?}"
    );
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!((venue.ws_creates(), venue.rest_creates()), (1, 0));
}
