//! The watcher's own kill (C-WATCH part 11, ADR-032), U19b2a's share: the
//! types the loop and the kill sequence speak in ([`NotSent`],
//! [`StopOutcome`], [`AbortWhy`], [`Notice`]), the texts of the kill
//! notices ([`notice_text`]), the position block every notice carries
//! ([`position_block`]) and the reader of the positions behind it
//! ([`read_positions`]). Written anew from the spec and the venue's
//! documentation; the OMS's kill loop is a model only, nothing of it is
//! used. U19b2b adds the loop itself ([`KillLoop`], sans-IO: C2–C5 of
//! table 7-8, the order budget, the request deadlines and the notice rule).
//!
//! Every text says `워처 킬`, the OMS's say `킬 루프`, so a person can tell
//! the two loops apart in one chat.

use serde_json::Value;

use crate::clock::Clock;
use crate::config::Kill;
use crate::decimal::Dec;
use crate::rest::{CancelAll, Client, RestError};

/// The position list of the block (gateway facts Q12): with `settleCoin`
/// and no `symbol` only rows of size > 0 come back (Q12.1).
pub const POSITIONS: &str = "/v5/position/list";
pub const POSITION_PAGE_LIMIT: usize = 200;
/// At most this many pages per read (판단(모델 아님)).
pub const POSITION_MAX_PAGES: usize = 5;
/// The extra time of a read's deadline over `request_timeout_ms`: the
/// request guard's 10 s window plus 1 s, as the engine's request deadline
/// (C-WATCH part 5).
pub const DEADLINE_EXTRA_MS: u64 = 11_000;
/// At most this many rows are listed; the rest is a count (판단(모델 아님):
/// one notice stays within one 3,500-unit part).
pub const ROWS_SHOWN: usize = 20;
/// A venue or local text is cut to this many characters in a notice.
pub const TEXT_MAX: usize = 200;

/// Why a cancel-all was not sent: nothing left the host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotSent {
    /// The key latch: `Client::auth_stop()` is `Some`. Nothing is sent
    /// until the watcher restarts.
    Latched,
    /// The 403 pause of the client.
    Paused,
    /// A local error.
    Local(String),
}

/// How the OMS was stopped before the loop; it picks the last line of the
/// final notice (U19b3b gives the value). Only a latch and a stop with a
/// marker at a checked path name `clear-hold`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopOutcome {
    /// K2 halted the OMS (`watcher_kill` latched).
    Latched,
    /// K3 stopped the unit, the marker is in place and its path was
    /// checked against the OMS's in this process.
    StoppedMarked,
    /// K3 stopped the unit and the marker is there, but its path was not
    /// checked against the OMS's.
    StoppedUncheckedMarker,
    /// K3 stopped the unit; the marker was not written.
    StoppedUnmarked,
    /// K2 did not halt and K3 failed or is unconfirmed.
    Unconfirmed,
}

/// Why a loop was aborted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AbortWhy {
    /// The watcher stops (SIGTERM, SIGINT).
    Shutdown,
    /// A start refusal during the loop, with its reason.
    StartRefused(String),
}

/// The last error of a loop, for its progress notice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LastError {
    /// The last cancel-all was not sent.
    NotSent(NotSent),
    /// The last cancel-all was sent and had no usable answer, or was
    /// refused.
    Failed(String),
}

/// A kill notice, before its text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notice {
    /// At the start of the loop.
    Start { round_ms: u64 },
    /// Once per `notice_ms` while the loop runs: the latest round, the
    /// open-order count (or why it is unknown) and the last error.
    Progress {
        round: u64,
        remaining: Result<usize, String>,
        cancel_all_calls: u64,
        last_error: Option<LastError>,
    },
    /// Zero open orders confirmed and no late order in the watch.
    Final {
        watch_ms: u64,
        round: u64,
        cancel_all_calls: u64,
        outcome: StopOutcome,
    },
    /// The loop was aborted; the positions are not read.
    Abort {
        why: AbortWhy,
        round: u64,
        remaining: Option<usize>,
        cancel_all_calls: u64,
    },
}

/// The fixed progress line of a cancel-all that the key latch held back.
pub const LATCHED_LINE: &str =
    "워처 키가 멈춰 일괄 취소를 보낼 수 없음: 워처를 다시 시작해야 루프가 끝남";
/// The fixed progress line of a cancel-all that the 403 pause held back.
pub const PAUSED_LINE: &str = "워처 요청 일시 정지(HTTP 403): 정지가 풀리면 다시 보냄";
/// The fixed last line of the abort notice: no position read.
pub const NOT_READ_ON_ABORT: &str =
    "포지션 조회 안 함(워처 종료 중). 수량·손익·마크·강제청산가 모름";

/// At most `max` characters, control characters and the Unicode line and
/// paragraph separators as spaces: one text stays on its line.
fn cut(text: &str, max: usize) -> String {
    text.chars()
        .take(max)
        .map(|c| {
            if c.is_control() || matches!(c, '\u{2028}' | '\u{2029}') {
                ' '
            } else {
                c
            }
        })
        .collect()
}

/// Milliseconds as seconds, exactly (`1000` → `1`, `1500` → `1.5`).
fn seconds(ms: u64) -> String {
    Dec(i128::from(ms) * 100_000).to_string()
}

/// The last line of the final notice.
fn outcome_line(o: StopOutcome) -> &'static str {
    match o {
        StopOutcome::Latched => {
            "OMS는 래치됨(watcher_kill). 해제는 운영자만: omsctl clear-hold --reason kill --note …"
        }
        StopOutcome::StoppedMarked => {
            "OMS는 멈춤(systemctl stop). 다음 시작은 킬 마커로 래치됨. 해제는 운영자만: omsctl clear-hold --reason kill --note …"
        }
        StopOutcome::StoppedUncheckedMarker => {
            "OMS는 멈춤(systemctl stop). 킬 마커는 OMS가 읽는 경로인지 확인 못 함: 워처는 OMS가 다시 답하면 watcher_kill을 보냄"
        }
        StopOutcome::StoppedUnmarked => {
            "OMS는 멈춤(systemctl stop). 킬 마커가 없어 다음 시작은 래치 없이 시작함: 워처는 OMS가 다시 답하면 watcher_kill을 보냄"
        }
        StopOutcome::Unconfirmed => {
            "OMS 정지 미확인: OMS가 살아 있으면 래치 없이 주문을 낼 수 있음. 마커는 다음 시작에만 걸림. 워처는 OMS가 다시 답하면 watcher_kill을 보냄"
        }
    }
}

fn last_error_text(e: &LastError) -> String {
    match e {
        LastError::NotSent(NotSent::Latched) => LATCHED_LINE.to_string(),
        LastError::NotSent(NotSent::Paused) => PAUSED_LINE.to_string(),
        LastError::NotSent(NotSent::Local(why)) => {
            format!("일괄 취소를 보내지 못함(로컬 오류: {})", cut(why, TEXT_MAX))
        }
        LastError::Failed(why) => format!("마지막 일괄 취소 실패: {}", cut(why, TEXT_MAX)),
    }
}

/// The body of a kill notice. `header` is the kill sequence's header
/// (U19b3b) and goes into the start notice only. The runtime adds the
/// position block ([`position_block`]) to every notice but the abort
/// notice, whose text already ends with [`NOT_READ_ON_ABORT`].
pub fn notice_text(n: &Notice, header: &str) -> String {
    match n {
        Notice::Start { round_ms } => format!(
            "워처 킬 시작({header}). 워처 키로 미체결 0건 확인까지 {}초마다 일괄 취소(linear, USDT 정산). 포지션은 닫지 않음.",
            seconds(*round_ms)
        ),
        Notice::Progress {
            round,
            remaining,
            cancel_all_calls,
            last_error,
        } => {
            let left = match remaining {
                Ok(n) => format!("{n}건"),
                Err(why) => format!("모름: {}", cut(why, TEXT_MAX)),
            };
            let mut text =
                format!("워처 킬 {round}회차: 남은 미체결 {left}, 일괄 취소 {cancel_all_calls}회.");
            if let Some(e) = last_error {
                text.push(' ');
                text.push_str(&last_error_text(e));
            }
            text
        }
        Notice::Final {
            watch_ms,
            round,
            cancel_all_calls,
            outcome,
        } => format!(
            "워처 킬 정리 완료: 미체결 0건 확인 뒤 {}초 동안 늦은 접수 없음({round}회차, 일괄 취소 {cancel_all_calls}회). {}",
            seconds(*watch_ms),
            outcome_line(*outcome)
        ),
        Notice::Abort {
            why,
            round,
            remaining,
            cancel_all_calls,
        } => {
            let why = match why {
                AbortWhy::Shutdown => "워처 종료".to_string(),
                AbortWhy::StartRefused(r) => format!("워처 시작 거부: {}", cut(r, TEXT_MAX)),
            };
            let left = match remaining {
                Some(n) => format!("{n}건"),
                None => "모름".to_string(),
            };
            format!(
                "{why}로 워처 킬 중단({round}회차, 남은 미체결 {left}, 일괄 취소 {cancel_all_calls}회). OMS의 래치·마커는 그대로.\n{NOT_READ_ON_ABORT}"
            )
        }
    }
}

/// The positions behind a notice's block.
#[derive(Debug, Clone, PartialEq)]
pub enum Positions {
    /// The rows of every page, as the venue gave them.
    Rows(Vec<Value>),
    /// The read was sent and failed: the client class, or `시간 초과`.
    Failed(String),
    /// Not asked: the client is latched or paused (why).
    NotRead(String),
}

/// The class of a read past its deadline.
pub const TIMED_OUT: &str = "시간 초과";

/// A decimal string of a row, if it reads.
fn dec(row: &Value, field: &str) -> Option<Dec> {
    row.get(field)
        .and_then(Value::as_str)
        .and_then(|s| Dec::parse(s).ok())
}

/// A venue symbol as it goes into a line: at most 32 of `[A-Z0-9]`,
/// anything else as `?`.
fn symbol(row: &Value) -> String {
    match row.get("symbol").and_then(Value::as_str) {
        Some(s) if !s.is_empty() => s
            .chars()
            .take(32)
            .map(|c| {
                if c.is_ascii_uppercase() || c.is_ascii_digit() {
                    c
                } else {
                    '?'
                }
            })
            .collect(),
        _ => "?".to_string(),
    }
}

/// `d = |mark − liq| / mark × 100`, rounded down to 0.01 %, or why it is
/// unknown.
fn liquidation_distance(row: &Value) -> String {
    let liq = row
        .get("liqPrice")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .and_then(|s| Dec::parse(s).ok())
        .filter(|l| l.0 > 0);
    let Some(liq) = liq else {
        return "모름(거래소가 청산가를 주지 않음)".to_string();
    };
    let Some(mark) = dec(row, "markPrice").filter(|m| m.0 > 0) else {
        return "모름(마크 가격 없음)".to_string();
    };
    let gap = mark.checked_sub(liq).ok().and_then(|g| {
        if g.0 < 0 {
            g.checked_neg().ok()
        } else {
            Some(g)
        }
    });
    match gap.and_then(|g| Dec::percent_floor(g, mark)) {
        Some(d) => format!("{d} %"),
        None => "모름(계산 넘침)".to_string(),
    }
}

/// One line of the block, or `None` for a row left out (`size` zero). A
/// size that does not read is a position of unknown size, `수량 모름`, never
/// left out (review 1 S3).
fn row_line(row: &Value) -> Option<String> {
    let size = match dec(row, "size") {
        Some(s) if s.is_zero() => return None,
        Some(s) => s.to_string(),
        None => "수량 모름".to_string(),
    };
    let side = match row.get("side").and_then(Value::as_str) {
        Some("Buy") => "매수",
        Some("Sell") => "매도",
        _ => "방향 모름",
    };
    let shown = |field: &str| match dec(row, field) {
        Some(d) => d.to_string(),
        None => "모름".to_string(),
    };
    Some(format!(
        "- {} {side} {size}, 미실현손익 {}, 마크 {}, 마크에서 강제청산가까지 {}",
        symbol(row),
        shown("unrealisedPnl"),
        shown("markPrice"),
        liquidation_distance(row)
    ))
}

/// The position block of a notice (`kst`: the time of the read). Rows of
/// size 0 are left out, a row of unreadable size is shown as `수량 모름`; at most [`ROWS_SHOWN`] rows,
/// then the count of the rest.
pub fn position_block(p: &Positions, kst: &str) -> String {
    let rows = match p {
        Positions::Rows(rows) => rows,
        Positions::Failed(class) => {
            return format!(
                "포지션 조회 실패({}). 수량·손익·마크·강제청산가 모름",
                cut(class, TEXT_MAX)
            );
        }
        Positions::NotRead(why) => {
            return format!("포지션 조회 안 함({}). 수량·손익 모름", cut(why, TEXT_MAX));
        }
    };
    let lines: Vec<String> = rows.iter().filter_map(row_line).collect();
    let mut text =
        format!("포지션(거래소 {POSITIONS}, {kst}, 강제청산가는 교차 증거금이면 거래소 추정):");
    if lines.is_empty() {
        text.push_str("\n포지션 없음");
        return text;
    }
    for l in lines.iter().take(ROWS_SHOWN) {
        text.push('\n');
        text.push_str(l);
    }
    if lines.len() > ROWS_SHOWN {
        text.push_str(&format!("\n그 밖에 {}종목", lines.len() - ROWS_SHOWN));
    }
    text
}

/// Why the positions are not asked for at `now_ms`, if the client would
/// send nothing: the key latch or the 403 pause.
fn not_read(client: &Client, now_ms: u64) -> Option<String> {
    if client.auth_stop().is_some() {
        return Some("워처 키가 멈춤".to_string());
    }
    match client.paused_until() {
        Some(until) if now_ms < until => Some("HTTP 403 뒤 요청 일시 정지".to_string()),
        _ => None,
    }
}

/// Reads the linear USDT positions for a notice, every page by W1's paging
/// rule up to [`POSITION_MAX_PAGES`]. The read keeps its deadline on
/// `clock`, `request_timeout_ms` + [`DEADLINE_EXTRA_MS`] from its start: a
/// page answered at or after it ends the read with [`TIMED_OUT`] and no
/// further page is sent. A latched or paused client is not asked.
pub async fn read_positions(
    client: &Client,
    clock: &dyn Clock,
    request_timeout_ms: u64,
) -> Positions {
    let start = clock.now_ms();
    if let Some(why) = not_read(client, start) {
        return Positions::NotRead(why);
    }
    let deadline = start
        .saturating_add(request_timeout_ms)
        .saturating_add(DEADLINE_EXTRA_MS);
    let unreadable =
        |why: &str| Positions::Failed(client.unreadable(POSITIONS, why).class().into());
    let mut rows = Vec::new();
    let mut cursor = String::new();
    for _ in 0..POSITION_MAX_PAGES {
        let mut params = vec![
            ("category", "linear".to_string()),
            ("settleCoin", "USDT".to_string()),
            ("limit", POSITION_PAGE_LIMIT.to_string()),
        ];
        if !cursor.is_empty() {
            params.push(("cursor", cursor.clone()));
        }
        let answer = client.get(POSITIONS, &params).await;
        if clock.now_ms() >= deadline {
            return Positions::Failed(TIMED_OUT.to_string());
        }
        let v = match answer {
            Ok(v) => v,
            Err(e) => return Positions::Failed(class(&e)),
        };
        let Some(list) = v.get("list").and_then(Value::as_array) else {
            return unreadable("result.list is not a list");
        };
        let Some(next) = v.get("nextPageCursor").and_then(Value::as_str) else {
            return unreadable("nextPageCursor is not a string");
        };
        rows.extend(list.iter().cloned());
        if next.is_empty() {
            if list.len() >= POSITION_PAGE_LIMIT {
                return unreadable("a full page without nextPageCursor");
            }
            return Positions::Rows(rows);
        }
        if next == cursor {
            return unreadable("nextPageCursor is the cursor just sent");
        }
        cursor = next.to_string();
    }
    unreadable(&format!(
        "more than {} positions",
        POSITION_MAX_PAGES * POSITION_PAGE_LIMIT
    ))
}

fn class(e: &RestError) -> String {
    e.class().to_string()
}

// ---- The loop (U19b2b) ----

/// What one cancel-all of the loop came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CancelAnswer {
    /// Sent and accepted.
    Done(CancelAll),
    /// Sent, and no usable answer or refused.
    Failed(String),
    /// Nothing left the host.
    NotSent(NotSent),
}

/// Where the loop is (table 7-8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Not started.
    Idle,
    /// C2: the round's cancel-all is due or open.
    Cancel,
    /// C3: the count after a sent cancel-all is open.
    Count,
    /// C4: waits for the next round.
    Repeat,
    /// C5: zero was counted; the watch counts again until its end.
    Watch,
    /// Zero confirmed; the final notice went out.
    Ended,
    /// Aborted; the abort notice went out.
    Aborted,
}

/// What the loop reports when it ends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub rounds: u64,
    pub cancel_all_calls: u64,
    /// The start of the watch that ended the loop (`z`).
    pub zero_since: Option<u64>,
    pub aborted: bool,
}

/// What the runtime must do next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Send one cancel-all; answer with `cancel_done(req_id)`.
    CancelAll { req_id: u64 },
    /// Count the open orders; tell each page with `count_page(req_id)`
    /// and the result with `count_done(req_id)`.
    Count { req_id: u64 },
    /// Send a notice (with the position block, U19b3c).
    Notice(Notice),
    /// The loop is over.
    Ended(Report),
}

/// The open-order count before the first count of the loop.
pub const NOT_COUNTED: &str = "아직 세지 않음";

/// The one request of the loop that is open.
#[derive(Debug, Clone, Copy)]
struct Open {
    id: u64,
    /// A count, and whether it is the count that ends the watch.
    count: Option<bool>,
}

/// The watcher's cancel-all confirmation loop (C-WATCH part 11), sans-IO:
/// every input takes `now` on the loop's clock and returns the actions due;
/// `next_wake` names the instant it next wants a `poll`.
#[derive(Debug, Clone)]
pub struct KillLoop {
    t: Kill,
    phase: Phase,
    header: String,
    outcome: StopOutcome,
    round: u64,
    /// `t_r`: when the current round started.
    round_start: u64,
    /// `z`: the start of the current watch.
    zero_since: Option<u64>,
    /// The instant the next watch count is due.
    watch_due: u64,
    open: Option<Open>,
    next_id: u64,
    calls: u64,
    remaining: Result<usize, String>,
    last_error: Option<LastError>,
}

impl KillLoop {
    /// The config ranges are checked at config load only; the loop takes
    /// the timings as they are.
    pub fn new(timing: Kill, _request_timeout_ms: u64) -> KillLoop {
        KillLoop {
            t: timing,
            phase: Phase::Idle,
            header: String::new(),
            outcome: StopOutcome::Unconfirmed,
            round: 0,
            round_start: 0,
            zero_since: None,
            watch_due: 0,
            open: None,
            next_id: 1,
            calls: 0,
            remaining: Err(NOT_COUNTED.to_string()),
            last_error: None,
        }
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// The current or last round (0 before the start).
    pub fn round(&self) -> u64 {
        self.round
    }

    /// The last count: known, or why not.
    pub fn remaining(&self) -> &Result<usize, String> {
        &self.remaining
    }

    /// Cancel-alls sent (answered `Done` or `Failed`).
    pub fn cancel_all_calls(&self) -> u64 {
        self.calls
    }

    /// The start notice's header (the kill sequence's, U19b3b).
    pub fn header(&self) -> &str {
        &self.header
    }

    /// The instant the loop next wants a `poll`; `None` before the start
    /// and after the end.
    pub fn next_wake(&self) -> Option<u64> {
        if !self.running() {
            return None;
        }
        let due = match (self.phase, self.open) {
            (Phase::Repeat, _) => Some(self.round_start.saturating_add(self.t.round_ms)),
            (Phase::Watch, None) => Some(self.watch_due),
            _ => None,
        };
        due
    }

    fn running(&self) -> bool {
        !matches!(self.phase, Phase::Idle | Phase::Ended | Phase::Aborted)
    }

    fn id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// C1 → C2: the start notice and round 1.
    pub fn start(&mut self, now: u64, header: &str, outcome: StopOutcome) -> Vec<Action> {
        if self.phase != Phase::Idle {
            return Vec::new();
        }
        self.header = header.to_string();
        self.outcome = outcome;
        self.phase = Phase::Cancel;
        self.round = 1;
        self.round_start = now;
        let mut out = vec![Action::Notice(Notice::Start {
            round_ms: self.t.round_ms,
        })];
        self.advance(now, &mut out);
        out
    }

    /// The open request, if `req_id` is it and of that kind; else the
    /// answer is ignored.
    fn take(&mut self, req_id: u64, count: bool) -> Option<Open> {
        match self.open {
            Some(o) if o.id == req_id && o.count.is_some() == count => self.open.take(),
            _ => None,
        }
    }

    /// The answer of a cancel-all (C2).
    pub fn cancel_done(&mut self, now: u64, req_id: u64, a: CancelAnswer) -> Vec<Action> {
        if !self.running() || self.take(req_id, false).is_none() {
            return Vec::new();
        }
        match a {
            CancelAnswer::Done(_) => {
                self.calls += 1;
                self.last_error = None;
                self.phase = Phase::Count;
            }
            CancelAnswer::Failed(why) => {
                self.calls += 1;
                self.last_error = Some(LastError::Failed(why));
                self.phase = Phase::Count;
            }
            CancelAnswer::NotSent(k) => {
                self.last_error = Some(LastError::NotSent(k));
                self.phase = Phase::Repeat;
            }
        }
        let mut out = Vec::new();
        self.advance(now, &mut out);
        out
    }

    /// The result of a count (C3, or a count of the watch).
    pub fn count_done(&mut self, now: u64, req_id: u64, r: Result<usize, String>) -> Vec<Action> {
        if !self.running() {
            return Vec::new();
        }
        let Some(o) = self.take(req_id, true) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        self.counted(now, o.count == Some(true), r, &mut out);
        self.advance(now, &mut out);
        out
    }

    fn counted(
        &mut self,
        now: u64,
        ends_watch: bool,
        r: Result<usize, String>,
        out: &mut Vec<Action>,
    ) {
        self.remaining = r.clone();
        match (self.phase, r) {
            (Phase::Count, Ok(0)) => {
                self.phase = Phase::Watch;
                self.zero_since = Some(now);
                self.watch_due = now
                    .saturating_add(self.t.watch_every_ms)
                    .min(now.saturating_add(self.t.watch_ms));
            }
            (Phase::Watch, Ok(0)) if ends_watch => {
                self.phase = Phase::Ended;
                out.push(Action::Notice(Notice::Final {
                    watch_ms: self.t.watch_ms,
                    round: self.round,
                    cancel_all_calls: self.calls,
                    outcome: self.outcome,
                }));
                out.push(Action::Ended(Report {
                    rounds: self.round,
                    cancel_all_calls: self.calls,
                    zero_since: self.zero_since,
                    aborted: false,
                }));
            }
            (Phase::Watch, Ok(0)) => {}
            (Phase::Watch, Ok(_)) => {
                // A late order: back to C2 at once.
                self.zero_since = None;
                self.round += 1;
                self.round_start = now;
                self.phase = Phase::Cancel;
            }
            _ => {
                self.zero_since = None;
                self.phase = Phase::Repeat;
            }
        }
    }

    /// Emits the request that is due at `now`, if any.
    fn advance(&mut self, now: u64, out: &mut Vec<Action>) {
        if self.phase == Phase::Repeat && now >= self.round_start.saturating_add(self.t.round_ms) {
            self.round += 1;
            self.round_start = now;
            self.phase = Phase::Cancel;
        }
        if self.open.is_some() {
            return;
        }
        match self.phase {
            Phase::Cancel => {
                let id = self.id();
                self.open = Some(Open { id, count: None });
                out.push(Action::CancelAll { req_id: id });
            }
            Phase::Count => {
                let id = self.id();
                self.open = Some(Open {
                    id,
                    count: Some(false),
                });
                out.push(Action::Count { req_id: id });
            }
            Phase::Watch if now >= self.watch_due => {
                let z = self.zero_since.unwrap_or(now);
                let end = z.saturating_add(self.t.watch_ms);
                let ends = now >= end;
                if !ends {
                    let every = self.t.watch_every_ms.max(1);
                    let k = (now - z) / every + 1;
                    self.watch_due = z.saturating_add(k.saturating_mul(every)).min(end);
                }
                let id = self.id();
                self.open = Some(Open {
                    id,
                    count: Some(ends),
                });
                out.push(Action::Count { req_id: id });
            }
            _ => {}
        }
    }

    /// The loop's clock moved to `now`.
    pub fn poll(&mut self, now: u64) -> Vec<Action> {
        let mut out = Vec::new();
        if !self.running() {
            return out;
        }
        self.advance(now, &mut out);
        out
    }

    /// Shutdown or a start refusal during the loop.
    pub fn abort(&mut self, _now: u64, _why: AbortWhy) -> Vec<Action> {
        Vec::new()
    }
}
