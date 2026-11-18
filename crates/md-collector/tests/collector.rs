//! Ticket 04: the collector against loopback fakes, end to end with the
//! md-lake binary (spec D5, D6, R3–R5).

mod common;
#[path = "common/fake_bybit.rs"]
mod fake;
#[path = "common/slowfs.rs"]
mod slowfs;

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use common::*;
use fake::{Behaviour, FakeBybit, FakeRest, Pages};
use md_collector::collector::{Collector, CollectorHandle, Deps, SystemClock};
use md_collector::config::Config;
use md_lake::fs::{Fs, RealFs};
use md_lake::qraw::{Record, Src};
use slowfs::{Gate, SetFree, SlowFs};

pub fn pages(markets: &[(&str, &[&str])]) -> Pages {
    markets
        .iter()
        .map(|(m, syms)| {
            (
                (m.to_string(), String::new()),
                (200u16, instruments_body(m, syms, "")),
            )
        })
        .collect()
}

pub struct Setup {
    pub dir: std::path::PathBuf,
    pub fs: Arc<dyn Fs>,
    pub ws: FakeBybit,
    pub rest: FakeRest,
}

pub async fn setup(name: &str, b: Behaviour, p: Pages) -> Setup {
    let dir = test_dir(name);
    let fs = RealFs::new(&dir);
    Setup {
        dir,
        fs,
        ws: FakeBybit::start(b).await,
        rest: FakeRest::start(p).await,
    }
}

impl Setup {
    /// A config with loopback fakes; `markets` is the TOML of the
    /// `[[markets]]` tables without `ws_url`/`rest_base` (added here).
    pub fn config(&self, markets: &[(&str, &str)], rest: &str) -> Config {
        let mut t = format!(
            "venue = \"bybit\"\nlake_root = {:?}\nhost = \"test\"\nloopback_only = true\n{rest}\n",
            self.dir.to_string_lossy()
        );
        for (name, body) in markets {
            t.push_str(&format!(
                "\n[[markets]]\nname = \"{name}\"\nws_url = \"{}\"\nrest_base = \"{}\"\n{body}\n",
                self.ws.ws_url(name),
                self.rest.base
            ));
        }
        Config::from_toml(&t).unwrap_or_else(|e| panic!("{e}\n{t}"))
    }

    pub fn deps(&self) -> Deps {
        Deps {
            fs: self.fs.clone(),
            free_space: SetFree::new(u64::MAX),
            clock: Arc::new(SystemClock),
        }
    }
}

pub async fn eventually(what: &str, mut f: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while !f() {
        assert!(Instant::now() < deadline, "not within 15 s: {what}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

pub fn written(h: &CollectorHandle) -> u64 {
    h.state()
        .ingest
        .conns
        .values()
        .map(|c| c.frames_written)
        .sum()
}

/// Records of every file below `dir` whose path contains `part`.
pub fn records_under(fs: &dyn Fs, dir: &Path, part: &str) -> Vec<Record> {
    let mut files = files_with(dir, part);
    files.retain(|p| p.contains(".qraw.zst") && !p.ends_with(".sha256"));
    files.sort();
    files.iter().flat_map(|p| records(fs, p)).collect()
}

pub fn texts(rs: &[Record]) -> Vec<String> {
    rs.iter()
        .map(|r| String::from_utf8_lossy(&r.payload).to_string())
        .collect()
}

const TWO_MARKETS: [(&str, &str); 2] = [
    (
        "linear",
        "symbols = [\"BTCUSDT\", \"ETHUSDT\"]\nchannels = [\"orderbook.50\", \"publicTrade\"]\nconn_budget = 10",
    ),
    (
        "spot",
        "symbols = \"all\"\nchannels = [\"orderbook.50\"]\nconn_budget = 10",
    ),
];

const TRADED: &str = "\n[[traded_channels]]\nmarket = \"linear\"\nsymbol = \"BTCUSDT\"\nchannel = \"orderbook.50\"\n";

fn seal(dir: &Path, first_hour_ns: i64, now_ns: i64) -> (i32, String, String) {
    let first = format!(
        "{}T{:02}",
        md_lake::time::date(first_hour_ns),
        md_lake::time::hour(first_hour_ns)
    );
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_md-lake"))
        .args([
            "seal",
            "--venue",
            "bybit",
            "--lake",
            &dir.to_string_lossy(),
            "--first-hour",
            &first,
            "--now-ns",
            &now_ns.to_string(),
        ])
        .env("MD_LAKE_TEST_ALLOW_FUTURE_NOW", "1")
        .output()
        .expect("run md-lake");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn end_to_end_seal() {
    let s = setup(
        "e2e-seal",
        Behaviour::default(),
        pages(&[
            ("linear", &["BTCUSDT", "ETHUSDT"]),
            ("spot", &["BTCUSDT", "SOLUSDT"]),
        ]),
    )
    .await;
    let cfg = s.config(&TWO_MARKETS, TRADED);
    let start_ns = md_collector::wall_ns();
    let h = Collector::start(cfg, s.deps()).await.expect("start");
    // linear: traded BTCUSDT book; other: BTCUSDT trades, ETHUSDT book + trades.
    // spot: BTCUSDT and SOLUSDT books. 4 per book, 4 per trade topic.
    let want = 4 * (1 + 1 + 2 + 2) as u64 + 2;
    eventually("all frames written", || written(&h) >= want).await;
    h.shutdown().await.expect("shutdown");
    let sent = s.ws.sent_by_topic();
    assert_eq!(sent.len(), 6, "{:?}", sent.keys());
    assert_eq!(s.ws.conns().len(), 4, "3 linear connections, 1 spot");

    let now = md_lake::time::day_start(md_collector::wall_ns()) + 2 * DAY + HOUR;
    let (code, out, err) = seal(&s.dir, start_ns, now);
    assert_eq!(code, 0, "{out}\n{err}");
    for ((market, channel, symbol), frames) in &sent {
        let got = records_under(
            s.fs.as_ref(),
            &s.dir,
            &format!("market={market}/channel={channel}/"),
        )
        .into_iter()
        .filter(|r| {
            String::from_utf8_lossy(&r.payload)
                .contains(&format!("\"topic\":\"{channel}.{symbol}\""))
        })
        .collect::<Vec<_>>();
        assert_eq!(&texts(&got), frames, "{market}/{channel}/{symbol}");
    }
    // No market-data frame in any session file; the subscribe answers are.
    let sess = records_under(s.fs.as_ref(), &s.dir, "/_session/");
    for r in &sess {
        let t = String::from_utf8_lossy(&r.payload);
        assert!(
            !t.contains("\"topic\""),
            "market data in a session file: {t}"
        );
    }
    let answers = sess
        .iter()
        .filter(|r| {
            r.src == Src::WebSocket
                && String::from_utf8_lossy(&r.payload).contains("\"op\":\"subscribe\"")
        })
        .count();
    assert_eq!(answers, 4);
    // The instrument bodies, verbatim, in each market's instruments file.
    for m in ["linear", "spot"] {
        let served: Vec<String> = s
            .rest
            .served
            .lock()
            .expect("served")
            .iter()
            .filter(|(_, b)| b.contains(&format!("\"category\":\"{m}\"")))
            .map(|(_, b)| b.clone())
            .collect();
        let got = records_under(
            s.fs.as_ref(),
            &s.dir,
            &format!("market={m}/channel=instruments/"),
        );
        assert_eq!(texts(&got), served, "{m}");
        assert!(got.iter().all(|r| r.src == Src::Rest && r.conn_id == 0));
    }
    // Every open file was sealed and removed.
    assert!(
        files_with(&s.dir, "/_open/")
            .iter()
            .all(|p| !p.contains(".qraw"))
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn open_files_locked_while_running() {
    let s = setup(
        "e2e-locked",
        Behaviour::default(),
        pages(&[("linear", &["BTCUSDT", "ETHUSDT"]), ("spot", &["BTCUSDT"])]),
    )
    .await;
    let h = Collector::start(s.config(&TWO_MARKETS, ""), s.deps())
        .await
        .expect("start");
    eventually("frames written", || written(&h) >= 10).await;
    let open = files_with(&s.dir, "/_open/");
    let open: Vec<String> = open
        .into_iter()
        .filter(|p| p.ends_with(".partial"))
        .collect();
    assert!(open.len() >= 3, "{open:?}");
    let session = files_with(&s.dir, "/_session/");
    for p in open.iter().chain(session.iter()) {
        let f = std::fs::File::open(s.dir.join(p)).expect("open");
        assert!(
            matches!(f.try_lock(), Err(std::fs::TryLockError::WouldBlock)),
            "{p} is not locked"
        );
    }
    let now = md_collector::wall_ns() + 2 * HOUR;
    let (code, out, err) = seal(&s.dir, md_collector::wall_ns(), now);
    assert_eq!(code, 0, "{out}\n{err}");
    let report: serde_json::Value = serde_json::from_str(&out).expect("report");
    assert!(report["waiting_on"].is_string(), "{report}");
    assert_eq!(report["sealed"], serde_json::json!([]));
    assert!(
        files_with(&s.dir, "market=").is_empty(),
        "no day file while the collector holds the hour"
    );
    for p in &open {
        assert!(s.dir.join(p).exists(), "{p} was removed");
    }
    h.shutdown().await.expect("shutdown");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn forced_disconnect_resubscribes() {
    let mut b = Behaviour::default();
    b.drop_after = BTreeMap::from([(0, 2)]);
    let s = setup("e2e-disconnect", b, pages(&[("linear", &["BTCUSDT"])])).await;
    let cfg = s.config(
        &[(
            "linear",
            "symbols = [\"BTCUSDT\"]\nchannels = [\"orderbook.50\"]\nconn_budget = 1",
        )],
        "",
    );
    let h = Collector::start(cfg, s.deps()).await.expect("start");
    eventually("a second connection with its frames", || {
        s.ws.conns().get(1).is_some_and(|c| c.data.len() == 4)
    })
    .await;
    eventually("written", || written(&h) >= 1 + 2 + 2 + 4).await;
    h.shutdown().await.expect("shutdown");
    let conns = s.ws.conns();
    assert!(conns[0].ended);
    assert!(
        conns[1]
            .got
            .iter()
            .any(|t| t.contains("\"op\":\"subscribe\""))
    );
    let ws_recs: Vec<Record> = records_under(s.fs.as_ref(), &s.dir, "/_open/")
        .into_iter()
        .filter(|r| r.src == Src::WebSocket)
        .collect();
    let by_conn = |id: u32| -> Vec<String> {
        texts(
            &ws_recs
                .iter()
                .filter(|r| r.conn_id == id)
                .cloned()
                .collect::<Vec<_>>(),
        )
    };
    assert_eq!(by_conn(1), conns[0].data);
    assert_eq!(by_conn(2), conns[1].data);
    let sess = records_under(s.fs.as_ref(), &s.dir, "/_session/");
    let answer_conns: Vec<u32> = sess
        .iter()
        .filter(|r| String::from_utf8_lossy(&r.payload).contains("\"op\":\"subscribe\""))
        .map(|r| r.conn_id)
        .collect();
    assert_eq!(answer_conns, vec![1, 2]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn instruments_failure_refuses_start() {
    let mut p = Pages::new();
    p.insert(
        ("linear".into(), String::new()),
        (500, r#"{"retCode":10016,"retMsg":"server error"}"#.into()),
    );
    let s = setup("e2e-instruments-fail", Behaviour::default(), p).await;
    let cfg = s.config(
        &[(
            "linear",
            "symbols = [\"BTCUSDT\"]\nchannels = [\"orderbook.50\"]\nconn_budget = 1",
        )],
        "",
    );
    let e = Collector::start(cfg, s.deps())
        .await
        .err()
        .expect("refused");
    assert!(e.contains("500"), "{e}");
    assert_eq!(s.ws.conns().len(), 0, "no connection was opened");
    let sess = records_under(s.fs.as_ref(), &s.dir, "/_session/");
    assert_eq!(
        texts(&sess),
        vec![r#"{"retCode":10016,"retMsg":"server error"}"#.to_string()]
    );
    assert_eq!(sess[0].src, Src::Rest);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn traded_topic_not_planned_refuses_start() {
    let s = setup(
        "e2e-traded-missing",
        Behaviour::default(),
        pages(&[("spot", &["BTCUSDT"])]),
    )
    .await;
    let cfg = s.config(
        &[("spot", "symbols = \"all\"\nchannels = [\"orderbook.50\"]\nconn_budget = 1")],
        "\n[[traded_channels]]\nmarket = \"spot\"\nsymbol = \"ETHUSDT\"\nchannel = \"orderbook.50\"\n",
    );
    let e = Collector::start(cfg, s.deps())
        .await
        .err()
        .expect("refused");
    assert!(e.contains("ETHUSDT"), "{e}");
    assert_eq!(s.ws.conns().len(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn slow_disk_overflow_reconnects() {
    let mut b = Behaviour::default();
    b.stream_every = Some(Duration::from_millis(1));
    b.pace = Duration::ZERO;
    let s = setup("e2e-slow-disk", b, pages(&[("linear", &["BTCUSDT"])])).await;
    let gate = Arc::new(Gate::default());
    gate.close();
    let deps = Deps {
        fs: Arc::new(SlowFs {
            inner: s.fs.clone(),
            gate: gate.clone(),
        }),
        free_space: SetFree::new(u64::MAX),
        clock: Arc::new(SystemClock),
    };
    let cfg = s.config(
        &[(
            "linear",
            "symbols = [\"BTCUSDT\"]\nchannels = [\"orderbook.50\"]\nconn_budget = 1",
        )],
        "[write]\nqueue_bytes = 8192",
    );
    let h = Collector::start(cfg, deps).await.expect("start");
    eventually("an overflow and a new connection", || {
        h.state()
            .ingest
            .conns
            .get(&1)
            .is_some_and(|c| c.overflows == 1)
            && s.ws.accepted.load(std::sync::atomic::Ordering::SeqCst) >= 2
    })
    .await;
    gate.open();
    eventually("a later connection writes", || {
        h.state()
            .ingest
            .conns
            .iter()
            .any(|(id, c)| *id > 1 && c.frames_written > 3)
    })
    .await;
    h.shutdown().await.expect("shutdown");
    let conns = s.ws.conns();
    assert!(conns[0].ended, "the overflowed connection was closed");
    assert!(
        conns[1]
            .got
            .iter()
            .any(|t| t.contains("\"op\":\"subscribe\""))
    );
    let sess = records_under(s.fs.as_ref(), &s.dir, "/_session/");
    let overflow: Vec<serde_json::Value> = sess
        .iter()
        .filter(|r| r.src == Src::Note)
        .map(|r| serde_json::from_slice(&r.payload).expect("note"))
        .filter(|n: &serde_json::Value| n["note"] == "overflow")
        .collect();
    assert!(overflow.iter().any(|n| n["conn_id"] == 1), "{overflow:?}");
    // Every frame accepted before the overflow is in L0, in order: a prefix of
    // what the first connection sent.
    let c1: Vec<String> = texts(
        &records_under(s.fs.as_ref(), &s.dir, "/_open/")
            .into_iter()
            .filter(|r| r.conn_id == 1 && r.src == Src::WebSocket)
            .collect::<Vec<_>>(),
    );
    assert!(c1.len() >= 2, "{}", c1.len());
    assert_eq!(c1[..], conns[0].data[..c1.len()]);
    assert_eq!(
        consecutive_us(&c1),
        c1.len(),
        "no frame of the connection is missing inside the prefix"
    );
}

/// Book `u` values of a run of deltas are consecutive (nothing dropped).
fn consecutive_us(frames: &[String]) -> usize {
    let us: Vec<i64> = frames
        .iter()
        .map(|f| {
            let v: serde_json::Value = serde_json::from_str(f).expect("json");
            v["data"]["u"].as_i64().expect("u")
        })
        .collect();
    assert!(us.windows(2).all(|w| w[1] == w[0] + 1), "{us:?}");
    us.len()
}
