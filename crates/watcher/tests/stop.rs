//! The stopper (C-WATCH part 12, U19b3a): exactly `<systemctl> [--user]
//! --no-ask-password stop -- <oms_unit>`, stdin and stdout null, stderr
//! piped and non-blocking, the exit read by `try_wait` at each tick,
//! stderr read only after the exit, the client killed at its deadline
//! (`unconfirmed`) and reaped later. No test runs the real `systemctl`:
//! every program here is a shell script under `OMS_TEST_DIR`, and the
//! unit name is `watcher-test-no-such-unit.service`.

#![allow(clippy::unwrap_used)]

mod common;

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use watcher::config::{Config, Kill, Systemd};
use watcher::stop::{Outcome, SYSTEMCTL, Stopper, args};

const T0: u64 = 1_800_000_000_000;
const UNIT: &str = "watcher-test-no-such-unit.service";

/// A fake `systemctl`: `body` as a shell script in `dir`, which must be
/// under `OMS_TEST_DIR` (the guard against running a program elsewhere).
fn fake_systemctl(dir: &Path, body: &str) -> PathBuf {
    let base = std::env::var("OMS_TEST_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir());
    assert!(
        dir.starts_with(&base) && dir != Path::new(SYSTEMCTL),
        "a fake systemctl outside OMS_TEST_DIR: {}",
        dir.display()
    );
    // One file per script: a file rewritten while another test forks
    // can be "busy" at its exec.
    static N: AtomicUsize = AtomicUsize::new(0);
    let p = dir.join(format!("systemctl-{}", N.fetch_add(1, Ordering::SeqCst)));
    std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o700)).unwrap();
    p
}

/// `Stopper::start`, again while the new script is "Text file busy": a
/// test thread that forked while the file was open for writing holds it
/// until its own exec (a race of the tests, not of the stopper).
fn spawn(script: &Path, k: &Kill, deadline_ms: u64) -> Stopper {
    for _ in 0..100 {
        match Stopper::start(script, k, deadline_ms) {
            Err(e) if e.contains("Text file busy") => std::thread::sleep(Duration::from_millis(10)),
            r => return r.unwrap(),
        }
    }
    panic!("{} stayed busy", script.display());
}

fn kill(systemd: Systemd, stop_wait_ms: u64) -> Kill {
    Kill {
        oms_unit: UNIT.into(),
        systemd,
        stop_wait_ms,
        ..Kill::default()
    }
}

/// Polls every `tick` of real time with the injected instant `now` moved
/// by `step` per tick, until an outcome or `bound` ticks; each poll must
/// return within 50 ms (it never waits). The outcome and the tick it came
/// at.
fn run(s: &mut Stopper, tick: Duration, step: u64, bound: usize) -> (Outcome, usize) {
    for k in 0..bound {
        let t = Instant::now();
        let got = s.poll(T0 + k as u64 * step);
        assert!(
            t.elapsed() < Duration::from_millis(50),
            "a poll waited {:?}",
            t.elapsed()
        );
        if let Some(o) = got {
            return (o, k);
        }
        std::thread::sleep(tick);
    }
    panic!("no outcome within {bound} ticks");
}

#[test]
fn the_stop_command_names_only_the_configured_unit() {
    let dir = common::test_dir("stop-argv");
    let argv = dir.join("argv");
    let script = fake_systemctl(
        &dir,
        &format!(
            "for a in \"$@\"; do printf '%s\\n' \"$a\"; done > {}\nexit 0",
            argv.display()
        ),
    );
    // The production path is the system's systemctl; tests give a script.
    assert_eq!(SYSTEMCTL, "/usr/bin/systemctl");
    for (systemd, want) in [
        (
            Systemd::System,
            vec!["--no-ask-password", "stop", "--", UNIT],
        ),
        (
            Systemd::User,
            vec!["--user", "--no-ask-password", "stop", "--", UNIT],
        ),
    ] {
        let k = kill(systemd, 10_000);
        assert_eq!(args(&k), want);
        let _ = std::fs::remove_file(&argv);
        let mut s = spawn(&script, &k, T0 + 10_000);
        let (o, _) = run(&mut s, Duration::from_millis(10), 10, 500);
        assert_eq!(o, Outcome::Stopped, "exit 0 is stopped");
        let seen = std::fs::read_to_string(&argv).unwrap();
        assert_eq!(seen.lines().collect::<Vec<_>>(), want);
        assert!(s.reaped());
        // One outcome: later polls give none.
        assert_eq!(s.poll(T0 + 20_000), None);
    }
    // A program that cannot be started: the stop failed, with the reason.
    let e = Stopper::start(&dir.join("missing"), &kill(Systemd::System, 10_000), T0).unwrap_err();
    assert!(e.contains("No such file"), "{e}");

    // A unit name with a space, a glob or a leading `-` never reaches the
    // stopper: the config refuses it at load.
    let cfg = |unit: &str| {
        Config::from_toml(&format!(
            r#"name = "w"
network = "testnet"
credentials_file = "/etc/watcher/creds.toml"
sub_account_uid = 1
symbols = ["BTCUSDT"]
ip_echo_url = "https://ip.example/"
data_dir = "/srv/oms/data"
status_url = "http://127.0.0.1:8188/api/status"
[kill]
marker = "{}"
oms_unit = {unit:?}
"#,
            dir.join("kill.marker").display()
        ))
    };
    assert_eq!(cfg(UNIT).unwrap().kill.unwrap().oms_unit, UNIT);
    for bad in [
        "hl oms.service",
        "hl-oms*.service",
        "-hl-oms.service",
        "--all.service",
        "hl-oms.service;reboot",
    ] {
        assert!(cfg(bad).unwrap_err().contains("oms_unit"), "{bad}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_watcher_starts_one_program_only() {
    // Plain text over crates/watcher/src (a comment counts too): the text
    // of a new process appears once, in stop.rs.
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut found = Vec::new();
    for e in std::fs::read_dir(&src).unwrap() {
        let p = e.unwrap().path();
        if p.extension().and_then(|x| x.to_str()) != Some("rs") {
            continue;
        }
        let text = std::fs::read_to_string(&p).unwrap();
        let n = text.matches("Command::new").count();
        if n > 0 {
            found.push((p.file_name().unwrap().to_string_lossy().to_string(), n));
        }
    }
    assert_eq!(found, vec![("stop.rs".to_string(), 1)]);
}

#[test]
fn a_child_that_keeps_stderr_open_does_not_block() {
    let dir = common::test_dir("stop-stderr");
    // The client writes one line, leaves a background `sleep 30` that
    // holds stderr open, and exits 1: the outcome comes within a tick of
    // the exit, and no poll waits for the pipe to close.
    let exited = dir.join("exited");
    let pid = dir.join("sleep.pid");
    let script = fake_systemctl(
        &dir,
        &format!(
            "sleep 30 >/dev/null &\necho $! > {}\necho 'Failed to stop {UNIT}: Interactive authentication required.' >&2\necho 'See system logs.' >&2\ntouch {}\nexit 1",
            pid.display(),
            exited.display()
        ),
    );
    let mut s = spawn(&script, &kill(Systemd::System, 10_000), T0 + 10_000);
    let start = Instant::now();
    let mut ticks_after_exit = None;
    let mut outcome = None;
    for k in 0..200 {
        let t = Instant::now();
        let got = s.poll(T0 + k * 100);
        assert!(
            t.elapsed() < Duration::from_millis(50),
            "a poll waited {:?}",
            t.elapsed()
        );
        if exited.exists() {
            *ticks_after_exit.get_or_insert(0) += 1;
        }
        if let Some(o) = got {
            outcome = Some(o);
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    assert_eq!(
        outcome,
        Some(Outcome::Failed(format!(
            "exit 1: Failed to stop {UNIT}: Interactive authentication required."
        )))
    );
    // The exit is seen at the tick after the script's last line, at most
    // one more (the shell exits right after `touch`).
    assert!(ticks_after_exit.unwrap() <= 2, "{ticks_after_exit:?}");
    assert!(
        start.elapsed() < Duration::from_secs(5),
        "{:?}",
        start.elapsed()
    );
    assert!(s.reaped());
    drop(s);
    if let Ok(p) = std::fs::read_to_string(&pid) {
        let _ = std::process::Command::new("kill").arg(p.trim()).status();
    }

    // A client that fills the pipe before it exits does not exit (a
    // client writing 1 MiB or more): at the
    // deadline (stop_wait_ms on the injected clock) it is killed, the
    // outcome is `unconfirmed`, and later ticks reap it without waiting.
    // (A shell loop of the builtin printf: 2 MiB, so the 64 KiB pipe fills
    // and the write blocks; the system's `head` may not block there.)
    let script = fake_systemctl(
        &dir,
        "i=0\nwhile [ $i -lt 2048 ]; do printf '%01023d\\n' 0 >&2; i=$((i+1)); done\nexit 0",
    );
    let mut s = spawn(&script, &kill(Systemd::System, 1000), T0 + 1000);
    let (o, k) = run(&mut s, Duration::from_millis(20), 100, 200);
    assert_eq!(o, Outcome::Unconfirmed);
    assert_eq!(k, 10, "at the deadline T0 + 1,000 ms, not before");
    let end = Instant::now() + Duration::from_secs(5);
    while !s.reaped() {
        assert!(Instant::now() < end, "the killed client was not reaped");
        let t = Instant::now();
        assert_eq!(s.poll(T0 + 2000), None);
        assert!(t.elapsed() < Duration::from_millis(50));
        std::thread::sleep(Duration::from_millis(20));
    }

    // A client killed by a signal before its deadline failed with it.
    let script = fake_systemctl(&dir, "kill -9 $$");
    let mut s = spawn(&script, &kill(Systemd::System, 10_000), T0 + 10_000);
    let (o, _) = run(&mut s, Duration::from_millis(10), 10, 500);
    assert_eq!(o, Outcome::Failed("signal 9".into()));
    let _ = std::fs::remove_dir_all(&dir);
}
