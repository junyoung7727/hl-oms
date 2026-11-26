//! The binary: `--version`, and the refusals that come before anything
//! touches the network (usage, a config file that cannot be read). No
//! test starts it with a config (it would call a real Bybit host).

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
    let build = if cfg!(feature = "test-endpoints") {
        " test-endpoints"
    } else {
        ""
    };
    assert_eq!(
        text.trim(),
        format!("watcher {}{build}", env!("CARGO_PKG_VERSION"))
    );
    for args in [
        &[][..],
        &["--config", "/nonexistent/watcher.toml"][..],
        &["--bogus"][..],
    ] {
        let out = Command::new(exe).args(args).output().expect("run watcher");
        assert_eq!(out.status.code(), Some(2), "{args:?}");
        let err = String::from_utf8_lossy(&out.stderr);
        let want = if args.len() == 2 {
            "watcher: config /nonexistent/watcher.toml: "
        } else {
            "watcher: usage: watcher --config <path> | watcher --version"
        };
        assert!(err.starts_with(want), "{args:?}: {err}");
        assert_eq!(err.lines().count(), 1, "{args:?}: {err}");
        assert!(out.stdout.is_empty(), "{args:?}");
    }
}
