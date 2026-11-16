//! Hyperliquid gateway.
//!
//! Orders, cancels and queries go over HTTPS (`/exchange`, `/info`); order
//! updates and fills arrive on a WebSocket. The gateway implements the
//! contract in docs/contracts/gateway.md; the reasons for each conservative
//! choice are in docs/adr/ADR-017-hyperliquid-gateway.md.
//!
//! What has been checked, and how, is listed in docs/verification. In short:
//! signing against the official SDK's test vectors, parsing against the
//! documented examples, and read-only calls against testnet. Placing real
//! orders has not been exercised by the automated tests.

pub mod sign;
pub mod wire;

use super::{GatewayPorts, QueryReq, VenueInputTx, session_number};
use crate::config::HyperliquidConfig;
use crate::util::wall_us;
use crate::{error, info, warn};
use futures_util::{SinkExt, StreamExt};
use oms_core::*;
use sign::Signer;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, RwLock};
use std::time::{Duration, Instant};
use tokio::sync::Semaphore;
use tokio_tungstenite::tungstenite::Message;
use wire::*;

/// Why an HTTP exchange produced no usable answer.
#[derive(Debug)]
enum CallError {
    /// Nothing left this host.
    NotSent(String),
    /// The request may have been processed.
    NoAnswer(String),
}

#[derive(Clone, Copy, Debug)]
struct Asset {
    index: u32,
    sz_decimals: u32,
}

/// Request budget against the documented per-IP limit of 1200 weight per
/// minute. Orders stop first, queries next; cancels may use the reserve.
#[derive(Clone)]
struct Budget {
    tokens: f64,
    last: Instant,
}

const BUDGET_MAX: f64 = 1200.0;
const BUDGET_PER_SEC: f64 = 20.0;
const RESERVE_FOR_CANCELS: f64 = 100.0;
const RESERVE_FOR_ORDERS: f64 = 200.0;

/// The documentation gives the fill page size as 2000 in one place and 500
/// in another. Only a page shorter than the smaller figure is certainly the
/// last one.
const SMALLEST_FILL_PAGE: usize = 500;
/// The venue serves only this many of the most recent fills.
const FILLS_SERVED: usize = 10_000;
const MAX_FILL_PAGES: usize = 30;

#[derive(Debug, PartialEq, Eq)]
enum PageVerdict {
    /// Every fill in the queried range has been seen.
    Complete,
    /// Ask again from the time of the last fill.
    More,
    /// The range may hold fills the venue no longer serves, or more fills in
    /// one millisecond than a page shows. Completeness cannot be claimed.
    CannotTell(&'static str),
}

/// Decides what one page of `userFillsByTime` proves. `new` counts the fills
/// of this page not seen on an earlier page of the same synchronisation.
fn page_verdict(page_len: usize, new: usize, total_new: usize) -> PageVerdict {
    if total_new >= FILLS_SERVED {
        return PageVerdict::CannotTell("the backlog reaches the number of fills the venue serves");
    }
    if page_len < SMALLEST_FILL_PAGE {
        return PageVerdict::Complete;
    }
    if new == 0 {
        return PageVerdict::CannotTell("a full page of fills at one timestamp");
    }
    PageVerdict::More
}

impl Budget {
    fn refill(&mut self, now: Instant) {
        self.tokens = (self.tokens + now.duration_since(self.last).as_secs_f64() * BUDGET_PER_SEC).min(BUDGET_MAX);
        self.last = now;
    }
    fn take(&mut self, weight: f64, keep: f64) -> bool {
        self.take_at(Instant::now(), weight, keep)
    }
    /// `take` at a given time.
    fn take_at(&mut self, now: Instant, weight: f64, keep: f64) -> bool {
        self.refill(now);
        if self.tokens - weight >= keep {
            self.tokens -= weight;
            true
        } else {
            false
        }
    }
    /// One cancel. It may use the whole budget, the reserve included, and
    /// the gateway sends it whatever this returns: a cancel lowers risk.
    fn for_cancel(&mut self, now: Instant) -> bool {
        self.take_at(now, 1.0, 0.0)
    }
    /// One info query. It goes only while the reserve for orders and
    /// cancels stays untouched.
    fn for_query(&mut self, now: Instant, weight: f64) -> bool {
        self.take_at(now, weight, RESERVE_FOR_CANCELS + RESERVE_FOR_ORDERS)
    }
}

struct Gateway {
    cfg: HyperliquidConfig,
    rest: String,
    ws: String,
    account: String,
    http: reqwest::Client,
    signer: Signer,
    input: VenueInputTx,
    instance_id: u64,
    session: AtomicU64,
    nonce: AtomicU64,
    assets: RwLock<HashMap<String, Asset>>,
    instruments: BTreeMap<InstrumentId, InstrumentSpec>,
    budget: Mutex<Budget>,
}

fn now_ms() -> u64 {
    wall_us() / 1000
}

/// An error with its causes: the top-level text of an HTTP client error says
/// only that sending failed, not whether it was the name lookup, the
/// connection or the TLS handshake.
fn error_chain(e: &dyn std::error::Error) -> String {
    let mut text = e.to_string();
    let mut cause = e.source();
    while let Some(c) = cause {
        text.push_str(": ");
        text.push_str(&c.to_string());
        cause = c.source();
    }
    text
}

impl Gateway {
    /// Strictly increasing, and never behind the clock, as the venue's nonce
    /// rule requires.
    fn next_nonce(&self) -> u64 {
        let now = now_ms();
        let mut prev = self.nonce.load(Ordering::SeqCst);
        loop {
            let next = now.max(prev + 1);
            match self.nonce.compare_exchange(prev, next, Ordering::SeqCst, Ordering::SeqCst) {
                Ok(_) => return next,
                Err(p) => prev = p,
            }
        }
    }

    fn take_budget(&self, weight: f64, keep: f64) -> bool {
        self.budget.lock().unwrap_or_else(|e| e.into_inner()).take(weight, keep)
    }

    async fn post(&self, path: &str, body: &serde_json::Value) -> Result<serde_json::Value, CallError> {
        let resp = self
            .http
            .post(format!("{}{path}", self.rest))
            .json(body)
            .send()
            .await
            .map_err(|e| {
                // Only a failure to establish the connection proves that no
                // byte of the request was sent.
                let text = error_chain(&e);
                if e.is_connect() || e.is_builder() {
                    CallError::NotSent(text)
                } else {
                    CallError::NoAnswer(text)
                }
            })?;
        let status = resp.status();
        let text = resp.text().await.map_err(|e| CallError::NoAnswer(e.to_string()))?;
        if !status.is_success() {
            // The documentation does not define the status codes, so no
            // non-200 answer is taken as proof that the action was ignored.
            let short: String = text.chars().take(200).collect();
            return Err(CallError::NoAnswer(format!("HTTP {status}: {short}")));
        }
        serde_json::from_str(&text).map_err(|e| CallError::NoAnswer(format!("unreadable body: {e}")))
    }

    async fn info(&self, body: serde_json::Value, weight: f64) -> Option<serde_json::Value> {
        if !self.budget.lock().unwrap_or_else(|e| e.into_inner()).for_query(Instant::now(), weight) {
            return None;
        }
        match self.post("/info", &body).await {
            Ok(v) => Some(v),
            Err(e) => {
                warn!("info request failed", "type": body.get("type").cloned(), "error": format!("{e:?}"));
                None
            }
        }
    }

    async fn exchange<A: serde::Serialize>(&self, action: A, expires_after: Option<u64>) -> Result<serde_json::Value, CallError> {
        let nonce = self.next_nonce();
        let signature = self
            .signer
            .sign_action(&action, nonce, expires_after)
            .map_err(CallError::NotSent)?;
        let body = serde_json::to_value(ExchangeRequest {
            action,
            nonce,
            signature,
            vault_address: None,
            expires_after,
        })
        .map_err(|e| CallError::NotSent(e.to_string()))?;
        self.post("/exchange", &body).await
    }

    fn asset(&self, instrument: &InstrumentId) -> Option<Asset> {
        self.assets
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(instrument.as_str())
            .copied()
    }

    /// Loads asset indices and checks the configured instruments against the
    /// venue's own rules. Asset indices differ between mainnet and testnet,
    /// so they are never hard-coded.
    async fn load_meta(&self) -> Result<(), String> {
        let meta = self
            .post("/info", &serde_json::json!({"type": "meta"}))
            .await
            .map_err(|e| format!("{e:?}"))?;
        let universe = meta
            .get("universe")
            .and_then(|u| u.as_array())
            .ok_or("meta has no universe")?;
        let mut map = HashMap::new();
        for (index, a) in universe.iter().enumerate() {
            let (Some(name), Some(sz)) = (a.get("name").and_then(|n| n.as_str()), a.get("szDecimals").and_then(|n| n.as_u64())) else {
                continue;
            };
            map.insert(name.to_string(), Asset {
                index: index as u32,
                sz_decimals: sz as u32,
            });
        }
        let mut problems = Vec::new();
        for (id, spec) in &self.instruments {
            let Some(a) = map.get(id.as_str()) else {
                problems.push(format!("{id}: not in the venue's universe"));
                continue;
            };
            // Size has szDecimals places; price has at most 6 - szDecimals
            // places and 5 significant figures (integers exempt).
            let lot = 10i64.pow(8u32.saturating_sub(a.sz_decimals));
            let px_unit = 10i64.pow(8u32.saturating_sub(6u32.saturating_sub(a.sz_decimals)));
            if spec.qty_step.0 % lot != 0 {
                problems.push(format!("{id}: qty_step is finer than the venue's {} size decimals", a.sz_decimals));
            }
            if spec.px_tick.0 % px_unit != 0 {
                problems.push(format!("{id}: px_tick is finer than the venue allows for this asset"));
            }
            if spec.px_max_sig_figs != Some(5) {
                problems.push(format!("{id}: px_max_sig_figs must be 5 for this venue"));
            }
        }
        if !problems.is_empty() {
            return Err(problems.join("; "));
        }
        *self.assets.write().unwrap_or_else(|e| e.into_inner()) = map;
        Ok(())
    }

    // ---- commands ---------------------------------------------------------

    async fn send_order(&self, cmd: VenueCommand) {
        let VenueCommand::SendOrder {
            order_id,
            instance_id,
            instrument,
            side,
            qty,
            px,
            tif,
            reduce_only,
            expires_at,
        } = cmd
        else {
            return;
        };
        let started_at = wall_us();
        let cloid = VenueClientId { instance_id, order_id }.to_hex();
        let outcome = self
            .place(&cloid, &instrument, side, qty, px, tif, reduce_only, expires_at, started_at)
            .await;
        let timing = GatewayTiming {
            started_at,
            finished_at: wall_us(),
            wire_id: cloid,
        };
        self.input
            .send(VenueInput::SendResult {
                order_id,
                outcome,
                timing,
            })
            .await;
    }

    #[allow(clippy::too_many_arguments)]
    async fn place(
        &self,
        cloid: &str,
        instrument: &InstrumentId,
        side: Side,
        qty: Qty,
        px: Px,
        tif: Tif,
        reduce_only: bool,
        expires_at: Ts,
        now: Ts,
    ) -> SendOutcome {
        let not_sent = |reason: &str| SendOutcome::NotSent { reason: reason.into() };
        if now >= expires_at {
            return not_sent("the send window had passed before the gateway could send");
        }
        let Some(asset) = self.asset(instrument) else {
            return not_sent("the instrument is not in the venue's universe");
        };
        if !self.take_budget(1.0, RESERVE_FOR_CANCELS) {
            return not_sent("local request budget exhausted");
        }
        let action = OrderAction::single(OrderWire {
            a: asset.index,
            b: side == Side::Buy,
            p: px.to_string(),
            s: qty.to_string(),
            r: reduce_only,
            t: OrderTypeWire::limit(tif_wire(tif)),
            c: Some(cloid.to_string()),
        });
        // The venue refuses the action after this time (milliseconds).
        let expires_after = expires_at / 1000;
        match self.exchange(action, Some(expires_after)).await {
            Err(CallError::NotSent(e)) => SendOutcome::NotSent { reason: e },
            Err(CallError::NoAnswer(e)) => SendOutcome::Unknown { reason: e },
            Ok(body) => match parse_place_answer(&body) {
                PlaceAnswer::Resting { oid } => placed(oid, None),
                PlaceAnswer::Filled { oid, total } => placed(oid, total),
                PlaceAnswer::Refused { reason } => SendOutcome::Rejected { reason },
                PlaceAnswer::ActionError { reason } if self.cfg.trust_top_level_error => SendOutcome::Rejected { reason },
                PlaceAnswer::ActionError { reason } => SendOutcome::Unknown {
                    reason: format!("top-level error (not taken as proof of absence): {reason}"),
                },
                PlaceAnswer::Unclear { why } => SendOutcome::Unknown { reason: why },
            },
        }
    }

    async fn send_cancel(&self, cmd: VenueCommand) {
        let VenueCommand::SendCancel {
            order_id,
            instance_id,
            attempt,
            instrument,
            ..
        } = cmd
        else {
            return;
        };
        let started_at = wall_us();
        let cloid = VenueClientId { instance_id, order_id }.to_hex();
        let outcome = match self.asset(&instrument) {
            None => CancelOutcome::NotSent {
                reason: "the instrument is not in the venue's universe".into(),
            },
            Some(asset) => {
                // Cancels may use the whole budget, including the reserve,
                // and are sent also when it is empty.
                self.budget.lock().unwrap_or_else(|e| e.into_inner()).for_cancel(Instant::now());
                match self
                    .exchange(CancelByCloidAction::single(asset.index, cloid.clone()), None)
                    .await
                {
                    Err(CallError::NotSent(e)) => CancelOutcome::NotSent { reason: e },
                    Err(CallError::NoAnswer(e)) => CancelOutcome::Unknown { reason: e },
                    Ok(body) => match parse_cancel_answer(&body) {
                        CancelAnswer::Success => CancelOutcome::Confirmed,
                        CancelAnswer::Refused { reason } => CancelOutcome::Rejected { reason },
                        CancelAnswer::Unclear { why } => CancelOutcome::Unknown { reason: why },
                    },
                }
            }
        };
        let timing = GatewayTiming {
            started_at,
            finished_at: wall_us(),
            wire_id: cloid,
        };
        self.input
            .send(VenueInput::CancelResult {
                order_id,
                attempt,
                outcome,
                timing,
            })
            .await;
    }

    // ---- queries ----------------------------------------------------------

    async fn query(&self, q: QueryReq) {
        let session = self.session.load(Ordering::SeqCst);
        let queried_at = wall_us();
        match q {
            QueryReq::Status { order_id, .. } => {
                let cloid = VenueClientId {
                    instance_id: self.instance_id,
                    order_id,
                }
                .to_hex();
                let body = serde_json::json!({"type": "orderStatus", "user": self.account, "oid": cloid});
                let Some(resp) = self.info(body, 2.0).await else { return };
                match resp.get("status").and_then(|s| s.as_str()) {
                    Some("order") => {
                        let Some(u) = resp
                            .get("order")
                            .and_then(|o| serde_json::from_value::<HlOrderUpdate>(o.clone()).ok())
                        else {
                            return;
                        };
                        let status = match status(&u.status, u.status_timestamp) {
                            StatusMeaning::Known(st) => st,
                            StatusMeaning::Ignored => return,
                            StatusMeaning::Undocumented => {
                                error!("order status not in the documented table; nothing is concluded from it", "order_id": order_id.0, "status": u.status.clone());
                                return;
                            }
                        };
                        self.input
                            .send(VenueInput::OrderStatus(OrderStatusIn {
                                session,
                                order_id: Some(order_id),
                                venue_oid: venue_oid(u.order.oid),
                                status,
                                source: EvidenceSource::Query,
                            }))
                            .await;
                    }
                    Some("unknownOid") => {
                        self.input
                            .send(VenueInput::StatusNotFound {
                                session,
                                order_id,
                                queried_at,
                            })
                            .await;
                    }
                    _ => {}
                }
            }
            QueryReq::Fills { since } => self.sync_fills(session, since).await,
            QueryReq::OpenOrders => {
                let body = serde_json::json!({"type": "frontendOpenOrders", "user": self.account});
                let Some(resp) = self.info(body, 20.0).await else { return };
                let Ok(list) = serde_json::from_value::<Vec<HlOpenOrder>>(resp) else { return };
                let orders = list
                    .iter()
                    .filter_map(|o| {
                        Some(VenueOpenOrder {
                            venue_oid: venue_oid(o.oid)?,
                            order_id: our_order(o.cloid.as_deref(), self.instance_id),
                            instrument: instrument_id(&o.coin)?,
                            side: side(&o.side)?,
                            // The documentation states the meaning of `sz`
                            // both ways, so no remaining quantity is claimed.
                            remaining: None,
                        })
                    })
                    .collect();
                self.input
                    .send(VenueInput::OpenOrders {
                        session,
                        queried_at,
                        orders,
                    })
                    .await;
            }
            // The three queries of the margin rules (C-MARGIN). They only
            // read. No answer gives no input: the last report keeps ageing,
            // a setting or a mode stays unknown, and the core refuses plain
            // orders on that ground, which is the safe side.
            QueryReq::Margin { seq } => {
                let body = margin_request(&self.account);
                let Some(resp) = self.info(body, f64::from(WEIGHT_MARGIN_QUERY)).await else { return };
                // The whole report, or the reason it is refused as a whole.
                let input = margin_input(&resp, session, seq, queried_at);
                if let VenueInput::MarginReportInvalid { reason, .. } = &input {
                    warn!("the margin answer was refused as a whole", "seq": seq, "reason": reason.clone());
                }
                self.input.send(input).await;
            }
            QueryReq::VenueSetting { instrument } => {
                let body = venue_setting_request(&self.account, instrument.as_str());
                let Some(resp) = self.info(body, f64::from(WEIGHT_OTHER_INFO_QUERY)).await else { return };
                match venue_setting_input(&resp, session, &instrument) {
                    Some(input) => {
                        self.input.send(input).await;
                    }
                    None => warn!("the venue-setting answer could not be read; the setting stays unknown", "instrument": instrument.to_string()),
                }
            }
            QueryReq::AccountMode => {
                let body = account_mode_request(&self.account);
                let Some(resp) = self.info(body, f64::from(WEIGHT_OTHER_INFO_QUERY)).await else { return };
                match account_mode_input(&resp, session) {
                    Some(input) => {
                        self.input.send(input).await;
                    }
                    None => warn!("the account-mode answer could not be read; the mode stays unknown"),
                }
            }
            QueryReq::Positions => {
                let body = serde_json::json!({"type": "clearinghouseState", "user": self.account});
                let Some(resp) = self.info(body, 2.0).await else { return };
                let Some(list) = resp.get("assetPositions").and_then(|a| a.as_array()) else {
                    return;
                };
                let positions = list
                    .iter()
                    .filter_map(|p| {
                        let pos = p.get("position")?;
                        Some(VenuePosition {
                            instrument: instrument_id(pos.get("coin")?.as_str()?)?,
                            qty: SQty::parse(pos.get("szi")?.as_str()?).ok()?,
                        })
                    })
                    .collect();
                self.input
                    .send(VenueInput::Positions {
                        session,
                        queried_at,
                        positions,
                    })
                    .await;
            }
        }
    }

    /// True for a coin of the perpetuals universe this gateway trades in.
    /// Fills on anything else (spot pairs, other dexes) are outside the scope
    /// of this OMS: they change no perpetual position.
    fn is_perp(&self, coin: &str) -> bool {
        self.assets.read().unwrap_or_else(|e| e.into_inner()).contains_key(coin)
    }

    /// Fetches every fill from `since` up to a time safely in the past, then
    /// declares fills complete up to just before that time. Completeness is
    /// declared only when a page proves it (see `page_verdict`).
    async fn sync_fills(&self, session: u64, since: Ts) {
        let end = now_ms().saturating_sub(self.cfg.fills_lag_ms);
        let mut start = since / 1000;
        if end <= start {
            return;
        }
        let mut seen: HashSet<(u64, u64, u64)> = HashSet::new();
        let mut total_new = 0usize;
        let mut out_of_scope = 0usize;
        for _page in 0..MAX_FILL_PAGES {
            let body = serde_json::json!({
                "type": "userFillsByTime", "user": self.account,
                "startTime": start, "endTime": end, "aggregateByTime": false,
            });
            let Some(resp) = self.info(body, 20.0).await else { return };
            let Ok(fills) = serde_json::from_value::<Vec<HlFill>>(resp) else {
                error!("the fills answer did not parse; fills are not declared complete");
                return;
            };
            let last = fills.iter().map(|f| f.time).max().unwrap_or(start);
            let mut new = 0usize;
            for f in &fills {
                if seen.insert((f.time, f.tid, f.oid)) {
                    new += 1;
                }
                if !self.is_perp(&f.coin) {
                    out_of_scope += 1;
                    continue;
                }
                match fill_input(f, self.instance_id) {
                    Some(i) => {
                        self.input.send(VenueInput::Fill(i)).await;
                    }
                    None => {
                        // A fill that cannot be represented must not be
                        // counted as delivered.
                        error!("a fill could not be translated; fills are not declared complete", "tid": f.tid, "coin": f.coin.clone());
                        return;
                    }
                }
            }
            total_new += new;
            match page_verdict(fills.len(), new, total_new) {
                PageVerdict::Complete => {
                    if out_of_scope > 0 {
                        warn!("fills outside the perpetuals universe were not booked", "count": out_of_scope);
                    }
                    self.input
                        .send(VenueInput::FillsSynced {
                            session,
                            up_to: (end * 1000).saturating_sub(1),
                        })
                        .await;
                    return;
                }
                PageVerdict::More => start = last, // inclusive on both ends: the overlap is deduplicated
                PageVerdict::CannotTell(why) => {
                    error!("fills are not declared complete", "why": why, "fetched": total_new);
                    return;
                }
            }
        }
        error!("fills are not declared complete", "why": "too many pages", "fetched": total_new);
    }

    // ---- stream -----------------------------------------------------------

    async fn stream_loop(self: Arc<Self>, boot: u32) {
        let mut connection = 0u32;
        let mut backoff = Duration::from_millis(500);
        loop {
            if self.assets.read().unwrap_or_else(|e| e.into_inner()).is_empty() {
                if let Err(e) = self.load_meta().await {
                    error!("venue metadata unusable; the link stays down", "error": e);
                    tokio::time::sleep(Duration::from_secs(10)).await;
                    continue;
                }
            }
            match self.run_stream(boot, &mut connection).await {
                Ok(()) => backoff = Duration::from_millis(500),
                Err(e) => warn!("venue stream ended", "error": e),
            }
            tokio::time::sleep(backoff).await;
            backoff = (backoff * 2).min(Duration::from_secs(10));
        }
    }

    async fn run_stream(&self, boot: u32, connection: &mut u32) -> Result<(), String> {
        let (ws, _) = tokio_tungstenite::connect_async(self.ws.as_str())
            .await
            .map_err(|e| e.to_string())?;
        let (mut write, mut read) = ws.split();
        for sub in [
            serde_json::json!({"type": "orderUpdates", "user": self.account}),
            serde_json::json!({"type": "userFills", "user": self.account}),
            serde_json::json!({"type": "allMids"}),
        ] {
            let msg = serde_json::json!({"method": "subscribe", "subscription": sub}).to_string();
            write.send(Message::text(msg)).await.map_err(|e| e.to_string())?;
        }
        *connection += 1;
        let session = session_number(boot, *connection);
        let mut acked = 0u8;
        let mut up = false;
        let subscribe_by = Instant::now() + Duration::from_secs(15);
        let mut ping = tokio::time::interval(Duration::from_secs(50));
        let mut last_ref: HashMap<String, (Px, Instant)> = HashMap::new();
        let reason = loop {
            let msg = tokio::select! {
                // The venue closes a connection it has not written to for 60 s.
                m = tokio::time::timeout(Duration::from_secs(75), read.next()) => match m {
                    Err(_) => break "no message for 75 s".to_string(),
                    Ok(None) => break "closed by the venue".to_string(),
                    Ok(Some(Err(e))) => break format!("read failed: {e}"),
                    Ok(Some(Ok(m))) => m,
                },
                _ = ping.tick() => {
                    if let Err(e) = write.send(Message::text(r#"{"method":"ping"}"#)).await {
                        break format!("ping failed: {e}");
                    }
                    continue;
                }
            };
            // Market data keeps the connection busy, so silence cannot be
            // relied on to notice that the user streams never started.
            if !up && Instant::now() > subscribe_by {
                break "the user streams were not confirmed within 15 s".to_string();
            }
            let Message::Text(text) = msg else { continue };
            let Ok(v) = serde_json::from_str::<serde_json::Value>(text.as_str()) else {
                continue;
            };
            let data = v.get("data");
            match v.get("channel").and_then(|c| c.as_str()) {
                Some("error") => {
                    // A refused subscription or request. The streams cannot be
                    // trusted to be complete after it; start over.
                    break format!("venue reported a stream error: {}", data.map(|d| d.to_string()).unwrap_or_default());
                }
                Some("subscriptionResponse") => {
                    let kind = data.and_then(|d| d.pointer("/subscription/type")).and_then(|t| t.as_str());
                    if matches!(kind, Some("orderUpdates") | Some("userFills")) {
                        acked += 1;
                    }
                    if acked >= 2 && !up {
                        up = true;
                        self.session.store(session, Ordering::SeqCst);
                        info!("venue stream subscribed", "session": session);
                        self.input
                            .send(VenueInput::Link {
                                up: true,
                                session,
                                reason: "user streams subscribed".into(),
                            })
                            .await;
                    }
                }
                Some("orderUpdates") => {
                    let Some(list) = data.and_then(|d| serde_json::from_value::<Vec<HlOrderUpdate>>(d.clone()).ok()) else {
                        // Dropping it would lose an order update without a
                        // trace. Reconnecting makes the reconciler re-read
                        // every open order over the query path instead.
                        break "an orderUpdates message could not be read".to_string();
                    };
                    for u in list {
                        let status = match status(&u.status, u.status_timestamp) {
                            StatusMeaning::Known(st) => st,
                            StatusMeaning::Ignored => continue,
                            StatusMeaning::Undocumented => {
                                error!("order status not in the documented table; nothing is concluded from it", "oid": u.order.oid, "status": u.status.clone());
                                continue;
                            }
                        };
                        self.input
                            .send(VenueInput::OrderStatus(OrderStatusIn {
                                session,
                                order_id: our_order(u.order.cloid.as_deref(), self.instance_id),
                                venue_oid: venue_oid(u.order.oid),
                                status,
                                source: EvidenceSource::Stream,
                            }))
                            .await;
                    }
                }
                Some("userFills") => {
                    let Some(fills) = data
                        .and_then(|d| d.get("fills"))
                        .and_then(|f| serde_json::from_value::<Vec<HlFill>>(f.clone()).ok())
                    else {
                        // As above: the fill query after a reconnect is the
                        // path that refuses to call fills complete.
                        break "a userFills message could not be read".to_string();
                    };
                    // The first message after subscribing is a snapshot of
                    // recent fills. It goes through the same path; the core
                    // drops what it has already booked.
                    let mut unusable = false;
                    for f in &fills {
                        if !self.is_perp(&f.coin) {
                            warn!("a fill outside the perpetuals universe was not booked", "coin": f.coin.clone(), "tid": f.tid);
                            continue;
                        }
                        match fill_input(f, self.instance_id) {
                            Some(i) => {
                                self.input.send(VenueInput::Fill(i)).await;
                            }
                            None => {
                                error!("a stream fill could not be translated", "tid": f.tid, "coin": f.coin.clone());
                                unusable = true;
                            }
                        }
                    }
                    if unusable {
                        break "a stream fill could not be translated".to_string();
                    }
                }
                Some("allMids") => {
                    let Some(mids) = data.and_then(|d| d.get("mids")).and_then(|m| m.as_object()) else {
                        continue;
                    };
                    for id in self.instruments.keys() {
                        let Some(px) = mids.get(id.as_str()).and_then(|p| p.as_str()).and_then(|p| Px::parse(p).ok()) else {
                            continue;
                        };
                        let due = last_ref.get(id.as_str()).is_none_or(|(_, at)| {
                            at.elapsed() >= Duration::from_millis(self.cfg.ref_price_interval_ms)
                        });
                        if due && up {
                            last_ref.insert(id.to_string(), (px, Instant::now()));
                            self.input
                                .send(VenueInput::RefPrice {
                                    instrument: id.clone(),
                                    px,
                                    venue_ts: wall_us(),
                                })
                                .await;
                        }
                    }
                }
                _ => {}
            }
        };
        if up {
            self.input
                .send(VenueInput::Link {
                    up: false,
                    session,
                    reason: reason.clone(),
                })
                .await;
        }
        Err(reason)
    }

    /// Keeps the venue's scheduled cancel a fixed time ahead. If this process
    /// stops refreshing it, the venue cancels every open order at that time.
    async fn dead_man_switch(self: Arc<Self>) {
        let secs = self.cfg.schedule_cancel_secs;
        if secs == 0 {
            return;
        }
        let mut tick = tokio::time::interval(Duration::from_secs((secs / 3).max(2)));
        loop {
            tick.tick().await;
            if self.session.load(Ordering::SeqCst) == 0 {
                continue;
            }
            let action = ScheduleCancel {
                r#type: "scheduleCancel",
                time: Some(now_ms() + secs * 1000),
            };
            self.take_budget(1.0, 0.0);
            match self.exchange(action, None).await {
                Err(e) => warn!("could not refresh the scheduled cancel", "error": format!("{e:?}")),
                Ok(body) if body.get("status").and_then(|s| s.as_str()) == Some("ok") => {}
                // The venue allows a limited number of triggers per day and
                // has account requirements; a refusal means the switch is
                // not armed, which the operator must not have to guess.
                Ok(body) => error!("the venue refused the scheduled cancel; the dead man's switch is NOT armed", "answer": body.to_string()),
            }
        }
    }
}

fn placed(oid: u64, reported: Option<Qty>) -> SendOutcome {
    match venue_oid(oid) {
        Some(v) => SendOutcome::Placed {
            venue_oid: v,
            // "Open" with the quantity reported so far. Whether the order is
            // already complete is learned from fills and order updates.
            status: StatusIn {
                status: VenueStatus::Open,
                cum: reported,
                venue_ts: wall_us(),
            },
        },
        None => SendOutcome::Unknown {
            reason: "unusable venue order id".into(),
        },
    }
}

/// Starts the gateway, taking the signing key from the environment variable
/// named in the configuration. Fails if it is missing or unusable.
pub fn spawn(
    cfg: HyperliquidConfig,
    instruments: BTreeMap<InstrumentId, InstrumentSpec>,
    ports: GatewayPorts,
    max_concurrent: usize,
) -> Result<String, String> {
    let key = std::env::var(&cfg.agent_key_env)
        .map_err(|_| format!("environment variable {} is not set", cfg.agent_key_env))?;
    spawn_with_key(cfg, &key, instruments, ports, max_concurrent)
}

/// Starts the gateway with the given private key (hex).
pub fn spawn_with_key(
    cfg: HyperliquidConfig,
    key: &str,
    instruments: BTreeMap<InstrumentId, InstrumentSpec>,
    mut ports: GatewayPorts,
    max_concurrent: usize,
) -> Result<String, String> {
    let mainnet = cfg.network == "mainnet";
    let (rest, ws) = if mainnet {
        ("https://api.hyperliquid.xyz", "wss://api.hyperliquid.xyz/ws")
    } else {
        ("https://api.hyperliquid-testnet.xyz", "wss://api.hyperliquid-testnet.xyz/ws")
    };
    let signer = Signer::new(key, mainnet)?;
    let account = cfg.account_address.to_lowercase();
    if !(account.starts_with("0x") && account.len() == 42 && account[2..].chars().all(|c| c.is_ascii_hexdigit())) {
        return Err("account_address is not a 20-byte hex address".into());
    }
    let http = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(3))
        .timeout(Duration::from_millis(cfg.request_timeout_ms))
        // A redirect would re-send the signed action. Nothing here follows one.
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| e.to_string())?;
    let agent = signer.address().to_string();
    let gw = Arc::new(Gateway {
        rest: rest.to_string(),
        ws: ws.to_string(),
        account,
        http,
        signer,
        input: ports.input.clone(),
        instance_id: ports.instance_id,
        session: AtomicU64::new(0),
        nonce: AtomicU64::new(0),
        assets: RwLock::new(HashMap::new()),
        instruments,
        budget: Mutex::new(Budget {
            tokens: BUDGET_MAX / 2.0,
            last: Instant::now(),
        }),
        cfg,
    });
    info!("hyperliquid gateway starting", "network": gw.cfg.network.clone(), "account": gw.account.clone(), "agent": agent.clone());

    tokio::spawn(gw.clone().stream_loop(ports.boot));
    tokio::spawn(gw.clone().dead_man_switch());

    let cmd_gw = gw.clone();
    tokio::spawn(async move {
        let permits = Arc::new(Semaphore::new(max_concurrent));
        while let Some(cmd) = ports.commands.recv().await {
            let Ok(permit) = permits.clone().acquire_owned().await else {
                return;
            };
            let gw = cmd_gw.clone();
            tokio::spawn(async move {
                match &cmd {
                    VenueCommand::SendOrder { .. } => gw.send_order(cmd).await,
                    VenueCommand::SendCancel { .. } => gw.send_cancel(cmd).await,
                }
                drop(permit);
            });
        }
    });

    let q_gw = gw.clone();
    tokio::spawn(async move {
        let permits = Arc::new(Semaphore::new(4));
        while let Some(q) = ports.queries.recv().await {
            let Ok(permit) = permits.clone().acquire_owned().await else {
                return;
            };
            let gw = q_gw.clone();
            tokio::spawn(async move {
                gw.query(q).await;
                drop(permit);
            });
        }
    });
    Ok(format!("hyperliquid:{}", gw.cfg.network))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_budget_stops_orders_before_cancels() {
        let mut b = Budget {
            tokens: RESERVE_FOR_CANCELS + 1.5,
            last: Instant::now(),
        };
        // One order fits above the cancel reserve, the next does not.
        assert!(b.take(1.0, RESERVE_FOR_CANCELS));
        assert!(!b.take(1.0, RESERVE_FOR_CANCELS));
        // A cancel may dip into the reserve.
        assert!(b.take(1.0, 0.0));
    }

    /// G7. Entering REDUCING can send a cancel for every open order at
    /// once, at most 500 (the open-order limit). A cancel is never held
    /// back by the local budget, so the queries pay for the burst: a query
    /// goes only while 300 units stay in reserve for orders and cancels,
    /// and the budget refills at 20 units per second up to 1200.
    ///
    /// Hand-worked, for the margin query (weight 2, so it needs 302):
    /// - full budget, 1200: 500 cancels leave 700; the query goes at once.
    /// - budget at start-up, 600: 500 cancels leave 100; the query needs
    ///   202 more, 202 / 20 = 10.1 s.
    /// - empty budget: the cancels leave 0; 302 / 20 = 15.1 s. This is the
    ///   longest wait, and it is shorter than the 30 s after which a
    ///   margin report stops being fresh.
    #[test]
    fn a_full_cancel_burst_leaves_the_margin_query_waiting_for_a_known_time() {
        let t0 = Instant::now();
        let after_burst = |tokens: f64| {
            let mut b = Budget { tokens, last: t0 };
            for _ in 0..500 {
                // What the gateway does for one cancel.
                b.for_cancel(t0);
            }
            b
        };
        // What the gateway does for the margin query, `ms` after the burst.
        let margin_query_goes = |b: &Budget, ms: u64| b.clone().for_query(t0 + Duration::from_millis(ms), 2.0);
        // Exactly `left` units are in the budget: all of them can be taken
        // and one more cannot.
        let holds = |b: &Budget, left: f64| b.clone().take_at(t0, left, 0.0) && !b.clone().take_at(t0, left + 1.0, 0.0);

        let full = after_burst(1200.0);
        assert!(holds(&full, 700.0));
        assert!(margin_query_goes(&full, 0));

        let start_up = after_burst(600.0);
        assert!(holds(&start_up, 100.0));
        assert!(!margin_query_goes(&start_up, 0));
        assert!(!margin_query_goes(&start_up, 10_099));
        assert!(margin_query_goes(&start_up, 10_101));

        let empty = after_burst(0.0);
        assert!(holds(&empty, 0.0));
        assert!(!margin_query_goes(&empty, 15_099));
        assert!(margin_query_goes(&empty, 15_101));
        // The refill stops at 1200 however long nothing is sent.
        let mut idle = empty.clone();
        assert!(idle.take_at(t0 + Duration::from_secs(3600), 1200.0, 0.0));
        assert!(!idle.take_at(t0 + Duration::from_secs(3600), 1.0, 0.0));
    }

    #[test]
    fn a_fill_page_proves_completeness_only_when_it_is_certainly_the_last() {
        use PageVerdict::*;
        // Nothing in the range.
        assert_eq!(page_verdict(0, 0, 0), Complete);
        // Shorter than the smallest documented page size: the last page.
        assert_eq!(page_verdict(499, 499, 499), Complete);
        // Could be a full page under the 500 reading of the documentation.
        assert_eq!(page_verdict(500, 500, 500), More);
        assert_eq!(page_verdict(2000, 2000, 2000), More);
        // The follow-up page that only repeats the boundary fills.
        assert_eq!(page_verdict(3, 0, 2000), Complete);
        // A full page of nothing new: more fills at one timestamp than a page shows.
        assert!(matches!(page_verdict(500, 0, 2000), CannotTell(_)));
        // Five full pages of 2000: the sixth holds only the overlap, yet older
        // fills may have fallen out of what the venue serves.
        assert!(matches!(page_verdict(1, 0, 10_000), CannotTell(_)));
        assert!(matches!(page_verdict(2000, 2000, 10_000), CannotTell(_)));
    }
}
