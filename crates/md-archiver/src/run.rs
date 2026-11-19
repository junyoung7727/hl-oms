//! One run for one venue (spec D1, D2, D7, D8, D9, D10, R5, R7): the lock,
//! the dates to build oldest first, each built and committed, then
//! retention.

use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use md_lake::time::{DAY_NS, date, date_start};
use serde_json::json;

use crate::build::{BuildError, Built, DiskCheck, Settings, build_date, tmp_path};
use crate::commit::{commit, unmark};
use crate::input::{DateState, date_state, has_l1, l0_dates};
use crate::registry;
use crate::retain::{Retained, retain};
use crate::table::Table;

/// Free bytes of the lake's file system; injectable for tests.
pub type FreeBytes = Arc<dyn Fn(&Path) -> io::Result<u64> + Send + Sync>;

/// `statvfs` of the lake root: bytes available to this user.
pub fn statvfs_free() -> FreeBytes {
    Arc::new(|p: &Path| {
        let s = rustix::fs::statvfs(p).map_err(io::Error::from)?;
        Ok(s.f_bavail.saturating_mul(s.f_frsize))
    })
}

pub struct RunArgs {
    pub lake: PathBuf,
    pub venue: String,
    pub settings: Settings,
    /// `--rebuild FROM..TO`, both dates included.
    pub rebuild: Option<(String, String)>,
    pub retain_l1_days: Option<i64>,
    pub disk_floor_bytes: u64,
    pub now_ns: i64,
    pub free: FreeBytes,
}

/// A run that did not finish: the exit code (1 retry later, 2 refused) and why.
#[derive(Debug)]
pub struct RunError {
    pub code: i32,
    pub message: String,
}

fn err(code: i32, message: impl Into<String>) -> RunError {
    RunError {
        code,
        message: message.into(),
    }
}

#[derive(Debug, Default)]
pub struct RunReport {
    pub built: Vec<Built>,
    /// Dates skipped because they (or the date before) are not day-sealed.
    pub waiting: Vec<(String, String)>,
    pub retained: Vec<Retained>,
}

fn log(v: serde_json::Value) {
    eprintln!("{v}");
}

/// Deletes every `*.parquet.tmp` of the venue below `crypto/l1/<table>/`.
fn remove_tmp(lake: &Path, venue: &str) -> io::Result<()> {
    fn walk(d: &Path) -> io::Result<()> {
        if !d.is_dir() {
            return Ok(());
        }
        for e in fs::read_dir(d)? {
            let p = e?.path();
            if p.is_dir() {
                walk(&p)?;
            } else if p.to_string_lossy().ends_with(".parquet.tmp") {
                fs::remove_file(&p)?;
            }
        }
        Ok(())
    }
    for t in Table::ALL {
        walk(&lake.join(format!("crypto/l1/{}/venue={venue}", t.name())))?;
    }
    Ok(())
}

fn remove_built_tmp(lake: &Path, venue: &str) {
    // Best effort: the next run removes what is left (spec D1).
    let _ = remove_tmp(lake, venue);
}

/// Every date from `from` to `to`, both included.
fn range(from: &str, to: &str) -> Option<Vec<String>> {
    let (a, b) = (date_start(from)?, date_start(to)?);
    if date(a) != from || date(b) != to || a > b {
        return None;
    }
    Some(
        (0..=(b - a) / DAY_NS)
            .map(|i| date(a + i * DAY_NS))
            .collect(),
    )
}

pub fn run(args: &RunArgs) -> Result<RunReport, RunError> {
    let venue = registry::venue(&args.venue)
        .ok_or_else(|| err(2, format!("unknown venue {}", args.venue)))?;
    let rebuild = match &args.rebuild {
        Some((a, b)) => Some(range(a, b).ok_or_else(|| err(2, format!("bad --rebuild {a}..{b}")))?),
        None => None,
    };
    let fs = crate::ro::ReadOnlyFs::new(&args.lake);
    // Refusals first, so that a refused run writes nothing (not even the lock).
    let dates = l0_dates(fs.as_ref(), venue.name()).map_err(|e| err(1, e.to_string()))?;
    for d in &dates {
        if let Err(crate::input::InputError::Refuse(r)) = date_state(fs.as_ref(), venue.name(), d) {
            return Err(err(2, r.0));
        }
    }
    let todo: Vec<String> = match &rebuild {
        Some(r) => {
            let with_l0: Vec<String> = r.iter().filter(|d| dates.contains(d)).cloned().collect();
            if with_l0.is_empty() {
                return Err(err(2, "no date of --rebuild has L0"));
            }
            with_l0
        }
        None => dates
            .iter()
            .filter(|d| !has_l1(fs.as_ref(), venue.name(), d))
            .cloned()
            .collect(),
    };
    let l1 = args.lake.join("crypto/l1");
    fs::create_dir_all(&l1).map_err(|e| err(1, e.to_string()))?;
    let lock = File::create(l1.join(format!(".md-archiver-{}.lock", venue.name())))
        .map_err(|e| err(1, e.to_string()))?;
    match lock.try_lock() {
        Ok(()) => {}
        Err(fs::TryLockError::WouldBlock) => {
            return Err(err(
                1,
                format!("another md-archiver run of {} holds the lock", venue.name()),
            ));
        }
        Err(fs::TryLockError::Error(e)) => return Err(err(1, e.to_string())),
    }
    remove_tmp(&args.lake, venue.name()).map_err(|e| err(1, e.to_string()))?;
    let mut report = RunReport::default();
    for d in todo {
        let sealed_bytes = match date_state(fs.as_ref(), venue.name(), &d) {
            Ok(DateState::Sealed(f)) => f.iter().map(|f| f.bytes).sum::<u64>(),
            Ok(DateState::NotSealed(why)) => {
                log(json!({"date": d, "waiting": why}));
                if rebuild.is_some() {
                    return Err(err(1, format!("{d} is not day-sealed: {why}")));
                }
                report.waiting.push((d.clone(), why));
                continue;
            }
            Ok(DateState::NoL0) => continue,
            Err(e) => return Err(err(1, e.to_string())),
        };
        // R7: room for the date's L1 (at most its L0 bytes) above the floor.
        let free = (args.free)(&args.lake).map_err(|e| err(1, e.to_string()))?;
        if free < args.disk_floor_bytes.saturating_add(sealed_bytes) {
            return Err(err(
                1,
                format!(
                    "{d}: {free} bytes free, below the floor {} + {sealed_bytes} for the build",
                    args.disk_floor_bytes
                ),
            ));
        }
        let mut settings = args.settings.clone();
        let (free_fn, lake, floor) = (args.free.clone(), args.lake.clone(), args.disk_floor_bytes);
        settings.disk_check = Some(DiskCheck(Arc::new(move || {
            let f = free_fn(&lake).map_err(|e| e.to_string())?;
            if f < floor {
                Err(format!("{f} bytes free, below the floor {floor}"))
            } else {
                Ok(())
            }
        })));
        if rebuild.is_some() {
            unmark(&args.lake, venue.name(), &d).map_err(|e| err(1, e.to_string()))?;
        }
        let started = std::time::Instant::now();
        let built = match build_date(&args.lake, fs.as_ref(), venue, &d, &settings) {
            Ok(b) => b,
            Err(e) => {
                remove_built_tmp(&args.lake, venue.name());
                match e {
                    BuildError::Wait(why) => {
                        log(json!({"date": d, "waiting": why}));
                        if rebuild.is_some() {
                            return Err(err(1, format!("{d}: {why}")));
                        }
                        report.waiting.push((d.clone(), why));
                        continue;
                    }
                    BuildError::Refuse(r) => return Err(err(2, r.0)),
                    other => return Err(err(1, format!("{d}: {other}"))),
                }
            }
        };
        if let Err(e) = commit(&args.lake, venue.name(), &built, None) {
            remove_built_tmp(&args.lake, venue.name());
            return Err(err(1, format!("{d}: commit: {e}")));
        }
        let mut rows = serde_json::Map::new();
        for f in &built.files {
            let n = rows
                .entry(f.table.name())
                .or_insert(json!(0))
                .as_u64()
                .unwrap_or(0);
            rows.insert(f.table.name().to_string(), json!(n + f.rows));
        }
        log(json!({
            "date": d,
            "files": built.files.len(),
            "rows": rows,
            "gap_rows": built.gap_rows_by_kind,
            "unattributed_trade_drops": built.unattributed_trade_drops,
            "seconds": started.elapsed().as_secs_f64(),
        }));
        if built.unattributed_trade_drops > 0 {
            log(
                json!({"date": d, "warning": "trade rows dropped with no handover row of their connection pair",
                       "rows": built.unattributed_trade_drops}),
            );
        }
        report.built.push(built);
    }
    if let Some(n) = args.retain_l1_days {
        report.retained = retain(&args.lake, fs.as_ref(), venue.name(), n, args.now_ns)
            .map_err(|e| err(1, format!("retention: {e}")))?;
        for r in &report.retained {
            log(json!({"date": r.date, "retained": r.removed, "kept": r.kept_reason}));
        }
    }
    log(
        json!({"run": "done", "built": report.built.len(), "waiting": report.waiting.len(),
               "peak_rss_kb": peak_rss_kb()}),
    );
    drop(lock);
    Ok(report)
}

/// VmHWM of this process (Linux), for the run's last log line.
fn peak_rss_kb() -> Option<u64> {
    let s = fs::read_to_string("/proc/self/status").ok()?;
    s.lines()
        .find_map(|l| l.strip_prefix("VmHWM:"))?
        .trim()
        .trim_end_matches("kB")
        .trim()
        .parse()
        .ok()
}

/// For tests of the crash order: the `.tmp` name of a final path.
pub fn tmp_of(path: &str) -> String {
    tmp_path(path)
}
