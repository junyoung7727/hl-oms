//! Segment files, the writer and recovery.
//!
//! A data directory holds:
//!
//! ```text
//! LOCK                              exclusive lock held by the one writer process
//! journal-<first seq, 20 digits>.oj segments, each starting with a header
//! snapshot-<seq, 20 digits>.snap    snapshots (see snapshot.rs)
//! torn/                             incomplete tails preserved by recovery
//! ```
//!
//! Segment header:
//!
//! ```text
//! 8 bytes   magic "OMSJRNL\x01"
//! 4 bytes   header JSON length, little endian
//! n bytes   header JSON (SegmentHeader)
//! 4 bytes   CRC-32 over the JSON bytes
//! ```

use crate::frame::{self, Decoded};
use crate::{Error, io_err, sync_dir};
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

const MAGIC: &[u8; 8] = b"OMSJRNL\x01";
const MAX_HEADER_JSON: usize = 64 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SegmentHeader {
    /// Version of the framing in this file.
    pub format: u16,
    /// Version of the payload schema, owned by the application.
    pub record_schema: u16,
    pub instance_id: u64,
    pub first_seq: u64,
    pub created_at_us: u64,
    /// Free-form identification of the writing binary.
    pub writer: String,
}

pub const FORMAT: u16 = 1;

#[derive(Clone, Debug)]
pub struct Options {
    /// A segment is closed at the first sync after it reaches this size.
    pub segment_max_bytes: u64,
    /// If false, an incomplete tail makes recovery fail instead of being
    /// preserved and truncated.
    pub truncate_torn_tail: bool,
    /// Schema this writer produces.
    pub record_schema: u16,
    /// Schemas this build can read. Must include `record_schema`.
    pub readable_schemas: Vec<u16>,
    pub instance_id: u64,
    pub writer: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TornTail {
    pub segment: PathBuf,
    pub offset: u64,
    pub bytes: u64,
    pub why: String,
    /// Where the removed bytes were preserved.
    pub saved_to: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Recovery {
    /// Last valid record on disk; 0 for an empty journal.
    pub last_seq: u64,
    /// First record present on disk; 0 for an empty journal.
    pub first_seq: u64,
    pub records_visited: u64,
    pub segments: usize,
    pub torn_tail: Option<TornTail>,
    pub instance_id: Option<u64>,
}

fn segment_name(first_seq: u64) -> String {
    format!("journal-{first_seq:020}.oj")
}

fn parse_segment_name(name: &str) -> Option<u64> {
    let body = name.strip_prefix("journal-")?.strip_suffix(".oj")?;
    if body.len() != 20 {
        return None;
    }
    body.parse().ok()
}

/// Segment paths in sequence order.
pub fn list_segments(dir: &Path) -> Result<Vec<(u64, PathBuf)>, Error> {
    let mut out = Vec::new();
    let rd = fs::read_dir(dir).map_err(io_err(format!("read dir {}", dir.display())))?;
    for entry in rd {
        let entry = entry.map_err(io_err("read dir entry"))?;
        if let Some(seq) = entry.file_name().to_str().and_then(parse_segment_name) {
            out.push((seq, entry.path()));
        }
    }
    out.sort();
    Ok(out)
}

fn encode_header(h: &SegmentHeader) -> Vec<u8> {
    let json = serde_json::to_vec(h).unwrap_or_default();
    let mut out = Vec::with_capacity(16 + json.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&(json.len() as u32).to_le_bytes());
    out.extend_from_slice(&json);
    out.extend_from_slice(&crc32fast::hash(&json).to_le_bytes());
    out
}

/// Returns the header and the offset of the first frame, or a description of
/// what is wrong.
pub(crate) fn decode_header(buf: &[u8]) -> Result<(SegmentHeader, usize), &'static str> {
    if buf.len() < 12 {
        return Err("incomplete segment header");
    }
    if &buf[0..8] != MAGIC {
        return Err("bad segment magic");
    }
    let len = u32::from_le_bytes([buf[8], buf[9], buf[10], buf[11]]) as usize;
    if len > MAX_HEADER_JSON {
        return Err("segment header too large");
    }
    let end = 12 + len + 4;
    if buf.len() < end {
        return Err("incomplete segment header");
    }
    let json = &buf[12..12 + len];
    let crc = u32::from_le_bytes([buf[12 + len], buf[13 + len], buf[14 + len], buf[15 + len]]);
    if crc32fast::hash(json) != crc {
        return Err("segment header checksum mismatch");
    }
    let h: SegmentHeader = serde_json::from_slice(json).map_err(|_| "segment header is not valid JSON")?;
    Ok((h, end))
}

fn read_file(path: &Path) -> Result<Vec<u8>, Error> {
    let mut f = File::open(path).map_err(io_err(format!("open {}", path.display())))?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf)
        .map_err(io_err(format!("read {}", path.display())))?;
    Ok(buf)
}

/// Takes the exclusive lock on a data directory. The lock is released when
/// the returned file is dropped or the process exits.
fn lock_dir(dir: &Path) -> Result<File, Error> {
    let path = dir.join("LOCK");
    let f = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
        .map_err(io_err(format!("open {}", path.display())))?;
    match f.try_lock() {
        Ok(()) => Ok(f),
        Err(fs::TryLockError::WouldBlock) => Err(Error::Locked(dir.to_path_buf())),
        Err(fs::TryLockError::Error(e)) => Err(Error::Io {
            context: format!("lock {}", path.display()),
            source: e,
        }),
    }
}

/// Result of scanning one segment's bytes.
struct Scan {
    header: SegmentHeader,
    /// Offset just after the last valid frame.
    valid_end: usize,
    last_seq: u64,
    /// Set if the bytes after `valid_end` are not a valid frame.
    bad: Option<&'static str>,
}

fn scan_segment<F>(buf: &[u8], expect_first: Option<u64>, after_seq: u64, visit: &mut F) -> Result<Scan, ScanFail>
where
    F: FnMut(u64, u16, &[u8]) -> Result<(), String>,
{
    let (header, mut off) = decode_header(buf).map_err(ScanFail::Header)?;
    if header.format != FORMAT {
        return Err(ScanFail::Fatal(format!(
            "segment format {} is not supported by this build (expects {FORMAT})",
            header.format
        )));
    }
    if let Some(e) = expect_first {
        if header.first_seq != e {
            return Err(ScanFail::Gap(format!(
                "segment starts at {} but {} was expected",
                header.first_seq, e
            )));
        }
    }
    let mut next = header.first_seq;
    let mut bad = None;
    loop {
        match frame::decode(&buf[off..]) {
            Decoded::End => break,
            Decoded::Bad { why } => {
                bad = Some(why);
                break;
            }
            Decoded::Frame { seq, payload, len } => {
                if seq != next {
                    return Err(ScanFail::CorruptAt(
                        off,
                        format!("record sequence {seq} where {next} was expected"),
                    ));
                }
                if seq > after_seq {
                    visit(seq, header.record_schema, payload).map_err(ScanFail::Visitor)?;
                }
                off += len;
                next += 1;
            }
        }
    }
    Ok(Scan {
        header,
        valid_end: off,
        last_seq: next - 1,
        bad,
    })
}

enum ScanFail {
    Header(&'static str),
    Fatal(String),
    Gap(String),
    CorruptAt(usize, String),
    Visitor(String),
}

fn preserve_tail(dir: &Path, segment: &Path, offset: usize, tail: &[u8]) -> Result<PathBuf, Error> {
    let torn_dir = dir.join("torn");
    fs::create_dir_all(&torn_dir).map_err(io_err("create torn dir"))?;
    let name = segment.file_name().and_then(|n| n.to_str()).unwrap_or("segment");
    let mut n = 0u32;
    let path = loop {
        let p = torn_dir.join(format!("{name}.at-{offset}.{n}.bin"));
        if !p.exists() {
            break p;
        }
        n += 1;
    };
    let mut f = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(io_err(format!("create {}", path.display())))?;
    f.write_all(tail).map_err(io_err("write torn tail copy"))?;
    f.sync_all().map_err(io_err("fsync torn tail copy"))?;
    sync_dir(&torn_dir)?;
    sync_dir(dir)?;
    Ok(path)
}

/// Opens a data directory for writing: locks it, verifies the journal from
/// the segment that contains `after_seq + 1`, calls `visit` for every record
/// with a sequence greater than `after_seq`, repairs an incomplete tail, and
/// returns a writer positioned after the last valid record.
///
/// `after_seq` is the sequence covered by the snapshot the caller loaded, or
/// 0. Records up to `after_seq` in the first scanned segment are still
/// checksum-verified but not visited.
pub fn recover<F>(dir: &Path, opts: &Options, after_seq: u64, mut visit: F) -> Result<(Writer, Recovery), Error>
where
    F: FnMut(u64, u16, &[u8]) -> Result<(), String>,
{
    fs::create_dir_all(dir).map_err(io_err(format!("create {}", dir.display())))?;
    let lock = lock_dir(dir)?;
    let all = list_segments(dir)?;

    // Start from the last segment whose first sequence is not beyond the
    // first record we need.
    let need = after_seq + 1;
    if all.is_empty() && after_seq > 0 {
        // Nothing prunes segments, so a snapshot never outlives all of them.
        return Err(Error::Gap {
            detail: format!("a snapshot covers record {after_seq} but the directory holds no journal segment"),
        });
    }
    let mut start = all.iter().rposition(|(first, _)| *first <= need).unwrap_or(0);
    // If the only segment in scope is a newest one whose header never became
    // durable, it will be set aside below; the segment before it is then the
    // one writing continues in, so it has to be verified as well.
    if start > 0 && start == all.len() - 1 && decode_header(&read_file(&all[start].1)?).is_err() {
        start -= 1;
    }
    let segments = &all[start..];
    if let Some((first, _)) = segments.first() {
        if *first > need {
            return Err(Error::Gap {
                detail: format!("journal starts at {first} but records from {need} are needed"),
            });
        }
    }

    let mut report = Recovery {
        last_seq: 0,
        first_seq: all.first().map(|(s, _)| *s).unwrap_or(0),
        records_visited: 0,
        segments: all.len(),
        torn_tail: None,
        instance_id: None,
    };
    let mut expect_first: Option<u64> = None;
    let mut last_good: Option<(PathBuf, u64, SegmentHeader)> = None;
    let mut visited = 0u64;
    // The one repair recovery may make, decided during the scan and carried
    // out only after every check has passed: a refused directory is left
    // exactly as it was found.
    let mut repair: Option<Repair> = None;

    for (idx, (_, path)) in segments.iter().enumerate() {
        let is_last = idx == segments.len() - 1;
        let buf = read_file(path)?;
        let mut counting = |seq: u64, schema: u16, payload: &[u8]| {
            visited += 1;
            visit(seq, schema, payload)
        };
        let scan = match scan_segment(&buf, expect_first, after_seq, &mut counting) {
            Ok(s) => s,
            Err(ScanFail::Header(why)) => {
                // A segment whose header never became durable can only be the
                // newest one, created just before a crash. It holds no record.
                if is_last && buf.len() < 12 + MAX_HEADER_JSON && !contains_frame(&buf) {
                    if !opts.truncate_torn_tail {
                        return Err(Error::Corrupt {
                            segment: path.clone(),
                            offset: 0,
                            detail: format!("{why} (incomplete newest segment)"),
                        });
                    }
                    repair = Some(Repair {
                        segment: path.clone(),
                        keep: 0,
                        tail: buf,
                        why: format!("{why}; the incomplete newest segment was removed"),
                        remove: true,
                    });
                    report.segments -= 1;
                    break;
                }
                return Err(Error::Corrupt {
                    segment: path.clone(),
                    offset: 0,
                    detail: why.to_string(),
                });
            }
            Err(ScanFail::Fatal(detail)) => {
                return Err(Error::Corrupt {
                    segment: path.clone(),
                    offset: 0,
                    detail,
                });
            }
            Err(ScanFail::Gap(detail)) => return Err(Error::Gap { detail }),
            Err(ScanFail::CorruptAt(off, detail)) => {
                return Err(Error::Corrupt {
                    segment: path.clone(),
                    offset: off as u64,
                    detail,
                });
            }
            Err(ScanFail::Visitor(why)) => return Err(Error::Visitor(why)),
        };

        if !opts.readable_schemas.contains(&scan.header.record_schema) {
            return Err(Error::Usage(format!(
                "segment {} was written with record schema {}, which this build cannot read (reads {:?})",
                path.display(),
                scan.header.record_schema,
                opts.readable_schemas
            )));
        }
        if let Some(id) = report.instance_id {
            if id != scan.header.instance_id {
                return Err(Error::Corrupt {
                    segment: path.clone(),
                    offset: 0,
                    detail: "segment belongs to a different instance".into(),
                });
            }
        }
        report.instance_id = Some(scan.header.instance_id);
        if scan.header.instance_id != opts.instance_id {
            return Err(Error::Usage(format!(
                "journal instance id {} differs from the requested {}",
                scan.header.instance_id, opts.instance_id
            )));
        }

        if let Some(why) = scan.bad {
            let tail = &buf[scan.valid_end..];
            // An incomplete tail is acceptable only at the very end of the
            // journal and only if no valid later record hides behind it.
            let later = frame::find_later_frame(tail, scan.last_seq);
            if !is_last || later.is_some() {
                let detail = match later {
                    Some((at, seq)) => format!(
                        "{why}; a valid record {seq} follows at offset {}, so this is damage, not an incomplete tail",
                        scan.valid_end + at
                    ),
                    None => format!("{why} in a segment that is not the newest"),
                };
                return Err(Error::Corrupt {
                    segment: path.clone(),
                    offset: scan.valid_end as u64,
                    detail,
                });
            }
            if !opts.truncate_torn_tail {
                return Err(Error::Corrupt {
                    segment: path.clone(),
                    offset: scan.valid_end as u64,
                    detail: format!("{why} (incomplete tail; truncation is disabled)"),
                });
            }
            repair = Some(Repair {
                segment: path.clone(),
                keep: scan.valid_end,
                tail: tail.to_vec(),
                why: why.to_string(),
                remove: false,
            });
        }

        expect_first = Some(scan.last_seq + 1);
        report.last_seq = scan.last_seq;
        last_good = Some((path.clone(), scan.valid_end as u64, scan.header));
    }
    report.records_visited = visited;

    if report.last_seq < after_seq {
        return Err(Error::Gap {
            detail: format!(
                "journal ends at {} but the snapshot covers {after_seq}",
                report.last_seq
            ),
        });
    }

    // Every check has passed. Preserve the incomplete tail, then cut it.
    if let Some(r) = repair {
        let saved_to = preserve_tail(dir, &r.segment, r.keep, &r.tail)?;
        if r.remove {
            fs::remove_file(&r.segment).map_err(io_err(format!("remove {}", r.segment.display())))?;
            sync_dir(dir)?;
        } else {
            let f = OpenOptions::new()
                .write(true)
                .open(&r.segment)
                .map_err(io_err(format!("open {}", r.segment.display())))?;
            f.set_len(r.keep as u64).map_err(io_err("truncate torn tail"))?;
            f.sync_all().map_err(io_err("fsync after truncate"))?;
        }
        report.torn_tail = Some(TornTail {
            segment: r.segment,
            offset: r.keep as u64,
            bytes: r.tail.len() as u64,
            why: r.why,
            saved_to,
        });
    }

    let writer = match last_good {
        Some((path, len, header)) => {
            // The records read here may still be only in the page cache: a
            // process killed before its `fdatasync` leaves them there. They
            // are reported as durable from now on (the node publishes
            // `last_seq` as its durable seq, and the tail reader trusts it,
            // C-TAIL), so they are made durable first.
            File::open(&path)
                .and_then(|f| f.sync_all())
                .map_err(io_err(format!("fsync {}", path.display())))?;
            // A segment holds records of one schema only. If this build
            // writes another schema, the next record opens a new segment.
            let file = if header.record_schema == opts.record_schema {
                Some(
                    OpenOptions::new()
                        .append(true)
                        .open(&path)
                        .map_err(io_err(format!("open {}", path.display())))?,
                )
            } else {
                None
            };
            Writer {
                dir: dir.to_path_buf(),
                opts: opts.clone(),
                _lock: lock,
                file,
                segment_path: path,
                segment_len: len,
                last_seq: report.last_seq,
                durable_seq: report.last_seq,
                pending: Vec::new(),
                failed: None,
                fail_next_sync: false,
                fail_sync_after_write: false,
                stats: WriterStats::default(),
            }
        }
        None => Writer {
            dir: dir.to_path_buf(),
            opts: opts.clone(),
            _lock: lock,
            file: None,
            segment_path: PathBuf::new(),
            segment_len: 0,
            // With no segment on disk the journal continues after the
            // snapshot, or starts at 1.
            last_seq: after_seq,
            durable_seq: after_seq,
            pending: Vec::new(),
            failed: None,
            fail_next_sync: false,
                fail_sync_after_write: false,
            stats: WriterStats::default(),
        },
    };
    Ok((writer, report))
}

struct Repair {
    segment: PathBuf,
    /// Bytes of the segment that stay.
    keep: usize,
    /// The bytes after `keep`, preserved under `torn/` before they are cut.
    tail: Vec<u8>,
    why: String,
    /// The segment has no usable header and is removed altogether.
    remove: bool,
}

fn contains_frame(buf: &[u8]) -> bool {
    frame::find_later_frame(buf, 0).is_some()
}

/// Reads every segment from the first one, verifying checksums and sequence
/// continuity, without taking the lock or changing anything. For offline
/// tools.
pub fn verify_all<F>(dir: &Path, mut visit: F) -> Result<Recovery, Error>
where
    F: FnMut(u64, u16, &[u8]) -> Result<(), String>,
{
    let all = list_segments(dir)?;
    let mut report = Recovery {
        last_seq: 0,
        first_seq: all.first().map(|(s, _)| *s).unwrap_or(0),
        records_visited: 0,
        segments: all.len(),
        torn_tail: None,
        instance_id: None,
    };
    let mut expect_first = None;
    for (idx, (_, path)) in all.iter().enumerate() {
        let buf = read_file(path)?;
        let mut n = 0u64;
        let mut counting = |seq: u64, schema: u16, payload: &[u8]| {
            n += 1;
            visit(seq, schema, payload)
        };
        let scan = scan_segment(&buf, expect_first, 0, &mut counting).map_err(|e| match e {
            ScanFail::Header(why) => Error::Corrupt {
                segment: path.clone(),
                offset: 0,
                detail: why.to_string(),
            },
            ScanFail::Fatal(detail) => Error::Corrupt {
                segment: path.clone(),
                offset: 0,
                detail,
            },
            ScanFail::Gap(detail) => Error::Gap { detail },
            ScanFail::CorruptAt(off, detail) => Error::Corrupt {
                segment: path.clone(),
                offset: off as u64,
                detail,
            },
            ScanFail::Visitor(why) => Error::Visitor(why),
        })?;
        report.records_visited += n;
        if let Some(why) = scan.bad {
            let tail = &buf[scan.valid_end..];
            let later = frame::find_later_frame(tail, scan.last_seq);
            if idx != all.len() - 1 || later.is_some() {
                return Err(Error::Corrupt {
                    segment: path.clone(),
                    offset: scan.valid_end as u64,
                    detail: why.to_string(),
                });
            }
            report.torn_tail = Some(TornTail {
                segment: path.clone(),
                offset: scan.valid_end as u64,
                bytes: tail.len() as u64,
                why: why.to_string(),
                saved_to: PathBuf::new(),
            });
        }
        report.instance_id = Some(scan.header.instance_id);
        report.last_seq = scan.last_seq;
        expect_first = Some(scan.last_seq + 1);
    }
    Ok(report)
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WriterStats {
    pub syncs: u64,
    pub records: u64,
    pub bytes: u64,
    pub rotations: u64,
}

/// The single writer of a data directory.
pub struct Writer {
    dir: PathBuf,
    opts: Options,
    _lock: File,
    file: Option<File>,
    segment_path: PathBuf,
    segment_len: u64,
    last_seq: u64,
    durable_seq: u64,
    pending: Vec<u8>,
    failed: Option<String>,
    fail_next_sync: bool,
    fail_sync_after_write: bool,
    stats: WriterStats,
}

impl Writer {
    /// Highest sequence appended (not necessarily durable).
    pub fn last_seq(&self) -> u64 {
        self.last_seq
    }

    /// Highest sequence for which `sync` has returned `Ok`.
    pub fn durable_seq(&self) -> u64 {
        self.durable_seq
    }

    pub fn stats(&self) -> &WriterStats {
        &self.stats
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn pending_bytes(&self) -> usize {
        self.pending.len()
    }

    /// Makes the next `sync` fail as if the device had reported an error.
    /// For fault-injection tests.
    pub fn inject_sync_failure(&mut self) {
        self.fail_next_sync = true;
    }

    /// Makes the next `sync` write its bytes and then fail before the flush:
    /// the records may or may not survive, which is what a real flush error
    /// leaves behind. For fault-injection tests.
    pub fn inject_sync_failure_after_write(&mut self) {
        self.fail_sync_after_write = true;
    }

    fn check(&self) -> Result<(), Error> {
        match &self.failed {
            Some(why) => Err(Error::Failed(why.clone())),
            None => Ok(()),
        }
    }

    fn fail<T>(&mut self, e: Error) -> Result<T, Error> {
        if self.failed.is_none() {
            self.failed = Some(e.to_string());
        }
        Err(e)
    }

    /// Buffers one record. Nothing reaches the file until `sync`.
    pub fn append(&mut self, seq: u64, payload: &[u8]) -> Result<(), Error> {
        self.check()?;
        if seq != self.last_seq + 1 {
            return Err(Error::Usage(format!(
                "append of sequence {seq} after {}",
                self.last_seq
            )));
        }
        if payload.len() > frame::MAX_PAYLOAD {
            return Err(Error::Usage(format!("payload of {} bytes is too large", payload.len())));
        }
        frame::encode(seq, payload, &mut self.pending);
        self.last_seq = seq;
        Ok(())
    }

    fn open_segment(&mut self, first_seq: u64, created_at_us: u64) -> Result<(), Error> {
        let path = self.dir.join(segment_name(first_seq));
        let header = SegmentHeader {
            format: FORMAT,
            record_schema: self.opts.record_schema,
            instance_id: self.opts.instance_id,
            first_seq,
            created_at_us,
            writer: self.opts.writer.clone(),
        };
        let bytes = encode_header(&header);
        let f = if path.exists() {
            // The name is taken. That is legitimate in one case only: the
            // previous run rotated and stopped, leaving a segment with a
            // header and no record, and this build writes another record
            // schema. Such a segment holds nothing, so it is replaced
            // atomically. Anything else is refused.
            let existing = read_file(&path)?;
            match decode_header(&existing) {
                Ok((old, end)) if end == existing.len() && old.first_seq == first_seq && old.instance_id == self.opts.instance_id => {}
                _ => {
                    return Err(Error::Usage(format!(
                        "{} already exists and is not an empty segment of this journal",
                        path.display()
                    )));
                }
            }
            let tmp = self.dir.join(format!(".{}.tmp", segment_name(first_seq)));
            let mut t = File::create(&tmp).map_err(io_err(format!("create {}", tmp.display())))?;
            t.write_all(&bytes).map_err(io_err("write segment header"))?;
            t.sync_all().map_err(io_err("fsync segment header"))?;
            fs::rename(&tmp, &path).map_err(io_err(format!("rename {}", tmp.display())))?;
            OpenOptions::new()
                .append(true)
                .open(&path)
                .map_err(io_err(format!("open {}", path.display())))?
        } else {
            let mut f = OpenOptions::new()
                .append(true)
                .create_new(true)
                .open(&path)
                .map_err(io_err(format!("create {}", path.display())))?;
            f.write_all(&bytes).map_err(io_err("write segment header"))?;
            f.sync_all().map_err(io_err("fsync segment header"))?;
            f
        };
        sync_dir(&self.dir)?;
        self.file = Some(f);
        self.segment_path = path;
        self.segment_len = bytes.len() as u64;
        Ok(())
    }

    /// Writes every buffered record and makes it durable. On `Ok`, all
    /// records up to `last_seq` are durable. On `Err`, the journal is failed
    /// for good and the caller must stop: after a failed flush the kernel may
    /// have dropped the data while reporting success to a later flush.
    pub fn sync(&mut self, now_us: u64) -> Result<u64, Error> {
        self.check()?;
        if self.pending.is_empty() {
            return Ok(self.durable_seq);
        }
        if self.file.is_none() {
            let first = self.durable_seq + 1;
            if let Err(e) = self.open_segment(first, now_us) {
                return self.fail(e);
            }
        }
        if self.fail_next_sync {
            self.fail_next_sync = false;
            return self.fail(Error::Io {
                context: "fdatasync (injected failure)".into(),
                source: std::io::Error::other("injected"),
            });
        }
        let Some(f) = self.file.as_mut() else {
            return self.fail(Error::Usage("no open segment".into()));
        };
        if let Err(e) = f.write_all(&self.pending) {
            return self.fail(Error::Io {
                context: format!("write {}", self.segment_path.display()),
                source: e,
            });
        }
        if self.fail_sync_after_write {
            self.fail_sync_after_write = false;
            return self.fail(Error::Io {
                context: "fdatasync after write (injected failure)".into(),
                source: std::io::Error::other("injected"),
            });
        }
        if let Err(e) = f.sync_data() {
            return self.fail(Error::Io {
                context: format!("fdatasync {}", self.segment_path.display()),
                source: e,
            });
        }
        self.stats.syncs += 1;
        self.stats.records += self.last_seq - self.durable_seq;
        self.stats.bytes += self.pending.len() as u64;
        self.segment_len += self.pending.len() as u64;
        self.pending.clear();
        self.durable_seq = self.last_seq;

        if self.segment_len >= self.opts.segment_max_bytes {
            // Rotation happens only at a sync boundary, so a batch never
            // spans two segments.
            self.file = None;
            if let Err(e) = self.open_segment(self.durable_seq + 1, now_us) {
                return self.fail(e);
            }
            self.stats.rotations += 1;
        }
        Ok(self.durable_seq)
    }
}
