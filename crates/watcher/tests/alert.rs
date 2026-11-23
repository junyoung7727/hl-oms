//! The pure parts of the watcher's own Telegram sender (C-WATCH part 10,
//! U19b1a): the message header, the split into parts of at most 3,500
//! UTF-16 units, the outbox of 50 parts, the outcome of an answer and the
//! repeat rule; the token is never printed. Nothing here sends.

#![allow(clippy::unwrap_used)]

use watcher::alert::Token;
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
