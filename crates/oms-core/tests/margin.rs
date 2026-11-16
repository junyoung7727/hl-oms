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
    // The first report of a run also sets the day base.
    assert_eq!(names(&out), vec!["margin_reported", "day_base_set"]);
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
    // The first report of a run also sets the day base.
    assert_eq!(names(&out), vec!["margin_reported", "day_base_set"]);
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
    // The first report of a run also sets the day base.
    assert_eq!(names(&out), vec!["margin_reported", "day_base_set"]);

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

// ---- C-MARGIN: daily loss and the day base ------------------------------------

/// Microseconds in one UTC day, written out: 24 × 3600 × 1 000 000.
const DAY: u64 = 86_400_000_000;

/// A margin policy at its defaults, and the node clock at 10:00:00 UTC of
/// day 20 000. No margin report has arrived yet.
fn on_day_20_000() -> H {
    let mut h = H::ready_with(config_with_margin());
    next_input_at(&mut h, 20_000 * DAY + 36_000 * SEC);
    h
}

/// S12. The first margin report sets the day base to its account value, on
/// the UTC day of its query time. A later report of the same day leaves the
/// base alone.
#[test]
fn first_report_sets_the_day_base() {
    let mut h = on_day_20_000();
    assert_eq!(h.core.margin.day_base, None);

    let out = h.report_margin("100");
    assert_eq!(
        out.events.last(),
        Some(&Event::DayBaseSet {
            day: 20_000,
            value: amt("100"),
            set_by: BaseSetBy::Report,
        })
    );
    assert_eq!(names(&out), vec!["margin_reported", "day_base_set"]);
    let base = DayBase {
        day: 20_000,
        value: amt("100"),
        set_by: BaseSetBy::Report,
    };
    assert_eq!(h.core.margin.day_base, Some(base.clone()));

    // Later the same day, with another value: the base stays.
    h.advance(3_600 * SEC);
    let out = h.report_margin("103");
    assert_eq!(names(&out), vec!["margin_reported"]);
    assert_eq!(h.core.margin.day_base, Some(base));
}

/// S8. Base 100 and a limit of 5 %: an account value of 95.00 is a loss of
/// exactly 5 % and does not trip the rule; 94.99 is more and does. The
/// account is then REDUCING with the rule `DailyLoss`. The day base stays.
#[test]
fn loss_greater_than_five_percent_of_the_base_enters_reducing() {
    let mut h = on_day_20_000();
    h.report_margin("100");

    h.advance(5 * SEC);
    let out = h.report_margin("95");
    assert_eq!(names(&out), vec!["margin_reported"]);
    assert_eq!(h.core.margin.reducing, None);
    assert_eq!(h.core.holds(None), vec![]);

    h.advance(5 * SEC);
    let out = h.report_margin("94.99");
    let entered_at = h.now;
    assert_eq!(names(&out), vec!["margin_reported", "reducing_entered"]);
    assert_eq!(
        out.events[1],
        Event::ReducingEntered {
            rule: RuleId::DailyLoss,
            detail: "account value 94.99 is more than 500 bps below the day base 100".into(),
        }
    );
    assert_eq!(
        h.core.margin.reducing,
        Some(Reducing {
            rule: RuleId::DailyLoss,
            detail: "account value 94.99 is more than 500 bps below the day base 100".into(),
            since: entered_at,
        })
    );
    assert_eq!(h.core.holds(None), vec![HoldReason::Reducing]);
    // Nothing is sent to the venue: there is no order to cancel, and the
    // OMS never closes a position.
    assert_eq!(out.commands, vec![]);
    assert_eq!(
        h.core.margin.day_base.as_ref().map(|b| (b.day, b.value)),
        Some((20_000, amt("100")))
    );
}

// ---- C-MARGIN: REDUCING -------------------------------------------------------

/// Day 20 000, a reference price of 100 for X, and a first margin report of
/// 100 that became the day base.
fn at_base_100() -> H {
    let mut h = on_day_20_000();
    let venue_ts = h.now;
    h.venue(VenueInput::RefPrice {
        instrument: iid("X"),
        px: px("100"),
        venue_ts,
    });
    h.report_margin("100");
    assert_eq!(
        h.core.margin.day_base.as_ref().map(|b| b.value),
        Some(amt("100"))
    );
    h
}

/// A resting order the venue acknowledged.
fn resting(h: &mut H, strategy: &str, key: &str, side: Side, qty: &str, price: &str) -> OrderId {
    let id = h.accept(strategy, key, side, qty, price);
    h.placed_open(id, key);
    assert_eq!(h.order(id).phase, Phase::Working);
    id
}

/// A resting order with the venue's reduce-only flag.
fn resting_reduce_only(h: &mut H, strategy: &str, key: &str, side: Side, qty: &str, price: &str) -> OrderId {
    let r = h.reduce_only_req(strategy, key, side, qty, price);
    let id = match h.new_order(r) {
        NewOrderReply::Accepted { order_id } => order_id,
        other => panic!("the reduce-only order was refused: {other:?}"),
    };
    h.placed_open(id, key);
    id
}

fn rule_cancel(id: OrderId) -> Event {
    Event::CancelRequested {
        order_id: id,
        by: Actor::Rule(RuleId::DailyLoss),
    }
}

fn cancel_sent(id: OrderId, attempt: u32) -> Event {
    Event::CancelSent {
        order_id: id,
        attempt,
    }
}

/// Every attempt of the order's current cancel request ends without an
/// answer from the venue, until the five attempts of the default policy are
/// used up. The core then raises `CancelStuck` and sends no further attempt.
fn exhaust_cancel(h: &mut H, id: OrderId) {
    let first = h.order(id).cancel.as_ref().unwrap().attempts;
    for attempt in first..first + 5 {
        h.venue(VenueInput::CancelResult {
            order_id: id,
            attempt,
            outcome: CancelOutcome::Unknown {
                reason: "timeout".into(),
            },
            timing: Default::default(),
        });
        h.advance(SEC);
        h.tick();
    }
    let c = h.order(id).cancel.as_ref().unwrap();
    assert_eq!((c.used(), c.inflight, c.confirmed), (5, false, false));
    assert_eq!(h.unacked_mismatches(), vec![MismatchKind::CancelStuck]);
}

/// S8, S24. In the input that enters REDUCING every resting plain order gets
/// a cancel by `Rule(DailyLoss)`, and the cancel goes to the venue. A sell
/// that would take profit but carries no reduce-only flag is a plain order
/// and is cancelled. An order with the flag is kept. An order whose cancel
/// the strategy already asked for gets no second request, unless every
/// attempt of that request is used up: then the rule asks again.
#[test]
fn entering_reducing_cancels_plain_orders_and_keeps_reduce_only_ones() {
    let mut h = at_base_100();
    h.long("a", "k1", "1");
    let bid = resting(&mut h, "a", "bid", Side::Buy, "0.2", "90");
    let take_profit = resting(&mut h, "a", "tp", Side::Sell, "0.5", "110");
    let flagged = resting_reduce_only(&mut h, "a", "flagged", Side::Sell, "0.5", "120");
    let asked = resting(&mut h, "b", "asked", Side::Buy, "0.2", "90");
    assert_eq!(h.cancel("b", asked), CancelReply::Requested { order_id: asked });
    assert_eq!(h.cancels(), vec![(asked, 1)]);
    let given_up = resting(&mut h, "b", "given-up", Side::Buy, "0.2", "90");
    h.cancel("b", given_up);
    exhaust_cancel(&mut h, given_up);

    let out = h.report_margin("94.99");
    assert_eq!(names(&out)[..2], ["margin_reported", "reducing_entered"]);
    assert_eq!(
        out.events[2..],
        [
            rule_cancel(bid),
            cancel_sent(bid, 1),
            rule_cancel(take_profit),
            cancel_sent(take_profit, 1),
            rule_cancel(given_up),
            cancel_sent(given_up, 6),
        ]
    );
    let sent: Vec<OrderId> = out
        .commands
        .iter()
        .map(|c| match c {
            VenueCommand::SendCancel { order_id, .. } => *order_id,
            other => panic!("entering REDUCING sent something that is not a cancel: {other:?}"),
        })
        .collect();
    assert_eq!(sent, vec![bid, take_profit, given_up]);

    let by = |id: OrderId| h.order(id).cancel.as_ref().map(|c| c.by.clone());
    assert_eq!(by(bid), Some(Actor::Rule(RuleId::DailyLoss)));
    assert_eq!(by(take_profit), Some(Actor::Rule(RuleId::DailyLoss)));
    assert_eq!(by(flagged), None, "a reduce-only order was cancelled");
    assert_eq!(by(asked), Some(Actor::Strategy(sid("b"))));
    assert_eq!(by(given_up), Some(Actor::Rule(RuleId::DailyLoss)));
    assert_eq!(h.order(flagged).phase, Phase::Working);
    // A cancel request frees nothing: the plain orders keep their
    // reservation until the venue reports them closed.
    assert_eq!(h.acct().buy_open, q("0.6"));
    assert_eq!(h.acct().sell_open, q("1"));
}

/// S8. While the account is REDUCING a plain order is refused with
/// `Hold(Reducing)`, also a plain sell that would only reduce the long
/// position: "reduce-only" is the venue flag on the order, not a direction
/// the OMS works out. The same sell with the flag is accepted and sent.
#[test]
fn under_reducing_a_plain_order_is_rejected_and_a_reduce_only_one_is_accepted() {
    let mut h = at_base_100();
    h.long("a", "k1", "1");
    h.report_margin("94.99");
    assert_eq!(h.core.holds(None), vec![HoldReason::Reducing]);

    assert_eq!(
        h.reject("a", "k2", Side::Buy, "0.1", "100"),
        vec![RejectReason::Hold(HoldReason::Reducing)]
    );
    assert_eq!(
        h.reject("a", "k3", Side::Sell, "0.5", "100"),
        vec![RejectReason::Hold(HoldReason::Reducing)]
    );

    let r = h.reduce_only_req("a", "k4", Side::Sell, "0.5", "100");
    let order_id = match h.new_order(r) {
        NewOrderReply::Accepted { order_id } => order_id,
        other => panic!("a reduce-only order was refused under REDUCING: {other:?}"),
    };
    assert!(h.order(order_id).reduce_only);
    assert_eq!(h.sends().last(), Some(&order_id));
    // The accepted order is not cancelled by the latch that is already set.
    assert_eq!(h.order(order_id).cancel, None);
}

/// S9. An order that is still waiting for its send result when the account
/// enters REDUCING gets the rule's cancel request at once, but no cancel is
/// sent: the venue may not have the order yet. The cancel goes out in the
/// input that brings the venue's acknowledgement. Until then the order can
/// fill.
#[test]
fn an_order_pending_send_at_entry_is_cancelled_once_acknowledged() {
    let mut h = at_base_100();
    let pending = h.accept("a", "k1", Side::Buy, "1", "100");
    assert_eq!(h.order(pending).phase, Phase::PendingSend);

    let out = h.report_margin("94.99");
    assert_eq!(
        out.events[1..],
        [
            Event::ReducingEntered {
                rule: RuleId::DailyLoss,
                detail: "account value 94.99 is more than 500 bps below the day base 100".into(),
            },
            rule_cancel(pending),
        ]
    );
    assert_eq!(
        out.commands,
        vec![],
        "a cancel was sent before the venue acknowledged the order"
    );
    assert_eq!(h.cancels(), vec![]);

    // Ticks in between send nothing either.
    h.advance(SEC);
    h.tick();
    assert_eq!(h.cancels(), vec![]);

    let out = h.placed_open(pending, "v1");
    assert_eq!(names(&out), vec!["order_acked", "cancel_sent"]);
    assert_eq!(out.events[1], cancel_sent(pending, 1));
    assert_eq!(h.cancels(), vec![(pending, 1)]);
    assert_eq!(
        h.order(pending).cancel.as_ref().map(|c| c.by.clone()),
        Some(Actor::Rule(RuleId::DailyLoss))
    );
}

/// S4. A rule cancel can lose the race to a fill. A fill that arrives after
/// the rule's cancel was sent is booked, and the venue's final status
/// decides how the order ends, as it does for a strategy's cancel
/// (`cancel_race_final_status_decides`).
#[test]
fn a_rule_cancel_that_races_a_fill_books_the_fill() {
    // The cancel lost: the order filled completely before it arrived.
    let mut h = at_base_100();
    let a = resting(&mut h, "a", "k1", Side::Buy, "1", "100");
    h.report_margin("94.99");
    assert_eq!(h.cancels(), vec![(a, 1)]);
    h.fill(a, "t1", "1", "100");
    h.venue(VenueInput::CancelResult {
        order_id: a,
        attempt: 1,
        outcome: CancelOutcome::Rejected {
            reason: "already filled".into(),
        },
        timing: Default::default(),
    });
    assert_eq!(h.order(a).phase, Phase::Closed);
    assert_eq!(
        h.order(a).close.as_ref().unwrap().reason,
        CloseReason::Filled
    );
    assert_eq!(h.acct().pos, sq("1"));
    assert_eq!(h.acct().buy_open, q("0"));
    assert_eq!(h.unacked_mismatches(), vec![]);
    // The position grew while REDUCING. The OMS does not close it.
    assert_eq!(h.core.holds(None), vec![HoldReason::Reducing]);
    assert_eq!(h.sends(), vec![a]);

    // The cancel won for the rest: 0.4 filled after the cancel was sent, and
    // the venue's final status says cancelled with 0.4 filled.
    let mut h = at_base_100();
    let a = resting(&mut h, "a", "k1", Side::Buy, "1", "100");
    h.report_margin("94.99");
    h.fill(a, "t1", "0.4", "100");
    h.venue(VenueInput::CancelResult {
        order_id: a,
        attempt: 1,
        outcome: CancelOutcome::Confirmed,
        timing: Default::default(),
    });
    assert_eq!(
        h.order(a).phase,
        Phase::Working,
        "a confirmed cancel is not terminal evidence"
    );
    assert_eq!(h.acct().buy_open, q("0.6"));
    h.status(
        a,
        VenueStatus::Canceled {
            reason: "user".into(),
        },
        Some("0.4"),
    );
    assert_eq!(h.order(a).phase, Phase::Closed);
    assert_eq!(
        h.order(a).close.as_ref().unwrap().reason,
        CloseReason::Canceled {
            venue_reason: "user".into()
        }
    );
    assert_eq!(h.acct().pos, sq("0.4"));
    assert_eq!(h.acct().buy_open, q("0"));
    assert_eq!(h.unacked_mismatches(), vec![]);
}

/// S10. The rule's cancel gets no answer from the venue and its attempts
/// run out: the core raises `CancelStuck`. The mismatch and REDUCING both
/// refuse plain orders. Neither refuses a reduce-only order, so the stuck
/// cancel of the rule does not trap the position.
#[test]
fn a_stuck_rule_cancel_does_not_block_reduce_only_orders() {
    let mut h = at_base_100();
    h.long("a", "k1", "1");
    let bid = resting(&mut h, "a", "bid", Side::Buy, "0.2", "90");
    h.report_margin("94.99");
    assert_eq!(h.cancels(), vec![(bid, 1)]);
    exhaust_cancel(&mut h, bid);
    assert_eq!(h.cancels().len(), 5);
    assert_eq!(h.order(bid).phase, Phase::Working);
    assert_eq!(
        h.core.holds(None),
        vec![HoldReason::Mismatch, HoldReason::Reducing]
    );

    assert_eq!(
        h.reject("a", "k2", Side::Sell, "0.5", "100"),
        vec![
            RejectReason::Hold(HoldReason::Mismatch),
            RejectReason::Hold(HoldReason::Reducing)
        ]
    );
    let r = h.reduce_only_req("a", "k3", Side::Sell, "0.5", "100");
    let order_id = match h.new_order(r) {
        NewOrderReply::Accepted { order_id } => order_id,
        other => panic!("a stuck rule cancel refused a reduce-only order: {other:?}"),
    };
    assert_eq!(h.sends().last(), Some(&order_id));
}

/// S8. REDUCING is entered once. A later report that breaches the limit
/// again emits no second entry event and no new cancel, also not for an
/// order whose rule cancel has used up its attempts. The first entry stays
/// recorded as it was.
#[test]
fn reducing_is_entered_once() {
    let mut h = at_base_100();
    let bid = resting(&mut h, "a", "bid", Side::Buy, "0.2", "90");
    h.report_margin("94.99");
    let first_entry = h.core.margin.reducing.clone();
    assert!(first_entry.is_some());
    exhaust_cancel(&mut h, bid);
    assert_eq!(h.cancels().len(), 5);

    h.advance(5 * SEC);
    let out = h.report_margin("90");
    assert_eq!(names(&out), vec!["margin_reported"]);
    assert_eq!(out.commands, vec![]);
    assert_eq!(h.cancels().len(), 5);
    assert_eq!(h.core.margin.reducing, first_entry);
    assert_eq!(
        h.order(bid).cancel.as_ref().map(|c| c.used()),
        Some(5),
        "the stuck cancel got a new budget"
    );
}

fn daily_loss_entered(value: &str, base: &str) -> Event {
    Event::ReducingEntered {
        rule: RuleId::DailyLoss,
        detail: format!("account value {value} is more than 500 bps below the day base {base}"),
    }
}

fn base_set(day: u64, value: &str) -> Event {
    Event::DayBaseSet {
        day,
        value: amt(value),
        set_by: BaseSetBy::Report,
    }
}

/// S12. The first report of a later UTC day is judged against the old base
/// first, and only then becomes the new base. So a loss taken across
/// midnight is not absorbed into the new day. After a gap of several days
/// the base rolls once, to the day of the report.
#[test]
fn first_report_after_midnight_is_judged_against_the_old_base_then_rolls_the_day() {
    // Base 100 on day 20 000. Five seconds into day 20 001 the account is
    // worth 94: a loss of 6 % of the old base.
    let mut h = at_base_100();
    next_input_at(&mut h, 20_001 * DAY + 5 * SEC);
    let out = h.report_margin("94");
    assert_eq!(
        out.events[1..],
        [daily_loss_entered("94", "100"), base_set(20_001, "94")]
    );

    // No report for three days. The first one after the gap is judged
    // against the base of day 20 000, and the base rolls once.
    let mut h = at_base_100();
    next_input_at(&mut h, 20_003 * DAY + 5 * SEC);
    let out = h.report_margin("94.99");
    assert_eq!(
        out.events[1..],
        [daily_loss_entered("94.99", "100"), base_set(20_003, "94.99")]
    );
    h.advance(5 * SEC);
    let out = h.report_margin("94.99");
    assert_eq!(names(&out), vec!["margin_reported"]);

    // A roll without a breach: 97 on the next day is 3 % below 100. From
    // then on the base is 97, and 5 % of 97 is 4.85: 92.15 does not trip,
    // 92.14 does.
    let mut h = at_base_100();
    next_input_at(&mut h, 20_001 * DAY + 5 * SEC);
    let out = h.report_margin("97");
    assert_eq!(out.events[1..], [base_set(20_001, "97")]);
    h.advance(5 * SEC);
    let out = h.report_margin("92.15");
    assert_eq!(names(&out), vec!["margin_reported"]);
    h.advance(5 * SEC);
    let out = h.report_margin("92.14");
    assert_eq!(out.events[1..], [daily_loss_entered("92.14", "97")]);
}

/// S12. A new UTC day rolls the day base and leaves REDUCING in place: the
/// entry stays as it was recorded, and a plain order is still refused,
/// although the account lost nothing against the new base.
#[test]
fn a_new_day_does_not_clear_reducing() {
    let mut h = at_base_100();
    h.report_margin("94.99");
    let entry = h.core.margin.reducing.clone();
    assert_eq!(entry.as_ref().map(|r| r.rule), Some(RuleId::DailyLoss));

    next_input_at(&mut h, 20_001 * DAY + 5 * SEC);
    let venue_ts = h.now;
    h.venue(VenueInput::RefPrice {
        instrument: iid("X"),
        px: px("100"),
        venue_ts,
    });
    let out = h.report_margin("94.99");
    assert_eq!(out.events[1..], [base_set(20_001, "94.99")]);
    assert_eq!(h.core.margin.reducing, entry);
    assert_eq!(h.core.holds(None), vec![HoldReason::Reducing]);
    assert_eq!(
        h.reject("a", "k1", Side::Buy, "0.1", "100"),
        vec![RejectReason::Hold(HoldReason::Reducing)]
    );
}
