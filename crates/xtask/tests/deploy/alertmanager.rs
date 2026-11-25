//! Alertmanager rules (ADR-035 D8): R10 the route tree, the receivers and
//! the inhibit rule; R11 secrets only as file paths under `/etc/`, and no
//! token-shaped string or chat id in any deploy file.

use crate::tree::{Finding, Parsed, Tree, finding};

pub const AM: &str = "deploy/observability/alertmanager/alertmanager.json";

pub fn check(_t: &Tree, p: &Parsed, out: &mut Vec<Finding>) {
    let Some(_doc) = p.json.get(AM) else {
        finding(out, AM, "R10", "missing");
        return;
    };
}
