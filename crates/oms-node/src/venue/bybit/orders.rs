//! Orders and cancels (ADR-028 §7d–§7e, U17a–U17b): the request, the
//! send-time rule (G-4), the rate budget, the path (WS Trade first, REST
//! when WS Trade wrote nothing) and the answer class (`answer`). Nothing
//! here sends a request twice (G-3).

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use oms_core::*;
use serde_json::{Value, json};

use super::answer::{self, Answer};
use super::rest::{AUTH_CODES, Rest, RestError};
use super::trade::{self, Reply, Trade};

pub const CREATE_PATH: &str = "/v5/order/create";
pub const CANCEL_PATH: &str = "/v5/order/cancel";

/// What an answer says about the key or the IP, for the gateway to act on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Effect {
    None,
    /// HTTP 403 or retCode 10018: pause every request (`IP_BAN_PAUSE`).
    Ban(String),
    /// HTTP 401 or an auth retCode: no request until the restart.
    RejectKey(String),
}

pub fn tif(t: Tif) -> &'static str {
    match t {
        Tif::Gtc => "GTC",
        Tif::Ioc => "IOC",
        Tif::Alo => "PostOnly",
    }
}

/// The create body: a limit order, one-way mode (`positionIdx` 0).
pub fn create_body(
    symbol: &str,
    side: Side,
    qty: Qty,
    px: Px,
    t: Tif,
    reduce_only: bool,
    link: &str,
) -> Value {
    json!({
        "category": "linear",
        "symbol": symbol,
        "side": match side { Side::Buy => "Buy", Side::Sell => "Sell" },
        "orderType": "Limit",
        "qty": qty.to_string(),
        "price": px.to_string(),
        "timeInForce": tif(t),
        "orderLinkId": link,
        "reduceOnly": reduce_only,
        "positionIdx": 0,
    })
}

pub fn cancel_body(symbol: &str, link: &str) -> Value {
    json!({"category": "linear", "symbol": symbol, "orderLinkId": link})
}

/// G-4 for Bybit (C-GW): the venue refuses a request whose time plus the
/// receive window is before its own clock, and accepts our clock up to 1 s
/// ahead. So a request sent at `ts_ms` is processed by `expires_at` only if
/// `ts + recv_window + 1 s ≤ expires_at`.
pub fn fits_expiry(ts_ms: u64, recv_window_ms: u64, expires_at: Ts) -> bool {
    last_taken_ms(ts_ms, recv_window_ms).saturating_mul(1_000) <= expires_at
}

/// The latest local time (ms) the venue can still take a request stamped
/// `ts_ms`, under the same rule as `fits_expiry`.
pub fn last_taken_ms(ts_ms: u64, recv_window_ms: u64) -> u64 {
    ts_ms.saturating_add(recv_window_ms).saturating_add(1_000)
}

/// What the gateway learns from an HTTP answer besides its class.
pub fn effect(status: u16, a: &Answer) -> Effect {
    match (status, a) {
        (403, _) => Effect::Ban(format!("HTTP 403 on the order path: {a:?}")),
        // "Exceeded the IP Rate Limit": the retCode form of the IP limit.
        (_, Answer::Rejected { code: 10018, msg }) => {
            Effect::Ban(format!("IP rate limit (10018) on the order path: {msg}"))
        }
        (401, _) => Effect::RejectKey("HTTP 401 on the order path".into()),
        (_, Answer::Rejected { code, msg }) if AUTH_CODES.contains(code) => {
            Effect::RejectKey(format!("key rejected ({code}): {msg}"))
        }
        _ => Effect::None,
    }
}

fn reason(code: i64, msg: &str) -> String {
    format!("bybit {code}: {msg}")
}

/// The answer to a create, as the core reads it. The ack only says the
/// request was accepted (the venue processes it asynchronously, F18 §3.1):
/// the order is `Open` with no cumulative quantity until the stream or a
/// query says more. An accepted answer without a usable `orderId` is
/// `Unknown`: the order may exist, but cannot be named.
pub fn send_outcome(a: Answer, venue_ts: Ts) -> SendOutcome {
    match a {
        Answer::Accepted(result) => {
            let oid = result.get("orderId").and_then(Value::as_str).unwrap_or("");
            match VenueOrderId::new(oid) {
                Ok(venue_oid) => SendOutcome::Placed {
                    venue_oid,
                    status: StatusIn {
                        status: VenueStatus::Open,
                        cum: None,
                        venue_ts,
                    },
                },
                Err(_) => SendOutcome::Unknown {
                    reason: format!("accepted without a usable orderId: {result}"),
                },
            }
        }
        Answer::Rejected { code, msg } => SendOutcome::Rejected {
            reason: reason(code, &msg),
        },
        Answer::Unknown(e) => SendOutcome::Unknown { reason: e },
    }
}

pub fn cancel_outcome(a: Answer) -> CancelOutcome {
    match a {
        Answer::Accepted(_) => CancelOutcome::Confirmed,
        Answer::Rejected { code, msg } => CancelOutcome::Rejected {
            reason: reason(code, &msg),
        },
        Answer::Unknown(e) => CancelOutcome::Unknown { reason: e },
    }
}

/// The order path with its rate budget: WS Trade when it is up, REST
/// otherwise.
pub struct Orders {
    rest: Rest,
    trade: Option<Arc<Trade>>,
    budget: Mutex<RateBudget>,
    /// The budget's clock: µs since this instant (monotonic).
    epoch: Instant,
}

/// A create to send.
pub struct Create<'a> {
    pub symbol: &'a str,
    pub side: Side,
    pub qty: Qty,
    pub px: Px,
    pub tif: Tif,
    pub reduce_only: bool,
    pub link: &'a str,
    pub expires_at: Ts,
}

impl Orders {
    pub fn new(rest: Rest) -> Orders {
        Orders {
            rest,
            trade: None,
            budget: Mutex::new(RateBudget::bybit()),
            epoch: Instant::now(),
        }
    }

    /// The same path with WS Trade first.
    pub fn with_trade(mut self, trade: Arc<Trade>) -> Orders {
        self.trade = Some(trade);
        self
    }

    fn mono_us(&self) -> u64 {
        self.epoch.elapsed().as_micros() as u64
    }

    fn budget(&self) -> std::sync::MutexGuard<'_, RateBudget> {
        self.budget.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Waits until the budget lets one request of `class` out, and takes
    /// the slot. Nothing is dropped; the wait has no limit (an order that
    /// waited too long fails the send-time rule after it).
    async fn take(&self, class: RateClass) -> Held<'_> {
        loop {
            let now = self.mono_us();
            let r = self.budget().take(class, now);
            match r {
                Ok(slot) => {
                    return Held {
                        orders: self,
                        slot: Some(slot),
                    };
                }
                Err(at) => tokio::time::sleep(Duration::from_micros(at.saturating_sub(now))).await,
            }
        }
    }

    /// Sends one create, after the budget. `allowed` is asked after the
    /// wait: a block that appeared meanwhile keeps the request home.
    pub async fn create(
        &self,
        c: &Create<'_>,
        allowed: impl Fn() -> Result<(), String> + Sync,
    ) -> (SendOutcome, Effect) {
        let held = self.take(RateClass::Order).await;
        if let Err(why) = allowed() {
            held.release();
            return (SendOutcome::NotSent { reason: why }, Effect::None);
        }
        // One time for the rule and the signature.
        let ts = self.rest.now_ms();
        if !fits_expiry(ts, self.rest.recv_window_ms(), c.expires_at) {
            held.release();
            return (
                SendOutcome::NotSent {
                    reason: format!(
                        "expires_at {} is closer than the receive window plus 1 s",
                        c.expires_at
                    ),
                },
                Effect::None,
            );
        }
        let body = create_body(c.symbol, c.side, c.qty, c.px, c.tif, c.reduce_only, c.link);
        let req = Request {
            path: CREATE_PATH,
            op: trade::CREATE_OP,
            req_id: trade::create_req_id(c.link),
            body,
            ts,
        };
        match self.send(held, req, &allowed).await {
            Ended::NotSent(e) => (SendOutcome::NotSent { reason: e }, Effect::None),
            Ended::NoAnswer(e) => (SendOutcome::Unknown { reason: e }, Effect::None),
            Ended::Answer(a, fx) => (send_outcome(a, ts.saturating_mul(1_000)), fx),
        }
    }

    /// Sends one cancel by our client id, after the budget (which lets
    /// cancels use the reserve). `attempt` names the WS Trade request.
    pub async fn cancel(
        &self,
        symbol: &str,
        link: &str,
        attempt: u32,
        allowed: impl Fn() -> Result<(), String> + Sync,
    ) -> (CancelOutcome, Effect) {
        let held = self.take(RateClass::Cancel).await;
        if let Err(why) = allowed() {
            held.release();
            return (CancelOutcome::NotSent { reason: why }, Effect::None);
        }
        let req = Request {
            path: CANCEL_PATH,
            op: trade::CANCEL_OP,
            req_id: trade::cancel_req_id(attempt, link),
            body: cancel_body(symbol, link),
            ts: self.rest.now_ms(),
        };
        match self.send(held, req, &allowed).await {
            Ended::NotSent(e) => (CancelOutcome::NotSent { reason: e }, Effect::None),
            Ended::NoAnswer(e) => (CancelOutcome::Unknown { reason: e }, Effect::None),
            Ended::Answer(a, fx) => (cancel_outcome(a), fx),
        }
    }

    /// Sends one request on its slot: over WS Trade, or over REST when WS
    /// Trade wrote no byte of it. `allowed` is asked again before REST: the
    /// block that stopped the write keeps the request home.
    async fn send(
        &self,
        held: Held<'_>,
        req: Request,
        allowed: &(dyn Fn() -> Result<(), String> + Sync),
    ) -> Ended {
        let window = self.rest.recv_window_ms();
        if let Some(t) = &self.trade {
            let text = trade::request_message(&req.req_id, req.op, req.ts, window, &req.body);
            match t.request(&req.req_id, text).await {
                Reply::Answered { text, code } => {
                    let a = answer::classify(200, &text);
                    let fx = trade::effect(&a, code);
                    return Ended::Answer(a, fx);
                }
                Reply::NoAnswer(e) => {
                    held.unanswered(last_taken_ms(req.ts, window));
                    return Ended::NoAnswer(format!("WS Trade: {e}"));
                }
                Reply::NotWritten(_) => {
                    if let Err(why) = allowed() {
                        held.release();
                        return Ended::NotSent(why);
                    }
                }
            }
        }
        match self.rest.post_raw_at(req.path, &req.body, req.ts).await {
            Err(RestError::NotSent(e)) => {
                held.release();
                Ended::NotSent(e)
            }
            Err(e) => {
                held.unanswered(last_taken_ms(req.ts, window));
                Ended::NoAnswer(e.to_string())
            }
            Ok((status, text)) => {
                let a = answer::classify(status, &text);
                let fx = effect(status, &a);
                Ended::Answer(a, fx)
            }
        }
    }
}

/// One request, for either path.
struct Request {
    path: &'static str,
    op: &'static str,
    req_id: String,
    body: Value,
    /// The request time: signs it and stamps it.
    ts: u64,
}

/// How a request ended, for the outcome.
enum Ended {
    /// No byte left the host; the slot is free.
    NotSent(String),
    /// It may have left and no answer came.
    NoAnswer(String),
    /// The answer's class, and what it says about the key or the IP.
    Answer(Answer, Effect),
}

/// A taken slot. Dropped, it finishes the request now (it may have left:
/// it counts one more window); `release` frees it when nothing left the
/// host; `unanswered` finishes it at the request's last possible arrival.
/// A request whose task is cancelled mid-flight still finishes.
struct Held<'a> {
    orders: &'a Orders,
    slot: Option<Slot>,
}

impl Held<'_> {
    fn release(mut self) {
        if let Some(slot) = self.slot.take() {
            self.orders.budget().release(slot);
        }
    }

    /// The request left and no answer came (a timeout, a dropped
    /// connection): it may still reach the venue until `last_ms` on the
    /// local clock (`last_taken_ms`), so it finishes then, not now.
    fn unanswered(mut self, last_ms: u64) {
        if let Some(slot) = self.slot.take() {
            let wait_us = last_ms
                .saturating_sub(self.orders.rest.now_ms())
                .saturating_mul(1_000);
            let end = self.orders.mono_us().saturating_add(wait_us);
            self.orders.budget().finish(slot, end);
        }
    }
}

impl Drop for Held<'_> {
    fn drop(&mut self) {
        if let Some(slot) = self.slot.take() {
            let now = self.orders.mono_us();
            self.orders.budget().finish(slot, now);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use http_fake::{Answer as Http, FakeHttp};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::super::trade::{IP_LIMIT, Reply, Trade};

    use crate::venue::bybit::rest::Credentials;

    const NOW_MS: u64 = 1_700_000_000_000;

    fn clock() -> u64 {
        NOW_MS
    }

    fn orders(api: &FakeHttp) -> Orders {
        let rest = Rest::new(
            &api.url(),
            Arc::new(Credentials::new("KEY", "SECRET")),
            5000,
            Duration::from_millis(300),
        )
        .unwrap()
        .with_clock(clock);
        Orders::new(rest)
    }

    fn create(expires_at: Ts) -> Create<'static> {
        Create {
            symbol: "BTCUSDT",
            side: Side::Sell,
            qty: Qty::parse("0.002").unwrap(),
            px: Px::parse("30000.5").unwrap(),
            tif: Tif::Alo,
            reduce_only: true,
            link: "LINKID",
            expires_at,
        }
    }

    /// Just far enough: request time + window + 1 s.
    const LATER: Ts = (NOW_MS + 5000 + 1000) * 1000;

    fn yes() -> Result<(), String> {
        Ok(())
    }

    fn placed(oid: &str) -> Http {
        Http::Reply(
            200,
            json!({"retCode": 0, "retMsg": "OK", "result": {"orderId": oid, "orderLinkId": "LINKID"}, "time": 1}).to_string(),
        )
    }

    #[tokio::test]
    async fn a_create_is_one_signed_limit_order_and_its_ack_is_an_open_order() {
        let api = FakeHttp::start(Http::Reply(404, "{}".into()));
        api.route(CREATE_PATH, [placed("o-77")]);
        let (out, fx) = orders(&api).create(&create(LATER), yes).await;
        assert_eq!(
            out,
            SendOutcome::Placed {
                venue_oid: VenueOrderId::new("o-77").unwrap(),
                status: StatusIn {
                    status: VenueStatus::Open,
                    cum: None,
                    venue_ts: NOW_MS * 1000
                },
            }
        );
        assert_eq!(fx, Effect::None);
        let reqs = api.requests();
        assert_eq!(reqs.len(), 1);
        let body: Value = serde_json::from_slice(&reqs[0].body).unwrap();
        assert_eq!(
            body,
            json!({"category": "linear", "symbol": "BTCUSDT", "side": "Sell", "orderType": "Limit",
                   "qty": "0.002", "price": "30000.5", "timeInForce": "PostOnly", "orderLinkId": "LINKID",
                   "reduceOnly": true, "positionIdx": 0})
        );
        assert_eq!(
            reqs[0].header("x-bapi-timestamp"),
            Some(NOW_MS.to_string().as_str())
        );
        assert_eq!(reqs[0].header("x-bapi-recv-window"), Some("5000"));
        let sig = crate::venue::bybit::sign::rest_signature(
            "SECRET",
            NOW_MS,
            "KEY",
            5000,
            &reqs[0].body_str(),
        );
        assert_eq!(reqs[0].header("x-bapi-sign"), Some(sig.as_str()));
    }

    /// RSA (ADR-034, spec T5): an order create sent with an RSA key is
    /// signed over the exact body sent and its `X-BAPI-SIGN` is openssl's
    /// base64 value (`printf '%s' "1700000000000KEY5000<body>" | openssl
    /// dgst -sha256 -sign key.pem | base64 -w0`, the test-only key).
    #[tokio::test]
    async fn a_create_with_an_rsa_key_is_signed_in_base64_over_the_body_sent() {
        let api = FakeHttp::start(Http::Reply(404, "{}".into()));
        api.route(CREATE_PATH, [placed("o-77")]);
        let pem = crate::venue::bybit::sign::test_keys::pem_2048();
        let rest = Rest::new(
            &api.url(),
            Arc::new(Credentials::rsa("KEY", &pem).unwrap()),
            5000,
            Duration::from_millis(300),
        )
        .unwrap()
        .with_clock(clock);
        let (out, _) = Orders::new(rest).create(&create(LATER), yes).await;
        assert!(matches!(out, SendOutcome::Placed { .. }), "{out:?}");
        let reqs = api.requests();
        assert_eq!(reqs.len(), 1);
        assert_eq!(reqs[0].method, "POST");
        assert_eq!(
            reqs[0].body_str(),
            r#"{"category":"linear","orderLinkId":"LINKID","orderType":"Limit","positionIdx":0,"price":"30000.5","qty":"0.002","reduceOnly":true,"side":"Sell","symbol":"BTCUSDT","timeInForce":"PostOnly"}"#
        );
        assert_eq!(reqs[0].header("x-bapi-sign-type"), None);
        assert_eq!(reqs[0].header("x-bapi-sign"), Some(
                "ThpbQFkmN21p/YqsJmbXrW10xGZAox+Au2t5NoQALPuzvTlHJBN8OiLGoTltkv2vx9o9Y7zGEkdOueJpI0t+ZhAKql5mRodsfF3X6FguKhtx9t7Y7xOqnlKBo7EX+LZjqC0qYDpibztqSu0Kbm+CyMnP9k4Uu0Uijh3ctLFl2vQuOmYxi+A3Mgi6dlCJPaIJns4Lbj0lI2pfOiFPJtDhLcDjbHSesdBlgPgkbZfZ0sqN96KbjjrR2I8en7Nxn1ih2BEYlsMxVLLw++gUtuqp7fdq4LfFK7wjHG9Zbex8HvFwjclF81QktCU3YbwquDAMrAiPaj8TUtidWt01L/kxpA=="
            ));
    }

    #[tokio::test]
    async fn an_expiry_inside_the_window_plus_one_second_sends_nothing() {
        let api = FakeHttp::start(Http::Reply(404, "{}".into()));
        api.route(CREATE_PATH, [placed("o-1")]);
        let o = orders(&api);
        let (out, _) = o.create(&create(LATER - 1), yes).await;
        assert!(matches!(out, SendOutcome::NotSent { .. }), "{out:?}");
        assert!(api.requests().is_empty());
        let (out, _) = o.create(&create(LATER), yes).await;
        assert!(matches!(out, SendOutcome::Placed { .. }), "{out:?}");
    }

    #[tokio::test]
    async fn a_block_after_the_wait_sends_nothing() {
        let api = FakeHttp::start(placed("o-1"));
        let o = orders(&api);
        let (out, _) = o
            .create(&create(LATER), || Err("ip_banned: x".into()))
            .await;
        assert_eq!(
            out,
            SendOutcome::NotSent {
                reason: "ip_banned: x".into()
            }
        );
        let (out, _) = o
            .cancel("BTCUSDT", "LINKID", 1, || Err("auth_rejected: y".into()))
            .await;
        assert_eq!(
            out,
            CancelOutcome::NotSent {
                reason: "auth_rejected: y".into()
            }
        );
        assert!(api.requests().is_empty());
    }

    /// A lost or silent answer is `Unknown` and the create is never sent
    /// again (G-3): the core resolves it by a query.
    #[tokio::test]
    async fn no_answer_is_unknown_and_nothing_is_sent_twice() {
        for silent in [
            Http::Hang,
            Http::Close,
            Http::Reply(502, "".into()),
            Http::Reply(200, "{".into()),
        ] {
            let api = FakeHttp::start(Http::Reply(404, "{}".into()));
            api.route(CREATE_PATH, [silent.clone()]);
            let (out, fx) = orders(&api).create(&create(LATER), yes).await;
            assert!(
                matches!(out, SendOutcome::Unknown { .. }),
                "{silent:?}: {out:?}"
            );
            assert_eq!(fx, Effect::None);
            tokio::time::sleep(Duration::from_millis(400)).await;
            assert_eq!(api.requests().len(), 1, "{silent:?}");
        }
    }

    #[tokio::test]
    async fn a_dead_venue_is_not_sent() {
        // A bound socket that does not listen: the port stays ours and a
        // connect is refused (a closed listener's port could be reused by
        // another test's server).
        let held = tokio::net::TcpSocket::new_v4().unwrap();
        held.bind("127.0.0.1:0".parse().unwrap()).unwrap();
        let rest = Rest::new(
            &format!("http://{}", held.local_addr().unwrap()),
            Arc::new(Credentials::new("KEY", "SECRET")),
            5000,
            Duration::from_millis(300),
        )
        .unwrap()
        .with_clock(clock);
        let (out, _) = Orders::new(rest).create(&create(LATER), yes).await;
        assert!(matches!(out, SendOutcome::NotSent { .. }), "{out:?}");
    }

    #[tokio::test]
    async fn refusals_are_rejected_and_the_key_and_ip_answers_have_their_effects() {
        let cases = [
            (
                Http::Reply(
                    200,
                    r#"{"retCode":110007,"retMsg":"Available balance is insufficient"}"#.into(),
                ),
                "rejected",
                Effect::None,
            ),
            (
                Http::Reply(
                    200,
                    r#"{"retCode":110072,"retMsg":"OrderLinkedID is duplicate"}"#.into(),
                ),
                "unknown",
                Effect::None,
            ),
            (
                Http::Reply(200, r#"{"retCode":10010,"retMsg":"Unmatched IP"}"#.into()),
                "rejected",
                Effect::RejectKey("key rejected (10010): Unmatched IP".into()),
            ),
            (
                Http::Reply(401, "".into()),
                "unknown",
                Effect::RejectKey("HTTP 401 on the order path".into()),
            ),
            (
                Http::Reply(403, "too frequent".into()),
                "unknown",
                Effect::Ban(String::new()),
            ),
            (
                Http::Reply(
                    200,
                    r#"{"retCode":10018,"retMsg":"Exceeded the IP Rate Limit."}"#.into(),
                ),
                "rejected",
                Effect::Ban(String::new()),
            ),
            (
                Http::Reply(
                    200,
                    r#"{"retCode":0,"result":{"orderLinkId":"LINKID"}}"#.into(),
                ),
                "unknown",
                Effect::None,
            ),
        ];
        for (a, class, want) in cases {
            let api = FakeHttp::start(Http::Reply(404, "{}".into()));
            api.route(CREATE_PATH, [a.clone()]);
            let (out, fx) = orders(&api).create(&create(LATER), yes).await;
            let got = match out {
                SendOutcome::Rejected { .. } => "rejected",
                SendOutcome::Unknown { .. } => "unknown",
                _ => "other",
            };
            assert_eq!(got, class, "{a:?}: {out:?}");
            match want {
                Effect::Ban(_) => assert!(matches!(fx, Effect::Ban(_)), "{a:?}: {fx:?}"),
                w => assert_eq!(fx, w, "{a:?}"),
            }
        }
    }

    #[tokio::test]
    async fn a_cancel_goes_by_client_id_and_has_the_same_classes() {
        let api = FakeHttp::start(Http::Reply(404, "{}".into()));
        api.route(
            CANCEL_PATH,
            [
                Http::Reply(
                    200,
                    r#"{"retCode":0,"result":{"orderId":"o-1","orderLinkId":"LINKID"}}"#.into(),
                ),
                Http::Reply(
                    200,
                    r#"{"retCode":110001,"retMsg":"Order does not exist"}"#.into(),
                ),
                Http::Hang,
            ],
        );
        let o = orders(&api);
        assert_eq!(
            o.cancel("BTCUSDT", "LINKID", 1, yes).await.0,
            CancelOutcome::Confirmed
        );
        assert_eq!(
            o.cancel("BTCUSDT", "LINKID", 1, yes).await.0,
            CancelOutcome::Rejected {
                reason: "bybit 110001: Order does not exist".into()
            }
        );
        assert!(matches!(
            o.cancel("BTCUSDT", "LINKID", 1, yes).await.0,
            CancelOutcome::Unknown { .. }
        ));
        let body: Value = serde_json::from_slice(&api.requests()[0].body).unwrap();
        assert_eq!(
            body,
            json!({"category": "linear", "symbol": "BTCUSDT", "orderLinkId": "LINKID"})
        );
    }

    /// The ninth create within a second waits for the window while a
    /// cancel still goes out at once.
    #[tokio::test]
    async fn the_ninth_create_in_a_second_waits_while_a_cancel_goes_out() {
        let api = FakeHttp::start(Http::Reply(404, "{}".into()));
        api.route(CREATE_PATH, [placed("o-1")]);
        api.route(
            CANCEL_PATH,
            [Http::Reply(200, r#"{"retCode":0,"result":{}}"#.into())],
        );
        let o = Arc::new(orders(&api));
        let start = Instant::now();
        for _ in 0..8 {
            o.create(&create(LATER), yes).await;
        }
        let ninth = {
            let o = o.clone();
            tokio::spawn(async move { o.create(&create(LATER), yes).await })
        };
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(
            o.cancel("BTCUSDT", "LINKID", 1, yes).await.0,
            CancelOutcome::Confirmed
        );
        let cancel_at = start.elapsed();
        assert!(!ninth.is_finished(), "the ninth create did not wait");
        assert!(matches!(ninth.await.unwrap().0, SendOutcome::Placed { .. }));
        let done = start.elapsed();
        assert!(
            cancel_at < Duration::from_millis(900),
            "the cancel waited: {cancel_at:?}"
        );
        assert!(
            done >= Duration::from_secs(1),
            "the ninth went out after {done:?}"
        );
        let creates = api
            .requests()
            .iter()
            .filter(|r| r.path() == CREATE_PATH)
            .count();
        assert_eq!(creates, 9);
    }

    /// A request with no answer may still arrive until its stamp plus the
    /// receive window plus 1 s, and counts one window after that: eight
    /// creates that timed out after 0.3 s, with a 0.1 s receive window and
    /// a clock that stands still, let the ninth go only 0.3 + 1.1 + 1 s
    /// after the start. A create refused at connect frees its slot at once.
    #[tokio::test]
    async fn a_slot_without_an_answer_counts_until_its_last_arrival_plus_a_second() {
        let api = FakeHttp::start(Http::Reply(404, "{}".into()));
        api.route(CREATE_PATH, [Http::Hang]);
        let rest = Rest::new(
            &api.url(),
            Arc::new(Credentials::new("KEY", "SECRET")),
            100,
            Duration::from_millis(300),
        )
        .unwrap()
        .with_clock(clock);
        let o = Arc::new(Orders::new(rest));
        let start = Instant::now();
        let tasks: Vec<_> = (0..8)
            .map(|_| {
                let o = o.clone();
                tokio::spawn(async move { o.create(&create(LATER), yes).await })
            })
            .collect();
        for t in tasks {
            assert!(matches!(t.await.unwrap().0, SendOutcome::Unknown { .. }));
        }
        api.route(CREATE_PATH, [placed("o-9")]);
        assert!(matches!(
            o.create(&create(LATER), yes).await.0,
            SendOutcome::Placed { .. }
        ));
        let ninth = start.elapsed();
        assert!(
            ninth >= Duration::from_millis(2_400),
            "the ninth went out after {ninth:?}"
        );
        // Refused at connect: nothing left, nothing counts.
        let held = tokio::net::TcpSocket::new_v4().unwrap();
        held.bind("127.0.0.1:0".parse().unwrap()).unwrap();
        let dead = Rest::new(
            &format!("http://{}", held.local_addr().unwrap()),
            Arc::new(Credentials::new("KEY", "SECRET")),
            5000,
            Duration::from_millis(300),
        )
        .unwrap()
        .with_clock(clock);
        let d = Orders::new(dead);
        let t0 = Instant::now();
        for _ in 0..20 {
            assert!(matches!(
                d.create(&create(LATER), yes).await.0,
                SendOutcome::NotSent { .. }
            ));
        }
        assert!(
            t0.elapsed() < Duration::from_millis(900),
            "refused creates held slots: {:?}",
            t0.elapsed()
        );
    }

    /// A fake WS Trade connection: records each request's message and
    /// answers it as `f` says (`None`: drops it unanswered).
    fn ws_conn(
        o: Orders,
        f: impl Fn(&Value) -> Option<Reply> + Send + 'static,
    ) -> (Orders, Arc<Mutex<Vec<Value>>>) {
        let t = Arc::new(Trade::default());
        let mut jobs = t.install();
        let seen: Arc<Mutex<Vec<Value>>> = Arc::default();
        let log = seen.clone();
        tokio::spawn(async move {
            while let Some(job) = jobs.recv().await {
                let m: Value = serde_json::from_str(&job.text).unwrap();
                assert_eq!(m["reqId"].as_str(), Some(job.req_id.as_str()));
                log.lock().unwrap().push(m.clone());
                if let Some(r) = f(&m) {
                    let _ = job.reply.send(r);
                }
            }
        });
        (o.with_trade(t), seen)
    }

    fn ws_ok(m: &Value, data: Value) -> Reply {
        Reply::Answered {
            text: json!({"reqId": m["reqId"], "retCode": 0, "retMsg": "OK", "op": m["op"], "data": data}).to_string(),
            code: Some(0),
        }
    }

    fn ws_code(m: &Value, code: i64) -> Reply {
        Reply::Answered {
            text: json!({"reqId": m["reqId"], "retCode": code, "retMsg": "m", "op": m["op"], "data": {}}).to_string(),
            code: Some(code),
        }
    }

    /// Up: the create and the cancel go over WS Trade only, each with its
    /// own reqId, the request time and window, and the REST body.
    #[tokio::test]
    async fn a_request_goes_over_ws_trade_when_it_is_up_and_not_over_rest() {
        let api = FakeHttp::start(placed("o-rest"));
        let (o, seen) = ws_conn(orders(&api), |m| Some(ws_ok(m, json!({"orderId": "o-ws", "orderLinkId": "LINKID"}))));
        let (out, fx) = o.create(&create(LATER), yes).await;
        assert_eq!(
            out,
            SendOutcome::Placed {
                venue_oid: VenueOrderId::new("o-ws").unwrap(),
                status: StatusIn {
                    status: VenueStatus::Open,
                    cum: None,
                    venue_ts: NOW_MS * 1000
                },
            }
        );
        assert_eq!(fx, Effect::None);
        let (out, _) = o.cancel("BTCUSDT", "LINKID", 3, yes).await;
        assert_eq!(out, CancelOutcome::Confirmed);
        assert!(api.requests().is_empty(), "REST was used");
        let seen = seen.lock().unwrap().clone();
        let c = &create(LATER);
        let header = json!({"X-BAPI-TIMESTAMP": NOW_MS.to_string(), "X-BAPI-RECV-WINDOW": "5000"});
        assert_eq!(
            seen,
            vec![
                json!({"reqId": "cLINKID", "header": header, "op": "order.create",
                       "args": [create_body(c.symbol, c.side, c.qty, c.px, c.tif, c.reduce_only, c.link)]}),
                json!({"reqId": "x3LINKID", "header": header, "op": "order.cancel",
                       "args": [cancel_body("BTCUSDT", "LINKID")]}),
            ]
        );
    }

    /// Nothing written: REST sends it, once. A block that came meanwhile
    /// keeps it home on both paths.
    #[tokio::test]
    async fn a_request_not_written_goes_over_rest_once_unless_a_block_came() {
        // No connection at all.
        let api = FakeHttp::start(placed("o-rest"));
        let o = orders(&api).with_trade(Arc::new(Trade::default()));
        assert!(matches!(o.create(&create(LATER), yes).await.0, SendOutcome::Placed { .. }));
        assert_eq!(api.requests().len(), 1);
        // A connection that wrote nothing.
        let api = FakeHttp::start(placed("o-rest"));
        let (o, seen) = ws_conn(orders(&api), |_| Some(Reply::NotWritten("down".into())));
        assert!(matches!(o.create(&create(LATER), yes).await.0, SendOutcome::Placed { .. }));
        assert!(matches!(o.cancel("BTCUSDT", "LINKID", 1, yes).await.0, CancelOutcome::Confirmed));
        assert_eq!(api.requests().len(), 2, "one REST request each");
        assert_eq!(seen.lock().unwrap().len(), 2);
        // The block came after the first look: neither path sends.
        let calls = Arc::new(AtomicUsize::new(0));
        let later_blocked = {
            let calls = calls.clone();
            move || match calls.fetch_add(1, Ordering::SeqCst) {
                0 => Ok(()),
                _ => Err("ip_banned: meanwhile".to_string()),
            }
        };
        let (out, _) = o.create(&create(LATER), later_blocked).await;
        assert_eq!(
            out,
            SendOutcome::NotSent {
                reason: "ip_banned: meanwhile".into()
            }
        );
        assert_eq!(api.requests().len(), 2, "the blocked create went over REST");
    }

    /// Written and no answer (a timeout, a dropped connection, a task
    /// gone): unknown, and REST does not send it again (G-3).
    #[tokio::test]
    async fn a_written_request_without_an_answer_is_unknown_and_not_sent_again() {
        let api = FakeHttp::start(placed("o-rest"));
        let (o, _) = ws_conn(orders(&api), |m| {
            (m["op"] == "order.create").then(|| Reply::NoAnswer("no answer within 500 ms".into()))
        });
        let (out, fx) = o.create(&create(LATER), yes).await;
        assert!(matches!(&out, SendOutcome::Unknown { reason } if reason.contains("no answer within")), "{out:?}");
        assert_eq!(fx, Effect::None);
        // The cancel's reply is dropped unsent.
        assert!(matches!(o.cancel("BTCUSDT", "LINKID", 1, yes).await.0, CancelOutcome::Unknown { .. }));
        assert!(api.requests().is_empty(), "REST sent a written request again");
    }

    #[tokio::test]
    async fn ws_answers_have_the_rest_classes_and_their_effects() {
        let cases: [(i64, &str, &str); 9] = [
            (110007, "rejected", "none"),
            (110072, "unknown", "none"),
            (20006, "unknown", "none"),
            (10019, "unknown", "none"),
            (10429, "unknown", "none"),
            (10016, "unknown", "none"),
            (10010, "rejected", "key"),
            (10018, "rejected", "ban"),
            (IP_LIMIT, "unknown", "ban"),
        ];
        for (code, class, effect) in cases {
            let api = FakeHttp::start(placed("o-rest"));
            let (o, _) = ws_conn(orders(&api), move |m| Some(ws_code(m, code)));
            let (out, fx) = o.create(&create(LATER), yes).await;
            let got = match out {
                SendOutcome::Rejected { .. } => "rejected",
                SendOutcome::Unknown { .. } => "unknown",
                _ => "other",
            };
            assert_eq!(got, class, "{code}: {out:?}");
            let fx_got = match fx {
                Effect::None => "none",
                Effect::Ban(_) => "ban",
                Effect::RejectKey(_) => "key",
            };
            assert_eq!(fx_got, effect, "{code}: {fx:?}");
            let (out, fx) = o.cancel("BTCUSDT", "LINKID", 1, yes).await;
            assert!(!matches!(out, CancelOutcome::Confirmed | CancelOutcome::NotSent { .. }), "{code}: {out:?}");
            assert_eq!(matches!(fx, Effect::None), effect == "none", "{code}: {fx:?}");
            assert!(api.requests().is_empty());
        }
        // Accepted without an orderId: the order may exist but has no name.
        let api = FakeHttp::start(placed("o-rest"));
        let (o, _) = ws_conn(orders(&api), |m| Some(ws_ok(m, json!({"orderLinkId": "LINKID"}))));
        assert!(matches!(o.create(&create(LATER), yes).await.0, SendOutcome::Unknown { .. }));
    }

    /// One slot per request whichever path: four creates over WS Trade and
    /// four over REST fill the second, and the ninth waits.
    #[tokio::test]
    async fn the_ninth_create_in_a_second_waits_across_both_paths() {
        let api = FakeHttp::start(placed("o-rest"));
        let n = Arc::new(AtomicUsize::new(0));
        let count = n.clone();
        let (o, seen) = ws_conn(orders(&api), move |m| {
            match count.fetch_add(1, Ordering::SeqCst) {
                4..=7 => Some(Reply::NotWritten("down".into())),
                _ => Some(ws_ok(m, json!({"orderId": "o-ws"}))),
            }
        });
        let start = Instant::now();
        for _ in 0..8 {
            assert!(matches!(o.create(&create(LATER), yes).await.0, SendOutcome::Placed { .. }));
        }
        assert_eq!(api.requests().len(), 4);
        assert!(start.elapsed() < Duration::from_millis(900));
        assert!(matches!(o.create(&create(LATER), yes).await.0, SendOutcome::Placed { .. }));
        let ninth = start.elapsed();
        assert!(ninth >= Duration::from_secs(1), "the ninth went out after {ninth:?}");
        assert_eq!(seen.lock().unwrap().len(), 9);
        assert_eq!(api.requests().len(), 4);
    }

    /// A written request without an answer counts until its last possible
    /// arrival plus the window, as on REST: 0 + 0.1 + 1 + 1 s.
    #[tokio::test]
    async fn a_ws_request_without_an_answer_counts_until_its_last_arrival_plus_a_second() {
        let api = FakeHttp::start(placed("o-rest"));
        let rest = Rest::new(
            &api.url(),
            Arc::new(Credentials::new("KEY", "SECRET")),
            100,
            Duration::from_millis(300),
        )
        .unwrap()
        .with_clock(clock);
        let n = Arc::new(AtomicUsize::new(0));
        let count = n.clone();
        let (o, _) = ws_conn(Orders::new(rest), move |m| {
            match count.fetch_add(1, Ordering::SeqCst) {
                0..8 => Some(Reply::NoAnswer("dropped".into())),
                _ => Some(ws_ok(m, json!({"orderId": "o-ws"}))),
            }
        });
        let start = Instant::now();
        for _ in 0..8 {
            assert!(matches!(o.create(&create(LATER), yes).await.0, SendOutcome::Unknown { .. }));
        }
        assert!(matches!(o.create(&create(LATER), yes).await.0, SendOutcome::Placed { .. }));
        let ninth = start.elapsed();
        assert!(ninth >= Duration::from_millis(2_050), "the ninth went out after {ninth:?}");
    }

    #[test]
    fn the_send_time_rule_is_exact_at_the_bound() {
        assert!(fits_expiry(1_000, 5_000, 7_000_000));
        assert!(!fits_expiry(1_000, 5_000, 6_999_999));
        assert!(!fits_expiry(u64::MAX, 5_000, u64::MAX - 1));
    }
}
