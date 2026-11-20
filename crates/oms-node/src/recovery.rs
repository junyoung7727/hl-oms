//! Rebuilding the state at startup.
//!
//! Recovery reads files and applies recorded events. It decides nothing, has
//! no gateway and cannot send: the types it uses (`Core::replay`) have no way
//! to produce a command. What happens to sends that were outstanding when
//! the process died is decided afterwards, by the `Boot` input and by
//! reconciliation with the venue.

use crate::pipeline::{STATE_SCHEMA, SnapshotState};
use oms_core::record::{READABLE_RECORD_SCHEMAS, RECORD_SCHEMA};
use oms_core::*;
use oms_journal::{Options, TornTail, Writer};
use serde::Serialize;
use std::fs;
use std::io::{Read, Write as _};
use std::path::Path;
use std::time::Instant;

#[derive(Clone, Debug, Serialize)]
pub struct RecoveryReport {
    pub instance_id: u64,
    pub fresh: bool,
    pub snapshot_seq: Option<u64>,
    pub snapshots_skipped: Vec<(String, String)>,
    pub records_replayed: u64,
    pub last_seq: u64,
    pub last_at: Ts,
    pub torn_tail: Option<TornTailReport>,
    pub state_digest: u64,
    pub elapsed_ms: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct TornTailReport {
    pub segment: String,
    pub offset: u64,
    pub bytes: u64,
    pub why: String,
    pub saved_to: String,
}

impl From<TornTail> for TornTailReport {
    fn from(t: TornTail) -> Self {
        TornTailReport {
            segment: t.segment.display().to_string(),
            offset: t.offset,
            bytes: t.bytes,
            why: t.why,
            saved_to: t.saved_to.display().to_string(),
        }
    }
}

pub struct Recovered {
    pub core: Core,
    pub writer: Writer,
    pub report: RecoveryReport,
}

/// Reads the instance id of a data directory, creating it on first use.
fn instance_id(dir: &Path) -> Result<(u64, bool), String> {
    let path = dir.join("INSTANCE");
    if path.exists() {
        let text = fs::read_to_string(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
        let id = u64::from_str_radix(text.trim(), 16).map_err(|e| format!("{}: {e}", path.display()))?;
        return Ok((id, false));
    }
    let has_journal = !oms_journal::list_segments(dir).map_err(|e| e.to_string())?.is_empty()
        || !oms_journal::list_snapshots(dir).map_err(|e| e.to_string())?.is_empty();
    if has_journal {
        return Err(format!(
            "{} is missing but the directory holds a journal; refusing to invent a new instance id",
            path.display()
        ));
    }
    let mut bytes = [0u8; 8];
    fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut bytes))
        .map_err(|e| format!("read /dev/urandom: {e}"))?;
    let id = u64::from_le_bytes(bytes) | 1; // never zero
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|e| format!("create {}: {e}", path.display()))?;
    f.write_all(format!("{id:016x}\n").as_bytes())
        .and_then(|()| f.sync_all())
        .map_err(|e| format!("write {}: {e}", path.display()))?;
    fs::File::open(dir)
        .and_then(|d| d.sync_all())
        .map_err(|e| format!("fsync {}: {e}", dir.display()))?;
    Ok((id, true))
}

pub fn journal_options(instance_id: u64, segment_max_bytes: u64, writer: &str) -> Options {
    Options {
        segment_max_bytes,
        truncate_torn_tail: true,
        record_schema: RECORD_SCHEMA,
        readable_schemas: READABLE_RECORD_SCHEMAS.to_vec(),
        instance_id,
        writer: writer.to_string(),
    }
}

/// Loads the newest usable snapshot and replays the journal after it.
pub fn recover(dir: &Path, segment_max_bytes: u64, writer_name: &str) -> Result<Recovered, String> {
    let started = Instant::now();
    fs::create_dir_all(dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
    let (instance, fresh) = instance_id(dir)?;

    // A snapshot is only a shortcut. One that is damaged, written by another
    // state schema, owned by another instance or whose content no longer
    // matches its recorded digest is skipped, and the next older one is tried;
    // with none usable the journal is replayed from its first record.
    let (snapshot, skipped) = oms_journal::load_snapshot_where(dir, |info, payload| {
        if info.state_schema != STATE_SCHEMA {
            return Err(format!("state schema {} (this build reads {STATE_SCHEMA})", info.state_schema));
        }
        let state: SnapshotState = serde_json::from_slice(payload).map_err(|e| format!("does not parse: {e}"))?;
        if state.core.instance_id != instance {
            return Err("belongs to a different instance".into());
        }
        if state.core.digest() != info.state_digest {
            return Err("state digest differs from the one recorded when it was written".into());
        }
        Ok(state)
    })
    .map_err(|e| e.to_string())?;
    let snapshots_skipped: Vec<(String, String)> =
        skipped.into_iter().map(|(p, why)| (p.display().to_string(), why)).collect();

    let (mut core, mut last_at, snapshot_seq) = match snapshot {
        Some((info, state)) => (state.core, state.last_at, Some(info.seq)),
        None => (Core::new(instance), 0, None),
    };

    let after = snapshot_seq.unwrap_or(0);
    let opts = journal_options(instance, segment_max_bytes, writer_name);
    let mut replayed = 0u64;
    let mut last_seq = after;
    let (writer, journal) = oms_journal::recover(dir, &opts, after, |seq, _schema, payload| {
        let rec: Record = serde_json::from_slice(payload).map_err(|e| format!("record {seq} does not parse: {e}"))?;
        if rec.seq != seq {
            return Err(format!("frame {seq} holds a record numbered {}", rec.seq));
        }
        if rec.at < last_at {
            return Err(format!("record {seq} has time {} before the previous {last_at}", rec.at));
        }
        core.replay(&rec).map_err(|e| format!("record {seq}: {e}"))?;
        last_at = rec.at;
        last_seq = seq;
        replayed += 1;
        Ok(())
    })
    .map_err(|e| e.to_string())?;

    core.check_invariants()
        .map_err(|e| format!("recovered state violates an invariant: {e}"))?;

    let report = RecoveryReport {
        instance_id: instance,
        fresh,
        snapshot_seq,
        snapshots_skipped,
        records_replayed: replayed,
        last_seq: journal.last_seq.max(last_seq),
        last_at,
        torn_tail: journal.torn_tail.map(Into::into),
        state_digest: core.digest(),
        elapsed_ms: started.elapsed().as_millis() as u64,
    };
    Ok(Recovered { core, writer, report })
}

#[derive(Clone, Debug, Serialize)]
pub struct VerifyReport {
    pub records: u64,
    pub last_seq: u64,
    pub state_digest: u64,
    pub re_decided: bool,
    pub torn_tail_bytes: u64,
    pub open_orders: usize,
    pub orders_in_memory: usize,
}

/// Offline check of a data directory, without the lock and without changing
/// anything: every checksum, sequence continuity, invariants after every
/// record and, optionally, that re-deciding each recorded input on the
/// pre-state gives exactly the recorded events.
pub fn verify(dir: &Path, re_decide: bool) -> Result<VerifyReport, String> {
    if !dir.join("INSTANCE").exists() {
        return Err(format!("{} is not an OMS data directory (no INSTANCE file)", dir.display()));
    }
    let (instance, _) = instance_id(dir)?;
    let mut core = Core::new(instance);
    let mut shadow = Core::new(instance);
    let mut records = 0u64;
    let journal = oms_journal::verify_all(dir, |seq, schema, payload| {
        if !READABLE_RECORD_SCHEMAS.contains(&schema) {
            return Err(format!("record {seq} has schema {schema}"));
        }
        let rec: Record = serde_json::from_slice(payload).map_err(|e| format!("record {seq} does not parse: {e}"))?;
        if rec.seq != seq {
            return Err(format!("frame {seq} holds a record numbered {}", rec.seq));
        }
        if re_decide {
            let out = shadow
                .handle(rec.at, &rec.input)
                .map_err(|e| format!("record {seq}: {e}"))?;
            if out.events != rec.events {
                return Err(format!(
                    "record {seq}: this build decides the input differently from what was recorded"
                ));
            }
        }
        core.replay(&rec).map_err(|e| format!("record {seq}: {e}"))?;
        core.check_invariants().map_err(|e| format!("after record {seq}: {e}"))?;
        records += 1;
        Ok(())
    })
    .map_err(|e| e.to_string())?;
    if journal.first_seq > 1 {
        return Err(format!(
            "the journal starts at record {}; a full verification needs it from record 1",
            journal.first_seq
        ));
    }
    Ok(VerifyReport {
        records,
        last_seq: journal.last_seq,
        state_digest: core.digest(),
        re_decided: re_decide,
        torn_tail_bytes: journal.torn_tail.map(|t| t.bytes).unwrap_or(0),
        open_orders: core.open.len(),
        orders_in_memory: core.orders.len(),
    })
}
