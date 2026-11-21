//! The node's direct alerts (C-ALERT), in-process: the real core thread,
//! the real commit thread, a real journal file, the `oms-alert` thread and
//! alert-direct's sender. Telegram is a loopback `http-fake`; the alert
//! clock is moved by the test. No request leaves the loopback interface.

#![allow(clippy::unwrap_used)]

use alert_direct::{AlertConfig, Clock, Route, SecondChannel, TelegramConfig};
use http_fake::{Answer, FakeHttp};
use oms_core::*;
use oms_node::alerts::{self, Alerts, Settings};
use oms_node::config::NodeConfig;
use oms_node::pipeline::{self, PipelineConfig, Shared};
use oms_node::published::Published;
use oms_node::queues::{CoreMsg, InputQueues, Lane};
use oms_node::recovery;
use oms_node::util::Fatal;
use std::collections::BTreeMap;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

const TOKEN: &str = "123456:ABCdef_ghi-JKL";
const OK: &str = r#"{"ok":true,"result":{}}"#;
const MIN: i64 = 60_000;

fn ok() -> Answer {
    Answer::Reply(200, OK.into())
}

fn fail() -> Answer {
    Answer::Reply(500, r#"{"ok":false,"description":"down"}"#.into())
}

fn base_dir() -> PathBuf {
    std::env::var("OMS_TEST_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_TARGET_TMPDIR")))
}

fn dir(name: &str) -> PathBuf {
    let d = base_dir().join(format!("alerts-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

struct TestClock(AtomicI64);

impl Clock for TestClock {
    fn now_ms(&self) -> i64 {
        self.0.load(Ordering::SeqCst)
    }
}

impl TestClock {
    fn advance(&self, ms: i64) {
        self.0.fetch_add(ms, Ordering::SeqCst);
    }
}

/// A second channel that keeps what it was given.
#[derive(Default)]
struct FakeSecond {
    got: Mutex<Vec<String>>,
}

impl SecondChannel for FakeSecond {
    fn name(&self) -> &str {
        "fake"
    }

    fn send(&self, text: &str) -> Result<(), String> {
        self.got.lock().unwrap().push(text.to_string());
        Ok(())
    }
}

impl FakeSecond {
    fn texts(&self) -> Vec<String> {
        self.got.lock().unwrap().clone()
    }
}

fn config() -> CoreConfig {
    let x = InstrumentId::new("X").unwrap();
    let mut instruments = BTreeMap::new();
    instruments.insert(
        x.clone(),
        InstrumentSpec {
            qty_step: Qty::parse("0.001").unwrap(),
            px_tick: Px::parse("0.1").unwrap(),
            px_max_sig_figs: None,
            min_notional: Amount::ZERO,
            max_order_qty: Qty::parse("100").unwrap(),
            max_order_notional: Amount::parse("1000000").unwrap(),
            price_band_bps: None,
            account_pos_min: SQty::parse("-100").unwrap(),
            account_pos_max: SQty::parse("100").unwrap(),
        },
    );
    let mut limits = BTreeMap::new();
    limits.insert(
        x,
        PosLimit {
            min: SQty::parse("-100").unwrap(),
            max: SQty::parse("100").unwrap(),
        },
    );
    let mut strategies = BTreeMap::new();
    strategies.insert(
        StrategyId::new("a").unwrap(),
        StrategySpec {
            enabled: true,
            max_open_orders: 10,
            limits,
        },
    );
    CoreConfig {
        version: 1,
        instruments,
        strategies,
        policy: Policy::default(),
    }
}

/// The pipeline of `tests/pipeline.rs`, with a gateway command queue of
/// the given size that only the test reads.
struct Rig {
    dir: PathBuf,
    queues: Arc<InputQueues>,
    published: Arc<Published>,
    fatal: Arc<Fatal>,
    commands: mpsc::Receiver<VenueCommand>,
    pipe: Option<pipeline::Pipeline>,
    key: u32,
}

impl Rig {
    fn start(name: &str, command_capacity: usize) -> Rig {
        let dir = dir(name);
        let rec = recovery::recover(&dir, 1 << 20, "test").unwrap();
        let queues = Arc::new(InputQueues::new(64, 64, 64));
        let published = Arc::new(Published::new(rec.core.clone(), rec.report.last_seq));
        let fatal = Arc::new(Fatal::default());
        let shared = Arc::new(Shared {
            queues: queues.clone(),
            published: published.clone(),
            fatal: fatal.clone(),
            snapshot_requested: AtomicBool::new(false),
            durable_seq: AtomicU64::new(rec.report.last_seq),
            #[cfg(feature = "failpoints")]
            fail_next_sync: AtomicBool::new(false),
        });
        let (tx, commands) = mpsc::channel(command_capacity);
        let pipe = pipeline::spawn(
            rec.core,
            rec.writer,
            rec.report.last_at,
            shared,
            tx,
            PipelineConfig {
                max_batch: 16,
                snapshot_every: 1_000_000,
                inline_commit: false,
                data_dir: dir.clone(),
                writer_name: "test".into(),
            },
        );
        let mut rig = Rig {
            dir,
            queues,
            published,
            fatal,
            commands,
            pipe: Some(pipe),
            key: 0,
        };
        rig.ready();
        rig
    }

    /// Pushes on the control lane, waiting while it is full.
    fn push(&self, input: Input) {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut msg = CoreMsg::new(input);
        loop {
            match self.queues.try_push(Lane::Control, msg) {
                Ok(()) => return,
                Err(back) => {
                    assert!(Instant::now() < deadline, "the control lane stayed full");
                    msg = back;
                    std::thread::yield_now();
                }
            }
        }
    }

    fn ask(&self, input: Input) -> Reply {
        let (msg, rx) = CoreMsg::with_reply(input);
        assert!(self.queues.try_push(Lane::Orders, msg).is_ok());
        rx.blocking_recv().unwrap()
    }

    fn sync(&self, session: u64) {
        self.push(Input::Venue(VenueInput::FillsSynced { session, up_to: 1 }));
        let far = u64::MAX / 2;
        self.push(Input::Venue(VenueInput::OpenOrders {
            session,
            queried_at: far,
            orders: vec![],
        }));
        self.push(Input::Venue(VenueInput::Positions {
            session,
            queried_at: far,
            positions: vec![],
        }));
    }

    fn link(&self, up: bool, session: u64) {
        self.push(Input::Venue(VenueInput::Link {
            up,
            session,
            reason: "t".into(),
        }));
    }

    /// Boot, configure, link up and sync: no hold.
    fn ready(&mut self) {
        self.push(Input::Boot {
            binary: "test".into(),
        });
        assert!(matches!(
            self.ask(Input::Config(Box::new(config()))),
            Reply::Config(Ok(1))
        ));
        self.link(true, 1);
        self.sync(1);
        self.wait(|p| p.core.holds(None).is_empty());
    }

    fn wait(&self, cond: impl Fn(&oms_node::published::Inner) -> bool) {
        for _ in 0..500 {
            if cond(&self.published.read()) {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!(
            "condition not reached; holds {:?}",
            self.published.read().core.holds(None)
        );
    }

    fn halt(&self, note: &str) {
        self.push(Input::Operator(OperatorCmd::Halt {
            scope: Scope::Account,
            note: note.into(),
        }));
    }

    fn resume(&self) {
        self.push(Input::Operator(OperatorCmd::Resume {
            scope: Scope::Account,
            note: "resume".into(),
        }));
    }

    fn order(&mut self) -> Input {
        self.key += 1;
        Input::NewOrder(NewOrderReq {
            strategy: StrategyId::new("a").unwrap(),
            client_order_id: ClientOrderId::new(format!("k{}", self.key)).unwrap(),
            instrument: InstrumentId::new("X").unwrap(),
            side: Side::Buy,
            qty: Qty::parse("1").unwrap(),
            px: Px::parse("100").unwrap(),
            tif: Tif::Gtc,
            reduce_only: false,
            expires_at: oms_node::util::wall_us() + 30_000_000,
            market_ref: None,
        })
    }

    fn durable_seq(&self) -> u64 {
        self.published.read().durable_seq
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        self.queues.close();
        // Unblocks a commit stage waiting on a full command queue.
        self.commands.close();
        while self.commands.try_recv().is_ok() {}
        if let Some(p) = self.pipe.take() {
            let _ = p.core_thread.join();
            if let Some(h) = p.commit_thread {
                let _ = h.join();
            }
            let _ = p.snapshot_thread.join();
        }
        if !std::thread::panicking() {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }
}

/// The alert thread on the rig's read model, with a manual clock.
struct Bell {
    alerts: Alerts,
    clock: Arc<TestClock>,
}

fn alert_config(f: &FakeHttp) -> AlertConfig {
    let tg = TelegramConfig::new(TOKEN, "42").unwrap();
    let mut c = AlertConfig::new(tg, alerts::PREFIX, "sim");
    c.routes = vec![Route::direct(&f.url()).unwrap()];
    c.timeout = Duration::from_millis(500);
    c.flush_every = Duration::from_millis(20);
    c
}

fn settings() -> Settings {
    Settings {
        grace_ms: 30_000,
        eval_ms: 100,
        poll: Duration::from_millis(5),
        exit_flush: Duration::from_secs(2),
    }
}

impl Bell {
    fn start(rig: &Rig, cfg: AlertConfig, second: Option<Arc<dyn SecondChannel>>) -> Bell {
        let clock = Arc::new(TestClock(AtomicI64::new(
            i64::try_from(oms_node::util::wall_us() / 1000).unwrap(),
        )));
        let alerts = Alerts::start(
            cfg,
            clock.clone(),
            rig.published.clone(),
            rig.fatal.clone(),
            settings(),
            second,
        )
        .unwrap();
        alerts.activate();
        let bell = Bell { alerts, clock };
        bell.settle();
        bell
    }

    /// Waits until an evaluation that began after this call has ended.
    fn settle(&self) {
        let n = self.alerts.evaluations();
        let deadline = Instant::now() + Duration::from_secs(5);
        while self.alerts.evaluations() < n + 2 {
            assert!(
                Instant::now() < deadline,
                "the alert thread does not evaluate"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    fn advance(&self, ms: i64) {
        self.clock.advance(ms);
        self.settle();
    }

    /// Settles and waits until everything queued is delivered.
    fn delivered(&self) {
        self.settle();
        assert!(
            self.alerts.flush_for(Duration::from_secs(5)),
            "{:?}",
            self.alerts.state()
        );
    }
}

/// The texts Telegram got, in order.
fn texts(f: &FakeHttp) -> Vec<String> {
    f.requests()
        .iter()
        .map(|r| {
            let v: serde_json::Value = serde_json::from_slice(&r.body).unwrap();
            v["text"].as_str().unwrap_or_default().to_string()
        })
        .collect()
}

/// The texts after the first `from`.
fn since(f: &FakeHttp, from: usize) -> Vec<String> {
    texts(f).split_off(from)
}

const HALT_TEXT: &str = "운영자가 계좌를 중지함";
const LINK_TEXT: &str = "거래소 연결 끊김";

/// Done criterion: the outbox bound holds and drops are counted. Telegram
/// answers 500; the start notice, the account halt and 58 repeats of it
/// (clock steps of 5 min) make 60 messages: 50 stay queued and 10 are
/// dropped and counted in `status.alerts`. When Telegram answers again the
/// 50 parts go out, then one notice of the 10 dropped.
#[test]
fn outbox_bound_holds_and_drops_are_counted() {
    let f = FakeHttp::start(fail());
    let rig = Rig::start("outbox", 64);
    let bell = Bell::start(&rig, alert_config(&f), None);
    rig.halt("bound-note");
    rig.wait(|p| p.core.halt_account.is_some());
    bell.settle();
    for _ in 0..58 {
        bell.advance(5 * MIN);
    }
    let st = alerts::status_json(Some(&bell.alerts));
    assert_eq!(st["configured"], true);
    assert_eq!(st["queued"], 50, "{st}");
    assert_eq!(st["dropped_total"], 10, "{st}");
    assert_eq!(st["active"], serde_json::json!(["hold:operator_account"]));
    assert!(st["failures"].as_u64().unwrap() > 0, "{st}");
    assert!(st["failing_since_ms"].as_i64().is_some(), "{st}");
    assert_eq!(st["source"], "process counters");
    assert!(st["second"].is_null());
    assert!(!st.to_string().contains(TOKEN), "{st}");

    f.set_default(ok());
    assert!(
        bell.alerts.flush_for(Duration::from_secs(10)),
        "{:?}",
        bell.alerts.state()
    );
    let s = bell.alerts.state();
    assert_eq!(
        (s.queued, s.delivered, s.dropped_total),
        (0, 51, 10),
        "{s:?}"
    );
    let all = texts(&f);
    let last: Vec<&String> = all[all.len() - 51..].iter().collect();
    for t in &last[..50] {
        assert!(t.contains(HALT_TEXT) && t.contains("bound-note"), "{t}");
    }
    assert_eq!(*last[50], "[OMS P1] 전송 실패로 알림 10개를 버림");
    assert_eq!(s.failing_since_ms, None);
}

/// Done criterion: a lasting condition is sent at its start, every 5 min
/// (with the latest text) while it lasts, and once at its clear.
#[test]
fn repeat_rule_with_an_injected_clock() {
    let f = FakeHttp::start(ok());
    let rig = Rig::start("repeat", 64);
    let bell = Bell::start(&rig, alert_config(&f), None);
    bell.delivered();
    let start = texts(&f);
    assert_eq!(start.len(), 1, "{start:?}");
    assert!(
        start[0].starts_with("[OMS P1] sim ")
            && start[0].contains("OMS 시작 (boot 1, config 1). 활성 조건 0개"),
        "{}",
        start[0]
    );

    let mut n = texts(&f).len();
    let mut next = |want: usize, what: &str| -> Vec<String> {
        bell.delivered();
        let got = since(&f, n);
        assert_eq!(got.len(), want, "{what}: {got:?}");
        n += got.len();
        got
    };
    rig.halt("first-note");
    rig.wait(|p| p.core.halt_account.is_some());
    let got = next(1, "halt");
    assert!(
        got[0].contains(HALT_TEXT) && got[0].contains("메모: first-note"),
        "{}",
        got[0]
    );
    let seq = rig.durable_seq();
    assert!(
        got[0].ends_with(&format!("근거: 저널 seq {seq}")),
        "{}",
        got[0]
    );

    bell.advance(4 * MIN);
    next(0, "+4 min");
    // A new note while it lasts: the repeat carries the latest text.
    rig.halt("second-note");
    rig.wait(|p| {
        p.core
            .halt_account
            .as_ref()
            .is_some_and(|h| h.note == "second-note")
    });
    next(0, "a new note inside the period");
    bell.advance(MIN);
    let got = next(1, "+5 min");
    assert!(got[0].contains("메모: second-note"), "{}", got[0]);
    bell.advance(5 * MIN);
    let got = next(1, "+10 min");
    assert!(got[0].contains("메모: second-note"), "{}", got[0]);
    rig.resume();
    rig.wait(|p| p.core.halt_account.is_none());
    let got = next(1, "resume");
    assert!(
        got[0].contains("해소: ") && got[0].contains("메모: first-note"),
        "{}",
        got[0]
    );
    bell.advance(5 * MIN);
    next(0, "+5 min after the clear");
    assert!(bell.alerts.state().active.is_empty());
}

/// Done criterion: Telegram failing for more than 60 s is told on the
/// second channel, which also gets every message at once.
#[test]
fn telegram_failing_over_60s_notifies_the_second_channel() {
    let f = FakeHttp::start(fail());
    let rig = Rig::start("second", 64);
    let second = Arc::new(FakeSecond::default());
    let bell = Bell::start(&rig, alert_config(&f), Some(second.clone()));
    rig.halt("second-copy");
    rig.wait(|p| p.core.halt_account.is_some());
    bell.settle();
    let deadline = Instant::now() + Duration::from_secs(1);
    while second.texts().len() < 2 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    let got = second.texts();
    assert_eq!(got.len(), 2, "{got:?}");
    assert!(got[0].contains("OMS 시작"), "{}", got[0]);
    assert!(
        got[1].contains(HALT_TEXT) && got[1].contains("second-copy"),
        "{}",
        got[1]
    );
    let failing_since = bell
        .alerts
        .state()
        .failing_since_ms
        .expect("telegram fails");

    let down = |t: &String| t.contains("Telegram 전송이") && t.contains("실패함");
    let to = |ms: i64| failing_since + ms - bell.clock.now_ms();
    bell.advance(to(59_000));
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(second.texts().len(), 2, "{:?}", second.texts());
    bell.advance(to(61_000));
    let t = Instant::now();
    while !second.texts().iter().any(down) {
        assert!(
            t.elapsed() < Duration::from_secs(1),
            "no notice within 1 s: {:?}",
            second.texts()
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    let notice = second.texts().into_iter().find(down).unwrap();
    assert!(
        notice.contains("61초째") && notice.contains("telegram answered 500"),
        "{notice}"
    );
    assert!(!texts(&f).iter().any(down), "the notice went to Telegram");

    f.set_default(ok());
    assert!(bell.alerts.flush_for(Duration::from_secs(5)));
    let last = second.texts().pop().unwrap();
    assert!(last.contains("해소: Telegram 전송이"), "{last}");
    let st = alerts::status_json(Some(&bell.alerts));
    assert_eq!(st["second"]["name"], "fake", "{st}");
    assert_eq!(st["second"]["queued"], 0, "{st}");
}

/// A passing state shorter than the grace sends nothing; one that holds
/// for the grace (exactly 30 s) sends its start, and its end the clear.
#[test]
fn grace_hides_a_short_link_down_and_reports_a_long_one() {
    let f = FakeHttp::start(ok());
    let rig = Rig::start("grace", 64);
    let bell = Bell::start(&rig, alert_config(&f), None);
    bell.delivered();
    let n = texts(&f).len();

    // Down for 20 s, then up and synced: nothing.
    rig.link(false, 1);
    rig.wait(|p| !p.core.link.up);
    bell.settle();
    bell.advance(20_000);
    rig.link(true, 2);
    rig.sync(2);
    rig.wait(|p| p.core.holds(None).is_empty());
    bell.delivered();
    assert_eq!(since(&f, n), Vec::<String>::new());

    // Down again: nothing at 29.999 s, the start at 30 s, nothing more at
    // 31 s; up again: the clear.
    rig.link(false, 2);
    rig.wait(|p| !p.core.link.up);
    bell.settle();
    bell.advance(29_999);
    bell.delivered();
    assert_eq!(since(&f, n), Vec::<String>::new());
    bell.advance(1);
    bell.delivered();
    let got = since(&f, n);
    assert_eq!(got.len(), 1, "{got:?}");
    assert!(got[0].contains(LINK_TEXT), "{}", got[0]);
    bell.advance(1_000);
    bell.delivered();
    assert_eq!(since(&f, n).len(), 1);
    rig.link(true, 3);
    rig.sync(3);
    rig.wait(|p| p.core.holds(None).is_empty());
    bell.delivered();
    let got = since(&f, n);
    assert_eq!(got.len(), 2, "{got:?}");
    assert!(
        got[1].contains("해소: ") && got[1].contains(LINK_TEXT),
        "{}",
        got[1]
    );
}

const STORAGE_TEXT: &str = "저널 저장 공간 부족";

impl Rig {
    /// The storage monitor's measurement, committed and published.
    fn storage(&self, low: bool) {
        self.push(Input::Storage {
            low,
            free_bytes: if low { 5 } else { 1 << 40 },
        });
        self.wait(|p| p.core.storage_low == low);
    }
}

/// Review S2. `hold:storage_low` is measured and can flap near the
/// threshold: low for 20 s, then ok for 5 s, ten times over, sends nothing
/// (each low run is shorter than the 30 s grace).
#[test]
fn a_storage_low_that_flaps_inside_the_grace_sends_nothing() {
    let f = FakeHttp::start(ok());
    let rig = Rig::start("storage-flap", 64);
    let bell = Bell::start(&rig, alert_config(&f), None);
    bell.delivered();
    let n = texts(&f).len();
    for _ in 0..10 {
        rig.storage(true);
        bell.settle();
        bell.advance(20_000);
        rig.storage(false);
        bell.settle();
        bell.advance(5_000);
    }
    bell.delivered();
    assert_eq!(since(&f, n), Vec::<String>::new());
}

/// Review S2. Storage low that lasts past the grace sends one start (none
/// at 29.999 s, one at 30 s, no second one at 35 s), and its end the clear.
#[test]
fn a_storage_low_that_lasts_past_the_grace_sends_one_start() {
    let f = FakeHttp::start(ok());
    let rig = Rig::start("storage-long", 64);
    let bell = Bell::start(&rig, alert_config(&f), None);
    bell.delivered();
    let n = texts(&f).len();
    rig.storage(true);
    bell.settle();
    bell.advance(29_999);
    bell.delivered();
    assert_eq!(since(&f, n), Vec::<String>::new());
    bell.advance(1);
    bell.delivered();
    bell.advance(5_000);
    bell.delivered();
    let got = since(&f, n);
    assert_eq!(got.len(), 1, "{got:?}");
    assert!(
        got[0].contains(STORAGE_TEXT) && !got[0].contains("해소: "),
        "{}",
        got[0]
    );
    rig.storage(false);
    bell.delivered();
    let got = since(&f, n);
    assert_eq!(got.len(), 2, "{got:?}");
    assert!(
        got[1].contains("해소: ") && got[1].contains(STORAGE_TEXT),
        "{}",
        got[1]
    );
}

/// Review N4. The start notice counts the conditions whose start went out
/// at that evaluation and, apart, those still inside their grace: an
/// account halt (grace 0) and a link down (30 s) give one and one.
#[test]
fn the_start_notice_counts_sent_and_pending_conditions_apart() {
    let f = FakeHttp::start(ok());
    let rig = Rig::start("start-count", 64);
    rig.halt("count-note");
    rig.link(false, 1);
    rig.wait(|p| p.core.halt_account.is_some() && !p.core.link.up);
    let bell = Bell::start(&rig, alert_config(&f), None);
    bell.delivered();
    let got = texts(&f);
    assert_eq!(got.len(), 2, "{got:?}");
    assert!(
        got[0].ends_with("\nOMS 시작 (boot 1, config 1). 활성 조건 1개(유예 중 1개)"),
        "{}",
        got[0]
    );
    assert!(got[1].contains("count-note"), "{}", got[1]);
}

/// Telegram that never answers does not slow the commit stage: 1,000
/// halt and resume records commit within 5 s (판단(모델 아님)).
#[test]
fn a_hung_telegram_never_slows_the_commit_stage() {
    let f = FakeHttp::start(Answer::Hang);
    let rig = Rig::start("hung", 64);
    let mut cfg = alert_config(&f);
    cfg.timeout = Duration::from_secs(10);
    let bell = Bell::start(&rig, cfg, None);
    // The start notice is being sent and hangs.
    assert!(!f.wait_requests(1, Duration::from_secs(2)).is_empty());
    let from = rig.durable_seq();
    let evaluations = bell.alerts.evaluations();
    let t = Instant::now();
    for i in 0..1_000 {
        if i % 2 == 0 {
            rig.halt(&format!("h{i}"));
        } else {
            rig.resume();
        }
    }
    rig.wait(|p| p.durable_seq >= from + 1_000);
    let took = t.elapsed();
    println!(
        "1000 halt/resume records committed in {} ms with a hung Telegram",
        took.as_millis()
    );
    assert!(took < Duration::from_secs(5), "{took:?}");
    // The alert thread kept evaluating meanwhile.
    assert!(bell.alerts.evaluations() > evaluations);
    assert_eq!(f.requests().len(), 1, "one request, still hanging");
}

/// The alert thread reads only what is durable: an account halt decided by
/// the core but waiting behind a commit (the commit stage is held on a
/// full gateway queue) is not sent; it is sent once its record is durable.
#[test]
fn nothing_is_sent_before_its_record_is_durable() {
    let f = FakeHttp::start(ok());
    let mut rig = Rig::start("durable", 1);
    let bell = Bell::start(&rig, alert_config(&f), None);
    bell.delivered();
    let n = texts(&f).len();
    // The first order's send fills the queue; the second order's batch is
    // durable and its reply leaves, then its send waits for room.
    let o1 = rig.order();
    assert!(matches!(
        rig.ask(o1),
        Reply::NewOrder(NewOrderReply::Accepted { .. })
    ));
    let o2 = rig.order();
    assert!(matches!(
        rig.ask(o2),
        Reply::NewOrder(NewOrderReply::Accepted { .. })
    ));
    let seq = rig.durable_seq();
    rig.halt("held-note");
    std::thread::sleep(Duration::from_millis(300));
    for _ in 0..3 {
        bell.advance(1_000);
    }
    bell.delivered();
    assert_eq!(
        rig.durable_seq(),
        seq,
        "the halt became durable while the commit stage was held"
    );
    assert!(rig.published.read().core.halt_account.is_none());
    assert_eq!(since(&f, n), Vec::<String>::new());

    // Room in the queue: the commit stage goes on and the halt is durable.
    assert!(rig.commands.blocking_recv().is_some());
    assert!(rig.commands.blocking_recv().is_some());
    rig.wait(|p| p.core.halt_account.is_some());
    bell.delivered();
    let got = since(&f, n);
    assert_eq!(got.len(), 1, "{got:?}");
    assert!(got[0].contains("held-note"), "{}", got[0]);
}

/// After a fatal stop the alert thread sends the fatal message once, with
/// the reason and the last durable sequence, and no start or clear after
/// it: a link-down inside its grace never starts, an operator halt that
/// the read model no longer shows is not cleared.
#[test]
fn after_a_fatal_stop_only_the_fatal_message_is_sent() {
    let f = FakeHttp::start(ok());
    let rig = Rig::start("fatal", 64);
    let bell = Bell::start(&rig, alert_config(&f), None);
    rig.halt("before-fatal");
    rig.wait(|p| p.core.halt_account.is_some());
    rig.link(false, 1);
    rig.wait(|p| !p.core.link.up);
    bell.delivered();
    let n = texts(&f).len();
    assert!(texts(&f)[n - 1].contains("before-fatal"));

    let seq = rig.durable_seq();
    rig.fatal
        .trip("commit failed: fdatasync (injected failure): injected");
    let t = Instant::now();
    while since(&f, n).is_empty() {
        assert!(t.elapsed() < Duration::from_secs(2), "no fatal message");
        std::thread::sleep(Duration::from_millis(5));
    }
    // Both would send if the thread still evaluated: the link-down's grace
    // passes, and the halt disappears from the read model.
    bell.clock.advance(31_000);
    rig.published.write().core.halt_account = None;
    std::thread::sleep(Duration::from_millis(400));
    assert!(bell.alerts.flush_for(Duration::from_secs(2)));
    let got = since(&f, n);
    assert_eq!(got.len(), 1, "{got:?}");
    assert!(got[0].starts_with("[OMS P1] sim "), "{}", got[0]);
    assert!(
        got[0].ends_with(&alerts::fatal_text(
            "commit failed: fdatasync (injected failure): injected",
            seq
        )),
        "{}",
        got[0]
    );
    assert!(bell.alerts.finish(2));
    assert_eq!(
        since(&f, n).len(),
        1,
        "the exit queued the fatal message again"
    );
}

// ---- configuration ------------------------------------------------------------

fn write_mode(path: &Path, body: &str, mode: u32) {
    std::fs::write(path, body).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
}

const CORE: &str = r#"data_dir = "data"
[venue]
kind = "sim"
socket = "venue.sock"
[core]
version = 1
[core.instruments.X]
qty_step = "0.001"
px_tick = "0.1"
min_notional = "0"
max_order_qty = "100"
max_order_notional = "1000000"
account_pos_min = "-10"
account_pos_max = "10"
[core.strategies.a]
enabled = true
max_open_orders = 10
[core.strategies.a.limits.X]
min = "-10"
max = "10"
[core.policy]
"#;

fn node_config(d: &Path, alert: Option<&Path>) -> PathBuf {
    let alert = alert
        .map(|p| format!("[alert]\ntelegram = \"{}\"\n", p.display()))
        .unwrap_or_default();
    let text = format!("{CORE}{alert}");
    let p = d.join("oms.toml");
    std::fs::write(&p, text).unwrap();
    p
}

fn in_a_repository(p: &Path) -> bool {
    p.ancestors().any(|d| d.join(".git").exists())
}

/// The token file is loaded at configuration load with alert-direct's
/// rules: 0600, a regular file, not inside a repository. A file that does
/// not load is a configuration error whose text never holds the token.
/// Without `[alert]` there are no alerts and `status.alerts` says so.
#[test]
fn the_token_file_is_loaded_with_its_rules() {
    let d = dir("token");
    let good = format!(r#"{{"token":"{TOKEN}","chat_id":"42"}}"#);
    let file = d.join("telegram.json");
    write_mode(&file, &good, 0o600);
    let loaded = NodeConfig::load(&node_config(&d, Some(&file)));
    if in_a_repository(&d) {
        // OMS_TEST_DIR inside a checkout: the rule refuses even a good file.
        assert!(loaded.unwrap_err().contains("repository"));
    } else {
        let cfg = loaded.unwrap();
        let a = cfg.alert.as_ref().unwrap();
        assert_eq!(a.loaded.as_ref().unwrap().chat_id(), "42");
        assert_eq!(a.test_api, None);
        assert!(!format!("{cfg:?}").contains(TOKEN), "Debug shows the token");
    }

    let wide = d.join("wide.json");
    write_mode(&wide, &good, 0o640);
    let link = d.join("link.json");
    symlink(&file, &link).unwrap();
    let repo = d.join("repo");
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    let inside = repo.join("telegram.json");
    write_mode(&inside, &good, 0o600);
    let missing = d.join("missing.json");
    for (path, because) in [
        (&wide, "0600"),
        (&link, "0600"),
        (&inside, "inside the repository"),
        (&missing, "telegram"),
    ] {
        let e = NodeConfig::load(&node_config(&d, Some(path))).unwrap_err();
        assert!(e.starts_with("[alert] telegram: "), "{e}");
        assert!(e.contains(because), "{because}: {e}");
        assert!(!e.contains(TOKEN) && !e.contains("ABCdef"), "{e}");
    }

    let cfg = NodeConfig::load(&node_config(&d, None)).unwrap();
    assert!(cfg.alert.is_none());
    assert_eq!(
        alerts::status_json(None),
        serde_json::json!({"configured": false})
    );
    std::fs::remove_dir_all(&d).unwrap();
}

/// The shipped binary (no `failpoints`) ignores `OMS_TEST_TELEGRAM_API`:
/// the repository rule still refuses a token file inside a checkout, and
/// the node exits with the configuration error (2).
#[cfg(not(feature = "failpoints"))]
#[test]
fn the_shipped_binary_ignores_the_test_telegram_api() {
    let d = dir("shipped");
    let repo = d.join("repo");
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    let inside = repo.join("telegram.json");
    write_mode(
        &inside,
        &format!(r#"{{"token":"{TOKEN}","chat_id":"42"}}"#),
        0o600,
    );
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_oms-node"))
        .arg("run")
        .arg("--config")
        .arg(node_config(&d, Some(&inside)))
        .env("OMS_TEST_TELEGRAM_API", "http://127.0.0.1:9")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let code = loop {
        if let Some(st) = child.try_wait().unwrap() {
            break st.code();
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("the node started: the test endpoint was taken");
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let mut err = String::new();
    std::io::Read::read_to_string(&mut child.stderr.take().unwrap(), &mut err).unwrap();
    assert_eq!(code, Some(2), "{err}");
    assert!(err.contains("inside the repository"), "{err}");
    std::fs::remove_dir_all(&d).unwrap();
}
