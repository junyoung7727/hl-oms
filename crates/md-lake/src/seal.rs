//! The hour seal (lake contract 2.5, 2.6): moves the open files of a closed
//! hour into the day files, losslessly, under the lock rule, in hour order.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, BinaryHeap};
use std::io;
use std::sync::Arc;

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::fs::{Fs, Held, LakeFile, parent};
use crate::paths::L0Path;
use crate::qraw::{self, Header, Record, Src};
use crate::read::{RecordIter, scan_file};
use crate::time::{self, HOUR_NS};
use crate::zst::{self, Contexts, FrameEncoder};

/// Level of every frame the sealer writes (2.1).
pub const SEAL_LEVEL: i32 = 9;

/// A topic of a venue, as the venue adapter's classifier gives it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Topic {
    pub market: String,
    pub channel: String,
    pub symbol: String,
}

pub trait Clock {
    fn now_ns(&self) -> i64;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    BySymbol,
    ByConnection,
}

impl Layout {
    fn name(self) -> &'static str {
        match self {
            Layout::BySymbol => "by_symbol",
            Layout::ByConnection => "by_connection",
        }
    }
}

/// A fault injected into the merge output, for the check's own test.
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fault {
    DropRecord(usize),
    RepeatRecord(usize),
    /// Flips one byte inside the first frame appended (a disk fault).
    CorruptAppend,
}

#[derive(Debug, Clone)]
pub struct SealSettings {
    pub venue: String,
    /// The sealer's writer name (`^[a-z0-9][a-z0-9_]*$`).
    pub sealer: String,
    pub sealer_ver: String,
    pub host: String,
    /// The first UTC hour this sealer seals (F7); earlier hours are never touched.
    pub first_hour_ns: i64,
    /// The start time used to name a new session file of the sealer; the
    /// run's start when `None`.
    pub sealer_start_ns: Option<i64>,
    /// Largest share of unroutable market-data records in an hour (default 1 %).
    pub unroutable_max_share: f64,
    /// Sum of the level-9 context sizes alive at once (default 0.5 GB).
    pub context_budget: usize,
    /// A gap between two collector sessions longer than this gives a
    /// `collector_down` note (default 10 s).
    pub session_gap_ns: i64,
    pub layout: Layout,
    pub measure_layouts: bool,
    #[doc(hidden)]
    pub fault: Option<Fault>,
}

impl SealSettings {
    pub fn new(
        venue: &str,
        sealer: &str,
        sealer_ver: &str,
        host: &str,
        first_hour_ns: i64,
    ) -> Self {
        Self {
            venue: venue.into(),
            sealer: sealer.into(),
            sealer_ver: sealer_ver.into(),
            host: host.into(),
            first_hour_ns: time::hour_start(first_hour_ns),
            sealer_start_ns: None,
            unroutable_max_share: 0.01,
            context_budget: 512 * 1024 * 1024,
            session_gap_ns: 10_000_000_000,
            layout: Layout::BySymbol,
            measure_layouts: false,
            fault: None,
        }
    }
}

#[derive(Debug)]
pub enum SealError {
    /// Another seal run of the venue holds the seal lock.
    Busy,
    /// Too many records the classifier could not route; nothing was written.
    Unroutable {
        hour: String,
        unroutable: u64,
        total: u64,
    },
    /// A done entry exists but an open file not named in its sources is there
    /// (2.5): a person must decide.
    UnknownSource {
        hour: String,
        path: String,
    },
    /// The appended records do not match the open files; no done entry.
    Check {
        hour: String,
        what: String,
    },
    Io(io::Error),
}

impl From<io::Error> for SealError {
    fn from(e: io::Error) -> Self {
        SealError::Io(e)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    /// Index of the connection slot (an open file) the topics belong to.
    pub slot: usize,
    pub topics: Vec<Topic>,
}

#[derive(Debug, Default)]
pub struct SealReport {
    /// Hours sealed by this run, in order.
    pub sealed: Vec<(String, u8)>,
    /// The locked file that made the run stop, if any.
    pub waiting_on: Option<String>,
    pub groups: Vec<Group>,
    /// The size pledged to zstd for each topic frame written.
    pub pledged: Vec<(Topic, u64)>,
    /// The record bytes of each topic counted in pass 1.
    pub topic_bytes: BTreeMap<Topic, u64>,
    pub max_live_context_bytes: usize,
    /// Dates this run day-sealed (2.7).
    pub days_sealed: Vec<String>,
}

/// Order-independent digest of a set of records: the wrapping sum of their
/// SHA-256 values, as four little-endian `u64` lanes (F4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct SetDigest([u64; 4]);

impl SetDigest {
    fn add(&mut self, bytes: &[u8]) {
        let h = Sha256::digest(bytes);
        for (i, lane) in self.0.iter_mut().enumerate() {
            let mut b = [0u8; 8];
            b.copy_from_slice(&h[i * 8..i * 8 + 8]);
            *lane = lane.wrapping_add(u64::from_le_bytes(b));
        }
    }
}

#[derive(Debug, Default, Clone)]
struct TopicStats {
    records: u64,
    bytes: u64,
    digest: SetDigest,
}

/// One open file of the hour.
#[derive(Debug)]
struct Source {
    path: String,
    collector: String,
    session_ns: i64,
    conn_id: u32,
    /// Bytes to read: the whole file, or up to its last complete block.
    end: u64,
    /// The file ended without a frame end: cut back to `end`.
    crashed: bool,
    last_recv_ns: Option<i64>,
    header: Option<Header>,
    topics: BTreeSet<Topic>,
    has_unroutable: bool,
    /// All records of the file (count, bytes, set digest).
    stat: TopicStats,
    /// The handle that holds the lock until the end of the hour.
    lock: Held,
}

/// What a target gets in this hour.
#[derive(Debug, Clone)]
struct Target {
    path: String,
    offset_before: u64,
    offset_after: u64,
    records: u64,
    sha256: String,
    sessions: Option<Vec<(String, i64, u64)>>,
}

/// A record of the merge with its sort key.
type Head = Reverse<(i64, u32, u64, usize)>;

pub(crate) fn note_payload(
    note: &str,
    writer: &str,
    conn_id: Option<u32>,
    start: i64,
    end: Option<i64>,
    detail: &Value,
) -> Vec<u8> {
    // Key order fixed by the golden notes (F9): not serde_json's sorted order.
    let conn = conn_id.map_or("null".to_string(), |c| c.to_string());
    let end = end.map_or("null".to_string(), |e| e.to_string());
    format!(
        "{{\"note\":{},\"writer\":{},\"conn_id\":{conn},\"topics\":null,\"start_recv_ns\":{start},\"end_recv_ns\":{end},\"detail\":{detail}}}",
        Value::from(note),
        Value::from(writer)
    )
    .into_bytes()
}

fn io_err(what: impl Into<String>) -> io::Error {
    io::Error::other(what.into())
}

pub(crate) fn read_whole(fs: &dyn Fs, path: &str) -> io::Result<Vec<u8>> {
    let mut f = fs.open(path)?;
    let len = f.len()? as usize;
    let mut buf = vec![0; len];
    let mut at = 0;
    while at < len {
        let n = f.read_at(at as u64, &mut buf[at..])?;
        if n == 0 {
            break;
        }
        at += n;
    }
    buf.truncate(at);
    Ok(buf)
}

/// SHA-256 (hex) and length of a file, read 1 MB at a time.
pub(crate) fn sha256_of(fs: &dyn Fs, path: &str) -> io::Result<(String, u64)> {
    let mut f = fs.open(path)?;
    let mut sha = Sha256::new();
    let mut buf = vec![0; 1 << 20];
    let mut at = 0u64;
    loop {
        let n = f.read_at(at, &mut buf)?;
        if n == 0 {
            break;
        }
        sha.update(&buf[..n]);
        at += n as u64;
    }
    Ok((hex::encode(sha.finalize()), at))
}

pub struct Sealer {
    pub(crate) fs: Arc<dyn Fs>,
    pub(crate) s: SealSettings,
}

/// What happened to one hour.
enum HourOutcome {
    Done,
    Waiting(String),
}

impl Sealer {
    pub fn new(fs: Arc<dyn Fs>, settings: SealSettings) -> Self {
        Self { fs, s: settings }
    }

    fn open_dir(&self, h: i64) -> String {
        format!(
            "crypto/l0/_open/venue={}/date={}/hour={:02}",
            self.s.venue,
            time::date(h),
            time::hour(h)
        )
    }

    fn log_path(&self, h: i64) -> String {
        L0Path::SealLog {
            venue: self.s.venue.clone(),
            date: time::date(h),
            hour: time::hour(h),
        }
        .to_path()
    }

    /// Seals every closed hour from `first_hour_ns` on, oldest first; stops
    /// at the first hour that has a locked open file.
    pub fn seal(
        &self,
        clock: &dyn Clock,
        classify: &dyn Fn(&Record) -> Option<Topic>,
    ) -> Result<SealReport, SealError> {
        let _lock = self.seal_lock()?;
        let start = clock.now_ns();
        let mut report = SealReport::default();
        let last_closed = time::hour_start(start) - HOUR_NS;
        let mut earliest = None;
        let hours = self.seal_hours(
            start,
            last_closed,
            clock,
            classify,
            &mut report,
            &mut earliest,
        );
        // A failed hour makes only its own date wait (2.7): the day seal
        // runs before the error is returned.
        let days = self.seal_days(clock, last_closed, earliest, &mut report);
        hours?;
        days?;
        Ok(report)
    }

    /// The per-venue seal lock: one seal or day seal run at a time.
    pub(crate) fn seal_lock(&self) -> Result<Held, SealError> {
        let mut lock = self
            .fs
            .open_dir(&format!("crypto/l0/_seal/venue={}", self.s.venue))?;
        if !lock.try_lock()? {
            return Err(SealError::Busy);
        }
        Ok(Held(lock))
    }

    /// The hour loop of a run; `earliest` gets the first hour it sealed.
    fn seal_hours(
        &self,
        start: i64,
        last_closed: i64,
        clock: &dyn Clock,
        classify: &dyn Fn(&Record) -> Option<Topic>,
        report: &mut SealReport,
        earliest: &mut Option<i64>,
    ) -> Result<(), SealError> {
        let mut tail: Option<(String, i64, i64)> = None;
        let mut h = self.s.first_hour_ns;
        while h <= last_closed {
            // A done hour with no open file left needs nothing: skip it
            // without reading its day frames (the tail is found again
            // before the next hour that is sealed).
            if self.is_settled(h)? {
                tail = None;
                h += HOUR_NS;
                continue;
            }
            match self.seal_hour(h, start, clock, classify, &mut tail, report)? {
                HourOutcome::Done => {
                    earliest.get_or_insert(h);
                }
                HourOutcome::Waiting(p) => {
                    report.waiting_on = Some(p);
                    break;
                }
            }
            h += HOUR_NS;
        }
        Ok(())
    }

    /// The hour has a done entry and no file in its open folder.
    fn is_settled(&self, h: i64) -> io::Result<bool> {
        if !self.fs.list(&self.open_dir(h))?.is_empty() {
            return Ok(false);
        }
        let log = self.read_log(h)?;
        Ok(log.last().and_then(|e| e["entry"].as_str()) == Some("done"))
    }

    /// The complete lines of a seal log; a torn last line is cut off.
    pub(crate) fn read_log(&self, h: i64) -> io::Result<Vec<Value>> {
        let path = self.log_path(h);
        if !self.fs.exists(&path) {
            return Ok(Vec::new());
        }
        let bytes = read_whole(self.fs.as_ref(), &path)?;
        let mut lines = Vec::new();
        let mut good = 0;
        for line in bytes.split_inclusive(|&b| b == b'\n') {
            if !line.ends_with(b"\n") {
                break;
            }
            match serde_json::from_slice::<Value>(&line[..line.len() - 1]) {
                Ok(v) => {
                    lines.push(v);
                    good += line.len();
                }
                Err(_) => break,
            }
        }
        if good < bytes.len() {
            let mut f = self.fs.open(&path)?;
            f.set_len(good as u64)?;
            f.sync()?;
        }
        Ok(lines)
    }

    fn append_log(&self, h: i64, entry: &Value) -> io::Result<()> {
        let path = self.log_path(h);
        let (mut f, created) = if self.fs.exists(&path) {
            (self.fs.open(&path)?, false)
        } else {
            (self.fs.create_new(&path)?, true)
        };
        let mut line = entry.to_string().into_bytes();
        line.push(b'\n');
        f.write_all(&line)?;
        f.sync()?;
        if created {
            self.fs.sync_dir(parent(&path))?;
        }
        Ok(())
    }

    fn base_entry(
        &self,
        kind: &str,
        h: i64,
        attempt: u64,
        at: i64,
    ) -> serde_json::Map<String, Value> {
        let v = json!({
            "entry": kind,
            "contract_ver": qraw::CONTRACT_VER,
            "venue": self.s.venue,
            "date": time::date(h),
            "hour": format!("{:02}", time::hour(h)),
            "layout": self.s.layout.name(),
            "attempt": attempt,
            "at_ns": at,
        });
        match v {
            Value::Object(m) => m,
            _ => serde_json::Map::new(),
        }
    }

    /// Takes the locks of the hour's open files; a `.tmp` file that can be
    /// locked is removed. `Err(path)` names the first locked file.
    fn lock_sources(&self, h: i64) -> io::Result<Result<Vec<(String, Held)>, String>> {
        let dir = self.open_dir(h);
        let mut held = Vec::new();
        for name in self.fs.list(&dir)? {
            let path = format!("{dir}/{name}");
            let mut f = self.fs.open(&path)?;
            if !f.try_lock()? {
                return Ok(Err(path));
            }
            let f = Held(f);
            if name.ends_with(".tmp") {
                self.fs.remove(&path)?;
                self.fs.sync_dir(&dir)?;
                continue;
            }
            held.push((path, f));
        }
        Ok(Ok(held))
    }

    #[allow(clippy::too_many_arguments)]
    fn seal_hour(
        &self,
        h: i64,
        run_start: i64,
        clock: &dyn Clock,
        classify: &dyn Fn(&Record) -> Option<Topic>,
        tail: &mut Option<(String, i64, i64)>,
        report: &mut SealReport,
    ) -> Result<HourOutcome, SealError> {
        let hour_name = format!("{} {:02}", time::date(h), time::hour(h));
        let log = self.read_log(h)?;
        let last = log.last().cloned();
        let locked = match self.lock_sources(h)? {
            Ok(l) => l,
            Err(p) => return Ok(HourOutcome::Waiting(p)),
        };
        let mut attempt = 1;
        match last.as_ref().and_then(|e| e["entry"].as_str()) {
            Some("done") => {
                if !locked.is_empty() {
                    let attempt = last.as_ref().and_then(|e| e["attempt"].as_u64());
                    let begin = log
                        .iter()
                        .rev()
                        .find(|e| e["entry"] == "begin" && e["attempt"].as_u64() == attempt);
                    let named: BTreeSet<String> = begin
                        .and_then(|b| b["sources"].as_array())
                        .into_iter()
                        .flatten()
                        .filter_map(|s| s["path"].as_str().map(str::to_owned))
                        .collect();
                    if let Some((p, _)) = locked.iter().find(|(p, _)| !named.contains(p)) {
                        return Err(SealError::UnknownSource {
                            hour: hour_name,
                            path: p.clone(),
                        });
                    }
                    // Delete only: the crash came after the done entry.
                    for (p, _) in &locked {
                        self.fs.remove(p)?;
                    }
                    self.fs.sync_dir(&self.open_dir(h))?;
                }
                self.update_tail_from_log(h, &log, tail)?;
                return Ok(HourOutcome::Done);
            }
            Some("begin") => {
                // Redo: cut every target of the begin entry back.
                let b = last.as_ref().ok_or_else(|| io_err("begin"))?;
                attempt = b["attempt"].as_u64().unwrap_or(1) + 1;
                for t in b["targets"].as_array().into_iter().flatten() {
                    let (Some(p), Some(off)) = (t["path"].as_str(), t["offset_before"].as_u64())
                    else {
                        continue;
                    };
                    if !self.fs.exists(p) {
                        continue;
                    }
                    if off == 0 {
                        self.fs.remove(p)?;
                        self.fs.sync_dir(parent(p))?;
                    } else {
                        let mut f = self.fs.open(p)?;
                        f.set_len(off)?;
                        f.sync()?;
                    }
                }
            }
            _ => {}
        }
        self.seal_fresh(h, attempt, locked, run_start, clock, classify, tail, report)?;
        report.sealed.push((time::date(h), time::hour(h)));
        Ok(HourOutcome::Done)
    }

    /// After a done hour found at the start of a run, the session tail for
    /// the gap rule comes from that hour's day frames.
    fn update_tail_from_log(
        &self,
        _h: i64,
        log: &[Value],
        tail: &mut Option<(String, i64, i64)>,
    ) -> io::Result<()> {
        let Some(done) = log.last() else {
            return Ok(());
        };
        if done["records"].as_u64().unwrap_or(0) == 0 {
            return Ok(());
        }
        // Per target: the session of its last run in this hour's frame and
        // that run's record count. by_symbol day files name their runs;
        // a by_connection file is one session's (named by its path).
        let mut runs: Vec<(String, i64, &Value, u64)> = Vec::new();
        for t in done["targets"].as_array().into_iter().flatten() {
            if let Some(lr) = t["sessions"].as_array().and_then(|r| r.last()) {
                if let (Some(c), Some(ns), Some(n)) = (
                    lr["collector"].as_str(),
                    lr["session_ns"].as_i64(),
                    lr["records"].as_u64(),
                ) {
                    runs.push((c.to_string(), ns, t, n));
                }
            } else if let Some(Ok(L0Path::DayByConnection {
                collector,
                session_ns,
                ..
            })) = t["path"].as_str().map(L0Path::parse)
            {
                if let Some(n) = t["records"].as_u64().filter(|&n| n > 0) {
                    runs.push((collector, session_ns, t, n));
                }
            }
        }
        let Some((c, ns)) = runs
            .iter()
            .map(|(c, ns, _, _)| (c.clone(), *ns))
            .max_by(|a, b| (a.1, &a.0).cmp(&(b.1, &b.0)))
        else {
            return Ok(());
        };
        let mut last_recv: Option<i64> = None;
        for (rc, rns, t, n) in &runs {
            if *rns != ns || *rc != c {
                continue;
            }
            let (Some(p), Some(after), Some(total)) = (
                t["path"].as_str(),
                t["offset_after"].as_u64(),
                t["records"].as_u64(),
            ) else {
                continue;
            };
            let n = *n;
            // The frame of this hour is the last frame ending at offset_after.
            let path = if self.fs.exists(p) {
                p.to_string()
            } else {
                L0Path::parse(p)
                    .map(|x| x.sealed().to_path())
                    .unwrap_or_default()
            };
            if !self.fs.exists(&path) {
                continue;
            }
            let mut f = self.fs.open(&path)?;
            let scan = scan_file(f.as_mut())?;
            let Some(frame) = scan.frames.iter().find(|fr| fr.end as u64 == after) else {
                continue;
            };
            let it = RecordIter::new(
                f.as_mut(),
                frame.start as u64,
                frame.end as u64,
                frame.start == 0,
            );
            for (i, r) in it.enumerate() {
                let r = r?;
                if i as u64 >= total - n {
                    last_recv = Some(last_recv.map_or(r.recv_ns, |x| x.max(r.recv_ns)));
                }
            }
        }
        if let Some(lr) = last_recv {
            *tail = Some((c, ns, lr));
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn seal_fresh(
        &self,
        h: i64,
        attempt: u64,
        locked: Vec<(String, Held)>,
        run_start: i64,
        clock: &dyn Clock,
        classify: &dyn Fn(&Record) -> Option<Topic>,
        tail: &mut Option<(String, i64, i64)>,
        report: &mut SealReport,
    ) -> Result<(), SealError> {
        let fs = self.fs.as_ref();
        let date = time::date(h);
        let hour_name = format!("{date} {:02}", time::hour(h));

        // ---- Pass 1: count per topic; recovery is decided, not yet applied.
        let mut sources: Vec<Source> = Vec::new();
        for (path, lock) in locked {
            let Ok(L0Path::Open {
                collector,
                session_ns,
                conn_id,
                ..
            }) = L0Path::parse(&path)
            else {
                return Err(SealError::Io(io_err(format!("not an open file: {path}"))));
            };
            let mut f = fs.open(&path)?;
            let scan = scan_file(f.as_mut())?;
            let (end, crashed) = match (scan.frames.as_slice(), scan.unfinished) {
                ([one], None) => (one.end as u64, false),
                ([], Some(u)) => (u.complete_blocks_end as u64, true),
                ([], None) => (0, true),
                _ => {
                    return Err(SealError::Io(io_err(format!(
                        "{path}: not one zstd stream"
                    ))));
                }
            };
            sources.push(Source {
                path,
                collector,
                session_ns,
                conn_id,
                end,
                crashed,
                last_recv_ns: None,
                header: None,
                topics: BTreeSet::new(),
                has_unroutable: false,
                stat: TopicStats::default(),
                lock,
            });
        }
        sources.sort_by(|a, b| {
            (a.session_ns, &a.collector, a.conn_id).cmp(&(b.session_ns, &b.collector, b.conn_id))
        });

        let mut stats: BTreeMap<Topic, TopicStats> = BTreeMap::new();
        let mut topic_slot: BTreeMap<Topic, usize> = BTreeMap::new();
        let mut unroutable_md = 0u64;
        let mut unroutable_all = TopicStats::default();
        let mut total = 0u64;
        // (session_ns, collector) → first recv
        let mut sessions: BTreeMap<(i64, String), i64> = BTreeMap::new();
        // (session_ns, collector) → last recv of a record that goes to a day
        // file: the session tail, the same value a later run reads back from
        // the day files (update_tail_from_log).
        let by_conn = self.s.layout == Layout::ByConnection;
        let mut day_last: BTreeMap<(i64, String), i64> = BTreeMap::new();
        for (slot, src) in sources.iter_mut().enumerate() {
            let mut f = fs.open(&src.path)?;
            let mut it = RecordIter::new(f.as_mut(), 0, src.end, true);
            for r in it.by_ref() {
                let r = r?;
                total += 1;
                let mut bytes = Vec::with_capacity(r.encoded_len());
                qraw::encode_record(&r, &mut bytes).map_err(|e| io_err(format!("{e:?}")))?;
                src.stat.records += 1;
                src.stat.bytes += bytes.len() as u64;
                src.stat.digest.add(&bytes);
                let routed = classify(&r);
                if by_conn || routed.is_some() {
                    let e = day_last
                        .entry((src.session_ns, src.collector.clone()))
                        .or_insert(r.recv_ns);
                    *e = (*e).max(r.recv_ns);
                }
                match routed {
                    Some(t) => {
                        let st = stats.entry(t.clone()).or_default();
                        st.records += 1;
                        st.bytes += bytes.len() as u64;
                        st.digest.add(&bytes);
                        topic_slot.entry(t.clone()).or_insert(slot);
                        src.topics.insert(t);
                    }
                    None => {
                        if r.src != Src::Note {
                            unroutable_md += 1;
                        }
                        unroutable_all.records += 1;
                        unroutable_all.bytes += bytes.len() as u64;
                        unroutable_all.digest.add(&bytes);
                        src.has_unroutable = true;
                    }
                }
                src.last_recv_ns = Some(r.recv_ns);
                let e = sessions
                    .entry((src.session_ns, src.collector.clone()))
                    .or_insert(r.recv_ns);
                *e = (*e).min(r.recv_ns);
            }
            src.header = it.header().cloned();
        }
        if by_conn {
            // by_connection does not classify (2.5 step 4): nothing is unroutable.
            unroutable_all = TopicStats::default();
            for src in &mut sources {
                src.has_unroutable = false;
            }
        } else if total > 0 && unroutable_md as f64 > self.s.unroutable_max_share * total as f64 {
            return Err(SealError::Unroutable {
                hour: hour_name,
                unroutable: unroutable_md,
                total,
            });
        }
        // One instruments file per market (2.2): its topic's symbol is
        // `_all`, or two symbols would share one target.
        if let Some(t) = stats
            .keys()
            .find(|t| t.channel == "instruments" && t.symbol != "_all")
        {
            return Err(SealError::Io(io_err(format!(
                "{hour_name}: instruments topic with symbol {:?}; the contract needs \"_all\"",
                t.symbol
            ))));
        }
        report
            .topic_bytes
            .extend(stats.iter().map(|(t, s)| (t.clone(), s.bytes)));

        // ---- Notes (F9 order: crash_truncate, collector_down, unroutable after).
        let mut notes: Vec<Record> = Vec::new();
        let note = |payload: Vec<u8>, conn: Option<u32>| Record {
            recv_ns: clock.now_ns(),
            src: Src::Note,
            conn_id: conn.unwrap_or(0),
            frame_no: 0,
            payload,
        };
        for src in &sources {
            if src.crashed {
                let start = src.last_recv_ns.unwrap_or(h);
                let detail = json!({"file": src.path});
                notes.push(note(
                    note_payload(
                        "crash_truncate",
                        &self.s.sealer,
                        Some(src.conn_id),
                        start,
                        None,
                        &detail,
                    ),
                    Some(src.conn_id),
                ));
            }
        }
        if sources.is_empty() {
            let detail = json!({"text": "no open file in the hour"});
            notes.push(note(
                note_payload("collector_down", &self.s.sealer, None, h, None, &detail),
                None,
            ));
        }
        if tail.is_none() {
            self.find_tail_before(h, tail)?;
        }
        for ((ns, c), first) in &sessions {
            if let Some((pc, pns, plast)) = tail.clone() {
                if (pns, pc.as_str()) != (*ns, c.as_str()) && first - plast > self.s.session_gap_ns
                {
                    let detail = json!({"prev_session_ns": pns, "next_session_ns": ns});
                    notes.push(note(
                        note_payload(
                            "collector_down",
                            &self.s.sealer,
                            None,
                            plast,
                            Some(*first),
                            &detail,
                        ),
                        None,
                    ));
                }
            }
            if let Some(last) = day_last.get(&(*ns, c.clone())) {
                *tail = Some((c.clone(), *ns, *last));
            }
        }

        // ---- Recovery is applied now (after the guard): cut crashed files back.
        for src in &mut sources {
            if src.crashed && src.lock.len()? != src.end {
                src.lock.set_len(src.end)?;
                src.lock.sync()?;
            }
        }

        // ---- Targets and the begin entry.
        let mut topic_target: BTreeMap<Topic, String> = BTreeMap::new();
        for t in stats.keys().filter(|_| !by_conn) {
            let p = if t.channel == "instruments" {
                L0Path::Instruments {
                    venue: self.s.venue.clone(),
                    market: t.market.clone(),
                    date: date.clone(),
                    partial: true,
                }
            } else {
                L0Path::DayBySymbol {
                    venue: self.s.venue.clone(),
                    market: t.market.clone(),
                    channel: t.channel.clone(),
                    date: date.clone(),
                    symbol: t.symbol.clone(),
                    partial: true,
                }
            };
            topic_target.insert(t.clone(), p.to_path());
        }
        let session_path = self.sealer_session_path(&date, run_start)?;
        let size = |p: &str| -> io::Result<u64> {
            if fs.exists(p) {
                fs.open(p)?.len()
            } else {
                Ok(0)
            }
        };
        let mut targets: Vec<Target> = Vec::new();
        for p in topic_target.values() {
            targets.push(Target {
                path: p.clone(),
                offset_before: size(p)?,
                offset_after: 0,
                records: 0,
                sha256: String::new(),
                sessions: Some(Vec::new()),
            });
        }
        let conn_ti0 = targets.len();
        if by_conn {
            for src in &sources {
                let p = L0Path::DayByConnection {
                    venue: self.s.venue.clone(),
                    date: date.clone(),
                    collector: src.collector.clone(),
                    session_ns: src.session_ns,
                    conn_id: src.conn_id,
                    partial: true,
                }
                .to_path();
                targets.push(Target {
                    path: p.clone(),
                    offset_before: size(&p)?,
                    offset_after: 0,
                    records: 0,
                    sha256: String::new(),
                    sessions: None,
                });
            }
        }
        targets.push(Target {
            path: session_path.clone(),
            offset_before: size(&session_path)?,
            offset_after: 0,
            records: 0,
            sha256: String::new(),
            sessions: None,
        });
        let mut begin = self.base_entry("begin", h, attempt, clock.now_ns());
        begin.insert(
            "sources".into(),
            Value::Array(
                sources
                    .iter()
                    .map(|s| json!({"path": s.path, "bytes": s.end}))
                    .collect(),
            ),
        );
        begin.insert(
            "targets".into(),
            Value::Array(
                targets
                    .iter()
                    .map(|t| json!({"path": t.path, "offset_before": t.offset_before}))
                    .collect(),
            ),
        );
        self.append_log(h, &Value::Object(begin))?;

        // ---- Groups: inside one connection slot, bounded by the context budget.
        let header_len = |t: &Topic| -> u64 {
            let hd = self.topic_header(t, &date, &sources, &topic_slot);
            qraw::encode_header(&hd).len() as u64
        };
        // Targets were built in topic order: topic i has target i.
        let topic_ti: BTreeMap<Topic, usize> = topic_target
            .keys()
            .cloned()
            .enumerate()
            .map(|(i, t)| (t, i))
            .collect();
        let mut frame_bytes: BTreeMap<Topic, u64> = BTreeMap::new();
        for (t, st) in stats.iter().filter(|_| !by_conn) {
            let first = targets[topic_ti[t]].offset_before == 0;
            frame_bytes.insert(t.clone(), st.bytes + if first { header_len(t) } else { 0 });
        }
        let groups = if by_conn {
            Vec::new()
        } else {
            self.make_groups(sources.len(), &topic_slot, &frame_bytes)
        };

        // ---- Merge and append, group by group.
        let contexts = Contexts::default();
        let mut merged_index = 0usize;
        let mut created: BTreeSet<String> = BTreeSet::new();
        let session_order: Vec<(i64, String)> = sessions.keys().cloned().collect();
        for g in &groups {
            let in_group: BTreeSet<&Topic> = g.topics.iter().collect();
            struct Out {
                file: Box<dyn LakeFile>,
                enc: FrameEncoder,
                buf: Vec<u8>,
                sha: Sha256,
                records: u64,
                runs: Vec<(String, i64, u64)>,
                ti: usize,
            }
            let mut outs: BTreeMap<Topic, Out> = BTreeMap::new();
            for t in &g.topics {
                let ti = topic_ti[t];
                let path = &targets[ti].path;
                let file = if targets[ti].offset_before == 0 {
                    created.insert(path.clone());
                    let f = fs.create_new(path)?;
                    fs.sync_dir(parent(path))?;
                    f
                } else {
                    fs.open(path)?
                };
                let mut enc = FrameEncoder::new(SEAL_LEVEL, Some(frame_bytes[t]), &contexts)?;
                report.pledged.push((t.clone(), frame_bytes[t]));
                let mut buf = Vec::new();
                if targets[ti].offset_before == 0 {
                    enc.write(
                        &qraw::encode_header(&self.topic_header(t, &date, &sources, &topic_slot)),
                        &mut buf,
                    )?;
                }
                outs.insert(
                    t.clone(),
                    Out {
                        file,
                        enc,
                        buf,
                        sha: Sha256::new(),
                        records: 0,
                        runs: Vec::new(),
                        ti,
                    },
                );
            }
            report.max_live_context_bytes =
                report.max_live_context_bytes.max(contexts.live_bytes());
            for (ns, c) in &session_order {
                let idx: Vec<usize> = (0..sources.len())
                    .filter(|&i| {
                        sources[i].session_ns == *ns
                            && &sources[i].collector == c
                            && sources[i].topics.iter().any(|t| in_group.contains(t))
                    })
                    .collect();
                self.merge(
                    &sources,
                    &idx,
                    |r| classify(r).filter(|t| in_group.contains(t)).is_some(),
                    &mut |r: Record| {
                        let Some(t) = classify(&r) else { return Ok(()) };
                        let o = outs
                            .get_mut(&t)
                            .ok_or_else(|| io_err("topic not in group"))?;
                        let times = match self.s.fault {
                            Some(Fault::DropRecord(n)) if n == merged_index => 0,
                            Some(Fault::RepeatRecord(n)) if n == merged_index => 2,
                            _ => 1,
                        };
                        merged_index += 1;
                        for _ in 0..times {
                            let mut bytes = Vec::with_capacity(r.encoded_len());
                            qraw::encode_record(&r, &mut bytes)
                                .map_err(|e| io_err(format!("{e:?}")))?;
                            o.sha.update(&bytes);
                            o.records += 1;
                            match o.runs.last_mut() {
                                Some((rc, rns, n)) if rc == c && *rns == *ns => *n += 1,
                                _ => o.runs.push((c.clone(), *ns, 1)),
                            }
                            o.enc.write(&bytes, &mut o.buf)?;
                            if o.buf.len() >= 1 << 20 {
                                o.file.write_all(&o.buf)?;
                                o.buf.clear();
                            }
                        }
                        Ok(())
                    },
                )?;
            }
            for (_, mut o) in outs {
                o.enc.finish(&mut o.buf)?;
                o.file.write_all(&o.buf)?;
                o.file.sync()?;
                let t = &mut targets[o.ti];
                t.offset_after = o.file.len()?;
                t.records = o.records;
                t.sha256 = hex::encode(o.sha.finalize());
                t.sessions = Some(o.runs);
            }
        }
        // ---- Layout by_connection: each open file whole, one frame per hour.
        if by_conn {
            for (k, src) in sources.iter().enumerate() {
                let ti = conn_ti0 + k;
                let path = targets[ti].path.clone();
                let new = targets[ti].offset_before == 0;
                let hb = qraw::encode_header(&self.conn_header(src, &date));
                let pledged = src.stat.bytes + if new { hb.len() as u64 } else { 0 };
                let mut file = if new {
                    created.insert(path.clone());
                    let f = fs.create_new(&path)?;
                    fs.sync_dir(parent(&path))?;
                    f
                } else {
                    fs.open(&path)?
                };
                let mut enc = FrameEncoder::new(SEAL_LEVEL, Some(pledged), &contexts)?;
                let mut buf = Vec::new();
                if new {
                    enc.write(&hb, &mut buf)?;
                }
                let mut sha = Sha256::new();
                let mut n = 0u64;
                let mut f = fs.open(&src.path)?;
                for r in RecordIter::new(f.as_mut(), 0, src.end, true) {
                    let r = r?;
                    let mut bytes = Vec::with_capacity(r.encoded_len());
                    qraw::encode_record(&r, &mut bytes).map_err(|e| io_err(format!("{e:?}")))?;
                    sha.update(&bytes);
                    n += 1;
                    enc.write(&bytes, &mut buf)?;
                    if buf.len() >= 1 << 20 {
                        file.write_all(&buf)?;
                        buf.clear();
                    }
                }
                enc.finish(&mut buf)?;
                file.write_all(&buf)?;
                file.sync()?;
                let t = &mut targets[ti];
                t.offset_after = file.len()?;
                t.records = n;
                t.sha256 = hex::encode(sha.finalize());
            }
        }
        let measure = if self.s.measure_layouts {
            let appended: u64 = targets[..targets.len() - 1]
                .iter()
                .map(|t| t.offset_after - t.offset_before)
                .sum();
            if by_conn {
                let mut fb: BTreeMap<Topic, u64> = BTreeMap::new();
                for (t, st) in &stats {
                    let hl =
                        qraw::encode_header(&self.topic_header(t, &date, &sources, &topic_slot))
                            .len() as u64;
                    fb.insert(t.clone(), st.bytes + hl);
                }
                let gs = self.make_groups(sources.len(), &topic_slot, &fb);
                let sym = self.measure_by_symbol(
                    &sources,
                    &session_order,
                    &gs,
                    &fb,
                    &date,
                    &topic_slot,
                    classify,
                )?;
                Some((sym, appended))
            } else {
                Some((appended, self.measure_by_connection(&sources, &date)?))
            }
        } else {
            None
        };
        report.groups.extend(groups);

        // ---- The sealer's session file: notes, then unroutable records.
        let si = targets.len() - 1;
        let mut sess_sha = Sha256::new();
        let mut sess_digest = SetDigest::default();
        let mut sess_records = 0u64;
        let has_any = !notes.is_empty() || unroutable_all.records > 0;
        if has_any {
            let path = targets[si].path.clone();
            let new = targets[si].offset_before == 0;
            let header = Header::session(
                &self.s.venue,
                &date,
                &self.s.sealer,
                &self.s.sealer_ver,
                &self.s.host,
                self.session_start_of(&path),
            );
            let note_bytes: u64 = notes.iter().map(|n| n.encoded_len() as u64).sum();
            let pledged = note_bytes
                + unroutable_all.bytes
                + if new {
                    qraw::encode_header(&header).len() as u64
                } else {
                    0
                };
            let mut file = if new {
                created.insert(path.clone());
                let f = fs.create_new(&path)?;
                fs.sync_dir(parent(&path))?;
                f
            } else {
                fs.open(&path)?
            };
            let mut enc = FrameEncoder::new(SEAL_LEVEL, Some(pledged), &contexts)?;
            let mut buf = Vec::new();
            if new {
                enc.write(&qraw::encode_header(&header), &mut buf)?;
            }
            let mut put =
                |r: &Record, enc: &mut FrameEncoder, buf: &mut Vec<u8>| -> io::Result<()> {
                    let mut bytes = Vec::with_capacity(r.encoded_len());
                    qraw::encode_record(r, &mut bytes).map_err(|e| io_err(format!("{e:?}")))?;
                    sess_sha.update(&bytes);
                    sess_digest.add(&bytes);
                    sess_records += 1;
                    enc.write(&bytes, buf)?;
                    if buf.len() >= 1 << 20 {
                        file.write_all(buf)?;
                        buf.clear();
                    }
                    Ok(())
                };
            for n in &notes {
                put(n, &mut enc, &mut buf)?;
            }
            for (ns, c) in &session_order {
                let idx: Vec<usize> = (0..sources.len())
                    .filter(|&i| {
                        sources[i].session_ns == *ns
                            && &sources[i].collector == c
                            && sources[i].has_unroutable
                    })
                    .collect();
                self.merge(
                    &sources,
                    &idx,
                    |r| classify(r).is_none(),
                    &mut |r: Record| put(&r, &mut enc, &mut buf),
                )?;
            }
            enc.finish(&mut buf)?;
            file.write_all(&buf)?;
            file.sync()?;
            targets[si].offset_after = file.len()?;
        } else {
            targets[si].offset_after = targets[si].offset_before;
        }
        targets[si].records = sess_records;
        targets[si].sha256 = hex::encode(sess_sha.finalize());

        if self.s.fault == Some(Fault::CorruptAppend) {
            if let Some(tg) = targets
                .iter()
                .find(|t| t.offset_after > t.offset_before + 40)
            {
                let mut f = fs.open(&tg.path)?;
                let mut tailb = vec![0; (tg.offset_after - tg.offset_before) as usize];
                let mut at = 0;
                while at < tailb.len() {
                    at += f.read_at(tg.offset_before + at as u64, &mut tailb[at..])?;
                }
                let mid = tailb.len() / 2;
                tailb[mid] ^= 0x5a;
                f.set_len(tg.offset_before)?;
                f.write_all(&tailb)?;
            }
        }

        // ---- Check (F4): read every appended frame back.
        if by_conn {
            for (k, src) in sources.iter().enumerate() {
                let tg = &targets[conn_ti0 + k];
                let (sha, n, digest) = self.read_back(tg).map_err(|e| SealError::Check {
                    hour: hour_name.clone(),
                    what: format!("{}: {e}", tg.path),
                })?;
                if sha != tg.sha256 || n != src.stat.records || digest != src.stat.digest {
                    return Err(SealError::Check {
                        hour: hour_name.clone(),
                        what: format!("{}: differs from {}", tg.path, src.path),
                    });
                }
            }
        }
        for (t, st) in stats.iter().filter(|_| !by_conn) {
            let tg = &targets[topic_ti[t]];
            let (sha, n, digest) = self.read_back(tg).map_err(|e| SealError::Check {
                hour: hour_name.clone(),
                what: format!("{}: {e}", tg.path),
            })?;
            if sha != tg.sha256 || n != tg.records || n != st.records || digest != st.digest {
                return Err(SealError::Check {
                    hour: hour_name.clone(),
                    what: format!("{}: {n} records read back, {} counted", tg.path, st.records),
                });
            }
        }
        if has_any {
            let tg = &targets[si];
            let (sha, n, digest) = self.read_back(tg).map_err(|e| SealError::Check {
                hour: hour_name.clone(),
                what: format!("{}: {e}", tg.path),
            })?;
            if sha != tg.sha256
                || n != tg.records
                || digest != sess_digest
                || n != notes.len() as u64 + unroutable_all.records
            {
                return Err(SealError::Check {
                    hour: hour_name.clone(),
                    what: format!("{}: session frame differs", tg.path),
                });
            }
        }

        // ---- The done entry.
        let moved: u64 =
            targets[..si].iter().map(|t| t.records).sum::<u64>() + unroutable_all.records;
        let mut done = self.base_entry("done", h, attempt, clock.now_ns());
        done.insert(
            "targets".into(),
            Value::Array(
                targets
                    .iter()
                    .map(|t| {
                        let sessions = match &t.sessions {
                            Some(runs) => Value::Array(runs.iter().map(|(c, ns, n)| json!({"collector": c, "session_ns": ns, "records": n})).collect()),
                            None => Value::Null,
                        };
                        json!({"path": t.path, "offset_after": t.offset_after, "records": t.records, "sha256": t.sha256, "sessions": sessions})
                    })
                    .collect(),
            ),
        );
        done.insert("records".into(), json!(moved));
        done.insert("unroutable_records".into(), json!(unroutable_all.records));
        done.insert("live_contexts_max".into(), json!(contexts.max_live()));
        if let Some((sym, conn)) = measure {
            done.insert(
                "measure".into(),
                json!({"by_symbol_bytes": sym, "by_connection_bytes": conn}),
            );
        }
        let seconds = (clock.now_ns() - run_start).max(0) as f64 / 1e9;
        done.insert("seconds".into(), json!(seconds));
        self.append_log(h, &Value::Object(done))?;

        // ---- Delete the open files, each under its lock.
        for src in &sources {
            fs.remove(&src.path)?;
        }
        if !sources.is_empty() {
            fs.sync_dir(&self.open_dir(h))?;
        }
        Ok(())
    }

    /// Groups of topics inside one connection slot, each bounded by the sum
    /// of its level-9 context sizes (a group holds at least one topic).
    fn make_groups(
        &self,
        slots: usize,
        topic_slot: &BTreeMap<Topic, usize>,
        frame_bytes: &BTreeMap<Topic, u64>,
    ) -> Vec<Group> {
        let mut groups = Vec::new();
        for slot in 0..slots {
            let mut cur: Vec<Topic> = Vec::new();
            let mut cur_bytes = 0usize;
            for (t, s) in topic_slot {
                if *s != slot {
                    continue;
                }
                let c = zst::context_bytes(SEAL_LEVEL, Some(frame_bytes[t]));
                if !cur.is_empty() && cur_bytes + c > self.s.context_budget {
                    groups.push(Group {
                        slot,
                        topics: std::mem::take(&mut cur),
                    });
                    cur_bytes = 0;
                }
                cur.push(t.clone());
                cur_bytes += c;
            }
            if !cur.is_empty() {
                groups.push(Group { slot, topics: cur });
            }
        }
        groups
    }

    /// Compressed bytes of `topics` frames in the by_symbol order, written to
    /// a counting sink (for `measure_layouts`).
    #[allow(clippy::too_many_arguments)]
    fn measure_by_symbol(
        &self,
        sources: &[Source],
        session_order: &[(i64, String)],
        groups: &[Group],
        frame_bytes: &BTreeMap<Topic, u64>,
        date: &str,
        topic_slot: &BTreeMap<Topic, usize>,
        classify: &dyn Fn(&Record) -> Option<Topic>,
    ) -> io::Result<u64> {
        let contexts = Contexts::default();
        let mut total = 0u64;
        for g in groups {
            let in_group: BTreeSet<&Topic> = g.topics.iter().collect();
            let mut encs: BTreeMap<Topic, (FrameEncoder, Vec<u8>)> = BTreeMap::new();
            for t in &g.topics {
                let mut enc = FrameEncoder::new(SEAL_LEVEL, Some(frame_bytes[t]), &contexts)?;
                let mut buf = Vec::new();
                enc.write(
                    &qraw::encode_header(&self.topic_header(t, date, sources, topic_slot)),
                    &mut buf,
                )?;
                encs.insert(t.clone(), (enc, buf));
            }
            for (ns, c) in session_order {
                let idx: Vec<usize> = (0..sources.len())
                    .filter(|&i| {
                        sources[i].session_ns == *ns
                            && &sources[i].collector == c
                            && sources[i].topics.iter().any(|t| in_group.contains(t))
                    })
                    .collect();
                self.merge(
                    sources,
                    &idx,
                    |r| classify(r).filter(|t| in_group.contains(t)).is_some(),
                    &mut |r: Record| {
                        let Some(t) = classify(&r) else { return Ok(()) };
                        let (enc, buf) = encs.get_mut(&t).ok_or_else(|| io_err("topic"))?;
                        let mut bytes = Vec::with_capacity(r.encoded_len());
                        qraw::encode_record(&r, &mut bytes)
                            .map_err(|e| io_err(format!("{e:?}")))?;
                        enc.write(&bytes, buf)?;
                        total += buf.len() as u64;
                        buf.clear();
                        Ok(())
                    },
                )?;
            }
            for (_, (mut enc, mut buf)) in encs {
                enc.finish(&mut buf)?;
                total += buf.len() as u64;
            }
        }
        Ok(total)
    }

    /// Compressed bytes of one level-9 frame per connection (for `measure_layouts`).
    fn measure_by_connection(&self, sources: &[Source], date: &str) -> io::Result<u64> {
        let contexts = Contexts::default();
        let mut total = 0u64;
        for src in sources {
            let header = self.conn_header(src, date);
            let hb = qraw::encode_header(&header);
            let mut enc = FrameEncoder::new(
                SEAL_LEVEL,
                Some(src.stat.bytes + hb.len() as u64),
                &contexts,
            )?;
            let mut buf = Vec::new();
            enc.write(&hb, &mut buf)?;
            let mut f = self.fs.open(&src.path)?;
            for r in RecordIter::new(f.as_mut(), 0, src.end, true) {
                let r = r?;
                let mut bytes = Vec::with_capacity(r.encoded_len());
                qraw::encode_record(&r, &mut bytes).map_err(|e| io_err(format!("{e:?}")))?;
                enc.write(&bytes, &mut buf)?;
                total += buf.len() as u64;
                buf.clear();
            }
            enc.finish(&mut buf)?;
            total += buf.len() as u64;
        }
        Ok(total)
    }

    /// The header of a by_connection day file: the connection form with the day's date.
    fn conn_header(&self, src: &Source, date: &str) -> Header {
        let get = |k: &str| {
            src.header
                .as_ref()
                .and_then(|h| h.get_str(k))
                .unwrap_or("unknown")
                .to_string()
        };
        Header::connection(
            &self.s.venue,
            date,
            &src.collector,
            &get("collector_ver"),
            &get("host"),
            src.session_ns,
            src.conn_id,
        )
    }

    /// Ordered SHA-256, count and set digest of the frame a target got.
    fn read_back(&self, t: &Target) -> io::Result<(String, u64, SetDigest)> {
        let mut f = self.fs.open(&t.path)?;
        let it = RecordIter::new(
            f.as_mut(),
            t.offset_before,
            t.offset_after,
            t.offset_before == 0,
        );
        let mut sha = Sha256::new();
        let mut n = 0;
        let mut d = SetDigest::default();
        for r in it {
            let r = r?;
            let mut bytes = Vec::with_capacity(r.encoded_len());
            qraw::encode_record(&r, &mut bytes).map_err(|e| io_err(format!("{e:?}")))?;
            sha.update(&bytes);
            d.add(&bytes);
            n += 1;
        }
        Ok((hex::encode(sha.finalize()), n, d))
    }

    /// k-way merge of `idx` sources by (recv_ns, conn_id, frame_no); a
    /// source keeps its own order. Only records for which `keep` holds.
    fn merge(
        &self,
        sources: &[Source],
        idx: &[usize],
        keep: impl Fn(&Record) -> bool,
        out: &mut dyn FnMut(Record) -> io::Result<()>,
    ) -> io::Result<()> {
        let mut files: Vec<Box<dyn LakeFile>> = Vec::new();
        for &i in idx {
            files.push(self.fs.open(&sources[i].path)?);
        }
        let mut iters: Vec<RecordIter<'_>> = files
            .iter_mut()
            .zip(idx)
            .map(|(f, &i)| RecordIter::new(f.as_mut(), 0, sources[i].end, true))
            .collect();
        let mut heads: Vec<Option<Record>> = Vec::new();
        let mut heap: BinaryHeap<Head> = BinaryHeap::new();
        let next = |it: &mut RecordIter<'_>| -> io::Result<Option<Record>> {
            for r in it.by_ref() {
                let r = r?;
                if keep(&r) {
                    return Ok(Some(r));
                }
            }
            Ok(None)
        };
        for (k, it) in iters.iter_mut().enumerate() {
            let r = next(it)?;
            if let Some(r) = &r {
                heap.push(Reverse((r.recv_ns, r.conn_id, r.frame_no, k)));
            }
            heads.push(r);
        }
        while let Some(Reverse((_, _, _, k))) = heap.pop() {
            let r = heads[k].take().ok_or_else(|| io_err("merge head"))?;
            out(r)?;
            let n = next(&mut iters[k])?;
            if let Some(r) = &n {
                heap.push(Reverse((r.recv_ns, r.conn_id, r.frame_no, k)));
            }
            heads[k] = n;
        }
        Ok(())
    }

    /// The header of a new day file: `collector` and its version and host
    /// are those of the open file of the topic's first record (2.2).
    fn topic_header(
        &self,
        t: &Topic,
        date: &str,
        sources: &[Source],
        topic_slot: &BTreeMap<Topic, usize>,
    ) -> Header {
        let src = topic_slot.get(t).and_then(|&i| sources.get(i));
        let get = |k: &str| {
            src.and_then(|s| s.header.as_ref())
                .and_then(|h| h.get_str(k))
                .unwrap_or("unknown")
                .to_string()
        };
        let collector = src.map_or("unknown".to_string(), |s| s.collector.clone());
        Header::topic(
            &self.s.venue,
            date,
            &collector,
            &get("collector_ver"),
            &get("host"),
            &t.market,
            &t.channel,
            &t.symbol,
        )
    }

    /// The sealer's session file of `date` (F3): its existing `.partial`
    /// file, else a new name with the configured or the run's start time.
    fn sealer_session_path(&self, date: &str, run_start: i64) -> io::Result<String> {
        let dir = format!("crypto/l0/venue={}/_session/date={date}", self.s.venue);
        let prefix = format!("{}-", self.s.sealer);
        for name in self.fs.list(&dir)? {
            if name.starts_with(&prefix) && name.ends_with(".qraw.zst.partial") {
                return Ok(format!("{dir}/{name}"));
            }
        }
        let start = self.s.sealer_start_ns.unwrap_or(run_start);
        Ok(L0Path::Session {
            venue: self.s.venue.clone(),
            date: date.into(),
            writer: self.s.sealer.clone(),
            start_ns: start,
            partial: true,
        }
        .to_path())
    }

    fn session_start_of(&self, path: &str) -> i64 {
        match L0Path::parse(path) {
            Ok(L0Path::Session { start_ns, .. }) => start_ns,
            _ => 0,
        }
    }

    /// The session tail before hour `h` when the run starts there: the
    /// latest earlier hour with records, up to 48 hours back.
    fn find_tail_before(&self, h: i64, tail: &mut Option<(String, i64, i64)>) -> io::Result<()> {
        let mut p = h - HOUR_NS;
        while p >= self.s.first_hour_ns && p >= h - 48 * HOUR_NS {
            let log = self.read_log(p)?;
            if let Some(done) = log.last() {
                if done["entry"] == "done" && done["records"].as_u64().unwrap_or(0) > 0 {
                    return self.update_tail_from_log(p, &log, tail);
                }
            }
            p -= HOUR_NS;
        }
        Ok(())
    }
}

/// Closed hours (before the hour of `now_ns`, at most `lookback_hours`
/// back) whose `_open` folder still holds a file: hours the seal has not
/// finished (U6 D7, `lake_seal_lag_hours`). Oldest first.
pub fn unsealed_hours(
    fs: &dyn Fs,
    venue: &str,
    now_ns: i64,
    lookback_hours: u32,
) -> io::Result<Vec<i64>> {
    let current = time::hour_start(now_ns);
    let mut out = Vec::new();
    for back in (1..=i64::from(lookback_hours)).rev() {
        let h = current - back * HOUR_NS;
        let dir = format!(
            "crypto/l0/_open/venue={venue}/date={}/hour={:02}",
            time::date(h),
            time::hour(h)
        );
        if !fs.list(&dir)?.is_empty() {
            out.push(h);
        }
    }
    Ok(out)
}
