//! Margin rules (contract C-MARGIN, ADR-018). Expected values are literals
//! worked out by hand from the contract, not derived from the implementation.
//!
//! The hold-table tests of C-MARGIN sit beside the other hold tests in
//! `scenarios.rs`.

#![allow(clippy::unwrap_used)]

mod common;
use common::*;
use oms_core::*;

// ---- C-MARGIN: the policy section ------------------------------------------

/// A run without a margin policy must keep the journal bytes it had before
/// the section existed: the absent section is not written at all, not
/// written as an empty value.
#[test]
fn absent_margin_policy_adds_no_bytes_to_a_journal_record() {
    let h = H::ready();
    let activation = h
        .log
        .iter()
        .find(|rec| matches!(rec.input, Input::Config(_)))
        .unwrap();
    let bytes = serde_json::to_string(activation).unwrap();
    assert!(
        bytes.contains("\"policy\":{"),
        "the record carries the policy: {bytes}"
    );
    assert!(
        !bytes.contains("\"margin\""),
        "an absent margin policy left bytes in the record: {bytes}"
    );
    // What was written reads back as "no margin policy".
    let back: Record = serde_json::from_str(&bytes).unwrap();
    assert_eq!(&back, activation);
    assert_eq!(h.core.config.policy.margin, None);
}

/// An empty section turns the rules on with the defaults of the contract's
/// 설정 table: limit 2x, daily loss 5 %, MM buffer 0.25 percentage points,
/// report age 30 s, stale cancel 60 s, venue setting 2x isolated, no extra
/// account mode allowed.
#[test]
fn empty_margin_section_takes_the_contract_defaults() {
    let policy: Policy = serde_json::from_str(r#"{"margin":{}}"#).unwrap();
    assert_eq!(
        serde_json::to_value(&policy.margin).unwrap(),
        serde_json::json!({
            "max_account_leverage": "2",
            "daily_loss_bps": 500,
            "mm_buffer_bps": 25,
            "margin_max_age_us": 30_000_000,
            "stale_cancel_us": 60_000_000,
            "venue_leverage": 2,
            "isolated": true,
            "account_modes_allowed": [],
        })
    );
    // The section is domain state: it is journaled with the configuration
    // and restored by replay.
    let h = H::ready_with(CoreConfig { policy, ..config() });
    assert!(h.core.config.policy.margin.is_some());
    assert_eq!(h.rebuilt().config, h.core.config);
}

// ---- C-MARGIN: margin report intake -----------------------------------------

fn names(out: &Output) -> Vec<&'static str> {
    out.events.iter().map(Event::name).collect()
}

/// An isolated position as the venue reports it.
fn position(instrument: &str, max_leverage: u32) -> MarginPosition {
    MarginPosition {
        instrument: iid(instrument),
        szi: sq("2"),
        position_value: amt("200"),
        margin_used: amt("100"),
        leverage_type: LeverageType::Isolated,
        leverage_value: 2,
        max_leverage,
    }
}

/// A report of the current link session becomes the newest report: one
/// `margin_reported` event, and the state holds the report as it came. A
/// position on an instrument that is not configured is kept.
#[test]
fn margin_report_of_the_current_session_is_recorded() {
    let mut h = H::ready_with(config_with_margin());
    let queried_at = h.now;
    let report = MarginReportIn {
        session: h.session,
        seq: 1,
        queried_at,
        account_value: amt("100"),
        positions: vec![position("X", 40), position("NOT-CONFIGURED", 20)],
    };
    let out = h.venue(VenueInput::MarginReport(report.clone()));
    assert_eq!(names(&out), vec!["margin_reported"]);
    assert_eq!(h.core.margin.report, Some(report));
    // Nothing is sent to the venue because of a report.
    assert_eq!(out.commands, vec![]);
}
