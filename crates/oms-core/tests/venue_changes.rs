//! Executions that are not fills of our orders (U13b): funding payments,
//! position changes made by the venue, executions the gateway could not
//! classify, and the position of record after a venue change.

#![allow(clippy::unwrap_used)]

mod common;
use common::*;
use oms_core::*;

fn names(out: &Output) -> Vec<&'static str> {
    out.events.iter().map(Event::name).collect()
}

fn funding(key: &str, amount: &str, venue_ts: Ts) -> VenueInput {
    VenueInput::Funding(FundingIn {
        key: fkey(key),
        instrument: iid("X"),
        amount: amt(amount),
        rate: "0.0001".into(),
        venue_ts,
    })
}

fn change(key: &str, kind: VenueChangeKind, qty: &str, venue_ts: Ts) -> VenueInput {
    VenueInput::PositionChange(VenuePositionChangeIn {
        key: fkey(key),
        kind,
        instrument: iid("X"),
        qty: sq(qty),
        px: px("100"),
        venue_ts,
    })
}

fn positions(h: &mut H, queried_at: Ts, qty: &str) -> Output {
    let session = h.session;
    h.venue(VenueInput::Positions {
        session,
        queried_at,
        positions: vec![VenuePosition {
            instrument: iid("X"),
            qty: sq(qty),
        }],
    })
}

fn unattributed(h: &H) -> SQty {
    h.core
        .unattributed
        .get(&iid("X"))
        .map(|b| b.pos)
        .unwrap_or_default()
}

/// The position a reader of the journal alone computes: the signed sum of
/// every quantity the journal books to the account.
fn journal_position(h: &H) -> i64 {
    let mut sum = 0i64;
    for rec in &h.log {
        for ev in &rec.events {
            sum += match ev {
                Event::FillBooked { order_id, qty, .. } => h
                    .core
                    .order(*order_id)
                    .map(|o| qty.signed(o.side).0)
                    .unwrap(),
                Event::ExternalFillBooked { fill } => fill.qty.signed(fill.side).0,
                Event::VenuePositionChanged { change } => change.qty.0,
                Event::PositionAdjusted { delta, .. } => delta.0,
                _ => 0,
            };
        }
    }
    sum
}

#[test]
fn a_funding_payment_is_cash_only_and_booked_once() {
    let mut h = H::ready();
    let id = h.accept("a", "k1", Side::Buy, "1", "100");
    h.placed_open(id, "v1");
    h.fill(id, "t1", "0.4", "100");
    let (acct, book, order) = (h.acct(), h.book("a"), h.order(id).clone());
    let fills = h.core.stats.fills_booked;

    let ts = h.now;
    let out = h.venue(funding("f1", "0.25", ts));
    assert_eq!(names(&out), vec!["funding_booked"]);
    assert_eq!(h.acct(), acct, "no position, open quantity or fee changes");
    assert_eq!(h.book("a"), book);
    assert_eq!(h.order(id), &order, "no order or reservation changes");
    assert_eq!(
        h.core.stats.fills_booked, fills,
        "a funding payment is not a fill"
    );
    assert_eq!(h.core.holds(None), vec![], "funding holds nothing");

    let again = h.venue(funding("f1", "0.25", ts));
    assert!(again.events.is_empty(), "a re-delivery is ignored");
    assert_eq!(h.rebuilt(), h.core);
}

#[test]
fn a_venue_position_change_is_booked_flagged_and_holds_plain_orders() {
    let mut h = H::ready_with(config_with_margin());
    h.report_margin("1000");
    h.ref_price("X", "100");
    h.long("a", "k1", "2");

    let ts = h.now;
    let out = h.venue(change("adl1", VenueChangeKind::Adl, "-0.5", ts));
    assert_eq!(
        names(&out),
        vec!["venue_position_changed", "mismatch_raised"]
    );
    assert_eq!(h.acct().pos, sq("1.5"));
    assert_eq!(
        h.book("a").pos,
        sq("2"),
        "the strategy ledger did not make it"
    );
    assert_eq!(unattributed(&h), sq("-0.5"));
    assert_eq!(
        h.unacked_mismatches(),
        vec![MismatchKind::VenuePositionChange]
    );
    assert!(h.core.holds(None).contains(&HoldReason::Mismatch));

    h.report_margin("1000");
    assert!(
        h.reject("a", "k2", Side::Buy, "0.1", "100")
            .contains(&RejectReason::Hold(HoldReason::Mismatch))
    );
    let r = h.reduce_only_req("a", "k3", Side::Sell, "0.5", "100");
    assert!(
        matches!(h.new_order(r), NewOrderReply::Accepted { .. }),
        "a reduce-only order passes"
    );

    let again = h.venue(change("adl1", VenueChangeKind::Adl, "-0.5", ts));
    assert!(again.events.is_empty(), "a re-delivery is ignored");

    let rebuilt = h.rebuilt();
    assert_eq!(rebuilt, h.core, "the hold and the position survive replay");
    assert_eq!(journal_position(&h), h.acct().pos.0);
}

#[test]
fn an_unclassified_execution_holds_plain_orders() {
    let mut h = H::ready();
    let ts = h.now;
    let out = h.venue(VenueInput::UnclassifiedExecution {
        key: fkey("e1"),
        instrument: iid("X"),
        exec_type: "SomethingNew".into(),
        detail: "execType SomethingNew, execQty 1".into(),
        venue_ts: ts,
    });
    assert_eq!(
        names(&out),
        vec!["unclassified_execution_noted", "mismatch_raised"]
    );
    assert_eq!(
        h.unacked_mismatches(),
        vec![MismatchKind::UnclassifiedExecution]
    );
    assert!(h.core.holds(None).contains(&HoldReason::Mismatch));
    assert_eq!(h.acct().pos, SQty::ZERO, "nothing is booked");
    assert_eq!(h.rebuilt(), h.core);
}

#[test]
fn the_next_positions_answer_after_a_change_is_the_position_of_record() {
    let mut h = H::ready();
    h.long("a", "k1", "2");
    let ts = h.now;
    h.venue(change("bust1", VenueChangeKind::Takeover, "1", ts));
    assert_eq!(h.acct().pos, sq("3"));

    // Asked before the change was booked: says nothing about it.
    let before = h.now - 1;
    let out = positions(&mut h, before, "2.7");
    assert!(!names(&out).contains(&"position_adjusted"));
    assert!(!names(&out).contains(&"venue_change_settled"));

    // The venue says 2.7, the journal 3: one adjustment of -0.3.
    let after = h.now + 1;
    let out = positions(&mut h, after, "2.7");
    let adjusted: Vec<&Event> = out
        .events
        .iter()
        .filter(|e| matches!(e, Event::PositionAdjusted { .. }))
        .collect();
    assert_eq!(adjusted.len(), 1);
    let Event::PositionAdjusted {
        ledger,
        delta,
        reason,
        ..
    } = adjusted[0]
    else {
        unreachable!()
    };
    assert_eq!(
        (ledger, *delta, *reason),
        (
            &Ledger::Unattributed,
            sq("-0.3"),
            AdjustReason::SnapshotAfterVenueChange
        )
    );
    assert_eq!(h.acct().pos, sq("2.7"));
    assert_eq!(
        journal_position(&h),
        sq("2.7").0,
        "the journal alone gives the position"
    );
    assert_eq!(h.rebuilt(), h.core);

    // The wait is over: a later answer is range-checked as before, not set.
    let later = h.now + 1;
    let out = positions(&mut h, later, "2.7");
    assert!(!names(&out).contains(&"position_adjusted"));
}

#[test]
fn a_positions_answer_equal_to_the_journal_settles_without_an_adjustment() {
    let mut h = H::ready();
    let ts = h.now;
    h.venue(change("dlv1", VenueChangeKind::Delivery, "-1", ts));
    let after = h.now + 1;
    let out = positions(&mut h, after, "-1");
    assert!(names(&out).contains(&"venue_change_settled"));
    assert!(!names(&out).contains(&"position_adjusted"));
    assert_eq!(h.acct().pos, sq("-1"));
    assert!(h.core.venue_change_wait.is_empty());
    assert_eq!(h.rebuilt(), h.core);
}

#[test]
fn a_positions_answer_of_an_old_session_does_not_end_the_wait() {
    let mut h = H::ready();
    let ts = h.now;
    h.venue(change("s1", VenueChangeKind::Settle, "1", ts));
    let after = h.now + 1;
    let out = h.venue(VenueInput::Positions {
        session: h.session - 1,
        queried_at: after,
        positions: vec![],
    });
    assert!(out.events.is_empty());
    assert!(!h.core.venue_change_wait.is_empty());
}

#[test]
fn an_operator_adjustment_is_written_as_in_schema_1() {
    let ev = Event::PositionAdjusted {
        instrument: iid("X"),
        ledger: Ledger::Unattributed,
        delta: sq("1"),
        note: "n".into(),
        reason: AdjustReason::Operator,
    };
    let json = serde_json::to_string(&ev).unwrap();
    assert_eq!(
        json,
        r#"{"position_adjusted":{"instrument":"X","ledger":"unattributed","delta":"1","note":"n"}}"#
    );
    let back: Event = serde_json::from_str(&json).unwrap();
    assert_eq!(back, ev);
}
