//! The bounded input queues in front of the core.
//!
//! Three lanes, one consumer. A lane never grows beyond its capacity; what
//! happens when it is full depends on the lane:
//!
//! | lane    | carries                              | when full                          |
//! |---------|--------------------------------------|------------------------------------|
//! | venue   | fills, order status, gateway results | the producer waits (backpressure)  |
//! | control | cancels, operator commands, ticks    | the request is refused: overloaded |
//! | orders  | new orders                           | the request is refused: overloaded |
//!
//! Venue input is never dropped. A producer that waits stops reading from its
//! socket, so the pressure reaches the transport instead of memory.
//!
//! This is a mutex and a condition variable around three `VecDeque`s. At the
//! request rates the venue allows, contention on the mutex is not measurable
//! next to a journal sync; see docs/performance for the numbers.

use oms_core::{Input, Reply};
use std::collections::VecDeque;
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::oneshot;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lane {
    Venue = 0,
    Control = 1,
    Orders = 2,
}

pub struct CoreMsg {
    pub input: Input,
    /// Where the core's reply goes once the record is durable.
    pub reply: Option<oneshot::Sender<Reply>>,
    pub enqueued: Instant,
}

impl CoreMsg {
    pub fn new(input: Input) -> CoreMsg {
        CoreMsg {
            input,
            reply: None,
            enqueued: Instant::now(),
        }
    }

    pub fn with_reply(input: Input) -> (CoreMsg, oneshot::Receiver<Reply>) {
        let (tx, rx) = oneshot::channel();
        (
            CoreMsg {
                input,
                reply: Some(tx),
                enqueued: Instant::now(),
            },
            rx,
        )
    }
}

struct Inner {
    lanes: [VecDeque<CoreMsg>; 3],
    closed: bool,
    tick_queued: bool,
    refused: [u64; 3],
    accepted: [u64; 3],
    high_water: [usize; 3],
}

pub struct InputQueues {
    inner: Mutex<Inner>,
    ready: Condvar,
    caps: [usize; 3],
}

#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct QueueStats {
    pub depth: [usize; 3],
    pub capacity: [usize; 3],
    pub high_water: [usize; 3],
    pub accepted: [u64; 3],
    pub refused: [u64; 3],
}

impl InputQueues {
    pub fn new(venue: usize, control: usize, orders: usize) -> InputQueues {
        InputQueues {
            inner: Mutex::new(Inner {
                lanes: [
                    VecDeque::with_capacity(venue),
                    VecDeque::with_capacity(control),
                    VecDeque::with_capacity(orders),
                ],
                closed: false,
                tick_queued: false,
                refused: [0; 3],
                accepted: [0; 3],
                high_water: [0; 3],
            }),
            ready: Condvar::new(),
            caps: [venue, control, orders],
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        // A poisoned lock means a producer panicked while holding it; the
        // queue contents are still structurally valid.
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Enqueues without waiting. Returns the message if the lane is full or
    /// the queues are closed.
    // The error hands the message back to the caller by value; boxing it
    // would allocate on the overload path.
    #[allow(clippy::result_large_err)]
    pub fn try_push(&self, lane: Lane, msg: CoreMsg) -> Result<(), CoreMsg> {
        let i = lane as usize;
        let mut g = self.lock();
        if g.closed || g.lanes[i].len() >= self.caps[i] {
            g.refused[i] += 1;
            return Err(msg);
        }
        g.lanes[i].push_back(msg);
        g.accepted[i] += 1;
        let depth = g.lanes[i].len();
        if depth > g.high_water[i] {
            g.high_water[i] = depth;
        }
        drop(g);
        self.ready.notify_one();
        Ok(())
    }

    /// Enqueues a tick unless one is already waiting. Ticks carry no data, so
    /// a second one adds nothing.
    pub fn push_tick(&self) {
        let mut g = self.lock();
        if g.closed || g.tick_queued || g.lanes[Lane::Control as usize].len() >= self.caps[Lane::Control as usize] {
            return;
        }
        g.tick_queued = true;
        g.lanes[Lane::Control as usize].push_back(CoreMsg::new(Input::Tick));
        drop(g);
        self.ready.notify_one();
    }

    /// Enqueues venue input, waiting while the lane is full. Returns false
    /// only if the queues were closed.
    pub async fn push_venue(&self, mut msg: CoreMsg) -> bool {
        loop {
            match self.try_push(Lane::Venue, msg) {
                Ok(()) => return true,
                Err(back) => {
                    if self.is_closed() {
                        return false;
                    }
                    msg = back;
                    tokio::time::sleep(Duration::from_millis(1)).await;
                }
            }
        }
    }

    pub fn is_closed(&self) -> bool {
        self.lock().closed
    }

    /// Stops accepting input. Messages already queued are still delivered.
    pub fn close(&self) {
        self.lock().closed = true;
        self.ready.notify_all();
    }

    /// Blocks until at least one message is available, then moves up to
    /// `max` messages into `out`. Returns false once closed and empty.
    ///
    /// Each lane gets a share of the batch first (venue half, control and
    /// orders a quarter each), then the rest is filled in priority order.
    /// So fills are favoured but a stream of them cannot starve cancels, and
    /// neither can starve new orders.
    pub fn pop_batch(&self, max: usize, out: &mut Vec<CoreMsg>) -> bool {
        let mut g = self.lock();
        loop {
            if g.lanes.iter().any(|l| !l.is_empty()) {
                break;
            }
            if g.closed {
                return false;
            }
            g = self.ready.wait(g).unwrap_or_else(|e| e.into_inner());
        }
        let shares = [max.div_ceil(2), max.div_ceil(4), max.div_ceil(4)];
        for (i, share) in shares.iter().enumerate() {
            for _ in 0..*share {
                if out.len() >= max {
                    break;
                }
                match g.lanes[i].pop_front() {
                    Some(m) => out.push(m),
                    None => break,
                }
            }
        }
        for i in 0..3 {
            while out.len() < max {
                match g.lanes[i].pop_front() {
                    Some(m) => out.push(m),
                    None => break,
                }
            }
        }
        if out.iter().any(|m| matches!(m.input, Input::Tick)) {
            g.tick_queued = false;
        }
        true
    }

    pub fn stats(&self) -> QueueStats {
        let g = self.lock();
        QueueStats {
            depth: [g.lanes[0].len(), g.lanes[1].len(), g.lanes[2].len()],
            capacity: self.caps,
            high_water: g.high_water,
            accepted: g.accepted,
            refused: g.refused,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tick() -> CoreMsg {
        CoreMsg::new(Input::Tick)
    }

    #[test]
    fn a_full_lane_refuses_and_never_grows() {
        let q = InputQueues::new(4, 2, 2);
        assert!(q.try_push(Lane::Orders, tick()).is_ok());
        assert!(q.try_push(Lane::Orders, tick()).is_ok());
        assert!(q.try_push(Lane::Orders, tick()).is_err());
        // The other lanes are unaffected.
        assert!(q.try_push(Lane::Control, tick()).is_ok());
        let s = q.stats();
        assert_eq!(s.depth, [0, 1, 2]);
        assert_eq!(s.refused, [0, 0, 1]);
    }

    #[test]
    fn every_lane_gets_a_share_of_each_batch() {
        let q = InputQueues::new(100, 100, 100);
        for _ in 0..50 {
            q.try_push(Lane::Venue, CoreMsg::new(Input::Storage { low: false, free_bytes: 0 }))
                .ok();
            q.try_push(Lane::Control, CoreMsg::new(Input::Boot { binary: String::new(), kill_marker: None }))
                .ok();
            q.try_push(Lane::Orders, tick()).ok();
        }
        let mut out = Vec::new();
        assert!(q.pop_batch(8, &mut out));
        let count = |f: fn(&Input) -> bool| out.iter().filter(|m| f(&m.input)).count();
        // Shares of 4, 2, 2 out of 8.
        assert_eq!(count(|i| matches!(i, Input::Storage { .. })), 4);
        assert_eq!(count(|i| matches!(i, Input::Boot { .. })), 2);
        assert_eq!(count(|i| matches!(i, Input::Tick)), 2);
    }

    #[test]
    fn ticks_coalesce() {
        let q = InputQueues::new(4, 4, 4);
        q.push_tick();
        q.push_tick();
        q.push_tick();
        assert_eq!(q.stats().depth[1], 1);
        let mut out = Vec::new();
        q.pop_batch(8, &mut out);
        q.push_tick();
        assert_eq!(q.stats().depth[1], 1);
    }

    #[test]
    fn close_drains_then_ends() {
        let q = InputQueues::new(4, 4, 4);
        q.try_push(Lane::Venue, tick()).ok();
        q.close();
        assert!(q.try_push(Lane::Venue, tick()).is_err());
        let mut out = Vec::new();
        assert!(q.pop_batch(8, &mut out));
        assert_eq!(out.len(), 1);
        out.clear();
        assert!(!q.pop_batch(8, &mut out));
    }
}
