//! The venue's request limit on the order path, as a pure rule with time as
//! input (ADR-028 §7d). The gateway asks before each create or cancel and
//! waits for the time it is told; nothing is dropped.
//!
//! The venue counts a request when it arrives, and the gateway cannot see
//! that time: it lies somewhere between the start of the request and its
//! answer. So a request counts from the moment its slot is taken until one
//! window after it finished (`finish`). Every arrival lies inside that span,
//! so no rolling window at the venue holds more requests than the limit.
//! A sliding window, not a token bucket: a bucket of capacity `n` refilled
//! at `n` per second lets `2n` requests into one second.

use crate::ids::Ts;

/// What a request is, for the budget.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RateClass {
    /// A new order. Only while fewer than `order_limit` requests count.
    Order,
    /// A cancel. Also uses the reserve above `order_limit`, up to
    /// `total_limit`: a cancel only lowers risk.
    Cancel,
}

/// A taken slot. Hand it back with `finish` (the request went out) or
/// `release` (nothing left the host).
#[derive(Debug, PartialEq, Eq)]
pub struct Slot(u64);

#[derive(Clone, Copy, Debug)]
struct Entry {
    id: u64,
    /// When the request finished; `None` while it is in flight.
    end: Option<Ts>,
}

#[derive(Clone, Debug)]
pub struct RateBudget {
    window_us: u64,
    order_limit: usize,
    total_limit: usize,
    next: u64,
    entries: Vec<Entry>,
}

/// How long to wait when only requests in flight fill the budget: their
/// end is not known yet.
pub const IN_FLIGHT_POLL_US: u64 = 10_000;

impl RateBudget {
    /// `order_limit` ≤ `total_limit`, both above zero.
    pub fn new(order_limit: usize, total_limit: usize, window_us: u64) -> RateBudget {
        assert!(0 < order_limit && order_limit <= total_limit && window_us > 0);
        RateBudget {
            window_us,
            order_limit,
            total_limit,
            next: 0,
            entries: Vec::with_capacity(total_limit),
        }
    }

    /// Bybit: 10 per second per UID for each of create, amend and cancel
    /// (F18 §4.2); 8 for orders and 2 more for cancels (Q23), counted
    /// together over WS and REST (T8 unknown).
    pub fn bybit() -> RateBudget {
        RateBudget::new(8, 10, 1_000_000)
    }

    /// Requests that count at `now`: in flight, or finished less than a
    /// window ago. A clock that went back keeps them counted.
    fn counted(&mut self, now: Ts) -> usize {
        let w = self.window_us;
        self.entries
            .retain(|e| e.end.is_none_or(|end| now < end.saturating_add(w)));
        self.entries.len()
    }

    /// Takes one slot at `now`, or returns the earliest time a slot of this
    /// class can be free (`now + IN_FLIGHT_POLL_US` when requests still in
    /// flight decide it).
    pub fn take(&mut self, class: RateClass, now: Ts) -> Result<Slot, Ts> {
        let limit = match class {
            RateClass::Order => self.order_limit,
            RateClass::Cancel => self.total_limit,
        };
        let n = self.counted(now);
        if n < limit {
            let id = self.next;
            self.next += 1;
            self.entries.push(Entry { id, end: None });
            return Ok(Slot(id));
        }
        // A slot frees when `n - limit + 1` requests have left the window.
        let mut ends: Vec<Ts> = self
            .entries
            .iter()
            .filter_map(|e| e.end)
            .map(|end| end.saturating_add(self.window_us))
            .collect();
        ends.sort_unstable();
        let k = n - limit;
        Err(ends
            .get(k)
            .copied()
            .unwrap_or_else(|| now.saturating_add(IN_FLIGHT_POLL_US)))
    }

    /// The request of `slot` finished (answered, failed or timed out) at
    /// `now`; it counts for one more window.
    pub fn finish(&mut self, slot: Slot, now: Ts) {
        if let Some(e) = self.entries.iter_mut().find(|e| e.id == slot.0) {
            e.end = Some(now);
        }
    }

    /// Nothing of the request left the host: the slot is free at once.
    pub fn release(&mut self, slot: Slot) {
        self.entries.retain(|e| e.id != slot.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: u64 = 1_000_000;

    /// Takes and finishes at once (zero latency).
    fn send(b: &mut RateBudget, class: RateClass, now: Ts) -> Result<(), Ts> {
        let slot = b.take(class, now)?;
        b.finish(slot, now);
        Ok(())
    }

    #[test]
    fn the_ninth_order_in_a_second_waits_while_two_cancels_still_go_out() {
        let mut b = RateBudget::bybit();
        for _ in 0..8 {
            assert_eq!(send(&mut b, RateClass::Order, 0), Ok(()));
        }
        assert_eq!(send(&mut b, RateClass::Order, 10), Err(S));
        assert_eq!(send(&mut b, RateClass::Cancel, 20), Ok(()));
        assert_eq!(send(&mut b, RateClass::Cancel, 30), Ok(()));
        assert_eq!(send(&mut b, RateClass::Cancel, 40), Err(S));
        assert_eq!(send(&mut b, RateClass::Order, S - 1), Err(S));
        // At 1 s the eight orders leave; the two cancels (20, 30 µs) still
        // count.
        assert_eq!(send(&mut b, RateClass::Order, S), Ok(()));
    }

    #[test]
    fn the_window_slides_with_each_request() {
        let mut b = RateBudget::bybit();
        for i in 0..8 {
            assert_eq!(send(&mut b, RateClass::Order, i * 100_000), Ok(()));
        }
        assert_eq!(send(&mut b, RateClass::Order, 850_000), Err(S));
        assert_eq!(send(&mut b, RateClass::Order, S), Ok(()));
        assert_eq!(send(&mut b, RateClass::Order, S + 1), Err(S + 100_000));
    }

    #[test]
    fn cancels_may_take_the_reserve_and_then_block_orders_too() {
        let mut b = RateBudget::bybit();
        for _ in 0..10 {
            assert_eq!(send(&mut b, RateClass::Cancel, 0), Ok(()));
        }
        assert_eq!(send(&mut b, RateClass::Cancel, 1), Err(S));
        assert_eq!(send(&mut b, RateClass::Order, 1), Err(S));
    }

    #[test]
    fn a_request_counts_until_one_window_after_it_finished() {
        let mut b = RateBudget::bybit();
        let slots: Vec<Slot> = (0..8).map(|_| b.take(RateClass::Order, 0).unwrap()).collect();
        // All eight in flight: no end is known, so the answer is a short poll.
        assert_eq!(b.take(RateClass::Order, 5 * S).map(|_| ()), Err(5 * S + IN_FLIGHT_POLL_US));
        let mut slots = slots.into_iter();
        b.finish(slots.next().unwrap(), 300_000);
        // One finished at 0.3 s: free at 1.3 s, not at 1 s.
        assert_eq!(b.take(RateClass::Order, S).map(|_| ()), Err(1_300_000));
        assert!(b.take(RateClass::Order, 1_300_000).is_ok());
        // A released slot is free at once.
        b.release(slots.next().unwrap());
        assert!(b.take(RateClass::Order, 1_300_001).is_ok());
    }

    #[test]
    fn a_clock_that_goes_back_does_not_free_a_slot() {
        let mut b = RateBudget::bybit();
        for _ in 0..8 {
            assert_eq!(send(&mut b, RateClass::Order, 5 * S), Ok(()));
        }
        assert_eq!(send(&mut b, RateClass::Order, 4 * S), Err(6 * S));
        assert_eq!(send(&mut b, RateClass::Cancel, 4 * S), Ok(()));
        assert_eq!(send(&mut b, RateClass::Cancel, 4 * S), Ok(()));
        // The two cancels sent at 4 s leave first.
        assert_eq!(send(&mut b, RateClass::Cancel, 4 * S), Err(5 * S));
    }

    /// Requests take a random time (up to 0.4 s) and arrive at the venue at
    /// a random moment between their start and their end. No rolling second
    /// of arrivals holds more than 8 orders or 10 requests, and a request is
    /// held only when its class is full.
    #[test]
    fn no_rolling_second_of_arrivals_holds_more_than_the_limits() {
        let mut seed: u64 = 0x9e37_79b9_7f4a_7c15;
        let mut next = || {
            seed = seed
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            seed >> 33
        };
        let mut b = RateBudget::bybit();
        let mut now = 0;
        // (start, end) of each request in flight, and arrivals.
        let mut flying: Vec<(Slot, Ts)> = Vec::new();
        let mut arrivals: Vec<(Ts, RateClass)> = Vec::new();
        let mut held = 0;
        for _ in 0..20_000 {
            now += next() % 120_000;
            // Finish what is due.
            let mut still = Vec::new();
            for (slot, end) in flying.drain(..) {
                if end <= now {
                    b.finish(slot, end);
                } else {
                    still.push((slot, end));
                }
            }
            flying = still;
            let class = if next() % 3 == 0 { RateClass::Cancel } else { RateClass::Order };
            match b.take(class, now) {
                Ok(slot) => {
                    let end = now + next() % 400_000;
                    let arrival = now + next() % (end - now + 1);
                    arrivals.push((arrival, class));
                    flying.push((slot, end));
                }
                Err(at) => {
                    held += 1;
                    assert!(at > now);
                }
            }
        }
        arrivals.sort_unstable_by_key(|a| a.0);
        for (i, (t, _)) in arrivals.iter().enumerate() {
            let window = arrivals[i..].iter().take_while(|(u, _)| *u < t + S);
            let (mut total, mut orders) = (0, 0);
            for (_, c) in window {
                total += 1;
                orders += usize::from(*c == RateClass::Order);
            }
            assert!(total <= 10 && orders <= 8, "{total} requests, {orders} orders from {t}");
        }
        assert!(held > 1_000 && arrivals.len() > 5_000, "held {held}, sent {}", arrivals.len());
    }
}
