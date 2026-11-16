//! Account exposure and the leverage gate (C-MARGIN, 계좌 레버리지 검사).
//!
//! Exposure is counted in units of 1e-16 quote: a price in 1e-8 units times
//! a quantity in 1e-8 units, as an `i128`. Nothing is divided, so nothing is
//! rounded. The limit side of the comparison has the same unit: a ratio in
//! 1e-8 units times an account value in 1e-8 quote units.

use crate::config::MarginPolicy;
use serde::{Serialize, Serializer};
use std::fmt;
use crate::core::Core;
use crate::input::NewOrderReq;
use crate::ids::InstrumentId;
use crate::model::Side;
use std::collections::BTreeMap;
use crate::num::{Px, Qty};

/// Account exposure in units of 1e-16 quote (see the module text). Shown and
/// serialised as a plain decimal of quote units.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Exposure(pub i128);

impl fmt::Display for Exposure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        const UNIT: u128 = 10_000_000_000_000_000;
        let abs = self.0.unsigned_abs();
        let sign = if self.0 < 0 { "-" } else { "" };
        write!(f, "{sign}{}", abs / UNIT)?;
        if abs % UNIT != 0 {
            let digits = format!("{:016}", abs % UNIT);
            write!(f, ".{}", digits.trim_end_matches('0'))?;
        }
        Ok(())
    }
}

impl Serialize for Exposure {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl Core {
    /// The account exposure now: the sum over instruments of the furthest
    /// the position can reach, valued as the leverage gate values it. `None`
    /// when it is not known.
    pub fn account_exposure(&self) -> Option<Exposure> {
        Some(Exposure(self.exposure_with(None)))
    }

    /// Account exposure with `extra` counted as one more resting order.
    ///
    /// Per instrument, with `pos` the account book position, `ref` the
    /// reference price and every resting order (reduce-only included) at
    /// its remaining reserved quantity:
    ///   N_up   = max(0,  pos x ref + sum of buys  x max(limit, ref))
    ///   N_down = max(0, -pos x ref + sum of sells x max(limit, ref))
    /// The instrument's exposure is the larger of the two, and the account
    /// exposure is the sum over instruments.
    fn exposure_with(&self, extra: Option<&NewOrderReq>) -> i128 {
        let reference = |i: &InstrumentId| self.ref_px.get(i).map_or(0, |p| p.px.0);
        // An order is valued at the higher of its limit price and the
        // reference price: the worse of where it rests and where the market
        // is.
        let value = |i: &InstrumentId, qty: Qty, px: Px| i128::from(qty.0) * i128::from(px.0.max(reference(i)));
        // (buys, sells) per instrument. Every instrument with a position or
        // an order has an account book.
        let mut legs: BTreeMap<&InstrumentId, (i128, i128)> = self.account.keys().map(|i| (i, (0, 0))).collect();
        // A resting order counts until terminal evidence releases its
        // reservation. A cancel request alone releases nothing.
        let resting = self
            .open
            .iter()
            .filter_map(|id| self.orders.get(id))
            .map(|o| (&o.instrument, o.side, o.reserved, o.px));
        let extra = extra.map(|r| (&r.instrument, r.side, r.qty, r.px));
        for (i, side, qty, px) in resting.chain(extra) {
            let leg = legs.entry(i).or_default();
            match side {
                Side::Buy => leg.0 += value(i, qty, px),
                Side::Sell => leg.1 += value(i, qty, px),
            }
        }
        let mut account = 0i128;
        for (i, (buys, sells)) in legs {
            let position = i128::from(self.account_book(i).pos.0) * i128::from(reference(i));
            let up = (position + buys).max(0);
            let down = (sells - position).max(0);
            account += up.max(down);
        }
        account
    }

    /// Whether the leverage gate refuses this plain order. The caller has
    /// checked that the quantity and the price are positive.
    pub(crate) fn leverage_gate_refuses(&self, policy: &MarginPolicy, r: &NewOrderReq) -> bool {
        let Some(report) = &self.margin.report else {
            return false;
        };
        let before = self.exposure_with(None);
        let after = self.exposure_with(Some(r));
        let limit = i128::from(policy.max_account_leverage.0) * i128::from(report.account_value.0);
        // Above the limit alone does not refuse: a price move can put the
        // account there, and an order that does not raise exposure must
        // still pass so that the account can come down.
        after > limit && after > before
    }
}
