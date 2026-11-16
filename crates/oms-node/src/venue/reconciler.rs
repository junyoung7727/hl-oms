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
//! process lives, so it rises within every link session. It asks for the
//! venue setting of an instrument and for the account mode when a link
//! session starts, again while the core does not know them, and again for
//! the setting of every instrument without a position, each at most once
//! per `venue_setting_poll_ms`. It never asks the venue to change either.

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

pub async fn run(
    published: Arc<Published>,
    queries: mpsc::Sender<QueryReq>,
    every: Duration,
    sweep: Duration,
    margin_poll: Duration,
    venue_setting_poll: Duration,
) {
    let mut session_seen: u64 = 0;
    let mut setting_asked: HashMap<InstrumentId, Instant> = HashMap::new();
    let mut mode_asked: Option<Instant> = None;
    let mut margin_seq: u64 = 0;
    let mut last_margin: Option<Instant> = None;
    let mut asked: HashMap<OrderId, (u32, Instant)> = HashMap::new();
    let mut last_sweep = Instant::now();
    let mut last_account: HashMap<&'static str, Instant> = HashMap::new();
    let mut tick = tokio::time::interval(every);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tick.tick().await;
        let (needs, link_up, session, fills_since, margin_rules, settings_unknown, mode_unknown, flat) = {
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
                p.core.link.session,
                p.core.sync.fills_up_to.max(p.core.fill_floor),
                margin_rules,
                settings_unknown,
                margin_rules && p.core.margin.account_mode.is_none(),
                // Configured instruments without a position. A position
                // states its setting in every margin report; an instrument
                // without one has to be asked about.
                p.core
                    .config
                    .instruments
                    .keys()
                    .filter(|i| p.core.account.get(*i).is_none_or(|b| b.pos.0 == 0))
                    .cloned()
                    .collect::<Vec<InstrumentId>>(),
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
        // The margin query is issued here and not with the rest: its timer
        // and its number move only when the gateway took it. A query that
        // found no room is offered again on the next round.
        if margin_rules && last_margin.is_none_or(|t| now.duration_since(t) >= margin_poll) {
            match queries.try_send(QueryReq::Margin { seq: margin_seq + 1 }) {
                Ok(()) => {
                    last_margin = Some(now);
                    margin_seq += 1;
                }
                Err(_) if queries.is_closed() => return,
                Err(_) => {}
            }
        }
        // Venue settings and the account mode are told per link session,
        // so their timers start again with each session.
        if session != session_seen {
            session_seen = session;
            setting_asked.clear();
            mode_asked = None;
        }
        // A venue setting is asked for when the core does not know it in
        // this link session (at once when the session starts), and when
        // the instrument has no position (a known setting can be changed
        // at the venue at any time, and no margin report states it). Each
        // instrument is asked about at most once per `venue_setting_poll`,
        // answered or not: the request budget counts exactly that. The
        // timer moves only when the gateway took the query.
        let asked_about = settings_unknown.iter().chain(flat.iter().filter(|i| !settings_unknown.contains(i)));
        for instrument in asked_about.filter(|_| margin_rules) {
            let waited = setting_asked.get(instrument).is_none_or(|t| now.duration_since(*t) >= venue_setting_poll);
            if !waited {
                continue;
            }
            let query = QueryReq::VenueSetting {
                instrument: instrument.clone(),
            };
            match queries.try_send(query) {
                Ok(()) => {
                    setting_asked.insert(instrument.clone(), now);
                }
                Err(_) if queries.is_closed() => return,
                Err(_) => {}
            }
        }
        // The account mode is read once per link session. While the core
        // has no answer it is asked for again at the same pace as a
        // setting, not faster.
        if mode_unknown && mode_asked.is_none_or(|t| now.duration_since(t) >= venue_setting_poll) {
            match queries.try_send(QueryReq::AccountMode) {
                Ok(()) => mode_asked = Some(now),
                Err(_) if queries.is_closed() => return,
                Err(_) => {}
            }
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
