//! The one clock every timer of the watcher reads (spec "Clock"): wall
//! time in ms since the epoch. The runtime uses [`SystemClock`]; tests use
//! [`ManualClock`], which moves only when the test moves it. Nothing in the
//! watcher sleeps on this clock: a wait is a step that returns the instant
//! it next wants to run. Elapsed times are `saturating_sub`, so a clock
//! that steps back holds the timers until it passes the old value again.

use std::sync::atomic::{AtomicU64, Ordering};

pub trait Clock: Send + Sync {
    fn now_ms(&self) -> u64;
}

/// `SystemTime` in ms (0 before the epoch).
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_ms(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }
}

/// A clock that only the test moves.
pub struct ManualClock(AtomicU64);

impl ManualClock {
    pub fn new(ms: u64) -> ManualClock {
        ManualClock(AtomicU64::new(ms))
    }

    pub fn set(&self, ms: u64) {
        self.0.store(ms, Ordering::SeqCst);
    }

    pub fn advance(&self, ms: u64) {
        self.0.fetch_add(ms, Ordering::SeqCst);
    }
}

impl Clock for ManualClock {
    fn now_ms(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }
}
