//! Venue-neutral market data for the collector, the archiver and the strategy.
//!
//! No network, file, clock or randomness access: the dependency list keeps
//! I/O out, as for `oms-core` (ADR-002). Prices and quantities are `i64` with a
//! per-instrument exponent, as in the lake contract (`contracts/lake/`, 1.3).

// No clock call (clippy.toml), and an exception only as
// `#[expect(lint, reason = "..")]` (ADR-033 part 3).
#![deny(
    clippy::disallowed_methods,
    clippy::allow_attributes,
    clippy::allow_attributes_without_reason
)]

pub mod adapter;
pub mod book;
pub mod conformance;
pub mod decimal;
pub mod seq;
pub mod types;

pub use book::{BookSide, OrderBook};
pub use seq::{GapDetector, GapEvent, SeqRule};
pub use types::{BookUpdate, Control, Level, Normalized, Side, StreamKey, Ticker, Trade};
