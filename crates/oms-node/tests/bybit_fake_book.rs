//! The stateful fake Bybit (`bybit_fake::book`, `bybit_fake::venue`) on
//! its own: each input's transitions at fixed instants, the queries both
//! programs read, the signature check, the frames read by the OMS's own
//! parsers, and the key sets of the recorded testnet answers. Test
//! support: nothing here is evidence of how Bybit behaves.

#![allow(clippy::unwrap_used)]

mod bybit_fake;

use bybit_fake::book::{
    Book, DUPLICATE, NOT_OPEN, OKEY, OSECRET, SYMBOL, UID, WKEY, WSECRET, two_keys,
};
use bybit_fake::venue::FakeBybit;
use http_fake::{Answer, Request};
use oms_core::*;
use oms_node::venue::bybit::{exec, rest, sign, stream, ticker};
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;

const T0: u64 = 1_800_000_000_000;

/// A request signed as both programs sign: `X-BAPI-*` over the query of a
/// GET or the body of a POST.
fn signed(method: &str, target: &str, body: &str, key: &str, secret: &str) -> Request {
    let payload = if method == "GET" {
        target.split_once('?').map_or("", |(_, q)| q)
    } else {
        body
    };
    let sig = sign::rest_signature(secret, T0, key, 5000, payload);
    Request {
        method: method.into(),
        target: target.into(),
        headers: vec![
            ("x-bapi-api-key".into(), key.into()),
            ("x-bapi-timestamp".into(), T0.to_string()),
            ("x-bapi-recv-window".into(), "5000".into()),
            ("x-bapi-sign".into(), sig),
        ],
        body: body.as_bytes().to_vec(),
    }
}

/// `GET target` from `key` at `now`: the envelope as JSON.
fn get(b: &mut Book, now: u64, key: &str, target: &str) -> Value {
    let secret = if key == OKEY { OSECRET } else { WSECRET };
    match b.http(now, &signed("GET", target, "", key, secret)).0 {
        Answer::Reply(_, body) => serde_json::from_str(&body).unwrap(),
        a => panic!("{a:?}"),
    }
}

fn post(b: &mut Book, now: u64, key: &str, path: &str, body: Value) -> (Value, Vec<String>) {
    let secret = if key == OKEY { OSECRET } else { WSECRET };
    let (a, frames) = b.http(now, &signed("POST", path, &body.to_string(), key, secret));
    let Answer::Reply(_, text) = a else {
        panic!("{a:?}")
    };
    (serde_json::from_str(&text).unwrap(), frames)
}

fn rows(v: &Value) -> Vec<Value> {
    assert_eq!(v["retCode"], 0, "{v}");
    v["result"]["list"].as_array().unwrap().clone()
}

fn create_msg(link: &str, side: &str, qty: &str, price: &str) -> Value {
    json!({"reqId": format!("c{link}"), "op": "order.create",
        "header": {"X-BAPI-TIMESTAMP": T0.to_string(), "X-BAPI-RECV-WINDOW": "5000"},
        "args": [{"category": "linear", "symbol": SYMBOL, "side": side, "orderType": "Limit",
            "qty": qty, "price": price, "timeInForce": "PostOnly", "orderLinkId": link,
            "reduceOnly": false, "positionIdx": 0}]})
}

/// A create over WS Trade from the OMS's key: the new `orderId`.
fn create(b: &mut Book, now: u64, link: &str, side: &str, qty: &str, price: &str) -> String {
    let (reply, _) = b.ws_trade(now, OKEY, &create_msg(link, side, qty, price));
    let reply = reply.unwrap();
    assert_eq!(reply["retCode"], 0, "{reply}");
    reply["data"]["orderId"].as_str().unwrap().to_string()
}

fn book() -> Book {
    Book::new(two_keys())
}

fn frame_rows(frames: &[String], topic: &str) -> Vec<Value> {
    frames
        .iter()
        .map(|f| serde_json::from_str::<Value>(f).unwrap())
        .filter(|v| v["topic"] == topic)
        .flat_map(|v| v["data"].as_array().unwrap().clone())
        .collect()
}

#[test]
fn a_ws_create_books_a_new_order_replies_placed_and_makes_its_order_frame() {
    let mut b = book();
    let (reply, frames) = b.ws_trade(T0, OKEY, &create_msg("L1", "Buy", "0.002", "30000"));
    let reply = reply.unwrap();
    assert_eq!(reply["reqId"], "cL1");
    assert_eq!(reply["retCode"], 0);
    assert_eq!(reply["op"], "order.create");
    assert_eq!(
        reply["data"],
        json!({"orderId": "o-1", "orderLinkId": "L1"})
    );
    let o = b.by_link("L1").unwrap();
    assert_eq!((o.status, o.qty, o.price, o.cum), ("New", 2, 300_000, 0));
    assert_eq!((o.created_ms, o.key.as_str()), (T0, OKEY));
    let rows = frame_rows(&frames, "order.linear");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["orderStatus"], "New");
    assert_eq!(rows[0]["leavesQty"], "0.002");
    assert_eq!(b.creates_for("L1"), 1);
}

#[test]
fn a_create_whose_order_link_id_is_at_the_venue_is_refused_and_logged() {
    let mut b = book();
    create(&mut b, T0, "L1", "Buy", "0.001", "30000");
    let (reply, frames) = b.ws_trade(T0 + 5, OKEY, &create_msg("L1", "Buy", "0.001", "30000"));
    assert_eq!(reply.unwrap()["retCode"], DUPLICATE);
    assert!(frames.is_empty());
    let (r, _) = post(
        &mut b,
        T0 + 6,
        OKEY,
        "/v5/order/create",
        create_msg("L1", "Buy", "0.001", "30000")["args"][0].clone(),
    );
    assert_eq!(r["retCode"], DUPLICATE);
    assert_eq!(b.orders.len(), 1);
    // Over WS Trade and REST together.
    assert_eq!(b.creates_for("L1"), 3);
}

#[test]
fn a_cancel_by_order_link_id_closes_the_order_over_ws_and_rest() {
    let mut b = book();
    create(&mut b, T0, "L1", "Buy", "0.001", "30000");
    create(&mut b, T0, "L2", "Buy", "0.001", "30000");
    let msg = json!({"reqId": "x0L1", "op": "order.cancel",
        "args": [{"category": "linear", "symbol": SYMBOL, "orderLinkId": "L1"}]});
    let (reply, frames) = b.ws_trade(T0 + 10, OKEY, &msg);
    assert_eq!(reply.unwrap()["retCode"], 0);
    assert_eq!(
        frame_rows(&frames, "order.linear")[0]["orderStatus"],
        "Cancelled"
    );
    let o = b.by_link("L1").unwrap();
    assert_eq!((o.status, o.updated_ms), ("Cancelled", T0 + 10));
    let (again, _) = b.ws_trade(T0 + 11, OKEY, &msg);
    assert_eq!(again.unwrap()["retCode"], NOT_OPEN);
    let (r, _) = post(
        &mut b,
        T0 + 12,
        OKEY,
        "/v5/order/cancel",
        json!({"category": "linear", "symbol": SYMBOL, "orderLinkId": "L2"}),
    );
    assert_eq!(r["retCode"], 0);
    assert!(b.open_orders().is_empty());
}

#[test]
fn realtime_by_order_link_id_lists_only_that_order() {
    let mut b = book();
    create(&mut b, T0, "L1", "Buy", "0.001", "30000");
    create(&mut b, T0, "L2", "Sell", "0.001", "31000");
    b.plant_order(T0, "manual-1", "Buy", "0.001", "29000");
    let v = get(
        &mut b,
        T0,
        OKEY,
        "/v5/order/realtime?category=linear&orderLinkId=L2&settleCoin=USDT",
    );
    let r = rows(&v);
    assert_eq!(r.len(), 1, "a status lookup must see only its order: {v}");
    assert_eq!(r[0]["orderLinkId"], "L2");
    let v = get(
        &mut b,
        T0,
        WKEY,
        "/v5/order/realtime?category=linear&orderId=o-3&settleCoin=USDT",
    );
    assert_eq!(rows(&v)[0]["orderLinkId"], "manual-1");
    let all = get(
        &mut b,
        T0,
        WKEY,
        "/v5/order/realtime?category=linear&settleCoin=USDT&limit=50",
    );
    assert_eq!(rows(&all).len(), 3);
    let none = get(&mut b, T0, WKEY, "/v5/order/realtime?category=linear");
    assert_eq!(none["retCode"], 10001);
}

#[test]
fn realtime_pages_with_a_cursor_of_allowed_characters_and_a_full_page_has_one() {
    let mut b = book();
    for i in 0..5 {
        create(&mut b, T0 + i, &format!("L{i}"), "Buy", "0.001", "30000");
    }
    let base = "/v5/order/realtime?category=linear&settleCoin=USDT&limit=2";
    let mut seen = Vec::new();
    let mut cursor = String::new();
    for _ in 0..4 {
        let target = if cursor.is_empty() {
            base.to_string()
        } else {
            format!("{base}&cursor={cursor}")
        };
        let v = get(&mut b, T0, WKEY, &target);
        seen.extend(
            rows(&v)
                .iter()
                .map(|r| r["orderLinkId"].as_str().unwrap().to_string()),
        );
        cursor = v["result"]["nextPageCursor"].as_str().unwrap().to_string();
        assert!(
            sign::query_string(&[("cursor", cursor.clone())]).is_ok(),
            "{cursor}"
        );
        if cursor.is_empty() {
            break;
        }
    }
    // Newest first, each order once.
    assert_eq!(seen, ["L4", "L3", "L2", "L1", "L0"]);
    // Exactly full: a cursor, then an empty last page.
    let v = get(
        &mut b,
        T0,
        WKEY,
        "/v5/order/realtime?category=linear&settleCoin=USDT&limit=5",
    );
    assert_eq!(v["result"]["nextPageCursor"], "p5");
    let v = get(
        &mut b,
        T0,
        WKEY,
        "/v5/order/realtime?category=linear&settleCoin=USDT&limit=5&cursor=p5",
    );
    assert!(rows(&v).is_empty());
    assert_eq!(v["result"]["nextPageCursor"], "");
    assert_eq!(
        get(&mut b, T0, WKEY, &base.replace("limit=2", "limit=51"))["retCode"],
        10001
    );
}

#[test]
fn history_lists_closed_orders_in_its_window() {
    let mut b = book();
    let a = create(&mut b, T0, "L1", "Buy", "0.001", "30000");
    create(&mut b, T0 + 1000, "L2", "Buy", "0.001", "30000");
    b.cancel_at_venue(T0 + 2000, &a);
    let h = |b: &mut Book, q: &str| {
        get(
            b,
            T0 + 3000,
            OKEY,
            &format!("/v5/order/history?category=linear&settleCoin=USDT{q}"),
        )
    };
    let v = h(&mut b, "&orderLinkId=L1");
    assert_eq!(rows(&v).len(), 1);
    assert_eq!(rows(&v)[0]["orderStatus"], "Cancelled");
    assert!(
        rows(&h(&mut b, "&orderLinkId=L2")).is_empty(),
        "an open order is not in the history"
    );
    let since = format!("&orderLinkId=L1&startTime={}&endTime={}", T0 + 1, T0 + 3000);
    assert!(
        rows(&h(&mut b, &since)).is_empty(),
        "created before the window"
    );
    let wide = format!("&startTime={T0}&endTime={}", T0 + 8 * 24 * 3600 * 1000);
    assert_eq!(h(&mut b, &wide)["retCode"], 10001);
    let o = get(
        &mut b,
        T0 + 3000,
        OKEY,
        "/v5/order/realtime?category=linear&settleCoin=USDT&orderLinkId=L1",
    );
    assert!(rows(&o).is_empty(), "a closed order is not open");
}

#[test]
fn cancel_all_cancels_every_open_order_whoever_created_it() {
    let mut b = book();
    create(&mut b, T0, "L1", "Buy", "0.001", "30000");
    let f = b.plant_order(T0 + 1, "manual-1", "Sell", "0.001", "31000");
    let body = json!({"category": "linear", "settleCoin": "USDT"});
    let (r, frames) = post(&mut b, T0 + 5, WKEY, "/v5/order/cancel-all", body);
    assert_eq!(r["retCode"], 0);
    assert_eq!(r["result"]["success"], "1");
    let list = r["result"]["list"].as_array().unwrap();
    assert_eq!(
        list.len(),
        2,
        "the watcher's key cancels the OMS's order and a planted one: {r}"
    );
    assert!(b.open_orders().is_empty());
    assert_eq!(b.order(&f).unwrap().status, "Cancelled");
    assert_eq!(frame_rows(&frames, "order.linear").len(), 2);
    assert_eq!(b.cancel_alls(WKEY), [T0 + 5]);
    assert!(b.cancel_alls(OKEY).is_empty());
}

#[test]
fn leave_one_once_leaves_the_newest_order_after_that_keys_first_cancel_all_only() {
    let mut b = book();
    create(&mut b, T0, "L1", "Buy", "0.001", "30000");
    create(&mut b, T0 + 1, "L2", "Buy", "0.001", "30000");
    b.leave_one_once(WKEY);
    let body = json!({"category": "linear", "settleCoin": "USDT"});
    let (r, _) = post(&mut b, T0 + 5, OKEY, "/v5/order/cancel-all", body.clone());
    assert_eq!(
        r["result"]["list"].as_array().unwrap().len(),
        2,
        "another key's cancel-all is whole"
    );
    create(&mut b, T0 + 6, "L3", "Buy", "0.001", "30000");
    create(&mut b, T0 + 7, "L4", "Buy", "0.001", "30000");
    let (r, _) = post(&mut b, T0 + 8, WKEY, "/v5/order/cancel-all", body.clone());
    assert_eq!(
        r["result"]["list"],
        json!([{"orderId": "o-3", "orderLinkId": "L3"}])
    );
    assert_eq!(
        b.open_orders()
            .iter()
            .map(|o| o.link.as_str())
            .collect::<Vec<_>>(),
        ["L4"]
    );
    let (r, _) = post(&mut b, T0 + 9, WKEY, "/v5/order/cancel-all", body);
    assert_eq!(r["result"]["list"].as_array().unwrap().len(), 1);
    assert!(b.open_orders().is_empty());
}

#[test]
fn a_cancel_all_with_another_body_is_refused() {
    let mut b = book();
    create(&mut b, T0, "L1", "Buy", "0.001", "30000");
    let (r, _) = post(
        &mut b,
        T0,
        WKEY,
        "/v5/order/cancel-all",
        json!({"category": "linear", "symbol": SYMBOL}),
    );
    assert_eq!(r["retCode"], 10001);
    assert_eq!(b.open_orders().len(), 1);
}

fn exec_list(b: &mut Book, now: u64, q: &str) -> Value {
    get(
        b,
        now,
        WKEY,
        &format!("/v5/execution/list?category=linear&settleCoin=USDT&limit=100{q}"),
    )
}

#[test]
fn the_execution_list_filters_on_exec_time_inclusive() {
    let mut b = book();
    let ex =
        |b: &mut Book, at: u64| b.plant_execution("Trade", "Buy", "0.001", "30000", "manual", at);
    let (e1, e2, e3) = (
        ex(&mut b, T0 - 3000),
        ex(&mut b, T0 - 2000),
        ex(&mut b, T0 - 1000),
    );
    let v = exec_list(
        &mut b,
        T0,
        &format!("&startTime={}&endTime={}", T0 - 2000, T0 - 1000),
    );
    let ids: Vec<Value> = rows(&v).iter().map(|r| r["execId"].clone()).collect();
    assert_eq!(
        ids,
        [json!(e3), json!(e2)],
        "newest first, both ends inclusive, {e1} outside"
    );
    let v = exec_list(&mut b, T0, &format!("&startTime={}", T0 - 1500));
    assert_eq!(rows(&v).len(), 1);
    let v = exec_list(
        &mut b,
        T0,
        &format!("&startTime={}&endTime={}", T0 - 3000, T0 - 2500),
    );
    assert_eq!(rows(&v).len(), 1);
}

#[test]
fn an_execution_window_over_seven_days_is_refused() {
    let mut b = book();
    let week = 7 * 24 * 3600 * 1000;
    let ok = exec_list(
        &mut b,
        T0,
        &format!("&startTime={}&endTime={T0}", T0 - week),
    );
    assert_eq!(ok["retCode"], 0);
    let wide = exec_list(
        &mut b,
        T0,
        &format!("&startTime={}&endTime={T0}", T0 - week - 1),
    );
    assert_eq!(wide["retCode"], 10001);
}

#[test]
fn the_execution_list_pages_with_cursors() {
    let mut b = book();
    for i in 0..5 {
        b.plant_execution("Trade", "Buy", "0.001", "30000", "m", T0 - 5000 + i);
    }
    let q = |c: &str| {
        format!("&startTime={}&endTime={T0}{c}", T0 - 6000).replace("limit=100", "limit=2")
    };
    let first = get(
        &mut b,
        T0,
        WKEY,
        &format!(
            "/v5/execution/list?category=linear&settleCoin=USDT&limit=2{}",
            q("")
        ),
    );
    assert_eq!(rows(&first).len(), 2);
    let c = first["result"]["nextPageCursor"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(c, "e2");
    let mut all = rows(&first);
    let mut cursor = c;
    for _ in 0..3 {
        let v = get(
            &mut b,
            T0,
            WKEY,
            &format!(
                "/v5/execution/list?category=linear&settleCoin=USDT&limit=2{}",
                q(&format!("&cursor={cursor}"))
            ),
        );
        all.extend(rows(&v));
        cursor = v["result"]["nextPageCursor"].as_str().unwrap().to_string();
        if cursor.is_empty() {
            break;
        }
    }
    assert_eq!(all.len(), 5);
    assert_eq!(all[0]["execTime"], (T0 - 4996).to_string());
}

#[test]
fn partial_fills_move_the_position_by_the_filled_quantity() {
    let mut b = book();
    let a = create(&mut b, T0, "L1", "Buy", "0.003", "30000");
    let frames = b.fill(T0 + 100, &a, "0.001", "30000");
    assert_eq!(b.size, 1);
    assert_eq!(b.order(&a).unwrap().status, "PartiallyFilled");
    assert_eq!(
        frame_rows(&frames, "execution.linear")[0]["execQty"],
        "0.001"
    );
    assert_eq!(frame_rows(&frames, "order.linear")[0]["leavesQty"], "0.002");
    b.fill(T0 + 200, &a, "0.002", "30010");
    assert_eq!(b.size, 3);
    assert_eq!(b.order(&a).unwrap().status, "Filled");
    let p = &rows(&get(
        &mut b,
        T0 + 300,
        OKEY,
        "/v5/position/list?category=linear&symbol=BTCUSDT",
    ))[0];
    assert_eq!(
        (p["side"].as_str(), p["size"].as_str()),
        (Some("Buy"), Some("0.003"))
    );
    assert_eq!(p["avgPrice"], "30006.7");
    assert_eq!(p["updatedTime"], (T0 + 200).to_string());
    // A sell reduces it; through zero, the rest opens short.
    let s = create(&mut b, T0 + 400, "L2", "Sell", "0.005", "30100");
    b.fill(T0 + 500, &s, "0.005", "30100");
    assert_eq!(b.size, -2);
    let p = &rows(&get(
        &mut b,
        T0 + 600,
        OKEY,
        "/v5/position/list?category=linear&symbol=BTCUSDT",
    ))[0];
    assert_eq!(
        (
            p["side"].as_str(),
            p["size"].as_str(),
            p["avgPrice"].as_str()
        ),
        (Some("Sell"), Some("0.002"), Some("30100.0"))
    );
}

#[test]
fn the_settle_coin_list_has_no_flat_rows_and_the_symbol_list_has_one() {
    let mut b = book();
    let by_coin = "/v5/position/list?category=linear&settleCoin=USDT&limit=200";
    assert!(
        rows(&get(&mut b, T0, WKEY, by_coin)).is_empty(),
        "flat: no row by settleCoin"
    );
    let flat = rows(&get(
        &mut b,
        T0,
        OKEY,
        "/v5/position/list?category=linear&symbol=BTCUSDT",
    ));
    assert_eq!(flat.len(), 1);
    assert_eq!(
        (flat[0]["side"].as_str(), flat[0]["size"].as_str()),
        (Some(""), Some("0.000"))
    );
    assert_eq!(flat[0]["positionIdx"], 0);
    assert_eq!(flat[0]["liqPrice"], "");
    b.plant_execution("Trade", "Sell", "0.001", "30000", "manual", T0 - 2500);
    let short = rows(&get(&mut b, T0, WKEY, by_coin));
    assert_eq!(short.len(), 1);
    assert_eq!(
        (short[0]["side"].as_str(), short[0]["size"].as_str()),
        (Some("Sell"), Some("0.001"))
    );
    assert_eq!(
        short[0]["updatedTime"],
        (T0 - 2500).to_string(),
        "a planted row dates the position"
    );
}

#[test]
fn planted_rows_are_on_rest_and_never_pushed_and_a_venue_cancel_returns_its_frame() {
    let mut b = book();
    let f = b.plant_order(T0, "manual-1", "Buy", "0.001", "29000");
    let e = b.plant_execution("Funding", "Buy", "0.001", "30000", "", T0 - 10);
    assert_eq!(b.size, 0, "funding moves no position");
    let v = exec_list(&mut b, T0, "");
    assert_eq!(rows(&v)[0]["execId"], e.as_str());
    assert_eq!(rows(&v)[0]["execType"], "Funding");
    let frames = b.cancel_at_venue(T0 + 1, &f);
    assert_eq!(
        frame_rows(&frames, "order.linear")[0]["orderLinkId"],
        "manual-1"
    );
    assert_eq!(b.order(&f).unwrap().status, "Cancelled");
}

#[test]
fn a_bad_signature_is_refused_with_10004_and_logged() {
    let mut b = book();
    let target = "/v5/order/realtime?category=linear&settleCoin=USDT";
    let bad = signed("GET", target, "", WKEY, OSECRET);
    let (a, _) = b.http(T0, &bad);
    let Answer::Reply(200, text) = a else {
        panic!()
    };
    assert_eq!(
        serde_json::from_str::<Value>(&text).unwrap()["retCode"],
        10004
    );
    let mut tampered = signed("GET", target, "", WKEY, WSECRET);
    tampered.target.push_str("&limit=1");
    assert_eq!(
        b.http(T0, &tampered).0,
        http_fake::bybit::error(10004, "error sign!")
    );
    let unknown = signed("GET", target, "", "XKEY", WSECRET);
    assert_eq!(
        b.http(T0, &unknown).0,
        http_fake::bybit::error(10003, "API key is invalid.")
    );
    assert_eq!(get(&mut b, T0, WKEY, target)["retCode"], 0);
    let answers: Vec<&str> = b.log.iter().map(|l| l.answer.as_str()).collect();
    assert_eq!(answers, ["ret 10004", "ret 10004", "ret 10003", "ok"]);
    assert_eq!(
        (
            b.log[0].key.as_str(),
            b.log[0].method.as_str(),
            b.log[0].path.as_str()
        ),
        (WKEY, "GET", "/v5/order/realtime")
    );
    // A POST signs its body.
    let body = r#"{"category":"linear","settleCoin":"USDT"}"#;
    let mut p = signed("POST", "/v5/order/cancel-all", body, WKEY, WSECRET);
    p.body = br#"{"category":"linear","settleCoin":"USDC"}"#.to_vec();
    assert_eq!(
        b.http(T0, &p).0,
        http_fake::bybit::error(10004, "error sign!")
    );
}

#[test]
fn each_key_reads_its_own_query_api_and_a_fault_hits_one_key_and_path() {
    let mut b = book();
    let o = get(&mut b, T0, OKEY, "/v5/user/query-api");
    let w = get(&mut b, T0, WKEY, "/v5/user/query-api");
    assert_eq!(
        o["result"]["permissions"]["ContractTrade"],
        json!(["Order", "Position"])
    );
    assert_eq!(
        w["result"]["permissions"]["ContractTrade"],
        json!(["Order"])
    );
    for v in [&o, &w] {
        assert_eq!(v["result"]["userID"], UID);
        assert_eq!(v["result"]["readOnly"], 0);
        assert_eq!(v["result"]["isMaster"], false);
        assert_eq!(v["result"]["ips"], json!(["127.0.0.1"]));
    }
    let realtime = "/v5/order/realtime?category=linear&settleCoin=USDT";
    b.fault(WKEY, "/v5/order/realtime", Answer::Reply(503, "{}".into()));
    let r = b.http(T0, &signed("GET", realtime, "", WKEY, WSECRET)).0;
    assert_eq!(r, Answer::Reply(503, "{}".into()));
    assert_eq!(
        get(&mut b, T0, OKEY, realtime)["retCode"],
        0,
        "the other key is not faulted"
    );
    assert_eq!(
        get(
            &mut b,
            T0,
            WKEY,
            "/v5/position/list?category=linear&settleCoin=USDT"
        )["retCode"],
        0
    );
    b.clear_fault(WKEY, "/v5/order/realtime");
    assert_eq!(get(&mut b, T0, WKEY, realtime)["retCode"], 0);
    let kinds: Vec<&str> = b.log.iter().map(|l| l.answer.as_str()).collect();
    assert_eq!(&kinds[2..], ["fault", "ok", "ok", "ok"]);
    // Unsigned: the IP echo and the server time (venue clock = now).
    let plain = |t: &str| Request {
        method: "GET".into(),
        target: t.into(),
        headers: vec![],
        body: vec![],
    };
    assert_eq!(
        b.http(T0, &plain("/ip")).0,
        Answer::Reply(200, "127.0.0.1\n".into())
    );
    let Answer::Reply(_, t) = b.http(T0, &plain("/v5/market/time")).0 else {
        panic!()
    };
    let t: Value = serde_json::from_str(&t).unwrap();
    assert_eq!(t["result"]["timeNano"], (T0 * 1_000_000).to_string());
}

/// The fake's frames read by the OMS's own parsers into the inputs the
/// OMS acts on.
#[test]
fn the_fakes_frames_parse_with_the_oms_stream_and_execution_readers() {
    let instance = 0x1234;
    let link = exec::WIRE_ID.encode(VenueClientId {
        instance_id: instance,
        order_id: OrderId(7),
    });
    let mut b = book();
    let (_, created) = b.ws_trade(T0, OKEY, &create_msg(&link, "Buy", "0.002", "30000"));
    let filled = b.fill(T0 + 50, "o-1", "0.001", "30000");
    let read_order = |f: &str| {
        let stream::Frame::Orders(rows) = stream::parse(f).unwrap() else {
            panic!("{f}")
        };
        assert_eq!(rows.len(), 1);
        stream::order_input(&rows[0], 3, instance).unwrap().unwrap()
    };
    let VenueInput::OrderStatus(s) = read_order(&created[0]) else {
        panic!()
    };
    assert_eq!(s.order_id, Some(OrderId(7)));
    assert_eq!(s.venue_oid, Some(VenueOrderId::new("o-1").unwrap()));
    assert_eq!(s.status.status, VenueStatus::Open);
    assert_eq!(s.status.cum, Some(Qty::parse("0").unwrap()));
    assert_eq!(s.status.venue_ts, T0 * 1000);
    let stream::Frame::Executions(execs) = stream::parse(&filled[0]).unwrap() else {
        panic!()
    };
    let VenueInput::Fill(f) = exec::classify(&execs[0], instance, None).unwrap() else {
        panic!()
    };
    assert_eq!(f.order_id, Some(OrderId(7)));
    assert_eq!(
        (f.side, f.qty, f.px),
        (
            Side::Buy,
            Qty::parse("0.001").unwrap(),
            Px::parse("30000").unwrap()
        )
    );
    assert_eq!(f.fee, Amount::parse("0.0165").unwrap());
    assert_eq!(f.venue_ts, (T0 + 50) * 1000);
    let VenueInput::OrderStatus(s) = read_order(&filled[1]) else {
        panic!()
    };
    assert_eq!(s.status.cum, Some(Qty::parse("0.001").unwrap()));
    // A cancel reads as cancelled; a planted execution as an external fill.
    let cancelled = b.cancel_at_venue(T0 + 60, "o-1");
    let VenueInput::OrderStatus(s) = read_order(&cancelled[0]) else {
        panic!()
    };
    assert!(
        matches!(s.status.status, VenueStatus::Canceled { .. }),
        "{:?}",
        s.status
    );
    b.plant_execution("Trade", "Sell", "0.001", "30000", "manual", T0 - 2500);
    let row = &rows(&exec_list(&mut b, T0, ""))[0];
    let VenueInput::Fill(f) = exec::classify(row, instance, None).unwrap() else {
        panic!()
    };
    assert_eq!(f.order_id, None);
    assert_eq!(f.venue_ts, (T0 - 2500) * 1000);
    let t = ticker::parse(&b.ticker(T0), &[SYMBOL.to_string()].into()).unwrap();
    let ticker::Frame::Ticker(t) = t else {
        panic!()
    };
    assert_eq!(
        (t.mark, t.venue_ts),
        (Some(Px::parse("30000").unwrap()), T0 * 1000)
    );
}

/// Every key of `want` is in `got`, through nested objects and the first
/// row of lists that both have.
fn keys_cover(got: &Value, want: &Value, path: &str, missing: &mut Vec<String>) {
    match (got, want) {
        (Value::Object(g), Value::Object(w)) => {
            for (k, wv) in w {
                match g.get(k) {
                    None => missing.push(format!("{path}.{k}")),
                    Some(gv) => keys_cover(gv, wv, &format!("{path}.{k}"), missing),
                }
            }
        }
        (Value::Array(g), Value::Array(w)) => {
            if let (Some(g0), Some(w0)) = (g.first(), w.first()) {
                keys_cover(g0, w0, &format!("{path}[0]"), missing);
            }
        }
        _ => {}
    }
}

#[test]
fn the_fakes_answers_have_every_key_of_the_recorded_testnet_answers() {
    let mut b = book();
    let a = create(&mut b, T0, "L1", "Buy", "0.002", "30000");
    b.fill(T0 + 1, &a, "0.001", "30000");
    let dir = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../http-fake/recorded/bybit-testnet"
    );
    let cases = [
        (
            "order_realtime_one",
            "/v5/order/realtime?category=linear&symbol=BTCUSDT",
        ),
        (
            "position_list_one",
            "/v5/position/list?category=linear&symbol=BTCUSDT",
        ),
        (
            "execution_list_rows",
            "/v5/execution/list?category=linear&symbol=BTCUSDT",
        ),
        (
            "wallet_balance",
            "/v5/account/wallet-balance?accountType=UNIFIED",
        ),
        ("query_api", "/v5/user/query-api"),
        (
            "instruments_info",
            "/v5/market/instruments-info?category=linear&symbol=BTCUSDT",
        ),
        ("account_info", "/v5/account/info"),
    ];
    let mut missing = Vec::new();
    for (name, target) in cases {
        let text = std::fs::read_to_string(format!("{dir}/{name}.json")).unwrap();
        let recorded: Value = serde_json::from_str(&text).unwrap();
        let want = &recorded["response"];
        assert!(want["result"].is_object(), "{name}");
        let got = get(&mut b, T0 + 2, OKEY, target);
        assert_eq!(got["retCode"], 0, "{name}: {got}");
        if let Some(list) = want["result"]["list"].as_array() {
            assert!(
                !list.is_empty() && !rows(&got).is_empty(),
                "{name}: a row to compare"
            );
        }
        keys_cover(&got, want, name, &mut missing);
    }
    assert!(
        missing.is_empty(),
        "keys the fake does not write: {missing:?}"
    );
}

type Ws =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// The next text frame as JSON, within 5 s.
async fn next(ws: &mut Ws) -> Result<Value, tokio::time::error::Elapsed> {
    use futures_util::StreamExt;
    let f = async {
        loop {
            if let Some(Ok(tokio_tungstenite::tungstenite::Message::Text(t))) = ws.next().await {
                return serde_json::from_str::<Value>(t.as_str()).unwrap();
            }
        }
    };
    tokio::time::timeout(Duration::from_secs(5), f).await
}

/// The servers: REST through the OMS's own client (good and bad secret),
/// WS Trade, the private stream and the ticker pump, loopback only.
#[test]
fn fake_bybit_serves_rest_signed_by_the_oms_client_and_pushes_frames_to_the_private_stream() {
    let fake = FakeBybit::start();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let base = fake.api.url();
    let client = |secret: &str| {
        rest::Rest::new(
            &base,
            Arc::new(rest::Credentials::new(WKEY, secret)),
            5000,
            Duration::from_secs(3),
        )
        .unwrap()
    };
    let p = [
        ("category", "linear".to_string()),
        ("settleCoin", "USDT".to_string()),
    ];
    let good = rt
        .block_on(client(WSECRET).get("/v5/order/realtime", &p))
        .unwrap();
    assert_eq!(good["list"], json!([]));
    let bad = rt.block_on(client("WRONG").get("/v5/order/realtime", &p));
    assert!(
        matches!(bad, Err(rest::RestError::Auth { code: 10004, .. })),
        "{bad:?}"
    );
    let body = json!({"category": "linear", "settleCoin": "USDT"});
    let r = rt
        .block_on(client(WSECRET).post("/v5/order/cancel-all", &body))
        .unwrap();
    assert_eq!(r["success"], "1");
    let log = fake.with(|b, _| b.log.clone());
    assert_eq!(
        log.iter().map(|l| l.answer.as_str()).collect::<Vec<_>>(),
        ["ok", "ret 10004", "ok"]
    );

    rt.block_on(async {
        use futures_util::{SinkExt, StreamExt};
        use tokio_tungstenite::tungstenite::Message;
        let (mut private, _) = tokio_tungstenite::connect_async(&fake.private.url)
            .await
            .unwrap();
        let (mut trade, _) = tokio_tungstenite::connect_async(&fake.trade.url)
            .await
            .unwrap();
        let (mut public, _) = tokio_tungstenite::connect_async(&fake.public.url)
            .await
            .unwrap();
        trade
            .send(Message::text(
                json!({"op": "auth", "args": [OKEY, 1, "s"]}).to_string(),
            ))
            .await
            .unwrap();
        assert_eq!(next(&mut trade).await.unwrap()["op"], "auth");
        private
            .send(Message::text(stream::subscribe_message()))
            .await
            .unwrap();
        assert_eq!(next(&mut private).await.unwrap()["op"], "subscribe");
        trade
            .send(Message::text(
                create_msg("L1", "Buy", "0.001", "30000").to_string(),
            ))
            .await
            .unwrap();
        let reply = next(&mut trade).await.unwrap();
        assert_eq!(
            (
                reply["retCode"].as_i64(),
                reply["data"]["orderLinkId"].as_str()
            ),
            (Some(0), Some("L1"))
        );
        let pushed = next(&mut private).await.unwrap();
        assert_eq!(pushed["topic"], "order.linear");
        assert_eq!(
            fake.with(|b, _| b.by_link("L1").unwrap().key.clone()),
            OKEY,
            "the WS Trade key"
        );
        // A fill control pushes the execution, then the order.
        fake.fill("o-1", "0.001", "30000");
        assert_eq!(
            next(&mut private).await.unwrap()["topic"],
            "execution.linear"
        );
        assert_eq!(next(&mut private).await.unwrap()["topic"], "order.linear");
        // A silent private stream (a half-open network) carries no frame.
        fake.private
            .silent
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let planted = fake.with(|b, now| b.plant_order(now, "manual-1", "Buy", "0.001", "29000"));
        fake.cancel_at_venue(&planted, true);
        let pushed = tokio::time::timeout(Duration::from_millis(300), private.next()).await;
        assert!(pushed.is_err(), "a frame on a silent stream: {pushed:?}");
        fake.private
            .silent
            .store(false, std::sync::atomic::Ordering::SeqCst);
        // A silent create: booked, the hook runs, no reply, no frame.
        let (tx, rx) = std::sync::mpsc::channel();
        fake.silent_for_next_create(Box::new(move || tx.send(()).unwrap()));
        trade
            .send(Message::text(
                create_msg("L2", "Buy", "0.001", "30000").to_string(),
            ))
            .await
            .unwrap();
        rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(fake.with(|b, _| b.by_link("L2").is_some()));
        let reply = tokio::time::timeout(Duration::from_millis(300), trade.next()).await;
        assert!(reply.is_err(), "no reply for a silent create: {reply:?}");
        assert!(
            tokio::time::timeout(Duration::from_millis(300), private.next())
                .await
                .is_err(),
            "no frame for a silent create"
        );
        // The ticker: a snapshot after subscribe, then about every second.
        public
            .send(Message::text(
                json!({"op": "subscribe", "args": ["tickers.BTCUSDT"], "req_id": "t"}).to_string(),
            ))
            .await
            .unwrap();
        let mut tickers = Vec::new();
        while tickers.len() < 2 {
            let v = next(&mut public).await.unwrap();
            if v["topic"] == "tickers.BTCUSDT" {
                tickers.push(v["ts"].as_u64().unwrap());
            }
        }
        assert!(
            (800..=1500).contains(&(tickers[1] - tickers[0])),
            "{tickers:?}"
        );
        // A dropped private connection ends that socket.
        fake.private.drop_connection();
        let end = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                match private.next().await {
                    None | Some(Err(_)) | Some(Ok(Message::Close(_))) => return,
                    _ => {}
                }
            }
        });
        end.await.unwrap();
    });
}

/// Each refusal of the fake with its answer kind in the log: a program
/// that sends such a request meets the same answer in a joint test.
#[test]
fn each_refused_request_gets_its_documented_answer() {
    let mut b = book();
    create(&mut b, T0, "L1", "Buy", "0.001", "30000");
    let realtime = "/v5/order/realtime?category=linear&settleCoin=USDT";
    // A fault comes after the key and the signature: a request refused
    // for them never reaches it.
    b.fault(WKEY, "/v5/order/realtime", Answer::Reply(503, "{}".into()));
    let without = |header: &str| {
        let mut r = signed("GET", realtime, "", WKEY, WSECRET);
        r.headers.retain(|(k, _)| k != header);
        r
    };
    let w = |target: &str| signed("GET", target, "", WKEY, WSECRET);
    let create_body = |category: &str, symbol: &str| {
        json!({"category": category, "symbol": symbol, "side": "Buy", "orderType": "Limit",
            "qty": "0.001", "price": "30000", "orderLinkId": format!("{category}-{symbol}")})
        .to_string()
    };
    let cases = [
        ("an unknown path", w("/v5/order/amend"), "http 404"),
        (
            "cancel-all over GET",
            w("/v5/order/cancel-all?category=linear&settleCoin=USDT"),
            "http 404",
        ),
        (
            "no X-BAPI-TIMESTAMP",
            without("x-bapi-timestamp"),
            "ret 10004",
        ),
        (
            "no X-BAPI-RECV-WINDOW",
            without("x-bapi-recv-window"),
            "ret 10004",
        ),
        (
            "a bad signature on a faulted path",
            signed("GET", realtime, "", WKEY, OSECRET),
            "ret 10004",
        ),
        (
            "an execution page above 100",
            w("/v5/execution/list?category=linear&settleCoin=USDT&limit=101"),
            "ret 10001",
        ),
        (
            "a position list without category",
            w("/v5/position/list?symbol=BTCUSDT"),
            "ret 10001",
        ),
        (
            "a position list of another category",
            w("/v5/position/list?category=inverse&symbol=BTCUSDT"),
            "ret 10001",
        ),
        (
            "a create in another category",
            signed(
                "POST",
                "/v5/order/create",
                &create_body("inverse", SYMBOL),
                OKEY,
                OSECRET,
            ),
            "ret 10001",
        ),
        (
            "a create of another symbol",
            signed(
                "POST",
                "/v5/order/create",
                &create_body("linear", "ETHUSDT"),
                OKEY,
                OSECRET,
            ),
            "ret 10001",
        ),
        ("the faulted path, signed", w(realtime), "fault"),
        (
            "an execution page of 100",
            w("/v5/execution/list?category=linear&settleCoin=USDT&limit=100"),
            "ok",
        ),
    ];
    for (name, req, want) in cases {
        let (a, frames) = b.http(T0 + 1, &req);
        assert!(frames.is_empty(), "{name}");
        assert_eq!(b.log.last().unwrap().answer, want, "{name}: {a:?}");
        assert!(
            !matches!(a, Answer::Reply(403, _)),
            "{name}: the fake never answers 403"
        );
    }
    assert_eq!(b.orders.len(), 1, "no refused create was booked");
}

#[test]
fn the_order_and_execution_lists_filter_by_symbol_settle_coin_and_order_id() {
    let mut b = book();
    let a = create(&mut b, T0, "L1", "Buy", "0.002", "30000");
    let c = create(&mut b, T0 + 1, "L2", "Buy", "0.001", "30000");
    let d = create(&mut b, T0 + 2, "L3", "Buy", "0.001", "30000");
    b.fill(T0 + 3, &a, "0.001", "30000");
    b.cancel_at_venue(T0 + 4, &c);
    b.cancel_at_venue(T0 + 5, &d);
    let n = |b: &mut Book, target: &str| rows(&get(b, T0 + 10, WKEY, target)).len();
    let rt = "/v5/order/realtime?category=linear";
    assert_eq!(n(&mut b, &format!("{rt}&symbol=BTCUSDT")), 1);
    assert_eq!(n(&mut b, &format!("{rt}&symbol=ETHUSDT")), 0);
    assert_eq!(n(&mut b, &format!("{rt}&settleCoin=USDC")), 0);
    assert_eq!(n(&mut b, &format!("{rt}&settleCoin=USDT&orderId={a}")), 1);
    let hist = "/v5/order/history?category=linear&settleCoin=USDT";
    assert_eq!(n(&mut b, hist), 2);
    let one = rows(&get(&mut b, T0 + 10, WKEY, &format!("{hist}&orderId={c}")));
    assert_eq!(one.len(), 1, "history by orderId: {one:?}");
    assert_eq!(one[0]["orderLinkId"], "L2");
    assert_eq!(
        n(&mut b, "/v5/order/history?category=linear&symbol=ETHUSDT"),
        0
    );
    let ex = "/v5/execution/list?category=linear";
    assert_eq!(n(&mut b, &format!("{ex}&symbol=BTCUSDT")), 1);
    assert_eq!(n(&mut b, &format!("{ex}&symbol=ETHUSDT")), 0);
    assert_eq!(n(&mut b, &format!("{ex}&settleCoin=USDC")), 0);
}

#[test]
fn every_position_changing_execution_type_moves_the_position_and_funding_does_not() {
    let mut b = book();
    let steps = [
        ("AdlTrade", "Sell", "0.001", -1),
        ("BustTrade", "Buy", "0.002", 1),
        ("Delivery", "Buy", "0.001", 2),
        ("Settle", "Sell", "0.001", 1),
        ("Funding", "Sell", "0.001", 1),
    ];
    for (i, (exec_type, side, qty, size)) in steps.into_iter().enumerate() {
        b.plant_execution(exec_type, side, qty, "30000", "", T0 + i as u64);
        assert_eq!(b.size, size, "after {exec_type}");
    }
    let p = &rows(&get(
        &mut b,
        T0 + 10,
        OKEY,
        "/v5/position/list?category=linear&symbol=BTCUSDT",
    ))[0];
    assert_eq!(
        (p["side"].as_str(), p["size"].as_str()),
        (Some("Buy"), Some("0.001"))
    );
    assert_eq!(
        p["updatedTime"],
        (T0 + 3).to_string(),
        "funding dates nothing"
    );
}

fn rest_client(base: &str, key: &str, secret: &str) -> rest::Rest {
    rest::Rest::new(
        base,
        Arc::new(rest::Credentials::new(key, secret)),
        5000,
        Duration::from_secs(3),
    )
    .unwrap()
}

/// The private stream of a fresh fake, subscribed.
async fn subscribed(fake: &FakeBybit) -> Ws {
    use futures_util::SinkExt;
    let (mut private, _) = tokio_tungstenite::connect_async(&fake.private.url)
        .await
        .unwrap();
    private
        .send(tokio_tungstenite::tungstenite::Message::text(
            stream::subscribe_message(),
        ))
        .await
        .unwrap();
    assert_eq!(next(&mut private).await.unwrap()["op"], "subscribe");
    private
}

/// What the joint scenarios lean on: a REST request's frames are pushed
/// (the watcher's cancel-all, the OMS's REST fallback), and a venue cancel
/// is pushed only when the test asks (J2a).
#[test]
fn fake_bybit_pushes_the_frames_of_rest_requests_and_a_venue_cancel_only_when_asked() {
    let fake = FakeBybit::start();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let (oms, watcher) = (
        rest_client(&fake.api.url(), OKEY, OSECRET),
        rest_client(&fake.api.url(), WKEY, WSECRET),
    );
    rt.block_on(async {
        let mut private = subscribed(&fake).await;
        let order = |f: &Value| {
            assert_eq!(f["topic"], "order.linear", "{f}");
            let r = &f["data"][0];
            (
                r["orderLinkId"].as_str().unwrap().to_string(),
                r["orderStatus"].as_str().unwrap().to_string(),
            )
        };
        let body = json!({"category": "linear", "symbol": SYMBOL, "side": "Buy",
            "orderType": "Limit", "qty": "0.001", "price": "30000", "orderLinkId": "R1"});
        oms.post("/v5/order/create", &body).await.unwrap();
        assert_eq!(
            order(&next(&mut private).await.unwrap()),
            ("R1".into(), "New".into())
        );
        let cancel = json!({"category": "linear", "symbol": SYMBOL, "orderLinkId": "R1"});
        oms.post("/v5/order/cancel", &cancel).await.unwrap();
        assert_eq!(
            order(&next(&mut private).await.unwrap()),
            ("R1".into(), "Cancelled".into())
        );
        let planted = fake.with(|b, now| b.plant_order(now, "manual-1", "Buy", "0.001", "29000"));
        let all = json!({"category": "linear", "settleCoin": "USDT"});
        watcher.post("/v5/order/cancel-all", &all).await.unwrap();
        assert_eq!(
            order(&next(&mut private).await.unwrap()),
            ("manual-1".into(), "Cancelled".into())
        );
        assert_eq!(
            fake.with(|b, _| b.order(&planted).unwrap().status),
            "Cancelled"
        );
        // A venue cancel the operator's page does not push.
        let (quiet, loud) = fake.with(|b, now| {
            (
                b.plant_order(now, "manual-2", "Buy", "0.001", "29000"),
                b.plant_order(now, "manual-3", "Buy", "0.001", "29000"),
            )
        });
        fake.cancel_at_venue(&quiet, false);
        let pushed = tokio::time::timeout(Duration::from_millis(300), next(&mut private)).await;
        assert!(
            pushed.is_err(),
            "a frame for an unpushed cancel: {pushed:?}"
        );
        assert_eq!(
            fake.with(|b, _| b.order(&quiet).unwrap().status),
            "Cancelled"
        );
        fake.cancel_at_venue(&loud, true);
        assert_eq!(
            order(&next(&mut private).await.unwrap()),
            ("manual-3".into(), "Cancelled".into())
        );
    });
}

/// Inputs from two threads at once (the test's fills, REST cancels on the
/// server's thread): the frames arrive in the order the book made them,
/// as one account's private topic does on a real venue.
#[test]
fn frames_of_concurrent_inputs_reach_the_private_stream_in_book_order() {
    const N: usize = 40;
    let fake = FakeBybit::start();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let mut private = rt.block_on(subscribed(&fake));
    let (fills, cancels): (Vec<String>, Vec<String>) = fake.with(|b, now| {
        (0..N)
            .map(|i| {
                (
                    b.plant_order(now, &format!("F{i}"), "Buy", "0.002", "30000"),
                    b.plant_order(now, &format!("C{i}"), "Buy", "0.001", "30000"),
                )
            })
            .unzip()
    });
    let base = fake.api.url();
    std::thread::scope(|s| {
        s.spawn(|| {
            for id in &fills {
                fake.fill(id, "0.001", "30000");
            }
        });
        s.spawn(|| {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            let oms = rest_client(&base, OKEY, OSECRET);
            for id in &cancels {
                let body = json!({"category": "linear", "symbol": SYMBOL, "orderId": id});
                rt.block_on(oms.post("/v5/order/cancel", &body)).unwrap();
            }
        });
    });
    // Two frames per fill, one per cancel.
    let ids: Vec<u64> = rt.block_on(async {
        let mut ids = Vec::new();
        while ids.len() < 3 * N {
            let f = next(&mut private).await.unwrap();
            let id = f["id"].as_str().unwrap();
            ids.push(id.strip_prefix("f-").unwrap().parse().unwrap());
        }
        ids
    });
    assert!(
        ids.windows(2).all(|w| w[0] < w[1]),
        "frames out of book order: {ids:?}"
    );
}

#[test]
fn an_older_planted_execution_moves_the_position_but_not_its_updated_time_back() {
    let mut b = book();
    let a = create(&mut b, T0, "L1", "Buy", "0.001", "30000");
    b.fill(T0 + 500, &a, "0.001", "30000");
    b.plant_execution("Trade", "Buy", "0.001", "30000", "manual", T0 - 100);
    let p = &rows(&get(
        &mut b,
        T0 + 600,
        OKEY,
        "/v5/position/list?category=linear&symbol=BTCUSDT",
    ))[0];
    assert_eq!(p["size"], "0.002");
    assert_eq!(
        p["updatedTime"],
        (T0 + 500).to_string(),
        "updatedTime is the latest change"
    );
}

#[test]
fn executions_with_the_same_exec_time_are_listed_newest_booked_first() {
    let mut b = book();
    let e1 = b.plant_execution("Trade", "Buy", "0.001", "30000", "manual", T0 - 10);
    let e2 = b.plant_execution("Trade", "Buy", "0.001", "30000", "manual", T0 - 10);
    let ids: Vec<Value> = rows(&exec_list(&mut b, T0, ""))
        .iter()
        .map(|r| r["execId"].clone())
        .collect();
    assert_eq!(ids, [json!(e2), json!(e1)]);
}

#[test]
#[should_panic(expected = "never fires")]
fn a_fault_on_a_public_path_is_a_test_error() {
    book().fault(WKEY, "/v5/market/time", Answer::Reply(503, "{}".into()));
}

#[test]
fn a_create_keeps_reduce_only_and_one_without_an_order_link_id_is_refused_with_10001() {
    let mut b = book();
    let mut body = create_msg("R1", "Sell", "0.001", "30000")["args"][0].clone();
    body["reduceOnly"] = json!(true);
    let (r, _) = post(&mut b, T0, OKEY, "/v5/order/create", body);
    assert_eq!(r["retCode"], 0, "{r}");
    create(&mut b, T0, "L2", "Buy", "0.001", "30000");
    let reduce_only = |b: &mut Book, link: &str| {
        let q = format!("/v5/order/realtime?category=linear&orderLinkId={link}");
        rows(&get(b, T0 + 1, OKEY, &q))[0]["reduceOnly"].clone()
    };
    assert_eq!(reduce_only(&mut b, "R1"), json!(true));
    assert_eq!(reduce_only(&mut b, "L2"), json!(false));
    let mut msg = create_msg("", "Buy", "0.001", "30000");
    msg["args"][0]
        .as_object_mut()
        .unwrap()
        .remove("orderLinkId");
    let (reply, frames) = b.ws_trade(T0 + 2, OKEY, &msg);
    assert_eq!(reply.unwrap()["retCode"], 10001);
    assert!(frames.is_empty());
    let (r, _) = post(
        &mut b,
        T0 + 3,
        OKEY,
        "/v5/order/create",
        msg["args"][0].clone(),
    );
    assert_eq!(r["retCode"], 10001);
    assert_eq!(b.orders.len(), 2);
}
