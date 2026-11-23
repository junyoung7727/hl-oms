//! The start checks and the IP recheck (C-WATCH part 4, U19a4b): the
//! sans-IO `Start` machine (query-api, days left, public IP, settle coin)
//! and the `IpCheck` step, driven on a manual clock; with the real client
//! against a loopback fake (`http-fake`) where the test needs to show that
//! no request goes out.

#![allow(clippy::unwrap_used)]

use std::sync::{Arc, Mutex};

use http_fake::{Answer, FakeHttp};
use serde_json::{Value, json};
use watcher::clock::{Clock, ManualClock};
use watcher::config::Config;
use watcher::metrics::Metrics;
use watcher::rest::{Client, ClientConfig, Credentials, IP_BAN_PAUSE_MS, RestError};
use watcher::start::{
    DAYS_LEFT, INSTRUMENTS, IpCheck, PUBLIC_IP_OK, QUERY_API, RETRY_MS, Request, Start, Step,
};

/// 2023-11-14T22:13:20Z.
const T0: u64 = 1_700_000_000_000;
const UID: u64 = 4242;
const IP: &str = "203.0.113.7";

fn config(symbols: &[&str], ip_check_ms: u64) -> Config {
    let list: Vec<String> = symbols.iter().map(|s| format!("{s:?}")).collect();
    Config::from_toml(&format!(
        r#"
name = "w1"
network = "testnet"
credentials_file = "/nonexistent-watcher-start/creds.toml"
sub_account_uid = {UID}
symbols = [{}]
ip_echo_url = "https://ip.example/"
data_dir = "/nonexistent-watcher-start/data"
status_url = "http://127.0.0.1:8188/api/status"
host = "h1"

[intervals]
ip_check_ms = {ip_check_ms}
"#,
        list.join(", ")
    ))
    .unwrap()
}

/// A start of `symbols` at `now`, its metrics and the lines it logs.
struct S {
    start: Start,
    metrics: Arc<Metrics>,
    lines: Arc<Mutex<Vec<String>>>,
}

fn start_of(symbols: &[&str], now: u64) -> S {
    let metrics = Arc::new(Metrics::new("h1", "w1"));
    let (log, lines) = watcher::log::collect();
    let start = Start::new(&config(symbols, 60_000), metrics.clone(), log, now);
    S {
        start,
        metrics,
        lines,
    }
}

fn lines(l: &Arc<Mutex<Vec<String>>>) -> Vec<String> {
    l.lock().unwrap().clone()
}

/// A query-api `result`.
fn key(uid: u64, master: bool, ips: Value, expired_at: Option<&str>) -> Value {
    let mut v = json!({
        "id": "1", "note": "w", "apiKey": "WKEY", "readOnly": 1, "secret": "",
        "permissions": {"ContractTrade": [], "Wallet": []},
        "ips": ips, "type": 1, "deadlineDay": -2,
        "userID": uid, "isMaster": master, "parentUid": "1",
    });
    if let Some(e) = expired_at {
        v["expiredAt"] = json!(e);
    }
    v
}

fn good_key() -> Value {
    key(UID, false, json!([IP]), Some("1970-01-01T00:00:00Z"))
}

/// An instruments-info `result` with one row.
fn instrument(symbol: &str, coin: &str) -> Value {
    json!({"category": "linear", "list": [{"symbol": symbol, "settleCoin": coin, "status": "Trading"}], "nextPageCursor": ""})
}

fn query_api() -> Request {
    Request::Venue {
        path: QUERY_API,
        params: vec![],
    }
}

fn instruments(symbol: &str) -> Request {
    Request::Venue {
        path: INSTRUMENTS,
        params: vec![("category", "linear".into()), ("symbol", symbol.into())],
    }
}

/// Takes the request due at `now`; it must be `want`, and no second
/// request is due while it has no answer.
fn take(s: &mut Start, now: u64, want: &Request) {
    assert_eq!(s.poll(now).as_ref(), Some(want), "at +{}", now - T0);
    assert_eq!(s.poll(now), None, "a request in flight is not asked again");
}

/// Runs a start through query-api and the echo at `now` (answers at the
/// same instant), up to the first instruments-info request.
fn to_symbols(s: &mut S, now: u64, key: Value, echo: &str) {
    take(&mut s.start, now, &query_api());
    assert_eq!(s.start.on_answer(now, Ok(key)), Step::Next);
    take(&mut s.start, now, &Request::Echo);
    assert_eq!(s.start.on_answer(now, Ok(json!(echo))), Step::Next);
}

#[test]
fn a_symbol_missing_from_instruments_info_refuses_the_start() {
    for (case, result) in [
        (
            "empty list",
            json!({"category": "linear", "list": [], "nextPageCursor": ""}),
        ),
        ("another symbol only", instrument("ETHUSDT", "USDT")),
    ] {
        let mut s = start_of(&["BTCUSDT"], T0);
        to_symbols(&mut s, T0, good_key(), IP);
        take(&mut s.start, T0, &instruments("BTCUSDT"));
        let step = s.start.on_answer(T0 + 10, Ok(result));
        assert_eq!(
            step,
            Step::Refuse {
                reason: "BTCUSDT is not in instruments-info for category linear; the watcher's scope is linear/USDT".into()
            },
            "{case}"
        );
        // Nothing is asked after a refusal.
        for at in [T0 + 10, T0 + 10 + RETRY_MS, T0 + 3_600_000] {
            assert_eq!(s.start.poll(at), None, "{case}");
        }
        assert!(s.start.ip_check().is_none(), "{case}");
    }
}

#[test]
fn a_master_key_another_uid_or_a_key_bound_to_no_ip_refuses_the_start() {
    let master =
        "query-api: the API key is the master account's; the watcher needs a sub-account key";
    let other = format!(
        "query-api: the API key belongs to user 4243; the config says sub_account_uid {UID}"
    );
    let unbound = "query-api: the API key is bound to no IP address (ips [] or \"*\"); bind it to this host's address";
    let no_address = "query-api: the API key's ips hold no IP address";
    for (case, v, reason) in [
        (
            "master",
            key(UID, true, json!([IP]), None),
            master.to_string(),
        ),
        ("other uid", key(4243, false, json!([IP]), None), other),
        (
            "no ips",
            key(UID, false, json!([]), None),
            unbound.to_string(),
        ),
        (
            "star",
            key(UID, false, json!(["*"]), None),
            unbound.to_string(),
        ),
        (
            "an ip and a star",
            key(UID, false, json!([IP, " * "]), None),
            unbound.to_string(),
        ),
        (
            "no address",
            key(UID, false, json!(["host.example"]), None),
            no_address.to_string(),
        ),
    ] {
        let mut s = start_of(&["BTCUSDT"], T0);
        take(&mut s.start, T0, &query_api());
        assert_eq!(
            s.start.on_answer(T0 + 5, Ok(v)),
            Step::Refuse { reason },
            "{case}"
        );
        for at in [T0 + 5, T0 + 5 + RETRY_MS, T0 + IP_BAN_PAUSE_MS] {
            assert_eq!(s.start.poll(at), None, "{case}: nothing after a refusal");
        }
        // The refusal is the runtime's one line (exit 2); the start logs nothing.
        assert!(lines(&s.lines).is_empty(), "{case}: {:?}", lines(&s.lines));
        assert_eq!(s.metrics.get(PUBLIC_IP_OK, &[]), None, "{case}");
    }
    // The good key: one bound address, mapped or not, goes on to the echo.
    for ips in [
        json!([IP]),
        json!(["198.51.100.1", IP]),
        json!([format!("::ffff:{IP}")]),
    ] {
        let mut s = start_of(&["BTCUSDT"], T0);
        take(&mut s.start, T0, &query_api());
        assert_eq!(
            s.start
                .on_answer(T0, Ok(key(UID, false, ips.clone(), None))),
            Step::Next,
            "{ips}"
        );
        take(&mut s.start, T0, &Request::Echo);
    }
}

#[test]
fn a_key_that_expires_in_10_days_gives_days_left_10() {
    for (expired_at, days) in [
        ("2023-11-24T22:13:20Z", 10.0),
        ("2023-11-24T22:13:20.999Z", 10.0),
        ("2023-11-24T21:13:20Z", 9.0),
        ("2023-11-24T22:13:19Z", 9.0),
        ("2023-11-15T22:13:20Z", 1.0),
        ("2023-11-14T22:13:20Z", 0.0),
        ("2023-11-14T21:13:20Z", -1.0),
        ("2024-02-29T00:00:00Z", 106.0),
    ] {
        let mut s = start_of(&["BTCUSDT"], T0);
        take(&mut s.start, T0, &query_api());
        let v = key(UID, false, json!([IP]), Some(expired_at));
        assert_eq!(s.start.on_answer(T0, Ok(v)), Step::Next, "{expired_at}");
        assert_eq!(s.metrics.get(DAYS_LEFT, &[]), Some(days), "{expired_at}");
        assert!(lines(&s.lines).is_empty(), "{expired_at}");
    }
    // The export carries it.
    let mut s = start_of(&["BTCUSDT"], T0);
    take(&mut s.start, T0, &query_api());
    s.start.on_answer(
        T0,
        Ok(key(UID, false, json!([IP]), Some("2023-11-24T22:13:20Z"))),
    );
    let body = s.metrics.import_lines(T0);
    assert!(
        body.contains(r#"{"metric":{"__name__":"watcher_api_key_days_left","host":"h1","venue":"bybit","watcher":"w1"},"values":[10.0]"#),
        "{body}"
    );
}

#[test]
fn the_documented_epoch_expiry_exports_no_days_left() {
    // Not exported, without a line: absent, null, the documented epoch,
    // and anything up to 1970-01-02T00:00:00Z.
    for expired_at in [
        None,
        Some(Value::Null),
        Some(json!("1970-01-01T00:00:00Z")),
        Some(json!("1970-01-01T23:59:59.999Z")),
        Some(json!("1970-01-02T00:00:00Z")),
    ] {
        let mut s = start_of(&["BTCUSDT"], T0);
        s.metrics.set(DAYS_LEFT, &[], 5.0);
        take(&mut s.start, T0, &query_api());
        let mut v = key(UID, false, json!([IP]), None);
        if let Some(e) = &expired_at {
            v["expiredAt"] = e.clone();
        }
        assert_eq!(s.start.on_answer(T0, Ok(v)), Step::Next, "{expired_at:?}");
        assert_eq!(s.metrics.get(DAYS_LEFT, &[]), None, "{expired_at:?}");
        assert!(lines(&s.lines).is_empty(), "{expired_at:?}");
    }
    // One second later it applies (and the key has long expired).
    let mut s = start_of(&["BTCUSDT"], T0);
    take(&mut s.start, T0, &query_api());
    s.start.on_answer(
        T0,
        Ok(key(UID, false, json!([IP]), Some("1970-01-02T00:00:01Z"))),
    );
    assert_eq!(s.metrics.get(DAYS_LEFT, &[]), Some(-19675.0));
    // Unreadable: not exported, one warn line each, and the start goes on.
    for bad in [
        json!("2023-11-24 22:13:20Z"),
        json!("2023-11-24T22:13:20+09:00"),
        json!("2023-11-24T22:13:20"),
        json!("2023-13-01T00:00:00Z"),
        json!("2023-02-29T00:00:00Z"),
        json!("2023-11-24T24:00:00Z"),
        json!("2023-11-24T22:13:20.Z"),
        json!("2023-11-24T22:13:20Z "),
        json!("+2023-11-24T22:13:20Z"),
        json!(""),
        json!(1700864000),
    ] {
        let mut s = start_of(&["BTCUSDT"], T0);
        s.metrics.set(DAYS_LEFT, &[], 5.0);
        take(&mut s.start, T0, &query_api());
        let mut v = key(UID, false, json!([IP]), None);
        v["expiredAt"] = bad.clone();
        assert_eq!(s.start.on_answer(T0, Ok(v)), Step::Next, "{bad}");
        assert_eq!(s.metrics.get(DAYS_LEFT, &[]), None, "{bad}");
        let l = lines(&s.lines);
        assert_eq!(l.len(), 1, "{bad}: {l:?}");
        assert!(l[0].starts_with("warn: query-api: expiredAt "), "{l:?}");
        assert!(
            l[0].ends_with("cannot be read; watcher_api_key_days_left is not exported"),
            "{l:?}"
        );
        take(&mut s.start, T0, &Request::Echo);
    }
}

#[test]
fn an_echo_failure_or_a_body_that_is_not_an_address_gives_public_ip_ok_0() {
    let ok = |b: &str| Ok(json!(b));
    let cases: Vec<(&str, Result<Value, RestError>, f64)> = vec![
        ("the address", ok(IP), 1.0),
        ("mapped", ok(&format!("::ffff:{IP}")), 1.0),
        ("another address", ok("198.51.100.9"), 0.0),
        ("not an address", ok("<html>203.0.113.7</html>"), 0.0),
        ("empty", ok(""), 0.0),
        (
            "no answer",
            Err(RestError::NoAnswer("IP echo HTTP 500".into())),
            0.0,
        ),
        (
            "not sent",
            Err(RestError::NotSent("connection refused".into())),
            0.0,
        ),
        (
            "paused",
            Err(RestError::IpBanned("not sent: paused".into())),
            0.0,
        ),
        ("not a string", Ok(json!(42)), 0.0),
    ];
    for (case, answer, want) in cases {
        let mut s = start_of(&["BTCUSDT"], T0);
        take(&mut s.start, T0, &query_api());
        s.start.on_answer(T0, Ok(good_key()));
        take(&mut s.start, T0, &Request::Echo);
        // Not a refusal: the start goes on to the settle coin at once.
        assert_eq!(s.start.on_answer(T0 + 7, answer), Step::Next, "{case}");
        assert_eq!(s.metrics.get(PUBLIC_IP_OK, &[]), Some(want), "{case}");
        let l = lines(&s.lines);
        if want == 1.0 {
            assert!(l.is_empty(), "{case}: {l:?}");
        } else {
            assert_eq!(l.len(), 1, "{case}: {l:?}");
            assert!(l[0].starts_with("warn: public IP "), "{case}: {l:?}");
            assert!(l[0].ends_with("watcher_public_ip_ok = 0"), "{case}: {l:?}");
        }
        take(&mut s.start, T0 + 7, &instruments("BTCUSDT"));
    }
    // A bound IPv4-mapped address matches a plain IPv4 echo.
    let mut s = start_of(&["BTCUSDT"], T0);
    take(&mut s.start, T0, &query_api());
    s.start.on_answer(
        T0,
        Ok(key(UID, false, json!([format!("::ffff:{IP}")]), None)),
    );
    take(&mut s.start, T0, &Request::Echo);
    s.start.on_answer(T0, Ok(json!(IP)));
    assert_eq!(s.metrics.get(PUBLIC_IP_OK, &[]), Some(1.0));
}

/// A client of the fake on a manual clock at `T0`, with its metrics and
/// the lines it logs.
struct C {
    client: Client,
    clock: Arc<ManualClock>,
    metrics: Arc<Metrics>,
    lines: Arc<Mutex<Vec<String>>>,
}

fn client(fake: &FakeHttp) -> C {
    let clock = Arc::new(ManualClock::new(T0));
    let metrics = Arc::new(Metrics::new("h1", "w1"));
    let (log, lines) = watcher::log::collect();
    let cfg = ClientConfig {
        base: fake.url(),
        recv_window_ms: 5000,
        request_timeout_ms: 1000,
    };
    let client = Client::new(
        &cfg,
        Credentials::new("WKEY", "watcher-made-up-secret-42"),
        clock.clone(),
        metrics.clone(),
        log,
    )
    .unwrap();
    C {
        client,
        clock,
        metrics,
        lines,
    }
}

fn ok(result: &Value) -> Answer {
    Answer::Reply(
        200,
        format!(r#"{{"retCode":0,"retMsg":"OK","result":{result},"time":1}}"#),
    )
}

fn code(c: i64) -> Answer {
    Answer::Reply(
        200,
        format!(r#"{{"retCode":{c},"retMsg":"m{c}","result":{{}},"time":1}}"#),
    )
}

/// Drives `start` through the client until it refuses, finishes or stays
/// up without requests; a wait moves the manual clock to its instant. At
/// most 50 steps.
async fn drive(start: &mut Start, c: &C, echo_url: &str) -> Step {
    for _ in 0..50 {
        let now = c.clock.now_ms();
        let Some(req) = start.poll(now) else {
            panic!("no request at +{} ms", now - T0);
        };
        let answer = watcher::start::send(&c.client, echo_url, &req).await;
        match start.on_answer(c.clock.now_ms(), answer) {
            Step::Next => {}
            Step::Wait { until_ms } => c.clock.set(until_ms),
            end => return end,
        }
    }
    panic!("the start did not end in 50 steps");
}

#[tokio::test]
async fn a_traded_symbol_that_does_not_settle_in_usdt_refuses_the_start() {
    let echo = FakeHttp::start(Answer::Reply(200, format!("{IP}\n")));
    let echo_url = format!("{}/ip", echo.url());
    for (coin, end) in [
        (
            "USDC",
            Some("BTCPERP settles in USDC; the watcher's scope is linear/USDT"),
        ),
        (
            "usdt",
            Some("BTCPERP settles in usdt; the watcher's scope is linear/USDT"),
        ),
        (
            "",
            Some("BTCPERP settles in \"\"; the watcher's scope is linear/USDT"),
        ),
        ("USDT", None),
    ] {
        let fake = FakeHttp::start(code(10016));
        fake.route(QUERY_API, [ok(&good_key())]);
        fake.route(
            INSTRUMENTS,
            [
                ok(&instrument("BTCUSDT", "USDT")),
                ok(&instrument("BTCPERP", coin)),
                ok(&instrument("ETHUSDT", "USDT")),
            ],
        );
        let c = client(&fake);
        let metrics = Arc::new(Metrics::new("h1", "w1"));
        let (log, _lines) = watcher::log::collect();
        let mut start = Start::new(
            &config(&["BTCUSDT", "BTCPERP", "ETHUSDT"], 60_000),
            metrics.clone(),
            log,
            T0,
        );
        let step = drive(&mut start, &c, &echo_url).await;
        let targets: Vec<String> = fake.requests().iter().map(|r| r.target.clone()).collect();
        match end {
            Some(reason) => {
                assert_eq!(
                    step,
                    Step::Refuse {
                        reason: reason.into()
                    },
                    "{coin}"
                );
                // No check request after it: query-api and the first two
                // symbols, nothing for the third, nothing later.
                assert_eq!(
                    targets,
                    [
                        "/v5/user/query-api".to_string(),
                        "/v5/market/instruments-info?category=linear&symbol=BTCUSDT".into(),
                        "/v5/market/instruments-info?category=linear&symbol=BTCPERP".into(),
                    ],
                    "{coin}"
                );
                for at in [T0 + RETRY_MS, T0 + 600_000] {
                    assert_eq!(start.poll(at), None, "{coin}");
                }
                assert!(start.ip_check().is_none(), "{coin}");
            }
            None => {
                assert_eq!(step, Step::Done);
                assert_eq!(targets.len(), 4, "{targets:?}");
                assert_eq!(start.poll(T0 + 600_000), None);
                assert!(start.ip_check().is_some());
            }
        }
        assert_eq!(metrics.get(PUBLIC_IP_OK, &[]), Some(1.0), "{coin}");
    }
    assert_eq!(echo.requests().len(), 4);
}
