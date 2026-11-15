//! Snapshot files.
//!
//! ```text
//! snapshot-<seq, 20 digits>.snap
//!   line 1: JSON SnapshotInfo, terminated by '\n'
//!   rest:   payload bytes (length and CRC-32 stated in line 1)
//! ```
//!
//! A snapshot is written to a temporary name, flushed, renamed into place and
//! the directory is flushed. A reader therefore sees either no file or a
//! complete one; a file that fails its checksum is skipped, never repaired.
//!
//! The caller must write a snapshot for sequence `n` only after the journal is
//! durable up to `n`. Otherwise a crash could leave a snapshot that describes
//! records the journal does not contain.

use crate::{Error, io_err, sync_dir};
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotInfo {
    pub format: u16,
    /// Application schema version of the payload.
    pub state_schema: u16,
    /// The snapshot contains the effect of every record up to this sequence.
    pub seq: u64,
    pub len: u64,
    pub crc32: u32,
    pub created_at_us: u64,
    pub writer: String,
    /// Application-level digest of the state, for cross-checking a replay.
    pub state_digest: u64,
}

pub const SNAPSHOT_FORMAT: u16 = 1;

fn snapshot_name(seq: u64) -> String {
    format!("snapshot-{seq:020}.snap")
}

fn parse_snapshot_name(name: &str) -> Option<u64> {
    let body = name.strip_prefix("snapshot-")?.strip_suffix(".snap")?;
    if body.len() != 20 {
        return None;
    }
    body.parse().ok()
}

pub fn list_snapshots(dir: &Path) -> Result<Vec<(u64, PathBuf)>, Error> {
    let mut out = Vec::new();
    if !dir.exists() {
        return Ok(out);
    }
    let rd = fs::read_dir(dir).map_err(io_err(format!("read dir {}", dir.display())))?;
    for entry in rd {
        let entry = entry.map_err(io_err("read dir entry"))?;
        if let Some(seq) = entry.file_name().to_str().and_then(parse_snapshot_name) {
            out.push((seq, entry.path()));
        }
    }
    out.sort();
    Ok(out)
}

/// Writes a snapshot atomically and returns its path.
pub fn write_snapshot(dir: &Path, mut info: SnapshotInfo, payload: &[u8]) -> Result<PathBuf, Error> {
    info.format = SNAPSHOT_FORMAT;
    info.len = payload.len() as u64;
    info.crc32 = crc32fast::hash(payload);
    let mut head = serde_json::to_vec(&info).map_err(|e| Error::Usage(format!("snapshot header: {e}")))?;
    head.push(b'\n');

    let final_path = dir.join(snapshot_name(info.seq));
    let tmp_path = dir.join(format!("{}.tmp", snapshot_name(info.seq)));
    let _ = fs::remove_file(&tmp_path);
    let mut f = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&tmp_path)
        .map_err(io_err(format!("create {}", tmp_path.display())))?;
    f.write_all(&head).map_err(io_err("write snapshot header"))?;
    f.write_all(payload).map_err(io_err("write snapshot payload"))?;
    f.sync_all().map_err(io_err("fsync snapshot"))?;
    drop(f);
    fs::rename(&tmp_path, &final_path).map_err(io_err("rename snapshot"))?;
    sync_dir(dir)?;
    Ok(final_path)
}

fn read_snapshot(path: &Path) -> Result<(SnapshotInfo, Vec<u8>), String> {
    let mut buf = Vec::new();
    File::open(path)
        .and_then(|mut f| f.read_to_end(&mut buf))
        .map_err(|e| format!("read: {e}"))?;
    let nl = buf
        .iter()
        .position(|b| *b == b'\n')
        .ok_or("no header line")?;
    let info: SnapshotInfo = serde_json::from_slice(&buf[..nl]).map_err(|e| format!("header: {e}"))?;
    if info.format != SNAPSHOT_FORMAT {
        return Err(format!("unsupported snapshot format {}", info.format));
    }
    let payload = &buf[nl + 1..];
    if payload.len() as u64 != info.len {
        return Err(format!("payload is {} bytes, header says {}", payload.len(), info.len));
    }
    if crc32fast::hash(payload) != info.crc32 {
        return Err("payload checksum mismatch".into());
    }
    Ok((info, payload.to_vec()))
}

/// Loads the newest snapshot that passes its checks. Snapshots that fail are
/// reported in the second element and left untouched.
#[allow(clippy::type_complexity)]
pub fn load_latest_snapshot(dir: &Path) -> Result<(Option<(SnapshotInfo, Vec<u8>)>, Vec<(PathBuf, String)>), Error> {
    load_snapshot_where(dir, |_, payload| Ok(payload.to_vec()))
}

/// Like [`load_latest_snapshot`], but the caller decides whether a snapshot
/// whose file is intact is also usable (state schema, owner, content digest).
/// One that `accept` refuses is skipped with its reason and the next older
/// one is tried: a snapshot is only a shortcut, never a reason to stop.
pub fn load_snapshot_where<T, F>(dir: &Path, mut accept: F) -> Result<(Option<(SnapshotInfo, T)>, Vec<(PathBuf, String)>), Error>
where
    F: FnMut(&SnapshotInfo, &[u8]) -> Result<T, String>,
{
    let mut skipped = Vec::new();
    let mut all = list_snapshots(dir)?;
    all.reverse();
    for (seq, path) in all {
        match read_snapshot(&path) {
            Ok((info, payload)) if info.seq == seq => match accept(&info, &payload) {
                Ok(v) => return Ok((Some((info, v)), skipped)),
                Err(why) => skipped.push((path, why)),
            },
            Ok(_) => skipped.push((path, "sequence in header differs from file name".into())),
            Err(why) => skipped.push((path, why)),
        }
    }
    Ok((None, skipped))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("oms-snap-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn info(seq: u64) -> SnapshotInfo {
        SnapshotInfo {
            format: 0,
            state_schema: 1,
            seq,
            len: 0,
            crc32: 0,
            created_at_us: 1,
            writer: "test".into(),
            state_digest: 9,
        }
    }

    #[test]
    fn newest_valid_snapshot_wins_and_damaged_ones_are_skipped() {
        let d = tmp("pick");
        write_snapshot(&d, info(5), b"five").unwrap();
        let p9 = write_snapshot(&d, info(9), b"nine").unwrap();
        let (got, skipped) = load_latest_snapshot(&d).unwrap();
        assert_eq!(got.unwrap().1, b"nine");
        assert!(skipped.is_empty());

        // Damage the newest payload: the older snapshot is used instead.
        let mut bytes = fs::read(&p9).unwrap();
        let n = bytes.len();
        bytes[n - 1] ^= 1;
        fs::write(&p9, &bytes).unwrap();
        let (got, skipped) = load_latest_snapshot(&d).unwrap();
        let (i, payload) = got.unwrap();
        assert_eq!((i.seq, payload.as_slice()), (5, b"five".as_slice()));
        assert_eq!(skipped.len(), 1);
        assert!(p9.exists(), "a damaged snapshot is left in place for inspection");

        // A leftover temporary file from an interrupted write is ignored.
        fs::write(d.join("snapshot-00000000000000000012.snap.tmp"), b"partial").unwrap();
        assert_eq!(load_latest_snapshot(&d).unwrap().0.unwrap().0.seq, 5);
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn truncated_snapshot_is_rejected() {
        let d = tmp("trunc");
        let p = write_snapshot(&d, info(3), b"payload").unwrap();
        let bytes = fs::read(&p).unwrap();
        for cut in 0..bytes.len() {
            fs::write(&p, &bytes[..cut]).unwrap();
            assert!(load_latest_snapshot(&d).unwrap().0.is_none(), "cut at {cut}");
        }
        fs::remove_dir_all(&d).unwrap();
    }
}
