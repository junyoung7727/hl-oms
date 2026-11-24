//! The watcher's own Bybit v5 signing (C-WATCH part 3): the strings a GET
//! and the one POST sign, the HMAC-SHA256 and the query string. The expected values do not
//! come from the code under test: the venue's published example strings,
//! RFC 4231 test case 2, and HMAC values computed with Python's `hmac`
//! module for a made-up secret (not a key; allow-listed in
//! `tools/ci/secret_scan_allow.txt`).

#![allow(clippy::unwrap_used)]

mod common;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use common::rsa;
use watcher::sign::{
    Signer, hmac_hex, post_prehash, post_signature, prehash, query_string, signature,
};

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

#[test]
fn the_post_string_to_sign_matches_the_published_example_and_independent_values() {
    // The venue's POST example, copied exactly (gateway facts F20 Q1.10):
    // the body is signed as the bytes given, spaces included.
    assert_eq!(
        post_prehash(1658385579423, "XXXXXXXXXX", 5000, r#"{"category": "option"}"#),
        r#"1658385579423XXXXXXXXXX5000{"category": "option"}"#
    );
    // Python: hmac.new(b"watcher-made-up-secret-42", prehash, sha256).
    let secret = "watcher-made-up-secret-42";
    assert_eq!(
        post_signature(
            secret,
            1658385579423,
            "XXXXXXXXXX",
            5000,
            r#"{"category": "option"}"#
        ),
        "5e1bc32d2043b15cdf856e42986af413e272178c5e33408686b16f44f5333596"
    );
    // The watcher's one POST body (`rest::CANCEL_ALL_BODY`).
    assert_eq!(
        post_signature(
            secret,
            1_700_000_000_000,
            "WKEY",
            5000,
            r#"{"category":"linear","settleCoin":"USDT"}"#
        ),
        "df293dce1c898148cb8c548eaa8d9d5180068daaa25acc3b6b028df594ab0f96"
    );
    // A body with other bytes (a re-serialised one with spaces) signs to
    // another value: the signature is over the bytes, not the meaning.
    assert_ne!(
        post_signature(
            secret,
            1_700_000_000_000,
            "WKEY",
            5000,
            r#"{"category": "linear", "settleCoin": "USDT"}"#
        ),
        "df293dce1c898148cb8c548eaa8d9d5180068daaa25acc3b6b028df594ab0f96"
    );
}

/// RSA (ADR-034, spec T2): RSA-SHA256 PKCS#1 v1.5 over the same string to
/// sign, standard base64 with padding (F1). PKCS#1 v1.5 is deterministic,
/// so openssl's value for the test-only key is the exact expected value:
/// `printf '%s' "$prehash" | openssl dgst -sha256 -sign key.pem | base64 -w0`.
/// The signature is also checked with ring's public key of the key, parsed
/// here from the PEM body (not through the code under test). The HMAC
/// signer gives the lower-case hex of `hmac_hex` (the Python value above).
#[test]
fn an_rsa_signature_is_openssls_base64_and_verifies_with_the_public_key() {
    let pre = prehash(
        1658384314791,
        "XXXXXXXXXX",
        5000,
        "category=option&symbol=BTC-29JUL22-25000-C",
    );
    let signer = Signer::rsa(&rsa::pem_2048()).unwrap();
    assert!(signer.is_rsa());
    let got = signer.sign(&pre).unwrap();
    assert_eq!(
        got,
        "kGwAVU7pufiKNgdwHDO+kJtPDvnBTOgIv4UsxfTpGa0f2PX8CWcUZSwp9+AVg5juKY+M7pZyRoSHx7Hoad0EJ2za2jyBZOvHw3OjB15AL3lDKsUVbFZX0Mv+7eox7X6rLRoy0TYLohV6Qg2NxaTGCYwWX+H25kZNjBTMUhV7tP07BXgeTR9ZJtiZ+rbOogQMC/UTXKmp7CaGlPIXvqpYqwViTPShBF1BTnZECBeFPFMV5iLXIRlYgzeCt5KtJlWjFeIwbxYI0wv7ntm+JpGpKmvTnXipBye5/S53ZzEjkFuYVHig7KKk6zWMzSUH1vV5oi4g18penJP66TzJuMSotQ=="
    );
    // Deterministic: the same string signs to the same value again.
    assert_eq!(signer.sign(&pre).unwrap(), got);

    use ring::signature::{KeyPair, RSA_PKCS1_2048_8192_SHA256, RsaKeyPair, UnparsedPublicKey};
    let der = STANDARD.decode(rsa::KEY_2048.replace('\n', "")).unwrap();
    let pair = RsaKeyPair::from_pkcs8(&der).unwrap();
    let public = UnparsedPublicKey::new(&RSA_PKCS1_2048_8192_SHA256, pair.public_key().as_ref());
    let sig = STANDARD.decode(&got).unwrap();
    assert_eq!(sig.len(), 256);
    assert!(public.verify(pre.as_bytes(), &sig).is_ok());
    assert!(public.verify(format!("{pre}x").as_bytes(), &sig).is_err());

    let hmac = Signer::hmac("watcher-made-up-secret-42");
    assert!(!hmac.is_rsa());
    assert_eq!(
        hmac.sign(&pre).unwrap(),
        "cf187522bbc9e07889883ae4fae5981af1e894c584aede1a967a72c23c5d5c40"
    );
}
