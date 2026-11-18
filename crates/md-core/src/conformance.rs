//! The conformance checks every adapter runs with its own fixture (design §6.4):
//! routing, snapshot and delta application, zero-quantity deletes, gap
//! detection with the adapter's rule, the resync choice and time conversion.
//! Adapter crates call [`run`] from their tests and assert that it returns no failure.

use crate::adapter::{MarketDataAdapter, Resync, Route};
use crate::book::{BookSide, OrderBook};
use crate::seq::GapDetector;
use crate::{BookUpdate, Level, Normalized, SeqRule, StreamKey};

/// One order book stream of the adapter's venue, in its wire format.
#[derive(Debug, Clone)]
pub struct Fixture {
    pub key: StreamKey,
    pub snapshot: Vec<u8>,
    /// Deltas that follow the snapshot without a gap; at least one removes a level.
    pub deltas: Vec<Vec<u8>>,
    /// The book after the snapshot and the deltas, best first.
    pub expected_bids: Vec<Level>,
    pub expected_asks: Vec<Level>,
    /// A price the deltas remove with quantity 0.
    pub deleted_px: i64,
    pub expected_rule: SeqRule,
    /// A delta that breaks the rule after the deltas; `None` for a venue without sequence numbers.
    pub gap_delta: Option<Vec<u8>>,
    pub expected_resync: Resync,
    /// The receive time passed to `parse`.
    pub recv_ns: i64,
    /// `venue_ts_ns` the snapshot must give.
    pub expected_venue_ts_ns: i64,
    /// `venue_ts_ns` each delta must give, in order.
    pub expected_delta_venue_ts_ns: Vec<i64>,
    /// Exponents every book message must carry.
    pub expected_px_exp: i8,
    pub expected_qty_exp: i8,
    /// Frames that are not market data; `route` must give `Route::Control`.
    pub garbage: Vec<Vec<u8>>,
}

fn book_update(
    adapter: &dyn MarketDataAdapter,
    f: &Fixture,
    frame: &[u8],
    what: &str,
    failures: &mut Vec<String>,
) -> Option<(bool, BookUpdate)> {
    if adapter.route(frame) != Route::Stream(f.key.clone()) {
        failures.push(format!(
            "{what}: route does not give the fixture's stream key"
        ));
    }
    let checked = |u: &BookUpdate, failures: &mut Vec<String>| {
        if u.key != f.key {
            failures.push(format!("{what}: key is {:?}, expected {:?}", u.key, f.key));
        }
        if (u.px_exp, u.qty_exp) != (f.expected_px_exp, f.expected_qty_exp) {
            failures.push(format!(
                "{what}: exponents are ({}, {}), expected ({}, {})",
                u.px_exp, u.qty_exp, f.expected_px_exp, f.expected_qty_exp
            ));
        }
        if u.recv_ns != f.recv_ns {
            failures.push(format!(
                "{what}: recv_ns is {}, expected {}",
                u.recv_ns, f.recv_ns
            ));
        }
        u.clone()
    };
    match adapter.parse(&f.key, frame, f.recv_ns) {
        Ok(msgs) => match msgs.as_slice() {
            [Normalized::BookSnapshot(u)] => Some((true, checked(u, failures))),
            [Normalized::BookDelta(u)] => Some((false, checked(u, failures))),
            other => {
                failures.push(format!(
                    "{what}: parse gave {} messages, not one book message",
                    other.len()
                ));
                None
            }
        },
        Err(e) => {
            failures.push(format!("{what}: parse error {}", e.reason));
            None
        }
    }
}

/// A fixture too weak to catch a broken adapter: without deltas, without a
/// removed level that was in the snapshot, or without a gap delta for a venue
/// that has sequence numbers, the harness would pass an adapter that drops
/// zero-quantity levels or never reports a gap.
fn fixture_problems(adapter: &dyn MarketDataAdapter, f: &Fixture) -> Vec<String> {
    let mut problems = Vec::new();
    if f.deltas.is_empty() {
        problems.push("fixture: no deltas".to_string());
    }
    if f.expected_delta_venue_ts_ns.len() != f.deltas.len() {
        problems.push(format!(
            "fixture: {} delta times for {} deltas",
            f.expected_delta_venue_ts_ns.len(),
            f.deltas.len()
        ));
    }
    if f.expected_rule != SeqRule::None && f.gap_delta.is_none() {
        problems.push("fixture: no gap delta for a venue with sequence numbers".to_string());
    }
    let parsed = |frame: &[u8]| -> Vec<BookUpdate> {
        match adapter.parse(&f.key, frame, f.recv_ns) {
            Ok(msgs) => msgs
                .into_iter()
                .filter_map(|m| match m {
                    Normalized::BookSnapshot(u) | Normalized::BookDelta(u) => Some(u),
                    _ => None,
                })
                .collect(),
            Err(_) => Vec::new(),
        }
    };
    // If the level was never in the book, "it is gone at the end" proves
    // nothing. If it was, only a delta can remove it, so the book check at the
    // end catches an adapter that drops zero-quantity levels.
    let in_snapshot = parsed(&f.snapshot).iter().any(|u: &BookUpdate| {
        u.bids
            .iter()
            .chain(&u.asks)
            .any(|l| l.px == f.deleted_px && l.qty != 0)
    });
    if !in_snapshot {
        problems.push(format!(
            "fixture: deleted level {} is not in the snapshot",
            f.deleted_px
        ));
    }
    problems
}

/// Every failed check, as text; empty when the adapter conforms.
pub fn run(adapter: &dyn MarketDataAdapter, f: &Fixture) -> Vec<String> {
    let mut failures = fixture_problems(adapter, f);
    let rule = adapter.seq_rule(&f.key);
    if rule != f.expected_rule {
        failures.push(format!(
            "seq_rule is {rule:?}, expected {:?}",
            f.expected_rule
        ));
    }
    let mut book = OrderBook::new(None);
    let mut gaps = GapDetector::new(rule);

    match book_update(adapter, f, &f.snapshot, "snapshot", &mut failures) {
        Some((true, u)) => {
            if u.venue_ts_ns != f.expected_venue_ts_ns {
                failures.push(format!(
                    "snapshot: venue_ts_ns is {}, expected {}",
                    u.venue_ts_ns, f.expected_venue_ts_ns
                ));
            }
            book.apply_snapshot(&u);
            gaps.snapshot(u.update_id, u.recv_ns);
        }
        Some((false, _)) => failures.push("snapshot: parsed as a delta".into()),
        None => {}
    }
    for (i, frame) in f.deltas.iter().enumerate() {
        match book_update(adapter, f, frame, &format!("delta {i}"), &mut failures) {
            Some((false, u)) => {
                if let Some(&want) = f.expected_delta_venue_ts_ns.get(i) {
                    if u.venue_ts_ns != want {
                        failures.push(format!(
                            "delta {i}: venue_ts_ns is {}, expected {want}",
                            u.venue_ts_ns
                        ));
                    }
                }
                if gaps
                    .delta(u.update_id, u.prev_update_id, u.recv_ns)
                    .is_some()
                {
                    failures.push(format!(
                        "delta {i}: a gap was reported for a delta that follows the rule"
                    ));
                }
                book.apply_delta(&u);
            }
            Some((true, _)) => failures.push(format!("delta {i}: parsed as a snapshot")),
            None => {}
        }
    }
    if book.levels(BookSide::Bid) != f.expected_bids
        || book.levels(BookSide::Ask) != f.expected_asks
    {
        failures.push(format!(
            "book after the deltas is bids {:?} asks {:?}",
            book.levels(BookSide::Bid),
            book.levels(BookSide::Ask)
        ));
    }
    let all: Vec<Level> = book
        .levels(BookSide::Bid)
        .into_iter()
        .chain(book.levels(BookSide::Ask))
        .collect();
    if all.iter().any(|l| l.px == f.deleted_px) {
        failures.push(format!(
            "deleted level {} is still in the book",
            f.deleted_px
        ));
    }
    if let Some(frame) = &f.gap_delta {
        match book_update(adapter, f, frame, "gap delta", &mut failures) {
            Some((true, _)) => failures.push("gap delta: parsed as a snapshot".into()),
            None => {}
            Some((false, u)) => match gaps.delta(u.update_id, u.prev_update_id, u.recv_ns) {
                Some(gap) => {
                    let resync = adapter.on_gap(&f.key, &gap);
                    if resync != f.expected_resync {
                        failures.push(format!(
                            "on_gap chose {resync:?}, expected {:?}",
                            f.expected_resync
                        ));
                    }
                }
                None => failures.push("gap delta: no gap was reported".into()),
            },
        }
    }
    for (i, frame) in f.garbage.iter().enumerate() {
        if adapter.route(frame) != Route::Control {
            failures.push(format!(
                "garbage frame {i}: route did not give Route::Control"
            ));
        }
    }
    failures
}
