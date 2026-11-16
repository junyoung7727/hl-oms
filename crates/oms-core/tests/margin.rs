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
    next_input_at(&mut h, t0 + 30 * SEC);
    let id = h.accept("a", "k2", Side::Buy, "1", "100");
    assert_eq!(h.order(id).accepted_at, t0 + 30_000_000);

    // A second report asked for at t1. An order recorded 30 s and one
    // microsecond later is rejected, and the reason is the only one.
    let t1 = h.now;
    h.margin_report(1, 2, t1, "1000");
    next_input_at(&mut h, t1 + 30 * SEC + 1);
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
    next_input_at(&mut h, queried_at);
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
    h.core.account_exposure().expect("exposure is known").to_string()
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
    assert_eq!(rejected(h.new_order(r)), vec![RejectReason::AccountLeverage]);
    // A short on Y does not offset the long on X either.
    let r = req_on(&h, "Y", "k3", Side::Sell, "1.5", "100");
    assert_eq!(rejected(h.new_order(r)), vec![RejectReason::AccountLeverage]);
    assert_eq!(exposure(&h), "150");

    // 150 + 50 is at the limit.
    let r = req_on(&h, "Y", "k4", Side::Sell, "0.5", "100");
    accepted(h.new_order(r));
    assert_eq!(exposure(&h), "200");
    // Now neither instrument has room.
    let r = req_on(&h, "X", "k5", Side::Buy, "0.1", "100");
    assert_eq!(rejected(h.new_order(r)), vec![RejectReason::AccountLeverage]);
    let r = req_on(&h, "Y", "k6", Side::Sell, "0.1", "100");
    assert_eq!(rejected(h.new_order(r)), vec![RejectReason::AccountLeverage]);
}

// ---- C-MARGIN: replay ---------------------------------------------------------

/// S13. Margin state comes only from journal events. A core rebuilt from the
/// journal, a core that re-decides the recorded inputs, and a core read back
/// from its serialised form all equal the live one, digest included.
#[test]
fn margin_events_replay_to_the_same_state() {
    let mut h = H::ready_with(config_with_margin());
    h.margin_report(1, 1, h.now, "100");
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
