//! The default build (without the feature `test-endpoints`) says so on
//! `--version`: exactly `watcher <version>`, nothing after it. Compiled only
//! in the default build; a test build prints `watcher <version>
//! test-endpoints` instead.

#![cfg(not(feature = "test-endpoints"))]

use std::process::Command;

#[test]
fn the_default_build_prints_no_test_endpoints() {
    let out = Command::new(env!("CARGO_BIN_EXE_watcher"))
        .arg("--version")
        .output()
        .expect("run watcher");
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        format!("watcher {}\n", env!("CARGO_PKG_VERSION"))
    );
    assert!(out.stderr.is_empty());
}
