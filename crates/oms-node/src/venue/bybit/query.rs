//! The seven queries of the reconciler (`QueryReq`), answered over REST
//! (ADR-028 §7, U16c). A query only reads. It is answered with ordinary
//! venue inputs, or with none: a half-read snapshot is never passed on.
//!
//! Paging: the docs do not say that an empty `nextPageCursor` marks the
//! last page (F20 Q9.9, Q10.8). A page counts as the last only when the
//! cursor is empty and the page holds fewer rows than the limit. A full
//! page with an empty cursor cannot be told apart from a cut answer.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Mutex;

use oms_core::*;
use serde_json::Value;

use super::checks::SETTLE_COIN;
use super::exec::{self, text};
use super::rest::{Rest, RestError};
use crate::venue::QueryReq;

/// Why a query gave no input, or stopped early.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum QueryFail {
    /// The request failed (the caller acts on `RestError::Auth` and
    /// `IpBanned`).
    Rest(RestError),
    /// The venue answered, and the answer cannot be read as a whole.
    Unreadable(String),
}

impl From<RestError> for QueryFail {
    fn from(e: RestError) -> QueryFail {
        QueryFail::Rest(e)
    }
}

impl std::fmt::Display for QueryFail {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            QueryFail::Rest(e) => write!(f, "{e}"),
            QueryFail::Unreadable(e) => write!(f, "unreadable answer: {e}"),
        }
    }
}

/// The inputs of one query, in order, and why it stopped if it did not
/// finish. Inputs before a failure are true and are delivered (fills of
/// finished windows and their `FillsSynced`).
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Answer {
    pub inputs: Vec<VenueInput>,
    pub failed: Option<QueryFail>,
}

impl Answer {
    fn of(r: Result<Vec<VenueInput>, QueryFail>) -> Answer {
        match r {
            Ok(inputs) => Answer {
                inputs,
                failed: None,
            },
            Err(e) => Answer {
                inputs: Vec::new(),
                failed: Some(e),
            },
        }
    }
}

/// Most pages one snapshot query reads.
pub const MAX_PAGES: usize = 20;
/// Most pages one fill query reads; the fills of finished windows are
/// declared synced, so the next query goes on from there.
pub const MAX_FILL_PAGES: usize = 30;
/// Longest time window of `/v5/execution/list`: 7 days (F20 Q9.7).
const WEEK_MS: u64 = 7 * 24 * 3600 * 1000;
/// Each window starts 1 ms before the end of the last: whether the venue's
/// bounds are inclusive is not documented. The fill keys de-duplicate.
const OVERLAP_MS: u64 = 1;
/// How far before its acceptance an order's history is searched: the node's
/// clock and the venue's may differ.
const ORDER_TIME_MARGIN_MS: u64 = 60_000;
/// How far before `since` each fill query starts again. A row the venue
/// lists late (its endpoints warn of delays, F20 Q10.7, Q12.9) is read on
/// a later query; the keys de-duplicate (judgment: 2 minutes).
const FILL_LOOKBACK_MS: u64 = 120_000;
/// Longest margin mode passed on (journaled, so bounded).
const ACCOUNT_MODE_MAX_LEN: usize = 64;

const LIMIT_EXECUTIONS: usize = 100;
const LIMIT_ORDERS: usize = 50;
const LIMIT_POSITIONS: usize = 200;

fn unreadable(e: impl Into<String>) -> QueryFail {
    QueryFail::Unreadable(e.into())
}

fn linear() -> Vec<(&'static str, String)> {
    vec![
        ("category", "linear".into()),
        ("settleCoin", SETTLE_COIN.into()),
    ]
}

/// A whole positive number written as a decimal ("10", "100.00"). A
/// fraction is refused, not rounded.
fn whole(row: &Value, name: &str) -> Result<u32, String> {
    let s = text(row, name)?;
    let v = num::parse_scaled(s).map_err(|e| format!("{name} {s:?}: {e}"))?;
    if v <= 0 || v % num::SCALE != 0 {
        return Err(format!("{name} {s:?} is not a whole number above zero"));
    }
    u32::try_from(v / num::SCALE).map_err(|_| format!("{name} {s:?} is out of range"))
}

/// A position row's signed quantity (`size` is never negative, `side` gives
/// the sign, F20 Q12.3). Only one-way rows are read (`positionIdx` 0).
fn signed_size(row: &Value) -> Result<SQty, String> {
    if row.get("positionIdx").and_then(Value::as_i64) != Some(0) {
        return Err(format!(
            "positionIdx {:?}: not one-way mode",
            row.get("positionIdx")
        ));
    }
    let size = exec::number(row, "size", Qty::parse)?;
    match text(row, "side")? {
        "Buy" => Ok(size.signed(Side::Buy)),
        "Sell" => Ok(size.signed(Side::Sell)),
        "" if size.0 == 0 => Ok(SQty(0)),
        other => Err(format!("side {other:?} with size {size}")),
    }
}

/// What an order row's status means (F20 Q10.1). `None`: nothing is
/// concluded. `Untriggered`, `Triggered` and `Deactivated` belong to
/// conditional orders and `PartiallyFilledCanceled` to spot; the OMS sends
/// neither, so the row is not read as one of its orders.
pub(super) fn order_status(row: &Value) -> Result<Option<StatusIn>, String> {
    let reason = |names: &[&str]| -> String {
        names
            .iter()
            .filter_map(|n| {
                row.get(*n)
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty() && *s != "UNKNOWN")
            })
            .next()
            .unwrap_or("")
            .chars()
            .take(64)
            .collect()
    };
    let status = match text(row, "orderStatus")? {
        "New" | "PartiallyFilled" => VenueStatus::Open,
        "Filled" => VenueStatus::Filled,
        // A cancelled linear order may have fills (F20 Q10.1): `cum` says.
        "Cancelled" => VenueStatus::Canceled {
            reason: reason(&["cancelType", "rejectReason"]),
        },
        "Rejected" => VenueStatus::Rejected {
            reason: reason(&["rejectReason"]),
        },
        _ => return Ok(None),
    };
    Ok(Some(StatusIn {
        status,
        // Documented as the cumulative executed quantity (F20 Q10.3).
        cum: Some(exec::number(row, "cumExecQty", Qty::parse)?),
        venue_ts: exec::venue_ms(row, "updatedTime")?,
    }))
}

/// Answers the reconciler's queries for one account.
pub struct Queries {
    rest: Rest,
    instance_id: u64,
    fills_lag_ms: u64,
    now_us: fn() -> u64,
    /// `leverageFilter.maxLeverage` per symbol, read once per process. Only
    /// the isolated-margin rule reads it (C-MARGIN), and the account is
    /// cross margin; a report needs it above zero.
    max_leverage: Mutex<HashMap<String, u32>>,
}

impl Queries {
    pub fn new(rest: Rest, instance_id: u64, fills_lag_ms: u64) -> Queries {
        Queries {
            rest,
            instance_id,
            fills_lag_ms,
            now_us: crate::util::wall_us,
            max_leverage: Mutex::new(HashMap::new()),
        }
    }

    /// The same with a fixed clock (tests).
    pub fn with_clock(mut self, now_us: fn() -> u64) -> Queries {
        self.now_us = now_us;
        self
    }

    pub async fn answer(&self, q: QueryReq, session: u64) -> Answer {
        let queried_at = (self.now_us)();
        match q {
            QueryReq::Status {
                order_id,
                accepted_at,
                ..
            } => Answer::of(
                self.status(session, order_id, accepted_at, queried_at)
                    .await,
            ),
            QueryReq::Fills { since } => self.fills(session, since).await,
            QueryReq::OpenOrders => Answer::of(self.open_orders(session, queried_at).await),
            QueryReq::Positions => Answer::of(self.positions(session, queried_at).await),
            QueryReq::Margin { seq } => Answer::of(self.margin(session, seq, queried_at).await),
            QueryReq::VenueSetting { instrument } => {
                Answer::of(self.venue_setting(session, &instrument).await)
            }
            QueryReq::AccountMode => Answer::of(self.account_mode(session).await),
        }
    }

    /// One page: its rows and the cursor of the next page.
    async fn page(
        &self,
        path: &str,
        params: &[(&'static str, String)],
        cursor: Option<&str>,
    ) -> Result<(Vec<Value>, String), QueryFail> {
        let mut p = params.to_vec();
        if let Some(c) = cursor {
            p.push(("cursor", c.to_string()));
        }
        let v = self.rest.get(path, &p).await?;
        let rows = v
            .get("list")
            .and_then(Value::as_array)
            .cloned()
            .ok_or_else(|| unreadable(format!("{path}: no list")))?;
        let next = v
            .get("nextPageCursor")
            .and_then(Value::as_str)
            .ok_or_else(|| unreadable(format!("{path}: no nextPageCursor")))?;
        Ok((rows, next.to_string()))
    }

    /// Every row of a paged snapshot, or the reason there is no whole one.
    async fn all_pages(
        &self,
        path: &str,
        params: &[(&'static str, String)],
        limit: usize,
    ) -> Result<Vec<Value>, QueryFail> {
        let mut p = params.to_vec();
        p.push(("limit", limit.to_string()));
        let mut rows = Vec::new();
        let mut cursor: Option<String> = None;
        for _ in 0..MAX_PAGES {
            let (page, next) = self.page(path, &p, cursor.as_deref()).await?;
            let full = page.len() >= limit;
            rows.extend(page);
            match (next.is_empty(), full) {
                (true, false) => return Ok(rows),
                (true, true) => {
                    return Err(unreadable(format!(
                        "{path}: a full page without a cursor may not be the last"
                    )));
                }
                (false, _) if cursor.as_deref() == Some(next.as_str()) => {
                    return Err(unreadable(format!("{path}: the cursor did not move")));
                }
                (false, _) => cursor = Some(next),
            }
        }
        Err(unreadable(format!("{path}: more than {MAX_PAGES} pages")))
    }

    /// The order with our `orderLinkId`, in the open-order view and then in
    /// the order history. Not found only if it is in neither (C-GW G-4
    /// Bybit: the open-order view loses closed orders after 500 and on a
    /// venue restart). `queried_at` is the time before the first request.
    async fn status(
        &self,
        session: u64,
        order_id: OrderId,
        accepted_at: Ts,
        queried_at: Ts,
    ) -> Result<Vec<VenueInput>, QueryFail> {
        let link = exec::WIRE_ID.encode(VenueClientId {
            instance_id: self.instance_id,
            order_id,
        });
        let mut params = linear();
        params.push(("orderLinkId", link.clone()));
        // The history answers 7 days, the last 7 by default (F20 Q10.7). An
        // order older than that is searched from when the core accepted it
        // (less a margin for the clocks), so a long outage cannot hide a
        // filled order. The docs do not say which time the window filters.
        let from = (accepted_at / 1000).saturating_sub(ORDER_TIME_MARGIN_MS);
        let to = (from + WEEK_MS).min(queried_at / 1000);
        let mut since_accepted = params.clone();
        since_accepted.push(("startTime", from.to_string()));
        since_accepted.push(("endTime", to.max(from).to_string()));
        let lookups = [
            ("/v5/order/realtime", params.clone()),
            ("/v5/order/history", params),
            ("/v5/order/history", since_accepted),
        ];
        for (path, params) in &lookups {
            let path = *path;
            let (rows, _) = self.page(path, params, None).await?;
            let mine: Vec<&Value> = rows
                .iter()
                .filter(|r| r.get("orderLinkId").and_then(Value::as_str) == Some(link.as_str()))
                .collect();
            if mine.len() > 1 {
                return Err(unreadable(format!(
                    "{path}: {} orders with orderLinkId {link}",
                    mine.len()
                )));
            }
            if rows.len() > mine.len() {
                // An answer about other orders proves nothing about ours.
                return Err(unreadable(format!(
                    "{path}: rows of other orders for orderLinkId {link}"
                )));
            }
            if let Some(row) = mine.first() {
                let status =
                    order_status(row).map_err(|e| unreadable(format!("{path} {link}: {e}")))?;
                let Some(status) = status else {
                    return Err(unreadable(format!(
                        "{path} {link}: status {:?} is not one of a plain order",
                        row.get("orderStatus")
                    )));
                };
                return Ok(vec![VenueInput::OrderStatus(OrderStatusIn {
                    session,
                    order_id: Some(order_id),
                    venue_oid: exec::venue_oid(row),
                    status,
                    source: EvidenceSource::Query,
                })]);
            }
        }
        Ok(vec![VenueInput::StatusNotFound {
            session,
            order_id,
            queried_at,
        }])
    }

    async fn open_orders(
        &self,
        session: u64,
        queried_at: Ts,
    ) -> Result<Vec<VenueInput>, QueryFail> {
        let rows = self
            .all_pages("/v5/order/realtime", &linear(), LIMIT_ORDERS)
            .await?;
        let mut orders = Vec::with_capacity(rows.len());
        for r in &rows {
            let read = || -> Result<Option<VenueOpenOrder>, String> {
                // `openOnly` 0 lists open orders only (F20 Q10.4); a closed
                // row is not open, whatever else it says.
                if !matches!(
                    text(r, "orderStatus")?,
                    "New" | "PartiallyFilled" | "Untriggered"
                ) {
                    return Ok(None);
                }
                Ok(Some(VenueOpenOrder {
                    venue_oid: exec::venue_oid(r).ok_or("orderId is missing")?,
                    order_id: exec::our_order(
                        text(r, "orderLinkId").unwrap_or(""),
                        self.instance_id,
                    ),
                    instrument: exec::instrument(r)?,
                    side: exec::side(r)?,
                    // "The remaining qty not executed" (F20 Q10.3).
                    remaining: Some(exec::number(r, "leavesQty", Qty::parse)?),
                }))
            };
            if let Some(o) = read().map_err(|e| unreadable(format!("/v5/order/realtime: {e}")))? {
                orders.push(o);
            }
        }
        Ok(vec![VenueInput::OpenOrders {
            session,
            queried_at,
            orders,
        }])
    }

    /// Every linear USDT position (with `settleCoin` and no symbol the venue
    /// lists only positions above zero, F20 Q12.1).
    async fn position_rows(&self) -> Result<Vec<Value>, QueryFail> {
        self.all_pages("/v5/position/list", &linear(), LIMIT_POSITIONS)
            .await
    }

    async fn positions(&self, session: u64, queried_at: Ts) -> Result<Vec<VenueInput>, QueryFail> {
        let mut positions = Vec::new();
        for r in &self.position_rows().await? {
            let read = || -> Result<VenuePosition, String> {
                Ok(VenuePosition {
                    instrument: exec::instrument(r)?,
                    qty: signed_size(r)?,
                })
            };
            let p = read().map_err(|e| unreadable(format!("/v5/position/list: {e}")))?;
            if p.qty.0 != 0 {
                positions.push(p);
            }
        }
        Ok(vec![VenueInput::Positions {
            session,
            queried_at,
            positions,
        }])
    }

    /// The account's margin mode (`/v5/account/info`, F18 §6).
    async fn margin_mode(&self) -> Result<String, QueryFail> {
        let v = self.rest.get("/v5/account/info", &[]).await?;
        let mode =
            text(&v, "marginMode").map_err(|e| unreadable(format!("/v5/account/info: {e}")))?;
        Ok(mode.to_string())
    }

    async fn max_leverage(&self, symbol: &str) -> Result<u32, QueryFail> {
        if let Some(v) = self
            .max_leverage
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(symbol)
        {
            return Ok(*v);
        }
        let params = [
            ("category", "linear".to_string()),
            ("symbol", symbol.to_string()),
        ];
        let v = self
            .rest
            .get("/v5/market/instruments-info", &params)
            .await?;
        let row = v
            .get("list")
            .and_then(Value::as_array)
            .and_then(|l| {
                l.iter()
                    .find(|r| r.get("symbol").and_then(Value::as_str) == Some(symbol))
            })
            .ok_or_else(|| unreadable(format!("instruments-info: no row for {symbol}")))?;
        let max = row
            .get("leverageFilter")
            .ok_or("no leverageFilter".to_string())
            .and_then(|f| whole(f, "maxLeverage"))
            .map_err(|e| unreadable(format!("instruments-info {symbol}: {e}")))?;
        self.max_leverage
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(symbol.to_string(), max);
        Ok(max)
    }

    /// The margin report (C-MARGIN). Account value: `totalEquity` of the
    /// unified account (USD). Positions: every linear USDT position, with
    /// `positionIM` as the margin used and the account's margin mode as
    /// each position's (a unified account has one mode for all, F18 §6).
    /// An answer that cannot be read as a whole is `MarginReportInvalid`.
    async fn margin(
        &self,
        session: u64,
        seq: u64,
        queried_at: Ts,
    ) -> Result<Vec<VenueInput>, QueryFail> {
        let invalid = |reason: String| {
            Ok(vec![VenueInput::MarginReportInvalid {
                session,
                seq,
                queried_at,
                reason,
            }])
        };
        let wallet = self
            .rest
            .get(
                "/v5/account/wallet-balance",
                &[("accountType", "UNIFIED".to_string())],
            )
            .await?;
        let accounts: Vec<&Value> = wallet
            .get("list")
            .and_then(Value::as_array)
            .map(|l| {
                l.iter()
                    .filter(|a| a.get("accountType").and_then(Value::as_str) == Some("UNIFIED"))
                    .collect()
            })
            .unwrap_or_default();
        let [account] = accounts.as_slice() else {
            return invalid(format!(
                "wallet-balance: {} UNIFIED accounts, not one",
                accounts.len()
            ));
        };
        let account_value = match exec::number(account, "totalEquity", Amount::parse) {
            Ok(v) => v,
            Err(e) => return invalid(format!("wallet-balance: {e}")),
        };
        let leverage_type = match self.margin_mode().await {
            Ok(m) if m == "REGULAR_MARGIN" => LeverageType::Cross,
            Ok(m) if m == "ISOLATED_MARGIN" => LeverageType::Isolated,
            Ok(m) => {
                return invalid(format!(
                    "account/info: margin mode {m:?} has no per-position margin"
                ));
            }
            Err(QueryFail::Unreadable(e)) => return invalid(e),
            Err(e) => return Err(e),
        };
        let rows = match self.position_rows().await {
            Ok(rows) => rows,
            Err(QueryFail::Unreadable(e)) => return invalid(e),
            Err(e) => return Err(e),
        };
        let mut positions = Vec::with_capacity(rows.len());
        for r in &rows {
            let symbol = text(r, "symbol").unwrap_or("?");
            let read = || -> Result<MarginPosition, String> {
                Ok(MarginPosition {
                    instrument: exec::instrument(r)?,
                    qty: signed_size(r)?,
                    position_value: exec::number(r, "positionValue", Amount::parse)?,
                    margin_used: exec::number(r, "positionIM", Amount::parse)?,
                    leverage_type,
                    leverage_value: whole(r, "leverage")?,
                    max_leverage: 0,
                })
            };
            let mut p = match read() {
                Ok(p) => p,
                Err(e) => return invalid(format!("position/list {symbol}: {e}")),
            };
            p.max_leverage = match self.max_leverage(p.instrument.as_str()).await {
                Ok(m) => m,
                Err(QueryFail::Unreadable(e)) => return invalid(e),
                Err(e) => return Err(e),
            };
            positions.push(p);
        }
        Ok(vec![VenueInput::MarginReport(MarginReport {
            session,
            seq,
            queried_at,
            account_value,
            positions,
        })])
    }

    /// The margin mode (the account's) and the leverage of one instrument.
    /// Read only (C-MARGIN).
    async fn venue_setting(
        &self,
        session: u64,
        instrument: &InstrumentId,
    ) -> Result<Vec<VenueInput>, QueryFail> {
        let isolated = match self.margin_mode().await?.as_str() {
            "REGULAR_MARGIN" => false,
            "ISOLATED_MARGIN" => true,
            other => {
                return Err(unreadable(format!(
                    "account/info: margin mode {other:?} has no per-instrument setting"
                )));
            }
        };
        let params = [
            ("category", "linear".to_string()),
            ("symbol", instrument.to_string()),
        ];
        let (rows, _) = self.page("/v5/position/list", &params, None).await?;
        // With a symbol the venue returns its row also without a position
        // (F20 Q7.3). One-way mode has exactly one.
        let [row] = rows.as_slice() else {
            return Err(unreadable(format!(
                "position/list {instrument}: {} rows, not one",
                rows.len()
            )));
        };
        let read = || -> Result<u32, String> {
            if text(row, "symbol")? != instrument.as_str() {
                return Err(format!("the row is about {:?}", row.get("symbol")));
            }
            signed_size(row)?;
            whole(row, "leverage")
        };
        let leverage =
            read().map_err(|e| unreadable(format!("position/list {instrument}: {e}")))?;
        Ok(vec![VenueInput::VenueSetting {
            session,
            instrument: instrument.clone(),
            isolated,
            leverage,
        }])
    }

    /// The account's margin mode as the venue names it. The core accepts it
    /// only if the allow-list spells it the same way.
    async fn account_mode(&self, session: u64) -> Result<Vec<VenueInput>, QueryFail> {
        let mode = self.margin_mode().await?;
        if mode.len() > ACCOUNT_MODE_MAX_LEN {
            return Err(unreadable("account/info: marginMode is too long"));
        }
        Ok(vec![VenueInput::AccountMode { session, mode }])
    }

    /// Every execution from `since` up to a time `fills_lag_ms` in the past,
    /// in windows of at most 7 days. After each window whose pages are all
    /// read, `FillsSynced` up to just before its end (C-GW G-7). A window
    /// whose last page cannot be told is split in two and read again.
    async fn fills(&self, session: u64, since: Ts) -> Answer {
        let end = ((self.now_us)() / 1000).saturating_sub(self.fills_lag_ms);
        let mut out = Answer::default();
        if end <= since / 1000 {
            return out;
        }
        let start = (since / 1000).saturating_sub(FILL_LOOKBACK_MS);
        let mut windows: VecDeque<(u64, u64)> = VecDeque::new();
        let mut from = start;
        while from < end {
            let to = end.min(from + WEEK_MS - OVERLAP_MS);
            windows.push_back((from, to));
            from = to;
        }
        let mut seen: HashSet<FillKey> = HashSet::new();
        let mut pages = 0usize;
        'windows: while let Some((from, to)) = windows.pop_front() {
            let mut params = linear();
            params.push(("startTime", from.saturating_sub(OVERLAP_MS).to_string()));
            params.push(("endTime", to.to_string()));
            params.push(("limit", LIMIT_EXECUTIONS.to_string()));
            let mut cursor: Option<String> = None;
            loop {
                if pages == MAX_FILL_PAGES {
                    // Not a failure: the next query goes on from the last
                    // synced window.
                    return out;
                }
                pages += 1;
                let (rows, next) = match self
                    .page("/v5/execution/list", &params, cursor.as_deref())
                    .await
                {
                    Ok(p) => p,
                    Err(e) => {
                        out.failed = Some(e);
                        return out;
                    }
                };
                let full = rows.len() >= LIMIT_EXECUTIONS;
                for r in &rows {
                    match exec::classify(r, self.instance_id, None) {
                        Ok(input) => {
                            if let Some(k) = input_key(&input) {
                                if !seen.insert(k.clone()) {
                                    continue;
                                }
                            }
                            out.inputs.push(input);
                        }
                        Err(e) => {
                            out.failed = Some(unreadable(format!("/v5/execution/list: {e}")));
                            return out;
                        }
                    }
                }
                match (next.is_empty(), full) {
                    (true, false) => break,
                    (true, true) if to - from >= 2 => {
                        let mid = from + (to - from) / 2;
                        windows.push_front((mid, to));
                        windows.push_front((from, mid));
                        continue 'windows;
                    }
                    (true, true) => {
                        out.failed = Some(unreadable(
                            "/v5/execution/list: a full page without a cursor in a 1 ms window",
                        ));
                        return out;
                    }
                    (false, _) if cursor.as_deref() == Some(next.as_str()) => {
                        out.failed =
                            Some(unreadable("/v5/execution/list: the cursor did not move"));
                        return out;
                    }
                    (false, _) => cursor = Some(next),
                }
            }
            // Every execution before `to` (ms) is delivered.
            out.inputs.push(VenueInput::FillsSynced {
                session,
                up_to: (to * 1000).saturating_sub(1),
            });
        }
        out
    }
}

/// The execution key of an input made by `exec::classify`.
fn input_key(i: &VenueInput) -> Option<&FillKey> {
    match i {
        VenueInput::Fill(f) => Some(&f.key),
        VenueInput::Funding(f) => Some(&f.key),
        VenueInput::PositionChange(c) => Some(&c.key),
        VenueInput::UnclassifiedExecution { key, .. } => Some(key),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::venue::bybit::rest::Credentials;
    use http_fake::{Answer as Http, FakeHttp};
    use serde_json::json;
    use std::sync::Arc;
    use std::time::Duration;

    const INSTANCE: u64 = 7;
    const SESSION: u64 = 3;
    /// 2023-01-10 00:00:00 UTC.
    const NOW_MS: u64 = 1_673_308_800_000;
    const LAG_MS: u64 = 2_000;
    const DAY_MS: u64 = 24 * 3600 * 1000;
    /// When the core accepted the order of the status tests: 30 days ago.
    const ACCEPTED_US: u64 = (NOW_MS - 30 * DAY_MS) * 1000;

    fn now_us() -> u64 {
        NOW_MS * 1000
    }

    fn ok(result: Value) -> Http {
        Http::Reply(
            200,
            json!({"retCode": 0, "retMsg": "OK", "result": result, "time": 1}).to_string(),
        )
    }

    fn venue() -> FakeHttp {
        FakeHttp::start(Http::Reply(404, "{}".into()))
    }

    fn queries(fake: &FakeHttp) -> Queries {
        let rest = Rest::new(
            &fake.url(),
            Arc::new(Credentials::new("K", "S")),
            5000,
            Duration::from_millis(500),
        )
        .unwrap();
        Queries::new(rest, INSTANCE, LAG_MS).with_clock(now_us)
    }

    fn link(order: u64) -> String {
        exec::WIRE_ID.encode(VenueClientId {
            instance_id: INSTANCE,
            order_id: OrderId(order),
        })
    }

    fn list(rows: Vec<Value>, cursor: &str) -> Value {
        json!({"category": "linear", "list": rows, "nextPageCursor": cursor})
    }

    fn order(link: &str, status: &str, cum: &str) -> Value {
        json!({
            "orderId": "fd4300ae-7847-404e-b947-b46980a4d140", "orderLinkId": link, "symbol": "BTCUSDT",
            "side": "Buy", "price": "30000", "qty": "0.003", "orderStatus": status, "cancelType": "CancelByUser",
            "rejectReason": "EC_NoError", "leavesQty": "0.002", "cumExecQty": cum, "updatedTime": "1672217577306"
        })
    }

    fn targets(fake: &FakeHttp) -> Vec<String> {
        fake.requests().iter().map(|r| r.target.clone()).collect()
    }

    // ---- order status -----------------------------------------------------

    #[tokio::test]
    async fn the_status_of_our_order_is_read_from_the_open_order_view() {
        let fake = venue();
        fake.route(
            "/v5/order/realtime",
            [ok(list(
                vec![order(&link(42), "PartiallyFilled", "0.001")],
                "",
            ))],
        );
        let a = queries(&fake)
            .answer(
                QueryReq::Status {
                    order_id: OrderId(42),
                    venue_oid: None,
                    accepted_at: ACCEPTED_US,
                },
                SESSION,
            )
            .await;
        assert_eq!(a.failed, None);
        assert_eq!(
            a.inputs,
            vec![VenueInput::OrderStatus(OrderStatusIn {
                session: SESSION,
                order_id: Some(OrderId(42)),
                venue_oid: Some(VenueOrderId::new("fd4300ae-7847-404e-b947-b46980a4d140").unwrap()),
                status: StatusIn {
                    status: VenueStatus::Open,
                    cum: Some(Qty::parse("0.001").unwrap()),
                    venue_ts: 1_672_217_577_306_000
                },
                source: EvidenceSource::Query,
            })]
        );
        assert_eq!(
            targets(&fake),
            vec![format!(
                "/v5/order/realtime?category=linear&orderLinkId={}&settleCoin=USDT",
                link(42)
            )]
        );
    }

    #[tokio::test]
    async fn an_order_missing_from_the_open_order_view_is_looked_up_in_the_history() {
        let fake = venue();
        fake.route("/v5/order/realtime", [ok(list(vec![], ""))]);
        fake.route(
            "/v5/order/history",
            [ok(list(vec![order(&link(42), "Cancelled", "0.001")], ""))],
        );
        let a = queries(&fake)
            .answer(
                QueryReq::Status {
                    order_id: OrderId(42),
                    venue_oid: None,
                    accepted_at: ACCEPTED_US,
                },
                SESSION,
            )
            .await;
        let [VenueInput::OrderStatus(s)] = a.inputs.as_slice() else {
            panic!("{a:?}")
        };
        assert_eq!(
            s.status.status,
            VenueStatus::Canceled {
                reason: "CancelByUser".into()
            }
        );
        assert_eq!(s.status.cum, Some(Qty::parse("0.001").unwrap()));
        let paths: Vec<String> = fake
            .requests()
            .iter()
            .map(|r| r.path().to_string())
            .collect();
        assert_eq!(paths, vec!["/v5/order/realtime", "/v5/order/history"]);
    }

    #[tokio::test]
    async fn an_order_is_not_found_only_when_neither_view_has_it() {
        let fake = venue();
        fake.route("/v5/order/realtime", [ok(list(vec![], ""))]);
        fake.route("/v5/order/history", [ok(list(vec![], ""))]);
        let a = queries(&fake)
            .answer(
                QueryReq::Status {
                    order_id: OrderId(42),
                    venue_oid: None,
                    accepted_at: ACCEPTED_US,
                },
                SESSION,
            )
            .await;
        assert_eq!(
            a.inputs,
            vec![VenueInput::StatusNotFound {
                session: SESSION,
                order_id: OrderId(42),
                queried_at: now_us()
            }]
        );
        // The open-order view, the last 7 days of the history and 7 days
        // from when the order was accepted (less a minute for the clocks).
        let from = ACCEPTED_US / 1000 - 60_000;
        let base = format!("category=linear&orderLinkId={}&settleCoin=USDT", link(42));
        assert_eq!(
            targets(&fake),
            vec![
                format!("/v5/order/realtime?{base}"),
                format!("/v5/order/history?{base}"),
                format!(
                    "/v5/order/history?category=linear&endTime={}&orderLinkId={}&settleCoin=USDT&startTime={from}",
                    from + 7 * DAY_MS,
                    link(42)
                ),
            ]
        );

        // An order older than the history's default window is found from
        // the time it was accepted.
        let fake = venue();
        fake.route("/v5/order/realtime", [ok(list(vec![], ""))]);
        fake.route(
            "/v5/order/history",
            [
                ok(list(vec![], "")),
                ok(list(vec![order(&link(42), "Filled", "0.003")], "")),
            ],
        );
        let a = queries(&fake)
            .answer(
                QueryReq::Status {
                    order_id: OrderId(42),
                    venue_oid: None,
                    accepted_at: ACCEPTED_US,
                },
                SESSION,
            )
            .await;
        let [VenueInput::OrderStatus(s)] = a.inputs.as_slice() else {
            panic!("{a:?}")
        };
        assert_eq!(s.status.status, VenueStatus::Filled);

        // The history cannot be read: no conclusion.
        let fake = venue();
        fake.route("/v5/order/realtime", [ok(list(vec![], ""))]);
        fake.route("/v5/order/history", [Http::Reply(500, "".into())]);
        let a = queries(&fake)
            .answer(
                QueryReq::Status {
                    order_id: OrderId(42),
                    venue_oid: None,
                    accepted_at: ACCEPTED_US,
                },
                SESSION,
            )
            .await;
        assert!(a.inputs.is_empty());
        assert!(matches!(
            a.failed,
            Some(QueryFail::Rest(RestError::NoAnswer(_)))
        ));
    }

    #[tokio::test]
    async fn a_status_answer_about_another_order_or_of_a_kind_we_never_send_concludes_nothing() {
        for rows in [
            vec![order(&link(43), "Filled", "0.003")],
            vec![
                order(&link(42), "Filled", "0.003"),
                order(&link(43), "Filled", "0.003"),
            ],
            vec![order(&link(42), "Untriggered", "0")],
            vec![order(&link(42), "Deactivated", "0")],
            vec![order(&link(42), "Shiny", "0")],
            vec![json!({"orderLinkId": link(42), "orderStatus": "Filled"})],
        ] {
            let fake = venue();
            fake.route("/v5/order/realtime", [ok(list(rows.clone(), ""))]);
            fake.route("/v5/order/history", [ok(list(vec![], ""))]);
            let a = queries(&fake)
                .answer(
                    QueryReq::Status {
                        order_id: OrderId(42),
                        venue_oid: None,
                        accepted_at: ACCEPTED_US,
                    },
                    SESSION,
                )
                .await;
            assert!(a.inputs.is_empty(), "{rows:?}");
            assert!(
                matches!(a.failed, Some(QueryFail::Unreadable(_))),
                "{rows:?}"
            );
        }
    }

    #[test]
    fn the_status_table() {
        let read = |s: &str| {
            order_status(&order("x", s, "0.001"))
                .unwrap()
                .map(|st| st.status)
        };
        assert_eq!(read("New"), Some(VenueStatus::Open));
        assert_eq!(read("PartiallyFilled"), Some(VenueStatus::Open));
        assert_eq!(read("Filled"), Some(VenueStatus::Filled));
        assert_eq!(
            read("Rejected"),
            Some(VenueStatus::Rejected {
                reason: "EC_NoError".into()
            })
        );
        for s in [
            "Untriggered",
            "Triggered",
            "Deactivated",
            "PartiallyFilledCanceled",
            "cancelled",
        ] {
            assert_eq!(read(s), None, "{s}");
        }
    }

    // ---- paged snapshots --------------------------------------------------

    #[tokio::test]
    async fn open_orders_are_read_through_every_page() {
        let fake = venue();
        let page1: Vec<Value> = (0..LIMIT_ORDERS as u64)
            .map(|i| order(&link(i), "New", "0"))
            .collect();
        fake.route(
            "/v5/order/realtime",
            [
                ok(list(page1, "page_args%3Dfd43%26symbol%3D6%26")),
                ok(list(
                    vec![
                        order("manual", "PartiallyFilled", "0.001"),
                        order(&link(99), "Filled", "0.003"),
                    ],
                    "",
                )),
            ],
        );
        let a = queries(&fake).answer(QueryReq::OpenOrders, SESSION).await;
        let [
            VenueInput::OpenOrders {
                orders, queried_at, ..
            },
        ] = a.inputs.as_slice()
        else {
            panic!("{a:?}")
        };
        assert_eq!(*queried_at, now_us());
        // The closed row is not open.
        assert_eq!(orders.len(), LIMIT_ORDERS + 1);
        assert_eq!(orders[0].order_id, Some(OrderId(0)));
        assert_eq!(orders[0].remaining, Some(Qty::parse("0.002").unwrap()));
        assert_eq!(orders[LIMIT_ORDERS].order_id, None);
        assert_eq!(
            targets(&fake),
            vec![
                "/v5/order/realtime?category=linear&limit=50&settleCoin=USDT".to_string(),
                "/v5/order/realtime?category=linear&cursor=page_args%3Dfd43%26symbol%3D6%26&limit=50&settleCoin=USDT".to_string(),
            ]
        );
    }

    #[tokio::test]
    async fn a_snapshot_whose_last_page_cannot_be_told_gives_no_input() {
        let full: Vec<Value> = (0..LIMIT_ORDERS as u64)
            .map(|i| order(&link(i), "New", "0"))
            .collect();
        // The pages and the requests it takes to stop.
        for (pages, requests) in [
            (vec![ok(list(full.clone(), ""))], 1),
            // The cursor did not move: stop at once, not at MAX_PAGES.
            (
                vec![ok(list(full.clone(), "c1")), ok(list(full.clone(), "c1"))],
                2,
            ),
            (vec![ok(json!({"category": "linear", "list": []}))], 1),
            (
                vec![
                    ok(list(vec![], "c1")),
                    Http::Reply(200, r#"{"retCode":10016,"retMsg":"busy"}"#.into()),
                ],
                2,
            ),
        ] {
            let fake = venue();
            fake.route("/v5/order/realtime", pages);
            let a = queries(&fake).answer(QueryReq::OpenOrders, SESSION).await;
            assert!(a.inputs.is_empty(), "{a:?}");
            assert!(a.failed.is_some());
            assert_eq!(fake.requests().len(), requests, "{a:?}");
        }
        // A cursor that never ends is cut at MAX_PAGES.
        let fake = venue();
        let n = std::sync::atomic::AtomicUsize::new(0);
        let pages: Vec<Http> = (0..MAX_PAGES + 5)
            .map(|_| {
                ok(list(
                    vec![],
                    &format!("c{}", n.fetch_add(1, std::sync::atomic::Ordering::SeqCst)),
                ))
            })
            .collect();
        fake.route("/v5/order/realtime", pages);
        let a = queries(&fake).answer(QueryReq::OpenOrders, SESSION).await;
        assert!(a.inputs.is_empty());
        assert_eq!(fake.requests().len(), MAX_PAGES);
    }

    fn position(symbol: &str, side: &str, size: &str, idx: i64) -> Value {
        json!({
            "positionIdx": idx, "symbol": symbol, "side": side, "size": size, "positionValue": "300",
            "positionIM": "30.06", "leverage": "10", "markPrice": "30000", "updatedTime": "1672280219169"
        })
    }

    #[tokio::test]
    async fn positions_are_signed_by_side() {
        let fake = venue();
        fake.route(
            "/v5/position/list",
            [ok(list(
                vec![
                    position("BTCUSDT", "Buy", "0.01", 0),
                    position("ETHUSDT", "Sell", "0.5", 0),
                    position("XRPUSDT", "", "0", 0),
                ],
                "",
            ))],
        );
        let a = queries(&fake).answer(QueryReq::Positions, SESSION).await;
        assert_eq!(
            a.inputs,
            vec![VenueInput::Positions {
                session: SESSION,
                queried_at: now_us(),
                positions: vec![
                    VenuePosition {
                        instrument: InstrumentId::new("BTCUSDT").unwrap(),
                        qty: SQty::parse("0.01").unwrap()
                    },
                    VenuePosition {
                        instrument: InstrumentId::new("ETHUSDT").unwrap(),
                        qty: SQty::parse("-0.5").unwrap()
                    },
                ],
            }]
        );
        assert_eq!(
            targets(&fake),
            vec!["/v5/position/list?category=linear&limit=200&settleCoin=USDT"]
        );
        // A hedge-mode row or an unreadable side: no snapshot.
        for bad in [
            position("BTCUSDT", "Buy", "0.01", 1),
            position("BTCUSDT", "", "0.01", 0),
        ] {
            let fake = venue();
            fake.route("/v5/position/list", [ok(list(vec![bad.clone()], ""))]);
            let a = queries(&fake).answer(QueryReq::Positions, SESSION).await;
            assert!(a.inputs.is_empty(), "{bad}");
        }
    }

    // ---- margin, setting, mode --------------------------------------------

    fn wallet(equity: &str) -> Value {
        json!({"list": [{"accountType": "UNIFIED", "totalEquity": equity, "accountMMRate": "0", "coin": []}]})
    }

    fn account(mode: &str) -> Value {
        json!({"marginMode": mode, "unifiedMarginStatus": 5, "updatedTime": "1"})
    }

    fn instrument(symbol: &str, max: &str) -> Value {
        list(
            vec![
                json!({"symbol": symbol, "settleCoin": "USDT", "leverageFilter": {"minLeverage": "1", "maxLeverage": max, "leverageStep": "0.01"}}),
            ],
            "",
        )
    }

    fn margin_venue(positions: Vec<Value>) -> FakeHttp {
        let fake = venue();
        fake.route("/v5/account/wallet-balance", [ok(wallet("1000.5"))]);
        fake.route("/v5/account/info", [ok(account("REGULAR_MARGIN"))]);
        fake.route("/v5/position/list", [ok(list(positions, ""))]);
        fake.route(
            "/v5/market/instruments-info",
            [ok(instrument("BTCUSDT", "100.00"))],
        );
        fake
    }

    #[tokio::test]
    async fn the_margin_report_maps_the_unified_account() {
        let fake = margin_venue(vec![position("BTCUSDT", "Sell", "0.01", 0)]);
        let q = queries(&fake);
        let a = q.answer(QueryReq::Margin { seq: 9 }, SESSION).await;
        assert_eq!(
            a.inputs,
            vec![VenueInput::MarginReport(MarginReport {
                session: SESSION,
                seq: 9,
                queried_at: now_us(),
                account_value: Amount::parse("1000.5").unwrap(),
                positions: vec![MarginPosition {
                    instrument: InstrumentId::new("BTCUSDT").unwrap(),
                    qty: SQty::parse("-0.01").unwrap(),
                    position_value: Amount::parse("300").unwrap(),
                    margin_used: Amount::parse("30.06").unwrap(),
                    leverage_type: LeverageType::Cross,
                    leverage_value: 10,
                    max_leverage: 100,
                }],
            })]
        );
        // The maximum leverage is read once per symbol.
        q.answer(QueryReq::Margin { seq: 10 }, SESSION).await;
        let n = fake
            .requests()
            .iter()
            .filter(|r| r.path() == "/v5/market/instruments-info")
            .count();
        assert_eq!(n, 1);
        assert!(
            targets(&fake).contains(&"/v5/account/wallet-balance?accountType=UNIFIED".to_string())
        );
    }

    #[tokio::test]
    async fn a_margin_answer_that_cannot_be_read_as_a_whole_is_refused_as_a_whole() {
        let cases: Vec<(&str, Value, Value)> = vec![
            ("fraction", json!({}), json!({"leverage": "2.5"})),
            ("portfolio", json!({"mode": "PORTFOLIO_MARGIN"}), json!({})),
            ("no IM", json!({}), json!({"positionIM": ""})),
            ("equity", json!({"equity": "lots"}), json!({})),
            ("max", json!({"max": "0"}), json!({})),
            (
                "decimals",
                json!({}),
                json!({"positionValue": "1.123456789"}),
            ),
        ];
        for (name, venue_over, pos_over) in cases {
            let mut p = position("BTCUSDT", "Buy", "0.01", 0);
            for (k, v) in pos_over.as_object().unwrap() {
                p[k] = v.clone();
            }
            let fake = margin_venue(vec![p]);
            if let Some(m) = venue_over.get("mode") {
                fake.route("/v5/account/info", [ok(account(m.as_str().unwrap()))]);
            }
            if let Some(e) = venue_over.get("equity") {
                fake.route(
                    "/v5/account/wallet-balance",
                    [ok(wallet(e.as_str().unwrap()))],
                );
            }
            if let Some(m) = venue_over.get("max") {
                fake.route(
                    "/v5/market/instruments-info",
                    [ok(instrument("BTCUSDT", m.as_str().unwrap()))],
                );
            }
            let a = queries(&fake)
                .answer(QueryReq::Margin { seq: 9 }, SESSION)
                .await;
            let [VenueInput::MarginReportInvalid { seq: 9, reason, .. }] = a.inputs.as_slice()
            else {
                panic!("{name}: {a:?}")
            };
            assert!(!reason.is_empty(), "{name}");
        }
        // No answer: no input at all (the last report keeps ageing).
        let fake = margin_venue(vec![]);
        fake.route("/v5/position/list", [Http::Reply(502, "".into())]);
        let a = queries(&fake)
            .answer(QueryReq::Margin { seq: 9 }, SESSION)
            .await;
        assert!(a.inputs.is_empty());
        assert!(matches!(a.failed, Some(QueryFail::Rest(_))));
    }

    #[tokio::test]
    async fn the_venue_setting_and_the_account_mode_are_read_as_they_are() {
        let fake = margin_venue(vec![position("BTCUSDT", "", "0", 0)]);
        let q = queries(&fake);
        let btc = InstrumentId::new("BTCUSDT").unwrap();
        let a = q
            .answer(
                QueryReq::VenueSetting {
                    instrument: btc.clone(),
                },
                SESSION,
            )
            .await;
        assert_eq!(
            a.inputs,
            vec![VenueInput::VenueSetting {
                session: SESSION,
                instrument: btc.clone(),
                isolated: false,
                leverage: 10
            }]
        );
        assert!(
            targets(&fake)
                .contains(&"/v5/position/list?category=linear&symbol=BTCUSDT".to_string())
        );
        let a = q.answer(QueryReq::AccountMode, SESSION).await;
        assert_eq!(
            a.inputs,
            vec![VenueInput::AccountMode {
                session: SESSION,
                mode: "REGULAR_MARGIN".into()
            }]
        );

        fake.route("/v5/account/info", [ok(account("ISOLATED_MARGIN"))]);
        let a = q
            .answer(
                QueryReq::VenueSetting {
                    instrument: btc.clone(),
                },
                SESSION,
            )
            .await;
        assert_eq!(
            a.inputs,
            vec![VenueInput::VenueSetting {
                session: SESSION,
                instrument: btc.clone(),
                isolated: true,
                leverage: 10
            }]
        );

        // Unknown mode, a fraction, another symbol, two rows: the setting
        // stays unknown.
        for (mode, rows) in [
            ("PORTFOLIO_MARGIN", vec![position("BTCUSDT", "", "0", 0)]),
            (
                "REGULAR_MARGIN",
                vec![with(
                    position("BTCUSDT", "", "0", 0),
                    "leverage",
                    json!("2.5"),
                )],
            ),
            ("REGULAR_MARGIN", vec![position("ETHUSDT", "", "0", 0)]),
            (
                "REGULAR_MARGIN",
                vec![
                    position("BTCUSDT", "Buy", "1", 1),
                    position("BTCUSDT", "Sell", "1", 2),
                ],
            ),
        ] {
            fake.route("/v5/account/info", [ok(account(mode))]);
            fake.route("/v5/position/list", [ok(list(rows.clone(), ""))]);
            let a = q
                .answer(
                    QueryReq::VenueSetting {
                        instrument: btc.clone(),
                    },
                    SESSION,
                )
                .await;
            assert!(a.inputs.is_empty(), "{mode} {rows:?}");
        }
        fake.route("/v5/account/info", [ok(account(&"M".repeat(65)))]);
        assert!(
            q.answer(QueryReq::AccountMode, SESSION)
                .await
                .inputs
                .is_empty()
        );
    }

    fn with(mut v: Value, k: &str, x: Value) -> Value {
        v[k] = x;
        v
    }

    // ---- fills --------------------------------------------------------------

    fn execution(id: &str, exec_type: &str, time_ms: u64) -> Value {
        json!({
            "symbol": "BTCUSDT", "orderId": "fd4300ae-7847-404e-b947-b46980a4d140", "orderLinkId": link(42),
            "side": "Buy", "execFee": "0.01", "execId": id, "execPrice": "30000", "execQty": "0.001",
            "execType": exec_type, "execTime": time_ms.to_string(), "isMaker": true, "feeRate": "0.0001"
        })
    }

    fn params(r: &http_fake::Request) -> HashMap<String, String> {
        r.query()
            .split('&')
            .filter_map(|kv| kv.split_once('='))
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[tokio::test]
    async fn fills_are_read_in_windows_of_at_most_seven_days_each_declared_synced() {
        let fake = venue();
        let end = NOW_MS - LAG_MS;
        let since_ms = end - 10 * DAY_MS;
        fake.route(
            "/v5/execution/list",
            [
                ok(list(
                    vec![
                        execution("a", "Trade", since_ms + 5),
                        execution("f", "Funding", since_ms + 6),
                    ],
                    "",
                )),
                // The overlap returns "a" again; it is passed on once.
                ok(list(
                    vec![
                        execution("c", "AdlTrade", end - 1),
                        execution("u", "BlockTrade", end - 2),
                        execution("a", "Trade", since_ms + 5),
                    ],
                    "",
                )),
            ],
        );
        let a = queries(&fake)
            .answer(
                QueryReq::Fills {
                    since: since_ms * 1000,
                },
                SESSION,
            )
            .await;
        assert_eq!(a.failed, None);
        let kinds: Vec<&str> = a
            .inputs
            .iter()
            .map(|i| match i {
                VenueInput::Fill(_) => "fill",
                VenueInput::Funding(_) => "funding",
                VenueInput::PositionChange(_) => "change",
                VenueInput::UnclassifiedExecution { .. } => "unclassified",
                VenueInput::FillsSynced { .. } => "synced",
                _ => "other",
            })
            .collect();
        assert_eq!(
            kinds,
            vec![
                "fill",
                "funding",
                "synced",
                "change",
                "unclassified",
                "synced"
            ]
        );
        // Each query starts FILL_LOOKBACK_MS before `since`: a row the
        // venue lists late is read again.
        let start = since_ms - FILL_LOOKBACK_MS;
        let w1_end = start + WEEK_MS - OVERLAP_MS;
        assert_eq!(
            a.inputs[2],
            VenueInput::FillsSynced {
                session: SESSION,
                up_to: w1_end * 1000 - 1
            }
        );
        assert_eq!(
            a.inputs[5],
            VenueInput::FillsSynced {
                session: SESSION,
                up_to: end * 1000 - 1
            }
        );
        let reqs = fake.requests();
        assert_eq!(reqs.len(), 2);
        let (p1, p2) = (params(&reqs[0]), params(&reqs[1]));
        assert_eq!(
            (
                p1["category"].as_str(),
                p1["settleCoin"].as_str(),
                p1["limit"].as_str()
            ),
            ("linear", "USDT", "100")
        );
        assert_eq!(p1["startTime"], (start - 1).to_string());
        assert_eq!(p1["endTime"], w1_end.to_string());
        assert_eq!(p2["startTime"], (w1_end - 1).to_string());
        assert_eq!(p2["endTime"], end.to_string());
        for p in [&p1, &p2] {
            let span =
                p["endTime"].parse::<u64>().unwrap() - p["startTime"].parse::<u64>().unwrap();
            assert!(span <= WEEK_MS, "span {span}");
        }
    }

    #[tokio::test]
    async fn a_row_that_cannot_be_read_stops_the_sync_before_its_window_is_declared() {
        let fake = venue();
        let end = NOW_MS - LAG_MS;
        let since_ms = end - 10 * DAY_MS;
        fake.route(
            "/v5/execution/list",
            [
                ok(list(vec![execution("a", "Trade", since_ms + 5)], "")),
                ok(list(
                    vec![
                        execution("b", "Trade", end - 3),
                        with(execution("bad", "Trade", end - 2), "execQty", json!("much")),
                    ],
                    "",
                )),
            ],
        );
        let a = queries(&fake)
            .answer(
                QueryReq::Fills {
                    since: since_ms * 1000,
                },
                SESSION,
            )
            .await;
        assert!(matches!(a.failed, Some(QueryFail::Unreadable(_))));
        // The first window stays synced; the second is not declared.
        let synced: Vec<&VenueInput> = a
            .inputs
            .iter()
            .filter(|i| matches!(i, VenueInput::FillsSynced { .. }))
            .collect();
        assert_eq!(synced.len(), 1);
        assert!(
            matches!(a.inputs.last(), Some(VenueInput::Fill(f)) if f.key.as_str() == "BTCUSDT:b")
        );
    }

    #[tokio::test]
    async fn fills_follow_the_cursor_and_a_window_whose_last_page_cannot_be_told_is_split() {
        let end = NOW_MS - LAG_MS;
        let since_ms = end - DAY_MS;
        let full = |p: &str| {
            (0..LIMIT_EXECUTIONS)
                .map(|i| execution(&format!("{p}{i}"), "Trade", end - 10))
                .collect::<Vec<_>>()
        };
        let fake = venue();
        fake.route(
            "/v5/execution/list",
            [
                ok(list(full("x"), "next%3A1")),
                // Full and no cursor: split in two halves.
                ok(list(full("y"), "")),
                ok(list(vec![], "")),
                ok(list(vec![execution("z", "Trade", end - 10)], "")),
            ],
        );
        let a = queries(&fake)
            .answer(
                QueryReq::Fills {
                    since: since_ms * 1000,
                },
                SESSION,
            )
            .await;
        assert_eq!(a.failed, None);
        let reqs = fake.requests();
        assert_eq!(reqs.len(), 4);
        assert_eq!(params(&reqs[1])["cursor"], "next%3A1");
        let start = since_ms - FILL_LOOKBACK_MS;
        let mid = start + (end - start) / 2;
        assert_eq!(params(&reqs[2])["endTime"], mid.to_string());
        assert_eq!(params(&reqs[3])["startTime"], (mid - 1).to_string());
        let synced: Vec<Ts> = a
            .inputs
            .iter()
            .filter_map(|i| match i {
                VenueInput::FillsSynced { up_to, .. } => Some(*up_to),
                _ => None,
            })
            .collect();
        assert_eq!(synced, vec![mid * 1000 - 1, end * 1000 - 1]);
        let fills = a
            .inputs
            .iter()
            .filter(|i| matches!(i, VenueInput::Fill(_)))
            .count();
        assert_eq!(fills, 2 * LIMIT_EXECUTIONS + 1);
    }

    #[tokio::test]
    async fn a_long_gap_is_synced_over_several_queries_within_the_page_budget() {
        let fake = venue();
        fake.route("/v5/execution/list", [ok(list(vec![], ""))]);
        let end = NOW_MS - LAG_MS;
        let since_ms = end - 400 * DAY_MS;
        let a = queries(&fake)
            .answer(
                QueryReq::Fills {
                    since: since_ms * 1000,
                },
                SESSION,
            )
            .await;
        assert_eq!(a.failed, None);
        assert_eq!(fake.requests().len(), MAX_FILL_PAGES);
        let Some(VenueInput::FillsSynced { up_to, .. }) = a.inputs.last() else {
            panic!()
        };
        assert_eq!(
            *up_to,
            (since_ms - FILL_LOOKBACK_MS + MAX_FILL_PAGES as u64 * (WEEK_MS - OVERLAP_MS)) * 1000
                - 1
        );
        // Nothing to read yet.
        let fake = venue();
        let a = queries(&fake)
            .answer(QueryReq::Fills { since: end * 1000 }, SESSION)
            .await;
        assert_eq!(a, Answer::default());
        assert!(fake.requests().is_empty());
    }

    /// This unit reads, and cancels all only through `cancel_all`: no order
    /// create, amend or single cancel request exists in the gateway.
    #[test]
    fn the_gateway_has_no_order_request() {
        let sources = [
            include_str!("mod.rs"),
            include_str!("query.rs"),
            include_str!("exec.rs"),
            include_str!("rest.rs"),
            include_str!("checks.rs"),
            include_str!("sign.rs"),
            include_str!("stream.rs"),
            include_str!("gateway.rs"),
        ];
        // Built from parts, so that this test does not find itself.
        let mut needles: Vec<String> = [
            "create",
            "amend",
            "cancel\"",
            "create-batch",
            "amend-batch",
            "cancel-batch",
        ]
        .iter()
        .map(|op| format!("/v5/order/{op}"))
        .collect();
        needles.extend(
            ["create", "amend", "cancel"]
                .iter()
                .map(|op| format!("\"order.{op}")),
        );
        for s in sources {
            for needle in &needles {
                assert!(!s.contains(needle.as_str()), "{needle} in the gateway");
            }
        }
    }
}
