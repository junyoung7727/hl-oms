//! Contract tests of the Bybit gateway against the real Bybit testnet.
//!
//! Every test is `#[ignore]`: `cargo test` never sends anything. They run
//! only as the `bybit-testnet` suite (`xtask verify --bybit-testnet`), with
//! `OMS_BYBIT_TESTNET_CREDS` naming a credentials file outside the
//! repository (TOML `api_key`, `api_secret`, mode 0600). A run without it
//! fails instead of passing.
//!
//! The doubles (the virtual venue, the simulation, the loopback fake Bybit)
//! stay for fault injection. These tests are the only evidence of what Bybit
//! itself does. When one fails, the gateway or the recorded answers are
//! wrong, not the venue.
//!
//! Orders: limit post-only buys far below the market (half the mark price),
//! the smallest size, so nothing can fill. Each test leaves the account
//! with no open order.

#![allow(clippy::unwrap_used)]

use oms_core::*;
use oms_node::venue::QueryReq;
use oms_node::venue::bybit::answer::{self, Answer};
use oms_node::venue::bybit::query::Queries;
use oms_node::venue::bybit::rest::{Credentials, Rest};
use oms_node::venue::bybit::{Endpoints, cancel_all, checks, exec, orders};
use std::sync::Arc;
use std::time::Duration;

const SYMBOL: &str = "BTCUSDT";
const RECV_WINDOW_MS: u64 = 5_000;

fn rest() -> Rest {
    let path = std::env::var("OMS_BYBIT_TESTNET_CREDS")
        .expect("OMS_BYBIT_TESTNET_CREDS must name the testnet credentials file");
    let creds = Credentials::load(std::path::Path::new(&path)).unwrap();
    let ep = Endpoints::of("testnet").unwrap();
    Rest::new(
        &ep.rest,
        Arc::new(creds),
        RECV_WINDOW_MS,
        Duration::from_secs(10),
    )
    .unwrap()
}

/// A run-unique instance id, so that order link ids never repeat between
/// runs on the same account.
fn instance() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

fn link(instance_id: u64, order: u64) -> String {
    exec::WIRE_ID.encode(VenueClientId {
        instance_id,
        order_id: OrderId(order),
    })
}

/// Half the mark price, on the tick: a post-only buy there cannot fill.
async fn far_price(rest: &Rest) -> Px {
    let t = rest
        .get(
            "/v5/market/tickers",
            &[("category", "linear".into()), ("symbol", SYMBOL.into())],
        )
        .await
        .unwrap();
    let mark: f64 = t["list"][0]["markPrice"].as_str().unwrap().parse().unwrap();
    Px::parse(&format!("{:.1}", (mark / 2.0).floor())).unwrap()
}

fn body(rest_px: Px, link: &str) -> serde_json::Value {
    orders::create_body(
        SYMBOL,
        Side::Buy,
        Qty::parse("0.001").unwrap(),
        rest_px,
        Tif::Alo,
        false,
        link,
    )
}

/// The status query of the gateway (realtime, then the history twice).
async fn status(rest: Rest, instance_id: u64, order: u64) -> Vec<VenueInput> {
    let q = Queries::new(rest, instance_id, 2_000);
    let a = q
        .answer(
            QueryReq::Status {
                order_id: OrderId(order),
                venue_oid: None,
                accepted_at: rest_now_us(),
            },
            1,
        )
        .await;
    assert!(a.failed.is_none(), "status query failed: {:?}", a.failed);
    a.inputs
}

fn rest_now_us() -> u64 {
    instance() * 1000
}

fn not_found(inputs: &[VenueInput], order: u64) -> bool {
    matches!(inputs, [VenueInput::StatusNotFound { order_id, .. }] if *order_id == OrderId(order))
}

async fn open_count(rest: &Rest) -> usize {
    let v = rest
        .get(
            "/v5/order/realtime",
            &[
                ("category", "linear".into()),
                ("settleCoin", checks::SETTLE_COIN.into()),
            ],
        )
        .await
        .unwrap();
    v["list"].as_array().unwrap().len()
}

/// Sends one create stamped `offset_ms` from now and returns our class of
/// the answer and the status query's inputs for that order. An accepted
/// order is cancelled before this returns.
async fn create_at(
    offset_ms: i64,
    order: u64,
    b: impl Fn(Px, &str) -> serde_json::Value,
) -> (Answer, Vec<VenueInput>) {
    let r = rest();
    let id = instance();
    let px = far_price(&r).await;
    let l = link(id, order);
    let ts = (r.now_ms() as i64 + offset_ms) as u64;
    let (status_code, text) = r
        .post_raw_at("/v5/order/create", &b(px, &l), ts)
        .await
        .unwrap();
    let a = answer::classify(status_code, &text);
    println!(
        "bybit-testnet: create at {offset_ms:+} ms -> HTTP {status_code}: {}",
        text.chars().take(300).collect::<String>()
    );
    let inputs = status(rest(), id, order).await;
    if !matches!(a, Answer::Rejected { .. }) {
        flatten(&r, SYMBOL).await;
    }
    (a, inputs)
}

#[tokio::test]
#[ignore = "bybit-testnet suite only"]
async fn a_request_older_than_the_receive_window_is_refused_and_places_nothing() {
    // A-01 for Bybit: a request stamped more than recv_window before the
    // venue's clock is refused (10002), so an expired order never rests.
    let (a, inputs) = create_at(-(RECV_WINDOW_MS as i64) - 1_500, 1, body).await;
    assert!(matches!(a, Answer::Rejected { code: 10002, .. }), "{a:?}");
    assert!(not_found(&inputs, 1), "{inputs:?}");
}

#[tokio::test]
#[ignore = "bybit-testnet suite only"]
async fn a_request_more_than_one_second_ahead_is_refused_and_places_nothing() {
    // The other half of the rule `fits_expiry` uses: the venue takes our
    // clock up to 1 s ahead of its own.
    let (a, inputs) = create_at(1_500, 2, body).await;
    assert!(matches!(a, Answer::Rejected { code: 10002, .. }), "{a:?}");
    assert!(not_found(&inputs, 2), "{inputs:?}");
}

#[tokio::test]
#[ignore = "bybit-testnet suite only"]
async fn a_request_inside_the_receive_window_passes_the_time_check() {
    // Stamped 4 s old (inside 5 s) or 0.5 s ahead: whatever the venue says
    // next, it is not the time check.
    for (offset, order) in [(-4_000, 3), (500, 4)] {
        let (a, _) = create_at(offset, order, body).await;
        assert!(
            !matches!(a, Answer::Rejected { code: 10002, .. }),
            "{offset}: {a:?}"
        );
        assert!(!matches!(a, Answer::Unknown(_)), "{offset}: {a:?}");
    }
}

#[tokio::test]
#[ignore = "bybit-testnet suite only"]
async fn every_refused_create_leaves_no_order_at_the_venue() {
    // Our table says a refusal is `Rejected` (nothing processed). A size
    // below the smallest (0.001) is refused on every account (10001); the
    // order must not exist.
    let below = |px: Px, l: &str| {
        orders::create_body(
            SYMBOL,
            Side::Buy,
            Qty::parse("0.0001").unwrap(),
            px,
            Tif::Alo,
            false,
            l,
        )
    };
    let (a, inputs) = create_at(0, 5, below).await;
    assert!(matches!(a, Answer::Rejected { code: 10001, .. }), "{a:?}");
    assert!(not_found(&inputs, 5), "{a:?} but {inputs:?}");
}

#[tokio::test]
#[ignore = "bybit-testnet suite only"]
async fn cancel_all_answers_in_the_shape_the_kill_loop_reads() {
    // The kill loop's two requests (C-KILL): cancel-all, then the count of
    // open orders. With no order open both must read cleanly.
    let r = rest();
    let c = cancel_all(&r).await.unwrap();
    assert!(!c.maybe_more);
    assert_eq!(open_count(&r).await, 0);
}

#[tokio::test]
#[ignore = "bybit-testnet suite only"]
async fn the_clock_check_reads_the_venue_time() {
    // ADR-028 R0: the lead bound is measured from /v5/market/time.
    checks::check_clock(&rest()).await.unwrap();
}

#[tokio::test]
#[ignore = "bybit-testnet suite only"]
async fn the_account_and_margin_queries_read_the_real_answers() {
    // The queries the reconciler and the margin rules use, on real answers.
    let id = instance();
    let q = Queries::new(rest(), id, 2_000);
    for req in [
        QueryReq::OpenOrders,
        QueryReq::Positions,
        QueryReq::Fills {
            since: (id - 3_600_000) * 1000,
        },
        QueryReq::Margin { seq: 1 },
        QueryReq::AccountMode,
        QueryReq::VenueSetting {
            instrument: InstrumentId::new(SYMBOL).unwrap(),
        },
    ] {
        let a = q.answer(req.clone(), 1).await;
        println!(
            "bybit-testnet: {req:?} -> {} inputs, failed {:?}",
            a.inputs.len(),
            a.failed
        );
        assert!(a.failed.is_none(), "{req:?}: {:?}", a.failed);
        assert!(!a.inputs.is_empty(), "{req:?}: no input");
    }
}

// ---- recorded answers ------------------------------------------------------
//
// The fake Bybit (`http-fake`) answers with bodies recorded here, not with
// shapes written by hand. `OMS_BYBIT_TESTNET_RECORD=1` writes the files; a
// run without it checks that the venue still answers in the recorded shape
// (the keys and their kinds, not the values) and fails when it does not.
// Then re-record, and fix the gateway if its parsing no longer fits.

/// Fields that name the account or the key. The repository is public.
const SCRUB: &[&str] = &[
    "smpGroup",
    "ips",
    "createdAt",
    "expiredAt",
    "deadlineDay",
    "apiKey",
    "userID",
    "parentUid",
    "id",
    "userIDInt64",
    "rsaPublicKey",
    "note",
];

fn recorded_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../http-fake/recorded/bybit-testnet")
}

fn scrub(v: &mut serde_json::Value) {
    match v {
        serde_json::Value::Object(m) => {
            for (k, x) in m.iter_mut() {
                if SCRUB.contains(&k.as_str()) {
                    *x = serde_json::json!("scrubbed");
                } else {
                    scrub(x);
                }
            }
        }
        serde_json::Value::Array(a) => a.iter_mut().for_each(scrub),
        _ => {}
    }
}

/// The keys and the kinds of a value: what a parser depends on.
fn shape(v: &serde_json::Value) -> serde_json::Value {
    use serde_json::{Value, json};
    match v {
        Value::Object(m) => Value::Object(m.iter().map(|(k, x)| (k.clone(), shape(x))).collect()),
        Value::Array(a) => {
            let mut kinds: Vec<Value> = a.iter().map(shape).collect();
            kinds.sort_by_key(|x| x.to_string());
            kinds.dedup();
            Value::Array(kinds)
        }
        Value::String(_) => json!("string"),
        Value::Number(_) => json!("number"),
        Value::Bool(_) => json!("bool"),
        Value::Null => json!("null"),
    }
}

#[derive(serde::Deserialize)]
struct Secret {
    api_key: String,
    api_secret: String,
}

/// One raw request, signed the way the gateway signs (`sign::rest_signature`).
async fn raw(
    method: &str,
    path: &str,
    query: &str,
    body: &str,
    offset_ms: i64,
) -> (u16, serde_json::Value) {
    let path_creds = std::env::var("OMS_BYBIT_TESTNET_CREDS").unwrap();
    let s: Secret = toml::from_str(&std::fs::read_to_string(path_creds).unwrap()).unwrap();
    let ts = (rest().now_ms() as i64 + offset_ms) as u64;
    let payload = if method == "GET" { query } else { body };
    let sig = oms_node::venue::bybit::sign::rest_signature(
        &s.api_secret,
        ts,
        &s.api_key,
        RECV_WINDOW_MS,
        payload,
    );
    let url = format!(
        "{}{path}{}",
        Endpoints::of("testnet").unwrap().rest,
        if query.is_empty() {
            String::new()
        } else {
            format!("?{query}")
        }
    );
    let http = reqwest::Client::new();
    let rb = if method == "GET" {
        http.get(url)
    } else {
        http.post(url)
            .header("Content-Type", "application/json")
            .body(body.to_string())
    };
    let resp = rb
        .header("X-BAPI-API-KEY", &s.api_key)
        .header("X-BAPI-TIMESTAMP", ts.to_string())
        .header("X-BAPI-RECV-WINDOW", RECV_WINDOW_MS.to_string())
        .header("X-BAPI-SIGN", sig)
        .header("X-BAPI-SIGN-TYPE", "2")
        .send()
        .await
        .unwrap();
    let status = resp.status().as_u16();
    (
        status,
        serde_json::from_str(&resp.text().await.unwrap()).unwrap(),
    )
}

#[tokio::test]
#[ignore = "bybit-testnet suite only"]
async fn the_venue_answers_in_the_recorded_shapes() {
    let unknown = link(instance(), 9);
    let px = far_price(&rest()).await;
    let stale = serde_json::to_string(&body(px, &link(instance(), 10))).unwrap();
    // Below the smallest size (0.001): a refusal on every account.
    let refused = serde_json::to_string(&orders::create_body(
        SYMBOL,
        Side::Buy,
        Qty::parse("0.0001").unwrap(),
        px,
        Tif::Alo,
        false,
        &link(instance(), 11),
    ))
    .unwrap();
    let cancel_unknown = serde_json::to_string(&orders::cancel_body(SYMBOL, &unknown)).unwrap();
    let past = rest().now_ms() - 6 * 24 * 3600 * 1000;
    let cases: Vec<(&str, &str, &str, String, String, i64)> = vec![
        (
            "market_time",
            "GET",
            "/v5/market/time",
            String::new(),
            String::new(),
            0,
        ),
        (
            "instruments_info",
            "GET",
            "/v5/market/instruments-info",
            format!("category=linear&symbol={SYMBOL}"),
            String::new(),
            0,
        ),
        (
            "tickers",
            "GET",
            "/v5/market/tickers",
            format!("category=linear&symbol={SYMBOL}"),
            String::new(),
            0,
        ),
        (
            "query_api",
            "GET",
            "/v5/user/query-api",
            String::new(),
            String::new(),
            0,
        ),
        (
            "account_info",
            "GET",
            "/v5/account/info",
            String::new(),
            String::new(),
            0,
        ),
        (
            "wallet_balance",
            "GET",
            "/v5/account/wallet-balance",
            "accountType=UNIFIED".into(),
            String::new(),
            0,
        ),
        (
            "position_list_empty",
            "GET",
            "/v5/position/list",
            "category=linear&settleCoin=USDT".into(),
            String::new(),
            0,
        ),
        (
            "order_realtime_empty",
            "GET",
            "/v5/order/realtime",
            "category=linear&settleCoin=USDT".into(),
            String::new(),
            0,
        ),
        (
            "order_history_unknown",
            "GET",
            "/v5/order/history",
            format!("category=linear&orderLinkId={unknown}&settleCoin=USDT"),
            String::new(),
            0,
        ),
        // A one-second window six days ago: empty on any account.
        (
            "execution_list_empty",
            "GET",
            "/v5/execution/list",
            format!(
                "category=linear&endTime={}&limit=100&settleCoin=USDT&startTime={}",
                past + 1_000,
                past
            ),
            String::new(),
            0,
        ),
        (
            "cancel_all_empty",
            "POST",
            "/v5/order/cancel-all",
            String::new(),
            r#"{"category":"linear","settleCoin":"USDT"}"#.into(),
            0,
        ),
        (
            "cancel_unknown",
            "POST",
            "/v5/order/cancel",
            String::new(),
            cancel_unknown,
            0,
        ),
        (
            "create_stale",
            "POST",
            "/v5/order/create",
            String::new(),
            stale,
            -6_500,
        ),
        (
            "create_below_min_qty",
            "POST",
            "/v5/order/create",
            String::new(),
            refused,
            0,
        ),
    ];
    let record = std::env::var("OMS_BYBIT_TESTNET_RECORD").is_ok_and(|v| v == "1");
    let dir = recorded_dir();
    let mut differ = Vec::new();
    let mut check = |name: &str,
                     method: &str,
                     path: &str,
                     query: &str,
                     body: &str,
                     status: u16,
                     mut answer: serde_json::Value| {
        scrub(&mut answer);
        let file = dir.join(format!("{name}.json"));
        if record {
            std::fs::create_dir_all(&dir).unwrap();
            let rec = serde_json::json!({
                "method": method, "path": path, "query": query,
                "body": if body.is_empty() { serde_json::Value::Null } else { serde_json::from_str(body).unwrap() },
                "http_status": status, "response": answer,
            });
            std::fs::write(&file, serde_json::to_string_pretty(&rec).unwrap() + "\n").unwrap();
            println!(
                "bybit-testnet: recorded {name} (HTTP {status}, retCode {})",
                rec["response"]["retCode"]
            );
        } else {
            let old: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
            if shape(&old["response"]) != shape(&answer) || old["http_status"] != status {
                differ.push(format!(
                    "{name}: recorded {} now {}",
                    shape(&old["response"]),
                    shape(&answer)
                ));
            }
        }
    };
    for (name, method, path, query, body, offset) in cases {
        let (status, answer) = raw(method, path, &query, &body, offset).await;
        check(name, method, path, &query, &body, status, answer);
    }
    // Lists with rows: one resting order, then one fill and the position it
    // opens, closed again by a reduce-only IOC.
    let r = rest();
    let id = instance();
    let resting = link(id, 12);
    let (_, _, a) = create(&r, &body(px, &resting)).await;
    assert!(matches!(a, Answer::Accepted(_)), "{a:?}");
    let q = format!("category=linear&symbol={SYMBOL}");
    let (s, v) = raw("GET", "/v5/order/realtime", &q, "", 0).await;
    check(
        "order_realtime_one",
        "GET",
        "/v5/order/realtime",
        &q,
        "",
        s,
        v,
    );
    assert!(flatten(&r, SYMBOL).await);
    let (ask, _) = ask_and_bid(&r, SYMBOL).await;
    let qty = Qty::parse("0.001").unwrap();
    let buy = link(id, 13);
    let bpx = Px::parse(&format!("{:.1}", (ask * 1.01).ceil())).unwrap();
    let (_, _, a) = create(
        &r,
        &orders::create_body(SYMBOL, Side::Buy, qty, bpx, Tif::Ioc, false, &buy),
    )
    .await;
    assert!(matches!(a, Answer::Accepted(_)), "{a:?}");
    assert!(!executions(&r, SYMBOL, &buy).await.is_empty());
    let qe = format!("category=linear&orderLinkId={buy}&symbol={SYMBOL}");
    let (s, v) = raw("GET", "/v5/execution/list", &qe, "", 0).await;
    check(
        "execution_list_rows",
        "GET",
        "/v5/execution/list",
        &qe,
        "",
        s,
        v,
    );
    let (s, v) = raw("GET", "/v5/position/list", &q, "", 0).await;
    check(
        "position_list_one",
        "GET",
        "/v5/position/list",
        &q,
        "",
        s,
        v,
    );
    // The close is the reduce-only IOC of `flatten`.
    let flat = flatten(&r, SYMBOL).await;
    assert!(
        differ.is_empty(),
        "the venue's answers changed shape; re-record and check the gateway:\n{}",
        differ.join("\n")
    );
    assert!(flat, "the account is not flat after the recording");
}

// ---- orders that rest or fill ----------------------------------------------

async fn create(r: &Rest, b: &serde_json::Value) -> (u16, String, Answer) {
    let (status, text) = r
        .post_raw_at("/v5/order/create", b, r.now_ms())
        .await
        .unwrap();
    let a = answer::classify(status, &text);
    println!(
        "bybit-testnet: create {} -> {}",
        b["orderLinkId"],
        text.chars().take(240).collect::<String>()
    );
    (status, text, a)
}

/// Rows of `/v5/order/realtime` and `/v5/order/history` for one link id.
async fn rows_of(r: &Rest, symbol: &str, l: &str) -> Vec<serde_json::Value> {
    let mut out = Vec::new();
    for path in ["/v5/order/realtime", "/v5/order/history"] {
        let v = r
            .get(
                path,
                &[
                    ("category", "linear".into()),
                    ("symbol", symbol.into()),
                    ("orderLinkId", l.into()),
                ],
            )
            .await
            .unwrap();
        out.extend(v["list"].as_array().unwrap().iter().cloned());
    }
    out
}

/// Waits up to 10 s for the venue's open count to reach 0.
async fn open_count_reaches_zero(r: &Rest) -> bool {
    for _ in 0..20 {
        if open_count(r).await == 0 {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    false
}

#[tokio::test]
#[ignore = "bybit-testnet suite only"]
async fn a_second_create_with_the_same_link_id_never_makes_a_second_order() {
    // V-BYBIT-ORDERS-1 classes 110072 as Unknown: the order may already
    // exist. On the real venue the second create must be refused with
    // 110072 and add no order, while the first rests and after it ended.
    let r = rest();
    let id = instance();
    let l = link(id, 20);
    let b = body(far_price(&r).await, &l);
    let mut wrong = Vec::new();
    let (_, _, first) = create(&r, &b).await;
    if !matches!(first, Answer::Accepted(_)) {
        wrong.push(format!("first create: {first:?}"));
    }
    let (_, again_text, again) = create(&r, &b).await;
    if !again_text.contains("\"retCode\":110072") || !matches!(again, Answer::Unknown(_)) {
        wrong.push(format!(
            "duplicate while resting: {again_text} read as {again:?}"
        ));
    }
    let open = rows_of(&r, SYMBOL, &l).await;
    let resting = open.iter().filter(|o| o["orderStatus"] == "New").count();
    if resting != 1 {
        wrong.push(format!(
            "{resting} resting orders with the link id: {open:?}"
        ));
    }
    flatten(&r, SYMBOL).await;
    let (_, after_text, _) = create(&r, &b).await;
    if !after_text.contains("\"retCode\":110072") {
        wrong.push(format!(
            "the venue took the link id again after its order ended: {after_text}"
        ));
    }
    flatten(&r, SYMBOL).await;
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[tokio::test]
#[ignore = "bybit-testnet suite only"]
async fn cancel_all_then_the_open_count_reaches_zero() {
    // The kill loop (C-KILL): one cancel-all, then count until 0.
    let r = rest();
    let id = instance();
    let px = far_price(&r).await;
    let mut wrong = Vec::new();
    for n in 30..33 {
        let (_, _, a) = create(&r, &body(px, &link(id, n))).await;
        if !matches!(a, Answer::Accepted(_)) {
            wrong.push(format!("create {n}: {a:?}"));
        }
    }
    let before = open_count(&r).await;
    let c = cancel_all(&r).await;
    println!("bybit-testnet: {before} open, cancel-all -> {c:?}");
    let zero = open_count_reaches_zero(&r).await;
    flatten(&r, SYMBOL).await;
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    let c = c.unwrap();
    assert_eq!((before, c.cancelled, c.maybe_more), (3, 3, false));
    assert!(zero, "orders still open 10 s after cancel-all");
}

async fn ask_and_bid(r: &Rest, symbol: &str) -> (f64, f64) {
    let t = r
        .get(
            "/v5/market/tickers",
            &[("category", "linear".into()), ("symbol", symbol.into())],
        )
        .await
        .unwrap();
    // A side of the testnet book is sometimes empty (""); then the mark
    // price stands in for it.
    let p = |k: &str| t["list"][0][k].as_str().and_then(|s| s.parse::<f64>().ok());
    let mark = p("markPrice").unwrap();
    (
        p("ask1Price").unwrap_or(mark),
        p("bid1Price").unwrap_or(mark),
    )
}

/// The executions of one order link id, waited for up to 10 s.
async fn executions(r: &Rest, symbol: &str, l: &str) -> Vec<serde_json::Value> {
    for _ in 0..20 {
        let v = r
            .get(
                "/v5/execution/list",
                &[
                    ("category", "linear".into()),
                    ("symbol", symbol.into()),
                    ("orderLinkId", l.into()),
                ],
            )
            .await
            .unwrap();
        let rows = v["list"].as_array().unwrap().clone();
        if !rows.is_empty() {
            return rows;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    Vec::new()
}

async fn position_size(r: &Rest, symbol: &str) -> String {
    let v = r
        .get(
            "/v5/position/list",
            &[("category", "linear".into()), ("symbol", symbol.into())],
        )
        .await
        .unwrap();
    v["list"][0]["size"].as_str().unwrap_or("0").to_string()
}

/// Leaves the account as the tests found it: cancel-all, then a reduce-only
/// IOC against any position of `symbol`. True when no order rests and the
/// position is 0 within 10 s.
async fn flatten(r: &Rest, symbol: &str) -> bool {
    let _ = cancel_all(r).await;
    for _ in 0..3 {
        let v = r
            .get(
                "/v5/position/list",
                &[("category", "linear".into()), ("symbol", symbol.into())],
            )
            .await
            .unwrap();
        let p = &v["list"][0];
        let size = p["size"].as_str().unwrap_or("0");
        if size == "0" {
            break;
        }
        let side = if p["side"] == "Buy" {
            Side::Sell
        } else {
            Side::Buy
        };
        let (ask, bid) = ask_and_bid(r, symbol).await;
        let px = if side == Side::Sell {
            bid * 0.98
        } else {
            ask * 1.02
        };
        let tick = v["list"][0]["markPrice"]
            .as_str()
            .unwrap_or("0")
            .split('.')
            .nth(1)
            .map_or(0, str::len);
        let px = Px::parse(&format!("{px:.tick$}")).unwrap();
        let close = link(instance(), 99);
        let (_, text, _) = create(
            r,
            &orders::create_body(
                symbol,
                side,
                Qty::parse(size).unwrap(),
                px,
                Tif::Ioc,
                true,
                &close,
            ),
        )
        .await;
        println!(
            "bybit-testnet: flatten {symbol} {size} -> {}",
            text.chars().take(160).collect::<String>()
        );
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    open_count_reaches_zero(r).await && position_size(r, symbol).await == "0"
}

#[tokio::test]
#[ignore = "bybit-testnet suite only"]
async fn a_real_execution_row_becomes_our_fill_and_the_position_closes() {
    // R05, V-VENUE-EXEC-1: an execution row of the real venue reads as a
    // fill of our order (the link id decodes to our instance and order),
    // with the venue's quantity, side and a positive fee. Then a
    // reduce-only IOC closes the position.
    let r = rest();
    let id = instance();
    let (ask, _) = ask_and_bid(&r, SYMBOL).await;
    let qty = Qty::parse("0.001").unwrap();
    let buy = link(id, 40);
    let px = Px::parse(&format!("{:.1}", (ask * 1.01).ceil())).unwrap();
    let (_, _, a) = create(
        &r,
        &orders::create_body(SYMBOL, Side::Buy, qty, px, Tif::Ioc, false, &buy),
    )
    .await;
    let rows = executions(&r, SYMBOL, &buy).await;
    // The close is the gateway's own reduce-only IOC (flatten).
    let closed = flatten(&r, SYMBOL).await;
    assert!(matches!(a, Answer::Accepted(_)), "{a:?}");
    assert!(!rows.is_empty(), "no execution row for the IOC buy");
    let mut filled = 0i64;
    for row in &rows {
        match exec::classify(row, id, None).unwrap() {
            VenueInput::Fill(f) => {
                assert_eq!(f.order_id, Some(OrderId(40)), "{row}");
                assert_eq!(f.side, Side::Buy);
                assert!(f.fee.raw() > 0, "taker fee should be a cost: {row}");
                filled += f.qty.raw();
            }
            other => panic!("not a fill: {other:?} from {row}"),
        }
    }
    assert_eq!(filled, qty.raw(), "the rows add up to the order");
    assert!(closed, "the reduce-only IOC did not close the position");
}

#[tokio::test]
#[ignore = "bybit-testnet suite only"]
async fn two_of_our_crossing_orders_do_not_trade_with_each_other() {
    // R10: the OMS has no check of its own for two strategies crossing each
    // other. This records what Bybit does with the account's defaults: on a
    // symbol with a wide spread our resting buy is alone at its price, and
    // our sell at that price can only meet it.
    // ARCUSDT: tick 0.00001, size step 1, minimum value 5 USDT; on the
    // testnet its spread is many ticks wide. (ANTUSDT asks for a signed
    // agreement first, 110126.)
    const WIDE: &str = "ARCUSDT";
    let r = rest();
    let id = instance();
    let (ask, bid) = ask_and_bid(&r, WIDE).await;
    assert!(
        ask - bid >= 0.0001,
        "{WIDE} spread {bid}..{ask} too narrow for this test"
    );
    let mid = Px::parse(&format!("{:.5}", ((ask + bid) / 2.0 * 1e5).floor() / 1e5)).unwrap();
    let qty = Qty::parse("250").unwrap();
    let rest_buy = link(id, 50);
    let (_, _, a) = create(
        &r,
        &orders::create_body(WIDE, Side::Buy, qty, mid, Tif::Alo, false, &rest_buy),
    )
    .await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    let cross = link(id, 51);
    let (_, _, b) = create(
        &r,
        &orders::create_body(WIDE, Side::Sell, qty, mid, Tif::Ioc, false, &cross),
    )
    .await;
    println!("bybit-testnet: the crossing sell -> {b:?}");
    tokio::time::sleep(Duration::from_secs(2)).await;
    let buy_rows = rows_of(&r, WIDE, &rest_buy).await;
    let buy_execs = executions(&r, WIDE, &rest_buy).await;
    let size = position_size(&r, WIDE).await;
    let flat = flatten(&r, WIDE).await;
    println!(
        "bybit-testnet: self-match: buy {:?}, buy executions {}, position {size}",
        buy_rows
            .iter()
            .map(|o| (
                &o["orderStatus"],
                &o["cumExecQty"],
                &o["cancelType"],
                &o["smpType"]
            ))
            .collect::<Vec<_>>(),
        buy_execs.len()
    );
    assert!(matches!(a, Answer::Accepted(_)), "{a:?}");
    // Observed 2026-11-25 with the account's defaults: the venue's
    // self-match prevention cancels the resting (maker) order, nothing
    // trades, and the crossing IOC ends with nothing filled. A strategy's
    // resting order can therefore end because another strategy crossed it.
    assert!(
        buy_execs.is_empty(),
        "our two orders traded with each other: {buy_execs:?}"
    );
    assert!(
        buy_rows
            .iter()
            .any(|o| o["orderStatus"] == "Cancelled" && o["cancelType"] == "CancelBySmp"),
        "the resting buy did not end by self-match prevention: {buy_rows:?}"
    );
    assert_eq!(size, "0", "another trader took a side ({size})");
    assert!(flat);
}
