//! Record framing.
//!
//! ```text
//! offset  size  field
//!      0     4  payload length, little endian
//!      4     4  CRC-32 (IEEE) over bytes 8.. of the frame (sequence + payload)
//!      8     8  sequence number, little endian
//!     16     n  payload
//! ```
//!
//! The length is not covered by the checksum directly, but a wrong length
//! selects the wrong byte range and the checksum then fails, except with
//! probability about 2^-32 per damaged frame.

pub const FRAME_HEADER_LEN: usize = 16;
/// Upper bound on one payload. A larger length field is treated as damage.
pub const MAX_PAYLOAD: usize = 16 * 1024 * 1024;

pub(crate) fn encode(seq: u64, payload: &[u8], out: &mut Vec<u8>) {
    let seq_bytes = seq.to_le_bytes();
    let mut h = crc32fast::Hasher::new();
    h.update(&seq_bytes);
    h.update(payload);
    let crc = h.finalize();
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(&crc.to_le_bytes());
    out.extend_from_slice(&seq_bytes);
    out.extend_from_slice(payload);
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Decoded<'a> {
    Frame {
        seq: u64,
        payload: &'a [u8],
        /// Total bytes consumed, header included.
        len: usize,
    },
    /// The buffer ends before a complete, valid frame. `why` says what was
    /// wrong; whether that is an incomplete tail or damage depends on what
    /// follows, which the caller decides.
    Bad { why: &'static str },
    /// Exactly at the end of the data.
    End,
}

pub(crate) fn decode(buf: &[u8]) -> Decoded<'_> {
    if buf.is_empty() {
        return Decoded::End;
    }
    if buf.len() < FRAME_HEADER_LEN {
        return Decoded::Bad {
            why: "incomplete frame header",
        };
    }
    let len = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;
    let crc = u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]);
    if len > MAX_PAYLOAD {
        return Decoded::Bad {
            why: "length field exceeds the maximum payload",
        };
    }
    let total = FRAME_HEADER_LEN + len;
    if buf.len() < total {
        return Decoded::Bad {
            why: "incomplete frame payload",
        };
    }
    let mut h = crc32fast::Hasher::new();
    h.update(&buf[8..total]);
    if h.finalize() != crc {
        return Decoded::Bad {
            why: "checksum mismatch",
        };
    }
    let mut seq_bytes = [0u8; 8];
    seq_bytes.copy_from_slice(&buf[8..16]);
    Decoded::Frame {
        seq: u64::from_le_bytes(seq_bytes),
        payload: &buf[FRAME_HEADER_LEN..total],
        len: total,
    }
}

/// Looks for any valid frame with a sequence number greater than `after_seq`
/// starting at any byte offset of `buf`. Used to tell an incomplete tail
/// (nothing valid follows) from damage in the middle (something valid does).
pub(crate) fn find_later_frame(buf: &[u8], after_seq: u64) -> Option<(usize, u64)> {
    if buf.len() < FRAME_HEADER_LEN {
        return None;
    }
    for start in 0..=(buf.len() - FRAME_HEADER_LEN) {
        let b = &buf[start..];
        let len = u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize;
        if len > MAX_PAYLOAD || b.len() < FRAME_HEADER_LEN + len {
            continue;
        }
        let mut seq_bytes = [0u8; 8];
        seq_bytes.copy_from_slice(&b[8..16]);
        let seq = u64::from_le_bytes(seq_bytes);
        // A cheap plausibility filter before the checksum: a real later frame
        // has a sequence shortly after the last good one.
        if seq <= after_seq || seq - after_seq > 1_000_000_000 {
            continue;
        }
        if let Decoded::Frame { seq, .. } = decode(b) {
            return Some((start, seq));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_layout() {
        let mut buf = Vec::new();
        encode(7, b"hello", &mut buf);
        assert_eq!(buf.len(), FRAME_HEADER_LEN + 5);
        assert_eq!(&buf[0..4], &5u32.to_le_bytes());
        assert_eq!(&buf[8..16], &7u64.to_le_bytes());
        // CRC-32/IEEE of the 13 bytes (sequence + payload), computed with an
        // independent tool: python3 -c "import zlib,struct;
        // print(hex(zlib.crc32(struct.pack('<Q',7)+b'hello')))"
        // -> 0x82351141
        assert_eq!(u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]), 0x8235_1141);
        assert_eq!(decode(&buf), Decoded::Frame {
            seq: 7,
            payload: b"hello",
            len: buf.len()
        });
        assert_eq!(decode(&[]), Decoded::End);
    }

    #[test]
    fn every_truncation_and_every_bit_flip_is_detected() {
        let mut buf = Vec::new();
        encode(42, b"payload-bytes", &mut buf);
        for cut in 1..buf.len() {
            assert!(matches!(decode(&buf[..cut]), Decoded::Bad { .. }), "cut at {cut}");
        }
        for byte in 0..buf.len() {
            for bit in 0..8 {
                let mut damaged = buf.clone();
                damaged[byte] ^= 1 << bit;
                match decode(&damaged) {
                    Decoded::Bad { .. } => {}
                    other => panic!("flip at {byte}:{bit} gave {other:?}"),
                }
            }
        }
    }

    #[test]
    fn zero_filled_tail_is_not_a_frame() {
        // A zero length with a zero checksum must not parse: the CRC of eight
        // zero bytes (the sequence field) is not zero.
        assert!(matches!(decode(&[0u8; 64]), Decoded::Bad { .. }));
    }

    #[test]
    fn finds_a_later_frame_behind_damage() {
        let mut buf = vec![0xffu8; 37];
        let at = buf.len();
        encode(12, b"later", &mut buf);
        assert_eq!(find_later_frame(&buf, 10), Some((at, 12)));
        assert_eq!(find_later_frame(&buf, 12), None);
        assert_eq!(find_later_frame(&buf[..at + 10], 10), None);
    }
}
