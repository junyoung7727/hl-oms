//! The watcher's metrics registry and its own VictoriaMetrics import client
//! (C-WATCH part 3), against a loopback fake VM.

#![allow(clippy::unwrap_used)]

use http_fake::{Answer, FakeHttp};
use serde_json::Value;
use watcher::metrics::{Exporter, Metrics};

const T0: u64 = 1_700_000_000_000;

#[test]
fn the_import_lines_are_victoria_metrics_json() {
    let m = Metrics::new("h1", "w-1");
    let ep = [("endpoint", "/v5/order/realtime")];
    m.add("watcher_requests_total", &ep, 1.0);
    m.add("watcher_requests_total", &ep, 2.0);
    m.add(
        "watcher_request_errors_total",
        &[("endpoint", "/v5/order/realtime"), ("class", "no_answer")],
        1.0,
    );
    m.set("watcher_auth_rejected", &[], 0.0);
    m.set("watcher_auth_rejected", &[], 1.0);
    // An absent gauge is not sent; a value that is not finite is not sent.
    m.set("watcher_api_key_days_left", &[], 10.0);
    m.remove("watcher_api_key_days_left", &[]);
    m.set("watcher_check_age_s", &[("check", "W1")], f64::NAN);
    assert_eq!(m.get("watcher_requests_total", &ep), Some(3.0));

    let text = m.import_lines(T0);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(
        lines,
        [
            r#"{"metric":{"__name__":"watcher_up","host":"h1","venue":"bybit","watcher":"w-1"},"values":[1.0],"timestamps":[1700000000000]}"#,
            r#"{"metric":{"__name__":"watcher_auth_rejected","host":"h1","venue":"bybit","watcher":"w-1"},"values":[1.0],"timestamps":[1700000000000]}"#,
            r#"{"metric":{"__name__":"watcher_request_errors_total","class":"no_answer","endpoint":"/v5/order/realtime","host":"h1","venue":"bybit","watcher":"w-1"},"values":[1.0],"timestamps":[1700000000000]}"#,
            r#"{"metric":{"__name__":"watcher_requests_total","endpoint":"/v5/order/realtime","host":"h1","venue":"bybit","watcher":"w-1"},"values":[3.0],"timestamps":[1700000000000]}"#,
        ]
    );
    assert!(text.ends_with('\n'));
    // Each line is one JSON object of the import format; labels are quoted
    // as JSON strings, so a quote in a label cannot break the line.
    let m = Metrics::new("h\"1", "w");
    m.set("watcher_x", &[("k", "a\\b\"c")], 2.5);
    for line in m.import_lines(T0).lines() {
        let v: Value = serde_json::from_str(line).unwrap();
        assert!(v["metric"]["__name__"].is_string());
        assert_eq!(v["metric"]["venue"], "bybit");
        assert_eq!(v["metric"]["host"], "h\"1");
        assert_eq!(v["values"].as_array().unwrap().len(), 1);
        assert_eq!(v["timestamps"], serde_json::json!([T0]));
    }
}

#[tokio::test]
async fn a_failed_push_is_dropped() {
    let vm = FakeHttp::start(Answer::Reply(204, String::new()));
    vm.route(
        "/api/v1/import",
        [
            Answer::Reply(503, "busy".into()),
            Answer::Close,
            Answer::Reply(500, String::new()),
            Answer::Reply(204, String::new()),
        ],
    );
    let (log, lines) = watcher::log::collect();
    let ex = Exporter::new(&vm.url(), 1_000, log).unwrap();
    let m = Metrics::new("h1", "w1");
    let ep = [("endpoint", "/v5/position/list")];
    m.add("watcher_requests_total", &ep, 1.0);
    m.set("watcher_auth_rejected", &[], 0.0);

    assert!(ex.push(&m, T0).await.is_err());
    m.add("watcher_requests_total", &ep, 1.0);
    assert!(ex.push(&m, T0 + 1_000).await.is_err());
    // Logged once per minute, not once per failure.
    let logged = lines.lock().unwrap().clone();
    assert_eq!(logged.len(), 1, "{logged:?}");
    assert!(ex.push(&m, T0 + 60_000).await.is_err());
    let logged = lines.lock().unwrap().clone();
    assert_eq!(logged.len(), 2, "{logged:?}");
    m.set("watcher_auth_rejected", &[], 1.0);
    ex.push(&m, T0 + 61_000).await.unwrap();

    let reqs = vm.requests();
    assert_eq!(reqs.len(), 4);
    for r in &reqs {
        assert_eq!(
            (r.method.as_str(), r.target.as_str()),
            ("POST", "/api/v1/import")
        );
    }
    // No queue: the push that went through holds only the current values,
    // one line per series, at its own export time; the counter is
    // cumulative and the gauge is fresh.
    let body = reqs[3].body_str();
    let got: Vec<Value> = body
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(got.len(), 3, "{body}");
    for v in &got {
        assert_eq!(v["timestamps"], serde_json::json!([T0 + 61_000]));
    }
    let value = |name: &str| {
        got.iter()
            .find(|v| v["metric"]["__name__"] == name)
            .map(|v| v["values"][0].as_f64().unwrap())
    };
    assert_eq!(value("watcher_up"), Some(1.0));
    assert_eq!(value("watcher_requests_total"), Some(2.0));
    assert_eq!(value("watcher_auth_rejected"), Some(1.0));
}
