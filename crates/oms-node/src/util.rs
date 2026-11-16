//! Small shared pieces: logging, wall clock, latency histogram, fatal latch.

use oms_core::Ts;
use serde::Serialize;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

pub fn wall_us() -> Ts {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_micros() as Ts)
        .unwrap_or(0)
}

/// One JSON object per line on stderr. Values are never secrets: callers pass
/// identifiers, counts and error texts only.
pub fn log(level: &str, msg: &str, fields: serde_json::Value) {
    let line = serde_json::json!({
        "ts": wall_us(),
        "level": level,
        "msg": msg,
        "fields": fields,
    });
    eprintln!("{line}");
}

#[macro_export]
macro_rules! info {
    ($msg:expr $(, $k:literal : $v:expr)* $(,)?) => {
        $crate::util::log("info", $msg, serde_json::json!({ $($k: $v),* }))
    };
}

#[macro_export]
macro_rules! warn {
    ($msg:expr $(, $k:literal : $v:expr)* $(,)?) => {
        $crate::util::log("warn", $msg, serde_json::json!({ $($k: $v),* }))
    };
}

#[macro_export]
macro_rules! error {
    ($msg:expr $(, $k:literal : $v:expr)* $(,)?) => {
        $crate::util::log("error", $msg, serde_json::json!({ $($k: $v),* }))
    };
}

/// Latency histogram over microseconds: 8 linear sub-buckets per power of
/// two, so a reported quantile is at most 12.5 % above the true value. Used
/// for the stage timings shown on the dashboard; the load generator measures
/// end-to-end latency separately with exact values.
#[derive(Clone, Debug)]
pub struct Hist {
    buckets: Vec<u64>,
    count: u64,
    max: u64,
    sum: u64,
}

const SUB: u64 = 8;

impl Default for Hist {
    fn default() -> Self {
        Hist {
            buckets: vec![0; 64 * SUB as usize],
            count: 0,
            max: 0,
            sum: 0,
        }
    }
}

impl Hist {
    fn index(v: u64) -> usize {
        if v < SUB {
            return v as usize;
        }
        let top = 63 - v.leading_zeros() as u64; // floor(log2 v), >= 3
        let shift = top - 3;
        let sub = (v >> shift) - SUB; // 0..8
        ((top - 2) * SUB + sub) as usize
    }

    /// Upper bound of the values that fall in bucket `i`.
    fn upper(i: usize) -> u64 {
        let i = i as u64;
        if i < SUB {
            return i;
        }
        let top = i / SUB + 2;
        let sub = i % SUB;
        let shift = top - 3;
        ((SUB + sub + 1) << shift) - 1
    }

    pub fn record(&mut self, v: u64) {
        let i = Self::index(v).min(self.buckets.len() - 1);
        self.buckets[i] += 1;
        self.count += 1;
        self.sum += v;
        if v > self.max {
            self.max = v;
        }
    }

    pub fn quantile(&self, q: f64) -> u64 {
        if self.count == 0 {
            return 0;
        }
        let rank = ((self.count as f64) * q).ceil().max(1.0) as u64;
        let mut seen = 0;
        for (i, n) in self.buckets.iter().enumerate() {
            seen += n;
            if seen >= rank {
                return Self::upper(i).min(self.max);
            }
        }
        self.max
    }

    pub fn summary(&self) -> HistSummary {
        HistSummary {
            count: self.count,
            mean_us: self.sum.checked_div(self.count).unwrap_or(0),
            p50_us: self.quantile(0.50),
            p90_us: self.quantile(0.90),
            p99_us: self.quantile(0.99),
            p999_us: self.quantile(0.999),
            max_us: self.max,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct HistSummary {
    pub count: u64,
    pub mean_us: u64,
    pub p50_us: u64,
    pub p90_us: u64,
    pub p99_us: u64,
    pub p999_us: u64,
    pub max_us: u64,
}

/// Set once when the process can no longer guarantee its contracts. Everyone
/// who must stop watches it.
#[derive(Default)]
pub struct Fatal {
    tripped: AtomicBool,
    reason: Mutex<Option<String>>,
    notify: tokio::sync::Notify,
}

impl Fatal {
    pub fn trip(&self, reason: impl Into<String>) {
        let reason = reason.into();
        // The reason is stored before the flag becomes visible, so nobody can
        // observe "stopped" without being able to read why.
        let first = {
            let mut slot = self.reason.lock().unwrap_or_else(|e| e.into_inner());
            if slot.is_none() {
                *slot = Some(reason.clone());
                self.tripped.store(true, Ordering::SeqCst);
                true
            } else {
                false
            }
        };
        if first {
            log("error", "fatal", serde_json::json!({ "reason": reason }));
        }
        self.notify.notify_waiters();
        self.notify.notify_one();
    }

    pub fn is_tripped(&self) -> bool {
        self.tripped.load(Ordering::SeqCst)
    }

    pub fn reason(&self) -> Option<String> {
        self.reason.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub async fn wait(&self) {
        loop {
            if self.is_tripped() {
                return;
            }
            self.notify.notified().await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Whoever sees the flag can read the reason: the two are set together.
    #[test]
    fn a_tripped_flag_always_has_its_reason() {
        for _ in 0..200 {
            let f = std::sync::Arc::new(Fatal::default());
            let g = f.clone();
            let t = std::thread::spawn(move || g.trip("first"));
            while !f.is_tripped() {
                std::hint::spin_loop();
            }
            assert_eq!(f.reason().as_deref(), Some("first"));
            t.join().unwrap();
            f.trip("second");
            assert_eq!(f.reason().as_deref(), Some("first"), "the first reason stands");
        }
    }

    #[test]
    fn histogram_bounds_hold_for_every_value() {
        // The bucket's upper bound is never below the value and never more
        // than 12.5 % above it (exact below 8).
        for v in (0u64..5000).chain([65_535, 65_536, 1_000_000, 123_456_789]) {
            let up = Hist::upper(Hist::index(v));
            assert!(up >= v, "{v} -> {up}");
            assert!(up as f64 <= (v as f64) * 1.125 + 1.0, "{v} -> {up}");
        }
    }

    #[test]
    fn quantiles_of_a_known_distribution() {
        let mut h = Hist::default();
        for v in 1..=1000u64 {
            h.record(v);
        }
        let s = h.summary();
        assert_eq!(s.count, 1000);
        assert_eq!(s.max_us, 1000);
        assert_eq!(s.mean_us, 500);
        // True p50 = 500, p99 = 990; reported values are bucket upper bounds.
        assert!((500..=563).contains(&s.p50_us), "{}", s.p50_us);
        assert!((990..=1000).contains(&s.p99_us), "{}", s.p99_us);
    }
}
