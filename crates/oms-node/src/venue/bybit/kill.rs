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
    if ms % 1000 == 0 {
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const T: KillTiming = KillTiming::DEFAULT;

    fn done() -> CancelAnswer {
        CancelAnswer::Done(CancelAll {
            cancelled: 1,
            maybe_more: false,
        })
    }

    /// Runs the machine against a venue given as two functions of the loop
    /// time, every step taking no time, until it ends or `until` passes.
    /// Returns the actions with their times.
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
}
