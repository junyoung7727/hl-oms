//! Decisions: how each input turns into events.
//!
//! `Core::handle` is a pure function of (state, record time, input). It reads
//! no clock, file, socket or random source. Every mutation goes through
//! `Tx::emit`, which applies the event with the same `Core::apply` that
//! recovery uses and derives the gateway command, if the event calls for one.
//!
//! A `CoreFault` leaves the in-memory state partially updated. The caller
//! must stop without journaling the record; the journal then still describes
//! the state before this input.

use crate::core::{Core, CoreFault};
use crate::event::*;
use crate::ids::*;
use crate::input::*;
use crate::model::*;
use crate::num::*;

struct Tx<'a> {
    core: &'a mut Core,
    at: Ts,
    out: Output,
}

impl Core {
    /// Processes one input at record time `at`. `at` must not decrease
    /// between calls; the sequencer guarantees that.
    pub fn handle(&mut self, at: Ts, input: &Input) -> Result<Output, CoreFault> {
        let mut tx = Tx {
            core: self,
            at,
            out: Output::default(),
        };
        match input {
            Input::Boot { binary } => tx.boot(binary)?,
            Input::Config(cfg) => tx.config(cfg)?,
            Input::NewOrder(r) => tx.new_order(r)?,
            Input::Cancel(c) => tx.cancel(c)?,
            Input::Operator(cmd) => tx.operator(cmd)?,
            Input::Venue(v) => tx.venue(v)?,
            Input::Tick => tx.tick()?,
            Input::Storage { low, free_bytes } => {
                if *low != tx.core.storage_low {
                    tx.emit(Event::StorageLowChanged {
                        low: *low,
                        free_bytes: *free_bytes,
                    })?;
                }
            }
        }
        Ok(tx.out)
    }
}

impl Tx<'_> {
    fn emit(&mut self, ev: Event) -> Result<(), CoreFault> {
        self.core.apply(self.at, &ev)?;
        if let Some(cmd) = self.core.command_for(&ev) {
            self.out.commands.push(cmd);
        }
        self.out.events.push(ev);
        Ok(())
    }

    /// Raises a mismatch unless an unacknowledged one with the same kind and
    /// subject already exists.
    fn raise(&mut self, kind: MismatchKind, subject: String, detail: String) -> Result<(), CoreFault> {
        let exists = self
            .core
            .mismatches
            .values()
            .any(|m| m.acked.is_none() && m.kind == kind && m.subject == subject);
        if exists {
            return Ok(());
        }
        let id = self.core.next_mismatch_id;
        self.emit(Event::MismatchRaised {
            id,
            kind,
            subject,
            detail,
        })
    }

    fn open_ids(&self) -> Vec<OrderId> {
        self.core.open.iter().copied().collect()
    }

    // ---- lifecycle --------------------------------------------------------

    fn boot(&mut self, binary: &str) -> Result<(), CoreFault> {
        let boot = self.core.boot + 1;
        // A new data directory does not inherit the account's history: fills
        // older than its first start (less a clock margin) are not its
        // business. Positions that already exist are adopted explicitly.
        let fill_floor = if boot == 1 {
            self.at.saturating_sub(self.core.config.policy.absence_margin_us)
        } else {
            self.core.fill_floor
        };
        self.emit(Event::Booted {
            boot,
            binary: binary.to_string(),
            fill_floor,
        })?;
        for id in self.open_ids() {
            let Some(o) = self.core.orders.get(&id) else { continue };
            let pending_without_result = o.phase == Phase::PendingSend && o.doubt.is_none();
            let cancel_inflight = o.cancel.as_ref().filter(|c| c.inflight).map(|c| c.attempts);
            if pending_without_result {
                self.emit(Event::OrderInDoubt {
                    order_id: id,
                    reason: DoubtReason::Restart,
                    detail: "the process restarted before a send result was recorded".into(),
                })?;
            }
            if let Some(attempt) = cancel_inflight {
                self.emit(Event::CancelOutcomeNoted {
                    order_id: id,
                    attempt,
                    outcome: CancelOutcome::LostOnRestart,
                })?;
            }
        }
        Ok(())
    }

    fn config(&mut self, cfg: &crate::config::CoreConfig) -> Result<(), CoreFault> {
        if *cfg == self.core.config {
            self.out.reply = Some(Reply::Config(Ok(cfg.version)));
            return Ok(());
        }
        let mut errs = cfg.validate().err().unwrap_or_default();
        if cfg.version <= self.core.config.version {
            errs.push(format!(
                "version {} must exceed the active version {}",
                cfg.version, self.core.config.version
            ));
        }
        // Exposure that already exists must stay describable under the new
        // configuration; otherwise books would refer to unknown instruments.
        for (i, b) in &self.core.account {
            if !b.is_empty() && (b.pos.0 != 0 || b.open_orders > 0) && !cfg.instruments.contains_key(i) {
                errs.push(format!("instrument {i} has exposure and cannot be removed"));
            }
        }
        for id in &self.core.open {
            if let Some(o) = self.core.orders.get(id) {
                if !cfg.strategies.contains_key(&o.strategy) {
                    errs.push(format!("strategy {} has open orders and cannot be removed", o.strategy));
                }
            }
        }
        if !errs.is_empty() {
            errs.sort();
            errs.dedup();
            self.out.reply = Some(Reply::Config(Err(errs)));
            return Ok(());
        }
        self.emit(Event::ConfigActivated {
            config: Box::new(cfg.clone()),
        })?;
        self.out.reply = Some(Reply::Config(Ok(cfg.version)));
        Ok(())
    }

    // ---- new order --------------------------------------------------------

    fn new_order(&mut self, r: &NewOrderReq) -> Result<(), CoreFault> {
        if let Some(id) = self.core.lookup_client(&r.strategy, &r.client_order_id) {
            let Some(o) = self.core.orders.get(&id) else {
                return crate::core::fault("idempotency index points to a missing order");
            };
            let same = o.instrument == r.instrument
                && o.side == r.side
                && o.qty == r.qty
                && o.px == r.px
                && o.tif == r.tif
                && o.reduce_only == r.reduce_only
                && o.expires_at == r.expires_at;
            self.out.reply = Some(Reply::NewOrder(if same {
                NewOrderReply::Duplicate {
                    order_id: id,
                    phase: o.phase,
                    cum_qty: o.cum_qty,
                }
            } else {
                NewOrderReply::Conflict { order_id: id }
            }));
            return Ok(());
        }

        let reasons = self.check_new_order(r);
        if !reasons.is_empty() {
            self.emit(Event::RequestRejected {
                request: r.clone(),
                reasons: reasons.clone(),
            })?;
            self.out.reply = Some(Reply::NewOrder(NewOrderReply::Rejected { reasons }));
            return Ok(());
        }

        let order_id = OrderId(self.core.next_order_id);
        let ref_px = self.core.ref_px.get(&r.instrument).map(|p| p.px);
        self.emit(Event::OrderAccepted {
            order_id,
            request: r.clone(),
            config_version: self.core.config.version,
            ref_px,
        })?;
        self.out.reply = Some(Reply::NewOrder(NewOrderReply::Accepted { order_id }));
        Ok(())
    }

    /// Every reason the request must be refused. Empty means acceptable. The
    /// whole check reads one consistent state and the reservation is made by
    /// the event emitted right after it, inside the same input.
    fn check_new_order(&self, r: &NewOrderReq) -> Vec<RejectReason> {
        use RejectReason as R;
        let core = &*self.core;
        let cfg = &core.config;
        let mut why = Vec::new();

        // Hold table (ADR-008 §7 as amended by ADR-018): with a margin
        // policy, a reduce-only order passes the holds that refuse only
        // plain orders. Without one every hold refuses every new order.
        let table_applies = cfg.policy.margin.is_some();
        for h in core.holds(Some(&r.strategy)) {
            if table_applies && r.reduce_only && h.refuses_only_plain_orders() {
                continue;
            }
            why.push(R::Hold(h));
        }
        // Margin rules are on: a plain order needs a fresh margin report. A
        // reduce-only order does not, so a position can be reduced while the
        // venue's queries fail.
        let needs_fresh_report = cfg.policy.margin.is_some() && !r.reduce_only;
        if needs_fresh_report && !core.margin_report_is_fresh(self.at) {
            why.push(R::MarginUnavailable);
        }
        let Some(strategy) = cfg.strategies.get(&r.strategy) else {
            why.push(R::UnknownStrategy);
            return why;
        };
        if !strategy.enabled {
            why.push(R::StrategyDisabled);
        }
        let Some(inst) = cfg.instruments.get(&r.instrument) else {
            why.push(R::UnknownInstrument);
            return why;
        };
        let strategy_limit = strategy.limits.get(&r.instrument);
        if strategy_limit.is_none() {
            why.push(R::InstrumentNotAllowed);
        }

        if self.at >= r.expires_at {
            why.push(R::Expired);
        } else if r.expires_at - self.at > cfg.policy.max_expiry_horizon_us {
            why.push(R::ExpiryTooFar);
        }

        if r.qty.0 <= 0 {
            why.push(R::QtyNotPositive);
        } else {
            if r.qty.0 % inst.qty_step.0 != 0 {
                why.push(R::QtyStep);
            }
            if r.qty > inst.max_order_qty {
                why.push(R::MaxOrderQty);
            }
        }
        if r.px.0 <= 0 {
            why.push(R::PxNotPositive);
        } else {
            if r.px.0 % inst.px_tick.0 != 0 {
                why.push(R::PxTick);
            }
            if let Some(max) = inst.px_max_sig_figs {
                let is_integer = r.px.0 % SCALE == 0;
                if !is_integer && significant_digits(r.px.0) > max {
                    why.push(R::PxSigFigs);
                }
            }
        }
        if r.qty.0 > 0 && r.px.0 > 0 {
            // Each bound is checked with the rounding that cannot let a
            // value through: down against the minimum, up against the maximum.
            match (notional_floor(r.px, r.qty), notional_ceil(r.px, r.qty)) {
                (Some(low), Some(high)) => {
                    if low < inst.min_notional {
                        why.push(R::BelowMinNotional);
                    }
                    if high > inst.max_order_notional {
                        why.push(R::MaxOrderNotional);
                    }
                }
                _ => why.push(R::Overflow),
            }
            if let Some(bps) = inst.price_band_bps {
                let fresh = core
                    .ref_px
                    .get(&r.instrument)
                    .filter(|p| self.at.saturating_sub(p.at) <= cfg.policy.ref_price_max_age_us);
                match fresh {
                    None => why.push(R::RefPriceUnavailable),
                    Some(p) => {
                        // Only the aggressive side is bounded: a buy far above
                        // or a sell far below the reference price.
                        let px = i128::from(r.px.0);
                        let rf = i128::from(p.px.0);
                        let band = i128::from(bps);
                        let outside = match r.side {
                            Side::Buy => px * 10_000 > rf * (10_000 + band),
                            Side::Sell => px * 10_000 < rf * (10_000 - band),
                        };
                        if outside {
                            why.push(R::PriceBand);
                        }
                    }
                }
            }
        }

        let policy = &cfg.policy;
        if core.open.len() as u64 >= u64::from(policy.max_open_orders) {
            why.push(R::MaxOpenOrdersAccount);
        }
        if core.strategy_open_orders(&r.strategy) >= strategy.max_open_orders {
            why.push(R::MaxOpenOrdersStrategy);
        }
        if core.inflight_sends >= policy.max_inflight_sends {
            why.push(R::GatewayBusy);
        }
        if core.orders.len() as u64 >= u64::from(policy.max_orders_in_memory) {
            why.push(R::CapacityExhausted);
        }

        // Position bounds including every fill that could still happen.
        if r.qty.0 > 0 {
            let acct = core.account_book(&r.instrument);
            match reach_after(&acct, r.side, r.qty) {
                None => why.push(R::Overflow),
                Some(v) => {
                    let ok = match r.side {
                        Side::Buy => v <= inst.account_pos_max.0,
                        Side::Sell => v >= inst.account_pos_min.0,
                    };
                    if !ok {
                        why.push(R::AccountPositionLimit);
                    }
                }
            }
            if let Some(lim) = strategy_limit {
                let book = core.strategy_book(&r.strategy, &r.instrument);
                match reach_after(&book, r.side, r.qty) {
                    None => why.push(R::Overflow),
                    Some(v) => {
                        let ok = match r.side {
                            Side::Buy => v <= lim.max.0,
                            Side::Sell => v >= lim.min.0,
                        };
                        if !ok {
                            why.push(R::StrategyPositionLimit);
                        }
                    }
                }
            }
        }
        // Leverage gate (C-MARGIN). A reduce-only order is not judged; it
        // still counts in the exposure of the orders that follow.
        if let Some(margin) = &cfg.policy.margin {
            if !r.reduce_only && r.qty.0 > 0 && r.px.0 > 0 {
                why.extend(core.leverage_gate(margin, r, self.at));
            }
        }
        why.dedup();
        why
    }

    // ---- cancel -----------------------------------------------------------

    fn cancel(&mut self, c: &CancelReq) -> Result<(), CoreFault> {
        let found = match &c.target {
            CancelTarget::Order(id) => self.core.orders.get(id).map(|o| o.id),
            CancelTarget::Client {
                strategy,
                client_order_id,
            } => self.core.lookup_client(strategy, client_order_id),
        };
        let Some(id) = found else {
            self.out.reply = Some(Reply::Cancel(CancelReply::NotFound));
            return Ok(());
        };
        let Some(o) = self.core.orders.get(&id) else {
            self.out.reply = Some(Reply::Cancel(CancelReply::NotFound));
            return Ok(());
        };
        if let Actor::Strategy(s) = &c.actor {
            if *s != o.strategy {
                self.out.reply = Some(Reply::Cancel(CancelReply::NotOwner));
                return Ok(());
            }
        }
        if o.is_closed() {
            self.out.reply = Some(Reply::Cancel(CancelReply::AlreadyClosed { order_id: id }));
            return Ok(());
        }
        if o.cancel.is_some() && !self.cancel_exhausted(id) {
            self.out.reply = Some(Reply::Cancel(CancelReply::AlreadyRequested { order_id: id }));
            return Ok(());
        }
        // Either the first request, or a repeated one after every attempt of
        // the earlier request ended without confirmation: asking again must
        // be able to try again.
        self.emit(Event::CancelRequested {
            order_id: id,
            by: c.actor.clone(),
        })?;
        self.maybe_send_cancel(id)?;
        self.out.reply = Some(Reply::Cancel(CancelReply::Requested { order_id: id }));
        Ok(())
    }

    /// True if a cancel was requested, nothing is in flight or confirmed, and
    /// the attempts of the current budget are used up.
    fn cancel_exhausted(&self, id: OrderId) -> bool {
        let max = self.core.config.policy.cancel_max_attempts;
        self.core
            .orders
            .get(&id)
            .and_then(|o| o.cancel.as_ref())
            .is_some_and(|c| !c.inflight && !c.confirmed && c.used() >= max)
    }

    /// Releases a cancel attempt if one is due. A cancel request never frees
    /// the order's reservation; only terminal evidence does.
    fn maybe_send_cancel(&mut self, id: OrderId) -> Result<(), CoreFault> {
        let policy = &self.core.config.policy;
        let Some(o) = self.core.orders.get(&id) else { return Ok(()) };
        let Some(c) = &o.cancel else { return Ok(()) };
        let sendable_phase = match o.phase {
            Phase::Working => true,
            // An order whose send outcome is unknown may exist; cancelling by
            // client id is harmless if it does not.
            Phase::PendingSend => o.doubt.is_some(),
            Phase::Closing | Phase::Closed => false,
        };
        let backoff_over = c.used() == 0 || self.at.saturating_sub(c.last_attempt_at) >= policy.cancel_retry_backoff_us;
        if self.core.link.up
            && sendable_phase
            && !c.inflight
            && !c.confirmed
            && c.used() < policy.cancel_max_attempts
            && backoff_over
        {
            let attempt = c.attempts + 1;
            self.emit(Event::CancelSent { order_id: id, attempt })?;
        }
        Ok(())
    }

    // ---- operator ---------------------------------------------------------

    fn operator(&mut self, cmd: &OperatorCmd) -> Result<(), CoreFault> {
        let reply: Result<String, String> = match cmd {
            OperatorCmd::Halt { scope, note } => {
                if let Scope::Strategy(s) = scope {
                    if !self.core.config.strategies.contains_key(s) {
                        self.out.reply = Some(Reply::Operator(Err(format!("unknown strategy {s}"))));
                        return Ok(());
                    }
                }
                self.emit(Event::OperatorHalted {
                    scope: scope.clone(),
                    note: note.clone(),
                })?;
                Ok("halted".into())
            }
            OperatorCmd::Resume { scope, note } => {
                let halted = match scope {
                    Scope::Account => self.core.halt_account.is_some(),
                    Scope::Strategy(s) => self.core.halt_strategy.contains_key(s),
                };
                if halted {
                    self.emit(Event::OperatorResumed {
                        scope: scope.clone(),
                        note: note.clone(),
                    })?;
                    Ok("resumed".into())
                } else {
                    Err("scope is not halted by an operator".into())
                }
            }
            OperatorCmd::AckMismatch { id, note } => match self.core.mismatches.get(id) {
                Some(m) if m.acked.is_none() => {
                    self.emit(Event::MismatchAcked {
                        id: *id,
                        note: note.clone(),
                    })?;
                    Ok("acknowledged".into())
                }
                Some(_) => Err("mismatch already acknowledged".into()),
                None => Err("unknown mismatch".into()),
            },
            OperatorCmd::ResolveOrder { order_id, note } => match self.core.orders.get(order_id) {
                Some(o) if !o.is_closed() => {
                    self.emit(Event::OrderClosed {
                        order_id: *order_id,
                        reason: CloseReason::Manual { note: note.clone() },
                    })?;
                    Ok("closed".into())
                }
                Some(_) => Err("order already closed".into()),
                None => Err("unknown order".into()),
            },
            OperatorCmd::CancelAll { scope, .. } => {
                let mut n = 0u32;
                for id in self.open_ids() {
                    let Some(o) = self.core.orders.get(&id) else { continue };
                    let in_scope = match scope {
                        Scope::Account => true,
                        Scope::Strategy(s) => *s == o.strategy,
                    };
                    if in_scope && (o.cancel.is_none() || self.cancel_exhausted(id)) {
                        self.emit(Event::CancelRequested {
                            order_id: id,
                            by: Actor::Operator,
                        })?;
                        self.maybe_send_cancel(id)?;
                        n += 1;
                    }
                }
                Ok(format!("cancel requested for {n} orders"))
            }
            OperatorCmd::AdjustPosition {
                instrument,
                ledger,
                delta,
                note,
            } => {
                let known_ledger = match ledger {
                    Ledger::Unattributed => true,
                    Ledger::Strategy(s) => self.core.config.strategies.contains_key(s),
                };
                let acct = self.core.account_book(instrument);
                let target = match ledger {
                    Ledger::Unattributed => self.core.unattributed.get(instrument).cloned().unwrap_or_default(),
                    Ledger::Strategy(s) => self.core.strategy_book(s, instrument),
                };
                if !self.core.config.instruments.contains_key(instrument) {
                    Err(format!("unknown instrument {instrument}"))
                } else if !known_ledger {
                    Err("unknown strategy".into())
                } else if acct.pos.checked_add(*delta).is_none() || target.pos.checked_add(*delta).is_none() {
                    Err("adjustment overflows the position".into())
                } else {
                    self.emit(Event::PositionAdjusted {
                        instrument: instrument.clone(),
                        ledger: ledger.clone(),
                        delta: *delta,
                        note: note.clone(),
                    })?;
                    Ok("adjusted".into())
                }
            }
            OperatorCmd::ClearReduce { note } => match self.clear_reduce_refusal(note) {
                Some(refusal) => Err(refusal.text().into()),
                None => {
                    self.clear_reduce(note)?;
                    Ok("cleared".into())
                }
            },
            OperatorCmd::RebaseDay { note } => {
                if !self.core.margin_report_is_fresh(self.at) {
                    Err(MarginRefusal::MarginUnavailable.text().into())
                } else if note.trim().is_empty() {
                    Err(MarginRefusal::NoteRequired.text().into())
                } else {
                    self.rebase_on_the_report()?;
                    Ok("rebased".into())
                }
            }
        };
        self.out.reply = Some(Reply::Operator(reply));
        Ok(())
    }

    /// Why `clear-reduce` cannot be taken now, if it cannot (C-MARGIN, 해제).
    /// The release is decided on numbers the venue gave within the last
    /// `margin_max_age_us`, and every release carries a reason.
    fn clear_reduce_refusal(&self, note: &str) -> Option<MarginRefusal> {
        if self.core.margin.reducing.is_none() {
            return Some(MarginRefusal::NotReducing);
        }
        if !self.core.margin_report_is_fresh(self.at) {
            return Some(MarginRefusal::MarginUnavailable);
        }
        if note.trim().is_empty() {
            return Some(MarginRefusal::NoteRequired);
        }
        None
    }

    /// Ends REDUCING on the newest report: its account value becomes the day
    /// base first, so that the loss that tripped the rule does not trip it
    /// again on the next report, and a further loss is measured from here.
    fn clear_reduce(&mut self, note: &str) -> Result<(), CoreFault> {
        let old_base = self.core.margin.day_base.as_ref().map(|base| base.value);
        let Some(new_base) = self.rebase_on_the_report()? else {
            return Ok(());
        };
        self.emit(Event::ReducingCleared {
            note: note.to_string(),
            old_base,
            new_base,
        })
    }

    /// Sets the day base to the account value and the UTC day of the newest
    /// report, by the operator. The day is the report's, also when it is
    /// earlier than the base's. Returns the new base value.
    fn rebase_on_the_report(&mut self) -> Result<Option<Amount>, CoreFault> {
        let Some(report) = &self.core.margin.report else {
            return Ok(None);
        };
        let (day, value) = (report.queried_at / DAY_US, report.account_value);
        self.emit(Event::DayBaseSet {
            day,
            value,
            set_by: BaseSetBy::Operator,
        })?;
        Ok(Some(value))
    }

    // ---- venue ------------------------------------------------------------

    fn venue(&mut self, v: &VenueInput) -> Result<(), CoreFault> {
        match v {
            VenueInput::Link { up, session, reason } => self.on_link(*up, *session, reason),
            VenueInput::SendResult { order_id, outcome, .. } => self.on_send_result(*order_id, outcome),
            VenueInput::CancelResult {
                order_id,
                attempt,
                outcome,
                ..
            } => self.on_cancel_result(*order_id, *attempt, outcome),
            VenueInput::OrderStatus(s) => self.on_status(s),
            VenueInput::StatusNotFound {
                order_id, queried_at, ..
            } => self.on_not_found(*order_id, *queried_at),
            VenueInput::Fill(f) => self.on_fill(f),
            VenueInput::FillsSynced { session, up_to } => self.on_fills_synced(*session, *up_to),
            VenueInput::OpenOrders {
                session,
                queried_at,
                orders,
            } => self.on_open_orders(*session, *queried_at, orders),
            VenueInput::Positions {
                session,
                queried_at,
                positions,
            } => self.on_positions(*session, *queried_at, positions),
            VenueInput::RefPrice {
                instrument,
                px,
                venue_ts,
            } => {
                if px.0 > 0 && self.core.config.instruments.contains_key(instrument) {
                    self.emit(Event::RefPriceUpdated {
                        instrument: instrument.clone(),
                        px: *px,
                        venue_ts: *venue_ts,
                    })?;
                }
                Ok(())
            }
            VenueInput::MarginReport(r) => self.on_margin_report(r),
            VenueInput::MarginReportInvalid {
                session,
                seq,
                queried_at,
                reason,
            } => {
                if !self.takes_margin_input(*session) {
                    return Ok(());
                }
                self.emit(Event::MarginReportRejected {
                    session: *session,
                    seq: *seq,
                    queried_at: *queried_at,
                    reason: reason.clone(),
                })
            }
        }
    }

    // ---- margin reports (C-MARGIN) ----------------------------------------

    /// Whether a margin input of link session `session` is looked at at all.
    /// Without a margin policy none is. An answer to a query of another link
    /// session, or one that arrives while the link is down, is dropped: not
    /// stored and not judged.
    fn takes_margin_input(&self, session: u64) -> bool {
        if self.core.config.policy.margin.is_none() {
            return false;
        }
        self.core.link.up && session == self.core.link.session
    }

    fn on_margin_report(&mut self, r: &MarginReportIn) -> Result<(), CoreFault> {
        if !self.takes_margin_input(r.session) {
            return Ok(());
        }
        // Answers can overtake each other. The query number decides which is
        // newer, not the query time: the wall clock can step backwards.
        let last_seq = self.core.margin.report.as_ref().filter(|last| last.session == r.session).map(|last| last.seq);
        if last_seq.is_some_and(|last| r.seq <= last) {
            return Ok(());
        }
        // A report is one fact. One value that no rule can use refuses the
        // whole report; the last recorded report stays and keeps ageing.
        if let Some(p) = r.positions.iter().find(|p| p.max_leverage == 0) {
            return self.emit(Event::MarginReportRejected {
                session: r.session,
                seq: r.seq,
                queried_at: r.queried_at,
                reason: format!("position {}: max_leverage is zero", p.instrument),
            });
        }
        self.emit(Event::MarginReported { report: r.clone() })?;
        self.judge_daily_loss(r)?;
        self.roll_day_base(r)
    }

    /// Daily loss (C-MARGIN): the account value of a recorded report is more
    /// than `daily_loss_bps` below the day base.
    fn judge_daily_loss(&mut self, r: &MarginReportIn) -> Result<(), CoreFault> {
        let (Some(policy), Some(base)) = (&self.core.config.policy.margin, &self.core.margin.day_base) else {
            return Ok(());
        };
        let (base_value, value) = (i128::from(base.value.0), i128::from(r.account_value.0));
        let lost_too_much = (base_value - value) * 10_000 > i128::from(policy.daily_loss_bps) * base_value;
        if !lost_too_much {
            return Ok(());
        }
        let detail = format!(
            "account value {} is more than {} bps below the day base {}",
            r.account_value, policy.daily_loss_bps, base.value
        );
        self.enter_reducing(RuleId::DailyLoss, detail)
    }

    /// Latches REDUCING for the whole account and, in the same input, asks
    /// to cancel every open plain order. An order with the venue's
    /// reduce-only flag is kept: it can only lower the position.
    fn enter_reducing(&mut self, rule: RuleId, detail: String) -> Result<(), CoreFault> {
        // Entered once: a second trigger emits nothing, and the rule that
        // tripped first stays recorded.
        if self.core.margin.reducing.is_some() {
            return Ok(());
        }
        self.emit(Event::ReducingEntered { rule, detail })?;
        for id in self.open_ids() {
            let Some(o) = self.core.orders.get(&id) else { continue };
            let is_plain = !o.reduce_only;
            if is_plain && (o.cancel.is_none() || self.cancel_exhausted(id)) {
                self.emit(Event::CancelRequested {
                    order_id: id,
                    by: Actor::Rule(rule),
                })?;
                // Sent now if the venue has the order. An order still
                // waiting for its send result keeps the request, and the
                // cancel goes out when the venue acknowledges it.
                self.maybe_send_cancel(id)?;
            }
        }
        Ok(())
    }

    /// Sets the day base from a recorded report when there is none, or when
    /// the report belongs to a later UTC day than the base. Runs after the
    /// report was judged against the old base. A report of an earlier day
    /// never moves the base back.
    fn roll_day_base(&mut self, r: &MarginReportIn) -> Result<(), CoreFault> {
        let day = r.queried_at / DAY_US;
        if self.core.margin.day_base.as_ref().is_some_and(|base| day <= base.day) {
            return Ok(());
        }
        self.emit(Event::DayBaseSet {
            day,
            value: r.account_value,
            set_by: BaseSetBy::Report,
        })
    }

    fn on_link(&mut self, up: bool, session: u64, reason: &str) -> Result<(), CoreFault> {
        if up {
            // A session that does not advance is a stale or duplicated notice.
            if session <= self.core.link.session {
                return Ok(());
            }
            self.emit(Event::LinkChanged {
                up: true,
                session,
                reason: reason.to_string(),
            })?;
            for id in self.open_ids() {
                self.maybe_send_cancel(id)?;
            }
        } else if self.core.link.up && session >= self.core.link.session {
            // A notice about an older session says nothing about this one.
            let session = self.core.link.session;
            self.emit(Event::LinkChanged {
                up: false,
                session,
                reason: reason.to_string(),
            })?;
        }
        Ok(())
    }

    fn on_send_result(&mut self, id: OrderId, outcome: &SendOutcome) -> Result<(), CoreFault> {
        let Some(o) = self.core.orders.get(&id) else { return Ok(()) };
        let phase = o.phase;
        match outcome {
            SendOutcome::Placed { venue_oid, status } => {
                let s = OrderStatusIn {
                    session: self.unsessioned_evidence(o),
                    order_id: Some(id),
                    venue_oid: Some(venue_oid.clone()),
                    status: status.clone(),
                    source: EvidenceSource::SendResult,
                };
                self.on_status(&s)
            }
            SendOutcome::Rejected { reason } => {
                if phase == Phase::PendingSend {
                    self.emit(Event::OrderClosed {
                        order_id: id,
                        reason: CloseReason::RejectedByVenue { reason: reason.clone() },
                    })
                } else if phase != Phase::Closed {
                    self.raise(
                        MismatchKind::StatusConflict,
                        format!("order:{id}"),
                        format!("send result says rejected ({reason}) but the order was already acknowledged"),
                    )
                } else {
                    Ok(())
                }
            }
            SendOutcome::NotSent { reason } => {
                if phase == Phase::PendingSend {
                    self.emit(Event::OrderClosed {
                        order_id: id,
                        reason: CloseReason::NotSent { reason: reason.clone() },
                    })
                } else if phase != Phase::Closed {
                    self.raise(
                        MismatchKind::StatusConflict,
                        format!("order:{id}"),
                        format!("gateway says not sent ({reason}) but the order was already acknowledged"),
                    )
                } else {
                    Ok(())
                }
            }
            SendOutcome::Unknown { reason } => {
                if phase == Phase::PendingSend && o.doubt.is_none() {
                    self.emit(Event::OrderInDoubt {
                        order_id: id,
                        reason: DoubtReason::Transport,
                        detail: reason.clone(),
                    })?;
                    self.maybe_send_cancel(id)?;
                }
                Ok(())
            }
        }
    }

    fn on_cancel_result(&mut self, id: OrderId, attempt: u32, outcome: &CancelOutcome) -> Result<(), CoreFault> {
        let Some(o) = self.core.orders.get(&id) else { return Ok(()) };
        let Some(c) = &o.cancel else { return Ok(()) };
        if !c.inflight || c.attempts != attempt || o.is_closed() {
            return Ok(());
        }
        self.emit(Event::CancelOutcomeNoted {
            order_id: id,
            attempt,
            outcome: outcome.clone(),
        })?;
        self.flag_stuck_cancel(id)
    }

    fn flag_stuck_cancel(&mut self, id: OrderId) -> Result<(), CoreFault> {
        let max = self.core.config.policy.cancel_max_attempts;
        let Some(o) = self.core.orders.get(&id) else { return Ok(()) };
        let Some(c) = &o.cancel else { return Ok(()) };
        if o.phase == Phase::Working && !c.confirmed && !c.inflight && c.used() >= max {
            self.raise(
                MismatchKind::CancelStuck,
                format!("order:{id}"),
                format!("cancel not confirmed after {} attempts ({} in all); request it again or cancel at the venue", c.used(), c.attempts),
            )?;
        }
        Ok(())
    }

    /// The session to credit evidence with that carries none of its own (a
    /// send result, a fill): the current one if the link has not changed
    /// since the order was released for sending, otherwise none.
    fn unsessioned_evidence(&self, o: &Order) -> u64 {
        if self.core.link.up && o.send_session == self.core.link.session {
            self.core.link.session
        } else {
            0
        }
    }

    /// True if `v` is already the venue id of an order other than `id`.
    fn venue_oid_taken(&self, v: &VenueOrderId, id: OrderId) -> bool {
        self.core.by_venue_oid.get(v).is_some_and(|other| *other != id)
    }

    fn resolve(&self, order_id: Option<OrderId>, venue_oid: Option<&VenueOrderId>) -> Option<OrderId> {
        if let Some(id) = order_id {
            if self.core.orders.contains_key(&id) {
                return Some(id);
            }
        }
        venue_oid.and_then(|v| self.core.by_venue_oid.get(v).copied())
    }

    fn on_status(&mut self, s: &OrderStatusIn) -> Result<(), CoreFault> {
        let Some(id) = self.resolve(s.order_id, s.venue_oid.as_ref()) else {
            return Ok(());
        };
        let Some(o) = self.core.orders.get(&id) else { return Ok(()) };
        if o.is_closed() {
            // An order closed without ever being acknowledged was closed on
            // the belief that the venue does not have it. Evidence that it
            // does must not vanish: its reservation is gone.
            let never_acked = o.acked_at == 0;
            let believed_absent = matches!(
                o.close.as_ref().map(|c| &c.reason),
                Some(CloseReason::NotPlaced { .. } | CloseReason::NotSent { .. } | CloseReason::Manual { .. })
            );
            if never_acked && matches!(s.status.status, VenueStatus::Open) {
                return self.raise(
                    MismatchKind::OpenAfterClose,
                    format!("order:{id}"),
                    format!("order {id} was closed without ever being acknowledged, and the venue reports it open"),
                );
            }
            if never_acked && believed_absent {
                return self.raise(
                    MismatchKind::StatusConflict,
                    format!("order:{id}"),
                    format!("order {id} was closed as never placed, and the venue reports a status for it"),
                );
            }
            return Ok(());
        }
        if let (Some(known), Some(reported)) = (&o.venue_oid, &s.venue_oid) {
            if known != reported {
                return self.raise(
                    MismatchKind::StatusConflict,
                    format!("order:{id}"),
                    format!("venue order id {reported} reported for an order known as {known}"),
                );
            }
        }
        if let Some(v) = &s.venue_oid {
            if self.venue_oid_taken(v, id) {
                return self.raise(
                    MismatchKind::StatusConflict,
                    format!("order:{id}"),
                    format!("venue order id {v} reported for order {id} already belongs to another order"),
                );
            }
        }
        let fresh = self.core.link.up && s.session == self.core.link.session;
        if o.phase == Phase::PendingSend {
            self.emit(Event::OrderAcked {
                order_id: id,
                venue_oid: s.venue_oid.clone(),
                via: s.source,
                session: if fresh { s.session } else { 0 },
            })?;
            self.drain_parked(id)?;
        } else if o.venue_oid.is_none() {
            if let Some(v) = &s.venue_oid {
                self.emit(Event::VenueOidLearned {
                    order_id: id,
                    venue_oid: v.clone(),
                })?;
                self.drain_parked(id)?;
            }
        }

        let Some(o) = self.core.orders.get(&id) else { return Ok(()) };
        if o.is_closed() {
            return Ok(());
        }
        let qty = o.qty;
        let reported = match (&s.status.status, s.status.cum) {
            (VenueStatus::Filled, _) => qty,
            (_, Some(c)) => c,
            (_, None) => o.reported_cum,
        }
        .max(o.reported_cum);
        // Evidence counts towards resync only if it was gathered on the
        // current link session. Older evidence is still true and is applied.
        let session = if fresh { s.session } else { o.evidence_session };
        if session != o.evidence_session || reported != o.reported_cum || o.recheck {
            self.emit(Event::StatusNoted {
                order_id: id,
                session,
                reported_cum: reported,
                venue_ts: s.status.venue_ts,
            })?;
        }

        match &s.status.status {
            VenueStatus::Open => {}
            VenueStatus::Filled => self.on_terminal(id, VenueFinal::Filled, "", Some(qty), s.status.venue_ts)?,
            VenueStatus::Canceled { reason } => {
                self.on_terminal(id, VenueFinal::Canceled, reason, s.status.cum, s.status.venue_ts)?
            }
            VenueStatus::Rejected { reason } => {
                self.on_terminal(id, VenueFinal::Rejected, reason, Some(Qty::ZERO), s.status.venue_ts)?
            }
        }
        self.maybe_send_cancel(id)
    }

    fn on_terminal(
        &mut self,
        id: OrderId,
        kind: VenueFinal,
        reason: &str,
        final_cum: Option<Qty>,
        venue_ts: Ts,
    ) -> Result<(), CoreFault> {
        let Some(o) = self.core.orders.get(&id) else { return Ok(()) };
        if o.is_closed() {
            return Ok(());
        }
        match &o.terminal {
            None => {
                self.emit(Event::VenueTerminal {
                    order_id: id,
                    kind,
                    reason: reason.to_string(),
                    final_cum,
                    venue_ts,
                })?;
            }
            Some(t) => match (t.final_cum, final_cum) {
                // A later report that adds the final quantity refines the
                // earlier one.
                (None, Some(_)) => {
                    self.emit(Event::VenueTerminal {
                        order_id: id,
                        kind: t.kind,
                        reason: t.reason.clone(),
                        final_cum,
                        venue_ts: t.venue_ts,
                    })?;
                }
                (Some(a), Some(b)) if a != b => {
                    self.raise(
                        MismatchKind::StatusConflict,
                        format!("order:{id}"),
                        format!("terminal reports disagree on final quantity: {a} then {b}"),
                    )?;
                }
                _ => {}
            },
        }
        self.try_close(id)
    }

    /// Closes an order that has terminal evidence once the booked fills
    /// account for everything the venue filled.
    fn try_close(&mut self, id: OrderId) -> Result<(), CoreFault> {
        let Some(o) = self.core.orders.get(&id) else { return Ok(()) };
        let Some(t) = &o.terminal else { return Ok(()) };
        if o.is_closed() {
            return Ok(());
        }
        let synced = self.core.sync.fills_up_to >= t.venue_ts;
        let reason = match t.kind {
            VenueFinal::Filled => CloseReason::Filled,
            // A venue that does not say "filled" reliably reports every
            // terminal state the same way; the booked fills tell which it was.
            VenueFinal::Canceled if o.cum_qty == o.qty => CloseReason::Filled,
            VenueFinal::Canceled => CloseReason::Canceled {
                venue_reason: t.reason.clone(),
            },
            VenueFinal::Rejected => CloseReason::RejectedByVenue {
                reason: t.reason.clone(),
            },
        };
        let (cum, flagged) = (o.cum_qty, t.missing_fills_flagged);
        match t.final_cum {
            Some(f) if cum == f => self.emit(Event::OrderClosed { order_id: id, reason }),
            Some(f) if cum > f => {
                self.raise(
                    MismatchKind::CumExceedsFinal,
                    format!("order:{id}"),
                    format!("booked fills {cum} exceed the venue's final quantity {f}"),
                )?;
                self.emit(Event::OrderClosed { order_id: id, reason })
            }
            Some(f) => {
                if synced && !flagged {
                    self.raise(
                        MismatchKind::MissingFills,
                        format!("order:{id}"),
                        format!("venue reports {f} filled; only {cum} found in fills up to the terminal time"),
                    )?;
                    self.emit(Event::MissingFillsFlagged { order_id: id })?;
                }
                Ok(())
            }
            None if synced => self.emit(Event::OrderClosed { order_id: id, reason }),
            None => Ok(()),
        }
    }

    fn on_not_found(&mut self, id: OrderId, queried_at: Ts) -> Result<(), CoreFault> {
        let policy = &self.core.config.policy;
        let Some(o) = self.core.orders.get(&id) else { return Ok(()) };
        if matches!(o.phase, Phase::Working | Phase::Closing) {
            // An answer to a query made before the acknowledgement, or so
            // soon after it that a lagging query node may not have seen the
            // order yet, says nothing.
            if queried_at < o.acked_at.saturating_add(policy.absence_margin_us) {
                return Ok(());
            }
            // The venue acknowledged this order once and now does not know
            // it. Nothing can be concluded about its fills, so it is not
            // closed; the operator has to look. Without this notice the
            // order would only show as a re-confirmation that never ends.
            return self.raise(
                MismatchKind::StatusConflict,
                format!("order:{id}"),
                format!("the venue no longer knows order {id}, which it had acknowledged (queried at {queried_at})"),
            );
        }
        if o.phase != Phase::PendingSend || o.doubt.is_none() || !policy.venue_enforces_expiry {
            return Ok(());
        }
        let Some(threshold) = o.expires_at.checked_add(policy.absence_margin_us) else {
            return Ok(());
        };
        if queried_at < threshold {
            return Ok(());
        }
        let evidence = format!(
            "not found by client id; queried at {queried_at}, after send expiry {} plus margin {}",
            o.expires_at, policy.absence_margin_us
        );
        self.emit(Event::OrderClosed {
            order_id: id,
            reason: CloseReason::NotPlaced { evidence },
        })
    }

    fn on_fill(&mut self, f: &FillIn) -> Result<(), CoreFault> {
        if f.qty.0 <= 0 {
            return Ok(());
        }
        // Duplicate: already booked, or already waiting.
        if self.core.seen_fills.contains_key(&f.key) || self.core.parked_fills.iter().any(|p| p.fill.key == f.key) {
            return Ok(());
        }
        // Re-delivery of a fill from before the floor: by the gateway's
        // fill-sync contract it was delivered, and booked, long ago.
        if f.venue_ts < self.core.fill_floor {
            return Ok(());
        }
        match self.resolve(f.order_id, f.venue_oid.as_ref()) {
            Some(id) => self.book_fill(id, f),
            None => {
                // The fill may belong to an order of ours whose venue id is
                // not known yet (its send result has not arrived).
                let may_be_ours = f.order_id.is_none()
                    && self
                        .core
                        .open
                        .iter()
                        .filter_map(|id| self.core.orders.get(id))
                        .any(|o| o.venue_oid.is_none() && o.instrument == f.instrument && o.side == f.side);
                if may_be_ours {
                    if (self.core.parked_fills.len() as u64) < u64::from(self.core.config.policy.max_parked_fills) {
                        return self.emit(Event::FillParked { fill: f.clone() });
                    }
                    self.raise(
                        MismatchKind::ParkedFillOverflow,
                        "parked".into(),
                        "too many fills waiting for their order to be identified".into(),
                    )?;
                }
                self.book_external(f, "fill for an order this instance does not know")
            }
        }
    }

    fn book_external(&mut self, f: &FillIn, why: &str) -> Result<(), CoreFault> {
        self.emit(Event::ExternalFillBooked { fill: f.clone() })?;
        // One entry per instrument while it is unacknowledged, however many
        // fills follow: each fill is its own journal record and is in the
        // unattributed ledger, so nothing is lost by not listing them all.
        self.raise(
            MismatchKind::ExternalFill,
            format!("instrument:{}", f.instrument),
            format!(
                "{why}: {:?} {} {} @ {} (fill {}; later external fills on this instrument are booked without a new entry)",
                f.side, f.qty, f.instrument, f.px, f.key
            ),
        )
    }

    fn book_fill(&mut self, id: OrderId, f: &FillIn) -> Result<(), CoreFault> {
        // Duplicates were already dropped by `on_fill`; a parked fill reaches
        // this point once, when it is unparked.
        let Some(o) = self.core.orders.get(&id) else { return Ok(()) };
        if o.instrument != f.instrument || o.side != f.side {
            self.raise(
                MismatchKind::FillFieldConflict,
                format!("order:{id}"),
                format!("fill {} names order {id} but its instrument or side differs", f.key),
            )?;
            return self.emit(Event::ExternalFillBooked { fill: f.clone() });
        }
        let was_closed = o.is_closed();
        // A venue order id that already belongs to another order is not
        // adopted; the fill itself is booked to the order it names.
        let mut venue_oid = f.venue_oid.clone();
        if venue_oid.as_ref().is_some_and(|v| self.venue_oid_taken(v, id)) {
            self.raise(
                MismatchKind::StatusConflict,
                format!("order:{id}"),
                format!("fill {} pairs order {id} with a venue order id that belongs to another order", f.key),
            )?;
            venue_oid = None;
        }
        let Some(o) = self.core.orders.get(&id) else { return Ok(()) };
        let mut identified = false;
        if o.phase == Phase::PendingSend {
            let session = self.unsessioned_evidence(o);
            identified = venue_oid.is_some();
            self.emit(Event::OrderAcked {
                order_id: id,
                venue_oid,
                via: EvidenceSource::Fill,
                session,
            })?;
        } else if o.venue_oid.is_none() && !was_closed {
            if let Some(v) = venue_oid {
                identified = true;
                self.emit(Event::VenueOidLearned {
                    order_id: id,
                    venue_oid: v,
                })?;
            }
        }
        self.emit(Event::FillBooked {
            order_id: id,
            key: f.key.clone(),
            qty: f.qty,
            px: f.px,
            fee: f.fee,
            venue_ts: f.venue_ts,
        })?;
        if identified {
            // Fills that named only this venue order id were waiting for it.
            self.drain_parked(id)?;
        }
        let Some(o) = self.core.orders.get(&id) else { return Ok(()) };
        let (cum, qty, has_terminal) = (o.cum_qty, o.qty, o.terminal.is_some());
        if cum > qty {
            self.raise(
                MismatchKind::Overfill,
                format!("order:{id}"),
                format!("booked fills {cum} exceed the order quantity {qty}"),
            )?;
        }
        if was_closed {
            return self.raise(
                MismatchKind::FillAfterClose,
                format!("order:{id}"),
                format!("fill {} of {} booked on closed order {id}", f.key, f.qty),
            );
        }
        if !has_terminal && cum >= qty {
            // The booked fills alone prove the order is complete.
            self.emit(Event::VenueTerminal {
                order_id: id,
                kind: VenueFinal::Filled,
                reason: "booked fills reached the order quantity".into(),
                final_cum: Some(qty),
                venue_ts: f.venue_ts,
            })?;
        }
        self.try_close(id)?;
        self.maybe_send_cancel(id)
    }

    /// Books parked fills that belong to an order whose venue id was just
    /// learned.
    fn drain_parked(&mut self, id: OrderId) -> Result<(), CoreFault> {
        let Some(v) = self.core.orders.get(&id).and_then(|o| o.venue_oid.clone()) else {
            return Ok(());
        };
        let matching: Vec<FillIn> = self
            .core
            .parked_fills
            .iter()
            .filter(|p| p.fill.venue_oid.as_ref() == Some(&v))
            .map(|p| p.fill.clone())
            .collect();
        for f in matching {
            self.emit(Event::FillUnparked { key: f.key.clone() })?;
            self.book_fill(id, &f)?;
        }
        Ok(())
    }

    fn on_fills_synced(&mut self, session: u64, up_to: Ts) -> Result<(), CoreFault> {
        if !self.core.link.up || session != self.core.link.session {
            return Ok(());
        }
        if self.core.sync.fills_session != session || up_to > self.core.sync.fills_up_to {
            self.emit(Event::FillsSynced { session, up_to })?;
        }
        for id in self.open_ids() {
            self.try_close(id)?;
        }
        Ok(())
    }

    fn on_open_orders(&mut self, session: u64, queried_at: Ts, orders: &[VenueOpenOrder]) -> Result<(), CoreFault> {
        if !self.core.link.up || session != self.core.link.session {
            return Ok(());
        }
        for vo in orders {
            // The venue's list may not carry our client id; an order whose
            // venue id we already know is still recognised as ours.
            let known = self
                .resolve(vo.order_id, Some(&vo.venue_oid))
                .and_then(|id| self.core.orders.get(&id));
            match known {
                Some(o) if o.is_closed() => {
                    // Only a snapshot taken after the close contradicts it.
                    let closed_at = o.close.as_ref().map(|c| c.at).unwrap_or(0);
                    if closed_at < queried_at {
                        let id = o.id;
                        self.raise(
                            MismatchKind::OpenAfterClose,
                            format!("venue:{}", vo.venue_oid),
                            format!("order {id} is closed here but open at the venue"),
                        )?;
                    }
                }
                Some(o) => {
                    let (id, qty) = (o.id, o.qty);
                    let cum = vo.remaining.filter(|r| *r <= qty).map(|r| Qty(qty.0 - r.0));
                    self.on_status(&OrderStatusIn {
                        session,
                        order_id: Some(id),
                        venue_oid: Some(vo.venue_oid.clone()),
                        status: StatusIn {
                            status: VenueStatus::Open,
                            cum,
                            venue_ts: 0,
                        },
                        source: EvidenceSource::Query,
                    })?;
                }
                None if vo.order_id.is_some() => {
                    self.raise(
                        MismatchKind::OrphanOrder,
                        format!("venue:{}", vo.venue_oid),
                        format!(
                            "open order with this instance's client id is unknown here: {:?} {}",
                            vo.side, vo.instrument
                        ),
                    )?;
                }
                None => {
                    // The list may not carry our client id. An order of ours
                    // whose venue id is not known yet could be this one; the
                    // next sweep sees it identified, or flags it then.
                    let may_be_ours = self
                        .core
                        .open
                        .iter()
                        .filter_map(|id| self.core.orders.get(id))
                        .any(|o| o.venue_oid.is_none() && o.instrument == vo.instrument && o.side == vo.side);
                    if may_be_ours {
                        continue;
                    }
                    self.raise(
                        MismatchKind::ForeignOrder,
                        format!("venue:{}", vo.venue_oid),
                        format!(
                            "open order not placed by this instance: {:?} {}",
                            vo.side, vo.instrument
                        ),
                    )?;
                }
            }
        }
        // An order acknowledged before the sweep was requested and absent
        // from it is no longer open at the venue, or the stream missed
        // something. Either way its status has to be asked for.
        for id in self.open_ids() {
            let Some(o) = self.core.orders.get(&id) else { continue };
            let listed = orders
                .iter()
                .any(|vo| vo.order_id == Some(id) || o.venue_oid.as_ref() == Some(&vo.venue_oid));
            if o.phase == Phase::Working && !o.recheck && !listed && o.acked_at < queried_at {
                self.emit(Event::OrderRecheck { order_id: id })?;
            }
        }
        if self.core.sync.open_orders_session != session {
            self.emit(Event::OpenOrdersSwept { session })?;
        }
        Ok(())
    }

    /// Compares venue positions with the account books.
    ///
    /// The venue position must lie in `[pos - sell_open, pos + buy_open]`:
    /// fills that happened but are not booked yet are covered by the open
    /// reservations. The comparison is skipped if a fill was booked after the
    /// query was sent, because the snapshot may predate that fill.
    fn on_positions(&mut self, session: u64, queried_at: Ts, positions: &[VenuePosition]) -> Result<(), CoreFault> {
        if !self.core.link.up || session != self.core.link.session {
            return Ok(());
        }
        if self.core.last_fill_at >= queried_at {
            return Ok(());
        }
        let mut clean = true;
        let mut instruments: Vec<InstrumentId> = self.core.config.instruments.keys().cloned().collect();
        for p in positions {
            if !instruments.contains(&p.instrument) {
                instruments.push(p.instrument.clone());
            }
        }
        for i in instruments {
            let venue = positions
                .iter()
                .find(|p| p.instrument == i)
                .map(|p| p.qty.0)
                .unwrap_or(0);
            if !self.core.config.instruments.contains_key(&i) {
                if venue != 0 {
                    clean = false;
                    self.raise(
                        MismatchKind::ForeignPosition,
                        format!("instrument:{i}"),
                        format!("venue position {} on an instrument that is not configured", SQty(venue)),
                    )?;
                }
                continue;
            }
            let b = self.core.account_book(&i);
            let (lo, hi) = (b.lower().unwrap_or(i64::MIN), b.upper().unwrap_or(i64::MAX));
            if venue < lo || venue > hi {
                clean = false;
                self.raise(
                    MismatchKind::PositionOutOfRange,
                    format!("instrument:{i}"),
                    format!(
                        "venue position {} outside the reachable range [{}, {}] (booked {})",
                        SQty(venue),
                        SQty(lo),
                        SQty(hi),
                        b.pos
                    ),
                )?;
            }
        }
        if clean && self.core.sync.positions_session != session {
            self.emit(Event::PositionsVerified { session })?;
        }
        Ok(())
    }

    // ---- time -------------------------------------------------------------

    fn tick(&mut self) -> Result<(), CoreFault> {
        let policy = self.core.config.policy.clone();
        for id in self.open_ids() {
            let Some(o) = self.core.orders.get(&id) else { continue };
            if o.phase == Phase::PendingSend
                && o.doubt.is_none()
                && self.at.saturating_sub(o.accepted_at) >= policy.send_result_timeout_us
            {
                self.emit(Event::OrderInDoubt {
                    order_id: id,
                    reason: DoubtReason::NoResult,
                    detail: "no send result reached the core in time".into(),
                })?;
            }
            // A cancel whose result never arrives must not stay in flight for
            // ever: after the same time a send is given, it counts as unknown.
            let lost = self
                .core
                .orders
                .get(&id)
                .and_then(|o| o.cancel.as_ref())
                .filter(|c| c.inflight && self.at.saturating_sub(c.last_attempt_at) >= policy.send_result_timeout_us)
                .map(|c| c.attempts);
            if let Some(attempt) = lost {
                self.emit(Event::CancelOutcomeNoted {
                    order_id: id,
                    attempt,
                    outcome: CancelOutcome::Unknown {
                        reason: "no cancel result reached the core in time".into(),
                    },
                })?;
                self.flag_stuck_cancel(id)?;
            }
            self.maybe_send_cancel(id)?;
        }

        // A parked fill is given up as external when no order of ours could
        // still turn out to own it, or when it has waited too long.
        let overdue: Vec<(FillIn, &'static str)> = self
            .core
            .parked_fills
            .iter()
            .filter_map(|p| {
                let candidate = self
                    .core
                    .open
                    .iter()
                    .filter_map(|id| self.core.orders.get(id))
                    .any(|o| o.venue_oid.is_none() && o.instrument == p.fill.instrument && o.side == p.fill.side);
                if !candidate {
                    Some((p.fill.clone(), "fill that no order of this instance can own"))
                } else if self.at.saturating_sub(p.parked_at) >= policy.unmatched_fill_wait_us {
                    Some((p.fill.clone(), "fill whose order was not identified in time"))
                } else {
                    None
                }
            })
            .collect();
        for (f, why) in overdue {
            self.emit(Event::FillUnparked { key: f.key.clone() })?;
            self.book_external(&f, why)?;
        }

        if self.at.saturating_sub(self.core.last_purge_at) >= policy.purge_interval_us {
            let order_ids: Vec<OrderId> = self
                .core
                .orders
                .values()
                .filter(|o| {
                    let Some(c) = &o.close else { return false };
                    // An idempotency key may be forgotten only after its
                    // request has expired, so a late duplicate is refused as
                    // expired instead of creating a second order.
                    self.at >= c.at.saturating_add(policy.closed_retention_us)
                        && self.at >= o.expires_at.saturating_add(policy.dedup_grace_us)
                })
                .map(|o| o.id)
                .collect();
            let cutoff = self.at.saturating_sub(policy.closed_retention_us);
            // The floor follows the fill-sync watermark, never the wall
            // clock: only fills known to be complete may be forgotten.
            let fill_floor = self
                .core
                .sync
                .fills_up_to
                .saturating_sub(policy.fill_dedup_retention_us)
                .max(self.core.fill_floor);
            self.emit(Event::Purged {
                order_ids,
                cutoff,
                fill_floor,
            })?;
        }
        Ok(())
    }
}

/// The furthest position the book can reach on `side` if `qty` more is
/// accepted: `pos + buy_open + qty` for a buy, `pos - sell_open - qty` for a
/// sell. `None` on overflow.
fn reach_after(b: &Book, side: Side, qty: Qty) -> Option<i64> {
    match side {
        Side::Buy => b.upper()?.checked_add(qty.0),
        Side::Sell => b.lower()?.checked_sub(qty.0),
    }
}
