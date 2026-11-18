//! Test helpers. Data directories are under `OMS_TEST_DIR`, else
//! `CARGO_TARGET_TMPDIR`, as in `oms-node/tests/common/mod.rs`: never
//! `std::env::temp_dir()`, because `/tmp` on the owner's PC is a tmpfs whose
//! fsync and ENOSPC behaviour differ from a real disk.
#![allow(dead_code)]

pub mod counting;
pub mod kit;
pub mod lock_holder;
pub mod schema;

use std::fs;
use std::path::{Path, PathBuf};

pub fn golden_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../contracts/lake/golden")
}

/// A fresh directory for one test.
pub fn test_dir(name: &str) -> PathBuf {
    let base = std::env::var("OMS_TEST_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_TARGET_TMPDIR")));
    let dir = base
        .join("md-lake")
        .join(format!("{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create test dir");
    dir
}

/// The segments of a golden L0 file, in name order.
pub fn golden_segments(l0_file_id: &str) -> Vec<Vec<u8>> {
    let dir = golden_dir().join("l0").join(l0_file_id);
    let mut names: Vec<PathBuf> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .map(|e| e.expect("entry").path())
        .filter(|p| p.extension().is_some_and(|x| x == "qraw"))
        .collect();
    names.sort();
    names
        .iter()
        .map(|p| fs::read(p).expect("segment"))
        .collect()
}

/// Every golden L0 `l0_file_id` (directory under golden/l0 that holds segments).
pub fn golden_file_ids() -> Vec<String> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) {
        let mut has_segment = false;
        for e in fs::read_dir(dir).expect("read_dir") {
            let p = e.expect("entry").path();
            if p.is_dir() {
                walk(root, &p, out);
            } else if p.extension().is_some_and(|x| x == "qraw") {
                has_segment = true;
            }
        }
        if has_segment {
            let rel = dir.strip_prefix(root).expect("under root");
            let parts: Vec<_> = rel
                .components()
                .map(|c| c.as_os_str().to_string_lossy())
                .collect();
            out.push(parts.join("/"));
        }
    }
    let root = golden_dir().join("l0");
    let mut out = Vec::new();
    walk(&root, &root, &mut out);
    out.sort();
    out
}
