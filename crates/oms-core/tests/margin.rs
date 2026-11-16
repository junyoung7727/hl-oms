//! Margin rules (contract C-MARGIN, ADR-018). Expected values are literals
//! worked out by hand from the contract, not derived from the implementation.
//!
//! The hold-table tests of C-MARGIN sit beside the other hold tests in
//! `scenarios.rs`.

#![allow(clippy::unwrap_used)]

mod common;
use common::*;
use oms_core::*;
use proptest::prelude::*;

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

fn link_down(h: &mut H) {
    h.venue(VenueInput::Link {
        up: false,
        session: h.session,
        reason: "ws closed".into(),
    });
}

/// The account value of the newest recorded report, with its number.
fn recorded(h: &H) -> Option<(u64, u64, Amount)> {
    h.core
        .margin
        .report
        .as_ref()
        .map(|r| (r.session, r.seq, r.account_value))
}

/// An answer that belongs to an earlier link session, or that arrives while
/// the link is down, is dropped: no event, nothing stored.
#[test]
fn margin_report_of_an_old_session_or_with_the_link_down_is_ignored() {
    let mut h = H::ready_with(config_with_margin());
    let old = h.session;
    link_down(&mut h);
    h.link_up_and_sync();
    assert_eq!((old, h.session), (1, 2));

    // The answer to a query of session 1 arrives in session 2.
    let out = h.margin_report(old, 9, h.now, "50");
    assert_eq!(names(&out), Vec::<&str>::new());
    assert_eq!(recorded(&h), None);

    // A report of session 2 is recorded.
    let out = h.margin_report(2, 1, h.now, "100");
    assert_eq!(names(&out), vec!["margin_reported"]);
    assert_eq!(recorded(&h), Some((2, 1, amt("100"))));

    // The link goes down. The answer to a query of session 2 arrives late.
    link_down(&mut h);
    let out = h.margin_report(2, 2, h.now, "50");
    assert_eq!(names(&out), Vec::<&str>::new());
    // The last report stays as it was.
    assert_eq!(recorded(&h), Some((2, 1, amt("100"))));
    // Nothing of the dropped reports reached the journal.
    let journaled = h
        .log
        .iter()
        .filter(|rec| matches!(rec.input, Input::Venue(VenueInput::MarginReport(_))))
        .count();
    assert_eq!(journaled, 1);
}

/// Answers can overtake each other. A report whose number is not greater
/// than that of the last report recorded in this session is dropped: not
/// stored, not judged. The numbers start again with a new link session.
#[test]
fn late_margin_report_with_a_lower_sequence_is_ignored() {
    let mut h = H::ready_with(config_with_margin());
    h.margin_report(1, 5, h.now, "100");
    assert_eq!(recorded(&h), Some((1, 5, amt("100"))));

    // Query 4 was sent before query 5 and is answered after it. Its query
    // time is the older one, but the number alone decides.
    let out = h.margin_report(1, 4, h.now - SEC, "50");
    assert_eq!(names(&out), Vec::<&str>::new());
    assert_eq!(recorded(&h), Some((1, 5, amt("100"))));

    // The same number again (a duplicated answer) is not greater either,
    // also when it claims a later query time.
    let out = h.margin_report(1, 5, h.now + SEC, "60");
    assert_eq!(names(&out), Vec::<&str>::new());
    assert_eq!(recorded(&h), Some((1, 5, amt("100"))));

    // The next number is recorded.
    let out = h.margin_report(1, 6, h.now, "99");
    assert_eq!(names(&out), vec!["margin_reported"]);
    assert_eq!(recorded(&h), Some((1, 6, amt("99"))));

    // A new link session starts its own numbering: number 1 of session 2 is
    // newer than number 6 of session 1.
    link_down(&mut h);
    h.link_up_and_sync();
    let out = h.margin_report(2, 1, h.now, "98");
    assert_eq!(names(&out), vec!["margin_reported"]);
    assert_eq!(recorded(&h), Some((2, 1, amt("98"))));
}

/// S25. A report that cannot be read, or that names a zero maximum leverage,
/// is refused as a whole. The refusal and its reason are journaled. The last
/// good report stays exactly as it was, so its age keeps counting from its
/// own query time.
#[test]
fn invalid_margin_report_is_journaled_and_never_becomes_fresh() {
    let mut h = H::ready_with(config_with_margin());
    let good_at = h.now;
    h.margin_report(1, 1, good_at, "100");
    let good = h.core.margin.report.clone();
    assert_eq!(good.as_ref().map(|r| r.queried_at), Some(good_at));

    // The gateway could not read the answer to query 2.
    h.advance(5 * SEC);
    let unreadable_at = h.now;
    let out = h.venue(VenueInput::MarginReportInvalid {
        session: 1,
        seq: 2,
        queried_at: unreadable_at,
        reason: "marginSummary.accountValue: not a number".into(),
    });
    assert_eq!(
        out.events,
        vec![Event::MarginReportRejected {
            session: 1,
            seq: 2,
            queried_at: unreadable_at,
            reason: "marginSummary.accountValue: not a number".into(),
        }]
    );
    assert_eq!(
        h.core.margin.report, good,
        "an unreadable report replaced the last good one"
    );
    assert_eq!(
        h.core
            .margin
            .last_rejection
            .as_ref()
            .map(|r| (r.seq, r.reason.as_str())),
        Some((2, "marginSummary.accountValue: not a number"))
    );

    // Query 3 is readable but one position has a maximum leverage of zero.
    // The core refuses it itself: the value would divide by zero in the
    // maintenance margin rate.
    h.advance(5 * SEC);
    let zero_at = h.now;
    let out = h.venue(VenueInput::MarginReport(MarginReportIn {
        session: 1,
        seq: 3,
        queried_at: zero_at,
        account_value: amt("70"),
        positions: vec![position("X", 40), position("Y", 0)],
    }));
    assert_eq!(
        out.events,
        vec![Event::MarginReportRejected {
            session: 1,
            seq: 3,
            queried_at: zero_at,
            reason: "position Y: max_leverage is zero".into(),
        }]
    );
    assert_eq!(
        h.core.margin.report, good,
        "a report with a zero maximum leverage was recorded"
    );
    assert_eq!(
        h.core.margin.last_rejection.as_ref().map(|r| r.seq),
        Some(3)
    );

    // Both refusals are in the journal with their reasons, and the rebuilt
    // state shows the same last reason.
    let reasons: Vec<&str> = h
        .log
        .iter()
        .flat_map(|rec| &rec.events)
        .filter_map(|ev| match ev {
            Event::MarginReportRejected { reason, .. } => Some(reason.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        reasons,
        vec![
            "marginSummary.accountValue: not a number",
            "position Y: max_leverage is zero"
        ]
    );
    assert_eq!(h.rebuilt().margin, h.core.margin);

    // A refused report does not block the next good one.
    let out = h.margin_report(1, 4, h.now, "99");
    assert_eq!(names(&out), vec!["margin_reported"]);
    assert_eq!(recorded(&h), Some((1, 4, amt("99"))));
}

// ---- C-MARGIN: freshness ----------------------------------------------------

/// Moves the harness clock so that the next input is recorded at `at`.
fn next_input_at(h: &mut H, at: Ts) {
    assert!(at >= h.now + 1_000, "the record time cannot go backwards");
    h.now = at - 1_000;
}

/// S11. With a margin policy a plain order needs a margin report whose age
/// (record time minus query time) is at most 30 s. Exactly 30 s is fresh;
/// one microsecond more is not.
#[test]
fn plain_order_is_rejected_margin_unavailable_without_a_fresh_report() {
    let mut h = H::ready_with(config_with_margin());

    // No report was ever recorded.
    assert_eq!(
        h.reject("a", "k1", Side::Buy, "1", "100"),
        vec![RejectReason::MarginUnavailable]
    );

    // A report asked for at t0. An order recorded exactly 30 s later is
    // accepted.
    let t0 = h.now;
    h.margin_report(1, 1, t0, "1000");
    next_input_at(&mut h, t0 + 30 * SEC - 1_000);
    h.ref_price("X", "100");
    let id = h.accept("a", "k2", Side::Buy, "1", "100");
    assert_eq!(h.order(id).accepted_at, t0 + 30_000_000);

    // A second report asked for at t1. An order recorded 30 s and one
    // microsecond later is rejected, and the reason is the only one.
    let t1 = h.now;
    h.margin_report(1, 2, t1, "1000");
    next_input_at(&mut h, t1 + 30 * SEC + 1 - 1_000);
    h.ref_price("X", "100");
    assert_eq!(h.now + 1_000, t1 + 30_000_001);
    assert_eq!(
        h.reject("a", "k3", Side::Buy, "1", "100"),
        vec![RejectReason::MarginUnavailable]
    );
    // The refusal is not a hold: it latches nothing.
    assert_eq!(h.core.holds(None), vec![]);

    // A new report makes plain orders acceptable again.
    h.margin_report(1, 3, h.now, "1000");
    h.accept("a", "k4", Side::Buy, "1", "100");
}

/// G5. The wall clock stepped backwards after a query was sent: the report
/// carries a query time after the record time. Its age is negative, and a
/// negative age is not fresh. Age zero is.
#[test]
fn margin_report_dated_after_the_record_time_is_not_fresh() {
    let mut h = H::ready_with(config_with_margin());
    // A reference price that stays fresh for the two refusals below, so
    // that the date of the report is their only reason.
    h.ref_price("X", "100");
    let queried_at = h.now + 10 * SEC;
    let out = h.margin_report(1, 1, queried_at, "1000");
    // The report is recorded: the number decides the order, not the time.
    assert_eq!(names(&out), vec!["margin_reported"]);

    // Ten seconds before its query time.
    assert_eq!(
        h.reject("a", "k1", Side::Buy, "1", "100"),
        vec![RejectReason::MarginUnavailable]
    );
    // One microsecond before its query time.
    next_input_at(&mut h, queried_at - 1);
    assert_eq!(
        h.reject("a", "k2", Side::Buy, "1", "100"),
        vec![RejectReason::MarginUnavailable]
    );

    // A second report, again dated ten seconds ahead. An order recorded
    // exactly at its query time sees age zero and is accepted.
    let queried_at = h.now + 10 * SEC;
    h.margin_report(1, 2, queried_at, "1000");
    next_input_at(&mut h, queried_at - 1_000);
    h.ref_price("X", "100");
    let id = h.accept("a", "k3", Side::Buy, "1", "100");
    assert_eq!(h.order(id).accepted_at, queried_at);
}

/// S11. An order with the venue's reduce-only flag needs no margin report:
/// a strategy can reduce while the venue's queries fail. The same order
/// without the flag is refused.
#[test]
fn reduce_only_order_needs_no_fresh_report() {
    let mut h = H::ready_with(config_with_margin());
    h.report_margin("1000");
    h.ref_price("X", "100");
    h.long("a", "k1", "1");

    // Reports stop. 31 s later the last one is not fresh.
    h.advance(31 * SEC);
    assert_eq!(
        h.reject("a", "k2", Side::Sell, "1", "100"),
        vec![RejectReason::MarginUnavailable]
    );
    let r = h.reduce_only_req("a", "k3", Side::Sell, "1", "100");
    let order_id = match h.new_order(r) {
        NewOrderReply::Accepted { order_id } => order_id,
        other => panic!("a reduce-only order was refused for staleness: {other:?}"),
    };
    assert!(h.order(order_id).reduce_only);
    assert_eq!(h.sends().last(), Some(&order_id));
}

/// S21. Without a margin policy the rules are off: no order is refused for
/// margin, and a margin input of any kind changes nothing and is not
/// journaled.
#[test]
fn without_a_margin_policy_no_margin_check_runs() {
    let mut h = H::ready();
    assert_eq!(h.core.config.policy.margin, None);
    let digest = h.core.digest();
    let journaled = h.log.len();

    let out = h.margin_report(1, 1, h.now, "100");
    assert_eq!(names(&out), Vec::<&str>::new());
    let out = h.venue(VenueInput::MarginReportInvalid {
        session: 1,
        seq: 2,
        queried_at: h.now,
        reason: "unreadable".into(),
    });
    assert_eq!(names(&out), Vec::<&str>::new());
    let out = h.venue(VenueInput::MarginReport(MarginReportIn {
        session: 1,
        seq: 3,
        queried_at: h.now,
        account_value: amt("100"),
        positions: vec![position("X", 0)],
    }));
    assert_eq!(names(&out), Vec::<&str>::new());

    assert!(h.core.margin.is_empty());
    assert_eq!(h.core.digest(), digest, "a margin input changed the state");
    assert_eq!(h.log.len(), journaled, "a margin input was journaled");

    // A plain order is accepted with no report at all, also a long time
    // after the inputs above.
    h.advance(120 * SEC);
    h.accept("a", "k1", Side::Buy, "1", "100");
}

// ---- C-MARGIN: leverage gate ------------------------------------------------

/// The worked numbers of the leverage gate: account value 100, limit 2.0
/// (the default), reference price of X 100. So the account may carry an
/// exposure of 200.
fn gate() -> H {
    let mut h = H::ready_with(config_with_margin());
    h.report_margin("100");
    h.ref_price("X", "100");
    h
}

/// S5. A plain order that would take account exposure above twice the
/// account value is refused with `AccountLeverage`: a buy of 2.01 at 100 is
/// an exposure of 201 against a limit of 200. Nothing latches.
#[test]
fn order_past_twice_the_account_value_is_rejected_account_leverage() {
    let mut h = gate();
    assert_eq!(
        h.reject("a", "k1", Side::Buy, "2.01", "100"),
        vec![RejectReason::AccountLeverage]
    );
    // The smallest step past the limit: 2.001 at 100 is 200.1.
    assert_eq!(
        h.reject("a", "k2", Side::Buy, "2.001", "100"),
        vec![RejectReason::AccountLeverage]
    );
    // The same on the sell side.
    assert_eq!(
        h.reject("a", "k3", Side::Sell, "2.01", "100"),
        vec![RejectReason::AccountLeverage]
    );
    // The refusal is not a hold and reserved nothing.
    assert_eq!(h.core.holds(None), vec![]);
    assert_eq!(h.acct().buy_open, q("0"));
}

/// S5. Exactly at the limit is accepted: a buy of 2.00 at 100 is an exposure
/// of 200 against a limit of 200.
#[test]
fn order_exactly_at_the_limit_is_accepted() {
    let mut h = gate();
    let id = h.accept("a", "k1", Side::Buy, "2", "100");
    assert_eq!(h.order(id).reserved, q("2"));

    // The same on the sell side, in a fresh account.
    let mut h = gate();
    h.accept("a", "k1", Side::Sell, "2", "100");
}

/// S1. An order that does not fill stays as it is. Over two hours of ticks,
/// with a fresh margin report and a fresh reference price at every 5 s poll
/// (so the stale cancel does not apply), the OMS sends no cancel, changes no
/// price and closes nothing. The order keeps its exposure: it still fills
/// the limit, so the smallest further buy is refused.
#[test]
fn an_unfilled_resting_order_keeps_its_exposure_and_is_never_repriced_or_expired_by_the_oms() {
    let mut h = gate();
    let id = h.accept("a", "k1", Side::Buy, "2", "100");
    h.placed_open(id, "v1");
    let resting = h.order(id).clone();
    assert_eq!(resting.phase, Phase::Working);
    let commands_before = h.commands.len();

    let mut seen = std::collections::BTreeSet::new();
    for _ in 0..1440 {
        h.advance(5 * SEC);
        for out in [h.report_margin("100"), h.ref_price("X", "100"), h.tick()] {
            seen.extend(names(&out));
        }
    }
    assert!(h.now - resting.accepted_at > 7_200 * SEC);

    // Nothing was sent to the venue and no event touched the order.
    assert_eq!(h.commands.len(), commands_before);
    assert_eq!(h.cancels(), vec![]);
    assert_eq!(
        seen.into_iter().collect::<Vec<_>>(),
        vec!["margin_reported", "purged", "ref_price_updated"]
    );
    assert_eq!(h.order(id), &resting, "the OMS changed a resting order");
    assert_eq!(h.order(id).px, px("100"));
    assert_eq!(h.order(id).reserved, q("2"));

    // It still counts: 2 at 100 is the whole limit of 200.
    assert_eq!(
        h.reject("a", "k2", Side::Buy, "0.1", "100"),
        vec![RejectReason::AccountLeverage]
    );
}

/// S1. An order counts at the higher of its limit price and the reference
/// price (100 here): a sell far above the market at the price it would
/// really fill at, a buy far below the market at the market.
#[test]
fn resting_order_counts_at_the_higher_of_limit_and_reference() {
    // A sell of 1 at 150 counts 150, not 100.
    let mut h = gate();
    h.accept("a", "k1", Side::Sell, "1", "150");
    // 150 + 0.6 x 100 = 210 is past the limit of 200.
    assert_eq!(
        h.reject("a", "k2", Side::Sell, "0.6", "100"),
        vec![RejectReason::AccountLeverage]
    );
    // 150 + 0.5 x 100 = 200 is at the limit.
    h.accept("a", "k3", Side::Sell, "0.5", "100");

    // A buy at 50 counts at 100, not at 50: 2.2 x 100 = 220.
    let mut h = gate();
    assert_eq!(
        h.reject("a", "k1", Side::Buy, "2.2", "50"),
        vec![RejectReason::AccountLeverage]
    );
    // 2 x 100 = 200 is at the limit, and the resting order keeps counting
    // at 100: a further 0.2 at 50 would make 220.
    h.accept("a", "k2", Side::Buy, "2", "50");
    assert_eq!(
        h.reject("a", "k3", Side::Buy, "0.2", "50"),
        vec![RejectReason::AccountLeverage]
    );
}

/// Strategy `a` sells `qty` at 100 and the venue reports it filled.
fn short(h: &mut H, key: &str, qty: &str) {
    let id = h.accept("a", key, Side::Sell, qty, "100");
    h.placed_open(id, key);
    h.fill(id, key, qty, "100");
    h.status(id, VenueStatus::Filled, Some(qty));
    assert_eq!(h.order(id).phase, Phase::Closed);
}

/// S6. Exposure is the furthest the position can reach. Short 5 with a
/// reduce-only buy of 5 and a plain buy of 8 can end long 8 (-5 + 13): the
/// exposure is 800, not 500 (the short alone) and not 1300 (the buys
/// alone). Account value 400, so the limit is 800.
#[test]
fn reduce_only_and_plain_buy_on_a_short_count_at_full_reach() {
    let mut h = H::ready_with(config_with_margin());
    h.report_margin("400");
    h.ref_price("X", "100");
    short(&mut h, "k1", "5");
    assert_eq!(h.acct().pos, sq("-5"));

    let r = h.reduce_only_req("a", "k2", Side::Buy, "5", "100");
    assert!(matches!(h.new_order(r), NewOrderReply::Accepted { .. }));
    // Counted as 13 x 100 the plain buy would be refused; it reaches 800,
    // which is at the limit.
    h.accept("a", "k3", Side::Buy, "8", "100");
    assert_eq!((h.acct().pos, h.acct().buy_open), (sq("-5"), q("13")));

    // The reach is 8, so the limit is used up: 0.1 more would make 810.
    // Counted as 5 (the short alone) this buy would pass.
    assert_eq!(
        h.reject("a", "k4", Side::Buy, "0.1", "100"),
        vec![RejectReason::AccountLeverage]
    );
    // The short side still reaches only 5: a sell of 3 makes 800.
    h.accept("a", "k5", Side::Sell, "3", "100");
    assert_eq!(
        h.reject("a", "k6", Side::Sell, "0.1", "100"),
        vec![RejectReason::AccountLeverage]
    );
}

/// `config_with_margin()` with a second instrument Y that has the limits of
/// X.
fn config_xy() -> CoreConfig {
    let mut cfg = config_with_margin();
    let spec = cfg.instruments[&iid("X")].clone();
    cfg.instruments.insert(iid("Y"), spec);
    for strategy in cfg.strategies.values_mut() {
        let limit = strategy.limits[&iid("X")].clone();
        strategy.limits.insert(iid("Y"), limit);
    }
    cfg
}

/// A plain request of strategy `a` on `instrument`.
fn req_on(h: &H, instrument: &str, key: &str, side: Side, qty: &str, price: &str) -> NewOrderReq {
    NewOrderReq {
        instrument: iid(instrument),
        ..h.req("a", key, side, qty, price)
    }
}

fn accepted(reply: NewOrderReply) -> OrderId {
    match reply {
        NewOrderReply::Accepted { order_id } => order_id,
        other => panic!("expected acceptance, got {other:?}"),
    }
}

fn rejected(reply: NewOrderReply) -> Vec<RejectReason> {
    match reply {
        NewOrderReply::Rejected { reasons } => reasons,
        other => panic!("expected rejection, got {other:?}"),
    }
}

/// The account exposure the core publishes, as a decimal of quote units.
fn exposure(h: &H) -> String {
    h.core
        .account_exposure()
        .expect("exposure is known")
        .to_string()
}

/// S5. Account exposure is the sum over instruments: two instruments cannot
/// each use the whole limit. With account value 100, 1.5x on X and 1.5x on Y
/// is 3x.
#[test]
fn exposure_is_the_sum_over_instruments() {
    let mut h = H::ready_with(config_xy());
    h.report_margin("100");
    h.ref_price("X", "100");
    h.ref_price("Y", "100");
    assert_eq!(exposure(&h), "0");

    let r = req_on(&h, "X", "k1", Side::Buy, "1.5", "100");
    accepted(h.new_order(r));
    assert_eq!(exposure(&h), "150");
    // 150 on X and 150 on Y is 300.
    let r = req_on(&h, "Y", "k2", Side::Buy, "1.5", "100");
    assert_eq!(
        rejected(h.new_order(r)),
        vec![RejectReason::AccountLeverage]
    );
    // A short on Y does not offset the long on X either.
    let r = req_on(&h, "Y", "k3", Side::Sell, "1.5", "100");
    assert_eq!(
        rejected(h.new_order(r)),
        vec![RejectReason::AccountLeverage]
    );
    assert_eq!(exposure(&h), "150");

    // 150 + 50 is at the limit.
    let r = req_on(&h, "Y", "k4", Side::Sell, "0.5", "100");
    accepted(h.new_order(r));
    assert_eq!(exposure(&h), "200");
    // Now neither instrument has room.
    let r = req_on(&h, "X", "k5", Side::Buy, "0.1", "100");
    assert_eq!(
        rejected(h.new_order(r)),
        vec![RejectReason::AccountLeverage]
    );
    let r = req_on(&h, "Y", "k6", Side::Sell, "0.1", "100");
    assert_eq!(
        rejected(h.new_order(r)),
        vec![RejectReason::AccountLeverage]
    );
}

/// S2. A cancel request frees nothing, and neither does the venue's
/// confirmation of the request. Exposure falls when terminal evidence
/// releases the reservation. Buy 2 at 100, 0.5 filled: position 50 plus the
/// resting 1.5 is still 200.
#[test]
fn partial_fill_then_cancel_frees_exposure_only_on_terminal_evidence() {
    let mut h = gate();
    let id = h.accept("a", "k1", Side::Buy, "2", "100");
    h.placed_open(id, "v1");
    h.fill(id, "t1", "0.5", "100");
    assert_eq!(exposure(&h), "200");

    assert_eq!(h.cancel("a", id), CancelReply::Requested { order_id: id });
    assert_eq!(exposure(&h), "200");
    assert_eq!(
        h.reject("a", "k2", Side::Buy, "0.1", "100"),
        vec![RejectReason::AccountLeverage]
    );

    // The venue confirms the cancel request. That is not terminal evidence.
    h.venue(VenueInput::CancelResult {
        order_id: id,
        attempt: 1,
        outcome: CancelOutcome::Confirmed,
        timing: Default::default(),
    });
    assert_eq!(exposure(&h), "200");
    assert_eq!(
        h.reject("a", "k3", Side::Buy, "0.1", "100"),
        vec![RejectReason::AccountLeverage]
    );

    // Terminal evidence: cancelled with 0.5 filled. The position of 0.5
    // stays, the resting 1.5 is gone.
    h.status(
        id,
        VenueStatus::Canceled {
            reason: "user".into(),
        },
        Some("0.5"),
    );
    assert_eq!(h.order(id).phase, Phase::Closed);
    assert_eq!(exposure(&h), "50");
    h.accept("a", "k4", Side::Buy, "1.5", "100");
    assert_eq!(exposure(&h), "200");
}

/// S20. An order the gate accepted and the venue then refuses stops
/// counting: a venue refusal does not use up the limit. The venue's reason
/// is kept as text and never read: whatever it says, the result is the
/// same, and nothing is held.
#[test]
fn a_venue_rejection_of_an_accepted_order_frees_its_exposure() {
    for reason in [
        "Insufficient margin to place order",
        "reduceOnlyCanceled",
        "",
    ] {
        // Refused in the answer to the send.
        let mut h = gate();
        let id = h.accept("a", "k1", Side::Buy, "2", "100");
        assert_eq!(exposure(&h), "200");
        assert_eq!(
            h.reject("a", "k2", Side::Buy, "0.1", "100"),
            vec![RejectReason::AccountLeverage]
        );
        h.venue(VenueInput::SendResult {
            order_id: id,
            outcome: SendOutcome::Rejected {
                reason: reason.into(),
            },
            timing: Default::default(),
        });
        assert_eq!(
            h.order(id).close.as_ref().map(|c| &c.reason),
            Some(&CloseReason::RejectedByVenue {
                reason: reason.into()
            })
        );
        assert_eq!(exposure(&h), "0", "venue reason {reason:?}");
        assert_eq!(h.core.holds(None), vec![], "venue reason {reason:?}");
        h.accept("a", "k3", Side::Buy, "2", "100");

        // Refused by a status after the order was acknowledged.
        let mut h = gate();
        let id = h.accept("a", "k1", Side::Sell, "2", "100");
        h.placed_open(id, "v1");
        assert_eq!(exposure(&h), "200");
        h.status(
            id,
            VenueStatus::Rejected {
                reason: reason.into(),
            },
            None,
        );
        assert_eq!(h.order(id).phase, Phase::Closed);
        assert_eq!(exposure(&h), "0", "venue reason {reason:?}");
        assert_eq!(h.core.holds(None), vec![], "venue reason {reason:?}");
        h.accept("a", "k2", Side::Sell, "2", "100");
    }
}

/// S7. The gate looks only at new orders. A price move that takes the
/// account above the limit (long 2, price 100 to 150: exposure 300 against
/// a limit of 200) latches nothing and cancels nothing. A plain order that
/// does not raise exposure passes; one that raises it is refused.
#[test]
fn after_a_passive_breach_a_plain_reducing_order_passes_and_an_increasing_one_does_not() {
    let mut h = gate();
    h.long("a", "k1", "2");
    assert_eq!(exposure(&h), "200");

    let commands_before = h.commands.len();
    let out = h.ref_price("X", "150");
    assert_eq!(names(&out), vec!["ref_price_updated"]);
    assert_eq!(exposure(&h), "300");
    assert_eq!(h.core.holds(None), vec![]);
    assert_eq!(h.commands.len(), commands_before);

    // A plain sell of 1 can only lower the position: exposure stays 300.
    let sell = h.accept("a", "k2", Side::Sell, "1", "150");
    assert!(!h.order(sell).reduce_only);
    assert_eq!(exposure(&h), "300");
    // A plain buy raises it to 315.
    assert_eq!(
        h.reject("a", "k3", Side::Buy, "0.1", "150"),
        vec![RejectReason::AccountLeverage]
    );
    // A plain sell large enough to turn the position raises it too: with
    // the resting sell of 1 the short side would reach 6 x 150 - 300 = 600.
    assert_eq!(
        h.reject("a", "k4", Side::Sell, "5", "150"),
        vec![RejectReason::AccountLeverage]
    );

    // The sell fills and the price comes back: long 1 at 100. Nothing was
    // latched, so a buy up to the limit is accepted again.
    h.placed_open(sell, "v2");
    h.fill(sell, "t2", "1", "150");
    h.ref_price("X", "100");
    assert_eq!(exposure(&h), "100");
    assert_eq!(h.core.holds(None), vec![]);
    h.accept("a", "k5", Side::Buy, "1", "100");
    assert_eq!(exposure(&h), "200");
}

/// S7. An order with the venue's reduce-only flag is not judged by the
/// gate, also when its size would count as more exposure (long 2 at a price
/// of 150 is 300; a reduce-only sell of 5 reaches 5 x 150 - 300 = 450). It
/// still counts for the plain orders that follow.
#[test]
fn reduce_only_order_skips_the_leverage_gate() {
    let mut h = gate();
    h.long("a", "k1", "2");
    h.ref_price("X", "150");
    assert_eq!(exposure(&h), "300");

    // The same order without the flag is refused.
    assert_eq!(
        h.reject("a", "k2", Side::Sell, "5", "150"),
        vec![RejectReason::AccountLeverage]
    );
    let r = h.reduce_only_req("a", "k3", Side::Sell, "5", "150");
    let id = accepted(h.new_order(r));
    assert!(h.order(id).reduce_only);
    assert_eq!(exposure(&h), "450");

    // It is in the sum: a plain sell of 0.1 would raise 450 to 465. Left
    // out of the sum, that sell would read as 300 before and after.
    assert_eq!(
        h.reject("a", "k4", Side::Sell, "0.1", "150"),
        vec![RejectReason::AccountLeverage]
    );
}

/// Exposure on an instrument cannot be valued without a fresh reference
/// price (15 s, the existing limit of the price band). It is then not
/// counted as zero: a plain order is refused with `MarginUnavailable`,
/// whichever instrument it is for. An instrument with no position and no
/// order needs no price.
#[test]
fn plain_order_is_rejected_when_an_exposed_instrument_has_no_fresh_reference_price() {
    let mut h = H::ready_with(config_xy());
    h.report_margin("100");

    // No reference price for X was ever recorded: the order itself would
    // be exposure on X.
    let r = req_on(&h, "X", "k1", Side::Buy, "1", "100");
    assert_eq!(
        rejected(h.new_order(r)),
        vec![RejectReason::MarginUnavailable]
    );

    // With a price for X the order is accepted. Y has no price and no
    // exposure, and is not asked about.
    h.ref_price("X", "100");
    let r = req_on(&h, "X", "k2", Side::Buy, "0.5", "100");
    accepted(h.new_order(r));
    assert_eq!(exposure(&h), "50");

    // An order rests on Y, valued with a price recorded at t0.
    h.ref_price("Y", "100");
    let t0 = h.now;
    let r = req_on(&h, "Y", "k3", Side::Buy, "0.5", "100");
    accepted(h.new_order(r));

    // Exactly 15 s after t0 the price of Y is still fresh: a plain order on
    // X is accepted. (The report and the price of X are kept fresh.)
    next_input_at(&mut h, t0 + 15 * SEC - 2_000);
    h.report_margin("100");
    h.ref_price("X", "100");
    assert_eq!(h.now, t0 + 15 * SEC - 1_000);
    let r = req_on(&h, "X", "k4", Side::Buy, "0.1", "100");
    let id = accepted(h.new_order(r));
    assert_eq!(h.order(id).accepted_at, t0 + 15_000_000);

    // A new price of Y at t1. 15 s and one microsecond after t1 it is not
    // fresh. The 50 resting on Y cannot be valued, so the order on X is
    // refused although X has a fresh price.
    h.ref_price("Y", "100");
    let t1 = h.now;
    next_input_at(&mut h, t1 + 15 * SEC + 1 - 2_000);
    h.report_margin("100");
    h.ref_price("X", "100");
    assert_eq!(h.now + 1_000, t1 + 15_000_001);
    let r = req_on(&h, "X", "k5", Side::Buy, "0.1", "100");
    assert_eq!(
        rejected(h.new_order(r)),
        vec![RejectReason::MarginUnavailable]
    );
    // Not a hold, and a reduce-only order is not stopped by it.
    assert_eq!(h.core.holds(None), vec![]);
    let r = NewOrderReq {
        reduce_only: true,
        ..req_on(&h, "X", "k6", Side::Sell, "0.1", "100")
    };
    accepted(h.new_order(r));

    // A new price for Y ends it.
    h.ref_price("Y", "100");
    let r = req_on(&h, "X", "k7", Side::Buy, "0.1", "100");
    accepted(h.new_order(r));
}

/// An account value of zero or below carries no exposure at all: every
/// plain order is refused, also one that would not raise exposure. A
/// reduce-only order is still accepted, and nothing latches.
#[test]
fn account_value_zero_or_negative_rejects() {
    for value in ["0", "-0.00000001", "-5"] {
        let mut h = gate();
        h.long("a", "k1", "2");
        h.report_margin(value);
        assert_eq!(
            h.reject("a", "k2", Side::Buy, "0.1", "100"),
            vec![RejectReason::AccountLeverage],
            "account value {value}"
        );
        // A plain sell against the long does not raise exposure. It is
        // refused all the same.
        assert_eq!(
            h.reject("a", "k3", Side::Sell, "1", "100"),
            vec![RejectReason::AccountLeverage],
            "account value {value}"
        );
        let r = h.reduce_only_req("a", "k4", Side::Sell, "1", "100");
        accepted(h.new_order(r));

        // The next report is positive again: the plain sell is accepted.
        assert_eq!(h.core.holds(None), vec![]);
        h.report_margin("100");
        h.accept("a", "k5", Side::Sell, "1", "100");
    }
    // The smallest positive value is not "zero or below": an order that
    // does not raise exposure passes.
    let mut h = gate();
    h.long("a", "k1", "2");
    h.report_margin("0.00000001");
    h.accept("a", "k2", Side::Sell, "1", "100");
}

// ---- C-MARGIN: leverage gate, properties (G3) ---------------------------------

/// One generated step of an account's life on the instruments X and Y.
#[derive(Clone, Debug)]
enum Step {
    /// A new order of strategy `a`. `qty` is in tenths, `px` an index into
    /// `PRICES`.
    Order {
        on_y: bool,
        buy: bool,
        qty: u8,
        px: usize,
        reduce_only: bool,
    },
    /// The venue fills up to `qty` tenths of the `nth` open order.
    Fill { nth: usize, qty: u8 },
    /// A new reference price.
    Price { on_y: bool, px: usize },
    /// A new margin report with this account value.
    Value(u16),
}

const PRICES: [&str; 5] = ["50", "99.5", "100", "100.5", "150"];

fn tenths(n: u8) -> String {
    format!("{}.{}", n / 10, n % 10)
}

fn steps() -> impl Strategy<Value = Vec<Step>> {
    let order = (
        any::<bool>(),
        any::<bool>(),
        1u8..=40,
        0..PRICES.len(),
        any::<bool>(),
    )
        .prop_map(|(on_y, buy, qty, px, flag)| Step::Order {
            on_y,
            buy,
            qty,
            px,
            // One order in four carries the reduce-only flag.
            reduce_only: flag && qty % 2 == 0,
        });
    let fill = (0usize..8, 1u8..=40).prop_map(|(nth, qty)| Step::Fill { nth, qty });
    let price = (any::<bool>(), 0..PRICES.len()).prop_map(|(on_y, px)| Step::Price { on_y, px });
    let value = (0u16..=1500).prop_map(Step::Value);
    prop::collection::vec(
        prop_oneof![6 => order, 3 => fill, 1 => price, 1 => value],
        1..40,
    )
}

/// What one step did: the published exposure before and after it, and the
/// decision if the step was an order (`None` for an accepted order).
#[derive(Debug, PartialEq)]
struct Trace {
    before: Exposure,
    after: Exposure,
    order: Option<Option<Vec<RejectReason>>>,
}

/// Plays the steps on a fresh account (account value 500, both reference
/// prices 100). With `mirrored`, every buy is a sell and every sell a buy,
/// so every position has the opposite sign.
fn play(steps: &[Step], mirrored: bool) -> Vec<Trace> {
    let mut h = H::ready_with(config_xy());
    h.report_margin("500");
    h.ref_price("X", "100");
    h.ref_price("Y", "100");
    let name = |on_y: bool| if on_y { "Y" } else { "X" };
    let mut out = Vec::new();
    for (n, step) in steps.iter().enumerate() {
        let before = h.core.account_exposure().expect("exposure is known");
        let mut order = None;
        match step {
            Step::Order {
                on_y,
                buy,
                qty,
                px,
                reduce_only,
            } => {
                let side = if *buy != mirrored {
                    Side::Buy
                } else {
                    Side::Sell
                };
                let key = format!("k{n}");
                let r = NewOrderReq {
                    reduce_only: *reduce_only,
                    ..req_on(&h, name(*on_y), &key, side, &tenths(*qty), PRICES[*px])
                };
                order = Some(match h.new_order(r) {
                    NewOrderReply::Accepted { order_id } => {
                        h.placed_open(order_id, &key);
                        None
                    }
                    NewOrderReply::Rejected { reasons } => Some(reasons),
                    other => panic!("unexpected reply {other:?}"),
                });
            }
            Step::Fill { nth, qty } => {
                let open: Vec<OrderId> = h.core.open.iter().copied().collect();
                if !open.is_empty() {
                    let o = h.order(open[nth % open.len()]).clone();
                    let part = q(&tenths(*qty)).min(o.remaining());
                    h.fill(o.id, &format!("t{n}"), &part.to_string(), &o.px.to_string());
                }
            }
            Step::Price { on_y, px } => {
                h.ref_price(name(*on_y), PRICES[*px]);
            }
            Step::Value(v) => {
                h.report_margin(&v.to_string());
            }
        }
        let after = h.core.account_exposure().expect("exposure is known");
        out.push(Trace {
            before,
            after,
            order,
        });
    }
    out
}

proptest! {
    /// G3. Whatever the account looks like, an order that is accepted (plain
    /// or reduce-only) never lowers the published account exposure.
    #[test]
    fn adding_an_order_never_lowers_exposure(steps in steps()) {
        for (n, t) in play(&steps, false).iter().enumerate() {
            if t.order == Some(None) {
                prop_assert!(t.after >= t.before, "step {} ({:?}) lowered exposure from {} to {}", n, steps[n], t.before, t.after);
            } else if t.order.is_some() {
                // A refused order changes nothing.
                prop_assert_eq!(t.after, t.before);
            }
        }
    }

    /// G3. Long and short are judged alike: the same run with every side
    /// swapped, and so every position with the opposite sign, publishes the
    /// same exposure after every step and decides every order the same way.
    #[test]
    fn mirrored_books_have_equal_exposure_and_equal_decisions(steps in steps()) {
        let (plain, mirrored) = (play(&steps, false), play(&steps, true));
        for (n, (a, b)) in plain.iter().zip(&mirrored).enumerate() {
            prop_assert_eq!(a, b, "step {} ({:?}) differs in the mirrored run", n, &steps[n]);
        }
    }
}

/// Instruments X, Y and Z with every limit as wide as the number types
/// allow, so that only the leverage gate judges an order.
fn config_extreme() -> CoreConfig {
    let mut cfg = config_with_margin();
    let spec = InstrumentSpec {
        qty_step: Qty(1),
        px_tick: Px(1),
        px_max_sig_figs: None,
        min_notional: Amount(0),
        max_order_qty: Qty(i64::MAX),
        max_order_notional: Amount(i64::MAX),
        price_band_bps: None,
        account_pos_min: SQty(-i64::MAX),
        account_pos_max: SQty(i64::MAX),
    };
    let limit = PosLimit {
        min: SQty(-i64::MAX),
        max: SQty(i64::MAX),
    };
    for name in ["X", "Y", "Z"] {
        cfg.instruments.insert(iid(name), spec.clone());
        for strategy in cfg.strategies.values_mut() {
            strategy.limits.insert(iid(name), limit.clone());
        }
    }
    cfg
}

/// The decision on one order in a fresh account with this account value,
/// with the reference price at the order's price. `None` is an acceptance.
fn decide_alone(
    side: Side,
    qty: &str,
    price: &str,
    account_value: &str,
) -> Option<Vec<RejectReason>> {
    let mut h = H::ready_with(config_extreme());
    h.report_margin(account_value);
    h.ref_price("X", price);
    let r = req_on(&h, "X", "k1", side, qty, price);
    match h.new_order(r) {
        NewOrderReply::Accepted { .. } => None,
        NewOrderReply::Rejected { reasons } => Some(reasons),
        other => panic!("unexpected reply {other:?}"),
    }
}

/// G3. The gate compares exact products. Near the limit no rounding flips a
/// decision, at the largest prices and quantities nothing overflows into an
/// acceptance, and a sum that does not fit is refused, never wrapped.
///
/// The limit is 2.0, so an order of quantity q at price p is accepted alone
/// exactly when p x q <= 2 x account value. Values below are decimal
/// literals worked out by hand; the generated part compares against its own
/// i128 product of the raw 1e-8 integers.
#[test]
fn exposure_is_exact_at_extreme_price_and_quantity() {
    let leverage = Some(vec![RejectReason::AccountLeverage]);

    // 90 000 000 000 x 1 = 2 x 45 000 000 000.
    assert_eq!(
        decide_alone(Side::Buy, "1", "90000000000", "45000000000"),
        None
    );
    assert_eq!(
        decide_alone(Side::Buy, "1", "90000000000", "44999999999.99999999"),
        leverage
    );
    assert_eq!(
        decide_alone(Side::Sell, "1", "90000000000", "45000000000"),
        None
    );
    assert_eq!(
        decide_alone(Side::Sell, "1", "90000000000", "44999999999.99999999"),
        leverage
    );
    // One quantity step more is 900 more: 2 x 45 000 000 450.
    assert_eq!(
        decide_alone(Side::Buy, "1.00000001", "90000000000", "45000000450"),
        None
    );
    assert_eq!(
        decide_alone(
            Side::Buy,
            "1.00000001",
            "90000000000",
            "45000000449.99999999"
        ),
        leverage
    );
    // The largest quantity at price 1: 92 233 720 368.54775807. Half of it
    // is 46 116 860 184.273879035, which is not a multiple of 1e-8: the
    // value just below refuses, the value just above accepts.
    assert_eq!(
        decide_alone(
            Side::Buy,
            "92233720368.54775807",
            "1",
            "46116860184.27387904"
        ),
        None
    );
    assert_eq!(
        decide_alone(
            Side::Buy,
            "92233720368.54775807",
            "1",
            "46116860184.27387903"
        ),
        leverage
    );
    // Products below 1e-8 quote: 0.00000001 x 4.00000003 = 4.00000003e-8,
    // just above 2 x 0.00000002. Rounded to 1e-8 it would equal the limit.
    assert_eq!(
        decide_alone(Side::Buy, "4.00000003", "0.00000001", "0.00000002"),
        leverage
    );
    assert_eq!(
        decide_alone(Side::Buy, "4.00000003", "0.00000001", "0.00000003"),
        None
    );
    assert_eq!(
        decide_alone(Side::Buy, "4", "0.00000001", "0.00000002"),
        None
    );

    // The largest quantity rests on X and on Y at 0.5, and 10 000 on Z,
    // with the largest account value: 2 x 46 116 860 184.273879035 + 5 000
    // is inside the limit.
    let max = "92233720368.54775807";
    let mut h = H::ready_with(config_extreme());
    h.report_margin(max);
    for (n, (name, qty)) in [("X", max), ("Y", max), ("Z", "10000")]
        .into_iter()
        .enumerate()
    {
        h.ref_price(name, "0.5");
        let r = req_on(&h, name, &format!("k{n}"), Side::Buy, qty, "0.5");
        accepted(h.new_order(r));
    }
    assert_eq!(exposure(&h), "92233725368.54775807");
    // The price of X goes to the largest price. The order on X is now
    // worth (2^63 - 1)^2 x 1e-16; the published value is exact (worked
    // out outside this code: M^2 + M x 5e7 + 5e19 with M = 2^63 - 1).
    h.ref_price("X", max);
    assert_eq!(exposure(&h), "8507059173069578449923.9646574582501249");
    // Far above the limit: the smallest plain order is refused for
    // leverage, not accepted by a wrapped number.
    let r = req_on(&h, "Z", "k3", Side::Buy, "0.00000001", "0.5");
    assert_eq!(
        rejected(h.new_order(r)),
        vec![RejectReason::AccountLeverage]
    );
    // The price of Y goes there too. The sum no longer fits in an i128: the
    // exposure is unknown and a plain order is refused with `Overflow`.
    h.ref_price("Y", max);
    assert_eq!(h.core.account_exposure(), None);
    let r = req_on(&h, "Z", "k4", Side::Buy, "0.00000001", "0.5");
    assert_eq!(rejected(h.new_order(r)), vec![RejectReason::Overflow]);
    // A reduce-only order is not judged by the gate.
    let r = NewOrderReq {
        reduce_only: true,
        ..req_on(&h, "Z", "k5", Side::Sell, "0.00000001", "0.5")
    };
    accepted(h.new_order(r));

    // Generated: any price and quantity whose order notional fits. The
    // boundary account value comes from the test's own i128 product.
    let notional_cap = i128::from(i64::MAX) * 100_000_000;
    let raw_px = prop_oneof![1i64..=1_000, 1i64..=10_000_000_000, 1i64..=i64::MAX];
    let cases = raw_px.prop_flat_map(move |px| {
        let max_qty = i64::try_from(notional_cap / i128::from(px)).unwrap_or(i64::MAX);
        // Half of the cases land exactly on a multiple of the limit factor
        // when the range allows it.
        let on_grid = (1..=(max_qty / 200_000_000).max(1))
            .prop_map(move |k| k.saturating_mul(200_000_000).min(max_qty));
        (Just(px), prop_oneof![1..=max_qty, on_grid])
    });
    let mut runner = proptest::test_runner::TestRunner::default();
    let result = runner.run(&cases, |(px, qty)| {
        let product = i128::from(px) * i128::from(qty);
        let (floor, exact) = (product / 200_000_000, product % 200_000_000 == 0);
        let (refused_at, accepted_at) = if exact {
            (floor - 1, floor)
        } else {
            (floor, floor + 1)
        };
        let amount = |raw: i128| Amount(i64::try_from(raw).expect("fits")).to_string();
        let (qty, px) = (Qty(qty).to_string(), Px(px).to_string());
        prop_assert_eq!(
            decide_alone(Side::Buy, &qty, &px, &amount(refused_at)),
            Some(vec![RejectReason::AccountLeverage]),
            "{} at {} with account value {}",
            &qty,
            &px,
            amount(refused_at)
        );
        prop_assert_eq!(
            decide_alone(Side::Sell, &qty, &px, &amount(accepted_at)),
            None,
            "{} at {} with account value {}",
            &qty,
            &px,
            amount(accepted_at)
        );
        Ok(())
    });
    if let Err(e) = result {
        panic!("{e}");
    }
}

// ---- C-MARGIN: replay ---------------------------------------------------------

/// S13. Margin state comes only from journal events. A core rebuilt from the
/// journal, a core that re-decides the recorded inputs, and a core read back
/// from its serialised form all equal the live one, digest included.
#[test]
fn margin_events_replay_to_the_same_state() {
    let mut h = H::ready_with(config_with_margin());
    h.margin_report(1, 1, h.now, "100");
    h.ref_price("X", "100");
    h.accept("a", "k1", Side::Buy, "1", "100");
    h.venue(VenueInput::MarginReportInvalid {
        session: 1,
        seq: 2,
        queried_at: h.now,
        reason: "unreadable".into(),
    });
    h.margin_report(1, 3, h.now, "97.5");
    h.margin_report(1, 2, h.now, "50"); // late: dropped, not journaled
    link_down(&mut h);
    h.link_up_and_sync();
    let last_at = h.now;
    h.venue(VenueInput::MarginReport(MarginReportIn {
        session: 2,
        seq: 1,
        queried_at: last_at,
        account_value: amt("96"),
        positions: vec![position("X", 40)],
    }));
    h.advance(31 * SEC);
    h.reject("a", "k2", Side::Buy, "1", "100");

    // Rebuilt from the events alone, as recovery does.
    let rebuilt = h.rebuilt();
    assert_eq!(rebuilt.digest(), h.core.digest());
    assert_eq!(rebuilt, h.core);
    // The comparison is about something: the rebuilt state holds the last
    // report and the last refusal.
    let report = rebuilt.margin.report.as_ref().unwrap();
    assert_eq!(
        (
            report.session,
            report.seq,
            report.queried_at,
            report.account_value
        ),
        (2, 1, last_at, amt("96"))
    );
    assert_eq!(report.positions, vec![position("X", 40)]);
    assert_eq!(
        rebuilt
            .margin
            .last_rejection
            .as_ref()
            .map(|r| (r.seq, r.reason.as_str())),
        Some((2, "unreadable"))
    );
    // The rebuilt core judges freshness as the live one does.
    assert!(rebuilt.margin_report_is_fresh(last_at + 30 * SEC));
    assert!(!rebuilt.margin_report_is_fresh(last_at + 30 * SEC + 1));

    // Re-deciding every recorded input gives the recorded events.
    let mut c = Core::new(7);
    for rec in &h.log {
        let out = c.handle(rec.at, &rec.input).unwrap();
        assert_eq!(
            out.events, rec.events,
            "record {} re-decided differently",
            rec.seq
        );
    }
    assert_eq!(c.digest(), h.core.digest());

    // The serialised forms (snapshot, journal) read back unchanged.
    let back: Core = serde_json::from_str(&serde_json::to_string(&h.core).unwrap()).unwrap();
    assert_eq!(back, h.core);
    let recs: Vec<Record> = serde_json::from_str(&serde_json::to_string(&h.log).unwrap()).unwrap();
    assert_eq!(recs, h.log);
}

// ---- C-MARGIN: venue setting and account mode -------------------------------

/// S17. With a margin policy the OMS must know, in the current link session,
/// that the instrument has the margin mode and the leverage the policy asks
/// for (isolated, 2). Until the venue has told it, a plain order on that
/// instrument is refused. An answer to a query of an earlier link session
/// tells nothing about this one.
#[test]
fn instrument_with_unknown_venue_setting_holds_plain_orders() {
    let mut h = H::configured(config_with_margin());
    h.link_up_and_sync_only();
    h.tell_account_mode();
    h.report_margin("1000");
    h.ref_price("X", "100");

    // Everything else is in order: the setting is the only reason.
    assert_eq!(
        h.reject("a", "k1", Side::Buy, "1", "100"),
        vec![RejectReason::Hold(HoldReason::VenueSetting(iid("X")))]
    );

    // A late answer that belongs to link session 0 is dropped.
    let out = h.venue(VenueInput::VenueSetting {
        session: 0,
        instrument: iid("X"),
        isolated: true,
        leverage: 2,
    });
    assert_eq!(names(&out), Vec::<&str>::new());
    assert_eq!(
        h.reject("a", "k1", Side::Buy, "1", "100"),
        vec![RejectReason::Hold(HoldReason::VenueSetting(iid("X")))]
    );

    // The answer of this session is recorded as it came. The OMS only
    // reads the setting: nothing is sent to the venue.
    let out = h.venue_setting("X", true, 2);
    assert_eq!(
        out.events,
        vec![Event::VenueSettingNoted {
            session: 1,
            instrument: iid("X"),
            isolated: true,
            leverage: 2,
        }]
    );
    assert_eq!(out.commands, vec![]);
    h.accept("a", "k1", Side::Buy, "1", "100");
    assert_eq!(h.commands.len(), 1, "only the order itself went to the venue");
}

fn venue_setting_hold(instrument: &str) -> Vec<RejectReason> {
    vec![RejectReason::Hold(HoldReason::VenueSetting(iid(instrument)))]
}

fn submit(h: &mut H, r: NewOrderReq) -> Result<OrderId, Vec<RejectReason>> {
    match h.new_order(r) {
        NewOrderReply::Accepted { order_id } => Ok(order_id),
        NewOrderReply::Rejected { reasons } => Err(reasons),
        other => panic!("unexpected reply {other:?}"),
    }
}

/// S17. The policy asks for isolated margin at leverage 2. An instrument the
/// venue has on cross margin, or at another leverage, refuses plain orders.
/// The hold is about that instrument alone: the other instrument trades.
#[test]
fn a_setting_that_differs_from_config_holds_only_that_instrument() {
    let mut h = H::ready_with(config_xy());
    h.report_margin("1000");
    h.ref_price("X", "100");
    h.ref_price("Y", "100");

    // Cross margin at the right leverage.
    h.venue_setting("X", false, 2);
    let r = req_on(&h, "X", "x1", Side::Buy, "1", "100");
    assert_eq!(submit(&mut h, r), Err(venue_setting_hold("X")));
    let r = req_on(&h, "Y", "y1", Side::Buy, "1", "100");
    assert!(submit(&mut h, r).is_ok(), "the hold on X also held Y");

    // Isolated margin at leverage 3, and at leverage 1: both differ from 2.
    for leverage in [3, 1] {
        h.venue_setting("X", true, leverage);
        let r = req_on(&h, "X", "x1", Side::Buy, "1", "100");
        assert_eq!(submit(&mut h, r), Err(venue_setting_hold("X")), "leverage {leverage}");
    }
    let r = req_on(&h, "Y", "y2", Side::Buy, "1", "100");
    assert!(submit(&mut h, r).is_ok(), "the hold on X also held Y");

    // Now Y differs and X is as configured: the hold moves with the fact.
    h.venue_setting("X", true, 2);
    h.venue_setting("Y", false, 3);
    let r = req_on(&h, "X", "x1", Side::Buy, "1", "100");
    assert!(submit(&mut h, r).is_ok());
    let r = req_on(&h, "Y", "y3", Side::Buy, "1", "100");
    assert_eq!(submit(&mut h, r), Err(venue_setting_hold("Y")));
}

/// S17. A setting that matches the policy releases the hold at once. What
/// the venue told in one link session says nothing about the next one: the
/// operator may have changed the setting while the link was down. After a
/// link loss the setting is unknown until the venue tells it again.
#[test]
fn a_matching_setting_releases_the_hold_and_link_loss_makes_it_unknown_again() {
    let mut h = H::ready_with(config_with_margin());
    h.report_margin("1000");
    h.ref_price("X", "100");

    h.venue_setting("X", false, 2);
    assert_eq!(h.reject("a", "k1", Side::Buy, "1", "100"), venue_setting_hold("X"));
    h.venue_setting("X", true, 2);
    h.accept("a", "k1", Side::Buy, "1", "100");

    // The link is lost and comes back as session 2, fully synced. The
    // setting told in session 1 does not count.
    link_down(&mut h);
    h.link_up_and_sync_only();
    assert_eq!(h.session, 2);
    h.tell_account_mode();
    h.report_margin("1000");
    h.ref_price("X", "100");
    assert_eq!(h.reject("a", "k2", Side::Buy, "1", "100"), venue_setting_hold("X"));

    // Told again in session 2: released.
    h.venue_setting("X", true, 2);
    h.accept("a", "k2", Side::Buy, "1", "100");
}

/// The next margin report of the current link session: account value 1000
/// and one position on X with this margin mode and leverage.
fn report_with_position_on_x(h: &mut H, leverage_type: LeverageType, leverage_value: u32) -> Output {
    h.margin_seq += 1;
    h.venue(VenueInput::MarginReport(MarginReportIn {
        session: h.session,
        seq: h.margin_seq,
        queried_at: h.now,
        account_value: amt("1000"),
        positions: vec![MarginPosition {
            leverage_type,
            leverage_value,
            ..position("X", 40)
        }],
    }))
}

/// S17. A position in a margin report states the margin mode and the
/// leverage of its instrument, so every report is a source of the venue
/// setting: a position that turns out to be on cross margin, or at another
/// leverage, raises the hold with the report that shows it.
#[test]
fn a_cross_position_in_a_margin_report_raises_the_hold() {
    let mut h = H::ready_with(config_with_margin());
    h.ref_price("X", "100");

    // The position is as the policy asks: no hold.
    let out = report_with_position_on_x(&mut h, LeverageType::Isolated, 2);
    assert_eq!(names(&out), vec!["margin_reported"]);
    h.accept("a", "k1", Side::Buy, "1", "100");

    // The next report shows the position on cross margin.
    let out = report_with_position_on_x(&mut h, LeverageType::Cross, 2);
    assert_eq!(names(&out), vec!["margin_reported"]);
    assert_eq!(h.reject("a", "k2", Side::Buy, "1", "100"), venue_setting_hold("X"));

    // Isolated again, but at leverage 3.
    report_with_position_on_x(&mut h, LeverageType::Isolated, 3);
    assert_eq!(h.reject("a", "k2", Side::Buy, "1", "100"), venue_setting_hold("X"));

    // A report that shows the policy's setting releases the hold.
    report_with_position_on_x(&mut h, LeverageType::Isolated, 2);
    h.accept("a", "k2", Side::Buy, "1", "100");
    // The state rebuilt from the journal holds the same settings.
    assert_eq!(h.rebuilt().margin, h.core.margin);
}

/// S17. The operator must see which instrument to fix at the venue: the
/// hold query lists one `VenueSetting` hold per instrument whose setting is
/// unknown or differs, each with its instrument. Without a margin policy
/// the hold does not exist.
#[test]
fn venue_setting_hold_names_its_instrument_in_holds() {
    let mut h = H::configured(config_xy());
    h.link_up_and_sync_only();
    h.tell_account_mode();
    let hold = |i: &str| HoldReason::VenueSetting(iid(i));
    assert_eq!(h.core.holds(None), vec![hold("X"), hold("Y")]);

    h.venue_setting("X", true, 2);
    assert_eq!(h.core.holds(None), vec![hold("Y")]);
    // The query for one strategy shows it too.
    assert_eq!(h.core.holds(Some(&sid("a"))), vec![hold("Y")]);

    // Known but different is the same hold.
    h.venue_setting("Y", true, 3);
    assert_eq!(h.core.holds(None), vec![hold("Y")]);
    h.venue_setting("Y", true, 2);
    assert_eq!(h.core.holds(None), vec![]);

    // No margin policy: no such hold, and the input leaves no trace.
    let mut h = H::configured(config());
    h.link_up_and_sync_only();
    assert_eq!(h.core.holds(None), vec![]);
    let out = h.venue_setting("X", false, 3);
    assert_eq!(names(&out), Vec::<&str>::new());
    assert!(h.core.margin.is_empty());
}

fn account_mode_hold() -> Vec<RejectReason> {
    vec![RejectReason::Hold(HoldReason::AccountMode)]
}

/// `config_xy()` with these account modes on the allow-list.
fn config_allowing(modes: &[&str]) -> CoreConfig {
    let mut cfg = config_xy();
    let margin = cfg.policy.margin.as_mut().unwrap();
    margin.account_modes_allowed = modes.iter().map(|m| m.to_string()).collect();
    cfg
}

/// S18. In a unified account or under portfolio margin the numbers of a
/// margin report do not mean what the rules take them to mean. Both modes
/// refuse plain orders on the whole account, also when the configuration
/// lists them as allowed.
#[test]
fn unified_or_portfolio_account_mode_holds_the_account() {
    // The operator listed both modes by mistake, next to a mode that is
    // allowed.
    let mut h = H::ready_with(config_allowing(&["unifiedAccount", "portfolioMargin", "standard"]));
    h.report_margin("1000");
    h.ref_price("X", "100");
    h.ref_price("Y", "100");

    for mode in ["unifiedAccount", "portfolioMargin"] {
        let out = h.account_mode(mode);
        assert_eq!(
            out.events,
            vec![Event::AccountModeNoted {
                session: 1,
                mode: mode.into(),
            }]
        );
        assert_eq!(out.commands, vec![]);
        assert_eq!(h.core.holds(None), vec![HoldReason::AccountMode], "{mode}");
        // The whole account: every instrument.
        for instrument in ["X", "Y"] {
            let r = req_on(&h, instrument, "k1", Side::Buy, "1", "100");
            assert_eq!(submit(&mut h, r), Err(account_mode_hold()), "{mode} on {instrument}");
        }
    }

    // The venue names the listed mode that can be allowed: released.
    h.account_mode("standard");
    assert_eq!(h.core.holds(None), vec![]);
    h.accept("a", "k1", Side::Buy, "1", "100");
}

/// S18. Only a mode the configuration lists lets plain orders through. A
/// mode that is not listed, an empty answer, and no answer at all in this
/// link session each hold the account. "standard" stands here for whatever
/// the venue calls its ordinary mode; which string that is on the real
/// venue is not known (T10), so the operator has to list it.
#[test]
fn an_account_mode_off_the_allow_list_or_unknown_holds_the_account() {
    let mut h = H::configured(config_allowing(&["standard"]));
    h.link_up_and_sync_only();
    h.venue_setting("X", true, 2);
    h.venue_setting("Y", true, 2);
    h.report_margin("1000");
    h.ref_price("X", "100");

    // The venue has not told the mode in this link session.
    assert_eq!(h.core.holds(None), vec![HoldReason::AccountMode]);
    assert_eq!(h.reject("a", "k1", Side::Buy, "1", "100"), account_mode_hold());

    // The listed mode releases the account.
    h.account_mode("standard");
    assert_eq!(h.core.holds(None), vec![]);
    h.accept("a", "k1", Side::Buy, "1", "100");

    // A mode that is not listed, and an empty answer.
    for mode in ["dexAbstraction", "Standard", ""] {
        h.account_mode(mode);
        assert_eq!(h.reject("a", "k2", Side::Buy, "1", "100"), account_mode_hold(), "mode {mode:?}");
    }
    h.account_mode("standard");
    h.accept("a", "k2", Side::Buy, "1", "100");

    // A new link session: the mode told in session 1 does not count, and a
    // late answer to a query of session 1 is dropped.
    link_down(&mut h);
    h.link_up_and_sync_only();
    h.venue_setting("X", true, 2);
    h.venue_setting("Y", true, 2);
    h.report_margin("1000");
    h.ref_price("X", "100");
    let out = h.venue(VenueInput::AccountMode {
        session: 1,
        mode: "standard".into(),
    });
    assert_eq!(names(&out), Vec::<&str>::new());
    assert_eq!(h.core.holds(None), vec![HoldReason::AccountMode]);
    assert_eq!(h.reject("a", "k3", Side::Buy, "1", "100"), account_mode_hold());
    h.account_mode("standard");
    h.accept("a", "k3", Side::Buy, "1", "100");

    // The default allow-list is empty: no mode passes until one is listed.
    let mut h = H::configured(config_allowing(&[]));
    h.link_up_and_sync_only();
    h.venue_setting("X", true, 2);
    h.venue_setting("Y", true, 2);
    h.account_mode("standard");
    assert_eq!(h.core.holds(None), vec![HoldReason::AccountMode]);
}
