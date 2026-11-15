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
}
