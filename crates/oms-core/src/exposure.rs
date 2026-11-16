//! Account exposure and the leverage gate (C-MARGIN, 계좌 레버리지 검사).
//!
//! Exposure is counted in units of 1e-16 quote: a price in 1e-8 units times
//! a quantity in 1e-8 units, as an `i128`. Nothing is divided, so nothing is
//! rounded. The limit side of the comparison has the same unit: a ratio in
//! 1e-8 units times an account value in 1e-8 quote units.

use crate::config::MarginPolicy;
use crate::core::Core;
use crate::input::NewOrderReq;
use crate::model::Side;
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
        // Every resting order of the instrument, reduce-only included,
        // counts at its remaining reserved quantity until terminal evidence
        // releases it.
        let (mut buys, mut sells) = (0i128, 0i128);
        for o in self.open.iter().filter_map(|id| self.orders.get(id)) {
            if o.instrument == r.instrument {
                match o.side {
                    Side::Buy => buys += value(o.reserved, o.px),
                    Side::Sell => sells += value(o.reserved, o.px),
                }
            }
        }
        match r.side {
            Side::Buy => buys += value(r.qty, r.px),
            Side::Sell => sells += value(r.qty, r.px),
        }
        // The furthest the position can reach on each side, at the
        // reference price; the larger side is the exposure.
        let position = i128::from(self.account_book(&r.instrument).pos.0) * i128::from(reference);
        let up = (position + buys).max(0);
        let down = (sells - position).max(0);
        let after = up.max(down);
        let limit = i128::from(policy.max_account_leverage.0) * i128::from(report.account_value.0);
        after > limit
    }
}
