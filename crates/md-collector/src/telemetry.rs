//! The collector's telemetry model (U6 spec D4, R6–R9): per-topic counts,
//! receive lag, gaps and reconnects, and the summaries pushed to
//! VictoriaMetrics, every fast period (1 s) for traded topics and every slow
//! period (10 s) for the rest. Metric and label names are design §5.4.5.
//!
//! Lock order (R6): the frame and message paths take one shard lock and
//! nothing else; a summary copies what it needs from a shard, releases it,
//! then reads books. No path holds a shard lock while it reads a book.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Mutex;

use md_core::StreamKey;
use md_feed::FeedEvent;
use obs_push::Sample;

const SHARDS: usize = 16;

/// A topic of one market.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TopicKey {
    pub market: String,
    pub symbol: String,
    pub channel: String,
}

impl TopicKey {
    pub fn new(market: &str, k: &StreamKey) -> TopicKey {
        TopicKey {
            market: market.to_string(),
            symbol: k.symbol.clone(),
            channel: k.channel.clone(),
        }
    }
}

/// Which summary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Period {
    /// Traded topics and their connections.
    Fast,
    /// Every other topic and connection.
    Slow,
}

// ---------------------------------------------------------------- lag histogram

/// Bucket edges in ms: 0.1 × 1.25^i up to 60 s (about 4 % relative error at
/// the bucket's geometric middle). Bucket 0 holds lags ≤ 0.1 ms.
const LAG_MIN_MS: f64 = 0.1;
const LAG_RATIO: f64 = 1.25;
const LAG_BUCKETS: usize = 62;

fn lag_bucket(ms: f64) -> usize {
    if ms <= LAG_MIN_MS {
        return 0;
    }
    let i = ((ms / LAG_MIN_MS).ln() / LAG_RATIO.ln()).ceil();
    (i as usize).min(LAG_BUCKETS - 1)
}

/// The value reported for a bucket: the geometric middle of its edges.
fn lag_value(b: usize) -> f64 {
    if b == 0 {
        return LAG_MIN_MS;
    }
    let hi = LAG_MIN_MS * LAG_RATIO.powi(b as i32);
    hi / LAG_RATIO.sqrt()
}

#[derive(Debug, Clone)]
pub struct LagHist {
    counts: [u32; LAG_BUCKETS],
    n: u64,
}

impl Default for LagHist {
    fn default() -> Self {
        LagHist {
            counts: [0; LAG_BUCKETS],
            n: 0,
        }
    }
}

impl LagHist {
    pub fn add(&mut self, ms: f64) {
        let b = lag_bucket(ms);
        self.counts[b] = self.counts[b].saturating_add(1);
        self.n += 1;
    }

    pub fn count(&self) -> u64 {
        self.n
    }

    /// Nearest-rank quantile; `None` without samples.
    pub fn quantile(&self, q: f64) -> Option<f64> {
        if self.n == 0 {
            return None;
        }
        let rank = ((q * self.n as f64).ceil() as u64).clamp(1, self.n);
        let mut seen = 0u64;
        for (b, c) in self.counts.iter().enumerate() {
            seen += u64::from(*c);
            if seen >= rank {
                return Some(lag_value(b));
            }
        }
        Some(lag_value(LAG_BUCKETS - 1))
    }
}

// ---------------------------------------------------------------- per topic

#[derive(Debug, Clone, Default)]
struct TopicStats {
    traded: bool,
    started_ns: i64,
    last_recv_ns: Option<i64>,
    lag: LagHist,
    lag_negative_total: u64,
    gaps_total: u64,
    reconnects_total: u64,
}

#[derive(Debug, Clone, Default)]
struct ConnStats {
    traded: bool,
    msgs: u64,
    bytes: u64,
}

/// What a summary reads from a book.
#[derive(Debug, Clone, PartialEq)]
pub struct BookInfo {
    pub spread_bp: Option<f64>,
    /// In price units (multiply by `10^px_exp`).
    pub mid: Option<f64>,
    /// (bid 10, bid 50, ask 10, ask 50) in quantity units.
    pub depth: [i128; 4],
    pub stale: bool,
    pub crossed: bool,
}

impl BookInfo {
    /// What a summary needs from a book (copied under the book's read lock).
    pub fn from_book(b: &md_core::OrderBook, stale: bool) -> BookInfo {
        use md_core::BookSide::{Ask, Bid};
        BookInfo {
            spread_bp: b.spread_bp(),
            mid: b.mid(),
            depth: [
                b.depth_sum(Bid, 10),
                b.depth_sum(Bid, 50),
                b.depth_sum(Ask, 10),
                b.depth_sum(Ask, 50),
            ],
            stale,
            crossed: b.is_crossed(),
        }
    }
}

/// The books and the instrument exponents, as the collector sees them.
pub trait BookSource {
    /// `None`: not a book topic, or no connection yet.
    fn book(&self, t: &TopicKey) -> Option<BookInfo>;
    /// `(px_exp, qty_exp)` of a symbol.
    fn exps(&self, market: &str, symbol: &str) -> Option<(i8, i8)>;
}

/// The reconnect rule of R8, per (market, conn_index).
#[derive(Debug, Default)]
struct SlotLink {
    active: Option<u32>,
}

pub struct Telemetry {
    venue: String,
    shards: Vec<Mutex<BTreeMap<TopicKey, TopicStats>>>,
    /// Frames and bytes per (market, channel, conn_index) since the last
    /// summary of that connection's period.
    conns: Mutex<BTreeMap<(String, String, u32), ConnStats>>,
    /// Topics of each (market, conn_index), and the R8 state.
    slots: Mutex<BTreeMap<(String, u32), (Vec<TopicKey>, SlotLink)>>,
}

fn shard_of(t: &TopicKey) -> usize {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    t.hash(&mut h);
    (h.finish() as usize) % SHARDS
}

fn labels(kv: &[(&str, &str)]) -> BTreeMap<String, String> {
    kv.iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

impl Telemetry {
    pub fn new(venue: &str) -> Telemetry {
        Telemetry {
            venue: venue.to_string(),
            shards: (0..SHARDS).map(|_| Mutex::new(BTreeMap::new())).collect(),
            conns: Mutex::new(BTreeMap::new()),
            slots: Mutex::new(BTreeMap::new()),
        }
    }

    /// A plan slot starts (group start or restart): its topics, whether
    /// they are traded, and the start time for `md_last_msg_age_s` (R9).
    pub fn register_slot(
        &self,
        market: &str,
        conn_index: u32,
        topics: &[StreamKey],
        traded: bool,
        now_ns: i64,
    ) {
        let keys: Vec<TopicKey> = topics.iter().map(|k| TopicKey::new(market, k)).collect();
        for t in &keys {
            if let Ok(mut s) = self.shards[shard_of(t)].lock() {
                let st = s.entry(t.clone()).or_default();
                st.traded = traded;
                if st.last_recv_ns.is_none() {
                    st.started_ns = now_ns;
                }
            }
        }
        let channels: BTreeSet<String> = keys.iter().map(|t| t.channel.clone()).collect();
        if let Ok(mut c) = self.conns.lock() {
            for ch in channels {
                c.entry((market.to_string(), ch, conn_index))
                    .or_default()
                    .traded = traded;
            }
        }
        if let Ok(mut s) = self.slots.lock() {
            let e = s
                .entry((market.to_string(), conn_index))
                .or_insert_with(|| (Vec::new(), SlotLink::default()));
            e.0 = keys;
            e.1 = SlotLink::default();
        }
    }

    /// One WebSocket frame of a stream topic (from the frame sink, R7).
    pub fn frame(
        &self,
        market: &str,
        key: &StreamKey,
        conn_index: u32,
        bytes: usize,
        recv_ns: i64,
    ) {
        let t = TopicKey::new(market, key);
        if let Ok(mut s) = self.shards[shard_of(&t)].lock() {
            if let Some(st) = s.get_mut(&t) {
                st.last_recv_ns = Some(st.last_recv_ns.map_or(recv_ns, |p| p.max(recv_ns)));
            }
        }
        if let Ok(mut c) = self.conns.lock() {
            if let Some(cs) = c.get_mut(&(market.to_string(), key.channel.clone(), conn_index)) {
                cs.msgs += 1;
                cs.bytes += bytes as u64;
            }
        }
    }

    /// One message with a venue time (books, trades, tickers).
    pub fn lag(&self, market: &str, key: &StreamKey, venue_ts_ns: i64, recv_ns: i64) {
        if venue_ts_ns <= 0 {
            return;
        }
        let t = TopicKey::new(market, key);
        if let Ok(mut s) = self.shards[shard_of(&t)].lock() {
            if let Some(st) = s.get_mut(&t) {
                let ms = (recv_ns - venue_ts_ns) as f64 / 1e6;
                if ms < 0.0 {
                    st.lag_negative_total += 1;
                }
                st.lag.add(ms.max(0.0));
            }
        }
    }

    fn slot_topics(&self, market: &str, conn_index: u32) -> Vec<TopicKey> {
        self.slots
            .lock()
            .ok()
            .and_then(|s| {
                s.get(&(market.to_string(), conn_index))
                    .map(|e| e.0.clone())
            })
            .unwrap_or_default()
    }

    fn bump(&self, t: &TopicKey, f: impl Fn(&mut TopicStats)) {
        if let Ok(mut s) = self.shards[shard_of(t)].lock() {
            if let Some(st) = s.get_mut(t) {
                f(st);
            }
        }
    }

    /// A feed event of `market`. Gaps count per topic. A reconnect (R8) is
    /// the end of the slot's active connection: the first `Connected` of a
    /// slot with no active connection makes it active, `SwitchDone` moves
    /// it, and a `Disconnected` of the active one counts one reconnect for
    /// every topic of the slot. Day-change candidates and failed connect
    /// attempts are never active, so they do not count.
    pub fn event(&self, market: &str, e: &FeedEvent) {
        match e {
            FeedEvent::Gap { key, .. } => {
                self.bump(&TopicKey::new(market, key), |st| st.gaps_total += 1);
            }
            FeedEvent::Connected {
                conn_index,
                conn_id,
            } => {
                if let Ok(mut s) = self.slots.lock() {
                    if let Some(e) = s.get_mut(&(market.to_string(), *conn_index)) {
                        if e.1.active.is_none() {
                            e.1.active = Some(*conn_id);
                        }
                    }
                }
            }
            FeedEvent::SwitchDone {
                conn_index,
                to_conn_id,
                ..
            } => {
                if let Ok(mut s) = self.slots.lock() {
                    if let Some(e) = s.get_mut(&(market.to_string(), *conn_index)) {
                        e.1.active = Some(*to_conn_id);
                    }
                }
            }
            FeedEvent::Disconnected {
                conn_index,
                conn_id,
                ..
            } => {
                let ended_active = match self.slots.lock() {
                    Ok(mut s) => match s.get_mut(&(market.to_string(), *conn_index)) {
                        Some(e) if e.1.active == Some(*conn_id) => {
                            e.1.active = None;
                            true
                        }
                        _ => false,
                    },
                    Err(_) => false,
                };
                if ended_active {
                    for t in self.slot_topics(market, *conn_index) {
                        self.bump(&t, |st| st.reconnects_total += 1);
                    }
                }
            }
            _ => {}
        }
    }

    /// The samples of one period at `now_ns`. Period stats (lag, msgs,
    /// bytes) are reset by the summary that emits them.
    pub fn summary(&self, now_ns: i64, period: Period, books: &dyn BookSource) -> Vec<Sample> {
        let want_traded = period == Period::Fast;
        let ts_ms = now_ns / 1_000_000;
        let venue = self.venue.as_str();
        let mut out = Vec::new();
        let mut push = |name: &str, l: BTreeMap<String, String>, v: f64| {
            out.push(Sample {
                name: name.to_string(),
                labels: l,
                value: v,
                ts_ms,
            });
        };
        // Copy and reset under each shard lock, then read books without it.
        let mut picked: Vec<(TopicKey, TopicStats)> = Vec::new();
        for shard in &self.shards {
            let Ok(mut s) = shard.lock() else { continue };
            for (t, st) in s.iter_mut().filter(|(_, st)| st.traded == want_traded) {
                picked.push((t.clone(), st.clone()));
                st.lag = LagHist::default();
            }
        }
        picked.sort_by(|a, b| a.0.cmp(&b.0));
        for (t, st) in picked {
            let base = labels(&[
                ("venue", venue),
                ("market", &t.market),
                ("symbol", &t.symbol),
                ("channel", &t.channel),
            ]);
            if let Some(b) = books.book(&t) {
                let exps = books.exps(&t.market, &t.symbol);
                if !b.stale && !b.crossed {
                    if let Some(sp) = b.spread_bp {
                        push("md_spread_bp", base.clone(), sp);
                    }
                    if let (Some(mid), Some((px_exp, _))) = (b.mid, exps) {
                        push(
                            "md_mid_px",
                            base.clone(),
                            mid * 10f64.powi(i32::from(px_exp)),
                        );
                    }
                }
                if let Some((_, qty_exp)) = exps {
                    let scale = 10f64.powi(i32::from(qty_exp));
                    for (i, (side, levels)) in
                        [("bid", "10"), ("bid", "50"), ("ask", "10"), ("ask", "50")]
                            .into_iter()
                            .enumerate()
                    {
                        let mut l = base.clone();
                        l.insert("side".into(), side.into());
                        l.insert("levels".into(), levels.into());
                        push("md_depth_qty", l, b.depth[i] as f64 * scale);
                    }
                }
            }
            for (q, label) in [(0.5, "0.5"), (0.99, "0.99")] {
                if let Some(v) = st.lag.quantile(q) {
                    let mut l = base.clone();
                    l.insert("quantile".into(), label.into());
                    push("md_recv_lag_ms", l, v);
                }
            }
            push(
                "md_recv_lag_negative_total",
                base.clone(),
                st.lag_negative_total as f64,
            );
            push("md_gaps_total", base.clone(), st.gaps_total as f64);
            push(
                "md_reconnects_total",
                base.clone(),
                st.reconnects_total as f64,
            );
            let since = st.last_recv_ns.unwrap_or(st.started_ns);
            push(
                "md_last_msg_age_s",
                base,
                (now_ns - since).max(0) as f64 / 1e9,
            );
        }
        if let Ok(mut c) = self.conns.lock() {
            for ((market, channel, conn), cs) in
                c.iter_mut().filter(|(_, cs)| cs.traded == want_traded)
            {
                let l = labels(&[
                    ("venue", venue),
                    ("market", market),
                    ("channel", channel),
                    ("conn", &conn.to_string()),
                ]);
                push("md_msgs", l.clone(), cs.msgs as f64);
                push("md_bytes", l, cs.bytes as f64);
                cs.msgs = 0;
                cs.bytes = 0;
            }
        }
        out
    }
}
