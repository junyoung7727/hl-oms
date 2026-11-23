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

use std::collections::{BTreeMap, VecDeque};
use std::fmt;

/// The prefix of every message of the watcher (design §7.7, Q18: the
/// same bot as the OMS, told apart by the prefix).
pub const PREFIX: &str = "[WATCH P1]";
/// The venue label of the header; the watcher watches Bybit only.
pub const VENUE: &str = "bybit";
/// Telegram's limit is 4,096 UTF-16 units per message; this leaves room
/// (PR #3).
pub const MSG_LIMIT: usize = 3500;
/// Parts kept for delivery (PR #3).
pub const OUTBOX_LIMIT: usize = 50;
/// The repeat period of a lasting condition (design §7.7).
pub const REPEAT_MS: u64 = 300_000;

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

/// The outcome of one send to Telegram.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Delivered,
    /// Telegram refused this message for good (400, 413: it is about this
    /// message): drop the part and count it.
    Refused(String),
    /// Anything else, and a network error: keep the part and end this
    /// flush.
    Retry(String),
}

/// The outcome of Telegram's answer `status` with `body` (PR #3):
/// `Delivered` for a 2xx answer whose body is JSON with `"ok": true`,
/// `Refused` for 400 and 413, `Retry` for everything else. The text names
/// the status and Telegram's `description` in one line of at most 200
/// characters; it never holds the request, so never the token.
pub fn outcome(status: u16, body: &[u8]) -> Outcome {
    let data = serde_json::from_slice::<serde_json::Value>(body).unwrap_or_default();
    if (200..300).contains(&status) && data["ok"].as_bool() == Some(true) {
        return Outcome::Delivered;
    }
    let description: String = data["description"]
        .as_str()
        .unwrap_or("")
        .chars()
        .take(200)
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let text = format!("telegram answered {status}: {description}");
    match status {
        400 | 413 => Outcome::Refused(text),
        _ => Outcome::Retry(text),
    }
}

/// One part handed out for sending; `id = None` is the drop notice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Part {
    pub id: Option<u64>,
    pub text: String,
    /// The drops a notice reports.
    pub count: u64,
}

/// Parts not yet delivered (PR #3 `Outbox`). Overflow drops the oldest
/// and counts it; once the queue is empty, one notice reports the count.
#[derive(Debug)]
pub struct Outbox {
    queue: VecDeque<(u64, String)>,
    next_id: u64,
    limit: usize,
    dropped: u64,
    dropped_total: u64,
}

impl Outbox {
    pub fn new(limit: usize) -> Outbox {
        Outbox {
            queue: VecDeque::new(),
            next_id: 0,
            limit: limit.max(1),
            dropped: 0,
            dropped_total: 0,
        }
    }

    /// Adds a message, split into parts of at most [`MSG_LIMIT`] units.
    pub fn add(&mut self, msg: &str) {
        for p in split(msg, MSG_LIMIT) {
            self.queue.push_back((self.next_id, p));
            self.next_id += 1;
        }
        while self.queue.len() > self.limit {
            self.queue.pop_front();
            self.count_drop();
        }
    }

    fn count_drop(&mut self) {
        self.dropped += 1;
        self.dropped_total += 1;
    }

    pub fn len(&self) -> usize {
        self.queue.len()
    }

    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    /// Drops not yet reported by a notice.
    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    pub fn dropped_total(&self) -> u64 {
        self.dropped_total
    }

    /// Nothing to send: no part and no notice owed.
    pub fn settled(&self) -> bool {
        self.queue.is_empty() && self.dropped == 0
    }

    /// The next part to send, or the drop notice
    /// `[WATCH P1] 전송 실패로 알림 N개를 버림` when the queue is empty.
    pub fn next(&self) -> Option<Part> {
        match self.queue.front() {
            Some((id, t)) => Some(Part {
                id: Some(*id),
                text: t.clone(),
                count: 0,
            }),
            None if self.dropped > 0 => Some(Part {
                id: None,
                text: format!("{PREFIX} 전송 실패로 알림 {}개를 버림", self.dropped),
                count: self.dropped,
            }),
            None => None,
        }
    }

    /// Applies the outcome of sending `part`; `false` ends the flush. A
    /// part that overflow dropped while it was being sent is not removed
    /// twice nor counted twice.
    pub fn done(&mut self, part: &Part, outcome: &Outcome) -> bool {
        let front = part.id.is_some() && self.queue.front().map(|(id, _)| *id) == part.id;
        match (outcome, part.id) {
            (Outcome::Retry(_), _) => false,
            (Outcome::Delivered, Some(_)) => {
                if front {
                    self.queue.pop_front();
                }
                true
            }
            (Outcome::Refused(_), Some(_)) => {
                if front {
                    self.queue.pop_front();
                    self.count_drop();
                }
                true
            }
            // The notice: delivered or refused, its count is told or
            // cannot be; drops after it was handed out stay owed.
            (_, None) => {
                self.dropped = self.dropped.saturating_sub(part.count);
                true
            }
        }
    }
}

#[derive(Debug, Clone)]
struct Cond {
    first_text: String,
    last_sent_ms: u64,
}

/// The repeat rule of design §7.7: once at start, every `period_ms` while
/// the condition lasts (with the latest text), once at clear
/// (`해소: <start text>`). In memory only: a restart sends a lasting
/// condition's start text once more. Elapsed time is `saturating_sub`, so
/// a clock that steps back holds the repeat.
#[derive(Debug)]
pub struct Repeat {
    period_ms: u64,
    conds: BTreeMap<String, Cond>,
}

impl Repeat {
    pub fn new(period_ms: u64) -> Repeat {
        Repeat {
            period_ms: period_ms.max(1),
            conds: BTreeMap::new(),
        }
    }

    /// The condition `key` holds (`Some(text)`) or not (`None`) at
    /// `now_ms`; the body to send now, if any.
    pub fn set(&mut self, key: &str, text: Option<&str>, now_ms: u64) -> Option<String> {
        match (text, self.conds.get_mut(key)) {
            (Some(t), None) => {
                self.conds.insert(
                    key.to_string(),
                    Cond {
                        first_text: t.to_string(),
                        last_sent_ms: now_ms,
                    },
                );
                Some(t.to_string())
            }
            (Some(t), Some(c)) => {
                if now_ms.saturating_sub(c.last_sent_ms) >= self.period_ms {
                    c.last_sent_ms = now_ms;
                    Some(t.to_string())
                } else {
                    None
                }
            }
            (None, Some(_)) => self
                .conds
                .remove(key)
                .map(|c| format!("해소: {}", c.first_text)),
            (None, None) => None,
        }
    }
}
