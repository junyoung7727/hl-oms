//! What the state is waiting to learn from the venue.
//!
//! The core never performs a query. It only states, as a pure function of its
//! state, which evidence would let it make progress. The reconciler in the
//! execution layer reads this from the published (durable) state and fetches
//! the evidence; every answer comes back as an ordinary, idempotent input. A
//! query may therefore be repeated any number of times, before or after a
//! restart, without changing the outcome.

use crate::core::Core;
use crate::ids::*;
use crate::model::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Need {
    /// Status of one order, by client id (and venue id if known).
    OrderStatus {
        order_id: OrderId,
        venue_oid: Option<VenueOrderId>,
        /// When the core accepted the order (record time). The venue cannot
        /// have created it earlier; a gateway whose history is searched by
        /// time windows uses it (Bybit, ADR-028 §7b).
        accepted_at: Ts,
        why: NeedReason,
    },
    /// Fills since the given venue time.
    Fills { since: Ts },
    OpenOrders,
    Positions,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NeedReason {
    InDoubt,
    StaleEvidence,
    AwaitingCancelResult,
    MissingFromOpenOrders,
}

impl Core {
    pub fn needs(&self) -> Vec<Need> {
        let mut out = Vec::new();
        if !self.link.up {
            return out;
        }
        let session = self.link.session;
        let mut want_fills = self.sync.fills_session != session;
        for id in &self.open {
            let Some(o) = self.orders.get(id) else { continue };
            let why = match o.phase {
                Phase::PendingSend if o.doubt.is_some() => Some(NeedReason::InDoubt),
                Phase::Working if o.evidence_session != session => Some(NeedReason::StaleEvidence),
                Phase::Working if o.recheck => Some(NeedReason::MissingFromOpenOrders),
                Phase::Working
                    if o.cancel
                        .as_ref()
                        .is_some_and(|c| !c.inflight && (c.confirmed || c.last_outcome.is_some())) =>
                {
                    Some(NeedReason::AwaitingCancelResult)
                }
                _ => None,
            };
            if let Some(why) = why {
                out.push(Need::OrderStatus {
                    order_id: *id,
                    venue_oid: o.venue_oid.clone(),
                    accepted_at: o.accepted_at,
                    why,
                });
            }
            if o.reported_cum > o.cum_qty {
                want_fills = true;
            }
            if let Some(t) = &o.terminal {
                if self.sync.fills_up_to < t.venue_ts || t.final_cum.is_some_and(|f| f > o.cum_qty) {
                    want_fills = true;
                }
            }
        }
        if want_fills {
            out.push(Need::Fills {
                since: self.sync.fills_up_to.max(self.fill_floor),
            });
        }
        if self.sync.open_orders_session != session {
            out.push(Need::OpenOrders);
        }
        if self.sync.positions_session != session {
            out.push(Need::Positions);
        }
        out
    }
}
