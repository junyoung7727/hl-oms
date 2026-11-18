//! Normalized market-data messages. Field names and units follow the L1 tables
//! of the lake contract (3.2–3.4), so that the archiver copies fields into rows.

/// A topic of one venue: `(channel, symbol)`, for example `("orderbook.50", "BTCUSDT")`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct StreamKey {
    pub channel: String,
    pub symbol: String,
}

impl StreamKey {
    pub fn new(channel: impl Into<String>, symbol: impl Into<String>) -> Self {
        Self { channel: channel.into(), symbol: symbol.into() }
    }
}

/// One price level: integer price and quantity in units of `10^px_exp` and `10^qty_exp`.
/// In a delta, quantity 0 means "remove the level".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Level {
    pub px: i64,
    pub qty: i64,
}

/// Fields shared by snapshots and deltas.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BookUpdate {
    pub key: StreamKey,
    /// The venue's event time (Bybit `ts`, Binance `E`), nanoseconds.
    pub venue_ts_ns: i64,
    /// Our receive time, nanoseconds.
    pub recv_ns: i64,
    /// Bybit `u`, Binance `u`, OKX `seqId`; `None` for Hyperliquid.
    pub update_id: Option<i64>,
    /// Binance `pu`, OKX `prevSeqId`.
    pub prev_update_id: Option<i64>,
    /// Binance `U`.
    pub first_update_id: Option<i64>,
    /// Bybit `seq`.
    pub venue_seq: Option<i64>,
    /// Levels as received, in the venue's order.
    pub bids: Vec<Level>,
    pub asks: Vec<Level>,
    pub px_exp: i8,
    pub qty_exp: i8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    /// The taker bought.
    Buy,
    /// The taker sold.
    Sell,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trade {
    pub key: StreamKey,
    /// Bybit `T`.
    pub venue_ts_ns: i64,
    pub recv_ns: i64,
    pub trade_id: String,
    pub side: Side,
    pub px: i64,
    pub qty: i64,
    pub px_exp: i8,
    pub qty_exp: i8,
    pub venue_seq: Option<i64>,
    /// Bit 0 block trade, bit 1 RPI trade.
    pub flags: u32,
}

/// A ticker message; a delta carries only the fields the venue sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ticker {
    pub key: StreamKey,
    pub is_snapshot: bool,
    pub venue_ts_ns: i64,
    pub recv_ns: i64,
    pub last_px: Option<i64>,
    pub mark_px: Option<i64>,
    pub index_px: Option<i64>,
    /// In units of 1e-8.
    pub funding_rate: Option<i64>,
    pub next_funding_ns: Option<i64>,
    /// With `qty_exp`.
    pub open_interest: Option<i64>,
    pub best_bid_px: Option<i64>,
    pub best_bid_qty: Option<i64>,
    pub best_ask_px: Option<i64>,
    pub best_ask_qty: Option<i64>,
    pub px_exp: i8,
    pub qty_exp: i8,
}

/// A frame that is not market data: subscribe answers, pongs, errors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Control {
    pub recv_ns: i64,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Normalized {
    BookSnapshot(BookUpdate),
    BookDelta(BookUpdate),
    Trade(Trade),
    Ticker(Ticker),
    Control(Control),
}
