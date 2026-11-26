//! W1 open orders (C-WATCH part 5): the venue's open linear USDT orders
//! (`/v5/order/realtime`, gateway facts Q10) against the journal's open
//! set and order info. Quantity is not compared (amends and partial fills
//! change it; W2 and W4 cover fills). An open journal order that is not
//! acked, or in doubt, and not at the venue is not compared: it may be
//! unsent or its result unknown, and the OMS resolves it itself.
//!
//! | kind | condition | subject |
//! |---|---|---|
//! | `foreign` | open at the venue; `orderLinkId` is not ours or names an order the journal does not hold | venue `orderId` |
//! | `closed_in_journal` | open at the venue; closed in the journal | `orderLinkId` |
//! | `open_after_terminal` | open at the venue; `venue_terminal` and no `order_closed` in the journal | `orderLinkId` |
//! | `details` | open in both; symbol or side differ | `orderLinkId` |
//! | `missing_at_venue` | open, acked, not in doubt, not terminal in the journal; not open at the venue; the journal holds no kill latch | `orderLinkId` |
//!
//! The age of a known order is its `fact_at`; of a foreign one, the first
//! round that saw it.
//!
//! While the journal holds the kill latch (`hold_set{kill}`, ADR-040) an
//! order missing at the venue is not a difference: the kill cancels every
//! order at the venue, the latch is durable before its first cancel-all,
//! and the OMS journals the closes only when its reconcile runs (not while
//! its link is down). The other kinds are compared as always; a fill
//! behind a missing order is still seen by W2 and W4.

use std::collections::BTreeSet;

use serde_json::Value;

use crate::engine::{Check, Diff, Logic, Page, Since, VenueRequest};
use crate::state::{JournalState, Side};
use crate::wire_id;

pub const PATH: &str = "/v5/order/realtime";
pub const PAGE_LIMIT: usize = 50;
/// At most this many pages per round (판단(모델 아님), as the OMS).
pub const MAX_PAGES: usize = 20;
/// The `orderStatus` values of an open order.
pub const OPEN: [&str; 3] = ["New", "PartiallyFilled", "Untriggered"];

struct Row {
    link: String,
    order_id: String,
    symbol: String,
    side: String,
}

/// The read of one round.
#[derive(Default)]
pub struct W1 {
    rows: Vec<Row>,
    pages: usize,
    /// The cursor of the last request ("" for the first page).
    cursor: String,
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

fn text(row: &Value, field: &str) -> Result<String, String> {
    row.get(field)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| format!("a row without {field}"))
}

fn side_name(s: Side) -> &'static str {
    match s {
        Side::Buy => "Buy",
        Side::Sell => "Sell",
    }
}

impl W1 {
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
            let status = text(row, "orderStatus")?;
            let r = Row {
                link: text(row, "orderLinkId")?,
                order_id: text(row, "orderId")?,
                symbol: text(row, "symbol")?,
                side: text(row, "side")?,
            };
            text(row, "qty")?;
            if OPEN.contains(&status.as_str()) {
                self.rows.push(r);
            }
        }
        // The OMS's own paging rule: a page that is not full and has no
        // cursor is the last; a full page without one may not be.
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
                "more than {} open orders",
                MAX_PAGES * PAGE_LIMIT
            )));
        }
        self.cursor = next.to_string();
        Ok(Page::Next(request(next)))
    }
}

impl Logic for W1 {
    fn check(&self) -> Check {
        Check::W1
    }

    fn first(&mut self, _now_ms: u64, _state: &JournalState) -> VenueRequest {
        *self = W1::default();
        request("")
    }

    fn page(&mut self, _now_ms: u64, result: &Value) -> Page {
        self.pages += 1;
        self.read(result).unwrap_or_else(Page::Unreadable)
    }

    fn compare(&mut self, _now_ms: u64, state: &JournalState) -> Vec<Diff> {
        let mut out = Vec::new();
        let mut at_venue = BTreeSet::new();
        let mut diff = |kind, subject: &str, value: String, what: String, since| {
            out.push(Diff {
                kind,
                subject: subject.to_string(),
                value,
                what,
                since,
            })
        };
        for r in &self.rows {
            let ours = wire_id::decode(&r.link)
                .filter(|&(i, _)| Some(i) == state.instance_id)
                .and_then(|(_, id)| state.orders.get(&id).map(|o| (id, o)));
            let Some((id, o)) = ours else {
                let what = format!(
                    "venue {} {} orderLinkId {:?}, journal no such order",
                    r.symbol, r.side, r.link
                );
                diff(
                    "foreign",
                    &r.order_id,
                    r.link.clone(),
                    what,
                    Since::FirstSeen,
                );
                continue;
            };
            at_venue.insert(id);
            // µs rounded up: an age is never taken as older than it is.
            let since = Since::At(o.fact_at.div_ceil(1000));
            let journal = format!("{} {}", o.instrument, side_name(o.side));
            let venue = format!("{} {}", r.symbol, r.side);
            if !o.open {
                let what = "venue open, journal closed".to_string();
                diff("closed_in_journal", &r.link, String::new(), what, since);
            } else if o.terminal {
                let what = "venue open, journal ended at the venue (venue_terminal)".to_string();
                diff("open_after_terminal", &r.link, String::new(), what, since);
            } else if venue != journal {
                let what = format!("venue {venue}, journal {journal}");
                diff("details", &r.link, venue, what, since);
            }
        }
        for id in &state.open {
            if state.holds.kill {
                break;
            }
            let Some(o) = state.orders.get(id) else {
                continue;
            };
            if o.acked && !o.in_doubt && !o.terminal && !at_venue.contains(id) {
                let link = wire_id::encode(state.instance_id.unwrap_or(0), *id);
                let what = "venue not open, journal open and acked".to_string();
                let since = Since::At(o.fact_at.div_ceil(1000));
                diff("missing_at_venue", &link, String::new(), what, since);
            }
        }
        out
    }
}
