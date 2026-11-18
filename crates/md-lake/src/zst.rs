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
    let mut out = Scan::default();
    let mut at = 0;
    while at < b.len() {
        let start = at;
        let unfinished = |end: usize| Unfinished {
            start,
            complete_blocks_end: end,
        };
        if b.len() - at < 4 {
            if MAGIC.to_le_bytes().starts_with(&b[at..]) {
                out.unfinished = Some(unfinished(start));
                return Ok(out);
            }
            return Err(ScanError::BadMagic { at });
        }
        if u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]]) != MAGIC {
            return Err(ScanError::BadMagic { at });
        }
        at += 4;
        let Some(&fhd) = b.get(at) else {
            out.unfinished = Some(unfinished(start));
            return Ok(out);
        };
        if fhd & 0x08 != 0 {
            return Err(ScanError::ReservedBit { at });
        }
        let fcs_flag = fhd >> 6;
        let single = fhd & 0x20 != 0;
        let checksum = fhd & 0x04 != 0;
        let dict = [0, 1, 2, 4][(fhd & 0x03) as usize];
        let fcs = match (fcs_flag, single) {
            (0, false) => 0,
            (0, true) => 1,
            (1, _) => 2,
            (2, _) => 4,
            _ => 8,
        };
        let header = 1 + usize::from(!single) + dict + fcs;
        if b.len() - at < header {
            out.unfinished = Some(unfinished(start));
            return Ok(out);
        }
        at += header;
        let mut last_complete = at;
        loop {
            if b.len() - at < 3 {
                out.unfinished = Some(unfinished(last_complete));
                return Ok(out);
            }
            let h = u32::from(b[at]) | (u32::from(b[at + 1]) << 8) | (u32::from(b[at + 2]) << 16);
            let last = h & 1 == 1;
            let kind = (h >> 1) & 3;
            let size = (h >> 3) as usize;
            let content = match kind {
                0 | 2 => size,
                1 => 1,
                _ => return Err(ScanError::BadBlock { at }),
            };
            if size > MAX_BLOCK {
                return Err(ScanError::BadBlock { at });
            }
            if b.len() - at - 3 < content {
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
            if b.len() - at < 4 {
                out.unfinished = Some(unfinished(last_complete));
                return Ok(out);
            }
            at += 4;
        }
        out.frames.push(FrameSpan { start, end: at });
    }
    Ok(out)
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
