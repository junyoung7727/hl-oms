//! The watcher's own Telegram sender (C-WATCH part 10, [ADR-032]): written
//! anew from the delivery rules of draft PR #3, as `alert-direct` implements
//! them for the OMS, without linking that crate (independence, part 1).
//! U19b1a holds the pure parts: the bot token, and nothing sends yet.
//!
//! The token is never written to an error, a log or `Debug`.
//!
//! [ADR-032]: ../../docs/adr/ADR-032-watcher-kill-and-alerts.md

use std::fmt;

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
