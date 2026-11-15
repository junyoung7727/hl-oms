//! Venue gateways.
//!
//! A gateway is a set of tasks connected to the rest of the process by three
//! channels. There is no gateway trait: the two implementations (the
//! simulated venue and Hyperliquid) are chosen once at startup, and what they
//! must honour is this message contract, documented in
//! docs/contracts/gateway.md:
//!
//! - For every `SendOrder` it receives, a gateway delivers exactly one
//!   `SendResult`, and it classifies the outcome conservatively: `NotSent`
//!   only if no byte of the request can have left the host, `Rejected` only
//!   on the venue's explicit refusal, `Unknown` for everything else.
//! - It never repeats an order request on its own.
//! - For every `SendCancel` it delivers exactly one `CancelResult`.
//! - It reports `Link` up with a session number larger than any before, and
//!   `Link` down when the event stream is lost.
//! - A query is answered with ordinary venue inputs, or not at all. A
//!   `FillsSynced { up_to }` is sent only after every fill with venue time
//!   up to `up_to` has been delivered.
//! - Venue input is never dropped: a full input queue makes the gateway wait.

pub mod hyperliquid;
pub mod reconciler;
pub mod simnet;
pub mod simproto;

use crate::queues::{CoreMsg, InputQueues};
use oms_core::*;
use std::sync::Arc;
use tokio::sync::mpsc;

#[derive(Clone, Debug)]
pub enum QueryReq {
    Status {
        order_id: OrderId,
        venue_oid: Option<VenueOrderId>,
    },
    Fills {
        since: Ts,
    },
    OpenOrders,
    Positions,
}

/// The gateway's side of the channels.
pub struct GatewayPorts {
    pub commands: mpsc::Receiver<VenueCommand>,
    pub queries: mpsc::Receiver<QueryReq>,
    pub input: VenueInputTx,
    pub instance_id: u64,
    pub boot: u32,
}

#[derive(Clone)]
pub struct VenueInputTx {
    queues: Arc<InputQueues>,
}

impl VenueInputTx {
    pub fn new(queues: Arc<InputQueues>) -> VenueInputTx {
        VenueInputTx { queues }
    }

    /// Delivers one venue input to the core, waiting if its queue is full.
    /// Returns false if the process is shutting down.
    pub async fn send(&self, v: VenueInput) -> bool {
        self.queues.push_venue(CoreMsg::new(Input::Venue(v))).await
    }
}

/// Session numbers must grow across restarts as well as reconnects, so the
/// boot number forms the high part.
pub fn session_number(boot: u32, connection: u32) -> u64 {
    (u64::from(boot) << 32) | u64::from(connection)
}
