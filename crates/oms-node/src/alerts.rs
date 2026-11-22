//! Trading-safety alerts sent by the node itself (C-ALERT, ADR-029).
//!
//! ```text
//! commit stage ──► published read model ──► oms-alert thread ──► alert-direct ──► Telegram
//!  (flush, then      (durable records         (conditions,          (outbox, repeat,
//!   step 1)           only)                    grace)                second channel)
//! ```
//!
//! The thread `oms-alert` reads only the published read model, which the
//! commit stage updates after the journal flush succeeded. A batch whose
//! flush failed never reaches it, so nothing it would cause is ever sent.
//! The one condition not read from the journal is `gateway_block`: the
//! gateway's own refusal to send, which carries no journaled data.
//!
//! The thread never writes the journal or the core and is not on the commit
//! path; `AlertHandle::set` and `once` only take alert-direct's mutex. The
//! one message that is not after durable is the fatal one: it reports that
//! durability failed, and holds only the fatal reason and the last durable
//! sequence.

use crate::published::Published;
use crate::util::Fatal;
use crate::venue::bybit::gateway::BlockView;
use crate::{info, warn};
use alert_direct::{AlertConfig, AlertDirect, AlertHandle, AlertState, Clock, SecondChannel};
use oms_core::*;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// The prefix of every message (grill Q18: same bot, prefixes).
pub const PREFIX: &str = "[OMS P1]";
/// Most mismatches listed per kind.
const MISMATCHES_LISTED: usize = 3;

/// One alert condition that holds now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cond {
    pub key: String,
    pub text: String,
    /// How long it must hold at every evaluation before its start is sent.
    pub grace_ms: i64,
}

/// How long a condition waits before its start is sent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Grace {
    /// Latches and operator acts: at once.
    None,
    /// States every reconnect or boot passes through, and measured states
    /// that can flap (storage low): `grace_ms`.
    Passing,
}

impl Grace {
    fn ms(self, passing_ms: i64) -> i64 {
        match self {
            Grace::None => 0,
            Grace::Passing => passing_ms,
        }
    }
}

/// The serde name of a value (`daily_loss`, `venue_position_change`).
fn snake<T: serde::Serialize>(v: &T) -> String {
    match serde_json::to_value(v) {
        Ok(serde_json::Value::String(s)) => s,
        Ok(other) => other.to_string(),
        Err(_) => String::new(),
    }
}

fn kst_us(at: Ts) -> String {
    alert_direct::kst(i64::try_from(at / 1000).unwrap_or(i64::MAX))
}

fn secs_between(from: Ts, to: Ts) -> u64 {
    to.saturating_sub(from) / 1_000_000
}

/// What a plain-only hold does to a reduce-only order: it passes only with
/// a margin policy; without one every hold refuses every order (C-HOLD).
fn reduce_only_clause(core: &Core) -> &'static str {
    if core.config.policy.margin.is_some() {
        "감소 전용은 받음"
    } else {
        "증거금 정책이 없어 감소 전용도 거절"
    }
}

/// The key, text and grace of one hold reason; `None` for the reasons that
/// are covered by other conditions. No wildcard arm: a new reason does not
/// compile until it has a text and a grace.
pub fn hold_condition(
    h: &HoldReason,
    core: &Core,
    now: Ts,
    gateway_block: Option<&str>,
) -> Option<(String, String, Grace)> {
    let c = match h {
        HoldReason::NoConfig => (
            "hold:no_config".to_string(),
            "설정이 아직 활성화되지 않음. 모든 신규 주문 거절".to_string(),
            Grace::Passing,
        ),
        HoldReason::LinkDown => {
            let mut text = format!(
                "거래소 연결 끊김(세션 {}). 모든 신규 주문 거절",
                core.link.session
            );
            if let Some(b) = gateway_block {
                text.push_str(&format!(". 게이트웨이가 연결을 막음: {b}"));
            }
            ("hold:link_down".to_string(), text, Grace::Passing)
        }
        HoldReason::Resync => (
            "hold:resync".to_string(),
            format!(
                "재연결 뒤 거래소 상태를 아직 다시 확인하지 않음(세션 {}). 일반 주문 거절",
                core.link.session
            ),
            Grace::Passing,
        ),
        HoldReason::InDoubt => {
            let since: Vec<Ts> = core
                .open
                .iter()
                .filter_map(|id| core.order(*id))
                .filter_map(|o| o.doubt.as_ref().map(|d| d.since))
                .collect();
            let oldest = since.iter().min().copied().unwrap_or(now);
            (
                "hold:in_doubt".to_string(),
                format!(
                    "결과를 모르는 주문이 {}개 있음(가장 오래된 것 {}초 전, {}부터). 일반 주문 거절",
                    since.len(),
                    secs_between(oldest, now),
                    kst_us(oldest)
                ),
                Grace::Passing,
            )
        }
        // Covered per kind by `mismatch:<kind>`.
        HoldReason::Mismatch => return None,
        HoldReason::OperatorAccount => {
            let note = core
                .halt_account
                .as_ref()
                .map(|h| h.note.as_str())
                .unwrap_or("");
            (
                "hold:operator_account".to_string(),
                format!("운영자가 계좌를 중지함. 모든 신규 주문 거절. 메모: {note}"),
                Grace::None,
            )
        }
        // Never in `holds(None)`; covered by `halt_strategy:<s>`.
        HoldReason::OperatorStrategy => return None,
        // Measured every 5 s with no hysteresis: it can flap near the
        // threshold, so it waits like a passing state (review S2).
        HoldReason::StorageLow => (
            "hold:storage_low".to_string(),
            "저널 저장 공간 부족. 모든 신규 주문 거절".to_string(),
            Grace::Passing,
        ),
        HoldReason::Reducing => {
            let detail = match &core.margin.reducing {
                Some(r) => format!(
                    ". 규칙 {}: {}. {}부터",
                    snake(&r.rule),
                    r.detail,
                    kst_us(r.since)
                ),
                None => String::new(),
            };
            (
                "hold:reducing".to_string(),
                format!("감소 전용(REDUCING). 일반 주문 거절, 운영자만 해제(clear-reduce){detail}"),
                Grace::None,
            )
        }
        HoldReason::VenueSetting(i) => {
            let seen = match core.margin.venue_settings.get(i) {
                Some(s) => format!(
                    "거래소: {}, 레버리지 {}",
                    if s.isolated { "격리" } else { "교차" },
                    s.leverage
                ),
                None => "이번 연결에서 아직 받지 못함".to_string(),
            };
            let wanted = match &core.config.policy.margin {
                Some(p) => format!(
                    "정책: {}, 레버리지 {}",
                    if p.isolated { "격리" } else { "교차" },
                    p.venue_leverage
                ),
                None => String::new(),
            };
            (
                format!("hold:venue_setting:{i}"),
                format!(
                    "{i}의 거래소 증거금 설정을 모르거나 정책과 다름({seen}; {wanted}). {i} 일반 주문 거절"
                ),
                Grace::Passing,
            )
        }
        HoldReason::AccountMode => {
            let mode = core
                .margin
                .account_mode
                .as_deref()
                .unwrap_or("이번 연결에서 아직 받지 못함");
            (
                "hold:account_mode".to_string(),
                format!(
                    "거래소 계정 모드를 모르거나 허용 목록에 없음(모드: {mode}). 일반 주문 거절"
                ),
                Grace::Passing,
            )
        }
        // The holds of C-HOLD are latches: at once.
        HoldReason::KillLatched => {
            let (head, confirmation) = match &core.kill {
                Some(k) => (
                    format!("{}, {}", snake(&k.source), kst_us(k.since)),
                    match (k.confirmed_at, k.outcome) {
                        (Some(at), Some(KillOutcome::Unsupported)) => format!(
                            "확인: 이 게이트웨이에는 일괄 취소가 없음({}). 걸린 주문은 운영자가 취소해야 함(omsctl cancel-all). 장부에 미체결이 남아 있으면 해제는 거절됨(kill_orders_open)",
                            kst_us(at)
                        ),
                        (Some(at), _) => format!("확인: 킬 루프가 0건을 확인함 {}", kst_us(at)),
                        (None, _) => "확인: 킬 루프가 아직 0건을 확인하지 않음".to_string(),
                    },
                ),
                None => (String::new(), String::new()),
            };
            let note = core.kill.as_ref().map(|k| k.note.as_str()).unwrap_or("");
            (
                "hold:kill_latched".to_string(),
                format!(
                    "킬 래치({head}). 모든 주문 거절(감소 전용 포함). 포지션은 닫지 않음. 운영자만 해제: omsctl clear-hold --reason kill. 메모: {note}\n{confirmation}"
                ),
                Grace::None,
            )
        }
        HoldReason::WatcherMismatch => {
            let reduce_only = reduce_only_clause(core);
            let (check, since, note) = match &core.watcher_mismatch {
                Some(w) => (snake(&w.source), kst_us(w.since), w.note.as_str()),
                None => (String::new(), String::new(), ""),
            };
            (
                "hold:watcher_mismatch".to_string(),
                format!(
                    "워처가 불일치를 확정함({check}, {since}). 일반 주문 거절, {reduce_only}. 운영자 해제: clear-hold --reason watcher_mismatch. 내용: {note}"
                ),
                Grace::None,
            )
        }
        HoldReason::VenueChange => {
            let reduce_only = reduce_only_clause(core);
            let (since, note) = match &core.venue_change {
                Some(v) => (kst_us(v.since), v.note.as_str()),
                None => (String::new(), ""),
            };
            (
                "hold:venue_change".to_string(),
                format!(
                    "거래소가 포지션을 바꿈 또는 분류표에 없는 체결({since}). 일반 주문 거절, {reduce_only}. 해제: ack + clear-hold --reason venue_change. 내용: {note}"
                ),
                Grace::None,
            )
        }
        // Never in `holds(None)`; covered by `strategy_link:<s>` (C-LIVE).
        HoldReason::StrategyLink => return None,
        // A passing state: every start passes through it until the
        // watcher's first fresh heartbeat (C-LIVE part 2).
        HoldReason::Unmonitored => {
            let reduce_only = reduce_only_clause(core);
            let (reason, since, stage) = match (&core.monitor, &core.config.policy.monitor) {
                (Some(m), Some(p)) => {
                    let stage = if core.monitor_cancel_due(now) {
                        "걸린 일반 주문 취소 중".to_string()
                    } else {
                        let due = m.since.saturating_add(p.unmonitored_cancel_us);
                        format!("{}초 뒤 걸린 일반 주문 취소", due.saturating_sub(now).div_ceil(1_000_000))
                    };
                    (m.reason.clone(), kst_us(m.since), stage)
                }
                _ => (String::new(), String::new(), String::new()),
            };
            (
                "hold:unmonitored".to_string(),
                format!(
                    "모니터링 상실({reason}, {since}). 포지션을 늘리는 일반 주문 거절, {reduce_only}. {stage}"
                ),
                Grace::Passing,
            )
        }
    };
    Some(c)
}

/// Every alert condition of the published core at `now` (record-time
/// microseconds). `durable_seq` is the read model's sequence; `passing_ms`
/// the grace of the passing states. Each text ends with its evidence line.
pub fn conditions(
    core: &Core,
    durable_seq: u64,
    now: Ts,
    gateway_block: Option<&str>,
    passing_ms: i64,
) -> Vec<Cond> {
    let mut out: Vec<(String, String, Grace)> = Vec::new();
    for h in core.holds(None) {
        if let Some(c) = hold_condition(&h, core, now, gateway_block) {
            out.push(c);
        }
    }
    for (s, halt) in &core.halt_strategy {
        out.push((
            format!("halt_strategy:{s}"),
            format!(
                "운영자가 전략 {s}를 중지함. 그 전략의 신규 주문 거절. 메모: {}",
                halt.note
            ),
            Grace::None,
        ));
    }
    // Per strategy, as the strategy halt (C-LIVE).
    for (s, loss) in &core.strategy_link {
        out.push((
            format!("strategy_link:{s}"),
            format!(
                "전략 {s} 연결 상실({}, {}). 그 전략의 걸린 일반 주문 취소 요청, 그 전략의 신규 일반 주문 거절({}). 걸린 감소 전용 주문은 유지. 그 전략이 다시 hello하면 풀림",
                loss.why,
                kst_us(loss.since),
                reduce_only_clause(core)
            ),
            Grace::None,
        ));
    }
    let mut by_kind: BTreeMap<String, Vec<&Mismatch>> = BTreeMap::new();
    for m in core.mismatches.values().filter(|m| m.acked.is_none()) {
        by_kind.entry(snake(&m.kind)).or_default().push(m);
    }
    for (kind, mut ms) in by_kind {
        ms.sort_by_key(|m| std::cmp::Reverse(m.id));
        let mut text = format!(
            "확인하지 않은 불일치 {kind} {}개. 일반 주문 거절, 운영자가 확인(ack)해야 풀림. 최근 {}개:",
            ms.len(),
            ms.len().min(MISMATCHES_LISTED)
        );
        for m in ms.iter().take(MISMATCHES_LISTED) {
            text.push_str(&format!("\n- #{} {}: {}", m.id, m.subject, m.detail));
        }
        out.push((format!("mismatch:{kind}"), text, Grace::None));
    }
    // Not a hold: without a fresh report plain orders are refused at the
    // order check, and resting ones are cancelled after stale_cancel_us.
    if let Some(p) = &core.config.policy.margin {
        if core.link.up && !core.margin_report_is_fresh(now) {
            let seen = match &core.margin.report {
                None => "아직 받은 증거금 보고가 없음".to_string(),
                Some(r) if r.queried_at > now => {
                    "증거금 보고 시각이 노드 시계보다 미래(시계 역행?)".to_string()
                }
                Some(r) => format!(
                    "마지막 증거금 보고가 {}초 전",
                    secs_between(r.queried_at, now)
                ),
            };
            out.push((
                "margin_stale".to_string(),
                format!(
                    "{seen}(신선도 한도 {}초). 일반 주문 거절, {}초가 지나면 걸린 일반 주문 취소",
                    p.margin_max_age_us / 1_000_000,
                    p.stale_cancel_us / 1_000_000
                ),
                Grace::Passing,
            ));
        }
    }
    let mut v: Vec<Cond> = out
        .into_iter()
        .map(|(key, text, grace)| Cond {
            key,
            text: format!("{text}\n근거: 저널 seq {durable_seq}"),
            grace_ms: grace.ms(passing_ms),
        })
        .collect();
    // The gateway's own state, not journaled (C-ALERT).
    if let Some(b) = gateway_block {
        v.push(Cond {
            key: "gateway_block".to_string(),
            text: format!(
                "게이트웨이가 주문을 보내지 않음: {b}\n근거: 게이트웨이 프로세스 상태(저널에 없음), 저널 seq {durable_seq}"
            ),
            grace_ms: Grace::Passing.ms(passing_ms),
        });
    }
    v
}

/// The fatal message: the reason and the last durable sequence, nothing of
/// the batch that failed.
pub fn fatal_text(reason: &str, durable_seq: u64) -> String {
    format!(
        "치명 종료: {reason}. 노드가 멈춤(exit 2). 마지막 내구 seq {durable_seq}. 걸린 주문은 거래소에 남아 있음, 워처와 운영자가 확인해야 함"
    )
}

/// The start-failure notice (exit 3 after the configuration is active): the
/// error kind only, no path and no token.
pub fn start_failure_text(kind: &str) -> String {
    format!("OMS 시작 실패(exit 3): {kind}")
}

/// The evaluator's pace and the exit flush.
#[derive(Clone, Copy, Debug)]
pub struct Settings {
    /// Grace of the passing states.
    pub grace_ms: i64,
    /// Evaluate at least this often (clock or real time).
    pub eval_ms: i64,
    /// How often the thread looks at the read model.
    pub poll: Duration,
    pub exit_flush: Duration,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            grace_ms: 30_000,
            eval_ms: 1_000,
            poll: Duration::from_millis(100),
            exit_flush: Duration::from_millis(15_000),
        }
    }
}

/// What the thread and the node share.
struct Common {
    handle: AlertHandle,
    published: Arc<Published>,
    fatal: Arc<Fatal>,
    gateway_block: Mutex<Option<BlockView>>,
    /// The configuration of this process is active: conditions are
    /// evaluated from now on.
    active: AtomicBool,
    fatal_sent: AtomicBool,
    /// The kind of a start-up failure after activation (exit 3).
    start_failure: Mutex<Option<&'static str>>,
    stop: AtomicBool,
    evaluations: AtomicU64,
}

impl Common {
    /// Queues the fatal message once, at the first look after the trip.
    /// `true` while the node is fatally stopped: nothing else is evaluated.
    fn fatal_notice(&self) -> bool {
        if !self.fatal.is_tripped() {
            return false;
        }
        if !self.fatal_sent.swap(true, Ordering::SeqCst) {
            let seq = self.published.read().durable_seq;
            let reason = self.fatal.reason().unwrap_or_default();
            self.handle.once(&fatal_text(&reason, seq));
        }
        true
    }
}

/// The evaluator's own state, on the `oms-alert` thread.
struct Evaluator {
    common: Arc<Common>,
    clock: Arc<dyn Clock>,
    settings: Settings,
    /// Clock time each condition was first seen in an unbroken run.
    since: BTreeMap<String, i64>,
    last_seq: Option<u64>,
    last_eval_ms: i64,
    last_eval_at: Instant,
    started: bool,
}

impl Evaluator {
    fn due(&self, seq: u64, now_ms: i64) -> bool {
        let moved = now_ms - self.last_eval_ms;
        self.last_seq != Some(seq)
            || moved >= self.settings.eval_ms
            || moved < 0
            || self.last_eval_at.elapsed() >= Duration::from_millis(self.settings.eval_ms as u64)
    }

    /// One look at the read model; evaluates when the durable sequence
    /// moved or `eval_ms` passed.
    fn tick(&mut self) {
        if !self.common.active.load(Ordering::SeqCst) {
            return;
        }
        let now_ms = self.clock.now_ms();
        let block = self
            .common
            .gateway_block
            .lock()
            .ok()
            .and_then(|b| b.as_ref().and_then(|f| f()));
        let now_us = u64::try_from(now_ms).unwrap_or(0).saturating_mul(1000);
        let (conds, seq, boot, version) = {
            let p = self.common.published.read();
            if !self.due(p.durable_seq, now_ms) {
                return;
            }
            let conds = conditions(
                &p.core,
                p.durable_seq,
                now_us,
                block.as_deref(),
                self.settings.grace_ms,
            );
            (conds, p.durable_seq, p.core.boot, p.core.config.version)
        };
        // The latch may have tripped after the thread's fatal check: then
        // nothing is set, not even the start notice (review N1).
        if self.common.fatal.is_tripped() {
            return;
        }
        self.last_seq = Some(seq);
        self.last_eval_ms = now_ms;
        self.last_eval_at = Instant::now();
        let handle = &self.common.handle;
        let mut seen = BTreeSet::new();
        let mut starts = Vec::new();
        let mut pending = 0usize;
        for c in conds {
            let since = *self.since.entry(c.key.clone()).or_insert(now_ms);
            if now_ms - since >= c.grace_ms {
                starts.push((c.key.clone(), c.text));
            } else {
                pending += 1;
            }
            seen.insert(c.key);
        }
        // Counts the conditions whose start goes out now, and apart those
        // still inside their grace (review N4).
        if !self.started {
            self.started = true;
            handle.once(&format!(
                "OMS 시작 (boot {boot}, config {version}). 활성 조건 {}개(유예 중 {pending}개)",
                starts.len()
            ));
        }
        for (key, text) in starts {
            handle.set(&key, Some(text));
        }
        let gone: Vec<String> = self
            .since
            .keys()
            .filter(|k| !seen.contains(*k))
            .cloned()
            .collect();
        for k in gone {
            self.since.remove(&k);
            // A condition that ended inside its grace sent nothing, and its
            // clear sends nothing either (alert-direct's repeat rule).
            handle.set(&k, None);
        }
        self.common.evaluations.fetch_add(1, Ordering::SeqCst);
    }
}

fn run(mut ev: Evaluator) {
    while !ev.common.stop.load(Ordering::SeqCst) {
        std::thread::park_timeout(ev.settings.poll);
        if ev.common.fatal_notice() {
            continue;
        }
        ev.tick();
    }
}

/// The node's alerts: the sender and the `oms-alert` thread.
pub struct Alerts {
    common: Arc<Common>,
    thread: Mutex<Option<JoinHandle<()>>>,
    exit_flush: Duration,
}

impl Alerts {
    /// Starts the sender (and the second channel's, if any) and the
    /// evaluator. Conditions are evaluated only after [`Alerts::activate`];
    /// a fatal stop is reported from the start.
    pub fn start(
        mut cfg: AlertConfig,
        clock: Arc<dyn Clock>,
        published: Arc<Published>,
        fatal: Arc<Fatal>,
        settings: Settings,
        second: Option<Arc<dyn SecondChannel>>,
    ) -> Result<Alerts, String> {
        cfg.second = second;
        // No state file: every start is made by a person, and a fresh start
        // message per active condition after a restart is wanted; `set`
        // then never writes a file on this thread.
        cfg.state_file = None;
        let handle = AlertDirect::start(cfg, clock.clone())?;
        let common = Arc::new(Common {
            handle,
            published,
            fatal,
            gateway_block: Mutex::new(None),
            active: AtomicBool::new(false),
            fatal_sent: AtomicBool::new(false),
            start_failure: Mutex::new(None),
            stop: AtomicBool::new(false),
            evaluations: AtomicU64::new(0),
        });
        let ev = Evaluator {
            common: common.clone(),
            clock,
            settings,
            since: BTreeMap::new(),
            last_seq: None,
            last_eval_ms: 0,
            last_eval_at: Instant::now(),
            started: false,
        };
        let thread = std::thread::Builder::new()
            .name("oms-alert".into())
            .spawn(move || run(ev))
            .map_err(|e| format!("cannot start the alert thread: {e}"))?;
        Ok(Alerts {
            common,
            thread: Mutex::new(Some(thread)),
            exit_flush: settings.exit_flush,
        })
    }

    /// The configuration of this process is active: the next evaluation
    /// sends the start notice and the conditions that hold.
    pub fn activate(&self) {
        self.common.active.store(true, Ordering::SeqCst);
    }

    /// The node is about to exit with 3 after a start-up failure of `kind`
    /// (`trading_socket`, ...); [`Alerts::finish`] tells it.
    pub fn start_failed(&self, kind: &'static str) {
        if let Ok(mut k) = self.common.start_failure.lock() {
            *k = Some(kind);
        }
    }

    /// The gateway's block view, once the gateway runs (Bybit only).
    pub fn set_gateway_block(&self, view: BlockView) {
        if let Ok(mut b) = self.common.gateway_block.lock() {
            *b = Some(view);
        }
    }

    /// One message with the prefix, outside the conditions: the kill
    /// loop's notices (C-KILL, U18b2). The caller sends it only for what is
    /// durable.
    pub fn notice(&self, text: &str) {
        self.common.handle.once(text);
    }

    /// Evaluations made so far (tests wait on it).
    pub fn evaluations(&self) -> u64 {
        self.common.evaluations.load(Ordering::SeqCst)
    }

    pub fn state(&self) -> AlertState {
        self.common.handle.state()
    }

    pub fn flush_for(&self, d: Duration) -> bool {
        self.common.handle.flush_for(d)
    }

    fn stop_thread(&self) {
        self.common.stop.store(true, Ordering::SeqCst);
        let t = self.thread.lock().ok().and_then(|mut t| t.take());
        if let Some(t) = t {
            t.thread().unpark();
            let _ = t.join();
        }
    }

    /// At the exit, after the drain: stops the evaluator, queues the fatal
    /// message (the latch is tripped, whatever the code), the stop notice
    /// (code 0) or the start-failure notice (code 3 after activation), and
    /// waits up to `exit_flush` for delivery. Exit 3 before activation is
    /// silent. Best effort: what is not delivered by then is lost. Returns
    /// whether everything was delivered.
    pub fn finish(&self, code: i32) -> bool {
        self.stop_thread();
        match code {
            _ if self.common.fatal.is_tripped() => {
                self.common.fatal_notice();
            }
            0 => {
                let open = self.common.published.read().core.open.len();
                self.common.handle.once(&format!(
                    "OMS 정상 종료. 걸린 주문 {open}개는 거래소에 남음"
                ));
            }
            3 if self.common.active.load(Ordering::SeqCst) => {
                let kind = self
                    .common
                    .start_failure
                    .lock()
                    .ok()
                    .and_then(|k| *k)
                    .unwrap_or("unknown");
                self.common.handle.once(&start_failure_text(kind));
            }
            _ => return false,
        }
        let started = Instant::now();
        let delivered = self.common.handle.flush_for(self.exit_flush);
        let ms = started.elapsed().as_millis() as u64;
        if delivered {
            info!("alerts delivered before the exit", "ms": ms);
        } else {
            warn!("alerts not delivered before the exit", "ms": ms, "state": state_json(&self.state()));
        }
        delivered
    }
}

impl Drop for Alerts {
    fn drop(&mut self) {
        self.stop_thread();
    }
}

fn state_json(s: &AlertState) -> serde_json::Value {
    serde_json::json!({
        "queued": s.queued,
        "delivered": s.delivered,
        "dropped_total": s.dropped_total,
        "failures": s.failures,
        "failing_since_ms": s.failing_since_ms,
        "last_errors": s.last_errors,
        "active": s.active,
    })
}

/// `status.alerts`. Neither the token nor the file path is shown.
pub fn status_json(alerts: Option<&Alerts>) -> serde_json::Value {
    let Some(a) = alerts else {
        return serde_json::json!({"configured": false});
    };
    let s = a.state();
    let mut v = state_json(&s);
    v["configured"] = true.into();
    v["second"] = match &s.second {
        Some(x) => serde_json::json!({
            "name": x.name,
            "queued": x.queued,
            "delivered": x.delivered,
            "dropped_total": x.dropped_total,
            "failures": x.failures,
        }),
        None => serde_json::Value::Null,
    };
    v["source"] = "process counters".into();
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    fn every_hold_reason() -> Vec<HoldReason> {
        let x = InstrumentId::new("X").unwrap();
        // Listed by a match without a wildcard arm, so a new reason fails
        // to compile here until it is added to the list.
        let all = vec![
            HoldReason::NoConfig,
            HoldReason::LinkDown,
            HoldReason::Resync,
            HoldReason::InDoubt,
            HoldReason::Mismatch,
            HoldReason::OperatorAccount,
            HoldReason::OperatorStrategy,
            HoldReason::StorageLow,
            HoldReason::Reducing,
            HoldReason::VenueSetting(x),
            HoldReason::AccountMode,
            HoldReason::KillLatched,
            HoldReason::WatcherMismatch,
            HoldReason::VenueChange,
            HoldReason::StrategyLink,
            HoldReason::Unmonitored,
        ];
        for h in &all {
            match h {
                HoldReason::NoConfig
                | HoldReason::LinkDown
                | HoldReason::Resync
                | HoldReason::InDoubt
                | HoldReason::Mismatch
                | HoldReason::OperatorAccount
                | HoldReason::OperatorStrategy
                | HoldReason::StorageLow
                | HoldReason::Reducing
                | HoldReason::VenueSetting(_)
                | HoldReason::AccountMode
                | HoldReason::KillLatched
                | HoldReason::WatcherMismatch
                | HoldReason::VenueChange
                | HoldReason::StrategyLink
                | HoldReason::Unmonitored => {}
            }
        }
        all
    }

    fn config(margin: bool) -> CoreConfig {
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
        for s in ["a", "b"] {
            strategies.insert(
                StrategyId::new(s).unwrap(),
                StrategySpec {
                    enabled: true,
                    max_open_orders: 10,
                    limits: limits.clone(),
                },
            );
        }
        let mut policy = Policy::default();
        if margin {
            policy.margin = Some(MarginPolicy::default());
        }
        CoreConfig {
            version: 1,
            instruments,
            strategies,
            policy,
        }
    }

    const T0: Ts = 1_800_000_000_000_000;

    /// A core built with `Core::handle`: booted and configured.
    fn core(margin: bool) -> Core {
        let mut c = Core::new(7);
        c.handle(
            T0,
            &Input::Boot {
                binary: "test".into(),
                kill_marker: None,
            },
        )
        .unwrap();
        c.handle(T0, &Input::Config(Box::new(config(margin))))
            .unwrap();
        c
    }

    fn handle(c: &mut Core, input: Input) {
        let out = c.handle(T0, &input).unwrap();
        assert!(!out.events.is_empty(), "{input:?} changed nothing");
    }

    fn unclassified(c: &mut Core, key: &str, detail: &str) {
        handle(
            c,
            Input::Venue(VenueInput::UnclassifiedExecution {
                key: FillKey::new(key).unwrap(),
                instrument: InstrumentId::new("X").unwrap(),
                exec_type: "Odd".into(),
                detail: detail.into(),
                venue_ts: T0,
            }),
        );
    }

    fn keys(conds: &[Cond]) -> Vec<&str> {
        conds.iter().map(|c| c.key.as_str()).collect()
    }

    /// C-LIVE part 2. With the gate, a start is unmonitored: the condition
    /// `hold:unmonitored` waits the passing grace, says when the resting
    /// plain orders go, then that they are going; a fresh heartbeat ends it.
    #[test]
    fn unmonitored_is_a_passing_condition_with_its_cancel_time() {
        let mut c = Core::new(7);
        c.handle(T0, &Input::Boot { binary: "test".into(), kill_marker: None }).unwrap();
        let mut cfg = config(true);
        cfg.policy.monitor = Some(MonitorPolicy::default());
        c.handle(T0, &Input::Config(Box::new(cfg))).unwrap();
        let got = conditions(&c, 2, T0 + 1_000_000, None, 30_000);
        let u = got.iter().find(|g| g.key == "hold:unmonitored").expect("hold:unmonitored");
        assert_eq!(u.grace_ms, 30_000);
        assert!(u.text.starts_with("모니터링 상실(no heartbeat since the gate came on, "), "{}", u.text);
        assert!(u.text.contains("포지션을 늘리는 일반 주문 거절, 감소 전용은 받음. 59초 뒤 걸린 일반 주문 취소"), "{}", u.text);
        let got = conditions(&c, 2, T0 + 60_000_001, None, 30_000);
        let u = got.iter().find(|g| g.key == "hold:unmonitored").unwrap();
        assert!(u.text.contains(". 걸린 일반 주문 취소 중"), "{}", u.text);
        let t = T0 + 61_000_000;
        let checks = [("W1".to_string(), Some(t)), ("W2".to_string(), Some(t))].into_iter().collect();
        c.handle(t, &Input::Watcher(WatcherInput::Heartbeat { watcher: "w".into(), checks })).unwrap();
        assert!(conditions(&c, 3, t, None, 30_000).iter().all(|g| g.key != "hold:unmonitored"));
    }

    /// C-LIVE. A lost strategy link is a condition of its own per
    /// strategy, at once, with the reason and the time; the text says what
    /// a reduce-only order gets, as the other plain-only holds. The
    /// strategy's hello ends it.
    #[test]
    fn a_lost_strategy_link_is_its_own_condition_at_once() {
        for (margin, clause) in [(true, "감소 전용은 받음"), (false, "증거금 정책이 없어 감소 전용도 거절")] {
            let mut c = core(margin);
            let a = StrategyId::new("a").unwrap();
            let before = keys(&conditions(&c, 2, T0, None, 30_000)).len();
            handle(
                &mut c,
                Input::Session(SessionInput::Lost {
                    strategy: a.clone(),
                    why: "last trading connection closed".into(),
                }),
            );
            let got = conditions(&c, 3, T0, None, 30_000);
            let links: Vec<&Cond> = got.iter().filter(|g| g.key.starts_with("strategy_link")).collect();
            assert_eq!(got.len(), before + 1, "{:?}", keys(&got));
            assert_eq!(links.len(), 1);
            let cond = links[0];
            assert_eq!(cond.key, "strategy_link:a");
            assert_eq!(cond.grace_ms, 0);
            assert_eq!(
                cond.text,
                format!(
                    "전략 a 연결 상실(last trading connection closed, {}). 그 전략의 걸린 일반 주문 취소 요청, 그 전략의 신규 일반 주문 거절({clause}). 걸린 감소 전용 주문은 유지. 그 전략이 다시 hello하면 풀림\n근거: 저널 seq 3",
                    kst_us(T0)
                )
            );
            handle(&mut c, Input::Session(SessionInput::Hello { strategy: a }));
            assert_eq!(keys(&conditions(&c, 4, T0, None, 30_000)).len(), before);
        }
    }

    /// C-HOLD, review S4. The texts of the two plain-only holds say that a
    /// reduce-only order is accepted only when a margin policy is set;
    /// without one it is refused as well.
    #[test]
    fn the_plain_only_hold_texts_follow_the_margin_policy() {
        for (margin, want, not) in [
            (true, "일반 주문 거절, 감소 전용은 받음", "감소 전용도 거절"),
            (false, "일반 주문 거절, 증거금 정책이 없어 감소 전용도 거절", "감소 전용은 받음"),
        ] {
            let c = core(margin);
            for h in [HoldReason::WatcherMismatch, HoldReason::VenueChange, HoldReason::Unmonitored] {
                let (_, text, _) = hold_condition(&h, &c, T0, None).unwrap();
                assert!(text.contains(want) && !text.contains(not), "margin {margin}, {h:?}: {text}");
            }
        }
    }

    /// Every hold reason has a non-empty text and the grace of the table in
    /// C-ALERT; `Mismatch` and `OperatorStrategy` map to no condition.
    #[test]
    fn every_hold_reason_has_a_text_and_the_grace_of_the_table() {
        let c = core(true);
        for h in every_hold_reason() {
            let got = hold_condition(&h, &c, T0, None);
            let want = match &h {
                HoldReason::Mismatch | HoldReason::OperatorStrategy | HoldReason::StrategyLink => None,
                HoldReason::OperatorAccount | HoldReason::Reducing => Some(Grace::None),
                HoldReason::KillLatched | HoldReason::WatcherMismatch | HoldReason::VenueChange => Some(Grace::None),
                HoldReason::NoConfig
                | HoldReason::StorageLow
                | HoldReason::LinkDown
                | HoldReason::Resync
                | HoldReason::InDoubt
                | HoldReason::VenueSetting(_)
                | HoldReason::AccountMode
                | HoldReason::Unmonitored => Some(Grace::Passing),
            };
            assert_eq!(got.as_ref().map(|g| g.2), want, "{h:?}");
            if let Some((key, text, _)) = got {
                assert!(key.starts_with("hold:"), "{h:?}: {key}");
                assert!(!text.is_empty(), "{h:?}");
                let name = match &h {
                    HoldReason::VenueSetting(i) => format!("venue_setting:{i}"),
                    other => snake(other),
                };
                assert_eq!(key, format!("hold:{name}"), "{h:?}");
            }
        }
        assert_eq!(Grace::None.ms(30_000), 0);
        assert_eq!(Grace::Passing.ms(30_000), 30_000);
    }

    /// `conditions` over cores built with `Core::handle` gives the keys of
    /// the C-ALERT table, each with its grace and its evidence line.
    #[test]
    fn conditions_give_the_keys_of_the_table() {
        // Booted and configured, link down: the passing state only.
        let mut c = core(false);
        let got = conditions(&c, 2, T0, None, 30_000);
        assert_eq!(keys(&got), ["hold:link_down"]);
        assert_eq!(got[0].grace_ms, 30_000);
        assert!(
            got[0].text.ends_with("\n근거: 저널 seq 2"),
            "{}",
            got[0].text
        );

        // The gateway's block is named in the link-down text and is its own
        // condition.
        let got = conditions(&c, 2, T0, Some("ip_banned"), 30_000);
        assert_eq!(keys(&got), ["hold:link_down", "gateway_block"]);
        assert!(got[0].text.contains("게이트웨이가 연결을 막음: ip_banned"));
        assert!(got[1].text.contains("ip_banned") && got[1].text.contains("저널에 없음"));
        assert_eq!(got[1].grace_ms, 30_000);

        // Link up: resync until confirmed.
        handle(
            &mut c,
            Input::Venue(VenueInput::Link {
                up: true,
                session: 1,
                reason: "t".into(),
            }),
        );
        assert_eq!(keys(&conditions(&c, 3, T0, None, 30_000)), ["hold:resync"]);

        // Operator acts and mismatches: grace 0; storage low: the passing
        // grace.
        let a = StrategyId::new("a").unwrap();
        handle(
            &mut c,
            Input::Operator(OperatorCmd::Halt {
                scope: Scope::Account,
                note: "acct-note".into(),
            }),
        );
        handle(
            &mut c,
            Input::Operator(OperatorCmd::Halt {
                scope: Scope::Strategy(a),
                note: "strat-note".into(),
            }),
        );
        handle(
            &mut c,
            Input::Storage {
                low: true,
                free_bytes: 5,
            },
        );
        unclassified(&mut c, "e1", "odd one");
        let got = conditions(&c, 9, T0, None, 30_000);
        assert_eq!(
            keys(&got),
            [
                "hold:resync",
                "hold:operator_account",
                "hold:storage_low",
                // U18b1: an unclassified execution also holds plain orders
                // until the operator clears it (C-HOLD).
                "hold:venue_change",
                "halt_strategy:a",
                "mismatch:unclassified_execution",
            ]
        );
        for g in &got[1..] {
            let want = if g.key == "hold:storage_low" {
                30_000
            } else {
                0
            };
            assert_eq!(g.grace_ms, want, "{}", g.key);
            assert!(g.text.ends_with("\n근거: 저널 seq 9"), "{}", g.text);
        }
        assert!(got[1].text.contains("acct-note"));
        assert!(got[4].text.contains("strat-note"));

        // A margin policy with no report while the link is up: margin_stale
        // and the venue setting and account mode not yet told.
        let mut m = core(true);
        handle(
            &mut m,
            Input::Venue(VenueInput::Link {
                up: true,
                session: 1,
                reason: "t".into(),
            }),
        );
        let got = conditions(&m, 4, T0, None, 30_000);
        assert_eq!(
            keys(&got),
            [
                "hold:resync",
                "hold:venue_setting:X",
                "hold:account_mode",
                "margin_stale"
            ]
        );
        assert!(got.iter().all(|g| g.grace_ms == 30_000));
        assert!(
            got[3].text.contains("아직 받은 증거금 보고가 없음"),
            "{}",
            got[3].text
        );
        assert!(got[3].text.contains("신선도 한도 30초") && got[3].text.contains("60초"));

        // The REDUCING latch, as the core restores it.
        m.margin.reducing = Some(Reducing {
            rule: RuleId::DailyLoss,
            detail: "account value 94 is more than 500 bps below the day base 100".into(),
            since: T0,
        });
        let got = conditions(&m, 5, T0, None, 30_000);
        let r = got
            .iter()
            .find(|g| g.key == "hold:reducing")
            .expect("reducing");
        assert_eq!(r.grace_ms, 0);
        assert!(
            r.text.contains("daily_loss") && r.text.contains("day base 100"),
            "{}",
            r.text
        );
    }

    /// One condition per mismatch kind, with the count and the newest 3
    /// (subject: detail); acknowledged ones are left out.
    #[test]
    fn a_mismatch_condition_per_kind_lists_the_count_and_the_newest_three() {
        let mut c = core(false);
        // The first through the core; the same (kind, subject) is raised
        // once, so the next four are added as the core would add them for
        // other subjects.
        unclassified(&mut c, "e1", "detail-1");
        for i in 2..=5u32 {
            let id = c.next_mismatch_id;
            c.next_mismatch_id += 1;
            c.mismatches.insert(
                id,
                Mismatch {
                    id,
                    kind: MismatchKind::UnclassifiedExecution,
                    subject: format!("instrument:X{i}"),
                    detail: format!("detail-{i}"),
                    raised_at: T0,
                    acked: None,
                },
            );
        }
        // Another kind, raised directly: its own key.
        c.mismatches.insert(
            99,
            Mismatch {
                id: 99,
                kind: MismatchKind::VenuePositionChange,
                subject: "instrument:X".into(),
                detail: "adl".into(),
                raised_at: T0,
                acked: None,
            },
        );
        let got = conditions(&c, 6, T0, None, 30_000);
        let un: Vec<&Cond> = got
            .iter()
            .filter(|g| g.key.starts_with("mismatch:"))
            .collect();
        assert_eq!(
            keys(&un.iter().map(|c| (*c).clone()).collect::<Vec<_>>()),
            [
                "mismatch:unclassified_execution",
                "mismatch:venue_position_change"
            ]
        );
        let t = &un[0].text;
        assert!(t.contains("unclassified_execution 5개"), "{t}");
        for i in [5, 4, 3] {
            assert!(t.contains(&format!("detail-{i}")), "{t}");
        }
        for i in [2, 1] {
            assert!(!t.contains(&format!("detail-{i}")), "{t}");
        }
        assert!(t.contains("instrument:X"), "{t}");

        // Acknowledging one leaves 4; acknowledging the other kind ends its
        // condition.
        let first = *c.mismatches.keys().next().unwrap();
        handle(
            &mut c,
            Input::Operator(OperatorCmd::AckMismatch {
                id: first,
                note: "seen".into(),
            }),
        );
        c.mismatches.get_mut(&99).unwrap().acked = Some(MismatchAck {
            note: "seen".into(),
            at: T0,
        });
        let got = conditions(&c, 7, T0, None, 30_000);
        let un: Vec<&Cond> = got
            .iter()
            .filter(|g| g.key.starts_with("mismatch:"))
            .collect();
        assert_eq!(un.len(), 1);
        assert!(
            un[0].text.contains("unclassified_execution 4개"),
            "{}",
            un[0].text
        );
    }

    /// An evaluator over `core` with alert-direct pointed at a closed
    /// loopback port, active, not yet ticked.
    fn evaluator(core: Core, fatal: Arc<Fatal>) -> Evaluator {
        let tg = alert_direct::TelegramConfig::new("123456:ABCdef_ghi-JKL", "42").unwrap();
        let mut cfg = AlertConfig::new(tg, PREFIX, "sim");
        cfg.routes = vec![alert_direct::Route::direct("http://127.0.0.1:9").unwrap()];
        let clock: Arc<dyn Clock> = Arc::new(alert_direct::SystemClock);
        let common = Arc::new(Common {
            handle: AlertDirect::start(cfg, clock.clone()).unwrap(),
            published: Arc::new(Published::new(core, 9)),
            fatal,
            gateway_block: Mutex::new(None),
            active: AtomicBool::new(true),
            fatal_sent: AtomicBool::new(false),
            start_failure: Mutex::new(None),
            stop: AtomicBool::new(false),
            evaluations: AtomicU64::new(0),
        });
        Evaluator {
            common,
            clock,
            settings: Settings::default(),
            since: BTreeMap::new(),
            last_seq: None,
            last_eval_ms: 0,
            last_eval_at: Instant::now(),
            started: false,
        }
    }

    /// Review N1. The latch trips after the thread's fatal check returned
    /// `false` (here: before `tick` is called at all). The evaluation reads
    /// the state, then sees the trip and sets nothing: no start notice, no
    /// start. Without the trip the same evaluation starts the halt.
    #[test]
    fn a_fatal_trip_during_an_evaluation_sets_nothing() {
        let mut c = core(false);
        handle(
            &mut c,
            Input::Operator(OperatorCmd::Halt {
                scope: Scope::Account,
                note: "n".into(),
            }),
        );
        let mut ev = evaluator(c.clone(), Arc::new(Fatal::default()));
        ev.tick();
        assert_eq!(ev.common.handle.state().active, ["hold:operator_account"]);

        let fatal = Arc::new(Fatal::default());
        let mut ev = evaluator(c, fatal.clone());
        fatal.trip("core fault: x");
        ev.tick();
        assert!(!ev.started);
        assert!(ev.common.handle.state().active.is_empty());
        assert_eq!(ev.common.evaluations.load(Ordering::SeqCst), 0);
    }

    /// The fatal text holds the reason and the last durable sequence only.
    #[test]
    fn the_fatal_text_holds_the_reason_and_the_seq_only() {
        let t = fatal_text("commit failed: fdatasync (injected failure): injected", 41);
        assert_eq!(
            t,
            "치명 종료: commit failed: fdatasync (injected failure): injected. 노드가 멈춤(exit 2). 마지막 내구 seq 41. 걸린 주문은 거래소에 남아 있음, 워처와 운영자가 확인해야 함"
        );
    }
}
