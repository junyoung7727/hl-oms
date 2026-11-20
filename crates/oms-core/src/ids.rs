//! Identifier types and their validity rules.
//!
//! String identifiers are validated on construction and on deserialisation, so
//! an invalid identifier cannot exist inside the core. Lifetimes and scopes of
//! each identifier are described in docs/contracts/identifiers.md.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Microseconds since the Unix epoch. Assigned by the sequencer, never read
/// from a clock inside the core. Microseconds keep the value below 2^53 so the
/// dashboard's JSON parser reads it exactly.
pub type Ts = u64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdError(pub &'static str);

impl fmt::Display for IdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

fn check(s: &str, max: usize, what: &'static str) -> Result<(), IdError> {
    if s.is_empty() || s.len() > max {
        return Err(IdError(what));
    }
    if !s
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.' | b':'))
    {
        return Err(IdError(what));
    }
    Ok(())
}

macro_rules! string_id {
    ($(#[$doc:meta])* $name:ident, $max:expr, $err:expr) => {
        $(#[$doc])*
        #[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(String);

        impl $name {
            pub fn new(s: impl Into<String>) -> Result<Self, IdError> {
                let s = s.into();
                check(&s, $max, $err)?;
                Ok($name(s))
            }
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
        impl TryFrom<String> for $name {
            type Error = IdError;
            fn try_from(s: String) -> Result<Self, IdError> {
                $name::new(s)
            }
        }
        impl From<$name> for String {
            fn from(v: $name) -> String {
                v.0
            }
        }
        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}({})", stringify!($name), self.0)
            }
        }
    };
}

string_id!(
    /// Strategy name. Scope: one OMS instance. Defined in configuration.
    StrategyId,
    32,
    "strategy id must be 1..=32 chars of [A-Za-z0-9_.:-]"
);
string_id!(
    /// Idempotency key chosen by the strategy. Scope: (strategy, key). See ADR-006.
    ClientOrderId,
    64,
    "client order id must be 1..=64 chars of [A-Za-z0-9_.:-]"
);
string_id!(
    /// Instrument symbol as configured, e.g. "BTC".
    InstrumentId,
    32,
    "instrument id must be 1..=32 chars of [A-Za-z0-9_.:-]"
);
string_id!(
    /// Order identifier assigned by the venue.
    VenueOrderId,
    64,
    "venue order id must be 1..=64 chars of [A-Za-z0-9_.:-]"
);
string_id!(
    /// Identity of one economic fill as defined by the venue adapter.
    FillKey,
    96,
    "fill key must be 1..=96 chars of [A-Za-z0-9_.:-]"
);

string_id!(
    /// A name in a `market_ref` (venue, market, symbol, channel), as the
    /// market-data side writes it, e.g. "orderbook.50".
    RefName,
    64,
    "market ref name must be 1..=64 chars of [A-Za-z0-9_.:-]"
);

/// Order identifier assigned by the core, strictly increasing, never reused.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct OrderId(pub u64);

impl fmt::Display for OrderId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl fmt::Debug for OrderId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "OrderId({})", self.0)
    }
}

/// The identifier this OMS instance presents to the venue for an order.
///
/// It is derived, not stored: `(instance_id, order_id)`. The instance id is
/// fixed when the data directory is created, so an order placed by another
/// data directory (or by hand) is recognisable as foreign during
/// reconciliation.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct VenueClientId {
    pub instance_id: u64,
    pub order_id: OrderId,
}

impl VenueClientId {
    pub fn to_u128(self) -> u128 {
        (u128::from(self.instance_id) << 64) | u128::from(self.order_id.0)
    }
    pub fn from_u128(v: u128) -> VenueClientId {
        VenueClientId {
            instance_id: (v >> 64) as u64,
            order_id: OrderId(v as u64),
        }
    }
    /// 0x-prefixed 32 hex digits, the form Hyperliquid calls a cloid.
    pub fn to_hex(self) -> String {
        format!("0x{:032x}", self.to_u128())
    }
    pub fn parse_hex(s: &str) -> Option<VenueClientId> {
        let body = s.strip_prefix("0x")?;
        if body.len() != 32 {
            return None;
        }
        u128::from_str_radix(body, 16).ok().map(VenueClientId::from_u128)
    }
}

/// The form in which a gateway writes a [`VenueClientId`] on the wire
/// (ADR-022). Each gateway declares one; the status API prints the id in the
/// same form, so the operator sees what the venue knows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WireIdForm {
    /// `0x` + 32 lower-case hex digits ([`VenueClientId::to_hex`]):
    /// Hyperliquid's cloid, also used by the simulated venue.
    Hex128,
    /// `instance_id` and `order_id`, each as 13 lower-case base32hex digits
    /// (`0-9a-v`, RFC 4648 §7), joined by `-`: 27 characters. For Bybit's
    /// `orderLinkId` (at most 36 characters of digits, letters, `-`, `_`).
    Base32Pair,
}

const BASE32HEX: &[u8; 32] = b"0123456789abcdefghijklmnopqrstuv";
const BASE32_DIGITS: usize = 13;

fn base32_digits(mut v: u64, out: &mut String) {
    let mut buf = [0u8; BASE32_DIGITS];
    for slot in buf.iter_mut().rev() {
        *slot = BASE32HEX[(v & 31) as usize];
        v >>= 5;
    }
    out.extend(buf.iter().map(|&b| b as char));
}

/// Exactly 13 digits of `0-9a-v`; `None` for anything else or a value above
/// `u64::MAX` (13 digits hold 65 bits).
fn parse_base32_digits(s: &str) -> Option<u64> {
    if s.len() != BASE32_DIGITS {
        return None;
    }
    let mut v: u128 = 0;
    for b in s.bytes() {
        let d = match b {
            b'0'..=b'9' => b - b'0',
            b'a'..=b'v' => b - b'a' + 10,
            _ => return None,
        };
        v = (v << 5) | u128::from(d);
    }
    u64::try_from(v).ok()
}

impl WireIdForm {
    pub fn encode(self, id: VenueClientId) -> String {
        match self {
            WireIdForm::Hex128 => id.to_hex(),
            WireIdForm::Base32Pair => {
                let mut s = String::with_capacity(2 * BASE32_DIGITS + 1);
                base32_digits(id.instance_id, &mut s);
                s.push('-');
                base32_digits(id.order_id.0, &mut s);
                s
            }
        }
    }

    /// Accepts only the exact form written by [`WireIdForm::encode`]: an id
    /// in another form or case is foreign, not ours.
    pub fn decode(self, s: &str) -> Option<VenueClientId> {
        match self {
            WireIdForm::Hex128 => {
                // `parse_hex` also takes upper case and a sign; this does not.
                let body = s.strip_prefix("0x")?;
                if !body.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
                    return None;
                }
                VenueClientId::parse_hex(s)
            }
            WireIdForm::Base32Pair => {
                let (instance, order) = s.split_once('-')?;
                Some(VenueClientId {
                    instance_id: parse_base32_digits(instance)?,
                    order_id: OrderId(parse_base32_digits(order)?),
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_invalid_identifiers() {
        assert!(StrategyId::new("mm-1").is_ok());
        assert!(StrategyId::new("").is_err());
        assert!(StrategyId::new("a b").is_err());
        assert!(StrategyId::new("x".repeat(33)).is_err());
        assert!(ClientOrderId::new("ord\n1").is_err());
        assert!(serde_json::from_str::<StrategyId>("\"bad id\"").is_err());
        assert!(serde_json::from_str::<StrategyId>("\"good\"").is_ok());
    }

    #[test]
    fn venue_client_id_roundtrip() {
        let id = VenueClientId {
            instance_id: 0x0123_4567_89ab_cdef,
            order_id: OrderId(42),
        };
        assert_eq!(id.to_hex(), "0x0123456789abcdef000000000000002a");
        assert_eq!(VenueClientId::parse_hex(&id.to_hex()), Some(id));
        assert_eq!(VenueClientId::parse_hex("0x12"), None);
        assert_eq!(VenueClientId::parse_hex("0123456789abcdef000000000000002a"), None);
    }

    fn edge_ids() -> Vec<VenueClientId> {
        let halves = [0, 1, 31, 32, u64::MAX - 1, u64::MAX];
        let mut ids = Vec::new();
        for instance_id in halves {
            for order in halves {
                ids.push(VenueClientId { instance_id, order_id: OrderId(order) });
            }
        }
        ids
    }

    #[test]
    fn hex128_is_the_hex_form_of_today() {
        for id in edge_ids() {
            let s = WireIdForm::Hex128.encode(id);
            assert_eq!(s, id.to_hex());
            assert_eq!(WireIdForm::Hex128.decode(&s), Some(id));
        }
        assert_eq!(WireIdForm::Hex128.decode("0x12"), None);
        for bad in ["0x0123456789ABCDEF000000000000002A", "0x+123456789abcdef000000000000002a", "0X0123456789abcdef000000000000002a"] {
            assert_eq!(WireIdForm::Hex128.decode(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn base32_pair_roundtrips_and_fits_an_order_link_id() {
        let id = VenueClientId { instance_id: 0x0123_4567_89ab_cdef, order_id: OrderId(42) };
        assert_eq!(WireIdForm::Base32Pair.encode(id), "028q5cu4qnjff-000000000001a");
        for id in edge_ids() {
            let s = WireIdForm::Base32Pair.encode(id);
            assert_eq!(s.len(), 27);
            assert!(s.len() <= 36);
            assert!(s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'v' | b'-')), "{s}");
            assert_eq!(WireIdForm::Base32Pair.encode(id), s);
            assert_eq!(WireIdForm::Base32Pair.decode(&s), Some(id));
        }
        let max = VenueClientId { instance_id: u64::MAX, order_id: OrderId(u64::MAX) };
        assert_eq!(WireIdForm::Base32Pair.encode(max), "fvvvvvvvvvvvv-fvvvvvvvvvvvv");
    }

    #[test]
    fn base32_pair_gives_a_million_distinct_ids() {
        let mut seen = std::collections::HashSet::new();
        for instance_id in [7, 8] {
            for order in 1..=500_000u64 {
                let id = VenueClientId { instance_id, order_id: OrderId(order) };
                assert!(seen.insert(WireIdForm::Base32Pair.encode(id)));
            }
        }
        assert_eq!(seen.len(), 1_000_000);
    }

    #[test]
    fn base32_pair_decoding_is_strict() {
        let good = "028q5cu4qnjff-000000000001a";
        assert!(WireIdForm::Base32Pair.decode(good).is_some());
        for bad in [
            "028Q5CU4QNJFF-000000000001A",
            "028q5cu4qnjff-00000000001a",
            "028q5cu4qnjff_000000000001a",
            "028q5cu4qnjff000000000001a0",
            "028q5cu4qnjff-000000000001w",
            "028q5cu4qnjff-000000000001a ",
            "g000000000000-0000000000000",
            "0000000000000-g000000000000",
            "-000000000001a",
            "",
            "0x0123456789abcdef000000000000002a",
        ] {
            assert_eq!(WireIdForm::Base32Pair.decode(bad), None, "{bad:?}");
        }
        assert_eq!(WireIdForm::Hex128.decode(good), None);
    }
}
