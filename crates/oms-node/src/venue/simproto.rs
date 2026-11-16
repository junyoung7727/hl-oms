//! Wire protocol of the simulated venue process (`simvenue`).
//!
//! One JSON object per line over a Unix socket. It exists for integration
//! tests and load tests: it gives the OMS a venue in another process, with
//! real sockets, real timeouts and a state that survives an OMS restart.
//! The control operations (`ctl`) inject faults and drive the market; they
//! are sent to the venue by the test, never through the OMS.

use oms_core::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SimReq {
    pub id: u64,
    #[serde(flatten)]
    pub op: SimOp,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum SimOp {
    Place {
        /// Client id in hex, as `VenueClientId::to_hex` renders it.
        client: String,
        instrument: InstrumentId,
        side: Side,
        qty: Qty,
        px: Px,
        tif: Tif,
        expires_at: Ts,
    },
    Cancel {
        client: String,
    },
    Status {
        client: String,
    },
    Fills {
        since: Ts,
    },
    OpenOrders,
    Positions,
    /// The account's margin report. Answered with a `MarginResult`.
    Margin,
    /// Margin mode and leverage of one instrument. Answered with a
    /// `VenueSettingResult`.
    VenueSetting {
        instrument: InstrumentId,
    },
    /// The account mode. Answered with an `AccountModeResult`.
    AccountMode,
    /// Turns this connection into an event stream.
    Subscribe,
    Ctl {
        action: Ctl,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Ctl {
    /// Trade `qty` (or everything left) of an open order.
    Fill { client: String, qty: Option<Qty> },
    /// Cancel an order from the venue's side.
    VenueCancel { client: String },
    /// A trade on the account that no OMS order caused.
    ExternalFill { instrument: InstrumentId, side: Side, qty: Qty, px: Px },
    /// Process the next `n` place/cancel requests but never answer them.
    DropResponses { n: u32 },
    /// Ignore the next `n` place/cancel requests entirely.
    DropRequests { n: u32 },
    /// Wait this long before processing each place/cancel request.
    DelayMs { ms: u64 },
    /// What a new resting order does on arrival.
    AutoFill { mode: AutoFill },
    /// Close every event stream connection.
    KickStreams,
    /// Refuse new stream subscriptions while set.
    RefuseStreams { on: bool },
    RefPrice { instrument: InstrumentId, px: Px },
    /// Sets the account value the venue states in its margin reports.
    AccountValue { value: Amount },
    /// Sets the margin the venue states for the position on one instrument,
    /// in place of its model's figure (position value / leverage).
    MarginUsed { instrument: InstrumentId, value: Amount },
    /// Sets the margin mode and the leverage the venue states for one
    /// instrument. Stands for the operator changing it at the venue.
    VenueSetting { instrument: InstrumentId, isolated: bool, leverage: u32 },
    /// Sets the account mode the venue states.
    AccountMode { mode: String },
    /// Counters and the venue's own view, for test assertions.
    Dump,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutoFill {
    /// Orders rest until the test fills them.
    None,
    /// Every order fills completely on arrival.
    Full,
    /// Half of every order fills on arrival.
    Half,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SimResp {
    pub id: u64,
    pub ok: bool,
    #[serde(default)]
    pub result: serde_json::Value,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WireStatus {
    Open,
    Filled,
    Canceled,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WireOrder {
    pub oid: u64,
    pub client: Option<String>,
    pub instrument: InstrumentId,
    pub side: Side,
    pub qty: Qty,
    pub filled: Qty,
    pub status: WireStatus,
    #[serde(default)]
    pub reason: String,
    pub ts: Ts,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WireFill {
    pub tid: u64,
    pub oid: u64,
    pub client: Option<String>,
    pub instrument: InstrumentId,
    pub side: Side,
    pub qty: Qty,
    pub px: Px,
    pub fee: Amount,
    pub ts: Ts,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaceResult {
    Placed(WireOrder),
    Rejected { reason: String },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StatusResult {
    Found(WireOrder),
    NotFound,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FillsResult {
    pub fills: Vec<WireFill>,
    /// Venue time at which the answer was computed.
    pub venue_now: Ts,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WirePosition {
    pub instrument: InstrumentId,
    pub qty: SQty,
}

/// Answer to `SimOp::Margin`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MarginResult {
    pub account_value: Amount,
    /// Every position of the account, with its value, margin, margin mode,
    /// leverage and maximum leverage.
    #[serde(default)]
    pub positions: Vec<MarginPosition>,
}

/// Answer to `SimOp::VenueSetting`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VenueSettingResult {
    pub isolated: bool,
    pub leverage: u32,
}

/// Answer to `SimOp::AccountMode`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AccountModeResult {
    pub mode: String,
}

/// What the simulated venue calls its ordinary account mode. This is the
/// simulated venue's own name, not a Hyperliquid value (T10).
pub const SIM_STANDARD_MODE: &str = "standard";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StreamMsg {
    Order(WireOrder),
    Fill(WireFill),
    RefPrice { instrument: InstrumentId, px: Px, ts: Ts },
}

/// A line on a stream connection.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StreamLine {
    pub stream: StreamMsg,
}
