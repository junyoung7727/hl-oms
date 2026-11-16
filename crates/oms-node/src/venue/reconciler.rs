//! The reconciler: fetches the venue evidence the core says it needs.
//!
//! It reads `Core::needs` from the published (durable) state and turns each
//! need into a query for the gateway. It holds no domain state and decides
//! nothing: every answer goes to the core as an ordinary input, and the core
//! applies it idempotently. So a query can be issued twice, be lost, or be
//! answered after a restart without any effect other than a repeated fetch.
//!
//! Besides the needs it also sweeps open orders, positions and fills on a
//! timer, which is how activity the core does not expect is noticed.
//!
//! With a margin policy it also asks for a margin report on its own timer
//! (C-MARGIN). Each such query gets a number that only rises while this
//! process lives, so it rises within every link session.

use super::QueryReq;
use crate::published::Published;
use oms_core::*;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

/// Per-order pacing of status queries: 0.5 s, 1 s, 2 s, then every 5 s.
fn backoff(attempts: u32) -> Duration {
    match attempts {
        0 => Duration::ZERO,
        1 => Duration::from_millis(500),
        2 => Duration::from_secs(1),
        3 => Duration::from_secs(2),
        _ => Duration::from_secs(5),
    }
}

pub async fn run(published: Arc<Published>, queries: mpsc::Sender<QueryReq>, every: Duration, sweep: Duration, margin_poll: Duration) {
    let mut margin_seq: u64 = 0;
    let mut last_margin: Option<Instant> = None;
    let mut asked: HashMap<OrderId, (u32, Instant)> = HashMap::new();
    let mut last_sweep = Instant::now();
    let mut last_account: HashMap<&'static str, Instant> = HashMap::new();
    let mut tick = tokio::time::interval(every);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tick.tick().await;
        let (needs, link_up, fills_since, margin_rules, settings_unknown, mode_unknown) = {
            let p = published.read();
            let margin_rules = p.core.config.policy.margin.is_some();
            // Instruments whose venue setting the core has not been told in
            // this link session.
            let settings_unknown: Vec<InstrumentId> = if margin_rules {
                p.core
                    .config
                    .instruments
                    .keys()
                    .filter(|i| !p.core.margin.venue_settings.contains_key(*i))
                    .cloned()
                    .collect()
            } else {
                Vec::new()
            };
            (
                p.core.needs(),
                p.core.link.up,
                p.core.sync.fills_up_to.max(p.core.fill_floor),
                margin_rules,
                settings_unknown,
                margin_rules && p.core.margin.account_mode.is_none(),
            )
        };
        if !link_up {
            asked.clear();
            continue;
        }
        let now = Instant::now();
        let mut wanted: Vec<QueryReq> = Vec::new();
        let mut still_needed: Vec<OrderId> = Vec::new();
        for need in needs {
            match need {
                Need::OrderStatus { order_id, venue_oid, .. } => {
                    still_needed.push(order_id);
                    let (attempts, last) = asked.get(&order_id).copied().unwrap_or((0, now));
                    if attempts == 0 || now.duration_since(last) >= backoff(attempts) {
                        asked.insert(order_id, (attempts + 1, now));
                        wanted.push(QueryReq::Status { order_id, venue_oid });
                    }
                }
                // Account-level queries are paced at one per second each.
                Need::Fills { since } => {
                    if due(&mut last_account, "fills", now) {
                        wanted.push(QueryReq::Fills { since });
                    }
                }
                Need::OpenOrders => {
                    if due(&mut last_account, "open_orders", now) {
                        wanted.push(QueryReq::OpenOrders);
                    }
                }
                Need::Positions => {
                    if due(&mut last_account, "positions", now) {
                        wanted.push(QueryReq::Positions);
                    }
                }
            }
        }
        asked.retain(|id, _| still_needed.contains(id));
        if now.duration_since(last_sweep) >= sweep {
            last_sweep = now;
            wanted.push(QueryReq::Fills { since: fills_since });
            wanted.push(QueryReq::OpenOrders);
            wanted.push(QueryReq::Positions);
        }
        if margin_rules && last_margin.is_none_or(|t| now.duration_since(t) >= margin_poll) {
            last_margin = Some(now);
            margin_seq += 1;
            wanted.push(QueryReq::Margin { seq: margin_seq });
        }
        // A venue setting that is not known yet is asked for at once and
        // then once per second until the answer is recorded.
        if !settings_unknown.is_empty() && due(&mut last_account, "venue_settings", now) {
            for instrument in settings_unknown {
                wanted.push(QueryReq::VenueSetting { instrument });
            }
        }
        // The account mode is asked for once per link session: at link-up,
        // and again each second until the answer is recorded.
        if mode_unknown && due(&mut last_account, "account_mode", now) {
            wanted.push(QueryReq::AccountMode);
        }
        for q in wanted {
            // If the gateway is busy the query is simply not issued this
            // round; the need is still there on the next one.
            if queries.try_send(q).is_err() && queries.is_closed() {
                return;
            }
        }
    }
}

fn due(last: &mut HashMap<&'static str, Instant>, key: &'static str, now: Instant) -> bool {
    match last.get(key) {
        Some(t) if now.duration_since(*t) < Duration::from_secs(1) => false,
        _ => {
            last.insert(key, now);
            true
        }
    }
}
