//! Committing a built date (spec D7, R5). The gap file marks a finished
//! build, so it is deleted first and renamed last, each step fsynced:
//!
//! 1. delete the date's gap file, fsync its folder;
//! 2. (the build wrote and fsynced every `.tmp`);
//! 3. delete the date's L1 files the build did not produce, rename every
//!    other `.tmp`, fsync each folder;
//! 4. rename the gap file, fsync its folder.
//!
//! A crash at any step leaves no gap file, so the next run builds the date
//! again.

use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::Path;

use crate::build::{Built, tmp_path};
use crate::table::Table;

fn sync_dir(dir: &Path) -> io::Result<()> {
    fs::File::open(dir)?.sync_all()
}

fn parent(root: &Path, rel: &str) -> std::path::PathBuf {
    root.join(rel)
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| root.to_path_buf())
}

/// The final paths of every L1 file of `date` of `venue` now on disk
/// (`.tmp` files not included).
pub fn l1_files(root: &Path, venue: &str, date: &str) -> io::Result<Vec<String>> {
    let mut out = Vec::new();
    for table in Table::ALL {
        let base = format!("crypto/l1/{}/venue={venue}", table.name());
        let mut dirs = Vec::new();
        match table {
            Table::Gap => dirs.push(format!("{base}/date={date}")),
            Table::Instrument => {
                for m in list(&root.join(&base))? {
                    dirs.push(format!("{base}/{m}/date={date}"));
                }
            }
            _ => {
                for m in list(&root.join(&base))? {
                    for c in list(&root.join(format!("{base}/{m}")))? {
                        dirs.push(format!("{base}/{m}/{c}/date={date}"));
                    }
                }
            }
        }
        for d in dirs {
            for n in list(&root.join(&d))? {
                if n.ends_with(".parquet") {
                    out.push(format!("{d}/{n}"));
                }
            }
        }
    }
    out.sort();
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

/// The gap file of a date.
pub fn gap_path(venue: &str, date: &str) -> String {
    format!("crypto/l1/gap/venue={venue}/date={date}/day.parquet")
}

/// Step 1: the date counts as not built from here on.
pub fn unmark(root: &Path, venue: &str, date: &str) -> io::Result<()> {
    let p = gap_path(venue, date);
    match fs::remove_file(root.join(&p)) {
        Ok(()) => sync_dir(&parent(root, &p)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

/// Steps 3 and 4. `fail_after` stops after that many renames (tests of the
/// crash order); `None` in a run.
pub fn commit(
    root: &Path,
    venue: &str,
    built: &Built,
    fail_after: Option<usize>,
) -> io::Result<()> {
    let gap = gap_path(venue, &built.date);
    let new: BTreeSet<&str> = built.files.iter().map(|f| f.path.as_str()).collect();
    let mut dirs = BTreeSet::new();
    for old in l1_files(root, venue, &built.date)? {
        if !new.contains(old.as_str()) && old != gap {
            fs::remove_file(root.join(&old))?;
            dirs.insert(parent(root, &old));
        }
    }
    let mut renamed = 0;
    for f in built.files.iter().filter(|f| f.path != gap) {
        if fail_after == Some(renamed) {
            return Err(io::Error::other("injected failure"));
        }
        fs::rename(root.join(tmp_path(&f.path)), root.join(&f.path))?;
        dirs.insert(parent(root, &f.path));
        renamed += 1;
    }
    for d in &dirs {
        sync_dir(d)?;
    }
    if fail_after == Some(renamed) {
        return Err(io::Error::other("injected failure"));
    }
    fs::rename(root.join(tmp_path(&gap)), root.join(&gap))?;
    sync_dir(&parent(root, &gap))
}
