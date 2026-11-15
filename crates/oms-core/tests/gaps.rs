//! Cases the gap review (2026-10-02) found without a test: reject reasons
//! and mismatch kinds no scenario produced, fills for orders closed in each
//! way, and the three sweeps of a re-confirmation taken one at a time.

#![allow(clippy::unwrap_used)]

mod common;
use common::*;
use oms_core::*;

fn fill_for(key: &str, order_id: Option<OrderId>, oid: Option<&str>, instrument: &str, side: Side, qty: &str, ts: Ts) -> FillIn {
    FillIn {
        key: fkey(key),
        order_id,
        venue_oid: oid.map(void),
        instrument: iid(instrument),
        side,
        qty: q(qty),
        px: px("100"),
        fee: Amount::ZERO,
        venue_ts: ts,
    }
}

// ---- reject reasons -----------------------------------------------------------

#[test]
fn every_remaining_reject_reason_has_a_case() {
    use RejectReason as R;
    let mut cfg = config();
    cfg.instruments.insert(iid("Y"), cfg.instruments[&iid("X")].clone());
    cfg.instruments.get_mut(&iid("X")).unwrap().max_order_notional = amt("5000");
    cfg.strategies.get_mut(&sid("b")).unwrap().max_open_orders = 1;
    cfg.strategies.insert(
        sid("off"),
        StrategySpec {
            enabled: false,
            ..cfg.strategies[&sid("a")].clone()
        },
    );
    let mut h = H::ready_with(cfg);

    // Quantity and price must be positive; nothing else is evaluated on them.
    assert!(h.reject("a", "k1", Side::Buy, "0", "100").contains(&R::QtyNotPositive));
    assert!(h.reject("a", "k2", Side::Buy, "1", "0").contains(&R::PxNotPositive));
    // 60 * 100 = 6000 exceeds the order notional cap of 5000; the quantity cap (100) is not reached.
    assert_eq!(h.reject("a", "k3", Side::Buy, "60", "100"), vec![R::MaxOrderNotional, R::AccountPositionLimit, R::StrategyPositionLimit]);
    assert_eq!(h.reject("a", "k3", Side::Buy, "6", "1000"), vec![R::MaxOrderNotional]);

    // An instrument the configuration does not have, and one this strategy may not trade.
    let mut r = h.req("a", "k4", Side::Buy, "1", "100");
    r.instrument = iid("Z");
    assert!(matches!(h.new_order(r), NewOrderReply::Rejected { reasons } if reasons.contains(&R::UnknownInstrument)));
    let mut r = h.req("a", "k5", Side::Buy, "1", "100");
    r.instrument = iid("Y");
    assert!(matches!(h.new_order(r), NewOrderReply::Rejected { reasons } if reasons == vec![R::InstrumentNotAllowed]));

    assert!(h.reject("off", "k6", Side::Buy, "1", "100").contains(&R::StrategyDisabled));

    // Strategy b may hold one open order.
    h.accept("b", "k7", Side::Buy, "1", "100");
    assert_eq!(h.reject("b", "k8", Side::Buy, "1", "100"), vec![R::MaxOpenOrdersStrategy]);
}

#[test]
fn the_strategy_bound_binds_on_its_own() {
    let mut cfg = config();
    let lim = cfg.strategies.get_mut(&sid("a")).unwrap().limits.get_mut(&iid("X")).unwrap();
    lim.max = sq("2");
    let mut h = H::ready_with(cfg);
    // The account allows 10; strategy a allows 2.
    assert_eq!(h.reject("a", "k1", Side::Buy, "3", "100"), vec![RejectReason::StrategyPositionLimit]);
    h.accept("b", "k1", Side::Buy, "3", "100");
}

#[test]
fn memory_capacity_and_arithmetic_overflow_are_rejections_not_faults() {
    let mut cfg = config();
    cfg.policy.max_orders_in_memory = 2;
    cfg.policy.max_open_orders = 2;
    let mut h = H::ready_with(cfg);
    let a = h.accept("a", "k1", Side::Buy, "1", "100");
    let b = h.accept("a", "k2", Side::Buy, "1", "100");
    for id in [a, b] {
        h.venue(VenueInput::SendResult {
            order_id: id,
            outcome: SendOutcome::Rejected { reason: "no".into() },
            timing: Default::default(),
        });
    }
    // Both are closed but still held for duplicate answers.
    assert_eq!(h.reject("a", "k3", Side::Buy, "1", "100"), vec![RejectReason::CapacityExhausted]);

    // A product that does not fit the amount type.
    let mut cfg = config();
    let spec = cfg.instruments.get_mut(&iid("X")).unwrap();
    spec.qty_step = q("1");
    spec.px_tick = px("1");
    spec.px_max_sig_figs = None;
    spec.max_order_qty = Qty(i64::MAX);
    let mut h = H::ready_with(cfg);
    let mut r = h.req("a", "k1", Side::Buy, "1", "100");
    r.qty = Qty((i64::MAX / SCALE_I64) * SCALE_I64);
    r.px = Px((i64::MAX / SCALE_I64) * SCALE_I64);
    assert!(matches!(h.new_order(r), NewOrderReply::Rejected { reasons } if reasons.contains(&RejectReason::Overflow)));
}

const SCALE_I64: i64 = 100_000_000;

// ---- mismatch kinds -----------------------------------------------------------

#[test]
fn a_position_on_an_instrument_that_is_not_configured_is_flagged() {
    let mut h = H::ready();
    let session = h.session;
    let queried_at = h.now + 1;
    h.venue(VenueInput::Positions {
        session,
        queried_at,
        positions: vec![VenuePosition {
            instrument: iid("ETH"),
            qty: sq("2"),
        }],
    });
    assert_eq!(h.unacked_mismatches(), vec![MismatchKind::ForeignPosition]);
}

#[test]
fn a_fill_on_an_instrument_that_is_not_configured_is_booked_as_external_without_a_fault() {
    let mut h = H::ready();
    let ts = h.now;
    h.venue(VenueInput::Fill(fill_for("t1", None, Some("z1"), "ETH", Side::Buy, "2", ts)));
    assert_eq!(h.unacked_mismatches(), vec![MismatchKind::ExternalFill]);
    assert_eq!(h.core.unattributed[&iid("ETH")].pos, sq("2"));
    assert_eq!(h.core.check_invariants(), Ok(()));
    // The position sweep then sees the same position at the venue and calls it foreign.
    let (session, queried_at) = (h.session, h.now + 1);
    h.advance(10);
    h.venue(VenueInput::Positions {
        session,
        queried_at: queried_at + 10,
        positions: vec![VenuePosition {
            instrument: iid("ETH"),
            qty: sq("2"),
        }],
    });
    assert!(h.unacked_mismatches().contains(&MismatchKind::ForeignPosition));
}

#[test]
fn a_fill_that_contradicts_its_order_is_booked_to_the_account_and_flagged() {
    let mut h = H::ready();
    let a = h.accept("a", "k1", Side::Buy, "4", "100");
    h.placed_open(a, "v1");
    let ts = h.now;
    // Names order a, but sells.
    h.venue(VenueInput::Fill(fill_for("t1", Some(a), Some("v1"), "X", Side::Sell, "1", ts)));
    assert_eq!(h.unacked_mismatches(), vec![MismatchKind::FillFieldConflict]);
    assert_eq!(h.order(a).cum_qty, q("0"), "not booked to the order");
    assert_eq!(h.acct().pos, sq("-1"), "booked to the account: the venue position did change");
    assert_eq!(h.acct().buy_open, q("4"), "the order's reservation is untouched");
}

#[test]
fn too_many_unidentified_fills_are_booked_as_external_instead_of_waiting() {
    let mut cfg = config();
    cfg.policy.max_parked_fills = 2;
    let mut h = H::ready_with(cfg);
    h.accept("a", "k1", Side::Buy, "4", "100"); // unidentified: its send result has not arrived
    let ts = h.now;
    for i in 0..2 {
        h.venue(VenueInput::Fill(fill_for(&format!("t{i}"), None, Some("v9"), "X", Side::Buy, "1", ts)));
    }
    assert_eq!(h.core.parked_fills.len(), 2);
    assert_eq!(h.unacked_mismatches(), vec![]);
    h.venue(VenueInput::Fill(fill_for("t2", None, Some("v9"), "X", Side::Buy, "1", ts)));
    assert_eq!(h.core.parked_fills.len(), 2);
    let kinds = h.unacked_mismatches();
    assert!(kinds.contains(&MismatchKind::ParkedFillOverflow) && kinds.contains(&MismatchKind::ExternalFill), "{kinds:?}");
    assert_eq!(h.acct().pos, sq("1"), "the third fill is booked, not dropped");
}

#[test]
fn an_acknowledged_order_the_venue_no_longer_knows_is_reported_not_closed() {
    let mut h = H::ready();
    let a = h.accept("a", "k1", Side::Buy, "4", "100");
    h.placed_open(a, "v1");
    h.advance(100 * SEC);
    let (session, queried_at) = (h.session, h.now);
    h.venue(VenueInput::StatusNotFound {
        session,
        order_id: a,
        queried_at,
    });
    assert_eq!(h.order(a).phase, Phase::Working, "absence proves nothing about its fills");
    assert_eq!(h.acct().buy_open, q("4"));
    assert_eq!(h.unacked_mismatches(), vec![MismatchKind::StatusConflict]);
}

// ---- fills for closed orders ----------------------------------------------------

/// A fill for an order closed as cancelled, as never placed, or by the
/// operator is booked to that order's strategy and flagged. Never dropped.
#[test]
fn a_fill_after_any_kind_of_close_is_booked_and_flagged() {
    // Closed as cancelled by the venue.
    let mut h = H::ready();
    let a = h.accept("a", "k1", Side::Buy, "4", "100");
    h.placed_open(a, "v1");
    h.status(a, VenueStatus::Canceled { reason: "canceled".into() }, Some("0"));
    assert!(h.order(a).is_closed());
    h.fill(a, "t1", "1", "100");
    assert_eq!(h.unacked_mismatches(), vec![MismatchKind::FillAfterClose]);
    assert_eq!((h.order(a).cum_qty, h.book("a").pos, h.acct().pos), (q("1"), sq("1"), sq("1")));
    assert_eq!(h.acct().buy_open, q("0"), "a closed order has no reservation to consume");

    // Closed as never placed.
    let mut h = H::ready();
    let a = h.accept("a", "k1", Side::Buy, "4", "100");
    h.venue(VenueInput::SendResult {
        order_id: a,
        outcome: SendOutcome::Unknown { reason: "timeout".into() },
        timing: Default::default(),
    });
    h.advance(100 * SEC);
    let (session, queried_at) = (h.session, h.now);
    h.venue(VenueInput::StatusNotFound { session, order_id: a, queried_at });
    assert!(matches!(h.order(a).close.as_ref().map(|c| &c.reason), Some(CloseReason::NotPlaced { .. })));
    h.fill(a, "t1", "1", "100");
    assert_eq!(h.unacked_mismatches(), vec![MismatchKind::FillAfterClose]);
    assert_eq!((h.order(a).cum_qty, h.book("a").pos), (q("1"), sq("1")));

    // Closed by the operator.
    let mut h = H::ready();
    let a = h.accept("a", "k1", Side::Buy, "4", "100");
    h.placed_open(a, "v1");
    h.op(OperatorCmd::ResolveOrder { order_id: a, note: "gone".into() }).unwrap();
    h.fill(a, "t1", "1", "100");
    assert_eq!(h.unacked_mismatches(), vec![MismatchKind::FillAfterClose]);
    assert_eq!((h.order(a).cum_qty, h.book("a").pos), (q("1"), sq("1")));
}

/// A fill whose order has been purged from memory can no longer be
/// attributed; it is booked as external. (The contract says so since the
/// review; before, it only said "never dropped".)
#[test]
fn a_fill_for_a_purged_order_is_booked_as_external() {
    let mut cfg = config();
    cfg.policy.closed_retention_us = SEC;
    cfg.policy.dedup_grace_us = SEC;
    cfg.policy.purge_interval_us = SEC;
    let mut h = H::ready_with(cfg);
    let a = h.accept("a", "k1", Side::Buy, "4", "100");
    h.placed_open(a, "v1");
    h.status(a, VenueStatus::Canceled { reason: "canceled".into() }, Some("0"));
    h.advance(100 * SEC);
    h.tick();
    assert!(h.core.order(a).is_none(), "purged");
    let ts = h.now;
    h.venue(VenueInput::Fill(fill_for("late", Some(a), Some("v1"), "X", Side::Buy, "1", ts)));
    assert_eq!(h.unacked_mismatches(), vec![MismatchKind::ExternalFill]);
    assert_eq!(h.acct().pos, sq("1"));
    assert_eq!(h.core.unattributed[&iid("X")].pos, sq("1"));
}

/// The only fills the core does not book: a non-positive quantity (not a
/// fill), a key already booked or waiting, and a fill older than the floor
/// below which every fill is known to have been delivered.
#[test]
fn the_three_ways_a_fill_input_changes_nothing() {
    let mut h = H::ready();
    let a = h.accept("a", "k1", Side::Buy, "4", "100");
    h.placed_open(a, "v1");
    let n = h.log.len();
    let ts = h.now;
    h.venue(VenueInput::Fill(fill_for("zero", Some(a), Some("v1"), "X", Side::Buy, "0", ts)));
    assert_eq!(h.log.len(), n, "a zero-quantity fill is not a fill");
    h.fill(a, "t1", "1", "100");
    let n = h.log.len();
    h.fill(a, "t1", "1", "100");
    assert_eq!(h.log.len(), n, "same key");
    let floor = h.core.fill_floor;
    assert!(floor > 0);
    h.venue(VenueInput::Fill(fill_for("ancient", Some(a), Some("v1"), "X", Side::Buy, "1", floor - 1)));
    assert_eq!(h.log.len(), n, "older than the floor");
    assert_eq!(h.order(a).cum_qty, q("1"));
}

// ---- re-confirmation ------------------------------------------------------------

/// After a reconnect each of the four confirmations is required by itself:
/// leaving any one out keeps the hold.
#[test]
fn each_part_of_the_re_confirmation_is_required() {
    for missing in ["status", "fills", "open_orders", "positions"] {
        let mut h = H::ready();
        let a = h.accept("a", "k1", Side::Buy, "4", "100");
        h.placed_open(a, "v1");
        h.venue(VenueInput::Link {
            up: false,
            session: h.session,
            reason: "lost".into(),
        });
        h.session += 1;
        let session = h.session;
        h.venue(VenueInput::Link {
            up: true,
            session,
            reason: "back".into(),
        });
        assert_eq!(h.core.holds(None), vec![HoldReason::Resync]);
        let t = h.now;
        if missing != "status" {
            h.status(a, VenueStatus::Open, Some("0"));
        }
        if missing != "fills" {
            h.venue(VenueInput::FillsSynced { session, up_to: t });
        }
        if missing != "open_orders" {
            h.venue(VenueInput::OpenOrders {
                session,
                queried_at: t + 1,
                orders: vec![VenueOpenOrder {
                    venue_oid: void("v1"),
                    order_id: Some(a),
                    instrument: iid("X"),
                    side: Side::Buy,
                    remaining: None,
                }],
            });
        }
        if missing != "positions" {
            h.venue(VenueInput::Positions {
                session,
                queried_at: t + 1,
                positions: vec![],
            });
        }
        // The open-order list is itself evidence for the orders it shows, so
        // only the order's own status may be replaced by it.
        if missing == "status" {
            assert_eq!(h.core.holds(None), vec![], "the open-order list confirmed order a");
        } else {
            assert_eq!(h.core.holds(None), vec![HoldReason::Resync], "without {missing}");
        }
    }
}
