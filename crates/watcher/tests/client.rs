//! The watcher's own Bybit client (C-WATCH part 3) against a loopback fake
//! (`http-fake`), on a manual clock: the request guard, the headers and the
//! signed query, the answer classes, the never-retried key rejection and
//! the 403 pause; the one signed POST, the cancel-all (U19b2a).

#![allow(clippy::unwrap_used)]

use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use http_fake::{Answer, FakeHttp};
use serde_json::json;
use watcher::clock::ManualClock;
use watcher::metrics::Metrics;
use watcher::rest::{
    CANCEL_ALL, CANCEL_ALL_BODY, CancelAll, Client, ClientConfig, Credentials, GUARD_MAX,
    GUARD_WINDOW_MS, Guard, RestError,
};
use watcher::sign::post_signature;

const T0: u64 = 1_700_000_000_000;

/// A client of the fake on a manual clock at `T0`, with its metrics and
/// the lines it logs.
struct C {
    client: Client,
    clock: Arc<ManualClock>,
    metrics: Arc<Metrics>,
    lines: Arc<Mutex<Vec<String>>>,
}

fn client_of(base: &str, timeout_ms: u64) -> C {
    let clock = Arc::new(ManualClock::new(T0));
    let metrics = Arc::new(Metrics::new("h1", "w1"));
    let (log, lines) = watcher::log::collect();
    let cfg = ClientConfig {
        base: base.to_string(),
        recv_window_ms: 5000,
        request_timeout_ms: timeout_ms,
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

fn client(fake: &FakeHttp) -> C {
    client_of(&fake.url(), 300)
}

fn ok(result: &str) -> Answer {
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

fn linear() -> Vec<(&'static str, String)> {
    vec![
        ("symbol", "BTCUSDT".into()),
        ("settleCoin", "USDT".into()),
        ("category", "linear".into()),
    ]
}

#[test]
fn the_guard_holds_requests_beyond_60_in_10_s() {
    assert_eq!((GUARD_MAX, GUARD_WINDOW_MS), (60, 10_000));
    // 60 requests 100 ms apart are let through; the 61st waits for the
    // first to leave the window.
    let mut g = Guard::default();
    for i in 0..60 {
        assert_eq!(g.admit(T0 + i * 100), Ok(()), "request {i}");
    }
    assert_eq!(g.admit(T0 + 5_950), Err(T0 + 10_000));
    assert_eq!(g.admit(T0 + 9_999), Err(T0 + 10_000));
    // A refused request is not recorded: at the instant it was given it goes.
    assert_eq!(g.admit(T0 + 10_000), Ok(()));
    assert_eq!(g.admit(T0 + 10_000), Err(T0 + 10_100));
    assert_eq!(g.admit(T0 + 10_100), Ok(()));

    // A clock that steps back holds the requests until it passes the old
    // values again.
    let mut g = Guard::default();
    for _ in 0..60 {
        g.admit(T0).unwrap();
    }
    assert_eq!(g.admit(T0 - 5_000), Err(T0 + 10_000));
    assert_eq!(g.admit(T0 + 10_000), Ok(()));

    // A runaway caller that asks every 10 ms for a simulated minute, and
    // when refused waits for the given instant: never more than 60 in any
    // 10 s window, and every refusal names an instant at which it goes.
    let mut g = Guard::default();
    let mut sent: Vec<u64> = Vec::new();
    let mut now = T0;
    let mut steps = 0;
    let mut waited = false;
    while now < T0 + 60_000 {
        steps += 1;
        assert!(steps < 100_000, "bounded loop");
        match g.admit(now) {
            Ok(()) => {
                sent.push(now);
                now += 10;
                waited = false;
            }
            Err(at) => {
                assert!(!waited, "refused again at the instant it was given");
                assert!(at > now, "the instant {at} is after {now}");
                now = at;
                waited = true;
            }
        }
    }
    for (i, &t) in sent.iter().enumerate() {
        let in_window = sent[i..].iter().take_while(|&&u| u < t + 10_000).count();
        assert!(in_window <= 60, "{in_window} requests in the 10 s from {t}");
    }
    // Six full windows in a minute, no more.
    assert_eq!(sent.len(), 360);
}

#[tokio::test]
async fn every_request_carries_the_five_headers_and_the_signed_query() {
    let fake = FakeHttp::start(Answer::Close);
    fake.route(
        "/v5/position/list",
        [ok(r#"{"list":[{"symbol":"BTCUSDT"}]}"#)],
    );
    let c = client(&fake);
    // Counted before it is sent: the fake sees the counter already up.
    let seen = Arc::new(Mutex::new(Vec::new()));
    let (m, s) = (c.metrics.clone(), seen.clone());
    fake.on_request(move |_| {
        let v = m.get(
            "watcher_requests_total",
            &[("endpoint", "/v5/position/list")],
        );
        s.lock().unwrap().push(v);
    });
    let v = c.client.get("/v5/position/list", &linear()).await.unwrap();
    assert_eq!(v, json!({"list": [{"symbol": "BTCUSDT"}]}));
    assert_eq!(*seen.lock().unwrap(), [Some(1.0)]);

    let reqs = fake.requests();
    assert_eq!(reqs.len(), 1);
    let r = &reqs[0];
    assert_eq!(r.method, "GET");
    assert_eq!(
        r.target,
        "/v5/position/list?category=linear&settleCoin=USDT&symbol=BTCUSDT"
    );
    assert!(r.body.is_empty());
    assert_eq!(r.header("x-bapi-api-key"), Some("WKEY"));
    assert_eq!(r.header("x-bapi-timestamp"), Some("1700000000000"));
    assert_eq!(r.header("x-bapi-recv-window"), Some("5000"));
    assert_eq!(r.header("x-bapi-sign-type"), Some("2"));
    // Python hmac over "1700000000000WKEY5000" + the query sent (tests/sign.rs).
    assert_eq!(
        r.header("x-bapi-sign"),
        Some("97cc2c39beda79a8c5988ad70fe71a8696a99f45ea55b8288ef0ebcefa1c8103")
    );
    // The timestamp is the injected clock at the send.
    c.clock.advance(1_234);
    c.client.get("/v5/position/list", &linear()).await.unwrap();
    assert_eq!(
        fake.requests()[1].header("x-bapi-timestamp"),
        Some("1700000001234")
    );
    assert_eq!(
        c.metrics.get(
            "watcher_requests_total",
            &[("endpoint", "/v5/position/list")]
        ),
        Some(2.0)
    );

    // A value that is not sent as is: refused before anything is sent.
    let e = c
        .client
        .get("/v5/order/realtime", &[("cursor", "a&b".into())])
        .await
        .unwrap_err();
    assert!(matches!(e, RestError::NotSent(_)), "{e:?}");
    assert_eq!(fake.requests().len(), 2);
    assert_eq!(
        c.metrics.get(
            "watcher_requests_total",
            &[("endpoint", "/v5/order/realtime")]
        ),
        None
    );
    assert_eq!(
        c.metrics.get(
            "watcher_request_errors_total",
            &[("endpoint", "/v5/order/realtime"), ("class", "not_sent")]
        ),
        Some(1.0)
    );
    assert!(c.lines.lock().unwrap().is_empty());
}

/// The client waits at a full guard and sends when the clock reaches the
/// instant the guard named: the request is held, not dropped. Real time:
/// the client re-reads the clock while it waits.
#[tokio::test]
async fn a_request_beyond_the_guard_waits_for_the_window() {
    let fake = FakeHttp::start(ok("{}"));
    let c = client(&fake);
    for _ in 0..GUARD_MAX {
        c.client
            .get("/v5/account/wallet-balance", &[])
            .await
            .unwrap();
    }
    let client = Arc::new(c.client);
    let cl = client.clone();
    let late = tokio::spawn(async move { cl.get("/v5/order/realtime", &[]).await });
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert_eq!(fake.requests().len(), GUARD_MAX, "held at the guard");
    assert!(!late.is_finished());
    c.clock.advance(GUARD_WINDOW_MS - 1);
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert_eq!(
        fake.requests().len(),
        GUARD_MAX,
        "held until the window ends"
    );
    c.clock.advance(1);
    let r = tokio::time::timeout(Duration::from_secs(5), late).await;
    assert!(matches!(r, Ok(Ok(Ok(_)))), "{r:?}");
    let reqs = fake.requests();
    assert_eq!(reqs.len(), GUARD_MAX + 1);
    // Signed with the clock at the send, after the wait: a timestamp taken
    // before it would be up to 10 s old, outside a 5 s recv window.
    assert_eq!(
        reqs[GUARD_MAX].header("x-bapi-timestamp"),
        Some((T0 + GUARD_WINDOW_MS).to_string().as_str())
    );
}

#[tokio::test]
async fn answers_are_classified() {
    let p = "/v5/order/realtime";
    let cases: Vec<(Answer, &str)> = vec![
        (Answer::Hang, "no_answer"),
        (Answer::Close, "no_answer"),
        (Answer::Reply(500, "oops".into()), "no_answer"),
        (Answer::Reply(502, String::new()), "no_answer"),
        (Answer::Reply(200, "not json".into()), "no_answer"),
        (
            Answer::Reply(200, r#"{"retMsg":"OK","result":{}}"#.into()),
            "no_answer",
        ),
        (
            Answer::Reply(200, r#"{"retCode":"0","result":{}}"#.into()),
            "no_answer",
        ),
        (code(10016), "no_answer"),
        (Answer::Reply(429, "slow down".into()), "rate_limited"),
        (code(10006), "rate_limited"),
        (code(10002), "clock"),
        (code(10001), "refused"),
        (code(110001), "refused"),
        (code(-1), "refused"),
    ];
    for (answer, class) in cases {
        let fake = FakeHttp::start(Answer::Close);
        fake.route(p, [answer.clone()]);
        let c = client(&fake);
        let e = c.client.get(p, &[]).await.unwrap_err();
        assert_eq!(e.class(), class, "{answer:?}: {e:?}");
        assert_eq!(
            c.metrics.get(
                "watcher_request_errors_total",
                &[("endpoint", p), ("class", class)]
            ),
            Some(1.0),
            "{answer:?}"
        );
        // None of these stops or pauses the client: the next request goes.
        fake.route(p, [ok("{}")]);
        c.client.get(p, &[]).await.unwrap();
        assert_eq!(fake.requests().len(), 2, "{answer:?}");
        assert_eq!(c.metrics.get("watcher_auth_rejected", &[]), Some(0.0));
    }
    // retCode 0: the result, or null when there is none (the check decides
    // whether it can read the rows: `unreadable`).
    let fake = FakeHttp::start(Answer::Close);
    let c = client(&fake);
    fake.route(p, [ok(r#"{"list":[1]}"#)]);
    assert_eq!(c.client.get(p, &[]).await.unwrap(), json!({"list": [1]}));
    fake.route(
        p,
        [Answer::Reply(200, r#"{"retCode":0,"retMsg":"OK"}"#.into())],
    );
    assert_eq!(c.client.get(p, &[]).await.unwrap(), serde_json::Value::Null);
    let e = c.client.unreadable(p, "row 3 has no orderId");
    assert_eq!(e.class(), "unreadable");
    assert_eq!(
        c.metrics.get(
            "watcher_request_errors_total",
            &[("endpoint", p), ("class", "unreadable")]
        ),
        Some(1.0)
    );
    // A message of the venue (retMsg, or the body of a status) is kept to
    // 200 characters on one line: control characters and the Unicode line
    // and paragraph separators become spaces (review 2 N2).
    let long = format!("line one\nline two\r\t\u{2028}\u{2029}{}", "x".repeat(400));
    let body = serde_json::to_string(&json!({"retCode": 10001, "retMsg": long})).unwrap();
    for answer in [
        Answer::Reply(200, body),
        Answer::Reply(500, long.clone()),
        Answer::Reply(429, long.clone()),
    ] {
        fake.route(p, [answer.clone()]);
        let e = c.client.get(p, &[]).await.unwrap_err();
        let msg = match &e {
            RestError::Refused { msg, .. } => msg.clone(),
            RestError::NoAnswer(m) | RestError::RateLimited(m) => m.clone(),
            e => panic!("{answer:?}: {e:?}"),
        };
        assert!(msg.chars().count() <= 210, "{answer:?}: {msg:?}");
        assert!(
            msg.contains("line one line two    x"),
            "{answer:?}: {msg:?}"
        );
        assert!(
            !e.to_string()
                .chars()
                .any(|c| c.is_control() || matches!(c, '\u{2028}' | '\u{2029}')),
            "{e:?}"
        );
    }
    // Connection refused: nothing sent.
    let c = client_of(&format!("http://{}", http_fake::dead_addr()), 300);
    let e = c.client.get(p, &[]).await.unwrap_err();
    assert_eq!(e.class(), "not_sent", "{e:?}");
    assert_eq!(
        c.metrics.get("watcher_requests_total", &[("endpoint", p)]),
        Some(1.0)
    );
}

/// Bybit paths the later checks use; every one stops after a rejection.
const PATHS: [&str; 6] = [
    "/v5/user/query-api",
    "/v5/order/realtime",
    "/v5/position/list",
    "/v5/execution/list",
    "/v5/account/wallet-balance",
    "/v5/market/instruments-info",
];

#[tokio::test]
async fn auth_answers_are_never_retried() {
    let answers = [
        (10010, code(10010)),
        (10003, code(10003)),
        (33004, code(33004)),
        // The venue's 10004 message quotes the string it signed, over lines.
        (
            10004,
            Answer::Reply(
                200,
                serde_json::to_string(&json!({
                    "retCode": 10004,
                    "retMsg": format!("error sign!\norigin_string[{}]", "y".repeat(400)),
                }))
                .unwrap(),
            ),
        ),
        (10005, code(10005)),
        (10007, code(10007)),
        (401, Answer::Reply(401, "no".into())),
    ];
    for (c_want, answer) in answers {
        let fake = FakeHttp::start(ok("{}"));
        fake.route("/v5/user/query-api", [answer.clone()]);
        let echo = FakeHttp::start(Answer::Reply(200, "203.0.113.7".into()));
        let c = client(&fake);
        let e = c.client.get("/v5/user/query-api", &[]).await.unwrap_err();
        assert!(
            matches!(&e, RestError::Auth { code, .. } if *code == c_want),
            "{c_want}: {e:?}"
        );
        assert_eq!(c.metrics.get("watcher_auth_rejected", &[]), Some(1.0));
        assert_eq!(c.client.auth_rejected(), Some(c_want));
        // Ten minutes on the manual clock, every path each second: nothing
        // more reaches the venue.
        for _ in 0..600 {
            c.clock.advance(1_000);
            for p in PATHS {
                let e = c.client.get(p, &linear()).await.unwrap_err();
                assert_eq!(e.class(), "auth", "{c_want} {p}: {e:?}");
            }
        }
        assert_eq!(fake.requests().len(), 1, "{c_want}");
        // One error line with the code, not one per refused request.
        let lines = c.lines.lock().unwrap().clone();
        assert_eq!(lines.len(), 1, "{c_want}: {lines:?}");
        assert!(lines[0].contains(&c_want.to_string()), "{lines:?}");
        assert!(!lines[0].chars().any(char::is_control), "{lines:?}");
        assert!(lines[0].chars().count() < 400, "{lines:?}");
        assert_eq!(
            c.metrics.get(
                "watcher_request_errors_total",
                &[("endpoint", "/v5/user/query-api"), ("class", "auth")]
            ),
            Some(1.0)
        );
        // The IP echo is not a Bybit request: it still goes (a changed
        // public IP is a usual cause of 10010).
        let ip = c.client.echo(&format!("{}/ip", echo.url())).await.unwrap();
        assert_eq!(ip, "203.0.113.7");
        assert_eq!(
            c.metrics
                .get("watcher_requests_total", &[("endpoint", "ip_echo")]),
            Some(1.0)
        );
    }
}

#[tokio::test]
async fn http_403_pauses_every_request_for_600_s() {
    let fake = FakeHttp::start(ok("{}"));
    fake.route(
        "/v5/position/list",
        [Answer::Reply(403, "access too frequent".into()), ok("{}")],
    );
    let echo = FakeHttp::start(Answer::Reply(200, "203.0.113.7\n".into()));
    let echo_url = format!("{}/ip", echo.url());
    let c = client(&fake);
    // The answer comes 500 ms after the send: the pause runs from the
    // answer.
    let clock = c.clock.clone();
    fake.on_request(move |_| clock.advance(500));
    let e = c
        .client
        .get("/v5/position/list", &linear())
        .await
        .unwrap_err();
    assert_eq!(e.class(), "ip_banned", "{e:?}");
    let end = T0 + 500 + 600_000;
    assert_eq!(c.client.paused_until(), Some(end));
    assert_eq!(c.metrics.get("watcher_auth_rejected", &[]), Some(0.0));
    for at in [T0 + 500, T0 + 60_500, T0 + 60_000 * 9, end - 1] {
        c.clock.set(at);
        for p in PATHS {
            let e = c.client.get(p, &[]).await.unwrap_err();
            assert_eq!(e.class(), "ip_banned", "{p} at +{}: {e:?}", at - T0);
        }
        let e = c.client.echo(&echo_url).await.unwrap_err();
        assert_eq!(e.class(), "ip_banned", "echo at +{}", at - T0);
    }
    assert_eq!(fake.requests().len(), 1);
    assert!(echo.requests().is_empty());
    assert_eq!(
        c.metrics.get(
            "watcher_request_errors_total",
            &[("endpoint", "/v5/position/list"), ("class", "ip_banned")]
        ),
        Some(1.0)
    );
    assert_eq!(c.lines.lock().unwrap().len(), 1);
    // At the end of the pause every request goes again.
    c.clock.set(end);
    c.client.get("/v5/position/list", &linear()).await.unwrap();
    assert_eq!(c.client.echo(&echo_url).await.unwrap(), "203.0.113.7");
    assert_eq!(fake.requests().len(), 2);
    // The echo's own 403 is no answer from the echo, not a Bybit ban.
    echo.set_default(Answer::Reply(403, String::new()));
    let e = c.client.echo(&echo_url).await.unwrap_err();
    assert_eq!(e.class(), "no_answer");
    c.client.get("/v5/position/list", &linear()).await.unwrap();
}

/// A loopback server that answers each of at most 16 connections with
/// `status` and `location: <location>`, and counts the requests it read.
fn redirector(status: u16, location: String) -> (String, Arc<AtomicUsize>) {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    let n = Arc::new(AtomicUsize::new(0));
    let seen = n.clone();
    std::thread::spawn(move || {
        for s in l.incoming().take(16) {
            let Ok(mut s) = s else { return };
            let mut buf = Vec::new();
            let mut chunk = [0u8; 4096];
            while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
                match s.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(k) => buf.extend_from_slice(&chunk[..k]),
                }
            }
            seen.fetch_add(1, Ordering::SeqCst);
            let head = format!(
                "HTTP/1.1 {status} Moved\r\nlocation: {location}\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
            );
            let _ = s.write_all(head.as_bytes());
        }
    });
    (url, n)
}

/// A redirect is not followed: following it would send the signed request
/// (key and signature) again, to another host.
#[tokio::test]
async fn a_redirect_is_not_followed() {
    let other = FakeHttp::start(ok("{}"));
    for status in [301, 302, 307, 308] {
        let (url, seen) = redirector(status, format!("{}/v5/position/list", other.url()));
        let c = client_of(&url, 2_000);
        let e = c
            .client
            .get("/v5/position/list", &linear())
            .await
            .unwrap_err();
        assert_eq!(e.class(), "no_answer", "{status}: {e:?}");
        let e = c.client.echo(&format!("{url}/ip")).await.unwrap_err();
        assert_eq!(e.class(), "no_answer", "{status} echo: {e:?}");
        assert_eq!(seen.load(Ordering::SeqCst), 2, "{status}");
        assert!(
            other.requests().is_empty(),
            "{status}: {:?}",
            other.requests()
        );
    }
}

/// A loopback server that takes at most 8 connections in groups of
/// `group`: it reads one request on each connection of a group, then writes
/// `answer` (status line, headers and body as raw text) to all of them and
/// closes them. It counts the requests it read.
fn raw_server(group: usize, answer: String) -> (String, Arc<AtomicUsize>) {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    let n = Arc::new(AtomicUsize::new(0));
    let seen = n.clone();
    std::thread::spawn(move || {
        let mut held = Vec::new();
        for s in l.incoming().take(8) {
            let Ok(mut s) = s else { return };
            let mut buf = Vec::new();
            let mut chunk = [0u8; 4096];
            while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
                match s.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(k) => buf.extend_from_slice(&chunk[..k]),
                }
            }
            seen.fetch_add(1, Ordering::SeqCst);
            held.push(s);
            if held.len() == group {
                for mut s in held.drain(..) {
                    let _ = s.write_all(answer.as_bytes());
                }
            }
        }
    });
    (url, n)
}

/// The status counts when the body is cut: a 401 or 403 whose connection
/// closes before the whole body still latches the key stop or starts the
/// 403 pause, and the next request is not sent (review 2 S1).
#[tokio::test]
async fn a_status_counts_when_its_body_is_cut() {
    for (status, class) in [
        (401, "auth"),
        (403, "ip_banned"),
        (429, "rate_limited"),
        (500, "no_answer"),
        (200, "no_answer"),
    ] {
        let (url, seen) = raw_server(
            1,
            format!(
                "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: 100\r\nconnection: close\r\n\r\nshort"
            ),
        );
        let c = client_of(&url, 2_000);
        let e = c
            .client
            .get("/v5/position/list", &linear())
            .await
            .unwrap_err();
        assert_eq!(e.class(), class, "{status}: {e:?}");
        assert!(!e.to_string().chars().any(char::is_control), "{e}");
        let stops = matches!(status, 401 | 403);
        assert_eq!(
            c.client.auth_rejected(),
            (status == 401).then_some(401),
            "{status}"
        );
        assert_eq!(
            c.client.paused_until(),
            (status == 403).then_some(T0 + 600_000),
            "{status}"
        );
        assert_eq!(
            c.lines.lock().unwrap().len(),
            usize::from(stops),
            "{status}"
        );
        // The next request: not sent after a cut 401 or 403, sent after
        // the others.
        let e = c
            .client
            .get("/v5/position/list", &linear())
            .await
            .unwrap_err();
        assert_eq!(e.class(), class, "{status} next: {e:?}");
        assert_eq!(
            seen.load(Ordering::SeqCst),
            if stops { 1 } else { 2 },
            "{status}"
        );
    }
}

/// Two requests in flight that both get a key rejection write one error
/// line (review 2 N1).
#[tokio::test]
async fn two_rejections_in_flight_write_one_error_line() {
    let body = r#"{"retCode":10010,"retMsg":"m10010","result":{},"time":1}"#;
    let (url, seen) = raw_server(
        2,
        format!(
            "HTTP/1.1 200 X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        ),
    );
    let c = client_of(&url, 5_000);
    let params = linear();
    let (a, b) = tokio::time::timeout(Duration::from_secs(10), async {
        tokio::join!(
            c.client.get("/v5/position/list", &params),
            c.client.get("/v5/order/realtime", &params),
        )
    })
    .await
    .unwrap();
    assert_eq!(a.unwrap_err().class(), "auth");
    assert_eq!(b.unwrap_err().class(), "auth");
    assert_eq!(seen.load(Ordering::SeqCst), 2);
    assert_eq!(c.client.auth_rejected(), Some(10010));
    let lines = c.lines.lock().unwrap().clone();
    assert_eq!(lines.len(), 1, "{lines:?}");
}

/// A request that waits at a full guard looks at the key rejection and the
/// 403 pause again when it wakes: an answer that came while it waited
/// stops it. Real time: the guard's last place goes to a request whose
/// answer the fake holds until the waiter is waiting.
#[tokio::test]
async fn a_request_waiting_at_the_guard_sees_a_later_rejection_or_pause() {
    let held = "/v5/user/query-api";
    for (answer, class) in [
        (code(10010), "auth"),
        (
            Answer::Reply(403, "access too frequent".into()),
            "ip_banned",
        ),
    ] {
        let fake = FakeHttp::start(ok("{}"));
        fake.route(held, [answer.clone()]);
        let c = client_of(&fake.url(), 10_000);
        for _ in 0..GUARD_MAX - 1 {
            c.client
                .get("/v5/account/wallet-balance", &[])
                .await
                .unwrap();
        }
        let arrived = Arc::new(AtomicBool::new(false));
        let release = Arc::new(AtomicBool::new(false));
        let (a, r) = (arrived.clone(), release.clone());
        fake.on_request(move |req| {
            if req.path() == held {
                a.store(true, Ordering::SeqCst);
                for _ in 0..2_000 {
                    if r.load(Ordering::SeqCst) {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
            }
        });
        let client = Arc::new(c.client);
        let cl = client.clone();
        let first = tokio::spawn(async move { cl.get(held, &[]).await });
        for _ in 0..1_000 {
            if arrived.load(Ordering::SeqCst) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        assert!(
            arrived.load(Ordering::SeqCst),
            "{class}: the held request arrived"
        );
        let cl = client.clone();
        let waiter = tokio::spawn(async move { cl.get("/v5/order/realtime", &[]).await });
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(!waiter.is_finished(), "{class}: waits at the full guard");
        release.store(true, Ordering::SeqCst);
        let e = tokio::time::timeout(Duration::from_secs(5), first)
            .await
            .unwrap()
            .unwrap()
            .unwrap_err();
        assert_eq!(e.class(), class, "{e:?}");
        // The guard's window ends; the waiter wakes into the stop or pause.
        c.clock.advance(GUARD_WINDOW_MS);
        let e = tokio::time::timeout(Duration::from_secs(5), waiter)
            .await
            .unwrap()
            .unwrap()
            .unwrap_err();
        assert_eq!(e.class(), class, "waiter: {e:?}");
        assert_eq!(fake.requests().len(), GUARD_MAX, "{class}");
    }
}

/// U19b1c: the text of the key stop for the `auth` P1, kept from the
/// answer (or the rule) that latched it; nothing later changes it.
#[tokio::test]
async fn auth_stop_keeps_the_first_text() {
    // A retCode: its retMsg, as one line of at most 200 characters.
    let fake = FakeHttp::start(ok("{}"));
    let long = format!("error sign!\norigin_string[{}]", "y".repeat(400));
    fake.route(
        "/v5/user/query-api",
        [Answer::Reply(
            200,
            json!({"retCode": 10004, "retMsg": long}).to_string(),
        )],
    );
    let c = client(&fake);
    assert_eq!(c.client.auth_stop(), None);
    c.client.get("/v5/user/query-api", &[]).await.unwrap_err();
    let s = c.client.auth_stop().unwrap();
    assert_eq!((s.code, s.http), (10004, false));
    assert!(s.text.starts_with("error sign! origin_string[yyy"), "{s:?}");
    assert_eq!(s.text.chars().count(), 200);
    // A later stop of the watcher's own and later refused requests do not
    // change it.
    c.client.stop_key("the key no longer passes the start check");
    c.client.get("/v5/order/realtime", &linear()).await.unwrap_err();
    assert_eq!(c.client.auth_stop(), Some(s));
    assert_eq!(c.client.auth_rejected(), Some(10004));

    // HTTP 401: `http`, the body as the text.
    let fake = FakeHttp::start(ok("{}"));
    fake.route(
        "/v5/user/query-api",
        [Answer::Reply(401, "invalid api key".into())],
    );
    let c = client(&fake);
    c.client.get("/v5/user/query-api", &[]).await.unwrap_err();
    let s = c.client.auth_stop().unwrap();
    assert_eq!((s.code, s.http, s.text.as_str()), (401, true, "invalid api key"));

    // The rule stop: code -1 and its reason; a second stop does not change
    // it, nor does a later rejection (nothing is sent after a stop).
    let fake = FakeHttp::start(code(10010));
    let c = client(&fake);
    c.client.stop_key("query-api: the API key is the master account's\nnow");
    c.client.stop_key("another reason");
    c.client.get("/v5/order/realtime", &linear()).await.unwrap_err();
    let s = c.client.auth_stop().unwrap();
    assert_eq!(
        (s.code, s.http, s.text.as_str()),
        (-1, false, "query-api: the API key is the master account's now")
    );
    assert!(fake.requests().is_empty());
}

#[tokio::test]
async fn the_only_post_is_cancel_all_with_the_fixed_body() {
    assert_eq!(CANCEL_ALL, "/v5/order/cancel-all");
    assert_eq!(CANCEL_ALL_BODY, r#"{"category":"linear","settleCoin":"USDT"}"#);
    let fake = FakeHttp::start(Answer::Close);
    fake.route(
        CANCEL_ALL,
        [ok(
            r#"{"list":[{"orderId":"1","orderLinkId":"a"},{"orderId":"2","orderLinkId":""}],"success":"1"}"#,
        )],
    );
    // The REST base as configured, with a trailing slash: the URL is the
    // base plus the path constant.
    let c = client_of(&format!("{}/", fake.url()), 300);
    let seen = Arc::new(Mutex::new(Vec::new()));
    let (m, s) = (c.metrics.clone(), seen.clone());
    fake.on_request(move |_| {
        let v = m.get("watcher_requests_total", &[("endpoint", CANCEL_ALL)]);
        s.lock().unwrap().push(v);
    });
    assert_eq!(c.client.cancel_all().await, Ok(CancelAll { cancelled: 2 }));
    // Counted before it is sent.
    assert_eq!(*seen.lock().unwrap(), [Some(1.0)]);

    let reqs = fake.requests();
    assert_eq!(reqs.len(), 1);
    let r = &reqs[0];
    assert_eq!(r.method, "POST");
    assert_eq!(r.target, CANCEL_ALL);
    // The bytes sent are the body constant, byte for byte.
    assert_eq!(r.body, CANCEL_ALL_BODY.as_bytes());
    assert_eq!(r.header("content-type"), Some("application/json"));
    assert_eq!(r.header("x-bapi-api-key"), Some("WKEY"));
    assert_eq!(r.header("x-bapi-timestamp"), Some("1700000000000"));
    assert_eq!(r.header("x-bapi-recv-window"), Some("5000"));
    assert_eq!(r.header("x-bapi-sign-type"), Some("2"));
    // Python hmac over "1700000000000WKEY5000" + the body (tests/sign.rs).
    assert_eq!(
        r.header("x-bapi-sign"),
        Some("df293dce1c898148cb8c548eaa8d9d5180068daaa25acc3b6b028df594ab0f96")
    );
    // And the signature is over the bytes that arrived: signed and sent
    // are the same bytes.
    let arrived = String::from_utf8(r.body.clone()).unwrap();
    assert_eq!(
        r.header("x-bapi-sign").map(str::to_string),
        Some(post_signature(
            "watcher-made-up-secret-42",
            T0,
            "WKEY",
            5000,
            &arrived
        ))
    );
    // The timestamp is the injected clock at the send.
    c.clock.advance(1_234);
    c.client.cancel_all().await.unwrap();
    let r = &fake.requests()[1];
    assert_eq!(r.header("x-bapi-timestamp"), Some("1700000001234"));
    assert_eq!(
        r.header("x-bapi-sign").map(str::to_string),
        Some(post_signature(
            "watcher-made-up-secret-42",
            T0 + 1_234,
            "WKEY",
            5000,
            CANCEL_ALL_BODY
        ))
    );
    assert_eq!(
        c.metrics
            .get("watcher_requests_total", &[("endpoint", CANCEL_ALL)]),
        Some(2.0)
    );
    assert!(c.lines.lock().unwrap().is_empty());
}

#[tokio::test]
async fn cancel_all_answers_are_classified() {
    let row = r#"{"orderId":"1","orderLinkId":"a"}"#;
    // retCode 0: `success` "1" or absent is the count of the list.
    for (result, n) in [
        (format!(r#"{{"list":[{row},{row},{row}],"success":"1"}}"#), 3),
        (format!(r#"{{"list":[{row}]}}"#), 1),
        (r#"{"list":[],"success":"1"}"#.to_string(), 0),
    ] {
        let fake = FakeHttp::start(Answer::Close);
        fake.route(CANCEL_ALL, [ok(&result)]);
        let c = client(&fake);
        assert_eq!(
            c.client.cancel_all().await,
            Ok(CancelAll { cancelled: n }),
            "{result}"
        );
    }
    // `success` "0" is refused; a list that is not an array, no result or
    // another `success` value is unreadable. Each is counted under the
    // path, and none stops or pauses the client.
    for (result, class) in [
        (format!(r#"{{"list":[{row}],"success":"0"}}"#), "refused"),
        (r#"{"list":{},"success":"1"}"#.to_string(), "unreadable"),
        (r#"{"success":"1"}"#.to_string(), "unreadable"),
        ("null".to_string(), "unreadable"),
        (format!(r#"{{"list":[{row}],"success":1}}"#), "unreadable"),
        (format!(r#"{{"list":[{row}],"success":"2"}}"#), "unreadable"),
    ] {
        let fake = FakeHttp::start(Answer::Close);
        fake.route(CANCEL_ALL, [ok(&result), ok(r#"{"list":[]}"#)]);
        let c = client(&fake);
        let e = c.client.cancel_all().await.unwrap_err();
        assert_eq!(e.class(), class, "{result}: {e:?}");
        if class == "refused" {
            assert_eq!(
                e,
                RestError::Refused {
                    code: 0,
                    msg: "cancel-all answered success 0".into()
                }
            );
        }
        assert_eq!(
            c.metrics.get(
                "watcher_request_errors_total",
                &[("endpoint", CANCEL_ALL), ("class", class)]
            ),
            Some(1.0),
            "{result}"
        );
        assert_eq!(
            c.client.cancel_all().await,
            Ok(CancelAll { cancelled: 0 }),
            "{result}"
        );
    }
    // The client classes of every request: 5xx is no answer.
    let fake = FakeHttp::start(Answer::Close);
    fake.route(CANCEL_ALL, [Answer::Reply(503, "busy".into())]);
    let c = client(&fake);
    assert_eq!(c.client.cancel_all().await.unwrap_err().class(), "no_answer");

    // 10010 latches the key: nothing more is sent, by any path.
    let fake = FakeHttp::start(ok("{}"));
    fake.route(CANCEL_ALL, [code(10010)]);
    let c = client(&fake);
    let e = c.client.cancel_all().await.unwrap_err();
    assert!(matches!(e, RestError::Auth { code: 10010, .. }), "{e:?}");
    assert_eq!(c.client.auth_rejected(), Some(10010));
    assert_eq!(c.metrics.get("watcher_auth_rejected", &[]), Some(1.0));
    c.clock.advance(60_000);
    assert_eq!(c.client.cancel_all().await.unwrap_err().class(), "auth");
    assert_eq!(
        c.client
            .get("/v5/order/realtime", &linear())
            .await
            .unwrap_err()
            .class(),
        "auth"
    );
    assert_eq!(fake.requests().len(), 1);

    // 403 pauses every request for 600 s from the answer.
    let fake = FakeHttp::start(ok(r#"{"list":[]}"#));
    fake.route(CANCEL_ALL, [Answer::Reply(403, "banned".into()), ok(r#"{"list":[]}"#)]);
    let c = client(&fake);
    assert_eq!(c.client.cancel_all().await.unwrap_err().class(), "ip_banned");
    assert_eq!(c.client.paused_until(), Some(T0 + 600_000));
    c.clock.set(T0 + 599_999);
    assert_eq!(c.client.cancel_all().await.unwrap_err().class(), "ip_banned");
    assert_eq!(fake.requests().len(), 1);
    c.clock.set(T0 + 600_000);
    assert_eq!(c.client.cancel_all().await, Ok(CancelAll { cancelled: 0 }));
    assert_eq!(fake.requests().len(), 2);
}

/// Plain-text count of `needle` in `text`.
fn count(text: &str, needle: &str) -> usize {
    text.matches(needle).count()
}

/// The source of the watcher, read at test time: one `.post(` in each of
/// `rest.rs` (the cancel-all), `metrics.rs` (the VM import) and `alert.rs`
/// (Telegram), none in any other file, and no generic way to send a
/// request with another method. A comment counts too (plain text).
#[test]
fn no_other_post_exists() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    for e in std::fs::read_dir(&dir).unwrap() {
        let p = e.unwrap().path();
        if p.extension().and_then(|x| x.to_str()) == Some("rs") {
            files.push(p);
        }
    }
    files.sort();
    assert!(files.len() >= 20, "{files:?}");
    let mut posts = Vec::new();
    for p in &files {
        let text = std::fs::read_to_string(p).unwrap();
        let name = p.file_name().unwrap().to_str().unwrap().to_string();
        let n = count(&text, ".post(");
        if n > 0 {
            posts.push((name.clone(), n));
        }
        for banned in ["Method::", ".request(", ".put(", ".patch(", ".delete("] {
            assert_eq!(count(&text, banned), 0, "{banned} in {name}");
        }
    }
    assert_eq!(
        posts,
        [
            ("alert.rs".to_string(), 1),
            ("metrics.rs".to_string(), 1),
            ("rest.rs".to_string(), 1)
        ]
    );
    // The one in rest.rs is inside `cancel_all`.
    let rest = std::fs::read_to_string(dir.join("rest.rs")).unwrap();
    let at = rest.find(".post(").unwrap();
    let f = rest[..at].rfind("pub async fn ").unwrap();
    assert!(
        rest[f..].starts_with("pub async fn cancel_all("),
        "{}",
        &rest[f..f + 40]
    );
}
