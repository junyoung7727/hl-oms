//! A simulated venue: the ground truth the OMS cannot see directly.
//!
//! It is deliberately not a matching engine. Fills are caused by the test
//! driver ("fill this much of that order now"), which lets a test produce any
//! partial-fill pattern and any race without modelling a market. What it does
//! model faithfully is the contract the OMS depends on:
//!
//! - an order is identified by the client id the OMS presents;
//! - a request that arrives after its expiry is refused and never placed;
//! - a cancel removes only what is still open;
//! - status, open orders, positions and fills can be queried at any time.
//!
//! It also records what a correct OMS must never cause, such as two placement
//! requests with the same client id.

use oms_core::*;
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SimStatus {
    Open,
    Filled,
    Canceled(String),
}

#[derive(Clone, Debug)]
pub struct SimOrder {
    pub oid: u64,
    pub client: Option<VenueClientId>,
    pub instrument: InstrumentId,
    pub side: Side,
    pub qty: Qty,
    pub px: Px,
    pub filled: Qty,
    pub status: SimStatus,
    pub updated_at: Ts,
}

impl SimOrder {
    pub fn remaining(&self) -> Qty {
        Qty(self.qty.0 - self.filled.0)
    }
    pub fn venue_oid(&self) -> VenueOrderId {
        oid_string(self.oid)
    }
}

pub fn oid_string(oid: u64) -> VenueOrderId {
    VenueOrderId::new(format!("v{oid}")).expect("\"v<digits>\" is a valid venue order id")
}

#[derive(Clone, Debug)]
pub struct SimFill {
    pub tid: u64,
    pub oid: u64,
    pub client: Option<VenueClientId>,
    pub instrument: InstrumentId,
    pub side: Side,
    pub qty: Qty,
    pub px: Px,
    pub fee: Amount,
    pub ts: Ts,
}

#[derive(Clone, Debug)]
pub enum StreamEvent {
    Order { oid: u64, client: Option<VenueClientId>, status: SimStatus, filled: Qty, ts: Ts },
    Fill(SimFill),
}

#[derive(Clone, Debug)]
pub struct PlaceReq {
    pub client: VenueClientId,
    pub instrument: InstrumentId,
    pub side: Side,
    pub qty: Qty,
    pub px: Px,
    pub tif: Tif,
    pub expires_at: Ts,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlaceResp {
    /// The order exists (or existed). `status` is its state right after the
    /// request was processed.
    Placed { oid: u64, status: SimStatus, filled: Qty, ts: Ts },
    Rejected { reason: String },
}

#[derive(Clone, Debug, Default)]
pub struct SimVenue {
    pub orders: BTreeMap<u64, SimOrder>,
    pub by_client: BTreeMap<u128, u64>,
    pub positions: BTreeMap<InstrumentId, i64>,
    /// Position change caused by activity that did not come from the OMS.
    pub external_positions: BTreeMap<InstrumentId, i64>,
    pub fills: Vec<SimFill>,
    pub stream: Vec<StreamEvent>,
    pub next_oid: u64,
    pub next_tid: u64,
    pub enforce_expiry: bool,
    /// Fee charged per fill, in basis points of notional.
    pub fee_bps: i64,
    /// Placement requests seen per client id, including refused ones.
    pub place_requests: BTreeMap<u128, u32>,
    /// Client ids that produced more than one live order. Must stay empty.
    pub duplicate_placements: Vec<u128>,
    /// Account value the venue states in a margin report. A control knob:
    /// nothing in this model moves it.
    pub account_value: Amount,
}

impl SimVenue {
    pub fn new(enforce_expiry: bool) -> SimVenue {
        SimVenue {
            next_oid: 1000,
            next_tid: 1,
            enforce_expiry,
            fee_bps: 2,
            ..SimVenue::default()
        }
    }

    fn emit_order(&mut self, oid: u64, now: Ts) {
        if let Some(o) = self.orders.get(&oid) {
            self.stream.push(StreamEvent::Order {
                oid,
                client: o.client,
                status: o.status.clone(),
                filled: o.filled,
                ts: now,
            });
        }
    }

    /// Processes a placement request at venue time `now`. `immediate` is how
    /// much trades at once (the driver's choice; clamped to the quantity).
    pub fn place(&mut self, now: Ts, req: &PlaceReq, immediate: Qty) -> PlaceResp {
        let key = req.client.to_u128();
        *self.place_requests.entry(key).or_default() += 1;
        if self.enforce_expiry && now > req.expires_at {
            return PlaceResp::Rejected { reason: "request expired".into() };
        }
        if self.by_client.contains_key(&key) {
            // The real venue's behaviour here is undocumented. The simulator
            // places a second order, the outcome an OMS must never provoke,
            // and records it.
            self.duplicate_placements.push(key);
        }
        let immediate = immediate.min(req.qty);
        match req.tif {
            Tif::Ioc if immediate.is_zero() => {
                return PlaceResp::Rejected { reason: "could not immediately match".into() };
            }
            Tif::Alo if !immediate.is_zero() => {
                return PlaceResp::Rejected { reason: "post-only order would cross".into() };
            }
            _ => {}
        }
        let oid = self.next_oid;
        self.next_oid += 1;
        self.orders.insert(oid, SimOrder {
            oid,
            client: Some(req.client),
            instrument: req.instrument.clone(),
            side: req.side,
            qty: req.qty,
            px: req.px,
            filled: Qty::ZERO,
            status: SimStatus::Open,
            updated_at: now,
        });
        self.by_client.insert(key, oid);
        self.emit_order(oid, now);
        if !immediate.is_zero() {
            self.fill(now, oid, immediate);
        }
        if req.tif == Tif::Ioc {
            self.cancel_oid(now, oid, "ioc remainder");
        }
        let o = &self.orders[&oid];
        PlaceResp::Placed { oid, status: o.status.clone(), filled: o.filled, ts: now }
    }

    /// Trades `qty` of an open order at its limit price. Returns the fill, or
    /// `None` if the order is not open.
    pub fn fill(&mut self, now: Ts, oid: u64, qty: Qty) -> Option<SimFill> {
        let o = self.orders.get_mut(&oid)?;
        if o.status != SimStatus::Open || qty.0 <= 0 {
            return None;
        }
        let qty = qty.min(o.remaining());
        if qty.is_zero() {
            return None;
        }
        o.filled = Qty(o.filled.0 + qty.0);
        o.updated_at = now;
        let notional = num_notional(o.px, qty);
        let fill = SimFill {
            tid: self.next_tid,
            oid,
            client: o.client,
            instrument: o.instrument.clone(),
            side: o.side,
            qty,
            px: o.px,
            fee: Amount(notional * self.fee_bps / 10_000),
            ts: now,
        };
        self.next_tid += 1;
        if o.filled == o.qty {
            o.status = SimStatus::Filled;
        }
        *self.positions.entry(fill.instrument.clone()).or_default() += qty.signed(fill.side).0;
        self.fills.push(fill.clone());
        self.stream.push(StreamEvent::Fill(fill.clone()));
        self.emit_order(oid, now);
        Some(fill)
    }

    fn cancel_oid(&mut self, now: Ts, oid: u64, reason: &str) -> bool {
        let Some(o) = self.orders.get_mut(&oid) else { return false };
        if o.status != SimStatus::Open {
            return false;
        }
        o.status = SimStatus::Canceled(reason.to_string());
        o.updated_at = now;
        self.emit_order(oid, now);
        true
    }

    /// A cancel request by client id.
    pub fn cancel(&mut self, now: Ts, client: VenueClientId) -> Result<(), String> {
        match self.by_client.get(&client.to_u128()).copied() {
            Some(oid) if self.cancel_oid(now, oid, "cancelled by user") => Ok(()),
            _ => Err("order was never placed, already cancelled, or filled".into()),
        }
    }

    /// A cancel the OMS did not ask for (margin, self-trade prevention, …).
    pub fn venue_cancel(&mut self, now: Ts, oid: u64, reason: &str) -> bool {
        self.cancel_oid(now, oid, reason)
    }

    /// A trade on the account that the OMS did not originate.
    pub fn external_fill(&mut self, now: Ts, instrument: &InstrumentId, side: Side, qty: Qty, px: Px) -> SimFill {
        let oid = self.next_oid;
        self.next_oid += 1;
        let fill = SimFill {
            tid: self.next_tid,
            oid,
            client: None,
            instrument: instrument.clone(),
            side,
            qty,
            px,
            fee: Amount::ZERO,
            ts: now,
        };
        self.next_tid += 1;
        let signed = qty.signed(side).0;
        *self.positions.entry(instrument.clone()).or_default() += signed;
        *self.external_positions.entry(instrument.clone()).or_default() += signed;
        self.fills.push(fill.clone());
        self.stream.push(StreamEvent::Fill(fill.clone()));
        fill
    }

    pub fn status(&self, client: VenueClientId) -> Option<&SimOrder> {
        self.by_client.get(&client.to_u128()).and_then(|oid| self.orders.get(oid))
    }

    pub fn open_orders(&self) -> Vec<&SimOrder> {
        self.orders.values().filter(|o| o.status == SimStatus::Open).collect()
    }

    pub fn open_oids(&self) -> Vec<u64> {
        self.open_orders().iter().map(|o| o.oid).collect()
    }

    pub fn position(&self, i: &InstrumentId) -> i64 {
        self.positions.get(i).copied().unwrap_or(0)
    }

    pub fn fills_since(&self, since: Ts) -> Vec<SimFill> {
        self.fills.iter().filter(|f| f.ts >= since).cloned().collect()
    }

    pub fn take_stream(&mut self) -> Vec<StreamEvent> {
        std::mem::take(&mut self.stream)
    }
}

fn num_notional(px: Px, qty: Qty) -> i64 {
    oms_core::num::notional_round(px, qty).map(|a| a.0).unwrap_or(0)
}

/// Translates the venue's view of one order into the core's status input.
pub fn status_in(status: &SimStatus, filled: Qty, ts: Ts, report_cum: bool) -> StatusIn {
    let cum = report_cum.then_some(filled);
    let status = match status {
        SimStatus::Open => VenueStatus::Open,
        SimStatus::Filled => VenueStatus::Filled,
        SimStatus::Canceled(r) => VenueStatus::Canceled { reason: r.clone() },
    };
    StatusIn { status, cum, venue_ts: ts }
}

pub fn fill_in(f: &SimFill, instance_id: u64, with_client_id: bool) -> FillIn {
    let order_id = f
        .client
        .filter(|c| with_client_id && c.instance_id == instance_id)
        .map(|c| c.order_id);
    FillIn {
        key: FillKey::new(format!("t{}", f.tid)).expect("\"t<digits>\" is a valid fill key"),
        order_id,
        venue_oid: Some(oid_string(f.oid)),
        instrument: f.instrument.clone(),
        side: f.side,
        qty: f.qty,
        px: f.px,
        fee: f.fee,
        venue_ts: f.ts,
    }
}
