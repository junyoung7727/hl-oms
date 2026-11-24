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

use std::sync::Arc;

use serde_json::Value;
use tokio::sync::mpsc;

use crate::clock::Clock;
use crate::config::Kill;
use crate::decimal::Dec;
use crate::rest::{CancelAll, Client, RestError};
use crate::w1;

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

// ---- The count reader (U19b2c) ----

/// The open orders the loop counts (C3): W1's list, linear USDT.
pub const ORDERS: &str = w1::PATH;

/// Counts the open linear USDT orders for the loop (C3, C5): every page of
/// [`ORDERS`] by W1's paging rule, at most [`w1::MAX_PAGES`] (more is the
/// count error "more than 1000 open orders"); a row is open when its
/// `orderStatus` is one of [`w1::OPEN`], whoever placed it (a foreign or
/// empty `orderLinkId` counts: zero open orders is the goal). A row
/// without `orderStatus`, or a page that does not read, is a count error,
/// never a number.
///
/// The count keeps its own per-page deadline on `clock`, as the loop does:
/// its start, then each page's answer, + `request_timeout_ms` +
/// [`DEADLINE_EXTRA_MS`]. After a page that answered at or after it, the
/// count sends no further page and ends with [`PAST_DEADLINE`]: the task
/// asks for the next page at once, with no await the runtime could abort
/// it at (review 3 S2). `on_page` is called for each page request that
/// returned before its deadline (also one the client refused to send: the
/// result that follows is then the error).
pub async fn count_open(
    client: &Client,
    clock: &dyn Clock,
    request_timeout_ms: u64,
    on_page: &mut (dyn FnMut() + Send),
) -> Result<usize, String> {
    let extra = request_timeout_ms.saturating_add(DEADLINE_EXTRA_MS);
    let mut deadline = clock.now_ms().saturating_add(extra);
    let unreadable = |why: &str| client.unreadable(ORDERS, why).to_string();
    let mut open = 0;
    let mut cursor = String::new();
    for _ in 0..w1::MAX_PAGES {
        let mut params = vec![
            ("category", "linear".to_string()),
            ("settleCoin", "USDT".to_string()),
            ("limit", w1::PAGE_LIMIT.to_string()),
        ];
        if !cursor.is_empty() {
            params.push(("cursor", cursor.clone()));
        }
        let answer = client.get(ORDERS, &params).await;
        let now = clock.now_ms();
        if now >= deadline {
            return Err(PAST_DEADLINE.to_string());
        }
        deadline = now.saturating_add(extra);
        on_page();
        let v = answer.map_err(|e| e.to_string())?;
        let Some(list) = v.get("list").and_then(Value::as_array) else {
            return Err(unreadable("result.list is not a list"));
        };
        let Some(next) = v.get("nextPageCursor").and_then(Value::as_str) else {
            return Err(unreadable("nextPageCursor is not a string"));
        };
        for row in list {
            let Some(status) = row.get("orderStatus").and_then(Value::as_str) else {
                return Err(unreadable("a row without orderStatus"));
            };
            if w1::OPEN.contains(&status) {
                open += 1;
            }
        }
        if next.is_empty() {
            if list.len() >= w1::PAGE_LIMIT {
                return Err(unreadable("a full page without nextPageCursor"));
            }
            return Ok(open);
        }
        if next == cursor {
            return Err(unreadable("nextPageCursor is the cursor just sent"));
        }
        cursor = next.to_string();
    }
    Err(format!(
        "more than {} open orders",
        w1::MAX_PAGES * w1::PAGE_LIMIT
    ))
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
/// The order budget: at most this many cancel-alls in any half-open
/// window of [`ORDER_WINDOW_MS`], counted from emission (table 7-8 C4).
pub const ORDER_BUDGET: usize = 2;
pub const ORDER_WINDOW_MS: u64 = 1000;
/// Why a request past its deadline failed (cancel-all `Failed`, count
/// error).
pub const PAST_DEADLINE: &str = "no answer before the deadline";

/// The one request of the loop that is open.
#[derive(Debug, Clone, Copy)]
struct Open {
    id: u64,
    /// A count, and whether it is the count that ends the watch.
    count: Option<bool>,
    /// Emission (a count: its last page) + `request_timeout_ms` +
    /// [`DEADLINE_EXTRA_MS`]; an answer handled at or after it is past it.
    deadline: u64,
}

/// The watcher's cancel-all confirmation loop (C-WATCH part 11), sans-IO:
/// every input takes `now` on the loop's clock and returns the actions due;
/// `next_wake` names the instant it next wants a `poll`.
#[derive(Debug, Clone)]
pub struct KillLoop {
    t: Kill,
    /// `request_timeout_ms` + [`DEADLINE_EXTRA_MS`].
    extra_ms: u64,
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
    /// The next progress mark: `s + k × notice_ms`, `s` the start notice.
    next_mark: u64,
    ignored: u64,
    /// The emission instants of the last [`ORDER_BUDGET`] cancel-alls,
    /// oldest first.
    sent_at: Vec<u64>,
    /// The due cancel-all is held back by the budget.
    holding: bool,
    holds: u64,
}

impl KillLoop {
    /// The config ranges are checked at config load only; the loop takes
    /// the timings as they are.
    pub fn new(timing: Kill, request_timeout_ms: u64) -> KillLoop {
        KillLoop {
            t: timing,
            extra_ms: request_timeout_ms.saturating_add(DEADLINE_EXTRA_MS),
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
            next_mark: 0,
            ignored: 0,
            sent_at: Vec::with_capacity(ORDER_BUDGET),
            holding: false,
            holds: 0,
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

    /// Answers and pages ignored: past their deadline, or of no open
    /// request (the runtime writes one debug line each, U19b3c).
    pub fn ignored(&self) -> u64 {
        self.ignored
    }

    /// Cancel-alls the order budget held back (each counted once).
    pub fn budget_holds(&self) -> u64 {
        self.holds
    }

    /// The first instant the budget admits one more cancel-all: the
    /// oldest of the last [`ORDER_BUDGET`] emissions + [`ORDER_WINDOW_MS`].
    fn budget_at(&self) -> Option<u64> {
        if self.sent_at.len() < ORDER_BUDGET {
            return None;
        }
        self.sent_at
            .first()
            .map(|t| t.saturating_add(ORDER_WINDOW_MS))
    }

    /// The stop outcome the final notice will name.
    pub fn outcome(&self) -> StopOutcome {
        self.outcome
    }

    /// A halt retried during the loop latched (U19b3b): the final notice
    /// names the new outcome.
    pub fn set_outcome(&mut self, outcome: StopOutcome) {
        self.outcome = outcome;
    }

    /// The start notice's header (the kill sequence's, U19b3b).
    pub fn header(&self) -> &str {
        &self.header
    }

    /// The open request and its deadline in force, `(req_id, deadline)`:
    /// what [`Inflight::abort_due`] aborts by (U19b2c).
    pub fn open_request(&self) -> Option<(u64, u64)> {
        self.open.map(|o| (o.id, o.deadline))
    }

    /// What a request task sent ([`Inflight::take`]), as the input it is.
    pub fn arrived(&mut self, now: u64, a: Arrived) -> Vec<Action> {
        match a {
            Arrived::Cancel { req_id, answer } => self.cancel_done(now, req_id, answer),
            Arrived::Page { req_id } => self.count_page(now, req_id),
            Arrived::Counted { req_id, result } => self.count_done(now, req_id, result),
        }
    }

    /// The instant the loop next wants a `poll`; `None` before the start
    /// and after the end.
    pub fn next_wake(&self) -> Option<u64> {
        if !self.running() {
            return None;
        }
        let due = match (self.phase, self.open) {
            (_, Some(o)) => Some(o.deadline),
            (Phase::Cancel, None) => self.budget_at(),
            (Phase::Repeat, None) => Some(self.round_start.saturating_add(self.t.round_ms)),
            (Phase::Watch, None) => Some(self.watch_due),
            _ => None,
        };
        Some(due.map_or(self.next_mark, |d| d.min(self.next_mark)))
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
        self.next_mark = now.saturating_add(self.t.notice_ms);
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
            _ => {
                self.ignored += 1;
                None
            }
        }
    }

    /// The answer of a cancel-all (C2).
    pub fn cancel_done(&mut self, now: u64, req_id: u64, a: CancelAnswer) -> Vec<Action> {
        if !self.running() {
            return Vec::new();
        }
        let Some(o) = self.take(req_id, false) else {
            return Vec::new();
        };
        let a = if now >= o.deadline {
            CancelAnswer::Failed(PAST_DEADLINE.to_string())
        } else {
            a
        };
        let mut out = Vec::new();
        self.cancelled(now, a, &mut out);
        out
    }

    /// C2 → C3 after every sent cancel-all, also a failed one (unlike
    /// the OMS: the count is the truth, and a refusal when nothing is open
    /// must not keep the loop from ending); C2 → C4 after one not sent.
    fn cancelled(&mut self, now: u64, a: CancelAnswer, out: &mut Vec<Action>) {
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
        self.advance(now, out);
    }

    /// One page of the open count answered: the count's deadline moves to
    /// `now` + `request_timeout_ms` + [`DEADLINE_EXTRA_MS`]. A page at or
    /// after the deadline in force fails the count.
    pub fn count_page(&mut self, now: u64, req_id: u64) -> Vec<Action> {
        let mut out = Vec::new();
        if !self.running() {
            return out;
        }
        match self.open.as_mut() {
            Some(o) if o.id == req_id && o.count.is_some() => {
                if now < o.deadline {
                    o.deadline = now.saturating_add(self.extra_ms);
                    return out;
                }
            }
            _ => {
                self.ignored += 1;
                return out;
            }
        }
        if let Some(o) = self.open.take() {
            self.counted(
                now,
                o.count == Some(true),
                Err(PAST_DEADLINE.into()),
                &mut out,
            );
            self.advance(now, &mut out);
        }
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
        let r = if now >= o.deadline {
            Err(PAST_DEADLINE.to_string())
        } else {
            r
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
            // Only the count emitted at or after z + watch_ms ends the
            // watch, however late it answers; a slow answer to an earlier
            // count is not it.
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
            (Phase::Count, Ok(0)) => {
                self.phase = Phase::Watch;
                self.zero_since = Some(now);
                self.watch_due = now
                    .saturating_add(self.t.watch_every_ms)
                    .min(now.saturating_add(self.t.watch_ms));
            }
            (Phase::Watch, Ok(0)) => {}
            (Phase::Watch, Ok(_)) => {
                // A late order: back to C2 at once.
                self.zero_since = None;
                self.round += 1;
                self.round_start = now;
                self.phase = Phase::Cancel;
            }
            // n > 0 after a cancel-all, or a count error (never zero):
            // C4, the next round at t_r + round_ms.
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
                if self.budget_at().is_some_and(|at| now < at) {
                    if !self.holding {
                        self.holding = true;
                        self.holds += 1;
                    }
                    return;
                }
                self.holding = false;
                if self.sent_at.len() == ORDER_BUDGET {
                    self.sent_at.remove(0);
                }
                self.sent_at.push(now);
                let id = self.id();
                self.open = Some(Open {
                    id,
                    count: None,
                    deadline: now.saturating_add(self.extra_ms),
                });
                out.push(Action::CancelAll { req_id: id });
            }
            Phase::Count => {
                let id = self.id();
                self.open = Some(Open {
                    id,
                    count: Some(false),
                    deadline: now.saturating_add(self.extra_ms),
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
                    deadline: now.saturating_add(self.extra_ms),
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
        // Past the deadline of the open request: it failed.
        if let Some(o) = self.open.filter(|o| now >= o.deadline) {
            self.open = None;
            match o.count {
                Some(ends) => self.counted(now, ends, Err(PAST_DEADLINE.into()), &mut out),
                None => self.cancelled(now, CancelAnswer::Failed(PAST_DEADLINE.into()), &mut out),
            }
        }
        self.advance(now, &mut out);
        self.progress(now, &mut out);
        out
    }

    /// The progress notice of the C4 rule: one at the first poll at or
    /// after a mark; the marks stay anchored to the start notice, and one
    /// notice covers every mark passed before it.
    fn progress(&mut self, now: u64, out: &mut Vec<Action>) {
        if now < self.next_mark {
            return;
        }
        let every = self.t.notice_ms.max(1);
        let passed = (now - self.next_mark) / every + 1;
        self.next_mark = self.next_mark.saturating_add(passed.saturating_mul(every));
        out.push(Action::Notice(Notice::Progress {
            round: self.round,
            remaining: self.remaining.clone(),
            cancel_all_calls: self.calls,
            last_error: self.last_error.clone(),
        }));
    }

    /// Shutdown or a start refusal during the loop (U19b3d): one abort
    /// notice and the end; nothing after it. Before the start
    /// it does nothing (the kill sequence tells an abort before the loop,
    /// U19b3b). The open request is dropped: its answer is never used.
    pub fn abort(&mut self, _now: u64, why: AbortWhy) -> Vec<Action> {
        if !self.running() {
            return Vec::new();
        }
        self.phase = Phase::Aborted;
        self.open = None;
        vec![
            Action::Notice(Notice::Abort {
                why,
                round: self.round,
                remaining: self.remaining.clone().ok(),
                cancel_all_calls: self.calls,
            }),
            Action::Ended(Report {
                rounds: self.round,
                cancel_all_calls: self.calls,
                zero_since: self.zero_since,
                aborted: true,
            }),
        ]
    }
}

// ---- Inflight (U19b2c) ----

/// What a request task of the loop sent ([`Inflight::take`]); the loop
/// takes it with [`KillLoop::arrived`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Arrived {
    /// The answer of a cancel-all.
    Cancel { req_id: u64, answer: CancelAnswer },
    /// One page of a count answered before the count's own deadline.
    Page { req_id: u64 },
    /// The result of a count.
    Counted {
        req_id: u64,
        result: Result<usize, String>,
    },
}

impl Arrived {
    fn req_id(&self) -> u64 {
        match self {
            Arrived::Cancel { req_id, .. }
            | Arrived::Page { req_id }
            | Arrived::Counted { req_id, .. } => *req_id,
        }
    }

    /// The task's last message: it has ended.
    fn last(&self) -> bool {
        !matches!(self, Arrived::Page { .. })
    }
}

/// What one cancel-all came to. Whether it left the host is told from the
/// client's state before the call: `cancel_all` gives the same error for a
/// request it did not send after a latch or in a 403 pause as for one that
/// got 10010 or 403 (C-WATCH part 11, U19b2a). Latched and paused send
/// nothing; a request that never connected is `Local`; every other error
/// was sent and is `Failed` (the loop counts after it).
pub async fn cancel_answer(client: &Client, clock: &dyn Clock) -> CancelAnswer {
    if client.auth_stop().is_some() {
        return CancelAnswer::NotSent(NotSent::Latched);
    }
    if client.paused_until().is_some_and(|u| clock.now_ms() < u) {
        return CancelAnswer::NotSent(NotSent::Paused);
    }
    match client.cancel_all().await {
        Ok(c) => CancelAnswer::Done(c),
        Err(RestError::NotSent(why)) => CancelAnswer::NotSent(NotSent::Local(why)),
        Err(e) => CancelAnswer::Failed(e.to_string()),
    }
}

struct Task {
    req_id: u64,
    handle: tokio::task::AbortHandle,
}

/// The request tasks of the loop (C-WATCH part 11, review 2 S2): each
/// `CancelAll` and `Count` the loop emits runs as one tokio task with the
/// real client; what it sends comes back through [`Inflight::take`]. The
/// deadline is the loop's ([`KillLoop::open_request`]): at every step,
/// before the loop's next input, [`Inflight::abort_due`] aborts the task
/// whose deadline passed and every task the loop no longer has open, and
/// drops whatever they sent. So an abandoned request sends nothing more,
/// never two requests of the loop run at once, and the request guard's 60
/// per 10 s is taken only by live requests. Used by the kill rig of the
/// tests and by the runtime (U19b3c).
pub struct Inflight {
    client: Arc<Client>,
    clock: Arc<dyn Clock>,
    request_timeout_ms: u64,
    tx: mpsc::UnboundedSender<Arrived>,
    rx: mpsc::UnboundedReceiver<Arrived>,
    tasks: Vec<Task>,
    aborted: u64,
    stray: u64,
}

impl Inflight {
    /// `clock` is the loop's; `request_timeout_ms` gives the count task
    /// its own per-page deadline ([`count_open`]).
    pub fn new(client: Arc<Client>, clock: Arc<dyn Clock>, request_timeout_ms: u64) -> Inflight {
        let (tx, rx) = mpsc::unbounded_channel();
        Inflight {
            client,
            clock,
            request_timeout_ms,
            tx,
            rx,
            tasks: Vec::new(),
            aborted: 0,
            stray: 0,
        }
    }

    /// Starts the task of a `CancelAll` or `Count` (any other action is no
    /// request: `false`). The loop opens a request only when it closed the
    /// last one, so a task still live was given up: it is aborted first.
    pub fn start(&mut self, a: &Action) -> bool {
        let (req_id, count) = match a {
            Action::CancelAll { req_id } => (*req_id, false),
            Action::Count { req_id } => (*req_id, true),
            _ => return false,
        };
        self.abort_where(|_| true);
        let (client, clock, tx) = (self.client.clone(), self.clock.clone(), self.tx.clone());
        let rt = self.request_timeout_ms;
        let handle = if count {
            tokio::spawn(async move {
                let pages = tx.clone();
                let mut on_page = move || {
                    let _ = pages.send(Arrived::Page { req_id });
                };
                let result = count_open(&client, &*clock, rt, &mut on_page).await;
                let _ = tx.send(Arrived::Counted { req_id, result });
            })
        } else {
            tokio::spawn(async move {
                let answer = cancel_answer(&client, &*clock).await;
                let _ = tx.send(Arrived::Cancel { req_id, answer });
            })
        };
        self.tasks.push(Task {
            req_id,
            handle: handle.abort_handle(),
        });
        true
    }

    /// Aborts the open request's task at or after its deadline and every
    /// task that is not the open request (`open`: the loop's
    /// [`KillLoop::open_request`]); call it at every step before the loop's
    /// next input. Returns how many it aborted.
    pub fn abort_due(&mut self, now: u64, open: Option<(u64, u64)>) -> usize {
        self.abort_where(|id| match open {
            Some((open_id, deadline)) if open_id == id => now >= deadline,
            _ => true,
        })
    }

    /// Aborts every task (the loop ended or was aborted).
    pub fn abort_all(&mut self) {
        self.abort_where(|_| true);
    }

    fn abort_where(&mut self, abort: impl Fn(u64) -> bool) -> usize {
        let before = self.tasks.len();
        self.tasks.retain(|t| {
            if abort(t.req_id) {
                t.handle.abort();
                false
            } else {
                true
            }
        });
        let n = before - self.tasks.len();
        self.aborted += n as u64;
        n
    }

    /// What the live tasks sent since the last call, in order. A task's
    /// answer or count result ends it. What an aborted task sent is dropped
    /// ([`Inflight::stray`]).
    pub fn take(&mut self) -> Vec<Arrived> {
        let mut out = Vec::new();
        while let Ok(a) = self.rx.try_recv() {
            let id = a.req_id();
            if !self.tasks.iter().any(|t| t.req_id == id) {
                self.stray += 1;
                continue;
            }
            if a.last() {
                self.tasks.retain(|t| t.req_id != id);
            }
            out.push(a);
        }
        out
    }

    /// Tasks started and neither ended nor aborted.
    pub fn open(&self) -> usize {
        self.tasks.len()
    }

    /// Messages sent and not yet taken.
    pub fn queued(&self) -> usize {
        self.rx.len()
    }

    /// Tasks aborted.
    pub fn aborted(&self) -> u64 {
        self.aborted
    }

    /// Messages of aborted tasks, dropped (the runtime writes one debug line
    /// each, U19b3c).
    pub fn stray(&self) -> u64 {
        self.stray
    }
}

impl Drop for Inflight {
    /// No request of the loop outlives it.
    fn drop(&mut self) {
        self.abort_all();
    }
}
