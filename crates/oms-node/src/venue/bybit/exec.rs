//! The execution table (ADR-028 §7, ADR-023): every execution row, from the
//! private stream or from `/v5/execution/list`, becomes one venue input
//! through `classify`, and only through it.
//!
//! | `execType` | input |
//! |---|---|
//! | `Trade` | `Fill` (ours if `orderLinkId` decodes to this instance; otherwise an external fill) |
//! | `Funding` | `Funding` (cash only, never a fill) |
//! | `AdlTrade` | `PositionChange(adl)` |
//! | `BustTrade` | `PositionChange(takeover)` |
//! | `Delivery` | `PositionChange(delivery)` |
//! | `Settle` | `PositionChange(settle)` |
//! | anything else, or none | `UnclassifiedExecution` (the core holds plain orders) |
//!
//! The table is the default until the testnet shows how the non-trade rows
//! look (design TN T15).

use oms_core::*;
use serde_json::Value;

/// The form of our `orderLinkId` (ADR-022).
pub const WIRE_ID: WireIdForm = WireIdForm::Base32Pair;

/// Longest `execType` passed on to the journal. The documented values have
/// at most 18 letters.
const EXEC_TYPE_MAX_LEN: usize = 32;

/// The gateway's key of an execution (C-GW G-8): `<symbol>:<execId>`. The
/// docs do not say that `execId` is unique across symbols, so the symbol is
/// part of the key.
pub fn fill_key(symbol: &str, exec_id: &str) -> Option<FillKey> {
    FillKey::new(format!("{symbol}:{exec_id}")).ok()
}

/// The order of this instance that a venue `orderLinkId` names, if any. An
/// id in another form, or of another instance, is not ours.
pub fn our_order(order_link_id: &str, instance_id: u64) -> Option<OrderId> {
    WIRE_ID
        .decode(order_link_id)
        .filter(|c| c.instance_id == instance_id)
        .map(|c| c.order_id)
}

pub fn text<'a>(row: &'a Value, name: &str) -> Result<&'a str, String> {
    row.get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{name} is missing or not a string"))
}

pub fn number<T>(
    row: &Value,
    name: &str,
    parse: fn(&str) -> Result<T, num::NumError>,
) -> Result<T, String> {
    let s = text(row, name)?;
    parse(s).map_err(|e| format!("{name} {s:?}: {e}"))
}

/// A venue time in ms (a decimal string) as a core time (µs).
pub fn venue_ms(row: &Value, name: &str) -> Result<Ts, String> {
    let s = text(row, name)?;
    let ms: u64 = s
        .parse()
        .map_err(|_| format!("{name} {s:?} is not a time in ms"))?;
    ms.checked_mul(1000)
        .ok_or_else(|| format!("{name} {s:?} is out of range"))
}

pub fn side(row: &Value) -> Result<Side, String> {
    match text(row, "side")? {
        "Buy" => Ok(Side::Buy),
        "Sell" => Ok(Side::Sell),
        other => Err(format!("side {other:?} is neither Buy nor Sell")),
    }
}

pub fn instrument(row: &Value) -> Result<InstrumentId, String> {
    let s = text(row, "symbol")?;
    InstrumentId::new(s).map_err(|e| format!("symbol {s:?}: {e}"))
}

/// The venue order id of a row; an empty one is none.
pub fn venue_oid(row: &Value) -> Option<VenueOrderId> {
    row.get("orderId")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .and_then(|s| VenueOrderId::new(s).ok())
}

/// One execution row as the core's input. `Err` if the row cannot be read:
/// the caller must not count it as delivered (C-GW G-9). An execution type
/// outside the table is not an error; it is `UnclassifiedExecution`.
pub fn classify(row: &Value, instance_id: u64, recv_ns: Option<i64>) -> Result<VenueInput, String> {
    let symbol = text(row, "symbol")?;
    let exec_id = text(row, "execId")?;
    let key = fill_key(symbol, exec_id)
        .ok_or_else(|| format!("no fill key from {symbol:?} and execId {exec_id:?}"))?;
    let instrument = instrument(row)?;
    let venue_ts = venue_ms(row, "execTime")?;
    let exec_type = row.get("execType").and_then(Value::as_str);
    let change = |kind: VenueChangeKind| -> Result<VenueInput, String> {
        let qty = number(row, "execQty", Qty::parse)?;
        Ok(VenueInput::PositionChange(VenuePositionChangeIn {
            key: key.clone(),
            kind,
            instrument: instrument.clone(),
            qty: qty.signed(side(row)?),
            px: number(row, "execPrice", Px::parse)?,
            venue_ts,
        }))
    };
    match exec_type {
        Some("Trade") => Ok(VenueInput::Fill(FillIn {
            order_id: our_order(text(row, "orderLinkId").unwrap_or(""), instance_id),
            venue_oid: venue_oid(row),
            side: side(row)?,
            qty: number(row, "execQty", Qty::parse)?,
            px: number(row, "execPrice", Px::parse)?,
            // Positive is a cost (F20 Q9.5: the venue's sign for a paid fee).
            fee: number(row, "execFee", Amount::parse)?,
            is_maker: row.get("isMaker").and_then(Value::as_bool),
            recv_ns,
            key,
            instrument,
            venue_ts,
        })),
        // execFee of a funding row: positive means the account pays (F20
        // Q9.5, the transaction log's sign is the opposite), the sign of
        // `FundingIn::amount`.
        Some("Funding") => Ok(VenueInput::Funding(FundingIn {
            amount: number(row, "execFee", Amount::parse)?,
            rate: text(row, "feeRate")
                .unwrap_or("")
                .chars()
                .take(32)
                .collect(),
            key,
            instrument,
            venue_ts,
        })),
        Some("AdlTrade") => change(VenueChangeKind::Adl),
        Some("BustTrade") => change(VenueChangeKind::Takeover),
        Some("Delivery") => change(VenueChangeKind::Delivery),
        Some("Settle") => change(VenueChangeKind::Settle),
        other => {
            let field = |n: &str| {
                row.get(n)
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .chars()
                    .take(64)
                    .collect::<String>()
            };
            Ok(VenueInput::UnclassifiedExecution {
                exec_type: match other {
                    Some(t) => t.chars().take(EXEC_TYPE_MAX_LEN).collect(),
                    None => "(none)".into(),
                },
                detail: format!(
                    "side={} execQty={} execPrice={} execFee={} orderId={}",
                    field("side"),
                    field("execQty"),
                    field("execPrice"),
                    field("execFee"),
                    field("orderId")
                ),
                key,
                instrument,
                venue_ts,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const INSTANCE: u64 = 7;

    /// A row shaped like the example of `/v5/execution/list` (F20 Q9),
    /// with our order id.
    fn row(exec_type: &str) -> Value {
        json!({
            "symbol": "BTCUSDT", "orderId": "fd4300ae-7847-404e-b947-b46980a4d140",
            "orderLinkId": WIRE_ID.encode(VenueClientId { instance_id: INSTANCE, order_id: OrderId(42) }),
            "side": "Sell", "orderPrice": "60000", "orderQty": "0.002", "leavesQty": "0",
            "orderType": "Limit", "execFee": "0.071409", "execId": "e0cbe81d-0f18-5866-9415-cf319b5dab3b",
            "execPrice": "59508", "execQty": "0.002", "execType": exec_type, "execValue": "119.016",
            "execTime": "1672364174443", "isMaker": false, "feeRate": "0.0006", "seq": 4688002127u64
        })
    }

    fn with(mut v: Value, k: &str, x: Value) -> Value {
        v[k] = x;
        v
    }

    #[test]
    fn a_trade_of_our_order_is_a_fill_of_that_order() {
        let VenueInput::Fill(f) = classify(&row("Trade"), INSTANCE, Some(5)).unwrap() else {
            panic!()
        };
        assert_eq!(
            f.key.as_str(),
            "BTCUSDT:e0cbe81d-0f18-5866-9415-cf319b5dab3b"
        );
        assert_eq!(f.order_id, Some(OrderId(42)));
        assert_eq!(
            f.venue_oid.unwrap().as_str(),
            "fd4300ae-7847-404e-b947-b46980a4d140"
        );
        assert_eq!(
            (f.side, f.qty, f.px, f.fee),
            (
                Side::Sell,
                Qty::parse("0.002").unwrap(),
                Px::parse("59508").unwrap(),
                Amount::parse("0.071409").unwrap()
            )
        );
        assert_eq!(
            (f.venue_ts, f.is_maker, f.recv_ns),
            (1_672_364_174_443_000, Some(false), Some(5))
        );
        assert_eq!(f.instrument.as_str(), "BTCUSDT");
    }

    #[test]
    fn a_trade_of_an_order_that_is_not_ours_is_an_external_fill() {
        let other = WIRE_ID.encode(VenueClientId {
            instance_id: INSTANCE + 1,
            order_id: OrderId(42),
        });
        for link in [
            json!(""),
            json!(other),
            json!("my-manual-order"),
            Value::Null,
        ] {
            let VenueInput::Fill(f) = classify(
                &with(row("Trade"), "orderLinkId", link.clone()),
                INSTANCE,
                None,
            )
            .unwrap() else {
                panic!("{link}")
            };
            assert_eq!(f.order_id, None, "{link}");
            assert!(f.venue_oid.is_some());
        }
    }

    #[test]
    fn a_funding_row_is_cash_and_never_a_fill() {
        let r = with(
            with(row("Funding"), "execFee", json!("-0.0123")),
            "orderType",
            json!("UNKNOWN"),
        );
        assert_eq!(
            classify(&r, INSTANCE, None).unwrap(),
            VenueInput::Funding(FundingIn {
                key: fill_key("BTCUSDT", "e0cbe81d-0f18-5866-9415-cf319b5dab3b").unwrap(),
                instrument: InstrumentId::new("BTCUSDT").unwrap(),
                amount: Amount::parse("-0.0123").unwrap(),
                rate: "0.0006".into(),
                venue_ts: 1_672_364_174_443_000,
            })
        );
    }

    #[test]
    fn each_venue_made_execution_type_is_a_position_change_of_its_kind() {
        for (t, kind) in [
            ("AdlTrade", VenueChangeKind::Adl),
            ("BustTrade", VenueChangeKind::Takeover),
            ("Delivery", VenueChangeKind::Delivery),
            ("Settle", VenueChangeKind::Settle),
        ] {
            let VenueInput::PositionChange(c) = classify(&row(t), INSTANCE, None).unwrap() else {
                panic!("{t}")
            };
            assert_eq!(c.kind, kind, "{t}");
            // Sell reduces a long: negative.
            assert_eq!(c.qty, SQty::parse("-0.002").unwrap(), "{t}");
            assert_eq!(c.px, Px::parse("59508").unwrap(), "{t}");
            let VenueInput::PositionChange(b) =
                classify(&with(row(t), "side", json!("Buy")), INSTANCE, None).unwrap()
            else {
                panic!()
            };
            assert_eq!(b.qty, SQty::parse("0.002").unwrap(), "{t}");
        }
    }

    #[test]
    fn every_other_execution_type_is_unclassified() {
        let long = "X".repeat(100);
        for t in [
            json!("BlockTrade"),
            json!("MovePosition"),
            json!("FutureSpread"),
            json!("ForwardSplitSettle"),
            json!("ReverseSplitOpen"),
            json!("UNKNOWN"),
            json!("trade"),
            json!(long),
            Value::Null,
            json!(3),
        ] {
            let r = with(row("Trade"), "execType", t.clone());
            let VenueInput::UnclassifiedExecution {
                key,
                exec_type,
                detail,
                ..
            } = classify(&r, INSTANCE, None).unwrap()
            else {
                panic!("{t}")
            };
            assert_eq!(key.as_str(), "BTCUSDT:e0cbe81d-0f18-5866-9415-cf319b5dab3b");
            assert!(
                exec_type.len() <= EXEC_TYPE_MAX_LEN && !exec_type.is_empty(),
                "{t}"
            );
            assert!(detail.contains("execQty=0.002"), "{detail}");
        }
    }

    #[test]
    fn a_row_that_cannot_be_read_is_an_error_not_an_input() {
        for (k, v) in [
            ("execId", Value::Null),
            ("symbol", json!("BTC USDT")),
            ("execTime", json!("soon")),
            ("execQty", json!("")),
            ("execPrice", json!("1e3")),
            ("execFee", Value::Null),
            ("side", json!("")),
        ] {
            assert!(
                classify(&with(row("Trade"), k, v.clone()), INSTANCE, None).is_err(),
                "Trade {k}={v}"
            );
        }
        assert!(classify(&with(row("Funding"), "execFee", json!("x")), INSTANCE, None).is_err());
        assert!(
            classify(
                &with(row("AdlTrade"), "side", json!("None")),
                INSTANCE,
                None
            )
            .is_err()
        );
    }
}
