//! The one clock every timer of the watcher reads (spec "Clock"): wall
//! time in ms since the epoch. The runtime uses [`SystemClock`]; tests use
//! [`ManualClock`], which moves only when the test moves it. Nothing in the
//! watcher sleeps on this clock: a wait is a step that returns the instant
//! it next wants to run. Elapsed times are `saturating_sub`, so a clock
//! that steps back holds the timers until it passes the old value again;
//! the kill path reads [`Steady`] instead, which never steps back (U19b3a).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

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

/// A clock that does not step back (C-WATCH part 12, review 4 S3): the
/// inner clock plus an offset; when the inner clock reads lower than the
/// last value returned, the offset rises so the value stays at that last
/// value. A step back becomes a pause of zero length, a step forward
/// passes through (W5's gap rule handles it). The offset and the last
/// value sit in one `Mutex`, so every task that reads one `Steady` sees
/// one timeline. W5 and the kill path read it, and so does the client's
/// request guard (its own `Steady`, U19b3c); signing, texts, the engine
/// and the sender read the raw clock.
pub struct Steady {
    inner: Arc<dyn Clock>,
    /// `(offset, last value returned)`.
    state: Mutex<(u64, u64)>,
}

impl Steady {
    pub fn new(inner: Arc<dyn Clock>) -> Steady {
        Steady {
            inner,
            state: Mutex::new((0, 0)),
        }
    }
}

impl Clock for Steady {
    fn now_ms(&self) -> u64 {
        let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let (offset, last) = *s;
        let raw = self.inner.now_ms().saturating_add(offset);
        let now = if raw < last {
            s.0 = offset + (last - raw);
            last
        } else {
            raw
        };
        s.1 = now;
        now
    }
}
