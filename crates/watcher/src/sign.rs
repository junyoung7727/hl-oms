//! The watcher's own Bybit v5 request signing (C-WATCH part 3), written
//! from the venue's documentation (`docs/contracts/gateway.md`, row "서명"),
//! not from the OMS's code. GETs, and from U19b2a the one signed POST,
//! the cancel-all of the watcher's kill loop (`rest::Client::cancel_all`).
//!
//! `X-BAPI-SIGN = hex(HMAC_SHA256(secret, timestamp + api_key +
//! recv_window + query))`, where the query is the string sent in the URL,
//! byte for byte; for a POST the body sent takes the place of the query,
//! byte for byte (gateway facts Q1.1, Q1.15).
//!
//! With an RSA key (ADR-034, measured on Bybit testnet 2026-11-25, spec
//! F1): `X-BAPI-SIGN = base64(RSA_SHA256 PKCS#1 v1.5 signature over the
//! same string)`, standard base64 with padding (Bybit v5 "Create A
//! Request": "lowercase HEX string for HMAC_SHA256, or base64 for
//! RSA_SHA256"). [`Signer`] is the one place that signs.

use base64::Engine;
use hmac::{Hmac, KeyInit, Mac};
use ring::signature::{RSA_PKCS1_SHA256, RsaKeyPair};
use sha2::Sha256;

/// How to fix a key file that is not unencrypted PKCS#8 (spec D2).
const PKCS8_FIX: &str = "openssl pkcs8 -topk8 -nocrypt -in old.pem -out new.pem";

/// The signing key of a credentials file: an HMAC secret or a parsed RSA
/// key pair. `Debug` is not implemented: nothing prints the key.
pub struct Signer(Key);

enum Key {
    Hmac(String),
    Rsa(Box<RsaKeyPair>),
}

impl Signer {
    /// An HMAC-SHA256 secret (`api_secret`).
    pub fn hmac(secret: &str) -> Signer {
        Signer(Key::Hmac(secret.to_string()))
    }

    /// An RSA key (`rsa_private_key`): the text of an unencrypted PKCS#8
    /// PEM (`BEGIN PRIVATE KEY`), a modulus of 2048 bits or more (ring
    /// refuses less). Parsed once, here. No error quotes the text.
    pub fn rsa(pem: &str) -> Result<Signer, String> {
        if pem.contains("BEGIN RSA PRIVATE KEY") {
            return Err(format!(
                "rsa_private_key is a PKCS#1 key (BEGIN RSA PRIVATE KEY); convert it to PKCS#8 with: {PKCS8_FIX}"
            ));
        }
        if pem.contains("BEGIN ENCRYPTED PRIVATE KEY") {
            return Err(format!(
                "rsa_private_key is encrypted (BEGIN ENCRYPTED PRIVATE KEY); write it unencrypted with: {PKCS8_FIX}"
            ));
        }
        let not_pem = || {
            "rsa_private_key is not a PEM private key (BEGIN PRIVATE KEY, PKCS#8)".to_string()
        };
        let begin = format!("-----{}-----", "BEGIN PRIVATE KEY");
        let end = format!("-----{}-----", "END PRIVATE KEY");
        let body = pem
            .trim()
            .strip_prefix(&begin)
            .and_then(|rest| rest.strip_suffix(&end))
            .ok_or_else(not_pem)?;
        let body: String = body.chars().filter(|c| !c.is_ascii_whitespace()).collect();
        let der = base64::engine::general_purpose::STANDARD
            .decode(body)
            .map_err(|_| not_pem())?;
        // ring's reason is a fixed word (e.g. "TooSmall"), never key bytes.
        let pair = RsaKeyPair::from_pkcs8(&der).map_err(|e| {
            format!("rsa_private_key is not an RSA key of 2048 bits or more that ring accepts ({e})")
        })?;
        Ok(Signer(Key::Rsa(Box::new(pair))))
    }

    /// Whether this is an RSA key.
    pub fn is_rsa(&self) -> bool {
        matches!(self.0, Key::Rsa(_))
    }

    /// `X-BAPI-SIGN` of `prehash`: lower-case hex of the HMAC, or standard
    /// base64 of the RSA signature. `Err` only when ring cannot sign with
    /// a parsed key (not expected; the caller sends nothing, spec D7).
    pub fn sign(&self, prehash: &str) -> Result<String, String> {
        match &self.0 {
            Key::Hmac(secret) => Ok(hmac_hex(secret, prehash)),
            Key::Rsa(pair) => {
                let mut sig = vec![0; pair.public().modulus_len()];
                pair.sign(
                    &RSA_PKCS1_SHA256,
                    &ring::rand::SystemRandom::new(),
                    prehash.as_bytes(),
                    &mut sig,
                )
                .map_err(|_| "the RSA signature failed".to_string())?;
                Ok(base64::engine::general_purpose::STANDARD.encode(sig))
            }
        }
    }
}

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

/// `X-BAPI-SIGN` of a GET with an HMAC secret. The client signs through
/// [`Signer::sign`]; this keeps the HMAC test vectors.
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

/// The string a POST signs: the body is the exact bytes sent.
pub fn post_prehash(timestamp_ms: u64, api_key: &str, recv_window_ms: u64, body: &str) -> String {
    format!("{timestamp_ms}{api_key}{recv_window_ms}{body}")
}

/// `X-BAPI-SIGN` of a POST of `body` with an HMAC secret (the test
/// vectors; the client signs through [`Signer::sign`]).
pub fn post_signature(
    secret: &str,
    timestamp_ms: u64,
    api_key: &str,
    recv_window_ms: u64,
    body: &str,
) -> String {
    hmac_hex(
        secret,
        &post_prehash(timestamp_ms, api_key, recv_window_ms, body),
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
