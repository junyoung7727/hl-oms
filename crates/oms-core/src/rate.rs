//! The venue's request limit on the order path, as a pure rule with time as
//! input (ADR-028 §7d). The gateway asks before each create or cancel and
//! waits for the time it is told; nothing is dropped.
//!
//! A sliding window, not a token bucket: a bucket of capacity `n` refilled
//! at `n` per second lets `2n` requests into one second, a window bounds
//! every rolling second.

use std::collections::VecDeque;

use crate::ids::Ts;

/// What a request is, for the budget.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RateClass {
    /// A new order. Only while fewer than `order_limit` requests went out in
    /// the window.
    Order,
    /// A cancel. Also uses the reserve above `order_limit`, up to
    /// `total_limit`: a cancel only lowers risk.
    Cancel,
}

#[derive(Clone, Debug)]
pub struct RateBudget {
    window_us: u64,
    order_limit: usize,
    total_limit: usize,
    /// Times of the requests still inside the window, oldest first.
    sent: VecDeque<Ts>,
}

impl RateBudget {
    /// `order_limit` ≤ `total_limit`, both above zero.
    pub fn new(order_limit: usize, total_limit: usize, window_us: u64) -> RateBudget {
        assert!(0 < order_limit && order_limit <= total_limit && window_us > 0);
        RateBudget {
            window_us,
            order_limit,
            total_limit,
            sent: VecDeque::with_capacity(total_limit),
        }
    }

    /// Bybit: 10 per second per UID for each of create, amend and cancel
    /// (F18 §4.2); 8 for orders and 2 more for cancels (Q23), counted
    /// together over WS and REST (T8 unknown).
    pub fn bybit() -> RateBudget {
        RateBudget::new(8, 10, 1_000_000)
    }

    /// Takes one slot at `now`, or returns the earliest time a slot of this
    /// class can be free. A request at `t` counts while `now < t + window`;
    /// a clock that went back keeps it counted.
    pub fn take(&mut self, class: RateClass, now: Ts) -> Result<(), Ts> {
        while self
            .sent
            .front()
            .is_some_and(|t| now >= t.saturating_add(self.window_us))
        {
            self.sent.pop_front();
        }
        let limit = match class {
            RateClass::Order => self.order_limit,
            RateClass::Cancel => self.total_limit,
        };
        if self.sent.len() < limit {
            // Kept sorted: a clock that went back records the latest time.
            let at = self.sent.back().map_or(now, |last| now.max(*last));
            self.sent.push_back(at);
            return Ok(());
        }
        // The slot frees when all but `limit - 1` of the counted requests
        // have left the window.
        let k = self.sent.len() - limit;
        Err(self.sent[k].saturating_add(self.window_us))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: u64 = 1_000_000;

    #[test]
    fn the_ninth_order_in_a_second_waits_while_two_cancels_still_go_out() {
        let mut b = RateBudget::bybit();
        for _ in 0..8 {
            assert_eq!(b.take(RateClass::Order, 0), Ok(()));
        }
        assert_eq!(b.take(RateClass::Order, 10), Err(S));
        assert_eq!(b.take(RateClass::Cancel, 20), Ok(()));
        assert_eq!(b.take(RateClass::Cancel, 30), Ok(()));
        assert_eq!(b.take(RateClass::Cancel, 40), Err(S));
        // The orders keep waiting for the first slot to leave.
        assert_eq!(b.take(RateClass::Order, S - 1), Err(S));
        assert_eq!(b.take(RateClass::Order, S), Ok(()));
    }

    #[test]
    fn the_window_slides_with_each_request() {
        let mut b = RateBudget::bybit();
        for i in 0..8 {
            assert_eq!(b.take(RateClass::Order, i * 100_000), Ok(()));
        }
        // At 0.85 s eight went out in the last second; the first leaves at 1 s.
        assert_eq!(b.take(RateClass::Order, 850_000), Err(S));
        assert_eq!(b.take(RateClass::Order, S), Ok(()));
        // Now the second (0.1 s) is the oldest counted.
        assert_eq!(b.take(RateClass::Order, S + 1), Err(S + 100_000));
    }

    #[test]
    fn cancels_may_take_the_reserve_and_then_block_orders_too() {
        let mut b = RateBudget::bybit();
        for _ in 0..10 {
            assert_eq!(b.take(RateClass::Cancel, 0), Ok(()));
        }
        assert_eq!(b.take(RateClass::Cancel, 1), Err(S));
        // Ten counted, the order limit is eight: three must leave.
        assert_eq!(b.take(RateClass::Order, 1), Err(S));
    }

    #[test]
    fn a_clock_that_goes_back_does_not_free_a_slot() {
        let mut b = RateBudget::bybit();
        for _ in 0..8 {
            assert_eq!(b.take(RateClass::Order, 5 * S), Ok(()));
        }
        assert_eq!(b.take(RateClass::Order, 4 * S), Err(6 * S));
        assert_eq!(b.take(RateClass::Cancel, 4 * S), Ok(()));
        assert_eq!(b.take(RateClass::Cancel, 4 * S), Ok(()));
        assert_eq!(b.take(RateClass::Cancel, 4 * S), Err(6 * S));
    }

    /// Any rolling second holds at most 8 orders and 10 requests, however
    /// they arrive, and a request is held only when its class is full.
    #[test]
    fn no_rolling_second_holds_more_than_the_limits() {
        let mut seed: u64 = 0x9e37_79b9_7f4a_7c15;
        let mut next = || {
            seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
            seed >> 33
        };
        let mut b = RateBudget::bybit();
        let mut now = 0;
        let mut sent: Vec<(Ts, RateClass)> = Vec::new();
        for _ in 0..20_000 {
            now += next() % 150_000;
            let class = if next() % 3 == 0 { RateClass::Cancel } else { RateClass::Order };
            let in_window = |list: &[(Ts, RateClass)]| list.iter().filter(|(t, _)| now < t + S).count();
            let counted = in_window(&sent);
            match b.take(class, now) {
                Ok(()) => sent.push((now, class)),
                Err(at) => {
                    let limit = if class == RateClass::Order { 8 } else { 10 };
                    assert!(counted >= limit, "held with {counted} counted");
                    assert!(at > now);
                }
            }
        }
        for (i, (t, _)) in sent.iter().enumerate() {
            let window = &sent[i..];
            let total = window.iter().filter(|(u, _)| *u < t + S).count();
            let orders = window
                .iter()
                .filter(|(u, c)| *u < t + S && *c == RateClass::Order)
                .count();
            assert!(total <= 10 && orders <= 8, "{total} requests, {orders} orders from {t}");
        }
        assert!(sent.len() > 10_000, "the schedule never filled the window: {}", sent.len());
    }
}
