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
    match adapter.parse(&f.key, frame, f.recv_ns) {
        Ok(msgs) => match msgs.as_slice() {
            [Normalized::BookSnapshot(u)] => Some((true, u.clone())),
            [Normalized::BookDelta(u)] => Some((false, u.clone())),
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

/// Every failed check, as text; empty when the adapter conforms.
pub fn run(adapter: &dyn MarketDataAdapter, f: &Fixture) -> Vec<String> {
    let mut failures = Vec::new();
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
            if (u.venue_ts_ns, u.recv_ns) != (f.expected_venue_ts_ns, f.recv_ns) {
                failures.push(format!(
                    "snapshot times are ({}, {}), expected ({}, {})",
                    u.venue_ts_ns, u.recv_ns, f.expected_venue_ts_ns, f.recv_ns
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
        if let Some((false, u)) = book_update(adapter, f, frame, "gap delta", &mut failures) {
            match gaps.delta(u.update_id, u.prev_update_id, u.recv_ns) {
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
            }
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
