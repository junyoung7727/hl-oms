//! Hyperliquid L1 action signing.
//!
//! The official documentation does not specify the procedure; it points to
//! the Python SDK. This follows `hyperliquid/utils/signing.py` at commit
//! 2fdb18f (2026-06-04) and is checked against that repository's own test
//! vectors (`tests/signing_test.py`):
//!
//! ```text
//! action hash  = keccak256( msgpack(action) ‖ nonce (8 bytes, big endian)
//!                           ‖ 0x00                       (no vault address)
//!                           ‖ [0x00 ‖ expiresAfter (8 bytes, big endian)] )
//! message      = Agent { source: "a" (mainnet) | "b" (testnet), connectionId: action hash }
//! domain       = { name: "Exchange", version: "1", chainId: 1337, verifyingContract: 0x0 }
//! signature    = secp256k1 over the EIP-712 digest, v = 27 + recovery id
//! ```
//!
//! The private key lives only inside `Signer`. It is never formatted, logged
//! or sent anywhere; `Debug` prints the derived address only.

use k256::ecdsa::SigningKey;
use serde::Serialize;
use sha3::{Digest, Keccak256};

fn keccak(data: &[u8]) -> [u8; 32] {
    let mut h = Keccak256::new();
    h.update(data);
    h.finalize().into()
}

/// `keccak256(msgpack(action) ‖ nonce ‖ vault flag ‖ [expiry])`.
pub fn action_hash<A: Serialize>(action: &A, nonce_ms: u64, expires_after_ms: Option<u64>) -> Result<[u8; 32], String> {
    Ok(keccak(&action_preimage(action, nonce_ms, expires_after_ms)?))
}

pub fn action_preimage<A: Serialize>(action: &A, nonce_ms: u64, expires_after_ms: Option<u64>) -> Result<Vec<u8>, String> {
    // Field names and field order are part of the hash: maps, in declaration order.
    let mut data = rmp_serde::to_vec_named(action).map_err(|e| e.to_string())?;
    data.extend_from_slice(&nonce_ms.to_be_bytes());
    data.push(0x00); // no vault address
    if let Some(e) = expires_after_ms {
        data.push(0x00);
        data.extend_from_slice(&e.to_be_bytes());
    }
    Ok(data)
}

fn domain_separator() -> [u8; 32] {
    let type_hash = keccak(b"EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)");
    let mut buf = Vec::with_capacity(32 * 5);
    buf.extend_from_slice(&type_hash);
    buf.extend_from_slice(&keccak(b"Exchange"));
    buf.extend_from_slice(&keccak(b"1"));
    let mut chain = [0u8; 32];
    chain[24..].copy_from_slice(&1337u64.to_be_bytes());
    buf.extend_from_slice(&chain);
    buf.extend_from_slice(&[0u8; 32]); // verifyingContract = 0x0, left padded
    keccak(&buf)
}

/// EIP-712 digest of `Agent { source, connectionId }`.
pub fn agent_digest(connection_id: &[u8; 32], mainnet: bool) -> [u8; 32] {
    let type_hash = keccak(b"Agent(string source,bytes32 connectionId)");
    let source: &[u8] = if mainnet { b"a" } else { b"b" };
    let mut s = Vec::with_capacity(96);
    s.extend_from_slice(&type_hash);
    s.extend_from_slice(&keccak(source));
    s.extend_from_slice(connection_id);
    let struct_hash = keccak(&s);
    let mut d = Vec::with_capacity(66);
    d.extend_from_slice(&[0x19, 0x01]);
    d.extend_from_slice(&domain_separator());
    d.extend_from_slice(&struct_hash);
    keccak(&d)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Signature {
    pub r: String,
    pub s: String,
    pub v: u8,
}

pub struct Signer {
    key: SigningKey,
    address: String,
    mainnet: bool,
}

impl std::fmt::Debug for Signer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Signer({}, mainnet={})", self.address, self.mainnet)
    }
}

impl Signer {
    /// `hex_key` is the 32-byte private key in hex, with or without `0x`.
    pub fn new(hex_key: &str, mainnet: bool) -> Result<Signer, String> {
        let raw = hex::decode(hex_key.trim().trim_start_matches("0x")).map_err(|_| "the key is not hex".to_string())?;
        let key = SigningKey::from_slice(&raw).map_err(|_| "the key is not a valid secp256k1 private key".to_string())?;
        let point = key.verifying_key().to_sec1_point(false);
        let hash = keccak(&point.as_bytes()[1..]);
        let address = format!("0x{}", hex::encode(&hash[12..]));
        Ok(Signer { key, address, mainnet })
    }

    /// Address derived from the key (the agent wallet's address).
    pub fn address(&self) -> &str {
        &self.address
    }

    pub fn sign_digest(&self, digest: &[u8; 32]) -> Result<Signature, String> {
        let (sig, recid) = self.key.sign_prehash_recoverable(digest);
        let bytes = sig.to_bytes();
        Ok(Signature {
            r: format!("0x{}", hex::encode(&bytes[..32])),
            s: format!("0x{}", hex::encode(&bytes[32..])),
            v: 27 + recid.to_byte(),
        })
    }

    pub fn sign_action<A: Serialize>(&self, action: &A, nonce_ms: u64, expires_after_ms: Option<u64>) -> Result<Signature, String> {
        let hash = action_hash(action, nonce_ms, expires_after_ms)?;
        self.sign_digest(&agent_digest(&hash, self.mainnet))
    }
}

#[cfg(test)]
mod tests {
    use super::super::wire::*;
    use super::*;

    // Private key used by the SDK's tests. It controls nothing.
    const TEST_KEY: &str = "0x0123456789012345678901234567890123456789012345678901234567890123";

    /// The SDK prints `r` and `s` without leading zeros; compare as numbers.
    fn same_hex(a: &str, b: &str) -> bool {
        a.trim_start_matches("0x").trim_start_matches('0') == b.trim_start_matches("0x").trim_start_matches('0')
    }

    fn check(sig: &Signature, r: &str, s: &str, v: u8) {
        assert!(same_hex(&sig.r, r), "r {} != {r}", sig.r);
        assert!(same_hex(&sig.s, s), "s {} != {s}", sig.s);
        assert_eq!(sig.v, v);
    }

    #[derive(Serialize)]
    struct Dummy {
        r#type: &'static str,
        num: u64,
    }

    fn order(asset: u32, px: &str, sz: &str, tif: &'static str, cloid: Option<&str>) -> OrderAction {
        OrderAction::single(OrderWire {
            a: asset,
            b: true,
            p: px.into(),
            s: sz.into(),
            r: false,
            t: OrderTypeWire::limit(tif),
            c: cloid.map(String::from),
        })
    }

    /// signing_test.py::test_phantom_agent_creation_matches_production
    #[test]
    fn action_hash_matches_the_sdk_vector() {
        let action = order(4, "1670.1", "0.0147", "Ioc", None);
        let hash = action_hash(&action, 1677777606040, None).unwrap();
        assert_eq!(
            hex::encode(hash),
            "0fcbeda5ae3c4950a548021552a4fea2226858c4453571bf3f24ba017eac2908"
        );
    }

    /// signing_test.py::test_l1_action_signing_matches
    #[test]
    fn dummy_action_signature_matches_the_sdk_vectors() {
        let action = Dummy { r#type: "dummy", num: 100_000_000_000 };
        let main = Signer::new(TEST_KEY, true).unwrap().sign_action(&action, 0, None).unwrap();
        check(
            &main,
            "0x53749d5b30552aeb2fca34b530185976545bb22d0b3ce6f62e31be961a59298",
            "0x755c40ba9bf05223521753995abb2f73ab3229be8ec921f350cb447e384d8ed8",
            27,
        );
        let test = Signer::new(TEST_KEY, false).unwrap().sign_action(&action, 0, None).unwrap();
        check(
            &test,
            "0x542af61ef1f429707e3c76c5293c80d01f74ef853e34b76efffcb57e574f9510",
            "0x17b8b32f086e8cdede991f1e2c529f5dd5297cbe8128500e00cbaf766204a613",
            28,
        );
    }

    /// signing_test.py::test_l1_action_signing_order_matches
    #[test]
    fn order_signature_matches_the_sdk_vectors() {
        let action = order(1, "100", "100", "Gtc", None);
        let main = Signer::new(TEST_KEY, true).unwrap().sign_action(&action, 0, None).unwrap();
        check(
            &main,
            "0xd65369825a9df5d80099e513cce430311d7d26ddf477f5b3a33d2806b100d78e",
            "0x2b54116ff64054968aa237c20ca9ff68000f977c93289157748a3162b6ea940e",
            28,
        );
        let test = Signer::new(TEST_KEY, false).unwrap().sign_action(&action, 0, None).unwrap();
        check(
            &test,
            "0x82b2ba28e76b3d761093aaded1b1cdad4960b3af30212b343fb2e6cdfa4e3d54",
            "0x6b53878fc99d26047f4d7e8c90eb98955a109f44209163f52d8dc4278cbbd9f5",
            27,
        );
    }

    /// signing_test.py::test_l1_action_signing_order_with_cloid_matches
    #[test]
    fn order_with_cloid_signature_matches_the_sdk_vectors() {
        let action = order(1, "100", "100", "Gtc", Some("0x00000000000000000000000000000001"));
        let main = Signer::new(TEST_KEY, true).unwrap().sign_action(&action, 0, None).unwrap();
        check(
            &main,
            "0x41ae18e8239a56cacbc5dad94d45d0b747e5da11ad564077fcac71277a946e3",
            "0x3c61f667e747404fe7eea8f90ab0e76cc12ce60270438b2058324681a00116da",
            27,
        );
        let test = Signer::new(TEST_KEY, false).unwrap().sign_action(&action, 0, None).unwrap();
        check(
            &test,
            "0xeba0664bed2676fc4e5a743bf89e5c7501aa6d870bdb9446e122c9466c5cd16d",
            "0x7f3e74825c9114bc59086f1eebea2928c190fdfbfde144827cb02b85bbe90988",
            28,
        );
    }

    /// signing_test.py::test_schedule_cancel_action
    #[test]
    fn schedule_cancel_signature_matches_the_sdk_vectors() {
        let signer = Signer::new(TEST_KEY, true).unwrap();
        check(
            &signer.sign_action(&ScheduleCancel { r#type: "scheduleCancel", time: None }, 0, None).unwrap(),
            "0x6cdfb286702f5917e76cd9b3b8bf678fcc49aec194c02a73e6d4f16891195df9",
            "0x6557ac307fa05d25b8d61f21fb8a938e703b3d9bf575f6717ba21ec61261b2a0",
            27,
        );
        check(
            &signer
                .sign_action(&ScheduleCancel { r#type: "scheduleCancel", time: Some(123456789) }, 0, None)
                .unwrap(),
            "0x609cb20c737945d070716dcc696ba030e9976fcf5edad87afa7d877493109d55",
            "0x16c685d63b5c7a04512d73f183b3d7a00da5406ff1f8aad33f8ae2163bab758b",
            28,
        );
    }

    /// No SDK vector covers `expiresAfter`. The SDK source appends
    /// `0x00 ‖ expires (8 bytes BE)` after the vault flag; this pins that
    /// layout byte for byte.
    #[test]
    fn expires_after_is_appended_as_in_the_sdk_source() {
        let action = Dummy { r#type: "dummy", num: 1 };
        let without = action_preimage(&action, 5, None).unwrap();
        let with = action_preimage(&action, 5, Some(0x0102_0304_0506_0708)).unwrap();
        // msgpack: fixmap(2) "type" "dummy" "num" 1
        let msgpack = [
            0x82, 0xa4, b't', b'y', b'p', b'e', 0xa5, b'd', b'u', b'm', b'm', b'y', 0xa3, b'n', b'u', b'm', 0x01,
        ];
        let mut expected = msgpack.to_vec();
        expected.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 5]); // nonce
        expected.push(0x00); // no vault
        assert_eq!(without, expected);
        expected.push(0x00);
        expected.extend_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(with, expected);
    }

    #[test]
    fn the_address_is_derived_and_the_key_is_not_printed() {
        let s = Signer::new(TEST_KEY, false).unwrap();
        // Address of this well-known test key, as printed by eth_account.
        assert_eq!(s.address(), "0x14791697260e4c9a71f18484c9f997b308e59325");
        let shown = format!("{s:?}");
        assert!(!shown.contains("0123456789012345678901234567890123456789012345678901234567890123"));
        assert!(Signer::new("zz", false).is_err());
        assert!(Signer::new("0x00", false).is_err());
    }
}
