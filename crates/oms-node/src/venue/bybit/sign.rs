//! Bybit v5 request signing (ADR-028).
//!
//! REST: `hex(HMAC_SHA256(secret, timestamp + api_key + recv_window +
//! payload))`, where the payload is the query string of a GET or the body
//! of a POST, byte for byte as sent. Private stream: the same HMAC over
//! `"GET/realtime" + expires`.

use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;

fn hmac_hex(secret: &str, message: &str) -> String {
    // HMAC takes a key of any length; `new_from_slice` cannot fail for it.
    let mut mac = match Hmac::<Sha256>::new_from_slice(secret.as_bytes()) {
        Ok(m) => m,
        Err(_) => unreachable!("HMAC accepts keys of any length"),
    };
    mac.update(message.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

/// The string a REST request signs.
pub fn rest_prehash(
    timestamp_ms: u64,
    api_key: &str,
    recv_window_ms: u64,
    payload: &str,
) -> String {
    format!("{timestamp_ms}{api_key}{recv_window_ms}{payload}")
}

/// `X-BAPI-SIGN` of a REST request.
pub fn rest_signature(
    secret: &str,
    timestamp_ms: u64,
    api_key: &str,
    recv_window_ms: u64,
    payload: &str,
) -> String {
    hmac_hex(
        secret,
        &rest_prehash(timestamp_ms, api_key, recv_window_ms, payload),
    )
}

/// The signature of the private stream's `auth` message.
pub fn ws_auth_signature(secret: &str, expires_ms: u64) -> String {
    hmac_hex(secret, &format!("GET/realtime{expires_ms}"))
}

/// The query string of a GET: `k=v` joined by `&`, sorted by key. The same
/// string goes into the URL and the signature. Values are sent as they are,
/// as the venue's SDK does (F20 Q1.13): a cursor comes back from the venue
/// already percent-encoded. A value with a character that would change the
/// query's meaning is refused instead of encoded.
pub fn query_string(params: &[(&str, String)]) -> Result<String, String> {
    let mut p: Vec<&(&str, String)> = params.iter().collect();
    p.sort_by(|a, b| a.0.cmp(b.0));
    for (k, v) in &p {
        let ok = |c: char| c.is_ascii_alphanumeric() || "-_.~%:,".contains(c);
        if !k.chars().all(ok) || !v.chars().all(ok) {
            return Err(format!(
                "query parameter {k}={v:?} has a character that is not sent as is"
            ));
        }
    }
    Ok(p.iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("&"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The venue's published examples (F20 Q1.9, Q1.10): the strings to
    /// sign. The docs do not publish the secret, so the signatures cannot be
    /// checked against them (F20 Q1.11).
    #[test]
    fn the_strings_to_sign_are_the_venues_examples() {
        assert_eq!(
            rest_prehash(
                1658384314791,
                "XXXXXXXXXX",
                5000,
                "category=option&symbol=BTC-29JUL22-25000-C"
            ),
            "1658384314791XXXXXXXXXX5000category=option&symbol=BTC-29JUL22-25000-C"
        );
        assert_eq!(
            rest_prehash(
                1658385579423,
                "XXXXXXXXXX",
                5000,
                r#"{"category": "option"}"#
            ),
            r#"1658385579423XXXXXXXXXX5000{"category": "option"}"#
        );
        assert_eq!(
            query_string(&[
                ("symbol", "BTC-29JUL22-25000-C".into()),
                ("category", "option".into())
            ])
            .unwrap(),
            "category=option&symbol=BTC-29JUL22-25000-C"
        );
    }

    /// RFC 4231 test case 2, then the venue's strings with a fixed secret,
    /// computed independently with Python's `hmac` module.
    #[test]
    fn signatures_match_independent_hmac_values() {
        assert_eq!(
            hmac_hex("Jefe", "what do ya want for nothing?"),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
        let secret = "test-secret-0123456789";
        assert_eq!(
            rest_signature(
                secret,
                1658384314791,
                "XXXXXXXXXX",
                5000,
                "category=option&symbol=BTC-29JUL22-25000-C"
            ),
            "44cd56dd7f586d4c0f7374b4ea02c8605d31f5c1eb48ef865cfd2236378f1678"
        );
        assert_eq!(
            rest_signature(
                secret,
                1658385579423,
                "XXXXXXXXXX",
                5000,
                r#"{"category": "option"}"#
            ),
            "df5050fe26da2a69f633ffd909968d7a5234623cf215c08229a1e78d27aa5528"
        );
        assert_eq!(
            ws_auth_signature(secret, 1662350400000),
            "15fb3c95518f3b90ae93f4bc1e31adb7e167feb1286f8349d259270f69661eb9"
        );
    }

    #[test]
    fn a_venue_cursor_is_sent_as_is_and_other_characters_are_refused() {
        let cursor = "fb56c285-02ac%3A1668178953132%2Cfb56c285";
        assert_eq!(
            query_string(&[("cursor", cursor.into())]).unwrap(),
            format!("cursor={cursor}")
        );
        assert!(query_string(&[("cursor", "a=b".into())]).is_err());
        assert!(query_string(&[("symbol", "BTC USDT".into())]).is_err());
        assert!(query_string(&[("symbol", "a&b".into())]).is_err());
    }
}
