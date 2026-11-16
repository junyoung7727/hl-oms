//! Domain state: orders, books, holds and mismatches.
//!
//! Everything here is owned by the single core writer. Other threads only
//! ever see copies that the commit stage publishes after the corresponding
//! journal record is durable.

use crate::ids::*;
use crate::num::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    Buy,
    Sell,
}

/// Time in force.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tif {
    /// Rests until filled or cancelled.
    Gtc,
    /// Fills what it can immediately; the rest is cancelled by the venue.
    Ioc,
    /// Add-liquidity-only (post-only): rejected or cancelled if it would cross.
    Alo,
}

/// Where the order is in its life, from this OMS's point of view.
///
/// The phase records *what we have evidence for*, not what the venue's state
/// is at this instant. The venue may already have filled an order that is
/// still `PendingSend` here.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    /// Durably accepted and released to the gateway; no venue evidence yet.
    PendingSend,
    /// The venue has acknowledged the order (directly, via a stream update, a
    /// query result or a fill).
    Working,
    /// The venue reported a terminal status, but the fills booked so far do
    /// not yet account for the final cumulative quantity.
    Closing,
    /// No further effect is possible. The reservation is fully released.
    Closed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DoubtReason {
    /// The gateway sent the request and did not obtain a definite answer.
    Transport,
    /// The process restarted while the send was outstanding.
    Restart,
    /// No send result reached the core within the configured time.
    NoResult,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Doubt {
    pub reason: DoubtReason,
    pub detail: String,
    pub since: Ts,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VenueFinal {
    Filled,
    Canceled,
    Rejected,
}

/// Terminal evidence received from the venue.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Terminal {
    pub kind: VenueFinal,
    pub reason: String,
    /// Final cumulative filled quantity if the venue stated it.
    pub final_cum: Option<Qty>,
    pub venue_ts: Ts,
    pub at: Ts,
    /// Set once a `MissingFills` mismatch has been raised for this order.
    pub missing_fills_flagged: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloseReason {
    Filled,
    Canceled { venue_reason: String },
    RejectedByVenue { reason: String },
    /// The gateway is certain that nothing left this host.
    NotSent { reason: String },
    /// An in-doubt order shown to be absent under the venue's expiry contract.
    NotPlaced { evidence: String },
    /// Closed by an operator, with their stated evidence.
    Manual { note: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Close {
    pub reason: CloseReason,
    pub at: Ts,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Actor {
    Strategy(StrategyId),
    Operator,
    /// A margin rule of the core itself (C-MARGIN). The API builds only the
    /// two actors above; a request from outside cannot carry this one.
    Rule(RuleId),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CancelOutcome {
    /// The venue confirmed the cancel request. Terminal status still has to
    /// arrive separately; a confirmed cancel says nothing about earlier fills.
    Confirmed,
    Rejected { reason: String },
    Unknown { reason: String },
    NotSent { reason: String },
    /// The attempt was outstanding when the process restarted.
    LostOnRestart,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CancelState {
    pub requested_at: Ts,
    pub by: Actor,
    pub attempts: u32,
    /// Attempts made before the current budget started. The budget restarts
    /// when the cancel is requested again and when the order, in doubt until
    /// then, is found at the venue; attempt numbers keep counting up.
    #[serde(default)]
    pub base: u32,
    pub inflight: bool,
    pub last_attempt_at: Ts,
    pub confirmed: bool,
    pub last_outcome: Option<CancelOutcome>,
}

impl CancelState {
    /// Attempts used of the current budget.
    pub fn used(&self) -> u32 {
        self.attempts.saturating_sub(self.base)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Order {
    pub id: OrderId,
    pub strategy: StrategyId,
    pub client_order_id: ClientOrderId,
    pub instrument: InstrumentId,
    pub side: Side,
    pub qty: Qty,
    pub px: Px,
    pub tif: Tif,
    pub reduce_only: bool,
    /// The send must not be placed by the venue after this time. Also the end
    /// of the window in which the idempotency key must be remembered.
    pub expires_at: Ts,
    pub accepted_at: Ts,
    pub config_version: u32,
    /// Boot incarnation that released the send.
    pub send_boot: u32,
    /// Link session in which the order was accepted and released for sending.
    /// A send result or a fill counts as evidence of the current session only
    /// if the link has not changed since.
    #[serde(default)]
    pub send_session: u64,
    pub phase: Phase,
    pub doubt: Option<Doubt>,
    pub venue_oid: Option<VenueOrderId>,
    /// Sum of booked fills.
    pub cum_qty: Qty,
    pub cum_notional: Amount,
    pub fee: Amount,
    pub fills: Vec<FillKey>,
    /// Largest cumulative quantity the venue has reported in a status message.
    pub reported_cum: Qty,
    /// Quantity that can still be filled without having been booked yet. This
    /// is exactly this order's contribution to the books' open quantity.
    pub reserved: Qty,
    pub terminal: Option<Terminal>,
    pub close: Option<Close>,
    pub cancel: Option<CancelState>,
    /// Link session in which status evidence for this order was last seen.
    pub evidence_session: u64,
    pub last_venue_ts: Ts,
    pub overfilled: bool,
    /// Record time of the acknowledgement; 0 while unacknowledged.
    pub acked_at: Ts,
    /// Set when an open-order sweep taken after the acknowledgement did not
    /// list this order. Cleared by the next status evidence.
    pub recheck: bool,
}

impl Order {
    pub fn is_closed(&self) -> bool {
        self.phase == Phase::Closed
    }
    pub fn remaining(&self) -> Qty {
        Qty((self.qty.0 - self.cum_qty.0).max(0))
    }
}

/// Position and open-order exposure for one instrument within one scope.
///
/// `pos` moves only when a fill is booked. `buy_open` and `sell_open` are the
/// sums of `Order::reserved` over the scope's orders, so the true venue
/// position, including fills that have happened but are not yet booked, lies
/// in `[pos - sell_open, pos + buy_open]` as long as every order in the scope
/// went through this OMS.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Book {
    pub pos: SQty,
    pub buy_open: Qty,
    pub sell_open: Qty,
    pub open_orders: u32,
    pub traded_qty: Qty,
    pub traded_notional: Amount,
    pub fees: Amount,
    pub fills: u64,
}

impl Book {
    pub fn upper(&self) -> Option<i64> {
        self.pos.0.checked_add(self.buy_open.0)
    }
    pub fn lower(&self) -> Option<i64> {
        self.pos.0.checked_sub(self.sell_open.0)
    }
    pub fn is_empty(&self) -> bool {
        *self == Book::default()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    Account,
    Strategy(StrategyId),
}

/// Which ledger a position adjustment or an external fill is booked to.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Ledger {
    Unattributed,
    Strategy(StrategyId),
}

/// Reasons new exposure is currently refused. Several can hold at once and
/// clearing one never clears another.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HoldReason {
    /// No configuration has been activated yet.
    NoConfig,
    /// The venue link is down.
    LinkDown,
    /// The link is up but state has not been re-confirmed against the venue
    /// since it came up.
    Resync,
    /// At least one order's send outcome is unknown.
    InDoubt,
    /// At least one detected mismatch has not been acknowledged.
    Mismatch,
    /// An operator halted the account.
    OperatorAccount,
    /// An operator halted this strategy.
    OperatorStrategy,
    /// Journal storage is running low.
    StorageLow,
    /// The account is REDUCING: a margin rule tripped and only the operator
    /// clears it (C-MARGIN).
    Reducing,
}

impl HoldReason {
    /// The hold table of ADR-008 §7. False: the hold refuses every new
    /// order. True: it refuses only plain orders, and an order with the
    /// venue's reduce-only flag passes it. The table is used only when a
    /// margin policy is set; the new-order check decides that.
    pub fn refuses_only_plain_orders(&self) -> bool {
        match self {
            HoldReason::NoConfig | HoldReason::LinkDown | HoldReason::OperatorAccount | HoldReason::OperatorStrategy | HoldReason::StorageLow => false,
            HoldReason::Reducing => false,
            HoldReason::Resync | HoldReason::InDoubt | HoldReason::Mismatch => true,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MismatchKind {
    Overfill,
    FillAfterClose,
    CumExceedsFinal,
    MissingFills,
    ExternalFill,
    FillFieldConflict,
    OrphanOrder,
    ForeignOrder,
    OpenAfterClose,
    PositionOutOfRange,
    ForeignPosition,
    StatusConflict,
    CancelStuck,
    ParkedFillOverflow,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mismatch {
    pub id: u32,
    pub kind: MismatchKind,
    /// Deduplication key: the same (kind, subject) is not raised again while
    /// an unacknowledged entry exists.
    pub subject: String,
    pub detail: String,
    pub raised_at: Ts,
    pub acked: Option<MismatchAck>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MismatchAck {
    pub note: String,
    pub at: Ts,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Halt {
    pub note: String,
    pub at: Ts,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RefPrice {
    pub px: Px,
    pub venue_ts: Ts,
    pub at: Ts,
}

/// A fill whose order is not known yet. It is kept, durably, until either the
/// order is identified or the wait expires and it is booked as external.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParkedFill {
    pub fill: crate::input::FillIn,
    pub parked_at: Ts,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Link {
    pub up: bool,
    pub session: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncState {
    pub fills_session: u64,
    pub fills_up_to: Ts,
    pub open_orders_session: u64,
    pub positions_session: u64,
}

/// What the core knows from margin reports (C-MARGIN). Empty until the first
/// report is recorded, and always empty without a margin policy.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarginState {
    /// The newest report recorded. It stays, and ages, across link changes.
    pub report: Option<crate::input::MarginReportIn>,
    /// Why the most recent refused report was refused. Shown to the operator;
    /// no rule reads it.
    pub last_rejection: Option<MarginRejection>,
    /// The account value the daily loss is measured against. Changed only by
    /// `day_base_set`; a restart and a link change keep it.
    #[serde(default)]
    pub day_base: Option<DayBase>,
    /// Set while the account is REDUCING. One latch for the whole account.
    #[serde(default)]
    pub reducing: Option<Reducing>,
}

/// A margin rule that acts on its own: it latches REDUCING or cancels
/// orders.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleId {
    /// The account value fell more than `daily_loss_bps` below the day base.
    DailyLoss,
}

/// The REDUCING latch (C-MARGIN, 감소 전용 상태).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reducing {
    /// The rule that tripped first. A later trigger does not replace it.
    pub rule: RuleId,
    /// The numbers the rule saw, for the operator.
    pub detail: String,
    pub since: Ts,
}

/// The day base of the daily-loss rule (C-MARGIN, 일 손실과 기준 금액).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DayBase {
    /// UTC day number (days since 1970-01-01) of the query time of the
    /// report the value was taken from.
    pub day: u64,
    pub value: Amount,
    pub set_by: BaseSetBy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BaseSetBy {
    /// The first report, or the first report of a later UTC day.
    Report,
}

/// Microseconds in one UTC day.
pub const DAY_US: u64 = 86_400_000_000;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarginRejection {
    pub seq: u64,
    pub queried_at: Ts,
    pub reason: String,
    pub at: Ts,
}

impl MarginState {
    pub fn is_empty(&self) -> bool {
        *self == MarginState::default()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stats {
    pub accepted: u64,
    pub rejected: u64,
    pub fills_booked: u64,
    pub external_fills: u64,
    pub closed: u64,
    pub purged: u64,
}
