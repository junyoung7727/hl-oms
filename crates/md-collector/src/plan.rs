//! Plan groups (spec D3): the topics of a market, traded and other on
//! separate connections, cut into calls of the adapter's planner (one book
//! depth per call), with consecutive `conn_index` values inside the market's
//! budget. Each group runs as one feed.

use std::collections::{BTreeMap, BTreeSet};

use md_core::StreamKey;
use md_core::adapter::ConnPlan;

use crate::config::{MarketConfig, TradedChannel};
use crate::registry::Venue;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    pub market: String,
    /// Holds only traded topics (kept above the disk floor).
    pub traded: bool,
    pub plans: Vec<ConnPlan>,
}

impl Group {
    pub fn topics(&self) -> Vec<StreamKey> {
        self.plans.iter().flat_map(|p| p.topics.clone()).collect()
    }
}

/// Book depth used for a channel set without a book channel (the planner
/// needs one; no book is subscribed).
const NO_BOOK_DEPTH: u32 = 50;

/// The channel sets of one symbol: its other channels with its first book
/// channel, then each further book channel alone.
fn channel_sets(venue: &dyn Venue, channels: &BTreeSet<String>) -> Vec<Vec<String>> {
    let (books, rest): (Vec<&String>, Vec<&String>) =
        channels.iter().partition(|c| venue.book_depth(c).is_some());
    let mut first: Vec<String> = rest.into_iter().cloned().collect();
    let mut sets = Vec::new();
    let mut books = books.into_iter();
    if let Some(b) = books.next() {
        first.push(b.clone());
    }
    if !first.is_empty() {
        sets.push(first);
    }
    for b in books {
        sets.push(vec![b.clone()]);
    }
    sets
}

/// The groups of one market. `symbols` is the resolved symbol list (the
/// config's list, or the trading instruments for `"all"`); `base` is the
/// market's first `conn_index`.
pub fn groups(
    venue: &dyn Venue,
    m: &MarketConfig,
    base: u32,
    traded: &[TradedChannel],
    symbols: &[String],
) -> Result<Vec<Group>, String> {
    // (symbol, channel) of every collected topic, in symbol order of `symbols`.
    let mut topics: Vec<(String, String)> = Vec::new();
    for s in symbols {
        for c in &m.channels {
            topics.push((s.clone(), c.clone()));
        }
    }
    for e in &m.extra {
        for s in &e.symbols {
            if !topics.iter().any(|(ts, tc)| ts == s && *tc == e.channel) {
                topics.push((s.clone(), e.channel.clone()));
            }
        }
    }
    let traded: BTreeSet<(String, String)> = traded
        .iter()
        .filter(|t| t.market == m.name)
        .map(|t| (t.symbol.clone(), t.channel.clone()))
        .collect();
    let mut out = Vec::new();
    let mut next = base;
    let budget_end = base
        .checked_add(m.conn_budget)
        .ok_or("conn_index past u32::MAX")?;
    for want_traded in [true, false] {
        // Per symbol (first appearance order) its channels of this side.
        let mut order: Vec<String> = Vec::new();
        let mut by_symbol: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for (s, c) in &topics {
            if traded.contains(&(s.clone(), c.clone())) != want_traded {
                continue;
            }
            if !by_symbol.contains_key(s) {
                order.push(s.clone());
            }
            by_symbol.entry(s.clone()).or_default().insert(c.clone());
        }
        // Channel set → its symbols, in symbol order.
        let mut sets: BTreeMap<Vec<String>, Vec<String>> = BTreeMap::new();
        for s in &order {
            for set in channel_sets(venue, &by_symbol[s]) {
                sets.entry(set).or_default().push(s.clone());
            }
        }
        for (channels, syms) in sets {
            let depth = channels
                .iter()
                .find_map(|c| venue.book_depth(c))
                .unwrap_or(NO_BOOK_DEPTH);
            let plans = venue
                .plan(
                    &m.name,
                    &syms,
                    &channels,
                    depth,
                    next,
                    budget_end - next,
                    m.max_topics_per_conn,
                )
                .map_err(|e| format!("market {}: {e}", m.name))?;
            next += u32::try_from(plans.len()).map_err(|_| "too many plans")?;
            out.push(Group {
                market: m.name.clone(),
                traded: want_traded,
                plans,
            });
        }
    }
    Ok(out)
}
