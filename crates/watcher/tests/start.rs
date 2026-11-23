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
use watcher::rest::{Client, ClientConfig, Credentials, IP_BAN_PAUSE_MS, OWN_RULE_CODE, RestError};
use watcher::start::{
    DAYS_LEFT, INSTRUMENTS, IpCheck, PUBLIC_IP_OK, QUERY_API, RETRY_MS, Request, Start, Step,
    apply, send,
};

/// 2023-11-14T22:13:20Z.
const T0: u64 = 1_700_000_000_000;
const UID: u64 = 4242;
const IP: &str = "203.0.113.7";
const MASTER: &str = "the API key is the master account's; the watcher needs a sub-account key";
const OTHER_UID: &str = "the API key belongs to user 4243; the config says sub_account_uid 4242";
const WARN_IP: &str = "warn: public IP ";
const IP_OK_0: &str = "watcher_public_ip_ok = 0";
const UNBOUND: &str =
    "the API key is bound to no IP address (ips [] or \"*\"); bind it to this host's address";

type Lines = Arc<Mutex<Vec<String>>>;

fn config(symbols: &[&str], ip_check_ms: u64) -> Config {
    config_with(symbols, ip_check_ms, "")
}

/// The config of `config` with `extra` (whole sections) at its end.
fn config_with(symbols: &[&str], ip_check_ms: u64, extra: &str) -> Config {
    let list: Vec<String> = symbols.iter().map(|s| format!("{s:?}")).collect();
    let text = format!(
        r#"name = "w1"
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
{extra}"#,
        list.join(", ")
    );
    Config::from_toml(&text).unwrap()
}

/// A start of `symbols` at `T0` (recheck every 60 s), its metrics and the
/// lines it logs.
struct S {
    start: Start,
    metrics: Arc<Metrics>,
    lines: Lines,
}

fn start_of(symbols: &[&str]) -> S {
    start_with(symbols, 60_000, Arc::new(Metrics::new("h1", "w1")))
}

fn start_with(symbols: &[&str], ip_check_ms: u64, metrics: Arc<Metrics>) -> S {
    let (log, lines) = watcher::log::collect();
    let start = Start::new(&config(symbols, ip_check_ms), metrics.clone(), log, T0);
    S {
        start,
        metrics,
        lines,
    }
}

fn unreadable(m: &Metrics, endpoint: &str) -> Option<f64> {
    m.get(
        "watcher_request_errors_total",
        &[("endpoint", endpoint), ("class", "unreadable")],
    )
}

fn lines(l: &Lines) -> Vec<String> {
    l.lock().unwrap().clone()
}

/// Exactly one line, starting with `head` and ending with `tail`.
fn one_line(l: &Lines, head: &str, tail: &str, case: &str) {
    let l = lines(l);
    assert_eq!(l.len(), 1, "{case}: {l:?}");
    assert!(
        l[0].starts_with(head) && l[0].ends_with(tail),
        "{case}: {l:?}"
    );
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

fn bound(ips: Value) -> Value {
    key(UID, false, ips, None)
}

/// An instruments-info `result` with one row.
fn instrument(symbol: &str, coin: &str) -> Value {
    json!({"category": "linear", "list": [{"symbol": symbol, "settleCoin": coin}], "nextPageCursor": ""})
}

fn query_api() -> Request {
    Request::Venue {
        path: QUERY_API,
        params: vec![],
    }
}

fn instruments(symbol: &str) -> Request {
    let params = vec![("category", "linear".into()), ("symbol", symbol.into())];
    Request::Venue {
        path: INSTRUMENTS,
        params,
    }
}

fn wait(until_ms: u64) -> Step {
    Step::Wait { until_ms }
}

fn refuse(reason: &str) -> Step {
    Step::Refuse {
        reason: reason.into(),
    }
}

/// An error of `class` as the client gives it.
fn err(class: &str) -> RestError {
    let m = format!("made-up {class}");
    match class {
        "not_sent" => RestError::NotSent(m),
        "no_answer" => RestError::NoAnswer(m),
        "rate_limited" => RestError::RateLimited(m),
        "clock" => RestError::Clock(m),
        "refused" => RestError::Refused {
            code: 10001,
            msg: m,
        },
        "ip_banned" => RestError::IpBanned(m),
        _ => panic!("{class}"),
    }
}

type Case = (String, Result<Value, RestError>, u64);

/// Every client class that is neither `auth` nor a readable answer, and
/// the wait after it: 5 s, after a 403 the 600 s pause.
fn retried_cases() -> Vec<Case> {
    let classes = "not_sent no_answer rate_limited clock refused ip_banned";
    let wait = |c: &str| [RETRY_MS, IP_BAN_PAUSE_MS][usize::from(c == "ip_banned")];
    let case = |c: &str| (c.to_string(), Err(err(c)), wait(c));
    classes.split(' ').map(case).collect()
}

/// Takes the request due at `now`; it must be `want`, and no second
/// request is due while it has no answer.
fn take(s: &mut Start, now: u64, want: &Request) {
    assert_eq!(s.poll(now).as_ref(), Some(want), "at +{}", now - T0);
    assert_eq!(s.poll(now), None, "a request in flight is not asked again");
}

/// Asks and answers the request due at `now`; returns the step.
fn answer(s: &mut Start, now: u64, want: &Request, a: Result<Value, RestError>) -> Step {
    take(s, now, want);
    s.on_answer(now, a)
}

/// Runs a start through query-api and the echo at `T0`, up to the first
/// instruments-info request.
fn to_symbols(s: &mut S) {
    for (req, a) in [(query_api(), good_key()), (Request::Echo, json!(IP))] {
        assert_eq!(answer(&mut s.start, T0, &req, Ok(a)), Step::Next);
    }
}

/// Nothing is asked at any of `ats`.
fn asks_nothing(s: &mut Start, ats: &[u64], case: &str) {
    for &at in ats {
        assert_eq!(s.poll(at), None, "{case}: at +{}", at - T0);
    }
}

#[test]
fn a_symbol_missing_from_instruments_info_refuses_the_start() {
    let reason = "BTCUSDT is not in instruments-info for category linear; the watcher's scope is linear/USDT";
    let empty = json!({"category": "linear", "list": [], "nextPageCursor": ""});
    for (case, result) in [
        ("empty list", empty),
        ("another symbol", instrument("ETHUSDT", "USDT")),
    ] {
        let mut s = start_of(&["BTCUSDT"]);
        to_symbols(&mut s);
        take(&mut s.start, T0, &instruments("BTCUSDT"));
        let at = T0 + 10;
        assert_eq!(s.start.on_answer(at, Ok(result)), refuse(reason), "{case}");
        asks_nothing(&mut s.start, &[at, at + RETRY_MS, T0 + 3_600_000], case);
        assert!(s.start.ip_check().is_none(), "{case}");
    }
}

#[test]
fn a_master_key_another_uid_or_a_key_bound_to_no_ip_refuses_the_start() {
    for (case, v, why) in [
        ("master", key(UID, true, json!([IP]), None), MASTER),
        ("other uid", key(4243, false, json!([IP]), None), OTHER_UID),
        ("no ips", bound(json!([])), UNBOUND),
        ("star", bound(json!(["*"])), UNBOUND),
        ("an ip and a star", bound(json!([IP, " * "])), UNBOUND),
        (
            "no address",
            bound(json!(["a.example"])),
            "the API key's ips hold no IP address",
        ),
    ] {
        let mut s = start_of(&["BTCUSDT"]);
        take(&mut s.start, T0, &query_api());
        let want = refuse(&format!("query-api: {why}"));
        let at = T0 + 5;
        assert_eq!(s.start.on_answer(at, Ok(v)), want, "{case}");
        asks_nothing(
            &mut s.start,
            &[at, at + RETRY_MS, at + IP_BAN_PAUSE_MS],
            case,
        );
        // The refusal is the runtime's one line (exit 2); the start logs nothing.
        assert!(lines(&s.lines).is_empty(), "{case}: {:?}", lines(&s.lines));
        assert_eq!(s.metrics.get(PUBLIC_IP_OK, &[]), None, "{case}");
    }
    // One bound address among others, mapped or not, goes on to the echo.
    let mapped = format!("::ffff:{IP}");
    for ips in [json!([IP]), json!(["198.51.100.1", IP]), json!([mapped])] {
        let mut s = start_of(&["BTCUSDT"]);
        let step = answer(&mut s.start, T0, &query_api(), Ok(bound(ips.clone())));
        assert_eq!(step, Step::Next, "{ips}");
        take(&mut s.start, T0, &Request::Echo);
    }
}

/// A start whose query-api at `T0` says `expired_at` (`Value` as written).
fn days_of(expired_at: Option<Value>) -> S {
    let mut s = start_of(&["BTCUSDT"]);
    s.metrics.set(DAYS_LEFT, &[], 5.0);
    let mut v = bound(json!([IP]));
    if let Some(e) = expired_at {
        v["expiredAt"] = e;
    }
    assert_eq!(answer(&mut s.start, T0, &query_api(), Ok(v)), Step::Next);
    s
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
        let s = days_of(Some(json!(expired_at)));
        assert_eq!(s.metrics.get(DAYS_LEFT, &[]), Some(days), "{expired_at}");
        assert!(lines(&s.lines).is_empty(), "{expired_at}");
    }
    // The export carries it.
    let s = days_of(Some(json!("2023-11-24T22:13:20Z")));
    let body = s.metrics.import_lines(T0);
    let line = r#"{"metric":{"__name__":"watcher_api_key_days_left","host":"h1","venue":"bybit","watcher":"w1"},"values":[10.0]"#;
    assert!(body.contains(line), "{body}");
}

#[test]
fn the_documented_epoch_expiry_exports_no_days_left() {
    // Not exported, without a line: absent, null, the documented epoch,
    // and anything up to 1970-01-02T00:00:00Z.
    for e in [
        None,
        Some(Value::Null),
        Some(json!("1970-01-01T00:00:00Z")),
        Some(json!("1970-01-01T23:59:59.999Z")),
        Some(json!("1970-01-02T00:00:00Z")),
    ] {
        let s = days_of(e.clone());
        assert_eq!(s.metrics.get(DAYS_LEFT, &[]), None, "{e:?}");
        assert!(lines(&s.lines).is_empty(), "{e:?}");
    }
    // One second later it applies (and the key has long expired).
    let s = days_of(Some(json!("1970-01-02T00:00:01Z")));
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
        let mut s = days_of(Some(bad.clone()));
        assert_eq!(s.metrics.get(DAYS_LEFT, &[]), None, "{bad}");
        let tail = "cannot be read; watcher_api_key_days_left is not exported";
        let head = "warn: query-api: expiredAt ";
        one_line(&s.lines, head, tail, &bad.to_string());
        take(&mut s.start, T0, &Request::Echo);
    }
}

#[test]
fn an_echo_failure_or_a_body_that_is_not_an_address_gives_public_ip_ok_0() {
    let body = |b: &str| Ok(json!(b));
    for (case, a, want) in [
        ("the address", body(IP), 1.0),
        ("mapped", body(&format!("::ffff:{IP}")), 1.0),
        ("another address", body("198.51.100.9"), 0.0),
        ("not an address", body("<html>203.0.113.7</html>"), 0.0),
        ("empty", body(""), 0.0),
        ("no answer", Err(err("no_answer")), 0.0),
        ("not sent", Err(err("not_sent")), 0.0),
        ("paused", Err(err("ip_banned")), 0.0),
        ("not a string", Ok(json!(42)), 0.0),
    ] {
        let mut s = start_of(&["BTCUSDT"]);
        answer(&mut s.start, T0, &query_api(), Ok(good_key()));
        take(&mut s.start, T0, &Request::Echo);
        // Not a refusal: the start goes on to the settle coin at once.
        assert_eq!(s.start.on_answer(T0 + 7, a), Step::Next, "{case}");
        assert_eq!(s.metrics.get(PUBLIC_IP_OK, &[]), Some(want), "{case}");
        if want == 1.0 {
            assert!(lines(&s.lines).is_empty(), "{case}");
        } else {
            one_line(&s.lines, WARN_IP, IP_OK_0, case);
        }
        take(&mut s.start, T0 + 7, &instruments("BTCUSDT"));
    }
    // A bound IPv4-mapped address matches a plain IPv4 echo.
    let mut s = start_of(&["BTCUSDT"]);
    let k = Ok(bound(json!([format!("::ffff:{IP}")])));
    answer(&mut s.start, T0, &query_api(), k);
    answer(&mut s.start, T0, &Request::Echo, Ok(json!(IP)));
    assert_eq!(s.metrics.get(PUBLIC_IP_OK, &[]), Some(1.0));
}

/// A client of a fake Bybit on a manual clock at `T0`, its metrics, the
/// lines it logs, and an echo fake that answers `IP`.
struct C {
    client: Client,
    clock: Arc<ManualClock>,
    metrics: Arc<Metrics>,
    lines: Lines,
    echo: FakeHttp,
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
    let creds = Credentials::new("WKEY", "watcher-made-up-secret-42");
    let client = Client::new(&cfg, creds, clock.clone(), metrics.clone(), log).unwrap();
    let echo = FakeHttp::start(Answer::Reply(200, format!("{IP}\n")));
    C {
        client,
        clock,
        metrics,
        lines,
        echo,
    }
}

impl C {
    fn echo_url(&self) -> String {
        format!("{}/ip", self.echo.url())
    }

    /// Sends the request and gives the answer at the clock after it.
    async fn ask(&self, req: &Request) -> Result<Value, RestError> {
        send(&self.client, &self.echo_url(), req).await
    }

    /// Drives `start` through the client until it refuses, finishes or
    /// stays up without requests; a wait moves the clock to its instant.
    /// At most 50 steps.
    async fn drive(&self, start: &mut Start) -> Step {
        for _ in 0..50 {
            let now = self.clock.now_ms();
            let req = start.poll(now).expect("a request");
            let a = self.ask(&req).await;
            match start.on_answer(self.clock.now_ms(), a) {
                Step::Next => {}
                Step::Wait { until_ms } => self.clock.set(until_ms),
                end => return end,
            }
        }
        panic!("the start did not end in 50 steps");
    }
}

fn ok(result: &Value) -> Answer {
    let body = format!(r#"{{"retCode":0,"retMsg":"OK","result":{result},"time":1}}"#);
    Answer::Reply(200, body)
}

fn code(c: i64) -> Answer {
    let body = format!(r#"{{"retCode":{c},"retMsg":"m{c}","result":{{}},"time":1}}"#);
    Answer::Reply(200, body)
}

fn targets(fake: &FakeHttp) -> Vec<String> {
    fake.requests().iter().map(|r| r.target.clone()).collect()
}

#[tokio::test]
async fn a_traded_symbol_that_does_not_settle_in_usdt_refuses_the_start() {
    let scope = "; the watcher's scope is linear/USDT";
    for (coin, why) in [
        ("USDC", Some("BTCPERP settles in USDC")),
        ("usdt", Some("BTCPERP settles in usdt")),
        ("", Some("BTCPERP settles in \"\"")),
        ("USDT", None),
    ] {
        let fake = FakeHttp::start(code(10016));
        fake.route(QUERY_API, [ok(&good_key())]);
        let rows = [("BTCUSDT", "USDT"), ("BTCPERP", coin), ("ETHUSDT", "USDT")];
        fake.route(INSTRUMENTS, rows.map(|(s, c)| ok(&instrument(s, c))));
        let c = client(&fake);
        let mut s = start_of(&["BTCUSDT", "BTCPERP", "ETHUSDT"]);
        let step = c.drive(&mut s.start).await;
        assert_eq!(s.metrics.get(PUBLIC_IP_OK, &[]), Some(1.0), "{coin}");
        assert_eq!(c.echo.requests().len(), 1, "{coin}");
        let Some(why) = why else {
            assert_eq!(step, Step::Done);
            assert_eq!(targets(&fake).len(), 4);
            assert!(s.start.ip_check().is_some());
            continue;
        };
        assert_eq!(step, refuse(&format!("{why}{scope}")), "{coin}");
        // No check request after it: query-api and the first two symbols,
        // nothing for the third, nothing later.
        let info = "/v5/market/instruments-info?category=linear&symbol=";
        let symbols = ["BTCUSDT", "BTCPERP"].map(|s| format!("{info}{s}"));
        let want = [&[QUERY_API.to_string()][..], &symbols].concat();
        assert_eq!(targets(&fake), want, "{coin}");
        asks_nothing(&mut s.start, &[T0 + RETRY_MS, T0 + 600_000], coin);
        assert!(s.start.ip_check().is_none(), "{coin}");
    }
}

/// Gives a retried answer at `at` to the request in flight: one line, a
/// wait of `wait`, no request before its end and `req` again at it.
fn retried(s: &mut S, at: u64, req: &Request, a: Result<Value, RestError>, wait: u64, case: &str) {
    let before = lines(&s.lines).len();
    assert_eq!(s.start.on_answer(at, a), self::wait(at + wait), "{case}");
    let l = lines(&s.lines);
    assert_eq!(l.len(), before + 1, "{case}: one line: {l:?}");
    let tail = format!("asked again at {} ms", at + wait);
    assert!(
        l[before].starts_with("start: ") && l[before].ends_with(&tail),
        "{case}: {l:?}"
    );
    let end = at + wait;
    asks_nothing(&mut s.start, &[at, at + RETRY_MS - 1, end - 1], case);
    take(&mut s.start, at + wait, req);
}

#[test]
fn query_api_answers_other_than_auth_or_a_refusal_retry_every_5_s() {
    let mut cases = retried_cases();
    // Readable as JSON but not as the key: unreadable, counted by the start.
    let mut master_text = good_key();
    master_text["isMaster"] = json!("false");
    let mut uid_text = good_key();
    uid_text["userID"] = json!("4242");
    for (case, v) in [
        ("no fields", json!({})),
        ("isMaster a string", master_text),
        ("userID a string", uid_text),
        ("ips a string", bound(json!(IP))),
        ("ips with a number", bound(json!([IP, 1]))),
        ("null result", Value::Null),
    ] {
        cases.push((case.into(), Ok(v), RETRY_MS));
    }
    let metrics = Arc::new(Metrics::new("h1", "w1"));
    for (case, a, wait) in cases {
        let mut s = start_with(&["BTCUSDT"], 60_000, metrics.clone());
        take(&mut s.start, T0, &query_api());
        // The answer comes 300 ms after the request: the wait runs from it.
        retried(&mut s, T0 + 300, &query_api(), a, wait, &case);
        // Twice in a row: the pace stays the same.
        let at = T0 + 300 + wait + 40;
        let again = Err(err("no_answer"));
        retried(&mut s, at, &query_api(), again, RETRY_MS, &case);
        // The start does not go on before a readable answer, then does.
        let at = at + RETRY_MS;
        assert_eq!(s.start.on_answer(at, Ok(good_key())), Step::Next, "{case}");
        take(&mut s.start, at, &Request::Echo);
    }
    assert_eq!(unreadable(&metrics, QUERY_API), Some(6.0));
}

#[test]
fn settle_coin_answers_other_than_auth_or_a_refusal_retry_every_5_s() {
    let mut cases = retried_cases();
    for (case, v) in [
        ("no list", json!({"category": "linear"})),
        ("list an object", json!({"category": "linear", "list": {}})),
        ("null result", Value::Null),
    ] {
        cases.push((case.into(), Ok(v), RETRY_MS));
    }
    let metrics = Arc::new(Metrics::new("h1", "w1"));
    for (case, a, wait) in cases {
        let mut s = start_with(&["BTCUSDT", "ETHUSDT"], 60_000, metrics.clone());
        to_symbols(&mut s);
        let row = Ok(instrument("BTCUSDT", "USDT"));
        assert_eq!(
            answer(&mut s.start, T0, &instruments("BTCUSDT"), row),
            Step::Next
        );
        // The second symbol fails: it is asked again, not the first.
        take(&mut s.start, T0 + 20, &instruments("ETHUSDT"));
        retried(&mut s, T0 + 250, &instruments("ETHUSDT"), a, wait, &case);
        let at = T0 + 250 + wait;
        let last = s.start.on_answer(at, Ok(instrument("ETHUSDT", "USDT")));
        assert_eq!(last, Step::Done, "{case}");
        asks_nothing(&mut s.start, &[at + IP_BAN_PAUSE_MS], &case);
        assert!(s.start.ip_check().is_some(), "{case}");
    }
    assert_eq!(unreadable(&metrics, INSTRUMENTS), Some(3.0));
}

/// After a key stop, no Bybit request reaches `fake` (it saw `seen`) for
/// 10 minutes of the clock while `poll` is asked every 30 s; the gauge is 1.
async fn nothing_sent(
    c: &C,
    fake: &FakeHttp,
    mut poll: impl FnMut(u64) -> Option<Request>,
    case: &str,
) {
    let seen = fake.requests().len();
    for _ in 0..20 {
        c.clock.advance(30_000);
        assert_eq!(poll(c.clock.now_ms()), None, "{case}");
    }
    for p in [
        QUERY_API,
        INSTRUMENTS,
        "/v5/order/realtime",
        "/v5/position/list",
    ] {
        let e = c.client.get(p, &[]).await.unwrap_err();
        assert_eq!(e.class(), "auth", "{case} {p}");
    }
    assert_eq!(fake.requests().len(), seen, "{case}");
    let rejected = c.metrics.get("watcher_auth_rejected", &[]);
    assert_eq!(rejected, Some(1.0), "{case}");
}

#[tokio::test]
async fn an_auth_answer_at_start_stays_up_without_requests() {
    // Rejected at query-api (step 2) and at the settle coin (step 5).
    for (case, at_query, rejection, seen) in [
        ("query-api 10010", true, code(10010), 1),
        ("query-api 10003", true, code(10003), 1),
        ("query-api 33004", true, code(33004), 1),
        ("query-api HTTP 401", true, Answer::Reply(401, "".into()), 1),
        ("instruments-info 10004", false, code(10004), 2),
    ] {
        let fake = FakeHttp::start(code(10016));
        if at_query {
            fake.route(QUERY_API, [rejection]);
        } else {
            fake.route(QUERY_API, [ok(&good_key())]);
            fake.route(INSTRUMENTS, [rejection]);
        }
        let c = client(&fake);
        let mut s = start_of(&["BTCUSDT"]);
        let echo_at = c.clock.now_ms();
        let step = c.drive(&mut s.start).await;
        assert_eq!(step, Step::StayUpNoRequests, "{case}");
        assert_eq!(fake.requests().len(), seen, "{case}");
        // No wait: step 4's echo, if asked, answered at `echo_at`. The
        // first echo-only recheck is due 60 s after it (review 2 S1).
        assert_eq!(c.clock.now_ms(), echo_at, "{case}");
        if let Some(mut check) = s.start.ip_check() {
            assert_eq!(check.poll(echo_at + 59_999), None, "{case}");
            let req = check.poll(echo_at + 60_000);
            assert_eq!(req, Some(Request::Echo), "{case}");
        }
        // No exit and no request.
        nothing_sent(&c, &fake, |t| s.start.poll(t), case).await;
        // Visible: the gauge and the client's one error line; the start
        // itself adds no line.
        let head = "error: Bybit rejected the key";
        one_line(&c.lines, head, "until the watcher restarts", case);
        assert!(lines(&s.lines).is_empty(), "{case}");
        // At step 5 the key's IPs are known (step 4): the public IP is
        // still watched, with the echo only. At step 2 nothing is known.
        match s.start.ip_check() {
            None => assert!(at_query, "{case}"),
            Some(mut check) => {
                assert!(!at_query, "{case}");
                echo_only(&c, &fake, &mut check, &s.metrics, case).await;
            }
        }
    }
}

/// A recheck after a key stop: every `ip_check_ms` after its last answer it
/// asks the echo only (no Bybit request reaches `fake`) and compares it with
/// the key's last IPs that passed: 1 for the bound address, then 0 for
/// another one.
async fn echo_only(c: &C, fake: &FakeHttp, check: &mut IpCheck, m: &Metrics, case: &str) {
    let (seen, echoes) = (fake.requests().len(), c.echo.requests().len());
    let other = Answer::Reply(200, "198.51.100.4\n".into());
    c.echo
        .route("/ip", [Answer::Reply(200, format!("{IP}\n")), other]);
    for ip_ok in [1.0, 0.0] {
        let now = c.clock.now_ms();
        let req = check.poll(now);
        assert_eq!(req, Some(Request::Echo), "{case}");
        assert_eq!(check.poll(now), None, "{case}");
        let step = check.on_answer(now, c.ask(&Request::Echo).await);
        assert_eq!(step, wait(now + 60_000), "{case}");
        assert_eq!(m.get(PUBLIC_IP_OK, &[]), Some(ip_ok), "{case}");
        assert_eq!(check.poll(now + 59_999), None, "{case}");
        c.clock.set(now + 60_000);
    }
    assert_eq!(fake.requests().len(), seen, "{case}");
    assert_eq!(c.echo.requests().len(), echoes + 2, "{case}");
    assert_eq!(c.metrics.get("watcher_auth_rejected", &[]), Some(1.0));
}

/// A start of one symbol run to the end on answers: query-api at `T0`
/// (key expiring in 10 days), the echo answered at `echo_at`, the settle
/// coin at `done_at`; its recheck.
fn rechecked(echo_at: u64, done_at: u64, ip_check_ms: u64) -> (S, IpCheck) {
    let metrics = Arc::new(Metrics::new("h1", "w1"));
    let mut s = start_with(&["BTCUSDT"], ip_check_ms, metrics);
    let k = key(UID, false, json!([IP]), Some("2023-11-24T22:13:20Z"));
    assert_eq!(answer(&mut s.start, T0, &query_api(), Ok(k)), Step::Next);
    take(&mut s.start, T0, &Request::Echo);
    assert_eq!(s.start.on_answer(echo_at, Ok(json!(IP))), Step::Next);
    take(&mut s.start, echo_at, &instruments("BTCUSDT"));
    let row = Ok(instrument("BTCUSDT", "USDT"));
    assert_eq!(s.start.on_answer(done_at, row), Step::Done);
    assert_eq!(s.metrics.get(DAYS_LEFT, &[]), Some(10.0));
    assert_eq!(s.metrics.get(PUBLIC_IP_OK, &[]), Some(1.0));
    let check = s.start.ip_check().unwrap();
    (s, check)
}

/// Takes the recheck's request due at `now`: it must be `want`, and no
/// second one is due while it has no answer.
fn take_check(c: &mut IpCheck, now: u64, want: &Request) {
    assert_eq!(c.poll(now).as_ref(), Some(want), "at +{}", now - T0);
    assert_eq!(c.poll(now), None, "a request in flight is not asked again");
}

#[test]
fn the_public_ip_is_rechecked_every_60_s() {
    let echo_at = T0 + 1_000;
    let (s, mut check) = rechecked(echo_at, T0 + 1_500, 60_000);
    // The first recheck is due 60 s after step 4's echo answered.
    assert_eq!(check.poll(T0 + 1_500), None);
    assert_eq!(check.poll(echo_at + 59_999), None);
    let mut due = echo_at + 60_000;
    // Each later one 60 s after the previous recheck's last answer; the
    // key's list and expiry are read again every time.
    let other = "198.51.100.4";
    for (i, (ips, expired_at, echo, ip_ok, days)) in [
        (json!([IP]), "2023-11-24T22:13:20Z", IP, 1.0, Some(9.0)),
        (json!([other]), "2023-11-24T22:13:20Z", IP, 0.0, Some(9.0)),
        (
            json!([other]),
            "2023-12-24T22:13:20Z",
            other,
            1.0,
            Some(39.0),
        ),
        (json!([IP, other]), "1970-01-01T00:00:00Z", IP, 1.0, None),
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(check.poll(due - 1), None, "round {i}");
        take_check(&mut check, due, &query_api());
        let k = key(UID, false, ips, Some(expired_at));
        assert_eq!(check.on_answer(due + 200, Ok(k)), Step::Next, "round {i}");
        take_check(&mut check, due + 200, &Request::Echo);
        let (last, echo) = (due + 350, Ok(json!(echo)));
        assert_eq!(
            check.on_answer(last, echo),
            wait(last + 60_000),
            "round {i}"
        );
        assert_eq!(s.metrics.get(PUBLIC_IP_OK, &[]), Some(ip_ok), "round {i}");
        assert_eq!(s.metrics.get(DAYS_LEFT, &[]), days, "round {i}");
        due = last + 60_000;
    }
    // One warn line, for the round whose IP was not bound.
    let not_bound =
        "warn: public IP 203.0.113.7 is not among the key's bound IPs; watcher_public_ip_ok = 0";
    assert_eq!(lines(&s.lines), [not_bound]);

    // A start whose step 5 took longer than the interval: the first
    // recheck is due at once, still counted from the echo.
    let (_s, mut check) = rechecked(T0 + 1_000, T0 + 90_000, 60_000);
    take_check(&mut check, T0 + 90_000, &query_api());
    // The configured interval.
    let (_s, mut check) = rechecked(T0, T0, 10_000);
    assert_eq!(check.poll(T0 + 9_999), None);
    take_check(&mut check, T0 + 10_000, &query_api());
}

#[test]
fn a_failed_recheck_sets_public_ip_ok_0_and_keeps_days_left() {
    let mut cases = retried_cases();
    cases.push(("unreadable: no fields".into(), Ok(json!({})), 0));
    cases.push(("unreadable: ips a string".into(), Ok(bound(json!(IP))), 0));
    for (case, a, _) in cases {
        let (s, mut check) = rechecked(T0 + 1_000, T0 + 1_500, 60_000);
        take_check(&mut check, T0 + 61_000, &query_api());
        // A failed query-api: 0, days kept, no echo, the next recheck at
        // the normal pace (also after a 403: the client's pause answers it).
        let at = T0 + 61_300;
        assert_eq!(check.on_answer(at, a), wait(at + 60_000), "{case}");
        assert_eq!(s.metrics.get(PUBLIC_IP_OK, &[]), Some(0.0), "{case}");
        assert_eq!(s.metrics.get(DAYS_LEFT, &[]), Some(10.0), "{case}");
        one_line(
            &s.lines,
            "warn: IP recheck: query-api failed (",
            IP_OK_0,
            &case,
        );
        let counted = case.starts_with("unreadable").then_some(1.0);
        assert_eq!(unreadable(&s.metrics, QUERY_API), counted, "{case}");
        assert_eq!(check.poll(at), None, "{case}");
        assert_eq!(check.poll(at + 59_999), None, "{case}");
        take_check(&mut check, at + 60_000, &query_api());
        // The next recheck passes: 1 again.
        let at = at + 60_000;
        assert_eq!(check.on_answer(at, Ok(good_key())), Step::Next, "{case}");
        take_check(&mut check, at, &Request::Echo);
        check.on_answer(at, Ok(json!(IP)));
        assert_eq!(s.metrics.get(PUBLIC_IP_OK, &[]), Some(1.0), "{case}");
    }
    // A failed echo after a good query-api: 0 with one line, the days
    // refreshed from that query-api, the next recheck at the normal pace.
    for (case, a) in [
        ("no answer", Err(err("no_answer"))),
        ("not an address", Ok(json!("nope"))),
        ("paused", Err(err("ip_banned"))),
    ] {
        let (s, mut check) = rechecked(T0 + 1_000, T0 + 1_500, 60_000);
        take_check(&mut check, T0 + 61_000, &query_api());
        let k = key(UID, false, json!([IP]), Some("2023-11-23T22:13:20Z"));
        check.on_answer(T0 + 61_000, Ok(k));
        take_check(&mut check, T0 + 61_000, &Request::Echo);
        let at = T0 + 61_100;
        assert_eq!(check.on_answer(at, a), wait(at + 60_000), "{case}");
        assert_eq!(s.metrics.get(PUBLIC_IP_OK, &[]), Some(0.0), "{case}");
        assert_eq!(s.metrics.get(DAYS_LEFT, &[]), Some(8.0), "{case}");
        one_line(&s.lines, WARN_IP, IP_OK_0, case);
        assert_eq!(check.poll(T0 + 121_099), None, "{case}");
        take_check(&mut check, T0 + 121_100, &query_api());
    }
}

/// A fake whose query-api answers a good key, then `recheck`; a client of
/// it; the recheck after a start run through it, at its first due time.
async fn first_recheck(recheck: Answer) -> (FakeHttp, C, IpCheck) {
    let fake = FakeHttp::start(ok(&json!({"list": []})));
    fake.route(QUERY_API, [ok(&good_key()), recheck]);
    fake.route(INSTRUMENTS, [ok(&instrument("BTCUSDT", "USDT"))]);
    let c = client(&fake);
    let mut s = start_with(&["BTCUSDT"], 60_000, c.metrics.clone());
    assert_eq!(c.drive(&mut s.start).await, Step::Done);
    assert!(lines(&s.lines).is_empty());
    let check = s.start.ip_check().unwrap();
    c.clock.advance(60_000);
    (fake, c, check)
}

#[tokio::test]
async fn a_recheck_with_another_uid_a_master_key_or_no_bound_ip_stops_every_request() {
    for (case, recheck, why) in [
        ("other uid", key(4243, false, json!([IP]), None), OTHER_UID),
        ("master", key(UID, true, json!([IP]), None), MASTER),
        ("no bound ip", bound(json!([])), UNBOUND),
        ("star", bound(json!(["*"])), UNBOUND),
    ] {
        let (fake, c, mut check) = first_recheck(ok(&recheck)).await;
        let req = check.poll(c.clock.now_ms()).unwrap();
        assert_eq!(req, query_api(), "{case}");
        let step = check.on_answer(c.clock.now_ms(), c.ask(&req).await);
        let reason = format!("the key no longer passes the start check: query-api: {why}");
        assert!(
            matches!(&step, Step::StopRequests { reason: r } if *r == reason),
            "{case}: {step:?}"
        );
        apply(&c.client, &step);
        assert_eq!(c.client.auth_rejected(), Some(OWN_RULE_CODE), "{case}");
        // The echo's address is in no list that passes: not ok.
        assert_eq!(c.metrics.get(PUBLIC_IP_OK, &[]), Some(0.0), "{case}");
        // Every Bybit request stops until restart, as for a rejected key;
        // the recheck asks nothing more (no echo either).
        assert_eq!(fake.requests().len(), 3, "{case}");
        nothing_sent(&c, &fake, |t| check.poll(t), case).await;
        assert_eq!(c.echo.requests().len(), 1, "{case}");
        // One error line naming the rule; a second apply adds none.
        apply(&c.client, &step);
        let line = format!("error: {reason}; every Bybit request stops until the watcher restarts");
        assert_eq!(lines(&c.lines), [line], "{case}");
    }
    // A rejected key at the recheck: as at start, the client stopped. A
    // changed public IP is a usual cause of 10010, so the recheck goes on
    // with its echo at once and then the echo alone (review 1 M1).
    let (fake, c, mut check) = first_recheck(code(10010)).await;
    let req = check.poll(c.clock.now_ms()).unwrap();
    let step = check.on_answer(c.clock.now_ms(), c.ask(&req).await);
    assert_eq!(step, Step::Next);
    assert_eq!(c.client.auth_rejected(), Some(10010));
    assert_eq!(fake.requests().len(), 3);
    let metrics = c.metrics.clone();
    echo_only(&c, &fake, &mut check, &metrics, "10010").await;
    for p in [QUERY_API, "/v5/position/list"] {
        assert_eq!(c.client.get(p, &[]).await.unwrap_err().class(), "auth");
    }
    assert_eq!(fake.requests().len(), 3);
}

#[test]
fn an_answer_without_a_request_in_flight_changes_nothing() {
    let stray = || Ok(good_key());
    // Before the first request: query-api is still the first.
    let mut s = start_of(&["BTCUSDT"]);
    assert_eq!(s.start.on_answer(T0, stray()), Step::Next);
    // During a wait: the same wait, the same request at the same instant.
    take(&mut s.start, T0, &query_api());
    let until = T0 + 100 + RETRY_MS;
    assert_eq!(
        s.start.on_answer(T0 + 100, Err(err("no_answer"))),
        wait(until)
    );
    assert_eq!(s.start.on_answer(T0 + 200, stray()), wait(until));
    asks_nothing(&mut s.start, &[until - 1], "wait");
    assert_eq!(
        answer(&mut s.start, until, &query_api(), stray()),
        Step::Next
    );
    take(&mut s.start, until, &Request::Echo);
    assert_eq!(s.start.on_answer(until, Ok(json!(IP))), Step::Next);
    take(&mut s.start, until, &instruments("BTCUSDT"));
    let row = || Ok(instrument("BTCUSDT", "USDT"));
    assert_eq!(s.start.on_answer(until, row()), Step::Done);
    // After the end: the same end, nothing asked.
    assert_eq!(s.start.on_answer(until + 1, row()), Step::Done);
    asks_nothing(&mut s.start, &[until + 1], "done");
    let mut r = start_of(&["BTCUSDT"]);
    let master = Ok(key(UID, true, json!([IP]), None));
    let refused = refuse(&format!("query-api: {MASTER}"));
    assert_eq!(answer(&mut r.start, T0, &query_api(), master), refused);
    assert_eq!(r.start.on_answer(T0 + 1, stray()), refused);
    asks_nothing(&mut r.start, &[T0 + 1, T0 + 600_000], "refused");
    let mut u = start_of(&["BTCUSDT"]);
    let rejected = Err(RestError::Auth {
        code: 10010,
        msg: "m".into(),
    });
    let step = answer(&mut u.start, T0, &query_api(), rejected);
    assert_eq!(step, Step::StayUpNoRequests);
    assert_eq!(u.start.on_answer(T0 + 1, stray()), Step::StayUpNoRequests);
    asks_nothing(&mut u.start, &[T0 + 1, T0 + 600_000], "stayed up");

    // An idle recheck: the same due instant, and no gauge changed.
    let (s, mut check) = rechecked(T0 + 1_000, T0 + 1_500, 60_000);
    let due = T0 + 61_000;
    assert_eq!(check.on_answer(T0 + 2_000, stray()), wait(due));
    assert_eq!(
        check.on_answer(T0 + 2_000, Ok(json!("198.51.100.4"))),
        wait(due)
    );
    assert_eq!(s.metrics.get(PUBLIC_IP_OK, &[]), Some(1.0));
    assert_eq!(s.metrics.get(DAYS_LEFT, &[]), Some(10.0));
    assert_eq!(check.poll(due - 1), None);
    take_check(&mut check, due, &query_api());
    // The echo due: still the echo.
    assert_eq!(check.on_answer(due, stray()), Step::Next);
    assert_eq!(check.on_answer(due, stray()), Step::Next);
    take_check(&mut check, due, &Request::Echo);
    assert_eq!(check.on_answer(due, Ok(json!(IP))), wait(due + 60_000));
    // After a key stop: the echo alone, at the same instants.
    take_check(&mut check, due + 60_000, &query_api());
    let rejected = Err(RestError::Auth {
        code: 10010,
        msg: "m".into(),
    });
    let at = due + 60_000;
    assert_eq!(check.on_answer(at, rejected), Step::Next);
    take_check(&mut check, at, &Request::Echo);
    assert_eq!(check.on_answer(at, Ok(json!(IP))), wait(at + 60_000));
    assert_eq!(check.on_answer(at + 1, stray()), wait(at + 60_000));
    assert_eq!(check.poll(at + 59_999), None);
    take_check(&mut check, at + 60_000, &Request::Echo);
    // After a rule stop: the same stop, nothing asked.
    let (_s, mut check) = rechecked(T0 + 1_000, T0 + 1_500, 60_000);
    take_check(&mut check, due, &query_api());
    let step = check.on_answer(due, Ok(key(UID, true, json!([IP]), None)));
    assert!(matches!(step, Step::StopRequests { .. }), "{step:?}");
    assert_eq!(check.on_answer(due + 1, stray()), step);
    for t in [due + 1, due + 60_000, due + 600_000] {
        assert_eq!(check.poll(t), None);
    }
}

/// A key that can cancel (U19b2a): read-write, `ContractTrade` with
/// `Order`, no `Withdraw` anywhere.
fn cancel_key(read_only: Value, permissions: Value) -> Value {
    let mut v = good_key();
    v["readOnly"] = read_only;
    v["permissions"] = permissions;
    v
}

fn can_cancel() -> Value {
    cancel_key(
        json!(0),
        json!({"ContractTrade": ["Order", "Position"], "Wallet": ["AccountTransfer"], "NFT": []}),
    )
}

/// A start with `[kill]` (the key must cancel) of BTCUSDT.
fn start_kill(metrics: Arc<Metrics>) -> S {
    let (log, lines) = watcher::log::collect();
    let cfg = config_with(&["BTCUSDT"], 60_000, "[kill]\n");
    let start = Start::new(&cfg, metrics.clone(), log, T0);
    S {
        start,
        metrics,
        lines,
    }
}

#[tokio::test]
async fn a_key_that_must_cancel_needs_read_write_order_and_no_withdraw() {
    const READ_ONLY: &str = "the API key is read-only (readOnly 1); with [kill] the watcher cancels orders and needs a read-write key";
    const NO_ORDER: &str = "the API key has no ContractTrade Order permission; with [kill] the watcher needs it to cancel orders";
    let withdraw = |list: &str| {
        format!("the API key has the Withdraw permission ({list}); the watcher's key must not be able to withdraw")
    };
    let refusals = [
        ("read only", cancel_key(json!(1), can_cancel()["permissions"].clone()), READ_ONLY.to_string()),
        (
            "no Order",
            cancel_key(json!(0), json!({"ContractTrade": ["Position"], "Wallet": []})),
            NO_ORDER.to_string(),
        ),
        (
            "no ContractTrade",
            cancel_key(json!(0), json!({"Spot": ["SpotTrade"]})),
            NO_ORDER.to_string(),
        ),
        (
            "Withdraw in Wallet",
            cancel_key(json!(0), json!({"ContractTrade": ["Order"], "Wallet": ["Withdraw"]})),
            withdraw("Wallet"),
        ),
        (
            "Withdraw in another list",
            cancel_key(json!(0), json!({"ContractTrade": ["Order", "Withdraw"]})),
            withdraw("ContractTrade"),
        ),
    ];
    // With [kill]: each rule refuses the start (exit 2) with the rule named.
    for (case, v, why) in &refusals {
        let mut s = start_kill(Arc::new(Metrics::new("h1", "w1")));
        take(&mut s.start, T0, &query_api());
        let want = refuse(&format!("query-api: {why}"));
        assert_eq!(s.start.on_answer(T0 + 5, Ok(v.clone())), want, "{case}");
        asks_nothing(&mut s.start, &[T0 + 5, T0 + 5 + RETRY_MS], case);
    }
    // A key that can cancel goes on to the echo.
    let mut s = start_kill(Arc::new(Metrics::new("h1", "w1")));
    assert_eq!(answer(&mut s.start, T0, &query_api(), Ok(can_cancel())), Step::Next);
    take(&mut s.start, T0, &Request::Echo);
    // Fields that cannot be read: asked again after 5 s, never passed.
    for (case, v) in [
        ("no readOnly", {
            let mut v = can_cancel();
            v.as_object_mut().unwrap().remove("readOnly");
            v
        }),
        ("readOnly a boolean", cancel_key(json!(false), can_cancel()["permissions"].clone())),
        ("readOnly a string", cancel_key(json!("0"), can_cancel()["permissions"].clone())),
        ("permissions not an object", cancel_key(json!(0), json!(["Order"]))),
        ("ContractTrade not a list", cancel_key(json!(0), json!({"ContractTrade": "Order"}))),
        ("a list with a number", cancel_key(json!(0), json!({"ContractTrade": ["Order"], "Wallet": [1]}))),
    ] {
        let metrics = Arc::new(Metrics::new("h1", "w1"));
        let mut s = start_kill(metrics.clone());
        let step = answer(&mut s.start, T0, &query_api(), Ok(v));
        assert_eq!(step, wait(T0 + RETRY_MS), "{case}");
        assert_eq!(unreadable(&metrics, QUERY_API), Some(1.0), "{case}");
        asks_nothing(&mut s.start, &[T0 + RETRY_MS - 1], case);
        take(&mut s.start, T0 + RETRY_MS, &query_api());
    }
    // Without [kill] these fields are not read: a read-only key without
    // Order passes (the watcher of U19a only reads).
    for (case, v, _) in &refusals {
        let mut s = start_of(&["BTCUSDT"]);
        let step = answer(&mut s.start, T0, &query_api(), Ok(v.clone()));
        assert_eq!(step, Step::Next, "{case}");
    }
    let mut s = start_of(&["BTCUSDT"]);
    let step = answer(&mut s.start, T0, &query_api(), Ok(cancel_key(json!("x"), json!(7))));
    assert_eq!(step, Step::Next, "unread fields");

    // The recheck applies the same rules: a key that no longer can cancel
    // stops every Bybit request until restart (a rule stop).
    for (case, v, why) in &refusals {
        let fake = FakeHttp::start(ok(&json!({"list": []})));
        fake.route(QUERY_API, [ok(&can_cancel()), ok(v)]);
        fake.route(INSTRUMENTS, [ok(&instrument("BTCUSDT", "USDT"))]);
        let c = client(&fake);
        let mut s = start_kill(c.metrics.clone());
        assert_eq!(c.drive(&mut s.start).await, Step::Done, "{case}");
        let mut check = s.start.ip_check().unwrap();
        c.clock.advance(60_000);
        let req = check.poll(c.clock.now_ms()).unwrap();
        assert_eq!(req, query_api(), "{case}");
        let step = check.on_answer(c.clock.now_ms(), c.ask(&req).await);
        let reason = format!("the key no longer passes the start check: query-api: {why}");
        assert_eq!(step, Step::StopRequests { reason }, "{case}");
        apply(&c.client, &step);
        assert_eq!(c.client.auth_rejected(), Some(OWN_RULE_CODE), "{case}");
        assert_eq!(c.metrics.get(PUBLIC_IP_OK, &[]), Some(0.0), "{case}");
        assert_eq!(fake.requests().len(), 3, "{case}");
        nothing_sent(&c, &fake, |t| check.poll(t), case).await;
        // The cancel-all is a Bybit request like the others: not sent.
        assert_eq!(c.client.cancel_all().await.unwrap_err().class(), "auth", "{case}");
        assert_eq!(fake.requests().len(), 3, "{case}");
    }
}
