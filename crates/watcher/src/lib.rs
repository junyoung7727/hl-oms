//! The independent watcher of the OMS (U19a, [ADR-031]).
//!
//! It shares no code with the OMS: it reads the journal with its own reader,
//! built from the documented frame format (ADR-011) and the C-TAIL field
//! table, and `tools/ci/watcher_independent.sh` proves that it depends on no
//! `oms-*` or `md-*` crate. U19a1 holds the reader, the known events, the
//! decimals and the `Base32Pair` identifiers; the checks come in U19a5 on.
//!
//! [ADR-031]: ../../docs/adr/ADR-031-watcher.md

pub mod decimal;

/// The package version, printed by `watcher --version`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
