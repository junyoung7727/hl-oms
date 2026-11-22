//! Bybit `orderLinkId` of the OMS, the `Base32Pair` form
//! (`docs/contracts/identifiers.md`, ADR-022 §7): the instance id in 13
//! digits, `-`, the order id in 13 digits; lower-case base32hex `0-9a-v`,
//! 27 characters. Decoding is strict: another case, length or separator,
//! or a 13-digit value above `u64::MAX` (first digit above `f`) is not ours.

const DIGITS: usize = 13;
const ALPHABET: &[u8; 32] = b"0123456789abcdefghijklmnopqrstuv";

pub fn encode(instance_id: u64, order_id: u64) -> String {
    let mut s = String::with_capacity(2 * DIGITS + 1);
    push_digits(instance_id, &mut s);
    s.push('-');
    push_digits(order_id, &mut s);
    s
}

pub fn decode(s: &str) -> Option<(u64, u64)> {
    let (instance, order) = s.split_once('-')?;
    Some((digits(instance)?, digits(order)?))
}

fn push_digits(v: u64, out: &mut String) {
    for i in (0..DIGITS).rev() {
        // Digit i from the right holds bits 5i..5i+4; the leftmost only
        // bits 60..63, so its value is at most `f`.
        let d = (u128::from(v) >> (5 * i)) & 31;
        out.push(char::from(ALPHABET[d as usize]));
    }
}

fn digits(s: &str) -> Option<u64> {
    if s.len() != DIGITS {
        return None;
    }
    let mut v: u128 = 0;
    for b in s.bytes() {
        let d = ALPHABET.iter().position(|&a| a == b)?;
        v = (v << 5) | d as u128;
    }
    u64::try_from(v).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base32_pair_matches_the_documented_vector() {
        // identifiers.md / ADR-022: instance 0x0123456789abcdef, order 42.
        assert_eq!(
            encode(0x0123_4567_89ab_cdef, 42),
            "028q5cu4qnjff-000000000001a"
        );
        assert_eq!(
            decode("028q5cu4qnjff-000000000001a"),
            Some((0x0123_4567_89ab_cdef, 42))
        );
        assert_eq!(encode(0, 0), "0000000000000-0000000000000");
        assert_eq!(encode(u64::MAX, u64::MAX), "fvvvvvvvvvvvv-fvvvvvvvvvvvv");
        assert_eq!(
            decode("fvvvvvvvvvvvv-fvvvvvvvvvvvv"),
            Some((u64::MAX, u64::MAX))
        );
        assert_eq!(encode(1, 32), "0000000000001-0000000000010");
        for bad in [
            "",
            "-",
            "028q5cu4qnjff000000000001a",
            "028q5cu4qnjff-000000000001A",
            "028Q5CU4QNJFF-000000000001a",
            "028q5cu4qnjff-00000000001a",
            "028q5cu4qnjff-0000000000001a",
            "028q5cu4qnjf-0000000000001a",
            "028q5cu4qnjff_000000000001a",
            "028q5cu4qnjff-000000000001w",
            "g000000000000-0000000000000",
            "0000000000000-g000000000000",
            "028q5cu4qnjff-000000000001a-",
            "028q5cu4qnjff--00000000001a",
            " 028q5cu4qnjff-000000000001",
            "+28q5cu4qnjff-000000000001a",
            "0x0123456789abcdef000000000000002a",
        ] {
            assert_eq!(decode(bad), None, "{bad:?}");
        }
    }
}
