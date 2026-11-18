//! zstd frames (RFC 8878): a frame encoder with block flush, a scanner that
//! finds frame ends and the last complete block of an unfinished frame, and
//! a decoder over concatenated frames. Only the safe `zstd-safe` API is used.

use std::collections::HashMap;
use std::io;
use std::sync::{Arc, Mutex, OnceLock};

use zstd::zstd_safe::{self, CCtx, CParameter, DCtx, InBuffer, OutBuffer, zstd_sys};

const MAGIC: u32 = 0xFD2F_B528;
/// Block_Maximum_Size: no block is larger (RFC 8878 3.1.1.2.3).
const MAX_BLOCK: usize = 128 * 1024;
const CHUNK: usize = 128 * 1024;

fn zerr(code: usize) -> io::Error {
    io::Error::other(zstd_safe::get_error_name(code).to_string())
}

#[derive(Debug, Default)]
struct Meter {
    live: usize,
    max_live: usize,
    live_bytes: usize,
    max_live_bytes: usize,
    pledged: Vec<Option<u64>>,
}

/// Counts the compression contexts alive at once and their sizes (a shared
/// handle: clones count into the same meter).
#[derive(Debug, Clone, Default)]
pub struct Contexts(Arc<Mutex<Meter>>);

impl Contexts {
    fn with<T>(&self, f: impl FnOnce(&mut Meter) -> T) -> T {
        let mut m = self.0.lock().unwrap_or_else(|e| e.into_inner());
        f(&mut m)
    }
    pub fn live(&self) -> usize {
        self.with(|m| m.live)
    }
    pub fn max_live(&self) -> usize {
        self.with(|m| m.max_live)
    }
    /// The sum of the estimated sizes of the live contexts.
    pub fn live_bytes(&self) -> usize {
        self.with(|m| m.live_bytes)
    }
    pub fn max_live_bytes(&self) -> usize {
        self.with(|m| m.max_live_bytes)
    }
    /// The pledged size of every context created, in order.
    pub fn pledged(&self) -> Vec<Option<u64>> {
        self.with(|m| m.pledged.clone())
    }
}

/// (level, pledged size rounded up) → measured context bytes.
type SizeCache = HashMap<(i32, Option<u64>), usize>;

/// The memory of a streaming context at `level` for a frame of `pledged`
/// bytes, measured on a probe context (`ZSTD_sizeof_CCtx` after init) and
/// cached. The size is rounded up to a power of two first, which never gives
/// a smaller context than the exact size would.
pub fn context_bytes(level: i32, pledged: Option<u64>) -> usize {
    static CACHE: OnceLock<Mutex<SizeCache>> = OnceLock::new();
    let key = (level, pledged.map(|n| n.max(1024).next_power_of_two()));
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(&n) = cache.lock().unwrap_or_else(|e| e.into_inner()).get(&key) {
        return n;
    }
    let mut c = CCtx::create();
    let mut out: Vec<u8> = Vec::with_capacity(CHUNK);
    let measured = (|| -> Option<usize> {
        c.set_parameter(CParameter::CompressionLevel(level)).ok()?;
        c.set_pledged_src_size(key.1).ok()?;
        let mut ob = OutBuffer::around_pos(&mut out, 0);
        let mut ib = InBuffer::around(&[]);
        c.compress_stream2(&mut ob, &mut ib, zstd_sys::ZSTD_EndDirective::ZSTD_e_flush)
            .ok()?;
        Some(c.sizeof())
    })()
    .unwrap_or(0);
    cache
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(key, measured);
    measured
}

/// One zstd frame being written. The context is counted in `Contexts` from
/// creation until drop.
pub struct FrameEncoder {
    cctx: CCtx<'static>,
    contexts: Contexts,
    bytes: usize,
    finished: bool,
}

impl FrameEncoder {
    /// `pledged`: the exact number of bytes this frame will hold, when known;
    /// zstd then sizes the context for it (a small topic gets a small one).
    pub fn new(level: i32, pledged: Option<u64>, contexts: &Contexts) -> io::Result<Self> {
        let mut cctx = CCtx::create();
        cctx.set_parameter(CParameter::CompressionLevel(level))
            .map_err(zerr)?;
        cctx.set_parameter(CParameter::ChecksumFlag(true))
            .map_err(zerr)?;
        cctx.set_pledged_src_size(pledged).map_err(zerr)?;
        let bytes = context_bytes(level, pledged);
        contexts.with(|m| {
            m.live += 1;
            m.max_live = m.max_live.max(m.live);
            m.live_bytes += bytes;
            m.max_live_bytes = m.max_live_bytes.max(m.live_bytes);
            m.pledged.push(pledged);
        });
        Ok(Self {
            cctx,
            contexts: contexts.clone(),
            bytes,
            finished: false,
        })
    }

    fn run(
        &mut self,
        input: &[u8],
        end: zstd_sys::ZSTD_EndDirective,
        out: &mut Vec<u8>,
    ) -> io::Result<()> {
        if self.finished {
            return Err(io::Error::other("frame already finished"));
        }
        let mut ib = InBuffer::around(input);
        loop {
            out.reserve(CHUNK);
            let pos = out.len();
            let mut ob = OutBuffer::around_pos(out, pos);
            let left = self
                .cctx
                .compress_stream2(&mut ob, &mut ib, end)
                .map_err(zerr)?;
            let done = match end {
                zstd_sys::ZSTD_EndDirective::ZSTD_e_continue => ib.pos == input.len(),
                _ => left == 0 && ib.pos == input.len(),
            };
            if done {
                return Ok(());
            }
        }
    }

    pub fn write(&mut self, data: &[u8], out: &mut Vec<u8>) -> io::Result<()> {
        self.run(data, zstd_sys::ZSTD_EndDirective::ZSTD_e_continue, out)
    }

    /// Ends the current block (`ZSTD_e_flush`): everything written so far can
    /// be decoded from `out`. The window is kept.
    pub fn flush(&mut self, out: &mut Vec<u8>) -> io::Result<()> {
        self.run(&[], zstd_sys::ZSTD_EndDirective::ZSTD_e_flush, out)
    }

    /// Ends the frame (with its checksum).
    pub fn finish(&mut self, out: &mut Vec<u8>) -> io::Result<()> {
        self.run(&[], zstd_sys::ZSTD_EndDirective::ZSTD_e_end, out)?;
        self.finished = true;
        Ok(())
    }

    pub fn context_bytes(&self) -> usize {
        self.bytes
    }
}

impl Drop for FrameEncoder {
    fn drop(&mut self) {
        let bytes = self.bytes;
        self.contexts.with(|m| {
            m.live -= 1;
            m.live_bytes -= bytes;
        });
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameSpan {
    pub start: usize,
    pub end: usize,
}

/// The last frame of a file that has no frame end (its writer crashed).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unfinished {
    pub start: usize,
    /// Offset after its last complete block (`start` when it holds none).
    /// Cutting the file here keeps every block that can be decoded.
    pub complete_blocks_end: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Scan {
    pub frames: Vec<FrameSpan>,
    pub unfinished: Option<Unfinished>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScanError {
    BadMagic { at: usize },
    ReservedBit { at: usize },
    BadBlock { at: usize },
}

/// Reads the frame and block headers of concatenated zstd frames without
/// decompressing. A file whose end lies inside a frame gives `unfinished`.
pub fn scan(b: &[u8]) -> Result<Scan, ScanError> {
    scan_with(b.len() as u64, |at, buf| {
        let at = at as usize;
        let n = buf.len().min(b.len().saturating_sub(at));
        buf[..n].copy_from_slice(&b[at..at + n]);
        Ok(n)
    })
    .map_err(|e| match e {
        ScanIo::Scan(s) => s,
        ScanIo::Io(_) => ScanError::BadMagic { at: 0 },
    })
}

#[derive(Debug)]
pub enum ScanIo {
    Scan(ScanError),
    Io(io::Error),
}

/// `scan` over a file of `len` bytes read by `read_at(offset, buf)`: only
/// the frame and block headers are read, a few bytes per 128 KiB block.
pub fn scan_with(
    len: u64,
    mut read_at: impl FnMut(u64, &mut [u8]) -> io::Result<usize>,
) -> Result<Scan, ScanIo> {
    let mut out = Scan::default();
    let mut at: u64 = 0;
    let mut read = |at: u64, n: usize| -> Result<Vec<u8>, ScanIo> {
        let mut buf = vec![0; n];
        let mut got = 0;
        while got < n {
            let k = read_at(at + got as u64, &mut buf[got..]).map_err(ScanIo::Io)?;
            if k == 0 {
                break;
            }
            got += k;
        }
        buf.truncate(got);
        Ok(buf)
    };
    while at < len {
        let start = at as usize;
        let left = |at: u64| len - at;
        let unfinished = |end: u64| Unfinished {
            start,
            complete_blocks_end: end as usize,
        };
        if left(at) < 4 {
            let tail = read(at, left(at) as usize)?;
            if MAGIC.to_le_bytes().starts_with(&tail) {
                out.unfinished = Some(unfinished(at));
                return Ok(out);
            }
            return Err(ScanIo::Scan(ScanError::BadMagic { at: start }));
        }
        let m = read(at, 4)?;
        if u32::from_le_bytes([m[0], m[1], m[2], m[3]]) != MAGIC {
            return Err(ScanIo::Scan(ScanError::BadMagic { at: start }));
        }
        at += 4;
        if left(at) < 1 {
            out.unfinished = Some(unfinished(start as u64));
            return Ok(out);
        }
        let fhd = read(at, 1)?[0];
        if fhd & 0x08 != 0 {
            return Err(ScanIo::Scan(ScanError::ReservedBit { at: at as usize }));
        }
        let single = fhd & 0x20 != 0;
        let checksum = fhd & 0x04 != 0;
        let dict = [0u64, 1, 2, 4][(fhd & 0x03) as usize];
        let fcs = match (fhd >> 6, single) {
            (0, false) => 0,
            (0, true) => 1,
            (1, _) => 2,
            (2, _) => 4,
            _ => 8,
        };
        let header = 1 + u64::from(!single) + dict + fcs;
        if left(at) < header {
            out.unfinished = Some(unfinished(start as u64));
            return Ok(out);
        }
        at += header;
        let mut last_complete = at;
        loop {
            if left(at) < 3 {
                out.unfinished = Some(unfinished(last_complete));
                return Ok(out);
            }
            let hb = read(at, 3)?;
            let h = u32::from(hb[0]) | (u32::from(hb[1]) << 8) | (u32::from(hb[2]) << 16);
            let last = h & 1 == 1;
            let size = (h >> 3) as u64;
            let content = match (h >> 1) & 3 {
                0 | 2 => size,
                1 => 1,
                _ => return Err(ScanIo::Scan(ScanError::BadBlock { at: at as usize })),
            };
            if size > MAX_BLOCK as u64 {
                return Err(ScanIo::Scan(ScanError::BadBlock { at: at as usize }));
            }
            if left(at) - 3 < content {
                out.unfinished = Some(unfinished(last_complete));
                return Ok(out);
            }
            at += 3 + content;
            last_complete = at;
            if last {
                break;
            }
        }
        if checksum {
            if left(at) < 4 {
                out.unfinished = Some(unfinished(last_complete));
                return Ok(out);
            }
            at += 4;
        }
        out.frames.push(FrameSpan {
            start,
            end: at as usize,
        });
    }
    Ok(out)
}

/// A streaming decoder of concatenated frames: feed compressed bytes, get
/// decompressed bytes, in bounded memory.
pub struct StreamDecoder {
    d: DCtx<'static>,
}

impl Default for StreamDecoder {
    fn default() -> Self {
        Self { d: DCtx::create() }
    }
}

impl StreamDecoder {
    /// Decompresses `input` into `out` (appended). Returns the bytes of
    /// `input` consumed; call again with the rest.
    pub fn feed(&mut self, input: &[u8], out: &mut Vec<u8>) -> io::Result<usize> {
        let mut ib = InBuffer::around(input);
        loop {
            out.reserve(CHUNK);
            let pos = out.len();
            let mut ob = OutBuffer::around_pos(out, pos);
            self.d.decompress_stream(&mut ob, &mut ib).map_err(zerr)?;
            let produced = ob.pos() > pos;
            if ib.pos == input.len() && !produced {
                return Ok(ib.pos);
            }
            if out.len() >= 4 * CHUNK {
                return Ok(ib.pos);
            }
        }
    }
}

fn decompress(input: &[u8], whole: bool) -> io::Result<Vec<u8>> {
    let mut d = DCtx::create();
    let mut ib = InBuffer::around(input);
    let mut out: Vec<u8> = Vec::new();
    loop {
        out.reserve(CHUNK);
        let pos = out.len();
        let mut ob = OutBuffer::around_pos(&mut out, pos);
        let hint = d.decompress_stream(&mut ob, &mut ib).map_err(zerr)?;
        let produced = ob.pos() > pos;
        if whole && hint == 0 {
            if ib.pos != input.len() {
                return Err(io::Error::other("bytes after the frame end"));
            }
            return Ok(out);
        }
        if ib.pos == input.len() && !produced {
            if whole {
                return Err(io::Error::other("frame ends early"));
            }
            return Ok(out);
        }
    }
}

/// The decompressed content of each frame, in order: the segments of one
/// QRAW1 stream. An unfinished last frame gives what its complete blocks hold.
pub fn decode_frames(b: &[u8], scan: &Scan) -> io::Result<Vec<Vec<u8>>> {
    let mut segments = Vec::with_capacity(scan.frames.len() + 1);
    for f in &scan.frames {
        segments.push(decompress(&b[f.start..f.end], true)?);
    }
    if let Some(u) = scan.unfinished {
        if u.complete_blocks_end > u.start {
            segments.push(decompress(&b[u.start..u.complete_blocks_end], false)?);
        }
    }
    Ok(segments)
}
