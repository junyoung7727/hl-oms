//! The monitoring gate's node side (C-LIVE part 2): the monitor socket's
//! rules, the watcher's heartbeat line turned into a core input, and the
//! gate's status.
//!
//! The gate itself is the core's: it judges the journaled heartbeats at
//! record time, holds plain orders while unmonitored and asks to cancel the
//! resting plain orders later (stage 2). This module decides nothing about
//! freshness. It checks the line's shape, converts milliseconds to
//! microseconds and paces each connection, so that a runaway watcher cannot
//! flood the journal.

use oms_core::*;
use std::collections::{BTreeMap, VecDeque};
use std::time::{Duration, Instant};

/// Connections the monitor socket takes at once, in a pool of its own.
/// 판단(모델 아님): one watcher and one spare for its restart.
pub const MONITOR_MAX_CONNECTIONS: usize = 2;

/// Heartbeats one connection may send per second; the rest are refused
/// with `overloaded` and recorded nowhere. 판단(모델 아님): the watcher
/// sends one per 5 s.
pub const HEARTBEATS_PER_SEC: usize = 2;

/// The heartbeats a connection sent in the last second (C-LIVE part 2).
/// Time is an argument.
#[derive(Debug, Default)]
pub struct HeartbeatPace {
    taken: VecDeque<Instant>,
}

impl HeartbeatPace {
    /// Whether one more heartbeat may go to the core at `now`: fewer than
    /// `HEARTBEATS_PER_SEC` taken within the second before it. A refused
    /// heartbeat is not counted.
    pub fn take(&mut self, now: Instant) -> bool {
        while self
            .taken
            .front()
            .is_some_and(|t| now.saturating_duration_since(*t) >= Duration::from_secs(1))
        {
            self.taken.pop_front();
        }
        if self.taken.len() >= HEARTBEATS_PER_SEC {
            return false;
        }
        self.taken.push_back(now);
        true
    }
}

/// The core input for a heartbeat line: check times from milliseconds to
/// microseconds. Refused, before the core sees it, for a shape the core
/// would refuse too (`heartbeat_shape_refusal`) and for a time that does
/// not fit in microseconds.
pub fn heartbeat_input(
    watcher: String,
    checks_ms: BTreeMap<String, Option<u64>>,
) -> Result<Input, String> {
    let mut checks = BTreeMap::new();
    for (name, ms) in checks_ms {
        let us = match ms {
            None => None,
            Some(ms) => Some(ms.checked_mul(1_000).ok_or_else(|| {
                format!("{name}: {ms} ms does not fit in microseconds; nothing was recorded")
            })?),
        };
        checks.insert(name, us);
    }
    if let Some(why) = heartbeat_shape_refusal(&watcher, &checks) {
        return Err(format!("{why}; nothing was recorded"));
    }
    Ok(Input::Watcher(WatcherInput::Heartbeat { watcher, checks }))
}

/// `status.core.monitor` (C-LIVE part 2), with `now` on the wall clock.
/// `gate: false` and nothing else without a gate. Ages are this process's
/// clock now minus the heartbeat's record time or the check's last success.
/// `cancel_stage`: `none` while monitored, `waiting` while unmonitored for
/// less than `unmonitored_cancel_us` (with `cancel_at`), `cancelling` after.
pub fn status_json(core: &Core, now: Ts) -> serde_json::Value {
    let (Some(policy), Some(m)) = (&core.config.policy.monitor, &core.monitor) else {
        return serde_json::json!({ "gate": false });
    };
    let age_ms = |t: Ts| now.saturating_sub(t) / 1_000;
    let last = m.last.as_ref();
    let checks: serde_json::Map<String, serde_json::Value> = last
        .map(|l| {
            l.checks
                .iter()
                .map(|(k, t)| (k.clone(), serde_json::json!(t.map(age_ms))))
                .collect()
        })
        .unwrap_or_default();
    let cancel_at = m.since.saturating_add(policy.unmonitored_cancel_us);
    let stage = if m.monitored {
        "none"
    } else if core.monitor_cancel_due(now) {
        "cancelling"
    } else {
        "waiting"
    };
    serde_json::json!({
        "gate": true,
        "monitored": m.monitored,
        "since": m.since,
        "reason": m.reason,
        "watcher": last.map(|l| l.watcher.clone()),
        "last_heartbeat_age_ms": last.map(|l| age_ms(l.at)),
        "checks": checks,
        "cancel_stage": stage,
        "cancel_at": (!m.monitored).then_some(cancel_at),
        "policy": policy,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two heartbeats per second per connection: a third within the second
    /// is refused and not counted; one second after the first, one more is
    /// taken.
    #[test]
    fn two_heartbeats_per_second_per_connection() {
        let mut p = HeartbeatPace::default();
        let t = Instant::now();
        assert!(p.take(t));
        assert!(p.take(t + Duration::from_millis(10)));
        assert!(!p.take(t + Duration::from_millis(20)));
        assert!(!p.take(t + Duration::from_millis(999)));
        assert!(p.take(t + Duration::from_millis(1_000)));
        assert!(!p.take(t + Duration::from_millis(1_009)));
        assert!(p.take(t + Duration::from_millis(1_010)));
        // A quiet second clears the window.
        assert!(p.take(t + Duration::from_millis(3_000)));
        assert!(p.take(t + Duration::from_millis(3_000)));
        assert!(!p.take(t + Duration::from_millis(3_000)));
    }

    /// Milliseconds become microseconds; `null` stays none; a time that
    /// overflows, a missing W2 and a bad label are refused before the core.
    #[test]
    fn a_heartbeat_line_becomes_a_core_input_in_microseconds() {
        let ms = |pairs: &[(&str, Option<u64>)]| -> BTreeMap<String, Option<u64>> {
            pairs.iter().map(|(k, v)| (k.to_string(), *v)).collect()
        };
        let input = heartbeat_input(
            "watcher-1".into(),
            ms(&[
                ("W1", Some(1_763_700_000_123)),
                ("W2", Some(5)),
                ("W4", None),
            ]),
        )
        .unwrap();
        let Input::Watcher(WatcherInput::Heartbeat { watcher, checks }) = input else {
            panic!("{input:?}")
        };
        assert_eq!(watcher, "watcher-1");
        assert_eq!(checks.get("W1"), Some(&Some(1_763_700_000_123_000)));
        assert_eq!(checks.get("W2"), Some(&Some(5_000)));
        assert_eq!(checks.get("W4"), Some(&None));
        let e = heartbeat_input("w".into(), ms(&[("W1", Some(u64::MAX)), ("W2", Some(1))]))
            .unwrap_err();
        assert!(e.contains("does not fit"), "{e}");
        let e = heartbeat_input("w".into(), ms(&[("W1", Some(1))])).unwrap_err();
        assert!(e.contains("W1 and W2"), "{e}");
        let e = heartbeat_input("bad\nlabel".into(), ms(&[("W1", Some(1)), ("W2", Some(1))]))
            .unwrap_err();
        assert!(e.contains("label"), "{e}");
        let e = heartbeat_input(
            "w".into(),
            ms(&[("W1", Some(1)), ("W2", Some(1)), ("W0", None)]),
        )
        .unwrap_err();
        assert!(e.contains("W1..W9"), "{e}");
    }
}
