//! Account exposure and the leverage gate (C-MARGIN, 계좌 레버리지 검사).
//!
//! Exposure is counted in units of 1e-16 quote: a price in 1e-8 units times
//! a quantity in 1e-8 units, as an `i128`. Nothing is divided, so nothing is
//! rounded. The limit side of the comparison has the same unit: a ratio in
//! 1e-8 units times an account value in 1e-8 quote units.

use crate::config::MarginPolicy;
use crate::core::Core;
use crate::input::NewOrderReq;
use crate::num::{Px, Qty};

impl Core {
    /// Whether the leverage gate refuses this plain order. The caller has
    /// checked that the quantity and the price are positive.
    pub(crate) fn leverage_gate_refuses(&self, policy: &MarginPolicy, r: &NewOrderReq) -> bool {
        let Some(report) = &self.margin.report else {
            return false;
        };
        // An order is valued at the higher of its limit price and the
        // reference price: the worse of where it rests and where the market
        // is.
        let reference = self.ref_px.get(&r.instrument).map_or(0, |p| p.px.0);
        let value = |qty: Qty, px: Px| i128::from(qty.0) * i128::from(px.0.max(reference));
        // Every resting order of the instrument counts at its remaining
        // reserved quantity until terminal evidence releases it.
        let resting: i128 = self
            .open
            .iter()
            .filter_map(|id| self.orders.get(id))
            .filter(|o| o.instrument == r.instrument)
            .map(|o| value(o.reserved, o.px))
            .sum();
        let after = resting + value(r.qty, r.px);
        let limit = i128::from(policy.max_account_leverage.0) * i128::from(report.account_value.0);
        after > limit
    }
}
