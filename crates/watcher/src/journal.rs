//! The watcher's own journal reader (C-WATCH part 1), written from the
//! documents only: the file format of ADR-011 §7 and the reading rules of
//! C-TAIL (`docs/contracts/tail.md`). It shares no code with `oms-journal`.
//!
//! ```text
//! segment  journal-<first seq, 20 digits>.oj
//!   header "OMSJRNL\x01" | u32 LE length (at most 64 KiB) | JSON | u32 LE CRC-32 of the JSON
//!   frame  u32 LE payload length (at most 16 MiB) | u32 LE CRC-32 over sequence + payload
//!          | u64 LE sequence | payload (JSON record)
//! ```
//!
//! Rules (C-TAIL): files are opened read-only, `LOCK` is never opened and
//! nothing is created, renamed or written; no record above the durable cap
//! is returned; damage at or below the cap is an error after the good
//! records before it, and the position does not move. The reader keeps
//! (segment, byte offset, next sequence) and reads frame by frame from the
//! offset, so a call never loads or re-reads a whole segment.

use crate::events;
use serde_json::Value;
use std::fmt;
use std::fs::File;
use std::io::{self, BufReader, Read, Seek, SeekFrom};
use std::path::PathBuf;

pub const MAGIC: &[u8; 8] = b"OMSJRNL\x01";
pub const MAX_HEADER_JSON: usize = 64 * 1024;
pub const FRAME_HEADER_LEN: usize = 16;
pub const MAX_PAYLOAD: usize = 16 * 1024 * 1024;
/// The framing version this reader knows.
pub const FORMAT: u64 = 1;
/// The record schemas this reader knows (C-TAIL "기록 형식").
pub const RECORD_SCHEMAS: [u64; 2] = [1, 2];
/// Records per `read` call (판단(모델 아님)): a boot replay of a long
/// journal is many calls, so nothing waits behind one of them.
pub const MAX_RECORDS: usize = 10_000;

/// One journal record. Only `events` is interpreted; `input` is not parsed.
#[derive(Clone, Debug, PartialEq)]
pub struct Record {
    pub seq: u64,
    /// Record time, µs.
    pub at: u64,
    pub record_schema: u16,
    /// `(event name, its fields)` in record order.
    pub events: Vec<(String, Value)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JournalError {
    /// Damage at or below the cap: a short header or frame, a bad checksum,
    /// a wrong sequence, a missing segment, a payload that is not a record.
    Damaged {
        seq: u64,
        why: String,
    },
    UnknownEvent {
        seq: u64,
        name: String,
    },
    Schema {
        segment: u64,
        schema: u64,
    },
    /// A framing version other than 1.
    Format {
        segment: u64,
        format: u64,
    },
    Instance {
        expected: u64,
        found: u64,
    },
    /// The first segment does not start at record 1: the watcher keeps no
    /// state and replays from record 1 at every start.
    NoStart,
    Io(io::ErrorKind),
    /// A record that breaks a rule of the journal state (C-WATCH part 2):
    /// a fill of an order the journal never accepted, a purge of an open
    /// order, a field missing or not of its documented form, a record out
    /// of order. Latched: the journal is not what C-TAIL says.
    State {
        seq: u64,
        why: String,
    },
}

impl JournalError {
    /// Retried: the position does not move and the next read tries again,
    /// so a passing condition heals. Every other error latches the reader
    /// until the process restarts (reading again cannot heal it).
    pub fn is_retried(&self) -> bool {
        matches!(
            self,
            JournalError::Io(_) | JournalError::Damaged { .. } | JournalError::NoStart
        )
    }
}

impl fmt::Display for JournalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            JournalError::Damaged { seq, why } => {
                write!(f, "journal damaged at record {seq}: {why}")
            }
            JournalError::UnknownEvent { seq, name } => {
                write!(
                    f,
                    "record {seq} holds the unknown event {name:?}; the watcher must be taught it"
                )
            }
            JournalError::Schema { segment, schema } => {
                write!(
                    f,
                    "segment {segment} has record schema {schema}; the watcher reads 1 and 2"
                )
            }
            JournalError::Format { segment, format } => {
                write!(
                    f,
                    "segment {segment} has format {format}; the watcher reads {FORMAT}"
                )
            }
            JournalError::Instance { expected, found } => {
                write!(
                    f,
                    "a segment of instance {found:016x} in the journal of instance {expected:016x}"
                )
            }
            JournalError::NoStart => {
                write!(f, "the journal's first segment does not start at record 1")
            }
            JournalError::Io(kind) => write!(f, "journal read failed: {kind}"),
            JournalError::State { seq, why } => {
                write!(f, "record {seq} breaks the journal state rules: {why}")
            }
        }
    }
}

pub fn segment_name(first_seq: u64) -> String {
    format!("journal-{first_seq:020}.oj")
}

fn parse_segment_name(name: &str) -> Option<u64> {
    let body = name.strip_prefix("journal-")?.strip_suffix(".oj")?;
    if body.len() != 20 || !body.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    body.parse().ok()
}

/// Where the next frame starts, in a segment of which a frame was read.
#[derive(Clone, Copy, Debug)]
struct Pos {
    first_seq: u64,
    offset: u64,
    schema: u16,
}

/// A segment open for this call.
struct Open {
    first_seq: u64,
    schema: u16,
    instance: u64,
    offset: u64,
    /// No frame of it was read yet (its header was just read).
    fresh: bool,
    reader: BufReader<File>,
}

pub struct Tail {
    dir: PathBuf,
    /// `None` until the first frame was read: a segment whose frames were
    /// not read yet has its header read again at each call (C-TAIL).
    pos: Option<Pos>,
    next_seq: u64,
    last_at: Option<u64>,
    instance_id: Option<u64>,
    latched: Option<JournalError>,
}

impl Tail {
    /// A reader at record 1 of the journal in `dir`. Nothing is opened yet.
    pub fn open(dir: impl Into<PathBuf>) -> Tail {
        Tail {
            dir: dir.into(),
            pos: None,
            next_seq: 1,
            last_at: None,
            instance_id: None,
            latched: None,
        }
    }

    /// The last record returned; 0 before the first.
    pub fn last_seq(&self) -> u64 {
        self.next_seq - 1
    }

    /// The instance of the segments read so far.
    pub fn instance_id(&self) -> Option<u64> {
        self.instance_id
    }

    /// The error that stopped this reader until a restart, if any.
    pub fn latched(&self) -> Option<&JournalError> {
        self.latched.as_ref()
    }

    /// The records after the last one returned, in order, up to `cap` and
    /// at most `max_records`. When an error follows good records, the good
    /// records are returned and the next call returns the error.
    pub fn read(&mut self, cap: u64, max_records: usize) -> Result<Vec<Record>, JournalError> {
        if let Some(e) = &self.latched {
            return Err(e.clone());
        }
        let mut out = Vec::new();
        match self.read_into(cap, max_records, &mut out) {
            Ok(()) => Ok(out),
            Err(e) => {
                if !e.is_retried() {
                    self.latched = Some(e.clone());
                }
                if out.is_empty() { Err(e) } else { Ok(out) }
            }
        }
    }

    fn read_into(
        &mut self,
        cap: u64,
        max_records: usize,
        out: &mut Vec<Record>,
    ) -> Result<(), JournalError> {
        let mut open: Option<Open> = None;
        while out.len() < max_records && self.next_seq <= cap {
            let mut seg = match open.take() {
                Some(o) => o,
                None => match self.pos {
                    Some(p) => self.reopen(p)?,
                    None => self.open_first()?,
                },
            };
            let seq = self.next_seq;
            match read_frame(&mut seg.reader, seq)? {
                Some((payload, len)) => {
                    let rec = self.decode(seq, seg.schema, &payload)?;
                    seg.offset += len;
                    seg.fresh = false;
                    self.pos = Some(Pos {
                        first_seq: seg.first_seq,
                        offset: seg.offset,
                        schema: seg.schema,
                    });
                    self.instance_id = Some(seg.instance);
                    self.next_seq += 1;
                    self.last_at = Some(rec.at);
                    out.push(rec);
                    open = Some(seg);
                }
                None if seg.fresh => {
                    return Err(JournalError::Damaged {
                        seq,
                        why: format!("{} ends after its header", segment_name(seg.first_seq)),
                    });
                }
                // The end of this segment: record `seq` starts the next one.
                None => open = Some(self.open_segment(seq)?),
            }
        }
        Ok(())
    }

    fn open_first(&self) -> Result<Open, JournalError> {
        let mut first = None;
        for entry in std::fs::read_dir(&self.dir).map_err(io)? {
            let entry = entry.map_err(io)?;
            if let Some(seq) = entry.file_name().to_str().and_then(parse_segment_name) {
                first = Some(first.map_or(seq, |f: u64| f.min(seq)));
            }
        }
        match first {
            None => Err(JournalError::Damaged {
                seq: 1,
                why: "no segment in the data directory".into(),
            }),
            Some(1) => self.open_segment(1),
            Some(_) => Err(JournalError::NoStart),
        }
    }

    /// Opens the segment that must start at `first_seq` and checks its header.
    fn open_segment(&self, first_seq: u64) -> Result<Open, JournalError> {
        let name = segment_name(first_seq);
        let file = match File::open(self.dir.join(&name)) {
            Ok(f) => f,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                return Err(JournalError::Damaged {
                    seq: first_seq,
                    why: format!("missing segment {name}"),
                });
            }
            Err(e) => return Err(io(e)),
        };
        let mut reader = BufReader::new(file);
        let damaged = |why: String| JournalError::Damaged {
            seq: first_seq,
            why: format!("{name}: {why}"),
        };
        let (h, len) = read_header(&mut reader)?.map_err(damaged)?;
        if h.format != FORMAT {
            return Err(JournalError::Format {
                segment: first_seq,
                format: h.format,
            });
        }
        if h.first_seq != first_seq {
            return Err(damaged(format!(
                "header first_seq {} in the file of {first_seq}",
                h.first_seq
            )));
        }
        if let Some(expected) = self.instance_id {
            if h.instance_id != expected {
                return Err(JournalError::Instance {
                    expected,
                    found: h.instance_id,
                });
            }
        }
        if !RECORD_SCHEMAS.contains(&h.record_schema) {
            return Err(JournalError::Schema {
                segment: first_seq,
                schema: h.record_schema,
            });
        }
        Ok(Open {
            first_seq,
            schema: h.record_schema as u16,
            instance: h.instance_id,
            offset: len,
            fresh: true,
            reader,
        })
    }

    /// The segment of the last record read, at the offset after it.
    fn reopen(&self, p: Pos) -> Result<Open, JournalError> {
        let name = segment_name(p.first_seq);
        let damaged = |why: String| JournalError::Damaged {
            seq: self.next_seq,
            why: format!("{name}: {why}"),
        };
        let file = match File::open(self.dir.join(&name)) {
            Ok(f) => f,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                return Err(damaged("the segment disappeared".into()));
            }
            Err(e) => return Err(io(e)),
        };
        let len = file.metadata().map_err(io)?.len();
        if len < p.offset {
            return Err(damaged(format!(
                "{len} bytes, shorter than the read position {}",
                p.offset
            )));
        }
        let mut reader = BufReader::new(file);
        reader.seek(SeekFrom::Start(p.offset)).map_err(io)?;
        Ok(Open {
            first_seq: p.first_seq,
            schema: p.schema,
            instance: self.instance_id.unwrap_or_default(),
            offset: p.offset,
            fresh: false,
            reader,
        })
    }

    /// The record of frame `seq`: `seq` as the frame's, `at` not below the
    /// previous record's, `events` a list of one-key objects with known names.
    fn decode(&self, seq: u64, schema: u16, payload: &[u8]) -> Result<Record, JournalError> {
        let damaged = |why: &str| JournalError::Damaged {
            seq,
            why: why.to_string(),
        };
        let v: Value =
            serde_json::from_slice(payload).map_err(|_| damaged("the record is not JSON"))?;
        if v.get("seq").and_then(Value::as_u64) != Some(seq) {
            return Err(damaged("the record's seq is not the frame's"));
        }
        let at = v
            .get("at")
            .and_then(Value::as_u64)
            .ok_or_else(|| damaged("the record has no time"))?;
        if self.last_at.is_some_and(|last| at < last) {
            return Err(damaged("the record time is below the previous record's"));
        }
        let list = v
            .get("events")
            .and_then(Value::as_array)
            .ok_or_else(|| damaged("the record has no events"))?;
        let mut events = Vec::with_capacity(list.len());
        for e in list {
            let Some((name, fields)) = e
                .as_object()
                .filter(|o| o.len() == 1)
                .and_then(|o| o.iter().next())
            else {
                return Err(damaged("an event is not an object of one key"));
            };
            if !events::is_known(name) {
                return Err(JournalError::UnknownEvent {
                    seq,
                    name: name.clone(),
                });
            }
            events.push((name.clone(), fields.clone()));
        }
        Ok(Record {
            seq,
            at,
            record_schema: schema,
            events,
        })
    }
}

fn io(e: io::Error) -> JournalError {
    JournalError::Io(e.kind())
}

struct Header {
    format: u64,
    record_schema: u64,
    instance_id: u64,
    first_seq: u64,
}

/// The header and its length in bytes; the inner `Err` is damage.
fn read_header(r: &mut impl Read) -> Result<Result<(Header, u64), String>, JournalError> {
    let mut fixed = [0u8; 12];
    if read_full(r, &mut fixed)? < fixed.len() {
        return Ok(Err("short segment header".into()));
    }
    if &fixed[..8] != MAGIC {
        return Ok(Err("bad segment magic".into()));
    }
    let len = u32::from_le_bytes([fixed[8], fixed[9], fixed[10], fixed[11]]) as usize;
    if len > MAX_HEADER_JSON {
        return Ok(Err(format!("segment header of {len} bytes, above 64 KiB")));
    }
    let mut rest = vec![0u8; len + 4];
    if read_full(r, &mut rest)? < rest.len() {
        return Ok(Err("short segment header".into()));
    }
    let (json, crc) = rest.split_at(len);
    if crc32fast::hash(json) != u32::from_le_bytes([crc[0], crc[1], crc[2], crc[3]]) {
        return Ok(Err("segment header checksum mismatch".into()));
    }
    Ok(header_fields(json).map(|h| (h, (12 + len + 4) as u64)))
}

fn header_fields(json: &[u8]) -> Result<Header, String> {
    let v: Value =
        serde_json::from_slice(json).map_err(|_| "segment header is not JSON".to_string())?;
    let field = |k: &str| {
        v.get(k)
            .and_then(Value::as_u64)
            .ok_or_else(|| format!("segment header has no {k}"))
    };
    Ok(Header {
        format: field("format")?,
        record_schema: field("record_schema")?,
        instance_id: field("instance_id")?,
        first_seq: field("first_seq")?,
    })
}

/// Frame `seq` at the reader's position: `None` at the end of the file;
/// its payload and length in bytes; damage as `Err`.
fn read_frame(r: &mut impl Read, seq: u64) -> Result<Option<(Vec<u8>, u64)>, JournalError> {
    let damaged = |why: String| JournalError::Damaged { seq, why };
    let mut head = [0u8; FRAME_HEADER_LEN];
    match read_full(r, &mut head)? {
        0 => return Ok(None),
        FRAME_HEADER_LEN => {}
        n => return Err(damaged(format!("short frame header ({n} of 16 bytes)"))),
    }
    let len = u32::from_le_bytes([head[0], head[1], head[2], head[3]]) as usize;
    if len > MAX_PAYLOAD {
        return Err(damaged(format!("frame length {len} above 16 MiB")));
    }
    let mut payload = vec![0u8; len];
    let n = read_full(r, &mut payload)?;
    if n < len {
        return Err(damaged(format!("short frame ({n} of {len} payload bytes)")));
    }
    let mut h = crc32fast::Hasher::new();
    h.update(&head[8..16]);
    h.update(&payload);
    if h.finalize() != u32::from_le_bytes([head[4], head[5], head[6], head[7]]) {
        return Err(damaged("frame checksum mismatch".into()));
    }
    let found = u64::from_le_bytes([
        head[8], head[9], head[10], head[11], head[12], head[13], head[14], head[15],
    ]);
    if found != seq {
        return Err(damaged(format!(
            "frame sequence {found} where {seq} was expected"
        )));
    }
    Ok(Some((payload, (FRAME_HEADER_LEN + len) as u64)))
}

/// Reads until `buf` is full or the file ends; the number of bytes read.
fn read_full(r: &mut impl Read, buf: &mut [u8]) -> Result<usize, JournalError> {
    let mut n = 0;
    while n < buf.len() {
        match r.read(&mut buf[n..]) {
            Ok(0) => break,
            Ok(k) => n += k,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(io(e)),
        }
    }
    Ok(n)
}
