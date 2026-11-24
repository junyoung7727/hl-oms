//! The watcher's own Telegram sender (C-WATCH part 10). U19b1a, the pure
//! parts: the message header, the split into parts of at most 3,500 UTF-16
//! units, the outbox of 50 parts, the outcome of an answer and the repeat
//! rule; the token is never printed. U19b1b, the sender task against
//! loopback fakes: the two routes (a network error moves to the next, the
//! route that answered goes first), the flush pace on the injected clock
//! (the runtime's tick notifies the task), a retry that keeps its part,
//! the token never in a log line, and the shutdown flush bounded in real
//! time. Nothing here starts the runtime.

#![allow(clippy::unwrap_used)]

mod common;

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use http_fake::{Answer, FakeHttp, Request, dead_addr};
use serde_json::Value;
use watcher::alert::{
    self, Alerts, Handle, MSG_LIMIT, MismatchNotices, OUTBOX_LIMIT, Outbox, Outcome, Part, Route, Token,
};
use watcher::clock::{Clock, ManualClock};
use watcher::config::Config;
use watcher::metrics::Metrics;
use watcher::runtime::Endpoints;

#[test]
fn the_token_is_never_in_debug() {
    let secret = "987654:made-up_secret-of-the-bot";
    let t = Token::new(secret, "-1001234").unwrap();
    let shown = format!("{t:?}");
    assert!(
        !shown.contains(secret) && !shown.contains("made-up"),
        "{shown}"
    );
    assert!(shown.contains("token: <hidden>"), "{shown}");
    assert!(shown.contains("-1001234"), "{shown}");
    assert_eq!(t.chat_id(), "-1001234");
    let pretty = format!("{t:#?}");
    assert!(!pretty.contains("made-up"), "{pretty}");
    // A refused token is not quoted either.
    let e = Token::new("made-up-no-colon", "1").unwrap_err();
    assert!(!e.contains("made-up"), "{e}");
    assert!(Token::new(secret, "").is_err());

    // The config holds the path only: its `Debug` names the file.
    let cfg = Config::from_toml(
        r#"name = "w1"
network = "testnet"
credentials_file = "/etc/watcher/creds.toml"
sub_account_uid = 4242
symbols = ["BTCUSDT"]
ip_echo_url = "https://ip.example/"
data_dir = "/srv/oms/data"
status_url = "http://127.0.0.1:8188/api/status"

[telegram]
token_file = "/etc/watcher/telegram.json"
"#,
    )
    .unwrap();
    let shown = format!("{cfg:?}");
    assert!(shown.contains("/etc/watcher/telegram.json"), "{shown}");
    assert!(!shown.contains("made-up"), "{shown}");
}

fn units(s: &str) -> usize {
    s.encode_utf16().count()
}

#[test]
fn a_message_has_the_watch_p1_header_with_the_name_and_kst() {
    // 2026-11-23 12:00:00 UTC is 21:00:00 KST.
    assert_eq!(
        alert::message("w-1", 1_795_435_200_000, "워처 W1 불일치 확정 1건."),
        "[WATCH P1] bybit w-1 11-23 21:00:00 KST\n워처 W1 불일치 확정 1건."
    );
    assert_eq!(alert::kst(0), "01-01 09:00:00 KST");
    // A leap day in UTC is the next morning in KST; milliseconds are cut.
    assert_eq!(alert::kst(1_835_481_599_999), "03-01 08:59:59 KST");
    // New Year in KST while UTC is still in the old year.
    assert_eq!(alert::kst(1_798_729_200_000), "01-01 00:00:00 KST");
    assert_eq!(alert::PREFIX, "[WATCH P1]");
}

#[test]
fn messages_split_at_3500_utf16_units_at_line_ends() {
    assert_eq!(MSG_LIMIT, 3500);
    let split = |t: &str| alert::split(t, MSG_LIMIT);
    // A short text is one part.
    assert_eq!(split("a\nb"), ["a\nb"]);
    // Whole lines fill a part up to the limit, the line end included.
    let line = "x".repeat(1000);
    let text = [line.as_str(); 7].join("\n");
    let parts = split(&text);
    assert_eq!(parts.len(), 3);
    assert_eq!(parts[0], [line.as_str(); 3].join("\n"));
    assert_eq!(parts[2], line);
    assert_eq!(parts.join("\n"), text);
    // Exactly 3,500 units is one part; one more cuts at the line end.
    let exact = format!("{}\n{}", "a".repeat(1749), "b".repeat(1750));
    assert_eq!(units(&exact), 3500);
    assert_eq!(split(&exact), [exact.as_str()]);
    let over = format!("{exact}b");
    assert_eq!(split(&over), ["a".repeat(1749), "b".repeat(1751)]);
    // UTF-16 units, not bytes or characters: a Hangul syllable is one
    // unit (three bytes), an emoji two (one character).
    let hangul = "가".repeat(3500);
    assert_eq!(split(&hangul), [hangul.as_str()]);
    let parts = split(&format!("{hangul}가"));
    assert_eq!(parts, [hangul.clone(), "가".to_string()]);
    let emoji = "😀".repeat(1751);
    let parts = split(&emoji);
    assert_eq!(parts, ["😀".repeat(1750), "😀".to_string()]);
    assert!(parts.iter().all(|p| units(p) <= 3500));
    // One unit left and a two-unit character next: the character goes to
    // the next part (3,499 units, not 3,501).
    let odd = format!("a{}", "😀".repeat(1750));
    let parts = split(&odd);
    assert_eq!(parts, [format!("a{}", "😀".repeat(1749)), "😀".to_string()]);
    assert_eq!(units(&parts[0]), 3499);
    // An overlong line is cut by characters, after the part before it;
    // its rest starts the next part and takes the following lines.
    let text = format!("x\n{}\nz", "y".repeat(8000));
    let parts = split(&text);
    assert_eq!(
        parts,
        [
            "x".to_string(),
            "y".repeat(3500),
            "y".repeat(3500),
            format!("{}\nz", "y".repeat(1000)),
        ]
    );
    // A line of exactly two parts leaves no empty part behind.
    assert_eq!(
        split(&"y".repeat(7000)),
        ["y".repeat(3500), "y".repeat(3500)]
    );
    // Every part of a long mixed text is within the limit and nothing is
    // lost (the cuts are at line ends or inside an overlong line).
    let mut text = String::new();
    for i in 0..400 {
        text.push_str(&"가😀a".repeat(i % 37));
        text.push('\n');
    }
    text.push_str(&"😀".repeat(5000));
    let parts = split(&text);
    assert!(parts.iter().all(|p| units(p) <= 3500 && !p.is_empty()));
    let rejoined: String = parts.concat();
    assert_eq!(rejoined.replace('\n', ""), text.replace('\n', ""));
    // A blank line at a cut or at the start makes no empty part and no part
    // that starts with a line end: it is the one thing a split may drop
    // (PR #3; review 1 S1).
    let full = "x".repeat(3500);
    assert_eq!(split(&format!("{full}\n\ny")), [full.as_str(), "y"]);
    assert_eq!(split("\n\nabc"), ["abc"]);
    assert_eq!(split(&format!("{full}\n")), [full.as_str()]);
    assert_eq!(split("a\n\nb"), ["a\n\nb"]);
}

/// Sends the next part with `o` and applies the outcome; the part sent.
fn send(b: &mut Outbox, o: Outcome) -> (Part, bool) {
    let p = b.next().expect("a part to send");
    let go_on = b.done(&p, &o);
    (p, go_on)
}

#[test]
fn answers_400_and_413_drop_the_part_and_other_answers_keep_it() {
    let ok = br#"{"ok": true, "result": {"message_id": 7}}"#;
    // Delivered: a 2xx answer whose body says "ok": true.
    for status in [200, 201, 299] {
        assert_eq!(alert::outcome(status, ok), Outcome::Delivered, "{status}");
    }
    // Refused for good (this message): 400 and 413, with Telegram's
    // description in one line of at most 200 characters.
    let long = format!(
        r#"{{"ok": false, "error_code": 400, "description": "Bad Request: message is too long\n{}"}}"#,
        "z".repeat(1000)
    );
    for (status, body) in [(400, long.as_bytes()), (413, b"" as &[u8]), (400, ok)] {
        match alert::outcome(status, body) {
            Outcome::Refused(t) => {
                assert!(t.starts_with(&format!("telegram answered {status}")), "{t}");
                assert!(!t.contains('\n') && t.chars().count() <= 240, "{t}");
            }
            o => panic!("{status}: {o:?}"),
        }
    }
    match alert::outcome(400, long.as_bytes()) {
        Outcome::Refused(t) => assert!(t.contains("Bad Request: message is too long"), "{t}"),
        o => panic!("{o:?}"),
    }
    // Everything else keeps the part: other statuses (the bot, the chat,
    // a rate limit, Telegram's own trouble), a 2xx without "ok": true.
    for (status, body) in [
        (200, br#"{"ok": false}"# as &[u8]),
        (200, b"not json"),
        (200, b""),
        (204, b""),
        (300, ok),
        (301, ok),
        (401, br#"{"ok": false, "description": "Unauthorized"}"#),
        (403, b""),
        (404, b""),
        (409, b""),
        (429, br#"{"ok": false, "parameters": {"retry_after": 5}}"#),
        (500, b""),
        (502, b""),
        (503, b""),
        (504, b""),
    ] {
        match alert::outcome(status, body) {
            Outcome::Retry(t) => assert!(t.contains(&status.to_string()), "{t}"),
            o => panic!("{status}: {o:?}"),
        }
    }

    // In the outbox: a refused part is dropped and counted, a retry keeps
    // the part and ends the flush, the next flush sends it again.
    let mut b = Outbox::new(OUTBOX_LIMIT);
    for m in ["A", "B", "C"] {
        b.add(m);
    }
    let (p, go_on) = send(&mut b, alert::outcome(400, b""));
    assert_eq!((p.text.as_str(), go_on), ("A", true));
    assert_eq!((b.len(), b.dropped()), (2, 1));
    let (p, go_on) = send(&mut b, alert::outcome(500, b""));
    assert_eq!((p.text.as_str(), go_on), ("B", false));
    assert_eq!((b.len(), b.dropped()), (2, 1));
    let (p, go_on) = send(&mut b, alert::outcome(200, ok));
    assert_eq!((p.text.as_str(), go_on), ("B", true));
    let (p, _) = send(&mut b, alert::outcome(413, b""));
    assert_eq!(p.text, "C");
    // The refused parts are told once the queue is empty.
    let (p, go_on) = send(&mut b, alert::outcome(200, ok));
    assert_eq!(p.text, "[WATCH P1] 전송 실패로 알림 2개를 버림");
    assert!(go_on && b.settled() && b.next().is_none());
    assert_eq!(b.dropped_total(), 2);
}

#[test]
fn the_outbox_keeps_50_parts_and_reports_the_drops_once() {
    assert_eq!(OUTBOX_LIMIT, 50);
    let ok = Outcome::Delivered;
    let mut b = Outbox::new(OUTBOX_LIMIT);
    assert!(b.settled() && b.next().is_none());
    // 60 messages: the 10 oldest are dropped and counted.
    for i in 0..60 {
        b.add(&format!("m{i}"));
    }
    assert_eq!((b.len(), b.dropped(), b.dropped_total()), (50, 10, 10));
    for i in 10..60 {
        let (p, go_on) = send(&mut b, ok.clone());
        assert_eq!(p.text, format!("m{i}"));
        assert!(go_on);
    }
    // Then one notice with the count, and nothing after it.
    let n = b.next().unwrap();
    assert_eq!((n.id, n.count), (None, 10));
    assert_eq!(n.text, "[WATCH P1] 전송 실패로 알림 10개를 버림");
    // A failed notice is kept for the next flush, as a part is.
    assert!(!b.done(&n, &Outcome::Retry("telegram answered 502: ".into())));
    assert_eq!(b.dropped(), 10);
    assert_eq!(b.next(), Some(n.clone()));
    assert!(b.done(&n, &ok));
    assert!(b.settled() && b.next().is_none());
    b.add("later");
    let (p, _) = send(&mut b, ok.clone());
    assert_eq!(p.text, "later");
    assert!(b.settled() && b.next().is_none(), "the drops are told once");
    assert_eq!(b.dropped_total(), 10);

    // A message of several parts takes several places.
    b.add(&"q".repeat(3 * MSG_LIMIT));
    assert_eq!(b.len(), 3);
    while b.next().is_some() {
        send(&mut b, ok.clone());
    }

    // A part that overflow dropped while it was in flight is not removed
    // twice, delivered or refused, and counted once.
    for i in 0..50 {
        b.add(&format!("f{i}"));
    }
    let p = b.next().unwrap();
    assert_eq!(p.text, "f0");
    b.add("f50");
    assert_eq!((b.len(), b.dropped()), (50, 1));
    assert!(b.done(&p, &ok));
    assert_eq!(b.next().unwrap().text, "f1");
    let p = b.next().unwrap();
    b.add("f51");
    assert!(b.done(&p, &Outcome::Refused("telegram answered 400: ".into())));
    assert_eq!((b.len(), b.dropped()), (50, 2));
    assert_eq!(b.next().unwrap().text, "f2");

    // Drops after the notice was handed out stay owed for the next one.
    while b.next().is_some_and(|p| p.id.is_some()) {
        send(&mut b, ok.clone());
    }
    let n = b.next().unwrap();
    assert_eq!(n.count, 2);
    for i in 0..52 {
        b.add(&format!("g{i}"));
    }
    assert_eq!(b.dropped(), 4);
    assert!(b.done(&n, &ok));
    assert_eq!(b.dropped(), 2);
    while b.next().is_some_and(|p| p.id.is_some()) {
        send(&mut b, ok.clone());
    }
    let n = b.next().unwrap();
    assert_eq!(n.text, "[WATCH P1] 전송 실패로 알림 2개를 버림");
    // A refused notice counts as told (its count cannot be told).
    assert!(b.done(&n, &Outcome::Refused("telegram answered 400: ".into())));
    assert!(b.settled());
    assert_eq!(b.dropped_total(), 14);
}

#[test]
fn the_repeat_rule_sends_start_every_5_min_and_clear() {
    assert_eq!(alert::REPEAT_MS, 300_000);
    let t0 = 1_795_435_200_000;
    let mut r = alert::Repeat::new(alert::REPEAT_MS);
    // Once at start.
    assert_eq!(r.set("auth", Some("A1"), t0), Some("A1".into()));
    // Nothing while it lasts, until 5 min after the last send.
    for dt in [0, 1, 1000, 150_000, 299_999] {
        assert_eq!(r.set("auth", Some("A1"), t0 + dt), None, "+{dt}");
    }
    // Then the latest text, and the next 5 min from that send.
    assert_eq!(r.set("auth", Some("A2"), t0 + 300_000), Some("A2".into()));
    assert_eq!(r.set("auth", Some("A2"), t0 + 599_999), None);
    // A late caller gets the repeat once, and the next is 5 min after it.
    assert_eq!(r.set("auth", Some("A3"), t0 + 700_000), Some("A3".into()));
    assert_eq!(r.set("auth", Some("A3"), t0 + 999_999), None);
    assert_eq!(r.set("auth", Some("A3"), t0 + 1_000_000), Some("A3".into()));
    // A clock that steps back holds the repeat (saturating).
    assert_eq!(r.set("auth", Some("A3"), t0), None);
    // Once at clear, with the start text; not again.
    assert_eq!(r.set("auth", None, t0 + 1_000_001), Some("해소: A1".into()));
    assert_eq!(r.set("auth", None, t0 + 1_000_002), None);
    // A condition that holds again starts again, at once.
    assert_eq!(r.set("auth", Some("B"), t0 + 1_000_003), Some("B".into()));
    assert_eq!(r.set("auth", None, t0 + 1_000_004), Some("해소: B".into()));
    // Never held: no clear.
    assert_eq!(r.set("public_ip", None, t0), None);
    // Conditions are apart: one key's repeat does not move another's.
    assert_eq!(r.set("public_ip", Some("P"), t0), Some("P".into()));
    assert_eq!(r.set("auth", Some("Q"), t0 + 100_000), Some("Q".into()));
    assert_eq!(
        r.set("public_ip", Some("P"), t0 + 300_000),
        Some("P".into())
    );
    assert_eq!(r.set("auth", Some("Q"), t0 + 300_000), None);
    assert_eq!(r.set("auth", Some("Q"), t0 + 400_000), Some("Q".into()));
    // Bound (spec U19b1c, 1 + ⌊T / 5 min⌋ + 1): polled every 100 ms for
    // T = 3,599,900 ms, a lasting condition gives 1 + 11 messages, and one
    // more at clear.
    let mut r = alert::Repeat::new(alert::REPEAT_MS);
    let mut sent = 0;
    for step in 0..36_000 {
        sent += usize::from(r.set("k", Some("x"), t0 + step * 100).is_some());
    }
    assert_eq!(sent, 1 + 11);
    assert!(r.set("k", None, t0 + 3_600_000).is_some());
}

// ---------------------------------------------------------------- sender task (U19b1b)

const SECRET: &str = "987654:made-up_secret-of-the-bot";
const T0: u64 = 1_795_435_200_000;

fn ok() -> Answer {
    Answer::Reply(200, r#"{"ok":true,"result":{}}"#.into())
}

fn token() -> Token {
    Token::new(SECRET, "-1001234").unwrap()
}

/// A loopback route to `fake` through the reserved name `telegram.invalid`
/// (never `api.telegram.org`: without the `resolve` it fails to resolve).
fn pinned(fake: &FakeHttp) -> Route {
    Route::pinned("http://telegram.invalid/", "telegram.invalid", fake.addr())
}

struct Rig {
    clock: Arc<ManualClock>,
    metrics: Arc<Metrics>,
    lines: Arc<Mutex<Vec<String>>>,
    handle: Handle,
    finish: Option<tokio::sync::oneshot::Sender<u64>>,
    finished: std::sync::mpsc::Receiver<usize>,
    join: Option<std::thread::JoinHandle<()>>,
}

impl Rig {
    /// The sender on its own thread and current-thread runtime, as the
    /// watcher runs it; the runtime ends after `finish`.
    fn new(routes: Vec<Route>) -> Rig {
        let clock = Arc::new(ManualClock::new(T0));
        let metrics = Arc::new(Metrics::new("h", "w-1"));
        let (log, lines) = watcher::log::collect();
        let (c, m): (Arc<dyn Clock>, _) = (clock.clone(), metrics.clone());
        let (ready_tx, ready) = std::sync::mpsc::channel();
        let (finish, finish_rx) = tokio::sync::oneshot::channel::<u64>();
        let (finished_tx, finished) = std::sync::mpsc::channel();
        let join = std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            rt.block_on(async move {
                let (handle, task) = alert::spawn("w-1", routes, token(), c, m, log).unwrap();
                ready_tx.send(handle).unwrap();
                if let Ok(ms) = finish_rx.await {
                    let _ = finished_tx.send(task.finish(ms).await);
                }
            });
        });
        let handle = ready.recv_timeout(Duration::from_secs(5)).unwrap();
        Rig {
            clock,
            metrics,
            lines,
            handle,
            finish: Some(finish),
            finished,
            join: Some(join),
        }
    }

    fn metric(&self, name: &str) -> f64 {
        self.metrics.get(name, &[]).unwrap_or(-1.0)
    }

    fn lines(&self) -> Vec<String> {
        self.lines.lock().unwrap().clone()
    }

    /// Waits (bounded, real time) until `f` holds.
    fn until(&self, what: &str, f: impl Fn(&Rig) -> bool) {
        self.until_within(Duration::from_secs(5), what, f);
    }

    /// Waits at most `bound` of real time until `f` holds.
    fn until_within(&self, bound: Duration, what: &str, f: impl Fn(&Rig) -> bool) {
        let end = Instant::now() + bound;
        while !f(self) {
            assert!(Instant::now() < end, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// `Task::finish(ms)` on the sender's runtime; the parts it left.
    fn finish(&mut self, ms: u64) -> usize {
        self.finish.take().unwrap().send(ms).unwrap();
        let left = self.finished.recv_timeout(Duration::from_secs(30)).unwrap();
        self.join.take().unwrap().join().unwrap();
        left
    }
}

fn text(r: &Request) -> String {
    let v: Value = serde_json::from_slice(&r.body).unwrap();
    v["text"].as_str().unwrap().to_string()
}

#[test]
fn a_network_error_moves_to_the_pinned_route() {
    // The production routes: DNS, then api.telegram.org pinned to
    // 149.154.167.220:443 (the host name still gives SNI and certificate).
    let prod = Route::telegram();
    assert_eq!(prod.len(), 2);
    assert_eq!(prod[0], Route::direct("https://api.telegram.org/"));
    assert_eq!(
        prod[1],
        Route::pinned(
            "https://api.telegram.org/",
            "api.telegram.org",
            "149.154.167.220:443".parse().unwrap()
        )
    );

    // The binary takes them from `Endpoints::of`; there is no override.
    // Mainnet needs `[kill]` (U19b3a), whose marker directory must exist.
    let kill_dir = common::test_dir("alert-endpoints");
    let cfg = Config::from_toml(&format!(
        r#"name = "w1"
network = "mainnet"
mainnet_confirmed = true
credentials_file = "/etc/watcher/creds.toml"
sub_account_uid = 4242
symbols = ["BTCUSDT"]
ip_echo_url = "https://ip.example/"
data_dir = "/srv/oms/data"
status_url = "http://127.0.0.1:8188/api/status"

[telegram]
token_file = "/etc/watcher/telegram.json"

[kill]
marker = "{}"
oms_unit = "watcher-test-no-such-unit.service"
"#,
        kill_dir.join("kill.marker").display()
    ))
    .unwrap();
    assert_eq!(Endpoints::of(&cfg).telegram, prod);

    let fake = FakeHttp::start(ok());
    let dead = Route::direct(&format!("http://{}/", dead_addr()));
    let rig = Rig::new(vec![dead, pinned(&fake)]);
    rig.handle.once("워처 W1 불일치 확정 1건.");
    let got = fake.wait_requests(1, Duration::from_secs(5));
    assert_eq!(got.len(), 1);
    let r = &got[0];
    assert_eq!(r.method, "POST");
    assert_eq!(r.path(), format!("/bot{SECRET}/sendMessage"));
    assert_eq!(r.header("host"), Some("telegram.invalid"));
    let body: Value = serde_json::from_slice(&r.body).unwrap();
    assert_eq!(body["chat_id"], "-1001234");
    assert_eq!(body["disable_web_page_preview"], true);
    assert_eq!(
        body["text"],
        "[WATCH P1] bybit w-1 11-23 21:00:00 KST\n워처 W1 불일치 확정 1건."
    );
    rig.until("the delivery", |r| {
        r.metric("watcher_alerts_delivered_total") == 1.0
    });
    // The dead route was tried once and failed.
    assert_eq!(rig.metric("watcher_alert_failures_total"), 1.0);
    assert_eq!(rig.metric("watcher_alerts_queued"), 0.0);

    // The route that answered goes first next time: the dead route is not
    // tried again.
    rig.handle.once("두 번째");
    let got = fake.wait_requests(2, Duration::from_secs(5));
    assert_eq!(got.len(), 2);
    assert!(text(&got[1]).ends_with("\n두 번째"));
    rig.until("the second delivery", |r| {
        r.metric("watcher_alerts_delivered_total") == 2.0
    });
    assert_eq!(rig.metric("watcher_alert_failures_total"), 1.0);
}

/// Real time in which a task that is not woken must send nothing
/// (판단(모델 아님)).
const QUIET: Duration = Duration::from_millis(300);

#[test]
fn the_sender_task_flushes_on_the_manual_clock() {
    let fake = FakeHttp::start(ok());
    fake.script([Answer::Reply(500, r#"{"ok":false}"#.into())]);
    let rig = Rig::new(vec![Route::direct(&fake.url())]);
    assert_eq!(rig.handle.next_flush(), None);
    rig.handle.once("a");
    assert_eq!(fake.wait_requests(1, Duration::from_secs(5)).len(), 1);
    // The flush that left the part ended at T0: the next is due 5,000 ms
    // later on the injected clock.
    rig.until("the retry instant", |r| r.handle.next_flush().is_some());
    assert_eq!(rig.handle.next_flush(), Some(T0 + 5_000));
    assert_eq!(rig.metric("watcher_alerts_queued"), 1.0);

    // A tick before it is due wakes nothing.
    rig.clock.set(T0 + 4_999);
    rig.handle.tick(T0 + 4_999);
    std::thread::sleep(QUIET);
    assert_eq!(fake.requests().len(), 1);
    // The clock alone wakes nothing either: the task does not sleep on any
    // timer; the runtime's tick wakes it.
    rig.clock.set(T0 + 5_000);
    std::thread::sleep(QUIET);
    assert_eq!(fake.requests().len(), 1);

    rig.handle.tick(T0 + 5_000);
    let got = fake.wait_requests(2, Duration::from_secs(5));
    assert_eq!(got.len(), 2);
    assert_eq!(text(&got[1]), text(&got[0]));
    rig.until("the delivery", |r| {
        r.metric("watcher_alerts_delivered_total") == 1.0
    });
    assert_eq!(rig.handle.next_flush(), None);
    assert_eq!(rig.metric("watcher_alerts_queued"), 0.0);
    // Nothing is due any more: later ticks send nothing.
    rig.clock.set(T0 + 60_000);
    rig.handle.tick(T0 + 60_000);
    std::thread::sleep(QUIET);
    assert_eq!(fake.requests().len(), 2);
}

#[test]
fn a_retry_keeps_the_part_for_the_next_flush() {
    let fake = FakeHttp::start(ok());
    fake.script([Answer::Reply(
        502,
        r#"{"ok":false,"description":"Bad Gateway"}"#.into(),
    )]);
    let rig = Rig::new(vec![Route::direct(&fake.url())]);
    rig.handle.once("a");
    assert_eq!(fake.wait_requests(1, Duration::from_secs(5)).len(), 1);
    rig.until("the retry instant", |r| r.handle.next_flush().is_some());
    // The part is kept; the flush ended at it.
    assert_eq!(rig.metric("watcher_alerts_queued"), 1.0);
    assert_eq!(rig.metric("watcher_alerts_delivered_total"), 0.0);
    assert_eq!(rig.metric("watcher_alert_failures_total"), 1.0);

    // A new message goes at once (it wakes the task) behind the kept part.
    rig.handle.once("b");
    let got = fake.wait_requests(3, Duration::from_secs(5));
    let texts: Vec<String> = got.iter().map(text).collect();
    assert_eq!(texts.len(), 3, "{texts:?}");
    assert!(
        texts[0].ends_with("\na") && texts[1].ends_with("\na"),
        "{texts:?}"
    );
    assert!(texts[2].ends_with("\nb"), "{texts:?}");
    rig.until("two deliveries", |r| {
        r.metric("watcher_alerts_delivered_total") == 2.0
    });
    assert_eq!(rig.handle.next_flush(), None);

    // 400 drops the part and counts it; the flush goes on to the drop
    // notice.
    fake.script([Answer::Reply(
        400,
        r#"{"ok":false,"description":"Bad Request: message is too long"}"#.into(),
    )]);
    rig.handle.once("c");
    let got = fake.wait_requests(5, Duration::from_secs(5));
    assert_eq!(got.len(), 5);
    assert!(text(&got[3]).ends_with("\nc"));
    assert_eq!(text(&got[4]), "[WATCH P1] 전송 실패로 알림 1개를 버림");
    rig.until("the notice", |r| {
        r.metric("watcher_alerts_delivered_total") == 3.0
    });
    assert_eq!(rig.metric("watcher_alerts_dropped_total"), 1.0);
    assert_eq!(rig.metric("watcher_alert_failures_total"), 2.0);
    assert_eq!(rig.metric("watcher_alerts_queued"), 0.0);
    assert_eq!(rig.handle.next_flush(), None);
    let lines = rig.lines();
    assert!(
        lines
            .iter()
            .any(|l| l == "warn: telegram send failed: route 0: telegram answered 502: Bad Gateway"),
        "{lines:?}"
    );
    assert!(
        lines
            .iter()
            .any(|l| l.contains("route 0: telegram answered 400") && l.ends_with("(part dropped)")),
        "{lines:?}"
    );
}

#[test]
fn set_queues_what_the_repeat_rule_gives() {
    let fake = FakeHttp::start(ok());
    let rig = Rig::new(vec![Route::direct(&fake.url())]);
    rig.handle
        .set("auth", Some("워처 키가 거부됨: retCode 10010"));
    rig.handle
        .set("auth", Some("워처 키가 거부됨: retCode 10010"));
    rig.clock.advance(300_000);
    rig.handle
        .set("auth", Some("워처 키가 거부됨: retCode 10003"));
    rig.handle.set("auth", None);
    let got = fake.wait_requests(3, Duration::from_secs(5));
    std::thread::sleep(QUIET);
    let texts: Vec<String> = fake.requests().iter().map(text).collect();
    assert_eq!(got.len(), 3);
    assert_eq!(
        texts,
        [
            "[WATCH P1] bybit w-1 11-23 21:00:00 KST\n워처 키가 거부됨: retCode 10010",
            "[WATCH P1] bybit w-1 11-23 21:05:00 KST\n워처 키가 거부됨: retCode 10003",
            "[WATCH P1] bybit w-1 11-23 21:05:00 KST\n해소: 워처 키가 거부됨: retCode 10010",
        ]
    );
}

#[test]
fn the_token_is_never_in_a_log_line() {
    let dead = dead_addr();
    let rig = Rig::new(vec![
        Route::direct(&format!("http://{dead}/")),
        Route::pinned("http://telegram.invalid/", "telegram.invalid", dead),
    ]);
    rig.handle.once("x");
    rig.until("two failed routes", |r| {
        r.metric("watcher_alert_failures_total") == 2.0
    });
    rig.until("the retry instant", |r| r.handle.next_flush().is_some());
    let mut rig = rig;
    assert_eq!(rig.finish(1_000), 1);
    let lines = rig.lines();
    let failed: Vec<&String> = lines
        .iter()
        .filter(|l| l.starts_with("warn: telegram send failed: route "))
        .collect();
    // The first flush and the last one (at `finish`) each tried both
    // routes; no route answered, so the order stayed.
    assert_eq!(failed.len(), 4, "{lines:?}");
    for (n, l) in failed.iter().enumerate() {
        let route = format!("warn: telegram send failed: route {}: ", n % 2);
        assert!(l.starts_with(&route), "{l}");
    }
    // The cause is told (more than the route), the URL is not.
    for l in &lines {
        assert!(!l.contains(SECRET), "{l}");
        assert!(!l.contains("made-up") && !l.contains("bot987654"), "{l}");
        assert!(!l.contains("sendMessage"), "{l}");
    }
    assert!(
        failed
            .iter()
            .all(|l| l.len() > "warn: telegram send failed: route 0: ".len() + 5)
    );
}

#[test]
fn shutdown_flushes_within_5_s_and_counts_the_rest() {
    // A part kept by a failed flush goes out in the last flush, without a
    // tick.
    let fake = FakeHttp::start(ok());
    fake.script([Answer::Reply(500, r#"{"ok":false}"#.into())]);
    let mut rig = Rig::new(vec![Route::direct(&fake.url())]);
    rig.handle.once("a");
    rig.until("the retry instant", |r| r.handle.next_flush().is_some());
    let t = Instant::now();
    assert_eq!(rig.finish(alert::FINISH_MS), 0);
    assert!(t.elapsed() < Duration::from_secs(3), "{:?}", t.elapsed());
    assert_eq!(fake.requests().len(), 2);
    assert_eq!(rig.metric("watcher_alerts_delivered_total"), 1.0);
    assert_eq!(
        rig.lines().last().map(String::as_str),
        Some("alert sender stopped: nothing left to send")
    );

    // Telegram does not answer (the request timeout is 10 s): the last
    // flush is cut at 5 s of real time and the parts left are counted.
    let hang = FakeHttp::start(Answer::Hang);
    let mut rig = Rig::new(vec![Route::direct(&hang.url())]);
    rig.handle.once("a");
    assert_eq!(hang.wait_requests(1, Duration::from_secs(5)).len(), 1);
    rig.handle.once("b");
    rig.handle.once("c");
    let t = Instant::now();
    assert_eq!(rig.finish(alert::FINISH_MS), 3);
    let took = t.elapsed();
    assert!(
        took >= Duration::from_millis(4_900) && took < Duration::from_millis(7_000),
        "{took:?}"
    );
    assert_eq!(hang.requests().len(), 1);
    assert_eq!(
        rig.lines().last().map(String::as_str),
        Some("warn: alert sender stopped at its bound: 3 parts not delivered are dropped")
    );
}

#[test]
fn a_request_without_an_answer_fails_after_10_s() {
    // Telegram (or a box on the way) takes the connection and never
    // answers: the request timeout ends the send, the failure is counted
    // and logged, and the part waits for the next flush (review 1 S1).
    let hang = FakeHttp::start(Answer::Hang);
    let mut rig = Rig::new(vec![Route::direct(&hang.url())]);
    let t = Instant::now();
    rig.handle.once("a");
    assert_eq!(hang.wait_requests(1, Duration::from_secs(5)).len(), 1);
    rig.until_within(Duration::from_secs(15), "the request timeout", |r| {
        r.metric("watcher_alert_failures_total") == 1.0 && r.handle.next_flush().is_some()
    });
    let took = t.elapsed();
    assert_eq!(alert::REQUEST_TIMEOUT_MS, 10_000);
    assert!(
        took >= Duration::from_millis(9_500) && took < Duration::from_millis(13_000),
        "{took:?}"
    );
    assert_eq!(rig.handle.next_flush(), Some(T0 + 5_000));
    assert_eq!(rig.metric("watcher_alerts_queued"), 1.0);
    assert_eq!(rig.metric("watcher_alerts_delivered_total"), 0.0);
    let lines = rig.lines();
    let failed: Vec<&String> = lines
        .iter()
        .filter(|l| l.starts_with("warn: telegram send failed: route 0: "))
        .collect();
    assert_eq!(failed.len(), 1, "{lines:?}");
    assert!(failed[0].ends_with("timed out"), "{}", failed[0]);
    assert!(!failed[0].contains(SECRET), "{}", failed[0]);
    // The last flush meets the same silence and is cut at its bound.
    assert_eq!(rig.finish(200), 1);
}

// ---------------------------------------------------------------- P1 conditions (U19b1c)

const MISMATCH_HEAD: &str =
    "불일치 확정 {k}건. OMS에 일반 주문 차단(watcher_mismatch)을 보내기 시작함(실패하면 다시 보냄; 결과는 OMS의 hold 알림). ";

fn head(check: &str, k: u64) -> String {
    format!("워처 {check} {}", MISMATCH_HEAD.replace("{k}", &k.to_string()))
}

fn strings(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

#[test]
fn mismatch_notices_are_at_most_one_per_check_per_60_s_and_collect_the_rest() {
    use watcher::engine::Check::{W1, W2, W4};
    let mut n = MismatchNotices::default();
    assert!(n.poll(T0).is_empty());
    // The first confirmation goes at once.
    n.add(W1, &strings(&["W1 foreign a"]));
    assert_eq!(n.poll(T0), vec![format!("{}W1 foreign a", head("W1", 1))]);
    assert!(n.poll(T0).is_empty());
    // Within 60 s the next ones of W1 are collected for the next message;
    // another check has its own pace.
    n.add(W1, &strings(&["W1 foreign b"]));
    n.add(W1, &strings(&["W1 foreign c", "W1 foreign d"]));
    assert!(n.poll(T0 + 1_000).is_empty());
    assert!(n.poll(T0 + 59_999).is_empty());
    n.add(W2, &strings(&["W2 position x"]));
    assert_eq!(
        n.poll(T0 + 59_999),
        vec![format!("{}W2 position x", head("W2", 1))]
    );
    assert_eq!(
        n.poll(T0 + 60_000),
        vec![format!(
            "{}W1 foreign b\n그 밖에: W1 foreign c; W1 foreign d",
            head("W1", 3)
        )]
    );
    // The pace counts from the last message.
    n.add(W1, &strings(&["W1 foreign e"]));
    assert!(n.poll(T0 + 60_001).is_empty());
    assert!(n.poll(T0 + 119_999).is_empty());
    assert_eq!(
        n.poll(T0 + 120_000),
        vec![format!("{}W1 foreign e", head("W1", 1))]
    );
    // A clock that steps back holds the pace.
    n.add(W1, &strings(&["W1 foreign f"]));
    assert!(n.poll(T0).is_empty());
    assert_eq!(n.poll(T0 + 180_000).len(), 1);

    // W4 confirms 300 in one round: five details, each cut to 600 UTF-16
    // units, and the count of the rest; the message is one part.
    let long = "가".repeat(700);
    let many: Vec<String> = (0..300)
        .map(|i| format!("W4 unknown_exec_type e{i}: {long}"))
        .collect();
    n.add(W4, &many);
    let m = n.poll(T0 + 200_000);
    assert_eq!(m.len(), 1);
    let body = &m[0];
    assert!(body.starts_with(&head("W4", 300)), "{body}");
    assert!(body.ends_with("; 그리고 295건 더"), "{body}");
    let (first, rest) = body[head("W4", 300).len()..].split_once("\n그 밖에: ").unwrap();
    let rest = rest.strip_suffix("; 그리고 295건 더").unwrap();
    let shown: Vec<&str> = std::iter::once(first).chain(rest.split("; ")).collect();
    assert_eq!(shown.len(), 5);
    for (i, d) in shown.iter().enumerate() {
        assert!(d.starts_with(&format!("W4 unknown_exec_type e{i}: 가")), "{d}");
        assert_eq!(units(d), 600, "{i}");
    }
    let name = "워".repeat(64);
    assert_eq!(alert::split(&alert::message(&name, T0, body), MSG_LIMIT).len(), 1);
}

#[test]
fn mismatch_notices_drain_tells_what_the_pace_holds() {
    // Review 1 S2: at shutdown what is collected inside the 60 s pace is
    // given whatever the pace; then nothing is left.
    use watcher::engine::Check::{W1, W2};
    let mut n = MismatchNotices::default();
    assert!(n.drain().is_empty());
    n.add(W1, &strings(&["W1 foreign a"]));
    assert_eq!(n.poll(T0).len(), 1);
    n.add(W1, &strings(&["W1 foreign b", "W1 foreign c"]));
    n.add(W2, &strings(&["W2 position x"]));
    assert_eq!(n.poll(T0 + 1_000), vec![format!("{}W2 position x", head("W2", 1))]);
    n.add(W2, &strings(&["W2 position y"]));
    assert_eq!(
        n.drain(),
        vec![
            format!("{}W1 foreign b\n그 밖에: W1 foreign c", head("W1", 2)),
            format!("{}W2 position y", head("W2", 1)),
        ]
    );
    assert!(n.drain().is_empty());
    assert!(n.poll(T0 + 1_000_000).is_empty());
}

#[test]
fn the_start_refusal_reason_is_cut_to_200_characters() {
    // Review 1 S3: `워처 시작 거부: {reason cut to 200}` counts characters.
    let tail = ". 워처가 꺼짐: W1~W4와 W5가 돌지 않음";
    let reason = format!("{}{}", "가".repeat(199), "나b".repeat(51));
    let body = alert::refusal_text(&reason);
    assert_eq!(body, format!("워처 시작 거부: {}나{tail}", "가".repeat(199)));
    let short = "query-api: the API key is the master account's";
    assert_eq!(alert::refusal_text(short), format!("워처 시작 거부: {short}{tail}"));
}

/// A client that never sends (its base is never asked), on `clock`.
fn idle_client(clock: Arc<ManualClock>) -> watcher::rest::Client {
    bybit_client("http://127.0.0.1:9", clock)
}

fn bybit_client(base: &str, clock: Arc<ManualClock>) -> watcher::rest::Client {
    let cfg = watcher::rest::ClientConfig {
        base: base.to_string(),
        recv_window_ms: 5000,
        request_timeout_ms: 2000,
    };
    let (log, _) = watcher::log::collect();
    watcher::rest::Client::new(
        &cfg,
        watcher::rest::Credentials::new("WKEY", "watcher-made-up-secret-42"),
        clock,
        Arc::new(Metrics::new("h", "w-1")),
        log,
    )
    .unwrap()
}

fn current_thread() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

const KEY_TAIL: &str = "워처의 Bybit 요청이 모두 멈춤(재시작 전까지): W1~W4는 성공하지 않아 OMS가 일반 주문을 막고, 워처의 일괄 취소(W5)도 나갈 수 없음. 키·IP 목록을 확인하고 워처를 다시 시작";

#[test]
fn an_auth_answer_sends_one_watch_p1_at_once() {
    let rt = current_thread();
    let cases = [
        (
            Answer::Reply(200, r#"{"retCode":10010,"retMsg":"Unmatched IP"}"#.into()),
            "retCode 10010: Unmatched IP",
        ),
        (
            Answer::Reply(200, r#"{"retCode":10003,"retMsg":"API key is invalid."}"#.into()),
            "retCode 10003: API key is invalid.",
        ),
        (
            Answer::Reply(200, r#"{"retCode":33004,"retMsg":"api key expire"}"#.into()),
            "retCode 33004: api key expire",
        ),
        (
            Answer::Reply(401, "invalid api key".into()),
            "HTTP 401: invalid api key",
        ),
    ];
    for (answer, form) in cases {
        let bybit = FakeHttp::start(Answer::Reply(404, "{}".into()));
        bybit.route("/v5/user/query-api", [answer]);
        let fake = FakeHttp::start(ok());
        let rig = Rig::new(vec![Route::direct(&fake.url())]);
        let client = bybit_client(&bybit.url(), rig.clock.clone());
        let ip = watcher::start::PublicIp::default();
        let mut alerts = Alerts::telegram(rig.handle.clone());
        alerts.poll(T0, &client, &ip);
        std::thread::sleep(QUIET);
        assert!(fake.requests().is_empty(), "{form}");
        // The answer latches the key stop: the next poll (the runtime's next
        // tick) sends one message.
        rt.block_on(client.get("/v5/user/query-api", &[]))
            .unwrap_err();
        rig.clock.set(T0 + 100);
        alerts.poll(T0 + 100, &client, &ip);
        let got = fake.wait_requests(1, Duration::from_secs(5));
        assert_eq!(got.len(), 1, "{form}");
        let want = format!("[WATCH P1] bybit w-1 11-23 21:00:00 KST\n워처 키가 거부됨({form}). {KEY_TAIL}");
        assert_eq!(text(&got[0]), want);
        // Every tick asks again: nothing more until 5 min after it.
        for t in [T0 + 200, T0 + 60_000, T0 + 300_099] {
            rig.clock.set(t);
            alerts.poll(t, &client, &ip);
        }
        std::thread::sleep(QUIET);
        assert_eq!(fake.requests().len(), 1, "{form}");
        rig.clock.set(T0 + 300_100);
        alerts.poll(T0 + 300_100, &client, &ip);
        let got = fake.wait_requests(2, Duration::from_secs(5));
        assert_eq!(got.len(), 2, "{form}");
        assert!(text(&got[1]).ends_with(&format!("워처 키가 거부됨({form}). {KEY_TAIL}")));
    }
    // The rule stop has its own first sentence and the same tail.
    let fake = FakeHttp::start(ok());
    let rig = Rig::new(vec![Route::direct(&fake.url())]);
    let client = idle_client(rig.clock.clone());
    client.stop_key("the key no longer passes the start check: query-api: the API key is the master account's");
    let mut alerts = Alerts::telegram(rig.handle.clone());
    alerts.poll(T0, &client, &watcher::start::PublicIp::default());
    let got = fake.wait_requests(1, Duration::from_secs(5));
    assert!(
        text(&got[0]).ends_with(&format!(
            "\n워처 키가 시작 규칙을 더는 지키지 않음(the key no longer passes the start check: query-api: the API key is the master account's). {KEY_TAIL}"
        )),
        "{}",
        text(&got[0])
    );
}

const IP_TAIL: &str = "Bybit가 10010으로 거부할 수 있음(그러면 워처 키 거부 P1이 따름). 이 호스트의 공인 IP와 키 설정을 확인";

#[test]
fn public_ip_ok_0_sends_one_watch_p1_and_a_clear() {
    use serde_json::json;
    use watcher::start::{Start, Step};
    let fake = FakeHttp::start(ok());
    let rig = Rig::new(vec![Route::direct(&fake.url())]);
    let client = idle_client(rig.clock.clone());
    let cfg = common::rig::config();
    let metrics = Arc::new(Metrics::new("h", "w-1"));
    let (log, _) = watcher::log::collect();
    let mut s = Start::new(&cfg, metrics.clone(), log, T0);
    let ip = s.public_ip();
    let mut alerts = Alerts::telegram(rig.handle.clone());
    let key = json!({"isMaster": false, "userID": 1, "ips": ["127.0.0.1"]});
    // Start step 4: the echo shows an address outside the key's list.
    s.poll(T0).unwrap();
    assert_eq!(s.on_answer(T0, Ok(key.clone())), Step::Next);
    s.poll(T0).unwrap();
    s.on_answer(T0, Ok(Value::String("10.0.0.9".into())));
    assert_eq!(metrics.get("watcher_public_ip_ok", &[]), Some(0.0));
    let why = "10.0.0.9 is not among the key's bound IPs";
    assert_eq!(ip.get().as_deref(), Some(why));
    alerts.poll(T0, &client, &ip);
    let got = fake.wait_requests(1, Duration::from_secs(5));
    let first = format!("워처의 공인 IP가 키의 IP 목록에 없음({why}). {IP_TAIL}");
    assert_eq!(text(&got[0]), format!("[WATCH P1] bybit w-1 11-23 21:00:00 KST\n{first}"));
    for t in [T0 + 100, T0 + 1_000, T0 + 59_000] {
        rig.clock.set(t);
        alerts.poll(t, &client, &ip);
    }
    std::thread::sleep(QUIET);
    assert_eq!(fake.requests().len(), 1);
    // Step 5, then the recheck finds the address in the list: one clear.
    s.poll(T0).unwrap();
    let instruments = json!({"list": [{"symbol": "BTCUSDT", "settleCoin": "USDT"}]});
    assert_eq!(s.on_answer(T0, Ok(instruments)), Step::Done);
    let mut check = s.ip_check().unwrap();
    let at = T0 + cfg.intervals.ip_check_ms;
    check.poll(at).unwrap();
    check.on_answer(at, Ok(key));
    check.poll(at).unwrap();
    check.on_answer(at, Ok(Value::String("127.0.0.1".into())));
    assert_eq!(metrics.get("watcher_public_ip_ok", &[]), Some(1.0));
    assert_eq!(ip.get(), None);
    rig.clock.set(at);
    alerts.poll(at, &client, &ip);
    let got = fake.wait_requests(2, Duration::from_secs(5));
    assert!(text(&got[1]).ends_with(&format!("\n해소: {first}")), "{}", text(&got[1]));
    // A recheck whose query-api fails is a 0 too: a new start with its why.
    let at = at + cfg.intervals.ip_check_ms;
    check.poll(at).unwrap();
    check.on_answer(
        at,
        Err(watcher::rest::RestError::NoAnswer("HTTP 502: x".into())),
    );
    assert_eq!(metrics.get("watcher_public_ip_ok", &[]), Some(0.0));
    rig.clock.set(at);
    alerts.poll(at, &client, &ip);
    let got = fake.wait_requests(3, Duration::from_secs(5));
    assert!(
        text(&got[2]).contains(
            "\n워처의 공인 IP가 키의 IP 목록에 없음(unknown: the IP recheck's query-api failed (no_answer): no answer: HTTP 502: x). "
        ),
        "{}",
        text(&got[2])
    );
    std::thread::sleep(QUIET);
    assert_eq!(fake.requests().len(), 3);
}

#[test]
fn a_confirmed_mismatch_sends_one_watch_p1() {
    // Done criterion (U19b1c): the engine rig confirms a W1 difference, the
    // runtime's P1 glue gives it to the sender, which delivers it to a
    // loopback fake Telegram.
    use common::rig::row;
    let fake = FakeHttp::start(ok());
    let rig = Rig::new(vec![Route::direct(&fake.url())]);
    let client = idle_client(rig.clock.clone());
    let ip = watcher::start::PublicIp::default();
    let mut alerts = Alerts::telegram(rig.handle.clone());
    let mut e = common::rig::Rig::new("alert-mismatch");
    e.venue.orders = vec![row("x", "v900", "BTCUSDT", "Buy")];
    let mut fed = 0;
    let mut step = |e: &mut common::rig::Rig, alerts: &mut Alerts, t: u64| {
        rig.clock.set(t);
        e.at(t);
        for (_, a) in &e.actions[fed..] {
            alerts.on_action(a);
        }
        fed = e.actions.len();
        alerts.poll(t, &client, &ip);
    };
    let t0 = common::rig::T0;
    for k in 0..3 {
        step(&mut e, &mut alerts, t0 + k * 1000);
    }
    assert_eq!(e.confirmed.len(), 1);
    let detail = &e.confirmed[0].1;
    assert!(detail.starts_with("W1 foreign v900: "), "{detail}");
    let got = fake.wait_requests(1, Duration::from_secs(5));
    assert_eq!(got.len(), 1);
    let t = text(&got[0]);
    assert!(t.starts_with("[WATCH P1] bybit w-1 "), "{t}");
    assert!(
        t.ends_with(&format!("\n{}{detail}", head("W1", 1))),
        "{t}"
    );
    // Later rounds that still see it, and the resend after an operator's
    // clear, send no further message (well past the 60 s pace).
    for k in 3..10 {
        step(&mut e, &mut alerts, t0 + k * 1000);
    }
    e.clear(t0 + 10_000);
    for k in 10..80 {
        step(&mut e, &mut alerts, t0 + k * 1000);
    }
    assert!(e.mismatches.len() >= 2, "{:?}", e.mismatches);
    std::thread::sleep(QUIET);
    assert_eq!(fake.requests().len(), 1);
}
