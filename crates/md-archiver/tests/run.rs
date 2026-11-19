//! The run (spec D1, D2, D7, D8, R5, R7; ticket 06) and the CLI.

mod common;

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use common::lake::golden_lake;
use md_archiver::build::{Settings, build_date};
use md_archiver::commit::{commit, gap_path};
use md_archiver::registry;
use md_archiver::run::{FreeBytes, RunArgs, run};
use md_lake::fs::RealFs;
use sha2::{Digest, Sha256};

const NOW: i64 = 1_767_999_000_000_000_000;

fn plenty() -> FreeBytes {
    Arc::new(|_: &Path| Ok(u64::MAX / 2))
}

fn args(lake: &Path) -> RunArgs {
    RunArgs {
        lake: lake.to_path_buf(),
        venue: "bybit".into(),
        settings: Settings::default(),
        rebuild: None,
        retain_l1_days: None,
        disk_floor_bytes: 1,
        now_ns: NOW,
        free: plenty(),
    }
}

/// SHA-256 of every file below `dir`, by lake-relative path.
fn hashes(lake: &Path, dir: &str) -> BTreeMap<String, String> {
    fn walk(root: &Path, d: &Path, out: &mut BTreeMap<String, String>) {
        if !d.is_dir() {
            return;
        }
        for e in fs::read_dir(d).expect("read_dir") {
            let p = e.expect("entry").path();
            if p.is_dir() {
                walk(root, &p, out);
            } else {
                let rel = p
                    .strip_prefix(root)
                    .expect("under")
                    .to_string_lossy()
                    .to_string();
                out.insert(
                    rel,
                    hex::encode(Sha256::digest(fs::read(&p).expect("read"))),
                );
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(lake, &lake.join(dir), &mut out);
    out
}

fn parquet_files(lake: &Path) -> BTreeMap<String, String> {
    hashes(lake, "crypto/l1")
        .into_iter()
        .filter(|(p, _)| p.ends_with(".parquet") || p.ends_with(".tmp"))
        .collect()
}

#[test]
fn two_missed_days_one_run() {
    let dir = common::test_dir("missed_days");
    golden_lake(&dir);
    let r = run(&args(&dir)).expect("run");
    let dates: Vec<&str> = r.built.iter().map(|b| b.date.as_str()).collect();
    assert_eq!(
        dates,
        ["2026-01-01", "2026-01-02", "2026-01-05"],
        "oldest first"
    );
    let again = run(&args(&dir)).expect("run");
    assert!(again.built.is_empty(), "built dates are not built again");
}

#[test]
fn second_run_and_rebuild_are_identical() {
    let dir = common::test_dir("identical");
    golden_lake(&dir);
    let l0 = hashes(&dir, "crypto/l0");
    run(&args(&dir)).expect("run");
    let first = parquet_files(&dir);
    assert!(first.len() > 10);
    let mut a = args(&dir);
    a.rebuild = Some(("2026-01-01".into(), "2026-01-05".into()));
    let r = run(&a).expect("rebuild");
    assert_eq!(r.built.len(), 3);
    assert_eq!(parquet_files(&dir), first, "byte-identical files");
    assert_eq!(hashes(&dir, "crypto/l0"), l0, "L0 unchanged");
}

#[test]
fn crash_mid_write_rebuilds() {
    let dir = common::test_dir("crash_mid_write");
    golden_lake(&dir);
    run(&args(&dir)).expect("run");
    let good = parquet_files(&dir);
    // A stale hour file from an earlier build, and a failure after one rename.
    let stale = dir.join("crypto/l1/book_msg/venue=bybit/market=linear/channel=orderbook.50/date=2026-01-02/hour=17.parquet");
    fs::write(&stale, b"old").expect("stale");
    let stray = dir.join("crypto/l1/trade/venue=bybit/market=linear/channel=publicTrade/date=2026-01-05/hour=03.parquet.tmp");
    fs::create_dir_all(stray.parent().expect("parent")).expect("mkdir");
    fs::write(&stray, b"tmp").expect("stray");
    let fs_ = RealFs::new(&dir);
    let venue = registry::venue("bybit").expect("bybit");
    md_archiver::commit::unmark(&dir, "bybit", "2026-01-02").expect("unmark");
    let built = build_date(
        &dir,
        fs_.as_ref(),
        venue,
        "2026-01-02",
        &Settings::default(),
    )
    .expect("build");
    assert!(commit(&dir, "bybit", &built, Some(1)).is_err());
    assert!(
        !dir.join(gap_path("bybit", "2026-01-02")).exists(),
        "no gap file: not built"
    );
    let r = run(&args(&dir)).expect("run");
    assert_eq!(
        r.built.iter().map(|b| b.date.as_str()).collect::<Vec<_>>(),
        ["2026-01-02"]
    );
    assert!(!stale.exists(), "the stale file is removed");
    assert!(!stray.exists(), "a leftover .tmp is removed at start");
    assert_eq!(parquet_files(&dir), good, "no .tmp left, the same files");
}

#[test]
fn waits_for_the_date_before() {
    let dir = common::test_dir("waits_prev");
    golden_lake(&dir);
    fs::write(
        dir.join("crypto/l0/venue=bybit/market=linear/channel=orderbook.50/date=2026-01-01/ETHUSDT.qraw.zst.partial"),
        b"x",
    )
    .expect("partial");
    let r = run(&args(&dir)).expect("run");
    let dates: Vec<&str> = r.built.iter().map(|b| b.date.as_str()).collect();
    assert_eq!(
        dates,
        ["2026-01-05"],
        "2026-01-01 is not sealed, so 2026-01-02 waits"
    );
    assert_eq!(r.waiting.len(), 2);
}

#[test]
fn disk_floor_stops_before_and_during_a_build() {
    let dir = common::test_dir("disk_floor");
    golden_lake(&dir);
    let mut a = args(&dir);
    a.free = Arc::new(|_: &Path| Ok(1000));
    a.disk_floor_bytes = 1000;
    let e = run(&a).expect_err("below floor + L0 bytes");
    assert_eq!(e.code, 1);
    assert!(parquet_files(&dir).is_empty(), "nothing written");
    // Enough at the start, below the floor after the first file.
    let calls = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let c = calls.clone();
    a.free = Arc::new(move |_: &Path| {
        let n = c.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(if n < 2 { u64::MAX / 2 } else { 10 })
    });
    let e = run(&a).expect_err("breach while writing");
    assert_eq!(e.code, 1);
    assert!(
        parquet_files(&dir).is_empty(),
        "the .tmp files are deleted: {:?}",
        parquet_files(&dir)
    );
}

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_md-archiver"))
}

fn cli(lake: &Path, extra: &[&str]) -> i32 {
    let mut c = Command::new(bin());
    c.arg("run").arg("--lake").arg(lake).args(extra);
    c.env_remove("MD_ARCHIVER_TEST_ALLOW_FUTURE_NOW");
    c.output().expect("run").status.code().expect("code")
}

#[test]
fn cli_refusals_write_nothing() {
    let dir = common::test_dir("cli_refusals");
    golden_lake(&dir);
    let floor = ["--disk-floor-bytes", "1"];
    assert_eq!(cli(&dir, &["--venue", "nope"]), 2);
    assert_eq!(
        cli(
            &dir,
            &["--venue", "bybit", "--rebuild", "2026-01-05..2026-01-01"]
        ),
        2
    );
    assert_eq!(
        cli(
            &dir,
            &["--venue", "bybit", "--rebuild", "2027-01-01..2027-01-02"]
        ),
        2,
        "no L0 in the range"
    );
    assert_eq!(
        cli(
            &dir,
            &["--venue", "bybit", "--now-ns", "9000000000000000000"]
        ),
        2
    );
    assert_eq!(cli(&dir, &["--venue", "bybit", "--threads", "0"]), 2);
    assert_eq!(cli(&dir, &["--venue", "bybit", "--bogus", "1"]), 2);
    let m = dir.join("crypto/l0/_manifest/venue=bybit/date=2026-01-05.jsonl");
    let text = fs::read_to_string(&m).expect("manifest");
    fs::write(
        &m,
        text.replacen(
            "\"layout\":\"by_symbol\"",
            "\"layout\":\"by_connection\"",
            1,
        ),
    )
    .expect("w");
    assert_eq!(
        cli(&dir, &["--venue", "bybit", floor[0], floor[1]]),
        2,
        "by_connection"
    );
    assert!(
        !dir.join("crypto/l1").exists(),
        "a refused run writes nothing"
    );
    fs::write(&m, text).expect("restore");
    assert_eq!(
        cli(
            &dir,
            &[
                "--venue",
                "bybit",
                "--disk-floor-bytes",
                "9000000000000000000"
            ]
        ),
        1
    );
    assert!(parquet_files(&dir).is_empty());
    // A held lock.
    fs::create_dir_all(dir.join("crypto/l1")).expect("mkdir");
    let lock = fs::File::create(dir.join("crypto/l1/.md-archiver-bybit.lock")).expect("lock");
    lock.try_lock().expect("hold");
    assert_eq!(cli(&dir, &["--venue", "bybit", floor[0], floor[1]]), 1);
    drop(lock);
    assert_eq!(cli(&dir, &["--venue", "bybit", floor[0], floor[1]]), 0);
    assert!(dir.join(gap_path("bybit", "2026-01-05")).exists());
}

#[test]
fn threads_do_not_change_the_bytes() {
    let one = common::test_dir("threads_one");
    let many = common::test_dir("threads_many");
    for (d, n) in [(&one, 1), (&many, 8)] {
        golden_lake(d);
        let mut a = args(d);
        a.settings.threads = n;
        run(&a).expect("run");
    }
    assert_eq!(parquet_files(&one), parquet_files(&many));
}
