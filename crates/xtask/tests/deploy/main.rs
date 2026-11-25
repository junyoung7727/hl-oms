//! The deployment check: the files under `deploy/` (systemd units, timers,
//! the slice, the install map, the venue list) against the rules of
//! ADR-035. Pure over an in-memory tree: no network, no clock, no temp file.
//! Run before an install with `cargo test --locked -p xtask --test deploy`.

#![allow(clippy::unwrap_used)]

mod expr;
mod ini;
mod tree;
mod units;

use std::collections::BTreeSet;

use tree::{Finding, Tree};

pub fn check(t: &Tree) -> Vec<Finding> {
    let mut out = Vec::new();
    let p = tree::parse(t, &mut out);
    units::check(t, &p, &mut out);
    out
}

/// Asserts that the rules of `f` are exactly `want`: the planted defect is
/// found, and by the rules the test is about only.
pub fn assert_rules(f: &[Finding], want: &[&str]) {
    let got: BTreeSet<&str> = f.iter().map(|x| x.rule).collect();
    let want: BTreeSet<&str> = want.iter().copied().collect();
    let list: Vec<String> = f.iter().map(ToString::to_string).collect();
    assert_eq!(got, want, "findings:\n{}", list.join("\n"));
}

#[test]
fn the_deploy_tree_passes() {
    let f = check(&Tree::real());
    let list: Vec<String> = f.iter().map(ToString::to_string).collect();
    assert!(f.is_empty(), "{}", list.join("\n"));
}
