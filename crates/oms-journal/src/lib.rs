//! Append-only journal segments and snapshot files.
//!
//! The crate knows nothing about orders. It stores opaque payloads under
//! consecutive sequence numbers and answers three questions precisely:
//!
//! 1. Which records are durable? Exactly those for which `Writer::sync`
//!    returned `Ok`. Nothing else may be acted upon by the caller.
//! 2. What is on disk after a crash? A valid prefix of records, possibly
//!    followed by an incomplete tail in the last segment. `recover` keeps the
//!    prefix, preserves the tail bytes in `torn/` and truncates.
//! 3. What if the middle is damaged? `recover` refuses to start. It never
//!    skips a record.
//!
//! What it relies on (see docs/research/lmax-durability-rust.md, Part 2):
//! `fdatasync` makes written data and the file size durable; a directory
//! `fsync` makes a created or renamed entry durable; after a failed sync the
//! data may be lost even if a later sync succeeds, so a failure is final.
//! Whether the device honours a flush cannot be checked from software.

#![cfg_attr(test, allow(clippy::unwrap_used))]

mod frame;
mod segment;
mod snapshot;

pub use frame::{FRAME_HEADER_LEN, MAX_PAYLOAD};
pub use segment::{Options, Recovery, SegmentHeader, TornTail, Writer, list_segments, recover, verify_all};
pub use snapshot::{SnapshotInfo, list_snapshots, load_latest_snapshot, load_snapshot_where, write_snapshot};

use std::fmt;
use std::io;
use std::path::PathBuf;

#[derive(Debug)]
pub enum Error {
    Io {
        context: String,
        source: io::Error,
    },
    /// Another process holds the data directory.
    Locked(PathBuf),
    /// Damage that is not an incomplete tail: recovery must not proceed.
    Corrupt {
        segment: PathBuf,
        offset: u64,
        detail: String,
    },
    /// Segments or records are not consecutive.
    Gap {
        detail: String,
    },
    /// The writer failed earlier. A failed journal never accepts more data.
    Failed(String),
    /// The caller broke the append contract (wrong sequence, oversize).
    Usage(String),
    /// The record visitor rejected a record during recovery.
    Visitor(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io { context, source } => write!(f, "{context}: {source}"),
            Error::Locked(p) => write!(f, "data directory {} is locked by another process", p.display()),
            Error::Corrupt {
                segment,
                offset,
                detail,
            } => write!(f, "journal corrupt at {}:{offset}: {detail}", segment.display()),
            Error::Gap { detail } => write!(f, "journal gap: {detail}"),
            Error::Failed(why) => write!(f, "journal is in failed state: {why}"),
            Error::Usage(why) => write!(f, "journal misuse: {why}"),
            Error::Visitor(why) => write!(f, "record rejected during recovery: {why}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

pub(crate) fn io_err(context: impl Into<String>) -> impl FnOnce(io::Error) -> Error {
    let context = context.into();
    move |source| Error::Io { context, source }
}

/// Flushes a directory entry change (create, rename, unlink) to disk.
pub(crate) fn sync_dir(dir: &std::path::Path) -> Result<(), Error> {
    let d = std::fs::File::open(dir).map_err(io_err(format!("open dir {}", dir.display())))?;
    d.sync_all().map_err(io_err(format!("fsync dir {}", dir.display())))
}
