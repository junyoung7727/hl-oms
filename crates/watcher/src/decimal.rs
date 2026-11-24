//! Decimal strings of the journal (C-TAIL "기록 형식"): an optional `-`,
//! digits, and an optional `.` with 1 to 8 digits. No exponent, no `+`, no
//! more places. Read into an `i128` scaled by 10^8; arithmetic is exact and
//! checked. The venue's decimal strings of the kill notices' position block
//! (U19b2a) are read the same way.

use std::fmt;

/// 10^8: the journal's decimals have at most 8 places.
pub const SCALE: i128 = 100_000_000;
const PLACES: usize = 8;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Dec(pub i128);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DecError {
    /// Not of the documented form; the text is kept for the error line.
    Form(String),
    Overflow,
}

impl fmt::Display for DecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DecError::Form(s) => write!(f, "not a journal decimal: {s:?}"),
            DecError::Overflow => write!(f, "decimal overflow"),
        }
    }
}

impl Dec {
    pub const ZERO: Dec = Dec(0);

    pub fn parse(s: &str) -> Result<Dec, DecError> {
        let form = || DecError::Form(s.to_string());
        let (neg, body) = match s.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => (false, s),
        };
        let (int, frac) = match body.split_once('.') {
            Some((i, f)) => (i, Some(f)),
            None => (body, None),
        };
        if int.is_empty() || !int.bytes().all(|b| b.is_ascii_digit()) {
            return Err(form());
        }
        let frac = frac.unwrap_or("");
        if body.contains('.') && (frac.is_empty() || frac.len() > PLACES) {
            return Err(form());
        }
        if !frac.bytes().all(|b| b.is_ascii_digit()) {
            return Err(form());
        }
        let mut v: i128 = 0;
        for b in int
            .bytes()
            .chain(frac.bytes())
            .chain(std::iter::repeat_n(b'0', PLACES - frac.len()))
        {
            v = v
                .checked_mul(10)
                .and_then(|v| v.checked_add(i128::from(b - b'0')))
                .ok_or(DecError::Overflow)?;
        }
        Ok(Dec(if neg { -v } else { v }))
    }

    pub fn checked_add(self, o: Dec) -> Result<Dec, DecError> {
        self.0.checked_add(o.0).map(Dec).ok_or(DecError::Overflow)
    }

    pub fn checked_sub(self, o: Dec) -> Result<Dec, DecError> {
        self.0.checked_sub(o.0).map(Dec).ok_or(DecError::Overflow)
    }

    pub fn checked_neg(self) -> Result<Dec, DecError> {
        self.0.checked_neg().map(Dec).ok_or(DecError::Overflow)
    }

    pub fn is_zero(self) -> bool {
        self.0 == 0
    }

    /// `num / den × 100` in per cent, rounded toward zero to 0.01 % (the
    /// kill notices' distance to the liquidation price, U19b2a): `num.0 ×
    /// 10^4 / den.0` read as hundredths of a per cent, in checked `i128`.
    /// `None` when `num < 0`, `den ≤ 0` or a step overflows.
    pub fn percent_floor(num: Dec, den: Dec) -> Option<Dec> {
        if num.0 < 0 || den.0 <= 0 {
            return None;
        }
        let hundredths = num.0.checked_mul(10_000)? / den.0;
        hundredths.checked_mul(SCALE / 100).map(Dec)
    }
}

/// The canonical form: no trailing zeros after the point, no point for an
/// integer, `-` only for a negative value.
impl fmt::Display for Dec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sign = if self.0 < 0 { "-" } else { "" };
        let a = self.0.unsigned_abs();
        let (int, frac) = (a / SCALE as u128, a % SCALE as u128);
        if frac == 0 {
            return write!(f, "{sign}{int}");
        }
        let digits = format!("{frac:08}");
        write!(f, "{sign}{int}.{}", digits.trim_end_matches('0'))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decimals_parse_exactly_and_refuse_other_forms() {
        for (text, scaled) in [
            ("0", 0),
            ("1", SCALE),
            ("-1", -SCALE),
            ("0.5", 50_000_000),
            ("-0.00000001", -1),
            ("123.45678901", 12_345_678_901),
            ("0.1", 10_000_000),
            ("70000", 70_000 * SCALE),
            ("-0", 0),
            ("007.10", 710_000_000),
        ] {
            assert_eq!(Dec::parse(text), Ok(Dec(scaled)), "{text}");
        }
        for bad in [
            "",
            "-",
            ".",
            "1.",
            ".5",
            "+1",
            "1e5",
            "1E5",
            "1.000000001",
            " 1",
            "1 ",
            "1,5",
            "--1",
            "1.2.3",
            "0x10",
            "١",
            "1_000",
            "NaN",
            "inf",
            "-.5",
        ] {
            assert!(matches!(Dec::parse(bad), Err(DecError::Form(_))), "{bad:?}");
        }
        // The largest integer part that fits i128 after scaling, and one more digit.
        let big = (i128::MAX / SCALE).to_string();
        assert_eq!(Dec::parse(&big), Ok(Dec(i128::MAX / SCALE * SCALE)));
        assert_eq!(Dec::parse(&format!("{big}0")), Err(DecError::Overflow));
        assert_eq!(Dec::parse(&"9".repeat(60)), Err(DecError::Overflow));

        // Exact, checked arithmetic.
        let a = Dec::parse("0.1").unwrap_or_default();
        let b = Dec::parse("0.2").unwrap_or_default();
        assert_eq!(a.checked_add(b), Ok(Dec::parse("0.3").unwrap_or_default()));
        assert_eq!(a.checked_sub(b), Ok(Dec::parse("-0.1").unwrap_or_default()));
        assert_eq!(Dec(i128::MAX).checked_add(Dec(1)), Err(DecError::Overflow));
        assert_eq!(Dec(i128::MIN).checked_sub(Dec(1)), Err(DecError::Overflow));
        assert_eq!(Dec(i128::MIN).checked_neg(), Err(DecError::Overflow));
        assert_eq!(b.checked_neg(), Ok(Dec(-20_000_000)));

        // Shown in the canonical form (for detail lines).
        for (scaled, text) in [
            (0, "0"),
            (SCALE, "1"),
            (-50_000_000, "-0.5"),
            (12_345_678_901, "123.45678901"),
            (-1, "-0.00000001"),
        ] {
            assert_eq!(Dec(scaled).to_string(), text);
        }
    }
    #[test]
    fn percent_floor_rounds_toward_zero_and_refuses_overflow() {
        let d = |t: &str| Dec::parse(t).unwrap_or_default();
        let pct = |n: &str, m: &str| Dec::percent_floor(d(n), d(m)).map(|p| p.to_string());
        // Rounded down to 0.01 %, never up.
        assert_eq!(pct("1", "3").as_deref(), Some("33.33"));
        assert_eq!(pct("2", "3").as_deref(), Some("66.66"));
        assert_eq!(pct("7000", "70000").as_deref(), Some("10"));
        assert_eq!(pct("0.99999999", "100").as_deref(), Some("0.99"));
        assert_eq!(pct("0.00000001", "70000").as_deref(), Some("0"));
        assert_eq!(pct("0", "1").as_deref(), Some("0"));
        assert_eq!(pct("3", "1").as_deref(), Some("300"));
        assert_eq!(pct("1234.5", "67890.12345678").as_deref(), Some("1.81"));
        // num ≥ 0 and den > 0 only.
        assert_eq!(pct("-1", "3"), None);
        assert_eq!(pct("1", "0"), None);
        assert_eq!(pct("1", "-3"), None);
        // An overflow of either multiplication is None, not a wrong value.
        assert_eq!(Dec::percent_floor(Dec(i128::MAX / 1000), Dec(1)), None);
        assert_eq!(Dec::percent_floor(Dec(i128::MAX / 10_000), Dec(1)), None);
        assert_eq!(
            Dec::percent_floor(Dec(i128::MAX / 10_000), Dec(i128::MAX / 10_000)),
            Some(Dec(100 * SCALE))
        );
    }
}
