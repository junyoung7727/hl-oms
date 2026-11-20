//! The authoritative state and the only code that mutates it.
//!
//! `Core::apply` is total with respect to decisions: it performs the mutation
//! an event describes and checks only structural consistency (the order
//! exists, arithmetic fits). It is used unchanged by live processing and by
//! recovery, which is what makes a replayed state equal to the live one.

use crate::config::CoreConfig;
use crate::event::*;
use crate::ids::*;
use crate::model::*;
use crate::num::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// An internal inconsistency. The process must stop: the in-memory state is
/// no longer trustworthy, and the journal still holds the last good state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreFault(pub String);

impl fmt::Display for CoreFault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "core fault: {}", self.0)
    }
}
impl std::error::Error for CoreFault {}

pub(crate) fn fault<T>(msg: impl Into<String>) -> Result<T, CoreFault> {
    Err(CoreFault(msg.into()))
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Core {
    /// Fixed when the data directory is created. Namespaces the client ids
    /// this instance presents to the venue.
    pub instance_id: u64,
    pub config: CoreConfig,
    pub boot: u32,
    pub link: Link,
    pub next_order_id: u64,
    pub orders: BTreeMap<OrderId, Order>,
    /// Orders that are not closed. Bounded by `policy.max_open_orders`.
    pub open: BTreeSet<OrderId>,
    pub inflight_sends: u32,
    /// Idempotency index: one order per (strategy, client order id).
    pub by_client: BTreeMap<StrategyId, BTreeMap<ClientOrderId, OrderId>>,
    pub by_venue_oid: BTreeMap<VenueOrderId, OrderId>,
    pub account: BTreeMap<InstrumentId, Book>,
    pub strategy_books: BTreeMap<StrategyId, BTreeMap<InstrumentId, Book>>,
    pub unattributed: BTreeMap<InstrumentId, Book>,
    pub parked_fills: Vec<ParkedFill>,
    /// Every booked fill key with its venue time, kept independently of the
    /// orders so that a fill re-delivered after its order was purged is still
    /// recognised.
    pub seen_fills: BTreeMap<FillKey, Ts>,
    /// Venue time below which every fill is known to have been delivered
    /// already. A fill older than this is a re-delivery and is ignored.
    pub fill_floor: Ts,
    pub ref_px: BTreeMap<InstrumentId, RefPrice>,
    pub halt_account: Option<Halt>,
    pub halt_strategy: BTreeMap<StrategyId, Halt>,
    pub mismatches: BTreeMap<u32, Mismatch>,
    pub next_mismatch_id: u32,
    pub storage_low: bool,
    pub sync: SyncState,
    /// Record time of the most recent booked fill of any kind.
    pub last_fill_at: Ts,
    pub last_purge_at: Ts,
    pub stats: Stats,
    /// Margin reports (C-MARGIN). Not serialised while empty, so the state
    /// of a run without a margin policy keeps its bytes and its digest.
    #[serde(default, skip_serializing_if = "MarginState::is_empty")]
    pub margin: MarginState,
    /// Instruments whose position the venue changed on its own, with the
    /// record time of the last such change. The next positions answer
    /// asked after that time sets the position (U13b). Not serialised while
    /// empty.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub venue_change_wait: BTreeMap<InstrumentId, Ts>,
    /// Keys of unclassified executions, with their venue time. Kept apart
    /// from `seen_fills` so that the same key delivered later as a fill is
    /// booked. Purged with the fill keys. Not serialised while empty.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub seen_unclassified: BTreeMap<FillKey, Ts>,
}

impl Core {
    pub fn new(instance_id: u64) -> Core {
        Core {
            instance_id,
            config: CoreConfig::empty(),
            boot: 0,
            link: Link::default(),
            next_order_id: 1,
            orders: BTreeMap::new(),
            open: BTreeSet::new(),
            inflight_sends: 0,
            by_client: BTreeMap::new(),
            by_venue_oid: BTreeMap::new(),
            account: BTreeMap::new(),
            strategy_books: BTreeMap::new(),
            unattributed: BTreeMap::new(),
            parked_fills: Vec::new(),
            seen_fills: BTreeMap::new(),
            fill_floor: 0,
            ref_px: BTreeMap::new(),
            halt_account: None,
            halt_strategy: BTreeMap::new(),
            mismatches: BTreeMap::new(),
            next_mismatch_id: 1,
            storage_low: false,
            sync: SyncState::default(),
            last_fill_at: 0,
            last_purge_at: 0,
            stats: Stats::default(),
            margin: MarginState::default(),
            venue_change_wait: BTreeMap::new(),
            seen_unclassified: BTreeMap::new(),
        }
    }

    // ---- read helpers -----------------------------------------------------

    pub fn order(&self, id: OrderId) -> Option<&Order> {
        self.orders.get(&id)
    }

    pub fn lookup_client(&self, s: &StrategyId, c: &ClientOrderId) -> Option<OrderId> {
        self.by_client.get(s).and_then(|m| m.get(c)).copied()
    }

    pub fn account_book(&self, i: &InstrumentId) -> Book {
        self.account.get(i).cloned().unwrap_or_default()
    }

    pub fn strategy_book(&self, s: &StrategyId, i: &InstrumentId) -> Book {
        self.strategy_books
            .get(s)
            .and_then(|m| m.get(i))
            .cloned()
            .unwrap_or_default()
    }

    pub fn strategy_open_orders(&self, s: &StrategyId) -> u32 {
        self.strategy_books
            .get(s)
            .map(|m| m.values().map(|b| b.open_orders).sum())
            .unwrap_or(0)
    }

    /// True while the link is up but state has not been confirmed against the
    /// venue within the current link session.
    pub fn needs_resync(&self) -> bool {
        if !self.link.up {
            return false;
        }
        let s = self.link.session;
        self.sync.fills_session != s
            || self.sync.open_orders_session != s
            || self.sync.positions_session != s
            || self.open.iter().any(|id| {
                self.orders
                    .get(id)
                    .is_some_and(|o| o.phase == Phase::Working && o.evidence_session != s)
            })
    }

    pub fn has_in_doubt(&self) -> bool {
        self.open
            .iter()
            .any(|id| self.orders.get(id).is_some_and(|o| o.doubt.is_some()))
    }

    /// Every reason new exposure is refused right now. Derived from state on
    /// each call, so a reason cannot be forgotten or cleared by accident.
    pub fn holds(&self, strategy: Option<&StrategyId>) -> Vec<HoldReason> {
        let mut v = Vec::new();
        if self.config.version == 0 {
            v.push(HoldReason::NoConfig);
        }
        if !self.link.up {
            v.push(HoldReason::LinkDown);
        } else if self.needs_resync() {
            v.push(HoldReason::Resync);
        }
        if self.has_in_doubt() {
            v.push(HoldReason::InDoubt);
        }
        if self.mismatches.values().any(|m| m.acked.is_none()) {
            v.push(HoldReason::Mismatch);
        }
        if self.halt_account.is_some() {
            v.push(HoldReason::OperatorAccount);
        }
        if let Some(s) = strategy {
            if self.halt_strategy.contains_key(s) {
                v.push(HoldReason::OperatorStrategy);
            }
        }
        if self.storage_low {
            v.push(HoldReason::StorageLow);
        }
        // The holds of C-MARGIN exist only with a margin policy. The
        // REDUCING latch itself stays in the state without one, and holds
        // again when a policy is activated.
        let margin_rules_on = self.config.policy.margin.is_some();
        if margin_rules_on && self.margin.reducing.is_some() {
            v.push(HoldReason::Reducing);
        }
        if margin_rules_on {
            for instrument in self.config.instruments.keys() {
                if !self.venue_setting_is_as_configured(instrument) {
                    v.push(HoldReason::VenueSetting(instrument.clone()));
                }
            }
            if !self.account_mode_is_allowed() {
                v.push(HoldReason::AccountMode);
            }
        }
        v
    }

    /// Whether the venue told, in the current link session, that
    /// `instrument` has the margin mode and the leverage the margin policy
    /// asks for (C-MARGIN, 거래소 설정과 계좌 모드). False without a margin
    /// policy.
    pub fn venue_setting_is_as_configured(&self, instrument: &InstrumentId) -> bool {
        let Some(policy) = &self.config.policy.margin else {
            return false;
        };
        let Some(seen) = self.margin.venue_settings.get(instrument) else {
            return false;
        };
        let mode_matches = seen.isolated == policy.isolated;
        let leverage_matches = seen.leverage == policy.venue_leverage;
        mode_matches && leverage_matches
    }

    /// Whether the venue told, in the current link session, an account mode
    /// that lets the margin rules run (C-MARGIN, 거래소 설정과 계좌 모드).
    /// False without a margin policy.
    pub fn account_mode_is_allowed(&self) -> bool {
        let Some(policy) = &self.config.policy.margin else {
            return false;
        };
        // Not told in this link session: unknown.
        let Some(mode) = &self.margin.account_mode else {
            return false;
        };
        let never_allowed = ACCOUNT_MODES_NEVER_ALLOWED.contains(&mode.as_str());
        let listed = policy.account_modes_allowed.iter().any(|allowed| allowed == mode);
        listed && !never_allowed
    }

    /// Whether a margin report exists that is fresh at record time `at`
    /// (C-MARGIN, 신선도). False without a margin policy.
    pub fn margin_report_is_fresh(&self, at: Ts) -> bool {
        let (Some(policy), Some(report)) = (&self.config.policy.margin, &self.margin.report) else {
            return false;
        };
        // Age = record time − query time, both node clock values. A query
        // time after the record time means the clock stepped backwards: the
        // age is negative and the report does not count until it is not.
        let not_from_the_future = report.queried_at <= at;
        let young_enough = at.saturating_sub(report.queried_at) <= policy.margin_max_age_us;
        not_from_the_future && young_enough
    }

    /// The gateway command an event calls for, if any. Used only by live
    /// processing; recovery never calls this.
    pub fn command_for(&self, ev: &Event) -> Option<VenueCommand> {
        match ev {
            Event::OrderAccepted { order_id, .. } => {
                let o = self.orders.get(order_id)?;
                Some(VenueCommand::SendOrder {
                    order_id: o.id,
                    instance_id: self.instance_id,
                    instrument: o.instrument.clone(),
                    side: o.side,
                    qty: o.qty,
                    px: o.px,
                    tif: o.tif,
                    reduce_only: o.reduce_only,
                    expires_at: o.expires_at,
                })
            }
            Event::CancelSent { order_id, attempt } => {
                let o = self.orders.get(order_id)?;
                Some(VenueCommand::SendCancel {
                    order_id: o.id,
                    instance_id: self.instance_id,
                    attempt: *attempt,
                    instrument: o.instrument.clone(),
                    venue_oid: o.venue_oid.clone(),
                })
            }
            _ => None,
        }
    }

    /// Deterministic 64-bit digest (FNV-1a) of the serialised state. Equal
    /// states give equal digests on every platform and build.
    pub fn digest(&self) -> u64 {
        let bytes = serde_json::to_vec(self).unwrap_or_default();
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in bytes {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        h
    }

    // ---- mutation ---------------------------------------------------------

    fn order_mut(&mut self, id: OrderId) -> Result<&mut Order, CoreFault> {
        self.orders
            .get_mut(&id)
            .ok_or_else(|| CoreFault(format!("event refers to unknown order {id}")))
    }

    fn books_mut(&mut self, strategy: &StrategyId, instrument: &InstrumentId) -> [&mut Book; 2] {
        let acct = self.account.entry(instrument.clone()).or_default();
        let strat = self
            .strategy_books
            .entry(strategy.clone())
            .or_default()
            .entry(instrument.clone())
            .or_default();
        [acct, strat]
    }

    /// Releases `qty` of an order's reservation from both of its books.
    fn release(&mut self, id: OrderId, qty: Qty) -> Result<(), CoreFault> {
        if qty.is_zero() {
            return Ok(());
        }
        let o = self.order_mut(id)?;
        if qty > o.reserved {
            return fault(format!("order {id}: release {qty} exceeds reserved {}", o.reserved));
        }
        o.reserved = Qty(o.reserved.0 - qty.0);
        let (strategy, instrument, side) = (o.strategy.clone(), o.instrument.clone(), o.side);
        for b in self.books_mut(&strategy, &instrument) {
            let slot = match side {
                Side::Buy => &mut b.buy_open,
                Side::Sell => &mut b.sell_open,
            };
            if qty > *slot {
                return fault(format!("order {id}: book open quantity underflow"));
            }
            *slot = Qty(slot.0 - qty.0);
        }
        Ok(())
    }

    fn book_position(b: &mut Book, side: Side, qty: Qty, px: Px, fee: Amount) -> Result<(), CoreFault> {
        let signed = qty.signed(side);
        b.pos = b
            .pos
            .checked_add(signed)
            .ok_or_else(|| CoreFault("position overflow".into()))?;
        // The sums below are reports, not limits. They saturate: stopping the
        // process because a statistic no longer fits would leave a fill
        // unbooked, and the same fill would stop it again after every restart.
        b.traded_qty = Qty(b.traded_qty.0.saturating_add(qty.0));
        let n = notional_round(px, qty).unwrap_or(Amount(i64::MAX));
        b.traded_notional = Amount(b.traded_notional.0.saturating_add(n.0));
        b.fees = Amount(b.fees.0.saturating_add(fee.0));
        b.fills = b.fills.saturating_add(1);
        Ok(())
    }

    /// Applies one event that was decided at record time `at`.
    pub fn apply(&mut self, at: Ts, ev: &Event) -> Result<(), CoreFault> {
        match ev {
            Event::ConfigActivated { config } => {
                let policy_comes_on = self.config.policy.margin.is_none() && config.policy.margin.is_some();
                self.config = (**config).clone();
                // A policy that comes on while the link is up has no
                // link-up moment of its own: the stale cancel measures
                // from here.
                if policy_comes_on && self.link.up {
                    self.margin.link_up_at = Some(at);
                }
            }
            Event::Booted { boot, fill_floor, .. } => {
                self.boot = *boot;
                self.link.up = false;
                self.fill_floor = (*fill_floor).max(self.fill_floor);
            }
            Event::LinkChanged { up, session, .. } => {
                self.link.up = *up;
                self.link.session = *session;
                if *up && self.config.policy.margin.is_some() {
                    self.margin.link_up_at = Some(at);
                }
                // What the venue told in one link session says nothing
                // about the next (C-MARGIN): the venue settings and the
                // account mode are unknown again. The last margin report
                // stays.
                self.margin.venue_settings.clear();
                self.margin.account_mode = None;
            }
            Event::RequestRejected { .. } => {
                self.stats.rejected += 1;
            }
            Event::OrderAccepted {
                order_id,
                request: r,
                config_version,
                ..
            } => {
                if order_id.0 != self.next_order_id || self.orders.contains_key(order_id) {
                    return fault(format!("order id {order_id} is not the next id"));
                }
                if self.lookup_client(&r.strategy, &r.client_order_id).is_some() {
                    return fault(format!("client order id {} already used", r.client_order_id));
                }
                let order = Order {
                    id: *order_id,
                    strategy: r.strategy.clone(),
                    client_order_id: r.client_order_id.clone(),
                    instrument: r.instrument.clone(),
                    side: r.side,
                    qty: r.qty,
                    px: r.px,
                    tif: r.tif,
                    reduce_only: r.reduce_only,
                    expires_at: r.expires_at,
                    accepted_at: at,
                    config_version: *config_version,
                    send_boot: self.boot,
                    send_session: self.link.session,
                    phase: Phase::PendingSend,
                    doubt: None,
                    absence: None,
                    venue_oid: None,
                    cum_qty: Qty::ZERO,
                    cum_notional: Amount::ZERO,
                    fee: Amount::ZERO,
                    fills: Vec::new(),
                    reported_cum: Qty::ZERO,
                    reserved: r.qty,
                    terminal: None,
                    close: None,
                    cancel: None,
                    evidence_session: self.link.session,
                    last_venue_ts: 0,
                    overfilled: false,
                    acked_at: 0,
                    recheck: false,
                };
                for b in self.books_mut(&r.strategy, &r.instrument) {
                    let slot = match r.side {
                        Side::Buy => &mut b.buy_open,
                        Side::Sell => &mut b.sell_open,
                    };
                    *slot = slot
                        .checked_add(r.qty)
                        .ok_or_else(|| CoreFault("open quantity overflow".into()))?;
                    b.open_orders += 1;
                }
                self.by_client
                    .entry(r.strategy.clone())
                    .or_default()
                    .insert(r.client_order_id.clone(), *order_id);
                self.orders.insert(*order_id, order);
                self.open.insert(*order_id);
                self.inflight_sends += 1;
                self.next_order_id += 1;
                self.stats.accepted += 1;
            }
            Event::OrderAcked {
                order_id,
                venue_oid,
                session,
                ..
            } => {
                if let Some(v) = venue_oid {
                    if self.by_venue_oid.get(v).is_some_and(|other| other != order_id) {
                        return fault(format!("venue order id {v} already belongs to another order"));
                    }
                }
                let o = self.order_mut(*order_id)?;
                if o.phase != Phase::PendingSend {
                    return fault(format!("order {order_id}: acked in phase {:?}", o.phase));
                }
                o.phase = Phase::Working;
                o.doubt = None;
                o.evidence_session = *session;
                o.acked_at = at;
                // Cancel attempts made while nobody knew whether the order
                // existed do not count against the order now that it does.
                if let Some(c) = o.cancel.as_mut() {
                    c.base = c.attempts;
                }
                if let Some(v) = venue_oid {
                    o.venue_oid = Some(v.clone());
                    self.by_venue_oid.insert(v.clone(), *order_id);
                }
                self.inflight_sends = self.inflight_sends.saturating_sub(1);
            }
            Event::VenueOidLearned { order_id, venue_oid } => {
                if self.by_venue_oid.get(venue_oid).is_some_and(|other| other != order_id) {
                    return fault(format!("venue order id {venue_oid} already belongs to another order"));
                }
                let o = self.order_mut(*order_id)?;
                o.venue_oid = Some(venue_oid.clone());
                self.by_venue_oid.insert(venue_oid.clone(), *order_id);
            }
            Event::OrderInDoubt {
                order_id,
                reason,
                detail,
            } => {
                let o = self.order_mut(*order_id)?;
                o.doubt = Some(Doubt {
                    reason: *reason,
                    detail: detail.clone(),
                    since: at,
                });
            }
            Event::StatusNoted {
                order_id,
                session,
                reported_cum,
                venue_ts,
            } => {
                let o = self.order_mut(*order_id)?;
                o.evidence_session = *session;
                o.recheck = false;
                o.reported_cum = (*reported_cum).max(o.reported_cum);
                o.last_venue_ts = (*venue_ts).max(o.last_venue_ts);
            }
            Event::OrderRecheck { order_id } => {
                self.order_mut(*order_id)?.recheck = true;
            }
            Event::FillBooked {
                order_id,
                key,
                qty,
                px,
                fee,
                venue_ts,
            } => {
                let o = self.order_mut(*order_id)?;
                o.cum_qty = o
                    .cum_qty
                    .checked_add(*qty)
                    .ok_or_else(|| CoreFault("cumulative quantity overflow".into()))?;
                let n = notional_round(*px, *qty).unwrap_or(Amount(i64::MAX));
                o.cum_notional = Amount(o.cum_notional.0.saturating_add(n.0));
                o.fee = Amount(o.fee.0.saturating_add(fee.0));
                o.fills.push(key.clone());
                o.last_venue_ts = (*venue_ts).max(o.last_venue_ts);
                if o.cum_qty > o.qty {
                    o.overfilled = true;
                }
                let consumed = (*qty).min(o.reserved);
                let (strategy, instrument, side) = (o.strategy.clone(), o.instrument.clone(), o.side);
                self.release(*order_id, consumed)?;
                for b in self.books_mut(&strategy, &instrument) {
                    Self::book_position(b, side, *qty, *px, *fee)?;
                }
                self.seen_fills.insert(key.clone(), *venue_ts);
                self.last_fill_at = at;
                self.stats.fills_booked += 1;
            }
            Event::FillParked { fill } => {
                self.parked_fills.push(ParkedFill {
                    fill: fill.clone(),
                    parked_at: at,
                });
            }
            Event::FillUnparked { key } => {
                self.parked_fills.retain(|p| &p.fill.key != key);
            }
            Event::ExternalFillBooked { fill } => {
                let acct = self.account.entry(fill.instrument.clone()).or_default();
                Self::book_position(acct, fill.side, fill.qty, fill.px, fill.fee)?;
                let un = self.unattributed.entry(fill.instrument.clone()).or_default();
                Self::book_position(un, fill.side, fill.qty, fill.px, fill.fee)?;
                self.seen_fills.insert(fill.key.clone(), fill.venue_ts);
                self.last_fill_at = at;
                self.stats.external_fills += 1;
            }
            Event::VenueTerminal {
                order_id,
                kind,
                reason,
                final_cum,
                venue_ts,
            } => {
                let o = self.order_mut(*order_id)?;
                if o.phase == Phase::Closed {
                    return fault(format!("order {order_id}: terminal after close"));
                }
                let was_pending = o.phase == Phase::PendingSend;
                o.phase = Phase::Closing;
                o.doubt = None;
                o.recheck = false;
                o.last_venue_ts = (*venue_ts).max(o.last_venue_ts);
                let flagged = o.terminal.as_ref().is_some_and(|t| t.missing_fills_flagged);
                o.terminal = Some(Terminal {
                    kind: *kind,
                    reason: reason.clone(),
                    final_cum: *final_cum,
                    venue_ts: *venue_ts,
                    at,
                    missing_fills_flagged: flagged,
                });
                // The reservation only ever shrinks here: to what the final
                // cumulative quantity leaves unbooked.
                let release = match final_cum {
                    Some(f) => {
                        let still = Qty((f.0 - o.cum_qty.0).max(0));
                        Qty((o.reserved.0 - still.0).max(0))
                    }
                    None => Qty::ZERO,
                };
                if was_pending {
                    self.inflight_sends = self.inflight_sends.saturating_sub(1);
                }
                self.release(*order_id, release)?;
            }
            Event::MissingFillsFlagged { order_id } => {
                let o = self.order_mut(*order_id)?;
                if let Some(t) = o.terminal.as_mut() {
                    t.missing_fills_flagged = true;
                }
            }
            Event::OrderClosed { order_id, reason } => {
                let o = self.order_mut(*order_id)?;
                if o.phase == Phase::Closed {
                    return fault(format!("order {order_id}: closed twice"));
                }
                let was_pending = o.phase == Phase::PendingSend;
                let remaining = o.reserved;
                let (strategy, instrument) = (o.strategy.clone(), o.instrument.clone());
                self.release(*order_id, remaining)?;
                for b in self.books_mut(&strategy, &instrument) {
                    if b.open_orders == 0 {
                        return fault(format!("order {order_id}: open order count underflow"));
                    }
                    b.open_orders -= 1;
                }
                let o = self.order_mut(*order_id)?;
                o.phase = Phase::Closed;
                o.doubt = None;
                o.recheck = false;
                o.close = Some(Close {
                    reason: reason.clone(),
                    at,
                });
                if let Some(c) = o.cancel.as_mut() {
                    c.inflight = false;
                }
                if was_pending {
                    self.inflight_sends = self.inflight_sends.saturating_sub(1);
                }
                self.open.remove(order_id);
                self.stats.closed += 1;
            }
            Event::CancelRequested { order_id, by } => {
                let o = self.order_mut(*order_id)?;
                match o.cancel.as_mut() {
                    // Requested again after the attempts ran out: a new
                    // budget, the attempt numbers keep counting up.
                    Some(c) => {
                        c.requested_at = at;
                        c.by = by.clone();
                        c.base = c.attempts;
                    }
                    None => {
                        o.cancel = Some(CancelState {
                            requested_at: at,
                            by: by.clone(),
                            attempts: 0,
                            base: 0,
                            inflight: false,
                            last_attempt_at: 0,
                            confirmed: false,
                            last_outcome: None,
                        });
                    }
                }
            }
            Event::CancelSent { order_id, attempt } => {
                let o = self.order_mut(*order_id)?;
                let c = o
                    .cancel
                    .as_mut()
                    .ok_or_else(|| CoreFault(format!("order {order_id}: cancel sent without request")))?;
                c.attempts = *attempt;
                c.inflight = true;
                c.last_attempt_at = at;
            }
            Event::CancelOutcomeNoted { order_id, outcome, .. } => {
                let o = self.order_mut(*order_id)?;
                let c = o
                    .cancel
                    .as_mut()
                    .ok_or_else(|| CoreFault(format!("order {order_id}: cancel outcome without request")))?;
                c.inflight = false;
                if *outcome == CancelOutcome::Confirmed {
                    c.confirmed = true;
                }
                c.last_outcome = Some(outcome.clone());
            }
            Event::FillsSynced { session, up_to } => {
                self.sync.fills_session = *session;
                self.sync.fills_up_to = (*up_to).max(self.sync.fills_up_to);
            }
            Event::OpenOrdersSwept { session } => {
                self.sync.open_orders_session = *session;
            }
            Event::PositionsVerified { session } => {
                self.sync.positions_session = *session;
            }
            Event::MismatchRaised {
                id,
                kind,
                subject,
                detail,
            } => {
                if *id != self.next_mismatch_id {
                    return fault(format!("mismatch id {id} is not the next id"));
                }
                self.mismatches.insert(
                    *id,
                    Mismatch {
                        id: *id,
                        kind: *kind,
                        subject: subject.clone(),
                        detail: detail.clone(),
                        raised_at: at,
                        acked: None,
                    },
                );
                self.next_mismatch_id += 1;
            }
            Event::MismatchAcked { id, note } => {
                let m = self
                    .mismatches
                    .get_mut(id)
                    .ok_or_else(|| CoreFault(format!("unknown mismatch {id}")))?;
                m.acked = Some(MismatchAck {
                    note: note.clone(),
                    at,
                });
            }
            Event::OperatorHalted { scope, note } => {
                let halt = Halt {
                    note: note.clone(),
                    at,
                };
                match scope {
                    Scope::Account => self.halt_account = Some(halt),
                    Scope::Strategy(s) => {
                        self.halt_strategy.insert(s.clone(), halt);
                    }
                }
            }
            Event::OperatorResumed { scope, .. } => match scope {
                Scope::Account => self.halt_account = None,
                Scope::Strategy(s) => {
                    self.halt_strategy.remove(s);
                }
            },
            Event::StorageLowChanged { low, .. } => {
                self.storage_low = *low;
            }
            Event::RefPriceUpdated {
                instrument,
                px,
                venue_ts,
            } => {
                self.ref_px.insert(
                    instrument.clone(),
                    RefPrice {
                        px: *px,
                        venue_ts: *venue_ts,
                        at,
                    },
                );
            }
            Event::PositionAdjusted {
                instrument,
                ledger,
                delta,
                reason,
                ..
            } => {
                if *reason == AdjustReason::SnapshotAfterVenueChange {
                    self.venue_change_wait.remove(instrument);
                }
                let acct = self.account.entry(instrument.clone()).or_default();
                acct.pos = acct
                    .pos
                    .checked_add(*delta)
                    .ok_or_else(|| CoreFault("position overflow".into()))?;
                let b = match ledger {
                    Ledger::Unattributed => self.unattributed.entry(instrument.clone()).or_default(),
                    Ledger::Strategy(s) => self
                        .strategy_books
                        .entry(s.clone())
                        .or_default()
                        .entry(instrument.clone())
                        .or_default(),
                };
                b.pos = b
                    .pos
                    .checked_add(*delta)
                    .ok_or_else(|| CoreFault("position overflow".into()))?;
            }
            Event::Purged {
                order_ids,
                cutoff,
                fill_floor,
            } => {
                for id in order_ids {
                    let Some(o) = self.orders.remove(id) else {
                        return fault(format!("purge of unknown order {id}"));
                    };
                    if o.phase != Phase::Closed {
                        return fault(format!("purge of open order {id}"));
                    }
                    if let Some(m) = self.by_client.get_mut(&o.strategy) {
                        m.remove(&o.client_order_id);
                        if m.is_empty() {
                            self.by_client.remove(&o.strategy);
                        }
                    }
                    if let Some(v) = &o.venue_oid {
                        self.by_venue_oid.remove(v);
                    }
                    self.stats.purged += 1;
                }
                self.fill_floor = (*fill_floor).max(self.fill_floor);
                let floor = self.fill_floor;
                self.seen_fills.retain(|_, t| *t >= floor);
                self.seen_unclassified.retain(|_, t| *t >= floor);
                self.mismatches
                    .retain(|_, m| m.acked.as_ref().is_none_or(|a| a.at >= *cutoff));
                self.last_purge_at = at;
            }
            Event::MarginReported { report } => {
                self.margin.report = Some(report.clone());
                // The venue answers readably again: the reason an earlier
                // answer was refused is no longer the state of things.
                self.margin.last_rejection = None;
                // Every position states the margin mode and the leverage of
                // its instrument: a report is a source of venue settings.
                // Only for configured instruments, as with the
                // `VenueSetting` input: nothing reads another setting, and
                // the venue decides how many positions a report lists.
                let configured = &self.config.instruments;
                for p in report.positions.iter().filter(|p| configured.contains_key(&p.instrument)) {
                    self.margin.venue_settings.insert(
                        p.instrument.clone(),
                        VenueSettingSeen {
                            isolated: p.leverage_type == crate::input::LeverageType::Isolated,
                            leverage: p.leverage_value,
                        },
                    );
                }
            }
            Event::MarginReportRejected { seq, queried_at, reason, .. } => {
                self.margin.last_rejection = Some(MarginRejection {
                    seq: *seq,
                    queried_at: *queried_at,
                    reason: reason.clone(),
                    at,
                });
            }
            Event::DayBaseSet { day, value, set_by } => {
                self.margin.day_base = Some(DayBase {
                    day: *day,
                    value: *value,
                    set_by: *set_by,
                });
            }
            Event::ReducingEntered { rule, detail } => {
                self.margin.reducing = Some(Reducing {
                    rule: *rule,
                    detail: detail.clone(),
                    since: at,
                });
            }
            Event::VenueSettingNoted {
                instrument,
                isolated,
                leverage,
                ..
            } => {
                self.margin.venue_settings.insert(
                    instrument.clone(),
                    VenueSettingSeen {
                        isolated: *isolated,
                        leverage: *leverage,
                    },
                );
            }
            Event::AccountModeNoted { mode, .. } => {
                self.margin.account_mode = Some(mode.clone());
            }
            Event::ReducingCleared { .. } => {
                self.margin.reducing = None;
            }
            Event::FundingBooked { funding } => {
                self.seen_fills.insert(funding.key.clone(), funding.venue_ts);
                self.stats.funding_payments += 1;
            }
            Event::VenuePositionChanged { change } => {
                for b in [
                    self.account.entry(change.instrument.clone()).or_default(),
                    self.unattributed.entry(change.instrument.clone()).or_default(),
                ] {
                    b.pos = b
                        .pos
                        .checked_add(change.qty)
                        .ok_or_else(|| CoreFault("position overflow".into()))?;
                }
                self.seen_fills.insert(change.key.clone(), change.venue_ts);
                self.venue_change_wait.insert(change.instrument.clone(), at);
                self.last_fill_at = at;
                self.stats.venue_changes += 1;
            }
            Event::UnclassifiedExecutionNoted { key, venue_ts, .. } => {
                self.seen_unclassified.insert(key.clone(), *venue_ts);
            }
            Event::VenueChangeSettled { instrument } => {
                self.venue_change_wait.remove(instrument);
            }
            Event::AbsenceNoted { order_id, queried_at } => {
                let o = self.order_mut(*order_id)?;
                let count = o.absence.map_or(0, |a| a.count).saturating_add(1);
                o.absence = Some(Absence {
                    count,
                    last_queried_at: *queried_at,
                });
            }
        }
        Ok(())
    }

    // ---- invariants -------------------------------------------------------

    /// Structural invariants that must hold after every event, whatever the
    /// inputs were. Used by tests and by the offline journal checker.
    pub fn check_invariants(&self) -> Result<(), String> {
        let mut acct: BTreeMap<InstrumentId, (i64, i64, u32)> = BTreeMap::new();
        let mut strat: BTreeMap<(StrategyId, InstrumentId), (i64, i64, u32)> = BTreeMap::new();
        let mut inflight = 0u32;
        for (id, o) in &self.orders {
            if *id != o.id {
                return Err(format!("order {id}: key and id differ"));
            }
            if o.cum_qty.0 < 0 || o.reserved.0 < 0 {
                return Err(format!("order {id}: negative quantity"));
            }
            if !o.overfilled && o.cum_qty > o.qty {
                return Err(format!("order {id}: cumulative exceeds quantity without overfill flag"));
            }
            let closed = o.phase == Phase::Closed;
            if closed != o.close.is_some() {
                return Err(format!("order {id}: phase and close record disagree"));
            }
            if closed == self.open.contains(id) {
                return Err(format!("order {id}: open index disagrees with phase"));
            }
            match o.phase {
                Phase::PendingSend | Phase::Working => {
                    if o.reserved != o.remaining() {
                        return Err(format!(
                            "order {id}: reserved {} differs from remaining {}",
                            o.reserved,
                            o.remaining()
                        ));
                    }
                    if o.terminal.is_some() {
                        return Err(format!("order {id}: terminal evidence in phase {:?}", o.phase));
                    }
                }
                Phase::Closing => {
                    let Some(t) = &o.terminal else {
                        return Err(format!("order {id}: closing without terminal evidence"));
                    };
                    let expect = match t.final_cum {
                        Some(f) => Qty((f.0 - o.cum_qty.0).max(0)).min(o.remaining()),
                        None => o.remaining(),
                    };
                    if o.reserved != expect {
                        return Err(format!("order {id}: closing reservation {} != {}", o.reserved, expect));
                    }
                }
                Phase::Closed => {
                    if !o.reserved.is_zero() {
                        return Err(format!("order {id}: closed with reservation {}", o.reserved));
                    }
                }
            }
            if o.doubt.is_some() && o.phase != Phase::PendingSend {
                return Err(format!("order {id}: doubt in phase {:?}", o.phase));
            }
            if o.phase == Phase::PendingSend {
                inflight += 1;
            }
            if self.lookup_client(&o.strategy, &o.client_order_id) != Some(*id) {
                return Err(format!("order {id}: idempotency index does not point back"));
            }
            if let Some(v) = &o.venue_oid {
                if self.by_venue_oid.get(v) != Some(id) {
                    return Err(format!("order {id}: venue id index does not point back"));
                }
            }
            let mut seen = BTreeSet::new();
            for k in &o.fills {
                if !seen.insert(k) {
                    return Err(format!("order {id}: fill {k} booked twice"));
                }
            }
            let (b, s) = match o.side {
                Side::Buy => (o.reserved.0, 0),
                Side::Sell => (0, o.reserved.0),
            };
            let n = u32::from(!closed);
            let a = acct.entry(o.instrument.clone()).or_default();
            *a = (a.0 + b, a.1 + s, a.2 + n);
            let e = strat.entry((o.strategy.clone(), o.instrument.clone())).or_default();
            *e = (e.0 + b, e.1 + s, e.2 + n);
        }
        if inflight != self.inflight_sends {
            return Err(format!("inflight_sends {} != counted {}", self.inflight_sends, inflight));
        }
        let index_len: usize = self.by_client.values().map(|m| m.len()).sum();
        if index_len != self.orders.len() {
            return Err("idempotency index size differs from order count".into());
        }
        for (i, b) in &self.account {
            let (bo, so, n) = acct.get(i).copied().unwrap_or_default();
            if b.buy_open.0 != bo || b.sell_open.0 != so || b.open_orders != n {
                return Err(format!("account book {i}: open quantities differ from orders"));
            }
            let strat_pos: i64 = self
                .strategy_books
                .values()
                .filter_map(|m| m.get(i))
                .map(|b| b.pos.0)
                .sum();
            let un = self.unattributed.get(i).map(|b| b.pos.0).unwrap_or(0);
            if b.pos.0 != strat_pos + un {
                return Err(format!(
                    "account position {i} = {} but strategies {} + unattributed {}",
                    b.pos,
                    SQty(strat_pos),
                    SQty(un)
                ));
            }
        }
        for (i, sums) in &acct {
            if !self.account.contains_key(i) && *sums != (0, 0, 0) {
                return Err(format!("orders reserve on {i} but no account book exists"));
            }
        }
        for (s, m) in &self.strategy_books {
            for (i, b) in m {
                let (bo, so, n) = strat.get(&(s.clone(), i.clone())).copied().unwrap_or_default();
                if b.buy_open.0 != bo || b.sell_open.0 != so || b.open_orders != n {
                    return Err(format!("strategy book {s}/{i}: open quantities differ from orders"));
                }
            }
        }
        if self.open.len() > self.orders.len() {
            return Err("open index larger than order map".into());
        }
        Ok(())
    }

    /// The quantity-limit property: for every instrument and scope, the
    /// position stays inside the configured bounds for every fill that could
    /// still occur. It holds only while no mismatch was raised and limits
    /// were not lowered below existing exposure, so it is a conditional
    /// property and is checked separately from the structural invariants.
    pub fn check_position_bounds(&self) -> Result<(), String> {
        for (i, spec) in &self.config.instruments {
            let b = self.account_book(i);
            let (lo, hi) = (b.lower().unwrap_or(i64::MIN), b.upper().unwrap_or(i64::MAX));
            if lo < spec.account_pos_min.0 || hi > spec.account_pos_max.0 {
                return Err(format!(
                    "account {i}: reachable [{}, {}] outside [{}, {}]",
                    SQty(lo),
                    SQty(hi),
                    spec.account_pos_min,
                    spec.account_pos_max
                ));
            }
        }
        for (s, spec) in &self.config.strategies {
            for (i, lim) in &spec.limits {
                let b = self.strategy_book(s, i);
                let (lo, hi) = (b.lower().unwrap_or(i64::MIN), b.upper().unwrap_or(i64::MAX));
                if lo < lim.min.0 || hi > lim.max.0 {
                    return Err(format!(
                        "strategy {s}/{i}: reachable [{}, {}] outside [{}, {}]",
                        SQty(lo),
                        SQty(hi),
                        lim.min,
                        lim.max
                    ));
                }
            }
        }
        Ok(())
    }
}
