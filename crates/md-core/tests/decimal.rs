//! Venue prices arrive as decimal strings; a rounded price would put a book
//! level or a trade at a price the venue never quoted.

use md_core::decimal::{exponent_of, to_int};

#[test]
fn decimal_string_converts_exactly_or_fails() {
    assert_eq!(to_int("42000.1", -1), Ok(420001));
    assert_eq!(to_int("0.010", -3), Ok(10));
    assert_eq!(to_int("-0.0010", -4), Ok(-10));
    assert_eq!(to_int("5", 0), Ok(5));
    assert_eq!(to_int("1767340800000", 0), Ok(1767340800000));
    assert_eq!(to_int("0.0001", -8), Ok(10000));
    for bad in ["42000.15", "", "1e3", "1.", ".5", "+1", "1,0", " 1", "--1", "92233720368547758.08"] {
        assert!(to_int(bad, -1).is_err(), "{bad:?} must be refused");
    }
    assert!(to_int("9223372036854775808", 0).is_err(), "beyond i64");
    assert_eq!(to_int("9223372036854775807", 0), Ok(i64::MAX));
}

#[test]
fn exponent_is_minus_the_decimal_places_after_trailing_zeros() {
    assert_eq!(exponent_of("0.10"), Ok(-1));
    assert_eq!(exponent_of("5"), Ok(0));
    assert_eq!(exponent_of("0.001"), Ok(-3));
    assert_eq!(exponent_of("10"), Ok(0));
    assert!(exponent_of("abc").is_err());
}
