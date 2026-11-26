//! Readers of the node's journal files on disk, for the fakes and the
//! assertions: no lock and no node.

use oms_core::*;
use std::path::Path;

/// Every valid record in the journal files on disk, read without the lock
/// and without the node: a record being written is not one yet.
pub fn records(data: &Path) -> Vec<Record> {
    oms_journal::Tail::new(data, 0)
        .peek_beyond(usize::MAX)
        .unwrap_or_default()
        .into_iter()
        .map(|r| serde_json::from_slice(&r.payload).unwrap())
        .collect()
}

/// Whether the journal on disk holds the acceptance of the order whose
/// `orderLinkId` is `link`, for the instance in the data directory.
pub fn journal_holds(data: &Path, link: &str) -> bool {
    let Some(id) = WireIdForm::Base32Pair.decode(link) else {
        return false;
    };
    let instance = std::fs::read_to_string(data.join("INSTANCE"))
        .ok()
        .and_then(|t| u64::from_str_radix(t.trim(), 16).ok());
    instance == Some(id.instance_id)
        && records(data).iter().any(|r| {
            r.events.iter().any(
                |e| matches!(e, Event::OrderAccepted { order_id, .. } if *order_id == id.order_id),
            )
        })
}

/// The kill events in the journal files on disk.
pub fn kill_events(data: &Path) -> Vec<Event> {
    records(data)
        .into_iter()
        .flat_map(|r| r.events)
        .filter(|e| {
            matches!(
                e,
                Event::HoldSet {
                    hold: HoldKind::Kill,
                    ..
                } | Event::HoldCleared { .. }
                    | Event::KillLoopEnded { .. }
                    | Event::KillRearmed { .. }
            )
        })
        .collect()
}
