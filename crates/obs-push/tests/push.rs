//! obs-push (spec D2). Every store is a loopback fake.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use http_fake::{Answer, FakeHttp, dead_addr};
use obs_push::{LogLine, ObsConfig, ObsPush, Sample, vl_line, vm_line};

fn labels(kv: &[(&str, &str)]) -> BTreeMap<String, String> {
    kv.iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

fn sample(name: &str, kv: &[(&str, &str)], v: f64, ts: i64) -> Sample {
    Sample {
        name: name.into(),
        labels: labels(kv),
        value: v,
        ts_ms: ts,
    }
}

fn fast(vm: Option<String>, vl: Option<String>) -> ObsConfig {
    let mut c = ObsConfig::new(vm, vl);
    c.flush_every = Duration::from_millis(20);
    c.backoff_min = Duration::from_millis(20);
    c.backoff_max = Duration::from_millis(80);
    c.timeout = Duration::from_millis(300);
    c.self_every = Duration::from_millis(100);
    c.self_labels = labels(&[("service", "md-collector"), ("venue", "bybit")]);
    c
}

fn lines(f: &FakeHttp, path: &str) -> Vec<String> {
    f.requests()
        .iter()
        .filter(|r| r.path() == path)
        .flat_map(|r| {
            r.body_str()
                .split('\n')
                .filter(|l| !l.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .collect()
}

#[test]
fn vm_line_matches_documented_example() {
    // VictoriaMetrics docs, "JSON line format" (facts file V17):
    // {"metric":{"__name__":"up","job":"node_exporter","instance":"localhost:9100"},"values":[0,0,0],"timestamps":[1549891472010,1549891487724,1549891503438]}
    let doc: serde_json::Value = serde_json::from_str(
        r#"{"metric":{"__name__":"up","job":"node_exporter","instance":"localhost:9100"},"values":[0],"timestamps":[1549891472010]}"#,
    )
    .expect("doc example");
    let s = sample(
        "up",
        &[("job", "node_exporter"), ("instance", "localhost:9100")],
        0.0,
        1_549_891_472_010,
    );
    let line = vm_line(&s).expect("finite");
    let ours: serde_json::Value = serde_json::from_str(&line).expect("json");
    assert_eq!(ours["metric"], doc["metric"]);
    assert_eq!(ours["timestamps"], doc["timestamps"]);
    assert_eq!(ours["values"][0].as_f64(), doc["values"][0].as_f64());
    // Our exact bytes: __name__ first, then labels sorted, one sample.
    assert_eq!(
        line,
        r#"{"metric":{"__name__":"up","instance":"localhost:9100","job":"node_exporter"},"values":[0.0],"timestamps":[1549891472010]}"#
    );
    assert!(!line.contains('\n'));
    assert_eq!(vm_line(&sample("x", &[], f64::NAN, 1)), None);
    assert_eq!(vm_line(&sample("x", &[], f64::INFINITY, 1)), None);
}

#[test]
fn vl_line_and_stream_fields() {
    let mut fields = serde_json::Map::new();
    fields.insert("level".into(), "warn".into());
    fields.insert("conn_id".into(), 7.into());
    let l = LogLine {
        time_ns: 1_763_530_802_123_456_789,
        msg: "gap".into(),
        service: "md-collector".into(),
        host: "linux-pc".into(),
        venue: "bybit".into(),
        fields,
    };
    let line = vl_line(&l);
    assert_eq!(
        line,
        r#"{"_time":"2025-11-19T05:40:02.123456789Z","_msg":"gap","service":"md-collector","host":"linux-pc","venue":"bybit","conn_id":7,"level":"warn"}"#
    );
    let f = FakeHttp::start(Answer::Reply(200, String::new()));
    let h = ObsPush::start(fast(None, Some(f.url()))).expect("start");
    h.log(l);
    assert!(h.flush_for(Duration::from_secs(5)));
    let r = f.requests();
    assert_eq!(r.len(), 1);
    assert_eq!(r[0].method, "POST");
    assert_eq!(r[0].path(), "/insert/jsonline");
    assert_eq!(r[0].query(), "_stream_fields=service,host,venue");
    assert_eq!(r[0].body_str().trim_end(), line);
}

#[test]
fn reserved_field_names_do_not_overwrite() {
    let mut fields = serde_json::Map::new();
    fields.insert("_msg".into(), "evil".into());
    fields.insert("venue".into(), "other".into());
    let l = LogLine {
        time_ns: 0,
        msg: "m".into(),
        service: "s".into(),
        host: "h".into(),
        venue: "v".into(),
        fields,
    };
    let v: serde_json::Value = serde_json::from_str(&vl_line(&l)).expect("json");
    assert_eq!(v["_msg"], "m");
    assert_eq!(v["venue"], "v");
    assert_eq!(v["fields._msg"], "evil");
    assert_eq!(v["fields.venue"], "other");
}

#[test]
fn queue_drops_oldest_and_counts() {
    let dead = format!("http://{}", dead_addr());
    let mut c = fast(Some(dead), None);
    c.queue_lines = 10;
    c.self_every = Duration::from_secs(3600);
    let h = ObsPush::start(c).expect("start");
    for i in 0..15 {
        h.metric(sample(
            "m",
            &[("i", &i.to_string())],
            f64::from(i),
            1000 + i64::from(i),
        ));
    }
    let st = h.state();
    assert_eq!(st.vm.dropped, 5);
    assert!(st.vm.queued <= 10);
    drop(h);

    // Against a live store: the 10 newest arrive in order, and the drop
    // counter reaches the store.
    let f = FakeHttp::start(Answer::Reply(204, String::new()));
    f.script([Answer::Reply(503, String::new())]);
    let mut c = fast(Some(f.url()), None);
    c.queue_lines = 10;
    c.flush_every = Duration::from_secs(3600);
    c.self_every = Duration::from_millis(50);
    let h = ObsPush::start(c).expect("start");
    for i in 0..15 {
        h.metric(sample(
            "m",
            &[("i", &i.to_string())],
            f64::from(i),
            1000 + i64::from(i),
        ));
    }
    assert!(h.flush_for(Duration::from_secs(5)));
    // The first request (answered 503) is sent again whole.
    let all = lines(&f, "/api/v1/import");
    let got: Vec<i64> = all[all.len() / 2..]
        .iter()
        .filter_map(|l| {
            let v: serde_json::Value = serde_json::from_str(l).ok()?;
            (v["metric"]["__name__"] == "m").then(|| v["timestamps"][0].as_i64())?
        })
        .collect();
    assert_eq!(got, (1005..1015).collect::<Vec<_>>());
    assert_eq!(all[..all.len() / 2], all[all.len() / 2..]);
    // The self metrics go out on a later cycle (the sender wakes on flush).
    let end = Instant::now() + Duration::from_secs(3);
    let dropped = loop {
        std::thread::sleep(Duration::from_millis(60));
        h.flush_for(Duration::from_secs(1));
        let d: Vec<serde_json::Value> = lines(&f, "/api/v1/import")
            .iter()
            .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
            .filter(|v| v["metric"]["__name__"] == "obs_push_dropped_total")
            .collect();
        if !d.is_empty() || Instant::now() > end {
            break d;
        }
    };
    assert!(
        dropped.iter().any(|v| v["metric"]["endpoint"] == "vm"
            && v["metric"]["venue"] == "bybit"
            && v["values"][0].as_f64() == Some(5.0)),
        "{dropped:?}"
    );
}

#[test]
fn retry_keeps_batch() {
    let f = FakeHttp::start(Answer::Reply(204, String::new()));
    f.script([
        Answer::Reply(500, String::new()),
        Answer::Close,
        Answer::Reply(503, String::new()),
    ]);
    let mut c = fast(Some(f.url()), None);
    c.self_every = Duration::from_secs(3600);
    let h = ObsPush::start(c).expect("start");
    for i in 0..5 {
        h.metric(sample("m", &[], f64::from(i), i64::from(i)));
    }
    assert!(h.flush_for(Duration::from_secs(5)));
    let r = f.requests();
    assert_eq!(r.len(), 4, "three failures, one success");
    assert_eq!(r[0].body, r[3].body, "the same batch is sent again");
    assert_eq!(lines(&f, "/api/v1/import").len(), 20);
    let st = h.state();
    assert_eq!(st.vm.sent, 5);
    assert_eq!(st.vm.failed_requests, 3);
    assert_eq!(st.vm.dropped, 0);
}

#[test]
fn push_never_blocks() {
    let f = FakeHttp::start(Answer::Hang);
    let mut c = fast(Some(f.url()), Some(f.url()));
    c.timeout = Duration::from_secs(30);
    let h = ObsPush::start(c).expect("start");
    let t0 = Instant::now();
    for i in 0..100_000 {
        h.metric(sample("m", &[("a", "b")], 1.0, i));
    }
    assert!(
        t0.elapsed() < Duration::from_millis(500),
        "{:?}",
        t0.elapsed()
    );
    assert!(h.state().vm.queued <= 100_000);
}

#[test]
fn missing_endpoint_is_a_no_op() {
    let h = ObsPush::start(fast(None, None)).expect("start");
    h.metric(sample("m", &[], 1.0, 1));
    let st = h.state();
    assert_eq!((st.vm.queued, st.vm.dropped, st.vm.invalid), (0, 0, 0));
    assert!(h.flush_for(Duration::from_millis(100)));
}

#[test]
fn non_finite_is_counted_invalid() {
    let f = FakeHttp::start(Answer::Reply(204, String::new()));
    let mut c = fast(Some(f.url()), None);
    c.self_every = Duration::from_secs(3600);
    let h = ObsPush::start(c).expect("start");
    h.metric(sample("m", &[], f64::NAN, 1));
    h.metric(sample("m", &[], 2.0, 2));
    assert!(h.flush_for(Duration::from_secs(5)));
    assert_eq!(h.state().vm.invalid, 1);
    assert_eq!(lines(&f, "/api/v1/import").len(), 1);
}

#[test]
fn permanent_reject_drops_the_batch() {
    let f = FakeHttp::start(Answer::Reply(204, String::new()));
    f.script([Answer::Reply(400, String::new())]);
    let mut c = fast(Some(f.url()), None);
    c.self_every = Duration::from_secs(3600);
    c.batch_lines = 3;
    let h = ObsPush::start(c).expect("start");
    for i in 0..5 {
        h.metric(sample("m", &[], f64::from(i), i64::from(i)));
    }
    assert!(h.flush_for(Duration::from_secs(5)));
    let st = h.state();
    assert_eq!(st.vm.rejected, 3);
    assert_eq!(st.vm.sent, 2, "the lines after the refused batch still go");
    assert_eq!(f.requests().len(), 2);
}

#[test]
fn priority_goes_first() {
    let f = FakeHttp::start(Answer::Reply(204, String::new()));
    let mut c = fast(Some(f.url()), None);
    c.self_every = Duration::from_secs(3600);
    c.flush_every = Duration::from_secs(3600);
    c.batch_lines = 10_000;
    let h = ObsPush::start(c).expect("start");
    for i in 0..3000 {
        h.metric(sample("summary", &[], 1.0, i));
    }
    h.priority(sample("md_collector_heartbeat_ts", &[], 7.0, 1));
    assert!(h.flush_for(Duration::from_secs(5)));
    let first = &lines(&f, "/api/v1/import")[0];
    assert!(first.contains("md_collector_heartbeat_ts"), "{first}");
}
