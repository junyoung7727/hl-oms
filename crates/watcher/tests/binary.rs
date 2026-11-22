//! The binary of U19a1: `--version` only; everything else is refused
//! without touching the network or a file.

use std::process::Command;

#[test]
fn version_prints_and_anything_else_exits_2() {
    let exe = env!("CARGO_BIN_EXE_watcher");
    let out = Command::new(exe)
        .arg("--version")
        .output()
        .expect("run watcher");
    assert_eq!(out.status.code(), Some(0));
    let text = String::from_utf8_lossy(&out.stdout);
    assert_eq!(
        text.trim(),
        format!("watcher {}", env!("CARGO_PKG_VERSION"))
    );
    for args in [
        &[][..],
        &["--config", "/nonexistent/watcher.toml"][..],
        &["--bogus"][..],
    ] {
        let out = Command::new(exe).args(args).output().expect("run watcher");
        assert_eq!(out.status.code(), Some(2), "{args:?}");
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(
            err.contains("not built yet: the checks arrive in U19a5–U19a7"),
            "{args:?}: {err}"
        );
        assert!(out.stdout.is_empty(), "{args:?}");
    }
}
