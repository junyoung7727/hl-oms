//! `http_fake::bybit`: each builder writes the JSON the tests wrote by
//! hand before it existed (the literals below are those tests' text).

#![allow(clippy::unwrap_used)]

use http_fake::Answer;
use http_fake::bybit::*;
use serde_json::{Value, json};

fn body(a: Answer) -> Value {
    match a {
        Answer::Reply(200, b) => serde_json::from_str(&b).unwrap(),
        other => panic!("{other:?}"),
    }
}

#[test]
fn ok_wraps_the_result_in_ret_code_0() {
    assert_eq!(
        body(ok(json!({"a": 1}))),
        json!({"retCode": 0, "retMsg": "OK", "result": {"a": 1}, "time": 1})
    );
}

#[test]
fn error_is_http_200_with_its_ret_code() {
    assert_eq!(
        body(error(10004, "error sign!")),
        json!({"retCode": 10004, "retMsg": "error sign!", "result": {}, "time": 1})
    );
}

#[test]
fn list_writes_the_cursor_as_next_page_cursor() {
    let rows = vec![json!({"x": 1})];
    assert_eq!(
        page(rows.clone(), "p50"),
        json!({"category": "linear", "list": [{"x": 1}], "nextPageCursor": "p50"})
    );
    assert_eq!(
        body(list(rows, "")),
        json!({"retCode": 0, "retMsg": "OK", "time": 1,
            "result": {"category": "linear", "list": [{"x": 1}], "nextPageCursor": ""}})
    );
}

#[test]
fn with_fields_adds_and_replaces_keys() {
    let r = with_fields(json!({"a": 1, "b": 2}), json!({"b": 3, "c": 4}));
    assert_eq!(r, json!({"a": 1, "b": 3, "c": 4}));
}

#[test]
fn the_rows_have_the_keys_the_old_helpers_wrote() {
    assert_eq!(
        order_row("l0", "o0", "BTCUSDT", "Buy", "1", "New"),
        json!({"orderLinkId": "l0", "orderId": "o0", "symbol": "BTCUSDT", "side": "Buy",
            "qty": "1", "orderStatus": "New"})
    );
    assert_eq!(
        position_row("BTCUSDT", "", "0"),
        json!({"symbol": "BTCUSDT", "positionIdx": 0, "size": "0", "side": ""})
    );
    let e = Execution {
        symbol: "BTCUSDT",
        exec_id: "e1",
        exec_type: "Trade",
        side: "Buy",
        qty: "0.001",
        price: "100",
        fee: "0",
        exec_time_ms: 1_700_000_000_000,
        order_link_id: "",
        order_id: "",
    };
    assert_eq!(
        execution_row(&e),
        json!({"symbol": "BTCUSDT", "execId": "e1", "execType": "Trade", "side": "Buy",
            "execQty": "0.001", "execPrice": "100", "execFee": "0",
            "execTime": "1700000000000", "orderLinkId": "", "orderId": ""})
    );
    assert_eq!(
        wallet_unified("10000", "10", "9000"),
        json!({"accountType": "UNIFIED", "totalEquity": "10000",
            "totalMaintenanceMargin": "10", "totalMarginBalance": "9000",
            "totalWalletBalance": "9000", "coin": []})
    );
    assert_eq!(
        query_api_key(
            4242,
            "1000",
            0,
            json!({"ContractTrade": ["Order", "Position"], "Spot": []}),
            &["127.0.0.1"]
        ),
        json!({"isMaster": false, "userID": 4242, "parentUid": "1000", "readOnly": 0,
            "ips": ["127.0.0.1"],
            "permissions": {"ContractTrade": ["Order", "Position"], "Spot": []}})
    );
    assert_eq!(
        instrument_row("BTCUSDT", "0.10", "0.001"),
        json!({"symbol": "BTCUSDT", "settleCoin": "USDT", "status": "Trading",
            "priceFilter": {"tickSize": "0.10"}, "lotSizeFilter": {"qtyStep": "0.001"}})
    );
    assert_eq!(
        market_time(4_000_000_000_000_000_000),
        json!({"timeSecond": "4000000000", "timeNano": "4000000000000000000"})
    );
    assert_eq!(
        cancel_all(&[("o0".into(), "".into())]),
        json!({"list": [{"orderId": "o0", "orderLinkId": ""}], "success": "1"})
    );
    assert_eq!(cancel_all(&[]), json!({"list": [], "success": "1"}));
}

/// Each argument reaches its key: values that no builder writes by itself.
#[test]
fn every_argument_reaches_its_key() {
    let r = order_row("l7", "o7", "ETHUSDT", "Sell", "0.002", "PartiallyFilled");
    assert_eq!(
        (&r["orderLinkId"], &r["orderId"], &r["symbol"], &r["side"]),
        (
            &json!("l7"),
            &json!("o7"),
            &json!("ETHUSDT"),
            &json!("Sell")
        )
    );
    assert_eq!(
        (&r["qty"], &r["orderStatus"]),
        (&json!("0.002"), &json!("PartiallyFilled"))
    );
    assert_eq!(
        cancel_all(&[("o7".into(), "l7".into()), ("o8".into(), "l8".into())]),
        json!({"list": [{"orderId": "o7", "orderLinkId": "l7"},
            {"orderId": "o8", "orderLinkId": "l8"}], "success": "1"})
    );
}
