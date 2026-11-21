//! Shared harness for core scenario tests.
//!
//! Every input goes through `Core::handle`; the produced events are also
//! applied to a shadow core with `Core::apply` only. After each input the two
//! must be identical and the structural invariants must hold. So every
//! scenario also checks that recovery-by-replay reproduces the live state.

#![allow(dead_code, clippy::unwrap_used)]

use oms_core::*;
use std::collections::BTreeMap;

pub fn sid(s: &str) -> StrategyId {
    StrategyId::new(s).unwrap()
}
pub fn iid(s: &str) -> InstrumentId {
    InstrumentId::new(s).unwrap()
}
pub fn cid(s: &str) -> ClientOrderId {
    ClientOrderId::new(s).unwrap()
}
pub fn void(s: &str) -> VenueOrderId {
    VenueOrderId::new(s).unwrap()
}
pub fn fkey(s: &str) -> FillKey {
    FillKey::new(s).unwrap()
}
pub fn q(s: &str) -> Qty {
    Qty::parse(s).unwrap()
}
pub fn px(s: &str) -> Px {
    Px::parse(s).unwrap()
}
pub fn sq(s: &str) -> SQty {
    SQty::parse(s).unwrap()
}
pub fn amt(s: &str) -> Amount {
    Amount::parse(s).unwrap()
}

pub const SEC: u64 = 1_000_000;

/// Two strategies "a" and "b" on one instrument "X". Account and strategy
/// position bounds are both [-10, 10] unless a test changes them.
pub fn config() -> CoreConfig {
    let mut instruments = BTreeMap::new();
    instruments.insert(
        iid("X"),
        InstrumentSpec {
            qty_step: q("0.001"),
            px_tick: px("0.1"),
            px_max_sig_figs: Some(5),
            min_notional: amt("10"),
            max_order_qty: q("100"),
            max_order_notional: amt("1000000"),
            price_band_bps: None,
            account_pos_min: sq("-10"),
            account_pos_max: sq("10"),
        },
    );
    let mut strategies = BTreeMap::new();
    for name in ["a", "b"] {
        let mut limits = BTreeMap::new();
        limits.insert(
            iid("X"),
            PosLimit {
                min: sq("-10"),
                max: sq("10"),
            },
        );
        strategies.insert(
            sid(name),
            StrategySpec {
                enabled: true,
                max_open_orders: 50,
                limits,
            },
        );
    }
    CoreConfig {
        version: 1,
        instruments,
        strategies,
        policy: Policy::default(),
    }
}

/// `config()` with a margin policy at its default values, and the account
/// mode `STANDARD_MODE` on the allow-list (the default list is empty and
/// lets no plain order through). A plain order then needs a fresh margin
/// report: see `H::report_margin`.
pub fn config_with_margin() -> CoreConfig {
    let mut cfg = config();
    cfg.policy.margin = Some(MarginPolicy {
        account_modes_allowed: vec![STANDARD_MODE.into()],
        ..MarginPolicy::default()
    });
    cfg
}

/// What the venue of these tests calls its ordinary account mode. The
/// string the real venue uses is not known (T10); the default allow-list is
/// empty and the operator lists it.
pub const STANDARD_MODE: &str = "standard";

pub struct H {
    pub core: Core,
    pub shadow: Core,
    pub now: Ts,
    pub session: u64,
    pub log: Vec<Record>,
    pub commands: Vec<VenueCommand>,
    /// Number of the last margin query made with `report_margin`.
    pub margin_seq: u64,
}

impl H {
    pub fn new() -> H {
        H {
            core: Core::new(7),
            shadow: Core::new(7),
            now: 1_000 * SEC,
            session: 0,
            log: Vec::new(),
            commands: Vec::new(),
            margin_seq: 0,
        }
    }

    /// Booted, configured, link up and fully synced: ready to trade.
    pub fn ready() -> H {
        Self::ready_with(config())
    }

    pub fn ready_with(cfg: CoreConfig) -> H {
        let mut h = H::configured(cfg);
        h.link_up_and_sync();
        // A margin policy that does not list the mode of this venue holds
        // the account.
        let lists_the_mode = h.core.config.policy.margin.as_ref().is_none_or(|m| {
            m.account_modes_allowed
                .iter()
                .any(|mode| mode == STANDARD_MODE)
        });
        let expected = if lists_the_mode {
            vec![]
        } else {
            vec![HoldReason::AccountMode]
        };
        assert_eq!(h.core.holds(None), expected);
        h
    }

    /// Booted and configured. The link has never been up.
    pub fn configured(cfg: CoreConfig) -> H {
        let mut h = H::new();
        h.inp(Input::Boot {
            binary: "test".into(),
            kill_marker: None,
        });
        let out = h.inp(Input::Config(Box::new(cfg)));
        assert!(
            matches!(out.reply, Some(Reply::Config(Ok(_)))),
            "{:?}",
            out.reply
        );
        h
    }

    /// A new link session, fully synced. With a margin policy the venue
    /// also tells what the policy asks for: see `venue_matches_policy`.
    pub fn link_up_and_sync(&mut self) {
        self.link_up_and_sync_only();
        self.venue_matches_policy();
    }

    /// A new link session with the three account-level sweeps and nothing
    /// else: the venue has told no venue setting and no account mode in it.
    pub fn link_up_and_sync_only(&mut self) {
        self.session += 1;
        let session = self.session;
        self.venue(VenueInput::Link {
            up: true,
            session,
            reason: "test".into(),
        });
        self.sync();
    }

    /// With a margin policy: the venue tells, for every configured
    /// instrument, the margin mode and the leverage the policy asks for,
    /// and an account mode the policy lists. Without a margin policy:
    /// nothing.
    pub fn venue_matches_policy(&mut self) {
        let Some(policy) = self.core.config.policy.margin.clone() else {
            return;
        };
        let instruments: Vec<InstrumentId> = self.core.config.instruments.keys().cloned().collect();
        for instrument in instruments {
            self.venue(VenueInput::VenueSetting {
                session: self.session,
                instrument,
                isolated: policy.isolated,
                leverage: policy.venue_leverage,
            });
        }
        self.tell_account_mode();
    }

    /// With a margin policy that lists `STANDARD_MODE`: the venue tells that
    /// mode. Otherwise nothing.
    pub fn tell_account_mode(&mut self) {
        let listed = self.core.config.policy.margin.as_ref().and_then(|m| {
            m.account_modes_allowed
                .iter()
                .find(|mode| *mode == STANDARD_MODE)
                .cloned()
        });
        if let Some(mode) = listed {
            self.account_mode(&mode);
        }
    }

    /// The venue's answer, in the current link session, to the venue-setting
    /// query for `instrument`.
    pub fn venue_setting(&mut self, instrument: &str, isolated: bool, leverage: u32) -> Output {
        self.venue(VenueInput::VenueSetting {
            session: self.session,
            instrument: iid(instrument),
            isolated,
            leverage,
        })
    }

    /// Delivers the three account-level sweeps with nothing to report.
    pub fn sync(&mut self) {
        let session = self.session;
        let t = self.now;
        self.venue(VenueInput::FillsSynced { session, up_to: t });
        self.venue(VenueInput::OpenOrders {
            session,
            queried_at: self.now + 1,
            orders: vec![],
        });
        let positions = self
            .core
            .account
            .iter()
            .map(|(i, b)| VenuePosition {
                instrument: i.clone(),
                qty: b.pos,
            })
            .collect();
        self.venue(VenueInput::Positions {
            session,
            queried_at: self.now + 1,
            positions,
        });
    }

    pub fn advance(&mut self, us: u64) {
        self.now += us;
    }

    pub fn inp(&mut self, input: Input) -> Output {
        self.now += 1_000;
        let out = self.core.handle(self.now, &input).expect("core fault");
        for ev in &out.events {
            self.shadow.apply(self.now, ev).expect("apply fault");
        }
        assert_eq!(
            self.core, self.shadow,
            "replayed state differs from live state"
        );
        self.core.check_invariants().expect("invariant");
        if !out.events.is_empty() {
            self.log.push(Record {
                seq: self.log.len() as u64 + 1,
                at: self.now,
                input,
                events: out.events.clone(),
            });
        }
        self.commands.extend(out.commands.iter().cloned());
        out
    }

    pub fn venue(&mut self, v: VenueInput) -> Output {
        self.inp(Input::Venue(v))
    }

    /// A margin report of link session `session` with number `seq`, asked
    /// for at `queried_at`, with no positions.
    pub fn margin_report(
        &mut self,
        session: u64,
        seq: u64,
        queried_at: Ts,
        account_value: &str,
    ) -> Output {
        self.venue(VenueInput::MarginReport(MarginReport {
            session,
            seq,
            queried_at,
            account_value: amt(account_value),
            positions: vec![],
            account_mm: None,
        }))
    }

    /// The next margin query of the current link session, asked now and
    /// answered at once with this account value. With a margin policy a
    /// plain order needs such a report no older than 30 s.
    pub fn report_margin(&mut self, account_value: &str) -> Output {
        self.margin_seq += 1;
        self.margin_report(self.session, self.margin_seq, self.now, account_value)
    }

    /// The venue's reference price (mid) of `instrument`, recorded now.
    pub fn ref_price(&mut self, instrument: &str, price: &str) -> Output {
        let venue_ts = self.now;
        self.venue(VenueInput::RefPrice {
            instrument: iid(instrument),
            px: px(price),
            venue_ts,
        })
    }

    /// The venue's answer, in the current link session, to the
    /// account-mode query.
    pub fn account_mode(&mut self, mode: &str) -> Output {
        self.venue(VenueInput::AccountMode {
            session: self.session,
            mode: mode.into(),
        })
    }

    pub fn tick(&mut self) -> Output {
        self.inp(Input::Tick)
    }

    pub fn req(
        &self,
        strategy: &str,
        key: &str,
        side: Side,
        qty: &str,
        price: &str,
    ) -> NewOrderReq {
        NewOrderReq {
            strategy: sid(strategy),
            client_order_id: cid(key),
            instrument: iid("X"),
            side,
            qty: q(qty),
            px: px(price),
            tif: Tif::Gtc,
            reduce_only: false,
            expires_at: self.now + 5 * SEC,
            market_ref: None,
        }
    }

    /// `req` with the venue's reduce-only flag set.
    pub fn reduce_only_req(
        &self,
        strategy: &str,
        key: &str,
        side: Side,
        qty: &str,
        price: &str,
    ) -> NewOrderReq {
        NewOrderReq {
            reduce_only: true,
            ..self.req(strategy, key, side, qty, price)
        }
    }

    /// Strategy `strategy` buys `qty` and the venue reports it filled.
    pub fn long(&mut self, strategy: &str, key: &str, qty: &str) {
        let id = self.accept(strategy, key, Side::Buy, qty, "100");
        self.placed_open(id, key);
        self.fill(id, key, qty, "100");
        self.status(id, VenueStatus::Filled, Some(qty));
        assert_eq!(self.order(id).phase, Phase::Closed);
    }

    pub fn new_order(&mut self, r: NewOrderReq) -> NewOrderReply {
        match self.inp(Input::NewOrder(r)).reply {
            Some(Reply::NewOrder(r)) => r,
            other => panic!("unexpected reply {other:?}"),
        }
    }

    /// Submits and returns the id of an accepted order.
    pub fn accept(
        &mut self,
        strategy: &str,
        key: &str,
        side: Side,
        qty: &str,
        price: &str,
    ) -> OrderId {
        let r = self.req(strategy, key, side, qty, price);
        match self.new_order(r) {
            NewOrderReply::Accepted { order_id } => order_id,
            other => panic!("expected acceptance, got {other:?}"),
        }
    }

    pub fn reject(
        &mut self,
        strategy: &str,
        key: &str,
        side: Side,
        qty: &str,
        price: &str,
    ) -> Vec<RejectReason> {
        let r = self.req(strategy, key, side, qty, price);
        match self.new_order(r) {
            NewOrderReply::Rejected { reasons } => reasons,
            other => panic!("expected rejection, got {other:?}"),
        }
    }

    pub fn placed_open(&mut self, id: OrderId, oid: &str) -> Output {
        self.venue(VenueInput::SendResult {
            order_id: id,
            outcome: SendOutcome::Placed {
                venue_oid: void(oid),
                status: StatusIn {
                    status: VenueStatus::Open,
                    cum: Some(Qty::ZERO),
                    venue_ts: self.now,
                },
            },
            timing: Default::default(),
        })
    }

    pub fn fill(&mut self, id: OrderId, key: &str, qty: &str, price: &str) -> Output {
        let o = self.core.order(id).expect("order").clone();
        self.venue(VenueInput::Fill(FillIn {
            key: fkey(key),
            order_id: Some(id),
            venue_oid: o.venue_oid.clone(),
            instrument: o.instrument.clone(),
            side: o.side,
            qty: q(qty),
            px: px(price),
            fee: Amount::ZERO,
            venue_ts: self.now,
            is_maker: None,
            recv_ns: None,
        }))
    }

    pub fn status(&mut self, id: OrderId, status: VenueStatus, cum: Option<&str>) -> Output {
        let session = self.session;
        let venue_ts = self.now;
        self.venue(VenueInput::OrderStatus(OrderStatusIn {
            session,
            order_id: Some(id),
            venue_oid: None,
            status: StatusIn {
                status,
                cum: cum.map(q),
                venue_ts,
            },
            source: EvidenceSource::Stream,
        }))
    }

    pub fn cancel(&mut self, strategy: &str, id: OrderId) -> CancelReply {
        match self
            .inp(Input::Cancel(CancelReq {
                actor: Actor::Strategy(sid(strategy)),
                target: CancelTarget::Order(id),
            }))
            .reply
        {
            Some(Reply::Cancel(r)) => r,
            other => panic!("unexpected reply {other:?}"),
        }
    }

    pub fn op(&mut self, cmd: OperatorCmd) -> Result<String, String> {
        match self.inp(Input::Operator(cmd)).reply {
            Some(Reply::Operator(r)) => r,
            other => panic!("unexpected reply {other:?}"),
        }
    }

    pub fn order(&self, id: OrderId) -> &Order {
        self.core.order(id).expect("order")
    }

    pub fn acct(&self) -> Book {
        self.core.account_book(&iid("X"))
    }

    pub fn book(&self, strategy: &str) -> Book {
        self.core.strategy_book(&sid(strategy), &iid("X"))
    }

    pub fn sends(&self) -> Vec<OrderId> {
        self.commands
            .iter()
            .filter_map(|c| match c {
                VenueCommand::SendOrder { order_id, .. } => Some(*order_id),
                _ => None,
            })
            .collect()
    }

    pub fn cancels(&self) -> Vec<(OrderId, u32)> {
        self.commands
            .iter()
            .filter_map(|c| match c {
                VenueCommand::SendCancel {
                    order_id, attempt, ..
                } => Some((*order_id, *attempt)),
                _ => None,
            })
            .collect()
    }

    pub fn unacked_mismatches(&self) -> Vec<MismatchKind> {
        self.core
            .mismatches
            .values()
            .filter(|m| m.acked.is_none())
            .map(|m| m.kind)
            .collect()
    }

    /// Rebuilds a core from the recorded log alone, as recovery does.
    pub fn rebuilt(&self) -> Core {
        let mut c = Core::new(self.core.instance_id);
        for rec in &self.log {
            c.replay(rec).expect("replay");
        }
        c
    }
}
