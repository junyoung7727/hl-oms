//! `oms-export`: the lake's trading tables from the OMS journal (unit U15,
//! ADR-027, lake contract section 4).
//!
//! One run reads the journal from record 1 up to the durable sequence of
//! the OMS that owns it, maps every record to rows, and writes the files of
//! every complete UTC date. A file is replaced only when its bytes change.
//! The journal is never locked or written.

pub mod config;
pub mod rows;
pub mod tables;

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use md_archiver::parquet_out::{Cell, SchemaWriter, WriteSettings};
use md_archiver::table::TableSchema;
use oms_core::Record;
use oms_core::record::READABLE_RECORD_SCHEMAS;
use oms_journal::Tail;
use oms_projector::StatusSource;
use oms_projector::projector::read_instance_id;
use rows::{Mapper, Row, V};
use serde::Serialize;

pub use config::Config;

#[derive(Clone, Debug)]
pub struct Settings {
    /// The OMS data directory (holds `INSTANCE`, `LOCK` and the segments).
    pub data_dir: PathBuf,
    /// The lake root; files go to `crypto/trading/<table>/date=<date>/`.
    pub lake_root: PathBuf,
    pub venue: String,
    pub market: String,
    /// Records per tail read.
    pub batch: usize,
}

/// What a run did, printed as one JSON line.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Summary {
    /// The durable sequence the OMS reported.
    pub durable_seq: u64,
    /// The last record read (at most `durable_seq`).
    pub read_to_seq: u64,
    /// The UTC date of the last record read: not complete, not written.
    pub open_date: Option<String>,
    /// Files written because they were missing or their bytes changed.
    pub written: Vec<String>,
    /// Files whose bytes were already the same.
    pub unchanged: usize,
    /// Rows of the complete dates, per table: orders, fills, events.
    pub rows: [usize; 3],
}

#[derive(Debug, PartialEq, Eq)]
pub enum ExportError {
    /// No usable status answer of the OMS that owns this journal: nothing
    /// is known to be durable, so nothing is written.
    NoReadLimit(String),
    Failed(String),
}

impl std::fmt::Display for ExportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExportError::NoReadLimit(e) => write!(f, "no read limit: {e}"),
            ExportError::Failed(e) => write!(f, "{e}"),
        }
    }
}

fn failed(e: impl std::fmt::Display) -> ExportError {
    ExportError::Failed(e.to_string())
}

/// UTC date of a journal time in µs.
fn date_of(at_us: u64) -> Result<String, ExportError> {
    let ns = i64::try_from(at_us)
        .ok()
        .and_then(|us| us.checked_mul(1000))
        .ok_or_else(|| failed(format!("record time {at_us} µs does not fit ns")))?;
    Ok(md_lake::time::date(ns))
}

/// Rows of one date, per table in the order of [`tables::all`].
type DateRows = [Vec<Row>; 3];

/// Whether the OMS's `node.venue` (`sim`, `hyperliquid:testnet`, …) is the
/// configured lake venue on its live network: the venue alone or
/// `<venue>:mainnet`. A testnet or simulated journal is not the venue's
/// trading.
pub fn venue_matches(node_venue: &str, venue: &str) -> bool {
    node_venue == venue || node_venue.strip_suffix(":mainnet") == Some(venue)
}

pub fn export(s: &Settings, status: &mut dyn StatusSource) -> Result<Summary, ExportError> {
    let instance_id = read_instance_id(&s.data_dir).map_err(ExportError::Failed)?;
    let st = status
        .poll()
        .ok_or_else(|| ExportError::NoReadLimit("the OMS status gave no usable answer".into()))?;
    if st.instance_id != Some(instance_id) {
        return Err(ExportError::NoReadLimit(format!(
            "the status answer is of instance {:?}, the journal is of {instance_id:016x}",
            st.instance_id.map(|i| format!("{i:016x}"))
        )));
    }
    let node_venue = st
        .venue
        .as_deref()
        .ok_or_else(|| ExportError::NoReadLimit("the status answer has no node.venue".into()))?;
    if !venue_matches(node_venue, &s.venue) {
        return Err(failed(format!(
            "the OMS runs on {node_venue}, the configuration says {}: refusing to write it as that venue's trading",
            s.venue
        )));
    }
    let _lock = lock(&s.lake_root).map_err(failed)?;
    let mut sum = Summary {
        durable_seq: st.durable_seq,
        ..Summary::default()
    };
    let mut mapper = Mapper::new(&s.venue, &s.market, instance_id, st.wire_id_form);
    // Only the open date's rows are held: record times never go back
    // (oms-node `core_loop`), so a record on a later date completes it.
    let mut open: Option<(String, DateRows)> = None;
    let mut tail = Tail::new(&s.data_dir, 0);
    loop {
        let batch = tail
            .read(st.durable_seq, s.batch.max(1))
            .map_err(|e| failed(format!("journal tail: {e:?}")))?;
        if batch.is_empty() {
            break;
        }
        for tr in batch {
            if !READABLE_RECORD_SCHEMAS.contains(&tr.record_schema) {
                return Err(failed(format!(
                    "record {} has record schema {}, this build reads {:?}",
                    tr.seq, tr.record_schema, READABLE_RECORD_SCHEMAS
                )));
            }
            let rec: Record = serde_json::from_slice(&tr.payload)
                .map_err(|e| failed(format!("record {} does not parse: {e}", tr.seq)))?;
            let raw: serde_json::Value = serde_json::from_slice(&tr.payload)
                .map_err(|e| failed(format!("record {} does not parse: {e}", tr.seq)))?;
            if rec.seq != tr.seq {
                return Err(failed(format!("frame {} holds record {}", tr.seq, rec.seq)));
            }
            let rr = mapper
                .record(&rec, &raw, tr.record_schema)
                .map_err(failed)?;
            let date = date_of(rec.at)?;
            match &open {
                Some((d, _)) if *d == date => {}
                Some((d, _)) if *d > date => {
                    return Err(failed(format!(
                        "record {} is on {date}, after a record on {d}: record times went back",
                        rec.seq
                    )));
                }
                _ => {
                    if let Some((d, day)) = open.take() {
                        write_date(&s.lake_root, &d, &day, &mut sum).map_err(failed)?;
                    }
                    open = Some((date, DateRows::default()));
                }
            }
            if let Some((_, day)) = open.as_mut() {
                day[0].extend(rr.orders);
                day[1].extend(rr.fills);
                day[2].extend(rr.events);
            }
            sum.read_to_seq = rec.seq;
        }
    }
    if sum.read_to_seq != st.durable_seq {
        return Err(failed(format!(
            "the journal holds records up to {}, the OMS reports {} durable: records are missing",
            sum.read_to_seq, st.durable_seq
        )));
    }
    sum.open_date = open.map(|(d, _)| d);
    Ok(sum)
}

/// Holds `<lake>/crypto/trading/.oms-export.lock` for the run: two runs
/// would write the same temporary files.
fn lock(lake_root: &Path) -> Result<fs::File, String> {
    let dir = lake_root.join("crypto/trading");
    fs::create_dir_all(&dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
    let path = dir.join(".oms-export.lock");
    let f = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
        .map_err(|e| format!("open {}: {e}", path.display()))?;
    match f.try_lock() {
        Ok(()) => Ok(f),
        Err(fs::TryLockError::WouldBlock) => {
            Err(format!("another oms-export run holds {}", path.display()))
        }
        Err(fs::TryLockError::Error(e)) => Err(format!("lock {}: {e}", path.display())),
    }
}

/// Writes the files of a complete date. Every table's file is built first;
/// only then are the changed ones renamed into place, so a refused row
/// leaves the date as it was.
fn write_date(
    lake_root: &Path,
    date: &str,
    day: &DateRows,
    sum: &mut Summary,
) -> Result<(), String> {
    let mut built = Vec::new();
    for (k, (schema, rows)) in tables::all().into_iter().zip(day).enumerate() {
        if rows.is_empty() {
            continue;
        }
        let rel = format!("crypto/trading/{}/date={date}/day.parquet", schema.name);
        let path = lake_root.join(&rel);
        let tmp = build(&path, schema, rows)?;
        sum.rows[k] += rows.len();
        built.push((rel, path, tmp));
    }
    for (rel, path, tmp) in built {
        if replace_if_changed(&tmp, &path)? {
            sum.written.push(rel);
        } else {
            sum.unchanged += 1;
        }
    }
    Ok(())
}

fn cell(v: &V) -> Cell<'_> {
    match v {
        V::Null => Cell::Null,
        V::Str(s) => Cell::Str(s),
        V::Bool(b) => Cell::Bool(*b),
        V::Int(i) => Cell::Int(*i),
    }
}

fn metadata(schema: &TableSchema) -> Result<Vec<(String, String)>, String> {
    schema
        .metadata
        .iter()
        .map(|k| {
            let v = match k.as_str() {
                "contract_ver" => schema.contract_ver.to_string(),
                "export_ver" => tables::EXPORT_VER.to_string(),
                "table" => schema.name.clone(),
                other => return Err(format!("{}: unknown metadata key {other}", schema.name)),
            };
            Ok((k.clone(), v))
        })
        .collect()
}

fn sync_dir(dir: &Path) -> io::Result<()> {
    fs::File::open(dir)?.sync_all()
}

/// Writes `rows` to the temporary file next to `path` (fsynced) and
/// returns its path.
fn build(path: &Path, schema: &'static TableSchema, rows: &[Row]) -> Result<PathBuf, String> {
    let dir = path
        .parent()
        .ok_or_else(|| format!("{}: no parent", path.display()))?;
    fs::create_dir_all(dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
    let tmp = dir.join(".day.parquet.tmp");
    let mut w = SchemaWriter::create(&tmp, schema, metadata(schema)?, WriteSettings::default())
        .map_err(|e| format!("{}: {e}", tmp.display()))?;
    for r in rows {
        let cells: Vec<Cell> = r.cells.iter().map(cell).collect();
        w.push(&cells)
            .map_err(|e| format!("{}: {e}", path.display()))?;
    }
    w.finish(Vec::new())
        .map_err(|e| format!("{}: {e}", tmp.display()))?;
    Ok(tmp)
}

/// Replaces `path` by `tmp` when the bytes differ (rename, then directory
/// fsync); otherwise removes `tmp`. True when `path` was (re)written.
fn replace_if_changed(tmp: &Path, path: &Path) -> Result<bool, String> {
    let new = fs::read(tmp).map_err(|e| format!("read {}: {e}", tmp.display()))?;
    let same = match fs::read(path) {
        Ok(old) => old == new,
        Err(e) if e.kind() == io::ErrorKind::NotFound => false,
        Err(e) => return Err(format!("read {}: {e}", path.display())),
    };
    if same {
        fs::remove_file(tmp).map_err(|e| format!("remove {}: {e}", tmp.display()))?;
        return Ok(false);
    }
    fs::rename(tmp, path).map_err(|e| format!("rename to {}: {e}", path.display()))?;
    let dir = path
        .parent()
        .ok_or_else(|| format!("{}: no parent", path.display()))?;
    sync_dir(dir).map_err(|e| format!("fsync {}: {e}", dir.display()))?;
    Ok(true)
}
