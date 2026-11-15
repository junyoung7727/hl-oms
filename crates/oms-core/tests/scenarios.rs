//! Worked business scenarios. Expected values are written out by hand from
//! the requirement documents, not derived from the implementation.
//!
//! Contract ids (C-…) refer to docs/contracts/.

#![allow(clippy::unwrap_used)]

mod common;
use common::*;
use oms_core::*;

// ---- C-NEW: admission, reservation, quantity bounds -------------------------

/// The worked example of docs/architecture/FIA-OMS-model.md §7.1:
/// limit [-10, 10], q starts at 0.
#[test]
fn fia_7_1_reservation_follows_potential_fills() {
    let mut h = H::ready();

    // 1. A buys 6: accepted, upper bound becomes 6.
    let a = h.accept("a", "a1", Side::Buy, "6", "100");
    assert_eq!(h.acct().pos, sq("0"));
    assert_eq!(h.acct().buy_open, q("6"));

    // 2. B wants to buy 5: 0 + 6 + 5 = 11 > 10, refused before any send.
    let why = h.reject("b", "b1", Side::Buy, "5", "100");
    assert_eq!(why, vec![RejectReason::AccountPositionLimit]);
    assert_eq!(h.sends(), vec![a]);

    // 3-4. Two units fill; booking moves them from reservation to position.
    h.placed_open(a, "v1");
    h.fill(a, "t1", "2", "100");
    assert_eq!(h.acct().pos, sq("2"));
    assert_eq!(h.acct().buy_open, q("4"));

    // 5. Cancel requested: nothing is released yet.
    assert_eq!(h.cancel("a", a), CancelReply::Requested { order_id: a });
    assert_eq!(h.acct().buy_open, q("4"));
    assert_eq!(h.cancels(), vec![(a, 1)]);

    // 6. One more unit fills before the cancel takes effect.
    h.fill(a, "t2", "1", "100");
    assert_eq!(h.acct().pos, sq("3"));
    assert_eq!(h.acct().buy_open, q("3"));

    // 7. Final cumulative 3 confirmed with the cancel: the rest is released.
    h.status(a, VenueStatus::Canceled { reason: "user".into() }, Some("3"));
    assert_eq!(h.order(a).phase, Phase::Closed);
    assert_eq!(h.acct().pos, sq("3"));
    assert_eq!(h.acct().buy_open, q("0"));
    assert_eq!(h.book("a").pos, sq("3"));
    assert_eq!(h.unacked_mismatches(), vec![]);
}

/// Two strategies cannot both consume the same remaining headroom
/// (oms-three-guarantees.md §5: "두 전략이 같은 잔여 한도 소비").
#[test]
fn concurrent_requests_cannot_double_spend_headroom() {
    let mut h = H::ready();
    h.accept("a", "a1", Side::Buy, "6", "100");
    assert_eq!(
        h.reject("b", "b1", Side::Buy, "6", "100"),
        vec![RejectReason::AccountPositionLimit]
    );
    // What still fits is accepted.
    h.accept("b", "b2", Side::Buy, "4", "100");
    assert_eq!(h.acct().buy_open, q("10"));
}

/// Pending orders on the other side are not netted against new exposure
/// (FIA-OMS-model.md §2.1 counterexample: buy 4 and sell 4 open, another buy 4
/// would allow +8 if only the buys fill).
#[test]
fn open_orders_on_the_other_side_do_not_create_headroom() {
    let mut cfg = config();
    let x = cfg.instruments.get_mut(&iid("X")).unwrap();
    x.account_pos_min = sq("-5");
    x.account_pos_max = sq("5");
    let mut h = H::ready_with(cfg);
    h.accept("a", "a1", Side::Buy, "4", "100");
    h.accept("a", "a2", Side::Sell, "4", "100");
    assert_eq!(
        h.reject("a", "a3", Side::Buy, "4", "100"),
        vec![RejectReason::AccountPositionLimit]
    );
}

/// A strategy closing its own position can still breach the account bound
/// (oms-three-guarantees.md §2.5: A=+80, B=-80, account limit ±50).
#[test]
fn strategy_reduction_is_not_account_reduction() {
    let mut cfg = config();
    let x = cfg.instruments.get_mut(&iid("X")).unwrap();
    x.account_pos_min = sq("-50");
    x.account_pos_max = sq("50");
    x.max_order_qty = q("100");
    for s in cfg.strategies.values_mut() {
        s.limits.insert(
            iid("X"),
            PosLimit {
                min: sq("-80"),
                max: sq("80"),
            },
        );
    }
    let mut h = H::ready_with(cfg);
    // Build A=+50 then B=-50 then A=+30, B=-30 so that account stays within ±50.
    for (i, (s, side, n)) in [
        ("a", Side::Buy, "50"),
        ("b", Side::Sell, "50"),
        ("a", Side::Buy, "30"),
        ("b", Side::Sell, "30"),
    ]
    .into_iter()
    .enumerate()
    {
        let id = h.accept(s, &format!("k{i}"), side, n, "100");
        h.placed_open(id, &format!("v{i}"));
        h.fill(id, &format!("t{i}"), n, "100");
        assert_eq!(h.order(id).phase, Phase::Closed);
    }
    assert_eq!(h.book("a").pos, sq("80"));
    assert_eq!(h.book("b").pos, sq("-80"));
    assert_eq!(h.acct().pos, sq("0"));
    // A selling its whole +80 would take the account to -80.
    assert_eq!(
        h.reject("a", "close", Side::Sell, "80", "100"),
        vec![RejectReason::AccountPositionLimit]
    );
    h.accept("a", "close50", Side::Sell, "50", "100");
}

#[test]
fn field_validation_reasons() {
    let mut h = H::ready();
    // qty step 0.001, px tick 0.1, at most 5 significant figures, min notional 10.
    assert_eq!(h.reject("a", "k1", Side::Buy, "0.0005", "100"), vec![
        RejectReason::QtyStep,
        RejectReason::BelowMinNotional
    ]);
    assert_eq!(h.reject("a", "k2", Side::Buy, "1", "100.05"), vec![RejectReason::PxTick]);
    // 1234.5 is on the tick and has exactly 5 significant figures.
    h.accept("a", "k3", Side::Buy, "1", "1234.5");
}

#[test]
fn significant_figure_rule_exempts_integers() {
    let mut h = H::ready();
    // 12345.6 has 6 significant figures and is not an integer.
    assert_eq!(h.reject("a", "k1", Side::Buy, "0.001", "12345.6"), vec![RejectReason::PxSigFigs]);
    // 123456 has 6 digits but is an integer price, which the rule allows.
    h.accept("a", "k2", Side::Buy, "0.001", "123456");
    assert_eq!(h.reject("a", "k3", Side::Buy, "0.001", "100"), vec![RejectReason::BelowMinNotional]);
    assert_eq!(h.reject("a", "k4", Side::Buy, "200", "100"), vec![
        RejectReason::MaxOrderQty,
        RejectReason::AccountPositionLimit,
        RejectReason::StrategyPositionLimit
    ]);
    assert_eq!(h.reject("zz", "k5", Side::Buy, "1", "100"), vec![RejectReason::UnknownStrategy]);
}

#[test]
fn price_band_bounds_only_the_aggressive_side_and_needs_a_fresh_reference() {
    let mut cfg = config();
    cfg.instruments.get_mut(&iid("X")).unwrap().price_band_bps = Some(500); // 5%
    let mut h = H::ready_with(cfg);
    assert_eq!(
        h.reject("a", "k0", Side::Buy, "1", "100"),
        vec![RejectReason::RefPriceUnavailable]
    );
    h.venue(VenueInput::RefPrice {
        instrument: iid("X"),
        px: px("100"),
        venue_ts: 0,
    });
    // 5% above 100 is 105: 105 passes, 105.1 does not.
    h.accept("a", "k1", Side::Buy, "1", "105");
    assert_eq!(h.reject("a", "k2", Side::Buy, "1", "105.1"), vec![RejectReason::PriceBand]);
    // A passive buy far below is allowed.
    h.accept("a", "k3", Side::Buy, "1", "50");
    assert_eq!(h.reject("a", "k4", Side::Sell, "1", "94.9"), vec![RejectReason::PriceBand]);
    h.accept("a", "k5", Side::Sell, "1", "95");
    // The reference price goes stale after 15 s.
    h.advance(16 * SEC);
    assert_eq!(
        h.reject("a", "k6", Side::Buy, "1", "100"),
        vec![RejectReason::RefPriceUnavailable]
    );
}

// ---- C-IDEM: idempotency -----------------------------------------------------

#[test]
fn same_key_same_request_creates_one_order_and_one_send() {
    let mut h = H::ready();
    let r = h.req("a", "k1", Side::Buy, "1", "100");
    let first = h.new_order(r.clone());
    let NewOrderReply::Accepted { order_id } = first else {
        panic!("{first:?}")
    };
    let journal_len = h.log.len();
    let again = h.new_order(r.clone());
    assert_eq!(again, NewOrderReply::Duplicate {
        order_id,
        phase: Phase::PendingSend,
        cum_qty: q("0")
    });
    // The duplicate changed nothing: no event, no command, no reservation.
    assert_eq!(h.log.len(), journal_len);
    assert_eq!(h.sends(), vec![order_id]);
    assert_eq!(h.acct().buy_open, q("1"));
}

#[test]
fn same_key_different_request_is_a_conflict_without_effect() {
    let mut h = H::ready();
    let r = h.req("a", "k1", Side::Buy, "1", "100");
    let NewOrderReply::Accepted { order_id } = h.new_order(r.clone()) else {
        panic!()
    };
    let mut other = r.clone();
    other.qty = q("2");
    assert_eq!(h.new_order(other), NewOrderReply::Conflict { order_id });
    assert_eq!(h.sends().len(), 1);
    assert_eq!(h.acct().buy_open, q("1"));
}

#[test]
fn the_key_scope_is_per_strategy() {
    let mut h = H::ready();
    let a = h.accept("a", "k1", Side::Buy, "1", "100");
    let b = h.accept("b", "k1", Side::Buy, "1", "100");
    assert_ne!(a, b);
}

#[test]
fn a_rejection_does_not_consume_the_key() {
    let mut h = H::ready();
    h.op(OperatorCmd::Halt {
        scope: Scope::Account,
        note: "t".into(),
    })
    .unwrap();
    let r = h.req("a", "k1", Side::Buy, "1", "100");
    assert_eq!(h.new_order(r.clone()), NewOrderReply::Rejected {
        reasons: vec![RejectReason::Hold(HoldReason::OperatorAccount)]
    });
    h.op(OperatorCmd::Resume {
        scope: Scope::Account,
        note: "t".into(),
    })
    .unwrap();
    assert!(matches!(h.new_order(r), NewOrderReply::Accepted { .. }));
}

#[test]
fn the_duplicate_answer_survives_restart() {
    let mut h = H::ready();
    let r = h.req("a", "k1", Side::Buy, "1", "100");
    let NewOrderReply::Accepted { order_id } = h.new_order(r.clone()) else {
        panic!()
    };
    h.placed_open(order_id, "v1");
    // Restart: state is rebuilt from the journal alone.
    let mut restarted = H::new();
    restarted.core = h.rebuilt();
    restarted.shadow = restarted.core.clone();
    restarted.now = h.now;
    restarted.inp(Input::Boot { binary: "test".into() });
    assert_eq!(restarted.new_order(r), NewOrderReply::Duplicate {
        order_id,
        phase: Phase::Working,
        cum_qty: q("0")
    });
    assert_eq!(restarted.sends(), vec![]);
}

#[test]
fn a_key_is_forgotten_only_after_its_request_expired() {
    let mut cfg = config();
    cfg.policy.closed_retention_us = SEC;
    cfg.policy.dedup_grace_us = 2 * SEC;
    cfg.policy.purge_interval_us = SEC;
    let mut h = H::ready_with(cfg);
    let r = h.req("a", "k1", Side::Buy, "1", "100"); // expires in 5 s
    let NewOrderReply::Accepted { order_id } = h.new_order(r.clone()) else {
        panic!()
    };
    h.venue(VenueInput::SendResult {
        order_id,
        outcome: SendOutcome::Rejected { reason: "x".into() },
        timing: Default::default(),
    });
    assert_eq!(h.order(order_id).phase, Phase::Closed);

    // 3 s later the retention (1 s) has passed, the request has not expired.
    h.advance(3 * SEC);
    h.tick();
    assert!(h.core.order(order_id).is_some(), "key must still be remembered");
    assert!(matches!(h.new_order(r.clone()), NewOrderReply::Duplicate { .. }));

    // After expiry (5 s) plus grace (2 s) the order is purged ...
    h.advance(5 * SEC);
    h.tick();
    assert!(h.core.order(order_id).is_none());
    // ... and a late duplicate is refused as expired, so no second order.
    assert_eq!(h.new_order(r), NewOrderReply::Rejected {
        reasons: vec![RejectReason::Expired]
    });
    assert_eq!(h.sends(), vec![order_id]);
}

#[test]
fn expiry_rules() {
    let mut h = H::ready();
    let mut r = h.req("a", "k1", Side::Buy, "1", "100");
    r.expires_at = h.now; // not in the future once processed
    assert_eq!(h.new_order(r), NewOrderReply::Rejected {
        reasons: vec![RejectReason::Expired]
    });
    let mut r = h.req("a", "k2", Side::Buy, "1", "100");
    r.expires_at = h.now + 61 * SEC; // horizon is 60 s
    assert_eq!(h.new_order(r), NewOrderReply::Rejected {
        reasons: vec![RejectReason::ExpiryTooFar]
    });
}

// ---- C-SEND / C-DOUBT: unknown outcome ---------------------------------------

/// hyperliquid-trading/docs/state-machine/02-recovery.md, case A: no answer is
/// not "not sent".
#[test]
fn unknown_send_outcome_keeps_the_reservation_and_blocks_new_exposure() {
    let mut h = H::ready();
    let a = h.accept("a", "k1", Side::Buy, "4", "100");
    h.venue(VenueInput::SendResult {
        order_id: a,
        outcome: SendOutcome::Unknown { reason: "timeout".into() },
        timing: Default::default(),
    });
    let o = h.order(a);
    assert_eq!(o.phase, Phase::PendingSend);
    assert_eq!(o.doubt.as_ref().map(|d| d.reason), Some(DoubtReason::Transport));
    assert_eq!(h.acct().buy_open, q("4"), "an unknown result must not release");
    assert_eq!(
        h.reject("b", "k2", Side::Buy, "1", "100"),
        vec![RejectReason::Hold(HoldReason::InDoubt)]
    );
    // No resend of any kind.
    assert_eq!(h.sends(), vec![a]);
    assert_eq!(h.core.needs(), vec![Need::OrderStatus {
        order_id: a,
        venue_oid: None,
        why: needs::NeedReason::InDoubt
    }]);
}

#[test]
fn in_doubt_order_found_by_query_becomes_working() {
    let mut h = H::ready();
    let a = h.accept("a", "k1", Side::Buy, "4", "100");
    h.venue(VenueInput::SendResult {
        order_id: a,
        outcome: SendOutcome::Unknown { reason: "timeout".into() },
        timing: Default::default(),
    });
    let session = h.session;
    let venue_ts = h.now;
    h.venue(VenueInput::OrderStatus(OrderStatusIn {
        session,
        order_id: Some(a),
        venue_oid: Some(void("v9")),
        status: StatusIn {
            status: VenueStatus::Open,
            cum: Some(q("0")),
            venue_ts,
        },
        source: EvidenceSource::Query,
    }));
    assert_eq!(h.order(a).phase, Phase::Working);
    assert_eq!(h.order(a).doubt, None);
    assert_eq!(h.core.holds(None), vec![]);
    assert_eq!(h.sends(), vec![a]);
}

/// "Not found" proves absence only after the send can no longer be placed.
#[test]
fn not_found_closes_an_in_doubt_order_only_after_expiry_plus_margin() {
    let mut h = H::ready();
    let a = h.accept("a", "k1", Side::Buy, "4", "100");
    let expires_at = h.order(a).expires_at;
    h.venue(VenueInput::SendResult {
        order_id: a,
        outcome: SendOutcome::Unknown { reason: "timeout".into() },
        timing: Default::default(),
    });
    let session = h.session;

    // Queried before expiry + 10 s margin: the request may still land.
    h.venue(VenueInput::StatusNotFound {
        session,
        order_id: a,
        queried_at: expires_at + 9 * SEC,
    });
    assert_eq!(h.order(a).phase, Phase::PendingSend);
    assert_eq!(h.acct().buy_open, q("4"));

    h.venue(VenueInput::StatusNotFound {
        session,
        order_id: a,
        queried_at: expires_at + 10 * SEC,
    });
    assert_eq!(h.order(a).phase, Phase::Closed);
    assert!(matches!(
        h.order(a).close.as_ref().unwrap().reason,
        CloseReason::NotPlaced { .. }
    ));
    assert_eq!(h.acct().buy_open, q("0"));
    assert_eq!(h.core.holds(None), vec![]);
}

#[test]
fn not_found_never_closes_when_the_venue_does_not_enforce_expiry() {
    let mut cfg = config();
    cfg.policy.venue_enforces_expiry = false;
    let mut h = H::ready_with(cfg);
    let a = h.accept("a", "k1", Side::Buy, "4", "100");
    let expires_at = h.order(a).expires_at;
    h.venue(VenueInput::SendResult {
        order_id: a,
        outcome: SendOutcome::Unknown { reason: "timeout".into() },
        timing: Default::default(),
    });
    let session = h.session;
    h.venue(VenueInput::StatusNotFound {
        session,
        order_id: a,
        queried_at: expires_at + 3600 * SEC,
    });
    assert_eq!(h.order(a).phase, Phase::PendingSend);
    // Only an operator, on stated evidence, can close it.
    h.op(OperatorCmd::ResolveOrder {
        order_id: a,
        note: "checked on venue UI: no such order".into(),
    })
    .unwrap();
    assert_eq!(h.order(a).phase, Phase::Closed);
}

#[test]
fn not_found_is_ignored_while_the_send_is_simply_outstanding() {
    let mut h = H::ready();
    let a = h.accept("a", "k1", Side::Buy, "4", "100");
    let session = h.session;
    let late = h.order(a).expires_at + 100 * SEC;
    h.venue(VenueInput::StatusNotFound {
        session,
        order_id: a,
        queried_at: late,
    });
    assert_eq!(h.order(a).phase, Phase::PendingSend);
}

#[test]
fn definite_gateway_failures_close_and_release() {
    let mut h = H::ready();
    let a = h.accept("a", "k1", Side::Buy, "4", "100");
    h.venue(VenueInput::SendResult {
        order_id: a,
        outcome: SendOutcome::NotSent { reason: "connect refused".into() },
        timing: Default::default(),
    });
    assert!(matches!(h.order(a).close.as_ref().unwrap().reason, CloseReason::NotSent { .. }));
    let b = h.accept("a", "k2", Side::Buy, "4", "100");
    h.venue(VenueInput::SendResult {
        order_id: b,
        outcome: SendOutcome::Rejected { reason: "bad price".into() },
        timing: Default::default(),
    });
    assert!(matches!(
        h.order(b).close.as_ref().unwrap().reason,
        CloseReason::RejectedByVenue { .. }
    ));
    assert_eq!(h.acct().buy_open, q("0"));
    assert_eq!(h.acct().open_orders, 0);
}

#[test]
fn a_send_without_any_result_becomes_in_doubt_after_the_timeout() {
    let mut h = H::ready();
    let a = h.accept("a", "k1", Side::Buy, "4", "100");
    h.advance(29 * SEC);
    h.tick();
    assert_eq!(h.order(a).doubt, None);
    h.advance(2 * SEC);
    h.tick();
    assert_eq!(h.order(a).doubt.as_ref().map(|d| d.reason), Some(DoubtReason::NoResult));
}

// ---- C-FILL: fills, order, duplicates ----------------------------------------

#[test]
fn duplicate_fill_is_booked_once() {
    let mut h = H::ready();
    let a = h.accept("a", "k1", Side::Buy, "4", "100");
    h.placed_open(a, "v1");
    h.fill(a, "t1", "1", "100");
    let n = h.log.len();
    h.fill(a, "t1", "1", "100");
    assert_eq!(h.log.len(), n, "a duplicate fill must not produce a record");
    assert_eq!(h.acct().pos, sq("1"));
    assert_eq!(h.order(a).cum_qty, q("1"));
}

#[test]
fn fill_before_send_result_acknowledges_the_order() {
    let mut h = H::ready();
    let a = h.accept("a", "k1", Side::Buy, "4", "100");
    // The stream delivers the fill before the HTTP answer.
    h.venue(VenueInput::Fill(FillIn {
        key: fkey("t1"),
        order_id: Some(a),
        venue_oid: Some(void("v1")),
        instrument: iid("X"),
        side: Side::Buy,
        qty: q("1"),
        px: px("100"),
        fee: amt("0.05"),
        venue_ts: h.now,
    }));
    assert_eq!(h.order(a).phase, Phase::Working);
    assert_eq!(h.order(a).venue_oid, Some(void("v1")));
    // The late answer adds nothing and breaks nothing.
    h.placed_open(a, "v1");
    assert_eq!(h.order(a).cum_qty, q("1"));
    assert_eq!(h.order(a).fee, amt("0.05"));
    // A late "unknown" must not put an acknowledged order in doubt.
    h.venue(VenueInput::SendResult {
        order_id: a,
        outcome: SendOutcome::Unknown { reason: "timeout".into() },
        timing: Default::default(),
    });
    assert_eq!(h.order(a).doubt, None);
}

/// A fill that only carries the venue's order id, arriving before that id is
/// known, waits for its order instead of being booked as foreign.
#[test]
fn fill_by_venue_id_before_the_id_is_known_is_parked_then_booked() {
    let mut h = H::ready();
    let a = h.accept("a", "k1", Side::Buy, "4", "100");
    h.venue(VenueInput::Fill(FillIn {
        key: fkey("t1"),
        order_id: None,
        venue_oid: Some(void("v1")),
        instrument: iid("X"),
        side: Side::Buy,
        qty: q("1"),
        px: px("100"),
        fee: Amount::ZERO,
        venue_ts: h.now,
    }));
    assert_eq!(h.core.parked_fills.len(), 1);
    assert_eq!(h.acct().pos, sq("0"));
    h.placed_open(a, "v1");
    assert_eq!(h.core.parked_fills.len(), 0);
    assert_eq!(h.acct().pos, sq("1"));
    assert_eq!(h.order(a).cum_qty, q("1"));
    assert_eq!(h.unacked_mismatches(), vec![]);
}

#[test]
fn unidentified_fill_is_booked_as_external_after_the_wait_and_holds_trading() {
    let mut h = H::ready();
    h.accept("a", "k1", Side::Buy, "4", "100");
    h.venue(VenueInput::Fill(FillIn {
        key: fkey("t1"),
        order_id: None,
        venue_oid: Some(void("other")),
        instrument: iid("X"),
        side: Side::Buy,
        qty: q("1"),
        px: px("100"),
        fee: Amount::ZERO,
        venue_ts: h.now,
    }));
    assert_eq!(h.core.parked_fills.len(), 1);
    h.advance(121 * SEC);
    h.tick();
    // It is never dropped: booked to the account and the unattributed ledger.
    assert_eq!(h.core.parked_fills.len(), 0);
    assert_eq!(h.acct().pos, sq("1"));
    assert_eq!(h.core.unattributed[&iid("X")].pos, sq("1"));
    assert_eq!(h.book("a").pos, sq("0"));
    assert_eq!(h.unacked_mismatches(), vec![MismatchKind::ExternalFill]);
    // Order a has had no send result for 121 s (> 30 s), so it is in doubt
    // as well; both holds are reported.
    assert_eq!(
        h.reject("b", "k2", Side::Buy, "1", "100"),
        vec![
            RejectReason::Hold(HoldReason::InDoubt),
            RejectReason::Hold(HoldReason::Mismatch)
        ]
    );
}

/// Once no order of ours can turn out to own a parked fill, it is external
/// without waiting for the timeout.
#[test]
fn a_parked_fill_is_external_once_no_order_can_own_it() {
    let mut h = H::ready();
    let a = h.accept("a", "k1", Side::Buy, "4", "100");
    h.venue(VenueInput::Fill(FillIn {
        key: fkey("t1"),
        order_id: None,
        venue_oid: Some(void("other")),
        instrument: iid("X"),
        side: Side::Buy,
        qty: q("1"),
        px: px("100"),
        fee: Amount::ZERO,
        venue_ts: h.now,
    }));
    assert_eq!(h.core.parked_fills.len(), 1);
    h.tick();
    assert_eq!(h.core.parked_fills.len(), 1, "order a could still be its owner");
    // Order a turns out to have a different venue id.
    h.placed_open(a, "v1");
    h.tick();
    assert_eq!(h.core.parked_fills.len(), 0);
    assert_eq!(h.core.unattributed[&iid("X")].pos, sq("1"));
    assert_eq!(h.order(a).cum_qty, q("0"));
    assert_eq!(h.unacked_mismatches(), vec![MismatchKind::ExternalFill]);
}

#[test]
fn overfill_is_booked_in_full_and_flagged() {
    let mut h = H::ready();
    let a = h.accept("a", "k1", Side::Buy, "2", "100");
    h.placed_open(a, "v1");
    h.fill(a, "t1", "2", "100");
    assert_eq!(h.order(a).phase, Phase::Closed);
    // The venue reports one more unit on a completed order.
    h.fill(a, "t2", "1", "100");
    assert_eq!(h.acct().pos, sq("3"), "the real fill is not clipped");
    assert_eq!(h.order(a).cum_qty, q("3"));
    let kinds = h.unacked_mismatches();
    assert!(kinds.contains(&MismatchKind::Overfill));
    assert!(kinds.contains(&MismatchKind::FillAfterClose));
}

#[test]
fn full_fill_by_fills_alone_closes_the_order() {
    let mut h = H::ready();
    let a = h.accept("a", "k1", Side::Sell, "2", "100");
    h.placed_open(a, "v1");
    h.fill(a, "t1", "0.5", "100");
    assert_eq!(h.order(a).phase, Phase::Working);
    h.fill(a, "t2", "1.5", "101");
    assert_eq!(h.order(a).phase, Phase::Closed);
    assert_eq!(h.order(a).close.as_ref().unwrap().reason, CloseReason::Filled);
    assert_eq!(h.acct().pos, sq("-2"));
    // 0.5*100 + 1.5*101 = 201.5
    assert_eq!(h.order(a).cum_notional, amt("201.5"));
}

// ---- C-CANCEL: cancel and fill races ------------------------------------------

/// 02-recovery.md case B, rows 4a and 4b.
#[test]
fn cancel_race_final_status_decides() {
    // 4a: cancelled with 0.4 filled.
    let mut h = H::ready();
    let a = h.accept("a", "k1", Side::Buy, "1", "100");
    h.placed_open(a, "v1");
    h.fill(a, "t1", "0.4", "100");
    h.cancel("a", a);
    h.venue(VenueInput::CancelResult {
        order_id: a,
        attempt: 1,
        outcome: CancelOutcome::Confirmed,
        timing: Default::default(),
    });
    assert_eq!(h.order(a).phase, Phase::Working, "a confirmed cancel is not terminal evidence");
    assert_eq!(h.acct().buy_open, q("0.6"));
    h.status(a, VenueStatus::Canceled { reason: "user".into() }, Some("0.4"));
    assert_eq!(h.order(a).phase, Phase::Closed);
    assert_eq!(h.acct().pos, sq("0.4"));

    // 4b: the cancel lost the race; the order filled completely.
    let mut h = H::ready();
    let a = h.accept("a", "k1", Side::Buy, "1", "100");
    h.placed_open(a, "v1");
    h.fill(a, "t1", "0.4", "100");
    h.cancel("a", a);
    h.venue(VenueInput::CancelResult {
        order_id: a,
        attempt: 1,
        outcome: CancelOutcome::Rejected {
            reason: "already filled".into(),
        },
        timing: Default::default(),
    });
    h.fill(a, "t2", "0.6", "100");
    assert_eq!(h.order(a).phase, Phase::Closed);
    assert_eq!(h.order(a).close.as_ref().unwrap().reason, CloseReason::Filled);
    assert_eq!(h.acct().pos, sq("1"));
}

/// 02-recovery.md case B, row 4c: a final quantity below what was already
/// booked is a contradiction, not a correction.
#[test]
fn final_quantity_below_booked_fills_is_flagged() {
    let mut h = H::ready();
    let a = h.accept("a", "k1", Side::Buy, "1", "100");
    h.placed_open(a, "v1");
    h.fill(a, "t1", "0.4", "100");
    h.status(a, VenueStatus::Canceled { reason: "user".into() }, Some("0"));
    assert_eq!(h.acct().pos, sq("0.4"), "booked fills are never taken back");
    assert_eq!(h.unacked_mismatches(), vec![MismatchKind::CumExceedsFinal]);
}

/// Terminal status arrives before the fills it implies: the order waits in
/// `Closing` with the unbooked quantity still reserved.
#[test]
fn terminal_before_fills_keeps_the_unbooked_quantity_reserved() {
    let mut h = H::ready();
    let a = h.accept("a", "k1", Side::Buy, "1", "100");
    h.placed_open(a, "v1");
    h.status(a, VenueStatus::Canceled { reason: "user".into() }, Some("0.4"));
    assert_eq!(h.order(a).phase, Phase::Closing);
    assert_eq!(h.acct().buy_open, q("0.4"));
    assert_eq!(h.acct().pos, sq("0"));
    h.fill(a, "t1", "0.4", "100");
    assert_eq!(h.order(a).phase, Phase::Closed);
    assert_eq!(h.acct().buy_open, q("0"));
    assert_eq!(h.acct().pos, sq("0.4"));
}

/// When the venue does not state the final quantity, the order closes only
/// once fills are known to be complete up to the terminal time.
#[test]
fn terminal_without_final_quantity_waits_for_fill_sync() {
    let mut h = H::ready();
    let a = h.accept("a", "k1", Side::Buy, "1", "100");
    h.placed_open(a, "v1");
    h.fill(a, "t1", "0.3", "100");
    h.status(a, VenueStatus::Canceled { reason: "user".into() }, None);
    let terminal_ts = h.order(a).terminal.as_ref().unwrap().venue_ts;
    assert_eq!(h.order(a).phase, Phase::Closing);
    assert_eq!(h.acct().buy_open, q("0.7"), "nothing is released on an unknown final quantity");
    assert_eq!(h.core.needs(), vec![Need::Fills {
        since: h.core.sync.fills_up_to
    }]);

    // A fill that the stream had not delivered yet arrives through backfill.
    h.fill(a, "t2", "0.2", "100");
    let session = h.session;
    h.venue(VenueInput::FillsSynced {
        session,
        up_to: terminal_ts - 1,
    });
    assert_eq!(h.order(a).phase, Phase::Closing, "sync does not yet cover the terminal time");
    h.venue(VenueInput::FillsSynced {
        session,
        up_to: terminal_ts,
    });
    assert_eq!(h.order(a).phase, Phase::Closed);
    assert_eq!(h.acct().pos, sq("0.5"));
    assert_eq!(h.acct().buy_open, q("0"));
}

#[test]
fn missing_fills_after_sync_are_flagged_and_stay_reserved() {
    let mut h = H::ready();
    let a = h.accept("a", "k1", Side::Buy, "1", "100");
    h.placed_open(a, "v1");
    h.status(a, VenueStatus::Canceled { reason: "user".into() }, Some("0.4"));
    let session = h.session;
    let up_to = h.now;
    h.venue(VenueInput::FillsSynced { session, up_to });
    assert_eq!(h.order(a).phase, Phase::Closing);
    assert_eq!(h.acct().buy_open, q("0.4"));
    assert_eq!(h.unacked_mismatches(), vec![MismatchKind::MissingFills]);
}

#[test]
fn cancel_is_authorised_per_owner_and_idempotent() {
    let mut h = H::ready();
    let a = h.accept("a", "k1", Side::Buy, "1", "100");
    h.placed_open(a, "v1");
    assert_eq!(h.cancel("b", a), CancelReply::NotOwner);
    assert_eq!(h.cancel("a", a), CancelReply::Requested { order_id: a });
    assert_eq!(h.cancel("a", a), CancelReply::AlreadyRequested { order_id: a });
    assert_eq!(h.cancels(), vec![(a, 1)]);
    assert_eq!(h.cancel("a", OrderId(999)), CancelReply::NotFound);
}

#[test]
fn cancel_of_an_unacknowledged_order_is_sent_once_the_order_is_acknowledged() {
    let mut h = H::ready();
    let a = h.accept("a", "k1", Side::Buy, "1", "100");
    assert_eq!(h.cancel("a", a), CancelReply::Requested { order_id: a });
    assert_eq!(h.cancels(), vec![], "the order may not have reached the venue yet");
    h.placed_open(a, "v1");
    assert_eq!(h.cancels(), vec![(a, 1)]);
}

#[test]
fn cancel_is_retried_with_backoff_and_then_flagged() {
    let mut cfg = config();
    cfg.policy.cancel_max_attempts = 2;
    let mut h = H::ready_with(cfg);
    let a = h.accept("a", "k1", Side::Buy, "1", "100");
    h.placed_open(a, "v1");
    h.cancel("a", a);
    h.venue(VenueInput::CancelResult {
        order_id: a,
        attempt: 1,
        outcome: CancelOutcome::Unknown { reason: "timeout".into() },
        timing: Default::default(),
    });
    h.tick();
    assert_eq!(h.cancels(), vec![(a, 1)], "backoff of 1 s has not passed");
    h.advance(SEC);
    h.tick();
    assert_eq!(h.cancels(), vec![(a, 1), (a, 2)]);
    h.venue(VenueInput::CancelResult {
        order_id: a,
        attempt: 2,
        outcome: CancelOutcome::Unknown { reason: "timeout".into() },
        timing: Default::default(),
    });
    h.advance(10 * SEC);
    h.tick();
    assert_eq!(h.cancels().len(), 2, "attempts are bounded");
    assert_eq!(h.unacked_mismatches(), vec![MismatchKind::CancelStuck]);
}

#[test]
fn cancels_are_accepted_while_new_orders_are_held() {
    let mut h = H::ready();
    let a = h.accept("a", "k1", Side::Buy, "1", "100");
    h.placed_open(a, "v1");
    h.op(OperatorCmd::Halt {
        scope: Scope::Account,
        note: "t".into(),
    })
    .unwrap();
    assert_eq!(
        h.reject("a", "k2", Side::Buy, "1", "100"),
        vec![RejectReason::Hold(HoldReason::OperatorAccount)]
    );
    assert_eq!(h.cancel("a", a), CancelReply::Requested { order_id: a });
    assert_eq!(h.cancels(), vec![(a, 1)]);
    // A late fill on a halted account is still booked.
    h.fill(a, "t1", "0.5", "100");
    assert_eq!(h.acct().pos, sq("0.5"));
}

// ---- C-LINK / C-RECON: link loss, resync, reconciliation ------------------------

#[test]
fn link_loss_blocks_new_orders_and_resync_is_required_after_it_returns() {
    let mut h = H::ready();
    let a = h.accept("a", "k1", Side::Buy, "1", "100");
    h.placed_open(a, "v1");
    h.venue(VenueInput::Link {
        up: false,
        session: h.session,
        reason: "ws closed".into(),
    });
    assert_eq!(
        h.reject("a", "k2", Side::Buy, "1", "100"),
        vec![RejectReason::Hold(HoldReason::LinkDown)]
    );

    h.session += 1;
    let session = h.session;
    h.venue(VenueInput::Link {
        up: true,
        session,
        reason: "reconnected".into(),
    });
    assert_eq!(h.core.holds(None), vec![HoldReason::Resync]);
    let needs = h.core.needs();
    assert!(needs.contains(&Need::OrderStatus {
        order_id: a,
        venue_oid: Some(void("v1")),
        why: needs::NeedReason::StaleEvidence
    }));
    assert!(needs.contains(&Need::OpenOrders));
    assert!(needs.contains(&Need::Positions));

    // Sweeps alone are not enough: the working order needs fresh evidence.
    h.sync();
    assert_eq!(h.core.holds(None), vec![HoldReason::Resync]);
    h.status(a, VenueStatus::Open, Some("0"));
    assert_eq!(h.core.holds(None), vec![]);
    assert_eq!(h.core.needs(), vec![]);
}

#[test]
fn stale_session_evidence_does_not_satisfy_resync() {
    let mut h = H::ready();
    let a = h.accept("a", "k1", Side::Buy, "1", "100");
    h.placed_open(a, "v1");
    let old = h.session;
    h.venue(VenueInput::Link {
        up: false,
        session: h.session,
        reason: "x".into(),
    });
    h.link_up_and_sync();
    // An answer gathered on the previous session arrives late.
    let venue_ts = h.now;
    h.venue(VenueInput::OrderStatus(OrderStatusIn {
        session: old,
        order_id: Some(a),
        venue_oid: None,
        status: StatusIn {
            status: VenueStatus::Open,
            cum: Some(q("0")),
            venue_ts,
        },
        source: EvidenceSource::Query,
    }));
    assert_eq!(h.core.holds(None), vec![HoldReason::Resync]);
}

#[test]
fn a_pending_cancel_is_sent_when_the_link_returns() {
    let mut h = H::ready();
    let a = h.accept("a", "k1", Side::Buy, "1", "100");
    h.placed_open(a, "v1");
    h.venue(VenueInput::Link {
        up: false,
        session: h.session,
        reason: "x".into(),
    });
    assert_eq!(h.cancel("a", a), CancelReply::Requested { order_id: a });
    assert_eq!(h.cancels(), vec![], "nothing can be sent while the link is down");
    h.link_up_and_sync();
    assert_eq!(h.cancels(), vec![(a, 1)]);
}

/// The venue position must lie within what booked fills plus open
/// reservations can explain (FIA-OMS-model.md §6.1, Bridge).
#[test]
fn position_check_uses_the_reachable_range() {
    let mut h = H::ready();
    let a = h.accept("a", "k1", Side::Buy, "4", "100");
    h.placed_open(a, "v1");
    h.fill(a, "t1", "1", "100");
    // Booked 1, 3 more could have filled: any venue value in [1, 4] is consistent.
    let down = h.session;
    h.session += 1;
    let session = h.session;
    h.venue(VenueInput::Link {
        up: false,
        session: down,
        reason: "x".into(),
    });
    h.venue(VenueInput::Link {
        up: true,
        session,
        reason: "x".into(),
    });
    let queried_at = h.now + 1;
    h.venue(VenueInput::Positions {
        session,
        queried_at,
        positions: vec![VenuePosition {
            instrument: iid("X"),
            qty: sq("3"),
        }],
    });
    assert_eq!(h.unacked_mismatches(), vec![]);
    assert_eq!(h.core.sync.positions_session, session);

    // 5 cannot be explained by this instance's orders.
    let queried_at = h.now + 1;
    h.venue(VenueInput::Positions {
        session,
        queried_at,
        positions: vec![VenuePosition {
            instrument: iid("X"),
            qty: sq("5"),
        }],
    });
    assert_eq!(h.unacked_mismatches(), vec![MismatchKind::PositionOutOfRange]);
}

#[test]
fn a_position_snapshot_older_than_the_last_booked_fill_is_inconclusive() {
    let mut h = H::ready();
    let a = h.accept("a", "k1", Side::Buy, "4", "100");
    h.placed_open(a, "v1");
    let queried_at = h.now; // snapshot requested before the fill is booked
    h.fill(a, "t1", "4", "100");
    let session = h.session;
    // The snapshot still says 0; booked is 4 with nothing open, so [4, 4].
    h.venue(VenueInput::Positions {
        session,
        queried_at,
        positions: vec![],
    });
    assert_eq!(h.unacked_mismatches(), vec![], "a stale snapshot must not raise a mismatch");
}

#[test]
fn foreign_and_orphan_open_orders_are_flagged() {
    let mut h = H::ready();
    let session = h.session;
    let queried_at = h.now + 1;
    h.venue(VenueInput::OpenOrders {
        session,
        queried_at,
        orders: vec![
            VenueOpenOrder {
                venue_oid: void("m1"),
                order_id: None,
                instrument: iid("X"),
                side: Side::Buy,
                remaining: Some(q("1")),
            },
            VenueOpenOrder {
                venue_oid: void("m2"),
                order_id: Some(OrderId(77)),
                instrument: iid("X"),
                side: Side::Sell,
                remaining: Some(q("1")),
            },
        ],
    });
    assert_eq!(h.unacked_mismatches(), vec![
        MismatchKind::ForeignOrder,
        MismatchKind::OrphanOrder
    ]);
    // The same sweep again does not raise the same mismatch twice.
    let queried_at = h.now + 1;
    h.venue(VenueInput::OpenOrders {
        session,
        queried_at,
        orders: vec![VenueOpenOrder {
            venue_oid: void("m1"),
            order_id: None,
            instrument: iid("X"),
            side: Side::Buy,
            remaining: Some(q("1")),
        }],
    });
    assert_eq!(h.core.mismatches.len(), 2);
}

#[test]
fn an_in_doubt_order_seen_in_the_open_order_sweep_is_acknowledged() {
    let mut h = H::ready();
    let a = h.accept("a", "k1", Side::Buy, "4", "100");
    h.venue(VenueInput::SendResult {
        order_id: a,
        outcome: SendOutcome::Unknown { reason: "timeout".into() },
        timing: Default::default(),
    });
    let session = h.session;
    let queried_at = h.now + 1;
    h.venue(VenueInput::OpenOrders {
        session,
        queried_at,
        orders: vec![VenueOpenOrder {
            venue_oid: void("v1"),
            order_id: Some(a),
            instrument: iid("X"),
            side: Side::Buy,
            remaining: Some(q("3")),
        }],
    });
    assert_eq!(h.order(a).phase, Phase::Working);
    assert_eq!(h.order(a).reported_cum, q("1"));
    // One unit was filled according to the venue but is not booked yet.
    assert!(h.core.needs().contains(&Need::Fills {
        since: h.core.sync.fills_up_to
    }));
}

// ---- C-BOOT: restart -----------------------------------------------------------

#[test]
fn restart_puts_outstanding_sends_in_doubt_and_never_resends() {
    let mut h = H::ready();
    let a = h.accept("a", "k1", Side::Buy, "1", "100"); // send outstanding
    let b = h.accept("a", "k2", Side::Buy, "1", "100");
    h.placed_open(b, "v2");
    h.cancel("a", b); // cancel outstanding
    let sends_before = h.sends();

    let mut r = H::new();
    r.core = h.rebuilt();
    assert_eq!(r.core, h.core, "journal replay must rebuild the exact state");
    r.shadow = r.core.clone();
    r.now = h.now;
    r.session = h.session;
    let out = r.inp(Input::Boot { binary: "test".into() });
    assert!(out.commands.is_empty(), "boot must not send anything");
    assert_eq!(r.order(a).doubt.as_ref().map(|d| d.reason), Some(DoubtReason::Restart));
    assert_eq!(r.order(b).phase, Phase::Working);
    assert_eq!(
        r.order(b).cancel.as_ref().unwrap().last_outcome,
        Some(CancelOutcome::LostOnRestart)
    );
    assert_eq!(r.core.holds(None), vec![HoldReason::LinkDown, HoldReason::InDoubt]);
    assert_eq!(r.core.boot, 2);

    // After the link returns: the cancel intent is re-sent (cancels are
    // repeatable), the new order is not.
    r.advance(2 * SEC);
    r.link_up_and_sync();
    assert_eq!(r.sends(), vec![], "no order send after restart");
    assert_eq!(r.cancels(), vec![(b, 2)]);
    assert_eq!(sends_before, vec![a, b]);
}

// ---- C-OPS: operator controls, configuration -----------------------------------

#[test]
fn holds_are_independent() {
    let mut h = H::ready();
    h.op(OperatorCmd::Halt {
        scope: Scope::Strategy(sid("a")),
        note: "t".into(),
    })
    .unwrap();
    assert_eq!(
        h.reject("a", "k1", Side::Buy, "1", "100"),
        vec![RejectReason::Hold(HoldReason::OperatorStrategy)]
    );
    h.accept("b", "k1", Side::Buy, "1", "100");
    h.op(OperatorCmd::Halt {
        scope: Scope::Account,
        note: "t".into(),
    })
    .unwrap();
    // Resuming the account does not resume the strategy.
    h.op(OperatorCmd::Resume {
        scope: Scope::Account,
        note: "t".into(),
    })
    .unwrap();
    assert_eq!(
        h.reject("a", "k2", Side::Buy, "1", "100"),
        vec![RejectReason::Hold(HoldReason::OperatorStrategy)]
    );
    assert!(
        h.op(OperatorCmd::Resume {
            scope: Scope::Account,
            note: "t".into()
        })
        .is_err()
    );
}

#[test]
fn acknowledging_one_mismatch_does_not_clear_another() {
    let mut h = H::ready();
    let session = h.session;
    for oid in ["m1", "m2"] {
        let queried_at = h.now + 1;
        h.venue(VenueInput::OpenOrders {
            session,
            queried_at,
            orders: vec![VenueOpenOrder {
                venue_oid: void(oid),
                order_id: None,
                instrument: iid("X"),
                side: Side::Buy,
                remaining: Some(q("1")),
            }],
        });
    }
    let ids: Vec<u32> = h.core.mismatches.keys().copied().collect();
    assert_eq!(ids.len(), 2);
    h.op(OperatorCmd::AckMismatch {
        id: ids[0],
        note: "cancelled by hand".into(),
    })
    .unwrap();
    assert_eq!(h.core.holds(None), vec![HoldReason::Mismatch]);
    h.op(OperatorCmd::AckMismatch {
        id: ids[1],
        note: "cancelled by hand".into(),
    })
    .unwrap();
    assert_eq!(h.core.holds(None), vec![]);
}

#[test]
fn cancel_all_requests_every_open_order_in_scope() {
    let mut h = H::ready();
    let a = h.accept("a", "k1", Side::Buy, "1", "100");
    let b = h.accept("b", "k1", Side::Buy, "1", "100");
    h.placed_open(a, "v1");
    h.placed_open(b, "v2");
    h.op(OperatorCmd::CancelAll {
        scope: Scope::Strategy(sid("a")),
        note: "t".into(),
    })
    .unwrap();
    assert_eq!(h.cancels(), vec![(a, 1)]);
    h.op(OperatorCmd::CancelAll {
        scope: Scope::Account,
        note: "t".into(),
    })
    .unwrap();
    assert_eq!(h.cancels(), vec![(a, 1), (b, 1)]);
}

#[test]
fn position_adjustment_is_explicit_and_keeps_the_ledgers_consistent() {
    let mut h = H::ready();
    h.op(OperatorCmd::AdjustPosition {
        instrument: iid("X"),
        ledger: Ledger::Unattributed,
        delta: sq("-1.5"),
        note: "manual trade on venue UI".into(),
    })
    .unwrap();
    assert_eq!(h.acct().pos, sq("-1.5"));
    assert_eq!(h.core.unattributed[&iid("X")].pos, sq("-1.5"));
    assert_eq!(h.book("a").pos, sq("0"));
}

#[test]
fn configuration_changes_are_validated_against_existing_exposure() {
    let mut h = H::ready();
    h.accept("a", "k1", Side::Buy, "1", "100");

    let mut same_version = config();
    same_version.policy.max_open_orders = 10;
    let out = h.inp(Input::Config(Box::new(same_version)));
    assert!(matches!(out.reply, Some(Reply::Config(Err(_)))));

    let mut removed = config();
    removed.version = 2;
    removed.strategies.remove(&sid("a"));
    let out = h.inp(Input::Config(Box::new(removed)));
    assert!(matches!(out.reply, Some(Reply::Config(Err(_)))));
    assert_eq!(h.core.config.version, 1);

    let mut ok = config();
    ok.version = 2;
    ok.instruments.get_mut(&iid("X")).unwrap().account_pos_max = sq("1");
    let out = h.inp(Input::Config(Box::new(ok)));
    assert_eq!(out.reply, Some(Reply::Config(Ok(2))));
    // The new bound applies to the next decision; existing exposure remains.
    assert_eq!(
        h.reject("a", "k2", Side::Buy, "1", "100"),
        vec![RejectReason::AccountPositionLimit]
    );
    // Re-submitting the active configuration is a no-op.
    let n = h.log.len();
    let active = h.core.config.clone();
    assert_eq!(h.inp(Input::Config(Box::new(active))).reply, Some(Reply::Config(Ok(2))));
    assert_eq!(h.log.len(), n);
}

#[test]
fn no_configuration_means_nothing_is_accepted() {
    let mut h = H::new();
    h.inp(Input::Boot { binary: "test".into() });
    let r = h.req("a", "k1", Side::Buy, "1", "100");
    let NewOrderReply::Rejected { reasons } = h.new_order(r) else {
        panic!()
    };
    assert!(reasons.contains(&RejectReason::Hold(HoldReason::NoConfig)));
    assert!(reasons.contains(&RejectReason::UnknownStrategy));
}

#[test]
fn capacity_limits_bound_open_orders_and_outstanding_sends() {
    let mut cfg = config();
    cfg.policy.max_inflight_sends = 2;
    cfg.policy.max_open_orders = 3;
    let mut h = H::ready_with(cfg);
    let a = h.accept("a", "k1", Side::Buy, "0.1", "100");
    let b = h.accept("a", "k2", Side::Buy, "0.1", "100");
    assert_eq!(h.reject("a", "k3", Side::Buy, "0.1", "100"), vec![RejectReason::GatewayBusy]);
    h.placed_open(a, "v1");
    h.placed_open(b, "v2");
    h.accept("a", "k3", Side::Buy, "0.1", "100");
    assert_eq!(
        h.reject("a", "k4", Side::Buy, "0.1", "100"),
        vec![RejectReason::MaxOpenOrdersAccount]
    );
}

#[test]
fn storage_low_blocks_new_orders_but_not_fills_or_cancels() {
    let mut h = H::ready();
    let a = h.accept("a", "k1", Side::Buy, "1", "100");
    h.placed_open(a, "v1");
    h.inp(Input::Storage {
        low: true,
        free_bytes: 1,
    });
    assert_eq!(
        h.reject("a", "k2", Side::Buy, "1", "100"),
        vec![RejectReason::Hold(HoldReason::StorageLow)]
    );
    h.fill(a, "t1", "0.5", "100");
    assert_eq!(h.acct().pos, sq("0.5"));
    assert_eq!(h.cancel("a", a), CancelReply::Requested { order_id: a });
    h.inp(Input::Storage {
        low: false,
        free_bytes: 1 << 40,
    });
    assert_eq!(h.core.holds(None), vec![]);
}

// ---- determinism ---------------------------------------------------------------

/// Re-deciding every recorded input on the pre-state reproduces the recorded
/// events exactly, and the state survives a JSON round trip unchanged.
#[test]
fn recorded_inputs_re_decide_to_the_recorded_events() {
    let mut h = H::ready();
    let a = h.accept("a", "k1", Side::Buy, "4", "100");
    h.placed_open(a, "v1");
    h.fill(a, "t1", "1", "100");
    h.cancel("a", a);
    h.status(a, VenueStatus::Canceled { reason: "user".into() }, Some("1"));
    h.reject("b", "k9", Side::Buy, "100", "100");

    let mut c = Core::new(7);
    for rec in &h.log {
        let out = c.handle(rec.at, &rec.input).unwrap();
        assert_eq!(out.events, rec.events, "record {} re-decided differently", rec.seq);
    }
    assert_eq!(c, h.core);
    assert_eq!(c.digest(), h.core.digest());

    let json = serde_json::to_string(&h.core).unwrap();
    let back: Core = serde_json::from_str(&json).unwrap();
    assert_eq!(back, h.core);
    let rec_json = serde_json::to_string(&h.log).unwrap();
    let recs: Vec<Record> = serde_json::from_str(&rec_json).unwrap();
    assert_eq!(recs, h.log);
}
