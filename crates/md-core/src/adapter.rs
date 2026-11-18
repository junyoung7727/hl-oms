//! The contract every venue adapter implements (design §6.4). The collector,
//! the archiver and the strategy hold an adapter and never parse venue frames
//! themselves.

use crate::{BookUpdate, GapEvent, Normalized, SeqRule, StreamKey};

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

/// What a delta does to a REST snapshot with `update_id` S (see
/// [`MarketDataAdapter::bridge`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bridge {
    /// The delta is already in the snapshot: drop it.
    Drop,
    /// The first delta after the snapshot: apply it, the book is in sync.
    Bridges,
    /// The snapshot is older than the delta: fetch a new snapshot.
    Fails,
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
        Self {
            reason: reason.into(),
        }
    }
}

pub trait MarketDataAdapter {
    fn venue(&self) -> &'static str;
    /// Split the topics over connections within the venue's limits.
    fn plan(
        &self,
        symbols: &[String],
        channels: &[String],
        depth: u32,
    ) -> Result<Vec<ConnPlan>, PlanError>;
    /// Frames to send after connecting.
    fn subscribe_frames(&self, plan: &ConnPlan) -> Vec<String>;
    fn keepalive(&self) -> Keepalive;
    /// Never fails: a frame that cannot be routed is `Route::Control`.
    fn route(&self, frame: &[u8]) -> Route;
    fn parse(
        &self,
        key: &StreamKey,
        frame: &[u8],
        recv_ns: i64,
    ) -> Result<Vec<Normalized>, ParseError>;
    fn seq_rule(&self, key: &StreamKey) -> SeqRule;
    fn on_gap(&self, key: &StreamKey, gap: &GapEvent) -> Resync;
    fn instruments_request(&self) -> RestRequest;
    fn parse_instruments(&self, body: &[u8], recv_ns: i64) -> Result<Vec<Instrument>, ParseError>;
    /// The venue field used as `venue_ts_ns` on this channel (for example Bybit
    /// order book `ts`, trade `T`), for the documentation of the parser.
    fn venue_ts_field(&self, channel: &str) -> &'static str;

    /// Whether the topic carries an order book. The feed keeps a book, a stale
    /// flag and a `market_ref` only for these. Default: topics with a sequence
    /// rule; an adapter whose books have none (Hyperliquid) overrides it.
    fn is_book(&self, key: &StreamKey) -> bool {
        self.seq_rule(key) != SeqRule::None
    }

    /// Frames that resubscribe one topic on its connection (after a gap with
    /// [`Resync::Resubscribe`]). Empty: the venue cannot do that for one
    /// topic, and the feed reconnects instead. Default: empty.
    fn resubscribe_frames(&self, _plan: &ConnPlan, _key: &StreamKey) -> Vec<String> {
        Vec::new()
    }

    /// The REST request of a book snapshot (after a gap with
    /// [`Resync::RestSnapshotThenReplay`]). `None`: no snapshot endpoint, the
    /// feed reconnects instead. Default: `None`.
    fn snapshot_request(&self, _key: &StreamKey, _depth: u32) -> Option<RestRequest> {
        None
    }

    /// Parses the body of [`Self::snapshot_request`] into a snapshot with its
    /// `update_id`. Default: an error.
    fn parse_snapshot(
        &self,
        _key: &StreamKey,
        _body: &[u8],
        _recv_ns: i64,
    ) -> Result<BookUpdate, ParseError> {
        Err(ParseError::new("this adapter has no REST snapshot"))
    }

    /// How buffered delta `d` relates to a REST snapshot with `update_id`
    /// `s`. Default: `d.update_id <= s` is dropped; the next one bridges when
    /// `U <= s + 1 <= u` if it has a first update id (Binance spot `U`), else
    /// when it follows `s` by the stream's rule. Binance USD-M uses another
    /// rule (`u < s` dropped, `U <= s <= u`) and overrides this (U10).
    fn bridge(&self, key: &StreamKey, s: i64, d: &BookUpdate) -> Bridge {
        let Some(u) = d.update_id else {
            return Bridge::Fails;
        };
        if u <= s {
            return Bridge::Drop;
        }
        let Some(next) = s.checked_add(1) else {
            return Bridge::Fails;
        };
        let ok = match d.first_update_id {
            Some(first) => first <= next && next <= u,
            None => match self.seq_rule(key) {
                SeqRule::PlusOne => u == next,
                SeqRule::PrevEquals => d.prev_update_id == Some(s),
                SeqRule::None => true,
            },
        };
        if ok { Bridge::Bridges } else { Bridge::Fails }
    }

    /// The market this adapter was built for (`linear`, `spot`, ...), when it
    /// knows it. The feed refuses a config with another market, so that
    /// `market_ref` names the right market. Default: `None`.
    fn market_name(&self) -> Option<&'static str> {
        None
    }
}
