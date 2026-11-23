//! The watcher's own Telegram sender (C-WATCH part 10, [ADR-032]): written
//! anew from the delivery rules of draft PR #3, as `alert-direct` implements
//! them for the OMS, without linking that crate (independence, part 1).
//! U19b1a holds the pure parts: the bot token, the message and its split
//! into parts, the outbox, the outcome of an answer and the repeat rule.
//! Nothing sends yet (the sender task is U19b1b).
//!
//! The token is never written to an error, a log or `Debug`.
//!
//! [ADR-032]: ../../docs/adr/ADR-032-watcher-kill-and-alerts.md

use std::fmt;

/// The prefix of every message of the watcher (design §7.7, Q18: the
/// same bot as the OMS, told apart by the prefix).
pub const PREFIX: &str = "[WATCH P1]";
/// The venue label of the header; the watcher watches Bybit only.
pub const VENUE: &str = "bybit";
/// Telegram's limit is 4,096 UTF-16 units per message; this leaves room
/// (PR #3).
pub const MSG_LIMIT: usize = 3500;

/// The bot token and the chat of the token file (PR #3 format,
/// `{"token": "<digits>:<chars>", "chat_id": …}`; read by
/// [`crate::creds::load_token`]).
#[derive(Clone, PartialEq, Eq)]
pub struct Token {
    token: String,
    chat_id: String,
}

impl fmt::Debug for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Token {{ chat_id: {:?}, token: <hidden> }}",
            self.chat_id
        )
    }
}

/// `<digits>:<[A-Za-z0-9_-]+>`, both sides non-empty.
fn token_ok(t: &str) -> bool {
    match t.split_once(':') {
        Some((id, key)) => {
            !id.is_empty()
                && id.bytes().all(|b| b.is_ascii_digit())
                && !key.is_empty()
                && key
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        }
        None => false,
    }
}

impl Token {
    /// A token of the form `<digits>:<[A-Za-z0-9_-]+>` and a non-empty
    /// chat id. The refusal does not quote the token.
    pub fn new(token: &str, chat_id: &str) -> Result<Token, String> {
        if !token_ok(token) {
            return Err("the token is not of the form <digits>:<letters, digits, _ or ->".into());
        }
        if chat_id.is_empty() {
            return Err("chat_id must be a non-empty string or an integer".into());
        }
        Ok(Token {
            token: token.to_string(),
            chat_id: chat_id.to_string(),
        })
    }

    pub fn chat_id(&self) -> &str {
        &self.chat_id
    }
}

/// `MM-DD HH:MM:SS KST` of a Unix time in ms (seconds cut).
pub fn kst(ms: u64) -> String {
    let s = ms / 1000 + 9 * 3600;
    let days = (s / 86_400) as i64;
    let sec = s % 86_400;
    // Civil date from days since 1970-01-01 (H. Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    format!(
        "{m:02}-{d:02} {:02}:{:02}:{:02} KST",
        sec / 3600,
        sec / 60 % 60,
        sec % 60
    )
}

/// `[WATCH P1] bybit {name} {MM-DD HH:MM:SS KST}\n{body}`: `name` is the
/// watcher's config name, the time the injected clock's.
pub fn message(name: &str, now_ms: u64, body: &str) -> String {
    format!("{PREFIX} {VENUE} {name} {}\n{body}", kst(now_ms))
}

fn units(s: &str) -> usize {
    s.encode_utf16().count()
}

/// Cuts `text` at line ends so that each part is within `limit` UTF-16
/// units; one overlong line is cut by characters (PR #3 `split_message`).
/// No part is empty unless the text is.
pub fn split(text: &str, limit: usize) -> Vec<String> {
    let limit = limit.max(2);
    let mut parts = Vec::new();
    let mut cur = String::new();
    for line in text.split('\n') {
        let mut line = line;
        while units(line) > limit {
            let mut n = 0;
            let mut used = 0;
            for c in line.chars() {
                if used + c.len_utf16() > limit {
                    break;
                }
                used += c.len_utf16();
                n += c.len_utf8();
            }
            if !cur.is_empty() {
                parts.push(std::mem::take(&mut cur));
            }
            parts.push(line[..n].to_string());
            line = &line[n..];
        }
        if cur.is_empty() {
            cur = line.to_string();
        } else if units(&cur) + 1 + units(line) > limit {
            parts.push(std::mem::replace(&mut cur, line.to_string()));
        } else {
            cur.push('\n');
            cur.push_str(line);
        }
    }
    if !cur.is_empty() || parts.is_empty() {
        parts.push(cur);
    }
    parts
}
