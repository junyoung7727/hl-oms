//! alert-direct (spec D1). Every peer is a loopback fake; no test opens a
//! non-loopback socket.

use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use alert_direct::{
    AlertConfig, AlertDirect, AlertHandle, Clock, ConfigError, Outbox, Repeat, Route,
    SecondChannel, SendOutcome, TelegramConfig, split_message,
};
use http_fake::{Answer, FakeHttp, dead_addr};

const TOKEN: &str = "123456:ABCdef_ghi-JKL";

fn test_dir(name: &str) -> PathBuf {
    let base = std::env::var("OMS_TEST_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_TARGET_TMPDIR")));
    let dir = base
        .join("alert-direct")
        .join(format!("{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create test dir");
    dir
}

fn write_cfg(path: &Path, body: &str, mode: u32) {
    std::fs::write(path, body).expect("write");
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).expect("chmod");
}

fn units(s: &str) -> usize {
    s.encode_utf16().count()
}

struct TestClock(AtomicI64);

impl Clock for TestClock {
    fn now_ms(&self) -> i64 {
        self.0.load(Ordering::SeqCst)
    }
}

const OK: &str = r#"{"ok":true,"result":{}}"#;

fn ok() -> Answer {
    Answer::Reply(200, OK.into())
}

fn cfg_for(routes: Vec<Route>) -> AlertConfig {
    let tg = TelegramConfig::new(TOKEN, "42").expect("config");
    let mut c = AlertConfig::new(tg, "[MD P1]", "bybit");
    c.routes = routes;
    c.timeout = Duration::from_millis(500);
    c.flush_every = Duration::from_millis(50);
    c
}

fn loopback(f: &FakeHttp) -> Route {
    Route::direct(&f.url()).expect("route")
}

fn texts(f: &FakeHttp) -> Vec<String> {
    f.requests()
        .iter()
        .map(|r| {
            let v: serde_json::Value = serde_json::from_slice(&r.body).expect("json body");
            v["text"].as_str().unwrap_or_default().to_string()
        })
        .collect()
}

#[test]
fn config_refuses_unsafe_files() {
    let d = test_dir("cfg");
    let good = format!(r#"{{"token":"{TOKEN}","chat_id":"42"}}"#);
    let p = d.join("ok.json");
    write_cfg(&p, &good, 0o600);
    let c = TelegramConfig::load_with(&p, false).expect("a 0600 file loads");
    assert_eq!(c.chat_id(), "42");
    assert!(
        !format!("{c:?}").contains(TOKEN),
        "Debug must not show the token"
    );

    let mut errors: Vec<ConfigError> = Vec::new();
    let wide = d.join("wide.json");
    write_cfg(&wide, &good, 0o640);
    errors.push(TelegramConfig::load_with(&wide, false).expect_err("0640 refused"));
    let link = d.join("link.json");
    symlink(&p, &link).expect("symlink");
    errors.push(TelegramConfig::load_with(&link, false).expect_err("symlink refused"));
    let repo = d.join("repo");
    std::fs::create_dir_all(repo.join(".git")).expect("mkdir");
    std::fs::create_dir_all(repo.join("sub")).expect("mkdir");
    let inrepo = repo.join("sub/t.json");
    write_cfg(&inrepo, &good, 0o600);
    errors.push(TelegramConfig::load_with(&inrepo, true).expect_err("file in a repo refused"));
    for (name, body) in [
        (
            "badtok",
            r#"{"token":"123456:AB CD","chat_id":"42"}"#.to_string(),
        ),
        ("notok", r#"{"token":"abc","chat_id":"42"}"#.to_string()),
        ("nochat", format!(r#"{{"token":"{TOKEN}","chat_id":""}}"#)),
        ("notjson", format!("token={TOKEN}")),
    ] {
        let f = d.join(format!("{name}.json"));
        write_cfg(&f, &body, 0o600);
        errors.push(TelegramConfig::load_with(&f, false).expect_err(name));
    }
    assert_eq!(errors.len(), 7);
    for e in &errors {
        let text = format!("{e} {e:?}");
        assert!(!text.contains(TOKEN), "error shows the token: {text}");
        assert!(
            !text.contains("ABCdef"),
            "error shows part of the token: {text}"
        );
    }
}

#[test]
fn split_matches_pr3() {
    let short = "a\nb";
    assert_eq!(split_message(short, 3500), vec![short.to_string()]);
    let lines: Vec<String> = (0..400).map(|i| format!("line {i:04} ok")).collect();
    let text = lines.join("\n");
    let parts = split_message(&text, 3500);
    assert!(parts.len() > 1);
    for p in &parts {
        assert!(units(p) <= 3500);
        assert!(
            !p.starts_with('\n') && !p.ends_with('\n'),
            "cut at line ends"
        );
    }
    assert_eq!(parts.join("\n"), text);
    // One overlong line with characters of two UTF-16 units each.
    let long: String = "😀".repeat(2000) + "x";
    let parts = split_message(&long, 3500);
    assert!(parts.iter().all(|p| units(p) <= 3500));
    assert_eq!(parts.concat(), long);
    assert_eq!(parts.len(), 2);
}

#[test]
fn outbox_bound_and_drop_notice() {
    let mut o = Outbox::new(50);
    for i in 0..60 {
        o.add(&format!("m{i}"));
    }
    assert_eq!(o.len(), 50);
    assert_eq!(o.dropped(), 10);
    let mut sent = Vec::new();
    o.flush(
        &mut |t: &str| {
            sent.push(t.to_string());
            SendOutcome::Delivered
        },
        "[MD P1]",
    );
    assert_eq!(sent.len(), 51);
    assert_eq!(sent[0], "m10");
    assert_eq!(sent[49], "m59");
    assert_eq!(sent[50], "[MD P1] 전송 실패로 알림 10개를 버림");
    assert_eq!(o.dropped(), 0);
    assert_eq!(o.dropped_total(), 10);
}

#[test]
fn outbox_outcomes() {
    let mut o = Outbox::new(50);
    for m in ["a", "b", "c"] {
        o.add(m);
    }
    // b is refused for good; c gets a retryable error.
    let mut calls = Vec::new();
    o.flush(
        &mut |t: &str| {
            calls.push(t.to_string());
            match t {
                "b" => SendOutcome::Refused("400".into()),
                "c" => SendOutcome::Retry("500".into()),
                _ => SendOutcome::Delivered,
            }
        },
        "[MD P1]",
    );
    assert_eq!(calls, vec!["a", "b", "c"]);
    assert_eq!(o.len(), 1, "c is kept");
    assert_eq!(o.dropped(), 1);
    let mut calls = Vec::new();
    o.flush(
        &mut |t: &str| {
            calls.push(t.to_string());
            SendOutcome::Delivered
        },
        "[MD P1]",
    );
    assert_eq!(calls, vec!["c", "[MD P1] 전송 실패로 알림 1개를 버림"]);
    assert_eq!(o.len(), 0);
}

#[test]
fn send_outcomes() {
    let f = FakeHttp::start(ok());
    f.script([
        Answer::Reply(400, r#"{"ok":false,"description":"bad"}"#.into()),
        Answer::Reply(413, r#"{"ok":false}"#.into()),
        Answer::Reply(401, r#"{"ok":false}"#.into()),
        Answer::Reply(403, r#"{"ok":false}"#.into()),
        Answer::Reply(429, r#"{"ok":false}"#.into()),
        Answer::Reply(500, "oops".into()),
        Answer::Close,
        Answer::Reply(200, r#"{"ok":false}"#.into()),
    ]);
    let clock = Arc::new(TestClock(AtomicI64::new(0)));
    let h = AlertDirect::start(cfg_for(vec![loopback(&f)]), clock).expect("start");
    h.once("refused 1");
    h.once("refused 2");
    h.once("kept");
    // 400, 413 drop the first two; 401, 403, 429, 500, close, ok:false keep
    // "kept" through six flushes; then 200 delivers it and the drop notice.
    assert!(h.flush_for(Duration::from_secs(10)), "outbox empties");
    let t = texts(&f);
    assert_eq!(t.len(), 10, "{t:?}");
    assert!(t[2].ends_with("\nkept"));
    assert!(
        t[2..9].iter().all(|x| x == &t[2]),
        "the same part is retried"
    );
    assert_eq!(t[9], "[MD P1] 전송 실패로 알림 2개를 버림");
    let r = &f.requests()[0];
    assert_eq!(r.method, "POST");
    assert_eq!(r.path(), format!("/bot{TOKEN}/sendMessage"));
    let body: serde_json::Value = serde_json::from_slice(&r.body).expect("json");
    assert_eq!(body["chat_id"], "42");
    assert_eq!(body["disable_web_page_preview"], true);
    let st = h.state();
    assert_eq!(st.dropped_total, 2);
    assert!(st.failures >= 6);
    for e in &st.last_errors {
        assert!(!e.contains(TOKEN), "error shows the token: {e}");
    }
}

#[test]
fn timeout_keeps_the_part() {
    let f = FakeHttp::start(ok());
    f.script([Answer::Hang]);
    let clock = Arc::new(TestClock(AtomicI64::new(0)));
    let h = AlertDirect::start(cfg_for(vec![loopback(&f)]), clock).expect("start");
    h.once("slow");
    assert!(h.flush_for(Duration::from_secs(10)));
    let t = texts(&f);
    assert_eq!(t.len(), 2, "sent again after the timeout: {t:?}");
    assert_eq!(t[0], t[1]);
}

#[test]
fn fallback_route_and_last_good_first() {
    let f = FakeHttp::start(ok());
    let dead = Route::direct(&format!("http://{}", dead_addr())).expect("route");
    let pinned = Route::pinned(
        &format!("http://tg.example.test:{}", f.addr().port()),
        "tg.example.test",
        f.addr(),
    )
    .expect("route");
    let clock = Arc::new(TestClock(AtomicI64::new(0)));
    let h = AlertDirect::start(cfg_for(vec![dead, pinned]), clock).expect("start");
    h.once("one");
    assert!(h.flush_for(Duration::from_secs(10)));
    let r = f.requests();
    assert_eq!(r.len(), 1);
    assert_eq!(
        r[0].header("host"),
        Some(format!("tg.example.test:{}", f.addr().port()).as_str())
    );
    assert_eq!(
        h.state().route_order,
        vec![1, 0],
        "the route that worked goes first"
    );
    h.once("two");
    assert!(h.flush_for(Duration::from_secs(10)));
    assert_eq!(f.requests().len(), 2);
    assert_eq!(h.state().failures, 1, "the dead route was tried once only");
}

#[test]
fn default_routes_are_telegram_and_pinned_ip() {
    let tg = TelegramConfig::new(TOKEN, "42").expect("config");
    let c = AlertConfig::new(tg, "[MD P1]", "bybit");
    let d: Vec<String> = c.routes.iter().map(|r| format!("{r:?}")).collect();
    assert_eq!(d.len(), 2);
    assert!(
        d[0].contains("https://api.telegram.org/") && d[0].contains("None"),
        "{}",
        d[0]
    );
    assert!(
        d[1].contains("https://api.telegram.org/") && d[1].contains("149.154.167.220:443"),
        "{}",
        d[1]
    );
}

#[test]
fn repeat_rule_with_injected_clock() {
    const MIN: i64 = 60_000;
    let mut r = Repeat::new(5 * MIN);
    assert_eq!(
        r.set("disk", Some("floor 1"), 0),
        Some("floor 1".to_string())
    );
    assert_eq!(r.set("disk", Some("floor 2"), 4 * MIN), None);
    assert_eq!(
        r.set("disk", Some("floor 3"), 5 * MIN),
        Some("floor 3".to_string())
    );
    assert_eq!(r.set("disk", Some("floor 4"), 9 * MIN), None);
    assert_eq!(
        r.set("disk", Some("floor 5"), 10 * MIN),
        Some("floor 5".to_string())
    );
    assert_eq!(
        r.set("disk", None, 11 * MIN),
        Some("해소: floor 1".to_string())
    );
    assert_eq!(r.set("disk", None, 12 * MIN), None);
    assert_eq!(r.set("other", None, 12 * MIN), None);
    assert_eq!(r.active(), Vec::<String>::new());
}

#[test]
fn handle_set_uses_the_repeat_rule_and_the_prefix() {
    let f = FakeHttp::start(ok());
    let clock = Arc::new(TestClock(AtomicI64::new(1_763_500_000_000)));
    let h = AlertDirect::start(cfg_for(vec![loopback(&f)]), clock.clone()).expect("start");
    h.set("disk_floor", Some("floor".into()));
    h.set("disk_floor", Some("floor".into()));
    assert!(h.flush_for(Duration::from_secs(5)));
    clock.0.fetch_add(5 * 60_000, Ordering::SeqCst);
    h.set("disk_floor", Some("floor".into()));
    h.set("disk_floor", None);
    assert!(h.flush_for(Duration::from_secs(5)));
    let t = texts(&f);
    assert_eq!(t.len(), 3, "{t:?}");
    // 1_763_500_000_000 ms = 2025-11-18 21:06:40 UTC = 11-19 06:06:40 KST.
    assert_eq!(t[0], "[MD P1] bybit 11-19 06:06:40 KST\nfloor");
    assert!(t[2].ends_with("\n해소: floor"));
}

#[test]
fn set_never_waits() {
    let f = FakeHttp::start(Answer::Hang);
    let clock = Arc::new(TestClock(AtomicI64::new(0)));
    let mut c = cfg_for(vec![loopback(&f)]);
    c.timeout = Duration::from_secs(30);
    let h = AlertDirect::start(c, clock).expect("start");
    let t0 = Instant::now();
    for i in 0..500 {
        h.once(&format!("m{i}"));
        h.set(&format!("k{i}"), Some("x".into()));
    }
    assert!(
        t0.elapsed() < Duration::from_millis(50),
        "{:?}",
        t0.elapsed()
    );
    assert!(h.state().queued <= 50, "the outbox stays bounded");
}

#[test]
fn repeat_state_survives_restart() {
    const MIN: i64 = 60_000;
    let d = test_dir("state");
    let state = d.join("alert-state.json");
    let f = FakeHttp::start(ok());
    let clock = Arc::new(TestClock(AtomicI64::new(1_763_500_000_000)));
    let start = |clock: Arc<TestClock>| {
        let mut c = cfg_for(vec![loopback(&f)]);
        c.state_file = Some(state.clone());
        AlertDirect::start(c, clock).expect("start")
    };
    // A restart loop: each process sees the condition again.
    let h = start(clock.clone());
    h.set("l0_write", Some("ENOSPC".into()));
    assert!(h.flush_for(Duration::from_secs(5)));
    drop(h);
    for _ in 0..3 {
        clock.0.fetch_add(MIN, Ordering::SeqCst);
        let h = start(clock.clone());
        h.set("l0_write", Some("ENOSPC".into()));
        assert!(h.flush_for(Duration::from_secs(5)));
    }
    assert_eq!(texts(&f).len(), 1, "no message per restart");
    clock.0.fetch_add(2 * MIN, Ordering::SeqCst);
    let h = start(clock.clone());
    h.set("l0_write", Some("ENOSPC".into()));
    assert!(h.flush_for(Duration::from_secs(5)));
    assert_eq!(texts(&f).len(), 2, "the 5 min repeat holds across restarts");
    drop(h);
    let h = start(clock.clone());
    h.set("l0_write", None);
    assert!(h.flush_for(Duration::from_secs(5)));
    let t = texts(&f);
    assert_eq!(t.len(), 3);
    assert!(t[2].ends_with("\n해소: ENOSPC"), "{}", t[2]);
}

#[test]
fn default_timeouts() {
    let tg = TelegramConfig::new(TOKEN, "42").expect("config");
    let c = AlertConfig::new(tg, "[MD P1]", "bybit");
    assert_eq!(c.connect_timeout, Duration::from_secs(3));
    assert_eq!(c.timeout, Duration::from_secs(10));
    assert_eq!(c.outbox_limit, 50);
    assert_eq!(c.repeat_ms, 300_000);
}

#[test]
fn forget_makes_the_next_start_send_again() {
    let d = test_dir("forget");
    let state = d.join("alert-state.json");
    let f = FakeHttp::start(ok());
    let clock = Arc::new(TestClock(AtomicI64::new(0)));
    let mut c = cfg_for(vec![loopback(&f)]);
    c.state_file = Some(state.clone());
    let h = AlertDirect::start(c.clone(), clock.clone()).expect("start");
    h.set("l0_write", Some("EIO".into()));
    assert!(h.flush_for(Duration::from_secs(5)));
    h.forget("l0_write");
    drop(h);
    let h = AlertDirect::start(c, clock).expect("start");
    h.set("l0_write", Some("EIO".into()));
    assert!(h.flush_for(Duration::from_secs(5)));
    assert_eq!(texts(&f).len(), 2, "sent again after forget");
    assert_eq!(h.state().active, vec!["l0_write".to_string()]);
}

// ---------------------------------------------------------------- U18a1

/// A second channel that keeps what it delivered; `fail` makes every send
/// an error, `delay` makes each send take that long.
struct FakeSecond {
    sent: Mutex<Vec<String>>,
    fail: AtomicBool,
    delay: Duration,
}

impl FakeSecond {
    fn new(fail: bool, delay: Duration) -> Arc<FakeSecond> {
        Arc::new(FakeSecond {
            sent: Mutex::new(Vec::new()),
            fail: AtomicBool::new(fail),
            delay,
        })
    }

    fn sent(&self) -> Vec<String> {
        self.sent.lock().expect("lock").clone()
    }
}

impl SecondChannel for FakeSecond {
    fn name(&self) -> &str {
        "fake"
    }

    fn send(&self, text: &str) -> Result<(), String> {
        std::thread::sleep(self.delay);
        if self.fail.load(Ordering::SeqCst) {
            return Err("fake second channel refuses".into());
        }
        self.sent.lock().expect("lock").push(text.to_string());
        Ok(())
    }
}

fn with_second(routes: Vec<Route>, second: &Arc<FakeSecond>) -> AlertConfig {
    let mut c = cfg_for(routes);
    let s: Arc<dyn SecondChannel> = second.clone();
    c.second = Some(s);
    c
}

/// Polls `f` every 5 ms until it is true or `d` passed.
fn wait_until(d: Duration, mut f: impl FnMut() -> bool) -> bool {
    let end = Instant::now() + d;
    loop {
        if f() {
            return true;
        }
        if Instant::now() >= end {
            return false;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn failing_since(h: &AlertHandle) -> Option<i64> {
    h.state().failing_since_ms
}

#[test]
fn failing_since_set_and_reset() {
    const T0: i64 = 1_763_500_000_000;
    let f = FakeHttp::start(Answer::Reply(500, r#"{"ok":false}"#.into()));
    let clock = Arc::new(TestClock(AtomicI64::new(T0)));
    let h = AlertDirect::start(cfg_for(vec![loopback(&f)]), clock.clone()).expect("start");
    assert_eq!(failing_since(&h), None);
    // 500: set at the clock time of the first attempt, kept while it fails.
    h.once("a");
    assert!(
        wait_until(Duration::from_secs(5), || failing_since(&h) == Some(T0)),
        "{:?}",
        h.state()
    );
    clock.0.fetch_add(10_000, Ordering::SeqCst);
    let n = f.requests().len();
    f.wait_requests(n + 3, Duration::from_secs(5));
    assert_eq!(failing_since(&h), Some(T0), "later failures keep the start");
    // 200: Telegram answered, reachable again.
    f.set_default(ok());
    assert!(h.flush_for(Duration::from_secs(5)));
    assert_eq!(failing_since(&h), None, "reset on delivery");
    // 500 again, then 400: a refusal is an answer, so it resets as well.
    f.set_default(Answer::Reply(500, r#"{"ok":false}"#.into()));
    let t1 = clock.0.fetch_add(60_000, Ordering::SeqCst) + 60_000;
    h.once("b");
    assert!(
        wait_until(Duration::from_secs(5), || failing_since(&h) == Some(t1)),
        "{:?}",
        h.state()
    );
    f.set_default(Answer::Reply(400, r#"{"ok":false}"#.into()));
    assert!(h.flush_for(Duration::from_secs(5)));
    assert_eq!(failing_since(&h), None, "reset on a refusal");
    // Telegram fails now, but nothing is queued: no attempt, no episode.
    f.set_default(Answer::Reply(500, r#"{"ok":false}"#.into()));
    let n = f.requests().len();
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(f.requests().len(), n, "an empty outbox sends nothing");
    assert_eq!(failing_since(&h), None, "an empty outbox starts no episode");
}

#[test]
fn second_channel_gets_every_message() {
    let f = FakeHttp::start(ok());
    let second = FakeSecond::new(false, Duration::ZERO);
    let clock = Arc::new(TestClock(AtomicI64::new(1_763_500_000_000)));
    let h =
        AlertDirect::start(with_second(vec![loopback(&f)], &second), clock.clone()).expect("start");
    h.once("one");
    h.set("disk", Some("start".into()));
    h.set("disk", Some("start".into()));
    assert!(h.flush_for(Duration::from_secs(5)));
    clock.0.fetch_add(5 * 60_000, Ordering::SeqCst);
    h.set("disk", Some("again".into()));
    h.set("disk", None);
    h.set("disk", None);
    assert!(h.flush_for(Duration::from_secs(5)));
    let t = texts(&f);
    assert_eq!(t.len(), 4, "{t:?}");
    assert!(t[0].ends_with("\none") && t[1].ends_with("\nstart"));
    assert!(t[2].ends_with("\nagain") && t[3].ends_with("\n해소: start"));
    assert_eq!(second.sent(), t, "each message once, whole, in order");
    let st = h.state().second.expect("second state");
    assert_eq!(st.name, "fake");
    assert_eq!((st.queued, st.delivered, st.dropped_total), (0, 4, 0));
}

#[test]
fn second_outbox_bound() {
    let f = FakeHttp::start(ok());
    let second = FakeSecond::new(true, Duration::ZERO);
    let clock = Arc::new(TestClock(AtomicI64::new(0)));
    let h = AlertDirect::start(with_second(vec![loopback(&f)], &second), clock).expect("start");
    for i in 0..60 {
        h.once(&format!("m{i}"));
    }
    assert!(
        wait_until(Duration::from_secs(10), || h.state().queued == 0),
        "Telegram delivers its copies: {:?}",
        h.state()
    );
    assert!(
        !h.flush_for(Duration::from_millis(100)),
        "the second keeps failing"
    );
    let st = h.state().second.expect("second state");
    assert_eq!((st.queued, st.dropped_total), (50, 10), "{st:?}");
    assert!(st.failures >= 1);
    // An error ends the flush: the next try is a flush period later
    // (50 ms here), not at once.
    std::thread::sleep(Duration::from_millis(300));
    let tries = h.state().second.expect("second state").failures - st.failures;
    assert!(tries <= 12, "{tries} tries in 300 ms");
    second.fail.store(false, Ordering::SeqCst);
    assert!(h.flush_for(Duration::from_secs(5)));
    let s = second.sent();
    assert_eq!(s.len(), 51, "{s:?}");
    assert!(s[0].ends_with("\nm10") && s[49].ends_with("\nm59"));
    assert_eq!(s[50], "[MD P1] 전송 실패로 알림 10개를 버림");
    // Telegram's outbox has the same bound and its own count.
    let tg = h.state().dropped_total;
    let own = format!("[MD P1] 전송 실패로 알림 {tg}개를 버림");
    let t = texts(&f);
    assert!(
        t.iter()
            .filter(|x| x.contains("개를 버림"))
            .all(|x| x == &own),
        "the second's drop notice is its own: {t:?}"
    );
}

fn notices(second: &FakeSecond) -> Vec<String> {
    second
        .sent()
        .into_iter()
        .filter(|t| t.contains("Telegram 전송이"))
        .collect()
}

fn wait_notices(second: &FakeSecond, n: usize, d: Duration) -> bool {
    wait_until(d, || notices(second).len() == n)
}

#[test]
fn telegram_down_notice_after_60s() {
    const T0: i64 = 1_763_500_000_000;
    let f = FakeHttp::start(Answer::Reply(500, r#"{"ok":false}"#.into()));
    let second = FakeSecond::new(false, Duration::ZERO);
    let clock = Arc::new(TestClock(AtomicI64::new(T0)));
    let mut c = with_second(vec![loopback(&f)], &second);
    c.flush_every = Duration::from_millis(20);
    let h = AlertDirect::start(c, clock.clone()).expect("start");
    h.once("P1");
    assert!(wait_until(Duration::from_secs(5), || {
        second.sent().len() == 1 && failing_since(&h) == Some(T0)
    }));
    assert!(second.sent()[0].ends_with("\nP1"), "the copy goes at once");
    // 59 s and exactly 60 s: not yet (the rule is "more than").
    for at in [59_000, 60_000] {
        clock.0.store(T0 + at, Ordering::SeqCst);
        let n = f.requests().len();
        f.wait_requests(n + 5, Duration::from_secs(5));
        assert_eq!(second.sent().len(), 1, "at +{at} ms: {:?}", second.sent());
    }
    // 61 s: the notice within 1 s real time.
    clock.0.store(T0 + 61_000, Ordering::SeqCst);
    let t = Instant::now();
    assert!(wait_notices(&second, 1, Duration::from_secs(1)));
    assert!(t.elapsed() < Duration::from_secs(1));
    let n = &notices(&second)[0];
    assert!(
        n.starts_with("[MD P1] bybit ")
            && n.ends_with(
                "\nTelegram 전송이 61초째 실패함. 대기 1개. 마지막 오류: telegram answered 500: "
            ),
        "{n}"
    );
    // The same notice does not come again within the repeat period ...
    clock.0.store(T0 + 61_000 + 299_000, Ordering::SeqCst);
    let k = f.requests().len();
    f.wait_requests(k + 5, Duration::from_secs(5));
    assert_eq!(notices(&second).len(), 1);
    // ... and once after it.
    clock.0.store(T0 + 61_000 + 300_000, Ordering::SeqCst);
    assert!(wait_notices(&second, 2, Duration::from_secs(1)));
    let k = f.requests().len();
    f.wait_requests(k + 5, Duration::from_secs(5));
    assert_eq!(notices(&second).len(), 2, "one repeat");
    assert!(notices(&second)[1].contains("361초째"));
    // Telegram answers: the clear notice.
    f.set_default(ok());
    assert!(wait_notices(&second, 3, Duration::from_secs(2)));
    assert!(h.flush_for(Duration::from_secs(5)));
    assert!(
        notices(&second)[2].ends_with(
            "\n해소: Telegram 전송이 61초째 실패함. 대기 1개. 마지막 오류: telegram answered 500: "
        ),
        "{:?}",
        notices(&second)
    );
    assert_eq!(second.sent().len(), 4);
    assert!(
        !texts(&f).iter().any(|t| t.contains("Telegram 전송이")),
        "the notice goes to the second channel only"
    );
    assert!(h.state().active.is_empty(), "not a Telegram condition");
}

#[test]
fn flush_for_waits_for_both() {
    let f = FakeHttp::start(ok());
    let slow = FakeSecond::new(false, Duration::from_millis(300));
    let clock = Arc::new(TestClock(AtomicI64::new(0)));
    let h =
        AlertDirect::start(with_second(vec![loopback(&f)], &slow), clock.clone()).expect("start");
    h.once("x");
    assert!(h.flush_for(Duration::from_secs(5)));
    assert_eq!(
        slow.sent().len(),
        1,
        "flush_for returned before the second sent"
    );
    assert_eq!(texts(&f).len(), 1);
    // A second channel that fails holds flush_for to its time limit.
    let failing = FakeSecond::new(true, Duration::ZERO);
    let h = AlertDirect::start(with_second(vec![loopback(&f)], &failing), clock).expect("start");
    h.once("y");
    let t = Instant::now();
    assert!(!h.flush_for(Duration::from_millis(300)));
    assert!(t.elapsed() >= Duration::from_millis(300));
    assert_eq!(texts(&f).len(), 2, "Telegram delivered");
}
