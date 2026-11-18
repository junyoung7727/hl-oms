//! Venue connection and resync driver for the collector and the strategy
//! (design §6.4, §6.7). One [`Feed`] runs the connections of one venue market
//! for an adapter: it connects, subscribes, keeps alive, reconnects, stamps
//! `recv_ns` on every frame, hands frames to an optional sink, keeps an
//! in-process book per (connection, topic), resyncs after a gap, and reports
//! stale books. The adapter does the parsing; this crate does the I/O.

mod config;
mod conn;
mod event;
mod feed;
mod net;
mod sink;
mod state;

pub use config::{ConnIds, FeedConfig};
pub use event::FeedEvent;
pub use feed::{Feed, FeedHandle};
pub use net::RestClient;
pub use sink::{FrameSink, MessageSink, Overflow, RawFrame, Src};
pub use state::{BookView, MarketRef};
