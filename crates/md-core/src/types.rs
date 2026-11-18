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
        Self {
            channel: channel.into(),
            symbol: symbol.into(),
        }
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
    /// With `ref_px_exp`: computed prices can be finer than the tick (lake contract 1.3).
    pub mark_px: Option<i64>,
    /// With `ref_px_exp`.
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
    /// `px_exp - 2`, the exponent of `mark_px` and `index_px`.
    pub ref_px_exp: i8,
    pub qty_exp: i8,
}

impl Ticker {
    /// Applies a delta to this state: each field the delta carries replaces
    /// the state's, the others stay, and the times become the delta's. Both
    /// must be of the same instrument row: with other exponents nothing
    /// changes and `false` is returned, because the integers would not be
    /// comparable.
    pub fn merge(&mut self, delta: &Ticker) -> bool {
        if (self.px_exp, self.ref_px_exp, self.qty_exp)
            != (delta.px_exp, delta.ref_px_exp, delta.qty_exp)
        {
            return false;
        }
        self.venue_ts_ns = delta.venue_ts_ns;
        self.recv_ns = delta.recv_ns;
        for (to, from) in [
            (&mut self.last_px, delta.last_px),
            (&mut self.mark_px, delta.mark_px),
            (&mut self.index_px, delta.index_px),
            (&mut self.funding_rate, delta.funding_rate),
            (&mut self.next_funding_ns, delta.next_funding_ns),
            (&mut self.open_interest, delta.open_interest),
            (&mut self.best_bid_px, delta.best_bid_px),
            (&mut self.best_bid_qty, delta.best_bid_qty),
            (&mut self.best_ask_px, delta.best_ask_px),
            (&mut self.best_ask_qty, delta.best_ask_qty),
        ] {
            if from.is_some() {
                *to = from;
            }
        }
        true
    }
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
