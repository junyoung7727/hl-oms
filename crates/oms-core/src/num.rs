//! Fixed-point quantities, prices and amounts.
//!
//! Every value is an `i64` count of 1e-8 units. There is no floating point in
//! the core. Arithmetic that can overflow is checked and returns `None`; the
//! caller turns that into a rejection or a fault, never into a wrapped value.
//!
//! On the wire (journal, snapshot, API) the values are canonical decimal
//! strings ("0.001", "-12.5"), so that tools reading the JSON never round
//! through a double. See ADR-009.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

pub const SCALE: i64 = 100_000_000;
pub const DECIMALS: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NumError {
    Empty,
    BadChar,
    TooManyDecimals,
    Overflow,
    Negative,
}

impl fmt::Display for NumError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            NumError::Empty => "empty number",
            NumError::BadChar => "unexpected character in number",
            NumError::TooManyDecimals => "more than 8 decimal places",
            NumError::Overflow => "number out of range",
            NumError::Negative => "negative value not allowed",
        })
    }
}

/// Parses a plain decimal ("12", "12.5", "-0.00000001") into 1e-8 units.
/// Exponents, a leading '+', digit separators and more than 8 decimals are
/// rejected rather than rounded.
pub fn parse_scaled(s: &str) -> Result<i64, NumError> {
    let (neg, body) = match s.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, s),
    };
    if body.is_empty() {
        return Err(NumError::Empty);
    }
    let (int_part, frac_part) = match body.split_once('.') {
        Some((i, f)) => (i, f),
        None => (body, ""),
    };
    if int_part.is_empty() && frac_part.is_empty() {
        return Err(NumError::Empty);
    }
    if frac_part.len() > DECIMALS {
        return Err(NumError::TooManyDecimals);
    }
    let mut acc: i128 = 0;
    for b in int_part.bytes() {
        if !b.is_ascii_digit() {
            return Err(NumError::BadChar);
        }
        acc = acc * 10 + i128::from(b - b'0');
        if acc > i128::from(i64::MAX) {
            return Err(NumError::Overflow);
        }
    }
    acc *= i128::from(SCALE);
    let mut unit = i128::from(SCALE) / 10;
    for b in frac_part.bytes() {
        if !b.is_ascii_digit() {
            return Err(NumError::BadChar);
        }
        acc += i128::from(b - b'0') * unit;
        unit /= 10;
    }
    if neg {
        acc = -acc;
    }
    i64::try_from(acc).map_err(|_| NumError::Overflow)
}

/// Canonical decimal rendering: no exponent, no trailing zeros, "-0" never.
pub fn format_scaled(v: i64) -> String {
    let neg = v < 0;
    let abs = v.unsigned_abs();
    let scale = SCALE as u64;
    let int = abs / scale;
    let frac = abs % scale;
    let mut out = String::with_capacity(24);
    if neg {
        out.push('-');
    }
    out.push_str(&int.to_string());
    if frac != 0 {
        let digits = format!("{frac:08}");
        out.push('.');
        out.push_str(digits.trim_end_matches('0'));
    }
    out
}

macro_rules! scaled_type {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
        pub struct $name(pub i64);

        impl $name {
            pub const ZERO: $name = $name(0);

            pub fn raw(self) -> i64 {
                self.0
            }
            pub fn is_zero(self) -> bool {
                self.0 == 0
            }
            pub fn checked_add(self, rhs: $name) -> Option<$name> {
                self.0.checked_add(rhs.0).map($name)
            }
            pub fn checked_sub(self, rhs: $name) -> Option<$name> {
                self.0.checked_sub(rhs.0).map($name)
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&format_scaled(self.0))
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}({})", stringify!($name), format_scaled(self.0))
            }
        }

        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.serialize_str(&format_scaled(self.0))
            }
        }
    };
}

scaled_type!(
    /// Order or fill quantity. Never negative.
    Qty
);
scaled_type!(
    /// Price in quote units per one base unit. Never negative; zero is not a
    /// valid order price.
    Px
);
scaled_type!(
    /// Signed quote-currency amount (notional, fee).
    Amount
);
scaled_type!(
    /// Signed position quantity.
    SQty
);
scaled_type!(
    /// A factor without a unit, such as a leverage limit. Never negative.
    Ratio
);

macro_rules! parse_impl {
    ($name:ident, non_negative) => {
        impl $name {
            pub fn parse(s: &str) -> Result<$name, NumError> {
                let v = parse_scaled(s)?;
                if v < 0 {
                    return Err(NumError::Negative);
                }
                Ok($name(v))
            }
        }
        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                let s = <std::borrow::Cow<'de, str>>::deserialize(d)?;
                $name::parse(&s).map_err(serde::de::Error::custom)
            }
        }
    };
    ($name:ident, signed) => {
        impl $name {
            pub fn parse(s: &str) -> Result<$name, NumError> {
                parse_scaled(s).map($name)
            }
        }
        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                let s = <std::borrow::Cow<'de, str>>::deserialize(d)?;
                $name::parse(&s).map_err(serde::de::Error::custom)
            }
        }
    };
}

parse_impl!(Qty, non_negative);
parse_impl!(Px, non_negative);
parse_impl!(Amount, signed);
parse_impl!(SQty, signed);
parse_impl!(Ratio, non_negative);

impl Qty {
    pub fn signed(self, side: crate::model::Side) -> SQty {
        match side {
            crate::model::Side::Buy => SQty(self.0),
            crate::model::Side::Sell => SQty(-self.0),
        }
    }
    pub fn min(self, other: Qty) -> Qty {
        Qty(self.0.min(other.0))
    }
}

/// `px * qty`, rounded up to the next 1e-8 quote unit.
///
/// Rounding up is the conservative direction when the result is compared
/// against a maximum: an exposure is never understated. For a comparison
/// against a minimum use [`notional_floor`].
/// `None` means the product does not fit in an `i64` amount.
pub fn notional_ceil(px: Px, qty: Qty) -> Option<Amount> {
    if px.0 < 0 || qty.0 < 0 {
        return None;
    }
    let prod = i128::from(px.0) * i128::from(qty.0);
    let scale = i128::from(SCALE);
    let q = (prod + scale - 1) / scale;
    i64::try_from(q).ok().map(Amount)
}

/// `px * qty`, rounded down. The conservative direction when the result is
/// compared against a minimum: a value just below it is not rounded up to it.
pub fn notional_floor(px: Px, qty: Qty) -> Option<Amount> {
    if px.0 < 0 || qty.0 < 0 {
        return None;
    }
    let prod = i128::from(px.0) * i128::from(qty.0);
    i64::try_from(prod / i128::from(SCALE)).ok().map(Amount)
}

/// `px * qty`, rounded half-up. Used only for reporting sums (traded
/// notional), never for a limit check.
pub fn notional_round(px: Px, qty: Qty) -> Option<Amount> {
    if px.0 < 0 || qty.0 < 0 {
        return None;
    }
    let prod = i128::from(px.0) * i128::from(qty.0);
    let scale = i128::from(SCALE);
    let q = (prod + scale / 2) / scale;
    i64::try_from(q).ok().map(Amount)
}

/// Number of significant decimal digits in a scaled value, ignoring trailing
/// zeros: 123.45 -> 5, 0.00120 -> 2, 1000 -> 1.
pub fn significant_digits(v: i64) -> u32 {
    let mut n = v.unsigned_abs();
    if n == 0 {
        return 0;
    }
    while n.is_multiple_of(10) {
        n /= 10;
    }
    let mut digits = 0;
    while n > 0 {
        digits += 1;
        n /= 10;
    }
    digits
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn parses_business_examples() {
        // Expected values are written out by hand, not computed by the code under test.
        assert_eq!(parse_scaled("0"), Ok(0));
        assert_eq!(parse_scaled("1"), Ok(100_000_000));
        assert_eq!(parse_scaled("0.001"), Ok(100_000));
        assert_eq!(parse_scaled("70000.5"), Ok(7_000_050_000_000));
        assert_eq!(parse_scaled("-12.5"), Ok(-1_250_000_000));
        assert_eq!(parse_scaled("0.00000001"), Ok(1));
        assert_eq!(parse_scaled(".5"), Ok(50_000_000));
        assert_eq!(parse_scaled("5."), Ok(500_000_000));
    }

    #[test]
    fn rejects_instead_of_rounding() {
        assert_eq!(parse_scaled("0.000000001"), Err(NumError::TooManyDecimals));
        assert_eq!(parse_scaled("1e3"), Err(NumError::BadChar));
        assert_eq!(parse_scaled("+1"), Err(NumError::BadChar));
        assert_eq!(parse_scaled("1,000"), Err(NumError::BadChar));
        assert_eq!(parse_scaled(""), Err(NumError::Empty));
        assert_eq!(parse_scaled("-"), Err(NumError::Empty));
        assert_eq!(parse_scaled("."), Err(NumError::Empty));
        assert_eq!(parse_scaled("92233720369"), Err(NumError::Overflow));
        assert_eq!(Qty::parse("-1"), Err(NumError::Negative));
    }

    #[test]
    fn formats_canonically() {
        assert_eq!(format_scaled(0), "0");
        assert_eq!(format_scaled(100_000), "0.001");
        assert_eq!(format_scaled(-1_250_000_000), "-12.5");
        assert_eq!(format_scaled(i64::MAX), "92233720368.54775807");
        assert_eq!(format_scaled(i64::MIN), "-92233720368.54775808");
    }

    #[test]
    fn notional_rounds_in_the_documented_direction() {
        // 0.00000003 * 0.5 = 0.000000015 -> ceil 0.00000002, half-up 0.00000002
        assert_eq!(notional_ceil(Px(3), Qty(50_000_000)), Some(Amount(2)));
        assert_eq!(notional_round(Px(3), Qty(50_000_000)), Some(Amount(2)));
        // 0.00000001 * 0.4 = 0.000000004 -> ceil 0.00000001, half-up 0
        assert_eq!(notional_ceil(Px(1), Qty(40_000_000)), Some(Amount(1)));
        assert_eq!(notional_round(Px(1), Qty(40_000_000)), Some(Amount(0)));
        // 70000 * 0.001 = 70 exactly
        assert_eq!(
            notional_ceil(Px::parse("70000").unwrap(), Qty::parse("0.001").unwrap()),
            Some(Amount::parse("70").unwrap())
        );
    }

    #[test]
    fn notional_overflow_is_reported() {
        assert_eq!(notional_ceil(Px(i64::MAX), Qty(i64::MAX)), None);
        assert_eq!(notional_ceil(Px(-1), Qty(1)), None);
    }

    #[test]
    fn significant_digits_examples() {
        assert_eq!(significant_digits(parse_scaled("123.45").unwrap()), 5);
        assert_eq!(significant_digits(parse_scaled("0.0012").unwrap()), 2);
        assert_eq!(significant_digits(parse_scaled("1000").unwrap()), 1);
        assert_eq!(significant_digits(parse_scaled("70000.5").unwrap()), 6);
        assert_eq!(significant_digits(0), 0);
    }

    proptest! {
        #[test]
        fn format_parse_roundtrip(v in any::<i64>()) {
            prop_assert_eq!(parse_scaled(&format_scaled(v)), Ok(v));
        }

        #[test]
        fn serde_roundtrip_is_exact(v in 0..i64::MAX) {
            let q = Qty(v);
            let json = serde_json::to_string(&q).unwrap();
            let back: Qty = serde_json::from_str(&json).unwrap();
            prop_assert_eq!(q, back);
        }

        // Independent oracle: exact rational comparison in i128, no division.
        #[test]
        fn notional_ceil_is_the_least_upper_bound(px in 0..2_000_000_000_000_000i64, qty in 0..2_000_000_000_000i64) {
            let n = notional_ceil(Px(px), Qty(qty));
            let exact = i128::from(px) * i128::from(qty);
            if let Some(Amount(n)) = n {
                let scaled = i128::from(n) * i128::from(SCALE);
                prop_assert!(scaled >= exact);
                prop_assert!(scaled - exact < i128::from(SCALE));
            } else {
                prop_assert!(exact > i128::from(i64::MAX) * i128::from(SCALE) - i128::from(SCALE));
            }
        }
    }
}
