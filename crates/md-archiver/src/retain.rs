//! `--retain-l1-days N` (spec D9, R11): the `book_msg`, `trade` and
//! `ticker` partitions of a date older than N days are deleted, only when
//! the date's L0 is day-sealed and its audit result is clean. The `gap` and
//! `instrument` day files stay; the gap file keeps the date marked as built.
//! Nothing under `crypto/l0/` or `crypto/audit/` is touched.

use std::fs;
use std::io;
use std::path::Path;

use md_lake::fs::Fs;
use md_lake::time::{DAY_NS, date, day_start};
use serde_json::Value;

use crate::input::{CONTRACT_VER, DateState, date_state, has_l1, l0_dates};

/// What retention did with one date older than the limit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Retained {
    pub date: String,
    /// Date folders deleted, lake-relative.
    pub removed: Vec<String>,
    /// Why the date was kept, when it was.
    pub kept_reason: Option<String>,
}

/// The audit result of a date, checked (contract 5.3): `Ok(())` only for a
/// complete JSON file of this contract, venue and date with status `clean`.
fn audit_clean(lake: &Path, venue: &str, d: &str) -> Result<(), String> {
    let p = lake.join(format!("crypto/audit/venue={venue}/date={d}.json"));
    let text = fs::read(&p).map_err(|e| format!("no audit result ({e})"))?;
    let v: Value =
        serde_json::from_slice(&text).map_err(|_| "the audit result is not JSON".to_string())?;
    match v["contract_ver"].as_i64() {
        Some(c) if c <= CONTRACT_VER => {}
        other => return Err(format!("audit contract_ver {other:?}")),
    }
    if v["venue"] != venue || v["date"] != d {
        return Err("the audit result is of another venue or date".into());
    }
    match v["status"].as_str() {
        Some("clean") => Ok(()),
        other => Err(format!("audit status {other:?}")),
    }
}

/// The `book_msg`, `trade` and `ticker` date folders of `d`.
fn large_partitions(lake: &Path, venue: &str, d: &str) -> io::Result<Vec<String>> {
    let mut out = Vec::new();
    for table in ["book_msg", "trade", "ticker"] {
        let base = format!("crypto/l1/{table}/venue={venue}");
        for m in list(&lake.join(&base))? {
            for c in list(&lake.join(format!("{base}/{m}")))? {
                let dir = format!("{base}/{m}/{c}/date={d}");
                if lake.join(&dir).is_dir() {
                    out.push(dir);
                }
            }
        }
    }
    Ok(out)
}

fn list(dir: &Path) -> io::Result<Vec<String>> {
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut v: Vec<String> = fs::read_dir(dir)?
        .map(|e| e.map(|e| e.file_name().to_string_lossy().into_owned()))
        .collect::<io::Result<_>>()?;
    v.sort();
    Ok(v)
}

pub fn retain(
    lake: &Path,
    lfs: &dyn Fs,
    venue: &str,
    days: i64,
    now_ns: i64,
) -> io::Result<Vec<Retained>> {
    let cutoff = date(day_start(now_ns) - days.max(0) * DAY_NS);
    let mut out = Vec::new();
    let dates = l0_dates(lfs, venue)?;
    for d in dates.iter().filter(|d| d.as_str() < cutoff.as_str()) {
        if !has_l1(lfs, venue, d) {
            continue;
        }
        let parts = large_partitions(lake, venue, d)?;
        if parts.is_empty() {
            continue;
        }
        let sealed = matches!(date_state(lfs, venue, d), Ok(DateState::Sealed(_)));
        let check = if sealed {
            audit_clean(lake, venue, d)
        } else {
            Err("L0 is not day-sealed".into())
        };
        match check {
            Ok(()) => {
                for p in &parts {
                    fs::remove_dir_all(lake.join(p))?;
                    if let Some(parent) = lake.join(p).parent() {
                        fs::File::open(parent)?.sync_all()?;
                    }
                }
                out.push(Retained {
                    date: d.clone(),
                    removed: parts,
                    kept_reason: None,
                });
            }
            Err(why) => out.push(Retained {
                date: d.clone(),
                removed: Vec::new(),
                kept_reason: Some(why),
            }),
        }
    }
    Ok(out)
}
