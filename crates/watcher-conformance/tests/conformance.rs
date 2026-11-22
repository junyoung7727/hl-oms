//! The watcher against the OMS's own code: its `Base32Pair` against the
//! core's, and its journal reader against journals that `oms-journal`
//! writes from records the core decided. This package is the only one that
//! links the watcher with OMS code (`tools/ci/watcher_independent.sh`).

#![allow(clippy::unwrap_used)]

#[path = "../../oms-core/tests/common/mod.rs"]
mod common;

use common::*;
use oms_core::{OrderId, VenueClientId, WireIdForm};
use oms_journal::{Error, Options, Writer, recover};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use watcher::journal::{MAX_RECORDS, Tail};

fn tmp(name: &str) -> PathBuf {
    let base = std::env::var("OMS_TEST_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir());
    let d = base.join(format!("watcher-conformance-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn opts(segment_max_bytes: u64) -> Options {
    Options {
        segment_max_bytes,
        truncate_torn_tail: true,
        record_schema: 2,
        readable_schemas: vec![1, 2],
        instance_id: 7,
        writer: "watcher-conformance".into(),
    }
}

/// A journal of the records in `h.log`, every one synced. The writer (and
/// so the journal lock) is returned.
fn journal(dir: &Path, h: &H, segment_max_bytes: u64) -> Writer {
    let (mut w, _) = recover(dir, &opts(segment_max_bytes), 0, |_, _, _| Ok(())).unwrap();
    for rec in &h.log {
        w.append(rec.seq, &serde_json::to_vec(rec).unwrap())
            .unwrap();
        w.sync(rec.at).unwrap();
    }
    w
}

/// Every directory entry with its length and modification time.
fn listing(dir: &Path) -> BTreeMap<String, (u64, SystemTime)> {
    std::fs::read_dir(dir)
        .unwrap()
        .map(|e| {
            let e = e.unwrap();
            let m = e.metadata().unwrap();
            (
                e.file_name().to_string_lossy().into_owned(),
                (m.len(), m.modified().unwrap()),
            )
        })
        .collect()
}

/// xorshift64 (Marsaglia), so that the random pairs need no dependency.
struct XorShift(u64);

impl XorShift {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
}

#[test]
fn the_watcher_base32_pair_equals_the_core_form() {
    let core = |i: u64, o: u64| {
        WireIdForm::Base32Pair.encode(VenueClientId {
            instance_id: i,
            order_id: OrderId(o),
        })
    };
    let core_decode = |s: &str| {
        WireIdForm::Base32Pair
            .decode(s)
            .map(|id| (id.instance_id, id.order_id.0))
    };
    let edges = [
        0,
        1,
        31,
        32,
        u64::MAX - 1,
        u64::MAX,
        0x0123_4567_89ab_cdef,
        42,
    ];
    let mut pairs: Vec<(u64, u64)> = edges
        .iter()
        .flat_map(|&i| edges.iter().map(move |&o| (i, o)))
        .collect();
    let mut rng = XorShift(0x9e37_79b9_7f4a_7c15);
    for _ in 0..10_000 {
        pairs.push((rng.next(), rng.next()));
    }
    for (i, o) in pairs {
        let s = watcher::wire_id::encode(i, o);
        assert_eq!(s, core(i, o), "{i} {o}");
        assert_eq!(watcher::wire_id::decode(&s), Some((i, o)));
        assert_eq!(core_decode(&s), Some((i, o)));
    }
    let malformed = [
        "",
        "-",
        "0000000000000",
        "0000000000000-",
        "-0000000000000",
        "0000000000000-000000000000",
        "0000000000000-00000000000000",
        "000000000000-00000000000000",
        "0000000000000_0000000000000",
        "0000000000000--000000000000",
        "0000000000000-0000000000000-",
        "000000000000A-0000000000000",
        "0000000000000-000000000000w",
        "0000000000000-000000000000z",
        "g000000000000-0000000000000",
        "0000000000000-g000000000000",
        "vvvvvvvvvvvvv-0000000000000",
        " 000000000000-0000000000000",
        "+000000000000-0000000000000",
        "028q5cu4qnjff-000000000001a\n",
        "028Q5CU4QNJFF-000000000001A",
        "0x0123456789abcdef000000000000002a",
        "000000000000é-0000000000000",
    ];
    for bad in malformed {
        assert_eq!(watcher::wire_id::decode(bad), None, "{bad:?}");
        assert_eq!(core_decode(bad), None, "{bad:?}");
    }
}

#[test]
fn the_watcher_reads_while_the_oms_writer_holds_the_lock() {
    let dir = tmp("lock");
    let h = H::ready();
    assert!(
        h.log.len() >= 3,
        "the harness wrote {} records",
        h.log.len()
    );
    // Small segments, so that the reader also crosses rotations.
    let w = journal(&dir, &h, 512);
    let last = h.log.last().unwrap().seq;
    let before = listing(&dir);
    assert!(before.contains_key("LOCK"));
    assert!(
        before.keys().filter(|k| k.starts_with("journal-")).count() >= 2,
        "{before:?}"
    );

    let mut t = Tail::open(&dir);
    let recs = t.read(last, MAX_RECORDS).unwrap();
    assert_eq!(t.instance_id(), Some(7));
    assert_eq!(recs.len(), h.log.len());
    for (mine, theirs) in recs.iter().zip(&h.log) {
        let v: Value = serde_json::to_value(theirs).unwrap();
        assert_eq!((mine.seq, mine.at), (theirs.seq, theirs.at));
        let events: Vec<(String, Value)> = v["events"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| {
                let (k, f) = e.as_object().unwrap().iter().next().unwrap();
                (k.clone(), f.clone())
            })
            .collect();
        assert_eq!(mine.events, events, "record {}", mine.seq);
    }
    assert_eq!(t.read(last, MAX_RECORDS).unwrap(), vec![]);

    // The reader changed nothing in the directory, and the writer still
    // holds the lock.
    assert_eq!(listing(&dir), before);
    assert!(matches!(
        recover(&dir, &opts(512), 0, |_, _, _| Ok(())),
        Err(Error::Locked(_))
    ));
    drop(w);
}
