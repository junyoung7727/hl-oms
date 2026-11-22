//! The event names the watcher knows: every row of the C-TAIL field table
//! (`docs/contracts/tail.md`, "이벤트 필드 표"). An event outside this list
//! stops the reader (C-TAIL "기록 형식": unknown names are not skipped).

/// In the order of the field table.
pub const KNOWN: &[&str] = &[
    "config_activated",
    "booted",
    "link_changed",
    "request_rejected",
    "order_accepted",
    "order_acked",
    "venue_oid_learned",
    "order_in_doubt",
    "status_noted",
    "order_recheck",
    "fill_booked",
    "fill_parked",
    "fill_unparked",
    "external_fill_booked",
    "venue_terminal",
    "missing_fills_flagged",
    "order_closed",
    "cancel_requested",
    "cancel_sent",
    "cancel_outcome_noted",
    "fills_synced",
    "open_orders_swept",
    "positions_verified",
    "mismatch_raised",
    "mismatch_acked",
    "operator_halted",
    "operator_resumed",
    "storage_low_changed",
    "ref_price_updated",
    "position_adjusted",
    "purged",
    "margin_reported",
    "margin_report_rejected",
    "day_base_set",
    "reducing_entered",
    "venue_setting_noted",
    "account_mode_noted",
    "reducing_cleared",
    "funding_booked",
    "venue_position_changed",
    "unclassified_execution_noted",
    "venue_change_settled",
    "absence_noted",
    "hold_set",
    "hold_cleared",
    "kill_loop_ended",
    "kill_rearmed",
    "strategy_link_lost",
    "strategy_link_restored",
    "watcher_heartbeat",
    "monitor_changed",
];

pub fn is_known(name: &str) -> bool {
    KNOWN.contains(&name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    /// The first column of the field table, read from the contract at test
    /// time (a file read, so the watcher holds no copy of OMS text).
    fn table_names() -> BTreeSet<String> {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/contracts/tail.md");
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        let begin = text
            .find("<!-- event-fields:begin -->")
            .expect("begin marker");
        let end = text.find("<!-- event-fields:end -->").expect("end marker");
        text[begin..end]
            .lines()
            .filter_map(|l| l.strip_prefix("| `"))
            .filter_map(|l| l.split_once('`').map(|(name, _)| name.to_string()))
            .collect()
    }

    #[test]
    fn the_known_events_are_the_rows_of_the_tail_table() {
        let table = table_names();
        let known: BTreeSet<String> = KNOWN.iter().map(|s| s.to_string()).collect();
        assert!(
            table.len() > 40,
            "the table was not found or not read: {table:?}"
        );
        let untaught: Vec<_> = table.difference(&known).collect();
        let stale: Vec<_> = known.difference(&table).collect();
        assert!(
            untaught.is_empty(),
            "events of the tail table the watcher does not know: {untaught:?}"
        );
        assert!(
            stale.is_empty(),
            "known events that are not in the tail table: {stale:?}"
        );
        assert_eq!(known.len(), KNOWN.len(), "a name is listed twice");
    }
}
