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

/// How often the reconciler does each of its things.
#[derive(Clone, Copy, Debug)]
pub struct Pace {
    /// One round: read the published state and issue what is due.
    pub round: Duration,
    /// The sweep of fills, open orders and positions.
    pub sweep: Duration,
    /// The margin query (`runtime.margin_poll_ms`).
    pub margin_poll: Duration,
    /// The venue-setting query of one instrument, and the account-mode
    /// query while it has no answer (`runtime.venue_setting_poll_ms`).
    pub venue_setting_poll: Duration,
}

/// What one round reads from the published state.
struct Seen {
    needs: Vec<Need>,
    link_up: bool,
    session: u64,
    /// Fills are complete up to here: the sweep asks from here on.
    fills_since: Ts,
    /// A margin policy is configured.
    margin_rules: bool,
    /// Instruments whose venue setting is asked for: the core has not
    /// been told it in this link session, or the instrument has no
    /// position (a position states its setting in every margin report; an
    /// instrument without one has to be asked about). Empty without a
    /// margin policy.
    settings_to_ask: Vec<InstrumentId>,
    /// The core has not been told the account mode in this link session.
    /// False without a margin policy.
    mode_unknown: bool,
}

impl Seen {
    fn read(published: &Published) -> Seen {
        let p = published.read();
        let core = &p.core;
        let margin_rules = core.config.policy.margin.is_some();
        let setting_unknown = |i: &InstrumentId| !core.margin.venue_settings.contains_key(i);
        let flat = |i: &InstrumentId| core.account.get(i).is_none_or(|b| b.pos.0 == 0);
        let settings_to_ask = core
            .config
            .instruments
            .keys()
            .filter(|i| margin_rules && (setting_unknown(i) || flat(i)))
            .cloned()
            .collect();
        Seen {
            needs: core.needs(),
            link_up: core.link.up,
            session: core.link.session,
            fills_since: core.sync.fills_up_to.max(core.fill_floor),
            margin_rules,
            settings_to_ask,
            mode_unknown: margin_rules && core.margin.account_mode.is_none(),
        }
    }
}

pub async fn run(published: Arc<Published>, queries: mpsc::Sender<QueryReq>, pace: Pace) {
    let Pace {
        round,
        sweep,
        margin_poll,
        venue_setting_poll,
    } = pace;
    let mut session_seen: u64 = 0;
    let mut setting_asked: HashMap<InstrumentId, Instant> = HashMap::new();
    let mut mode_asked: Option<Instant> = None;
    let mut margin_seq: u64 = 0;
    let mut last_margin: Option<Instant> = None;
    let mut asked: HashMap<OrderId, (u32, Instant)> = HashMap::new();
    let mut last_sweep = Instant::now();
    let mut last_account: HashMap<&'static str, Instant> = HashMap::new();
    let mut tick = tokio::time::interval(round);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tick.tick().await;
        let Seen {
            needs,
            link_up,
            session,
            fills_since,
            margin_rules,
            settings_to_ask,
            mode_unknown,
        } = Seen::read(&published);
        if !link_up {
            asked.clear();
            continue;
        }
        let now = Instant::now();
        let mut wanted: Vec<QueryReq> = Vec::new();
        let mut still_needed: Vec<OrderId> = Vec::new();
        for need in needs {
            match need {
                Need::OrderStatus { order_id, venue_oid, accepted_at, .. } => {
                    still_needed.push(order_id);
                    let (attempts, last) = asked.get(&order_id).copied().unwrap_or((0, now));
                    if attempts == 0 || now.duration_since(last) >= backoff(attempts) {
                        asked.insert(order_id, (attempts + 1, now));
                        wanted.push(QueryReq::Status { order_id, venue_oid, accepted_at });
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
        // A venue setting is asked for at once when the link session
        // starts and its timer is new. Each instrument is asked about at
        // most once per `venue_setting_poll`, answered or not: the request
        // budget counts exactly that. The timer moves only when the
        // gateway took the query.
        for instrument in &settings_to_ask {
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
