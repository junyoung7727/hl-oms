//! Profit and loss per instrument by the average-cost method.
//!
//! The OMS books keep the position, the traded notional and the fees, but
//! no cost basis (C-TAIL), so the projector keeps one from the journal:
//!
//! - A trade that grows the position (or opens one) moves the average price
//!   to the quantity-weighted mean.
//! - A trade against the position realises `closed × (px − avg)` for a long
//!   (`(avg − px)` for a short) on the closed part; a remainder past zero
//!   opens a new position at `px`.
//! - Fees and funding paid are subtracted from the realised amount; funding
//!   received is added.
//! - A position adjustment has no price: it changes the quantity at the
//!   average price and realises nothing. An adjustment from flat leaves the
//!   average price unknown (`None`) until the next trade.
//!
//! The values are floats: they are observations, never inputs to a
//! decision. Quantities are kept exact (`i128` in units of 1e-8).

use oms_core::num::SCALE;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pnl {
    /// Signed position in units of 1e-8.
    pub pos: i128,
    /// Average entry price of `pos`; `None` while flat or unknown.
    pub avg: Option<f64>,
    /// Realised profit in quote units, after fees and funding.
    pub realized: f64,
}

fn units(q: i128) -> f64 {
    q as f64 / SCALE as f64
}

impl Pnl {
    /// A trade of `qty` (signed: buy positive) at `px`, paying `fee`.
    pub fn trade(&mut self, qty: i128, px: f64, fee: f64) {
        self.realized -= fee;
        if qty == 0 {
            return;
        }
        let same_way = self.pos == 0 || (self.pos > 0) == (qty > 0);
        if same_way {
            let old = units(self.pos.abs());
            let add = units(qty.abs());
            self.avg = Some(match self.avg {
                Some(avg) if old > 0.0 => (avg * old + px * add) / (old + add),
                _ => px,
            });
            self.pos += qty;
            return;
        }
        let closed = qty.abs().min(self.pos.abs());
        if let Some(avg) = self.avg {
            let per_unit = if self.pos > 0 { px - avg } else { avg - px };
            self.realized += units(closed) * per_unit;
        }
        self.pos += qty;
        if self.pos == 0 {
            self.avg = None;
        } else if (self.pos > 0) == (qty > 0) {
            // Crossed zero: the remainder is a new position at this price.
            self.avg = Some(px);
        }
    }

    /// A change of the quantity without a price (an adjustment).
    pub fn shift(&mut self, qty: i128) {
        let before = self.pos;
        self.pos += qty;
        if self.pos == 0 || before == 0 || (before > 0) != (self.pos > 0) {
            self.avg = None;
        }
    }

    /// Funding: `amount` is positive when the account pays.
    pub fn funding(&mut self, amount: f64) {
        self.realized -= amount;
    }

    /// Unrealised profit at `mark`; `None` without a position or an average
    /// price.
    pub fn upnl(&self, mark: f64) -> Option<f64> {
        if self.pos == 0 {
            return Some(0.0);
        }
        self.avg.map(|avg| units(self.pos) * (mark - avg))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ONE: i128 = SCALE as i128;

    #[test]
    fn average_cost_realises_on_the_closed_part_and_reopens_past_zero() {
        let mut p = Pnl::default();
        p.trade(ONE, 100.0, 0.1);
        p.trade(ONE, 110.0, 0.1);
        assert_eq!(p.avg, Some(105.0));
        // Sell 3: closes 2 at +5 each, opens a short of 1 at 120.
        p.trade(-3 * ONE, 120.0, 0.0);
        assert_eq!(p.pos, -ONE);
        assert!((p.realized - (30.0 - 0.2)).abs() < 1e-9, "{}", p.realized);
        assert_eq!(p.avg, Some(120.0));
        assert_eq!(p.upnl(110.0), Some(10.0));
        p.funding(1.5);
        assert!((p.realized - 28.3).abs() < 1e-9);
    }

    #[test]
    fn an_adjustment_from_flat_has_no_average_price() {
        let mut p = Pnl::default();
        p.shift(ONE);
        assert_eq!(p.avg, None);
        assert_eq!(p.upnl(100.0), None);
        p.trade(ONE, 100.0, 0.0);
        assert_eq!(p.avg, Some(100.0));
    }
}
