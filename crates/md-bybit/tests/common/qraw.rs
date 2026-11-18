//! A test-only QRAW1 reader for the golden L0 files (lake contract 2.2). The
//! golden files are uncompressed segments `<HH>.qraw` (or `stream.qraw`) under
//! `golden/l0/<l0_file_id>/`; read in name order they form one stream.

use std::fs;
use std::path::{Path, PathBuf};

pub const MAGIC: &[u8; 6] = b"QRAW1\0";
const FIXED: usize = 25;

#[derive(Debug, Clone)]
pub struct Record {
    pub recv_ns: i64,
    pub src: u8,
    pub conn_id: u32,
    pub frame_no: u64,
    pub payload: Vec<u8>,
}

pub fn golden_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../contracts/lake/golden")
}

fn take<const N: usize>(b: &[u8], at: usize) -> [u8; N] {
    let mut out = [0; N];
    out.copy_from_slice(&b[at..at + N]);
    out
}

/// The records of one stream, `rec_no` = index. A segment must hold whole
/// records; a torn record is an error, never a short list.
pub fn read_segments(segments: &[Vec<u8>]) -> Result<Vec<Record>, String> {
    let mut records = Vec::new();
    for (i, seg) in segments.iter().enumerate() {
        let mut at = 0;
        if i == 0 {
            if seg.len() < 10 || &seg[..6] != MAGIC {
                return Err("no QRAW1 magic".into());
            }
            let header_len = u32::from_le_bytes(take(seg, 6)) as usize;
            at = 10 + header_len;
            if at > seg.len() {
                return Err("torn header".into());
            }
        }
        while at < seg.len() {
            if seg.len() - at < FIXED {
                return Err(format!("segment {i}: torn record at {at}"));
            }
            let recv_ns = u64::from_le_bytes(take(seg, at));
            let src = seg[at + 8];
            let conn_id = u32::from_le_bytes(take(seg, at + 9));
            let frame_no = u64::from_le_bytes(take(seg, at + 13));
            let len = u32::from_le_bytes(take(seg, at + 21)) as usize;
            let start = at + FIXED;
            if src > 2 {
                return Err(format!("segment {i}: src {src}"));
            }
            if seg.len() - start < len {
                return Err(format!("segment {i}: torn payload at {at}"));
            }
            records.push(Record {
                recv_ns: i64::try_from(recv_ns).map_err(|e| e.to_string())?,
                src,
                conn_id,
                frame_no,
                payload: seg[start..start + len].to_vec(),
            });
            at = start + len;
        }
    }
    Ok(records)
}

/// The records of the golden L0 file `l0_file_id`.
pub fn read_file(l0_file_id: &str) -> Vec<Record> {
    let dir = golden_dir().join("l0").join(l0_file_id);
    let mut names: Vec<PathBuf> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .map(|e| e.expect("dir entry").path())
        .filter(|p| p.extension().is_some_and(|x| x == "qraw"))
        .collect();
    names.sort();
    let segments: Vec<Vec<u8>> = names
        .iter()
        .map(|p| fs::read(p).expect("read segment"))
        .collect();
    read_segments(&segments).unwrap_or_else(|e| panic!("{l0_file_id}: {e}"))
}
