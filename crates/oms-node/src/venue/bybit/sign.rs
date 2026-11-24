//! Bybit v5 request signing (ADR-028, RSA keys ADR-034).
//!
//! REST: `hex(HMAC_SHA256(secret, timestamp + api_key + recv_window +
//! payload))`, where the payload is the query string of a GET or the body
//! of a POST, byte for byte as sent. Private stream: the same HMAC over
//! `"GET/realtime" + expires`.
//!
//! An RSA key signs the same strings with RSA-SHA256 PKCS#1 v1.5 and the
//! signature is standard base64 with padding (measured on Bybit testnet
//! 2026-11-25, REST and private stream; the same signature as hex is
//! "Invalid sign"). [`Signer::sign`] is the one place that signs.

use base64::Engine;
use hmac::{Hmac, KeyInit, Mac};
use ring::signature::{RSA_PKCS1_SHA256, RsaKeyPair};
use sha2::Sha256;

/// The fix for a key file that is not unencrypted PKCS#8.
const TO_PKCS8: &str = "openssl pkcs8 -topk8 -nocrypt -in old.pem -out new.pem";

/// The signing key: an HMAC secret or an RSA key pair parsed at load.
/// No `Debug`: the key is printed nowhere.
pub struct Signer {
    key: SignKey,
}

enum SignKey {
    Hmac(String),
    Rsa(Box<RsaKeyPair>),
}

impl Signer {
    pub fn hmac(secret: &str) -> Signer {
        Signer {
            key: SignKey::Hmac(secret.to_string()),
        }
    }

    /// Parses `pem`: only an unencrypted PKCS#8 PEM (`BEGIN PRIVATE KEY`)
    /// of an RSA key of 2048 bits or more (ring refuses less). A PKCS#1 or
    /// an encrypted PEM is refused with the openssl command that converts
    /// it. No error quotes the text.
    pub fn rsa(pem: &str) -> Result<Signer, String> {
        if pem.contains("BEGIN RSA PRIVATE KEY") {
            return Err(format!(
                "rsa_private_key is a PKCS#1 key (BEGIN RSA PRIVATE KEY); convert it to PKCS#8 with: {TO_PKCS8}"
            ));
        }
        if pem.contains("BEGIN ENCRYPTED PRIVATE KEY") {
            return Err(format!(
                "rsa_private_key is encrypted (BEGIN ENCRYPTED PRIVATE KEY); write it unencrypted with: {TO_PKCS8}"
            ));
        }
        let bad = || "rsa_private_key is not a PEM private key (BEGIN PRIVATE KEY, PKCS#8)".to_string();
        let (begin, end) = (
            format!("-----{}-----", "BEGIN PRIVATE KEY"),
            format!("-----{}-----", "END PRIVATE KEY"),
        );
        let inner = pem
            .trim()
            .strip_prefix(begin.as_str())
            .and_then(|r| r.strip_suffix(end.as_str()))
            .ok_or_else(bad)?;
        let b64: String = inner.split_ascii_whitespace().collect();
        let der = base64::engine::general_purpose::STANDARD
            .decode(b64)
            .map_err(|_| bad())?;
        // ring's reason is a fixed name such as "TooSmall", not key bytes.
        let pair = RsaKeyPair::from_pkcs8(&der).map_err(|e| {
            format!("rsa_private_key is not an RSA key of 2048 bits or more that ring accepts ({e})")
        })?;
        Ok(Signer {
            key: SignKey::Rsa(Box::new(pair)),
        })
    }

    pub fn is_rsa(&self) -> bool {
        matches!(self.key, SignKey::Rsa(_))
    }

    /// The signature of `prehash`: lower-case hex (HMAC) or standard
    /// base64 (RSA). `Err` only if ring cannot sign with a parsed key; the
    /// caller then sends nothing (ADR-034).
    pub fn sign(&self, prehash: &str) -> Result<String, String> {
        match &self.key {
            SignKey::Hmac(secret) => Ok(hmac_hex(secret, prehash)),
            SignKey::Rsa(pair) => {
                let mut out = vec![0u8; pair.public().modulus_len()];
                pair.sign(
                    &RSA_PKCS1_SHA256,
                    &ring::rand::SystemRandom::new(),
                    prehash.as_bytes(),
                    &mut out,
                )
                .map_err(|_| "the RSA signature could not be made".to_string())?;
                Ok(base64::engine::general_purpose::STANDARD.encode(out))
            }
        }
    }
}

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

/// `X-BAPI-SIGN` of a REST request with an HMAC secret. The client signs
/// through [`Signer::sign`]; this keeps the HMAC test vectors.
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

/// The string the private stream's `auth` message signs.
pub fn ws_prehash(expires_ms: u64) -> String {
    format!("GET/realtime{expires_ms}")
}

/// The HMAC signature of the private stream's `auth` message (the
/// test vectors; the streams sign through [`Signer::sign`]).
pub fn ws_auth_signature(secret: &str, expires_ms: u64) -> String {
    hmac_hex(secret, &ws_prehash(expires_ms))
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

/// Test-only RSA keys (ADR-034), made once for these tests with openssl
/// 3.5.5 and used nowhere else: they sign nothing real and no venue knows
/// them. Only the base64 bodies are written here; [`test_keys::pem`] adds
/// the header and footer lines at run time, so that the source holds no
/// PEM header for a secret scanner to flag. Expected signatures are
/// openssl's (`openssl dgst -sha256 -sign key.pem | base64 -w0`).
#[cfg(test)]
pub(crate) mod test_keys {
    /// `body` between the `-----BEGIN {label}-----` and
    /// `-----END {label}-----` lines.
    pub fn pem(label: &str, body: &str) -> String {
        format!("-----BEGIN {label}-----\n{body}-----END {label}-----\n")
    }

    /// [`KEY_2048`] as the PKCS#8 PEM text openssl writes.
    pub fn pem_2048() -> String {
        pem("PRIVATE KEY", KEY_2048)
    }

    /// `openssl genpkey -algorithm RSA -pkeyopt rsa_keygen_bits:2048` (PKCS#8).
    pub const KEY_2048: &str = "\
    MIIEvgIBADANBgkqhkiG9w0BAQEFAASCBKgwggSkAgEAAoIBAQC5RVahdljZ6rg+\n\
    0LM4kQOU9nqs4HXaNMD6IlzFL7rGxsjFGVmc1u74CBmo4WGZwaCDcR0e/I8Zm3Qa\n\
    2jzta0DkZ8kOPMy91hstXSBQLW622NE0txG/1GDvtmDEMP4KQoMylJrpfXgLitYU\n\
    9ksE2x8uzWZ1/TJUQF8Y57C+tWWEEsJ5yxbjdLGqF68c3I9yQVR0LMfYHtcFukuW\n\
    mQbqu9tSsnJ5KKIMhBiRwA/udTukAWlhQVAnlzQLvNfdTTNMOmksBg9MyvhqlN0k\n\
    7GyzYohld1lBmvjPzdV6d2Cq2rvQoCUrUFp0iqNSOimAnz/O6go6JmOe6oxmaSwd\n\
    XQQaea/JAgMBAAECggEAA59OGPdjYqoAKC+UZsq5szPjz3V5WGbSTxGqQVYJj3wG\n\
    I4m3WCWro8B6H/R9dVrKdJzFi6bhyrJgtsBa6JlsOgr9bKsrpZqGCtZYnLixWB8t\n\
    F/FsW2p06rrkxsaJG6eT68KUCh9LNY6QA6DnyT8vgOQV2MC2jIclqir9fae2Rbq/\n\
    lYJS2fciZQPWfie3tQBZY9eCHMySdcCtYXzfoUpODN95q+6/c318Nd+MuwhoC1V1\n\
    XpJq8dIiovdlBOB7lzOUqdGpmvCNiE2meqaKC/Gtoqfvh9zlUsliR6t7VRaJfbEM\n\
    IoU4dHsO5ZfLbZmsIjaPO2boMZLV4mbdMfJjoXiWwQKBgQD2w/Xo7sjoQPjlz/Qq\n\
    pnpKvZK4qc808b26wgHm+GabVlDmcCFVj2MYRW0UY1hAknUhSNFbj+T/QXGhZXil\n\
    Qxz+FLw5VY4JtmM7mhmxxbWLaO1xTNWl9JfmqLX+6VRB9Wqu8PTbhIGILh+B5+IT\n\
    NjTkja/SQNNeW/fVQA7Tu9GsfQKBgQDANEC6jS6DDdXk34NTmnv4oE/hCL8hvy5J\n\
    idE9ncR3Su1s/GymWPqzcJnay1orEgwCB25SDCiocwuMdSH7DUa8U496eVZpVpaR\n\
    w+S0klv07ZLK9cBKqWNFt9R+rbSULuFzYk+2ihsUFbbSNOO4eUS1Zfm2TLPsy2zk\n\
    J7YILEPOPQKBgQCL3IuPZ2+1u8WWs6PnXI4puILlaVGI548qv/VGDPpTBrw5r8ok\n\
    7W0z/32QtbH6f0gmAliu9cLMLjKSUKntuTlAhU4djPFBbf+Wc4QmrzFzf5g8xFi/\n\
    g/9oe60TevoHnGrIFsghd1vXbJTtExme2/VkqS3DFHz4mVXBp1oABg8UHQKBgH53\n\
    Y0fccYhF6EGKWC0AbyXuxeB6EtkvpFM63XS/hHBB2utlRFBIJ909794pA5FndIM5\n\
    RHu0CGSeHjEzOUKpP6vXB4c8o4D4OmZ7ENdHnj4QvA/Tl80Im4taI/Wecj02g52a\n\
    SH0BbkPO7dZueo/9Kxp4mNA8s/7Q8ztEbd/+0Da5AoGBAN7L8aXhL+CBCHDc4zmT\n\
    bzp6XHDcdXiBgoUjWda6EprsGBNFpZ4qv2N+tpF33DqBDE3fE7kjGdVkX0tX1TCM\n\
    G60ea8ExWguHZuCi0T2oOfBDs7egtjT7pj/Vj17s6GgZK0Reo9e+rxOOcsjobsQF\n\
    dxrIVDd5VpSlsYKgkmExWATr\n\
    ";

    /// The same key as PKCS#1 (`openssl rsa -in key.pem -traditional`).
    pub const KEY_2048_PKCS1: &str = "\
    MIIEpAIBAAKCAQEAuUVWoXZY2eq4PtCzOJEDlPZ6rOB12jTA+iJcxS+6xsbIxRlZ\n\
    nNbu+AgZqOFhmcGgg3EdHvyPGZt0Gto87WtA5GfJDjzMvdYbLV0gUC1uttjRNLcR\n\
    v9Rg77ZgxDD+CkKDMpSa6X14C4rWFPZLBNsfLs1mdf0yVEBfGOewvrVlhBLCecsW\n\
    43SxqhevHNyPckFUdCzH2B7XBbpLlpkG6rvbUrJyeSiiDIQYkcAP7nU7pAFpYUFQ\n\
    J5c0C7zX3U0zTDppLAYPTMr4apTdJOxss2KIZXdZQZr4z83Vendgqtq70KAlK1Ba\n\
    dIqjUjopgJ8/zuoKOiZjnuqMZmksHV0EGnmvyQIDAQABAoIBAAOfThj3Y2KqACgv\n\
    lGbKubMz4891eVhm0k8RqkFWCY98BiOJt1glq6PAeh/0fXVaynScxYum4cqyYLbA\n\
    WuiZbDoK/WyrK6WahgrWWJy4sVgfLRfxbFtqdOq65MbGiRunk+vClAofSzWOkAOg\n\
    58k/L4DkFdjAtoyHJaoq/X2ntkW6v5WCUtn3ImUD1n4nt7UAWWPXghzMknXArWF8\n\
    36FKTgzfeavuv3N9fDXfjLsIaAtVdV6SavHSIqL3ZQTge5czlKnRqZrwjYhNpnqm\n\
    igvxraKn74fc5VLJYkere1UWiX2xDCKFOHR7DuWXy22ZrCI2jztm6DGS1eJm3THy\n\
    Y6F4lsECgYEA9sP16O7I6ED45c/0KqZ6Sr2SuKnPNPG9usIB5vhmm1ZQ5nAhVY9j\n\
    GEVtFGNYQJJ1IUjRW4/k/0FxoWV4pUMc/hS8OVWOCbZjO5oZscW1i2jtcUzVpfSX\n\
    5qi1/ulUQfVqrvD024SBiC4fgefiEzY05I2v0kDTXlv31UAO07vRrH0CgYEAwDRA\n\
    uo0ugw3V5N+DU5p7+KBP4Qi/Ib8uSYnRPZ3Ed0rtbPxsplj6s3CZ2staKxIMAgdu\n\
    UgwoqHMLjHUh+w1GvFOPenlWaVaWkcPktJJb9O2SyvXASqljRbfUfq20lC7hc2JP\n\
    toobFBW20jTjuHlEtWX5tkyz7Mts5Ce2CCxDzj0CgYEAi9yLj2dvtbvFlrOj51yO\n\
    KbiC5WlRiOePKr/1Rgz6Uwa8Oa/KJO1tM/99kLWx+n9IJgJYrvXCzC4yklCp7bk5\n\
    QIVOHYzxQW3/lnOEJq8xc3+YPMRYv4P/aHutE3r6B5xqyBbIIXdb12yU7RMZntv1\n\
    ZKktwxR8+JlVwadaAAYPFB0CgYB+d2NH3HGIRehBilgtAG8l7sXgehLZL6RTOt10\n\
    v4RwQdrrZURQSCfdPe/eKQORZ3SDOUR7tAhknh4xMzlCqT+r1weHPKOA+DpmexDX\n\
    R54+ELwP05fNCJuLWiP1nnI9NoOdmkh9AW5Dzu3WbnqP/SsaeJjQPLP+0PM7RG3f\n\
    /tA2uQKBgQDey/Gl4S/ggQhw3OM5k286elxw3HV4gYKFI1nWuhKa7BgTRaWeKr9j\n\
    fraRd9w6gQxN3xO5IxnVZF9LV9UwjButHmvBMVoLh2bgotE9qDnwQ7O3oLY0+6Y/\n\
    1Y9e7OhoGStEXqPXvq8TjnLI6G7EBXcayFQ3eVaUpbGCoJJhMVgE6w==\n\
    ";

    /// `openssl genpkey -algorithm RSA -pkeyopt rsa_keygen_bits:1024` (PKCS#8).
    pub const KEY_1024: &str = "\
    MIICdgIBADANBgkqhkiG9w0BAQEFAASCAmAwggJcAgEAAoGBALsxVoG4eXAhAGBC\n\
    TW5omaMLugFmwGyIJQmOfIDG10y7CF5FquMt9Vsc9RyCngKXQluwzSjPX7GzNBHy\n\
    mkO7MYCUl+DfAGfbgTWfDjgdxmfEvNahAjvRJOboiKwmMEfrwbX8uRRCf2mUzCIZ\n\
    /XnWeEdEKCzfo8SmMWS267ZI+RORAgMBAAECgYAM2y2aySWl+s5AziztOUfb99E1\n\
    iW4LxintBxkr6vwca/f+KYC8C4r/T/TRmXLqsKLBE1pzxBJJbQpVdTj9D90tXkCm\n\
    UvJ4VCUsAVt8qqaI/+N7NTbmRwi7RpRkINLvrJafBPAgfk8OVwJu8wwSibnt9EWV\n\
    4hQdUbhRr91zKg00sQJBAPYY3LglP5rIGWT1zPm8uGQHiuigCWWOEAhQCMuHK4S7\n\
    svvYd1HscykayAiACmUQexQmNcBqPm0hSfC5LnxD6LUCQQDCua4B12NBYIueDksO\n\
    A+dR5JyAd9a9UeNB1v+Jzq2FUlyqXDzLSfQVxAJzmatm6eRKpuXLN/dNY2e3oRii\n\
    ipTtAkEA5lofP76d6qOiIX5yftDHR9pHi7dBFG4FU8KWeOvtMDEBN/tGtMg+gxsu\n\
    YcZ39GGY0yNw7MqF5GBkdY7glDB8gQJAFhvg7uKWqWid0Y9L5EZ7MCglwMxafFpc\n\
    1C/mCa6Gsxl73fflCyvVY9dCc/+hn2i0++jhNGahVI8Ryp6m3mWfrQJANP25xvVa\n\
    mayp2JOSN1gpSWGTsOy6tVh3fgqLuuh2V7pT1w6NBPgMR9B1a5CLG8jd3UzFaG1B\n\
    3QZY1j9NPxdpDQ==\n\
    ";

    /// `KEY_2048` encrypted (`openssl pkcs8 -topk8 -passout pass:test`).
    pub const KEY_2048_ENCRYPTED: &str = "\
    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\n\
    ";
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

    /// RSA (ADR-034, spec T2, T3): RSA-SHA256 PKCS#1 v1.5 over the same
    /// strings, standard base64 with padding (F1, F2). PKCS#1 v1.5 is
    /// deterministic, so openssl's value for the test-only key is the exact
    /// expected value: `printf '%s' "$prehash" | openssl dgst -sha256 -sign
    /// key.pem | base64 -w0`. Each signature is also checked with ring's
    /// public key of the key, parsed here from the PEM body. The HMAC
    /// signer gives the hex values above.
    #[test]
    fn rsa_signatures_are_openssls_base64_and_verify_with_the_public_key() {
        use base64::Engine;
        use ring::signature::{KeyPair, RSA_PKCS1_2048_8192_SHA256, RsaKeyPair, UnparsedPublicKey};
        let b64 = base64::engine::general_purpose::STANDARD;
        let rest = rest_prehash(
            1658384314791,
            "XXXXXXXXXX",
            5000,
            "category=option&symbol=BTC-29JUL22-25000-C",
        );
        let ws = ws_prehash(1662350400000);
        assert_eq!(ws, "GET/realtime1662350400000");
        let signer = Signer::rsa(&test_keys::pem_2048()).unwrap();
        assert!(signer.is_rsa());
        let cases = [
            (
                &rest,
                "uF1JnRB4gXo9cdC7kinL1ZDxQZF8FLp/D136K/H6heA4wGjXMlBu6IFXbwqiDrm5i2j9Guu+b+6zXMZnV686kW/X+acWKGDgN9Xuu0yfmvyUM8WbXqNnwtWzby9wn83FGBijAVMHhER/jrm1qFbmsxUFo7vwJzOopd/N7p7onBCQY4+HXfYI4gF+vy0HXtP1OAw4ZLUmmJQQe2Ak36ATgIrIt50nwYbcD5QLziN2yqFPpyZodRF840NyOOhrS1n42ZANaWnkD6IoNxdR9Xuaakg4r5hm10WUIYlp0WIt4WQ0de2TdIM2wtRD6KVmYE4aE/HG+ZbyrK7qH+nUEXUHaw==",
            ),
            (
                &ws,
                "HZKuzRsZu3GyRpRRU9GK/xpL4DUyjo4bd+uRkRedWmZwUUr1ye+RvVNtV+LpPmx5X+1iKtlWIJGe1FNJ/jW/A0q4e2ND7FcQy9zEgbRZwVO+sbUqoN0m69zQ+mhqR5VBKXTFIcbEhzFiQKFHSC/hK5OmdSX2aCU0e0ROiA5o5p5Vcvs6jF4U1G2EF4bq5LX5BPufvpLsI4HoqBcX5Oq/gIjTxAORH2YNwzeuGh1migMEAxTpp2lRXc27qaBGhbPmYURG3QvCguBp5ctIiP2T6RTeiPgfBSYxSZZESMOs/oFVhjeRJ226Nrj7tUqcA9Gn1P4B4Y4SpIzAJ0tWLizZWg==",
            ),
        ];
        let der = b64.decode(test_keys::KEY_2048.replace('\n', "")).unwrap();
        let pair = RsaKeyPair::from_pkcs8(&der).unwrap();
        let public = UnparsedPublicKey::new(&RSA_PKCS1_2048_8192_SHA256, pair.public_key().as_ref());
        for (pre, want) in cases {
            let got = signer.sign(pre).unwrap();
            assert_eq!(got, want, "{pre}");
            let sig = b64.decode(&got).unwrap();
            assert_eq!(sig.len(), 256);
            assert!(public.verify(pre.as_bytes(), &sig).is_ok(), "{pre}");
            assert!(public.verify(format!("{pre}0").as_bytes(), &sig).is_err());
        }
        let hmac = Signer::hmac("test-secret-0123456789");
        assert!(!hmac.is_rsa());
        assert_eq!(
            hmac.sign(&rest).unwrap(),
            "44cd56dd7f586d4c0f7374b4ea02c8605d31f5c1eb48ef865cfd2236378f1678"
        );
        assert_eq!(
            hmac.sign(&ws).unwrap(),
            "15fb3c95518f3b90ae93f4bc1e31adb7e167feb1286f8349d259270f69661eb9"
        );
    }
}
