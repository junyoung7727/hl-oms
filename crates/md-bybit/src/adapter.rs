//! `BybitAdapter`: the md-core trait for one Bybit market.

use std::collections::BTreeMap;

use md_core::adapter::{
    ConnPlan, Instrument, Keepalive, MarketDataAdapter, ParseError, PlanError, RestRequest, Resync,
    Route,
};
use md_core::{GapEvent, Normalized, SeqRule, StreamKey};

use crate::{Market, instruments, parse, route};

/// The adapter of one market. It knows the exponents of the instruments it
/// was built with; a symbol without an instrument row cannot be parsed.
#[derive(Debug, Clone)]
pub struct BybitAdapter {
    market: Market,
    instruments: BTreeMap<String, Instrument>,
}

impl BybitAdapter {
    pub fn new(market: Market, instruments: &[Instrument]) -> Self {
        Self {
            market,
            instruments: instruments
                .iter()
                .map(|i| (i.symbol.clone(), i.clone()))
                .collect(),
        }
    }

    pub fn market(&self) -> Market {
        self.market
    }

    /// The instruments request of a later page (`cursor` from
    /// [`crate::next_page_cursor`]). Spot ignores the cursor: it has one page.
    pub fn instruments_request_page(&self, cursor: Option<&str>) -> RestRequest {
        instruments::request(self.market, cursor)
    }

    pub(crate) fn instrument(&self, symbol: &str) -> Result<&Instrument, ParseError> {
        self.instruments
            .get(symbol)
            .ok_or_else(|| ParseError::new(format!("no instrument row for {symbol}")))
    }
}

impl MarketDataAdapter for BybitAdapter {
    fn venue(&self) -> &'static str {
        "bybit"
    }

    fn plan(
        &self,
        _symbols: &[String],
        _channels: &[String],
        _depth: u32,
    ) -> Result<Vec<ConnPlan>, PlanError> {
        Err(PlanError("plan is built in ticket 04".into()))
    }

    fn subscribe_frames(&self, _plan: &ConnPlan) -> Vec<String> {
        Vec::new()
    }

    fn keepalive(&self) -> Keepalive {
        Keepalive {
            interval_ms: 20_000,
            frame: Some(r#"{"op":"ping"}"#.into()),
        }
    }

    fn route(&self, frame: &[u8]) -> Route {
        route::route(frame)
    }

    fn parse(
        &self,
        key: &StreamKey,
        frame: &[u8],
        recv_ns: i64,
    ) -> Result<Vec<Normalized>, ParseError> {
        let inst = self.instrument(&key.symbol)?;
        match key.channel.as_str() {
            c if c.starts_with("orderbook.") => parse::book(key, inst, frame, recv_ns),
            "publicTrade" => parse::trades(key, inst, frame, recv_ns),
            "tickers" => parse::ticker(key, inst, self.market == Market::Spot, frame, recv_ns),
            other => Err(ParseError::new(format!("channel {other} is not parsed"))),
        }
    }

    fn seq_rule(&self, key: &StreamKey) -> SeqRule {
        if key.channel.starts_with("orderbook.") {
            SeqRule::PlusOne
        } else {
            SeqRule::None
        }
    }

    fn on_gap(&self, _key: &StreamKey, _gap: &GapEvent) -> Resync {
        Resync::Resubscribe
    }

    fn instruments_request(&self) -> RestRequest {
        instruments::request(self.market, None)
    }

    fn parse_instruments(&self, body: &[u8], recv_ns: i64) -> Result<Vec<Instrument>, ParseError> {
        instruments::parse(self.market, body, recv_ns)
    }

    fn venue_ts_field(&self, channel: &str) -> &'static str {
        if channel == "publicTrade" { "T" } else { "ts" }
    }
}
