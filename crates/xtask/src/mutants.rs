//! Selective mutation check.
//!
//! Each entry removes or weakens one protective piece of logic in a copy of
//! the workspace and runs the tests that are supposed to depend on it. The
//! tests must fail. If they pass, the protection is not actually guarded by
//! those tests, and the mutant is reported as survived.
//!
//! This is a hand-picked list, not exhaustive mutation testing. It answers
//! one question per entry: "would we notice if this were gone?"

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

struct Mutant {
    id: &'static str,
    what: &'static str,
    file: &'static str,
    find: &'static str,
    replace: &'static str,
    /// Arguments after `cargo test`.
    test: &'static [&'static str],
    guards: &'static str,
}

const MUTANTS: &[Mutant] = &[
    Mutant {
        id: "M01-ignore-flush-failure",
        what: "the commit stage ignores a failed flush and releases outputs anyway",
        file: "crates/oms-node/src/pipeline.rs",
        find: "            synced.map_err(|e| self.fail(e))?;",
        replace: "            let _ = synced;",
        test: &["-p", "oms-node", "--features", "failpoints", "--test", "pipeline", "outputs_follow_durability_with_a_commit_thread"],
        guards: "I-WAL / C-COMMIT",
    },
    Mutant {
        id: "M02-no-duplicate-lookup",
        what: "a new-order request no longer checks whether its key was already used",
        file: "crates/oms-core/src/decide.rs",
        find: "if let Some(id) = self.core.lookup_client(&r.strategy, &r.client_order_id) {",
        replace: "if let Some(id) = None::<OrderId> {",
        test: &["-p", "oms-core", "--test", "scenarios", "same_key"],
        guards: "I-KEY / C-IDEM",
    },
    Mutant {
        id: "M03-no-account-limit",
        what: "the account position bound is not checked for buys",
        file: "crates/oms-core/src/decide.rs",
        find: "Side::Buy => v <= inst.account_pos_max.0,",
        replace: "Side::Buy => true,",
        test: &["-p", "oms-core", "--test", "scenarios", "fia_7_1"],
        guards: "I-QTY / C-NEW",
    },
    Mutant {
        id: "M04-absence-without-waiting",
        what: "'not found' closes an in-doubt order without waiting for expiry plus margin",
        file: "crates/oms-core/src/decide.rs",
        find: "if queried_at < threshold {",
        replace: "if false {",
        test: &["-p", "oms-core", "--test", "scenarios", "not_found_closes"],
        guards: "C-DOUBT",
    },
    Mutant {
        id: "M05-restart-forgets-outstanding-sends",
        what: "after a restart, sends without a recorded result are not marked in doubt",
        file: "crates/oms-core/src/decide.rs",
        find: "if pending_without_result {",
        replace: "if false {",
        test: &["-p", "oms-core", "--test", "scenarios", "restart_puts"],
        guards: "C-BOOT",
    },
    Mutant {
        id: "M06-no-fill-dedup",
        what: "a fill whose key was already booked is booked again",
        file: "crates/oms-core/src/decide.rs",
        find: "if self.core.seen_fills.contains_key(&f.key) || self.core.parked_fills.iter().any(|p| p.fill.key == f.key) {",
        replace: "if false {",
        test: &["-p", "oms-core", "--test", "scenarios", "duplicate_fill"],
        guards: "I-FILL / C-FILL",
    },
    Mutant {
        id: "M07-holds-not-enforced",
        what: "hold reasons no longer cause a rejection, in either column of the hold table",
        file: "crates/oms-core/src/decide.rs",
        find: "why.push(R::Hold(h));",
        replace: "let _ = h;",
        test: &["-p", "oms-core", "--test", "scenarios", "unknown_send_outcome"],
        guards: "C-NEW / holds",
    },
    Mutant {
        id: "M08-close-without-fill-sync",
        what: "an order with unknown final quantity is closed before fills are known to be complete",
        file: "crates/oms-core/src/decide.rs",
        find: "None if synced => self.emit(Event::OrderClosed { order_id: id, reason }),",
        replace: "None => self.emit(Event::OrderClosed { order_id: id, reason }),",
        test: &["-p", "oms-core", "--test", "scenarios", "terminal_without_final_quantity"],
        guards: "C-TERM",
    },
    Mutant {
        id: "M09-key-forgotten-before-expiry",
        what: "a closed order is purged without waiting for its request to expire",
        file: "crates/oms-core/src/decide.rs",
        find: "&& self.at >= o.expires_at.saturating_add(policy.dedup_grace_us)",
        replace: "",
        test: &["-p", "oms-core", "--test", "scenarios", "a_key_is_forgotten"],
        guards: "C-IDEM",
    },
    Mutant {
        id: "M10-stale-position-snapshot-judged",
        what: "a position snapshot older than the last booked fill is compared anyway",
        file: "crates/oms-core/src/decide.rs",
        find: "if self.core.last_fill_at >= queried_at {",
        replace: "if false {",
        test: &["-p", "oms-core", "--test", "scenarios", "a_position_snapshot_older"],
        guards: "C-RECON",
    },
    Mutant {
        id: "M11-mid-journal-damage-truncated",
        what: "damage in the middle of the journal is treated as an incomplete tail",
        file: "crates/oms-journal/src/segment.rs",
        find: "if !is_last || later.is_some() {",
        replace: "if false {",
        test: &["-p", "oms-journal", "--test", "journal", "damage_in_the_middle"],
        guards: "C-JOURNAL",
    },
    Mutant {
        id: "M12-checksum-not-verified",
        what: "frame checksums are not verified",
        file: "crates/oms-journal/src/frame.rs",
        find: "if h.finalize() != crc {",
        replace: "if false {",
        test: &["-p", "oms-journal", "--lib", "every_truncation_and_every_bit_flip"],
        guards: "C-JOURNAL",
    },
    Mutant {
        id: "M13-sim-cancel-releases-reservation",
        what: "a cancel request releases the order's reservation at once (caught by the simulation)",
        file: "crates/oms-core/src/core.rs",
        find: "            Event::CancelRequested { order_id, by } => {\n                let o = self.order_mut(*order_id)?;",
        replace: "            Event::CancelRequested { order_id, by } => {\n                let r = self.order_mut(*order_id)?.reserved;\n                self.release(*order_id, r)?;\n                let o = self.order_mut(*order_id)?;",
        test: &["--release", "-p", "oms-sim", "--test", "sim", "profile_calm"],
        guards: "I-RES / I-BRIDGE (simulation)",
    },
    Mutant {
        id: "M14-sim-resend-on-timeout",
        what: "the simulated gateway model reports 'not sent' for a request that timed out (caught by the simulation)",
        file: "crates/oms-sim/src/world.rs",
        find: "outcome: SendOutcome::Unknown { reason: reason.to_string() }, timing",
        replace: "outcome: SendOutcome::NotSent { reason: reason.to_string() }, timing",
        test: &["--release", "-p", "oms-sim", "--test", "sim", "profile_lossy"],
        guards: "G-2 of C-GW (simulation)",
    },
    Mutant {
        id: "M15-gateway-timeout-as-not-sent",
        what: "the real gateway reports 'not sent' when no answer arrived (process test)",
        file: "crates/oms-node/src/venue/simnet.rs",
        find: "Err(CallError::NoAnswer(e)) => SendOutcome::Unknown { reason: e },",
        replace: "Err(CallError::NoAnswer(e)) => SendOutcome::NotSent { reason: e },",
        test: &["-p", "oms-node", "--test", "e2e", "a_lost_request_is_closed_only_after_its_send_window"],
        guards: "G-2 of C-GW (process)",
    },
    // Protections added after the gap review of 2026-10-02.
    Mutant {
        id: "M16-parked-fill-not-drained",
        what: "fills waiting for a venue order id are not booked when another fill reveals that id",
        file: "crates/oms-core/src/decide.rs",
        find: "if identified {",
        replace: "if false {",
        test: &["-p", "oms-core", "--test", "review_core", "f2_parked_fill"],
        guards: "C-FILL (review F2)",
    },
    Mutant {
        id: "M17-cancel-budget-never-restarts",
        what: "a cancel whose attempts ran out can never be requested again",
        file: "crates/oms-core/src/decide.rs",
        find: ".is_some_and(|c| !c.inflight && !c.confirmed && c.used() >= max)",
        replace: ".is_some_and(|_| false)",
        test: &["-p", "oms-core", "--test", "review_core", "f1_cancel_can_be_reissued"],
        guards: "C-CANCEL (review F1)",
    },
    Mutant {
        id: "M18-stale-link-down-accepted",
        what: "a link-down notice for an older session takes the current session down",
        file: "crates/oms-core/src/decide.rs",
        find: "} else if self.core.link.up && session >= self.core.link.session {",
        replace: "} else if self.core.link.up {",
        test: &["-p", "oms-core", "--test", "review_core", "f5_stale_link_down"],
        guards: "C-LINK (review F5)",
    },
    Mutant {
        id: "M19-fills-complete-on-a-possibly-full-page",
        what: "the Hyperliquid gateway takes any page shorter than 2000 fills as the last one",
        file: "crates/oms-node/src/venue/hyperliquid/mod.rs",
        find: "if page_len < SMALLEST_FILL_PAGE {",
        replace: "if page_len < 2000 {",
        test: &["-p", "oms-node", "--lib", "a_fill_page_proves_completeness"],
        guards: "G-7 of C-GW",
    },
    Mutant {
        id: "M20-one-connection-pool",
        what: "the admin socket shares its connection pool with the trading socket",
        file: "crates/oms-node/src/server.rs",
        find: "let pool = if admin { &ctx.admin_connections } else { &ctx.connections };",
        replace: "let pool = &ctx.connections;",
        test: &["-p", "oms-node", "--test", "review_node", "idle_trading_connections"],
        guards: "C-API / C-OVERLOAD",
    },
    // The hold table of ADR-008 §7 (margin rules, ticket 01).
    Mutant {
        id: "M21-partial-hold-refuses-reduce-only",
        what: "a hold that refuses only plain orders also refuses a reduce-only order",
        file: "crates/oms-core/src/model.rs",
        find: "HoldReason::Resync | HoldReason::InDoubt | HoldReason::Mismatch => true,",
        replace: "HoldReason::Resync | HoldReason::InDoubt | HoldReason::Mismatch => false,",
        test: &["-p", "oms-core", "--test", "scenarios", "reduce_only_order_is_accepted"],
        guards: "C-MARGIN / hold table",
    },
    Mutant {
        id: "M22-block-all-hold-passes-reduce-only",
        what: "a hold that refuses every new order lets a reduce-only order pass",
        file: "crates/oms-core/src/model.rs",
        find: "HoldReason::OperatorStrategy | HoldReason::StorageLow => false,",
        replace: "HoldReason::OperatorStrategy | HoldReason::StorageLow => true,",
        test: &["-p", "oms-core", "--test", "scenarios", "reduce_only_order_is_rejected_by_every_block_all_hold"],
        guards: "C-MARGIN / hold table",
    },
    Mutant {
        id: "M23-hold-table-without-margin-policy",
        what: "the hold table applies although no margin policy is set",
        file: "crates/oms-core/src/decide.rs",
        find: "let table_applies = cfg.policy.margin.is_some();",
        replace: "let table_applies = true;",
        test: &["-p", "oms-core", "--test", "scenarios", "without_a_margin_policy_every_hold_still_refuses_a_reduce_only_order"],
        guards: "C-MARGIN / hold table",
    },
    // Margin report intake and freshness (margin rules, ticket 02).
    Mutant {
        id: "M24-margin-report-of-another-session-taken",
        what: "a margin report is taken although it belongs to another link session",
        file: "crates/oms-core/src/decide.rs",
        find: "self.core.link.up && session == self.core.link.session",
        replace: "self.core.link.up",
        test: &["-p", "oms-core", "--test", "margin", "margin_report_of_an_old_session_or_with_the_link_down_is_ignored"],
        guards: "C-MARGIN / report intake",
    },
    Mutant {
        id: "M25-margin-report-taken-with-link-down",
        what: "a margin report is taken while the link is down",
        file: "crates/oms-core/src/decide.rs",
        find: "self.core.link.up && session == self.core.link.session",
        replace: "session == self.core.link.session",
        test: &["-p", "oms-core", "--test", "margin", "margin_report_of_an_old_session_or_with_the_link_down_is_ignored"],
        guards: "C-MARGIN / report intake",
    },
    Mutant {
        id: "M26-no-margin-sequence-guard",
        what: "a margin report with a lower number replaces a newer one",
        file: "crates/oms-core/src/decide.rs",
        find: "if last_seq.is_some_and(|last| r.seq <= last) {",
        replace: "if false {",
        test: &["-p", "oms-core", "--test", "margin", "late_margin_report_with_a_lower_sequence_is_ignored"],
        guards: "C-MARGIN / report intake",
    },
    Mutant {
        id: "M27-margin-sequence-guard-lets-equal-pass",
        what: "a margin report with the same number as the last one is taken again",
        file: "crates/oms-core/src/decide.rs",
        find: "if last_seq.is_some_and(|last| r.seq <= last) {",
        replace: "if last_seq.is_some_and(|last| r.seq < last) {",
        test: &["-p", "oms-core", "--test", "margin", "late_margin_report_with_a_lower_sequence_is_ignored"],
        guards: "C-MARGIN / report intake",
    },
    Mutant {
        id: "M28-old-margin-report-is-fresh",
        what: "a margin report of any age counts as fresh",
        file: "crates/oms-core/src/core.rs",
        find: "let young_enough = at.saturating_sub(report.queried_at) <= policy.margin_max_age_us;",
        replace: "let young_enough = policy.margin_max_age_us > 0;",
        test: &["-p", "oms-core", "--test", "margin", "plain_order_is_rejected_margin_unavailable_without_a_fresh_report"],
        guards: "C-MARGIN / freshness",
    },
    Mutant {
        id: "M29-margin-report-at-the-age-limit-is-stale",
        what: "a margin report exactly as old as the limit does not count as fresh",
        file: "crates/oms-core/src/core.rs",
        find: "at.saturating_sub(report.queried_at) <= policy.margin_max_age_us;",
        replace: "at.saturating_sub(report.queried_at) < policy.margin_max_age_us;",
        test: &["-p", "oms-core", "--test", "margin", "plain_order_is_rejected_margin_unavailable_without_a_fresh_report"],
        guards: "C-MARGIN / freshness",
    },
    Mutant {
        id: "M30-future-margin-report-is-fresh",
        what: "a margin report dated after the record time counts as fresh",
        file: "crates/oms-core/src/core.rs",
        find: "let not_from_the_future = report.queried_at <= at;",
        replace: "let not_from_the_future = report.queried_at > 0;",
        test: &["-p", "oms-core", "--test", "margin", "margin_report_dated_after_the_record_time_is_not_fresh"],
        guards: "C-MARGIN / freshness",
    },
    Mutant {
        id: "M31-margin-report-of-age-zero-is-stale",
        what: "a margin report asked for at the record time itself does not count as fresh",
        file: "crates/oms-core/src/core.rs",
        find: "let not_from_the_future = report.queried_at <= at;",
        replace: "let not_from_the_future = report.queried_at < at;",
        test: &["-p", "oms-core", "--test", "margin", "margin_report_dated_after_the_record_time_is_not_fresh"],
        guards: "C-MARGIN / freshness",
    },
    Mutant {
        id: "M32-leverage-gate-refuses-at-the-limit",
        what: "the leverage gate refuses an order that lands exactly on the limit",
        file: "crates/oms-core/src/exposure.rs",
        find: "(after > limit && after > before).then_some(RejectReason::AccountLeverage)",
        replace: "(after >= limit && after > before).then_some(RejectReason::AccountLeverage)",
        test: &["-p", "oms-core", "--test", "margin", "order_exactly_at_the_limit_is_accepted"],
        guards: "C-MARGIN / leverage gate",
    },
    Mutant {
        id: "M33-leverage-gate-ignores-exposure-before",
        what: "the leverage gate refuses every plain order while exposure is above the limit, also one that does not raise it",
        file: "crates/oms-core/src/exposure.rs",
        find: "(after > limit && after > before).then_some(RejectReason::AccountLeverage)",
        replace: "(after > limit).then_some(RejectReason::AccountLeverage)",
        test: &["-p", "oms-core", "--test", "margin", "after_a_passive_breach_a_plain_reducing_order_passes_and_an_increasing_one_does_not"],
        guards: "C-MARGIN / leverage gate",
    },
    Mutant {
        id: "M34-order-valued-at-the-lower-price",
        what: "an order counts at the lower of its limit price and the reference price, not the higher",
        file: "crates/oms-core/src/exposure.rs",
        find: "i128::from(px.0.max(reference(i)?));",
        replace: "i128::from(px.0.min(reference(i)?));",
        test: &["-p", "oms-core", "--test", "margin", "resting_order_counts_at_the_higher_of_limit_and_reference"],
        guards: "C-MARGIN / leverage gate",
    },
    Mutant {
        id: "M35-order-valued-at-its-limit-price-only",
        what: "an order far below the reference price counts at its limit price, not at the reference price",
        file: "crates/oms-core/src/exposure.rs",
        find: "i128::from(px.0.max(reference(i)?));",
        replace: "i128::from(px.0.max(reference(i)?.min(px.0)));",
        test: &["-p", "oms-core", "--test", "margin", "resting_order_counts_at_the_higher_of_limit_and_reference"],
        guards: "C-MARGIN / leverage gate",
    },
    Mutant {
        id: "M36-exposure-is-the-largest-instrument",
        what: "account exposure is the largest instrument exposure, not the sum over instruments",
        file: "crates/oms-core/src/exposure.rs",
        find: ".checked_add(up.max(down))",
        replace: ".max(up.max(down)).checked_add(0)",
        test: &["-p", "oms-core", "--test", "margin", "exposure_is_the_sum_over_instruments"],
        guards: "C-MARGIN / leverage gate",
    },
    Mutant {
        id: "M37-daily-loss-judged-only-on-a-fresh-report",
        what: "a report that is older than the freshness limit when it arrives is recorded but not judged for the daily loss",
        file: "crates/oms-core/src/decide.rs",
        find: "        self.judge_daily_loss(r)?;\n",
        replace: "        if self.core.margin_report_is_fresh(self.at) {\n            self.judge_daily_loss(r)?;\n        }\n",
        test: &["-p", "oms-core", "--test", "margin", "a_report_older_than_the_freshness_limit_on_arrival_is_still_judged"],
        guards: "C-MARGIN / daily loss",
    },
    Mutant {
        id: "M38-daily-loss-trips-at-exactly-the-limit",
        what: "a loss of exactly the daily-loss limit enters REDUCING",
        file: "crates/oms-core/src/decide.rs",
        find: "(base_value - value) * 10_000 > i128::from(policy.daily_loss_bps) * base_value;",
        replace: "(base_value - value) * 10_000 >= i128::from(policy.daily_loss_bps) * base_value;",
        test: &["-p", "oms-core", "--test", "margin", "loss_greater_than_five_percent_of_the_base_enters_reducing"],
        guards: "C-MARGIN / daily loss",
    },
    Mutant {
        id: "M39-day-rolls-before-the-report-is-judged",
        what: "the first report of a new day becomes the day base before it is judged, so a loss across midnight is absorbed",
        file: "crates/oms-core/src/decide.rs",
        find: "        self.judge_daily_loss(r)?;\n        self.roll_day_base(r)\n",
        replace: "        self.roll_day_base(r)?;\n        self.judge_daily_loss(r)\n",
        test: &["-p", "oms-core", "--test", "margin", "first_report_after_midnight_is_judged_against_the_old_base_then_rolls_the_day"],
        guards: "C-MARGIN / daily loss",
    },
    Mutant {
        id: "M40-entering-reducing-cancels-reduce-only-orders",
        what: "entering REDUCING also cancels orders with the reduce-only flag",
        file: "crates/oms-core/src/decide.rs",
        find: "let is_plain = !o.reduce_only;",
        replace: "let is_plain = true;",
        test: &["-p", "oms-core", "--test", "margin", "entering_reducing_cancels_plain_orders_and_keeps_reduce_only_ones"],
        guards: "C-MARGIN / REDUCING",
    },
    Mutant {
        id: "M41-reducing-entered-again",
        what: "a trigger while the account is already REDUCING enters it again and asks for cancels again",
        file: "crates/oms-core/src/decide.rs",
        find: "if self.core.margin.reducing.is_some() {",
        replace: "if false {",
        test: &["-p", "oms-core", "--test", "margin", "reducing_is_entered_once"],
        guards: "C-MARGIN / REDUCING",
    },
    Mutant {
        id: "M42-reducing-refuses-reduce-only-orders",
        what: "the hold Reducing refuses every new order, reduce-only included",
        file: "crates/oms-core/src/model.rs",
        find: "HoldReason::Reducing => true,",
        replace: "HoldReason::Reducing => false,",
        test: &["-p", "oms-core", "--test", "margin", "under_reducing_a_plain_order_is_rejected_and_a_reduce_only_one_is_accepted"],
        guards: "C-MARGIN / hold table (ADR-008 §7)",
    },
    Mutant {
        id: "M43-venue-margin-mode-not-compared",
        what: "an instrument on cross margin passes although the policy asks for isolated",
        file: "crates/oms-core/src/core.rs",
        find: "let mode_matches = seen.isolated == policy.isolated;",
        replace: "let mode_matches = seen.leverage > 0;",
        test: &["-p", "oms-core", "--test", "margin", "a_setting_that_differs_from_config_holds_only_that_instrument"],
        guards: "C-MARGIN / venue setting and account mode",
    },
    Mutant {
        id: "M44-venue-leverage-not-compared",
        what: "an instrument at another venue leverage passes",
        file: "crates/oms-core/src/core.rs",
        find: "let leverage_matches = seen.leverage == policy.venue_leverage;",
        replace: "let leverage_matches = seen.leverage > 0;",
        test: &["-p", "oms-core", "--test", "margin", "a_setting_that_differs_from_config_holds_only_that_instrument"],
        guards: "C-MARGIN / venue setting and account mode",
    },
    Mutant {
        id: "M45-venue-settings-survive-a-link-change",
        what: "a venue setting told in an earlier link session still counts",
        file: "crates/oms-core/src/core.rs",
        find: "self.margin.venue_settings.clear();",
        replace: "",
        test: &["-p", "oms-core", "--test", "margin", "a_matching_setting_releases_the_hold_and_link_loss_makes_it_unknown_again"],
        guards: "C-MARGIN / venue setting and account mode",
    },
    Mutant {
        id: "M46-account-mode-survives-a-link-change",
        what: "an account mode told in an earlier link session still counts",
        file: "crates/oms-core/src/core.rs",
        find: "self.margin.account_mode = None;",
        replace: "",
        test: &["-p", "oms-core", "--test", "margin", "an_account_mode_off_the_allow_list_or_unknown_holds_the_account"],
        guards: "C-MARGIN / venue setting and account mode",
    },
    Mutant {
        id: "M47-unified-account-passes-when-listed",
        what: "the unified account mode passes when the allow-list names it",
        file: "crates/oms-core/src/model.rs",
        find: "[\"unifiedAccount\", \"portfolioMargin\"];",
        replace: "[\"\", \"portfolioMargin\"];",
        test: &["-p", "oms-core", "--test", "margin", "unified_or_portfolio_account_mode_holds_the_account"],
        guards: "C-MARGIN / venue setting and account mode",
    },
    Mutant {
        id: "M48-portfolio-margin-passes-when-listed",
        what: "the portfolio margin mode passes when the allow-list names it",
        file: "crates/oms-core/src/model.rs",
        find: "[\"unifiedAccount\", \"portfolioMargin\"];",
        replace: "[\"unifiedAccount\", \"\"];",
        test: &["-p", "oms-core", "--test", "margin", "unified_or_portfolio_account_mode_holds_the_account"],
        guards: "C-MARGIN / venue setting and account mode",
    },
    Mutant {
        id: "M49-account-mode-allow-list-not-read",
        what: "an account mode that the allow-list does not name passes",
        file: "crates/oms-core/src/core.rs",
        find: "listed && !never_allowed",
        replace: "!never_allowed",
        test: &["-p", "oms-core", "--test", "margin", "an_account_mode_off_the_allow_list_or_unknown_holds_the_account"],
        guards: "C-MARGIN / venue setting and account mode",
    },
    Mutant {
        id: "M50-setting-and-mode-holds-refuse-reduce-only",
        what: "the venue-setting and account-mode holds also refuse a reduce-only order",
        file: "crates/oms-core/src/model.rs",
        find: "HoldReason::VenueSetting(_) | HoldReason::AccountMode => true,",
        replace: "HoldReason::VenueSetting(_) | HoldReason::AccountMode => false,",
        test: &["-p", "oms-core", "--test", "margin", "reduce_only_order_passes_the_venue_setting_and_account_mode_holds"],
        guards: "C-MARGIN / hold table",
    },
    Mutant {
        id: "M60-clear-reduce-when-not-reducing",
        what: "clear-reduce is accepted, and re-bases the day, when the account is not REDUCING",
        file: "crates/oms-core/src/decide.rs",
        find: "if nothing_to_clear {",
        replace: "if false {",
        test: &["-p", "oms-core", "--test", "margin", "clear_reduce_is_refused_when_stale_or_without_a_note_or_when_not_reducing"],
        guards: "C-MARGIN / clear and rebase",
    },
    Mutant {
        id: "M61-clear-reduce-on-a-stale-report",
        what: "clear-reduce is accepted on a margin report that is not fresh",
        file: "crates/oms-core/src/decide.rs",
        find: "if !clear_on_a_fresh_report {",
        replace: "if false {",
        test: &["-p", "oms-core", "--test", "margin", "clear_reduce_is_refused_when_stale_or_without_a_note_or_when_not_reducing"],
        guards: "C-MARGIN / clear and rebase",
    },
    Mutant {
        id: "M62-clear-reduce-without-a-note",
        what: "clear-reduce is accepted with an empty note",
        file: "crates/oms-core/src/decide.rs",
        find: "if !clear_has_a_note {",
        replace: "if false {",
        test: &["-p", "oms-core", "--test", "margin", "clear_reduce_is_refused_when_stale_or_without_a_note_or_when_not_reducing"],
        guards: "C-MARGIN / clear and rebase",
    },
    Mutant {
        id: "M63-clear-reduce-keeps-the-old-day-base",
        what: "clear-reduce ends REDUCING without taking the current account value as the day base",
        file: "crates/oms-core/src/decide.rs",
        find: "let Some(new_base) = self.rebase_on_the_report()? else {",
        replace: "let Some(new_base) = self.core.margin.report.as_ref().map(|r| r.account_value) else {",
        test: &["-p", "oms-core", "--test", "margin", "clear_reduce_rebases_the_day_and_a_further_loss_latches_again"],
        guards: "C-MARGIN / clear and rebase",
    },
    Mutant {
        id: "M64-rebase-day-on-a-stale-report",
        what: "rebase-day is accepted on a margin report that is not fresh",
        file: "crates/oms-core/src/decide.rs",
        find: "if !rebase_on_a_fresh_report {",
        replace: "if false {",
        test: &["-p", "oms-core", "--test", "margin", "rebase_day_sets_the_base_and_leaves_the_latch_alone"],
        guards: "C-MARGIN / clear and rebase",
    },
    Mutant {
        id: "M65-rebase-day-without-a-note",
        what: "rebase-day is accepted with an empty note",
        file: "crates/oms-core/src/decide.rs",
        find: "if !rebase_has_a_note {",
        replace: "if false {",
        test: &["-p", "oms-core", "--test", "margin", "rebase_day_sets_the_base_and_leaves_the_latch_alone"],
        guards: "C-MARGIN / clear and rebase",
    },
    Mutant {
        id: "M66-rebase-day-clears-reducing",
        what: "rebase-day also ends REDUCING",
        file: "crates/oms-core/src/decide.rs",
        find: "                    self.rebase_on_the_report()?;\n                    Ok(\"rebased\".into())",
        replace: "                    self.clear_reduce(note)?;\n                    Ok(\"rebased\".into())",
        test: &["-p", "oms-core", "--test", "margin", "rebase_day_sets_the_base_and_leaves_the_latch_alone"],
        guards: "C-MARGIN / clear and rebase",
    },
    Mutant {
        id: "M67-clear-reduce-from-the-strategy-socket",
        what: "clear-reduce is taken from a strategy connection",
        file: "crates/oms-node/src/server.rs",
        find: "Op::ClearReduce { note } => operator_only(s).or_else(|| {",
        replace: "Op::ClearReduce { note } => None::<Response>.or_else(|| {",
        test: &["-p", "oms-node", "--test", "e2e", "strategy_socket_cannot_clear_reducing_or_rebase"],
        guards: "C-MARGIN / clear and rebase (operator socket only)",
    },
    Mutant {
        id: "M68-rebase-day-from-the-strategy-socket",
        what: "rebase-day is taken from a strategy connection",
        file: "crates/oms-node/src/server.rs",
        find: "Op::RebaseDay { note } => operator_only(s).or_else(|| {",
        replace: "Op::RebaseDay { note } => None::<Response>.or_else(|| {",
        test: &["-p", "oms-node", "--test", "e2e", "strategy_socket_cannot_clear_reducing_or_rebase"],
        guards: "C-MARGIN / clear and rebase (operator socket only)",
    },
];

fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for e in std::fs::read_dir(from)? {
        let e = e?;
        let (src, dst) = (e.path(), to.join(e.file_name()));
        if src.is_dir() {
            if e.file_name() == "target" {
                continue;
            }
            copy_tree(&src, &dst)?;
        } else if e.file_type()?.is_file() {
            // Sockets and the like (left behind by a test run) are not source.
            std::fs::copy(&src, &dst)?;
        }
    }
    Ok(())
}

pub fn run(root: &Path, only: Option<&str>) -> i32 {
    // The copy and its build output (several GiB) go next to this build's
    // own output, wherever the build configuration put that: the directory
    // two levels above the running executable is the target directory. The
    // source tree's disk may be the one the journal is measured on.
    let target = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().and_then(|p| p.parent()).map(|p| p.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target"));
    let work = target.join("mutants-work");
    let _ = std::fs::remove_dir_all(work.join("crates"));
    for item in ["crates", "Cargo.toml", "Cargo.lock"] {
        let (src, dst) = (root.join(item), work.join(item));
        let r = if src.is_dir() { copy_tree(&src, &dst) } else { std::fs::create_dir_all(&work).and_then(|()| std::fs::copy(&src, &dst).map(|_| ())) };
        if let Err(e) = r {
            eprintln!("copy {item}: {e}");
            return 2;
        }
    }
    let target_dir = work.join("target");
    let digest = crate::digest::source_digest(root);
    let mut results = Vec::new();
    let mut survived = 0;
    for m in MUTANTS.iter().filter(|m| only.is_none_or(|o| m.id.starts_with(o))) {
        let path = work.join(m.file);
        let original = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("{}: {e}", path.display());
                return 2;
            }
        };
        let n = original.matches(m.find).count();
        if n != 1 {
            eprintln!("{}: the text to mutate occurs {n} times in {}; the list is out of date", m.id, m.file);
            results.push(serde_json::json!({ "id": m.id, "what": m.what, "guards": m.guards, "killed": false, "note": "not applicable: source changed" }));
            survived += 1;
            continue;
        }
        let _ = std::fs::write(&path, original.replacen(m.find, m.replace, 1));
        let t = Instant::now();
        let out = Command::new("cargo")
            .arg("test")
            .args(m.test)
            .current_dir(&work)
            .env("CARGO_TARGET_DIR", &target_dir)
            .env("OMS_SIM_SEEDS", "60")
            .output();
        let _ = std::fs::write(&path, &original);
        let (killed, note) = match out {
            Err(e) => (false, format!("cargo did not run: {e}")),
            Ok(o) => {
                let text = format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
                let compiled = !text.contains("could not compile");
                let ran = text.contains("test result:");
                if !compiled {
                    // A mutant that does not compile proves nothing.
                    (false, "the mutant did not compile".to_string())
                } else if !ran {
                    (false, "no test ran".to_string())
                } else {
                    (!o.status.success(), String::new())
                }
            }
        };
        if !killed {
            survived += 1;
        }
        eprintln!("{} {:<44} {:.0} s  {}", if killed { "killed  " } else { "SURVIVED" }, m.id, t.elapsed().as_secs_f64(), note);
        results.push(serde_json::json!({
            "id": m.id, "what": m.what, "guards": m.guards, "file": m.file,
            "test": format!("cargo test {}", m.test.join(" ")),
            "killed": killed, "note": note,
        }));
    }
    let report = serde_json::json!({
        "source_digest": digest,
        "taken_at_us": oms_api::client::now_us(),
        "mutants": results,
    });
    if only.is_none() {
        let path = root.join("evidence/mutants.json");
        let _ = std::fs::write(&path, serde_json::to_string_pretty(&report).unwrap_or_default());
        eprintln!("written {}", path.display());
    }
    i32::from(survived > 0)
}
