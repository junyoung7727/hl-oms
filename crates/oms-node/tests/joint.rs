//! The OMS and watcher joint tests: the real `oms-node` (a `failpoints`
//! build with the Bybit gateway) and the real watcher (its
//! `test-endpoints` build) run together against one stateful fake Bybit,
//! a fake Telegram, a fake VictoriaMetrics and a fake `systemctl`,
//! loopback only ([`joint_rig`]). Needs `--features joint` and the
//! variables `OMS_TEST_DIR`, `JOINT_WATCHER_BIN` and `JOINT_OMSCTL_BIN`;
//! `cargo run -p xtask -- joint` builds the two other binaries and sets
//! them. One test at a time (`--test-threads=1`): each world holds several
//! processes and the scenarios run in real time.

#![allow(clippy::unwrap_used)]

mod bybit_fake;
mod joint_rig;

use joint_rig::*;
use std::time::Duration;

/// The rig's guard refuses a binary that is not the watcher's test build
/// before any watcher configuration is written; the real test build
/// started without `WATCHER_TEST_ENDPOINTS` exits 2 with its one line
/// and sends nothing to the fake.
#[test]
fn j0_the_rig_refuses_a_watcher_built_without_test_endpoints_and_a_test_build_without_wiring_exits_2()
 {
    let w = World::new("j0", Timers::Short, Duration::from_secs(20));
    let script = w.dir.join("bin/watcher");
    std::fs::write(&script, "#!/bin/sh\necho 'watcher 0.1.0'\n").unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
    let refused = w.watcher_with(&script).err().unwrap_or_default();
    assert!(
        refused.contains("printed \"watcher 0.1.0\": not the watcher's test build"),
        "the guard took a default build: {refused:?}"
    );
    assert!(
        !w.dir.join("w/watcher.toml").exists(),
        "a configuration was written for a binary the guard refused"
    );

    let mut watcher = w.watcher();
    watcher.start_unwired();
    w.wait(
        "the unwired test build exits",
        Duration::from_secs(15),
        || watcher.exit().is_some(),
    );
    let (status, _) = watcher.exit().unwrap();
    let lines = watcher.log_lines("");
    assert_eq!(status.code(), Some(2), "{lines:?}");
    assert_eq!(
        lines,
        ["this build is for tests: WATCHER_TEST_ENDPOINTS is required"],
        "the refusal line"
    );
    let seen = w.fake.with(|b, _| b.log.len());
    assert_eq!(seen, 0, "the unwired watcher reached the fake");
    assert!(w.telegram.requests().is_empty() && w.vm.requests().is_empty());
}
