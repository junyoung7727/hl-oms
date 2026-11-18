//! `verify` (lake contract 5.2): decodes every sealed file of a date frame by
//! frame and reports what it holds. It does not parse payloads.

use std::io;

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::fs::{Fs, parent};
use crate::paths::L0Path;
use crate::qraw;
use crate::read::{RecordIter, scan_file};
use crate::seal::Clock;

/// The sealed files of a date (no `.partial`, no sidecar).
fn sealed_files(fs: &dyn Fs, venue: &str, date: &str) -> io::Result<Vec<String>> {
    let root = format!("crypto/l0/venue={venue}");
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
    let mut out = Vec::new();
    for d in dirs {
        for n in fs.list(&d)? {
            if n.ends_with(".qraw.zst") {
                out.push(format!("{d}/{n}"));
            }
        }
    }
    out.sort_by_key(|p| {
        L0Path::parse(p)
            .ok()
            .and_then(|x| x.l0_file_id())
            .unwrap_or_default()
    });
    Ok(out)
}

fn verify_file(fs: &dyn Fs, path: &str) -> io::Result<Value> {
    let id = L0Path::parse(path)
        .ok()
        .and_then(|x| x.l0_file_id())
        .unwrap_or_default();
    let mut f = fs.open(path)?;
    let len = f.len()?;
    // SHA-256 of the file as read, in chunks.
    let mut sha = Sha256::new();
    let mut buf = vec![0; 1 << 20];
    let mut at = 0u64;
    while at < len {
        let n = f.read_at(at, &mut buf)?;
        if n == 0 {
            break;
        }
        sha.update(&buf[..n]);
        at += n as u64;
    }
    let mut errors = 0u64;
    let mut first_error: Option<String> = None;
    let mut note = |e: String, errors: &mut u64| {
        *errors += 1;
        if first_error.is_none() {
            first_error = Some(e);
        }
    };
    let (mut frames, mut records) = (0u64, 0u64);
    let (mut first, mut last) = (None, None);
    match scan_file(f.as_mut()) {
        Err(e) => note(format!("zstd framing: {e}"), &mut errors),
        Ok(scan) => {
            if scan.unfinished.is_some() {
                note("the file ends inside a zstd frame".into(), &mut errors);
            }
            for (k, fr) in scan.frames.iter().enumerate() {
                let mut it = RecordIter::new(f.as_mut(), fr.start as u64, fr.end as u64, k == 0);
                let mut ok = true;
                for r in it.by_ref() {
                    match r {
                        Ok(r) => {
                            records += 1;
                            first.get_or_insert(r.recv_ns);
                            last = Some(r.recv_ns);
                        }
                        Err(e) => {
                            note(format!("frame {k}: {e}"), &mut errors);
                            ok = false;
                            break;
                        }
                    }
                }
                if ok && it.truncated_tail {
                    note(
                        format!("frame {k}: a record runs past the frame end"),
                        &mut errors,
                    );
                } else if ok {
                    frames += 1;
                }
            }
        }
    }
    Ok(json!({
        "l0_file_id": id,
        "path": path,
        "bytes": len,
        "sha256": hex::encode(sha.finalize()),
        "frames": frames,
        "records": records,
        "first_recv_ns": first,
        "last_recv_ns": last,
        "framing_errors": errors,
        "error": first_error,
    }))
}

/// Verifies every sealed file of `date`, writes the result whole to
/// `crypto/l0/_verify/venue=<venue>/date=<date>.json` (renamed over the old
/// one) and returns it.
pub fn verify(
    fs: &dyn Fs,
    venue: &str,
    date: &str,
    verifier_ver: &str,
    clock: &dyn Clock,
) -> io::Result<Value> {
    let mut files = Vec::new();
    for p in sealed_files(fs, venue, date)? {
        files.push(verify_file(fs, &p)?);
    }
    let v = json!({
        "contract_ver": qraw::CONTRACT_VER,
        "venue": venue,
        "date": date,
        "verifier": "md-lake",
        "verifier_ver": verifier_ver,
        "verified_at": clock.now_ns(),
        "files": files,
    });
    let path = format!("crypto/l0/_verify/venue={venue}/date={date}.json");
    let tmp = format!("{path}.tmp");
    if fs.exists(&tmp) {
        fs.remove(&tmp)?;
    }
    let mut f = fs.create_new(&tmp)?;
    f.write_all(v.to_string().as_bytes())?;
    f.sync()?;
    drop(f);
    fs.rename(&tmp, &path)?;
    fs.sync_dir(parent(&path))?;
    Ok(v)
}
