//! The venues this build can collect and seal (spec D1). Each venue gives its
//! markets in a fixed slot order, its classifier for the seal, and its
//! adapter. U10/U11 add theirs here.
//!
//! **Market slots are pinned.** A record's market is read from its `conn_id`:
//! market slot `k` owns `conn_id`s `[max(k << 28, 1), (k + 1) << 28)`. Old L0
//! files are classified with the same rule, so a slot is never renumbered.

use std::sync::Arc;

use md_core::adapter::{ConnPlan, Instrument, MarketDataAdapter, RestRequest, Route};
use md_feed::{FeedConfig, FeedHandle, FrameSink, MessageSink};
use md_lake::qraw::{Record, Src};
use md_lake::seal::Topic;

/// Bits of `conn_id` below the market slot.
pub const SLOT_SHIFT: u32 = 28;

/// `conn_id` range `[start, end)` of market slot `slot`.
pub fn conn_range(slot: u32) -> (u32, u32) {
    let start = (slot << SLOT_SHIFT).max(1);
    let end = slot
        .checked_add(1)
        .and_then(|s| s.checked_shl(SLOT_SHIFT))
        .filter(|&e| e > start)
        .unwrap_or(u32::MAX);
    (start, end)
}

/// The market slot of a `conn_id`.
pub fn slot_of_conn_id(conn_id: u32) -> u32 {
    conn_id >> SLOT_SHIFT
}

/// Where a feed hands its frames and its parsed messages.
#[derive(Default, Clone)]
pub struct Sinks {
    pub frames: Option<Arc<dyn FrameSink>>,
    /// U6: receive lag of books, trades and tickers.
    pub msgs: Option<Arc<dyn MessageSink>>,
}

/// Venue code the collector and the seal need.
pub trait Venue: Send + Sync {
    fn name(&self) -> &'static str;
    /// Markets in slot order (position = slot).
    fn markets(&self) -> &'static [&'static str];
    /// Largest sum of the markets' connection budgets.
    fn max_conn_budget(&self) -> u32;
    fn default_ws_url(&self, market: &str) -> Option<&'static str>;
    fn default_rest_base(&self) -> &'static str;
    /// The book depth of a book channel, `None` for other channels.
    fn book_depth(&self, channel: &str) -> Option<u32>;
    /// The adapter's plan for `symbols × channels`, `conn_index` from `first`,
    /// at most `max` connections.
    #[allow(clippy::too_many_arguments)]
    fn plan(
        &self,
        market: &str,
        symbols: &[String],
        channels: &[String],
        depth: u32,
        first: u32,
        max: u32,
        max_topics: Option<usize>,
    ) -> Result<Vec<ConnPlan>, String>;
    fn instruments_request(&self, market: &str, cursor: Option<&str>) -> Option<RestRequest>;
    fn next_page_cursor(&self, body: &[u8]) -> Option<String>;
    fn parse_instruments(
        &self,
        market: &str,
        body: &[u8],
        recv_ns: i64,
    ) -> Result<Vec<Instrument>, String>;
    /// Whether a symbol is collected under `symbols = "all"`.
    fn is_trading(&self, i: &Instrument) -> bool;
    /// Where a WebSocket frame of `market` belongs (reads only the topic).
    fn route(&self, market: &str, payload: &[u8]) -> Route;
    /// The market a REST body names, if it names one.
    fn rest_market(&self, body: &[u8]) -> Option<String>;
    /// Starts a feed of one market for these plans.
    fn start_feed(
        &self,
        market: &str,
        instruments: &[Instrument],
        max_topics: Option<usize>,
        cfg: FeedConfig,
        plans: Vec<ConnPlan>,
        sinks: Sinks,
    ) -> Result<FeedHandle, String>;

    fn market_slot(&self, market: &str) -> Option<u32> {
        let i = self.markets().iter().position(|m| *m == market)?;
        u32::try_from(i).ok()
    }

    /// The seal's classifier (spec D1): WebSocket records by `conn_id` slot
    /// and topic, REST bodies by the market they name, notes never.
    fn classify(&self, r: &Record) -> Option<Topic> {
        match r.src {
            Src::WebSocket => {
                let slot = usize::try_from(slot_of_conn_id(r.conn_id)).ok()?;
                let market = self.markets().get(slot)?;
                match self.route(market, &r.payload) {
                    Route::Stream(k) => Some(Topic {
                        market: market.to_string(),
                        channel: k.channel,
                        symbol: k.symbol,
                    }),
                    Route::Control => None,
                }
            }
            Src::Rest => {
                let market = self.rest_market(&r.payload)?;
                self.market_slot(&market)?;
                Some(Topic {
                    market,
                    channel: "instruments".into(),
                    symbol: "_all".into(),
                })
            }
            Src::Note => None,
        }
    }
}

/// The registered venue of this name.
pub fn venue(name: &str) -> Option<&'static dyn Venue> {
    match name {
        "bybit" => Some(&Bybit),
        _ => None,
    }
}

/// Bybit v5 public market data (md-bybit).
pub struct Bybit;

fn bybit_market(market: &str) -> Option<md_bybit::Market> {
    match market {
        "linear" => Some(md_bybit::Market::Linear),
        "inverse" => Some(md_bybit::Market::Inverse),
        "spot" => Some(md_bybit::Market::Spot),
        _ => None,
    }
}

fn bybit_adapter(
    market: &str,
    instruments: &[Instrument],
    max_topics: Option<usize>,
) -> Result<md_bybit::BybitAdapter, String> {
    let m = bybit_market(market).ok_or_else(|| format!("bybit has no market {market}"))?;
    let a = md_bybit::BybitAdapter::new(m, instruments);
    Ok(match max_topics {
        Some(n) => a.with_max_topics_per_conn(n),
        None => a,
    })
}

impl Venue for Bybit {
    fn name(&self) -> &'static str {
        "bybit"
    }

    fn markets(&self) -> &'static [&'static str] {
        &["linear", "inverse", "spot"]
    }

    /// Bybit counts 500 new connections per 5 minutes over all markets; the
    /// budgets stay ≤ 250 to leave room for a full reconnect (md-bybit,
    /// judgement, not a model).
    fn max_conn_budget(&self) -> u32 {
        250
    }

    fn default_ws_url(&self, market: &str) -> Option<&'static str> {
        bybit_market(market).map(md_bybit::Market::ws_url)
    }

    fn default_rest_base(&self) -> &'static str {
        "https://api.bybit.com"
    }

    fn book_depth(&self, channel: &str) -> Option<u32> {
        let d = channel.strip_prefix("orderbook.")?;
        // Exact digits only (md-bybit refuses "+50" or "050" too).
        if d.is_empty() || d.starts_with('0') || !d.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        d.parse().ok()
    }

    fn plan(
        &self,
        market: &str,
        symbols: &[String],
        channels: &[String],
        depth: u32,
        first: u32,
        max: u32,
        max_topics: Option<usize>,
    ) -> Result<Vec<ConnPlan>, String> {
        bybit_adapter(market, &[], max_topics)?
            .with_conn_budget(first, max)
            .plan(symbols, channels, depth)
            .map_err(|e| e.0)
    }

    fn instruments_request(&self, market: &str, cursor: Option<&str>) -> Option<RestRequest> {
        let a = bybit_adapter(market, &[], None).ok()?;
        Some(a.instruments_request_page(cursor))
    }

    fn next_page_cursor(&self, body: &[u8]) -> Option<String> {
        md_bybit::next_page_cursor(body)
    }

    fn parse_instruments(
        &self,
        market: &str,
        body: &[u8],
        recv_ns: i64,
    ) -> Result<Vec<Instrument>, String> {
        bybit_adapter(market, &[], None)?
            .parse_instruments(body, recv_ns)
            .map_err(|e| e.reason)
    }

    fn is_trading(&self, i: &Instrument) -> bool {
        i.status == "Trading"
    }

    fn route(&self, market: &str, payload: &[u8]) -> Route {
        // The router reads only the topic; it needs no instrument rows.
        match bybit_adapter(market, &[], None) {
            Ok(a) => a.route(payload),
            Err(_) => Route::Control,
        }
    }

    fn rest_market(&self, body: &[u8]) -> Option<String> {
        let v: serde_json::Value = serde_json::from_slice(body).ok()?;
        let c = v.get("result")?.get("category")?.as_str()?;
        bybit_market(c).map(|m| m.category().to_string())
    }

    fn start_feed(
        &self,
        market: &str,
        instruments: &[Instrument],
        max_topics: Option<usize>,
        cfg: FeedConfig,
        plans: Vec<ConnPlan>,
        sinks: Sinks,
    ) -> Result<FeedHandle, String> {
        let a = Arc::new(bybit_adapter(market, instruments, max_topics)?);
        md_feed::Feed::start_with(a, cfg, plans, sinks.frames, sinks.msgs)
    }
}
