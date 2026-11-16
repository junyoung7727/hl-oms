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

/// `config()` with a margin policy at its default values.
pub fn config_with_margin() -> CoreConfig {
    let mut cfg = config();
    cfg.policy.margin = Some(MarginPolicy::default());
    cfg
}

pub struct H {
    pub core: Core,
    pub shadow: Core,
    pub now: Ts,
    pub session: u64,
    pub log: Vec<Record>,
    pub commands: Vec<VenueCommand>,
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
        }
    }

    /// Booted, configured, link up and fully synced: ready to trade.
    pub fn ready() -> H {
        Self::ready_with(config())
    }

    pub fn ready_with(cfg: CoreConfig) -> H {
        let mut h = H::new();
        h.inp(Input::Boot { binary: "test".into() });
        let out = h.inp(Input::Config(Box::new(cfg)));
        assert!(matches!(out.reply, Some(Reply::Config(Ok(_)))), "{:?}", out.reply);
        h.link_up_and_sync();
        assert_eq!(h.core.holds(None), vec![]);
        h
    }

    pub fn link_up_and_sync(&mut self) {
        self.session += 1;
        let session = self.session;
        self.venue(VenueInput::Link {
            up: true,
            session,
            reason: "test".into(),
        });
        self.sync();
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
        assert_eq!(self.core, self.shadow, "replayed state differs from live state");
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

    pub fn tick(&mut self) -> Output {
        self.inp(Input::Tick)
    }

    pub fn req(&self, strategy: &str, key: &str, side: Side, qty: &str, price: &str) -> NewOrderReq {
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
        }
    }

    /// `req` with the venue's reduce-only flag set.
    pub fn reduce_only_req(&self, strategy: &str, key: &str, side: Side, qty: &str, price: &str) -> NewOrderReq {
        NewOrderReq { reduce_only: true, ..self.req(strategy, key, side, qty, price) }
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
    pub fn accept(&mut self, strategy: &str, key: &str, side: Side, qty: &str, price: &str) -> OrderId {
        let r = self.req(strategy, key, side, qty, price);
        match self.new_order(r) {
            NewOrderReply::Accepted { order_id } => order_id,
            other => panic!("expected acceptance, got {other:?}"),
        }
    }

    pub fn reject(&mut self, strategy: &str, key: &str, side: Side, qty: &str, price: &str) -> Vec<RejectReason> {
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
                VenueCommand::SendCancel { order_id, attempt, .. } => Some((*order_id, *attempt)),
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
