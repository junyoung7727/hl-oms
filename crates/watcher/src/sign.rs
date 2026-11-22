//! The watcher's own Bybit v5 request signing (C-WATCH part 3), written
//! from the venue's documentation (`docs/contracts/gateway.md`, row "서명"),
//! not from the OMS's code. GET only: the watcher sends no POST in U19a.
//!
//! `X-BAPI-SIGN = hex(HMAC_SHA256(secret, timestamp + api_key +
//! recv_window + query))`, where the query is the string sent in the URL,
//! byte for byte.

use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;

/// Lower-case hex of HMAC-SHA256 of `message` under `secret`.
pub fn hmac_hex(secret: &str, message: &str) -> String {
    // HMAC takes a key of any length, so `new_from_slice` cannot fail.
    let mut mac = match Hmac::<Sha256>::new_from_slice(secret.as_bytes()) {
        Ok(m) => m,
        Err(_) => unreachable!("HMAC accepts keys of any length"),
    };
    mac.update(message.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

/// The string a GET signs.
pub fn prehash(timestamp_ms: u64, api_key: &str, recv_window_ms: u64, query: &str) -> String {
    format!("{timestamp_ms}{api_key}{recv_window_ms}{query}")
}

/// `X-BAPI-SIGN` of a GET.
pub fn signature(
    secret: &str,
    timestamp_ms: u64,
    api_key: &str,
    recv_window_ms: u64,
    query: &str,
) -> String {
    hmac_hex(
        secret,
        &prehash(timestamp_ms, api_key, recv_window_ms, query),
    )
}

/// A character that is sent as it is: it cannot change the meaning of the
/// query (no `=`, `&`, `+`, `#`, `?`, space or byte above ASCII).
fn plain(c: char) -> bool {
    c.is_ascii_alphanumeric() || "-_.~%:,".contains(c)
}

/// The query string of a GET: `k=v` joined by `&`, sorted by key. The same
/// string goes into the URL and the signature. Values are sent as they are
/// (a venue cursor is already percent-encoded); a key or value with another
/// character is refused, before anything is sent, instead of encoded.
pub fn query_string(params: &[(&str, String)]) -> Result<String, String> {
    let mut p: Vec<&(&str, String)> = params.iter().collect();
    p.sort_by(|a, b| a.0.cmp(b.0));
    let mut out = String::new();
    for (k, v) in p {
        if !k.chars().all(plain) || !v.chars().all(plain) {
            return Err(format!(
                "query parameter {k:?}={v:?} has a character that is not sent as is"
            ));
        }
        if !out.is_empty() {
            out.push('&');
        }
        out.push_str(k);
        out.push('=');
        out.push_str(v);
    }
    Ok(out)
}
