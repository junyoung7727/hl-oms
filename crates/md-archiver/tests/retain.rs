//! `--retain-l1-days` (spec D9, R11; ticket 07).

mod common;

use std::fs;
use std::path::Path;
use std::sync::Arc;

use common::lake::golden_lake;
use md_archiver::build::Settings;
use md_archiver::run::{RunArgs, run};
use md_lake::time::date_start;
use serde_json::json;

/// Changes the lake after the build.
type Setup = Box<dyn Fn(&Path)>;

fn args(lake: &Path, retain: Option<i64>) -> RunArgs {
    RunArgs {
        lake: lake.to_path_buf(),
        venue: "bybit".into(),
        settings: Settings::default(),
        rebuild: None,
        retain_l1_days: retain,
        disk_floor_bytes: 1,
        now_ns: date_start("2026-01-20").expect("date") + 3_600_000_000_000,
        free: Arc::new(|_: &Path| Ok(u64::MAX / 2)),
    }
}

fn audit(lake: &Path, d: &str, body: &str) {
    let p = lake.join(format!("crypto/audit/venue=bybit/date={d}.json"));
    fs::create_dir_all(p.parent().expect("parent")).expect("mkdir");
    fs::write(p, body).expect("audit");
}

fn clean(d: &str) -> String {
    json!({"contract_ver": 1, "venue": "bybit", "date": d, "status": "clean",
           "auditor_ver": "0.1.0", "audited_at": 1, "l0_files": [], "l1_files": [], "gap_summary": {}})
    .to_string()
}

fn has(lake: &Path, table: &str, d: &str) -> bool {
    let base = lake.join(format!("crypto/l1/{table}/venue=bybit"));
    let mut found = false;
    for m in fs::read_dir(&base).into_iter().flatten().flatten() {
        if table == "instrument" {
            found |= m.path().join(format!("date={d}")).exists();
            continue;
        }
        for c in fs::read_dir(m.path()).into_iter().flatten().flatten() {
            found |= c.path().join(format!("date={d}")).exists();
        }
    }
    if table == "gap" {
        found = base.join(format!("date={d}/day.parquet")).exists();
    }
    found
}

fn tree(lake: &Path, dir: &str) -> Vec<(String, Vec<u8>)> {
    fn walk(root: &Path, d: &Path, out: &mut Vec<(String, Vec<u8>)>) {
        for e in fs::read_dir(d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(root, &p, out);
            } else {
                out.push((
                    p.strip_prefix(root)
                        .expect("under")
                        .to_string_lossy()
                        .to_string(),
                    fs::read(&p).expect("read"),
                ));
            }
        }
    }
    let mut out = Vec::new();
    walk(lake, &lake.join(dir), &mut out);
    out.sort();
    out
}

#[test]
fn retention_needs_sealed_and_clean() {
    let cases: [(&str, Setup, bool); 8] = [
        (
            "clean",
            Box::new(|l| audit(l, "2026-01-02", &clean("2026-01-02"))),
            true,
        ),
        (
            "not_audited",
            Box::new(|l| {
                audit(
                    l,
                    "2026-01-02",
                    &clean("2026-01-02").replace("clean", "not_audited"),
                )
            }),
            false,
        ),
        (
            "fail",
            Box::new(|l| {
                audit(
                    l,
                    "2026-01-02",
                    &clean("2026-01-02").replace("clean", "fail"),
                )
            }),
            false,
        ),
        ("missing", Box::new(|_| {}), false),
        (
            "cut",
            Box::new(|l| audit(l, "2026-01-02", &clean("2026-01-02")[..20])),
            false,
        ),
        (
            "contract_ver 2",
            Box::new(|l| {
                audit(
                    l,
                    "2026-01-02",
                    &clean("2026-01-02").replace("\"contract_ver\":1", "\"contract_ver\":2"),
                )
            }),
            false,
        ),
        (
            "another date",
            Box::new(|l| audit(l, "2026-01-02", &clean("2026-01-05"))),
            false,
        ),
        (
            "not day-sealed",
            Box::new(|l| {
                audit(l, "2026-01-02", &clean("2026-01-02"));
                fs::write(
                    l.join("crypto/l0/venue=bybit/_session/date=2026-01-02/md9-1.qraw.zst.partial"),
                    b"x",
                )
                .expect("partial");
            }),
            false,
        ),
    ];
    for (name, setup, removed) in cases {
        let dir = common::test_dir("retention");
        golden_lake(&dir);
        run(&args(&dir, None)).expect("build");
        setup(&dir);
        let l0 = tree(&dir, "crypto/l0");
        let audits = tree(&dir, "crypto/audit");
        let r = run(&args(&dir, Some(10))).expect("retain");
        assert_eq!(
            has(&dir, "book_msg", "2026-01-02"),
            !removed,
            "{name}: book_msg"
        );
        assert_eq!(has(&dir, "trade", "2026-01-02"), !removed, "{name}: trade");
        assert_eq!(
            has(&dir, "ticker", "2026-01-02"),
            !removed,
            "{name}: ticker"
        );
        assert!(has(&dir, "gap", "2026-01-02"), "{name}: the gap file stays");
        assert!(
            has(&dir, "instrument", "2026-01-02"),
            "{name}: the instrument file stays"
        );
        assert!(
            has(&dir, "book_msg", "2026-01-05"),
            "{name}: 2026-01-05 has no clean audit"
        );
        assert_eq!(tree(&dir, "crypto/l0"), l0, "{name}: L0 untouched");
        assert_eq!(
            tree(&dir, "crypto/audit"),
            audits,
            "{name}: audit untouched"
        );
        let d02 = r
            .retained
            .iter()
            .find(|x| x.date == "2026-01-02")
            .expect("reported");
        assert_eq!(d02.kept_reason.is_none(), removed, "{name}: {d02:?}");
        let next = run(&args(&dir, Some(10))).expect("next run");
        assert!(
            next.built.is_empty(),
            "{name}: a retained date is not built again"
        );
    }
}

#[test]
fn recent_dates_are_kept() {
    let dir = common::test_dir("retention_recent");
    golden_lake(&dir);
    run(&args(&dir, None)).expect("build");
    for d in ["2026-01-01", "2026-01-02", "2026-01-05"] {
        audit(&dir, d, &clean(d));
    }
    // 2026-01-20 − 15 days = 2026-01-05: only dates before it go.
    let r = run(&args(&dir, Some(15))).expect("retain");
    let gone: Vec<&str> = r
        .retained
        .iter()
        .filter(|x| x.kept_reason.is_none())
        .map(|x| x.date.as_str())
        .collect();
    assert_eq!(gone, ["2026-01-01", "2026-01-02"]);
    assert!(has(&dir, "book_msg", "2026-01-05"));
}
