//! QRAW1, the decompressed content of an L0 file (lake contract 2.2):
//! magic, `u32` header length, header JSON, then records of a 25-byte fixed
//! part and a payload. All integers little-endian.

use serde_json::{Map, Value, json};

pub const MAGIC: &[u8; 6] = b"QRAW1\0";
/// The fixed part of a record.
pub const FIXED_LEN: usize = 25;
/// The largest payload.
pub const MAX_LEN: usize = 16_777_216;
pub const CONTRACT_VER: i64 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Src {
    WebSocket,
    Rest,
    Note,
}

impl Src {
    pub fn byte(self) -> u8 {
        match self {
            Src::WebSocket => 0,
            Src::Rest => 1,
            Src::Note => 2,
        }
    }

    pub fn from_byte(b: u8) -> Option<Src> {
        match b {
            0 => Some(Src::WebSocket),
            1 => Some(Src::Rest),
            2 => Some(Src::Note),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    /// Receive time, UTC ns; written as `u64`, at most 2^63 − 1.
    pub recv_ns: i64,
    pub src: Src,
    pub conn_id: u32,
    pub frame_no: u64,
    pub payload: Vec<u8>,
}

impl Record {
    /// Bytes this record takes in QRAW1.
    pub fn encoded_len(&self) -> usize {
        FIXED_LEN + self.payload.len()
    }
}

/// The header JSON. Kept as a JSON object: encoding sorts the keys and uses
/// no spaces, the same bytes quant-data-platform writes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header(pub Map<String, Value>);

impl Header {
    fn base(
        form: &str,
        venue: &str,
        date: &str,
        collector: &str,
        ver: &str,
        host: &str,
    ) -> Map<String, Value> {
        let v = json!({
            "contract_ver": CONTRACT_VER,
            "form": form,
            "venue": venue,
            "date": date,
            "collector": collector,
            "collector_ver": ver,
            "host": host,
        });
        match v {
            Value::Object(m) => m,
            _ => Map::new(),
        }
    }

    /// `by_symbol` day file and instruments day file (`symbol = "_all"`).
    #[allow(clippy::too_many_arguments)]
    pub fn topic(
        venue: &str,
        date: &str,
        collector: &str,
        ver: &str,
        host: &str,
        market: &str,
        channel: &str,
        symbol: &str,
    ) -> Header {
        let mut m = Self::base("topic", venue, date, collector, ver, host);
        m.insert("market".into(), market.into());
        m.insert("channel".into(), channel.into());
        m.insert("symbol".into(), symbol.into());
        Header(m)
    }

    /// Open file and `by_connection` day file.
    #[allow(clippy::too_many_arguments)]
    pub fn connection(
        venue: &str,
        date: &str,
        collector: &str,
        ver: &str,
        host: &str,
        session_ns: i64,
        conn_id: u32,
    ) -> Header {
        let mut m = Self::base("connection", venue, date, collector, ver, host);
        m.insert("session_ns".into(), session_ns.into());
        m.insert("conn_id".into(), conn_id.into());
        Header(m)
    }

    /// Session file.
    pub fn session(
        venue: &str,
        date: &str,
        writer: &str,
        ver: &str,
        host: &str,
        session_ns: i64,
    ) -> Header {
        let mut m = Self::base("session", venue, date, writer, ver, host);
        m.insert("session_ns".into(), session_ns.into());
        Header(m)
    }

    pub fn get_str(&self, key: &str) -> Option<&str> {
        self.0.get(key).and_then(Value::as_str)
    }

    pub fn get_i64(&self, key: &str) -> Option<i64> {
        self.0.get(key).and_then(Value::as_i64)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FramingError {
    NoMagic,
    TornHeader,
    BadHeader(String),
    /// A record runs past the end of a segment that is not the last.
    RecordCrossesSegment {
        segment: usize,
    },
    BadSrc {
        rec_no: usize,
        src: u8,
    },
    TooLong {
        rec_no: usize,
        len: usize,
    },
    BadRecvNs {
        rec_no: usize,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decoded {
    pub header: Header,
    pub records: Vec<Record>,
    /// The content ended inside a record (2.2); the records before it are valid.
    pub truncated_tail: bool,
}

pub fn encode_header(header: &Header) -> Vec<u8> {
    let body = Value::Object(header.0.clone()).to_string().into_bytes();
    let mut out = Vec::with_capacity(10 + body.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&u32::try_from(body.len()).unwrap_or(u32::MAX).to_le_bytes());
    out.extend_from_slice(&body);
    out
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EncodeError {
    TooLong(usize),
    NegativeRecvNs,
}

pub fn encode_record(r: &Record, out: &mut Vec<u8>) -> Result<(), EncodeError> {
    if r.payload.len() > MAX_LEN {
        return Err(EncodeError::TooLong(r.payload.len()));
    }
    let recv = u64::try_from(r.recv_ns).map_err(|_| EncodeError::NegativeRecvNs)?;
    out.extend_from_slice(&recv.to_le_bytes());
    out.push(r.src.byte());
    out.extend_from_slice(&r.conn_id.to_le_bytes());
    out.extend_from_slice(&r.frame_no.to_le_bytes());
    out.extend_from_slice(&(r.payload.len() as u32).to_le_bytes());
    out.extend_from_slice(&r.payload);
    Ok(())
}

fn le<const N: usize>(b: &[u8], at: usize) -> [u8; N] {
    let mut a = [0; N];
    a.copy_from_slice(&b[at..at + N]);
    a
}

/// Parses the magic and header at the start of `seg`; returns the header and
/// its end offset.
pub fn decode_header(seg: &[u8]) -> Result<(Header, usize), FramingError> {
    if seg.len() < 6 || &seg[..6] != MAGIC {
        return Err(FramingError::NoMagic);
    }
    if seg.len() < 10 {
        return Err(FramingError::TornHeader);
    }
    let n = u32::from_le_bytes(le(seg, 6)) as usize;
    let end = 10usize.checked_add(n).ok_or(FramingError::TornHeader)?;
    if end > seg.len() {
        return Err(FramingError::TornHeader);
    }
    match serde_json::from_slice::<Value>(&seg[10..end]) {
        Ok(Value::Object(m)) => Ok((Header(m), end)),
        Ok(_) => Err(FramingError::BadHeader("not an object".into())),
        Err(e) => Err(FramingError::BadHeader(e.to_string())),
    }
}

/// Reads one record at `at`. `Ok(None)` when fewer bytes are left than the
/// record needs (the caller decides: tail or framing error).
pub fn decode_record(
    seg: &[u8],
    at: usize,
    rec_no: usize,
) -> Result<Option<(Record, usize)>, FramingError> {
    if seg.len() - at < FIXED_LEN {
        return Ok(None);
    }
    let recv = u64::from_le_bytes(le(seg, at));
    let src_b = seg[at + 8];
    let src = Src::from_byte(src_b).ok_or(FramingError::BadSrc { rec_no, src: src_b })?;
    let conn_id = u32::from_le_bytes(le(seg, at + 9));
    let frame_no = u64::from_le_bytes(le(seg, at + 13));
    let len = u32::from_le_bytes(le(seg, at + 21)) as usize;
    if len > MAX_LEN {
        return Err(FramingError::TooLong { rec_no, len });
    }
    let recv_ns = i64::try_from(recv).map_err(|_| FramingError::BadRecvNs { rec_no })?;
    let start = at + FIXED_LEN;
    if seg.len() - start < len {
        return Ok(None);
    }
    Ok(Some((
        Record {
            recv_ns,
            src,
            conn_id,
            frame_no,
            payload: seg[start..start + len].to_vec(),
        },
        start + len,
    )))
}

/// Decodes segments (the decompressed zstd frames of one file) read back to
/// back. Every segment holds whole records; only the last may end inside one.
pub fn decode(segments: &[&[u8]]) -> Result<Decoded, FramingError> {
    let first = segments.first().copied().unwrap_or(&[]);
    let (header, mut at) = decode_header(first)?;
    let mut records = Vec::new();
    let mut truncated_tail = false;
    for (i, seg) in segments.iter().enumerate() {
        if i > 0 {
            at = 0;
        }
        while at < seg.len() {
            match decode_record(seg, at, records.len())? {
                Some((r, next)) => {
                    records.push(r);
                    at = next;
                }
                None if i + 1 == segments.len() => {
                    truncated_tail = true;
                    break;
                }
                None => return Err(FramingError::RecordCrossesSegment { segment: i }),
            }
        }
    }
    Ok(Decoded {
        header,
        records,
        truncated_tail,
    })
}
