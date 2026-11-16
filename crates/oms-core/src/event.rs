//! State transitions. An event is a decided fact: applying it never fails for
//! a reason that depends on configuration, limits or time, and never causes
//! an external effect. Recovery applies events and nothing else.
//!
//! The journal stores each input together with the events it produced, as one
//! record. See ADR-003.

use crate::config::CoreConfig;
use crate::ids::*;
use crate::input::*;
use crate::model::*;
use crate::num::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Event {
    ConfigActivated {
        config: Box<CoreConfig>,
    },
    /// A new process incarnation began. The link is down until the gateway
    /// reports otherwise.
    Booted {
        boot: u32,
        binary: String,
        /// Venue time before which fills are not this instance's concern. Set
        /// on the very first boot; later boots repeat the current value.
        fill_floor: Ts,
    },
    LinkChanged {
        up: bool,
        session: u64,
        reason: String,
    },
    /// A new-order request was refused. No order exists and the idempotency
    /// key is not consumed.
    RequestRejected {
        request: NewOrderReq,
        reasons: Vec<RejectReason>,
    },
    /// The order exists, its exposure is reserved and its send is released to
    /// the gateway. In live processing this event is followed by exactly one
    /// `SendOrder` command, emitted only after the record is durable.
    OrderAccepted {
        order_id: OrderId,
        request: NewOrderReq,
        config_version: u32,
        ref_px: Option<Px>,
    },
    /// Evidence that the venue has the order.
    OrderAcked {
        order_id: OrderId,
        venue_oid: Option<VenueOrderId>,
        via: EvidenceSource,
        /// The link session this evidence belongs to, or 0 if it was not
        /// gathered in the current one. Evidence that is true but old must
        /// not satisfy the re-confirmation that follows a reconnect.
        #[serde(default)]
        session: u64,
    },
    /// The venue order id became known for an order already acknowledged.
    VenueOidLearned {
        order_id: OrderId,
        venue_oid: VenueOrderId,
    },
    OrderInDoubt {
        order_id: OrderId,
        reason: DoubtReason,
        detail: String,
    },
    /// Status evidence refreshed: session and largest reported cumulative
    /// quantity.
    StatusNoted {
        order_id: OrderId,
        session: u64,
        reported_cum: Qty,
        venue_ts: Ts,
    },
    /// An open-order sweep did not list an order believed to be working. Its
    /// status must be asked for; nothing else changes.
    OrderRecheck {
        order_id: OrderId,
    },
    FillBooked {
        order_id: OrderId,
        key: FillKey,
        qty: Qty,
        px: Px,
        fee: Amount,
        venue_ts: Ts,
    },
    FillParked {
        fill: FillIn,
    },
    /// A parked fill left the waiting list, either to be booked against an
    /// identified order or as external.
    FillUnparked {
        key: FillKey,
    },
    /// A fill that belongs to no order of this instance. Booked to the
    /// account and to the unattributed ledger.
    ExternalFillBooked {
        fill: FillIn,
    },
    /// The venue reported a terminal status. The order stops being working;
    /// its reservation shrinks to what the final cumulative quantity leaves
    /// unbooked.
    VenueTerminal {
        order_id: OrderId,
        kind: VenueFinal,
        reason: String,
        final_cum: Option<Qty>,
        venue_ts: Ts,
    },
    MissingFillsFlagged {
        order_id: OrderId,
    },
    OrderClosed {
        order_id: OrderId,
        reason: CloseReason,
    },
    CancelRequested {
        order_id: OrderId,
        by: Actor,
    },
    /// A cancel attempt is released to the gateway. Followed in live
    /// processing by exactly one `SendCancel` command.
    CancelSent {
        order_id: OrderId,
        attempt: u32,
    },
    CancelOutcomeNoted {
        order_id: OrderId,
        attempt: u32,
        outcome: CancelOutcome,
    },
    FillsSynced {
        session: u64,
        up_to: Ts,
    },
    OpenOrdersSwept {
        session: u64,
    },
    PositionsVerified {
        session: u64,
    },
    MismatchRaised {
        id: u32,
        kind: MismatchKind,
        subject: String,
        detail: String,
    },
    MismatchAcked {
        id: u32,
        note: String,
    },
    OperatorHalted {
        scope: Scope,
        note: String,
    },
    OperatorResumed {
        scope: Scope,
        note: String,
    },
    StorageLowChanged {
        low: bool,
        free_bytes: u64,
    },
    RefPriceUpdated {
        instrument: InstrumentId,
        px: Px,
        venue_ts: Ts,
    },
    PositionAdjusted {
        instrument: InstrumentId,
        ledger: Ledger,
        delta: SQty,
        note: String,
    },
    /// Closed orders removed from memory after their retention and
    /// idempotency windows passed. External-fill keys and acknowledged
    /// mismatches older than `cutoff` are removed with them.
    Purged {
        order_ids: Vec<OrderId>,
        cutoff: Ts,
        /// Fill keys with a venue time below this are forgotten; a fill older
        /// than this is treated as a re-delivery from now on.
        fill_floor: Ts,
    },
    /// A margin report of the current link session was taken as the newest
    /// one (C-MARGIN).
    MarginReported {
        report: MarginReportIn,
    },
}

/// Stable reason codes for a refused new-order request.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RejectReason {
    UnknownStrategy,
    StrategyDisabled,
    UnknownInstrument,
    InstrumentNotAllowed,
    QtyNotPositive,
    QtyStep,
    PxNotPositive,
    PxTick,
    PxSigFigs,
    BelowMinNotional,
    MaxOrderQty,
    MaxOrderNotional,
    RefPriceUnavailable,
    PriceBand,
    AccountPositionLimit,
    StrategyPositionLimit,
    MaxOpenOrdersAccount,
    MaxOpenOrdersStrategy,
    GatewayBusy,
    CapacityExhausted,
    Expired,
    ExpiryTooFar,
    Overflow,
    Hold(HoldReason),
}

/// What the gateway must do. Commands are produced only by live processing,
/// never by replay, and are handed to the gateway only after the record that
/// contains their event is durable.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VenueCommand {
    SendOrder {
        order_id: OrderId,
        instance_id: u64,
        instrument: InstrumentId,
        side: Side,
        qty: Qty,
        px: Px,
        tif: Tif,
        reduce_only: bool,
        expires_at: Ts,
    },
    SendCancel {
        order_id: OrderId,
        instance_id: u64,
        attempt: u32,
        instrument: InstrumentId,
        venue_oid: Option<VenueOrderId>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reply {
    NewOrder(NewOrderReply),
    Cancel(CancelReply),
    Operator(Result<String, String>),
    Config(Result<u32, Vec<String>>),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NewOrderReply {
    Accepted {
        order_id: OrderId,
    },
    Rejected {
        reasons: Vec<RejectReason>,
    },
    /// The key was already used by an identical request. Nothing new happened.
    Duplicate {
        order_id: OrderId,
        phase: Phase,
        cum_qty: Qty,
    },
    /// The key was already used by a different request. Nothing new happened.
    Conflict {
        order_id: OrderId,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CancelReply {
    Requested { order_id: OrderId },
    AlreadyRequested { order_id: OrderId },
    AlreadyClosed { order_id: OrderId },
    NotFound,
    NotOwner,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Output {
    pub events: Vec<Event>,
    pub commands: Vec<VenueCommand>,
    pub reply: Option<Reply>,
}

impl Event {
    /// The order this event is about, if it is about exactly one.
    pub fn order_id(&self) -> Option<OrderId> {
        match self {
            Event::OrderAccepted { order_id, .. }
            | Event::OrderAcked { order_id, .. }
            | Event::VenueOidLearned { order_id, .. }
            | Event::OrderInDoubt { order_id, .. }
            | Event::StatusNoted { order_id, .. }
            | Event::OrderRecheck { order_id }
            | Event::FillBooked { order_id, .. }
            | Event::VenueTerminal { order_id, .. }
            | Event::MissingFillsFlagged { order_id }
            | Event::OrderClosed { order_id, .. }
            | Event::CancelRequested { order_id, .. }
            | Event::CancelSent { order_id, .. }
            | Event::CancelOutcomeNoted { order_id, .. } => Some(*order_id),
            _ => None,
        }
    }

    /// Short stable name, used by tools and the dashboard to link an event to
    /// its contract.
    pub fn name(&self) -> &'static str {
        match self {
            Event::ConfigActivated { .. } => "config_activated",
            Event::Booted { .. } => "booted",
            Event::LinkChanged { .. } => "link_changed",
            Event::RequestRejected { .. } => "request_rejected",
            Event::OrderAccepted { .. } => "order_accepted",
            Event::OrderAcked { .. } => "order_acked",
            Event::VenueOidLearned { .. } => "venue_oid_learned",
            Event::OrderInDoubt { .. } => "order_in_doubt",
            Event::StatusNoted { .. } => "status_noted",
            Event::OrderRecheck { .. } => "order_recheck",
            Event::FillBooked { .. } => "fill_booked",
            Event::FillParked { .. } => "fill_parked",
            Event::FillUnparked { .. } => "fill_unparked",
            Event::ExternalFillBooked { .. } => "external_fill_booked",
            Event::VenueTerminal { .. } => "venue_terminal",
            Event::MissingFillsFlagged { .. } => "missing_fills_flagged",
            Event::OrderClosed { .. } => "order_closed",
            Event::CancelRequested { .. } => "cancel_requested",
            Event::CancelSent { .. } => "cancel_sent",
            Event::CancelOutcomeNoted { .. } => "cancel_outcome_noted",
            Event::FillsSynced { .. } => "fills_synced",
            Event::OpenOrdersSwept { .. } => "open_orders_swept",
            Event::PositionsVerified { .. } => "positions_verified",
            Event::MismatchRaised { .. } => "mismatch_raised",
            Event::MismatchAcked { .. } => "mismatch_acked",
            Event::OperatorHalted { .. } => "operator_halted",
            Event::OperatorResumed { .. } => "operator_resumed",
            Event::StorageLowChanged { .. } => "storage_low_changed",
            Event::RefPriceUpdated { .. } => "ref_price_updated",
            Event::PositionAdjusted { .. } => "position_adjusted",
            Event::Purged { .. } => "purged",
            Event::MarginReported { .. } => "margin_reported",
        }
    }
}

impl Input {
    pub fn name(&self) -> &'static str {
        match self {
            Input::Boot { .. } => "boot",
            Input::Config(_) => "config",
            Input::NewOrder(_) => "new_order",
            Input::Cancel(_) => "cancel",
            Input::Operator(_) => "operator",
            Input::Tick => "tick",
            Input::Storage { .. } => "storage",
            Input::Venue(v) => match v {
                VenueInput::Link { .. } => "venue.link",
                VenueInput::SendResult { .. } => "venue.send_result",
                VenueInput::CancelResult { .. } => "venue.cancel_result",
                VenueInput::OrderStatus(_) => "venue.order_status",
                VenueInput::StatusNotFound { .. } => "venue.status_not_found",
                VenueInput::Fill(_) => "venue.fill",
                VenueInput::FillsSynced { .. } => "venue.fills_synced",
                VenueInput::OpenOrders { .. } => "venue.open_orders",
                VenueInput::Positions { .. } => "venue.positions",
                VenueInput::RefPrice { .. } => "venue.ref_price",
                VenueInput::MarginReport(_) => "venue.margin_report",
            },
        }
    }
}
