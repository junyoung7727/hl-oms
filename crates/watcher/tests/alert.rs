//! The pure parts of the watcher's own Telegram sender (C-WATCH part 10,
//! U19b1a): the message header, the split into parts of at most 3,500
//! UTF-16 units, the outbox of 50 parts, the outcome of an answer and the
//! repeat rule; the token is never printed. Nothing here sends.

#![allow(clippy::unwrap_used)]

use watcher::alert::{self, MSG_LIMIT, OUTBOX_LIMIT, Outbox, Outcome, Part, Token};
use watcher::config::Config;

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
    let text = vec![line.as_str(); 7].join("\n");
    let parts = split(&text);
    assert_eq!(parts.len(), 3);
    assert_eq!(parts[0], vec![line.as_str(); 3].join("\n"));
    assert_eq!(parts[2], line);
    assert_eq!(parts.join("\n"), text);
    // Exactly 3,500 units is one part; one more cuts at the line end.
    let exact = format!("{}\n{}", "a".repeat(1749), "b".repeat(1750));
    assert_eq!(units(&exact), 3500);
    assert_eq!(split(&exact), [exact.clone()]);
    let over = format!("{exact}b");
    assert_eq!(split(&over), ["a".repeat(1749), "b".repeat(1751)]);
    // UTF-16 units, not bytes or characters: a Hangul syllable is one
    // unit (three bytes), an emoji two (one character).
    let hangul = "가".repeat(3500);
    assert_eq!(split(&hangul), [hangul.clone()]);
    let parts = split(&format!("{hangul}가"));
    assert_eq!(parts, [hangul.clone(), "가".to_string()]);
    let emoji = "😀".repeat(1751);
    let parts = split(&emoji);
    assert_eq!(parts, ["😀".repeat(1750), "😀".to_string()]);
    assert!(parts.iter().all(|p| units(p) <= 3500));
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
