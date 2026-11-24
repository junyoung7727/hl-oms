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
//! W2; U19a7 the OMS links and the runtime; U19a8 W4; U19a9 W3 and W2's
//! fallback to W4's execution times. U19b1a (ADR-032) the `[telegram]`
//! config, the token file and the pure parts of the watcher's own Telegram
//! sender; U19b1b its sender task. U19b2a the one signed POST (the
//! cancel-all), the key rules with `[kill]` and the kill notices' texts and
//! position block (`kill`). U19b3a W5, the OMS silence check (`w5`),
//! the clock of the kill path (`clock::Steady`), the `watcher_kill` client
//! and the stopper of the OMS's unit (`stop`); nothing starts them yet.
//!
//! [ADR-031]: ../../docs/adr/ADR-031-watcher.md

pub mod alert;
pub mod clock;
pub mod config;
pub mod creds;
pub mod decimal;
pub mod engine;
pub mod events;
pub mod journal;
pub mod kill;
pub mod log;
pub mod metrics;
pub mod oms;
pub mod rest;
pub mod runtime;
pub mod sign;
pub mod start;
pub mod state;
pub mod stop;
pub mod w1;
pub mod w2;
pub mod w3;
pub mod w4;
pub mod w5;
pub mod wire_id;

/// The package version, printed by `watcher --version`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
