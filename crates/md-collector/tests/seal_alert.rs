//! Ticket 06: `md-lake seal` sends one `[MD P1]` per run for the hours with
//! no open file (U6 spec D9, R4, R12), with no observability stack.

mod common;

use std::os::unix::fs::PermissionsExt;
use std::time::Duration;

use common::*;
use http_fake::{Answer, FakeHttp, dead_addr};

/// 2026-01-02 14:00:00 UTC.
const T14: i64 = 1_767_362_400_000_000_000;
const TOKEN: &str = "123456:ABCdef_ghi-JKL";

fn token_file(dir: &std::path::Path, mode: u32) -> String {
    let p = dir.join("telegram.json");
    std::fs::write(&p, format!(r#"{{"token":"{TOKEN}","chat_id":"42"}}"#)).expect("write");
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(mode)).expect("chmod");
    p.to_string_lossy().into_owned()
}

fn run(dir: &std::path::Path, extra: &[&str], now_ns: i64) -> (i32, String, String) {
    let mut args = vec![
        "seal".to_string(),
        "--venue".into(),
        "bybit".into(),
        "--lake".into(),
        dir.join("lake").to_string_lossy().into_owned(),
        "--first-hour".into(),
        "2026-01-02T14".into(),
        "--now-ns".into(),
        now_ns.to_string(),
    ];
    args.extend(extra.iter().map(|s| s.to_string()));
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_md-lake"))
        .args(&args)
        .env("MD_LAKE_TEST_ALLOW_FUTURE_NOW", "1")
        .output()
        .expect("run md-lake");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn texts(f: &FakeHttp) -> Vec<String> {
    f.requests()
        .iter()
        .filter_map(|r| serde_json::from_slice::<serde_json::Value>(&r.body).ok())
        .map(|v| v["text"].as_str().unwrap_or_default().to_string())
        .collect()
}

#[test]
fn seal_sends_one_md_p1_for_down_hours() {
    let dir = test_dir("seal-alert");
    let tg = FakeHttp::start(Answer::Reply(200, r#"{"ok":true}"#.into()));
    let file = token_file(&dir, 0o600);
    // Hours 14 and 15 are closed and have no open file; the fake
    // VictoriaMetrics is not involved at all (the seal never pushes).
    let now = T14 + 2 * HOUR + S;
    let (code, out, err) = run(
        &dir,
        &["--telegram", &file, "--telegram-api", &tg.url()],
        now,
    );
    assert_eq!(code, 0, "{out}\n{err}");
    let t = texts(&tg);
    assert_eq!(t.len(), 1, "one message per run: {t:?}");
    assert!(t[0].starts_with("[MD P1] bybit "), "{}", t[0]);
    assert!(t[0].contains("2026-01-02T14, 2026-01-02T15"), "{}", t[0]);
    assert!(t[0].contains("2개"), "{}", t[0]);
    assert!(!err.contains(TOKEN) && !out.contains(TOKEN));
    // The next run finds both hours settled: no message.
    let (code, _, _) = run(
        &dir,
        &["--telegram", &file, "--telegram-api", &tg.url()],
        now,
    );
    assert_eq!(code, 0);
    assert_eq!(texts(&tg).len(), 1);
}

#[test]
fn seal_alert_failure_keeps_exit_code() {
    let dir = test_dir("seal-alert-dead");
    let file = token_file(&dir, 0o600);
    let dead = format!("http://{}", dead_addr());
    let t0 = std::time::Instant::now();
    let (code, out, err) = run(
        &dir,
        &["--telegram", &file, "--telegram-api", &dead],
        T14 + HOUR + S,
    );
    assert_eq!(code, 0, "{out}\n{err}");
    assert!(err.contains("not delivered"), "{err}");
    assert!(!err.contains(TOKEN), "{err}");
    assert!(t0.elapsed() < Duration::from_secs(30));
}

#[test]
fn seal_alert_config_refused_before_the_lake() {
    let dir = test_dir("seal-alert-refused");
    let tg = FakeHttp::start(Answer::Reply(200, r#"{"ok":true}"#.into()));
    let file = token_file(&dir, 0o644);
    let now = T14 + HOUR + S;
    let (code, _, err) = run(
        &dir,
        &["--telegram", &file, "--telegram-api", &tg.url()],
        now,
    );
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("0600"), "{err}");
    let ok = token_file(&dir, 0o600);
    let (code, _, err) = run(
        &dir,
        &["--telegram", &ok, "--telegram-api", "http://example.com:80"],
        now,
    );
    assert_eq!(code, 2, "{err}");
    let (code, _, _) = run(&dir, &["--telegram-api", &tg.url()], now);
    assert_eq!(code, 2, "--telegram-api alone");
    assert!(!dir.join("lake").exists(), "nothing touched");
    assert!(tg.requests().is_empty());
}
