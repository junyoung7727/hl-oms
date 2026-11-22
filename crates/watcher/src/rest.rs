//! The watcher's own Bybit v5 REST client (C-WATCH part 3).

use std::collections::VecDeque;

/// At most this many requests in any [`GUARD_WINDOW_MS`], all Bybit paths
/// and the IP echo together (판단(모델 아님)).
pub const GUARD_MAX: usize = 60;
pub const GUARD_WINDOW_MS: u64 = 10_000;

/// The request guard: a sliding window on the injected clock.
pub struct Guard {
    max: usize,
    window_ms: u64,
    sent: VecDeque<u64>,
}

impl Default for Guard {
    fn default() -> Guard {
        Guard::new(GUARD_MAX, GUARD_WINDOW_MS)
    }
}

impl Guard {
    pub fn new(max: usize, window_ms: u64) -> Guard {
        Guard {
            max,
            window_ms,
            sent: VecDeque::new(),
        }
    }

    /// Records a request at `now_ms` and lets it go, or returns the
    /// earliest instant (ms) at which it may go; a refused request is not
    /// recorded. A request leaves the window `window_ms` after it went.
    pub fn admit(&mut self, now_ms: u64) -> Result<(), u64> {
        while let Some(&t) = self.sent.front() {
            if now_ms.saturating_sub(t) >= self.window_ms {
                self.sent.pop_front();
            } else {
                break;
            }
        }
        match self.sent.front() {
            Some(&first) if self.sent.len() >= self.max => Err(first + self.window_ms),
            _ => {
                self.sent.push_back(now_ms);
                Ok(())
            }
        }
    }
}
