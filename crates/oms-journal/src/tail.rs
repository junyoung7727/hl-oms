//! A read-only follower of a live journal (U14, docs/contracts/tail.md).
//!
//! Another process (the projector, the watcher) reads the journal while the
//! OMS writes it. The follower never takes `LOCK`, never writes, and never
//! returns a record above the durable sequence the caller supplies: a frame
//! beyond it may still be torn, and after a crash recovery may truncate it
//! and write a different record under the same sequence.
//!
//! - [`Tail::read`] returns records up to the durable cap and advances.
//! - [`Tail::peek_beyond`] returns the valid frames after the position,
//!   without a cap and without advancing. It is for a reader that knows the
//!   writer is dead and labels what it shows as unconfirmed.

use crate::frame::{self, Decoded};
use crate::segment::{SegmentHeader, decode_header, list_segments};
use crate::{Error, io_err};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// One record as stored: its sequence, the record schema of its segment and
/// the payload bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TailRecord {
    pub seq: u64,
    pub record_schema: u16,
    pub payload: Vec<u8>,
}

/// Where the follower stands inside one segment.
#[derive(Clone, Debug)]
struct Position {
    path: PathBuf,
    header: SegmentHeader,
    /// Offset of the next frame to read.
    offset: u64,
}

pub struct Tail {
    dir: PathBuf,
    /// The next sequence this follower returns.
    next_seq: u64,
    at: Option<Position>,
}

impl Tail {
    /// A follower that returns records after `after_seq` (0: from the first
    /// record). Opens nothing yet.
    pub fn new(dir: &Path, after_seq: u64) -> Tail {
        Tail {
            dir: dir.to_path_buf(),
            next_seq: after_seq + 1,
            at: None,
        }
    }

    /// The sequence the next record will have.
    pub fn next_seq(&self) -> u64 {
        self.next_seq
    }

    /// Up to `max` records with sequence at most `durable_seq`, in order.
    /// Returns fewer, or none, when the segment files do not hold them yet.
    /// A damaged frame at or below the cap is an error, and the position
    /// does not move: the same call fails again until the damage is
    /// repaired.
    pub fn read(&mut self, durable_seq: u64, max: usize) -> Result<Vec<TailRecord>, Error> {
        let mut out = Vec::new();
        while out.len() < max && self.next_seq <= durable_seq {
            let room = (durable_seq - self.next_seq + 1).min((max - out.len()) as u64) as usize;
            let (records, new_pos) = match self.scan(room, true) {
                Ok(r) => r,
                // What came before the damage is returned; the next call
                // reports it.
                Err(_) if !out.is_empty() => break,
                Err(e) => return Err(e),
            };
            if records.is_empty() {
                break;
            }
            self.next_seq += records.len() as u64;
            self.at = new_pos;
            out.extend(records);
        }
        Ok(out)
    }

    /// Every valid frame after the position, up to `max`, without moving.
    /// Stops at the first frame that is incomplete or damaged: past the
    /// durable sequence that is an incomplete tail, not an error. Best
    /// effort: a segment whose header cannot be read yet (one being created)
    /// also ends it; only an error before the first record is returned.
    pub fn peek_beyond(&self, max: usize) -> Result<Vec<TailRecord>, Error> {
        let mut probe = Tail {
            dir: self.dir.clone(),
            next_seq: self.next_seq,
            at: self.at.clone(),
        };
        let mut out = Vec::new();
        while out.len() < max {
            let (records, new_pos) = match probe.scan(max - out.len(), false) {
                Ok(r) => r,
                Err(e) if out.is_empty() => return Err(e),
                Err(_) => break,
            };
            if records.is_empty() {
                break;
            }
            probe.next_seq += records.len() as u64;
            probe.at = new_pos;
            out.extend(records);
        }
        Ok(out)
    }

    /// Reads up to `max` frames starting at `next_seq` from the segment that
    /// holds it. Returns the frames and the position after them. With
    /// `strict`, a damaged frame is an error; otherwise it ends the scan.
    fn scan(&self, max: usize, strict: bool) -> Result<(Vec<TailRecord>, Option<Position>), Error> {
        let Some(mut pos) = self.position_for_next()? else {
            return Ok((Vec::new(), self.at.clone()));
        };
        let mut file =
            File::open(&pos.path).map_err(io_err(format!("open {}", pos.path.display())))?;
        file.seek(SeekFrom::Start(pos.offset))
            .map_err(io_err(format!("seek {}", pos.path.display())))?;
        let mut buf = Vec::new();
        file.read_to_end(&mut buf)
            .map_err(io_err(format!("read {}", pos.path.display())))?;
        let mut off = 0usize;
        let mut out = Vec::new();
        let mut expect = self.next_seq;
        while out.len() < max {
            match frame::decode(&buf[off..]) {
                Decoded::End => break,
                Decoded::Bad { why } => {
                    // A frame the writer has not finished, or damage. Below
                    // the cap it must be complete: the caller says it is
                    // durable. The good records before it are returned
                    // first; the next call reports the damage.
                    if strict && out.is_empty() {
                        return Err(Error::Corrupt {
                            segment: pos.path.clone(),
                            offset: pos.offset + off as u64,
                            detail: format!(
                                "record {expect} is durable but its frame is not valid: {why}"
                            ),
                        });
                    }
                    break;
                }
                Decoded::Frame { seq, payload, len } => {
                    if seq != expect {
                        return Err(Error::Corrupt {
                            segment: pos.path.clone(),
                            offset: pos.offset + off as u64,
                            detail: format!("record sequence {seq} where {expect} was expected"),
                        });
                    }
                    out.push(TailRecord {
                        seq,
                        record_schema: pos.header.record_schema,
                        payload: payload.to_vec(),
                    });
                    off += len;
                    expect += 1;
                }
            }
        }
        pos.offset += off as u64;
        Ok((out, Some(pos)))
    }

    /// The segment and offset where `next_seq` is, if a segment holds it or
    /// will hold it. When the current segment has no more frames and the
    /// next segment starts at `next_seq`, moves there.
    fn position_for_next(&self) -> Result<Option<Position>, Error> {
        let segments = list_segments(&self.dir)?;
        if let Some(pos) = &self.at {
            // A later segment that starts exactly here takes over; the
            // writer only opens one after the current one is complete.
            let next = segments.iter().find(|(first, _)| *first == self.next_seq);
            if let Some((_, path)) = next {
                if *path != pos.path {
                    return open_at_start(path).map(Some);
                }
            }
            // A header-only segment may have been rewritten with a new
            // schema (C-JOURNAL): read its header again before any frame.
            if pos.header.first_seq == self.next_seq {
                return open_at_start(&pos.path).map(Some);
            }
            return Ok(Some(pos.clone()));
        }
        // First call: the last segment that starts at or before next_seq.
        let Some((first, path)) = segments
            .iter()
            .rev()
            .find(|(first, _)| *first <= self.next_seq)
        else {
            return Ok(None);
        };
        let mut pos = open_at_start(path)?;
        if *first == self.next_seq {
            return Ok(Some(pos));
        }
        // Skip the records before next_seq.
        let mut file = File::open(path).map_err(io_err(format!("open {}", path.display())))?;
        let mut buf = Vec::new();
        file.read_to_end(&mut buf)
            .map_err(io_err(format!("read {}", path.display())))?;
        let mut off = pos.offset as usize;
        let mut seq = *first;
        while seq < self.next_seq {
            match frame::decode(&buf[off..]) {
                Decoded::Frame { len, .. } => {
                    off += len;
                    seq += 1;
                }
                // Not written yet (or not valid): the reader waits.
                _ => return Ok(None),
            }
        }
        pos.offset = off as u64;
        Ok(Some(pos))
    }
}

fn open_at_start(path: &Path) -> Result<Position, Error> {
    let mut file = File::open(path).map_err(io_err(format!("open {}", path.display())))?;
    let mut buf = Vec::new();
    file.read_to_end(&mut buf)
        .map_err(io_err(format!("read {}", path.display())))?;
    let (header, start) = decode_header(&buf).map_err(|why| Error::Corrupt {
        segment: path.to_path_buf(),
        offset: 0,
        detail: why.to_string(),
    })?;
    Ok(Position {
        path: path.to_path_buf(),
        header,
        offset: start as u64,
    })
}
