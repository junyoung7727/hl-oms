//! Deterministic simulation of the whole order flow under faults.
//!
//! One `World` holds a real `oms_core::Core`, a model of the commit gate
//! (records become durable in batches; outputs are released only then), a
//! model gateway with an unreliable network, the simulated venue and a
//! strategy that submits, repeats and cancels requests. Everything is driven
//! by one seeded generator and one virtual clock, so a failing run is
//! reproduced exactly by its seed.
//!
//! What is real here: the core's decisions and state. What is modelled: the
//! journal (a vector with a durable prefix), the gateway, the network and the
//! venue. The real journal, threads and sockets are exercised by the
//! `oms-node` integration tests instead.

use crate::rng::Rng;
use crate::venue::*;
use oms_core::*;
use std::collections::{BTreeMap, BTreeSet};

/// Probabilities are per mille of one draw.
#[derive(Clone, Debug)]
pub struct Profile {
    pub name: &'static str,
    pub steps: u32,
    // actions per step
    pub p_new: u32,
    pub p_dup: u32,
    pub p_cancel: u32,
    pub p_fill: u32,
    pub p_venue_cancel: u32,
    pub p_tick: u32,
    pub p_crash: u32,
    pub p_link_drop: u32,
    pub p_external_fill: u32,
    pub p_operator: u32,
    // network faults
    pub p_not_sent: u32,
    pub p_req_lost: u32,
    pub p_req_slow: u32,
    pub p_resp_lost: u32,
    pub p_stream_dup: u32,
    pub p_stream_late: u32,
    pub p_query_lost: u32,
    // commit gate
    pub p_commit: u32,
    pub max_pending: usize,
    // venue behaviour
    pub report_cum: bool,
    pub fills_have_client_id: bool,
    /// Venue clock minus local clock, microseconds.
    pub venue_skew_us: i64,
    pub enforce_expiry: bool,
    // policy overrides
    pub absence_margin_us: u64,
    pub closed_retention_us: u64,
    pub dedup_grace_us: u64,
    /// Runs the core with a margin policy (C-MARGIN) and the venue with its
    /// margin model. Off in every profile that existed before the margin
    /// rules: such a run draws the same random numbers and writes the same
    /// journal as before.
    pub margin: bool,
    /// Share of new orders that carry the reduce-only flag (margin only).
    pub p_reduce_only: u32,
    // actions per step of the venue's margin model and of the operator
    pub p_price: u32,
    pub p_funding: u32,
    pub p_clear: u32,
    /// The venue leaves margin queries unanswered for a while.
    pub p_margin_outage: u32,
    /// The venue states no margin at all for the position for a while, so
    /// that the MM buffer trips.
    pub p_mm_squeeze: u32,
    /// The operator's account moves and base resets: a withdrawal or a
    /// deposit at the venue, and `rebase-day`.
    pub p_transfer: u32,
    pub p_rebase: u32,
    /// Virtual time at the start of the run.
    pub start_at: Ts,
    // the margin clock (C-MARGIN: poll, freshness limit, stale-cancel limit)
    pub margin_poll_us: u64,
    pub margin_max_age_us: u64,
    pub stale_cancel_us: u64,
    /// How long the venue leaves margin queries unanswered, in polls.
    pub outage_polls: (u64, u64),
    /// Share of UTC midnights over which the venue leaves margin queries
    /// unanswered: from some time in the hour before it to some time in the
    /// ten minutes after it. The first report of the new day is then the
    /// first to show what the account lost meanwhile (S12).
    pub p_night_outage: u32,
    /// Share of margin answers that the network delivers a second time.
    pub p_report_dup: u32,
    pub reconcile_every_us: u64,
    /// Share of steps before which nothing acts for a while: only the
    /// timers run (margin poll, reconcile, tick).
    pub p_idle: u32,
    /// Length of one such pause.
    pub idle_us: (u64, u64),
    /// The run goes on past `steps` until this much virtual time has passed.
    pub run_us: u64,
}

impl Profile {
    pub fn calm() -> Profile {
        Profile {
            name: "calm",
            steps: 1500,
            p_new: 250,
            p_dup: 0,
            p_cancel: 120,
            p_fill: 300,
            p_venue_cancel: 20,
            p_tick: 100,
            p_crash: 0,
            p_link_drop: 0,
            p_external_fill: 0,
            p_operator: 0,
            p_not_sent: 0,
            p_req_lost: 0,
            p_req_slow: 0,
            p_resp_lost: 0,
            p_stream_dup: 0,
            p_stream_late: 0,
            p_query_lost: 0,
            p_commit: 700,
            max_pending: 8,
            report_cum: true,
            fills_have_client_id: true,
            venue_skew_us: 0,
            enforce_expiry: true,
            absence_margin_us: 2_000_000,
            closed_retention_us: 3_600_000_000,
            dedup_grace_us: 60_000_000,
            margin: false,
            p_reduce_only: 0,
            p_price: 0,
            p_funding: 0,
            p_clear: 0,
            p_margin_outage: 0,
            p_mm_squeeze: 0,
            p_transfer: 0,
            p_rebase: 0,
            start_at: 1_700_000_000_000_000,
            // Fifty times faster than the contract's defaults (poll 5 s,
            // fresh 30 s, stale cancel 60 s), because a run of 1500 steps
            // lasts about 25 virtual seconds. The proportions are kept.
            margin_poll_us: 100_000,
            margin_max_age_us: 600_000,
            stale_cancel_us: 1_200_000,
            outage_polls: (5, 25),
            p_night_outage: 0,
            p_report_dup: 0,
            reconcile_every_us: 300_000,
            p_idle: 0,
            idle_us: (0, 0),
            run_us: 0,
        }
    }

    /// Lossy network, duplicated and late stream messages, repeated requests.
    pub fn lossy() -> Profile {
        Profile {
            name: "lossy",
            p_dup: 80,
            p_not_sent: 30,
            p_req_lost: 40,
            p_req_slow: 60,
            p_resp_lost: 80,
            p_stream_dup: 150,
            p_stream_late: 200,
            p_query_lost: 100,
            p_link_drop: 8,
            p_operator: 5,
            ..Profile::calm()
        }
    }

    /// Everything in `lossy` plus process crashes at arbitrary points.
    pub fn crashy() -> Profile {
        Profile {
            name: "crashy",
            p_crash: 12,
            p_commit: 500,
            ..Profile::lossy()
        }
    }

    /// The venue states no cumulative quantity and fills carry only the
    /// venue's order id, the weakest evidence the core must cope with.
    pub fn weak_evidence() -> Profile {
        Profile {
            name: "weak_evidence",
            report_cum: false,
            fills_have_client_id: false,
            ..Profile::crashy()
        }
    }

    /// Short retention, so idempotency keys are forgotten during the run and
    /// late duplicates must be refused as expired.
    pub fn purge() -> Profile {
        Profile {
            name: "purge",
            p_dup: 200,
            closed_retention_us: 1_000_000,
            dedup_grace_us: 500_000,
            ..Profile::crashy()
        }
    }

    /// Trades appear on the account that the OMS did not send.
    pub fn external() -> Profile {
        Profile {
            name: "external",
            p_external_fill: 6,
            ..Profile::lossy()
        }
    }

    /// Venue clock one second behind the local clock, inside the margin.
    pub fn skewed() -> Profile {
        Profile {
            name: "skewed",
            venue_skew_us: -1_000_000,
            ..Profile::crashy()
        }
    }

    /// `calm` with the margin rules on: the venue's account value follows
    /// fills, price moves, fees and funding, some orders are reduce-only,
    /// and an operator clears REDUCING now and then. No fault is injected.
    pub fn margin_calm() -> Profile {
        Profile {
            name: "margin_calm",
            margin: true,
            p_reduce_only: 150,
            p_price: 100,
            p_funding: 10,
            p_clear: 5,
            // Twelve seconds before a UTC midnight (day 19676), so that a
            // run of about 25 s crosses it.
            start_at: 19_676 * DAY_US - 12_000_000,
            ..Profile::calm()
        }
    }

    /// `margin_calm` with a lossy network, link drops, process restarts,
    /// margin queries that go unanswered for a while or are answered late,
    /// and a venue that at times states too little margin for the position.
    pub fn margin_lossy() -> Profile {
        let calm = Profile::margin_calm();
        Profile {
            name: "margin_lossy",
            margin: true,
            // Orders rest longer than in `lossy` (fewer fills and strategy
            // cancels), so that a stale cancel finds orders to cancel.
            p_new: 230,
            p_cancel: 40,
            p_fill: 150,
            p_tick: 150,
            // Fewer link drops and restarts than `crashy`: each one costs
            // the margin rules a fresh report, and a run that is blind most
            // of the time accepts too few plain orders to say anything.
            p_link_drop: 3,
            p_crash: 2,
            p_operator: 2,
            p_commit: 500,
            p_reduce_only: calm.p_reduce_only,
            p_price: calm.p_price,
            p_funding: calm.p_funding,
            p_clear: 12,
            p_margin_outage: 2,
            p_mm_squeeze: 1,
            start_at: calm.start_at,
            ..Profile::lossy()
        }
    }

    /// The margin rules over several UTC days (G8), on the contract's own
    /// clock: poll 5 s, fresh 30 s, stale cancel 60 s. The run starts one
    /// hour before a UTC midnight and lasts 50 hours, so it crosses three
    /// midnights. Activity comes in bursts with pauses of up to ten minutes
    /// in between, during which only the timers run. On top of the faults
    /// of `margin_lossy` (rarer per step, the run has about ten times the
    /// steps): margin queries unanswered for up to an hour, and over every
    /// second midnight; margin answers delivered twice; withdrawals and
    /// deposits; the operator's `rebase-day`.
    pub fn margin_days() -> Profile {
        let policy = MarginPolicy::default();
        Profile {
            name: "margin_days",
            steps: 0,
            run_us: 50 * 3_600_000_000,
            start_at: 19_676 * DAY_US - 3_600_000_000,
            margin_poll_us: 5_000_000,
            margin_max_age_us: policy.margin_max_age_us,
            stale_cancel_us: policy.stale_cancel_us,
            outage_polls: (12, 720),
            p_night_outage: 500,
            p_report_dup: 50,
            reconcile_every_us: 2_000_000,
            p_idle: 40,
            idle_us: (5_000_000, 600_000_000),
            p_link_drop: 2,
            p_crash: 1,
            p_clear: 4,
            p_margin_outage: 1,
            p_transfer: 3,
            p_rebase: 2,
            ..Profile::margin_lossy()
        }
    }

    /// The profiles that existed before the margin rules. None of them sets
    /// a margin policy.
    pub fn all() -> Vec<Profile> {
        vec![
            Profile::calm(),
            Profile::lossy(),
            Profile::crashy(),
            Profile::weak_evidence(),
            Profile::purge(),
            Profile::skewed(),
            Profile::external(),
        ]
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SimStats {
    pub steps: u32,
    pub inputs: u64,
    pub records: u64,
    pub commits: u64,
    pub crashes: u32,
    pub link_drops: u32,
    pub requests: u64,
    pub accepted: u64,
    pub rejected: u64,
    pub duplicates: u64,
    pub conflicts: u64,
    pub sends: u64,
    pub venue_orders: u64,
    pub venue_fills: u64,
    pub external_fills: u64,
    pub in_doubt_seen: u64,
    pub closed_not_placed: u64,
    pub parked_seen: u64,
    pub purged: u64,
    pub mismatches: u64,
    pub final_digest: u64,
    pub trace_hash: u64,
    pub sim_seconds: u64,
    // margin profiles
    /// Accepted plain orders that the `leverage-gate` check judged.
    pub leverage_checked: u64,
    /// Requests refused with `AccountLeverage`.
    pub leverage_rejects: u64,
    /// Times the account entered REDUCING.
    pub reducing_entries: u64,
    /// Plain orders that were open at an entry and had their cancel request.
    pub reducing_entry_cancels: u64,
    /// Plain requests refused with `Hold(Reducing)`.
    pub reducing_plain_rejects: u64,
    /// Reduce-only requests accepted while REDUCING.
    pub reducing_reduce_only_accepts: u64,
    /// Times the operator's clear ended REDUCING.
    pub reducing_clears: u64,
    /// Times the operator's clear was refused.
    pub clear_refusals: u64,
    /// Reports that rolled the day base to a new UTC day while REDUCING.
    pub reducing_day_rolls: u64,
    /// Reports recorded while REDUCING whose account value was back within
    /// the daily-loss limit of the day base.
    pub reducing_recoveries: u64,
    /// Restarts while the journal said REDUCING.
    pub reducing_restarts: u64,
    /// Plain requests refused with `MarginUnavailable`.
    pub margin_unavailable_rejects: u64,
    /// Margin reports the core did not record (late, or of an old session).
    pub reports_dropped: u64,
    /// Ticks at which the stale-cancel condition held and a plain order
    /// was open.
    pub stale_cancel_ticks: u64,
    /// Cancel requests made by the rule `MarginStale`.
    pub stale_rule_cancels: u64,
    /// Times the account entered REDUCING by the MM buffer.
    pub mm_entries: u64,
    /// Durable `day_base_set` records by a report that replaced the base of
    /// an earlier UTC day: the midnights the day base crossed.
    pub day_rolls: u64,
    /// Reports that rolled the day base and whose account value was more
    /// than the daily-loss limit below the old base, the account not being
    /// REDUCING before: a loss first seen across a midnight.
    pub roll_losses: u64,
    /// Margin queries the world made (one per poll of a link session).
    pub margin_polls: u64,
    /// Durable journal records whose input is a margin report.
    pub margin_records: u64,
    /// Polls (link session and query number) that those records answer.
    pub margin_polls_recorded: u64,
    /// Margin answers that the network delivered a second time.
    pub report_dups: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Violation {
    pub property: &'static str,
    pub detail: String,
    pub at_step: u32,
    pub seed: u64,
    pub profile: &'static str,
}

impl std::fmt::Display for Violation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "[{}] seed {} profile {} step {}: {}",
            self.property, self.seed, self.profile, self.at_step, self.detail
        )
    }
}

#[derive(Clone, Debug)]
enum Query {
    Status(OrderId),
    Fills(Ts),
    OpenOrders,
    Positions,
    Margin { seq: u64 },
    VenueSetting,
    AccountMode,
}

#[derive(Clone, Debug)]
enum Net {
    PlaceArrive { req: u64, epoch: u32, place: PlaceReq, order_id: OrderId, immediate: Qty },
    CancelArrive { req: u64, epoch: u32, order_id: OrderId, attempt: u32, client: VenueClientId },
    Resp { req: u64, epoch: u32, input: VenueInput },
    Timeout { req: u64, epoch: u32 },
    Stream { session: u64, epoch: u32, ev: StreamEvent },
    LinkUp { epoch: u32 },
    Reconcile { epoch: u32, n: u32 },
    QueryArrive { epoch: u32, session: u64, queried_at: Ts, q: Query },
    QueryResp { epoch: u32, inputs: Vec<VenueInput> },
    /// The margin poll of one link session (margin profiles only).
    MarginPoll { epoch: u32, session: u64 },
    /// A reference price on the stream (margin profiles only).
    RefPrice { epoch: u32, session: u64, px: Px, ts: Ts },
}

impl Net {
    /// Traffic that never ends: the reconcile timer and, in a margin
    /// profile, the margin poll with its query, its answer and the
    /// reference price.
    fn is_periodic(&self) -> bool {
        match self {
            Net::Reconcile { .. } | Net::MarginPoll { .. } | Net::RefPrice { .. } => true,
            Net::QueryArrive { q, .. } => matches!(q, Query::Margin { .. }),
            Net::QueryResp { inputs, .. } => inputs.iter().all(|i| matches!(i, VenueInput::MarginReport(_))),
            _ => false,
        }
    }
}

#[derive(Clone, Debug)]
enum Inflight {
    Place(OrderId),
    Cancel(OrderId, u32),
}

/// What the world notes before an input, to judge the core's answer to it.
struct MarginBefore {
    exposure: Option<i128>,
    reducing: bool,
    day_base: Option<Amount>,
}

pub const INSTANCE: u64 = 0x51_4d_55_4c;
const GW_TIMEOUT_US: u64 = 2_000_000;
/// What the account holds at the start of a margin run: the owner's 100.
const START_CAPITAL: i64 = 100 * oms_core::num::SCALE;

pub struct World {
    pub p: Profile,
    pub seed: u64,
    rng: Rng,
    pub now: Ts,
    step: u32,
    pub core: Core,
    pub durable: Vec<Record>,
    pending: Vec<(Option<Record>, Output)>,
    pub venue: SimVenue,
    queue: BTreeMap<(Ts, u64), Net>,
    qseq: u64,
    epoch: u32,
    link_up: bool,
    session: u64,
    inflight: BTreeMap<u64, Inflight>,
    next_req: u64,
    sent: Vec<NewOrderReq>,
    key_counter: u64,
    calm: bool,
    released_sends: BTreeMap<OrderId, u32>,
    released_keys: BTreeSet<(StrategyId, ClientOrderId)>,
    accepted_keys: BTreeMap<(StrategyId, ClientOrderId), OrderId>,
    external_seen: bool,
    /// Number of the last margin query. It rises for the life of the
    /// process, as the node's does.
    margin_seq: u64,
    /// The venue answers no margin query before this time.
    margin_out_until: Ts,
    /// The venue answers no margin query from the first time to the second
    /// (an outage over a midnight), and the midnight that was drawn for.
    night_out: (Ts, Ts),
    night_drawn_for: Ts,
    /// The venue states no margin for the position before this time.
    mm_squeeze_until: Ts,
    /// When the current link session came up, by the world's own clock.
    link_up_at: Option<Ts>,
    /// The durable journal holds a `day_base_set`.
    base_in_journal: bool,
    pub stats: SimStats,
}

fn inst() -> InstrumentId {
    InstrumentId::new("X").expect("valid id")
}

fn strat(name: &str) -> StrategyId {
    StrategyId::new(name).expect("valid id")
}

const QTY_STEP: i64 = 1_000_000; // 0.01
const PX_TICK: i64 = 100_000_000; // 1

pub fn sim_config(p: &Profile) -> CoreConfig {
    let mut instruments = BTreeMap::new();
    instruments.insert(inst(), InstrumentSpec {
        qty_step: Qty(QTY_STEP),
        px_tick: Px(PX_TICK),
        px_max_sig_figs: Some(5),
        min_notional: Amount(0),
        max_order_qty: Qty(200 * QTY_STEP),
        max_order_notional: Amount(i64::MAX / 4),
        price_band_bps: None,
        account_pos_min: SQty(-500 * QTY_STEP),
        account_pos_max: SQty(500 * QTY_STEP),
    });
    let mut strategies = BTreeMap::new();
    for name in ["a", "b"] {
        let mut limits = BTreeMap::new();
        limits.insert(inst(), PosLimit { min: SQty(-400 * QTY_STEP), max: SQty(400 * QTY_STEP) });
        strategies.insert(strat(name), StrategySpec { enabled: true, max_open_orders: 40, limits });
    }
    let policy = Policy {
        max_open_orders: 60,
        max_inflight_sends: 16,
        max_expiry_horizon_us: 10_000_000,
        absence_margin_us: p.absence_margin_us,
        venue_enforces_expiry: true,
        cancel_retry_backoff_us: 500_000,
        cancel_max_attempts: 50,
        closed_retention_us: p.closed_retention_us,
        dedup_grace_us: p.dedup_grace_us,
        purge_interval_us: 1_000_000,
        unmatched_fill_wait_us: 40_000_000,
        fill_dedup_retention_us: p.closed_retention_us,
        ..Policy::default()
    };
    let mut policy = policy;
    if p.margin {
        policy.ref_price_max_age_us = p.margin_max_age_us;
        policy.margin = Some(MarginPolicy {
            margin_max_age_us: p.margin_max_age_us,
            stale_cancel_us: p.stale_cancel_us,
            account_modes_allowed: vec!["standard".into()],
            ..MarginPolicy::default()
        });
    }
    CoreConfig { version: 1, instruments, strategies, policy }
}

fn fnv(mut h: u64, bytes: &[u8]) -> u64 {
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

impl World {
    pub fn new(seed: u64, p: Profile) -> World {
        let venue = SimVenue::new(p.enforce_expiry);
        let mut w = World {
            seed,
            rng: Rng::new(seed),
            now: p.start_at,
            step: 0,
            core: Core::new(INSTANCE),
            durable: Vec::new(),
            pending: Vec::new(),
            venue,
            queue: BTreeMap::new(),
            qseq: 0,
            epoch: 0,
            link_up: false,
            session: 0,
            inflight: BTreeMap::new(),
            next_req: 1,
            sent: Vec::new(),
            key_counter: 0,
            calm: false,
            released_sends: BTreeMap::new(),
            released_keys: BTreeSet::new(),
            accepted_keys: BTreeMap::new(),
            external_seen: false,
            margin_seq: 0,
            margin_out_until: 0,
            night_out: (0, 0),
            night_drawn_for: 0,
            mm_squeeze_until: 0,
            link_up_at: None,
            base_in_journal: false,
            stats: SimStats { trace_hash: 0xcbf2_9ce4_8422_2325, ..SimStats::default() },
            p,
        };
        w.stats.steps = w.p.steps;
        if w.p.margin {
            w.venue.deposit(Amount(START_CAPITAL));
            w.venue.set_mark(&inst(), Px(100 * PX_TICK));
        }
        w
    }

    fn violation(&self, property: &'static str, detail: String) -> Violation {
        Violation { property, detail, at_step: self.step, seed: self.seed, profile: self.p.name }
    }

    fn venue_now(&self) -> Ts {
        self.now.saturating_add_signed(self.p.venue_skew_us)
    }

    fn schedule(&mut self, delay_us: u64, ev: Net) {
        self.qseq += 1;
        self.queue.insert((self.now + delay_us, self.qseq), ev);
    }

    fn chance(&mut self, per_mille: u32) -> bool {
        !self.calm && self.rng.chance(per_mille)
    }

    fn net_delay(&mut self) -> u64 {
        self.rng.range(2_000, 40_000)
    }

    // ---- core, commit gate ------------------------------------------------

    fn feed(&mut self, input: Input) -> Result<(), Violation> {
        self.stats.inputs += 1;
        let before = self.p.margin.then(|| self.margin_before());
        let out = self
            .core
            .handle(self.now, &input)
            .map_err(|e| self.violation("core-fault", format!("{e} on {input:?}")))?;
        self.core
            .check_invariants()
            .map_err(|e| self.violation("structural-invariant", format!("{e} after {input:?}")))?;
        if let Some(before) = before {
            self.check_margin_rules(&input, &out, &before)?;
        }
        let rec = (!out.events.is_empty()).then(|| {
            let pending_records = self.pending.iter().filter(|(r, _)| r.is_some()).count();
            Record {
                seq: (self.durable.len() + pending_records) as u64 + 1,
                at: self.now,
                input: input.clone(),
                events: out.events.clone(),
            }
        });
        self.pending.push((rec, out));
        if self.rng.chance(self.p.p_commit) || self.pending.len() >= self.p.max_pending {
            self.commit()?;
        }
        Ok(())
    }

    /// Makes every pending record durable, then releases the outputs that
    /// were waiting for it. This is the only place a command reaches the
    /// gateway or a reply reaches the strategy.
    pub fn commit(&mut self) -> Result<(), Violation> {
        if self.pending.is_empty() {
            return Ok(());
        }
        self.stats.commits += 1;
        let batch = std::mem::take(&mut self.pending);
        let mut outputs = Vec::with_capacity(batch.len());
        for (rec, out) in batch {
            if let Some(rec) = rec {
                let bytes = serde_json::to_vec(&rec).unwrap_or_default();
                self.stats.trace_hash = fnv(self.stats.trace_hash, &bytes);
                self.stats.records += 1;
                self.stats.margin_records += u64::from(matches!(rec.input, Input::Venue(VenueInput::MarginReport(_))));
                self.note_events(&rec.events);
                self.durable.push(rec);
            }
            outputs.push(out);
        }
        for out in outputs {
            if let Some(reply) = out.reply {
                self.on_reply(reply)?;
            }
            for cmd in out.commands {
                self.gateway_send(cmd)?;
            }
        }
        Ok(())
    }

    fn note_events(&mut self, events: &[Event]) {
        for e in events {
            match e {
                Event::OrderInDoubt { .. } => self.stats.in_doubt_seen += 1,
                Event::OrderClosed { reason: CloseReason::NotPlaced { .. }, .. } => self.stats.closed_not_placed += 1,
                Event::FillParked { .. } => self.stats.parked_seen += 1,
                Event::Purged { order_ids, .. } => self.stats.purged += order_ids.len() as u64,
                Event::MismatchRaised { .. } => self.stats.mismatches += 1,
                Event::DayBaseSet { set_by, .. } => {
                    let by_report = matches!(set_by, BaseSetBy::Report);
                    self.stats.day_rolls += u64::from(by_report && self.base_in_journal);
                    self.base_in_journal = true;
                }
                _ => {}
            }
        }
    }

    fn on_reply(&mut self, reply: Reply) -> Result<(), Violation> {
        if let Reply::NewOrder(r) = reply {
            match r {
                NewOrderReply::Accepted { order_id } => {
                    self.stats.accepted += 1;
                    let o = self
                        .core
                        .order(order_id)
                        .ok_or_else(|| self.violation("reply", format!("accepted order {order_id} is unknown")))?;
                    let key = (o.strategy.clone(), o.client_order_id.clone());
                    if let Some(prev) = self.accepted_keys.insert(key.clone(), order_id) {
                        // A key may be accepted again only after the earlier
                        // order was purged, which requires its request to have
                        // expired; and then the request is refused as expired.
                        return Err(self.violation(
                            "one-order-per-key",
                            format!("key {key:?} accepted as order {prev} and again as {order_id}"),
                        ));
                    }
                }
                NewOrderReply::Rejected { .. } => self.stats.rejected += 1,
                NewOrderReply::Duplicate { .. } => self.stats.duplicates += 1,
                NewOrderReply::Conflict { .. } => self.stats.conflicts += 1,
            }
        }
        Ok(())
    }

    // ---- gateway ----------------------------------------------------------

    fn gateway_send(&mut self, cmd: VenueCommand) -> Result<(), Violation> {
        match cmd {
            VenueCommand::SendOrder { order_id, instance_id, instrument, side, qty, px, tif, reduce_only, expires_at } => {
                self.stats.sends += 1;
                let n = {
                    let n = self.released_sends.entry(order_id).or_default();
                    *n += 1;
                    *n
                };
                if n > 1 {
                    return Err(self.violation(
                        "single-send",
                        format!("order {order_id} released to the gateway {n} times"),
                    ));
                }
                if let Some(o) = self.core.order(order_id) {
                    let key = (o.strategy.clone(), o.client_order_id.clone());
                    if !self.released_keys.insert(key.clone()) {
                        return Err(self.violation("one-send-per-key", format!("key {key:?} sent twice")));
                    }
                }
                let client = VenueClientId { instance_id, order_id };
                if !self.link_up || self.chance(self.p.p_not_sent) {
                    let d = self.net_delay();
                    let req = self.new_req(Inflight::Place(order_id));
                    self.schedule(d, Net::Resp {
                        req,
                        epoch: self.epoch,
                        input: VenueInput::SendResult {
                            order_id,
                            outcome: SendOutcome::NotSent { reason: "connection could not be established".into() },
                            timing: Default::default(),
                        },
                    });
                    return Ok(());
                }
                let req = self.new_req(Inflight::Place(order_id));
                self.schedule(GW_TIMEOUT_US, Net::Timeout { req, epoch: self.epoch });
                if self.chance(self.p.p_req_lost) {
                    return Ok(());
                }
                let d = if self.chance(self.p.p_req_slow) { self.rng.range(500_000, 9_000_000) } else { self.net_delay() };
                let immediate = match tif {
                    Tif::Ioc => Qty(self.rng.below(qty.0 as u64 / QTY_STEP as u64 + 1) as i64 * QTY_STEP),
                    Tif::Alo => {
                        if self.rng.chance(150) { Qty(QTY_STEP) } else { Qty::ZERO }
                    }
                    Tif::Gtc => {
                        if self.rng.chance(200) {
                            Qty(self.rng.below(qty.0 as u64 / QTY_STEP as u64 + 1) as i64 * QTY_STEP)
                        } else {
                            Qty::ZERO
                        }
                    }
                };
                let place = PlaceReq { client, instrument, side, qty, px, tif, expires_at, reduce_only };
                self.schedule(d, Net::PlaceArrive { req, epoch: self.epoch, place, order_id, immediate });
            }
            VenueCommand::SendCancel { order_id, instance_id, attempt, .. } => {
                let client = VenueClientId { instance_id, order_id };
                let req = self.new_req(Inflight::Cancel(order_id, attempt));
                if !self.link_up || self.chance(self.p.p_not_sent) {
                    let d = self.net_delay();
                    self.schedule(d, Net::Resp {
                        req,
                        epoch: self.epoch,
                        input: VenueInput::CancelResult {
                            order_id,
                            attempt,
                            outcome: CancelOutcome::NotSent { reason: "connection could not be established".into() },
                            timing: Default::default(),
                        },
                    });
                    return Ok(());
                }
                self.schedule(GW_TIMEOUT_US, Net::Timeout { req, epoch: self.epoch });
                if self.chance(self.p.p_req_lost) {
                    return Ok(());
                }
                let d = self.net_delay();
                self.schedule(d, Net::CancelArrive { req, epoch: self.epoch, order_id, attempt, client });
            }
        }
        Ok(())
    }

    fn new_req(&mut self, kind: Inflight) -> u64 {
        let req = self.next_req;
        self.next_req += 1;
        self.inflight.insert(req, kind);
        req
    }

    fn pump_stream(&mut self) {
        let events = self.venue.take_stream();
        if !self.link_up {
            return; // nobody is subscribed; the messages are gone
        }
        for ev in events {
            let mut d = self.net_delay();
            if self.chance(self.p.p_stream_late) {
                d += self.rng.range(50_000, 1_500_000);
            }
            self.schedule(d, Net::Stream { session: self.session, epoch: self.epoch, ev: ev.clone() });
            if self.chance(self.p.p_stream_dup) {
                let d2 = d + self.rng.range(1_000, 800_000);
                self.schedule(d2, Net::Stream { session: self.session, epoch: self.epoch, ev });
            }
        }
    }

    fn stream_input(&self, session: u64, ev: &StreamEvent) -> VenueInput {
        match ev {
            StreamEvent::Order { oid, client, status, filled, ts } => VenueInput::OrderStatus(OrderStatusIn {
                session,
                order_id: client.filter(|c| c.instance_id == INSTANCE).map(|c| c.order_id),
                venue_oid: Some(oid_string(*oid)),
                status: status_in(status, *filled, *ts, self.p.report_cum),
                source: EvidenceSource::Stream,
            }),
            StreamEvent::Fill(f) => VenueInput::Fill(fill_in(f, INSTANCE, self.p.fills_have_client_id)),
        }
    }

    fn deliver(&mut self, ev: Net) -> Result<(), Violation> {
        match ev {
            Net::PlaceArrive { req, epoch, place, order_id, immediate } => {
                // The request is on the wire: it reaches the venue even if the
                // gateway that sent it has since died.
                let resp = self.venue.place(self.venue_now(), &place, immediate);
                if matches!(resp, PlaceResp::Placed { .. }) {
                    self.stats.venue_orders += 1;
                }
                self.pump_stream();
                if self.chance(self.p.p_resp_lost) {
                    return Ok(());
                }
                let outcome = match resp {
                    PlaceResp::Placed { oid, status, filled, ts } => SendOutcome::Placed {
                        venue_oid: oid_string(oid),
                        status: status_in(&status, filled, ts, self.p.report_cum),
                    },
                    PlaceResp::Rejected { reason } => SendOutcome::Rejected { reason },
                };
                let d = self.net_delay();
                self.schedule(d, Net::Resp { req, epoch, input: VenueInput::SendResult { order_id, outcome, timing: Default::default() } });
            }
            Net::CancelArrive { req, epoch, order_id, attempt, client } => {
                let r = self.venue.cancel(self.venue_now(), client);
                self.pump_stream();
                if self.chance(self.p.p_resp_lost) {
                    return Ok(());
                }
                let outcome = match r {
                    Ok(()) => CancelOutcome::Confirmed,
                    Err(reason) => CancelOutcome::Rejected { reason },
                };
                let d = self.net_delay();
                self.schedule(d, Net::Resp { req, epoch, input: VenueInput::CancelResult { order_id, attempt, outcome, timing: Default::default() } });
            }
            Net::Resp { req, epoch, input } => {
                if epoch == self.epoch && self.inflight.remove(&req).is_some() {
                    self.feed(Input::Venue(input))?;
                }
            }
            Net::Timeout { req, epoch } => {
                if epoch == self.epoch {
                    if let Some(kind) = self.inflight.remove(&req) {
                        self.feed(Input::Venue(unknown_result(kind, "no response within the gateway timeout")))?;
                    }
                }
            }
            Net::Stream { session, epoch, ev } => {
                if epoch == self.epoch && self.link_up && session == self.session {
                    let input = self.stream_input(session, &ev);
                    self.feed(Input::Venue(input))?;
                }
            }
            Net::LinkUp { epoch } => {
                if epoch == self.epoch && !self.link_up {
                    self.session += 1;
                    self.link_up = true;
                    self.link_up_at = Some(self.now);
                    let session = self.session;
                    self.feed(Input::Venue(VenueInput::Link { up: true, session, reason: "connected".into() }))?;
                    self.schedule(1_000, Net::Reconcile { epoch, n: 0 });
                    if self.p.margin {
                        self.schedule(1_000, Net::MarginPoll { epoch, session });
                    }
                }
            }
            Net::MarginPoll { epoch, session } => {
                // One poll chain per link session; it ends with the session.
                if epoch == self.epoch && self.link_up && session == self.session {
                    self.issue_margin_queries();
                    self.schedule(self.p.margin_poll_us, Net::MarginPoll { epoch, session });
                }
            }
            Net::RefPrice { epoch, session, px, ts } => {
                if epoch == self.epoch && self.link_up && session == self.session {
                    self.feed(Input::Venue(VenueInput::RefPrice { instrument: inst(), px, venue_ts: ts }))?;
                }
            }
            Net::Reconcile { epoch, n } => {
                if epoch == self.epoch {
                    if self.link_up {
                        // The reconciler reads published state, which is the
                        // durable state.
                        self.commit()?;
                        self.issue_queries(n);
                    }
                    self.schedule(self.p.reconcile_every_us, Net::Reconcile { epoch, n: n + 1 });
                }
            }
            Net::QueryArrive { epoch, session, queried_at, q } => {
                if epoch != self.epoch || self.chance(self.p.p_query_lost) {
                    return Ok(());
                }
                let is_margin = matches!(q, Query::Margin { .. });
                let late = self.p.margin && is_margin && self.chance(self.p.p_req_slow);
                let inputs = self.answer(session, queried_at, q);
                let mut d = self.net_delay();
                if late {
                    // An answer that is old when it arrives, and that a
                    // later query's answer can overtake.
                    d += self.rng.range(2 * self.p.margin_poll_us, 15 * self.p.margin_poll_us);
                }
                if is_margin && !inputs.is_empty() && self.p.p_report_dup > 0 && self.chance(self.p.p_report_dup) {
                    // The same answer once more, a little later.
                    self.stats.report_dups += 1;
                    let again = d + self.rng.range(1_000, self.p.margin_poll_us);
                    self.schedule(again, Net::QueryResp { epoch, inputs: inputs.clone() });
                }
                self.schedule(d, Net::QueryResp { epoch, inputs });
            }
            Net::QueryResp { epoch, inputs } => {
                if epoch == self.epoch {
                    for i in inputs {
                        self.feed(Input::Venue(i))?;
                    }
                }
            }
        }
        Ok(())
    }

    fn issue_queries(&mut self, n: u32) {
        let mut queries: Vec<Query> = self
            .core
            .needs()
            .into_iter()
            .map(|need| match need {
                Need::OrderStatus { order_id, .. } => Query::Status(order_id),
                Need::Fills { since } => Query::Fills(since),
                Need::OpenOrders => Query::OpenOrders,
                Need::Positions => Query::Positions,
            })
            .collect();
        if n % 10 == 9 {
            queries.push(Query::OpenOrders);
            queries.push(Query::Positions);
            queries.push(Query::Fills(self.core.sync.fills_up_to.max(self.core.fill_floor)));
        }
        for q in queries {
            let d = self.net_delay();
            self.schedule(d, Net::QueryArrive { epoch: self.epoch, session: self.session, queried_at: self.now, q });
        }
    }

    /// What the node's reconciler does for the margin rules: the margin
    /// query on its timer, and the venue setting and the account mode while
    /// the core does not know them in this link session. The venue also
    /// publishes its mark as the reference price.
    fn issue_margin_queries(&mut self) {
        self.margin_seq += 1;
        self.stats.margin_polls += 1;
        let mut queries = vec![Query::Margin { seq: self.margin_seq }];
        if !self.core.margin.venue_settings.contains_key(&inst()) {
            queries.push(Query::VenueSetting);
        }
        if self.core.margin.account_mode.is_none() {
            queries.push(Query::AccountMode);
        }
        for q in queries {
            let d = self.net_delay();
            self.schedule(d, Net::QueryArrive { epoch: self.epoch, session: self.session, queried_at: self.now, q });
        }
        let d = self.net_delay();
        let (px, ts) = (self.venue.mark(&inst()), self.venue_now());
        self.schedule(d, Net::RefPrice { epoch: self.epoch, session: self.session, px, ts });
    }

    fn answer(&self, session: u64, queried_at: Ts, q: Query) -> Vec<VenueInput> {
        match q {
            Query::Margin { .. } if self.now < self.margin_out_until => Vec::new(),
            Query::Margin { .. } if self.night_out.0 <= self.now && self.now < self.night_out.1 => Vec::new(),
            Query::Margin { seq } => vec![VenueInput::MarginReport(MarginReport {
                session,
                seq,
                queried_at,
                account_value: self.venue.account_value(),
                positions: self.venue.margin_positions(),
            })],
            Query::VenueSetting => {
                let (mode, leverage) = self.venue.setting(&inst());
                vec![VenueInput::VenueSetting { session, instrument: inst(), isolated: mode == LeverageType::Isolated, leverage }]
            }
            Query::AccountMode => vec![VenueInput::AccountMode { session, mode: self.venue.account_mode.clone() }],
            Query::Status(order_id) => {
                let client = VenueClientId { instance_id: INSTANCE, order_id };
                match self.venue.status(client) {
                    Some(o) => vec![VenueInput::OrderStatus(OrderStatusIn {
                        session,
                        order_id: Some(order_id),
                        venue_oid: Some(o.venue_oid()),
                        status: status_in(&o.status, o.filled, o.updated_at, self.p.report_cum),
                        source: EvidenceSource::Query,
                    })],
                    None => vec![VenueInput::StatusNotFound { session, order_id, queried_at }],
                }
            }
            Query::Fills(since) => {
                let mut v: Vec<VenueInput> = self
                    .venue
                    .fills_since(since)
                    .iter()
                    .map(|f| VenueInput::Fill(fill_in(f, INSTANCE, self.p.fills_have_client_id)))
                    .collect();
                // A fill can still happen at this very venue time after the
                // answer was computed, so the watermark stops just before it.
                v.push(VenueInput::FillsSynced { session, up_to: self.venue_now().saturating_sub(1) });
                v
            }
            Query::OpenOrders => vec![VenueInput::OpenOrders {
                session,
                queried_at,
                orders: self
                    .venue
                    .open_orders()
                    .iter()
                    .map(|o| VenueOpenOrder {
                        venue_oid: o.venue_oid(),
                        order_id: o.client.filter(|c| c.instance_id == INSTANCE).map(|c| c.order_id),
                        instrument: o.instrument.clone(),
                        side: o.side,
                        remaining: Some(o.remaining()),
                    })
                    .collect(),
            }],
            Query::Positions => vec![VenueInput::Positions {
                session,
                queried_at,
                positions: self
                    .venue
                    .positions
                    .iter()
                    .map(|(i, q)| VenuePosition { instrument: i.clone(), qty: SQty(*q) })
                    .collect(),
            }],
        }
    }

    // ---- margin rules: what the world checks after every input ------------

    fn margin_before(&self) -> MarginBefore {
        MarginBefore {
            exposure: self.oracle_exposure(),
            reducing: self.core.margin.reducing.is_some(),
            day_base: self.core.margin.day_base.as_ref().map(|b| b.value),
        }
    }

    /// Whether the durable journal says the account is REDUCING. Fails when
    /// a `reducing_cleared` stands under an input that is not the
    /// operator's clear (S14).
    fn journal_reducing(&self) -> Result<bool, Violation> {
        let mut reducing = false;
        for rec in &self.durable {
            for e in &rec.events {
                match e {
                    Event::ReducingEntered { .. } => reducing = true,
                    Event::ReducingCleared { .. } => {
                        if !matches!(rec.input, Input::Operator(OperatorCmd::ClearReduce { .. })) {
                            return Err(self.violation(
                                "reducing-release",
                                format!("record {} clears REDUCING and its input is not the operator's clear", rec.seq),
                            ));
                        }
                        reducing = false;
                    }
                    _ => {}
                }
            }
        }
        Ok(reducing)
    }

    /// S14: REDUCING ends only by the operator's clear.
    fn check_release(&mut self, input: &Input, out: &Output, before: &MarginBefore) -> Result<(), Violation> {
        let is_clear = matches!(input, Input::Operator(OperatorCmd::ClearReduce { .. }));
        if is_clear && matches!(out.reply, Some(Reply::Operator(Err(_)))) {
            self.stats.clear_refusals += 1;
        }
        if !before.reducing {
            return Ok(());
        }
        if self.core.margin.reducing.is_none() {
            if !is_clear || !out.events.iter().any(|e| matches!(e, Event::ReducingCleared { .. })) {
                return Err(self.violation("reducing-release", format!("REDUCING ended by {input:?} with events {:?}", out.events)));
            }
            self.stats.reducing_clears += 1;
            return Ok(());
        }
        // Still REDUCING. Note the things that must not end it.
        for e in &out.events {
            match e {
                Event::DayBaseSet { set_by: BaseSetBy::Report, .. } => self.stats.reducing_day_rolls += 1,
                Event::MarginReported { .. } => {
                    let value = self.core.margin.report.as_ref().map(|r| i128::from(r.account_value.0));
                    if let (Some(value), Some(base)) = (value, before.day_base.map(|b| i128::from(b.0))) {
                        // Not more than 5 % below the base any more.
                        if (base - value) * 10_000 <= 500 * base {
                            self.stats.reducing_recoveries += 1;
                        }
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Account exposure worked out by the world from the core's books: the
    /// position and every open order's reserved quantity, an order valued
    /// at the higher of its limit and the reference price. Units of 1e-16
    /// quote. `None` while there is no reference price. The world has one
    /// instrument.
    fn oracle_exposure(&self) -> Option<i128> {
        let reference = i128::from(self.core.ref_px.get(&inst())?.px.0);
        let position = i128::from(self.core.account_book(&inst()).pos.0) * reference;
        let (mut buys, mut sells) = (0i128, 0i128);
        for o in self.core.open.iter().filter_map(|id| self.core.order(*id)) {
            let value = i128::from(o.reserved.0) * i128::from(o.px.0).max(reference);
            match o.side {
                Side::Buy => buys += value,
                Side::Sell => sells += value,
            }
        }
        Some((position + buys).max(sells - position).max(0))
    }

    /// S23: once the link has been up, without a recorded margin report,
    /// for longer than the stale-cancel limit, a tick leaves no open plain
    /// order without a cancel request. The link-up time is the world's own.
    fn check_stale_cancel(&mut self) -> Result<(), Violation> {
        let Some(up_at) = self.link_up_at.filter(|_| self.link_up) else { return Ok(()) };
        let reported_at = self.core.margin.report.as_ref().map_or(0, |r| r.queried_at);
        if self.now.saturating_sub(reported_at.max(up_at)) <= self.p.stale_cancel_us {
            return Ok(());
        }
        let mut open_plain = false;
        for o in self.core.open.iter().filter_map(|id| self.core.order(*id)).filter(|o| !o.reduce_only) {
            if o.cancel.is_none() {
                return Err(self.violation(
                    "stale-cancel",
                    format!("plain order {} has no cancel request after a tick past the stale-cancel limit", o.id),
                ));
            }
            open_plain = true;
        }
        self.stats.stale_cancel_ticks += u64::from(open_plain);
        Ok(())
    }

    /// S12, for a report the core recorded: it is judged against the day
    /// base that stood before it. A report more than 5 % below that base
    /// leaves the account REDUCING, also when the same report starts a new
    /// UTC day.
    fn check_day_base(&mut self, report: &MarginReport, out: &Output, before: &MarginBefore) -> Result<(), Violation> {
        let value = i128::from(report.account_value.0);
        let rolled = out.events.iter().any(|e| matches!(e, Event::DayBaseSet { set_by: BaseSetBy::Report, .. }));
        let Some(base) = before.day_base.map(|b| i128::from(b.0)) else { return Ok(()) };
        let lost_too_much = (base - value) * 10_000 > 500 * base;
        if lost_too_much && self.core.margin.reducing.is_none() {
            return Err(self.violation(
                "daily-loss",
                format!(
                    "a report of account value {} is more than 5 % below the day base {} and the account is not REDUCING (the report set a new base: {rolled})",
                    report.account_value,
                    Amount(base as i64)
                ),
            ));
        }
        self.stats.roll_losses += u64::from(lost_too_much && rolled && !before.reducing);
        Ok(())
    }

    fn check_margin_rules(&mut self, input: &Input, out: &Output, before: &MarginBefore) -> Result<(), Violation> {
        self.check_release(input, out, before)?;
        let recorded = out.events.iter().find_map(|e| match e {
            Event::MarginReported { report } => Some(report),
            _ => None,
        });
        if let Some(report) = recorded {
            self.check_day_base(report, out, before)?;
        }
        let is_report = matches!(input, Input::Venue(VenueInput::MarginReport(_)));
        if is_report && out.events.is_empty() {
            self.stats.reports_dropped += 1;
        }
        for e in &out.events {
            match e {
                Event::CancelRequested { by: Actor::Rule(RuleId::MarginStale), .. } => self.stats.stale_rule_cancels += 1,
                Event::ReducingEntered { rule, .. } => {
                    // Only a margin report latches; staleness never does.
                    if !is_report {
                        return Err(self.violation("reducing-entry", format!("REDUCING entered by {input:?}")));
                    }
                    self.stats.mm_entries += u64::from(*rule == RuleId::MmBuffer);
                }
                _ => {}
            }
        }
        if matches!(input, Input::Tick) {
            self.check_stale_cancel()?;
        }
        if out.events.iter().any(|e| matches!(e, Event::ReducingEntered { .. })) {
            // S8: every plain order that is open at the entry has a cancel
            // request when the input that entered is done.
            self.stats.reducing_entries += 1;
            for o in self.core.open.iter().filter_map(|id| self.core.order(*id)) {
                if o.reduce_only {
                    continue;
                }
                if o.cancel.is_none() {
                    return Err(self.violation(
                        "reducing-cancel",
                        format!("plain order {} is open at the entry into REDUCING and has no cancel request", o.id),
                    ));
                }
                self.stats.reducing_entry_cancels += 1;
            }
        }
        let Input::NewOrder(req) = input else { return Ok(()) };
        match &out.reply {
            Some(Reply::NewOrder(NewOrderReply::Rejected { reasons })) => {
                if reasons.contains(&RejectReason::AccountLeverage) {
                    self.stats.leverage_rejects += 1;
                }
                if !req.reduce_only && reasons.contains(&RejectReason::Hold(HoldReason::Reducing)) {
                    self.stats.reducing_plain_rejects += 1;
                }
                if !req.reduce_only && reasons.contains(&RejectReason::MarginUnavailable) {
                    self.stats.margin_unavailable_rejects += 1;
                }
            }
            Some(Reply::NewOrder(NewOrderReply::Accepted { order_id })) if before.reducing && !req.reduce_only => {
                // S8: while REDUCING no plain order is accepted.
                return Err(self.violation("reducing-accept", format!("plain order {order_id} accepted while REDUCING")));
            }
            Some(Reply::NewOrder(NewOrderReply::Accepted { .. })) if req.reduce_only => {
                if before.reducing {
                    self.stats.reducing_reduce_only_accepts += 1;
                }
            }
            Some(Reply::NewOrder(NewOrderReply::Accepted { order_id })) => {
                // S5: at acceptance the exposure is within twice the
                // account value, or the order did not raise it.
                self.stats.leverage_checked += 1;
                // S11: the numbers it was judged with are fresh.
                let age = self.core.margin.report.as_ref().and_then(|r| self.now.checked_sub(r.queried_at));
                if age.is_none_or(|age| age > self.p.margin_max_age_us) {
                    return Err(self.violation(
                        "margin-fresh",
                        format!("plain order {order_id} accepted with a margin report of age {age:?} us"),
                    ));
                }
                let value = self.core.margin.report.as_ref().map(|r| r.account_value.0);
                let (Some(value), Some(was), Some(is)) = (value, before.exposure, self.oracle_exposure()) else {
                    return Err(self.violation(
                        "leverage-gate",
                        format!("plain order {order_id} accepted without an account value or a reference price"),
                    ));
                };
                let limit = 2 * i128::from(oms_core::num::SCALE) * i128::from(value);
                if value <= 0 || (is > limit && is > was) {
                    return Err(self.violation(
                        "leverage-gate",
                        format!(
                            "plain order {order_id} accepted: exposure {was} -> {is} (1e-16), limit {limit}, account value {}",
                            Amount(value)
                        ),
                    ));
                }
            }
            _ => {}
        }
        Ok(())
    }

    // ---- faults -----------------------------------------------------------

    /// Rebuilds a core from the durable records alone.
    pub fn rebuild(&self) -> Result<Core, Violation> {
        let mut c = Core::new(INSTANCE);
        for rec in &self.durable {
            c.replay(rec).map_err(|e| self.violation("replay", format!("{e} at record {}", rec.seq)))?;
        }
        Ok(c)
    }

    fn crash(&mut self) -> Result<(), Violation> {
        self.stats.crashes += 1;
        // Records that were not durable vanish together with the outputs that
        // were waiting for them: nothing of them was ever released.
        self.pending.clear();
        self.core = self.rebuild()?;
        if self.p.margin {
            // S14: a restart neither ends REDUCING nor starts it.
            let said = self.journal_reducing()?;
            if self.core.margin.reducing.is_some() != said {
                return Err(self.violation(
                    "reducing-release",
                    format!("after a restart REDUCING is {} and the journal says {said}", self.core.margin.reducing.is_some()),
                ));
            }
            self.stats.reducing_restarts += u64::from(said);
        }
        self.core
            .check_invariants()
            .map_err(|e| self.violation("structural-invariant", format!("{e} after recovery")))?;
        self.epoch += 1;
        self.inflight.clear();
        self.link_up = false;
        self.link_up_at = None;
        self.feed(Input::Boot { binary: "sim".into() })?;
        self.commit()?;
        let d = self.rng.range(100_000, 3_000_000);
        self.schedule(d, Net::LinkUp { epoch: self.epoch });
        Ok(())
    }

    fn link_drop(&mut self) -> Result<(), Violation> {
        if !self.link_up {
            return Ok(());
        }
        self.stats.link_drops += 1;
        self.link_up = false;
        self.link_up_at = None;
        let inflight = std::mem::take(&mut self.inflight);
        for (_, kind) in inflight {
            self.feed(Input::Venue(unknown_result(kind, "connection lost before a response")))?;
        }
        self.feed(Input::Venue(VenueInput::Link { up: false, session: self.session, reason: "connection lost".into() }))?;
        let d = self.rng.range(50_000, 2_000_000);
        self.schedule(d, Net::LinkUp { epoch: self.epoch });
        Ok(())
    }

    // ---- actors -----------------------------------------------------------

    fn act_new_order(&mut self) -> Result<(), Violation> {
        self.key_counter += 1;
        let strategy = strat(if self.rng.chance(500) { "a" } else { "b" });
        let side = if self.rng.chance(500) { Side::Buy } else { Side::Sell };
        // Mostly small orders; sometimes one that cannot fit the limits.
        let lots = if self.rng.chance(60) { self.rng.range(150, 260) } else { self.rng.range(1, 60) };
        let tif = match self.rng.below(10) {
            0 | 1 => Tif::Ioc,
            2 => Tif::Alo,
            _ => Tif::Gtc,
        };
        let px = if self.p.margin {
            // Near the venue's mark, so that a fill is not itself a large
            // gain or loss.
            Px(self.venue.mark(&inst()).0 + (self.rng.range(0, 5) as i64 - 2) * PX_TICK)
        } else {
            Px(self.rng.range(90, 110) as i64 * PX_TICK)
        };
        let reduce_only = self.p.margin && self.rng.chance(self.p.p_reduce_only);
        let req = NewOrderReq {
            strategy,
            client_order_id: ClientOrderId::new(format!("k{}", self.key_counter)).expect("valid id"),
            instrument: inst(),
            side,
            qty: Qty(lots as i64 * QTY_STEP),
            px,
            tif,
            reduce_only,
            expires_at: self.now + self.rng.range(800_000, 5_000_000),
        };
        self.stats.requests += 1;
        if self.sent.len() < 400 {
            self.sent.push(req.clone());
        } else {
            let i = self.rng.below(400) as usize;
            self.sent[i] = req.clone();
        }
        self.feed(Input::NewOrder(req))
    }

    fn act_duplicate(&mut self) -> Result<(), Violation> {
        if self.sent.is_empty() {
            return Ok(());
        }
        let i = self.rng.below(self.sent.len() as u64) as usize;
        let mut req = self.sent[i].clone();
        if self.rng.chance(200) {
            req.qty = Qty(req.qty.0 + QTY_STEP); // same key, different request
        }
        self.stats.requests += 1;
        self.feed(Input::NewOrder(req))
    }

    fn act_cancel(&mut self) -> Result<(), Violation> {
        let open: Vec<OrderId> = self.core.open.iter().copied().collect();
        if open.is_empty() {
            return Ok(());
        }
        let id = open[self.rng.below(open.len() as u64) as usize];
        let Some(o) = self.core.order(id) else { return Ok(()) };
        let actor = if self.rng.chance(50) { Actor::Strategy(strat("a")) } else { Actor::Strategy(o.strategy.clone()) };
        self.feed(Input::Cancel(CancelReq { actor, target: CancelTarget::Order(id) }))
    }

    fn act_fill(&mut self) {
        let open = self.venue.open_oids();
        if open.is_empty() {
            return;
        }
        let oid = open[self.rng.below(open.len() as u64) as usize];
        let remaining = self.venue.orders[&oid].remaining().0 / QTY_STEP;
        if remaining <= 0 {
            return;
        }
        let lots = if self.rng.chance(350) { remaining } else { self.rng.range(1, remaining as u64 + 1) as i64 };
        if self.venue.fill(self.venue_now(), oid, Qty(lots * QTY_STEP)).is_some() {
            self.stats.venue_fills += 1;
        }
        self.pump_stream();
    }

    fn act_venue_cancel(&mut self) {
        let open = self.venue.open_oids();
        if open.is_empty() {
            return;
        }
        let oid = open[self.rng.below(open.len() as u64) as usize];
        self.venue.venue_cancel(self.venue_now(), oid, "cancelled by venue");
        self.pump_stream();
    }

    fn act_external_fill(&mut self) {
        let side = if self.rng.chance(500) { Side::Buy } else { Side::Sell };
        let qty = Qty(self.rng.range(1, 10) as i64 * QTY_STEP);
        self.venue.external_fill(self.venue_now(), &inst(), side, qty, Px(100 * PX_TICK));
        self.external_seen = true;
        self.stats.external_fills += 1;
        self.pump_stream();
    }

    fn act_operator(&mut self) -> Result<(), Violation> {
        // Decide from the core's own state: a halt that was not durable
        // before a crash never happened.
        let halt = self.core.halt_account.is_none();
        let cmd = if halt {
            OperatorCmd::Halt { scope: Scope::Account, note: "sim".into() }
        } else {
            OperatorCmd::Resume { scope: Scope::Account, note: "sim".into() }
        };
        self.feed(Input::Operator(cmd))
    }

    /// The venue's mark moves by one tick, inside 80..=120.
    fn act_price(&mut self) {
        let step = if self.rng.chance(500) { PX_TICK } else { -PX_TICK };
        let px = (self.venue.mark(&inst()).0 + step).clamp(80 * PX_TICK, 120 * PX_TICK);
        self.venue.set_mark(&inst(), Px(px));
    }

    /// The venue charges (or pays) funding of 0.01 % to 0.3 % of the
    /// position value.
    fn act_funding(&mut self) {
        let rate = self.rng.range(100, 3_000) as i64;
        let rate = if self.rng.chance(500) { rate } else { -rate };
        self.venue.charge_funding(&inst(), rate);
    }

    /// The venue leaves margin queries unanswered for a while (0.5 to
    /// 2.5 s on the fast margin clock).
    fn act_margin_outage(&mut self) {
        let (lo, hi) = self.p.outage_polls;
        self.margin_out_until = self.now + self.rng.range(lo * self.p.margin_poll_us, hi * self.p.margin_poll_us);
    }

    /// Decides, within the hour before a UTC midnight, whether the venue
    /// leaves margin queries unanswered over that midnight, and from when
    /// to when.
    fn draw_night_outage(&mut self) {
        let midnight = (self.now / DAY_US + 1) * DAY_US;
        let left = midnight - self.now;
        if self.night_drawn_for == midnight || left > 3_600_000_000 {
            return;
        }
        self.night_drawn_for = midnight;
        if self.rng.chance(self.p.p_night_outage) {
            let from = midnight - self.rng.range(0, left);
            self.night_out = (from, midnight + self.rng.range(self.p.margin_poll_us, 600_000_000));
        }
    }

    /// The venue states a margin of zero for the position for two to ten
    /// polls (0.2 to 1 s on the fast margin clock).
    fn act_mm_squeeze(&mut self) {
        self.mm_squeeze_until = self.now + self.rng.range(2 * self.p.margin_poll_us, 10 * self.p.margin_poll_us);
        self.venue.margin_used_override.insert(inst(), Amount::ZERO);
    }

    fn end_mm_squeeze(&mut self) {
        self.mm_squeeze_until = 0;
        self.venue.margin_used_override.remove(&inst());
    }

    /// The operator asks to end REDUCING. The core may refuse.
    fn act_clear(&mut self) -> Result<(), Violation> {
        if self.core.margin.reducing.is_none() {
            return Ok(());
        }
        self.feed(Input::Operator(OperatorCmd::ClearReduce { note: "sim".into() }))
    }

    /// A withdrawal or a deposit of 0.5 % to 8 % of the start capital. An
    /// account under 70 % of the start capital gets a deposit, one above
    /// 130 % a withdrawal, so that the account value stays positive.
    fn act_transfer(&mut self) {
        let amount = START_CAPITAL / 1000 * self.rng.range(5, 81) as i64;
        let value = self.venue.account_value().0;
        let coin = self.rng.chance(500);
        let deposit = value < START_CAPITAL / 10 * 7 || (value <= START_CAPITAL / 10 * 13 && coin);
        self.venue.deposit(Amount(if deposit { amount } else { -amount }));
    }

    /// The operator resets the day base. The core may refuse.
    fn act_rebase(&mut self) -> Result<(), Violation> {
        self.feed(Input::Operator(OperatorCmd::RebaseDay { note: "sim".into() }))
    }

    /// Nothing acts for `us`: no strategy, no venue activity, no fault. The
    /// timers run on, each message at its own time, and the core gets one
    /// tick per margin poll interval.
    fn idle(&mut self, us: u64) -> Result<(), Violation> {
        let end = self.now + us;
        while self.now < end {
            let tick_at = (self.now + self.p.margin_poll_us).min(end);
            while let Some((&(t, n), _)) = self.queue.iter().next() {
                if t > tick_at {
                    break;
                }
                self.now = self.now.max(t);
                if self.mm_squeeze_until != 0 && self.now >= self.mm_squeeze_until {
                    self.end_mm_squeeze();
                }
                if let Some(ev) = self.queue.remove(&(t, n)) {
                    self.deliver(ev)?;
                }
            }
            self.now = tick_at;
            self.feed(Input::Tick)?;
            self.check_always()?;
        }
        Ok(())
    }

    // ---- checks -----------------------------------------------------------

    /// Properties that must hold at every instant, faults included.
    fn check_always(&self) -> Result<(), Violation> {
        if !self.venue.duplicate_placements.is_empty() {
            return Err(self.violation(
                "single-placement",
                format!("venue saw repeated placement for client ids {:?}", self.venue.duplicate_placements),
            ));
        }
        if let Some((c, n)) = self.venue.place_requests.iter().find(|(_, n)| **n > 1) {
            return Err(self.violation("single-placement", format!("client id {c:#x} was requested {n} times")));
        }
        if self.external_seen {
            return Ok(());
        }
        // Bridge: the venue's true position is always explained by booked
        // fills plus what open reservations could still add.
        let b = self.core.account_book(&inst());
        let truth = self.venue.position(&inst());
        let (lo, hi) = (b.pos.0 - b.sell_open.0, b.pos.0 + b.buy_open.0);
        if truth < lo || truth > hi {
            return Err(self.violation(
                "bridge",
                format!("venue position {} outside [{}, {}] (booked {})", SQty(truth), SQty(lo), SQty(hi), b.pos),
            ));
        }
        if self.core.mismatches.is_empty() {
            self.core.check_position_bounds().map_err(|e| self.violation("position-bounds", e))?;
        }
        let spec = &self.core.config.instruments[&inst()];
        if truth < spec.account_pos_min.0 || truth > spec.account_pos_max.0 {
            return Err(self.violation("venue-position-limit", format!("venue position {} outside the account limit", SQty(truth))));
        }
        Ok(())
    }

    /// Properties that must hold once faults have stopped and reconciliation
    /// has finished.
    fn check_final(&mut self) -> Result<(), Violation> {
        self.commit()?;
        let rebuilt = self.rebuild()?;
        if rebuilt != self.core {
            return Err(self.violation("replay-equivalence", "state rebuilt from the journal differs from the live state".into()));
        }
        if self.p.margin {
            self.journal_reducing()?;
            let answered: BTreeSet<(u64, u64)> = self
                .durable
                .iter()
                .filter_map(|r| match &r.input {
                    Input::Venue(VenueInput::MarginReport(m)) => Some((m.session, m.seq)),
                    _ => None,
                })
                .collect();
            self.stats.margin_polls_recorded = answered.len() as u64;
        }
        // Re-deciding every recorded input reproduces the recorded events.
        let mut redo = Core::new(INSTANCE);
        for rec in &self.durable {
            let out = redo.handle(rec.at, &rec.input).map_err(|e| self.violation("re-decide", e.to_string()))?;
            if out.events != rec.events {
                return Err(self.violation("re-decide", format!("record {} re-decided differently", rec.seq)));
            }
        }

        let truth = self.venue.position(&inst());
        let b = self.core.account_book(&inst());
        if b.pos.0 != truth {
            return Err(self.violation("final-position", format!("booked {} but venue has {}", b.pos, SQty(truth))));
        }
        let ext = self.venue.external_positions.get(&inst()).copied().unwrap_or(0);
        let un = self.core.unattributed.get(&inst()).map(|b| b.pos.0).unwrap_or(0);
        if un != ext {
            return Err(self.violation("final-unattributed", format!("unattributed {} but external activity was {}", SQty(un), SQty(ext))));
        }
        if self.external_seen {
            if self.stats.mismatches == 0 {
                return Err(self.violation("external-detection", "external fills happened and no mismatch was raised".into()));
            }
        } else {
            if !self.core.mismatches.is_empty() {
                return Err(self.violation("no-false-mismatch", format!("{:?}", self.core.mismatches.values().collect::<Vec<_>>())));
            }
            let holds = self.core.holds(None);
            if !holds.is_empty() {
                return Err(self.violation("final-holds", format!("{holds:?} with needs {:?}", self.core.needs())));
            }
        }

        // Every venue order of ours is known with the right fills and state.
        let purged: BTreeSet<OrderId> = self
            .durable
            .iter()
            .flat_map(|r| &r.events)
            .filter_map(|e| match e {
                Event::Purged { order_ids, .. } => Some(order_ids),
                _ => None,
            })
            .flatten()
            .copied()
            .collect();
        for vo in self.venue.orders.values() {
            let Some(c) = vo.client else { continue };
            let known = self.core.order(c.order_id);
            let Some(o) = known else {
                if purged.contains(&c.order_id) {
                    continue;
                }
                return Err(self.violation("final-orders", format!("venue order {} unknown to the core", vo.oid)));
            };
            if o.cum_qty != vo.filled {
                return Err(self.violation(
                    "final-fills",
                    format!("order {}: booked {} but venue filled {}", o.id, o.cum_qty, vo.filled),
                ));
            }
            match (&vo.status, o.phase) {
                (SimStatus::Open, Phase::Working) => {
                    if o.reserved != vo.remaining() {
                        return Err(self.violation("final-orders", format!("order {}: reserved {} != remaining {}", o.id, o.reserved, vo.remaining())));
                    }
                }
                (SimStatus::Filled | SimStatus::Canceled(_), Phase::Closed) => {}
                (s, p) => {
                    return Err(self.violation("final-orders", format!("order {}: venue {s:?} but core {p:?}", o.id)));
                }
            }
        }
        // Every order the core still considers open exists at the venue.
        for id in &self.core.open {
            let client = VenueClientId { instance_id: INSTANCE, order_id: *id };
            if self.venue.status(client).is_none_or(|o| o.status != SimStatus::Open) {
                return Err(self.violation("final-orders", format!("core order {id} is open but not open at the venue")));
            }
        }
        // Every order closed as never placed really is absent.
        for o in self.core.orders.values() {
            if let Some(Close { reason: CloseReason::NotSent { .. } | CloseReason::NotPlaced { .. } | CloseReason::RejectedByVenue { .. }, .. }) = &o.close {
                let client = VenueClientId { instance_id: INSTANCE, order_id: o.id };
                if o.cum_qty.is_zero() && self.venue.status(client).is_some() {
                    return Err(self.violation("final-orders", format!("order {} closed as absent but exists at the venue", o.id)));
                }
            }
        }
        let fee_core: i64 = self.core.orders.values().map(|o| o.fee.0).sum();
        let fee_venue: i64 = self.venue.fills.iter().filter(|f| f.client.is_some()).map(|f| f.fee.0).sum();
        if self.stats.purged == 0 && fee_core != fee_venue {
            return Err(self.violation("final-fees", format!("booked fees {} but venue charged {}", Amount(fee_core), Amount(fee_venue))));
        }
        self.stats.final_digest = self.core.digest();
        Ok(())
    }

    // ---- run --------------------------------------------------------------

    fn drain_due(&mut self) -> Result<(), Violation> {
        while let Some((&(t, n), _)) = self.queue.iter().next() {
            if t > self.now {
                break;
            }
            if let Some(ev) = self.queue.remove(&(t, n)) {
                self.deliver(ev)?;
            }
        }
        Ok(())
    }

    fn one_step(&mut self) -> Result<(), Violation> {
        if self.p.p_night_outage > 0 {
            self.draw_night_outage();
        }
        if self.p.p_idle > 0 && self.rng.chance(self.p.p_idle) {
            let us = self.rng.range(self.p.idle_us.0, self.p.idle_us.1);
            self.idle(us)?;
        }
        self.now += self.rng.range(1_000, 30_000);
        if self.mm_squeeze_until != 0 && self.now >= self.mm_squeeze_until {
            self.end_mm_squeeze();
        }
        self.drain_due()?;
        let p = self.p.clone();
        let roll = self.rng.below(1000) as u32;
        let mut edge = 0;
        let mut pick = |w: u32| {
            edge += w;
            roll < edge && roll >= edge - w
        };
        if pick(p.p_new) {
            self.act_new_order()?;
        } else if pick(p.p_dup) {
            self.act_duplicate()?;
        } else if pick(p.p_cancel) {
            self.act_cancel()?;
        } else if pick(p.p_fill) {
            self.act_fill();
        } else if pick(p.p_venue_cancel) {
            self.act_venue_cancel();
        } else if pick(p.p_tick) {
            self.feed(Input::Tick)?;
        } else if pick(p.p_crash) {
            self.crash()?;
        } else if pick(p.p_link_drop) {
            self.link_drop()?;
        } else if pick(p.p_external_fill) {
            self.act_external_fill();
        } else if pick(p.p_operator) {
            self.act_operator()?;
        } else if pick(p.p_price) {
            self.act_price();
        } else if pick(p.p_funding) {
            self.act_funding();
        } else if pick(p.p_clear) {
            self.act_clear()?;
        } else if pick(p.p_margin_outage) {
            self.act_margin_outage();
        } else if pick(p.p_mm_squeeze) {
            self.act_mm_squeeze();
        } else if pick(p.p_transfer) {
            self.act_transfer();
        } else if pick(p.p_rebase) {
            self.act_rebase()?;
        }
        self.check_always()
    }

    /// Stops injecting faults and lets the system settle.
    fn quiesce(&mut self) -> Result<(), Violation> {
        self.calm = true;
        self.margin_out_until = 0;
        self.night_out = (0, 0);
        self.end_mm_squeeze();
        if self.core.halt_account.is_some() {
            self.act_operator()?;
        }
        self.commit()?;
        let deadline = self.now + 90_000_000;
        let mut settled_for = 0u32;
        while self.now < deadline {
            self.now += 100_000;
            self.drain_due()?;
            self.feed(Input::Tick)?;
            // The operator ends REDUCING once the numbers are fresh.
            self.act_clear()?;
            self.commit()?;
            self.check_always()?;
            if self.external_seen {
                // Acknowledge what an operator would after inspecting it.
                let ids: Vec<u32> = self.core.mismatches.values().filter(|m| m.acked.is_none()).map(|m| m.id).collect();
                for id in ids {
                    self.feed(Input::Operator(OperatorCmd::AckMismatch { id, note: "sim".into() }))?;
                }
                self.commit()?;
            }
            let quiet = self.link_up
                && self.inflight.is_empty()
                && self.core.needs().is_empty()
                && !self.core.has_in_doubt()
                && self.core.parked_fills.is_empty()
                && self.core.margin.reducing.is_none()
                && self.queue.values().all(|e| e.is_periodic());
            settled_for = if quiet { settled_for + 1 } else { 0 };
            // Several quiet reconcile rounds in a row, so periodic sweeps ran.
            if settled_for >= 40 {
                break;
            }
        }
        Ok(())
    }

    // ---- scripted runs ----------------------------------------------------
    //
    // A test that needs one exact sequence drives the world itself with the
    // calls below and with `venue`, and reads `core`, `durable` and
    // `stats`. Random actions are those of `run` only.

    /// Boots the core, activates the configuration and lets the link come up.
    pub fn start(&mut self) -> Result<(), Violation> {
        self.feed(Input::Boot { binary: "sim".into() })?;
        self.feed(Input::Config(Box::new(sim_config(&self.p))))?;
        self.commit()?;
        self.schedule(10_000, Net::LinkUp { epoch: self.epoch });
        Ok(())
    }

    /// Lets `us` of virtual time pass in steps of 10 ms: messages that are
    /// due are delivered, what the venue did is put on the stream, and the
    /// always-properties are checked. Everything is durable at the end.
    pub fn advance(&mut self, us: u64) -> Result<(), Violation> {
        let end = self.now + us;
        while self.now < end {
            self.now += 10_000;
            self.pump_stream();
            self.drain_due()?;
            self.check_always()?;
        }
        self.commit()
    }

    /// Lets `us` of virtual time pass with nothing acting: the timers run,
    /// each message at its own time, and the core gets one tick per margin
    /// poll interval. For spans too long for `advance`. Everything is
    /// durable at the end.
    pub fn sleep(&mut self, us: u64) -> Result<(), Violation> {
        self.idle(us)?;
        self.commit()
    }

    /// The venue answers no margin query for the next `us`.
    pub fn margin_outage(&mut self, us: u64) {
        self.margin_out_until = self.now + us;
    }

    /// One operator command.
    pub fn operator(&mut self, cmd: OperatorCmd) -> Result<(), Violation> {
        self.feed(Input::Operator(cmd))?;
        self.commit()
    }

    /// Kills the process and starts it again from the journal.
    pub fn restart(&mut self) -> Result<(), Violation> {
        self.crash()
    }

    /// One new-order request of a strategy.
    pub fn submit(&mut self, req: NewOrderReq) -> Result<(), Violation> {
        self.stats.requests += 1;
        self.feed(Input::NewOrder(req))?;
        self.commit()
    }

    pub fn run(mut self) -> Result<SimStats, Violation> {
        let r = self.run_inner();
        if let (Err(v), Ok(filter)) = (&r, std::env::var("OMS_SIM_DUMP")) {
            // Debug aid: print the durable records that mention `filter`.
            eprintln!("violation: {v}");
            for rec in &self.durable {
                let line = serde_json::to_string(rec).unwrap_or_default();
                if filter.is_empty() || line.contains(&filter) {
                    eprintln!("{line}");
                }
            }
        }
        r.map(|()| self.stats)
    }

    fn run_inner(&mut self) -> Result<(), Violation> {
        self.feed(Input::Boot { binary: "sim".into() })?;
        self.feed(Input::Config(Box::new(sim_config(&self.p))))?;
        self.commit()?;
        self.schedule(10_000, Net::LinkUp { epoch: self.epoch });
        let start = self.now;
        let mut step = 0;
        while step < self.p.steps || self.now < start + self.p.run_us {
            self.step = step;
            self.one_step()?;
            step += 1;
        }
        self.step = step;
        self.stats.steps = step;
        self.quiesce()?;
        self.check_final()?;
        self.stats.sim_seconds = (self.now - start) / 1_000_000;
        Ok(())
    }
}

fn unknown_result(kind: Inflight, reason: &str) -> VenueInput {
    match kind {
        Inflight::Place(order_id) => {
            VenueInput::SendResult { order_id, outcome: SendOutcome::Unknown { reason: reason.to_string() }, timing: Default::default() }
        }
        Inflight::Cancel(order_id, attempt) => VenueInput::CancelResult {
            order_id,
            attempt,
            outcome: CancelOutcome::Unknown { reason: reason.to_string() },
            timing: Default::default(),
        },
    }
}

/// Runs one seed of one profile.
pub fn run(seed: u64, profile: Profile) -> Result<SimStats, Violation> {
    World::new(seed, profile).run()
}
