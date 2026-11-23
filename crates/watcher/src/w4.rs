//! W4 executions (C-WATCH part 8, U19a8): the venue's executions
//! (`/v5/execution/list`, gateway facts Q9) against the journal's fills,
//! venue changes and funding, by the key `<symbol>:<execId>`.
//!
//! A **sweep** reads the window `[start, end]`: `end` is the clock at its
//! first page and stays fixed, so the cursor's rows do not move under new
//! executions; `start = max(watermark − w4_overlap_ms, ⌈fill_floor / 1000⌉)`
//! (the floor is in µs, rounded up to ms), cut to 7 days. A round reads at
//! most `w4_max_pages` pages; a sweep with more continues at the next round
//! from its cursor ([`Page::Pause`]). The sweep ends at the first page that
//! is empty or has an empty `nextPageCursor` (also a full one: a known
//! limit), and only then are its rows compared.
//!
//! Rows are classified by `execType` first, and only the fields their type
//! is compared on are read. `Trade` is compared with the journal's fills,
//! `AdlTrade`/`BustTrade`/`Delivery`/`Settle` with its venue changes,
//! `Funding` with nothing; any other type is `unknown_exec_type`, confirmed
//! at once (at the completed sweep). A row that lacks a field its type
//! needs is `unreadable_row`.
//!
//! The watermark after a completed sweep is the newest `execTime` at or
//! below which every row agrees, is ignored or is confirmed; it never moves
//! back. So an unconfirmed difference stays in the next window and is read
//! again; a row without a readable `execTime` that is not confirmed holds
//! the watermark where it is.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

use serde_json::Value;

use crate::config::Rules;
use crate::decimal::Dec;
use crate::engine::{Check, Diff, Ident, Logic, Page, Since, VenueRequest};
use crate::log::Log;
use crate::state::{JournalState, Side};

pub const PATH: &str = "/v5/execution/list";
pub const PAGE_LIMIT: usize = 100;
/// The venue's longest window (facts Q9.7).
pub const WINDOW_MAX_MS: u64 = 7 * 86_400_000;
/// A sweep restarts from page 1 after this many failed rounds in a row
/// (판단(모델 아님)).
pub const RESTART_AFTER_FAILED: u32 = 3;
/// A sweep not complete after this many rounds restarts (판단(모델 아님)).
pub const RESTART_AFTER_ROUNDS: u32 = 50;
/// The subject of a row without a readable key: its JSON text cut to this
/// many bytes (판단(모델 아님)).
pub const SUBJECT_MAX: usize = 200;

/// Symbol → the newest `execTime` (ms) of an agreeing `Trade` or
/// venue-change row (U19a9 makes it W2's fallback for `updatedTime`).
pub type ExecTimes = Arc<Mutex<BTreeMap<String, u64>>>;

/// One sweep: its window, the cursor of the next page and the rows read.
struct Sweep {
    /// `max(watermark − overlap, floor)`, before the 7-day cut.
    from: u64,
    /// The clock at its first page; `None` until that page.
    end: Option<u64>,
    /// The cursor of the next page ("" for page 1).
    cursor: String,
    rows: Vec<Value>,
    /// The sweep read its last page.
    done: bool,
    /// Rounds that worked on it, and failed rounds in a row.
    rounds: u32,
    failed: u32,
}

pub struct W4 {
    overlap_ms: u64,
    max_pages: u64,
    lookback_ms: u64,
    log: Log,
    /// `None` until the first sweep.
    watermark: Option<u64>,
    sweep: Option<Sweep>,
    /// Pages of the current round.
    pages: u64,
    /// The window of the last sweep was cut to 7 days.
    cut: bool,
    /// The sweep restarted after failed rounds and no page has been read
    /// since (one warn line per run of failures).
    restarted: bool,
    /// Each row of the last comparison: its `execTime` and, for a
    /// difference, its identity.
    compared: Vec<(Option<u64>, Option<Ident>)>,
    times: ExecTimes,
}

fn request(start: u64, end: u64, cursor: &str) -> VenueRequest {
    let mut params = vec![
        ("category", "linear".to_string()),
        ("settleCoin", "USDT".to_string()),
        ("limit", PAGE_LIMIT.to_string()),
        ("startTime", start.to_string()),
        ("endTime", end.to_string()),
    ];
    if !cursor.is_empty() {
        params.push(("cursor", cursor.to_string()));
    }
    VenueRequest { path: PATH, params }
}

/// A non-empty string field.
fn text<'a>(row: &'a Value, field: &str) -> Option<&'a str> {
    row.get(field)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
}

fn side_name(s: Side) -> &'static str {
    match s {
        Side::Buy => "Buy",
        Side::Sell => "Sell",
    }
}

/// The journal's venue-change kind of a venue-change `execType`.
fn change_kind(exec_type: &str) -> Option<&'static str> {
    match exec_type {
        "AdlTrade" => Some("adl"),
        "BustTrade" => Some("takeover"),
        "Delivery" => Some("delivery"),
        "Settle" => Some("settle"),
        _ => None,
    }
}

impl W4 {
    pub fn new(rules: &Rules, log: Log) -> W4 {
        W4 {
            overlap_ms: rules.w4_overlap_ms,
            max_pages: rules.w4_max_pages,
            lookback_ms: rules.w4_initial_lookback_ms,
            log,
            watermark: None,
            sweep: None,
            pages: 0,
            cut: false,
            restarted: false,
            compared: Vec::new(),
            times: Arc::new(Mutex::new(BTreeMap::new())),
        }
    }

    /// The newest agreeing `execTime` per symbol, shared.
    pub fn exec_times(&self) -> ExecTimes {
        self.times.clone()
    }

    /// The window's start for `end`: cut to 7 days, one warn line per
    /// change of state; never after `end`.
    fn start(&mut self, from: u64, end: u64) -> u64 {
        let cut = end.saturating_sub(from) >= WINDOW_MAX_MS;
        if cut != self.cut {
            (self.log)(&if cut {
                format!(
                    "warn: W4 window from {from} to {end} ms is longer than 7 days; cut to the last 7 days"
                )
            } else {
                "W4 window is 7 days or shorter again".to_string()
            });
            self.cut = cut;
        }
        if cut {
            end - WINDOW_MAX_MS + 1
        } else {
            from.min(end)
        }
    }

    /// Back to page 1 with the same start; the end is the next first page.
    fn restart(&mut self) {
        if let Some(sw) = &mut self.sweep {
            sw.end = None;
            sw.cursor.clear();
            sw.rows.clear();
            sw.rounds = 0;
            sw.failed = 0;
        }
    }

    fn read(&mut self, result: &Value) -> Result<Page, String> {
        let list = result
            .get("list")
            .and_then(Value::as_array)
            .ok_or("result.list is not a list")?;
        let next = result
            .get("nextPageCursor")
            .and_then(Value::as_str)
            .ok_or("nextPageCursor is not a string")?;
        let Some(sw) = self.sweep.as_mut() else {
            return Ok(Page::Fail("a page without a sweep".into()));
        };
        let Some(end) = sw.end else {
            return Ok(Page::Fail("a page without a sweep".into()));
        };
        // The end of a sweep: an empty page, or an empty cursor (a full
        // page too, for W4 only: a known limit).
        if list.is_empty() || next.is_empty() {
            sw.rows.extend(list.iter().cloned());
            sw.done = true;
            return Ok(Page::Done);
        }
        if next == sw.cursor {
            return Err("nextPageCursor is the cursor just sent".into());
        }
        sw.rows.extend(list.iter().cloned());
        sw.cursor = next.to_string();
        if self.pages >= self.max_pages {
            sw.failed = 0;
            return Ok(Page::Pause);
        }
        let from = sw.from;
        let start = self.start(from, end);
        Ok(Page::Next(request(start, end, next)))
    }

    /// One row against the journal: `None` when it agrees or is ignored;
    /// `agreed` is set to its symbol for an agreeing `Trade` or venue
    /// change.
    fn row(
        row: &Value,
        state: &JournalState,
        end: u64,
        agreed: &mut Option<(String, u64)>,
    ) -> (Option<u64>, Option<Diff>) {
        let read_time = text(row, "execTime").and_then(|t| t.parse::<u64>().ok());
        // An `execTime` after the sweep's end is not trusted: as a
        // watermark it would move every later window past `end` (review 1
        // S1 of U19a8).
        let late = read_time.filter(|&t| t > end);
        let exec_time = read_time.filter(|&t| t <= end);
        // The OMS ignores a fill below the floor and the journal never
        // holds it.
        if exec_time
            .and_then(|t| t.checked_mul(1000))
            .is_some_and(|us| us < state.fill_floor)
        {
            return (exec_time, None);
        }
        let symbol = text(row, "symbol");
        let exec_id = text(row, "execId");
        let exec_type = text(row, "execType");
        let mut missing = Vec::new();
        for (name, v) in [
            ("execId", exec_id.is_some()),
            ("symbol", symbol.is_some()),
            ("execType", exec_type.is_some()),
            ("execTime", exec_time.is_some()),
        ] {
            if !v {
                missing.push(name);
            }
        }
        let since = exec_time.map_or(Since::Unknown, Since::At);
        let key = match (symbol, exec_id) {
            (Some(s), Some(e)) => format!("{s}:{e}"),
            _ => {
                let mut s = row.to_string();
                if s.len() > SUBJECT_MAX {
                    let mut n = SUBJECT_MAX;
                    while !s.is_char_boundary(n) {
                        n -= 1;
                    }
                    s.truncate(n);
                }
                s
            }
        };
        // Classify first, then read only what the type is compared on.
        let compared = exec_type.filter(|t| *t == "Trade" || change_kind(t).is_some());
        let mut qty_side = None;
        if compared.is_some() {
            let qty = text(row, "execQty")
                .filter(|q| !q.starts_with('-'))
                .and_then(|q| Dec::parse(q).ok());
            let side = match text(row, "side") {
                Some("Buy") => Some(Side::Buy),
                Some("Sell") => Some(Side::Sell),
                _ => None,
            };
            if qty.is_none() {
                missing.push("execQty");
            }
            if side.is_none() {
                missing.push("side");
            }
            qty_side = qty.zip(side);
        }
        if !missing.is_empty() {
            let names = missing.join(",");
            let what = match late {
                Some(t) => format!(
                    "venue row without {names} (execTime {t} after the sweep's end {end}), journal not compared"
                ),
                None => format!("venue row without {names}, journal not compared"),
            };
            let d = Diff {
                kind: "unreadable_row",
                subject: key,
                value: names,
                what,
                since,
            };
            return (exec_time, Some(d));
        }
        let (Some(exec_type), Some(t)) = (exec_type, exec_time) else {
            return (exec_time, None);
        };
        let diff = |kind, value: String, what: String| Diff {
            kind,
            subject: key.clone(),
            value,
            what,
            since,
        };
        let agreeing = || symbol.map(|s| (s.to_string(), t));
        let Some((qty, side)) = qty_side else {
            if exec_type == "Funding" {
                // Funding never changes a quantity: nothing is compared.
                return (exec_time, None);
            }
            let what = format!("venue execType {exec_type:?}, journal not compared");
            let d = diff("unknown_exec_type", exec_type.to_string(), what);
            return (exec_time, Some(d));
        };
        if qty.is_zero() {
            // The OMS books no fill and no venue change of quantity 0
            // (`decide.rs` `on_fill`, `on_venue_change`; review 1 S2).
            return (exec_time, None);
        }
        if let Some(kind) = change_kind(exec_type) {
            let signed = match side {
                Side::Buy => Ok(qty),
                Side::Sell => qty.checked_neg(),
            };
            let value = match signed {
                Ok(q) => format!("{kind} {q}"),
                Err(_) => format!("{kind} overflow"),
            };
            let d = match state.venue_changes.get(&key) {
                None => {
                    let what = format!("venue {exec_type} {value}, journal no venue change");
                    diff("missing_venue_change", value, what)
                }
                Some(c) if c.kind != kind || signed.as_ref().ok() != Some(&c.qty) => {
                    let what = format!("venue {value}, journal {} {}", c.kind, c.qty);
                    diff("venue_change_details", value, what)
                }
                Some(_) => {
                    *agreed = agreeing();
                    return (exec_time, None);
                }
            };
            return (exec_time, Some(d));
        }
        let value = format!("{} {qty}", side_name(side));
        let d = match state.fills.get(&key) {
            None => {
                let what = format!("venue Trade {value}, journal no fill");
                diff("missing_fill", value, what)
            }
            Some(f) if f.qty != qty || f.side != side => {
                let what = format!("venue {value}, journal {} {}", side_name(f.side), f.qty);
                diff("fill_details", value, what)
            }
            Some(_) => {
                *agreed = agreeing();
                return (exec_time, None);
            }
        };
        (exec_time, Some(d))
    }
}

impl Logic for W4 {
    fn check(&self) -> Check {
        Check::W4
    }

    fn first(&mut self, now_ms: u64, state: &JournalState) -> VenueRequest {
        self.pages = 0;
        let floor = state.fill_floor.div_ceil(1000);
        if self.sweep.as_ref().is_some_and(|s| s.done) {
            self.sweep = None;
        }
        if self
            .sweep
            .as_ref()
            .is_some_and(|s| s.rounds >= RESTART_AFTER_ROUNDS)
        {
            (self.log)(&format!(
                "error: W4 sweep not complete after {RESTART_AFTER_ROUNDS} rounds; it restarts from page 1"
            ));
            self.restart();
        }
        if self.sweep.is_none() {
            let lookback = self.lookback_ms;
            let wm = *self
                .watermark
                .get_or_insert(now_ms.saturating_sub(lookback).max(floor));
            self.sweep = Some(Sweep {
                from: wm.saturating_sub(self.overlap_ms).max(floor),
                end: None,
                cursor: String::new(),
                rows: Vec::new(),
                done: false,
                rounds: 0,
                failed: 0,
            });
        }
        let Some(sw) = self.sweep.as_mut() else {
            return request(0, now_ms, "");
        };
        sw.rounds += 1;
        let end = *sw.end.get_or_insert(now_ms);
        let (from, cursor) = (sw.from, sw.cursor.clone());
        let start = self.start(from, end);
        request(start, end, &cursor)
    }

    fn page(&mut self, _now_ms: u64, result: &Value) -> Page {
        self.pages += 1;
        let page = self.read(result).unwrap_or_else(Page::Unreadable);
        if self.restarted && matches!(page, Page::Next(_) | Page::Pause | Page::Done) {
            (self.log)("W4 pages are read again after a restart");
            self.restarted = false;
        }
        page
    }

    fn defers(&self) -> bool {
        false
    }

    fn at_once(&self, kind: &str) -> bool {
        kind == "unknown_exec_type"
    }

    fn failed(&mut self) {
        let Some(sw) = self.sweep.as_mut() else {
            return;
        };
        // A complete sweep that could not be compared is read again.
        if sw.done {
            self.sweep = None;
            return;
        }
        sw.failed += 1;
        if sw.failed >= RESTART_AFTER_FAILED {
            if !self.restarted {
                (self.log)(&format!(
                    "warn: W4 page failed {RESTART_AFTER_FAILED} rounds in a row; the sweep restarts from page 1 with a new end"
                ));
                self.restarted = true;
            }
            self.restart();
        }
    }

    fn compare(&mut self, now_ms: u64, state: &JournalState) -> Vec<Diff> {
        let (rows, end) = self
            .sweep
            .take()
            .map_or((Vec::new(), now_ms), |s| (s.rows, s.end.unwrap_or(now_ms)));
        self.compared.clear();
        let mut out = Vec::new();
        let mut times = self.times.lock().unwrap_or_else(|e| e.into_inner());
        for r in &rows {
            let mut agreed = None;
            let (t, d) = W4::row(r, state, end, &mut agreed);
            if let Some((symbol, t)) = agreed {
                let e = times.entry(symbol).or_insert(t);
                *e = (*e).max(t);
            }
            let id = d
                .as_ref()
                .map(|d| (d.kind, d.subject.clone(), d.value.clone()));
            self.compared.push((t, id));
            out.extend(d);
        }
        out
    }

    fn judged(&mut self, open: &BTreeSet<Ident>) {
        // Rows at or after the oldest open difference do not qualify; one
        // without a readable execTime is older than every row.
        let bound = self
            .compared
            .iter()
            .filter(|(_, id)| id.as_ref().is_some_and(|id| open.contains(id)))
            .map(|(t, _)| t.unwrap_or(0))
            .min()
            .unwrap_or(u64::MAX);
        let w = self
            .compared
            .iter()
            .filter_map(|(t, _)| t.filter(|&t| t < bound))
            .max();
        if let Some(w) = w {
            let wm = self.watermark.get_or_insert(w);
            *wm = (*wm).max(w);
        }
    }
}
