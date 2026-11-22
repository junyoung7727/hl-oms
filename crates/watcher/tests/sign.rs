//! The watcher's own Bybit v5 GET signing (C-WATCH part 3): the string to
//! sign, the HMAC-SHA256 and the query string. The expected values do not
//! come from the code under test: the venue's published example strings,
//! RFC 4231 test case 2, and HMAC values computed with Python's `hmac`
//! module for a made-up secret (not a key; allow-listed in
//! `tools/ci/secret_scan_allow.txt`).

#![allow(clippy::unwrap_used)]

use watcher::sign::{hmac_hex, prehash, query_string, signature};

#[test]
fn the_strings_to_sign_and_signatures_match_independent_values() {
    // The venue's GET example (gateway facts F20 Q1.9).
    assert_eq!(
        prehash(
            1658384314791,
            "XXXXXXXXXX",
            5000,
            "category=option&symbol=BTC-29JUL22-25000-C"
        ),
        "1658384314791XXXXXXXXXX5000category=option&symbol=BTC-29JUL22-25000-C"
    );
    // RFC 4231 test case 2.
    assert_eq!(
        hmac_hex("Jefe", "what do ya want for nothing?"),
        "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
    );
    // Python: hmac.new(b"watcher-made-up-secret-42", prehash, sha256).
    let secret = "watcher-made-up-secret-42";
    assert_eq!(
        signature(
            secret,
            1658384314791,
            "XXXXXXXXXX",
            5000,
            "category=option&symbol=BTC-29JUL22-25000-C"
        ),
        "cf187522bbc9e07889883ae4fae5981af1e894c584aede1a967a72c23c5d5c40"
    );
    assert_eq!(
        signature(
            secret,
            1_700_000_000_000,
            "WKEY",
            5000,
            "category=linear&settleCoin=USDT&symbol=BTCUSDT"
        ),
        "97cc2c39beda79a8c5988ad70fe71a8696a99f45ea55b8288ef0ebcefa1c8103"
    );
}

#[test]
fn query_strings_are_sorted_and_unsafe_values_refused() {
    let q = query_string(&[
        ("symbol", "BTCUSDT".into()),
        ("category", "linear".into()),
        ("settleCoin", "USDT".into()),
        ("limit", "50".into()),
    ])
    .unwrap();
    assert_eq!(q, "category=linear&limit=50&settleCoin=USDT&symbol=BTCUSDT");
    assert_eq!(query_string(&[]).unwrap(), "");
    // A cursor comes back from the venue already percent-encoded: sent as is.
    let cursor = "fb56c285-02ac%3A1668178953132%2Cfb56c285";
    assert_eq!(
        query_string(&[("cursor", cursor.into())]).unwrap(),
        format!("cursor={cursor}")
    );
    assert_eq!(
        query_string(&[("a", "A-Z_0.9~".into())]).unwrap(),
        "a=A-Z_0.9~"
    );
    for bad in [
        "a=b", "BTC USDT", "a&b", "a+b", "a/b", "a?b", "a#b", "é", "a\nb",
    ] {
        assert!(
            query_string(&[("symbol", bad.into())]).is_err(),
            "value {bad:?} must be refused"
        );
    }
    assert!(query_string(&[("sym bol", "x".into())]).is_err());
    assert!(query_string(&[("a=b", "x".into())]).is_err());
}
