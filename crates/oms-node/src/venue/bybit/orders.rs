//! Orders and cancels over REST (ADR-028 §7d, U17a): the request, the
//! send-time rule (G-4), the rate budget, and the answer class
//! (`answer`). Nothing here sends a request twice (G-3).

use std::sync::Mutex;
use std::time::{Duration, Instant};

use oms_core::*;
use serde_json::{Value, json};

use super::answer::{self, Answer};
use super::rest::{AUTH_CODES, Rest, RestError};

pub const CREATE_PATH: &str = "/v5/order/create";
pub const CANCEL_PATH: &str = "/v5/order/cancel";

/// What an answer says about the key or the IP, for the gateway to act on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Effect {
    None,
    /// HTTP 403: pause every request (`IP_BAN_PAUSE`).
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
    let last_us = ts_ms
        .saturating_add(recv_window_ms)
        .saturating_add(1_000)
        .saturating_mul(1_000);
    last_us <= expires_at
}

/// What the gateway learns from an HTTP answer besides its class.
pub fn effect(status: u16, a: &Answer) -> Effect {
    match (status, a) {
        (403, _) => Effect::Ban(format!("HTTP 403 on the order path: {a:?}")),
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

/// The REST order path with its rate budget.
pub struct Orders {
    rest: Rest,
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
            budget: Mutex::new(RateBudget::bybit()),
            epoch: Instant::now(),
        }
    }

    fn mono_us(&self) -> u64 {
        self.epoch.elapsed().as_micros() as u64
    }

    /// Waits until the budget lets one request of `class` out, and takes
    /// the slot. Nothing is dropped; the wait has no limit (an order that
    /// waited too long fails the send-time rule after it).
    pub async fn take(&self, class: RateClass) {
        loop {
            let now = self.mono_us();
            let r = self
                .budget
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .take(class, now);
            match r {
                Ok(()) => return,
                Err(at) => tokio::time::sleep(Duration::from_micros(at.saturating_sub(now))).await,
            }
        }
    }

    /// Sends one create, after the budget. `allowed` is asked after the
    /// wait: a block that appeared meanwhile keeps the request home.
    pub async fn create(&self, c: &Create<'_>, allowed: impl Fn() -> Result<(), String>) -> (SendOutcome, Effect) {
        self.take(RateClass::Order).await;
        if let Err(why) = allowed() {
            return (SendOutcome::NotSent { reason: why }, Effect::None);
        }
        // One time for the rule and the signature.
        let ts = self.rest.now_ms();
        if !fits_expiry(ts, self.rest.recv_window_ms(), c.expires_at) {
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
        match self.rest.post_raw_at(CREATE_PATH, &body, ts).await {
            Err(RestError::NotSent(e)) => (SendOutcome::NotSent { reason: e }, Effect::None),
            Err(e) => (SendOutcome::Unknown { reason: e.to_string() }, Effect::None),
            Ok((status, text)) => {
                let a = answer::classify(status, &text);
                let fx = effect(status, &a);
                (send_outcome(a, ts.saturating_mul(1_000)), fx)
            }
        }
    }

    /// Sends one cancel by our client id, after the budget (which lets
    /// cancels use the reserve).
    pub async fn cancel(&self, symbol: &str, link: &str, allowed: impl Fn() -> Result<(), String>) -> (CancelOutcome, Effect) {
        self.take(RateClass::Cancel).await;
        if let Err(why) = allowed() {
            return (CancelOutcome::NotSent { reason: why }, Effect::None);
        }
        let ts = self.rest.now_ms();
        match self.rest.post_raw_at(CANCEL_PATH, &cancel_body(symbol, link), ts).await {
            Err(RestError::NotSent(e)) => (CancelOutcome::NotSent { reason: e }, Effect::None),
            Err(e) => (CancelOutcome::Unknown { reason: e.to_string() }, Effect::None),
            Ok((status, text)) => {
                let a = answer::classify(status, &text);
                let fx = effect(status, &a);
                (cancel_outcome(a), fx)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use http_fake::{Answer as Http, FakeHttp};
    use std::sync::Arc;

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
                status: StatusIn { status: VenueStatus::Open, cum: None, venue_ts: NOW_MS * 1000 },
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
        assert_eq!(reqs[0].header("x-bapi-timestamp"), Some(NOW_MS.to_string().as_str()));
        assert_eq!(reqs[0].header("x-bapi-recv-window"), Some("5000"));
        let sig = crate::venue::bybit::sign::rest_signature("SECRET", NOW_MS, "KEY", 5000, &reqs[0].body_str());
        assert_eq!(reqs[0].header("x-bapi-sign"), Some(sig.as_str()));
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
        let (out, _) = o.create(&create(LATER), || Err("ip_banned: x".into())).await;
        assert_eq!(out, SendOutcome::NotSent { reason: "ip_banned: x".into() });
        let (out, _) = o.cancel("BTCUSDT", "LINKID", || Err("auth_rejected: y".into())).await;
        assert_eq!(out, CancelOutcome::NotSent { reason: "auth_rejected: y".into() });
        assert!(api.requests().is_empty());
    }

    /// A lost or silent answer is `Unknown` and the create is never sent
    /// again (G-3): the core resolves it by a query.
    #[tokio::test]
    async fn no_answer_is_unknown_and_nothing_is_sent_twice() {
        for silent in [Http::Hang, Http::Close, Http::Reply(502, "".into()), Http::Reply(200, "{".into())] {
            let api = FakeHttp::start(Http::Reply(404, "{}".into()));
            api.route(CREATE_PATH, [silent.clone()]);
            let (out, fx) = orders(&api).create(&create(LATER), yes).await;
            assert!(matches!(out, SendOutcome::Unknown { .. }), "{silent:?}: {out:?}");
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
            (Http::Reply(200, r#"{"retCode":110007,"retMsg":"Available balance is insufficient"}"#.into()), "rejected", Effect::None),
            (Http::Reply(200, r#"{"retCode":110072,"retMsg":"OrderLinkedID is duplicate"}"#.into()), "unknown", Effect::None),
            (Http::Reply(200, r#"{"retCode":10010,"retMsg":"Unmatched IP"}"#.into()), "rejected", Effect::RejectKey("key rejected (10010): Unmatched IP".into())),
            (Http::Reply(401, "".into()), "unknown", Effect::RejectKey("HTTP 401 on the order path".into())),
            (Http::Reply(403, "too frequent".into()), "unknown", Effect::Ban(String::new())),
            (Http::Reply(200, r#"{"retCode":0,"result":{"orderLinkId":"LINKID"}}"#.into()), "unknown", Effect::None),
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
                Http::Reply(200, r#"{"retCode":0,"result":{"orderId":"o-1","orderLinkId":"LINKID"}}"#.into()),
                Http::Reply(200, r#"{"retCode":110001,"retMsg":"Order does not exist"}"#.into()),
                Http::Hang,
            ],
        );
        let o = orders(&api);
        assert_eq!(o.cancel("BTCUSDT", "LINKID", yes).await.0, CancelOutcome::Confirmed);
        assert_eq!(
            o.cancel("BTCUSDT", "LINKID", yes).await.0,
            CancelOutcome::Rejected { reason: "bybit 110001: Order does not exist".into() }
        );
        assert!(matches!(o.cancel("BTCUSDT", "LINKID", yes).await.0, CancelOutcome::Unknown { .. }));
        let body: Value = serde_json::from_slice(&api.requests()[0].body).unwrap();
        assert_eq!(body, json!({"category": "linear", "symbol": "BTCUSDT", "orderLinkId": "LINKID"}));
    }

    /// The ninth create within a second waits for the window while a
    /// cancel still goes out at once.
    #[tokio::test]
    async fn the_ninth_create_in_a_second_waits_while_a_cancel_goes_out() {
        let api = FakeHttp::start(Http::Reply(404, "{}".into()));
        api.route(CREATE_PATH, [placed("o-1")]);
        api.route(CANCEL_PATH, [Http::Reply(200, r#"{"retCode":0,"result":{}}"#.into())]);
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
        assert_eq!(o.cancel("BTCUSDT", "LINKID", yes).await.0, CancelOutcome::Confirmed);
        let cancel_at = start.elapsed();
        assert!(!ninth.is_finished(), "the ninth create did not wait");
        assert!(matches!(ninth.await.unwrap().0, SendOutcome::Placed { .. }));
        let done = start.elapsed();
        assert!(cancel_at < Duration::from_millis(900), "the cancel waited: {cancel_at:?}");
        assert!(done >= Duration::from_secs(1), "the ninth went out after {done:?}");
        let creates = api.requests().iter().filter(|r| r.path() == CREATE_PATH).count();
        assert_eq!(creates, 9);
    }

    #[test]
    fn the_send_time_rule_is_exact_at_the_bound() {
        assert!(fits_expiry(1_000, 5_000, 7_000_000));
        assert!(!fits_expiry(1_000, 5_000, 6_999_999));
        assert!(!fits_expiry(u64::MAX, 5_000, u64::MAX - 1));
    }
}
