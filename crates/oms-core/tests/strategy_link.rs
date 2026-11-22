//! Strategy liveness in the core (C-LIVE, U18b3): the node reports that a
//! strategy's link is lost, and the core holds that strategy's plain orders
//! and asks to cancel its resting plain orders; the strategy's next `hello`
//! releases the hold.

#![allow(clippy::unwrap_used)]

mod common;
use common::*;
use oms_core::*;

fn names(out: &Output) -> Vec<&'static str> {
    out.events.iter().map(Event::name).collect()
}

fn session(h: &mut H, s: SessionInput) -> (Output, Result<String, String>) {
    let out = h.inp(Input::Session(s));
    let reply = match &out.reply {
        Some(Reply::Operator(r)) => r.clone(),
        other => panic!("unexpected reply {other:?}"),
    };
    (out, reply)
}

fn lost(h: &mut H, strategy: &str, why: &str) -> (Output, Result<String, String>) {
    session(
        h,
        SessionInput::Lost {
            strategy: sid(strategy),
            why: why.into(),
        },
    )
}

fn hello(h: &mut H, strategy: &str) -> (Output, Result<String, String>) {
    session(
        h,
        SessionInput::Hello {
            strategy: sid(strategy),
        },
    )
}

/// An order of `strategy` resting at the venue.
fn resting(h: &mut H, strategy: &str, key: &str, side: Side, reduce_only: bool) -> OrderId {
    let r = if reduce_only {
        h.reduce_only_req(strategy, key, side, "0.5", "100")
    } else {
        h.req(strategy, key, side, "0.5", "100")
    };
    let id = match h.new_order(r) {
        NewOrderReply::Accepted { order_id } => order_id,
        other => panic!("{key}: {other:?}"),
    };
    h.placed_open(id, key);
    id
}

fn cancel_by(h: &H, id: OrderId) -> Option<Actor> {
    h.order(id).cancel.as_ref().map(|c| c.by.clone())
}

/// The loss of `a` asks to cancel `a`'s resting plain order, in the same
/// record as the hold, and sends that cancel. `a`'s reduce-only order and
/// `b`'s plain order are left alone.
#[test]
fn a_lost_strategy_gets_cancel_requests_for_its_plain_orders_only() {
    let mut h = H::ready();
    h.long("a", "k0", "2");
    let a_plain = resting(&mut h, "a", "a-p", Side::Buy, false);
    let a_reduce = resting(&mut h, "a", "a-r", Side::Sell, true);
    let b_plain = resting(&mut h, "b", "b-p", Side::Buy, false);
    let records = h.log.len();

    let (out, reply) = lost(&mut h, "a", "last trading connection closed");
    assert_eq!(reply, Ok("held".to_string()));
    assert_eq!(
        names(&out),
        ["strategy_link_lost", "cancel_requested", "cancel_sent"]
    );
    assert_eq!(h.log.len(), records + 1, "one record");
    assert_eq!(
        out.events[0],
        Event::StrategyLinkLost {
            strategy: sid("a"),
            why: "last trading connection closed".into(),
        }
    );
    assert_eq!(
        cancel_by(&h, a_plain),
        Some(Actor::Rule(RuleId::StrategyLink))
    );
    assert_eq!(cancel_by(&h, a_reduce), None);
    assert_eq!(cancel_by(&h, b_plain), None);
    assert_eq!(h.cancels(), [(a_plain, 1)]);
    assert_eq!(h.rebuilt(), h.core);
}

/// While `a` is held its plain orders are refused, its reduce-only orders
/// pass with a margin policy, and `b` trades. The hold shows in
/// `holds(Some(a))` only, as the operator's strategy halt.
#[test]
fn the_hold_refuses_only_that_strategys_plain_orders() {
    let mut h = H::ready_with(config_with_margin());
    h.report_margin("1000");
    h.ref_price("X", "100");
    h.long("a", "k0", "2");
    lost(&mut h, "a", "no request for 5000 ms").1.unwrap();

    assert!(
        h.core
            .holds(Some(&sid("a")))
            .contains(&HoldReason::StrategyLink)
    );
    assert!(
        !h.core
            .holds(Some(&sid("b")))
            .contains(&HoldReason::StrategyLink)
    );
    assert!(!h.core.holds(None).contains(&HoldReason::StrategyLink));

    h.report_margin("1000");
    assert_eq!(
        h.new_order(h.req("a", "a-p", Side::Buy, "0.1", "100")),
        NewOrderReply::Rejected {
            reasons: vec![RejectReason::Hold(HoldReason::StrategyLink)]
        }
    );
    let r = h.reduce_only_req("a", "a-r", Side::Sell, "0.5", "100");
    assert!(matches!(h.new_order(r), NewOrderReply::Accepted { .. }));
    assert!(matches!(
        h.new_order(h.req("b", "b-p", Side::Buy, "0.1", "100")),
        NewOrderReply::Accepted { .. }
    ));

    // Without a margin policy every hold refuses every order, as before.
    let mut h = H::ready();
    h.long("a", "k0", "2");
    lost(&mut h, "a", "why").1.unwrap();
    let r = h.reduce_only_req("a", "a-r", Side::Sell, "0.5", "100");
    assert_eq!(
        h.new_order(r),
        NewOrderReply::Rejected {
            reasons: vec![RejectReason::Hold(HoldReason::StrategyLink)]
        }
    );
}

/// A loss while held records nothing and asks for nothing: the first
/// reason and time stay.
#[test]
fn a_second_loss_records_nothing_and_keeps_the_first_why() {
    let mut h = H::ready();
    let id = resting(&mut h, "a", "a-r", Side::Sell, true);
    lost(&mut h, "a", "first").1.unwrap();
    let first = h.core.strategy_link.get(&sid("a")).cloned().unwrap();
    assert_eq!(first.why, "first");
    assert_eq!(first.since, h.now);
    let records = h.log.len();
    let (out, reply) = lost(&mut h, "a", "second");
    assert_eq!(reply, Ok("already held".to_string()));
    assert!(out.events.is_empty() && out.commands.is_empty());
    assert_eq!(h.log.len(), records);
    assert_eq!(h.core.strategy_link.get(&sid("a")), Some(&first));
    assert_eq!(cancel_by(&h, id), None);
}

/// `hello` of a held strategy records `strategy_link_restored` and the
/// strategy trades again; `hello` of a strategy that is not held records
/// nothing.
#[test]
fn hello_restores_only_a_held_strategy() {
    let mut h = H::ready();
    let records = h.log.len();
    let (out, reply) = hello(&mut h, "a");
    assert_eq!(reply, Ok("not held".to_string()));
    assert!(out.events.is_empty());
    assert_eq!(h.log.len(), records);

    lost(&mut h, "a", "why").1.unwrap();
    lost(&mut h, "b", "why").1.unwrap();
    let (out, reply) = hello(&mut h, "a");
    assert_eq!(reply, Ok("restored".to_string()));
    assert_eq!(
        out.events,
        [Event::StrategyLinkRestored { strategy: sid("a") }]
    );
    assert!(out.commands.is_empty());
    assert!(
        !h.core
            .holds(Some(&sid("a")))
            .contains(&HoldReason::StrategyLink)
    );
    assert!(
        h.core
            .holds(Some(&sid("b")))
            .contains(&HoldReason::StrategyLink)
    );
    assert!(matches!(
        h.new_order(h.req("a", "a-p", Side::Buy, "0.1", "100")),
        NewOrderReply::Accepted { .. }
    ));
    assert_eq!(h.rebuilt(), h.core);
}

/// The node reports only strategies it knows, but the core checks again:
/// a loss of a strategy that is not configured records nothing.
#[test]
fn a_loss_of_an_unknown_strategy_records_nothing() {
    let mut h = H::ready();
    let records = h.log.len();
    let (out, reply) = lost(&mut h, "zz", "why");
    assert!(reply.unwrap_err().starts_with("bad_request:"));
    assert!(out.events.is_empty());
    assert_eq!(h.log.len(), records);
    assert!(h.core.strategy_link.is_empty());
}

/// The hold is state: a restart keeps it until the strategy's hello, and
/// replay gives the live state. A state without a held strategy does not
/// write the field.
#[test]
fn the_hold_survives_replay_and_a_restart() {
    let mut h = H::ready();
    let empty = serde_json::to_value(&h.core).unwrap();
    assert!(
        empty.get("strategy_link").is_none(),
        "not written while empty"
    );

    lost(&mut h, "a", "why").1.unwrap();
    let out = h.inp(Input::Boot {
        binary: "test".into(),
        kill_marker: None,
    });
    assert_eq!(names(&out), ["booted"]);
    assert!(
        h.core
            .holds(Some(&sid("a")))
            .contains(&HoldReason::StrategyLink)
    );
    let replayed = h.rebuilt();
    assert_eq!(replayed.strategy_link, h.core.strategy_link);
    assert_eq!(replayed, h.core);
    let (_, reply) = hello(&mut h, "a");
    assert_eq!(reply, Ok("restored".to_string()));
    assert_eq!(h.rebuilt(), h.core);
}

/// A configuration without the strategy drops its hold: the strategy has
/// no orders (a strategy with open orders cannot be removed) and no hello
/// can name it any more.
#[test]
fn a_configuration_without_the_strategy_drops_its_hold() {
    let mut h = H::ready();
    lost(&mut h, "a", "why").1.unwrap();
    lost(&mut h, "b", "why").1.unwrap();
    let mut cfg = config();
    cfg.version = 2;
    cfg.strategies.remove(&sid("b"));
    let out = h.inp(Input::Config(Box::new(cfg)));
    assert_eq!(names(&out), ["config_activated"]);
    assert_eq!(
        h.core.strategy_link.keys().cloned().collect::<Vec<_>>(),
        [sid("a")]
    );
    assert_eq!(h.rebuilt(), h.core);
}
