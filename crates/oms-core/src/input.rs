//! Everything that can reach the core. An input is a request or an
//! observation; it becomes a fact only through the events the core decides.

use crate::config::CoreConfig;
use crate::ids::*;
use crate::model::*;
use crate::num::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Input {
    /// First input of every process incarnation, issued after replay.
    Boot { binary: String },
    /// A configuration to activate.
    Config(Box<CoreConfig>),
    NewOrder(NewOrderReq),
    Cancel(CancelReq),
    Operator(OperatorCmd),
    Venue(VenueInput),
    /// Time has passed. Carries nothing; the time is the record timestamp.
    Tick,
    /// Journal storage headroom, measured by the execution layer.
    Storage { low: bool, free_bytes: u64 },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewOrderReq {
    pub strategy: StrategyId,
    pub client_order_id: ClientOrderId,
    pub instrument: InstrumentId,
    pub side: Side,
    pub qty: Qty,
    pub px: Px,
    pub tif: Tif,
    pub reduce_only: bool,
    pub expires_at: Ts,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CancelReq {
    pub actor: Actor,
    pub target: CancelTarget,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CancelTarget {
    Order(OrderId),
    Client {
        strategy: StrategyId,
        client_order_id: ClientOrderId,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperatorCmd {
    Halt { scope: Scope, note: String },
    Resume { scope: Scope, note: String },
    AckMismatch { id: u32, note: String },
    /// Closes an order on the operator's stated evidence. Asserts that no
    /// fill beyond those already booked exists.
    ResolveOrder { order_id: OrderId, note: String },
    CancelAll { scope: Scope, note: String },
    /// Explicit, audited correction of a ledger position.
    AdjustPosition {
        instrument: InstrumentId,
        ledger: Ledger,
        delta: SQty,
        note: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VenueInput {
    Link {
        up: bool,
        /// Chosen by the gateway; must exceed every earlier session.
        session: u64,
        reason: String,
    },
    SendResult {
        order_id: OrderId,
        outcome: SendOutcome,
        /// When the gateway started and finished the attempt (local clock).
        /// Recorded for audit; the core does not use them.
        #[serde(default)]
        timing: GatewayTiming,
    },
    CancelResult {
        order_id: OrderId,
        attempt: u32,
        outcome: CancelOutcome,
        #[serde(default)]
        timing: GatewayTiming,
    },
    /// Status of one order, from the stream or from a query.
    OrderStatus(OrderStatusIn),
    /// A query by client id found nothing.
    StatusNotFound {
        session: u64,
        order_id: OrderId,
        /// Local time immediately before the query was sent. The venue
        /// evaluated the query no earlier than this.
        queried_at: Ts,
    },
    Fill(FillIn),
    /// The gateway has fetched every fill with venue time up to `up_to` and
    /// delivered them before this message.
    FillsSynced { session: u64, up_to: Ts },
    OpenOrders {
        session: u64,
        /// Local time immediately before the query was sent.
        queried_at: Ts,
        orders: Vec<VenueOpenOrder>,
    },
    Positions {
        session: u64,
        queried_at: Ts,
        positions: Vec<VenuePosition>,
    },
    RefPrice {
        instrument: InstrumentId,
        px: Px,
        venue_ts: Ts,
    },
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatewayTiming {
    pub started_at: Ts,
    pub finished_at: Ts,
    /// Whatever the gateway used to identify the attempt on the wire.
    #[serde(default)]
    pub wire_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SendOutcome {
    /// The venue answered and the order exists (or existed). `status` is what
    /// the answer said about it.
    Placed {
        venue_oid: VenueOrderId,
        status: StatusIn,
    },
    /// The venue answered with a rejection: the order was never placed.
    Rejected { reason: String },
    /// The gateway is certain no byte of the request left this host.
    NotSent { reason: String },
    /// The request may or may not have been processed.
    Unknown { reason: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VenueStatus {
    Open,
    Filled,
    Canceled { reason: String },
    Rejected { reason: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusIn {
    pub status: VenueStatus,
    /// Cumulative filled quantity, if the venue states it reliably.
    pub cum: Option<Qty>,
    pub venue_ts: Ts,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceSource {
    SendResult,
    Stream,
    Query,
    Fill,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrderStatusIn {
    pub session: u64,
    /// Set by the gateway when the venue's client id decodes to this instance.
    pub order_id: Option<OrderId>,
    pub venue_oid: Option<VenueOrderId>,
    pub status: StatusIn,
    pub source: EvidenceSource,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FillIn {
    pub key: FillKey,
    pub order_id: Option<OrderId>,
    pub venue_oid: Option<VenueOrderId>,
    pub instrument: InstrumentId,
    pub side: Side,
    pub qty: Qty,
    pub px: Px,
    /// Positive is a cost, negative a rebate.
    pub fee: Amount,
    pub venue_ts: Ts,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VenueOpenOrder {
    pub venue_oid: VenueOrderId,
    /// `Some` if the venue's client id decodes to this instance.
    pub order_id: Option<OrderId>,
    pub instrument: InstrumentId,
    pub side: Side,
    /// Quantity still open, if the venue states it reliably.
    pub remaining: Option<Qty>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VenuePosition {
    pub instrument: InstrumentId,
    pub qty: SQty,
}
