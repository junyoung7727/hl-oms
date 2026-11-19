//! What the archiver reads (spec D2, D3): the day-sealed dates of a venue,
//! their sealed files and manifest lines, the connection of every record, the
//! connection lifetimes (pass 1) and the instrument row in effect.
//!
//! Only sealed L0 files and their manifest lines are read (lake contract
//! 3.1); nothing under `crypto/l0/` is written.

use std::collections::{BTreeMap, BTreeSet};
use std::io;

use md_core::adapter::Instrument;
use md_lake::fs::{Fs, LakeFile};
use md_lake::paths::L0Path;
use md_lake::qraw::{Record, Src};
use md_lake::read::RecordIter;
use serde_json::Value;

/// The contract version this build reads (lake contract 6).
pub const CONTRACT_VER: i64 = 1;

/// Why the input refuses a run (exit 2): no later run can cure it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal(pub String);

#[derive(Debug)]
pub enum InputError {
    Io(io::Error),
    Refuse(Refusal),
}

impl std::fmt::Display for InputError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InputError::Io(e) => write!(f, "{e}"),
            InputError::Refuse(r) => write!(f, "refused: {}", r.0),
        }
    }
}

impl std::error::Error for InputError {}

impl From<io::Error> for InputError {
    fn from(e: io::Error) -> Self {
        InputError::Io(e)
    }
}

fn refuse(msg: impl Into<String>) -> InputError {
    InputError::Refuse(Refusal(msg.into()))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRun {
    pub collector: String,
    pub session_ns: i64,
    pub first_rec_no: u64,
    pub records: u64,
}

/// A topic `(market, channel, symbol)`; instruments use channel
/// `instruments` and symbol `_all`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Topic {
    pub market: String,
    pub channel: String,
    pub symbol: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileKind {
    /// A `by_symbol` day file of a WebSocket topic.
    Topic(Topic),
    /// The instruments day file of a market.
    Instruments { market: String },
    /// A session file (a collector's or the sealer's): its header names the
    /// writer session.
    Session { writer: String, start_ns: i64 },
}

/// A sealed L0 file with its manifest line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct L0File {
    pub l0_file_id: String,
    pub path: String,
    pub date: String,
    pub kind: FileKind,
    pub bytes: u64,
    pub records: u64,
    /// For topic and instruments files; empty for session files.
    pub sessions: Vec<SessionRun>,
}

impl L0File {
    pub fn topic(&self) -> Option<Topic> {
        match &self.kind {
            FileKind::Topic(t) => Some(t.clone()),
            FileKind::Instruments { market } => Some(Topic {
                market: market.clone(),
                channel: "instruments".into(),
                symbol: "_all".into(),
            }),
            FileKind::Session { .. } => None,
        }
    }

    /// The writer session of record `rec_no` (lake contract 2.5, 3.1).
    pub fn session_of(&self, rec_no: u64) -> Option<(&str, i64)> {
        match &self.kind {
            FileKind::Session { writer, start_ns } => Some((writer.as_str(), *start_ns)),
            _ => self
                .sessions
                .iter()
                .find(|s| s.first_rec_no <= rec_no && rec_no < s.first_rec_no + s.records)
                .map(|s| (s.collector.as_str(), s.session_ns)),
        }
    }
}

/// A connection `(collector, session_ns, conn_id)` (lake contract 2.2).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ConnKey {
    pub collector: String,
    pub session_ns: i64,
    pub conn_id: u32,
}

/// The state of one date of a venue (spec D2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DateState {
    /// No manifest and no L0 folder of the date.
    NoL0,
    /// L0 exists but the day seal has not finished; the reason is for logs.
    NotSealed(String),
    /// Day-sealed: every sealed file with its manifest line, in `l0_file_id` order.
    Sealed(Vec<L0File>),
}

fn venue_root(venue: &str) -> String {
    format!("crypto/l0/venue={venue}")
}

/// The folders of a date that can hold L0 files (every layout and the
/// session folder).
fn date_dirs(fs: &dyn Fs, venue: &str, date: &str) -> io::Result<Vec<String>> {
    let root = venue_root(venue);
    let mut dirs = vec![
        format!("{root}/_conn/date={date}"),
        format!("{root}/_session/date={date}"),
    ];
    for m in fs.list(&root)? {
        if m.starts_with("market=") {
            for c in fs.list(&format!("{root}/{m}"))? {
                if c.starts_with("channel=") {
                    dirs.push(format!("{root}/{m}/{c}/date={date}"));
                }
            }
        }
    }
    Ok(dirs)
}

/// Every file below `dir`, as lake-relative paths. `Fs::list` of a file
/// fails (it is not a folder); that is how a file is told from a folder.
fn walk(fs: &dyn Fs, dir: &str, out: &mut Vec<String>) -> io::Result<()> {
    for n in fs.list(dir)? {
        let p = format!("{dir}/{n}");
        match fs.list(&p) {
            Ok(_) => walk(fs, &p, out)?,
            Err(_) => out.push(p),
        }
    }
    Ok(())
}

fn manifest_path(venue: &str, date: &str) -> String {
    format!("crypto/l0/_manifest/venue={venue}/date={date}.jsonl")
}

fn read_whole(fs: &dyn Fs, path: &str) -> io::Result<Vec<u8>> {
    let mut f = fs.open(path)?;
    read_file(f.as_mut())
}

fn read_file(f: &mut dyn LakeFile) -> io::Result<Vec<u8>> {
    let len = usize::try_from(f.len()?).map_err(io::Error::other)?;
    let mut buf = vec![0u8; len];
    let mut at = 0;
    while at < len {
        let n = f.read_at(at as u64, &mut buf[at..])?;
        if n == 0 {
            return Err(io::Error::other("short read"));
        }
        at += n;
    }
    Ok(buf)
}

fn int(v: &Value, k: &str) -> Option<i64> {
    v.get(k).and_then(Value::as_i64)
}

fn parse_line(line: &Value) -> Result<(String, L0File), String> {
    let s = |k: &str| {
        line.get(k)
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| format!("manifest line without {k}"))
    };
    let id = s("l0_file_id")?;
    let path = s("path")?;
    let bytes = int(line, "bytes").ok_or("manifest line without bytes")?;
    let records = int(line, "records").ok_or("manifest line without records")?;
    let parsed = L0Path::parse(&path)?;
    if parsed.l0_file_id().as_deref() != Some(id.as_str()) {
        return Err(format!("manifest path {path} is not {id}"));
    }
    let (date, kind) = match &parsed {
        L0Path::DayBySymbol {
            market,
            channel,
            date,
            symbol,
            ..
        } => (
            date.clone(),
            FileKind::Topic(Topic {
                market: market.clone(),
                channel: channel.clone(),
                symbol: symbol.clone(),
            }),
        ),
        L0Path::Instruments { market, date, .. } => (
            date.clone(),
            FileKind::Instruments {
                market: market.clone(),
            },
        ),
        L0Path::Session {
            date,
            writer,
            start_ns,
            ..
        } => (
            date.clone(),
            FileKind::Session {
                writer: writer.clone(),
                start_ns: *start_ns,
            },
        ),
        other => return Err(format!("manifest line for {other:?}")),
    };
    let sessions = match line.get("sessions") {
        Some(Value::Array(a)) => a
            .iter()
            .map(|r| {
                Ok(SessionRun {
                    collector: r
                        .get("collector")
                        .and_then(Value::as_str)
                        .ok_or("session run without collector")?
                        .to_string(),
                    session_ns: int(r, "session_ns").ok_or("session run without session_ns")?,
                    first_rec_no: int(r, "first_rec_no")
                        .and_then(|x| u64::try_from(x).ok())
                        .ok_or("session run without first_rec_no")?,
                    records: int(r, "records")
                        .and_then(|x| u64::try_from(x).ok())
                        .ok_or("session run without records")?,
                })
            })
            .collect::<Result<Vec<_>, String>>()?,
        _ => Vec::new(),
    };
    Ok((
        id.clone(),
        L0File {
            l0_file_id: id,
            path,
            date,
            kind,
            bytes: u64::try_from(bytes).map_err(|_| "negative bytes")?,
            records: u64::try_from(records).map_err(|_| "negative records")?,
            sessions,
        },
    ))
}

/// The state of `date` (spec D2). A manifest line or header of a newer
/// contract, or layout `by_connection`, is a refusal.
pub fn date_state(fs: &dyn Fs, venue: &str, date: &str) -> Result<DateState, InputError> {
    let mut on_disk = Vec::new();
    for d in date_dirs(fs, venue, date)? {
        walk(fs, &d, &mut on_disk)?;
    }
    let mut open = Vec::new();
    walk(
        fs,
        &format!("crypto/l0/_open/venue={venue}/date={date}"),
        &mut open,
    )?;
    let mpath = manifest_path(venue, date);
    let has_manifest = fs.exists(&mpath);
    if !has_manifest && on_disk.is_empty() && open.is_empty() {
        return Ok(DateState::NoL0);
    }
    if let Some(p) = open.first() {
        return Ok(DateState::NotSealed(format!("open file {p}")));
    }
    if let Some(p) = on_disk.iter().find(|p| p.ends_with(".partial")) {
        return Ok(DateState::NotSealed(format!("{p} is not sealed")));
    }
    if !has_manifest {
        return Ok(DateState::NotSealed("no manifest".into()));
    }
    let text = read_whole(fs, &mpath)?;
    if !text.is_empty() && !text.ends_with(b"\n") {
        return Ok(DateState::NotSealed("the last manifest line is cut".into()));
    }
    let mut files: BTreeMap<String, L0File> = BTreeMap::new();
    for raw in text.split(|b| *b == b'\n').filter(|l| !l.is_empty()) {
        let line: Value = match serde_json::from_slice(raw) {
            Ok(v) => v,
            Err(_) => return Ok(DateState::NotSealed("a manifest line is not JSON".into())),
        };
        match int(&line, "contract_ver") {
            Some(v) if v <= CONTRACT_VER => {}
            other => {
                return Err(refuse(format!(
                    "manifest of {date} has contract_ver {other:?}; this build reads {CONTRACT_VER}"
                )));
            }
        }
        if line.get("layout").and_then(Value::as_str) == Some("by_connection") {
            return Err(refuse(format!(
                "{date} has layout by_connection, which md-archiver does not read (spec D2)"
            )));
        }
        let (id, file) = match parse_line(&line) {
            Ok(x) => x,
            Err(e) => return Ok(DateState::NotSealed(e)),
        };
        if file.date != date {
            return Ok(DateState::NotSealed(format!("{id} is listed under {date}")));
        }
        if files.insert(id.clone(), file).is_some() {
            return Ok(DateState::NotSealed(format!("{id} has two manifest lines")));
        }
    }
    let mut sealed = BTreeSet::new();
    for p in &on_disk {
        if p.ends_with(".qraw.zst") {
            let id = L0Path::parse(p)
                .ok()
                .and_then(|x| x.l0_file_id())
                .ok_or_else(|| refuse(format!("not an L0 path: {p}")))?;
            if !files.contains_key(&id) {
                return Ok(DateState::NotSealed(format!("{p} has no manifest line")));
            }
            if !on_disk.contains(&md_lake::paths::sidecar(p)) {
                return Ok(DateState::NotSealed(format!("{p} has no sidecar")));
            }
            sealed.insert(id);
        }
    }
    for (id, f) in &files {
        if !sealed.contains(id) {
            return Ok(DateState::NotSealed(format!(
                "{id} is listed but not on disk"
            )));
        }
        let len = fs.open(&f.path)?.len()?;
        if len != f.bytes {
            return Ok(DateState::NotSealed(format!(
                "{id} has {len} bytes, the manifest says {}",
                f.bytes
            )));
        }
    }
    Ok(DateState::Sealed(files.into_values().collect()))
}

/// Whether the date has L1: its gap day file exists (spec D2).
pub fn has_l1(fs: &dyn Fs, venue: &str, date: &str) -> bool {
    fs.exists(&format!(
        "crypto/l1/gap/venue={venue}/date={date}/day.parquet"
    ))
}

/// Every date of the venue that has a manifest or an L0 folder, ascending.
pub fn l0_dates(fs: &dyn Fs, venue: &str) -> io::Result<Vec<String>> {
    let mut dates = BTreeSet::new();
    let mut add = |names: Vec<String>| {
        for n in names {
            let d = n
                .strip_prefix("date=")
                .map(|d| d.trim_end_matches(".jsonl"));
            if let Some(d) = d.filter(|d| md_lake::paths::valid_date(d)) {
                dates.insert(d.to_string());
            }
        }
    };
    add(fs.list(&format!("crypto/l0/_manifest/venue={venue}"))?);
    add(fs.list(&format!("crypto/l0/_open/venue={venue}"))?);
    let root = venue_root(venue);
    add(fs.list(&format!("{root}/_conn"))?);
    add(fs.list(&format!("{root}/_session"))?);
    for m in fs.list(&root)? {
        if m.starts_with("market=") {
            for c in fs.list(&format!("{root}/{m}"))? {
                if c.starts_with("channel=") {
                    add(fs.list(&format!("{root}/{m}/{c}"))?);
                }
            }
        }
    }
    Ok(dates.into_iter().collect())
}

/// Every record of a sealed file, in `rec_no` order, with its connection
/// (`None` for REST records, notes and records without a session). A header
/// of a newer contract is a refusal.
pub fn for_each_record(
    fs: &dyn Fs,
    file: &L0File,
    mut f: impl FnMut(u64, Option<ConnKey>, Record),
) -> Result<(), InputError> {
    let mut h = fs.open(&file.path)?;
    let len = h.len()?;
    let mut it = RecordIter::new(h.as_mut(), 0, len, true);
    let mut rec_no = 0u64;
    while let Some(r) = it.next() {
        let r = r?;
        if rec_no == 0 {
            if let Some(v) = it.header().and_then(|h| h.get_i64("contract_ver")) {
                if v > CONTRACT_VER {
                    return Err(refuse(format!(
                        "{} has contract_ver {v}; this build reads {CONTRACT_VER}",
                        file.l0_file_id
                    )));
                }
            }
        }
        let conn = match r.src {
            Src::WebSocket if r.conn_id != 0 => {
                file.session_of(rec_no)
                    .map(|(collector, session_ns)| ConnKey {
                        collector: collector.to_string(),
                        session_ns,
                        conn_id: r.conn_id,
                    })
            }
            _ => None,
        };
        f(rec_no, conn, r);
        rec_no += 1;
    }
    if it.truncated_tail {
        return Err(InputError::Io(io::Error::other(format!(
            "{} ends inside a record",
            file.l0_file_id
        ))));
    }
    if rec_no != file.records {
        return Err(InputError::Io(io::Error::other(format!(
            "{} has {rec_no} records, the manifest says {}",
            file.l0_file_id, file.records
        ))));
    }
    Ok(())
}

/// A record named by its file and position.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RecRef {
    pub l0_file_id: String,
    pub rec_no: u64,
    pub recv_ns: i64,
}

/// What pass 1 knows of one connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnInfo {
    pub first_ns: i64,
    pub last_ns: i64,
    /// The first record of each topic the connection carries.
    pub topics: BTreeMap<Topic, RecRef>,
}

/// A note (`src = 2`) and where it is.
#[derive(Debug, Clone, PartialEq)]
pub struct NoteRec {
    pub at: RecRef,
    /// The session of the file that holds it.
    pub writer: String,
    pub writer_start_ns: i64,
    pub body: Value,
}

/// Pass 1 over the files of D−1 and D (spec D3).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Pass1 {
    pub conns: BTreeMap<ConnKey, ConnInfo>,
    pub notes: Vec<NoteRec>,
}

impl Pass1 {
    pub fn scan(fs: &dyn Fs, files: &[L0File]) -> Result<Self, InputError> {
        let mut p = Pass1::default();
        for file in files {
            let topic = match &file.kind {
                FileKind::Topic(t) => Some(t.clone()),
                _ => None,
            };
            for_each_record(fs, file, |rec_no, conn, r| {
                if r.src == Src::Note {
                    if let (Ok(body), FileKind::Session { writer, start_ns }) =
                        (serde_json::from_slice::<Value>(&r.payload), &file.kind)
                    {
                        p.notes.push(NoteRec {
                            at: RecRef {
                                l0_file_id: file.l0_file_id.clone(),
                                rec_no,
                                recv_ns: r.recv_ns,
                            },
                            writer: writer.clone(),
                            writer_start_ns: *start_ns,
                            body,
                        });
                    }
                    return;
                }
                let Some(conn) = conn else { return };
                let info = p.conns.entry(conn).or_insert(ConnInfo {
                    first_ns: r.recv_ns,
                    last_ns: r.recv_ns,
                    topics: BTreeMap::new(),
                });
                info.first_ns = info.first_ns.min(r.recv_ns);
                info.last_ns = info.last_ns.max(r.recv_ns);
                if let Some(t) = &topic {
                    let here = RecRef {
                        l0_file_id: file.l0_file_id.clone(),
                        rec_no,
                        recv_ns: r.recv_ns,
                    };
                    let e = info.topics.entry(t.clone()).or_insert_with(|| here.clone());
                    if (here.recv_ns, &here.l0_file_id, here.rec_no)
                        < (e.recv_ns, &e.l0_file_id, e.rec_no)
                    {
                        *e = here;
                    }
                }
            })?;
        }
        Ok(p)
    }
}

/// One instruments response: its rows and where it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    pub at: RecRef,
    pub rows: Vec<Instrument>,
}

/// The instruments responses of one market over D−7 … D, by `recv_ns`
/// (spec D3).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InstrumentsInEffect {
    responses: Vec<Response>,
}

impl InstrumentsInEffect {
    pub fn new(mut responses: Vec<Response>) -> Self {
        responses.sort_by(|a, b| a.at.cmp(&b.at));
        Self { responses }
    }

    /// The row of `symbol` in effect at `recv_ns`: from the latest response
    /// at or before it that names the symbol; when there is none, from the
    /// earliest response that names it. The window holds no later date than
    /// the one being built, so the answer does not depend on when the run is.
    pub fn row(&self, symbol: &str, recv_ns: i64) -> Option<&Instrument> {
        let named = |r: &&Response| r.rows.iter().any(|i| i.symbol == symbol);
        let pick = self
            .responses
            .iter()
            .filter(named)
            .take_while(|r| r.at.recv_ns <= recv_ns)
            .last()
            .or_else(|| self.responses.iter().find(named))?;
        pick.rows.iter().find(|i| i.symbol == symbol)
    }

    /// Every symbol named by any response, with its rows in effect at `recv_ns`.
    pub fn rows_at(&self, recv_ns: i64) -> Vec<Instrument> {
        let symbols: BTreeSet<&str> = self
            .responses
            .iter()
            .flat_map(|r| r.rows.iter().map(|i| i.symbol.as_str()))
            .collect();
        symbols
            .into_iter()
            .filter_map(|s| self.row(s, recv_ns).cloned())
            .collect()
    }
}
