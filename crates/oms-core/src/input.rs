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
    /// The order book state the strategy decided on, for joining the order
    /// to the market-data lake (U13c). The core does not read it, and it is
    /// not part of the idempotency comparison: a repeated request keeps the
    /// first one. Not written when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub market_ref: Option<MarketRef>,
}

/// The fields of the last applied message of the book an order was decided
/// on (the lake contract's order-to-market join key; `md_feed::MarketRef`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarketRef {
    pub venue: RefName,
    pub market: RefName,
    pub symbol: RefName,
    pub channel: RefName,
    pub update_id: Option<i64>,
    pub venue_ts_ns: i64,
    pub recv_ns: i64,
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
    /// Ends REDUCING and takes the current account value as the new day
    /// base (C-MARGIN, 해제). Needs a fresh margin report and a note.
    ClearReduce { note: String },
    /// Takes the current account value as the day base and leaves REDUCING
    /// as it is (C-MARGIN, 기준 재설정). For use after a deposit or a
    /// withdrawal. Needs a fresh margin report and a note.
    RebaseDay { note: String },
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
    /// The venue's answer to one margin query (C-MARGIN).
    MarginReport(MarginReport),
    /// The venue answered a margin query and the gateway could not read the
    /// answer as a whole report. No part of it is used.
    MarginReportInvalid {
        session: u64,
        seq: u64,
        queried_at: Ts,
        reason: String,
    },
    /// The venue's answer to one venue-setting query: the margin mode and
    /// the leverage of one instrument (C-MARGIN). The OMS only reads them.
    VenueSetting {
        session: u64,
        instrument: InstrumentId,
        isolated: bool,
        leverage: u32,
    },
    /// The venue's answer to the account-mode query: the account-wide
    /// margin scheme, as the venue names it (C-MARGIN).
    AccountMode { session: u64, mode: String },
    /// A funding payment: cash only. It changes no position and is not a
    /// fill (U13b).
    Funding(FundingIn),
    /// A position change the venue made on its own (U13b).
    PositionChange(VenuePositionChangeIn),
    /// An execution the gateway could not classify (U13b). Nothing is
    /// booked; plain orders are held until the operator looks.
    UnclassifiedExecution {
        key: FillKey,
        instrument: InstrumentId,
        /// The venue's name for the kind of execution, as it came.
        exec_type: String,
        /// What else the gateway could read, for the operator.
        detail: String,
        venue_ts: Ts,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FundingIn {
    /// The gateway's key for the execution (C-GW G-8). De-duplicated with
    /// the fill keys.
    pub key: FillKey,
    pub instrument: InstrumentId,
    /// Positive is paid by the account, negative received: the sign of
    /// `FillIn::fee`.
    pub amount: Amount,
    /// The funding rate as the venue wrote it. For audit; the core does
    /// not read it.
    pub rate: String,
    pub venue_ts: Ts,
}

/// Why the venue moved a position (U13b). A trade of an order that is not
/// ours is not one of these: it is a fill without an order (external fill).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VenueChangeKind {
    /// Auto-deleveraging.
    Adl,
    /// Takeover of a liquidated position.
    Takeover,
    Delivery,
    Settle,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VenuePositionChangeIn {
    /// The gateway's key for the execution (C-GW G-8). De-duplicated with
    /// the fill keys.
    pub key: FillKey,
    pub kind: VenueChangeKind,
    pub instrument: InstrumentId,
    /// Signed: positive adds to a long.
    pub qty: SQty,
    pub px: Px,
    pub venue_ts: Ts,
}

/// What the venue said about the account's margin at one moment. The whole
/// report is one fact: it is recorded as it came or not at all.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarginReport {
    /// Link session in which the query was sent.
    pub session: u64,
    /// Number of the query. It rises within a link session, so a late answer
    /// is told from a new one without reading a clock.
    pub seq: u64,
    /// Node time immediately before the query was sent.
    pub queried_at: Ts,
    pub account_value: Amount,
    /// Every position the venue listed, also on instruments that are not
    /// configured here.
    pub positions: Vec<MarginPosition>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarginPosition {
    pub instrument: InstrumentId,
    /// Signed quantity of the position: positive is long. The journal and
    /// the status keep the key `szi` they were first written with.
    #[serde(rename = "szi")]
    pub qty: SQty,
    pub position_value: Amount,
    pub margin_used: Amount,
    pub leverage_type: LeverageType,
    pub leverage_value: u32,
    /// The venue's maximum leverage for the instrument. Never zero in a
    /// report that was recorded.
    pub max_leverage: u32,
}

/// Margin mode of one instrument at the venue.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LeverageType {
    Isolated,
    Cross,
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
    /// The venue's maker flag, when the gateway knows it (Bybit `isMaker`).
    /// Not written when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_maker: Option<bool>,
    /// When the gateway received the fill, ns since the Unix epoch. Not
    /// written when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recv_ns: Option<i64>,
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
