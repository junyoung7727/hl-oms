//! Hyperliquid wire types and the translation into the core's inputs.
//!
//! Request shapes follow the official Exchange endpoint documentation and the
//! Python SDK's `signing.py`. Field order in the request structs is part of
//! the signed hash and must not change.
//!
//! Translation is deliberately conservative where the documentation is
//! silent or contradicts itself (see docs/research/hyperliquid-protocol.md):
//!
//! - The cumulative filled quantity is never taken from an order's `sz`
//!   field, whose meaning the documentation states both ways.
//! - Any response shape that is not explicitly a placement or an explicit
//!   per-order error is treated as "outcome unknown".

use oms_core::*;
use serde::{Deserialize, Serialize};

// ---- requests --------------------------------------------------------------

#[derive(Clone, Debug, Serialize)]
pub struct LimitWire {
    pub tif: &'static str,
}

#[derive(Clone, Debug, Serialize)]
pub struct OrderTypeWire {
    pub limit: LimitWire,
}

impl OrderTypeWire {
    pub fn limit(tif: &'static str) -> OrderTypeWire {
        OrderTypeWire {
            limit: LimitWire { tif },
        }
    }
}

/// One order. Field order `a, b, p, s, r, t, c` is fixed by the signing scheme.
#[derive(Clone, Debug, Serialize)]
pub struct OrderWire {
    pub a: u32,
    pub b: bool,
    pub p: String,
    pub s: String,
    pub r: bool,
    pub t: OrderTypeWire,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub c: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct OrderAction {
    pub r#type: &'static str,
    pub orders: Vec<OrderWire>,
    pub grouping: &'static str,
}

impl OrderAction {
    pub fn single(order: OrderWire) -> OrderAction {
        OrderAction {
            r#type: "order",
            orders: vec![order],
            grouping: "na",
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct CancelByCloidWire {
    pub asset: u32,
    pub cloid: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct CancelByCloidAction {
    pub r#type: &'static str,
    pub cancels: Vec<CancelByCloidWire>,
}

impl CancelByCloidAction {
    pub fn single(asset: u32, cloid: String) -> CancelByCloidAction {
        CancelByCloidAction {
            r#type: "cancelByCloid",
            cancels: vec![CancelByCloidWire { asset, cloid }],
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct ScheduleCancel {
    pub r#type: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time: Option<u64>,
}

pub fn tif_wire(tif: Tif) -> &'static str {
    match tif {
        Tif::Gtc => "Gtc",
        Tif::Ioc => "Ioc",
        Tif::Alo => "Alo",
    }
}

/// Body of a POST to `/exchange`.
#[derive(Clone, Debug, Serialize)]
pub struct ExchangeRequest<A: Serialize> {
    pub action: A,
    pub nonce: u64,
    pub signature: super::sign::Signature,
    #[serde(rename = "vaultAddress")]
    pub vault_address: Option<String>,
    #[serde(rename = "expiresAfter", skip_serializing_if = "Option::is_none")]
    pub expires_after: Option<u64>,
}

// ---- responses -------------------------------------------------------------

/// What one `/exchange` answer says about a single-order request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlaceAnswer {
    /// `resting`: the order is on the book.
    Resting { oid: u64 },
    /// `filled`: at least `total` traded on arrival.
    Filled { oid: u64, total: Option<Qty> },
    /// A per-order error string: this order was not placed.
    Refused { reason: String },
    /// Top-level `status: "err"`. The SDKs treat it as "the action was not
    /// applied", but the documentation shows no such answer, so whether to
    /// believe it is the caller's (configured) decision.
    ActionError { reason: String },
    /// Anything else. The outcome is not known from this answer.
    Unclear { why: String },
}

pub fn parse_place_answer(body: &serde_json::Value) -> PlaceAnswer {
    match body.get("status").and_then(|s| s.as_str()) {
        Some("ok") => {}
        Some("err") => {
            return PlaceAnswer::ActionError {
                reason: body.get("response").map(text).unwrap_or_default(),
            };
        }
        _ => {
            return PlaceAnswer::Unclear {
                why: "no status field".into(),
            };
        }
    }
    let statuses = body.pointer("/response/data/statuses").and_then(|s| s.as_array());
    let Some([one]) = statuses.map(|v| v.as_slice()) else {
        return PlaceAnswer::Unclear {
            why: "expected exactly one order status".into(),
        };
    };
    if let Some(oid) = one.pointer("/resting/oid").and_then(|o| o.as_u64()) {
        return PlaceAnswer::Resting { oid };
    }
    if let Some(f) = one.get("filled") {
        return match f.get("oid").and_then(|o| o.as_u64()) {
            Some(oid) => PlaceAnswer::Filled {
                oid,
                total: f.get("totalSz").and_then(|s| s.as_str()).and_then(|s| Qty::parse(s).ok()),
            },
            None => PlaceAnswer::Unclear {
                why: "filled without an order id".into(),
            },
        };
    }
    if let Some(e) = one.get("error") {
        return PlaceAnswer::Refused { reason: text(e) };
    }
    PlaceAnswer::Unclear {
        why: format!("unrecognised order status {one}"),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CancelAnswer {
    Success,
    Refused { reason: String },
    Unclear { why: String },
}

pub fn parse_cancel_answer(body: &serde_json::Value) -> CancelAnswer {
    match body.get("status").and_then(|s| s.as_str()) {
        Some("ok") => {}
        Some("err") => {
            return CancelAnswer::Refused {
                reason: body.get("response").map(text).unwrap_or_default(),
            };
        }
        _ => {
            return CancelAnswer::Unclear {
                why: "no status field".into(),
            };
        }
    }
    let statuses = body.pointer("/response/data/statuses").and_then(|s| s.as_array());
    match statuses.map(|v| v.as_slice()) {
        Some([s]) if s.as_str() == Some("success") => CancelAnswer::Success,
        Some([s]) if s.get("error").is_some() => CancelAnswer::Refused {
            reason: s.get("error").map(text).unwrap_or_default(),
        },
        _ => CancelAnswer::Unclear {
            why: "unrecognised cancel status".into(),
        },
    }
}

fn text(v: &serde_json::Value) -> String {
    v.as_str().map(String::from).unwrap_or_else(|| v.to_string())
}

// ---- stream and query payloads ---------------------------------------------

#[derive(Clone, Debug, Deserialize)]
pub struct HlOrder {
    pub coin: String,
    pub side: String,
    pub oid: u64,
    #[serde(default)]
    pub cloid: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct HlOrderUpdate {
    pub order: HlOrder,
    pub status: String,
    #[serde(rename = "statusTimestamp")]
    pub status_timestamp: u64,
}

#[derive(Clone, Debug, Deserialize)]
pub struct HlFill {
    pub coin: String,
    pub px: String,
    pub sz: String,
    pub side: String,
    pub time: u64,
    pub oid: u64,
    pub tid: u64,
    #[serde(default)]
    pub fee: Option<String>,
    #[serde(default)]
    pub cloid: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct HlOpenOrder {
    pub coin: String,
    pub side: String,
    pub oid: u64,
    #[serde(default)]
    pub cloid: Option<String>,
}

pub fn side(s: &str) -> Option<Side> {
    match s {
        "B" => Some(Side::Buy),
        "A" => Some(Side::Sell),
        _ => None,
    }
}

pub fn venue_oid(oid: u64) -> Option<VenueOrderId> {
    VenueOrderId::new(oid.to_string()).ok()
}

/// The order of this instance that a venue client id refers to, if any.
pub fn our_order(cloid: Option<&str>, instance_id: u64) -> Option<OrderId> {
    cloid
        .and_then(VenueClientId::parse_hex)
        .filter(|c| c.instance_id == instance_id)
        .map(|c| c.order_id)
}

/// What an order status string from the venue means to the core.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StatusMeaning {
    Known(StatusIn),
    /// Documented, and nothing the core acts on (`triggered`).
    Ignored,
    /// Not in the documented table. Nothing is concluded from it: the order
    /// keeps its state and the open-order sweep and the operator decide.
    Undocumented,
}

/// Maps the documented order status strings to the core's view.
///
/// Every terminal status other than an explicit rejection is reported as a
/// cancel without a final quantity: the booked fills decide how much traded,
/// and an order whose fills reach its quantity is closed as filled.
pub fn status(s: &str, ts_ms: u64) -> StatusMeaning {
    let venue_ts = ts_ms.saturating_mul(1000);
    let st = match s {
        "open" => VenueStatus::Open,
        "triggered" => return StatusMeaning::Ignored,
        "rejected" => VenueStatus::Rejected { reason: s.into() },
        // tickRejected, perpMarginRejected, badAloPxRejected, …
        other if other.ends_with("Rejected") => VenueStatus::Rejected { reason: other.into() },
        // marginCanceled, selfTradeCanceled, liquidatedCanceled, …
        "filled" | "canceled" | "scheduledCancel" => VenueStatus::Canceled { reason: s.into() },
        other if other.ends_with("Canceled") => VenueStatus::Canceled { reason: other.into() },
        _ => return StatusMeaning::Undocumented,
    };
    StatusMeaning::Known(StatusIn {
        status: st,
        cum: None,
        venue_ts,
    })
}

/// Coin names that do not fit the identifier rules are mapped, not dropped,
/// so that a position, an open order or a fill on such a coin is still
/// reported.
pub fn instrument_id(coin: &str) -> Option<InstrumentId> {
    let cleaned: String = coin
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || "_-.:".contains(c) { c } else { '_' })
        .take(32)
        .collect();
    InstrumentId::new(cleaned).ok()
}

/// A fill's identity. The documentation says `tid` alone is not globally
/// unique and recommends `(block_time, coin, tid)`. The order id is added so
/// that the two sides of one trade, should both be ours, stay distinct.
pub fn fill_key(f: &HlFill) -> Option<FillKey> {
    let coin = instrument_id(&f.coin)?;
    FillKey::new(format!("{}:{}:{}:{}", f.time, coin, f.tid, f.oid)).ok()
}

pub fn fill_input(f: &HlFill, instance_id: u64) -> Option<FillIn> {
    Some(FillIn {
        key: fill_key(f)?,
        order_id: our_order(f.cloid.as_deref(), instance_id),
        venue_oid: venue_oid(f.oid),
        instrument: instrument_id(&f.coin)?,
        side: side(&f.side)?,
        qty: Qty::parse(&f.sz).ok()?,
        px: Px::parse(&f.px).ok()?,
        fee: f.fee.as_deref().and_then(|s| Amount::parse(s).ok()).unwrap_or(Amount::ZERO),
        venue_ts: f.time.saturating_mul(1000),
    })
}

// ---- margin queries (C-MARGIN) ----------------------------------------------

/// Turns the answer to a `clearinghouseState` query into the core's input:
/// a whole margin report, or a refusal with its reason. Never a part of one.
pub fn margin_input(body: &serde_json::Value, session: u64, seq: u64, queried_at: Ts) -> VenueInput {
    let amount = |v: Option<&serde_json::Value>| {
        v.and_then(|v| v.as_str())
            .and_then(|s| Amount::parse(s).ok())
            .unwrap_or(Amount::ZERO)
    };
    let whole = |v: Option<&serde_json::Value>| {
        v.and_then(|v| v.as_u64())
            .and_then(|n| u32::try_from(n).ok())
            .unwrap_or(0)
    };
    let list = body.get("assetPositions").and_then(|a| a.as_array());
    let positions = list
        .into_iter()
        .flatten()
        .filter_map(|p| {
            let pos = p.get("position")?;
            Some(MarginPosition {
                instrument: instrument_id(pos.get("coin")?.as_str()?)?,
                szi: SQty::parse(pos.get("szi")?.as_str()?).ok()?,
                position_value: amount(pos.get("positionValue")),
                margin_used: amount(pos.get("marginUsed")),
                leverage_type: match pos.pointer("/leverage/type").and_then(|t| t.as_str()) {
                    Some("isolated") => LeverageType::Isolated,
                    _ => LeverageType::Cross,
                },
                leverage_value: whole(pos.pointer("/leverage/value")),
                max_leverage: whole(pos.get("maxLeverage")),
            })
        })
        .collect();
    VenueInput::MarginReport(MarginReportIn {
        session,
        seq,
        queried_at,
        account_value: amount(body.pointer("/marginSummary/accountValue")),
        positions,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The example answer of the Info endpoint documentation (Perpetuals,
    /// `clearinghouseState`), as recorded in the research note of
    /// 2026-11-15. It names no account.
    const DOC_CLEARINGHOUSE_STATE: &str = r#"{
      "assetPositions": [
        {
          "position": {
            "coin": "ETH",
            "cumFunding": {"allTime": "514.085417", "sinceChange": "0.0", "sinceOpen": "0.0"},
            "entryPx": "2986.3",
            "leverage": {"rawUsd": "-95.059824", "type": "isolated", "value": 20},
            "liquidationPx": "2866.26936529",
            "marginUsed": "4.967826",
            "maxLeverage": 50,
            "positionValue": "100.02765",
            "returnOnEquity": "-0.0026789",
            "szi": "0.0335",
            "unrealizedPnl": "-0.0134"
          },
          "type": "oneWay"
        }
      ],
      "crossMaintenanceMarginUsed": "0.0",
      "crossMarginSummary": {
        "accountValue": "13104.514502",
        "totalMarginUsed": "0.0",
        "totalNtlPos": "0.0",
        "totalRawUsd": "13104.514502"
      },
      "marginSummary": {
        "accountValue": "13109.482328",
        "totalMarginUsed": "4.967826",
        "totalNtlPos": "100.02765",
        "totalRawUsd": "13009.454678"
      },
      "time": 1708622398623,
      "withdrawable": "13104.514502"
    }"#;

    fn doc_state() -> serde_json::Value {
        serde_json::from_str(DOC_CLEARINGHOUSE_STATE).unwrap()
    }

    /// S25. Every field of the documented example lands in its place. The
    /// expected numbers are the documented decimals written by hand as
    /// integers of 1e-8: the account value is the one of `marginSummary`
    /// (cross and isolated together), not the cross-only one.
    #[test]
    fn documented_clearinghouse_state_parses_to_a_margin_report() {
        assert_eq!(
            margin_input(&doc_state(), 7, 3, 1_700_000_000_000_000),
            VenueInput::MarginReport(MarginReportIn {
                session: 7,
                seq: 3,
                queried_at: 1_700_000_000_000_000,
                account_value: Amount(1_310_948_232_800),
                positions: vec![MarginPosition {
                    instrument: InstrumentId::new("ETH").unwrap(),
                    szi: SQty(3_350_000),
                    position_value: Amount(10_002_765_000),
                    margin_used: Amount(496_782_600),
                    leverage_type: LeverageType::Isolated,
                    leverage_value: 20,
                    max_leverage: 50,
                }],
            })
        );
    }

    /// Response examples from the Exchange endpoint documentation.
    #[test]
    fn documented_place_answers() {
        let resting = json!({"status":"ok","response":{"type":"order","data":{"statuses":[{"resting":{"oid":77738308}}]}}});
        assert_eq!(parse_place_answer(&resting), PlaceAnswer::Resting { oid: 77738308 });

        let filled = json!({"status":"ok","response":{"type":"order","data":{"statuses":[{"filled":{"totalSz":"0.02","avgPx":"1891.4","oid":77747314}}]}}});
        assert_eq!(parse_place_answer(&filled), PlaceAnswer::Filled {
            oid: 77747314,
            total: Some(Qty::parse("0.02").unwrap())
        });

        let error = json!({"status":"ok","response":{"type":"order","data":{"statuses":[{"error":"Order must have minimum value of $10."}]}}});
        assert_eq!(parse_place_answer(&error), PlaceAnswer::Refused {
            reason: "Order must have minimum value of $10.".into()
        });
    }

    #[test]
    fn anything_else_is_unclear_not_a_failure() {
        // A top-level error is its own case: the gateway decides, by
        // configuration, whether it proves that nothing was placed.
        let err = json!({"status":"err","response":"L1 error: User or API Wallet 0x0123 does not exist."});
        assert!(matches!(parse_place_answer(&err), PlaceAnswer::ActionError { .. }));
        // Shapes the documentation does not show must never be read as
        // "not placed".
        for body in [
            json!({}),
            json!({"status":"ok"}),
            json!({"status":"ok","response":{"type":"order","data":{"statuses":[]}}}),
            json!({"status":"ok","response":{"type":"order","data":{"statuses":[{"resting":{"oid":1}},{"resting":{"oid":2}}]}}}),
            json!({"status":"ok","response":{"type":"order","data":{"statuses":["waitingForFill"]}}}),
            json!({"status":"ok","response":{"type":"order","data":{"statuses":[{"filled":{"totalSz":"1"}}]}}}),
        ] {
            assert!(matches!(parse_place_answer(&body), PlaceAnswer::Unclear { .. }), "{body}");
        }
    }

    #[test]
    fn documented_cancel_answers() {
        let ok = json!({"status":"ok","response":{"type":"cancel","data":{"statuses":["success"]}}});
        assert_eq!(parse_cancel_answer(&ok), CancelAnswer::Success);
        let gone = json!({"status":"ok","response":{"type":"cancel","data":{"statuses":[{"error":"Order was never placed, already canceled, or filled."}]}}});
        assert_eq!(parse_cancel_answer(&gone), CancelAnswer::Refused {
            reason: "Order was never placed, already canceled, or filled.".into()
        });
        assert!(matches!(parse_cancel_answer(&json!({"status":"ok"})), CancelAnswer::Unclear { .. }));
    }

    /// Status names from the documented table for `orderStatus`.
    #[test]
    fn status_mapping() {
        let known = |s: &str| match status(s, 5) {
            StatusMeaning::Known(st) => st,
            other => panic!("{s}: {other:?}"),
        };
        assert_eq!(known("open").status, VenueStatus::Open);
        assert_eq!(known("open").venue_ts, 5000);
        for s in [
            "filled", "canceled", "marginCanceled", "vaultWithdrawalCanceled", "openInterestCapCanceled", "selfTradeCanceled",
            "reduceOnlyCanceled", "siblingFilledCanceled", "delistedCanceled", "liquidatedCanceled", "scheduledCancel",
        ] {
            let st = known(s);
            assert!(matches!(st.status, VenueStatus::Canceled { .. }), "{s}");
            assert_eq!(st.cum, None, "no final quantity is claimed");
        }
        for s in [
            "rejected", "tickRejected", "minTradeNtlRejected", "perpMarginRejected", "reduceOnlyRejected", "badAloPxRejected",
            "iocCancelRejected", "badTriggerPxRejected", "marketOrderNoLiquidityRejected",
            "positionIncreaseAtOpenInterestCapRejected", "positionFlipAtOpenInterestCapRejected",
            "tooAggressiveAtOpenInterestCapRejected", "openInterestIncreaseRejected", "insufficientSpotBalanceRejected",
            "oracleRejected", "perpMaxPositionRejected",
        ] {
            assert!(matches!(known(s).status, VenueStatus::Rejected { .. }), "{s}");
        }
        assert_eq!(status("triggered", 1), StatusMeaning::Ignored);
        // A status that is not in the documented table proves nothing: it is
        // neither terminal nor open.
        assert_eq!(status("somethingNew", 1), StatusMeaning::Undocumented);
        assert_eq!(status("", 1), StatusMeaning::Undocumented);
    }

    #[test]
    fn odd_coin_names_are_mapped_not_dropped() {
        assert_eq!(instrument_id("BTC").unwrap().as_str(), "BTC");
        assert_eq!(instrument_id("@107").unwrap().as_str(), "_107");
        assert_eq!(instrument_id("PURR/USDC").unwrap().as_str(), "PURR_USDC");
        assert_eq!(instrument_id("dex:XYZ").unwrap().as_str(), "dex:XYZ");
        // A fill on such a coin is still translatable.
        let f: HlFill = serde_json::from_value(json!({
            "coin": "@107", "px": "25.1", "sz": "1.5", "side": "B", "time": 1700000000123u64,
            "oid": 91490942u64, "tid": 118906512037719u64, "fee": "0.01"
        }))
        .unwrap();
        assert_eq!(fill_input(&f, 7).unwrap().instrument.as_str(), "_107");
    }

    #[test]
    fn fill_translation() {
        let f: HlFill = serde_json::from_value(json!({
            "coin":"BTC","px":"70000.5","sz":"0.001","side":"B","time":1700000000123u64,
            "startPosition":"0.0","dir":"Open Long","closedPnl":"0.0","hash":"0xabc","oid":91490942u64,
            "crossed":true,"fee":"0.0315","tid":118906512037719u64,"feeToken":"USDC"
        }))
        .unwrap();
        let i = fill_input(&f, 7).unwrap();
        assert_eq!(i.key.as_str(), "1700000000123:BTC:118906512037719:91490942");
        assert_eq!(i.order_id, None);
        assert_eq!(i.venue_oid.unwrap().as_str(), "91490942");
        assert_eq!((i.side, i.qty, i.px), (Side::Buy, Qty::parse("0.001").unwrap(), Px::parse("70000.5").unwrap()));
        assert_eq!(i.fee, Amount::parse("0.0315").unwrap());
        assert_eq!(i.venue_ts, 1_700_000_000_123_000);

        // A client id of this instance is recognised; another instance's is not.
        let ours = VenueClientId { instance_id: 7, order_id: OrderId(42) }.to_hex();
        assert_eq!(our_order(Some(&ours), 7), Some(OrderId(42)));
        assert_eq!(our_order(Some(&ours), 8), None);
        assert_eq!(our_order(None, 7), None);
    }

    /// The exact JSON an order request serialises to.
    #[test]
    fn order_wire_json() {
        let action = OrderAction::single(OrderWire {
            a: 3,
            b: false,
            p: Px::parse("70000.5").unwrap().to_string(),
            s: Qty::parse("0.00100").unwrap().to_string(),
            r: true,
            t: OrderTypeWire::limit(tif_wire(Tif::Alo)),
            c: Some("0x00000000000000070000000000000001".into()),
        });
        assert_eq!(
            serde_json::to_string(&action).unwrap(),
            r#"{"type":"order","orders":[{"a":3,"b":false,"p":"70000.5","s":"0.001","r":true,"t":{"limit":{"tif":"Alo"}},"c":"0x00000000000000070000000000000001"}],"grouping":"na"}"#
        );
    }
}
