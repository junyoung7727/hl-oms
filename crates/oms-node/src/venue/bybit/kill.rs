//! The Bybit kill loop (C-KILL, U18b2): cancel every order in the kill
//! scope until the venue lists none, then keep looking for late orders.
//!
//! `KillLoop` is the rule alone: no I/O, time as input (ms on the loop's
//! clock), like `RateBudget`. It answers each step with the actions to
//! perform. The driver (`run`) performs them on an injected `KillClock`.
//!
//! Rounds (TN table 7-8): C2 one cancel-all, C3 count the open orders,
//! C4 wait until the round's start plus `round_ms` while orders remain or a
//! step failed, C5 after the first zero count again every `watch_every_ms`
//! for `watch_ms`; a late order goes back to C2 at once. A failed count is
//! never zero.

use oms_core::*;
use serde_json::Value;

use super::CancelAll;
use super::exec::text;
use super::rest::RestError;

/// The loop's times (`[venue]` `kill_*`, validated at load).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KillTiming {
    /// One cancel-all per round; the next round starts this long after
    /// the last one started.
    pub round_ms: u64,
    /// Zero must hold this long after the first zero count.
    pub watch_ms: u64,
    /// The count during the watch is repeated this often.
    pub watch_every_ms: u64,
    /// A notice repeats when this long passed since the last one.
    pub notice_ms: u64,
}

impl KillTiming {
    /// The design values (TN table 7-8 C4, C5).
    pub const DEFAULT: KillTiming = KillTiming {
        round_ms: 1_000,
        watch_ms: 30_000,
        watch_every_ms: 5_000,
        notice_ms: 60_000,
    };
}

/// What a cancel-all achieved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CancelAnswer {
    /// The venue accepted it.
    Done(CancelAll),
    /// The request went out and failed (refused, 5xx, no answer).
    Failed(String),
    /// Nothing left the host (a block, the pause).
    NotSent(String),
}

/// Where the loop is (`status.gateway.kill_loop.phase`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Idle,
    Rounds,
    Watch,
    Done,
    Stopped,
}

/// Why a notice goes out (C4 rule).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoticeKind {
    Start,
    /// A known count above zero that differs from the last one shown.
    Change,
    /// `notice_ms` passed since the last notice of any kind.
    Repeat,
    /// Zero confirmed and kept for `watch_ms`.
    Final,
}

/// The remaining open orders as last counted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Remaining {
    Known(usize),
    /// The last cancel-all or count failed: the count is not known.
    Unknown(String),
}

impl std::fmt::Display for Remaining {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Remaining::Known(n) => write!(f, "{n}건"),
            Remaining::Unknown(why) => write!(f, "모름: {why}"),
        }
    }
}

/// One notice, before the position block is added.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notice {
    pub kind: NoticeKind,
    pub round: u32,
    pub remaining: Remaining,
    pub cancel_all_calls: u32,
    pub last_error: Option<String>,
}

/// What the loop reports to the core when it ends.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Report {
    pub rounds: u32,
    pub cancel_all_calls: u32,
    pub detail: String,
}

/// What the driver must do next.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    CancelAll,
    Count,
    Notice(Notice),
    /// Wait until this time on the loop's clock, then call `wake`.
    SleepUntil(u64),
    /// Send the end to the core; the loop is done.
    Ended(Report),
}

/// The rule of the kill loop.
#[derive(Clone, Debug)]
pub struct KillLoop {
    t: KillTiming,
    phase: Phase,
    round: u32,
    round_start: u64,
    zero_since: Option<u64>,
    watch_due: u64,
    watch_checks: u32,
    remaining: Remaining,
    /// The last known count put in a notice.
    last_shown: Option<usize>,
    last_notice_at: u64,
    calls: u32,
    last_error: Option<String>,
    notices: u32,
}

impl KillLoop {
    pub fn new(t: KillTiming) -> KillLoop {
        KillLoop {
            t,
            phase: Phase::Idle,
            round: 0,
            round_start: 0,
            zero_since: None,
            watch_due: 0,
            watch_checks: 0,
            remaining: Remaining::Unknown("not counted yet".into()),
            last_shown: None,
            last_notice_at: 0,
            calls: 0,
            last_error: None,
            notices: 0,
        }
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    pub fn round(&self) -> u32 {
        self.round
    }

    pub fn remaining(&self) -> &Remaining {
        &self.remaining
    }

    pub fn cancel_all_calls(&self) -> u32 {
        self.calls
    }

    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    pub fn zero_since(&self) -> Option<u64> {
        self.zero_since
    }

    pub fn notices(&self) -> u32 {
        self.notices
    }

    fn notice(&mut self, kind: NoticeKind, now: u64) -> Action {
        self.last_notice_at = now;
        self.notices += 1;
        if let Remaining::Known(n) = self.remaining {
            self.last_shown = Some(n);
        }
        Action::Notice(Notice {
            kind,
            round: self.round,
            remaining: self.remaining.clone(),
            cancel_all_calls: self.calls,
            last_error: self.last_error.clone(),
        })
    }

    /// The repeat notice, when `notice_ms` passed since the last notice.
    fn repeat(&mut self, now: u64, out: &mut Vec<Action>) {
        if now.saturating_sub(self.last_notice_at) >= self.t.notice_ms {
            out.push(self.notice(NoticeKind::Repeat, now));
        }
    }

    /// The change notice for a known count above zero that differs from
    /// the last one shown, else the repeat rule.
    fn shown(&mut self, now: u64, out: &mut Vec<Action>) {
        match self.remaining {
            Remaining::Known(n) if n > 0 && self.last_shown != Some(n) => {
                out.push(self.notice(NoticeKind::Change, now));
            }
            _ => self.repeat(now, out),
        }
    }

    fn next_round(&mut self) -> Action {
        Action::SleepUntil(self.round_start.saturating_add(self.t.round_ms))
    }

    /// C1 → C2: the start notice and round 1.
    pub fn start(&mut self, now: u64) -> Vec<Action> {
        self.phase = Phase::Rounds;
        self.round = 1;
        self.round_start = now;
        vec![self.notice(NoticeKind::Start, now), Action::CancelAll]
    }

    /// The answer of the round's cancel-all (C2).
    pub fn cancel_done(&mut self, now: u64, a: CancelAnswer) -> Vec<Action> {
        if self.phase != Phase::Rounds {
            return Vec::new();
        }
        let why = match a {
            CancelAnswer::Done(_) => {
                self.calls += 1;
                return vec![Action::Count];
            }
            CancelAnswer::Failed(why) => {
                self.calls += 1;
                why
            }
            CancelAnswer::NotSent(why) => why,
        };
        self.last_error = Some(why.clone());
        self.remaining = Remaining::Unknown(why);
        let mut out = Vec::new();
        self.repeat(now, &mut out);
        out.push(self.next_round());
        out
    }

    /// The count of open orders (C3), in a round or in the watch.
    pub fn count_done(&mut self, now: u64, r: Result<usize, String>) -> Vec<Action> {
        let mut out = Vec::new();
        match (self.phase, r) {
            (Phase::Rounds, Ok(0)) => {
                self.remaining = Remaining::Known(0);
                self.phase = Phase::Watch;
                self.zero_since = Some(now);
                self.watch_checks = 0;
                self.watch_due = now
                    .saturating_add(self.t.watch_every_ms)
                    .min(now.saturating_add(self.t.watch_ms));
                self.repeat(now, &mut out);
                out.push(Action::SleepUntil(self.watch_due));
            }
            (Phase::Rounds, Ok(n)) => {
                self.remaining = Remaining::Known(n);
                self.shown(now, &mut out);
                out.push(self.next_round());
            }
            (Phase::Watch, Ok(0)) => {
                self.watch_checks += 1;
                let z = self.zero_since.unwrap_or(now);
                let end = z.saturating_add(self.t.watch_ms);
                if now >= end {
                    self.phase = Phase::Done;
                    out.push(self.notice(NoticeKind::Final, now));
                    out.push(Action::Ended(Report {
                        rounds: self.round,
                        cancel_all_calls: self.calls,
                        detail: format!(
                            "zero open orders from loop time {z} ms, {} checks over {} ms; {} rounds",
                            self.watch_checks, self.t.watch_ms, self.round
                        ),
                    }));
                    return out;
                }
                self.watch_due = self
                    .watch_due
                    .saturating_add(self.t.watch_every_ms)
                    .min(end);
                self.repeat(now, &mut out);
                out.push(Action::SleepUntil(self.watch_due));
            }
            // A late order: back to C2 at once.
            (Phase::Watch, Ok(n)) => {
                self.remaining = Remaining::Known(n);
                self.phase = Phase::Rounds;
                self.zero_since = None;
                self.round += 1;
                self.round_start = now;
                self.shown(now, &mut out);
                out.push(Action::CancelAll);
            }
            (Phase::Rounds | Phase::Watch, Err(why)) => {
                self.last_error = Some(why.clone());
                self.remaining = Remaining::Unknown(why);
                self.phase = Phase::Rounds;
                self.zero_since = None;
                self.repeat(now, &mut out);
                out.push(self.next_round());
            }
            (Phase::Idle | Phase::Done | Phase::Stopped, _) => {}
        }
        out
    }

    /// The time of a `SleepUntil` came.
    pub fn wake(&mut self, now: u64) -> Vec<Action> {
        match self.phase {
            Phase::Rounds => {
                self.round += 1;
                self.round_start = now;
                vec![Action::CancelAll]
            }
            Phase::Watch => vec![Action::Count],
            Phase::Idle | Phase::Done | Phase::Stopped => Vec::new(),
        }
    }

    /// `KillStop`: the loop ends without a notice or an input.
    pub fn stop(&mut self) {
        self.phase = Phase::Stopped;
    }
}

/// What the notices say about the kill and the loop's times.
#[derive(Clone, Debug)]
pub struct NoticeContext {
    pub source: HoldSource,
    pub note: String,
    pub timing: KillTiming,
}

fn secs(ms: u64) -> String {
    if ms.is_multiple_of(1000) {
        format!("{}", ms / 1000)
    } else {
        format!("{}.{}", ms / 1000, (ms % 1000) / 100)
    }
}

fn source_text(s: HoldSource) -> &'static str {
    match s {
        HoldSource::Stream => "개인 스트림 끊김",
        HoldSource::Watcher => "워처",
        HoldSource::Marker => "킬 마커",
        HoldSource::Venue => "거래소",
        HoldSource::W1 => "W1",
        HoldSource::W2 => "W2",
        HoldSource::W3 => "W3",
        HoldSource::W4 => "W4",
    }
}

impl Notice {
    /// The notice text without the position block. Every notice holds the
    /// tag `킬 루프`, the round, the remaining count and the cancel-all
    /// calls so far.
    pub fn text(&self, ctx: &NoticeContext) -> String {
        let (r, k) = (self.round, self.cancel_all_calls);
        let progress = || {
            let err = self
                .last_error
                .as_deref()
                .map(|e| format!(" 마지막 오류: {e}"))
                .unwrap_or_default();
            format!(
                "킬 루프 {r}회차: 남은 미체결 {}, 일괄 취소 {k}회.{err}",
                self.remaining
            )
        };
        match self.kind {
            NoticeKind::Start => format!(
                "킬 루프 시작({}: {}). 모든 주문 거절, 미체결 0건 확인까지 {}초마다 일괄 취소. 포지션은 닫지 않음\n{}",
                source_text(ctx.source),
                ctx.note,
                secs(ctx.timing.round_ms),
                progress()
            ),
            NoticeKind::Change | NoticeKind::Repeat => progress(),
            NoticeKind::Final => format!(
                "킬 루프 정리 완료: 미체결 0건 확인 뒤 {}초 동안 늦은 접수 없음({r}회차, 일괄 취소 {k}회). 해제는 운영자만: omsctl clear-hold --reason kill --note …",
                secs(ctx.timing.watch_ms)
            ),
        }
    }
}

// ---- the position block ------------------------------------------------------

/// The distance from the mark price to the liquidation price in % of the
/// mark, rounded down to 0.01 %. `None` when the venue gives no usable
/// liquidation price (empty, zero, unreadable) or the mark is not above
/// zero.
pub fn liq_distance(mark: &str, liq: &str) -> Option<String> {
    let m = i128::from(num::parse_scaled(mark).ok()?);
    let l = i128::from(num::parse_scaled(liq).ok()?);
    if m <= 0 || l <= 0 {
        return None;
    }
    let d = (m - l).abs() * 10_000 / m;
    Some(format!("{}.{:02} %", d / 100, d % 100))
}

fn position_line(row: &Value) -> Option<String> {
    let symbol = text(row, "symbol").unwrap_or("?");
    let size = match text(row, "size") {
        Ok(s) => s,
        Err(e) => return Some(format!("- {symbol} 읽을 수 없는 행({e})")),
    };
    match Qty::parse(size) {
        Ok(q) if q.0 == 0 => return None,
        Ok(_) => {}
        Err(e) => return Some(format!("- {symbol} 읽을 수 없는 행(size {size:?}: {e})")),
    }
    let side = match text(row, "side") {
        Ok("Buy") => "매수",
        Ok("Sell") => "매도",
        other => return Some(format!("- {symbol} 읽을 수 없는 행(side {other:?})")),
    };
    let field = |name: &str| {
        text(row, name)
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or("모름")
    };
    let mark = field("markPrice");
    let distance = if num::parse_scaled(mark).is_ok_and(|m| m > 0) {
        liq_distance(mark, text(row, "liqPrice").unwrap_or(""))
            .unwrap_or_else(|| "모름(거래소가 청산가를 주지 않음)".to_string())
    } else {
        "모름(마크 가격 없음)".to_string()
    };
    Some(format!(
        "- {symbol} {side} {size}, 미실현손익 {}, 마크 {mark}, 마크에서 강제청산가까지 {distance}",
        field("unrealisedPnl")
    ))
}

/// The position block of every notice, from the rows of
/// `/v5/position/list` read just before it (or why they could not be
/// read). Rows of size 0 are left out. The liquidation price is the
/// venue's estimate under cross margin (F18 §1.4); the header says so.
pub fn position_block(rows: Result<&[Value], &str>, kst: &str) -> String {
    let rows = match rows {
        Ok(r) => r,
        Err(kind) => return format!("포지션 조회 실패({kind}). 수량·손익 모름"),
    };
    let lines: Vec<String> = rows.iter().filter_map(position_line).collect();
    let head = format!("포지션(거래소 /v5/position/list, {kst}, 강제청산가는 거래소 추정):");
    if lines.is_empty() {
        return format!("{head}\n포지션 없음");
    }
    format!("{head}\n{}", lines.join("\n"))
}

// ---- the driver ----------------------------------------------------------------

/// The loop's clock: ms since some start, and a wait until a time on it.
/// Production: tokio time (`TokioClock`); tests: a manual clock.
pub trait KillClock: Send + Sync + 'static {
    fn now_ms(&self) -> u64;
    fn sleep_until(&self, ms: u64) -> impl Future<Output = ()> + Send;
}

/// Tokio time, from the moment the clock was made.
pub struct TokioClock {
    epoch: tokio::time::Instant,
}

impl TokioClock {
    pub fn new() -> TokioClock {
        TokioClock {
            epoch: tokio::time::Instant::now(),
        }
    }
}

impl Default for TokioClock {
    fn default() -> Self {
        TokioClock::new()
    }
}

impl KillClock for TokioClock {
    fn now_ms(&self) -> u64 {
        self.epoch.elapsed().as_millis() as u64
    }

    async fn sleep_until(&self, ms: u64) {
        tokio::time::sleep_until(self.epoch + std::time::Duration::from_millis(ms)).await;
    }
}

/// What the driver needs from the gateway. The venue calls are the
/// gateway's: `cancel_all` (`mod.rs`), `Queries::kill_count`,
/// `Queries::position_rows`.
pub trait KillHost: Send + Sync + 'static {
    /// Whether a cancel-all may leave now: no block and no pause (C2).
    fn allowed(&self) -> Result<(), String>;
    /// True while no request may go out at all (the pause after an IP ban,
    /// a rejected key): no count and no position read then.
    fn quiet(&self) -> bool;
    fn cancel_all(&self) -> impl Future<Output = CancelAnswer> + Send;
    fn count(&self) -> impl Future<Output = Result<usize, String>> + Send;
    fn positions(&self) -> impl Future<Output = Result<Vec<Value>, String>> + Send;
    /// Sends one notice: `[OMS P1]` through the direct alerts, or a `warn`
    /// line without `[alert]`.
    fn notice(&self, text: &str);
    /// Delivers `KillLoopEnded { confirmed }` to the core.
    fn ended(&self, r: Report) -> impl Future<Output = ()> + Send;
}

/// The loop as `status.gateway.kill_loop` shows it (process state).
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct KillView {
    pub phase: Phase,
    pub round: u32,
    /// Null when the last cancel-all or count failed.
    pub remaining: Option<usize>,
    pub cancel_all_calls: u32,
    pub last_error: Option<String>,
    /// Wall time (µs) of the loop's start.
    pub started_at: Option<Ts>,
    /// Wall time (µs) of the first zero of the current watch.
    pub zero_since: Option<Ts>,
    pub notices: u32,
}

impl Default for KillView {
    fn default() -> Self {
        KillView {
            phase: Phase::Idle,
            round: 0,
            remaining: None,
            cancel_all_calls: 0,
            last_error: None,
            started_at: None,
            zero_since: None,
            notices: 0,
        }
    }
}

/// How a run of the loop ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunEnd {
    /// Zero confirmed; `KillLoopEnded` was delivered.
    Confirmed,
    /// `KillStop`, or the command channel closed: nothing more is sent.
    Stopped,
}

/// The emergency budget (C-KILL): at most 2 cancel-alls counted in any
/// 1 s, apart from the order budget (cancel-all has its own venue limit,
/// F20 Q11.6). An outer guard: one cancel-all per round of at least 1 s
/// keeps the loop at 1 per second.
pub fn emergency_budget() -> RateBudget {
    RateBudget::new(2, 2, 1_000_000)
}

enum Step {
    Next(Vec<Action>),
    End(RunEnd),
}

/// Runs one kill loop to its end, or until `KillStop`. A second `Kill`
/// while it runs is logged and ignored. Each action is performed under
/// `ctl`, so a stop ends the loop at the next await.
pub async fn run<H: KillHost, C: KillClock>(
    host: &H,
    clock: &C,
    timing: KillTiming,
    ctx: &NoticeContext,
    ctl: &mut tokio::sync::mpsc::UnboundedReceiver<VenueCommand>,
    view: &std::sync::Mutex<KillView>,
) -> RunEnd {
    let mut m = KillLoop::new(timing);
    let mut budget = emergency_budget();
    let start_ms = clock.now_ms();
    let start_wall = crate::util::wall_us();
    let mut queue: std::collections::VecDeque<Action> = m.start(start_ms).into();
    let show = |m: &KillLoop| {
        let mut v = view.lock().unwrap_or_else(|e| e.into_inner());
        *v = KillView {
            phase: m.phase(),
            round: m.round(),
            remaining: match m.remaining() {
                Remaining::Known(n) => Some(*n),
                Remaining::Unknown(_) => None,
            },
            cancel_all_calls: m.cancel_all_calls(),
            last_error: m.last_error().map(str::to_string),
            started_at: Some(start_wall),
            zero_since: m.zero_since().map(|z| {
                start_wall.saturating_add(z.saturating_sub(start_ms).saturating_mul(1000))
            }),
            notices: m.notices(),
        };
    };
    show(&m);
    while let Some(action) = queue.pop_front() {
        let step = {
            let fut = perform(host, clock, &mut m, &mut budget, ctx, action);
            tokio::pin!(fut);
            loop {
                tokio::select! {
                    biased;
                    c = ctl.recv() => match c {
                        Some(VenueCommand::Kill { .. }) => {
                            crate::warn!("a kill loop runs already; the new Kill is ignored");
                        }
                        Some(VenueCommand::KillStop) | None => break Step::End(RunEnd::Stopped),
                        Some(_) => {}
                    },
                    s = &mut fut => break s,
                }
            }
        };
        match step {
            Step::Next(next) => queue.extend(next),
            Step::End(RunEnd::Stopped) => {
                m.stop();
                show(&m);
                crate::info!("the kill loop stopped: the kill was cleared");
                return RunEnd::Stopped;
            }
            Step::End(RunEnd::Confirmed) => {
                show(&m);
                return RunEnd::Confirmed;
            }
        }
        show(&m);
    }
    RunEnd::Stopped
}

async fn perform<H: KillHost, C: KillClock>(
    host: &H,
    clock: &C,
    m: &mut KillLoop,
    budget: &mut RateBudget,
    ctx: &NoticeContext,
    action: Action,
) -> Step {
    let next = match action {
        Action::CancelAll => {
            let a = cancel_once(host, clock, budget).await;
            m.cancel_done(clock.now_ms(), a)
        }
        Action::Count => {
            let r = if host.quiet() {
                Err("requests are paused".to_string())
            } else {
                host.count().await
            };
            m.count_done(clock.now_ms(), r)
        }
        Action::Notice(n) => {
            let rows = if host.quiet() {
                Err("requests are paused".to_string())
            } else {
                host.positions().await
            };
            let kst = alert_direct::kst(i64::try_from(crate::util::wall_us() / 1000).unwrap_or(0));
            let block = match &rows {
                Ok(r) => position_block(Ok(r), &kst),
                Err(e) => position_block(Err(e), &kst),
            };
            let text = format!("{}\n{block}", n.text(ctx));
            crate::warn!("kill loop notice", "kind": format!("{:?}", n.kind), "round": n.round);
            host.notice(&text);
            Vec::new()
        }
        Action::SleepUntil(t) => {
            clock.sleep_until(t).await;
            m.wake(clock.now_ms())
        }
        Action::Ended(r) => {
            crate::info!("the kill loop confirmed zero open orders", "rounds": r.rounds, "cancel_all_calls": r.cancel_all_calls);
            host.ended(r).await;
            return Step::End(RunEnd::Confirmed);
        }
    };
    Step::Next(next)
}

/// C2: one cancel-all, after the block check and a slot of the emergency
/// budget (waited for on the loop's clock). The block is asked again after
/// the wait: a block that appeared meanwhile keeps the request home.
async fn cancel_once<H: KillHost, C: KillClock>(
    host: &H,
    clock: &C,
    budget: &mut RateBudget,
) -> CancelAnswer {
    if let Err(why) = host.allowed() {
        return CancelAnswer::NotSent(why);
    }
    let slot = loop {
        let now_us = clock.now_ms().saturating_mul(1000);
        match budget.take(RateClass::Cancel, now_us) {
            Ok(s) => break s,
            Err(at) => clock.sleep_until(at.div_ceil(1000)).await,
        }
    };
    if let Err(why) = host.allowed() {
        budget.release(slot);
        return CancelAnswer::NotSent(why);
    }
    let a = host.cancel_all().await;
    match a {
        // No byte left the host: the venue counted nothing.
        CancelAnswer::NotSent(_) => budget.release(slot),
        CancelAnswer::Done(_) | CancelAnswer::Failed(_) => {
            budget.finish(slot, clock.now_ms().saturating_mul(1000))
        }
    }
    a
}

/// The answer of `cancel_all` as the loop reads it. A connection that was
/// never made sent nothing; every other error is a failed request.
pub fn cancel_answer(r: Result<CancelAll, RestError>) -> CancelAnswer {
    match r {
        Ok(c) => CancelAnswer::Done(c),
        Err(e @ RestError::NotSent(_)) => CancelAnswer::NotSent(e.to_string()),
        Err(e) => CancelAnswer::Failed(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const T: KillTiming = KillTiming::DEFAULT;

    /// The longest scenario (ten failed minutes) takes about 2,500 actions.
    const MAX_STEPS: usize = 100_000;

    fn done() -> CancelAnswer {
        CancelAnswer::Done(CancelAll {
            cancelled: 1,
            maybe_more: false,
        })
    }

    /// Runs the machine against a venue given as two functions of the loop
    /// time, every step taking no time, until it ends or `until` passes.
    /// Returns the actions with their times. A machine that asks for more
    /// than `MAX_STEPS` actions (one that never lets time pass) fails the
    /// test instead of growing the log without bound.
    fn simulate(
        timing: KillTiming,
        until: u64,
        cancel: impl Fn(u64) -> CancelAnswer,
        count: impl Fn(u64) -> Result<usize, String>,
    ) -> (KillLoop, Vec<(u64, Action)>) {
        let mut m = KillLoop::new(timing);
        let mut now = 0;
        let mut queue: std::collections::VecDeque<Action> = m.start(now).into();
        let mut log = Vec::new();
        while let Some(a) = queue.pop_front() {
            assert!(
                log.len() < MAX_STEPS,
                "more than {MAX_STEPS} actions by loop time {now} ms: {:?}",
                &log[log.len() - 4..]
            );
            log.push((now, a.clone()));
            let next = match a {
                Action::CancelAll => m.cancel_done(now, cancel(now)),
                Action::Count => m.count_done(now, count(now)),
                Action::SleepUntil(t) => {
                    now = now.max(t);
                    if now > until {
                        break;
                    }
                    m.wake(now)
                }
                Action::Notice(_) => Vec::new(),
                Action::Ended(_) => break,
            };
            queue.extend(next);
        }
        (m, log)
    }

    fn notices(log: &[(u64, Action)]) -> Vec<(u64, NoticeKind)> {
        log.iter()
            .filter_map(|(t, a)| match a {
                Action::Notice(n) => Some((*t, n.kind)),
                _ => None,
            })
            .collect()
    }

    fn times(log: &[(u64, Action)], what: fn(&Action) -> bool) -> Vec<u64> {
        log.iter()
            .filter(|(_, a)| what(a))
            .map(|(t, _)| *t)
            .collect()
    }

    /// C4 rule: ten minutes in which every count fails (the cancel-alls
    /// pass), then zero: exactly start, ten repeats (one per 60 s) and the
    /// final notice, 12 in all. Unknown counts are no change.
    #[test]
    fn ten_minutes_of_failed_queries_send_at_most_twelve_notices() {
        let (m, log) = simulate(
            T,
            u64::MAX,
            |_| done(),
            |t| {
                if t < 600_000 {
                    Err("HTTP 500".into())
                } else {
                    Ok(0)
                }
            },
        );
        let n = notices(&log);
        let mut want = vec![(0, NoticeKind::Start)];
        want.extend((1..=10).map(|i| (i * 60_000, NoticeKind::Repeat)));
        want.push((630_000, NoticeKind::Final));
        assert_eq!(n, want);
        assert_eq!(n.len(), 12);
        assert_eq!(m.phase(), Phase::Done);
        // One cancel-all per round, one round per second.
        let cancels = times(&log, |a| matches!(a, Action::CancelAll));
        assert_eq!(cancels.len(), 601);
        assert!(
            cancels.windows(2).all(|w| w[1] - w[0] >= 1_000),
            "{cancels:?}"
        );
        // In any 10-minute window: at most 1 + 10 + 1.
        for start in (0..=640_000).step_by(1_000) {
            let k = n
                .iter()
                .filter(|(t, _)| (start..start + 600_000).contains(t))
                .count();
            assert!(k <= 12, "{k} notices from {start}");
        }
    }

    /// A count that fails is never zero: the rounds go on and the watch is
    /// never entered; the notice says the count is unknown, with the error.
    #[test]
    fn a_failed_count_is_never_zero() {
        let (m, log) = simulate(T, 10_000, |_| done(), |_| Err("HTTP 503".into()));
        assert!(!log.iter().any(|(_, a)| matches!(a, Action::Ended(_))));
        assert_eq!(m.phase(), Phase::Rounds);
        assert_eq!(m.remaining(), &Remaining::Unknown("HTTP 503".into()));
        assert_eq!(m.zero_since(), None);
        assert_eq!(times(&log, |a| matches!(a, Action::CancelAll)).len(), 11);
        // A refused cancel-all is a failed round too: no count follows it.
        let mut k = KillLoop::new(T);
        k.start(0);
        let a = k.cancel_done(0, CancelAnswer::Failed("HTTP 500".into()));
        assert_eq!(a, [Action::SleepUntil(1_000)]);
        assert_eq!(k.cancel_all_calls(), 1);
        let a = k.wake(1_000);
        assert_eq!(a, [Action::CancelAll]);
        let a = k.cancel_done(1_000, CancelAnswer::NotSent("ip_banned: x".into()));
        assert_eq!(a, [Action::SleepUntil(2_000)]);
        assert_eq!(
            k.cancel_all_calls(),
            1,
            "a request that never left is not a call"
        );
        assert_eq!(k.last_error(), Some("ip_banned: x"));
    }

    /// C5: after the first zero the count runs every 5 s for 30 s; the end
    /// comes only at +30 s with every check zero.
    #[test]
    fn the_machine_watches_30_s_after_the_first_zero() {
        let (m, log) = simulate(T, u64::MAX, |_| done(), |t| Ok(usize::from(t < 2_000)));
        let counts = times(&log, |a| matches!(a, Action::Count));
        assert_eq!(
            counts,
            [
                0, 1_000, 2_000, 7_000, 12_000, 17_000, 22_000, 27_000, 32_000
            ]
        );
        let ended = times(&log, |a| matches!(a, Action::Ended(_)));
        assert_eq!(ended, [32_000]);
        assert_eq!(m.zero_since(), Some(2_000));
        let Some((_, Action::Ended(r))) = log.last() else {
            panic!()
        };
        assert_eq!((r.rounds, r.cancel_all_calls), (3, 3));
        // A watch that is not a multiple of the check interval ends at its
        // own end, not later.
        let t = KillTiming {
            watch_ms: 12_000,
            ..T
        };
        let (_, log) = simulate(t, u64::MAX, |_| done(), |_| Ok(0));
        assert_eq!(
            times(&log, |a| matches!(a, Action::Count)),
            [0, 5_000, 10_000, 12_000]
        );
    }

    /// A late order in the watch goes back to C2 at once, and the watch
    /// starts again from the next zero. A failed count in the watch is a
    /// failed round.
    #[test]
    fn a_late_order_in_the_watch_restarts_the_rounds() {
        let (m, log) = simulate(T, u64::MAX, |_| done(), |t| Ok(usize::from(t == 10_000)));
        let cancels = times(&log, |a| matches!(a, Action::CancelAll));
        assert_eq!(cancels, [0, 10_000, 11_000]);
        assert_eq!(times(&log, |a| matches!(a, Action::Ended(_))), [41_000]);
        assert_eq!(m.zero_since(), Some(11_000));
        let kinds: Vec<NoticeKind> = notices(&log).into_iter().map(|n| n.1).collect();
        assert_eq!(
            kinds,
            [NoticeKind::Start, NoticeKind::Change, NoticeKind::Final]
        );

        let (_, log) = simulate(
            T,
            u64::MAX,
            |_| done(),
            |t| {
                if t == 5_000 {
                    Err("HTTP 500".into())
                } else {
                    Ok(0)
                }
            },
        );
        // The failed watch count is a failed round: the next round is due
        // at once (its start is long past), and its count at the same
        // time fails too.
        assert_eq!(
            times(&log, |a| matches!(a, Action::CancelAll)),
            [0, 5_000, 6_000]
        );
        assert_eq!(times(&log, |a| matches!(a, Action::Ended(_))), [36_000]);
    }

    /// A known count above zero that differs from the last one shown is a
    /// change; the same count again is not, and neither is an unknown one.
    #[test]
    fn a_count_change_is_shown_and_an_unknown_count_is_not_a_change() {
        let counts = |t: u64| -> Result<usize, String> {
            match t / 1_000 {
                0 | 1 => Ok(5),
                2 => Err("HTTP 500".into()),
                3 => Ok(5),
                4 => Ok(3),
                5 => Err("HTTP 500".into()),
                6 => Ok(3),
                _ => Ok(0),
            }
        };
        let (_, log) = simulate(T, u64::MAX, |_| done(), counts);
        assert_eq!(
            notices(&log),
            [
                (0, NoticeKind::Start),
                (0, NoticeKind::Change),
                (4_000, NoticeKind::Change),
                (37_000, NoticeKind::Final)
            ]
        );
        let Action::Notice(n) = &log[log.len() - 2].1 else {
            panic!("{log:?}")
        };
        assert_eq!((n.round, n.remaining.clone()), (8, Remaining::Known(0)));
    }

    /// `stop` ends the loop: later answers ask for nothing.
    #[test]
    fn a_stopped_machine_asks_for_nothing() {
        let mut m = KillLoop::new(T);
        m.start(0);
        m.stop();
        assert_eq!(m.phase(), Phase::Stopped);
        assert!(m.cancel_done(0, done()).is_empty());
        assert!(m.count_done(0, Ok(0)).is_empty());
        assert!(m.wake(1_000).is_empty());
    }

    #[test]
    fn the_notice_texts_hold_the_tag_the_round_the_count_and_the_calls() {
        let ctx = NoticeContext {
            source: HoldSource::Watcher,
            note: "W5: no heartbeat".into(),
            timing: T,
        };
        let n = Notice {
            kind: NoticeKind::Start,
            round: 1,
            remaining: Remaining::Unknown("not counted yet".into()),
            cancel_all_calls: 0,
            last_error: None,
        };
        assert_eq!(
            n.text(&ctx),
            "킬 루프 시작(워처: W5: no heartbeat). 모든 주문 거절, 미체결 0건 확인까지 1초마다 일괄 취소. 포지션은 닫지 않음\n킬 루프 1회차: 남은 미체결 모름: not counted yet, 일괄 취소 0회."
        );
        let n = Notice {
            kind: NoticeKind::Change,
            round: 4,
            remaining: Remaining::Known(2),
            cancel_all_calls: 4,
            last_error: Some("HTTP 500".into()),
        };
        assert_eq!(
            n.text(&ctx),
            "킬 루프 4회차: 남은 미체결 2건, 일괄 취소 4회. 마지막 오류: HTTP 500"
        );
        let n = Notice {
            kind: NoticeKind::Final,
            round: 3,
            remaining: Remaining::Known(0),
            cancel_all_calls: 3,
            last_error: None,
        };
        assert_eq!(
            n.text(&ctx),
            "킬 루프 정리 완료: 미체결 0건 확인 뒤 30초 동안 늦은 접수 없음(3회차, 일괄 취소 3회). 해제는 운영자만: omsctl clear-hold --reason kill --note …"
        );
    }

    /// Distance = |mark − liq| / mark × 100, rounded down to 0.01 %; no
    /// usable liquidation price or a mark not above zero is unknown.
    #[test]
    fn the_liquidation_distance_is_rounded_down_and_unknown_without_a_price() {
        assert_eq!(liq_distance("70000", "61362").as_deref(), Some("12.34 %"));
        // 1/3 = 33.333…: down, not to the nearest.
        assert_eq!(liq_distance("3", "2").as_deref(), Some("33.33 %"));
        assert_eq!(liq_distance("3", "1.0000001").as_deref(), Some("66.66 %"));
        // Short positions: the liquidation price is above the mark.
        assert_eq!(liq_distance("100", "150.5").as_deref(), Some("50.50 %"));
        for (mark, liq) in [
            ("100", ""),
            ("100", "0"),
            ("100", "abc"),
            ("0", "50"),
            ("-1", "50"),
            ("", "50"),
        ] {
            assert_eq!(liq_distance(mark, liq), None, "{mark:?} {liq:?}");
        }
    }

    #[test]
    fn the_position_block_lists_each_position_and_leaves_out_size_zero() {
        let rows = vec![
            json!({"symbol": "BTCUSDT", "size": "0.010", "side": "Buy", "unrealisedPnl": "-1.23", "markPrice": "70000.0", "liqPrice": "61362"}),
            json!({"symbol": "ETHUSDT", "size": "0.5", "side": "Sell", "unrealisedPnl": "2.00", "markPrice": "3000.0", "liqPrice": ""}),
            json!({"symbol": "SOLUSDT", "size": "0", "side": "", "unrealisedPnl": "0", "markPrice": "150", "liqPrice": ""}),
            json!({"symbol": "XRPUSDT", "size": "10", "side": "Buy", "unrealisedPnl": "0.1", "markPrice": "0", "liqPrice": "0.4"}),
        ];
        assert_eq!(
            position_block(Ok(&rows), "11-22 12:00:00 KST"),
            "포지션(거래소 /v5/position/list, 11-22 12:00:00 KST, 강제청산가는 거래소 추정):\n\
             - BTCUSDT 매수 0.010, 미실현손익 -1.23, 마크 70000.0, 마크에서 강제청산가까지 12.34 %\n\
             - ETHUSDT 매도 0.5, 미실현손익 2.00, 마크 3000.0, 마크에서 강제청산가까지 모름(거래소가 청산가를 주지 않음)\n\
             - XRPUSDT 매수 10, 미실현손익 0.1, 마크 0, 마크에서 강제청산가까지 모름(마크 가격 없음)"
        );
        assert!(position_block(Ok(&rows[2..3]), "t").ends_with("\n포지션 없음"));
        assert!(position_block(Ok(&[]), "t").ends_with("\n포지션 없음"));
        assert_eq!(
            position_block(Err("HTTP 500"), "t"),
            "포지션 조회 실패(HTTP 500). 수량·손익 모름"
        );
    }

    // ---- the driver against a loopback venue, on a manual clock ----------------

    use crate::venue::bybit::query::Queries;
    use crate::venue::bybit::rest::{Credentials, Rest};
    use http_fake::{Answer as Http, FakeHttp};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    /// A manual loop clock: `sleep_until` parks until the test moves the
    /// time past the deadline. `parked` is true while the driver waits at
    /// the current time, i.e. it has done everything due until now.
    pub(crate) struct ManualClock {
        now: tokio::sync::watch::Sender<u64>,
        seen: Mutex<Option<u64>>,
    }

    impl ManualClock {
        pub(crate) fn new() -> ManualClock {
            ManualClock {
                now: tokio::sync::watch::Sender::new(0),
                seen: Mutex::new(None),
            }
        }

        pub(crate) fn parked(&self) -> bool {
            *self.seen.lock().unwrap() == Some(self.now_ms())
        }

        pub(crate) fn advance(&self, ms: u64) {
            self.now.send_modify(|t| *t += ms);
        }
    }

    impl KillClock for ManualClock {
        fn now_ms(&self) -> u64 {
            *self.now.borrow()
        }

        async fn sleep_until(&self, ms: u64) {
            let mut rx = self.now.subscribe();
            loop {
                let now = *rx.borrow_and_update();
                if now >= ms {
                    *self.seen.lock().unwrap() = None;
                    return;
                }
                *self.seen.lock().unwrap() = Some(now);
                if rx.changed().await.is_err() {
                    return;
                }
            }
        }
    }

    /// The gateway's side for the driver: the same venue calls as the
    /// gateway, against the fake; the block is set by the test.
    struct TestHost {
        rest: Rest,
        queries: Queries,
        block: Mutex<Option<String>>,
        notices: Mutex<Vec<String>>,
        alerts: Option<crate::alerts::Alerts>,
        ended: Mutex<Vec<Report>>,
    }

    impl KillHost for TestHost {
        fn allowed(&self) -> Result<(), String> {
            match self.block.lock().unwrap().clone() {
                Some(b) => Err(b),
                None => Ok(()),
            }
        }

        fn quiet(&self) -> bool {
            false
        }

        async fn cancel_all(&self) -> CancelAnswer {
            cancel_answer(super::super::cancel_all(&self.rest).await)
        }

        async fn count(&self) -> Result<usize, String> {
            self.queries.kill_count().await.map_err(|e| e.to_string())
        }

        async fn positions(&self) -> Result<Vec<Value>, String> {
            self.queries
                .position_rows()
                .await
                .map_err(|e| e.to_string())
        }

        fn notice(&self, text: &str) {
            self.notices.lock().unwrap().push(text.to_string());
            if let Some(a) = &self.alerts {
                a.notice(text);
            }
        }

        async fn ended(&self, r: Report) {
            self.ended.lock().unwrap().push(r);
        }
    }

    fn ok(result: Value) -> Http {
        Http::Reply(
            200,
            json!({"retCode": 0, "retMsg": "OK", "result": result, "time": 1}).to_string(),
        )
    }

    fn page(rows: Vec<Value>) -> Http {
        ok(json!({"category": "linear", "list": rows, "nextPageCursor": ""}))
    }

    fn open_row(i: usize) -> Value {
        json!({"orderId": format!("o{i}"), "orderLinkId": "", "symbol": "BTCUSDT", "side": "Buy",
               "orderStatus": "New", "leavesQty": "0.001"})
    }

    fn cancelled() -> Http {
        ok(json!({"list": [{"orderId": "o1", "orderLinkId": ""}], "success": "1"}))
    }

    const CANCEL_ALL: &str = "/v5/order/cancel-all";
    const REALTIME: &str = "/v5/order/realtime";
    const POSITIONS: &str = "/v5/position/list";

    /// The fake venue: cancel-all accepted, no open order, one long.
    fn venue() -> FakeHttp {
        let api = FakeHttp::start(Http::Reply(404, "{}".into()));
        api.route(CANCEL_ALL, [cancelled()]);
        api.route(REALTIME, [page(vec![])]);
        api.route(
            POSITIONS,
            [page(vec![
                json!({"symbol": "BTCUSDT", "positionIdx": 0, "size": "0.010", "side": "Buy",
                "unrealisedPnl": "-1.23", "markPrice": "70000.0", "liqPrice": "61362"}),
            ])],
        );
        api
    }

    /// One driver running on a manual clock against `api`.
    struct Rig {
        host: Arc<TestHost>,
        clock: Arc<ManualClock>,
        ctl: tokio::sync::mpsc::UnboundedSender<VenueCommand>,
        view: Arc<Mutex<KillView>>,
        task: tokio::task::JoinHandle<RunEnd>,
        /// Loop time of every cancel-all POST seen at the fake.
        posts: Vec<u64>,
        seen: usize,
    }

    impl Rig {
        fn start(api: &FakeHttp, timing: KillTiming, alerts: Option<crate::alerts::Alerts>) -> Rig {
            Rig::start_with(api, timing, alerts, None)
        }

        fn start_with(
            api: &FakeHttp,
            timing: KillTiming,
            alerts: Option<crate::alerts::Alerts>,
            block: Option<&str>,
        ) -> Rig {
            let rest = || {
                Rest::new(
                    &api.url(),
                    Arc::new(Credentials::new("K", "S")),
                    5000,
                    Duration::from_millis(500),
                )
                .unwrap()
            };
            let host = Arc::new(TestHost {
                rest: rest(),
                queries: Queries::new(rest(), 7, 2000),
                block: Mutex::new(block.map(str::to_string)),
                notices: Mutex::default(),
                alerts,
                ended: Mutex::default(),
            });
            let clock = Arc::new(ManualClock::new());
            let view = Arc::new(Mutex::new(KillView::default()));
            let (ctl, mut rx) = tokio::sync::mpsc::unbounded_channel();
            let ctx = NoticeContext {
                source: HoldSource::Stream,
                note: "private stream down".into(),
                timing,
            };
            let (h, c, v) = (host.clone(), clock.clone(), view.clone());
            let task = tokio::spawn(async move { run(&*h, &*c, timing, &ctx, &mut rx, &v).await });
            Rig {
                host,
                clock,
                ctl,
                view,
                task,
                posts: Vec::new(),
                seen: 0,
            }
        }

        fn now(&self) -> u64 {
            self.clock.now_ms()
        }

        /// Waits until the driver did everything due now (parked) or ended,
        /// and books the new POSTs at the fake to the current loop time.
        async fn settle(&mut self, api: &FakeHttp) {
            for _ in 0..5_000 {
                if self.clock.parked() || self.task.is_finished() {
                    let reqs = api.requests();
                    for q in &reqs[self.seen..] {
                        assert!(
                            q.method != "POST" || q.path() == CANCEL_ALL,
                            "an order-path request other than cancel-all: {}",
                            q.path()
                        );
                        if q.method == "POST" {
                            self.posts.push(self.now());
                        }
                    }
                    self.seen = reqs.len();
                    return;
                }
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
            panic!("the driver neither parks nor ends at {}", self.now());
        }

        /// Moves the clock in 100 ms steps until `until` or the end.
        async fn run_to(&mut self, api: &FakeHttp, until: u64) {
            self.run_by(api, until, 100).await;
        }

        async fn run_by(&mut self, api: &FakeHttp, until: u64, step: u64) {
            self.settle(api).await;
            while self.now() < until && !self.task.is_finished() {
                self.clock.advance(step);
                self.settle(api).await;
            }
        }

        /// The driver's end, waited for at most 5 s of real time: a driver
        /// that does not end fails the test instead of hanging it.
        async fn end(&mut self) -> RunEnd {
            tokio::time::timeout(Duration::from_secs(5), &mut self.task)
                .await
                .unwrap_or_else(|_| panic!("the driver did not end at {}", self.now()))
                .unwrap()
        }

        fn ended(&self) -> Vec<Report> {
            self.host.ended.lock().unwrap().clone()
        }

        fn notices(&self) -> Vec<String> {
            self.host.notices.lock().unwrap().clone()
        }

        /// Every 1 s window of loop time holds at most two cancel-alls.
        fn at_most_two_per_second(&self) {
            for (i, &t) in self.posts.iter().enumerate() {
                let n = self.posts[i..]
                    .iter()
                    .take_while(|&&u| u < t + 1_000)
                    .count();
                assert!(
                    n <= 2,
                    "{n} cancel-alls in the second from {t}: {:?}",
                    self.posts
                );
            }
        }
    }

    fn gets(api: &FakeHttp, path: &str) -> usize {
        api.requests()
            .iter()
            .filter(|q| q.method == "GET" && q.path() == path)
            .count()
    }

    /// Done criterion: the fake leaves one order after the first cancel-all.
    /// The loop repeats one second later, sees zero, counts every 5 s for
    /// 30 s, and only then reports `confirmed`. Only cancel-alls go out on
    /// the order path, at most two in any second.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn one_order_left_after_the_first_cancel_all_repeats_until_zero_then_watches_30_s() {
        let api = venue();
        api.route(REALTIME, [page(vec![open_row(1)]), page(vec![])]);
        let mut r = Rig::start(&api, KillTiming::DEFAULT, None);
        r.run_to(&api, 30_900).await;
        assert_eq!(r.posts, [0, 1_000]);
        assert!(
            r.ended().is_empty(),
            "ended before the 30 s watch: {:?}",
            r.ended()
        );
        assert_eq!(r.view.lock().unwrap().phase, Phase::Watch);
        // Counts: rounds 1 and 2, then the watch at +5 … +25 s.
        assert_eq!(gets(&api, REALTIME), 2 + 5);
        r.run_to(&api, 31_000).await;
        assert_eq!(gets(&api, REALTIME), 2 + 6);
        let ended = r.ended();
        assert_eq!(ended.len(), 1, "{ended:?}");
        assert_eq!((ended[0].rounds, ended[0].cancel_all_calls), (2, 2));
        assert_eq!(r.end().await, RunEnd::Confirmed);
        let v = r.view.lock().unwrap().clone();
        assert_eq!(
            (v.phase, v.round, v.remaining, v.cancel_all_calls),
            (Phase::Done, 2, Some(0), 2)
        );
        r.at_most_two_per_second();
        let n = r.notices();
        assert_eq!(n.len(), 3, "start, the count 1, final: {n:#?}");
        assert!(
            n[1].starts_with("킬 루프 1회차: 남은 미체결 1건, 일괄 취소 1회."),
            "{}",
            n[1]
        );
        assert!(n[2].starts_with("킬 루프 정리 완료"), "{}", n[2]);
    }

    /// Done criterion: cancel-all answers 5xx twice. Each failed round
    /// waits for the next second; the third succeeds, zero follows, and
    /// the loop ends 30 s after that zero. Never more than two cancel-alls
    /// in a second.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_5xx_cancel_all_repeats_until_zero_and_never_exceeds_two_per_second() {
        let api = venue();
        let e500 = || Http::Reply(500, "{}".into());
        api.route(CANCEL_ALL, [e500(), e500(), cancelled()]);
        let mut r = Rig::start(&api, KillTiming::DEFAULT, None);
        r.run_to(&api, 31_900).await;
        assert_eq!(r.posts, [0, 1_000, 2_000]);
        assert_eq!(
            gets(&api, REALTIME),
            1 + 5,
            "no count after a failed cancel-all"
        );
        assert!(r.ended().is_empty());
        r.run_to(&api, 32_000).await;
        assert_eq!(r.ended().len(), 1);
        assert_eq!(
            r.ended()[0].cancel_all_calls,
            3,
            "failed requests count as calls"
        );
        r.at_most_two_per_second();
        let v = r.view.lock().unwrap().clone();
        assert_eq!(v.cancel_all_calls, 3);
    }

    /// The emergency budget is the outer guard: even with rounds of 100 ms
    /// (below what the configuration allows) and orders always left, no
    /// second holds more than two cancel-alls.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn the_emergency_budget_holds_two_per_second_with_a_short_round() {
        let api = venue();
        api.route(REALTIME, [page(vec![open_row(1)])]);
        let t = KillTiming {
            round_ms: 100,
            ..KillTiming::DEFAULT
        };
        let mut r = Rig::start(&api, t, None);
        r.run_to(&api, 5_000).await;
        r.at_most_two_per_second();
        assert_eq!(
            r.posts,
            [
                0, 100, 1_000, 1_100, 2_000, 2_100, 3_000, 3_100, 4_000, 4_100, 5_000
            ]
        );
    }

    /// C2: the block is asked again after the wait for a budget slot. A
    /// block that appears during the wait keeps that cancel-all home and
    /// the round fails with the block text; once it lifts the rounds send
    /// again.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_block_that_appears_during_the_budget_wait_keeps_the_cancel_all_home() {
        let api = venue();
        api.route(REALTIME, [page(vec![open_row(1)])]);
        let t = KillTiming {
            round_ms: 100,
            ..KillTiming::DEFAULT
        };
        let mut r = Rig::start(&api, t, None);
        // Round 3 (at 200) waits for a slot until 1,000.
        r.run_to(&api, 500).await;
        assert_eq!(r.posts, [0, 100]);
        *r.host.block.lock().unwrap() = Some("ip_banned: HTTP 403".into());
        r.run_to(&api, 1_500).await;
        assert_eq!(r.posts, [0, 100], "a cancel-all left under the block");
        assert_eq!(
            r.view.lock().unwrap().last_error.as_deref(),
            Some("ip_banned: HTTP 403")
        );
        *r.host.block.lock().unwrap() = None;
        r.run_to(&api, 2_500).await;
        assert!(r.posts.len() > 2 && r.posts[2] >= 1_500, "{:?}", r.posts);
        r.at_most_two_per_second();
    }

    /// Done criterion: every notice carries the position block, read just
    /// before it: each symbol with a position, its quantity, uPnL, mark and
    /// distance to liquidation, unknown without a liquidation price; a
    /// size-0 row is left out. A failed position query says so.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn every_notice_lists_each_position_with_its_liquidation_distance_or_unknown() {
        let api = venue();
        api.route(
            REALTIME,
            [page(vec![open_row(1), open_row(2)]), page(vec![])],
        );
        api.route(
            POSITIONS,
            [page(vec![
                json!({"symbol": "BTCUSDT", "positionIdx": 0, "size": "0.010", "side": "Buy",
                       "unrealisedPnl": "-1.23", "markPrice": "70000.0", "liqPrice": "61362"}),
                json!({"symbol": "ETHUSDT", "positionIdx": 0, "size": "0.5", "side": "Sell",
                       "unrealisedPnl": "2.00", "markPrice": "3000.0", "liqPrice": ""}),
                json!({"symbol": "SOLUSDT", "positionIdx": 0, "size": "0", "side": "",
                       "unrealisedPnl": "0", "markPrice": "150", "liqPrice": ""}),
            ])],
        );
        let mut r = Rig::start(&api, KillTiming::DEFAULT, None);
        r.run_to(&api, 20_000).await;
        api.route(POSITIONS, [Http::Reply(500, "{}".into())]);
        r.run_to(&api, 40_000).await;
        let n = r.notices();
        assert_eq!(n.len(), 3, "{n:#?}");
        for text in &n[..2] {
            assert!(
                text.contains("\n포지션(거래소 /v5/position/list, "),
                "{text}"
            );
            assert!(
                text.contains("\n- BTCUSDT 매수 0.010, 미실현손익 -1.23, 마크 70000.0, 마크에서 강제청산가까지 12.34 %"),
                "{text}"
            );
            assert!(
                text.contains("\n- ETHUSDT 매도 0.5, 미실현손익 2.00, 마크 3000.0, 마크에서 강제청산가까지 모름(거래소가 청산가를 주지 않음)"),
                "{text}"
            );
            assert!(!text.contains("SOLUSDT"), "{text}");
        }
        assert!(n[2].contains("\n포지션 조회 실패(no answer: "), "{}", n[2]);
        assert!(n[2].ends_with(". 수량·손익 모름"), "{}", n[2]);
        assert!(n.iter().all(|t| t.contains("킬 루프")));
        assert_eq!(gets(&api, POSITIONS), 3, "one position read per notice");
    }

    /// Done criterion: ten minutes in which the count fails, through the
    /// direct alerts to a loopback fake Telegram: at most 12 messages with
    /// `킬 루프` reach it (here: start, ten repeats, final).
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn ten_minutes_of_failed_queries_reach_the_fake_telegram_at_most_twelve_times() {
        let tg = FakeHttp::start(Http::Reply(200, r#"{"ok":true,"result":{}}"#.into()));
        let mut cfg = alert_direct::AlertConfig::new(
            alert_direct::TelegramConfig::new("123456:ABCdef_ghi-JKL", "42").unwrap(),
            crate::alerts::PREFIX,
            "bybit:testnet",
        );
        cfg.routes = vec![alert_direct::Route::direct(&tg.url()).unwrap()];
        cfg.flush_every = Duration::from_millis(20);
        let alerts = crate::alerts::Alerts::start(
            cfg,
            Arc::new(alert_direct::SystemClock),
            Arc::new(crate::published::Published::new(Core::new(1), 0)),
            Arc::new(crate::util::Fatal::default()),
            crate::alerts::Settings::default(),
            None,
        )
        .unwrap();
        let api = venue();
        api.route(REALTIME, [Http::Reply(500, "{}".into())]);
        let mut r = Rig::start(&api, KillTiming::DEFAULT, Some(alerts));
        // Every step of the loop falls on a whole second here: 1 s steps.
        r.run_by(&api, 599_000, 1_000).await;
        api.route(REALTIME, [page(vec![])]);
        r.run_by(&api, 700_000, 1_000).await;
        assert_eq!(r.ended().len(), 1);
        assert_eq!(r.notices().len(), 12);
        let a = r.host.alerts.as_ref().unwrap();
        assert!(a.flush_for(Duration::from_secs(5)), "{:?}", a.state());
        let texts: Vec<String> = tg
            .requests()
            .iter()
            .map(|q| {
                let v: Value = serde_json::from_slice(&q.body).unwrap();
                v["text"].as_str().unwrap_or_default().to_string()
            })
            .collect();
        let kill = texts.iter().filter(|t| t.contains("킬 루프")).count();
        assert!(
            (1..=12).contains(&kill),
            "{kill} kill-loop messages: {texts:#?}"
        );
        assert!(
            texts.iter().all(|t| t.starts_with(crate::alerts::PREFIX)),
            "{texts:#?}"
        );
        r.at_most_two_per_second();
    }

    /// `KillStop` ends the loop at the next await: no notice and no input
    /// after it. A second `Kill` while the loop runs is ignored. A blocked
    /// gateway sends no cancel-all and the round says why.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_stop_ends_the_loop_without_a_notice_or_an_input() {
        let api = venue();
        api.route(REALTIME, [page(vec![open_row(1)])]);
        let mut r = Rig::start_with(&api, KillTiming::DEFAULT, None, Some("ip_banned: HTTP 403"));
        r.run_to(&api, 1_500).await;
        assert!(r.posts.is_empty(), "a cancel-all left under the block");
        assert_eq!(
            gets(&api, REALTIME),
            0,
            "a count after a round that sent nothing"
        );
        {
            let v = r.view.lock().unwrap().clone();
            assert_eq!(
                (v.round, v.remaining, v.last_error.as_deref()),
                (2, None, Some("ip_banned: HTTP 403"))
            );
        }
        *r.host.block.lock().unwrap() = None;
        r.ctl
            .send(VenueCommand::Kill {
                since: 1,
                source: HoldSource::Watcher,
                note: "again".into(),
            })
            .unwrap();
        r.run_to(&api, 2_500).await;
        assert!(!r.task.is_finished(), "a second Kill ended the loop");
        assert_eq!(r.posts, [2_000]);
        let notices = r.notices().len();
        r.ctl.send(VenueCommand::KillStop).unwrap();
        assert_eq!(r.end().await, RunEnd::Stopped);
        r.clock.advance(60_000);
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(r.notices().len(), notices);
        assert!(r.ended().is_empty());
        assert_eq!(r.view.lock().unwrap().phase, Phase::Stopped);
    }
}
