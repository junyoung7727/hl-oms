//! Ticket 05: the collector's pushes and direct alerts against loopback
//! fakes (U6 spec D4–D11, R2–R4, R7, R12).

mod common;
#[path = "common/fake_bybit.rs"]
mod fake;
#[path = "common/run.rs"]
mod run;
#[path = "common/slowfs.rs"]
mod slowfs;

use std::collections::BTreeSet;
use std::os::unix::fs::PermissionsExt;
use std::sync::Arc;
use std::time::Duration;

use common::*;
use fake::Behaviour;
use http_fake::{Answer, FakeHttp, dead_addr};
use md_collector::collector::{Collector, Deps, SystemClock};
use md_collector::config::Config;
use md_collector::obs::{ClockOffset, parse_chrony};
use run::*;
use slowfs::{Gate, SetFree, SlowFs};

const TOKEN: &str = "123456:ABCdef_ghi-JKL";
const ONE_MARKET: [(&str, &str); 1] = [(
    "linear",
    "symbols = [\"BTCUSDT\", \"ETHUSDT\"]\nchannels = [\"orderbook.50\", \"publicTrade\"]\nconn_budget = 6",
)];
const TRADED: &str = "\n[[traded_channels]]\nmarket = \"linear\"\nsymbol = \"BTCUSDT\"\nchannel = \"orderbook.50\"\n";

struct Fixed(f64);

impl ClockOffset for Fixed {
    fn offset_ms(&self) -> Option<f64> {
        Some(self.0)
    }
}

fn token_file(dir: &std::path::Path) -> std::path::PathBuf {
    let p = dir.join("telegram.json");
    std::fs::write(&p, format!(r#"{{"token":"{TOKEN}","chat_id":"42"}}"#)).expect("write");
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600)).expect("chmod");
    p
}

fn alert_toml(dir: &std::path::Path, tg: &FakeHttp) -> String {
    format!(
        "\n[alert]\ntelegram = {:?}\nstate_file = {:?}\napi_base = \"{}\"\n",
        token_file(dir).to_string_lossy(),
        dir.join("alert-state.json").to_string_lossy(),
        tg.url()
    )
}

fn tg_texts(f: &FakeHttp) -> Vec<String> {
    f.requests()
        .iter()
        .filter_map(|r| serde_json::from_slice::<serde_json::Value>(&r.body).ok())
        .map(|v| v["text"].as_str().unwrap_or_default().to_string())
        .collect()
}

fn vm_lines(f: &FakeHttp) -> Vec<serde_json::Value> {
    f.requests()
        .iter()
        .filter(|r| r.path() == "/api/v1/import")
        .flat_map(|r| {
            r.body_str()
                .lines()
                .filter_map(|l| serde_json::from_str(l).ok())
                .collect::<Vec<_>>()
        })
        .collect()
}

fn base_toml(lake: &std::path::Path) -> String {
    format!(
        "venue = \"bybit\"\nlake_root = {:?}\nloopback_only = true\n[[markets]]\nname = \"linear\"\nsymbols = [\"BTCUSDT\"]\nchannels = [\"orderbook.50\"]\nconn_budget = 2\n",
        lake.to_string_lossy()
    )
}

#[test]
fn config_telemetry_and_alert_sections() {
    let dir = test_dir("out-config");
    let base = base_toml(&dir);
    let c = Config::from_toml(&format!(
        "{base}[telemetry]\nvm_url = \"http://127.0.0.1:8428\"\n"
    ))
    .expect("telemetry");
    let t = c.telemetry.expect("section");
    assert_eq!(
        (t.fast_ms, t.slow_ms, t.queue_lines),
        (1000, 10_000, 100_000)
    );
    assert!(c.alert.is_none());
    for (bad, why) in [
        ("[telemetry]\nfast_ms = 50\n", "fast_ms >= 100"),
        (
            "[telemetry]\nfast_ms = 2000\nslow_ms = 1000\n",
            "slow_ms >= fast_ms",
        ),
        (
            "[telemetry]\nvm_url = \"http://10.1.2.3:8428\"\n",
            "not loopback",
        ),
        ("[telemetry]\nbogus = 1\n", "unknown field"),
    ] {
        let e = Config::from_toml(&format!("{base}{bad}")).expect_err(bad);
        assert!(e.contains(why), "{bad}: {e}");
    }
    let tg = token_file(&dir);
    let ok = format!(
        "{base}[alert]\ntelegram = {:?}\napi_base = \"http://127.0.0.1:1\"\n",
        tg.to_string_lossy()
    );
    assert!(Config::from_toml(&ok).is_ok());
    // R4: a refused token file is a configuration error.
    std::fs::set_permissions(&tg, std::fs::Permissions::from_mode(0o644)).expect("chmod");
    let e = Config::from_toml(&ok).expect_err("0644");
    assert!(e.contains("0600") && !e.contains(TOKEN), "{e}");
    std::fs::set_permissions(&tg, std::fs::Permissions::from_mode(0o600)).expect("chmod");
    let missing = format!("{base}[alert]\ntelegram = \"/nonexistent/t.json\"\n");
    assert!(Config::from_toml(&missing).is_err());
    // api_base only with loopback_only, and only loopback.
    let prod = ok.replace("loopback_only = true\n", "");
    assert!(
        Config::from_toml(&prod)
            .expect_err("prod")
            .contains("tests only")
    );
    let far = ok.replace("http://127.0.0.1:1", "http://example.com");
    assert!(
        Config::from_toml(&far)
            .expect_err("far")
            .contains("not loopback")
    );
}

#[test]
fn chrony_csv_field_5() {
    let line = "5BBD5B71,91.189.91.113,3,1795066546.644743803,0.000466056,-0.000009033,0.000472667,-48.918,-0.001,0.102,0.239850715,0.002665596,1037.2,Normal\n";
    assert!((parse_chrony(line).expect("ms") - 0.466_056).abs() < 1e-9);
    assert_eq!(parse_chrony(""), None);
    assert_eq!(parse_chrony("a,b,c"), None);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn collector_pushes_metrics_and_logs() {
    let s = setup(
        "out-push",
        Behaviour {
            stream_every: Some(Duration::from_millis(50)),
            ..Behaviour::default()
        },
        pages(&[("linear", &["BTCUSDT", "ETHUSDT"])]),
    )
    .await;
    let vm = FakeHttp::start(Answer::Reply(204, String::new()));
    let vl = FakeHttp::start(Answer::Reply(200, String::new()));
    let cfg = s.config(
        &ONE_MARKET,
        &format!(
            "[telemetry]\nvm_url = \"{}\"\nvl_url = \"{}\"\nfast_ms = 200\nslow_ms = 1000\n[disk]\ncheck_ms = 100\n{TRADED}",
            vm.url(),
            vl.url()
        ),
    );
    let mut deps = s.deps();
    deps.free_space = SetFree::new(123_456_789);
    deps.clock_offset = Some(Arc::new(Fixed(0.25)));
    let h = Collector::start(cfg, deps).await.expect("start");
    let want: BTreeSet<&str> = [
        "md_spread_bp",
        "md_mid_px",
        "md_depth_qty",
        "md_msgs",
        "md_bytes",
        "md_recv_lag_ms",
        "md_gaps_total",
        "md_reconnects_total",
        "md_last_msg_age_s",
        "lake_l0_open_files",
        "lake_l0_bytes_total",
        "lake_l0_write_errors_total",
        "lake_seal_lag_hours",
        "md_queue_bytes",
        "lake_disk_free_bytes",
        "md_collector_heartbeat_ts",
        "node_clock_offset_ms",
    ]
    .into_iter()
    .collect();
    eventually("every collector metric reaches VictoriaMetrics", || {
        let got: BTreeSet<String> = vm_lines(&vm)
            .iter()
            .filter_map(|v| v["metric"]["__name__"].as_str().map(str::to_string))
            .collect();
        want.iter().all(|w| got.contains(*w))
    })
    .await;
    let lines = vm_lines(&vm);
    let get = |name: &str| -> Vec<&serde_json::Value> {
        lines
            .iter()
            .filter(|v| v["metric"]["__name__"] == name)
            .collect()
    };
    assert!(
        get("lake_disk_free_bytes")
            .iter()
            .all(|v| v["values"][0].as_f64() == Some(123_456_789.0))
    );
    assert!(
        get("node_clock_offset_ms")
            .iter()
            .all(|v| v["values"][0].as_f64() == Some(0.25) && v["metric"]["host"] == "test")
    );
    let spread = get("md_spread_bp");
    assert!(
        spread
            .iter()
            .all(|v| v["metric"]["venue"] == "bybit" && v["metric"]["market"] == "linear")
    );
    // bid 42000.0, ask 42000.5 (the fake's book).
    let mid: Vec<f64> = get("md_mid_px")
        .iter()
        .filter_map(|v| v["values"][0].as_f64())
        .collect();
    assert!(mid.iter().all(|m| (m - 42_000.25).abs() < 1e-6), "{mid:?}");
    // Traded BTCUSDT book every 200 ms, ETHUSDT book every 1 s.
    let count = |sym: &str| {
        spread
            .iter()
            .filter(|v| v["metric"]["symbol"] == sym)
            .count()
    };
    assert!(
        count("BTCUSDT") >= 3 * count("ETHUSDT").max(1),
        "{} vs {}",
        count("BTCUSDT"),
        count("ETHUSDT")
    );
    // Frames are counted per connection and topic (R7).
    assert!(
        get("md_msgs")
            .iter()
            .any(|v| v["values"][0].as_f64() > Some(0.0))
    );
    assert!(
        get("md_bytes")
            .iter()
            .any(|v| v["values"][0].as_f64() > Some(100.0))
    );
    let age = get("md_last_msg_age_s")
        .into_iter()
        .rfind(|v| v["metric"]["symbol"] == "BTCUSDT" && v["metric"]["channel"] == "orderbook.50")
        .and_then(|v| v["values"][0].as_f64())
        .expect("age");
    assert!(age < 1.0, "the fake streams every 50 ms: age {age}");
    // The conn label is the plan slot, never a conn_id.
    assert!(get("md_msgs").iter().all(|v| {
        v["metric"]["conn"]
            .as_str()
            .is_some_and(|c| c.parse::<u32>().is_ok_and(|n| n < 6))
    }));
    // Logs, with the stream fields.
    let logs: Vec<serde_json::Value> = vl
        .requests()
        .iter()
        .inspect(|r| assert_eq!(r.query(), "_stream_fields=service,host,venue"))
        .flat_map(|r| {
            r.body_str()
                .lines()
                .filter_map(|l| serde_json::from_str(l).ok())
                .collect::<Vec<_>>()
        })
        .collect();
    assert!(logs.iter().any(|l| l["_msg"] == "connected"), "{logs:?}");
    assert!(
        logs.iter().all(|l| l["service"] == "md-collector"
            && l["venue"] == "bybit"
            && l["host"] == "test")
    );
    h.shutdown().await.expect("shutdown");
}

/// The done criterion "a dead or slow store never blocks frame ingestion":
/// every frame the fake sent is written with the store refusing connections
/// and with a store that never answers; the drops are counted.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dead_or_slow_store_never_blocks_ingest() {
    for (name, slow) in [("out-dead", false), ("out-slow", true)] {
        let s = setup(
            name,
            Behaviour {
                per_topic: 400,
                ..Behaviour::default()
            },
            pages(&[("linear", &["BTCUSDT", "ETHUSDT"])]),
        )
        .await;
        let hang = FakeHttp::start(Answer::Hang);
        let url = if slow {
            hang.url()
        } else {
            format!("http://{}", dead_addr())
        };
        let cfg = s.config(
            &ONE_MARKET,
            &format!("[telemetry]\nvm_url = \"{url}\"\nvl_url = \"{url}\"\nfast_ms = 100\nslow_ms = 200\nqueue_lines = 50\n{TRADED}"),
        );
        let h = Collector::start(cfg, s.deps()).await.expect("start");
        // 4 topics, 401 frames each.
        let sent = || s.ws.sent_by_topic().values().map(Vec::len).sum::<usize>();
        eventually("the fake sent everything", || sent() == 4 * 401).await;
        let w = written(&h);
        eventually("written stops growing", || {
            std::thread::sleep(Duration::from_millis(300));
            written(&h) == w
        })
        .await;
        let st = h.state().ingest;
        assert!(
            st.conns.values().all(|c| c.overflows == 0),
            "{name}: nothing refused"
        );
        assert!(st.failed.is_none());
        eventually("drops are counted", || {
            h.out_state()
                .0
                .is_some_and(|o| o.vm.dropped > 0 && o.vl.queued <= 50)
        })
        .await;
        h.shutdown().await.expect("shutdown");
        let data = texts(&records_under(s.fs.as_ref(), &s.dir, "_open"))
            .iter()
            .filter(|t| {
                t.contains(r#""topic":"orderbook"#) || t.contains(r#""topic":"publicTrade"#)
            })
            .count();
        assert_eq!(data, 4 * 401, "{name}: every frame the fake sent is in L0");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn write_error_sends_one_md_p1() {
    let s = setup(
        "out-write-error",
        Behaviour {
            stream_every: Some(Duration::from_millis(20)),
            ..Behaviour::default()
        },
        pages(&[("linear", &["BTCUSDT", "ETHUSDT"])]),
    )
    .await;
    let tg = FakeHttp::start(Answer::Reply(200, r#"{"ok":true}"#.into()));
    let vm = format!("http://{}", dead_addr());
    let cfg = s.config(
        &ONE_MARKET,
        &format!(
            "[telemetry]\nvm_url = \"{vm}\"\n{}{TRADED}",
            alert_toml(&s.dir, &tg)
        ),
    );
    let gate = Arc::new(Gate::default());
    let mut deps = s.deps();
    deps.fs = Arc::new(SlowFs {
        inner: s.fs.clone(),
        gate: gate.clone(),
    });
    let h = Collector::start(cfg, deps).await.expect("start");
    eventually("frames written", || written(&h) > 10).await;
    gate.fail_with(28);
    eventually("the writer fails", || h.failed().is_some()).await;
    assert!(
        h.report_write_failure(Duration::from_secs(10)).await,
        "delivered"
    );
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let t = tg_texts(&tg);
    assert_eq!(t.len(), 1, "{t:?}");
    assert!(t[0].starts_with("[MD P1] bybit "), "{}", t[0]);
    assert!(t[0].contains("L0 쓰기 실패: ENOSPC"), "{}", t[0]);
    assert!(!t[0].contains(TOKEN));
    drop(h);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn disk_floor_sends_one_md_p1() {
    let s = setup(
        "out-disk-floor",
        Behaviour::default(),
        pages(&[("linear", &["BTCUSDT", "ETHUSDT"])]),
    )
    .await;
    let tg = FakeHttp::start(Answer::Reply(200, r#"{"ok":true}"#.into()));
    let vm = format!("http://{}", dead_addr());
    let cfg = s.config(
        &ONE_MARKET,
        &format!(
            "[telemetry]\nvm_url = \"{vm}\"\n[disk]\nfloor_bytes = 1000\nmargin_bytes = 1000\ncheck_ms = 50\n{}{TRADED}",
            alert_toml(&s.dir, &tg)
        ),
    );
    let free = SetFree::new(10_000);
    let mut deps = s.deps();
    deps.free_space = free.clone();
    let h = Collector::start(cfg, deps).await.expect("start");
    eventually("frames written", || written(&h) > 4).await;
    free.set(500);
    eventually("one [MD P1] at the floor", || tg_texts(&tg).len() == 1).await;
    tokio::time::sleep(Duration::from_millis(1000)).await;
    let t = tg_texts(&tg);
    assert_eq!(t.len(), 1, "no repeat within 5 min: {t:?}");
    assert!(
        t[0].starts_with("[MD P1] bybit ") && t[0].contains("디스크 하한: 여유 500 바이트"),
        "{}",
        t[0]
    );
    free.set(10_000);
    eventually("the clear", || tg_texts(&tg).len() == 2).await;
    assert!(tg_texts(&tg)[1].contains("해소: 디스크 하한"));
    h.shutdown().await.expect("shutdown");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn refused_token_file_refuses_start() {
    let s = setup(
        "out-refused",
        Behaviour::default(),
        pages(&[("linear", &["BTCUSDT"])]),
    )
    .await;
    let tg = FakeHttp::start(Answer::Reply(200, r#"{"ok":true}"#.into()));
    let extra = alert_toml(&s.dir, &tg);
    let cfg = s.config(&ONE_MARKET, &extra);
    std::fs::set_permissions(
        s.dir.join("telegram.json"),
        std::fs::Permissions::from_mode(0o640),
    )
    .expect("chmod");
    match Collector::start(cfg, Deps { ..s.deps() }).await {
        Err(md_collector::collector::StartError::Refused(e)) => assert!(e.contains("0600"), "{e}"),
        Err(e) => panic!("not refused: {e}"),
        Ok(_) => panic!("started"),
    }
    let _ = SystemClock;
}

/// Review findings 1 and 2: the L0 write condition clears only after a
/// session has run past its first fsync without a failure, and a writer
/// beat older than 15 s is a stall alert.
#[test]
fn l0_conditions_clear_late_and_report_stalls() {
    use md_collector::ingest::Health;
    use md_collector::obs::l0_conditions;
    let failed = Health {
        failed: Some("No space left on device (os error 28)".into()),
        failed_os_error: Some("ENOSPC".into()),
        heartbeat_s: Some(100),
        beats: 3,
        ..Health::default()
    };
    let c = l0_conditions(&failed, 200, 0, 10_000);
    let text = c.l0_write.expect("held").expect("set");
    assert!(text.starts_with("L0 쓰기 실패: ENOSPC"), "{text}");
    assert_eq!(c.writer_stall, None, "a failed writer is not a stall");

    let ok = Health {
        heartbeat_s: Some(100),
        beats: 2,
        ..Health::default()
    };
    // The first beats of a new session do not clear it.
    assert_eq!(l0_conditions(&ok, 101, 2_000, 10_000).l0_write, None);
    assert_eq!(l0_conditions(&ok, 110, 10_000, 10_000).l0_write, Some(None));
    let none = Health::default();
    assert_eq!(l0_conditions(&none, 110, 0, 10_000).l0_write, None);

    assert_eq!(l0_conditions(&ok, 115, 0, 10_000).writer_stall, None);
    let stall = l0_conditions(&ok, 116, 0, 10_000)
        .writer_stall
        .expect("stall");
    assert!(stall.contains("16초"), "{stall}");
}
