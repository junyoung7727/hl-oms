//! U13c: the absence rule with more than one "not found" answer (Bybit),
//! and the order book state a new order was decided on (`market_ref`).

#![allow(clippy::unwrap_used)]

mod common;
use common::*;
use oms_core::*;

fn names(out: &Output) -> Vec<&'static str> {
    out.events.iter().map(Event::name).collect()
}

/// An order whose send outcome is unknown: pending, in doubt, reserved.
fn in_doubt(h: &mut H) -> (OrderId, Ts) {
    let a = h.accept("a", "k1", Side::Buy, "4", "100");
    let expires_at = h.order(a).expires_at;
    h.venue(VenueInput::SendResult {
        order_id: a,
        outcome: SendOutcome::Unknown {
            reason: "timeout".into(),
        },
        timing: Default::default(),
    });
    (a, expires_at)
}

fn not_found(h: &mut H, order_id: OrderId, queried_at: Ts) -> Output {
    let session = h.session;
    h.venue(VenueInput::StatusNotFound {
        session,
        order_id,
        queried_at,
    })
}

fn two_answers() -> CoreConfig {
    let mut cfg = config();
    cfg.policy.absence_not_found_needed = 2;
    cfg
}

#[test]
fn two_answers_after_expiry_plus_margin_are_needed_when_the_policy_asks_for_two() {
    let mut h = H::ready_with(two_answers());
    let (a, expires_at) = in_doubt(&mut h);

    // Before expiry + 10 s: not counted.
    let out = not_found(&mut h, a, expires_at + 9 * SEC);
    assert!(out.events.is_empty());

    // The first counted answer: noted, nothing released.
    let out = not_found(&mut h, a, expires_at + 10 * SEC);
    assert_eq!(names(&out), vec!["absence_noted"]);
    assert_eq!(h.order(a).phase, Phase::PendingSend);
    assert_eq!(h.acct().buy_open, q("4"), "the reservation stays");

    // A second answer to the same query is not a second answer.
    let out = not_found(&mut h, a, expires_at + 10 * SEC);
    assert!(out.events.is_empty());
    assert_eq!(h.order(a).phase, Phase::PendingSend);

    // The rebuilt core keeps the count.
    assert_eq!(h.rebuilt(), h.core);

    // The second counted answer closes it.
    let out = not_found(&mut h, a, expires_at + 11 * SEC);
    assert_eq!(names(&out), vec!["absence_noted", "order_closed"]);
    assert!(matches!(
        h.order(a).close.as_ref().unwrap().reason,
        CloseReason::NotPlaced { .. }
    ));
    assert_eq!(h.acct().buy_open, q("0"));
    assert_eq!(h.rebuilt(), h.core);
}

#[test]
fn one_answer_is_enough_by_default_and_writes_no_new_event() {
    let mut h = H::ready();
    assert_eq!(h.core.config.policy.absence_not_found_needed, 1);
    let (a, expires_at) = in_doubt(&mut h);
    let out = not_found(&mut h, a, expires_at + 10 * SEC);
    assert_eq!(names(&out), vec!["order_closed"]);
}

#[test]
fn the_policy_refuses_zero_answers() {
    let mut cfg = config();
    cfg.policy.absence_not_found_needed = 0;
    assert!(cfg.validate().is_err());
}

#[test]
fn a_policy_at_its_default_writes_no_new_key() {
    let json = serde_json::to_string(&Policy::default()).unwrap();
    assert!(!json.contains("absence_not_found_needed"), "{json}");
    let p = Policy {
        absence_not_found_needed: 2,
        ..Policy::default()
    };
    let json = serde_json::to_string(&p).unwrap();
    let back: Policy = serde_json::from_str(&json).unwrap();
    assert_eq!(back, p);
}

fn market_ref() -> MarketRef {
    MarketRef {
        venue: RefName::new("bybit").unwrap(),
        market: RefName::new("linear").unwrap(),
        symbol: RefName::new("BTCUSDT").unwrap(),
        channel: RefName::new("orderbook.50").unwrap(),
        update_id: Some(123_456),
        venue_ts_ns: 1_700_000_000_000_000_000,
        recv_ns: 1_700_000_000_001_000_000,
    }
}

#[test]
fn market_ref_round_trips_through_the_journal() {
    let mut h = H::ready();
    let mut r = h.req("a", "k1", Side::Buy, "1", "100");
    r.market_ref = Some(market_ref());
    let NewOrderReply::Accepted { order_id } = h.new_order(r.clone()) else {
        panic!("not accepted")
    };
    let rec = h.log.last().unwrap();
    let text = serde_json::to_string(rec).unwrap();
    let back: Record = serde_json::from_str(&text).unwrap();
    let Event::OrderAccepted { request, .. } = &back.events[0] else {
        panic!("{:?}", back.events)
    };
    assert_eq!(request.market_ref, Some(market_ref()));
    assert_eq!(h.order(order_id).instrument, iid("X"));
}

#[test]
fn a_request_without_market_ref_keeps_its_bytes() {
    let h = H::ready();
    let r = h.req("a", "k1", Side::Buy, "1", "100");
    let json = serde_json::to_string(&r).unwrap();
    assert!(!json.contains("market_ref"), "{json}");
}

#[test]
fn a_repeated_request_with_another_market_ref_is_a_duplicate() {
    let mut h = H::ready();
    let mut r = h.req("a", "k1", Side::Buy, "1", "100");
    r.market_ref = Some(market_ref());
    let NewOrderReply::Accepted { order_id } = h.new_order(r.clone()) else {
        panic!("not accepted")
    };
    let mut again = r;
    let mut newer = market_ref();
    newer.update_id = Some(123_999);
    again.market_ref = Some(newer);
    assert!(matches!(
        h.new_order(again),
        NewOrderReply::Duplicate { order_id: id, .. } if id == order_id
    ));
}

#[test]
fn market_ref_names_are_validated() {
    assert!(RefName::new("orderbook.50").is_ok());
    assert!(RefName::new("").is_err());
    assert!(RefName::new("a b").is_err());
    assert!(RefName::new("x".repeat(65)).is_err());
    let bad = r#"{"venue":"bybit","market":"linear","symbol":"BTC USDT","channel":"trade","update_id":null,"venue_ts_ns":1,"recv_ns":2}"#;
    assert!(serde_json::from_str::<MarketRef>(bad).is_err());
}
