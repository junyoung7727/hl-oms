//! Account exposure and the leverage gate (C-MARGIN, 계좌 레버리지 검사).
//!
//! Exposure is counted in units of 1e-16 quote: a price in 1e-8 units times
//! a quantity in 1e-8 units, as an `i128`. Nothing is divided, so nothing is
//! rounded. The limit side of the comparison has the same unit: a ratio in
//! 1e-8 units times an account value in 1e-8 quote units.

use crate::config::MarginPolicy;
use crate::core::Core;
use crate::input::NewOrderReq;

impl Core {
    /// Whether the leverage gate refuses this plain order. The caller has
    /// checked that the quantity and the price are positive.
    pub(crate) fn leverage_gate_refuses(&self, policy: &MarginPolicy, r: &NewOrderReq) -> bool {
        let Some(report) = &self.margin.report else {
            return false;
        };
        let after = i128::from(r.qty.0) * i128::from(r.px.0);
        let limit = i128::from(policy.max_account_leverage.0) * i128::from(report.account_value.0);
        after > limit
    }
}
