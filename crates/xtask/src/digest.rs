//! Source digest and source checks.
//!
//! `source_digest` computes, independently of `crates/oms-node/build.rs`, the
//! same value that build script embeds in the binary: FNV-1a 64 over the
//! sorted (relative path, content) of every `.rs` and `Cargo.toml` under
//! `crates/`, plus `Cargo.lock`.

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

pub fn source_digest(root: &Path) -> String {
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let mut files = Vec::new();
    collect(&root.join("crates"), &mut files);
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
        eat(rel.to_string_lossy().as_bytes());
        eat(&[0]);
        eat(&fs::read(f).unwrap_or_default());
        eat(&[0]);
    }
    format!("{h:016x}")
}

/// The domain core must not name any API that reaches outside the process or
/// reads a clock. The dependency list already keeps crates out; this checks
/// the standard library, which a dependency list cannot.
pub fn core_purity(root: &Path) -> Result<(), Vec<String>> {
    const FORBIDDEN: &[&str] = &[
        "std::fs",
        "std::net",
        "std::time",
        "std::thread",
        "std::process",
        "std::env",
        "SystemTime",
        "Instant",
        "std::io",
        "rand",
        "unsafe ",
        "HashMap",
        "HashSet",
        "f32",
        "f64",
    ];
    let mut files = Vec::new();
    collect(&root.join("crates/oms-core/src"), &mut files);
    let mut found = Vec::new();
    for f in files {
        let text = fs::read_to_string(&f).unwrap_or_default();
        for (n, line) in text.lines().enumerate() {
            let code = line.split("//").next().unwrap_or("");
            for bad in FORBIDDEN {
                if code.contains(bad) {
                    found.push(format!("{}:{}: {bad}", f.display(), n + 1));
                }
            }
        }
    }
    let manifest = fs::read_to_string(root.join("crates/oms-core/Cargo.toml")).unwrap_or_default();
    let deps: Vec<&str> = manifest
        .split("[dependencies]")
        .nth(1)
        .unwrap_or("")
        .split('[')
        .next()
        .unwrap_or("")
        .lines()
        .filter_map(|l| l.split('=').next())
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    for d in deps {
        if d != "serde" && d != "serde_json" {
            found.push(format!("oms-core depends on {d}"));
        }
    }
    if found.is_empty() { Ok(()) } else { Err(found) }
}
