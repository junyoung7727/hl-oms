//! Exact conversion between the venue's decimal strings and `i64` with an
//! exponent (lake contract 1.3): `decimal value = integer × 10^exp`.
//! Nothing is rounded: a value that is not exact, or does not fit `i64`, is an error.

/// Why a decimal string could not be converted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecimalError {
    /// Not of the form `-?[0-9]+(\.[0-9]+)?`.
    Syntax,
    /// The value has more decimal places than the exponent allows.
    NotExact,
    /// The integer does not fit `i64`.
    Overflow,
}

fn split(s: &str) -> Result<(bool, &str, &str), DecimalError> {
    let (neg, body) = match s.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, s),
    };
    let (int, frac) = match body.split_once('.') {
        Some((i, f)) => (i, f),
        None => (body, ""),
    };
    let digits = |p: &str| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit());
    if !digits(int) || (body.contains('.') && !digits(frac)) {
        return Err(DecimalError::Syntax);
    }
    Ok((neg, int, frac))
}

/// The integer `n` with `n × 10^exp` equal to the decimal string `s`.
pub fn to_int(s: &str, exp: i8) -> Result<i64, DecimalError> {
    let (neg, int, frac) = split(s)?;
    let frac = frac.trim_end_matches('0');
    let places = -i32::from(exp);
    if places < 0 || frac.len() as i32 > places {
        // exp > 0 would need trailing zeros of the integer part; no venue uses it.
        return Err(DecimalError::NotExact);
    }
    let mut digits = String::with_capacity(int.len() + places as usize);
    digits.push_str(int);
    digits.push_str(frac);
    digits.extend(std::iter::repeat_n('0', places as usize - frac.len()));
    let trimmed = digits.trim_start_matches('0');
    if trimmed.len() > 20 {
        return Err(DecimalError::Overflow);
    }
    let magnitude: i128 = if trimmed.is_empty() { 0 } else { trimmed.parse().map_err(|_| DecimalError::Overflow)? };
    let value = if neg { -magnitude } else { magnitude };
    i64::try_from(value).map_err(|_| DecimalError::Overflow)
}

/// Minus the number of decimal places of `s` after trailing zeros are removed
/// (`"0.10"` → −1, `"5"` → 0). Used for `px_exp` from `tick_size` and
/// `qty_exp` from `lot_size`.
pub fn exponent_of(s: &str) -> Result<i8, DecimalError> {
    let (_, _, frac) = split(s)?;
    let places = frac.trim_end_matches('0').len();
    i8::try_from(places).map(|p| -p).map_err(|_| DecimalError::Overflow)
}
