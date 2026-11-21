//! Wire protocol between strategies or operators and the OMS process.
//!
//! Transport: a Unix domain socket on the same host. Framing: one JSON object
//! per line (`\n` terminated), at most [`MAX_LINE`] bytes. A connection sends
//! [`Request`]s and receives [`ServerMsg`]s. Each request carries an `id` that
//! the matching response repeats; responses to different requests on one
//! connection may arrive in any order. Unsolicited [`ServerMsg::Event`]s
//! arrive after `subscribe`.
//!
//! Two sockets exist. The trading socket accepts the strategy operations. The
//! admin socket additionally accepts operator operations. Which process may
//! open which socket is decided by file permissions and the peer's user id.
//!
//! Amounts, prices and quantities are decimal strings; times are microseconds
//! since the Unix epoch on the host clock.

#![cfg_attr(test, allow(clippy::unwrap_used))]

pub mod client;
pub mod ctl;

use oms_core::*;
use serde::{Deserialize, Serialize};

/// Longest accepted line, request or response, newline excluded.
pub const MAX_LINE: usize = 64 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Request {
    pub id: u64,
    #[serde(flatten)]
    pub op: Op,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Op {
    /// Must be the first request. A strategy connection states its strategy;
    /// an operator connection states none.
    Hello {
        strategy: Option<StrategyId>,
        #[serde(default)]
        client: String,
    },
    Ping,
    NewOrder {
        client_order_id: ClientOrderId,
        instrument: InstrumentId,
        side: Side,
        qty: Qty,
        px: Px,
        tif: Tif,
        #[serde(default)]
        reduce_only: bool,
        /// Absolute, so that a repeated request is byte-for-byte the same
        /// request. The send is not placed after this time.
        expires_at: Ts,
        /// The order book state the strategy decided on (U13c). Journaled
        /// with the order; not part of the idempotency comparison.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        market_ref: Option<MarketRef>,
    },
    Cancel {
        #[serde(default)]
        order_id: Option<OrderId>,
        #[serde(default)]
        client_order_id: Option<ClientOrderId>,
    },
    GetOrder {
        #[serde(default)]
        order_id: Option<OrderId>,
        #[serde(default)]
        client_order_id: Option<ClientOrderId>,
    },
    /// Open orders of this strategy (all strategies on the admin socket).
    ListOpen,
    Status,
    /// Start receiving order events for this strategy.
    Subscribe,

    // ---- admin socket only ------------------------------------------------
    Halt {
        strategy: Option<StrategyId>,
        note: String,
    },
    Resume {
        strategy: Option<StrategyId>,
        note: String,
    },
    AckMismatch {
        mismatch_id: u32,
        note: String,
    },
    ResolveOrder {
        order_id: OrderId,
        note: String,
    },
    CancelAll {
        strategy: Option<StrategyId>,
        note: String,
    },
    AdjustPosition {
        instrument: InstrumentId,
        /// `None` books to the unattributed ledger.
        strategy: Option<StrategyId>,
        delta: SQty,
        note: String,
    },
    /// End REDUCING; the current account value becomes the day base
    /// (C-MARGIN).
    ClearReduce {
        note: String,
    },
    /// Take the current account value as the day base, after a deposit or a
    /// withdrawal (C-MARGIN).
    RebaseDay {
        note: String,
    },
    /// Write a snapshot now.
    Snapshot,
    /// Release one hold of C-HOLD: `kill`, `watcher_mismatch` or
    /// `venue_change`. Releasing the kill also removes the kill marker.
    ClearHold {
        reason: HoldKind,
        note: String,
    },

    // ---- admin socket, sent by the watcher (C-HOLD) -----------------------
    /// A check of the watcher confirmed a mismatch: plain orders are held
    /// until the operator clears it.
    WatcherMismatch {
        check: WatcherCheck,
        detail: String,
    },
    /// The watcher killed: every order is held. Answered after the latch is
    /// durable.
    WatcherKill {
        detail: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ServerMsg {
    Response(Response),
    Event(EventMsg),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Response {
    pub id: u64,
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<ApiError>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiError {
    pub code: ErrorCode,
    pub message: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// The line was not a valid request.
    BadRequest,
    /// `hello` has not been sent, or names something not permitted.
    Unauthorized,
    /// The operation is not available on this socket.
    Forbidden,
    /// The input queue is full or the rate limit is exhausted. Nothing was
    /// recorded; the request may be repeated with the same key.
    Overloaded,
    /// The OMS is stopping.
    Unavailable,
    NotFound,
    Internal,
}

/// A durable journal record, as delivered to a subscriber.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventMsg {
    /// Journal sequence of the record. Increases; gaps are records about
    /// other strategies.
    pub seq: u64,
    pub at: Ts,
    pub order_id: OrderId,
    pub events: Vec<Event>,
}

impl Response {
    pub fn ok(id: u64, result: impl Serialize) -> Response {
        Response {
            id,
            ok: true,
            result: serde_json::to_value(result).ok(),
            error: None,
        }
    }
    pub fn err(id: u64, code: ErrorCode, message: impl Into<String>) -> Response {
        Response {
            id,
            ok: false,
            result: None,
            error: Some(ApiError {
                code,
                message: message.into(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The documented request shape parses, and a response keeps its shape.
    #[test]
    fn wire_examples() {
        let line = r#"{"id":7,"op":"new_order","client_order_id":"k1","instrument":"BTC","side":"buy","qty":"0.001","px":"70000","tif":"gtc","expires_at":1700000005000000}"#;
        let r: Request = serde_json::from_str(line).unwrap();
        assert_eq!(r.id, 7);
        match r.op {
            Op::NewOrder { qty, px, reduce_only, .. } => {
                assert_eq!(qty, Qty::parse("0.001").unwrap());
                assert_eq!(px, Px::parse("70000").unwrap());
                assert!(!reduce_only);
            }
            other => panic!("{other:?}"),
        }
        // Numbers are refused where decimal strings are required.
        let bad = line.replace(r#""qty":"0.001""#, r#""qty":0.001"#);
        assert!(serde_json::from_str::<Request>(&bad).is_err());

        let resp = Response::ok(7, NewOrderReply::Accepted { order_id: OrderId(3) });
        let json = serde_json::to_string(&ServerMsg::Response(resp.clone())).unwrap();
        assert_eq!(json, r#"{"id":7,"ok":true,"result":{"accepted":{"order_id":3}}}"#);
        assert_eq!(serde_json::from_str::<ServerMsg>(&json).unwrap(), ServerMsg::Response(resp));
    }

    /// U18b1. The operator's `clear_hold` and the watcher's two inputs, as
    /// the watcher (U19) writes them without this crate; a check outside
    /// W1..W4 or an unknown hold does not parse.
    #[test]
    fn wire_examples_of_the_holds() {
        let r: Request = serde_json::from_str(r#"{"id":1,"op":"watcher_mismatch","check":"W2","detail":"positions differ"}"#).unwrap();
        assert_eq!(
            r.op,
            Op::WatcherMismatch {
                check: WatcherCheck::W2,
                detail: "positions differ".into()
            }
        );
        let r: Request = serde_json::from_str(r#"{"id":2,"op":"watcher_kill","detail":"W5: the OMS is silent"}"#).unwrap();
        assert_eq!(r.op, Op::WatcherKill { detail: "W5: the OMS is silent".into() });
        let r: Request = serde_json::from_str(r#"{"id":3,"op":"clear_hold","reason":"venue_change","note":"ADL seen"}"#).unwrap();
        assert_eq!(
            r.op,
            Op::ClearHold {
                reason: HoldKind::VenueChange,
                note: "ADL seen".into()
            }
        );
        for bad in [
            r#"{"id":4,"op":"watcher_mismatch","check":"W5","detail":"d"}"#,
            r#"{"id":4,"op":"watcher_mismatch","check":"w1","detail":"d"}"#,
            r#"{"id":4,"op":"watcher_mismatch","detail":"d"}"#,
            r#"{"id":4,"op":"clear_hold","reason":"operator_account","note":"n"}"#,
            r#"{"id":4,"op":"watcher_kill"}"#,
        ] {
            assert!(serde_json::from_str::<Request>(bad).is_err(), "{bad}");
        }
    }

    /// U13c. A new order may carry the order book state it was decided on;
    /// a line without it parses as before, and a bad name is refused.
    #[test]
    fn a_new_order_line_may_carry_a_market_ref() {
        let line = r#"{"id":8,"op":"new_order","client_order_id":"k1","instrument":"BTC","side":"buy","qty":"0.001","px":"70000","tif":"gtc","expires_at":1700000005000000,"market_ref":{"venue":"bybit","market":"linear","symbol":"BTCUSDT","channel":"orderbook.50","update_id":42,"venue_ts_ns":1,"recv_ns":2}}"#;
        let r: Request = serde_json::from_str(line).unwrap();
        let Op::NewOrder { market_ref: Some(m), .. } = r.op else {
            panic!("{:?}", r.op)
        };
        assert_eq!((m.channel.as_str(), m.update_id), ("orderbook.50", Some(42)));
        let bad = line.replace(r#""symbol":"BTCUSDT""#, r#""symbol":"BTC USDT""#);
        assert!(serde_json::from_str::<Request>(&bad).is_err());
    }
}
