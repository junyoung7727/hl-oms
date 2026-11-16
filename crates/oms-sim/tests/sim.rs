//! Seeded simulation runs. See docs/verification/simulation.md.
//!
//! `OMS_SIM_SEEDS` sets how many seeds each profile runs (default 40).
//! `OMS_SIM_SEED` and `OMS_SIM_PROFILE` rerun exactly one failing case.

#![allow(clippy::unwrap_used)]

use oms_sim::*;

fn seeds() -> Vec<u64> {
    if let Ok(s) = std::env::var("OMS_SIM_SEED") {
        return vec![s.parse().unwrap()];
    }
    let n: u64 = std::env::var("OMS_SIM_SEEDS").ok().and_then(|s| s.parse().ok()).unwrap_or(40);
    (1..=n).collect()
}

fn run_profile(p: Profile) -> Vec<SimStats> {
    if let Ok(only) = std::env::var("OMS_SIM_PROFILE") {
        if only != p.name {
            return Vec::new();
        }
    }
    let mut all = Vec::new();
    for seed in seeds() {
        match run(seed, p.clone()) {
            Ok(s) => all.push(s),
            Err(v) => panic!("{v}\nrerun: OMS_SIM_PROFILE={} OMS_SIM_SEED={} cargo test -p oms-sim --test sim", p.name, seed),
        }
    }
    let sum = |f: fn(&SimStats) -> u64| all.iter().map(f).sum::<u64>();
    println!(
        "profile={} seeds={} inputs={} records={} requests={} accepted={} rejected={} duplicates={} conflicts={} sends={} venue_orders={} venue_fills={} crashes={} link_drops={} in_doubt={} not_placed={} parked={} purged={} mismatches={} external_fills={}",
        p.name,
        all.len(),
        sum(|s| s.inputs),
        sum(|s| s.records),
        sum(|s| s.requests),
        sum(|s| s.accepted),
        sum(|s| s.rejected),
        sum(|s| s.duplicates),
        sum(|s| s.conflicts),
        sum(|s| s.sends),
        sum(|s| s.venue_orders),
        sum(|s| s.venue_fills),
        sum(|s| u64::from(s.crashes)),
        sum(|s| u64::from(s.link_drops)),
        sum(|s| s.in_doubt_seen),
        sum(|s| s.closed_not_placed),
        sum(|s| s.parked_seen),
        sum(|s| s.purged),
        sum(|s| s.mismatches),
        sum(|s| s.external_fills),
    );
    if p.margin {
        println!(
            "profile={} margin: leverage_checked={} leverage_rejects={}",
            p.name,
            sum(|s| s.leverage_checked),
            sum(|s| s.leverage_rejects),
        );
        println!(
            "profile={} margin: reducing_entries={} reducing_entry_cancels={} reducing_plain_rejects={} reducing_reduce_only_accepts={}",
            p.name,
            sum(|s| s.reducing_entries),
            sum(|s| s.reducing_entry_cancels),
            sum(|s| s.reducing_plain_rejects),
            sum(|s| s.reducing_reduce_only_accepts),
        );
        println!(
            "profile={} margin: reducing_clears={} clear_refusals={} reducing_day_rolls={} reducing_recoveries={} reducing_restarts={}",
            p.name,
            sum(|s| s.reducing_clears),
            sum(|s| s.clear_refusals),
            sum(|s| s.reducing_day_rolls),
            sum(|s| s.reducing_recoveries),
            sum(|s| s.reducing_restarts),
        );
    }
    all
}

#[test]
fn profile_calm() {
    let all = run_profile(Profile::calm());
    // Without faults nothing may be in doubt and nothing may mismatch.
    assert!(all.iter().all(|s| s.in_doubt_seen == 0 && s.mismatches == 0));
}

#[test]
fn profile_lossy() {
    let all = run_profile(Profile::lossy());
    if all.len() >= 10 {
        // The profile must actually reach the paths it is meant to cover.
        assert!(all.iter().map(|s| s.in_doubt_seen).sum::<u64>() > 0, "no send outcome was ever unknown");
        assert!(all.iter().map(|s| s.closed_not_placed).sum::<u64>() > 0, "absence rule never used");
        assert!(all.iter().map(|s| s.duplicates).sum::<u64>() > 0, "no duplicate request was answered");
        assert!(all.iter().map(|s| s.conflicts).sum::<u64>() > 0, "no conflicting request was answered");
    }
}

#[test]
fn profile_crashy() {
    let all = run_profile(Profile::crashy());
    if all.len() >= 10 {
        assert!(all.iter().map(|s| u64::from(s.crashes)).sum::<u64>() > 0, "no crash was injected");
    }
}

#[test]
fn profile_weak_evidence() {
    let all = run_profile(Profile::weak_evidence());
    if all.len() >= 10 {
        assert!(all.iter().map(|s| s.parked_seen).sum::<u64>() > 0, "no fill ever waited for its order");
    }
}

#[test]
fn profile_purge() {
    let all = run_profile(Profile::purge());
    if all.len() >= 10 {
        assert!(all.iter().map(|s| s.purged).sum::<u64>() > 0, "nothing was purged");
    }
}

#[test]
fn profile_skewed() {
    run_profile(Profile::skewed());
}

#[test]
fn profile_external() {
    let all = run_profile(Profile::external());
    if all.len() >= 10 {
        assert!(all.iter().map(|s| s.external_fills).sum::<u64>() > 0, "no external fill was injected");
    }
}

/// The same seed gives the same journal, byte for byte.
#[test]
fn a_seed_reproduces_the_same_run() {
    for seed in [3u64, 17, 99] {
        let a = run(seed, Profile::crashy()).unwrap();
        let b = run(seed, Profile::crashy()).unwrap();
        assert_eq!(a, b);
        assert_ne!(a.trace_hash, run(seed + 1, Profile::crashy()).unwrap().trace_hash);
    }
}

// ---- assumptions: breaking one must be caught --------------------------------

/// The absence rule assumes the venue refuses a request after its expiry. If
/// the venue does not, an order can appear after it was closed as not placed.
#[test]
fn absence_rule_depends_on_venue_side_expiry() {
    let mut p = Profile::lossy();
    p.enforce_expiry = false;
    p.p_req_slow = 250;
    let caught = (1..=60u64).filter_map(|seed| run(seed, p.clone()).err()).next();
    let v = caught.expect("a venue that ignores expiry must break a checked property");
    println!("caught as expected: {v}");
    assert_broken_absence(&v);
}

/// The absence rule also assumes the clocks differ by less than the margin.
#[test]
fn absence_rule_depends_on_the_clock_margin() {
    let mut p = Profile::lossy();
    p.venue_skew_us = -6_000_000; // venue 6 s behind, margin 2 s
    p.p_req_slow = 250;
    let caught = (1..=60u64).filter_map(|seed| run(seed, p.clone()).err()).next();
    let v = caught.expect("a clock difference beyond the margin must break a checked property");
    println!("caught as expected: {v}");
    assert_broken_absence(&v);
}

/// What a wrongly closed order looks like to the checks: the venue holds or
/// fills an order the core closed as never placed. Any of these properties
/// can be the first to notice; an unrelated failure (an invariant, a replay
/// difference) would mean the test passed for the wrong reason.
fn assert_broken_absence(v: &oms_sim::world::Violation) {
    let expected = ["bridge", "venue-position-limit", "position-bounds", "final-orders", "final-fills", "final-position", "no-false-mismatch"];
    assert!(expected.contains(&v.property), "caught by an unexpected property: {v}");
}

// ---- the venue margin model (C-MARGIN) ---------------------------------------

fn x() -> oms_core::InstrumentId {
    oms_core::InstrumentId::new("X").unwrap()
}

fn amount(s: &str) -> oms_core::Amount {
    oms_core::Amount::parse(s).unwrap()
}

fn place_req(order: u64, side: oms_core::Side, qty: &str, px: &str) -> venue::PlaceReq {
    venue::PlaceReq {
        client: oms_core::VenueClientId { instance_id: 7, order_id: oms_core::OrderId(order) },
        instrument: x(),
        side,
        qty: oms_core::Qty::parse(qty).unwrap(),
        px: oms_core::Px::parse(px).unwrap(),
        tif: oms_core::Tif::Gtc,
        expires_at: u64::MAX,
        reduce_only: false,
    }
}

fn qty(s: &str) -> oms_core::Qty {
    oms_core::Qty::parse(s).unwrap()
}

/// The model's account value against a hand-worked example. The numbers are
/// worked out here, not with the model's code:
///   deposit                                   100
///   buy 0.5 at 100, mark 100: fee 2 bps of 50   -0.01  ->  99.99
///   mark 100 -> 104: 0.5 x 4                    +2     -> 101.99
///   funding 0.1 % of the position value 52      -0.052 -> 101.938
/// This is our model. It does not show that the venue computes it this way.
#[test]
fn world_account_value_follows_fills_price_fees_and_funding() {
    let mut v = venue::SimVenue::new(true);
    v.deposit(amount("100"));
    v.set_mark(&x(), oms_core::Px::parse("100").unwrap());
    assert_eq!(v.account_value(), amount("100"));

    let placed = v.place(1, &place_req(1, oms_core::Side::Buy, "0.5", "100"), oms_core::Qty::parse("0.5").unwrap());
    assert!(matches!(placed, venue::PlaceResp::Placed { .. }), "{placed:?}");
    assert_eq!(v.account_value(), amount("99.99"), "after the fill and its fee");

    v.set_mark(&x(), oms_core::Px::parse("104").unwrap());
    assert_eq!(v.account_value(), amount("101.99"), "after the price move");

    let charged = v.charge_funding(&x(), 1_000);
    assert_eq!(charged, amount("0.052"));
    assert_eq!(v.account_value(), amount("101.938"), "after funding");
}

/// The venue contract the hold table leans on: a reduce-only order never
/// makes the position larger and never opens one on the other side. This is
/// an assumption written into the model from the venue's documentation, not
/// something observed at the venue (T5).
#[test]
fn world_refuses_a_reduce_only_order_that_would_grow_the_position() {
    use oms_core::Side::{Buy, Sell};
    let reduce_only = |order, side, q: &str| venue::PlaceReq { reduce_only: true, ..place_req(order, side, q, "100") };
    let refused = |r: venue::PlaceResp| matches!(r, venue::PlaceResp::Rejected { .. });
    let mut v = venue::SimVenue::new(true);

    // No position: nothing to reduce, on either side.
    assert!(refused(v.place(1, &reduce_only(1, Buy, "0.1"), qty("0"))), "flat, buy");
    assert!(refused(v.place(1, &reduce_only(2, Sell, "0.1"), qty("0"))), "flat, sell");

    // Long 0.5.
    v.place(2, &place_req(3, Buy, "0.5", "100"), qty("0.5"));
    assert_eq!(v.position(&x()), 50_000_000);
    assert!(refused(v.place(3, &reduce_only(4, Buy, "0.1"), qty("0"))), "long, a buy adds to it");
    assert!(refused(v.place(3, &reduce_only(5, Sell, "0.51"), qty("0"))), "long 0.5, a sell of 0.51 would end short");
    assert_eq!(v.position(&x()), 50_000_000, "a refused order changes nothing");

    // A reduce-only sell of the whole position rests.
    let venue::PlaceResp::Placed { oid, .. } = v.place(4, &reduce_only(6, Sell, "0.5"), qty("0")) else {
        panic!("a reduce-only sell of the whole long position must be placed");
    };
    // The position shrinks to 0.2 by another order while it rests.
    v.place(5, &place_req(7, Sell, "0.3", "100"), qty("0.3"));
    assert_eq!(v.position(&x()), 20_000_000);
    // Filling the resting order in full trades only the 0.2 that is left,
    // and the venue cancels the rest of it.
    let fill = v.fill(6, oid, qty("0.5")).expect("the order trades what reduces");
    assert_eq!(fill.qty, qty("0.2"));
    assert_eq!(v.position(&x()), 0, "never through zero");
    assert!(matches!(v.orders[&oid].status, venue::SimStatus::Canceled(_)), "{:?}", v.orders[&oid].status);
    assert!(v.fill(7, oid, qty("0.1")).is_none());
}

// ---- the margin rules in the world (C-MARGIN) --------------------------------

/// A world with a margin policy and no random faults, driven by the test.
fn scripted_margin_world() -> World {
    let mut w = World::new(1, Profile { name: "margin_scripted", margin: true, ..Profile::calm() });
    w.start().unwrap();
    w.advance(2_000_000).unwrap();
    w
}

fn reducing_rules(w: &World) -> Vec<oms_core::RuleId> {
    w.durable
        .iter()
        .flat_map(|r| &r.events)
        .filter_map(|e| match e {
            oms_core::Event::ReducingEntered { rule, .. } => Some(*rule),
            _ => None,
        })
        .collect()
}

fn reported_value(w: &World) -> Option<oms_core::Amount> {
    w.core.margin.report.as_ref().map(|r| r.account_value)
}

/// Fees and funding count as loss (G10). The price never moves here, so the
/// loss is made of nothing else. Hand-worked, with the world's start capital
/// of 100 and the venue fee of 2 bps:
///   buy 0.5 at 100: fee 0.01                 ->  99.99
///   funding 9.98 % of the position value 50  ->  95.00   5.00 % below: no latch
///   funding 0.02 % of the position value 50  ->  94.99   more than 5 %: REDUCING
/// Which balance the venue pays fees and funding from is not verified (T3).
#[test]
fn a_loss_made_only_of_fees_and_funding_enters_reducing() {
    let mut w = scripted_margin_world();
    assert_eq!(reported_value(&w), Some(amount("100")), "the first reports state the start capital");

    w.submit(oms_core::NewOrderReq {
        strategy: oms_core::StrategyId::new("a").unwrap(),
        client_order_id: oms_core::ClientOrderId::new("k1").unwrap(),
        instrument: x(),
        side: oms_core::Side::Buy,
        qty: oms_core::Qty::parse("0.5").unwrap(),
        px: oms_core::Px::parse("100").unwrap(),
        tif: oms_core::Tif::Gtc,
        reduce_only: false,
        expires_at: w.now + 5_000_000,
    })
    .unwrap();
    assert_eq!(w.stats.accepted, 1, "the plain order is accepted");
    w.advance(500_000).unwrap();
    // The venue fills whatever is still open, at the limit price of 100.
    for oid in w.venue.open_oids() {
        let rest = w.venue.orders[&oid].remaining();
        w.venue.fill(w.now, oid, rest);
    }
    assert_eq!(w.venue.position(&x()), 50_000_000, "long 0.5");
    w.advance(1_000_000).unwrap();
    assert_eq!(reported_value(&w), Some(amount("99.99")));
    assert_eq!(reducing_rules(&w), vec![]);

    assert_eq!(w.venue.charge_funding(&x(), 99_800), amount("4.99"));
    w.advance(1_000_000).unwrap();
    assert_eq!(reported_value(&w), Some(amount("95")));
    assert_eq!(reducing_rules(&w), vec![], "exactly 5 % below the base does not latch");
    assert!(!w.core.holds(None).contains(&oms_core::HoldReason::Reducing));

    assert_eq!(w.venue.charge_funding(&x(), 200), amount("0.01"));
    w.advance(1_000_000).unwrap();
    assert_eq!(reported_value(&w), Some(amount("94.99")));
    assert_eq!(reducing_rules(&w), vec![oms_core::RuleId::DailyLoss]);
    assert!(w.core.holds(None).contains(&oms_core::HoldReason::Reducing));
    assert_eq!(w.venue.mark(&x()), oms_core::Px::parse("100").unwrap(), "the price never moved");
}

/// Every seed of `margin_calm`, run once for the tests that read it.
fn margin_calm_runs() -> &'static [SimStats] {
    static RUNS: std::sync::OnceLock<Vec<SimStats>> = std::sync::OnceLock::new();
    RUNS.get_or_init(|| run_profile(Profile::margin_calm()))
}

fn total(all: &[SimStats], f: fn(&SimStats) -> u64) -> u64 {
    all.iter().map(f).sum()
}

/// S5 under random interleavings. The run itself fails (`leverage-gate`)
/// when a plain order was accepted that took the account exposure above
/// twice the account value at acceptance. The exposure is worked out by the
/// world from the core's books, not by the core's own function.
#[test]
fn profile_margin_calm() {
    let all = margin_calm_runs();
    if all.len() >= 10 {
        // The profile must reach the rule: orders were judged, and some
        // were refused by it.
        assert!(total(all, |s| s.leverage_checked) > 0, "no accepted plain order was judged");
        assert!(total(all, |s| s.leverage_rejects) > 0, "the leverage gate never refused an order");
    }
}

/// S8 under random interleavings. The run itself fails when, while the
/// account is REDUCING, a plain order is accepted (`reducing-accept`), or
/// when a plain order that is open at the entry is left without a cancel
/// request by the end of that same input (`reducing-cancel`).
#[test]
fn profile_margin_calm_reducing_invariant() {
    let all = margin_calm_runs();
    if all.len() >= 10 {
        assert!(total(all, |s| s.reducing_entries) > 0, "REDUCING was never entered");
        assert!(total(all, |s| s.reducing_entry_cancels) > 0, "no plain order was open at an entry");
        assert!(total(all, |s| s.reducing_plain_rejects) > 0, "no plain order was refused while REDUCING");
        assert!(total(all, |s| s.reducing_reduce_only_accepts) > 0, "no reduce-only order was accepted while REDUCING");
    }
}

fn is_reducing(w: &World) -> bool {
    w.core.holds(None).contains(&oms_core::HoldReason::Reducing)
}

/// S14: only an operator input ends REDUCING. First one exact sequence
/// (a restart, then a recovered account value, then the clear), then the
/// random runs of `margin_calm`, which cross a UTC midnight. A run itself
/// fails (`reducing-release`) when the latch is gone after an input that is
/// not the operator's clear, when the journal holds a `reducing_cleared`
/// under another input, or when a restart gives a state that differs from
/// what the journal says.
#[test]
fn profile_margin_calm_release_invariant() {
    let mut w = scripted_margin_world();
    // A withdrawal of 6 reads as a loss of 6 %.
    w.venue.deposit(amount("-6"));
    w.advance(1_000_000).unwrap();
    assert!(is_reducing(&w));
    // A restart keeps the latch.
    w.restart().unwrap();
    w.advance(5_000_000).unwrap();
    assert_eq!(w.stats.crashes, 1);
    assert_eq!(reported_value(&w), Some(amount("94")), "reports arrive again after the restart");
    assert!(is_reducing(&w), "a restart does not end REDUCING");
    // The account value comes back to the day base. The latch stays.
    w.venue.deposit(amount("6"));
    w.advance(1_000_000).unwrap();
    assert_eq!(reported_value(&w), Some(amount("100")));
    assert!(is_reducing(&w), "a recovery does not end REDUCING");
    // The operator's clear ends it.
    w.operator(oms_core::OperatorCmd::ClearReduce { note: "checked by hand".into() }).unwrap();
    assert!(!is_reducing(&w));

    let all = margin_calm_runs();
    if all.len() >= 10 {
        assert!(total(all, |s| s.reducing_clears) > 0, "REDUCING was never cleared by the operator");
        assert!(total(all, |s| s.reducing_day_rolls) > 0, "no run crossed midnight while REDUCING");
        assert!(total(all, |s| s.reducing_recoveries) > 0, "the account value never recovered while REDUCING");
    }
}
