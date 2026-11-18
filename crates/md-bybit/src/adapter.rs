//! `BybitAdapter`: the md-core trait for one Bybit market.

use std::collections::BTreeMap;

use md_core::adapter::{
    ConnPlan, Instrument, Keepalive, MarketDataAdapter, ParseError, PlanError, RestRequest, Resync,
    Route,
};
use md_core::{GapEvent, Normalized, SeqRule, StreamKey};

use crate::{Market, instruments, parse, plan, route};

/// The adapter of one market. It knows the exponents of the instruments it
/// was built with; a symbol without an instrument row cannot be parsed.
#[derive(Debug, Clone)]
pub struct BybitAdapter {
    market: Market,
    instruments: BTreeMap<String, Instrument>,
    max_topics_per_conn: usize,
    first_conn_index: u32,
    max_conns: u32,
}

impl BybitAdapter {
    pub fn new(market: Market, instruments: &[Instrument]) -> Self {
        Self {
            market,
            instruments: instruments
                .iter()
                .map(|i| (i.symbol.clone(), i.clone()))
                .collect(),
            max_topics_per_conn: plan::DEFAULT_MAX_TOPICS,
            first_conn_index: 0,
            max_conns: 100,
        }
    }

    /// At most `n` topics per connection (default 200).
    pub fn with_max_topics_per_conn(mut self, n: usize) -> Self {
        self.max_topics_per_conn = n;
        self
    }

    /// The connections this market may open: `conn_index` runs from `first`
    /// and a plan that needs more than `max` connections is refused. Bybit
    /// counts 500 new connections per 5 minutes over all markets of
    /// `stream.bybit.com`, so the collector gives each market a disjoint range
    /// and keeps the sum of the budgets ≤ 250, leaving room for a full
    /// reconnect (judgement, not a model). Default `(0, 100)`.
    pub fn with_conn_budget(mut self, first: u32, max: u32) -> Self {
        self.first_conn_index = first;
        self.max_conns = max;
        self
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
        symbols: &[String],
        channels: &[String],
        depth: u32,
    ) -> Result<Vec<ConnPlan>, PlanError> {
        plan::plan(
            symbols,
            channels,
            depth,
            self.max_topics_per_conn,
            self.first_conn_index,
            self.max_conns,
        )
    }

    fn subscribe_frames(&self, plan: &ConnPlan) -> Vec<String> {
        plan::subscribe_frames(plan)
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

    fn resubscribe_frames(&self, plan: &ConnPlan, key: &StreamKey) -> Vec<String> {
        plan::resubscribe_frames(plan, key)
    }

    fn market_name(&self) -> Option<&'static str> {
        Some(self.market.category())
    }
}
