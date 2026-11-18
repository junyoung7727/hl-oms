//! The contract every venue adapter implements (design §6.4). The collector,
//! the archiver and the strategy hold an adapter and never parse venue frames
//! themselves.

use crate::{GapEvent, Normalized, SeqRule, StreamKey};

/// The topics one WebSocket connection carries, within the venue's limits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnPlan {
    pub conn_index: u32,
    pub topics: Vec<StreamKey>,
    /// Subscribed book depth (50, 200, 1000, ...).
    pub depth: u32,
}

/// The requested set cannot be planned (a venue limit would be broken, or a
/// channel or depth is not offered). The collector refuses to start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanError(pub String);

/// How often to send what, to keep a connection open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Keepalive {
    pub interval_ms: u64,
    /// `None` when the venue pings and the client only answers.
    pub frame: Option<String>,
}

/// Where a received frame belongs. `route` reads only the topic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route {
    Stream(StreamKey),
    /// Subscribe answers, pongs, errors and anything that cannot be routed;
    /// the collector writes these to its session file.
    Control,
}

/// What to do after a gap (design §6.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resync {
    Resubscribe,
    RestSnapshotThenReplay,
    Reconnect,
}

/// A REST request the feed sends; the adapter does no I/O.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestRequest {
    pub method: &'static str,
    pub path: String,
    pub query: Vec<(String, String)>,
}

/// One row of the venue's instrument list (L1 `instrument`, lake contract 3.6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Instrument {
    pub symbol: String,
    pub status: String,
    /// The venue's decimal strings, unchanged.
    pub tick_size: String,
    pub lot_size: String,
    pub px_exp: i8,
    pub qty_exp: i8,
    pub contract_type: Option<String>,
    pub settle_coin: Option<String>,
    pub venue_ts_ns: Option<i64>,
}

/// A frame could not be turned into messages (L1 gap kind `parse_error`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub reason: String,
}

impl ParseError {
    pub fn new(reason: impl Into<String>) -> Self {
        Self { reason: reason.into() }
    }
}

pub trait MarketDataAdapter {
    fn venue(&self) -> &'static str;
    /// Split the topics over connections within the venue's limits.
    fn plan(&self, symbols: &[String], channels: &[String], depth: u32) -> Result<Vec<ConnPlan>, PlanError>;
    /// Frames to send after connecting.
    fn subscribe_frames(&self, plan: &ConnPlan) -> Vec<String>;
    fn keepalive(&self) -> Keepalive;
    /// Never fails: a frame that cannot be routed is `Route::Control`.
    fn route(&self, frame: &[u8]) -> Route;
    fn parse(&self, key: &StreamKey, frame: &[u8], recv_ns: i64) -> Result<Vec<Normalized>, ParseError>;
    fn seq_rule(&self, key: &StreamKey) -> SeqRule;
    fn on_gap(&self, key: &StreamKey, gap: &GapEvent) -> Resync;
    fn instruments_request(&self) -> RestRequest;
    fn parse_instruments(&self, body: &[u8], recv_ns: i64) -> Result<Vec<Instrument>, ParseError>;
    /// The venue field used as `venue_ts_ns` on this channel (for example Bybit
    /// order book `ts`, trade `T`), for the documentation of the parser.
    fn venue_ts_field(&self, channel: &str) -> &'static str;
}
