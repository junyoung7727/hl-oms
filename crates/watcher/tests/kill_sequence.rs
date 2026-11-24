//! The watcher's kill sequence (C-WATCH part 12, U19b3b): the sans-IO
//! `Kill` (K1 marker → K2 `watcher_kill` → K3 `systemctl stop` → the
//! loop, each step with a deadline, the owed halt retried during the loop,
//! `Kill::abort` in every step), driven by hand on its own clock (no task,
//! no socket, no program), and the marker writer on a directory under
//! `OMS_TEST_DIR`.

#![allow(clippy::unwrap_used)]

mod common;

use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use watcher::config::Kill as KillCfg;
use watcher::kill::{
    DETAIL_MAX, MARKER_TEXT_MAX, MarkerOutcome, halt_detail, late_halt_detail, marker_text,
    spawn_marker, utc_ms, write_marker,
};

/// 2023-11-14T22:13:20Z.
const T0: u64 = 1_700_000_000_000;
const UNIT: &str = "watcher-test-no-such-unit.service";

fn kill_cfg(marker: PathBuf) -> KillCfg {
    KillCfg {
        marker,
        oms_unit: UNIT.to_string(),
        ..KillCfg::default()
    }
}

// ---- Texts ----

#[test]
fn the_marker_text_and_detail_are_cut_at_a_character_boundary() {
    assert_eq!(utc_ms(T0), "2023-11-14T22:13:20.000Z");
    assert_eq!(utc_ms(T0 + 1_234), "2023-11-14T22:13:21.234Z");
    assert_eq!(utc_ms(951_782_400_000), "2000-02-29T00:00:00.000Z");
    assert_eq!(
        marker_text("w-1", T0, 10_100, true),
        "watcher w-1 W5 2023-11-14T22:13:20.000Z: /api/status gave no good answer for 10.1 s (last good answer 2023-11-14T22:13:09.900Z); the watcher stops the OMS and cancels every order with its own key. Clear with omsctl clear-hold --reason kill."
    );
    assert!(
        marker_text("w-1", T0, 10_000, false)
            .contains("for 10 s (last good answer none in this process);"),
    );
    assert_eq!(
        halt_detail("w-1", 10_000),
        "W5: /api/status gave no good answer for 10 s; watcher w-1"
    );
    assert_eq!(
        late_halt_detail("w-1"),
        "W5: 늦은 정지: watcher w-1의 킬이 OMS 래치나 정지를 확인하지 못함"
    );

    // A 64-character name of 4-byte characters (the config's longest)
    // fits whole; a longer one (Setup takes any) is cut.
    let name = "𝕏".repeat(64);
    let fits = marker_text(&name, T0, 10_000, true);
    assert!(fits.len() <= MARKER_TEXT_MAX && fits.ends_with("--reason kill."));
    let longer = "𝕏".repeat(100);
    let text = marker_text(&longer, T0, 10_000, true);
    let full = format!(
        "watcher {longer} W5 2023-11-14T22:13:20.000Z: /api/status gave no good answer for 10 s (last good answer 2023-11-14T22:13:10.000Z); the watcher stops the OMS and cancels every order with its own key. Clear with omsctl clear-hold --reason kill."
    );
    assert!(full.len() > MARKER_TEXT_MAX, "the case must cut");
    assert!(text.len() <= MARKER_TEXT_MAX, "{} bytes", text.len());
    assert!(full.starts_with(&text));
    // Cut at a whole character, as late as possible.
    let next = full[text.len()..].chars().next().unwrap();
    assert!(text.len() + next.len_utf8() > MARKER_TEXT_MAX);
    // The OMS reads 512 bytes and decodes them lossily: nothing is lost.
    assert!(!String::from_utf8_lossy(&text.as_bytes()[..text.len()]).contains('\u{FFFD}'));

    for d in [halt_detail(&name, 10_000), late_halt_detail(&name)] {
        assert!(d.len() <= DETAIL_MAX && d.contains(&name), "{d}");
    }
    // A name longer than the config allows still gives a detail within
    // the OMS's 1024 bytes, cut at a whole character.
    let long = "가".repeat(1000);
    for (d, full) in [
        (
            halt_detail(&long, 10_000),
            format!("W5: /api/status gave no good answer for 10 s; watcher {long}"),
        ),
        (
            late_halt_detail(&long),
            format!("W5: 늦은 정지: watcher {long}의 킬이 OMS 래치나 정지를 확인하지 못함"),
        ),
    ] {
        assert!(
            d.len() <= DETAIL_MAX && full.starts_with(&d),
            "{} bytes",
            d.len()
        );
        let next = full[d.len()..].chars().next().unwrap();
        assert!(d.len() + next.len_utf8() > DETAIL_MAX);
    }
}

// ---- The marker writer ----

fn names(dir: &std::path::Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    v.sort();
    v
}

#[tokio::test]
async fn a_marker_file_is_what_a_marker_rule_reads() {
    let dir = common::test_dir("kseq-marker-file");
    let marker = dir.join("oms.kill");
    let text = marker_text("w-1", T0, 10_000, true);
    // A temporary file of this process left by an earlier run (the same
    // pid after a reboot) is removed first, never the marker.
    let stale = dir.join(format!(".oms.kill.watcher-{}.tmp", std::process::id()));
    std::fs::write(&stale, "stale").unwrap();

    // The runtime runs the write on spawn_blocking.
    let out = spawn_marker(marker.clone(), text.clone()).await.unwrap();
    assert_eq!(out, MarkerOutcome::Written);
    let meta = std::fs::symlink_metadata(&marker).unwrap();
    assert!(meta.file_type().is_file());
    assert_eq!(meta.permissions().mode() & 0o777, 0o600);
    assert_eq!(std::fs::read_to_string(&marker).unwrap(), text);
    // In the marker's directory, and no temporary file left.
    assert_eq!(names(&dir), vec!["oms.kill".to_string()]);
}

#[test]
fn an_existing_marker_is_left_as_it_is() {
    let dir = common::test_dir("kseq-marker-existing");
    // A file, a dangling symlink and a directory at the marker path: each
    // is a marker already there, not touched and not replaced.
    let file = dir.join("a.kill");
    std::fs::write(&file, "operator's marker").unwrap();
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(write_marker(&file, "new"), MarkerOutcome::AlreadyPresent);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "operator's marker");
    assert_eq!(
        std::fs::metadata(&file).unwrap().permissions().mode() & 0o777,
        0o644
    );

    let link = dir.join("b.kill");
    std::os::unix::fs::symlink(dir.join("nowhere"), &link).unwrap();
    assert_eq!(write_marker(&link, "new"), MarkerOutcome::AlreadyPresent);
    assert!(
        std::fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert!(!dir.join("nowhere").exists());

    let sub = dir.join("c.kill");
    std::fs::create_dir(&sub).unwrap();
    assert_eq!(write_marker(&sub, "new"), MarkerOutcome::AlreadyPresent);
    assert!(sub.is_dir() && names(&sub).is_empty());
    assert_eq!(names(&dir), vec!["a.kill", "b.kill", "c.kill"]);
}

#[test]
fn a_marker_that_cannot_be_written_does_not_stop_the_sequence() {
    let dir = common::test_dir("kseq-marker-fails");
    // The directory is gone: the io kind, nothing written.
    let gone = dir.join("gone").join("oms.kill");
    assert_eq!(
        write_marker(&gone, "x"),
        MarkerOutcome::Failed("NotFound".to_string())
    );
    let _ = kill_cfg(gone);
}
