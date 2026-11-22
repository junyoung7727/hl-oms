//! The watcher's own Bybit client (C-WATCH part 3) against a loopback fake
//! (`http-fake`), on a manual clock: the request guard, the headers and the
//! signed query, the answer classes, the never-retried key rejection and
//! the 403 pause.

#![allow(clippy::unwrap_used)]

use std::sync::{Arc, Mutex};
use std::time::Duration;

use http_fake::{Answer, FakeHttp};
use serde_json::json;
use watcher::clock::ManualClock;
use watcher::metrics::Metrics;
use watcher::rest::{
    Client, ClientConfig, Credentials, GUARD_MAX, GUARD_WINDOW_MS, Guard, RestError,
};

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
    assert_eq!(fake.requests().len(), GUARD_MAX + 1);
}
