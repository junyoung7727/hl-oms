//! What the projector asks the outside: the OMS status, whether some
//! process holds the journal lock, and the free space of the journal file
//! system. Each is a trait so tests inject answers.

use oms_core::WireIdForm;
use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The part of the OMS `/api/status` answer the projector reads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OmsStatus {
    /// `journal.durable_seq`: every record up to it is synced (C-TAIL).
    pub durable_seq: u64,
    /// `as_of`: the OMS wall clock when it assembled the answer, µs.
    pub as_of_us: u64,
    /// `node.wire_id_form`: how the started gateway writes client ids.
    pub wire_id_form: Option<WireIdForm>,
    /// `core.instance_id` (16 hex digits): which data directory the OMS
    /// runs on. The projector ignores an answer of another instance.
    pub instance_id: Option<u64>,
}

impl OmsStatus {
    /// Reads the fields from a status answer. `None` when `durable_seq` or
    /// `as_of` is missing.
    pub fn from_json(v: &serde_json::Value) -> Option<OmsStatus> {
        Some(OmsStatus {
            durable_seq: v.pointer("/journal/durable_seq")?.as_u64()?,
            as_of_us: v.pointer("/as_of")?.as_u64()?,
            wire_id_form: v
                .pointer("/node/wire_id_form")
                .and_then(serde_json::Value::as_str)
                .and_then(wire_id_form),
            instance_id: v
                .pointer("/core/instance_id")
                .and_then(serde_json::Value::as_str)
                .and_then(|h| u64::from_str_radix(h, 16).ok()),
        })
    }
}

/// The form named as the status writes it (`WireIdForm` is serialised in
/// snake case). An unknown name is `None`: the line then has no `wire_id`.
pub fn wire_id_form(name: &str) -> Option<WireIdForm> {
    match name {
        "hex128" => Some(WireIdForm::Hex128),
        "base32_pair" => Some(WireIdForm::Base32Pair),
        _ => None,
    }
}

pub trait StatusSource {
    /// One status query. `None`: no usable answer (refused, timed out, not
    /// 2xx, does not parse).
    fn poll(&mut self) -> Option<OmsStatus>;
}

/// `GET <url>` with a timeout, on a current-thread runtime of its own.
pub struct HttpStatus {
    url: String,
    rt: tokio::runtime::Runtime,
    client: reqwest::Client,
}

impl HttpStatus {
    pub fn new(url: &str, timeout: Duration) -> Result<HttpStatus, String> {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| format!("runtime: {e}"))?;
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .no_proxy()
            .build()
            .map_err(|e| format!("http client: {e}"))?;
        Ok(HttpStatus {
            url: url.to_string(),
            rt,
            client,
        })
    }
}

impl StatusSource for HttpStatus {
    fn poll(&mut self) -> Option<OmsStatus> {
        let fut = async {
            let resp = self.client.get(&self.url).send().await.ok()?;
            if !resp.status().is_success() {
                return None;
            }
            resp.bytes().await.ok()
        };
        let body = self.rt.block_on(fut)?;
        let v: serde_json::Value = serde_json::from_slice(&body).ok()?;
        OmsStatus::from_json(&v)
    }
}

pub trait LockProbe {
    /// Whether some process holds a lock on the journal's `LOCK` file. The
    /// probe never takes the lock itself.
    fn held(&self) -> io::Result<bool>;
}

/// Reads `/proc/locks` and looks for a lock on the inode of `<dir>/LOCK`.
///
/// A line is `N: FLOCK ADVISORY WRITE <pid> <maj>:<min>:<inode> <start>
/// <end>` (a waiter's line has `->` after `N:`). The device numbers are
/// compared only by inode: on some file systems (btrfs subvolumes) the
/// device in `/proc/locks` differs from `stat`'s, and a false "held" only
/// keeps the projector from showing unconfirmed lines (judgement).
pub struct ProcLocks {
    pub lock_file: PathBuf,
    pub proc_locks: PathBuf,
}

impl ProcLocks {
    pub fn new(data_dir: &Path) -> ProcLocks {
        ProcLocks {
            lock_file: data_dir.join("LOCK"),
            proc_locks: PathBuf::from("/proc/locks"),
        }
    }
}

/// The inodes that `/proc/locks` text lists as locked.
pub fn locked_inodes(text: &str) -> Vec<u64> {
    text.lines()
        .filter_map(|line| {
            line.split_whitespace().find_map(|tok| {
                let mut parts = tok.split(':');
                let (maj, min, ino) = (parts.next()?, parts.next()?, parts.next()?);
                if parts.next().is_some() || maj.is_empty() || min.is_empty() {
                    return None;
                }
                u64::from_str_radix(maj, 16).ok()?;
                u64::from_str_radix(min, 16).ok()?;
                ino.parse::<u64>().ok()
            })
        })
        .collect()
}

impl LockProbe for ProcLocks {
    fn held(&self) -> io::Result<bool> {
        let ino = match std::fs::metadata(&self.lock_file) {
            Ok(m) => m.ino(),
            // No LOCK file: no OMS has opened this directory.
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(e) => return Err(e),
        };
        let text = std::fs::read_to_string(&self.proc_locks)?;
        Ok(locked_inodes(&text).contains(&ino))
    }
}

pub trait FreeSpace {
    /// Bytes available to this user on the file system of the journal.
    fn free_bytes(&self) -> io::Result<u64>;
}

/// `statvfs` of a directory: `f_bavail × f_frsize` (as oms-node).
pub struct StatVfs(pub PathBuf);

impl FreeSpace for StatVfs {
    fn free_bytes(&self) -> io::Result<u64> {
        let s = rustix::fs::statvfs(&self.0).map_err(io::Error::from)?;
        Ok(s.f_bavail.saturating_mul(s.f_frsize))
    }
}
