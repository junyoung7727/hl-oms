//! Depth-agnostic in-memory order book.
//!
//! Each side is an ordered map from price to quantity. A snapshot replaces both
//! sides; a delta sets a level, and quantity 0 removes it. The book keeps every
//! level the venue sent; a max depth limits only what the read methods show
//! (the best N levels per side). Dropping levels on update would show a hole:
//! when a better level is removed, the venue sends nothing for a level the
//! book had dropped. A crossed book is not repaired; `is_crossed` reports it.

use std::collections::BTreeMap;

use crate::types::{BookUpdate, Level};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BookSide {
    Bid,
    Ask,
}

#[derive(Debug, Clone, Default)]
pub struct OrderBook {
    bids: BTreeMap<i64, i64>,
    asks: BTreeMap<i64, i64>,
    max_depth: Option<usize>,
}

impl OrderBook {
    pub fn new(max_depth: Option<usize>) -> Self {
        Self {
            bids: BTreeMap::new(),
            asks: BTreeMap::new(),
            max_depth,
        }
    }

    pub fn apply_snapshot(&mut self, update: &BookUpdate) {
        self.bids.clear();
        self.asks.clear();
        self.apply_delta(update);
    }

    pub fn apply_delta(&mut self, update: &BookUpdate) {
        for (side, levels) in [
            (&mut self.bids, &update.bids),
            (&mut self.asks, &update.asks),
        ] {
            for level in levels {
                if level.qty == 0 {
                    side.remove(&level.px);
                } else {
                    side.insert(level.px, level.qty);
                }
            }
        }
    }

    /// The levels of one side, best price first, at most the max depth.
    pub fn levels(&self, side: BookSide) -> Vec<Level> {
        self.iter(side).collect()
    }

    fn iter(&self, side: BookSide) -> Box<dyn Iterator<Item = Level> + '_> {
        let to_level = |(&px, &qty): (&i64, &i64)| Level { px, qty };
        let depth = self.max_depth.unwrap_or(usize::MAX);
        match side {
            BookSide::Bid => Box::new(self.bids.iter().rev().map(to_level).take(depth)),
            BookSide::Ask => Box::new(self.asks.iter().map(to_level).take(depth)),
        }
    }

    pub fn best_bid(&self) -> Option<Level> {
        self.iter(BookSide::Bid).next()
    }

    pub fn best_ask(&self) -> Option<Level> {
        self.iter(BookSide::Ask).next()
    }

    /// `best bid + best ask`, exact (twice the mid, in price units).
    pub fn mid_x2(&self) -> Option<i128> {
        Some(i128::from(self.best_bid()?.px) + i128::from(self.best_ask()?.px))
    }

    /// Mid price in price units, for monitoring; not an order input.
    pub fn mid(&self) -> Option<f64> {
        self.mid_x2().map(|m| m as f64 / 2.0)
    }

    /// `(ask − bid) / mid × 10^4`, for monitoring; not an order input.
    pub fn spread_bp(&self) -> Option<f64> {
        let (bid, ask) = (self.best_bid()?.px, self.best_ask()?.px);
        let mid_x2 = self.mid_x2()? as f64;
        Some((ask - bid) as f64 * 2.0 / mid_x2 * 10_000.0)
    }

    /// Sum of the quantities of the best `n` levels of one side (within the
    /// max depth), exact.
    pub fn depth_sum(&self, side: BookSide, n: usize) -> i128 {
        self.iter(side).take(n).map(|l| i128::from(l.qty)).sum()
    }

    pub fn is_crossed(&self) -> bool {
        matches!((self.best_bid(), self.best_ask()), (Some(b), Some(a)) if b.px >= a.px)
    }
}
