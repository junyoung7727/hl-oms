//! The state of one connection's topics, shared between its task and readers.

use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::AtomicBool;
use std::sync::{Mutex, RwLock};
use std::time::Instant;

use md_core::adapter::ConnPlan;
use md_core::{BookUpdate, GapDetector, OrderBook, SeqRule, StreamKey, Ticker};

/// The order book state an order was decided on (lake contract, journal
/// `Order.market_ref`): the fields of the last applied message of a book.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarketRef {
    pub venue: String,
    pub market: String,
    pub symbol: String,
    pub channel: String,
    pub update_id: Option<i64>,
    pub venue_ts_ns: i64,
    pub recv_ns: i64,
}

/// A consistent read of one book: all fields come from the same lock.
#[derive(Debug, Clone, Copy)]
pub struct BookView<'a> {
    pub book: &'a OrderBook,
    pub market_ref: Option<&'a MarketRef>,
    pub stale: bool,
    pub conn_id: u32,
}

/// How a REST resync of one topic is going.
#[derive(Debug, Default)]
pub(crate) struct RestSync {
    pub buffer: VecDeque<BookUpdate>,
    pub attempts: u32,
    /// `update_id` of the applied REST snapshot; `None` while it is fetched.
    pub snapshot: Option<i64>,
}

#[derive(Debug)]
pub(crate) struct TopicState {
    pub book: OrderBook,
    pub detector: GapDetector,
    pub last_ref: Option<MarketRef>,
    pub last_applied: Option<Instant>,
    pub has_book: bool,
    pub resyncing: bool,
    pub resync_started: Option<Instant>,
    pub rest: Option<RestSync>,
    /// Start times of the recent resyncs (spec F2).
    pub resyncs: VecDeque<Instant>,
}

impl TopicState {
    pub fn stale(&self, now: Instant, stale_after: std::time::Duration) -> bool {
        !self.has_book
            || self.resyncing
            || self
                .last_applied
                .is_none_or(|t| now.saturating_duration_since(t) > stale_after)
    }
}

/// One connection: its number, plan slot and the state of its topics.
#[derive(Debug)]
pub(crate) struct ConnState {
    pub conn_id: u32,
    pub plan: ConnPlan,
    /// Book topics only (adapter `is_book`).
    pub books: BTreeMap<StreamKey, RwLock<TopicState>>,
    pub tickers: BTreeMap<StreamKey, Mutex<Option<Ticker>>>,
    /// Non-book topics that have had a frame (switch readiness, spec F1).
    pub seen: BTreeMap<StreamKey, AtomicBool>,
    /// Connected and subscribed.
    pub connected: AtomicBool,
    pub subscribed_at: Mutex<Option<Instant>>,
    /// Reads and the message sink use the active connection of a slot.
    pub active: AtomicBool,
    /// A snapshot was applied on this connection (resets the backoff).
    pub applied_any: AtomicBool,
    /// Signalled after a snapshot or a bridge was applied (switch readiness).
    pub ready: tokio::sync::Notify,
    /// The connection ended: all its books are stale.
    pub closed: AtomicBool,
}

impl ConnState {
    pub fn new(
        conn_id: u32,
        plan: ConnPlan,
        is_book: impl Fn(&StreamKey) -> bool,
        rule: impl Fn(&StreamKey) -> SeqRule,
    ) -> Self {
        let depth = usize::try_from(plan.depth).ok();
        let mut books = BTreeMap::new();
        let mut tickers = BTreeMap::new();
        for key in &plan.topics {
            if is_book(key) {
                books.insert(
                    key.clone(),
                    RwLock::new(TopicState {
                        book: OrderBook::new(depth),
                        detector: GapDetector::new(rule(key)),
                        last_ref: None,
                        last_applied: None,
                        has_book: false,
                        resyncing: false,
                        resync_started: None,
                        rest: None,
                        resyncs: VecDeque::new(),
                    }),
                );
            } else {
                tickers.insert(key.clone(), Mutex::new(None));
            }
        }
        Self {
            conn_id,
            plan,
            books,
            seen: tickers
                .keys()
                .map(|k| (k.clone(), AtomicBool::new(false)))
                .collect(),
            tickers,
            connected: AtomicBool::new(false),
            subscribed_at: Mutex::new(None),
            active: AtomicBool::new(false),
            applied_any: AtomicBool::new(false),
            ready: tokio::sync::Notify::new(),
            closed: AtomicBool::new(false),
        }
    }

    /// Ready to take over a slot (spec F1): connected, every book topic has
    /// an applied book and every other topic has had a frame.
    pub fn all_books_ready(&self) -> bool {
        use std::sync::atomic::Ordering;
        self.connected.load(Ordering::Acquire)
            && !self.closed.load(Ordering::Acquire)
            && self.seen.values().all(|s| s.load(Ordering::Acquire))
            && self.books.values().all(|t| {
                t.read()
                    .map(|t| t.has_book && !t.resyncing)
                    .unwrap_or(false)
            })
    }
}
