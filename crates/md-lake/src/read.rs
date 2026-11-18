//! Streaming record readers over zstd files of the lake, in bounded memory.

use std::io;

use crate::fs::LakeFile;
use crate::qraw::{self, FramingError, Header, Record};
use crate::zst::{self, StreamDecoder};

const READ_CHUNK: usize = 256 * 1024;

/// Reads the records of `[start, end)` of a file: one or more whole zstd
/// frames, or an unfinished frame cut at its last complete block. With
/// `has_header`, the content starts with magic and header.
pub struct RecordIter<'a> {
    file: &'a mut dyn LakeFile,
    pos: u64,
    end: u64,
    dec: StreamDecoder,
    input: Vec<u8>,
    in_at: usize,
    out: Vec<u8>,
    out_at: usize,
    header: Option<Header>,
    want_header: bool,
    rec_no: usize,
    /// The content ended inside a record (2.2).
    pub truncated_tail: bool,
    failed: bool,
}

impl<'a> RecordIter<'a> {
    pub fn new(file: &'a mut dyn LakeFile, start: u64, end: u64, has_header: bool) -> Self {
        Self {
            file,
            pos: start,
            end,
            dec: StreamDecoder::default(),
            input: Vec::new(),
            in_at: 0,
            out: Vec::new(),
            out_at: 0,
            header: None,
            want_header: has_header,
            rec_no: 0,
            truncated_tail: false,
            failed: false,
        }
    }

    /// The header, once the first record (or the end) has been read.
    pub fn header(&self) -> Option<&Header> {
        self.header.as_ref()
    }

    /// Decompresses more; `false` when the input is used up and nothing came out.
    fn fill(&mut self) -> io::Result<bool> {
        if self.out_at > 0 {
            self.out.drain(..self.out_at);
            self.out_at = 0;
        }
        loop {
            if self.in_at == self.input.len() {
                if self.pos >= self.end {
                    // Drain what the decoder still holds.
                    let before = self.out.len();
                    self.dec.feed(&[], &mut self.out)?;
                    return Ok(self.out.len() > before);
                }
                let n = READ_CHUNK.min((self.end - self.pos) as usize);
                self.input.resize(n, 0);
                let mut got = 0;
                while got < n {
                    let k = self
                        .file
                        .read_at(self.pos + got as u64, &mut self.input[got..])?;
                    if k == 0 {
                        return Err(io::Error::new(
                            io::ErrorKind::UnexpectedEof,
                            "file shorter than expected",
                        ));
                    }
                    got += k;
                }
                self.pos += n as u64;
                self.in_at = 0;
            }
            let before = self.out.len();
            let used = self.dec.feed(&self.input[self.in_at..], &mut self.out)?;
            self.in_at += used;
            if self.out.len() > before {
                return Ok(true);
            }
        }
    }

    fn next_record(&mut self) -> io::Result<Option<Record>> {
        loop {
            let avail = &self.out[self.out_at..];
            if self.want_header {
                match qraw::decode_header(avail) {
                    Ok((h, end)) => {
                        self.header = Some(h);
                        self.out_at += end;
                        self.want_header = false;
                        continue;
                    }
                    Err(FramingError::NoMagic) if avail.len() >= 6 => {
                        return Err(io::Error::other("no QRAW1 magic"));
                    }
                    Err(FramingError::BadHeader(e)) => return Err(io::Error::other(e)),
                    Err(_) => {}
                }
            } else if !avail.is_empty() {
                if let Some((r, used)) = qraw::decode_record(avail, 0, self.rec_no)
                    .map_err(|e| io::Error::other(format!("{e:?}")))?
                {
                    self.out_at += used;
                    self.rec_no += 1;
                    return Ok(Some(r));
                }
            }
            if !self.fill()? {
                self.truncated_tail = self.out_at < self.out.len() || self.want_header;
                return Ok(None);
            }
        }
    }
}

impl Iterator for RecordIter<'_> {
    type Item = io::Result<Record>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.failed {
            return None;
        }
        match self.next_record() {
            Ok(Some(r)) => Some(Ok(r)),
            Ok(None) => None,
            Err(e) => {
                self.failed = true;
                Some(Err(e))
            }
        }
    }
}

/// The frame structure of a whole lake file, read header by header.
pub fn scan_file(file: &mut dyn LakeFile) -> io::Result<zst::Scan> {
    let len = file.len()?;
    zst::scan_with(len, |at, buf| file.read_at(at, buf)).map_err(|e| match e {
        zst::ScanIo::Io(e) => e,
        zst::ScanIo::Scan(s) => io::Error::new(io::ErrorKind::InvalidData, format!("{s:?}")),
    })
}
