//! W2 positions (C-WATCH part 6, U19a6): the venue's linear USDT positions
//! (`/v5/position/list`, gateway facts Q12) against the journal position
//! per instrument = Σ booked fills + external fills + venue changes +
//! adjustments (C-TAIL; parked fills are not counted).
//!
//! Per symbol in either set `d = venue − journal`; `d ≠ 0` is a difference
//! `(W2, position, symbol, d)`. Its age is judged from two times, the
//! journal's last change of the symbol and the venue's latest change: a
//! known time younger than `min_age_ms` defers the round, even at the
//! first sighting; a time that is not known is old.
//!
//! The venue's latest change is the row's `updatedTime`, else `seen_at`:
//! the first round that saw the symbol's current size. The list holds
//! only rows with size > 0, so a position that went flat has no row and
//! no `updatedTime`, exactly when it just closed; `seen_at` gives it a
//! time. The sizes of the first complete read have no `seen_at` (the state
//! found at start, not a change seen).

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use crate::decimal::Dec;
use crate::engine::{Check, Diff, Logic, Page, Since, VenueRequest};
use crate::state::JournalState;

pub const PATH: &str = "/v5/position/list";
pub const PAGE_LIMIT: usize = 200;
/// At most this many pages per round (판단(모델 아님)).
pub const MAX_PAGES: usize = 5;

/// One symbol of the venue read: the signed size and `updatedTime` (ms).
struct Row {
    size: Dec,
    updated: Option<u64>,
}

/// The read of one round, and what the rounds saw.
#[derive(Default)]
pub struct W2 {
    rows: BTreeMap<String, Row>,
    pages: usize,
    /// The cursor of the last request ("" for the first page).
    cursor: String,
    /// Symbol → the size at the last complete read (an absent row is 0)
    /// and `seen_at`, the time of the first read that saw it.
    seen: BTreeMap<String, (Dec, Option<u64>)>,
    /// A complete read happened.
    primed: bool,
    /// The clock at the last page of a complete read whose sizes are not
    /// yet in `seen`.
    unseen: Option<u64>,
}

fn request(cursor: &str) -> VenueRequest {
    let mut params = vec![
        ("category", "linear".to_string()),
        ("settleCoin", "USDT".to_string()),
        ("limit", PAGE_LIMIT.to_string()),
    ];
    if !cursor.is_empty() {
        params.push(("cursor", cursor.to_string()));
    }
    VenueRequest { path: PATH, params }
}

fn text<'a>(row: &'a Value, field: &str) -> Result<&'a str, String> {
    row.get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("a row without {field}"))
}

/// The signed size of a row: `Buy` +, `Sell` −, `""` or no side only with
/// size 0. A size is never negative, `side` gives the sign (the OMS's
/// own rule, `query.rs` `signed_size`): a size without a side is not read
/// as no position.
fn signed(row: &Value) -> Result<Dec, String> {
    let s = text(row, "size")?;
    let size = Dec::parse(s).map_err(|_| format!("size {s:?} is not a decimal"))?;
    if s.starts_with('-') {
        return Err(format!("size {s:?} is negative"));
    }
    match row.get("side").and_then(Value::as_str) {
        Some("Buy") => Ok(size),
        Some("Sell") => size.checked_neg().map_err(|e| e.to_string()),
        Some("") | None if size.is_zero() => Ok(Dec::ZERO),
        Some("") | None => Err(format!("size {s:?} without a side")),
        Some(other) => Err(format!("side {other:?} is not Buy, Sell or \"\"")),
    }
}

impl W2 {
    /// Takes the complete read into `seen` at `t`.
    fn observe(&mut self, t: u64) {
        self.unseen = None;
        let at = self.primed.then_some(t);
        self.primed = true;
        let symbols: BTreeSet<String> = self.seen.keys().chain(self.rows.keys()).cloned().collect();
        for symbol in symbols {
            let size = self.rows.get(&symbol).map_or(Dec::ZERO, |r| r.size);
            let e = self.seen.entry(symbol).or_insert((Dec::ZERO, None));
            if e.0 != size {
                *e = (size, at);
            }
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
        for row in list {
            let symbol = text(row, "symbol")?.to_string();
            let r = Row {
                size: signed(row)?,
                updated: row
                    .get("updatedTime")
                    .and_then(Value::as_str)
                    .and_then(|t| t.parse().ok()),
            };
            // The OMS refuses hedge mode: one row per symbol.
            if self.rows.insert(symbol.clone(), r).is_some() {
                return Err(format!("two rows for {symbol} (hedge mode)"));
            }
        }
        // W1's paging rule (the OMS's own).
        if next.is_empty() {
            if list.len() >= PAGE_LIMIT {
                return Err("a full page without nextPageCursor".into());
            }
            return Ok(Page::Done);
        }
        if next == self.cursor {
            return Err("nextPageCursor is the cursor just sent".into());
        }
        if self.pages >= MAX_PAGES {
            return Ok(Page::Fail(format!(
                "more than {} positions",
                MAX_PAGES * PAGE_LIMIT
            )));
        }
        self.cursor = next.to_string();
        Ok(Page::Next(request(next)))
    }
}

impl Logic for W2 {
    fn check(&self) -> Check {
        Check::W2
    }

    fn first(&mut self, _now_ms: u64, _state: &JournalState) -> VenueRequest {
        // The last round read every page and failed before its comparison
        // (status, journal): its sizes count, at its last venue answer.
        if let Some(t) = self.unseen {
            self.observe(t);
        }
        self.rows.clear();
        self.pages = 0;
        self.cursor.clear();
        request("")
    }

    fn page(&mut self, now_ms: u64, result: &Value) -> Page {
        self.pages += 1;
        let page = self.read(result).unwrap_or_else(Page::Unreadable);
        if page == Page::Done {
            self.unseen = Some(now_ms);
        }
        page
    }

    fn young_defers_at_first_sighting(&self) -> bool {
        true
    }

    fn compare(&mut self, now_ms: u64, state: &JournalState) -> Vec<Diff> {
        if self.unseen.is_some() {
            self.observe(now_ms);
        }
        let symbols: BTreeSet<&String> = self.rows.keys().chain(state.positions.keys()).collect();
        let mut out = Vec::new();
        for symbol in symbols {
            let row = self.rows.get(symbol);
            let venue = row.map_or(Dec::ZERO, |r| r.size);
            let journal = state.position(symbol);
            // A difference out of the decimal's range is still one, with
            // a fixed value (never read as agreement).
            let d = match venue.checked_sub(journal) {
                Ok(d) if d.is_zero() => continue,
                Ok(d) => d.to_string(),
                Err(_) => "overflow".to_string(),
            };
            // µs rounded up: an age is never taken as older than it is.
            let journal_at = state.last_change_at.get(symbol).map(|t| t.div_ceil(1000));
            let seen_at = self.seen.get(symbol).and_then(|s| s.1);
            let venue_at = row.and_then(|r| r.updated).or(seen_at);
            let since = match journal_at.max(venue_at) {
                Some(t) => Since::At(t),
                None => Since::Unknown,
            };
            out.push(Diff {
                kind: "position",
                subject: symbol.clone(),
                value: d,
                what: format!("venue {venue}, journal {journal}"),
                since,
            });
        }
        out
    }
}
