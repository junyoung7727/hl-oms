//! The private stream's messages (ADR-028 §7c, U16d): what the gateway
//! sends (auth, subscribe, ping) and what it reads (F20 Q2, F18 §1).
//!
//! Two topics, `order.linear` and `execution.linear`. Their rows become
//! venue inputs through the same functions as the queries: the status
//! table of `query::order_status` and the execution table `exec::classify`.
//! `position` and `wallet` are not subscribed: a push names only the
//! symbols that changed, and the core takes positions and the margin report
//! only as whole snapshots, which the queries read.

use oms_core::*;
use serde_json::Value;

use super::exec::{self, text};
use super::query::order_status;
use super::rest::Credentials;

/// The subscribed topics. Category topics: linear only (an All-In-One
/// topic would also carry spot and options rows, F18 §1).
pub const TOPICS: [&str; 2] = ["order.linear", "execution.linear"];

/// The `auth` message (F20 Q2.1): `GET/realtime` + `expires`, signed
/// (HMAC hex or RSA base64, ADR-034). `Err` when the signature cannot be
/// made: the message is not sent.
pub fn auth_message(creds: &Credentials, expires_ms: u64) -> Result<String, String> {
    let sig = creds.ws_auth_signature(expires_ms)?;
    Ok(serde_json::json!({
        "req_id": "auth",
        "op": "auth",
        "args": [creds.api_key, expires_ms, sig],
    })
    .to_string())
}

pub fn subscribe_message() -> String {
    serde_json::json!({"req_id": "subscribe", "op": "subscribe", "args": TOPICS}).to_string()
}

pub fn ping_message(n: u64) -> String {
    serde_json::json!({"req_id": format!("ping-{n}"), "op": "ping"}).to_string()
}

/// One text frame of the private stream.
#[derive(Debug, PartialEq)]
pub enum Frame {
    /// The answer to `auth`. `Err` holds what the venue said: the failure's
    /// shape is not documented (F20 Q2.6), so anything other than
    /// `success: true` is a failure.
    Auth(Result<(), String>),
    /// The answer to `subscribe`, read the same way (F20 Q2.15).
    Subscribe(Result<(), String>),
    /// A pong in either documented shape: `op = "pong"` (private), or
    /// `op = "ping"` with `ret_msg = "pong"` (F20 Q2.9–2.10).
    Pong,
    Orders(Vec<Value>),
    Executions(Vec<Value>),
    /// Another topic or operation. Nothing is read from it.
    Other,
}

/// `Ok` only for `success: true` (F20 Q2.15); `Err` holds what the venue said.
pub(super) fn outcome(v: &Value) -> Result<(), String> {
    if v.get("success").and_then(Value::as_bool) == Some(true) {
        return Ok(());
    }
    let msg: String = v
        .get("ret_msg")
        .and_then(Value::as_str)
        .unwrap_or("")
        .chars()
        .take(200)
        .collect();
    Err(format!("success {:?}, ret_msg {msg:?}", v.get("success")))
}

/// Reads one text frame. `Err` if it is not JSON, or a topic message has no
/// `data` list: the stream cannot be trusted to be complete after it.
pub fn parse(text: &str) -> Result<Frame, String> {
    let v: Value = serde_json::from_str(text).map_err(|e| format!("not JSON: {e}"))?;
    if let Some(topic) = v.get("topic").and_then(Value::as_str) {
        let rows = || -> Result<Vec<Value>, String> {
            v.get("data")
                .and_then(Value::as_array)
                .cloned()
                .ok_or_else(|| format!("topic {topic}: no data list"))
        };
        return match topic {
            "order.linear" => Ok(Frame::Orders(rows()?)),
            "execution.linear" => Ok(Frame::Executions(rows()?)),
            _ => Ok(Frame::Other),
        };
    }
    let op = v.get("op").and_then(Value::as_str);
    let ret_msg = v.get("ret_msg").and_then(Value::as_str);
    Ok(match op {
        Some("auth") => Frame::Auth(outcome(&v)),
        Some("subscribe") => Frame::Subscribe(outcome(&v)),
        Some("pong") => Frame::Pong,
        Some("ping") if ret_msg == Some("pong") => Frame::Pong,
        _ => Frame::Other,
    })
}

/// The symbol of a row, if the row is about a traded instrument. Rows of
/// other symbols are left to the queries, which are scoped to the kill
/// scope (`settleCoin=USDT`); the stream's category topic is not.
pub fn symbol(row: &Value) -> Option<&str> {
    row.get("symbol").and_then(Value::as_str)
}

/// One `order` row as the core's input. `Ok(None)`: a status the OMS never
/// produces (conditional or spot), nothing concluded. `Err`: the row cannot
/// be read, and the stream must start over.
pub fn order_input(
    row: &Value,
    session: u64,
    instance_id: u64,
) -> Result<Option<VenueInput>, String> {
    let Some(status) = order_status(row)? else {
        return Ok(None);
    };
    Ok(Some(VenueInput::OrderStatus(OrderStatusIn {
        session,
        order_id: exec::our_order(text(row, "orderLinkId").unwrap_or(""), instance_id),
        venue_oid: exec::venue_oid(row),
        status,
        source: EvidenceSource::Stream,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_sent_messages_have_the_documented_shapes() {
        let creds = Credentials::new("KEY", "SECRET");
        let auth: Value =
            serde_json::from_str(&auth_message(&creds, 1_662_350_400_000).unwrap()).unwrap();
        assert_eq!(auth["op"], "auth");
        assert_eq!(auth["args"][0], "KEY");
        assert_eq!(auth["args"][1], 1_662_350_400_000u64);
        assert_eq!(
            auth["args"][2],
            super::super::sign::ws_auth_signature("SECRET", 1_662_350_400_000)
        );
        let sub: Value = serde_json::from_str(&subscribe_message()).unwrap();
        assert_eq!(sub["op"], "subscribe");
        assert_eq!(sub["args"], json!(["order.linear", "execution.linear"]));
        let ping: Value = serde_json::from_str(&ping_message(3)).unwrap();
        assert_eq!(ping["op"], "ping");
    }

    /// RSA (ADR-034, spec T3, F2): the `auth` message signed with an RSA
    /// key carries the base64 signature of `GET/realtime{expires}`:
    /// openssl's value (`printf '%s' GET/realtime1662350400000 | openssl dgst
    /// -sha256 -sign key.pem | base64 -w0`, the test-only key), not hex.
    #[test]
    fn the_auth_message_with_an_rsa_key_carries_the_base64_signature() {
        let pem = crate::venue::bybit::sign::test_keys::pem_2048();
        let creds = super::super::rest::Credentials::rsa("KEY", &pem).unwrap();
        let m: Value =
            serde_json::from_str(&auth_message(&creds, 1_662_350_400_000).unwrap()).unwrap();
        assert_eq!(
            m,
            json!({"req_id": "auth", "op": "auth", "args": ["KEY", 1_662_350_400_000u64,
                "HZKuzRsZu3GyRpRRU9GK/xpL4DUyjo4bd+uRkRedWmZwUUr1ye+RvVNtV+LpPmx5X+1iKtlWIJGe1FNJ/jW/A0q4e2ND7FcQy9zEgbRZwVO+sbUqoN0m69zQ+mhqR5VBKXTFIcbEhzFiQKFHSC/hK5OmdSX2aCU0e0ROiA5o5p5Vcvs6jF4U1G2EF4bq5LX5BPufvpLsI4HoqBcX5Oq/gIjTxAORH2YNwzeuGh1migMEAxTpp2lRXc27qaBGhbPmYURG3QvCguBp5ctIiP2T6RTeiPgfBSYxSZZESMOs/oFVhjeRJ226Nrj7tUqcA9Gn1P4B4Y4SpIzAJ0tWLizZWg=="]})
        );
    }

    #[test]
    fn frames_are_read_by_their_documented_fields() {
        // F20 Q2.5, Q2.9, Q2.10.
        assert_eq!(
            parse(r#"{"success":true,"ret_msg":"","op":"auth","conn_id":"c"}"#).unwrap(),
            Frame::Auth(Ok(()))
        );
        assert!(matches!(
            parse(r#"{"success":false,"ret_msg":"error sign","op":"auth"}"#).unwrap(),
            Frame::Auth(Err(m)) if m.contains("error sign")
        ));
        assert!(matches!(
            parse(r#"{"op":"auth","ret_msg":"?"}"#).unwrap(),
            Frame::Auth(Err(_))
        ));
        assert_eq!(
            parse(r#"{"success":true,"ret_msg":"","op":"subscribe","conn_id":"c"}"#).unwrap(),
            Frame::Subscribe(Ok(()))
        );
        assert!(matches!(
            parse(r#"{"success":false,"ret_msg":"bad topic","op":"subscribe"}"#).unwrap(),
            Frame::Subscribe(Err(_))
        ));
        assert_eq!(
            parse(r#"{"req_id":"test","op":"pong","args":["1675418560633"],"conn_id":"c"}"#)
                .unwrap(),
            Frame::Pong
        );
        assert_eq!(
            parse(r#"{"success":true,"ret_msg":"pong","conn_id":"c","req_id":"","op":"ping"}"#)
                .unwrap(),
            Frame::Pong
        );
        assert_eq!(parse(r#"{"op":"ping"}"#).unwrap(), Frame::Other);
        assert_eq!(
            parse(r#"{"id":"1","topic":"order.linear","creationTime":1,"data":[{"a":1}]}"#)
                .unwrap(),
            Frame::Orders(vec![json!({"a":1})])
        );
        assert_eq!(
            parse(r#"{"id":"1","topic":"execution.linear","creationTime":1,"data":[]}"#).unwrap(),
            Frame::Executions(vec![])
        );
        assert_eq!(
            parse(r#"{"topic":"wallet","data":[]}"#).unwrap(),
            Frame::Other
        );
        assert!(parse(r#"{"topic":"order.linear","data":{}}"#).is_err());
        assert!(parse("not json").is_err());
    }

    fn order_row(link: &str, status: &str) -> Value {
        json!({
            "category": "linear", "symbol": "BTCUSDT", "orderId": "o-1",
            "orderLinkId": link, "side": "Buy", "orderStatus": status,
            "cancelType": "UNKNOWN", "rejectReason": "EC_NoError",
            "cumExecQty": "0.002", "leavesQty": "0", "updatedTime": "1700000000123"
        })
    }

    #[test]
    fn an_order_row_becomes_a_stream_status_of_our_order_or_of_a_foreign_one() {
        let ours = exec::WIRE_ID.encode(VenueClientId {
            instance_id: 7,
            order_id: OrderId(42),
        });
        let Some(VenueInput::OrderStatus(s)) =
            order_input(&order_row(&ours, "Filled"), 9, 7).unwrap()
        else {
            panic!("no status");
        };
        assert_eq!(s.order_id, Some(OrderId(42)));
        assert_eq!(s.session, 9);
        assert_eq!(s.source, EvidenceSource::Stream);
        assert_eq!(s.venue_oid, VenueOrderId::new("o-1").ok());
        assert_eq!(s.status.status, VenueStatus::Filled);
        assert_eq!(s.status.cum, Some(Qty::parse("0.002").unwrap()));
        assert_eq!(s.status.venue_ts, 1_700_000_000_123_000);

        let Some(VenueInput::OrderStatus(s)) =
            order_input(&order_row("someone-else", "New"), 9, 7).unwrap()
        else {
            panic!("no status");
        };
        assert_eq!(s.order_id, None, "a foreign orderLinkId is not ours");
        assert_eq!(
            order_input(&order_row(&ours, "Untriggered"), 9, 7).unwrap(),
            None
        );
        let mut bad = order_row(&ours, "New");
        bad["cumExecQty"] = json!("x");
        assert!(order_input(&bad, 9, 7).is_err());
    }
}
