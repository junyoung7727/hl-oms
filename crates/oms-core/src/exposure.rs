//! Account exposure and the leverage gate (C-MARGIN, 계좌 레버리지 검사).
//!
//! Exposure is counted in units of 1e-16 quote: a price in 1e-8 units times
//! a quantity in 1e-8 units, as an `i128`. Nothing is divided, so nothing is
//! rounded. The limit side of the comparison has the same unit: a ratio in
//! 1e-8 units times an account value in 1e-8 quote units.

use crate::config::MarginPolicy;
use crate::core::Core;
use crate::event::RejectReason;
use crate::ids::{InstrumentId, Ts};
use crate::input::NewOrderReq;
use crate::model::Side;
use serde::{Serialize, Serializer};
use std::collections::BTreeMap;
use std::fmt;

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
        if !abs.is_multiple_of(UNIT) {
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
    /// the position can reach, valued as the leverage gate values it, with
    /// the last reference price of each instrument whatever its age. `None`
    /// when an instrument with a position or an order has no reference
    /// price at all, or when the sum does not fit in an `i128`.
    pub fn account_exposure(&self) -> Option<Exposure> {
        self.exposure_with(None, None).ok().map(Exposure)
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
    ///
    /// With `fresh_at`, every reference price used must be fresh at that
    /// record time. Exposure that cannot be valued is never counted as zero.
    fn exposure_with(
        &self,
        extra: Option<&NewOrderReq>,
        fresh_at: Option<Ts>,
    ) -> Result<i128, Unvalued> {
        let max_age = self.config.policy.ref_price_max_age_us;
        let reference = |i: &InstrumentId| {
            self.ref_px
                .get(i)
                .filter(|p| fresh_at.is_none_or(|at| at.saturating_sub(p.at) <= max_age))
                .map(|p| p.px.0)
                .ok_or(Unvalued::NoReferencePrice)
        };
        // (buys, sells) per instrument.
        let mut legs: BTreeMap<&InstrumentId, (i128, i128)> = BTreeMap::new();
        for (i, book) in &self.account {
            if book.pos.0 != 0 {
                legs.insert(i, (0, 0));
            }
        }
        // A resting order counts until terminal evidence releases its
        // reservation. A cancel request alone releases nothing.
        let resting = self
            .open
            .iter()
            .filter_map(|id| self.orders.get(id))
            .filter(|o| o.reserved.0 > 0)
            .map(|o| (&o.instrument, o.side, o.reserved, o.px));
        let extra = extra.map(|r| (&r.instrument, r.side, r.qty, r.px));
        for (i, side, qty, px) in resting.chain(extra) {
            // An order is valued at the higher of its limit price and the
            // reference price: the worse of where it rests and where the
            // market is.
            let value = i128::from(qty.0) * i128::from(px.0.max(reference(i)?));
            let leg = legs.entry(i).or_default();
            let sum = match side {
                Side::Buy => &mut leg.0,
                Side::Sell => &mut leg.1,
            };
            *sum = sum.checked_add(value).ok_or(Unvalued::Overflow)?;
        }
        // One product of two 1e-8 integers always fits in an i128. A sum of
        // them may not: it is then reported, never wrapped.
        let mut account = 0i128;
        for (i, (buys, sells)) in legs {
            let position = i128::from(self.account_book(i).pos.0) * i128::from(reference(i)?);
            let up = position.checked_add(buys).ok_or(Unvalued::Overflow)?.max(0);
            let down = sells
                .checked_sub(position)
                .ok_or(Unvalued::Overflow)?
                .max(0);
            account = account
                .checked_add(up.max(down))
                .ok_or(Unvalued::Overflow)?;
        }
        Ok(account)
    }

    /// Judges a plain order at record time `at` against the leverage gate.
    /// `None` means the gate does not refuse it. The caller has checked
    /// that the quantity and the price are positive.
    pub(crate) fn leverage_gate(
        &self,
        policy: &MarginPolicy,
        r: &NewOrderReq,
        at: Ts,
    ) -> Option<RejectReason> {
        // Without a fresh report the order is already refused, and there is
        // no account value to judge it with.
        let report = self
            .margin
            .report
            .as_ref()
            .filter(|_| self.margin_report_is_fresh(at))?;
        // An account worth nothing carries nothing, whatever the order.
        if report.account_value.0 <= 0 {
            return Some(RejectReason::AccountLeverage);
        }
        let (before, after) = match (
            self.exposure_with(None, Some(at)),
            self.exposure_with(Some(r), Some(at)),
        ) {
            (Ok(before), Ok(after)) => (before, after),
            (Err(Unvalued::NoReferencePrice), _) | (_, Err(Unvalued::NoReferencePrice)) => {
                return Some(RejectReason::MarginUnavailable);
            }
            (Err(Unvalued::Overflow), _) | (_, Err(Unvalued::Overflow)) => {
                return Some(RejectReason::Overflow);
            }
        };
        let limit = i128::from(policy.max_account_leverage.0) * i128::from(report.account_value.0);
        // Above the limit alone does not refuse: a price move can put the
        // account there, and an order that does not raise exposure must
        // still pass so that the account can come down.
        (after > limit && after > before).then_some(RejectReason::AccountLeverage)
    }
}

/// Why an exposure has no value.
enum Unvalued {
    /// An instrument with a position or an order has no reference price
    /// (or, where freshness is asked for, no fresh one).
    NoReferencePrice,
    /// The sum does not fit in an `i128`.
    Overflow,
}
