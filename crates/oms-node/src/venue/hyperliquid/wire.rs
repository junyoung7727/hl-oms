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
    match margin_report(body) {
        Ok((account_value, positions)) => VenueInput::MarginReport(MarginReport {
            session,
            seq,
            queried_at,
            account_value,
            positions,
        }),
        Err(reason) => VenueInput::MarginReportInvalid {
            session,
            seq,
            queried_at,
            reason,
        },
    }
}

/// The account value and every position of a `clearinghouseState` answer,
/// or the first thing in it that cannot be read. Nothing is defaulted and
/// no position is skipped: a rule must never judge half an answer.
fn margin_report(body: &serde_json::Value) -> Result<(Amount, Vec<MarginPosition>), String> {
    // `marginSummary` covers cross and isolated margin together.
    let account_value = decimal(body.pointer("/marginSummary/accountValue"), "marginSummary.accountValue", Amount::parse)?;
    let list = body
        .get("assetPositions")
        .and_then(|a| a.as_array())
        .ok_or("assetPositions is missing or not a list")?;
    let mut positions = Vec::with_capacity(list.len());
    for (i, p) in list.iter().enumerate() {
        let pos = p.get("position").ok_or(format!("assetPositions[{i}]: position is missing"))?;
        // A coin that is not configured here is reported like any other.
        let coin = pos.get("coin").and_then(|c| c.as_str());
        let (Some(coin), Some(instrument)) = (coin, coin.and_then(instrument_id)) else {
            return Err(format!("assetPositions[{i}]: coin is missing or unusable"));
        };
        let at = |e: String| format!("assetPositions[{i}] ({coin}): {e}");
        let qty = decimal(pos.get("szi"), "szi", SQty::parse).map_err(at)?;
        let position_value = decimal(pos.get("positionValue"), "positionValue", Amount::parse).map_err(at)?;
        let margin_used = decimal(pos.get("marginUsed"), "marginUsed", Amount::parse).map_err(at)?;
        let leverage_type = leverage_type(pos.pointer("/leverage/type")).map_err(at)?;
        let leverage_value = whole(pos.pointer("/leverage/value"), "leverage.value").map_err(at)?;
        // The maintenance margin rate is 1 / (2 x maxLeverage): a report
        // without it cannot be judged, and zero would divide by nothing.
        let max_leverage = whole(pos.get("maxLeverage"), "maxLeverage").map_err(at)?;
        if max_leverage == 0 {
            return Err(at("maxLeverage is zero".into()));
        }
        positions.push(MarginPosition {
            instrument,
            qty,
            position_value,
            margin_used,
            leverage_type,
            leverage_value,
            max_leverage,
        });
    }
    Ok((account_value, positions))
}

/// The margin mode in a `leverage` object of the venue: the string
/// `isolated` or `cross`, exactly. Anything else is not read as either.
fn leverage_type(v: Option<&serde_json::Value>) -> Result<LeverageType, String> {
    let v = v.ok_or("leverage.type is missing")?;
    match v.as_str() {
        Some("isolated") => Ok(LeverageType::Isolated),
        Some("cross") => Ok(LeverageType::Cross),
        _ => Err(format!("leverage.type is neither isolated nor cross: {v}")),
    }
}

/// A decimal the venue sends as a string. A JSON number is not accepted in
/// its place: it may already have lost digits.
fn decimal<T, E>(v: Option<&serde_json::Value>, name: &str, parse: fn(&str) -> Result<T, E>) -> Result<T, String> {
    let v = v.ok_or(format!("{name} is missing"))?;
    v.as_str()
        .and_then(|s| parse(s).ok())
        .ok_or(format!("{name} is not a decimal string: {v}"))
}

/// A whole number the venue sends as a JSON integer.
fn whole(v: Option<&serde_json::Value>, name: &str) -> Result<u32, String> {
    let v = v.ok_or(format!("{name} is missing"))?;
    v.as_u64()
        .and_then(|n| u32::try_from(n).ok())
        .ok_or(format!("{name} is not a whole number: {v}"))
}

/// Turns the answer to an `activeAssetData` query about `instrument` into
/// the core's input, or into nothing: a setting that cannot be read stays
/// unknown, and an unknown setting holds the instrument.
pub fn venue_setting_input(body: &serde_json::Value, session: u64, instrument: &InstrumentId) -> Option<VenueInput> {
    // The answer names the coin it is about. One that names another coin,
    // or none, is not taken as this instrument's setting.
    if body.get("coin").and_then(|c| c.as_str()) != Some(instrument.as_str()) {
        return None;
    }
    let isolated = leverage_type(body.pointer("/leverage/type")).ok()? == LeverageType::Isolated;
    let leverage = whole(body.pointer("/leverage/value"), "leverage.value").ok()?;
    Some(VenueInput::VenueSetting {
        session,
        instrument: instrument.clone(),
        isolated,
        leverage,
    })
}

/// Longest account mode passed on. The documented values have at most 15
/// letters; the value is journaled, so it is bounded.
const ACCOUNT_MODE_MAX_LEN: usize = 64;

/// Turns the answer to a `userAbstraction` query into the core's input.
///
/// The answer is one JSON string. It is passed on exactly as it came: this
/// gateway does not decide which mode is acceptable and never rewrites a
/// value it does not know into one it knows. The core accepts a mode only
/// if the allow-list spells it the same way, so an unknown value holds the
/// account. An answer that is not a string gives no input.
pub fn account_mode_input(body: &serde_json::Value, session: u64) -> Option<VenueInput> {
    let mode = body.as_str()?;
    if mode.len() > ACCOUNT_MODE_MAX_LEN {
        return None;
    }
    Some(VenueInput::AccountMode {
        session,
        mode: mode.to_string(),
    })
}

/// Weight of a `clearinghouseState` query against the venue's limit of
/// 1200 per minute and IP (documented: weight 2). The margin query and the
/// position query of the sweep are this request.
pub const WEIGHT_MARGIN_QUERY: u32 = 2;
/// Weight of an `orderStatus` query (documented: weight 2).
pub const WEIGHT_ORDER_STATUS_QUERY: u32 = 2;
/// Weight of an `activeAssetData`, `userAbstraction`, `frontendOpenOrders`
/// or `userFillsByTime` query. None is in the documented list of weight-2
/// queries, so the documented default for every other info request
/// applies.
pub const WEIGHT_OTHER_INFO_QUERY: u32 = 20;

/// Body of the margin query (`/info`). The optional `dex` field is left
/// out: the first perp dex.
pub fn margin_request(account: &str) -> serde_json::Value {
    serde_json::json!({"type": "clearinghouseState", "user": account})
}

/// Body of the venue-setting query for one instrument (`/info`). The venue
/// calls the instrument a coin. Its name there is the instrument id: the
/// gateway does not start a link unless every configured instrument id is,
/// letter for letter, a name of the venue's universe (`load_meta`), and
/// orders find their asset by the same id.
pub fn venue_setting_request(account: &str, instrument: &InstrumentId) -> serde_json::Value {
    serde_json::json!({"type": "activeAssetData", "user": account, "coin": instrument.as_str()})
}

/// Body of the account-mode query (`/info`).
pub fn account_mode_request(account: &str) -> serde_json::Value {
    serde_json::json!({"type": "userAbstraction", "user": account})
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
            VenueInput::MarginReport(MarginReport {
                session: 7,
                seq: 3,
                queried_at: 1_700_000_000_000_000,
                account_value: Amount(1_310_948_232_800),
                positions: vec![MarginPosition {
                    instrument: InstrumentId::new("ETH").unwrap(),
                    qty: SQty(3_350_000),
                    position_value: Amount(10_002_765_000),
                    margin_used: Amount(496_782_600),
                    leverage_type: LeverageType::Isolated,
                    leverage_value: 20,
                    max_leverage: 50,
                }],
            })
        );
    }

    /// The documented example with one more position (BTC, sound) in front
    /// and `change` applied to the documented ETH position.
    fn state_with(change: impl FnOnce(&mut serde_json::Value)) -> serde_json::Value {
        let mut body = doc_state();
        let mut btc = body["assetPositions"][0].clone();
        btc["position"]["coin"] = json!("BTC");
        body["assetPositions"].as_array_mut().unwrap().insert(0, btc);
        change(&mut body["assetPositions"][1]["position"]);
        body
    }

    /// The refusal reason of an answer that must be refused as a whole.
    /// Panics if any report came out of it.
    fn refusal(body: &serde_json::Value) -> String {
        match margin_input(body, 7, 3, 1_700_000_000_000_000) {
            VenueInput::MarginReportInvalid {
                session: 7,
                seq: 3,
                queried_at: 1_700_000_000_000_000,
                reason,
            } => reason,
            other => panic!("not refused as a whole: {other:?}\nanswer: {body}"),
        }
    }

    /// S25. The maintenance margin rate comes from `maxLeverage`. Without a
    /// usable one on any position there is no report at all: the sound
    /// position next to it is not reported either.
    #[test]
    fn missing_zero_or_unparseable_max_leverage_rejects_the_whole_report() {
        let missing = state_with(|p| {
            p.as_object_mut().unwrap().remove("maxLeverage");
        });
        assert_eq!(refusal(&missing), "assetPositions[1] (ETH): maxLeverage is missing");
        let zero = state_with(|p| p["maxLeverage"] = json!(0));
        assert_eq!(refusal(&zero), "assetPositions[1] (ETH): maxLeverage is zero");
        for bad in [json!("50"), json!(null), json!(-1), json!(50.5), json!(4294967296u64), json!([50])] {
            let body = state_with(|p| p["maxLeverage"] = bad.clone());
            assert_eq!(
                refusal(&body),
                format!("assetPositions[1] (ETH): maxLeverage is not a whole number: {bad}")
            );
        }
        // The two-position answer is sound when nothing is changed.
        assert!(matches!(
            margin_input(&state_with(|_| {}), 7, 3, 1),
            VenueInput::MarginReport(r) if r.positions.len() == 2
        ));
    }

    /// S25. No number of the answer is ever replaced by a default: a bad
    /// account value, size, position value, margin or leverage, a margin
    /// mode that is not one of the two documented ones, or a list that
    /// cannot be walked, refuses the whole report and names what was wrong.
    #[test]
    fn any_unparseable_number_rejects_the_whole_report() {
        // Decimal fields of a position. The venue sends decimals as strings;
        // a JSON number, an exponent or more than 8 decimals is not read.
        for field in ["szi", "positionValue", "marginUsed"] {
            let gone = state_with(|p| {
                p.as_object_mut().unwrap().remove(field);
            });
            assert_eq!(refusal(&gone), format!("assetPositions[1] (ETH): {field} is missing"));
            for bad in [json!("abc"), json!(""), json!(4.967826), json!(null), json!("1e3"), json!("NaN"), json!("0.123456789")] {
                let body = state_with(|p| p[field] = bad.clone());
                assert_eq!(
                    refusal(&body),
                    format!("assetPositions[1] (ETH): {field} is not a decimal string: {bad}")
                );
            }
        }
        // The account value.
        let mut body = doc_state();
        body["marginSummary"].as_object_mut().unwrap().remove("accountValue");
        assert_eq!(refusal(&body), "marginSummary.accountValue is missing");
        let mut body = doc_state();
        body.as_object_mut().unwrap().remove("marginSummary");
        assert_eq!(refusal(&body), "marginSummary.accountValue is missing");
        for bad in [json!("abc"), json!(13109.482328), json!(null)] {
            let mut body = doc_state();
            body["marginSummary"]["accountValue"] = bad.clone();
            assert_eq!(refusal(&body), format!("marginSummary.accountValue is not a decimal string: {bad}"));
        }
        // The leverage of a position and its margin mode.
        let gone = state_with(|p| {
            p["leverage"].as_object_mut().unwrap().remove("value");
        });
        assert_eq!(refusal(&gone), "assetPositions[1] (ETH): leverage.value is missing");
        for bad in [json!("20"), json!(2.5), json!(-2), json!(null)] {
            let body = state_with(|p| p["leverage"]["value"] = bad.clone());
            assert_eq!(
                refusal(&body),
                format!("assetPositions[1] (ETH): leverage.value is not a whole number: {bad}")
            );
        }
        let gone = state_with(|p| {
            p.as_object_mut().unwrap().remove("leverage");
        });
        assert_eq!(refusal(&gone), "assetPositions[1] (ETH): leverage.type is missing");
        for bad in [json!("hedged"), json!("Isolated"), json!(""), json!(1)] {
            let body = state_with(|p| p["leverage"]["type"] = bad.clone());
            assert_eq!(
                refusal(&body),
                format!("assetPositions[1] (ETH): leverage.type is neither isolated nor cross: {bad}")
            );
        }
        // A position that cannot be identified is not skipped.
        let no_coin = state_with(|p| {
            p.as_object_mut().unwrap().remove("coin");
        });
        assert_eq!(refusal(&no_coin), "assetPositions[1]: coin is missing or unusable");
        let empty_coin = state_with(|p| p["coin"] = json!(""));
        assert_eq!(refusal(&empty_coin), "assetPositions[1]: coin is missing or unusable");
        let mut no_position = state_with(|_| {});
        no_position["assetPositions"][1] = json!({"type": "oneWay"});
        assert_eq!(refusal(&no_position), "assetPositions[1]: position is missing");
        // No list of positions is not the same as an empty list.
        let mut body = doc_state();
        body.as_object_mut().unwrap().remove("assetPositions");
        assert_eq!(refusal(&body), "assetPositions is missing or not a list");
        let mut body = doc_state();
        body["assetPositions"] = json!({});
        assert_eq!(refusal(&body), "assetPositions is missing or not a list");
        for not_an_answer in [json!(null), json!([]), json!("error"), json!({})] {
            refusal(&not_an_answer);
        }
        // An account without a position is a whole report.
        let mut flat = doc_state();
        flat["assetPositions"] = json!([]);
        assert_eq!(
            margin_input(&flat, 7, 3, 9),
            VenueInput::MarginReport(MarginReport {
                session: 7,
                seq: 3,
                queried_at: 9,
                account_value: Amount(1_310_948_232_800),
                positions: vec![],
            })
        );
        // A cross position and a short one are read as they are.
        let cross_short = state_with(|p| {
            p["leverage"] = json!({"type": "cross", "value": 3});
            p["szi"] = json!("-0.0335");
        });
        let VenueInput::MarginReport(r) = margin_input(&cross_short, 7, 3, 9) else {
            panic!("refused");
        };
        assert_eq!(
            (r.positions[1].leverage_type, r.positions[1].leverage_value, r.positions[1].qty),
            (LeverageType::Cross, 3, SQty(-3_350_000))
        );
    }

    /// S22. The parser is not given the configured instruments and has no
    /// way to leave a position out: a position on a coin this OMS does not
    /// trade (a manual trade, another dex, a name that needs mapping) is in
    /// the report, so the MM-buffer rule sees it.
    #[test]
    fn a_position_on_an_unconfigured_coin_is_kept() {
        let mut body = state_with(|p| p["coin"] = json!("kPEPE"));
        let mut other_dex = body["assetPositions"][0].clone();
        other_dex["position"]["coin"] = json!("xyz:XYZ100");
        other_dex["position"]["marginUsed"] = json!("1.5");
        let mut odd_name = body["assetPositions"][0].clone();
        odd_name["position"]["coin"] = json!("PURR/USDC");
        let list = body["assetPositions"].as_array_mut().unwrap();
        list.push(other_dex);
        list.push(odd_name);
        let VenueInput::MarginReport(r) = margin_input(&body, 7, 3, 9) else {
            panic!("refused");
        };
        let coins: Vec<&str> = r.positions.iter().map(|p| p.instrument.as_str()).collect();
        assert_eq!(coins, ["BTC", "kPEPE", "xyz:XYZ100", "PURR_USDC"]);
        assert_eq!(r.positions[2].margin_used, Amount(150_000_000));
    }

    /// The example answer of the Info endpoint documentation (Perpetuals,
    /// `activeAssetData`). The account of the example is replaced by a
    /// placeholder.
    const DOC_ACTIVE_ASSET_DATA: &str = r#"{
      "user": "0xUSER",
      "coin": "APT",
      "leverage": {"type": "cross", "value": 3},
      "maxTradeSzs": ["24836370.4400000013", "24836370.4400000013"],
      "availableToTrade": ["37019438.0284740031", "37019438.0284740031"],
      "markPx": "4.4716"
    }"#;

    /// S17. The margin mode and the leverage of the documented example,
    /// and of the same answer for an isolated instrument (the `leverage`
    /// object has the shape it has in `clearinghouseState`). An answer
    /// that does not state both, or that is about another coin, is no
    /// setting at all.
    #[test]
    fn documented_active_asset_data_parses_leverage_and_mode() {
        let apt = InstrumentId::new("APT").unwrap();
        let doc: serde_json::Value = serde_json::from_str(DOC_ACTIVE_ASSET_DATA).unwrap();
        assert_eq!(
            venue_setting_input(&doc, 7, &apt),
            Some(VenueInput::VenueSetting {
                session: 7,
                instrument: apt.clone(),
                isolated: false,
                leverage: 3,
            })
        );
        let mut isolated = doc.clone();
        isolated["leverage"] = json!({"type": "isolated", "value": 2, "rawUsd": "0.0"});
        assert_eq!(
            venue_setting_input(&isolated, 8, &apt),
            Some(VenueInput::VenueSetting {
                session: 8,
                instrument: apt.clone(),
                isolated: true,
                leverage: 2,
            })
        );

        let with = |change: &dyn Fn(&mut serde_json::Value)| {
            let mut body = isolated.clone();
            change(&mut body);
            venue_setting_input(&body, 8, &apt)
        };
        // A mode that is not one of the two documented ones is not read as
        // either.
        for mode in [json!("hedged"), json!("Isolated"), json!(""), json!(null), json!(true)] {
            assert_eq!(with(&|b| b["leverage"]["type"] = mode.clone()), None, "{mode}");
        }
        assert_eq!(with(&|b| { b["leverage"].as_object_mut().unwrap().remove("type"); }), None);
        // The leverage is a whole number or nothing.
        for value in [json!("2"), json!(2.5), json!(-2), json!(null), json!(4294967296u64)] {
            assert_eq!(with(&|b| b["leverage"]["value"] = value.clone()), None, "{value}");
        }
        assert_eq!(with(&|b| { b["leverage"].as_object_mut().unwrap().remove("value"); }), None);
        assert_eq!(with(&|b| { b.as_object_mut().unwrap().remove("leverage"); }), None);
        // An answer about another coin, or one that names none, says
        // nothing about this instrument.
        assert_eq!(with(&|b| b["coin"] = json!("BTC")), None);
        assert_eq!(with(&|b| { b.as_object_mut().unwrap().remove("coin"); }), None);
        assert_eq!(venue_setting_input(&json!(null), 8, &apt), None);
    }

    /// S18. The answer to `userAbstraction` is one JSON string. The five
    /// documented values reach the core letter for letter. A string that
    /// is not documented is not turned into one that is: it reaches the
    /// core as it came, where only an allow-list entry spelled the same
    /// way could pass it. Anything that is not a string is no mode.
    #[test]
    fn user_abstraction_values_parse_and_an_unknown_value_stays_unknown() {
        let mode = |body: serde_json::Value| match account_mode_input(&body, 7) {
            Some(VenueInput::AccountMode { session: 7, mode }) => Some(mode),
            None => None,
            other => panic!("{other:?}"),
        };
        for documented in ["unifiedAccount", "portfolioMargin", "disabled", "default", "dexAbstraction"] {
            assert_eq!(mode(json!(documented)).as_deref(), Some(documented));
        }
        for unknown in ["somethingNew", "Default", "DISABLED", " default", "default ", "standard", ""] {
            assert_eq!(mode(json!(unknown)).as_deref(), Some(unknown));
        }
        for not_a_mode in [json!(null), json!({"mode": "default"}), json!(["default"]), json!(1), json!(true)] {
            assert_eq!(mode(not_a_mode.clone()), None, "{not_a_mode}");
        }
        // The value is journaled, so its length is bounded.
        assert_eq!(mode(json!("x".repeat(64))), Some("x".repeat(64)));
        assert_eq!(mode(json!("x".repeat(65))), None);
    }

    /// The three request bodies as the Info endpoint documentation gives
    /// them. The optional `dex` field is left out: the first perp dex.
    #[test]
    fn margin_and_setting_request_json() {
        let documented = |text: &str| serde_json::from_str::<serde_json::Value>(text).unwrap();
        assert_eq!(
            margin_request("0xUSER"),
            documented(r#"{"type": "clearinghouseState", "user": "0xUSER"}"#)
        );
        assert_eq!(
            venue_setting_request("0xUSER", &InstrumentId::new("APT").unwrap()),
            documented(r#"{"type": "activeAssetData", "user": "0xUSER", "coin": "APT"}"#)
        );
        assert_eq!(
            account_mode_request("0xUSER"),
            documented(r#"{"type": "userAbstraction", "user": "0xUSER"}"#)
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
