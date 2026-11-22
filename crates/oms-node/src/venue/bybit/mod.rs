//! The Bybit v5 gateway (ADR-028), unified trading account, category
//! `linear`, USDT settled, one-way mode, cross margin.
//!
//! Built in parts. U16b: signing, the REST client, the boot checks and the
//! public-IP check, cancel-all. U16c: the queries (`query`) and the
//! execution table (`exec`). U16d: the private stream (`stream`) and the
//! gateway behind `VenueGateway` (`gateway`). U17a: orders and cancels over
//! REST (`orders`) and the answer table (`answer`). U17b: WS Trade
//! (`trade`), the first path for orders and cancels. U17c: the reference
//! price from the public stream (`ticker`). U17d: the account's maintenance
//! margin and margin balance in the margin report (`query`), judged by the
//! core's account maintenance-margin rule. U18b2: the kill switch (`kill`):
//! the stream-outage trigger and the cancel-all confirmation loop.

pub mod answer;
pub mod checks;
pub mod exec;
pub mod gateway;
pub mod kill;
pub mod orders;
pub mod query;
pub mod rest;
pub mod sign;
pub mod stream;
pub mod ticker;
pub mod trade;

use serde_json::Value;

use rest::{Rest, RestError};

/// The venue's endpoints of a network (F18 L63, L311).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Endpoints {
    pub rest: String,
    pub private_ws: String,
    /// WS Trade (F18 §3.1).
    pub trade_ws: String,
    /// The public stream of category linear (the reference price, U17c).
    pub public_ws: String,
}

impl Endpoints {
    pub fn of(network: &str) -> Option<Endpoints> {
        let (rest, ws) = match network {
            "mainnet" => ("https://api.bybit.com", "wss://stream.bybit.com/v5"),
            "testnet" => (
                "https://api-testnet.bybit.com",
                "wss://stream-testnet.bybit.com/v5",
            ),
            _ => return None,
        };
        Some(Endpoints {
            rest: rest.into(),
            private_ws: format!("{ws}/private"),
            trade_ws: format!("{ws}/trade"),
            public_ws: format!("{ws}/public/linear"),
        })
    }
}

/// The venue cancels at most this many orders per cancel-all, picked at
/// random when more are open (F20 Q11.3).
pub const CANCEL_ALL_CAP: usize = 500;

/// What a cancel-all request achieved. The cancels are asynchronous: the
/// stream confirms each order (F20 Q11.5).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CancelAll {
    /// Orders the venue accepted to cancel.
    pub cancelled: usize,
    /// The venue hit `CANCEL_ALL_CAP`: other orders may still be open, and
    /// the caller must send cancel-all again.
    pub maybe_more: bool,
}

/// Cancels every open order in the kill scope (`category=linear`,
/// `settleCoin=USDT`) with one request. The kill switch (U18b) calls it;
/// nothing else does. `result.success` other than `"1"` is a refusal
/// (F20 Q11.4: `"0"` = fail), so a failed cancel-all never reads as done.
pub async fn cancel_all(rest: &Rest) -> Result<CancelAll, RestError> {
    let v = rest
        .post(
            "/v5/order/cancel-all",
            &serde_json::json!({"category": "linear", "settleCoin": checks::SETTLE_COIN}),
        )
        .await?;
    let success = v.get("success").and_then(Value::as_str);
    if success != Some("1") {
        return Err(RestError::Refused {
            code: 0,
            msg: format!("cancel-all answered retCode 0 with success {success:?}"),
        });
    }
    let cancelled = v
        .get("list")
        .and_then(Value::as_array)
        .map(Vec::len)
        .ok_or_else(|| RestError::NoAnswer("cancel-all: no list".into()))?;
    Ok(CancelAll {
        cancelled,
        maybe_more: cancelled >= CANCEL_ALL_CAP,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoints_are_the_documented_hosts() {
        assert_eq!(
            Endpoints::of("mainnet").unwrap().rest,
            "https://api.bybit.com"
        );
        assert_eq!(
            Endpoints::of("testnet").unwrap().private_ws,
            "wss://stream-testnet.bybit.com/v5/private"
        );
        assert_eq!(
            Endpoints::of("mainnet").unwrap().trade_ws,
            "wss://stream.bybit.com/v5/trade"
        );
        assert_eq!(
            Endpoints::of("testnet").unwrap().trade_ws,
            "wss://stream-testnet.bybit.com/v5/trade"
        );
        assert_eq!(
            Endpoints::of("mainnet").unwrap().public_ws,
            "wss://stream.bybit.com/v5/public/linear"
        );
        assert_eq!(
            Endpoints::of("testnet").unwrap().public_ws,
            "wss://stream-testnet.bybit.com/v5/public/linear"
        );
        assert_eq!(Endpoints::of("demo"), None);
    }

    fn cancel_answer(list: usize, success: &str) -> http_fake::Answer {
        let list: Vec<serde_json::Value> = (0..list)
            .map(|i| serde_json::json!({"orderId": i.to_string(), "orderLinkId": ""}))
            .collect();
        http_fake::Answer::Reply(
            200,
            serde_json::json!({"retCode": 0, "retMsg": "OK", "result": {"list": list, "success": success}, "time": 1})
                .to_string(),
        )
    }

    async fn run_cancel_all(answer: http_fake::Answer) -> Result<CancelAll, RestError> {
        let fake = http_fake::FakeHttp::start(answer);
        let rest = Rest::new(
            &fake.url(),
            std::sync::Arc::new(rest::Credentials::new("K", "S")),
            5000,
            std::time::Duration::from_millis(500),
        )
        .unwrap();
        cancel_all(&rest).await
    }

    #[tokio::test]
    async fn cancel_all_fails_unless_the_venue_says_success_and_flags_the_cap() {
        assert_eq!(
            run_cancel_all(cancel_answer(2, "1")).await.unwrap(),
            CancelAll {
                cancelled: 2,
                maybe_more: false
            }
        );
        assert!(matches!(
            run_cancel_all(cancel_answer(0, "0")).await,
            Err(RestError::Refused { .. })
        ));
        assert!(matches!(
            run_cancel_all(http_fake::Answer::Reply(
                200,
                r#"{"retCode":0,"retMsg":"OK","result":{"list":[]},"time":1}"#.into()
            ))
            .await,
            Err(RestError::Refused { .. })
        ));
        assert!(
            run_cancel_all(cancel_answer(CANCEL_ALL_CAP, "1"))
                .await
                .unwrap()
                .maybe_more
        );
    }
}
