//! Computes a digest of the workspace sources so that a running binary, a
//! verification result and a document can name the exact code they refer to
//! even where there is no version-control commit to point at.
//!
//! The digest is FNV-1a 64 over the sorted list of (relative path, content)
//! of every `.rs` and `Cargo.toml` file under `crates/`, plus `Cargo.lock`.
//! `cargo xtask digest` computes the same value independently.

use std::fs;
use std::path::{Path, PathBuf};

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            if p.file_name().is_some_and(|n| n == "target") {
                continue;
            }
            collect(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") || p.file_name().is_some_and(|n| n == "Cargo.toml") {
            out.push(p);
        }
    }
}

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default());
    let root = manifest.join("../..").canonicalize().unwrap_or_default();
    let crates = root.join("crates");
    let mut files = Vec::new();
    collect(&crates, &mut files);
    files.push(root.join("Cargo.lock"));
    files.sort();
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut eat = |bytes: &[u8]| {
        for b in bytes {
            h ^= u64::from(*b);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    };
    for f in &files {
        let rel = f.strip_prefix(&root).unwrap_or(f);
        let rel = rel.to_string_lossy();
        eat(rel.as_bytes());
        eat(&[0]);
        eat(&fs::read(f).unwrap_or_default());
        eat(&[0]);
        println!("cargo:rerun-if-changed={}", f.display());
    }
    println!("cargo:rerun-if-changed={}", crates.display());
    println!("cargo:rustc-env=OMS_SOURCE_DIGEST={h:016x}");
}
