//! The market-data collector (design §6.2, unit U5b): one process per venue
//! that writes every received frame to the lake's stage-1 files, and the
//! `md-lake` commands that seal and verify them.

pub mod config;
pub mod disk;
pub mod ingest;
pub mod notes;
pub mod plan;
pub mod registry;

/// CLOCK_REALTIME in ns.
pub fn wall_ns() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_nanos()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

/// The host name for L0 headers: `/etc/hostname`, else `"unknown"`.
pub fn host_name() -> String {
    std::fs::read_to_string("/etc/hostname")
        .ok()
        .map(|h| h.trim().to_string())
        .filter(|h| !h.is_empty())
        .unwrap_or_else(|| "unknown".into())
}
