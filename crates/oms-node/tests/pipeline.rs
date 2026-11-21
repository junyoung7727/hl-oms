//! The commit gate, tested in-process with the real core thread, the real
//! commit thread and a real journal file. Needs `--features failpoints`.
//!
//! Property: an output (reply, gateway command, read-model change) is
//! released only after the record that caused it is durable. If the flush
//! fails, nothing of that batch is released and the process stops.

#![allow(clippy::unwrap_used)]

use oms_core::*;
use oms_node::pipeline::{self, PipelineConfig, Shared};
use oms_node::published::Published;
use oms_node::queues::{CoreMsg, InputQueues, Lane};
use oms_node::recovery;
use oms_node::util::Fatal;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;
use tokio::sync::mpsc;

fn dir(name: &str) -> PathBuf {
    let d = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("pipe-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

fn config() -> CoreConfig {
    let x = InstrumentId::new("X").unwrap();
    let mut instruments = BTreeMap::new();
    instruments.insert(x.clone(), InstrumentSpec {
        qty_step: Qty::parse("0.001").unwrap(),
        px_tick: Px::parse("0.1").unwrap(),
        px_max_sig_figs: None,
        min_notional: Amount::ZERO,
        max_order_qty: Qty::parse("100").unwrap(),
        max_order_notional: Amount::parse("1000000").unwrap(),
        price_band_bps: None,
        account_pos_min: SQty::parse("-100").unwrap(),
        account_pos_max: SQty::parse("100").unwrap(),
    });
    let mut limits = BTreeMap::new();
    limits.insert(x, PosLimit { min: SQty::parse("-100").unwrap(), max: SQty::parse("100").unwrap() });
    let mut strategies = BTreeMap::new();
    strategies.insert(StrategyId::new("a").unwrap(), StrategySpec { enabled: true, max_open_orders: 10, limits });
    CoreConfig { version: 1, instruments, strategies, policy: Policy::default() }
}

struct Rig {
    dir: PathBuf,
    queues: Arc<InputQueues>,
    published: Arc<Published>,
    shared: Arc<Shared>,
    fatal: Arc<Fatal>,
    commands: mpsc::Receiver<VenueCommand>,
    pipe: Option<pipeline::Pipeline>,
    key: u32,
}

impl Rig {
    fn start(dir: PathBuf, inline_commit: bool) -> Rig {
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
            fail_next_sync: AtomicBool::new(false),
        });
        let (tx, commands) = mpsc::channel(64);
        let pipe = pipeline::spawn(rec.core, rec.writer, rec.report.last_at, shared.clone(), tx, PipelineConfig {
            max_batch: 16,
            snapshot_every: 1_000_000,
            inline_commit,
            data_dir: dir.clone(),
            writer_name: "test".into(),
        });
        Rig { dir, queues, published, shared, fatal, commands, pipe: Some(pipe), key: 0 }
    }

    fn push(&self, input: Input) {
        assert!(self.queues.try_push(Lane::Control, CoreMsg::new(input)).is_ok());
    }

    fn ask(&self, input: Input) -> Result<Reply, tokio::sync::oneshot::error::RecvError> {
        let (msg, rx) = CoreMsg::with_reply(input);
        assert!(self.queues.try_push(Lane::Orders, msg).is_ok());
        rx.blocking_recv()
    }

    /// Boot, configure, link up and sync, so that orders are acceptable.
    fn ready(&mut self) {
        self.push(Input::Boot { binary: "test".into() });
        assert!(matches!(self.ask(Input::Config(Box::new(config()))), Ok(Reply::Config(Ok(1)))));
        let session = 1;
        self.push(Input::Venue(VenueInput::Link { up: true, session, reason: "t".into() }));
        self.push(Input::Venue(VenueInput::FillsSynced { session, up_to: 1 }));
        let far = u64::MAX / 2;
        self.push(Input::Venue(VenueInput::OpenOrders { session, queried_at: far, orders: vec![] }));
        self.push(Input::Venue(VenueInput::Positions { session, queried_at: far, positions: vec![] }));
        self.wait(|p| p.core.holds(None).is_empty());
    }

    fn wait(&self, cond: impl Fn(&oms_node::published::Inner) -> bool) {
        for _ in 0..500 {
            if cond(&self.published.read()) {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("condition not reached; holds {:?}", self.published.read().core.holds(None));
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

    fn stop(&mut self) {
        self.queues.close();
        if let Some(p) = self.pipe.take() {
            p.core_thread.join().unwrap();
            if let Some(h) = p.commit_thread {
                h.join().unwrap();
            }
            p.snapshot_thread.join().unwrap();
        }
    }
}

fn outputs_follow_durability(inline: bool) {
    let d = dir(if inline { "inline" } else { "thread" });
    let mut rig = Rig::start(d.clone(), inline);
    rig.ready();

    // Normal case: when the reply arrives, the record is already durable and
    // already visible in the read model, and the send was released.
    let input = rig.order();
    let reply = rig.ask(input).unwrap();
    let Reply::NewOrder(NewOrderReply::Accepted { order_id }) = reply else { panic!("{reply:?}") };
    {
        let p = rig.published.read();
        assert!(p.core.order(order_id).is_some(), "the read model must contain the order before the reply is sent");
        assert_eq!(p.durable_seq, rig.shared.durable_seq.load(Ordering::SeqCst));
    }
    let cmd = rig.commands.blocking_recv().unwrap();
    assert!(matches!(cmd, VenueCommand::SendOrder { order_id: o, .. } if o == order_id));
    let durable_before = rig.shared.durable_seq.load(Ordering::SeqCst);
    let orders_before = rig.published.read().core.orders.len();

    // The next flush fails.
    rig.shared.fail_next_sync.store(true, Ordering::SeqCst);
    let input = rig.order();
    let reply = rig.ask(input);
    assert!(reply.is_err(), "no reply may be released for a record that is not durable: {reply:?}");
    assert!(rig.fatal.is_tripped());
    assert!(rig.fatal.reason().unwrap().contains("commit failed"));
    // No command was released, and none ever will be.
    assert!(rig.commands.try_recv().is_err());
    // The read model did not move.
    assert_eq!(rig.published.read().core.orders.len(), orders_before);
    assert_eq!(rig.shared.durable_seq.load(Ordering::SeqCst), durable_before);
    // Input is refused from now on.
    assert!(rig.queues.try_push(Lane::Orders, CoreMsg::new(Input::Tick)).is_err());
    rig.stop();
    assert!(rig.commands.try_recv().is_err());

    // What a restart finds is the state before the failed batch.
    let rec = recovery::recover(&rig.dir, 1 << 20, "test").unwrap();
    assert_eq!(rec.report.last_seq, durable_before);
    assert_eq!(rec.core.orders.len(), orders_before);
    drop(rec);
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn outputs_follow_durability_with_a_commit_thread() {
    outputs_follow_durability(false);
}

#[test]
fn outputs_follow_durability_with_inline_commit() {
    outputs_follow_durability(true);
}

/// Replies come back in the order the core decided, and a batch of inputs
/// that arrive together shares one flush.
#[test]
fn inputs_that_arrive_together_share_a_flush() {
    let d = dir("group");
    let mut rig = Rig::start(d.clone(), false);
    rig.ready();
    let commits_before = rig.published.read().stats.commits;
    let mut waiting = Vec::new();
    for _ in 0..40 {
        let (msg, rx) = CoreMsg::with_reply(rig.order());
        assert!(rig.queues.try_push(Lane::Orders, msg).is_ok());
        waiting.push(rx);
    }
    let mut ids = Vec::new();
    for rx in waiting {
        if let Ok(Reply::NewOrder(NewOrderReply::Accepted { order_id })) = rx.blocking_recv() {
            ids.push(order_id.0);
        }
    }
    assert!(ids.windows(2).all(|w| w[0] < w[1]), "{ids:?}");
    let commits = rig.published.read().stats.commits - commits_before;
    println!("40 requests accepted={} commits={commits}", ids.len());
    assert!(commits < 40, "expected group commit, got {commits} flushes for 40 requests");
    rig.stop();
    std::fs::remove_dir_all(&d).unwrap();
}

/// The texts a fake Telegram got, in order.
fn telegram_texts(f: &http_fake::FakeHttp) -> Vec<String> {
    f.requests()
        .iter()
        .map(|r| serde_json::from_slice::<serde_json::Value>(&r.body).unwrap()["text"].as_str().unwrap_or_default().to_string())
        .collect()
}

/// Waits until the fake Telegram holds `n` requests; returns their texts.
fn wait_texts(f: &http_fake::FakeHttp, n: usize) -> Vec<String> {
    f.wait_requests(n, Duration::from_secs(5));
    let t = telegram_texts(f);
    assert!(t.len() >= n, "expected {n} requests: {t:?}");
    t
}

/// C-ALERT, done criterion: no Telegram request for an event whose batch
/// failed to flush. The real core thread, commit thread, journal and alert
/// thread: an account halt committed normally is sent, and so is its
/// clear; then the next flush fails while a second account halt is
/// committed. Two seconds later the fake has exactly one new request, the
/// fatal message with the last durable sequence, and no request holds the
/// second halt's note.
#[test]
fn no_alert_for_a_batch_that_failed_to_flush() {
    use oms_node::alerts::{self, Alerts, Settings};
    let f = http_fake::FakeHttp::start(http_fake::Answer::Reply(200, r#"{"ok":true,"result":{}}"#.into()));
    let d = dir("alert-flush");
    let mut rig = Rig::start(d.clone(), false);
    rig.ready();
    let tg = alert_direct::TelegramConfig::new("123456:ABCdef_ghi-JKL", "42").unwrap();
    let mut cfg = alert_direct::AlertConfig::new(tg, alerts::PREFIX, "sim");
    cfg.routes = vec![alert_direct::Route::direct(&f.url()).unwrap()];
    cfg.flush_every = Duration::from_millis(20);
    let settings = Settings { grace_ms: 30_000, eval_ms: 100, poll: Duration::from_millis(5), exit_flush: Duration::from_secs(2) };
    let a = Alerts::start(cfg, Arc::new(alert_direct::SystemClock), rig.published.clone(), rig.fatal.clone(), settings, None).unwrap();
    a.activate();
    assert!(wait_texts(&f, 1)[0].contains("OMS 시작"));

    rig.push(Input::Operator(OperatorCmd::Halt { scope: Scope::Account, note: "first-halt".into() }));
    let t = wait_texts(&f, 2);
    assert!(t[1].contains("운영자가 계좌를 중지함") && t[1].contains("first-halt"), "{}", t[1]);
    rig.push(Input::Operator(OperatorCmd::Resume { scope: Scope::Account, note: "r".into() }));
    let t = wait_texts(&f, 3);
    assert!(t[2].contains("해소: ") && t[2].contains("first-halt"), "{}", t[2]);
    assert!(a.flush_for(Duration::from_secs(2)));
    let before = telegram_texts(&f).len();
    let durable_before = rig.shared.durable_seq.load(Ordering::SeqCst);

    rig.shared.fail_next_sync.store(true, Ordering::SeqCst);
    rig.push(Input::Operator(OperatorCmd::Halt { scope: Scope::Account, note: "SECOND-HALT-NOTE".into() }));
    rig.wait(|_| rig.fatal.is_tripped());
    std::thread::sleep(Duration::from_secs(2));
    let all = telegram_texts(&f);
    let new = &all[before..];
    assert_eq!(new.len(), 1, "{new:?}");
    let reason = rig.fatal.reason().unwrap();
    assert!(reason.starts_with("commit failed: "), "{reason}");
    assert!(new[0].ends_with(&alerts::fatal_text(&reason, durable_before)), "{}", new[0]);
    assert!(!all.iter().any(|t| t.contains("SECOND-HALT-NOTE")), "{all:?}");
    assert!(rig.published.read().core.halt_account.is_none());
    rig.stop();
    drop(a);
    std::fs::remove_dir_all(&d).unwrap();
}
