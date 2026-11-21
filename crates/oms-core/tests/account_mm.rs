//! The account maintenance-margin rule of a cross-margin account (C-MARGIN,
//! 계좌 유지증거금률; ADR-028 §7g, U17d). Expected values are literals worked
//! out by hand from the contract, not derived from the implementation.

#![allow(clippy::unwrap_used)]

mod common;
use common::*;
use oms_core::*;

/// The limit of these tests: 5000 bps, the Bybit default (50 %).
const LIMIT_BPS: u32 = 5000;

fn names(out: &Output) -> Vec<&'static str> {
    out.events.iter().map(Event::name).collect()
}

/// `config_with_margin()` with the account maintenance-margin limit.
fn config_with_limit(limit_bps: Option<u32>) -> CoreConfig {
    let mut cfg = config_with_margin();
    cfg.policy.margin.as_mut().unwrap().account_mm_rate_max_bps = limit_bps;
    cfg
}

fn account_mm(maintenance: &str, margin_balance: &str) -> AccountMm {
    AccountMm {
        maintenance: amt(maintenance),
        margin_balance: amt(margin_balance),
    }
}

/// The next margin report of the current link session, asked now: account
/// value 100 and no positions, with or without the account's maintenance
/// margin and margin balance.
fn report(h: &mut H, account_mm: Option<AccountMm>) -> Output {
    report_with(h, "100", vec![], account_mm)
}

fn report_with(
    h: &mut H,
    account_value: &str,
    positions: Vec<MarginPosition>,
    account_mm: Option<AccountMm>,
) -> Output {
    h.margin_seq += 1;
    h.venue(VenueInput::MarginReport(MarginReport {
        session: h.session,
        seq: h.margin_seq,
        queried_at: h.now,
        account_value: amt(account_value),
        positions,
        account_mm,
    }))
}

/// Ready to trade with the limit at 5000 bps: a reference price of 100 on
/// X and a first report (account value 100, the day base; maintenance 0 of
/// a margin balance 100).
fn at_limit() -> H {
    at(Some(LIMIT_BPS))
}

fn at(limit_bps: Option<u32>) -> H {
    let mut h = H::ready_with(config_with_limit(limit_bps));
    h.ref_price("X", "100");
    let out = report(&mut h, Some(account_mm("0", "100")));
    assert_eq!(names(&out), vec!["margin_reported", "day_base_set"]);
    assert_eq!(h.core.margin.reducing, None);
    h
}

/// A resting order the venue acknowledged.
fn resting(h: &mut H, key: &str, side: Side, qty: &str, price: &str) -> OrderId {
    let id = h.accept("a", key, side, qty, price);
    h.placed_open(id, key);
    id
}

fn resting_reduce_only(h: &mut H, key: &str, side: Side, qty: &str, price: &str) -> OrderId {
    let r = h.reduce_only_req("a", key, side, qty, price);
    let NewOrderReply::Accepted { order_id } = h.new_order(r) else {
        panic!("the reduce-only order was refused")
    };
    h.placed_open(order_id, key);
    order_id
}

fn account_mm_entered(detail: &str) -> Event {
    Event::ReducingEntered {
        rule: RuleId::AccountMm,
        detail: detail.into(),
    }
}

// ---- the rule --------------------------------------------------------------

/// U17d. With the limit at 5000 bps and a margin balance of 1000, a
/// maintenance margin of 499.99 (a rate just below 50 %) latches nothing;
/// 500 (exactly 50 %) latches REDUCING with the rule `account_mm` and, in
/// the same input, cancels the resting plain order in the name of the rule.
/// The reduce-only order is kept.
#[test]
fn rate_just_below_the_limit_does_not_latch_and_at_the_limit_latches_with_the_cancels() {
    let mut h = at_limit();
    h.long("a", "k1", "1");
    let bid = resting(&mut h, "bid", Side::Buy, "0.2", "90");
    let flagged = resting_reduce_only(&mut h, "flagged", Side::Sell, "0.5", "120");

    let out = report(&mut h, Some(account_mm("499.99", "1000")));
    assert_eq!(names(&out), vec!["margin_reported"]);
    assert_eq!(h.core.margin.reducing, None);
    assert_eq!(h.core.holds(None), vec![]);

    let out = report(&mut h, Some(account_mm("500", "1000")));
    let entered_at = h.now;
    let detail = "account maintenance margin 500 is at or above 5000 bps \
                  of the margin balance 1000";
    assert_eq!(
        out.events[1..],
        [
            account_mm_entered(detail),
            Event::CancelRequested {
                order_id: bid,
                by: Actor::Rule(RuleId::AccountMm),
            },
            Event::CancelSent {
                order_id: bid,
                attempt: 1,
            },
        ]
    );
    assert_eq!(names(&out)[0], "margin_reported");
    assert_eq!(
        h.core.margin.reducing,
        Some(Reducing {
            rule: RuleId::AccountMm,
            detail: detail.into(),
            since: entered_at,
        })
    );
    assert_eq!(h.core.holds(None), vec![HoldReason::Reducing]);
    assert_eq!(
        h.order(flagged).cancel,
        None,
        "a reduce-only order was cancelled"
    );
    assert_eq!(h.order(flagged).phase, Phase::Working);
    let sent: Vec<OrderId> = out
        .commands
        .iter()
        .map(|c| match c {
            VenueCommand::SendCancel { order_id, .. } => *order_id,
            other => panic!("not a cancel: {other:?}"),
        })
        .collect();
    assert_eq!(sent, vec![bid]);
    // A plain order is now refused, a reduce-only one accepted.
    assert_eq!(
        h.reject("a", "k2", Side::Buy, "0.2", "90"),
        vec![RejectReason::Hold(HoldReason::Reducing)]
    );
    resting_reduce_only(&mut h, "flagged2", Side::Sell, "0.1", "120");
}

/// U17d. The limit is any whole number of basis points: at 1 bps a
/// maintenance margin of 0.01 on a balance of 100 is exactly at it, 0.00999999
/// is below.
#[test]
fn the_limit_is_judged_exactly_at_one_basis_point() {
    let mut h = at(Some(1));
    let out = report(&mut h, Some(account_mm("0.00999999", "100")));
    assert_eq!(names(&out), vec!["margin_reported"]);
    let out = report(&mut h, Some(account_mm("0.01", "100")));
    assert_eq!(names(&out), vec!["margin_reported", "reducing_entered"]);
}

/// U17d. A margin balance of zero or below with any maintenance margin is at
/// or above every limit: the venue liquidates there, and nothing is divided
/// by the balance.
#[test]
fn a_margin_balance_of_zero_or_below_with_maintenance_latches() {
    for (maintenance, balance) in [("0.01", "0"), ("1", "-5"), ("0.00000001", "-0.00000001")] {
        let mut h = at_limit();
        let out = report(&mut h, Some(account_mm(maintenance, balance)));
        let detail = format!(
            "account maintenance margin {maintenance} is at or above 5000 bps \
             of the margin balance {balance}"
        );
        assert_eq!(
            out.events[1..],
            [account_mm_entered(&detail)],
            "{maintenance} / {balance}"
        );
        assert_eq!(h.core.holds(None), vec![HoldReason::Reducing]);
    }
    // At the highest limit too.
    let mut h = at(Some(9999));
    let out = report(&mut h, Some(account_mm("0.01", "0")));
    assert_eq!(names(&out), vec!["margin_reported", "reducing_entered"]);
}

/// U17d. Without maintenance margin there is nothing the venue could
/// liquidate: a balance of 0, or below 0, with maintenance 0 latches nothing
/// (the daily-loss rule still sees the account value).
#[test]
fn no_maintenance_margin_latches_nothing_whatever_the_balance() {
    for balance in ["0", "-5", "1000"] {
        let mut h = at_limit();
        let out = report(&mut h, Some(account_mm("0", balance)));
        assert_eq!(names(&out), vec!["margin_reported"], "balance {balance}");
        assert_eq!(h.core.margin.reducing, None);
    }
}

/// U17d. The rule reads the numbers with no division and no rounding: the
/// largest amounts compare exactly. A maintenance margin of the largest
/// amount on the same balance is a rate of 100 %, above 9999 bps; one unit
/// of maintenance on the largest balance is far below 1 bps.
#[test]
fn the_comparison_is_exact_at_the_largest_amounts() {
    let max = Amount(i64::MAX);
    let mut h = at(Some(9999));
    let out = report(
        &mut h,
        Some(AccountMm {
            maintenance: Amount(1),
            margin_balance: max,
        }),
    );
    assert_eq!(names(&out), vec!["margin_reported"]);
    let out = report(
        &mut h,
        Some(AccountMm {
            maintenance: max,
            margin_balance: max,
        }),
    );
    assert_eq!(names(&out), vec!["margin_reported", "reducing_entered"]);
    // 9999 bps of the largest balance is above a maintenance margin of
    // 99.98 % of it, and at or below one of 99.99 %.
    let mut h = at(Some(9999));
    let just_below = Amount(i64::MAX / 10_000 * 9_998);
    let out = report(
        &mut h,
        Some(AccountMm {
            maintenance: just_below,
            margin_balance: max,
        }),
    );
    assert_eq!(names(&out), vec!["margin_reported"]);
}

/// U17d. With the limit set, a report without the account's maintenance
/// margin is refused as a whole, as one with a maximum leverage of 0 is:
/// the last recorded report stays and keeps ageing. A negative maintenance
/// margin is refused the same way. Without the limit the same reports are
/// recorded, and the numbers, when given, are not judged.
#[test]
fn a_report_without_account_mm_is_rejected_under_the_limit_and_recorded_without_it() {
    let mut h = at_limit();
    let before = h.core.margin.report.clone();
    let out = report(&mut h, None);
    assert_eq!(names(&out), vec!["margin_report_rejected"]);
    let Event::MarginReportRejected { reason, .. } = &out.events[0] else {
        unreachable!()
    };
    assert_eq!(
        reason,
        "account_mm is missing: the policy sets account_mm_rate_max_bps"
    );
    assert_eq!(
        h.core.margin.report, before,
        "the rejected report was recorded"
    );
    assert_eq!(
        h.core
            .margin
            .last_rejection
            .as_ref()
            .map(|r| r.reason.as_str()),
        Some(reason.as_str())
    );
    let out = report(&mut h, Some(account_mm("-0.01", "100")));
    assert_eq!(names(&out), vec!["margin_report_rejected"]);
    let Event::MarginReportRejected { reason, .. } = &out.events[0] else {
        unreachable!()
    };
    assert_eq!(reason, "account_mm: maintenance margin -0.01 is negative");
    assert_eq!(h.core.margin.report, before);
    // The last good report ages: 31 s later a plain order is refused.
    h.advance(31 * SEC);
    assert_eq!(
        h.reject("a", "k1", Side::Buy, "0.2", "90"),
        vec![RejectReason::MarginUnavailable]
    );

    let mut h = at(None);
    let out = report(&mut h, None);
    assert_eq!(names(&out), vec!["margin_reported"]);
    assert_eq!(h.core.margin.report.as_ref().unwrap().account_mm, None);
    let out = report(&mut h, Some(account_mm("1", "0")));
    assert_eq!(names(&out), vec!["margin_reported"]);
    assert_eq!(h.core.margin.reducing, None);
    let out = report(&mut h, Some(account_mm("-1", "100")));
    assert_eq!(names(&out), vec!["margin_reported"]);
}

/// An isolated position under the MM-buffer line: maximum leverage 40,
/// value 200, margin 2.99 (C-MARGIN, 유지증거금 여유).
fn under_the_mm_line() -> MarginPosition {
    MarginPosition {
        instrument: iid("X"),
        qty: sq("2"),
        position_value: amt("200"),
        margin_used: amt("2.99"),
        leverage_type: LeverageType::Isolated,
        leverage_value: 2,
        max_leverage: 40,
    }
}

/// U17d. The latch keeps the rule that tripped first. A later trigger of
/// another rule emits nothing and changes nothing, in both directions. When
/// one report trips several rules, they are judged in the contract's order:
/// MM buffer, account maintenance margin, daily loss.
#[test]
fn the_latch_is_not_replaced_by_a_later_rule() {
    let over = || Some(account_mm("600", "1000"));

    // Account maintenance margin first, then a 6 % loss and a position
    // under its MM line.
    let mut h = at_limit();
    report(&mut h, over());
    let first = h.core.margin.reducing.clone();
    assert_eq!(first.as_ref().map(|r| r.rule), Some(RuleId::AccountMm));
    let out = report_with(
        &mut h,
        "94",
        vec![under_the_mm_line()],
        Some(account_mm("0", "94")),
    );
    assert_eq!(names(&out), vec!["margin_reported"]);
    assert_eq!(h.core.margin.reducing, first);

    // Daily loss first, then the account rate over the limit.
    let mut h = at_limit();
    report_with(&mut h, "94", vec![], Some(account_mm("0", "94")));
    let first = h.core.margin.reducing.clone();
    assert_eq!(first.as_ref().map(|r| r.rule), Some(RuleId::DailyLoss));
    let out = report(&mut h, over());
    assert_eq!(names(&out), vec!["margin_reported"]);
    assert_eq!(h.core.margin.reducing, first);

    // One report trips the account rate and the daily loss: the account
    // rate is recorded.
    let mut h = at_limit();
    let out = report_with(&mut h, "94", vec![], over());
    assert_eq!(names(&out), vec!["margin_reported", "reducing_entered"]);
    assert_eq!(
        h.core.margin.reducing.as_ref().map(|r| r.rule),
        Some(RuleId::AccountMm)
    );

    // One report trips the MM buffer and the account rate: the MM buffer
    // is recorded.
    let mut h = at_limit();
    let out = report_with(&mut h, "100", vec![under_the_mm_line()], over());
    assert_eq!(names(&out), vec!["margin_reported", "reducing_entered"]);
    assert_eq!(
        h.core.margin.reducing.as_ref().map(|r| r.rule),
        Some(RuleId::MmBuffer)
    );
}

// ---- clear ------------------------------------------------------------------

fn clear_reduce(h: &mut H, note: &str) -> Output {
    h.inp(Input::Operator(OperatorCmd::ClearReduce {
        note: note.into(),
    }))
}

fn refused(out: &Output) -> &str {
    assert_eq!(out.events, vec![], "a refused command changed the state");
    match &out.reply {
        Some(Reply::Operator(Err(text))) => text,
        other => panic!("the command was not refused: {other:?}"),
    }
}

/// U17d (judgment, as for the MM buffer). `clear-reduce` is refused with
/// `account_mm_active` while the last report reads the account rate at or
/// above the limit, whichever rule set the latch. Below the limit it clears.
#[test]
fn clear_reduce_is_refused_while_the_account_rate_is_at_or_above_the_limit() {
    let active = "account_mm_active: the account maintenance margin is at or above \
                  account_mm_rate_max_bps of the margin balance on the last margin report";
    let mut h = at_limit();
    report(&mut h, Some(account_mm("500", "1000")));
    let latch = h.core.margin.reducing.clone();
    assert_eq!(latch.as_ref().map(|r| r.rule), Some(RuleId::AccountMm));
    assert_eq!(refused(&clear_reduce(&mut h, "looked at")), active);
    assert_eq!(h.core.margin.reducing, latch);
    // The note is checked after the rate, as after the MM buffer.
    assert_eq!(refused(&clear_reduce(&mut h, "")), active);

    report(&mut h, Some(account_mm("499.99", "1000")));
    let out = clear_reduce(&mut h, "margin added");
    assert_eq!(out.reply, Some(Reply::Operator(Ok("cleared".into()))));
    assert_eq!(h.core.margin.reducing, None);
    assert_eq!(h.core.holds(None), vec![]);

    // The daily-loss rule set the latch; the last report reads the account
    // rate over the limit.
    let mut h = at_limit();
    report_with(&mut h, "94", vec![], Some(account_mm("0", "94")));
    assert_eq!(
        h.core.margin.reducing.as_ref().map(|r| r.rule),
        Some(RuleId::DailyLoss)
    );
    report_with(&mut h, "94", vec![], Some(account_mm("0.01", "0")));
    assert_eq!(refused(&clear_reduce(&mut h, "loss understood")), active);
    // Without the limit the same numbers do not refuse a clear.
    let mut h = at(None);
    report_with(&mut h, "94", vec![], Some(account_mm("0.01", "0")));
    let out = clear_reduce(&mut h, "loss understood");
    assert_eq!(out.reply, Some(Reply::Operator(Ok("cleared".into()))));
}

// ---- status -----------------------------------------------------------------

/// U17d. `status` shows the account numbers of the last report and, once
/// the rule latched, the rule `account_mm` with what it saw. Without them
/// (a Hyperliquid report) the key is not shown.
#[test]
fn status_shows_the_account_numbers_and_the_rule() {
    let mut h = at_limit();
    report(&mut h, Some(account_mm("500", "1000")));
    let view = serde_json::to_value(h.core.margin_view(h.now).unwrap()).unwrap();
    assert_eq!(
        view["account_mm"],
        serde_json::json!({"maintenance": "500", "margin_balance": "1000"})
    );
    assert_eq!(view["reducing"]["rule"], "account_mm");
    assert_eq!(
        view["reducing"]["detail"],
        "account maintenance margin 500 is at or above 5000 bps of the margin balance 1000"
    );

    let mut h = at(None);
    report(&mut h, None);
    let view = serde_json::to_value(h.core.margin_view(h.now).unwrap()).unwrap();
    assert!(view.get("account_mm").is_none(), "{view}");
}

// ---- journal ----------------------------------------------------------------

/// U17d. Old journals keep their bytes: a report without the account
/// numbers and a policy without the limit write no key for them, and a
/// record written before the field existed reads back as "none". A journal
/// with the new field, and one without it, each rebuild to the live state,
/// and re-deciding their inputs gives the recorded events.
#[test]
fn journals_with_and_without_account_mm_replay_to_the_live_state() {
    // Without: no limit, reports without the numbers (Hyperliquid).
    let mut old = H::ready_with(config_with_limit(None));
    old.ref_price("X", "100");
    report(&mut old, None);
    report_with(&mut old, "94", vec![], None);
    // With: the limit and a latch by the rule.
    let mut new = at_limit();
    report(&mut new, Some(account_mm("499.99", "1000")));
    let bid = resting(&mut new, "bid", Side::Buy, "0.2", "90");
    report(&mut new, Some(account_mm("500", "1000")));
    assert_eq!(
        new.order(bid).cancel.as_ref().map(|c| c.by.clone()),
        Some(Actor::Rule(RuleId::AccountMm))
    );

    for h in [&old, &new] {
        let rebuilt = h.rebuilt();
        assert_eq!(rebuilt.digest(), h.core.digest());
        assert_eq!(rebuilt, h.core);
        let mut c = Core::new(7);
        for rec in &h.log {
            let out = c.handle(rec.at, &rec.input).unwrap();
            assert_eq!(
                out.events, rec.events,
                "record {} re-decided differently",
                rec.seq
            );
        }
        let recs: Vec<Record> =
            serde_json::from_str(&serde_json::to_string(&h.log).unwrap()).unwrap();
        assert_eq!(recs, h.log);
        let back: Core = serde_json::from_str(&serde_json::to_string(&h.core).unwrap()).unwrap();
        assert_eq!(back, h.core);
    }
    assert_eq!(
        new.core.margin.report.as_ref().unwrap().account_mm,
        Some(account_mm("500", "1000"))
    );

    // The journal and the state without the field carry no byte of it.
    let old_text = serde_json::to_string(&old.log).unwrap();
    assert!(!old_text.contains("account_mm"), "{old_text}");
    assert!(
        !serde_json::to_string(&old.core)
            .unwrap()
            .contains("account_mm")
    );
    let new_text = serde_json::to_string(&new.log).unwrap();
    assert!(new_text.contains("\"account_mm_rate_max_bps\":5000"));
    assert!(
        new_text.contains("\"account_mm\":{\"maintenance\":\"500\",\"margin_balance\":\"1000\"}")
    );
    assert!(new_text.contains("\"rule\":\"account_mm\""));

    // A record of a build before the field: the policy and the report as
    // that build wrote them.
    let before: Record = serde_json::from_str(
        r#"{"seq":1,"at":5,"input":{"venue":{"margin_report":{"session":1,"seq":1,
        "queried_at":4,"account_value":"100","positions":[]}}},
        "events":[{"margin_reported":{"report":{"session":1,"seq":1,"queried_at":4,
        "account_value":"100","positions":[]}}}]}"#,
    )
    .unwrap();
    let Event::MarginReported { report } = &before.events[0] else {
        panic!("{before:?}")
    };
    assert_eq!(report.account_mm, None);
    let policy: Policy = serde_json::from_str(r#"{"margin":{"mm_buffer_bps":25}}"#).unwrap();
    assert_eq!(policy.margin.unwrap().account_mm_rate_max_bps, None);
}

// ---- configuration ------------------------------------------------------------

/// U17d. The limit, when set, is 1..=9999 bps: 0 would latch on every
/// report, 10 000 is the venue's own liquidation.
#[test]
fn the_limit_is_one_to_9999_bps() {
    for (limit, ok) in [
        (None, true),
        (Some(0), false),
        (Some(1), true),
        (Some(5000), true),
        (Some(9999), true),
        (Some(10_000), false),
    ] {
        let r = config_with_limit(limit).validate();
        if ok {
            assert_eq!(r, Ok(()), "{limit:?}");
        } else {
            assert_eq!(
                r,
                Err(vec![
                    "policy.margin: account_mm_rate_max_bps must be 1..=9999".to_string()
                ]),
                "{limit:?}"
            );
        }
    }
}
