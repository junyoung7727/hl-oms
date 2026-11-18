//! Feed settings and the connection number allocator.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::Duration;

/// Hands out `conn_id`s for one collector session, over all its markets.
/// `0` is reserved for REST bodies and notes (lake contract 2.2), so the first
/// connection gets `1`. A reconnect is a new connection and gets a new number.
#[derive(Debug)]
pub struct ConnIds {
    next: AtomicU32,
    rest_frame_no: AtomicU64,
}

impl ConnIds {
    pub fn new() -> Self {
        Self {
            next: AtomicU32::new(1),
            rest_frame_no: AtomicU64::new(0),
        }
    }

    /// The next `conn_id`, or `None` after 2^32 − 1 numbers.
    pub fn next(&self) -> Option<u32> {
        self.next
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
            .ok()
    }

    /// `frame_no` of the next REST body (`conn_id` 0), counted from 0 per
    /// session.
    pub(crate) fn next_rest_frame_no(&self) -> u64 {
        self.rest_frame_no.fetch_add(1, Ordering::Relaxed)
    }
}

impl Default for ConnIds {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub struct FeedConfig {
    /// The market name of `market_ref` (`linear`, `spot`, ...).
    pub market: String,
    pub ws_url: String,
    /// Base URL of REST requests (snapshots), for example `https://api.bybit.com`.
    pub rest_base: Option<String>,
    pub conn_ids: Arc<ConnIds>,
    /// A book is stale when no message of its topic was applied for this long.
    pub stale_after: Duration,
    /// Per-channel overrides of `stale_after`.
    pub stale_after_by_channel: BTreeMap<String, Duration>,
    /// Overrides the adapter's keepalive interval (tests).
    pub keepalive_interval: Option<Duration>,
    /// Nothing received for this long closes the connection. `None`: twice the
    /// keepalive interval plus 5 s (judgement, not a model).
    pub silence_timeout: Option<Duration>,
    pub connect_timeout: Duration,
    pub backoff_min: Duration,
    pub backoff_max: Duration,
    /// A resync that has not applied a snapshot within this time reconnects.
    pub resync_timeout: Duration,
    /// Deltas buffered per topic during a REST resync; more restarts the resync.
    pub max_buffered_deltas: usize,
    /// Refuse every address that is not loopback (all tests set it).
    pub loopback_only: bool,
    /// Capacity of the event channel; events beyond it are dropped and counted.
    pub event_capacity: usize,
}

impl FeedConfig {
    /// Defaults: stale after 5 s, connect timeout 10 s, backoff 500 ms to
    /// 30 s, resync timeout 5 s (judgement, not a model: Bybit's first
    /// snapshot came 223–676 ms after subscribing, design §6.3).
    pub fn new(market: impl Into<String>, ws_url: impl Into<String>, conn_ids: Arc<ConnIds>) -> Self {
        Self {
            market: market.into(),
            ws_url: ws_url.into(),
            rest_base: None,
            conn_ids,
            stale_after: Duration::from_secs(5),
            stale_after_by_channel: BTreeMap::new(),
            keepalive_interval: None,
            silence_timeout: None,
            connect_timeout: Duration::from_secs(10),
            backoff_min: Duration::from_millis(500),
            backoff_max: Duration::from_secs(30),
            resync_timeout: Duration::from_secs(5),
            max_buffered_deltas: 100_000,
            loopback_only: false,
            event_capacity: 65_536,
        }
    }

    pub(crate) fn stale_after(&self, channel: &str) -> Duration {
        self.stale_after_by_channel
            .get(channel)
            .copied()
            .unwrap_or(self.stale_after)
    }
}
