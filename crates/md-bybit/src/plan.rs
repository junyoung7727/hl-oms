//! The connection plan and subscribe frames, within Bybit's documented limits:
//! args of one public connection ≤ 21,000 characters, 500 new connections per
//! 5 minutes counted per WebSocket domain (all markets of `stream.bybit.com`
//! together), spot ≤ 10 args per subscribe request.

use std::collections::BTreeSet;

use md_core::StreamKey;
use md_core::adapter::{ConnPlan, PlanError};

/// Bybit's args limit of one public connection, read as a sum over the
/// connection (the stricter of the two readings).
pub(crate) const MAX_ARGS_CHARS: usize = 21_000;
/// The spot per-request limit, used for every market.
pub(crate) const ARGS_PER_FRAME: usize = 10;
/// Judgement, not a model: 24 connections × ≈212 topics ran 5 minutes with no
/// reconnect (design facts §1.1).
pub(crate) const DEFAULT_MAX_TOPICS: usize = 200;

/// Order book depths Bybit offers on linear, inverse and spot.
const DEPTHS: [u32; 4] = [1, 50, 200, 1000];

/// Characters a topic takes in `args`: the string, its two quotes and a comma.
fn arg_chars(key: &StreamKey) -> usize {
    key.channel.len() + 1 + key.symbol.len() + 3
}

fn check_channel(channel: &str, depth: u32) -> Result<(), PlanError> {
    match channel.strip_prefix("orderbook.") {
        Some(d) => match d.parse::<u32>() {
            Ok(n) if DEPTHS.contains(&n) && n == depth => Ok(()),
            Ok(n) if DEPTHS.contains(&n) => {
                Err(PlanError(format!("channel {channel} with depth {depth}")))
            }
            _ => Err(PlanError(format!(
                "Bybit has no order book channel {channel}"
            ))),
        },
        None if channel == "publicTrade" || channel == "tickers" => Ok(()),
        None => Err(PlanError(format!("channel {channel} is not supported"))),
    }
}

/// Symbols in the given order; all channels of one symbol on one connection,
/// so that one reconnect touches one symbol's streams together.
pub(crate) fn plan(
    symbols: &[String],
    channels: &[String],
    depth: u32,
    max_topics: usize,
    first_conn_index: u32,
    max_conns: u32,
) -> Result<Vec<ConnPlan>, PlanError> {
    for c in channels {
        check_channel(c, depth)?;
    }
    let mut seen = BTreeSet::new();
    let mut plans: Vec<ConnPlan> = Vec::new();
    let mut chars = 0;
    for symbol in symbols {
        if !seen.insert(symbol) {
            return Err(PlanError(format!("symbol {symbol} is listed twice")));
        }
        let topics: Vec<StreamKey> = channels
            .iter()
            .map(|c| StreamKey::new(c.clone(), symbol.clone()))
            .collect();
        let need: usize = topics.iter().map(arg_chars).sum();
        if topics.len() > max_topics || need > MAX_ARGS_CHARS {
            return Err(PlanError(format!(
                "the {} topics of {symbol} do not fit one connection",
                topics.len()
            )));
        }
        let fits = plans.last().is_some_and(|p| {
            p.topics.len() + topics.len() <= max_topics && chars + need <= MAX_ARGS_CHARS
        });
        if !fits {
            let n = u32::try_from(plans.len()).unwrap_or(u32::MAX);
            if n >= max_conns {
                return Err(PlanError(format!(
                    "the plan needs more than its budget of {max_conns} connections"
                )));
            }
            plans.push(ConnPlan {
                conn_index: first_conn_index.saturating_add(n),
                topics: Vec::new(),
                depth,
            });
            chars = 0;
        }
        chars += need;
        if let Some(p) = plans.last_mut() {
            p.topics.extend(topics);
        }
    }
    Ok(plans)
}

pub(crate) fn subscribe_frames(plan: &ConnPlan) -> Vec<String> {
    plan.topics
        .chunks(ARGS_PER_FRAME)
        .enumerate()
        .map(|(n, chunk)| {
            let args: Vec<String> = chunk
                .iter()
                .map(|k| format!("{}.{}", k.channel, k.symbol))
                .collect();
            serde_json::json!({
                "op": "subscribe",
                "req_id": format!("c{}-{n}", plan.conn_index),
                "args": args,
            })
            .to_string()
        })
        .collect()
}
