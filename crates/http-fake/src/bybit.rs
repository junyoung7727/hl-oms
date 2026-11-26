//! Bybit V5 answers as the fakes write them: the envelope (`retCode`,
//! `retMsg`, `result`), a list page, and the rows of the queries both the
//! OMS and the watcher read. The shapes are the ones the tests wrote by
//! hand before; a caller adds the fields its test needs with
//! [`with_fields`]. No endpoint path is named here: the callers route.

use crate::Answer;
use serde_json::{Value, json};

/// `retCode` 0 around `result`, HTTP 200.
pub fn ok(result: Value) -> Answer {
    Answer::Reply(
        200,
        json!({"retCode": 0, "retMsg": "OK", "result": result, "time": 1}).to_string(),
    )
}

/// A refusal as Bybit writes it: HTTP 200, `retCode` `code`, an empty
/// `result`.
pub fn error(code: i64, msg: &str) -> Answer {
    Answer::Reply(
        200,
        json!({"retCode": code, "retMsg": msg, "result": {}, "time": 1}).to_string(),
    )
}

/// The `result` of a list query: `rows` and the cursor of the next page
/// (`""` on the last one).
pub fn page(rows: Vec<Value>, cursor: &str) -> Value {
    json!({"category": "linear", "list": rows, "nextPageCursor": cursor})
}

/// [`page`] in an [`ok`] envelope.
pub fn list(rows: Vec<Value>, cursor: &str) -> Answer {
    ok(page(rows, cursor))
}

/// `row` with the keys of `more` added or replaced. Both are objects.
pub fn with_fields(mut row: Value, more: Value) -> Value {
    if let (Some(r), Value::Object(m)) = (row.as_object_mut(), more) {
        r.extend(m);
    }
    row
}

/// An order row of `/v5/order/realtime` or `/history`: the keys every
/// reader needs. Prices, quantities left and times come through
/// [`with_fields`].
pub fn order_row(
    link: &str,
    order_id: &str,
    symbol: &str,
    side: &str,
    qty: &str,
    status: &str,
) -> Value {
    json!({"orderLinkId": link, "orderId": order_id, "symbol": symbol, "side": side,
        "qty": qty, "orderStatus": status})
}

/// A `/v5/position/list` row in one-way mode (`positionIdx` 0); `side`
/// `""` when flat.
pub fn position_row(symbol: &str, side: &str, size: &str) -> Value {
    json!({"symbol": symbol, "side": side, "size": size, "positionIdx": 0})
}

/// The fields of an `/v5/execution/list` row.
#[derive(Debug, Clone, Copy)]
pub struct Execution<'a> {
    pub symbol: &'a str,
    pub exec_id: &'a str,
    pub exec_type: &'a str,
    pub side: &'a str,
    pub qty: &'a str,
    pub price: &'a str,
    pub fee: &'a str,
    /// `execTime`, ms (written as a string, as Bybit does).
    pub exec_time_ms: u64,
    pub order_link_id: &'a str,
    pub order_id: &'a str,
}

/// An `/v5/execution/list` row.
pub fn execution_row(e: &Execution) -> Value {
    json!({"symbol": e.symbol, "execId": e.exec_id, "execType": e.exec_type, "side": e.side,
        "execQty": e.qty, "execPrice": e.price, "execFee": e.fee,
        "execTime": e.exec_time_ms.to_string(), "orderLinkId": e.order_link_id,
        "orderId": e.order_id})
}

/// A `/v5/account/wallet-balance` account row of type UNIFIED; the wallet
/// balance is the margin balance, and no coin rows.
pub fn wallet_unified(equity: &str, maintenance: &str, margin_balance: &str) -> Value {
    json!({"accountType": "UNIFIED", "totalEquity": equity,
        "totalMaintenanceMargin": maintenance, "totalMarginBalance": margin_balance,
        "totalWalletBalance": margin_balance, "coin": []})
}

/// The `result` of `/v5/user/query-api` for a sub-account key
/// (`isMaster` false).
pub fn query_api_key(
    user_id: u64,
    parent_uid: &str,
    read_only: u8,
    permissions: Value,
    ips: &[&str],
) -> Value {
    json!({"isMaster": false, "userID": user_id, "parentUid": parent_uid,
        "readOnly": read_only, "ips": ips, "permissions": permissions})
}

/// A linear USDT instrument row of `/v5/market/instruments-info`, trading.
pub fn instrument_row(symbol: &str, tick: &str, step: &str) -> Value {
    json!({"symbol": symbol, "settleCoin": "USDT", "status": "Trading",
        "priceFilter": {"tickSize": tick}, "lotSizeFilter": {"qtyStep": step}})
}

/// The `result` of `/v5/market/time` at `time_ns` (ns since the epoch).
pub fn market_time(time_ns: u64) -> Value {
    json!({"timeSecond": (time_ns / 1_000_000_000).to_string(), "timeNano": time_ns.to_string()})
}

/// The `result` of a cancel-all: the cancelled `(orderId, orderLinkId)`
/// pairs and `success` "1".
pub fn cancel_all(cancelled: &[(String, String)]) -> Value {
    let list: Vec<Value> = cancelled
        .iter()
        .map(|(id, link)| json!({"orderId": id, "orderLinkId": link}))
        .collect();
    json!({"list": list, "success": "1"})
}
