//! The independent watcher of the OMS (U19a, [ADR-031]).
//!
//! It shares no code with the OMS: it reads the journal with its own reader,
//! built from the documented frame format (ADR-011) and the C-TAIL field
//! table, and `tools/ci/watcher_independent.sh` proves that it depends on no
//! `oms-*` or `md-*` crate. U19a1 holds the reader, the known events, the
//! decimals and the `Base32Pair` identifiers; U19a2 the journal state
//! (orders, open set, fills, positions, holds); U19a3 the Bybit signer and
//! client, the request guard, the clock and the metrics export; U19a4 the
//! config; U19a4b the start checks; U19a5 the check engine and W1; U19a6
//! W2; the other checks come in U19a8 on.
//!
//! [ADR-031]: ../../docs/adr/ADR-031-watcher.md

pub mod clock;
pub mod config;
pub mod creds;
pub mod decimal;
pub mod engine;
pub mod events;
pub mod journal;
pub mod log;
pub mod metrics;
pub mod oms;
pub mod rest;
pub mod sign;
pub mod start;
pub mod state;
pub mod w1;
pub mod w2;
pub mod wire_id;

/// The package version, printed by `watcher --version`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
