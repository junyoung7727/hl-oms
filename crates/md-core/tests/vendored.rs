//! `contracts/lake/` is a byte copy of the lake contract that
//! quant-data-platform tests. If one byte drifts, hl-oms would read and write
//! a lake that the other repository no longer agrees with.

use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

/// Files that sit next to the contract but are not listed in its index.
const UNINDEXED: [&str; 3] = ["index.json", ".gitattributes", "VENDOR.toml"];

fn lake_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../contracts/lake")
}

fn sha256_hex(path: &Path) -> String {
    let bytes = fs::read(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    hex::encode(Sha256::digest(&bytes))
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) {
    for entry in fs::read_dir(dir).expect("read_dir") {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            walk(root, &path, out);
        } else {
            let rel = path.strip_prefix(root).expect("under root");
            let name: Vec<_> = rel
                .components()
                .map(|c| c.as_os_str().to_string_lossy())
                .collect();
            out.push(name.join("/"));
        }
    }
}

/// Every problem found in the copy at `dir`, one line each. Empty means the
/// copy matches its index and the index matches `VENDOR.toml`.
fn check(dir: &Path) -> Vec<String> {
    let mut problems = Vec::new();

    let vendor: toml::Table = fs::read_to_string(dir.join("VENDOR.toml"))
        .expect("VENDOR.toml")
        .parse()
        .expect("VENDOR.toml parses");
    for key in ["source_repo", "source_commit", "index_sha256"] {
        if vendor.get(key).and_then(|v| v.as_str()).is_none() {
            problems.push(format!("VENDOR.toml: {key} missing"));
        }
    }
    let recorded = vendor
        .get("index_sha256")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let actual = sha256_hex(&dir.join("index.json"));
    if recorded != actual {
        problems.push(format!(
            "index.json: sha256 {actual}, VENDOR.toml records {recorded}"
        ));
    }

    let index: serde_json::Value =
        serde_json::from_slice(&fs::read(dir.join("index.json")).expect("index.json"))
            .expect("index.json parses");
    let files = index["files"].as_array().expect("index.json files");
    let mut listed = Vec::new();
    for f in files {
        let name = f["name"].as_str().expect("file name").to_string();
        let want = f["sha256"].as_str().expect("file sha256");
        let path = dir.join(&name);
        if !path.is_file() {
            problems.push(format!("{name}: listed in index.json but missing"));
        } else if sha256_hex(&path) != want {
            problems.push(format!("{name}: sha256 differs from index.json"));
        }
        listed.push(name);
    }

    let mut present = Vec::new();
    walk(dir, dir, &mut present);
    for name in present {
        if !UNINDEXED.contains(&name.as_str()) && !listed.contains(&name) {
            problems.push(format!("{name}: present but not in index.json"));
        }
    }
    problems
}

fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).expect("create dir");
    for entry in fs::read_dir(from).expect("read_dir") {
        let path = entry.expect("dir entry").path();
        let target = to.join(path.file_name().expect("file name"));
        if path.is_dir() {
            copy_tree(&path, &target);
        } else {
            fs::copy(&path, &target).expect("copy");
        }
    }
}

#[test]
fn vendored_contract_files_match_the_index_and_the_record() {
    let problems = check(&lake_dir());
    assert!(
        problems.is_empty(),
        "vendored contract drifted:\n{}",
        problems.join("\n")
    );
}

#[test]
fn vendored_contract_check_fails_when_one_byte_changes() {
    let tmp = std::env::temp_dir().join(format!("md-core-vendored-{}", std::process::id()));
    let _ = fs::remove_dir_all(&tmp);
    copy_tree(&lake_dir(), &tmp);

    let victim = "golden/l1/book_msg.json";
    let mut bytes = fs::read(tmp.join(victim)).expect("read victim");
    bytes[0] ^= 0x01;
    fs::write(tmp.join(victim), &bytes).expect("write victim");

    fs::write(tmp.join("golden/stray.json"), b"{}").expect("write stray");

    let problems = check(&tmp);
    fs::remove_dir_all(&tmp).expect("clean up");

    assert_eq!(
        problems,
        vec![
            format!("{victim}: sha256 differs from index.json"),
            "golden/stray.json: present but not in index.json".to_string(),
        ]
    );
}
