//! Free space of the lake's file system (spec D8).

use std::io;
use std::path::PathBuf;

pub trait FreeSpace: Send + Sync {
    fn free_bytes(&self) -> io::Result<u64>;
}

/// `statvfs` of a directory (as oms-node): bytes available to this user.
pub struct StatVfs(pub PathBuf);

impl FreeSpace for StatVfs {
    fn free_bytes(&self) -> io::Result<u64> {
        let s = rustix::fs::statvfs(&self.0).map_err(io::Error::from)?;
        Ok(s.f_bavail.saturating_mul(s.f_frsize))
    }
}
