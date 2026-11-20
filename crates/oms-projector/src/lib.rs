//! The OMS projector (U14b, ADR-026): a separate process that follows the
//! journal of one OMS data directory and turns it into observations.
//!
//! - It reads the journal with [`oms_journal::Tail`]: no `LOCK`, no write,
//!   never past the durable sequence the OMS reports on `/api/status`
//!   (C-TAIL).
//! - It replays every record into its own [`oms_core::Core`] and pushes the
//!   `oms_*` and `projector_*` metrics of that state, and one VictoriaLogs
//!   line per journal event, through `obs-push`.
//! - It keeps a cursor file: the last record whose log lines the store took.
//!   After a restart it replays up to the cursor without emitting and emits
//!   from the next record (at least once; a duplicate has the same `seq`).
//! - When the OMS is dead (no status answer for `dead_after_ms` and no
//!   holder of `LOCK`), it also shows the valid frames past the durable
//!   sequence, marked `unconfirmed=true`.
//!
//! Every outside effect is behind a trait ([`StatusSource`], [`LockProbe`],
//! [`FreeSpace`]) so tests drive [`Projector::step`] with fakes and a
//! given clock.

pub mod config;
pub mod cursor;
pub mod lines;
pub mod metrics;
pub mod pnl;
pub mod probe;
pub mod projector;

pub use config::Config;
pub use probe::{FreeSpace, HttpStatus, LockProbe, OmsStatus, ProcLocks, StatVfs, StatusSource};
pub use projector::{Projector, Settings, StepReport};

/// Microseconds since the Unix epoch from the system clock.
pub fn wall_us() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_micros()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

/// The host name for log lines: `/etc/hostname`, else `"unknown"`.
pub fn host_name() -> String {
    std::fs::read_to_string("/etc/hostname")
        .ok()
        .map(|h| h.trim().to_string())
        .filter(|h| !h.is_empty())
        .unwrap_or_else(|| "unknown".into())
}
