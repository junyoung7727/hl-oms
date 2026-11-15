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
