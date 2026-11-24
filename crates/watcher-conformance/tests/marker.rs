//! The watcher's kill marker against the OMS's own reader (U19b3b, review
//! 2 N4): the file `watcher::kill::write_marker` leaves is read by
//! `oms_node::node::read_kill_marker`, the reader the OMS runs once before
//! its boot record, and gives the note `marker: <text>`.

#![allow(clippy::unwrap_used)]

use std::path::PathBuf;

use oms_node::node::read_kill_marker;
use watcher::kill::{MARKER_TEXT_MAX, MarkerOutcome, marker_text, write_marker};

fn tmp(name: &str) -> PathBuf {
    let base = std::env::var("OMS_TEST_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir());
    let d = base.join(format!("watcher-conformance-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn the_oms_reads_the_watchers_marker_file() {
    let dir = tmp("marker");
    assert_eq!(read_kill_marker(&dir.join("oms.kill")), None);

    let marker = dir.join("oms.kill");
    let text = marker_text("w-1", 1_700_000_000_000, 10_000, true);
    assert_eq!(write_marker(&marker, &text), MarkerOutcome::Written);
    assert_eq!(read_kill_marker(&marker), Some(format!("marker: {text}")));

    // The longest text the watcher writes is read whole: it is cut to
    // the OMS's 512 bytes at a whole character, so the OMS's lossy decode
    // loses nothing.
    let marker = dir.join("long.kill");
    let text = marker_text(&"𝕏".repeat(100), 1_700_000_000_000, 10_000, false);
    assert!(text.len() <= MARKER_TEXT_MAX && text.len() > MARKER_TEXT_MAX - 4);
    assert_eq!(write_marker(&marker, &text), MarkerOutcome::Written);
    assert_eq!(read_kill_marker(&marker), Some(format!("marker: {text}")));
}
