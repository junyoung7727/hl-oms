//! Venue-neutral market data for the collector, the archiver and the strategy.
//!
//! No network, file, clock or randomness access: the dependency list keeps
//! I/O out, as for `oms-core` (ADR-002). Prices and quantities are `i64` with a
//! per-instrument exponent, as in the lake contract (`contracts/lake/`, 1.3).

pub mod decimal;
