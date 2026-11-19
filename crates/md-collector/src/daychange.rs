//! The UTC day change (spec D7): every running slot switches to a new
//! overlapping connection (md-feed `switch`), one every `stagger_ms`, from
//! midnight + `start_offset_ms`, in config order (market, then group, then
//! `conn_index`). Times come from the injected wall clock (spec R7).

use std::sync::Arc;
use std::time::Duration;

use md_lake::time::{DAY_NS, day_start};
use serde_json::json;

use crate::collector::Inner;

/// The switch times of `n` slots after `midnight_ns`.
pub fn times(midnight_ns: i64, n: usize, start_offset_ms: u64, stagger_ms: u64) -> Vec<i64> {
    let ms = 1_000_000i64;
    let offset = i64::try_from(start_offset_ms).unwrap_or(i64::MAX / 4) * ms;
    let stagger = i64::try_from(stagger_ms).unwrap_or(i64::MAX / 4) * ms;
    (0..n)
        .map(|k| midnight_ns + offset + stagger * i64::try_from(k).unwrap_or(0))
        .collect()
}

async fn sleep_until(inner: &Inner, at_ns: i64) {
    let wait = at_ns - inner.clock.now_ns();
    if wait > 0 {
        tokio::time::sleep(Duration::from_nanos(u64::try_from(wait).unwrap_or(0))).await;
    }
}

pub(crate) async fn run(inner: Arc<Inner>) {
    let dc = inner.cfg.day_change;
    loop {
        let midnight = day_start(inner.clock.now_ns()) + DAY_NS;
        sleep_until(&inner, midnight).await;
        // The running slots, in order; a slot the disk floor stopped is skipped.
        let slots: Vec<(usize, u32)> = {
            let groups = inner.groups.lock().await;
            groups
                .iter()
                .enumerate()
                .filter(|(_, r)| r.handle.is_some())
                .flat_map(|(g, r)| r.group.plans.iter().map(move |p| (g, p.conn_index)))
                .collect()
        };
        for ((g, conn_index), at) in slots.iter().zip(times(
            midnight,
            slots.len(),
            dc.start_offset_ms,
            dc.stagger_ms,
        )) {
            sleep_until(&inner, at).await;
            let groups = inner.groups.lock().await;
            let Some(h) = groups.get(*g).and_then(|r| r.handle.as_ref()) else {
                continue;
            };
            if let Err(e) = h.switch(*conn_index) {
                inner.log(
                    "warn",
                    "day change switch not started",
                    json!({"conn_index": conn_index, "error": e}),
                );
            }
        }
    }
}
