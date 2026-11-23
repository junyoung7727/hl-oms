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

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use http_fake::{Answer, FakeHttp, Request, dead_addr};
use serde_json::Value;
use watcher::alert::{self, Handle, MSG_LIMIT, OUTBOX_LIMIT, Outbox, Outcome, Part, Route, Token};
use watcher::clock::{Clock, ManualClock};
use watcher::config::Config;
use watcher::metrics::Metrics;

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
        let end = Instant::now() + Duration::from_secs(5);
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
