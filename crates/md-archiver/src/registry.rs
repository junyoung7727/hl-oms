//! The venues the archiver can read (spec D1). Each gives an adapter per
//! market and instrument row, its `parser_ver`, and what a channel holds.
//! U10/U11 add theirs here, as in the md-collector registry.

use md_core::adapter::{Instrument, MarketDataAdapter, ParseError};

/// What the rows of a channel are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelKind {
    /// Order book snapshots and deltas, with the subscribed depth.
    Book {
        depth: i64,
    },
    Trade,
    Ticker,
    /// The REST instruments list.
    Instruments,
}

pub trait Venue: Send + Sync {
    fn name(&self) -> &'static str;
    fn parser_ver(&self) -> &'static str;
    fn markets(&self) -> &'static [&'static str];
    fn channel_kind(&self, channel: &str) -> Option<ChannelKind>;
    /// The adapter of `market` that knows `instruments`.
    fn adapter(
        &self,
        market: &str,
        instruments: &[Instrument],
    ) -> Option<Box<dyn MarketDataAdapter + Send>>;
    /// The rows of one instruments response body.
    fn parse_instruments(
        &self,
        market: &str,
        body: &[u8],
        recv_ns: i64,
    ) -> Result<Vec<Instrument>, ParseError>;
}

struct Bybit;

fn bybit_market(market: &str) -> Option<md_bybit::Market> {
    Some(match market {
        "linear" => md_bybit::Market::Linear,
        "inverse" => md_bybit::Market::Inverse,
        "spot" => md_bybit::Market::Spot,
        _ => return None,
    })
}

impl Venue for Bybit {
    fn name(&self) -> &'static str {
        "bybit"
    }

    fn parser_ver(&self) -> &'static str {
        crate::PARSER_VER_BYBIT
    }

    fn markets(&self) -> &'static [&'static str] {
        &["linear", "inverse", "spot"]
    }

    fn channel_kind(&self, channel: &str) -> Option<ChannelKind> {
        if let Some(d) = channel.strip_prefix("orderbook.") {
            return d.parse().ok().map(|depth| ChannelKind::Book { depth });
        }
        match channel {
            "publicTrade" => Some(ChannelKind::Trade),
            "tickers" => Some(ChannelKind::Ticker),
            "instruments" => Some(ChannelKind::Instruments),
            _ => None,
        }
    }

    fn adapter(
        &self,
        market: &str,
        instruments: &[Instrument],
    ) -> Option<Box<dyn MarketDataAdapter + Send>> {
        let m = bybit_market(market)?;
        Some(Box::new(md_bybit::BybitAdapter::new(m, instruments)))
    }

    fn parse_instruments(
        &self,
        market: &str,
        body: &[u8],
        recv_ns: i64,
    ) -> Result<Vec<Instrument>, ParseError> {
        let m = bybit_market(market)
            .ok_or_else(|| ParseError::new(format!("bybit has no market {market}")))?;
        md_bybit::BybitAdapter::new(m, &[]).parse_instruments(body, recv_ns)
    }
}

/// The venue named `name`, or `None` (the run is refused, exit 2).
pub fn venue(name: &str) -> Option<&'static dyn Venue> {
    match name {
        "bybit" => Some(&Bybit),
        _ => None,
    }
}
