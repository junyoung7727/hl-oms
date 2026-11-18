//! `GET /v5/market/instruments-info` and its rows (lake contract 3.6).

use md_core::adapter::{Instrument, ParseError, RestRequest};
use md_core::decimal::exponent_of;
use serde_json::Value;

use crate::Market;

pub(crate) const PATH: &str = "/v5/market/instruments-info";

/// Linear and inverse page with `limit` and `cursor`; spot does not page
/// ("Spot does not support pagination, so limit, cursor are invalid").
pub(crate) fn request(market: Market, cursor: Option<&str>) -> RestRequest {
    let mut query = vec![("category".to_string(), market.category().to_string())];
    if market != Market::Spot {
        query.push(("limit".into(), "1000".into()));
        if let Some(c) = cursor {
            query.push(("cursor".into(), c.to_string()));
        }
    }
    RestRequest {
        method: "GET",
        path: PATH.to_string(),
        query,
    }
}

/// The cursor of the next page, `None` on the last page (or for spot).
pub fn next_page_cursor(body: &[u8]) -> Option<String> {
    let v: Value = serde_json::from_slice(body).ok()?;
    let c = v.get("result")?.get("nextPageCursor")?.as_str()?;
    (!c.is_empty()).then(|| c.to_string())
}

fn text<'a>(v: &'a Value, path: &[&str]) -> Result<&'a str, ParseError> {
    let mut cur = v;
    for key in path {
        cur = cur
            .get(key)
            .ok_or_else(|| ParseError::new(format!("missing {}", path.join("."))))?;
    }
    cur.as_str()
        .ok_or_else(|| ParseError::new(format!("{} is not a string", path.join("."))))
}

fn exp(s: &str, field: &str) -> Result<i8, ParseError> {
    exponent_of(s).map_err(|e| ParseError::new(format!("{field} {s:?}: {e:?}")))
}

pub(crate) fn parse(
    market: Market,
    body: &[u8],
    _recv_ns: i64,
) -> Result<Vec<Instrument>, ParseError> {
    let v: Value = serde_json::from_slice(body).map_err(|e| ParseError::new(e.to_string()))?;
    match v.get("retCode").and_then(Value::as_i64) {
        Some(0) => {}
        other => return Err(ParseError::new(format!("retCode {other:?}"))),
    }
    let venue_ts_ns = match v.get("time").and_then(Value::as_i64) {
        Some(ms) => Some(
            ms.checked_mul(1_000_000)
                .ok_or_else(|| ParseError::new("time overflows"))?,
        ),
        None => None,
    };
    let list = v
        .get("result")
        .and_then(|r| r.get("list"))
        .and_then(Value::as_array)
        .ok_or_else(|| ParseError::new("missing result.list"))?;
    // Spot has no quantity step; its quantity precision is basePrecision.
    let lot_field = match market {
        Market::Spot => "basePrecision",
        Market::Linear | Market::Inverse => "qtyStep",
    };
    let optional = |row: &Value, key: &str| row.get(key).and_then(Value::as_str).map(str::to_owned);
    list.iter()
        .map(|row| {
            let tick_size = text(row, &["priceFilter", "tickSize"])?;
            let lot_size = text(row, &["lotSizeFilter", lot_field])?;
            Ok(Instrument {
                symbol: text(row, &["symbol"])?.to_string(),
                status: text(row, &["status"])?.to_string(),
                px_exp: exp(tick_size, "tickSize")?,
                qty_exp: exp(lot_size, lot_field)?,
                tick_size: tick_size.to_string(),
                lot_size: lot_size.to_string(),
                contract_type: optional(row, "contractType"),
                settle_coin: optional(row, "settleCoin"),
                venue_ts_ns,
            })
        })
        .collect()
}
