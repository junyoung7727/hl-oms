//! The stateful fake Bybit account (sans-IO): one order, execution and
//! position book that answers the OMS and the watcher alike, so both read
//! the same venue. Every input takes the time (`now_ms`) as an argument;
//! [`super::venue::FakeBybit`] passes the wall clock, the tests fixed
//! instants.
//!
//! One account (UID [`UID`]), linear USDT, one symbol ([`SYMBOL`], tick
//! 0.1, step 0.001). REST enters through [`Book::http`] (key, signature,
//! faults, the request log, the answer), WS Trade through
//! [`Book::ws_trade`], the test's venue actions through the controls
//! ([`Book::fill`], [`Book::plant_order`], …). An input that changes an
//! order or books an execution returns the private-stream frames Bybit
//! would push; the caller decides whether they are pushed.
//!
//! Values without a source in the repository (the duplicate-create code
//! 110072, the fee rate, the wallet) are judgements; no test asserts them.

use hmac::{Hmac, KeyInit, Mac};
use http_fake::{Answer, Request, bybit};
use serde_json::{Value, json};
use sha2::Sha256;

pub const SYMBOL: &str = "BTCUSDT";
/// The sub-account both keys belong to.
pub const UID: u64 = 4242;
/// The OMS's key and the watcher's key (made-up values).
pub const OKEY: &str = "OKEY";
pub const OSECRET: &str = "OSECRET";
pub const WKEY: &str = "WKEY";
pub const WSECRET: &str = "WSECRET";
/// A refused create whose `orderLinkId` is already at the venue.
pub const DUPLICATE: i64 = 110072;
/// A cancel of an order that is not open.
pub const NOT_OPEN: i64 = 110001;
const WEEK_MS: u64 = 7 * 24 * 3600 * 1000;
/// The paths answered without a key, before any fault.
const PUBLIC: [&str; 3] = ["/ip", "/v5/market/time", "/v5/market/instruments-info"];
/// Fee per unit of value (0.055 %).
const FEE_RATE: f64 = 0.00055;

/// An API key of the account and what `query-api` says about it.
#[derive(Clone, Debug)]
pub struct Key {
    pub api_key: String,
    pub secret: String,
    /// `permissions.ContractTrade`.
    pub contract_trade: Vec<&'static str>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Order {
    pub order_id: String,
    pub link: String,
    pub side: String,
    /// Price in ticks (0.1), quantities in steps (0.001).
    pub price: i64,
    pub qty: i64,
    pub cum: i64,
    pub status: &'static str,
    pub reduce_only: bool,
    pub created_ms: u64,
    pub updated_ms: u64,
    /// The key that created it; `venue` for a planted order.
    pub key: String,
    pub cancel_type: &'static str,
}

impl Order {
    pub fn is_open(&self) -> bool {
        matches!(self.status, "New" | "PartiallyFilled")
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Execution {
    pub exec_id: String,
    pub order_id: String,
    pub link: String,
    pub exec_type: String,
    pub side: String,
    pub qty: i64,
    pub price: i64,
    pub time_ms: u64,
    pub is_maker: bool,
}

/// One line of the request log.
#[derive(Clone, Debug, PartialEq)]
pub struct Logged {
    pub at_ms: u64,
    /// `X-BAPI-API-KEY`, the WS Trade key, or `""`.
    pub key: String,
    /// `GET`, `POST`, or `WS` for WS Trade.
    pub method: String,
    /// The REST path, or the WS Trade `op`.
    pub path: String,
    pub query: String,
    pub body: String,
    /// `ok`, `ret <code>`, `fault`, `http <status>`.
    pub answer: String,
}

#[derive(Default)]
pub struct Book {
    pub keys: Vec<Key>,
    pub orders: Vec<Order>,
    pub executions: Vec<Execution>,
    /// Signed size in steps, average entry price, the latest `execTime`
    /// that changed it.
    pub size: i64,
    pub avg_price: f64,
    pub position_updated_ms: u64,
    /// The mark price in ticks (the ticker's).
    pub mark: i64,
    pub log: Vec<Logged>,
    faults: Vec<(String, String, Answer)>,
    leave_one: Vec<String>,
    next_order: u64,
    next_exec: u64,
    next_frame: u64,
}

/// What a request asks for, read from its query or its JSON body.
struct Params(Vec<(String, String)>);

impl Params {
    fn get(&self, k: &str) -> Option<&str> {
        self.0.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str())
    }
    fn num(&self, k: &str) -> Result<Option<u64>, Answer> {
        self.get(k)
            .map(|v| {
                v.parse()
                    .map_err(|_| bybit::error(10001, &format!("{k} {v}")))
            })
            .transpose()
    }
}

fn query_params(q: &str) -> Params {
    Params(
        q.split('&')
            .filter_map(|kv| kv.split_once('='))
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
    )
}

fn body_params(v: &Value) -> Params {
    let pairs = v.as_object().into_iter().flatten();
    Params(
        pairs
            .map(|(k, v)| (k.clone(), v.as_str().map_or(v.to_string(), str::to_string)))
            .collect(),
    )
}

/// `"0.002"` in steps of `10^-places`; `None` if it has more places.
pub fn units(s: &str, places: u32) -> Option<i64> {
    let (int, frac) = s.split_once('.').unwrap_or((s, ""));
    if frac.len() > places as usize || int.is_empty() {
        return None;
    }
    let frac = format!("{frac:0<width$}", width = places as usize);
    let n: i64 = format!("{int}{frac}").parse().ok()?;
    (n >= 0).then_some(n)
}

fn decimal(n: i64, places: u32) -> String {
    let d = 10i64.pow(places);
    format!("{}.{:0w$}", n / d, n % d, w = places as usize)
}

pub fn qty_text(steps: i64) -> String {
    decimal(steps, 3)
}

pub fn price_text(ticks: i64) -> String {
    decimal(ticks, 1)
}

fn value(qty: i64, price: i64) -> f64 {
    qty as f64 / 1000.0 * price as f64 / 10.0
}

/// `retCode` 0 around `result`, with the envelope's every key.
fn ok(result: Value) -> Answer {
    Answer::Reply(
        200,
        json!({"retCode": 0, "retMsg": "OK", "result": result, "retExtInfo": {}, "time": 1})
            .to_string(),
    )
}

fn hmac_hex(secret: &str, text: &str) -> String {
    let Ok(mut mac) = Hmac::<Sha256>::new_from_slice(secret.as_bytes()) else {
        unreachable!("HMAC takes a key of any length");
    };
    mac.update(text.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

/// The OMS's key (`ContractTrade: [Order, Position]`) and the watcher's
/// (`[Order]`), both on [`UID`].
pub fn two_keys() -> Vec<Key> {
    vec![
        Key {
            api_key: OKEY.into(),
            secret: OSECRET.into(),
            contract_trade: vec!["Order", "Position"],
        },
        Key {
            api_key: WKEY.into(),
            secret: WSECRET.into(),
            contract_trade: vec!["Order"],
        },
    ]
}

impl Book {
    /// An empty account with these keys, flat, mark 30,000.
    pub fn new(keys: Vec<Key>) -> Book {
        Book {
            keys,
            mark: 300_000,
            ..Book::default()
        }
    }

    pub fn order(&self, order_id: &str) -> Option<&Order> {
        self.orders.iter().find(|o| o.order_id == order_id)
    }

    pub fn by_link(&self, link: &str) -> Option<&Order> {
        self.orders.iter().find(|o| o.link == link)
    }

    pub fn open_orders(&self) -> Vec<&Order> {
        self.orders.iter().filter(|o| o.is_open()).collect()
    }

    /// Every create for `link` that reached the venue, over WS Trade and
    /// REST, refused ones included.
    pub fn creates_for(&self, link: &str) -> usize {
        self.log
            .iter()
            .filter(|l| l.path == "order.create" || l.path == "/v5/order/create")
            .filter(|l| {
                serde_json::from_str::<Value>(&l.body)
                    .ok()
                    .as_ref()
                    .and_then(|b| b["orderLinkId"].as_str())
                    == Some(link)
            })
            .count()
    }

    /// The times of the cancel-alls `key` sent.
    pub fn cancel_alls(&self, key: &str) -> Vec<u64> {
        self.log
            .iter()
            .filter(|l| l.key == key && l.path == "/v5/order/cancel-all")
            .map(|l| l.at_ms)
            .collect()
    }

    /// The next `cancel-all` from `key` leaves the newest open order (the
    /// venue's cancel is asynchronous; this stands for the late order).
    pub fn leave_one_once(&mut self, key: &str) {
        self.leave_one.push(key.to_string());
    }

    /// Requests from `key` to `path` get `answer` until [`Book::clear_fault`].
    /// A public path ([`PUBLIC`]) is a test error: its fault never fires.
    pub fn fault(&mut self, key: &str, path: &str, answer: Answer) {
        assert!(
            !PUBLIC.contains(&path),
            "a fault on the public path {path} never fires"
        );
        self.clear_fault(key, path);
        self.faults.push((key.into(), path.into(), answer));
    }

    pub fn clear_fault(&mut self, key: &str, path: &str) {
        self.faults.retain(|(k, p, _)| !(k == key && p == path));
    }

    /// One REST request: the key and its signature, a fault, then the
    /// answer from the book. Returns the frames the request caused.
    pub fn http(&mut self, now_ms: u64, req: &Request) -> (Answer, Vec<String>) {
        let key = req.header("x-bapi-api-key").unwrap_or("").to_string();
        let (answer, frames, faulted) = self.answer(now_ms, &key, req);
        let kind = match &answer {
            Answer::Reply(200, b) => match serde_json::from_str::<Value>(b) {
                Ok(v) if v["retCode"] != json!(0) && v.get("retCode").is_some() => {
                    format!("ret {}", v["retCode"])
                }
                _ => "ok".into(),
            },
            Answer::Reply(s, _) => format!("http {s}"),
            _ => "fault".into(),
        };
        self.log.push(Logged {
            at_ms: now_ms,
            key,
            method: req.method.clone(),
            path: req.path().into(),
            query: req.query().into(),
            body: req.body_str(),
            answer: if faulted { "fault".into() } else { kind },
        });
        (answer, frames)
    }

    fn answer(&mut self, now_ms: u64, key: &str, req: &Request) -> (Answer, Vec<String>, bool) {
        let path = req.path();
        let none = |a: Answer| (a, Vec::new(), false);
        match path {
            "/ip" => return none(Answer::Reply(200, "127.0.0.1\n".into())),
            "/v5/market/time" => {
                return none(ok(bybit::market_time(now_ms * 1_000_000)));
            }
            "/v5/market/instruments-info" => return none(self.instruments(req.query())),
            _ => {}
        }
        let Some(k) = self.keys.iter().find(|k| k.api_key == key) else {
            return none(bybit::error(10003, "API key is invalid."));
        };
        let payload = if req.method == "GET" {
            req.query().to_string()
        } else {
            req.body_str()
        };
        let signed = match (
            req.header("x-bapi-timestamp"),
            req.header("x-bapi-recv-window"),
        ) {
            (Some(ts), Some(rw)) => hmac_hex(&k.secret, &format!("{ts}{key}{rw}{payload}")),
            _ => String::new(),
        };
        if signed.is_empty() || req.header("x-bapi-sign") != Some(signed.as_str()) {
            return none(bybit::error(10004, "error sign!"));
        }
        if let Some((_, _, a)) = self.faults.iter().find(|(fk, p, _)| fk == key && p == path) {
            return (a.clone(), Vec::new(), true);
        }
        let (get, post) = (req.method == "GET", req.method == "POST");
        let q = query_params(req.query());
        let body = serde_json::from_slice::<Value>(&req.body).unwrap_or(Value::Null);
        let result = match path {
            "/v5/order/realtime" if get => self.order_list(now_ms, &q, true),
            "/v5/order/history" if get => self.order_list(now_ms, &q, false),
            "/v5/execution/list" if get => self.execution_list(now_ms, &q),
            "/v5/position/list" if get => self.position_list(&q),
            "/v5/account/wallet-balance" if get => match q.get("accountType") {
                Some("UNIFIED") => Ok(json!({"list": [wallet()]})),
                _ => Err(bybit::error(10001, "accountType")),
            },
            "/v5/account/info" if get => Ok(account_info()),
            "/v5/user/query-api" if get => Ok(query_api(k)),
            "/v5/order/create" if post => {
                let (r, frames) = self.create(now_ms, key, &body);
                return (
                    r.map_or_else(|(c, m)| bybit::error(c, &m), ok),
                    frames,
                    false,
                );
            }
            "/v5/order/cancel" if post => {
                let (r, frames) = self.cancel(now_ms, &body_params(&body));
                return (
                    r.map_or_else(|(c, m)| bybit::error(c, &m), ok),
                    frames,
                    false,
                );
            }
            "/v5/order/cancel-all" if post => {
                if body != json!({"category": "linear", "settleCoin": "USDT"}) {
                    Err(bybit::error(10001, "cancel-all body"))
                } else {
                    let (r, frames) = self.cancel_all(now_ms, key);
                    return (ok(r), frames, false);
                }
            }
            _ => Err(Answer::Reply(404, "{}".into())),
        };
        none(result.map_or_else(|e| e, ok))
    }

    fn instruments(&self, query: &str) -> Answer {
        let q = query_params(query);
        if q.get("category") != Some("linear") {
            return bybit::error(10001, "category");
        }
        let rows = match q.get("symbol") {
            None | Some(SYMBOL) => vec![instrument()],
            Some(_) => vec![],
        };
        ok(bybit::page(rows, ""))
    }

    /// Open orders (`realtime`) or closed ones (`history`), newest first,
    /// filtered as the venue documents; pages of `limit` (at most 50) with
    /// the cursor `p<offset>`.
    fn order_list(&self, now_ms: u64, q: &Params, open: bool) -> Result<Value, Answer> {
        if q.get("category") != Some("linear") {
            return Err(bybit::error(10001, "category"));
        }
        if [
            q.get("symbol"),
            q.get("settleCoin"),
            q.get("orderLinkId"),
            q.get("orderId"),
        ]
        .iter()
        .all(Option::is_none)
        {
            return Err(bybit::error(
                10001,
                "symbol, settleCoin or an order id is needed",
            ));
        }
        let window = if open { None } else { Some(window(now_ms, q)?) };
        let rows: Vec<&Order> = self
            .orders
            .iter()
            .rev()
            .filter(|o| o.is_open() == open)
            .filter(|o| q.get("orderLinkId").is_none_or(|l| o.link == l))
            .filter(|o| q.get("orderId").is_none_or(|i| o.order_id == i))
            .filter(|_| scoped(q))
            .filter(|o| window.is_none_or(|(a, b)| (a..=b).contains(&o.created_ms)))
            .collect();
        let rows: Vec<Value> = rows.into_iter().map(|o| self.order_row(o)).collect();
        page(rows, q, 20, 50, 'p')
    }

    fn execution_list(&self, now_ms: u64, q: &Params) -> Result<Value, Answer> {
        if q.get("category") != Some("linear") {
            return Err(bybit::error(10001, "category"));
        }
        let (from, to) = window(now_ms, q)?;
        // Newest booked first, then a stable sort: equal `execTime`s stay
        // newest first.
        let mut rows: Vec<&Execution> = self
            .executions
            .iter()
            .rev()
            .filter(|e| (from..=to).contains(&e.time_ms))
            .filter(|e| q.get("orderLinkId").is_none_or(|l| e.link == l))
            .filter(|e| q.get("orderId").is_none_or(|i| e.order_id == i))
            .filter(|_| scoped(q))
            .collect();
        rows.sort_by_key(|e| std::cmp::Reverse(e.time_ms));
        page(
            rows.into_iter().map(execution_row).collect(),
            q,
            50,
            100,
            'e',
        )
    }

    /// With `symbol`: its row, flat or not. With `settleCoin` only: the
    /// rows above zero (gateway facts Q12.1).
    fn position_list(&self, q: &Params) -> Result<Value, Answer> {
        if q.get("category") != Some("linear") {
            return Err(bybit::error(10001, "category"));
        }
        let rows = match (q.get("symbol"), q.get("settleCoin")) {
            (Some(SYMBOL), _) => vec![self.position_row()],
            (Some(_), _) => vec![],
            (None, Some("USDT")) if self.size != 0 => vec![self.position_row()],
            (None, Some(_)) => vec![],
            (None, None) => return Err(bybit::error(10001, "symbol or settleCoin is needed")),
        };
        page(rows, q, 20, 200, 'q')
    }

    /// An order request (WS Trade `args[0]` or the REST body).
    fn create(
        &mut self,
        now_ms: u64,
        key: &str,
        body: &Value,
    ) -> (Result<Value, (i64, String)>, Vec<String>) {
        let p = body_params(body);
        let link = p.get("orderLinkId").unwrap_or("").to_string();
        let qty = p.get("qty").and_then(|s| units(s, 3)).filter(|q| *q > 0);
        let price = p.get("price").and_then(|s| units(s, 1)).filter(|p| *p > 0);
        let side = p.get("side").filter(|s| matches!(*s, "Buy" | "Sell"));
        let (Some(qty), Some(price), Some(side)) = (qty, price, side) else {
            return (Err((10001, "side, qty or price".into())), vec![]);
        };
        if p.get("category") != Some("linear") || p.get("symbol") != Some(SYMBOL) {
            return (Err((10001, "category or symbol".into())), vec![]);
        }
        if link.is_empty() {
            return (Err((10001, "orderLinkId".into())), vec![]);
        }
        if self.by_link(&link).is_some() {
            return (
                Err((DUPLICATE, "OrderLinkedID is duplicate".into())),
                vec![],
            );
        }
        let id = self.book_order(now_ms, key, &link, side, qty, price);
        if let Some(o) = self.orders.last_mut() {
            o.reduce_only = p.get("reduceOnly") == Some("true");
        }
        let frame = self.order_frame(now_ms, &id);
        (Ok(json!({"orderId": id, "orderLinkId": link})), vec![frame])
    }

    fn book_order(
        &mut self,
        now_ms: u64,
        key: &str,
        link: &str,
        side: &str,
        qty: i64,
        price: i64,
    ) -> String {
        self.next_order += 1;
        let id = format!("o-{}", self.next_order);
        self.orders.push(Order {
            order_id: id.clone(),
            link: link.into(),
            side: side.into(),
            price,
            qty,
            cum: 0,
            status: "New",
            reduce_only: false,
            created_ms: now_ms,
            updated_ms: now_ms,
            key: key.into(),
            cancel_type: "UNKNOWN",
        });
        id
    }

    /// A cancel by `orderLinkId` or `orderId`.
    fn cancel(&mut self, now_ms: u64, p: &Params) -> (Result<Value, (i64, String)>, Vec<String>) {
        let found = self.orders.iter().position(|o| {
            o.is_open()
                && (p.get("orderLinkId").is_some_and(|l| o.link == l)
                    || p.get("orderId").is_some_and(|i| o.order_id == i))
        });
        let Some(i) = found else {
            return (
                Err((NOT_OPEN, "order not exists or too late to cancel".into())),
                vec![],
            );
        };
        let frames = self.close(now_ms, i, "CancelByUser");
        let o = &self.orders[i];
        (
            Ok(json!({"orderId": o.order_id, "orderLinkId": o.link})),
            frames,
        )
    }

    fn close(&mut self, now_ms: u64, i: usize, cancel_type: &'static str) -> Vec<String> {
        let o = &mut self.orders[i];
        o.status = "Cancelled";
        o.cancel_type = cancel_type;
        o.updated_ms = now_ms;
        let id = o.order_id.clone();
        vec![self.order_frame(now_ms, &id)]
    }

    /// Every open order of the account, whoever created it (the kill
    /// scope); after [`Book::leave_one_once`] for `key`, its first
    /// cancel-all leaves the newest.
    fn cancel_all(&mut self, now_ms: u64, key: &str) -> (Value, Vec<String>) {
        let mut open: Vec<usize> = (0..self.orders.len())
            .filter(|i| self.orders[*i].is_open())
            .collect();
        if let Some(at) = self.leave_one.iter().position(|k| k == key) {
            self.leave_one.remove(at);
            open.pop();
        }
        let mut frames = Vec::new();
        let mut done = Vec::new();
        for i in open {
            frames.extend(self.close(now_ms, i, "CancelAll"));
            done.push((self.orders[i].order_id.clone(), self.orders[i].link.clone()));
        }
        (bybit::cancel_all(&done), frames)
    }

    /// One WS Trade request from the connection authenticated with `key`:
    /// the reply (in the shape of `bybit_fake::ws::placed`) and the frames.
    pub fn ws_trade(
        &mut self,
        now_ms: u64,
        key: &str,
        msg: &Value,
    ) -> (Option<Value>, Vec<String>) {
        let op = msg["op"].as_str().unwrap_or("").to_string();
        let args = msg["args"][0].clone();
        let (r, frames) = match op.as_str() {
            "order.create" => self.create(now_ms, key, &args),
            "order.cancel" => self.cancel(now_ms, &body_params(&args)),
            _ => return (None, vec![]),
        };
        self.log.push(Logged {
            at_ms: now_ms,
            key: key.into(),
            method: "WS".into(),
            path: op.clone(),
            query: String::new(),
            body: args.to_string(),
            answer: r
                .as_ref()
                .map_or_else(|(c, _)| format!("ret {c}"), |_| "ok".into()),
        });
        let (code, text, data) = match r {
            Ok(d) => (0, "OK".to_string(), d),
            Err((c, m)) => (c, m, json!({})),
        };
        let reply = json!({"reqId": msg["reqId"], "retCode": code, "retMsg": text, "op": op,
            "data": data, "header": {"X-Bapi-Limit": "10"}, "connId": "c"});
        (Some(reply), frames)
    }

    /// A fill of `qty` at `price` (decimal texts) of an open order: a
    /// `Trade` execution, the order and the position updated; the
    /// `execution` and `order` frames.
    pub fn fill(&mut self, now_ms: u64, order_id: &str, qty: &str, price: &str) -> Vec<String> {
        let (q, px) = (units(qty, 3).unwrap(), units(price, 1).unwrap());
        let i = self
            .orders
            .iter()
            .position(|o| o.order_id == order_id && o.is_open())
            .unwrap();
        let o = &mut self.orders[i];
        assert!(
            q > 0 && o.cum + q <= o.qty,
            "fill {qty} of {order_id}: more than is left"
        );
        o.cum += q;
        o.status = if o.cum == o.qty {
            "Filled"
        } else {
            "PartiallyFilled"
        };
        o.updated_ms = now_ms;
        let (link, side) = (o.link.clone(), o.side.clone());
        let e = self.book_execution("Trade", &side, q, px, &link, order_id, now_ms);
        vec![
            self.execution_frame(now_ms, &e),
            self.order_frame(now_ms, order_id),
        ]
    }

    /// A venue-only order (made on the web page): in the book and on REST,
    /// never pushed. Returns its `orderId`.
    pub fn plant_order(
        &mut self,
        now_ms: u64,
        link: &str,
        side: &str,
        qty: &str,
        price: &str,
    ) -> String {
        self.book_order(
            now_ms,
            "venue",
            link,
            side,
            units(qty, 3).unwrap(),
            units(price, 1).unwrap(),
        )
    }

    /// A venue-only execution at `exec_time_ms`, in the book and on REST,
    /// never pushed; a position-changing type moves the position. Returns
    /// its `execId`.
    #[allow(clippy::too_many_arguments)]
    pub fn plant_execution(
        &mut self,
        exec_type: &str,
        side: &str,
        qty: &str,
        price: &str,
        link: &str,
        exec_time_ms: u64,
    ) -> String {
        self.next_order += 1;
        let order_id = format!("o-{}", self.next_order);
        let (q, px) = (units(qty, 3).unwrap(), units(price, 1).unwrap());
        let e = self.book_execution(exec_type, side, q, px, link, &order_id, exec_time_ms);
        self.executions[e].exec_id.clone()
    }

    /// An operator's cancel on the venue's web page: the `order` frame,
    /// which the caller pushes or not.
    pub fn cancel_at_venue(&mut self, now_ms: u64, order_id: &str) -> Vec<String> {
        let i = self
            .orders
            .iter()
            .position(|o| o.order_id == order_id && o.is_open())
            .unwrap();
        self.close(now_ms, i, "CancelByUser")
    }

    #[allow(clippy::too_many_arguments)]
    fn book_execution(
        &mut self,
        exec_type: &str,
        side: &str,
        qty: i64,
        price: i64,
        link: &str,
        order_id: &str,
        time_ms: u64,
    ) -> usize {
        self.next_exec += 1;
        self.executions.push(Execution {
            exec_id: format!("e-{}", self.next_exec),
            order_id: order_id.into(),
            link: link.into(),
            exec_type: exec_type.into(),
            side: side.into(),
            qty,
            price,
            time_ms,
            is_maker: true,
        });
        if matches!(
            exec_type,
            "Trade" | "AdlTrade" | "BustTrade" | "Delivery" | "Settle"
        ) {
            let signed = if side == "Buy" { qty } else { -qty };
            let next = self.size + signed;
            let px = price as f64 / 10.0;
            if self.size == 0 || self.size.signum() == signed.signum() {
                // Opening or adding: the average moves.
                let (old, new) = (self.size.abs() as f64, qty as f64);
                self.avg_price = (self.avg_price * old + px * new) / (old + new);
            } else if next != 0 && next.signum() != self.size.signum() {
                // Through zero: the rest opens at this price.
                self.avg_price = px;
            }
            if next == 0 {
                self.avg_price = 0.0;
            }
            self.size = next;
            // A planted row older than the last change does not date it back.
            self.position_updated_ms = self.position_updated_ms.max(time_ms);
        }
        self.executions.len() - 1
    }

    fn frame(&mut self, now_ms: u64, topic: &str, row: Value) -> String {
        self.next_frame += 1;
        json!({"id": format!("f-{}", self.next_frame), "topic": topic,
            "creationTime": now_ms, "data": [row]})
        .to_string()
    }

    fn order_frame(&mut self, now_ms: u64, order_id: &str) -> String {
        let row = self.order_row(self.order(order_id).unwrap());
        self.frame(now_ms, "order.linear", row)
    }

    fn execution_frame(&mut self, now_ms: u64, e: &usize) -> String {
        let row = execution_row(&self.executions[*e]);
        self.frame(now_ms, "execution.linear", row)
    }

    /// The public ticker snapshot at `now_ms`.
    pub fn ticker(&self, now_ms: u64) -> String {
        let mark = price_text(self.mark);
        json!({"topic": format!("tickers.{SYMBOL}"), "type": "snapshot", "ts": now_ms, "cs": 1,
            "data": {"symbol": SYMBOL, "markPrice": mark, "lastPrice": mark, "indexPrice": mark}})
        .to_string()
    }

    pub fn order_row(&self, o: &Order) -> Value {
        let avg = self
            .executions
            .iter()
            .filter(|e| e.order_id == o.order_id)
            .fold((0.0, 0i64), |(v, q), e| {
                (v + value(e.qty, e.price), q + e.qty)
            });
        let avg = if avg.1 == 0 {
            String::new()
        } else {
            format!("{:.1}", avg.0 / (avg.1 as f64 / 1000.0))
        };
        let leaves = if o.is_open() { o.qty - o.cum } else { 0 };
        let exec_value = value(o.cum, o.price);
        bybit::with_fields(
            bybit::order_row(
                &o.link,
                &o.order_id,
                SYMBOL,
                &o.side,
                &qty_text(o.qty),
                o.status,
            ),
            json!({"price": price_text(o.price), "cumExecQty": qty_text(o.cum),
                "leavesQty": qty_text(leaves), "leavesValue": format!("{:.4}", value(leaves, o.price)),
                "cumExecValue": format!("{exec_value:.4}"), "cumExecFee": format!("{:.8}", exec_value * FEE_RATE),
                "avgPrice": avg, "cancelType": o.cancel_type, "rejectReason": "EC_NoError",
                "orderType": "Limit", "timeInForce": "PostOnly", "reduceOnly": o.reduce_only,
                "positionIdx": 0, "createdTime": o.created_ms.to_string(),
                "updatedTime": o.updated_ms.to_string(), "blockTradeId": "", "closeOnTrigger": false,
                "createType": "CreateByUser", "cumFeeDetail": {}, "isLeverage": "",
                "lastPriceOnCreated": price_text(self.mark), "marketUnit": "", "orderIv": "",
                "parentOrderLinkId": "", "placeType": "", "slLimitPrice": "0", "slTriggerBy": "",
                "smpGroup": 0, "smpOrderId": "", "smpType": "None", "stopLoss": "",
                "stopOrderType": "", "takeProfit": "", "tpLimitPrice": "0", "tpTriggerBy": "",
                "tpslMode": "", "triggerBy": "", "triggerDirection": 0, "triggerPrice": ""}),
        )
    }

    pub fn position_row(&self) -> Value {
        let side = match self.size.signum() {
            1 => "Buy",
            -1 => "Sell",
            _ => "",
        };
        let size = self.size.abs();
        let mark = self.mark as f64 / 10.0;
        let (avg, pnl) = if size == 0 {
            (String::new(), 0.0)
        } else {
            let pnl = (mark - self.avg_price) * self.size as f64 / 1000.0;
            (format!("{:.1}", self.avg_price), pnl)
        };
        bybit::with_fields(
            bybit::position_row(SYMBOL, side, &qty_text(size)),
            json!({"avgPrice": avg, "markPrice": price_text(self.mark),
                "positionValue": format!("{:.4}", value(size, self.mark)),
                "unrealisedPnl": format!("{pnl:.4}"), "liqPrice": "", "bustPrice": "",
                "leverage": "1", "updatedTime": self.position_updated_ms.to_string(),
                "createdTime": "1", "openTime": 0, "adlRankIndicator": 0, "autoAddMargin": 0,
                "breakEvenPrice": "", "cumRealisedPnl": "0", "curRealisedPnl": "0",
                "isReduceOnly": false, "leverageSysUpdatedTime": "", "mmrSysUpdatedTime": "",
                "netDeltaRatio": "", "positionBalance": "0", "positionIM": "0", "positionIMByMp": "0",
                "positionMM": "0", "positionMMByMp": "0", "positionStatus": "Normal",
                "riskId": 1, "riskLimitValue": "300000", "seq": 1, "sessionAvgPrice": "",
                "stopLoss": "", "takeProfit": "", "tpslMode": "Full", "tradeMode": 0,
                "trailingStop": "0"}),
        )
    }
}

/// Whether the `symbol` and `settleCoin` filters admit the account's one
/// symbol.
fn scoped(q: &Params) -> bool {
    q.get("symbol").is_none_or(|s| s == SYMBOL) && q.get("settleCoin").is_none_or(|s| s == "USDT")
}

/// `startTime`/`endTime` (ms, both inclusive): at most 7 days; one alone
/// spans 7 days from it; none, the last 7 days.
fn window(now_ms: u64, q: &Params) -> Result<(u64, u64), Answer> {
    let w = match (q.num("startTime")?, q.num("endTime")?) {
        (Some(a), Some(b)) => (a, b),
        (Some(a), None) => (a, a + WEEK_MS),
        (None, Some(b)) => (b.saturating_sub(WEEK_MS), b),
        (None, None) => (now_ms.saturating_sub(WEEK_MS), now_ms),
    };
    if w.1 < w.0 || w.1 - w.0 > WEEK_MS {
        return Err(bybit::error(10001, "the time window is longer than 7 days"));
    }
    Ok(w)
}

/// One page of `rows`: `limit` (default `default`, at most `max`) from
/// the offset in the cursor `<letter><offset>`. A full page always has a
/// cursor (the next page may be empty).
fn page(
    rows: Vec<Value>,
    q: &Params,
    default: usize,
    max: usize,
    letter: char,
) -> Result<Value, Answer> {
    let limit = q.num("limit")?.map_or(default, |l| l as usize);
    if limit == 0 || limit > max {
        return Err(bybit::error(10001, "limit"));
    }
    let from = match q.get("cursor") {
        None => 0,
        Some(c) => c
            .strip_prefix(letter)
            .and_then(|n| n.parse::<usize>().ok())
            .ok_or_else(|| bybit::error(10001, "cursor"))?,
    };
    let rows: Vec<Value> = rows.into_iter().skip(from).take(limit).collect();
    let next = if rows.len() == limit {
        format!("{letter}{}", from + limit)
    } else {
        String::new()
    };
    Ok(bybit::page(rows, &next))
}

pub fn execution_row(e: &Execution) -> Value {
    let v = value(e.qty, e.price);
    let fee = format!("{:.8}", v * FEE_RATE);
    bybit::with_fields(
        bybit::execution_row(&bybit::Execution {
            symbol: SYMBOL,
            exec_id: &e.exec_id,
            exec_type: &e.exec_type,
            side: &e.side,
            qty: &qty_text(e.qty),
            price: &price_text(e.price),
            fee: &fee,
            exec_time_ms: e.time_ms,
            order_link_id: &e.link,
            order_id: &e.order_id,
        }),
        json!({"isMaker": e.is_maker, "execValue": format!("{v:.4}"), "execFeeV2": fee,
            "feeRate": FEE_RATE.to_string(), "feeCurrency": "USDT", "closedSize": "0",
            "leavesQty": "0", "orderPrice": price_text(e.price), "orderQty": qty_text(e.qty),
            "orderType": "Limit", "stopOrderType": "UNKNOWN", "markPrice": price_text(e.price),
            "indexPrice": "", "markIv": "", "tradeIv": "", "underlyingPrice": "",
            "blockTradeId": "", "createType": "CreateByUser", "extraFees": "", "marketUnit": "",
            "seq": 1}),
    )
}

fn instrument() -> Value {
    bybit::with_fields(
        bybit::instrument_row(SYMBOL, "0.10", "0.001"),
        json!({"baseCoin": "BTC", "quoteCoin": "USDT", "contractType": "LinearPerpetual",
            "copyTrading": "both", "deliveryFeeRate": "", "deliveryTime": "0",
            "displayName": SYMBOL, "forbidUplWithdrawal": false, "fullName": "Bitcoin",
            "fundingInterval": 480, "isPreListing": false, "launchTime": "1585526400000",
            "leverageFilter": {"leverageStep": "0.01", "maxLeverage": "100.00", "minLeverage": "1"},
            "lotSizeFilter": {"maxMktOrderQty": "500.000", "maxOrderQty": "1190.000",
                "minNotionalValue": "5", "minOrderQty": "0.001",
                "postOnlyMaxOrderQty": "1190.000", "qtyStep": "0.001"},
            "lowerFundingRate": "-0.003", "upperFundingRate": "0.003", "marketRegion": "",
            "preListingInfo": null, "priceScale": "2",
            "priceFilter": {"maxPrice": "1999999.80", "minPrice": "0.10", "tickSize": "0.10"},
            "riskParameters": {"priceLimitRatioX": "0.01", "priceLimitRatioY": "0.02"},
            "symbolId": 1, "symbolType": "", "tags": [], "underlyingTicker": "",
            "unifiedMarginTrade": true}),
    )
}

/// A healthy wallet (equity 10,000, maintenance 10): W3 never fires.
fn wallet() -> Value {
    let coin = json!({"coin": "USDT", "equity": "10000", "walletBalance": "10000",
        "usdValue": "10000", "unrealisedPnl": "0", "cumRealisedPnl": "0", "locked": "0",
        "accruedInterest": "0", "availableToBorrow": "", "availableToWithdraw": "", "bonus": "0",
        "borrowAmount": "0", "colRes": "-1", "collateralSwitch": true, "marginCollateral": true,
        "spotBorrow": "0", "spotHedgingQty": "0", "totalOrderIM": "0", "totalPositionIM": "0",
        "totalPositionMM": "10"});
    bybit::with_fields(
        bybit::wallet_unified("10000", "10", "10000"),
        json!({"coin": [coin], "totalAvailableBalance": "10000", "totalInitialMargin": "0",
            "totalInitialMarginByMp": "0", "totalMaintenanceMarginByMp": "10", "totalPerpUPL": "0",
            "accountIMRate": "0", "accountIMRateByMp": "0", "accountLTV": "0",
            "accountMMRate": "0.001", "accountMMRateByMp": "0.001"}),
    )
}

fn account_info() -> Value {
    json!({"marginMode": "REGULAR_MARGIN", "unifiedMarginStatus": 5, "updatedTime": "1",
        "dcpStatus": "OFF", "isMasterTrader": false, "smpGroup": 0, "spotHedgingStatus": "OFF",
        "timeWindow": 0})
}

/// `query-api` of `k`: a sub-account key on [`UID`], IP-bound to
/// 127.0.0.1, no expiry (the epoch, as the venue writes it for an
/// IP-bound key).
fn query_api(k: &Key) -> Value {
    let mut perms = json!({"Affiliate": [], "BitCard": [], "BlockTrade": [], "ByXPost": [],
        "Compliance": [], "CopyTrading": [], "Derivatives": [], "Earn": [], "Exchange": [],
        "FiatBitPay": [], "FiatConvertBroker": [], "FiatP2P": [], "NFT": [], "Options": [],
        "Spot": [], "Stocks": [], "Wallet": []});
    perms["ContractTrade"] = json!(k.contract_trade);
    bybit::with_fields(
        bybit::query_api_key(UID, "1000", 0, perms, &["127.0.0.1"]),
        json!({"apiKey": k.api_key, "id": "1", "note": "fake", "secret": "", "type": 1,
            "createdAt": "2026-01-01T00:00:00Z", "expiredAt": "1970-01-01T00:00:00Z",
            "deadlineDay": -1, "affiliateID": 0, "affiliateIDInt64": "0", "inviterID": 0,
            "inviterIDInt64": "0", "isFixApi": false, "kycLevel": "LEVEL_DEFAULT",
            "kycRegion": "", "mktMakerLevel": "0", "rsaPublicKey": "", "unified": 0,
            "userIDInt64": UID.to_string(), "uta": 1, "vipLevel": "No VIP"}),
    )
}
