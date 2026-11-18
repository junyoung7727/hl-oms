//! Bybit v5 public market data (linear, inverse, spot) for md-core.
//!
//! No network, file, clock or randomness access: the feed (U5a) connects and
//! the adapter only plans, routes and parses (design §6.4). Topics:
//! `orderbook.{1,50,200,1000}.{symbol}`, `publicTrade.{symbol}`,
//! `tickers.{symbol}`. `venue_ts_ns` is the order book and ticker `ts` and the
//! trade `T` (lake contract 1.2).

mod adapter;
mod instruments;
mod parse;
mod plan;
mod route;

pub use adapter::BybitAdapter;
pub use instruments::next_page_cursor;

/// A Bybit public WebSocket market. Options are not supported.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Market {
    Linear,
    Inverse,
    Spot,
}

impl Market {
    /// The REST and WebSocket `category`.
    pub fn category(self) -> &'static str {
        match self {
            Market::Linear => "linear",
            Market::Inverse => "inverse",
            Market::Spot => "spot",
        }
    }

    /// The public stream of this market. All three are on `stream.bybit.com`,
    /// whose connection limit counts them together.
    pub fn ws_url(self) -> &'static str {
        match self {
            Market::Linear => "wss://stream.bybit.com/v5/public/linear",
            Market::Inverse => "wss://stream.bybit.com/v5/public/inverse",
            Market::Spot => "wss://stream.bybit.com/v5/public/spot",
        }
    }
}
