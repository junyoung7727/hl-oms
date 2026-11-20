//! The cursor file: the last record whose log lines the store took.
//!
//! Format: one JSON object `{"instance_id": "<16 hex>", "seq": <u64>}`. It is
//! written to a temporary file, synced, renamed over the old one, and the
//! directory is synced, so a crash leaves the old or the new cursor, never a
//! torn one. The instance id ties the cursor to one data directory: a cursor
//! of another journal is refused rather than used.

use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::Path;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CursorFile {
    instance_id: String,
    seq: u64,
}

/// Reads the cursor. A missing file is cursor 0 (nothing emitted yet).
pub fn load(path: &Path, instance_id: u64) -> Result<u64, String> {
    let text = match fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(e) => return Err(format!("read {}: {e}", path.display())),
    };
    let c: CursorFile =
        serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    let want = format!("{instance_id:016x}");
    if c.instance_id != want {
        return Err(format!(
            "{}: cursor of instance {} but the data directory is instance {want}; move the cursor file away to start from the first record",
            path.display(),
            c.instance_id
        ));
    }
    Ok(c.seq)
}

/// Writes the cursor durably (see the module text).
pub fn store(path: &Path, instance_id: u64, seq: u64) -> Result<(), String> {
    let body = serde_json::to_string(&CursorFile {
        instance_id: format!("{instance_id:016x}"),
        seq,
    })
    .map_err(|e| e.to_string())?;
    let tmp = path.with_extension("tmp");
    let mut f = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&tmp)
        .map_err(|e| format!("open {}: {e}", tmp.display()))?;
    f.write_all(body.as_bytes())
        .and_then(|()| f.write_all(b"\n"))
        .and_then(|()| f.sync_all())
        .map_err(|e| format!("write {}: {e}", tmp.display()))?;
    fs::rename(&tmp, path)
        .map_err(|e| format!("rename {} -> {}: {e}", tmp.display(), path.display()))?;
    let dir = match path.parent() {
        Some(d) if !d.as_os_str().is_empty() => d,
        _ => Path::new("."),
    };
    File::open(dir)
        .and_then(|d| d.sync_all())
        .map_err(|e| format!("sync {}: {e}", dir.display()))
}
