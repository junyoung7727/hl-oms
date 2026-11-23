//! The pure parts of the watcher's own Telegram sender (C-WATCH part 10,
//! U19b1a): the message header, the split into parts of at most 3,500
//! UTF-16 units, the outbox of 50 parts, the outcome of an answer and the
//! repeat rule; the token is never printed. Nothing here sends.

#![allow(clippy::unwrap_used)]

use watcher::alert::{self, MSG_LIMIT, Token};
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
