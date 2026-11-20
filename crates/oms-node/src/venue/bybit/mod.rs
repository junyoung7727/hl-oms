//! The Bybit v5 gateway (ADR-028), unified trading account, category
//! `linear`, USDT settled, one-way mode, cross margin.
//!
//! Built in parts. U16b (this): signing, the REST client, the boot checks
//! and the public-IP check, cancel-all. U16c: the queries and the
//! execution table. U16d: the private stream and the node wiring. Orders
//! are sent from U17 on.

pub mod checks;
pub mod rest;
pub mod sign;

use serde_json::Value;

use rest::{Rest, RestError};

/// The venue's endpoints of a network (F18 L63, L311).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Endpoints {
    pub rest: String,
    pub private_ws: String,
}

impl Endpoints {
    pub fn of(network: &str) -> Option<Endpoints> {
        let (rest, ws) = match network {
            "mainnet" => ("https://api.bybit.com", "wss://stream.bybit.com/v5/private"),
            "testnet" => (
                "https://api-testnet.bybit.com",
                "wss://stream-testnet.bybit.com/v5/private",
            ),
            _ => return None,
        };
        Some(Endpoints {
            rest: rest.into(),
            private_ws: ws.into(),
        })
    }
}

/// Cancels every open order in the kill scope (`category=linear`,
/// `settleCoin=USDT`) with one request. Returns the venue's `result`. The
/// kill switch (U18b) calls it; nothing else does.
pub async fn cancel_all(rest: &Rest) -> Result<Value, RestError> {
    rest.post(
        "/v5/order/cancel-all",
        &serde_json::json!({"category": "linear", "settleCoin": checks::SETTLE_COIN}),
    )
    .await
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
        assert_eq!(Endpoints::of("demo"), None);
    }
}
