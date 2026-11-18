//! Frame parsing into md-core messages. Prices and quantities are exact at the
//! instrument's exponents or the frame is refused (lake contract 1.3).

use md_core::adapter::{Instrument, ParseError};
use md_core::decimal::to_int;
use md_core::{BookUpdate, Level, Normalized, Side, StreamKey, Ticker, Trade};
use serde_json::Value;

pub(crate) fn field<'a>(v: &'a Value, name: &str) -> Result<&'a Value, ParseError> {
    v.get(name)
        .ok_or_else(|| ParseError::new(format!("missing {name}")))
}

pub(crate) fn int(v: &Value, name: &str) -> Result<i64, ParseError> {
    field(v, name)?
        .as_i64()
        .ok_or_else(|| ParseError::new(format!("{name} is not an integer")))
}

pub(crate) fn str_field<'a>(v: &'a Value, name: &str) -> Result<&'a str, ParseError> {
    field(v, name)?
        .as_str()
        .ok_or_else(|| ParseError::new(format!("{name} is not a string")))
}

/// Milliseconds since the epoch to nanoseconds.
pub(crate) fn ms_to_ns(ms: i64, name: &str) -> Result<i64, ParseError> {
    ms.checked_mul(1_000_000)
        .ok_or_else(|| ParseError::new(format!("{name} overflows")))
}

pub(crate) fn decimal(s: &str, exp: i8, name: &str) -> Result<i64, ParseError> {
    to_int(s, exp).map_err(|e| ParseError::new(format!("{name} {s:?} at 10^{exp}: {e:?}")))
}

/// The JSON of a frame whose topic is the stream's own topic.
pub(crate) fn frame_json(key: &StreamKey, frame: &[u8]) -> Result<Value, ParseError> {
    let v: Value = serde_json::from_slice(frame).map_err(|e| ParseError::new(e.to_string()))?;
    let topic = str_field(&v, "topic")?;
    if topic != format!("{}.{}", key.channel, key.symbol) {
        return Err(ParseError::new(format!(
            "topic {topic} is not the stream {}.{}",
            key.channel, key.symbol
        )));
    }
    Ok(v)
}

fn levels(data: &Value, name: &str, inst: &Instrument) -> Result<Vec<Level>, ParseError> {
    let rows = field(data, name)?
        .as_array()
        .ok_or_else(|| ParseError::new(format!("{name} is not a list")))?;
    rows.iter()
        .map(|row| match row.as_array().map(Vec::as_slice) {
            Some([px, qty]) => {
                let (Some(px), Some(qty)) = (px.as_str(), qty.as_str()) else {
                    return Err(ParseError::new(format!(
                        "{name}: a level is not two strings"
                    )));
                };
                Ok(Level {
                    px: decimal(px, inst.px_exp, "price")?,
                    qty: decimal(qty, inst.qty_exp, "quantity")?,
                })
            }
            _ => Err(ParseError::new(format!(
                "{name}: a level is not [price, size]"
            ))),
        })
        .collect()
}

/// `orderbook.{depth}.{symbol}`: `type` snapshot or delta, `ts` (ms), `data`
/// with `s`, `b`, `a`, `u` and `seq`. `cts` stays only in L0.
pub(crate) fn book(
    key: &StreamKey,
    inst: &Instrument,
    frame: &[u8],
    recv_ns: i64,
) -> Result<Vec<Normalized>, ParseError> {
    let v = frame_json(key, frame)?;
    let data = field(&v, "data")?;
    let s = str_field(data, "s")?;
    if s != key.symbol {
        return Err(ParseError::new(format!("data.s {s} is not {}", key.symbol)));
    }
    let update = BookUpdate {
        key: key.clone(),
        venue_ts_ns: ms_to_ns(int(&v, "ts")?, "ts")?,
        recv_ns,
        update_id: Some(int(data, "u")?),
        prev_update_id: None,
        first_update_id: None,
        venue_seq: data.get("seq").and_then(Value::as_i64),
        bids: levels(data, "b", inst)?,
        asks: levels(data, "a", inst)?,
        px_exp: inst.px_exp,
        qty_exp: inst.qty_exp,
    };
    match str_field(&v, "type")? {
        "snapshot" => Ok(vec![Normalized::BookSnapshot(update)]),
        "delta" => Ok(vec![Normalized::BookDelta(update)]),
        other => Err(ParseError::new(format!("order book type {other}"))),
    }
}

/// `publicTrade.{symbol}`: `data` is a list of up to 1,024 trades, each with
/// `T` (ms), `s`, `S` (taker side), `v`, `p`, `i`, `BT`, `RPI`, `seq`. `L`
/// (tick direction) is not an L1 column.
pub(crate) fn trades(
    key: &StreamKey,
    inst: &Instrument,
    frame: &[u8],
    recv_ns: i64,
) -> Result<Vec<Normalized>, ParseError> {
    let v = frame_json(key, frame)?;
    let list = field(&v, "data")?
        .as_array()
        .ok_or_else(|| ParseError::new("trade data is not a list"))?;
    let flag = |t: &Value, name: &str| -> Result<bool, ParseError> {
        match t.get(name) {
            None => Ok(false),
            Some(b) => b
                .as_bool()
                .ok_or_else(|| ParseError::new(format!("{name} is not a boolean"))),
        }
    };
    list.iter()
        .map(|t| {
            let s = str_field(t, "s")?;
            if s != key.symbol {
                return Err(ParseError::new(format!(
                    "trade s {s} is not {}",
                    key.symbol
                )));
            }
            let side = match str_field(t, "S")? {
                "Buy" => Side::Buy,
                "Sell" => Side::Sell,
                other => return Err(ParseError::new(format!("trade side {other}"))),
            };
            let flags = u32::from(flag(t, "BT")?) | (u32::from(flag(t, "RPI")?) << 1);
            Ok(Normalized::Trade(Trade {
                key: key.clone(),
                venue_ts_ns: ms_to_ns(int(t, "T")?, "T")?,
                recv_ns,
                trade_id: str_field(t, "i")?.to_string(),
                side,
                px: decimal(str_field(t, "p")?, inst.px_exp, "price")?,
                qty: decimal(str_field(t, "v")?, inst.qty_exp, "quantity")?,
                px_exp: inst.px_exp,
                qty_exp: inst.qty_exp,
                venue_seq: t.get("seq").and_then(Value::as_i64),
                flags,
            }))
        })
        .collect()
}

/// `tickers.{symbol}`: linear and inverse send a snapshot and then deltas that
/// carry only changed fields; spot sends snapshots only. Mark and index
/// prices use `ref_px_exp` (lake contract 1.3).
pub(crate) fn ticker(
    key: &StreamKey,
    inst: &Instrument,
    spot: bool,
    frame: &[u8],
    recv_ns: i64,
) -> Result<Vec<Normalized>, ParseError> {
    let v = frame_json(key, frame)?;
    let is_snapshot = match str_field(&v, "type")? {
        "snapshot" => true,
        "delta" if !spot => false,
        other => return Err(ParseError::new(format!("ticker type {other}"))),
    };
    let data = field(&v, "data")?;
    let symbol = str_field(data, "symbol")?;
    if symbol != key.symbol {
        return Err(ParseError::new(format!(
            "ticker symbol {symbol} is not {}",
            key.symbol
        )));
    }
    let ref_px_exp = inst
        .px_exp
        .checked_sub(2)
        .ok_or_else(|| ParseError::new("px_exp too small"))?;
    let opt = |name: &str, exp: i8| -> Result<Option<i64>, ParseError> {
        match data.get(name) {
            None => Ok(None),
            Some(x) => {
                let s = x
                    .as_str()
                    .ok_or_else(|| ParseError::new(format!("{name} is not a string")))?;
                decimal(s, exp, name).map(Some)
            }
        }
    };
    let next_funding_ns = match opt("nextFundingTime", 0)? {
        Some(ms) => Some(ms_to_ns(ms, "nextFundingTime")?),
        None => None,
    };
    Ok(vec![Normalized::Ticker(Ticker {
        key: key.clone(),
        is_snapshot,
        venue_ts_ns: ms_to_ns(int(&v, "ts")?, "ts")?,
        recv_ns,
        last_px: opt("lastPrice", inst.px_exp)?,
        mark_px: opt("markPrice", ref_px_exp)?,
        index_px: opt("indexPrice", ref_px_exp)?,
        funding_rate: opt("fundingRate", -8)?,
        next_funding_ns,
        open_interest: opt("openInterest", inst.qty_exp)?,
        best_bid_px: opt("bid1Price", inst.px_exp)?,
        best_bid_qty: opt("bid1Size", inst.qty_exp)?,
        best_ask_px: opt("ask1Price", inst.px_exp)?,
        best_ask_qty: opt("ask1Size", inst.qty_exp)?,
        px_exp: inst.px_exp,
        ref_px_exp,
        qty_exp: inst.qty_exp,
    })])
}
