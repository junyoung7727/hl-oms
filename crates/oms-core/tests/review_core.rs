//! Tests from the independent review of the deterministic core (2026-10-02).
//! Every test named `fN_…` was written by the reviewer to fail while defect
//! N existed and is kept as its regression test; where the fix changed what
//! the right behaviour is, the test says so. The `examined_…` test records
//! what random adversarial input could not break.

#![allow(clippy::unwrap_used)]

mod common;
use common::*;
use oms_core::*;

// ---- exploratory fuzz ----------------------------------------------------------

struct Rng(u64, Vec<FillIn>);
impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
    fn chance(&mut self, pct: u64) -> bool {
        self.below(100) < pct
    }
}

fn fuzz_inputs(r: &mut Rng, core: &Core, now: Ts, unique_oids: bool) -> Vec<Input> {
    let session = core.link.session;
    if r.chance(12) {
        // Operator acknowledges everything that is pending.
        return core
            .mismatches
            .values()
            .filter(|m| m.acked.is_none())
            .map(|m| Input::Operator(OperatorCmd::AckMismatch { id: m.id, note: "f".into() }))
            .collect();
    }
    if r.chance(15) {
        // An honest-looking full sweep.
        let mut v = Vec::new();
        if !core.link.up {
            v.push(Input::Venue(VenueInput::Link { up: true, session: session + 1, reason: "f".into() }));
        }
        let session = if core.link.up { session } else { session + 1 };
        v.push(Input::Venue(VenueInput::FillsSynced { session, up_to: now.saturating_sub(1) }));
        let orders = core
            .open
            .iter()
            .filter_map(|id| core.order(*id))
            .filter(|o| o.phase == Phase::Working)
            .map(|o| VenueOpenOrder {
                venue_oid: o.venue_oid.clone().unwrap_or_else(|| void(&format!("v{}", o.id.0))),
                order_id: Some(o.id),
                instrument: iid("X"),
                side: o.side,
                remaining: None,
            })
            .collect();
        v.push(Input::Venue(VenueInput::OpenOrders { session, queried_at: now, orders }));
        v.push(Input::Venue(VenueInput::Positions {
            session,
            queried_at: now,
            positions: vec![VenuePosition { instrument: iid("X"), qty: core.account_book(&iid("X")).pos }],
        }));
        return v;
    }
    vec![fuzz_input(r, core, now, unique_oids)]
}

fn fuzz_input(r: &mut Rng, core: &Core, now: Ts, unique_oids: bool) -> Input {
    let max_id = core.next_order_id.max(2);
    // Mostly aim at an order that is still open.
    let open: Vec<OrderId> = core.open.iter().copied().collect();
    let oid = if !open.is_empty() && r.chance(80) {
        open[r.below(open.len() as u64) as usize]
    } else {
        OrderId(1 + r.below(max_id))
    };
    let side = if r.chance(50) { Side::Buy } else { Side::Sell };
    let venue_oid_for = |r: &mut Rng, id: OrderId| {
        if unique_oids {
            void(&format!("v{}", id.0))
        } else {
            void(&format!("v{}", 1 + r.below(4)))
        }
    };
    let session = if r.chance(85) {
        core.link.session
    } else {
        core.link.session.saturating_sub(1)
    };
    match r.below(100) {
        0..=17 => Input::NewOrder(NewOrderReq {
            strategy: sid(if r.chance(50) { "a" } else { "b" }),
            client_order_id: cid(&format!("k{}", r.below(40))),
            instrument: iid("X"),
            side,
            qty: q(["1", "2", "3", "7"][r.below(4) as usize]),
            px: px("100"),
            tif: Tif::Gtc,
            reduce_only: false,
            expires_at: now + (1 + r.below(8)) * SEC,
        }),
        18..=23 => Input::Cancel(CancelReq {
            actor: if r.chance(30) {
                Actor::Operator
            } else {
                Actor::Strategy(sid(if r.chance(50) { "a" } else { "b" }))
            },
            target: CancelTarget::Order(oid),
        }),
        24..=33 => {
            let outcome = match r.below(6) {
                0 | 1 | 2 => SendOutcome::Placed {
                    venue_oid: venue_oid_for(r, oid),
                    status: StatusIn {
                        status: match r.below(5) {
                            0 => VenueStatus::Filled,
                            1 => VenueStatus::Canceled { reason: "x".into() },
                            2 => VenueStatus::Rejected { reason: "x".into() },
                            _ => VenueStatus::Open,
                        },
                        cum: if r.chance(50) { Some(q(["0", "1", "2"][r.below(3) as usize])) } else { None },
                        venue_ts: now.saturating_sub(r.below(3) * SEC),
                    },
                },
                3 => SendOutcome::Rejected { reason: "no".into() },
                4 => SendOutcome::NotSent { reason: "no".into() },
                _ => SendOutcome::Unknown { reason: "?".into() },
            };
            Input::Venue(VenueInput::SendResult {
                order_id: oid,
                outcome,
                timing: Default::default(),
            })
        }
        34..=38 => Input::Venue(VenueInput::CancelResult {
            order_id: oid,
            attempt: 1 + r.below(5) as u32,
            outcome: match r.below(4) {
                0 => CancelOutcome::Confirmed,
                1 => CancelOutcome::Rejected { reason: "x".into() },
                2 => CancelOutcome::Unknown { reason: "x".into() },
                _ => CancelOutcome::NotSent { reason: "x".into() },
            },
            timing: Default::default(),
        }),
        39..=50 => Input::Venue(VenueInput::OrderStatus(OrderStatusIn {
            session,
            order_id: if r.chance(80) { Some(oid) } else { None },
            venue_oid: if r.chance(70) { Some(venue_oid_for(r, oid)) } else { None },
            status: StatusIn {
                status: match r.below(6) {
                    0 => VenueStatus::Filled,
                    1 => VenueStatus::Canceled { reason: "x".into() },
                    2 => VenueStatus::Rejected { reason: "x".into() },
                    _ => VenueStatus::Open,
                },
                cum: if r.chance(50) { Some(q(["0", "1", "2", "3"][r.below(4) as usize])) } else { None },
                venue_ts: now.saturating_sub(r.below(3) * SEC),
            },
            source: EvidenceSource::Stream,
        })),
        51..=54 => Input::Venue(VenueInput::StatusNotFound {
            session,
            order_id: oid,
            queried_at: now.saturating_sub(r.below(20) * SEC),
        }),
        55..=72 => {
            // A fill key always denotes the same fill (G-8): re-deliveries
            // repeat an earlier fill exactly.
            if !r.1.is_empty() && r.chance(30) {
                let i = r.below(r.1.len() as u64) as usize;
                return Input::Venue(VenueInput::Fill(r.1[i].clone()));
            }
            let f = FillIn {
                key: fkey(&format!("t{}", r.1.len())),
                order_id: if r.chance(60) { Some(oid) } else { None },
                venue_oid: if r.chance(80) { Some(venue_oid_for(r, oid)) } else { None },
                instrument: iid("X"),
                side: match core.order(oid) {
                    Some(o) if r.chance(90) => o.side,
                    _ => side,
                },
                qty: q(["0.5", "1", "2"][r.below(3) as usize]),
                px: px("100"),
                fee: Amount::ZERO,
                venue_ts: now.saturating_sub(r.below(3) * SEC),
            };
            r.1.push(f.clone());
            Input::Venue(VenueInput::Fill(f))
        }
        73..=76 => Input::Venue(VenueInput::FillsSynced {
            session,
            up_to: now.saturating_sub(r.below(2) * SEC),
        }),
        77..=80 => {
            let n = r.below(3);
            let mut orders = Vec::new();
            for _ in 0..n {
                let id = OrderId(1 + r.below(max_id));
                orders.push(VenueOpenOrder {
                    venue_oid: venue_oid_for(r, id),
                    order_id: if r.chance(80) { Some(id) } else { None },
                    instrument: iid("X"),
                    side: core.order(id).map(|o| o.side).unwrap_or(side),
                    remaining: if r.chance(50) { Some(q("1")) } else { None },
                });
            }
            Input::Venue(VenueInput::OpenOrders {
                session,
                queried_at: now.saturating_sub(r.below(2) * SEC),
                orders,
            })
        }
        81..=84 => Input::Venue(VenueInput::Positions {
            session,
            queried_at: now.saturating_sub(r.below(2) * SEC),
            positions: vec![VenuePosition {
                instrument: iid("X"),
                qty: if r.chance(80) {
                    core.account_book(&iid("X")).pos
                } else {
                    sq("1")
                },
            }],
        }),
        85 => Input::Venue(VenueInput::Link {
            up: r.chance(50),
            session: core.link.session + r.below(2),
            reason: "f".into(),
        }),
        86..=95 => Input::Tick,
        96 if r.chance(30) => Input::Operator(OperatorCmd::CancelAll {
            scope: Scope::Account,
            note: "f".into(),
        }),
        96 => Input::Tick,
        97 | 98 => {
            let id = core
                .mismatches
                .values()
                .find(|m| m.acked.is_none())
                .map(|m| m.id)
                .unwrap_or(1);
            Input::Operator(OperatorCmd::AckMismatch { id, note: "f".into() })
        }
        _ if r.chance(30) => Input::Boot { binary: "fuzz".into() },
        _ => Input::Tick,
    }
}

/// Arbitrary (assumption-violating) input, with venue order ids and fill keys
/// kept self-consistent. Checked after every input: no fault, structural
/// invariants, apply-only shadow equals the live state, one send per order,
/// no acceptance under a hold, no command from Boot, every new fill is
/// booked or parked, no fill key booked twice, and position bounds are left
/// only together with a mismatch. At the end: once everything is settled and
/// acknowledged, no hold remains.
fn fuzz_run(seed: u64, steps: usize, unique_oids: bool) -> Result<(), String> {
    let mut r = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1, Vec::new());
    let mut cfg = config();
    cfg.policy.closed_retention_us = 5 * SEC;
    cfg.policy.dedup_grace_us = SEC;
    cfg.policy.purge_interval_us = 3 * SEC;
    cfg.policy.send_result_timeout_us = 4 * SEC;
    // Must exceed the expiry horizon plus the absence margin (validated since F10).
    cfg.policy.max_expiry_horizon_us = 9 * SEC;
    cfg.policy.unmatched_fill_wait_us = 12 * SEC;
    cfg.policy.absence_margin_us = SEC;
    cfg.policy.fill_dedup_retention_us = 20 * SEC;
    let mut core = Core::new(7);
    let mut shadow = Core::new(7);
    let mut now: Ts = 1_000 * SEC;
    let mut trace: Vec<String> = Vec::new();
    let mut sends: std::collections::BTreeSet<OrderId> = Default::default();
    let mut bounds_ok = true;
    let mut booked: std::collections::BTreeSet<FillKey> = Default::default();
    let boot = [
        Input::Boot { binary: "t".into() },
        Input::Config(Box::new(cfg)),
        Input::Venue(VenueInput::Link {
            up: true,
            session: 1,
            reason: "t".into(),
        }),
    ];
    for step in 0..steps + boot.len() {
        let inputs = if step < boot.len() {
            vec![boot[step].clone()]
        } else {
            fuzz_inputs(&mut r, &core, now, unique_oids)
        };
        for input in inputs {
            now += 1 + r.below(400_000);
            trace.push(format!("{now} {input:?}"));
            let fail = |why: String, trace: &Vec<String>| {
                let tail = trace.iter().rev().take(60).rev().cloned().collect::<Vec<_>>().join("\n");
                Err(format!("seed {seed} step {step}: {why}\n{tail}"))
            };
            let held = match &input {
                Input::NewOrder(n) => !core.holds(Some(&n.strategy)).is_empty(),
                _ => false,
            };
            let fresh_fill = match &input {
                Input::Venue(VenueInput::Fill(f)) => {
                    !booked.contains(&f.key)
                        && f.venue_ts >= core.fill_floor
                        && !core.parked_fills.iter().any(|p| p.fill.key == f.key)
                }
                _ => false,
            };
            let out = match core.handle(now, &input) {
                Ok(o) => o,
                Err(e) => return fail(format!("fault {e}"), &trace),
            };
            if held && out.events.iter().any(|e| matches!(e, Event::OrderAccepted { .. })) {
                return fail("accepted under a hold".into(), &trace);
            }
            if fresh_fill
                && !out.events.iter().any(|e| {
                    matches!(
                        e,
                        Event::FillBooked { .. } | Event::FillParked { .. } | Event::ExternalFillBooked { .. }
                    )
                })
            {
                return fail("a new fill produced no booking".into(), &trace);
            }
            for ev in &out.events {
                let key = match ev {
                    Event::FillBooked { key, .. } => Some(key.clone()),
                    Event::ExternalFillBooked { fill } => Some(fill.key.clone()),
                    _ => None,
                };
                if let Some(k) = key {
                    if !booked.insert(k.clone()) {
                        return fail(format!("fill {k} booked twice"), &trace);
                    }
                }
                trace.push(format!("      -> {ev:?}"));
                if let Err(e) = shadow.apply(now, ev) {
                    return fail(format!("apply fault {e}"), &trace);
                }
            }
            if core != shadow {
                return fail("replay diverged".into(), &trace);
            }
            if let Err(e) = core.check_invariants() {
                return fail(format!("invariant: {e}"), &trace);
            }
            for c in &out.commands {
                if let VenueCommand::SendOrder { order_id, .. } = c {
                    if !sends.insert(*order_id) {
                        return fail("second send".into(), &trace);
                    }
                }
            }
            if matches!(input, Input::Boot { .. }) && !out.commands.is_empty() {
                return fail("boot produced commands".into(), &trace);
            }
            // Bounds may only go from holding to violated in an input that
            // raises a mismatch, or while one is still unacknowledged.
            let bounds = core.check_position_bounds();
            let raised_now = out.events.iter().any(|e| matches!(e, Event::MismatchRaised { .. }));
            let pending = core.mismatches.values().any(|m| m.acked.is_none());
            if let (true, Err(e)) = (bounds_ok, &bounds) {
                if !raised_now && !pending {
                    return fail(format!("bounds without mismatch: {e}"), &trace);
                }
            }
            bounds_ok = bounds.is_ok();
        }
    }
    // Liveness: once the venue settles everything honestly and the operator
    // acknowledges what was raised, no hold may remain.
    let mut step_ok = |core: &mut Core, now: &mut Ts, input: Input| -> Result<(), String> {
        *now += 1_000;
        let out = core.handle(*now, &input).map_err(|e| format!("seed {seed} quiesce fault {e}"))?;
        for ev in &out.events {
            shadow.apply(*now, ev).map_err(|e| format!("seed {seed} quiesce apply {e}"))?;
        }
        core.check_invariants().map_err(|e| format!("seed {seed} quiesce invariant {e}"))
    };
    for round in 0..3 {
        let session = core.link.session + 1;
        step_ok(&mut core, &mut now, Input::Venue(VenueInput::Link { up: true, session, reason: "q".into() }))?;
        for id in core.open.iter().copied().collect::<Vec<_>>() {
            let cum = core.order(id).map(|o| o.cum_qty);
            let venue_oid = core.order(id).and_then(|o| o.venue_oid.clone());
            let ts = now;
            step_ok(
                &mut core,
                &mut now,
                Input::Venue(VenueInput::OrderStatus(OrderStatusIn {
                    session,
                    order_id: Some(id),
                    venue_oid,
                    status: StatusIn { status: VenueStatus::Canceled { reason: "q".into() }, cum, venue_ts: ts },
                    source: EvidenceSource::Query,
                })),
            )?;
            if round > 0 && core.order(id).is_some_and(|o| !o.is_closed()) {
                step_ok(&mut core, &mut now, Input::Operator(OperatorCmd::ResolveOrder { order_id: id, note: "q".into() }))?;
            }
        }
        now += 200 * SEC;
        step_ok(&mut core, &mut now, Input::Tick)?;
        for id in core.mismatches.values().filter(|m| m.acked.is_none()).map(|m| m.id).collect::<Vec<_>>() {
            step_ok(&mut core, &mut now, Input::Operator(OperatorCmd::AckMismatch { id, note: "q".into() }))?;
        }
        let t = now;
        step_ok(&mut core, &mut now, Input::Venue(VenueInput::FillsSynced { session, up_to: t }))?;
        step_ok(&mut core, &mut now, Input::Venue(VenueInput::OpenOrders { session, queried_at: t + 500, orders: vec![] }))?;
        let qty = core.account_book(&iid("X")).pos;
        step_ok(
            &mut core,
            &mut now,
            Input::Venue(VenueInput::Positions {
                session,
                queried_at: t + 500,
                positions: vec![VenuePosition { instrument: iid("X"), qty }],
            }),
        )?;
    }
    if !core.holds(None).is_empty() || !core.open.is_empty() {
        return Err(format!("seed {seed}: after quiescence holds {:?}, open {:?}", core.holds(None), core.open));
    }
    if std::env::var("FUZZ_STATS").is_ok() {
        eprintln!("seed {seed}: {:?} mismatches {} orders {}", core.stats, core.next_mismatch_id - 1, core.next_order_id - 1);
    }
    Ok(())
}

#[test]
fn examined_fuzz_arbitrary_venue_input_keeps_invariants_and_liveness() {
    for seed in 1..=3000 {
        if let Err(e) = fuzz_run(seed, 800, true) {
            panic!("{e}");
        }
    }
}


// ---- helpers -------------------------------------------------------------------

fn cancel_result(h: &mut H, id: OrderId, attempt: u32, outcome: CancelOutcome) {
    h.venue(VenueInput::CancelResult {
        order_id: id,
        attempt,
        outcome,
        timing: Default::default(),
    });
}

fn unknown_send(h: &mut H, id: OrderId) {
    h.venue(VenueInput::SendResult {
        order_id: id,
        outcome: SendOutcome::Unknown { reason: "timeout".into() },
        timing: Default::default(),
    });
}

fn fill_in(key: &str, order_id: Option<OrderId>, oid: &str, side: Side, qty: &str, price: &str, ts: Ts) -> FillIn {
    FillIn {
        key: fkey(key),
        order_id,
        venue_oid: Some(void(oid)),
        instrument: iid("X"),
        side,
        qty: q(qty),
        px: px(price),
        fee: Amount::ZERO,
        venue_ts: ts,
    }
}

// ---- F1: the cancel attempt budget is a one-way door ------------------------------

/// Five transient failures (here: the gateway could not even send) use up
/// the attempt budget for good. The cancel state is never reset, a second
/// cancel request answers `already_requested` and `CancelAll` skips the
/// order, so the OMS can never again try to cancel a resting order.
#[test]
fn f1_cancel_can_be_reissued_after_transient_failures_used_up_the_attempts() {
    let mut h = H::ready();
    let a = h.accept("a", "k1", Side::Buy, "1", "100");
    h.placed_open(a, "v1");
    h.cancel("a", a);
    for attempt in 1..=5 {
        cancel_result(&mut h, a, attempt, CancelOutcome::NotSent { reason: "rate limited".into() });
        h.advance(SEC);
        h.tick();
    }
    assert_eq!(h.cancels().len(), 5);
    assert_eq!(h.order(a).phase, Phase::Working, "the order still rests at the venue");
    // The outage is over. An explicit operator command must be able to try again.
    h.advance(60 * SEC);
    h.op(OperatorCmd::CancelAll {
        scope: Scope::Account,
        note: "retry".into(),
    })
    .unwrap();
    h.cancel("a", a);
    h.tick();
    assert_eq!(h.cancels().len(), 6, "no way to issue another cancel for a resting order");
}

/// The resulting `CancelStuck` hold cannot be cleared either: it is raised
/// again on the next tick after every acknowledgement.
#[test]
fn f1_acknowledging_cancel_stuck_releases_the_hold() {
    let mut h = H::ready();
    let a = h.accept("a", "k1", Side::Buy, "1", "100");
    h.placed_open(a, "v1");
    h.cancel("a", a);
    for attempt in 1..=5 {
        cancel_result(&mut h, a, attempt, CancelOutcome::Unknown { reason: "timeout".into() });
        h.advance(SEC);
        h.tick();
    }
    assert_eq!(h.unacked_mismatches(), vec![MismatchKind::CancelStuck]);
    let id = *h.core.mismatches.keys().next().unwrap();
    h.op(OperatorCmd::AckMismatch { id, note: "seen".into() }).unwrap();
    assert_eq!(h.core.holds(None), vec![]);
    h.tick();
    assert_eq!(h.core.holds(None), vec![], "the acknowledged mismatch came straight back");
}

/// Attempts are spent while the order is still in doubt (cancel by client id
/// is refused because the request has not reached the venue yet). When the
/// order then appears, the requested cancel can no longer be sent.
#[test]
fn f1_cancel_attempts_spent_in_doubt_do_not_strand_the_order_once_it_is_found() {
    let mut h = H::ready();
    let a = h.accept("a", "k1", Side::Buy, "1", "100");
    unknown_send(&mut h, a);
    assert_eq!(h.cancel("a", a), CancelReply::Requested { order_id: a });
    for attempt in 1..=5 {
        cancel_result(&mut h, a, attempt, CancelOutcome::Rejected { reason: "unknown order".into() });
        h.advance(SEC / 2);
        h.tick();
        h.advance(SEC / 2);
        h.tick();
    }
    let before = h.cancels().len();
    // The delayed request lands; a status query finds the order resting.
    h.status(a, VenueStatus::Open, Some("0"));
    assert_eq!(h.order(a).phase, Phase::Working);
    h.advance(2 * SEC);
    h.tick();
    assert!(
        h.cancels().len() > before,
        "the strategy asked for a cancel, the order rests, and no cancel is sent ({before} attempts were spent in doubt)"
    );
}

/// The reviewer found C-CANCEL and the code disagreeing about a rejected
/// cancel. The contract was corrected to the code's rule: the venue's
/// rejection text does not say whether the order is gone, so the status is
/// queried and, while the order is still not known to be terminal, the
/// cancel is tried again after the back-off, within the attempt budget.
/// Terminal evidence ends it.
#[test]
fn f1_a_rejected_cancel_is_retried_only_while_the_order_is_not_terminal() {
    let mut h = H::ready();
    let a = h.accept("a", "k1", Side::Buy, "1", "100");
    h.placed_open(a, "v1");
    h.cancel("a", a);
    cancel_result(&mut h, a, 1, CancelOutcome::Rejected { reason: "already filled or cancelled".into() });
    // The status query is asked for at once.
    assert!(h.core.needs().iter().any(|n| matches!(
        n,
        Need::OrderStatus { order_id, why: oms_core::needs::NeedReason::AwaitingCancelResult, .. } if *order_id == a
    )));
    h.advance(2 * SEC);
    h.tick();
    assert_eq!(h.cancels(), vec![(a, 1), (a, 2)]);

    // A second order: the query answers "cancelled" before the back-off is over.
    let b = h.accept("a", "k2", Side::Buy, "1", "100");
    h.placed_open(b, "v2");
    h.cancel("a", b);
    cancel_result(&mut h, b, 1, CancelOutcome::Rejected { reason: "already filled or cancelled".into() });
    h.status(b, VenueStatus::Canceled { reason: "canceled".into() }, Some("0"));
    h.advance(2 * SEC);
    h.tick();
    assert_eq!(h.cancels().iter().filter(|(id, _)| *id == b).count(), 1);
}

/// gateway.md: "the core tolerates a G-5 violation by timeout". There is no
/// timeout: a cancel whose result never arrives stays in flight for ever.
#[test]
fn f1_a_cancel_without_any_result_is_retried_after_a_timeout() {
    let mut h = H::ready();
    let a = h.accept("a", "k1", Side::Buy, "1", "100");
    h.placed_open(a, "v1");
    h.cancel("a", a);
    h.advance(600 * SEC);
    h.tick();
    assert_eq!(h.cancels(), vec![(a, 1), (a, 2)]);
}

// ---- F2: a parked fill is not drained when a fill reveals the venue id ----------

/// One fill arrives carrying only the venue order id and is parked. A second
/// fill carries both the client id and the same venue id: the order is now
/// identified, but `book_fill` does not drain the parked list, so the first
/// fill is booked as external on the next tick.
#[test]
fn f2_parked_fill_is_booked_to_its_order_when_another_fill_reveals_the_venue_id() {
    let mut h = H::ready();
    let a = h.accept("a", "k1", Side::Buy, "4", "100");
    let ts = h.now;
    h.venue(VenueInput::Fill(fill_in("t1", None, "v1", Side::Buy, "1", "100", ts)));
    assert_eq!(h.core.parked_fills.len(), 1);
    h.venue(VenueInput::Fill(fill_in("t2", Some(a), "v1", Side::Buy, "1", "100", ts)));
    assert_eq!(h.order(a).venue_oid, Some(void("v1")));
    h.tick();
    assert_eq!(h.unacked_mismatches(), vec![], "a fill of our own order was reported as external");
    assert_eq!(h.order(a).cum_qty, q("2"));
    assert_eq!(h.book("a").pos, sq("2"));
    assert_eq!(h.acct().buy_open, q("2"));
}

// ---- F3: evidence that contradicts a "never placed" close is dropped ------------

fn closed_as_not_placed(h: &mut H) -> OrderId {
    let a = h.accept("a", "k1", Side::Buy, "4", "100");
    unknown_send(h, a);
    h.advance(16 * SEC);
    let session = h.session;
    let queried_at = h.now;
    h.venue(VenueInput::StatusNotFound {
        session,
        order_id: a,
        queried_at,
    });
    assert!(matches!(
        h.order(a).close.as_ref().map(|c| &c.reason),
        Some(CloseReason::NotPlaced { .. })
    ));
    assert_eq!(h.acct().buy_open, q("0"));
    a
}

/// The order was closed as not placed and its reservation released. The
/// venue's own answer "placed, resting" then arrives. It is ignored without
/// a trace; new orders keep being accepted against the released headroom
/// until the next open-order sweep happens to list the order.
#[test]
fn f3_placed_evidence_for_an_order_closed_as_not_placed_is_flagged() {
    let mut h = H::ready();
    let a = closed_as_not_placed(&mut h);
    h.placed_open(a, "v1");
    assert_eq!(h.unacked_mismatches(), vec![MismatchKind::OpenAfterClose], "a live, unreserved order went unnoticed");
    // Further reports about the same order add nothing.
    h.status(a, VenueStatus::Open, Some("0"));
    assert_eq!(h.unacked_mismatches(), vec![MismatchKind::OpenAfterClose]);
    assert!(!h.core.holds(None).is_empty(), "new orders are held until the operator has looked");
}

/// order-lifecycle.md (C-DOUBT) said the sweep raises `OrphanOrder` here;
/// reconciliation.md and the code say `OpenAfterClose`, which is the right
/// name (the order is known, and closed). C-DOUBT was corrected.
#[test]
fn f3_a_reappearing_order_is_reported_as_open_after_close() {
    let mut h = H::ready();
    let a = closed_as_not_placed(&mut h);
    let session = h.session;
    let queried_at = h.now + 1;
    h.venue(VenueInput::OpenOrders {
        session,
        queried_at,
        orders: vec![VenueOpenOrder {
            venue_oid: void("v1"),
            order_id: Some(a),
            instrument: iid("X"),
            side: Side::Buy,
            remaining: None,
        }],
    });
    assert_eq!(h.unacked_mismatches(), vec![MismatchKind::OpenAfterClose]);
}

// ---- F4: stale evidence counts as fresh when it acknowledges an order -----------

/// reconciliation.md: evidence gathered in an earlier session is applied but
/// "not counted as fresh". If it is the evidence that acknowledges an
/// in-doubt order, `OrderAcked` stamps the order with the current session
/// and the `Resync` condition "every working order has evidence from this
/// session" is satisfied by evidence from before the outage.
#[test]
fn f4_stale_session_evidence_that_acknowledges_an_order_does_not_satisfy_resync() {
    let mut h = H::ready();
    let a = h.accept("a", "k1", Side::Buy, "4", "100");
    unknown_send(&mut h, a);
    let old = h.session;
    h.venue(VenueInput::Link {
        up: false,
        session: old,
        reason: "lost".into(),
    });
    h.link_up_and_sync();
    assert_eq!(h.core.holds(None), vec![HoldReason::InDoubt]);
    // A stream message from the old session, delivered late.
    h.venue(VenueInput::OrderStatus(OrderStatusIn {
        session: old,
        order_id: Some(a),
        venue_oid: Some(void("v1")),
        status: StatusIn {
            status: VenueStatus::Open,
            cum: Some(Qty::ZERO),
            venue_ts: 0,
        },
        source: EvidenceSource::Stream,
    }));
    assert_eq!(h.order(a).phase, Phase::Working);
    assert_eq!(
        h.core.holds(None),
        vec![HoldReason::Resync],
        "order a has no evidence from the current session, yet new orders are accepted"
    );
}

// ---- F5: a stale "link down" notice ------------------------------------------------

/// A late `up` with an old session is ignored; a late `down` for an old
/// session is not, and nothing will bring the link up again (the gateway
/// already reported the current session).
#[test]
fn f5_stale_link_down_does_not_take_a_newer_session_down() {
    let mut h = H::ready();
    let old = h.session;
    h.link_up_and_sync();
    h.venue(VenueInput::Link {
        up: false,
        session: old,
        reason: "late notice for the previous stream".into(),
    });
    assert!(h.core.link.up);
    assert_eq!(h.core.holds(None), vec![]);
}

// ---- F6: venue data that stops or bricks the core ----------------------------------

/// Reporting counters (`traded_qty`, `traded_notional`, `fees`) are checked
/// sums whose overflow is a `CoreFault`. The fill is never journaled, so it
/// is delivered again after the restart and faults again. Here the position
/// itself is flat and well inside i64.
#[test]
fn f6_reporting_counter_overflow_does_not_fail_stop_fill_booking() {
    let mut h = H::ready();
    let ts = h.now;
    let big = "50000000000";
    h.venue(VenueInput::Fill(fill_in("t1", None, "z1", Side::Buy, big, "0.00000001", ts)));
    assert_eq!(h.acct().pos, sq(big));
    let second = Input::Venue(VenueInput::Fill(fill_in("t2", None, "z2", Side::Sell, big, "0.00000001", ts)));
    let mut core = h.core.clone();
    let res = core.handle(h.now + 1, &second);
    assert!(res.is_ok(), "{res:?}");
}

/// One venue report that pairs a known venue order id with a second order
/// overwrites the venue-id index. The record is journaled; the structural
/// invariant then fails, and recovery refuses a state that fails it.
#[test]
fn f6_a_venue_order_id_reported_for_two_orders_keeps_the_state_recoverable() {
    let mut h = H::ready();
    let a = h.accept("a", "k1", Side::Buy, "1", "100");
    let b = h.accept("a", "k2", Side::Buy, "1", "100");
    h.placed_open(a, "v1");
    let mut core = h.core.clone();
    let res = core.handle(
        h.now + 1,
        &Input::Venue(VenueInput::SendResult {
            order_id: b,
            outcome: SendOutcome::Placed {
                venue_oid: void("v1"),
                status: StatusIn {
                    status: VenueStatus::Open,
                    cum: None,
                    venue_ts: h.now,
                },
            },
            timing: Default::default(),
        }),
    );
    assert!(res.is_ok());
    assert_eq!(core.check_invariants(), Ok(()));
}

// ---- F7: unbounded mismatch list ----------------------------------------------------

/// Every external fill raises its own mismatch (subject = fill key). The
/// unacknowledged list is never purged and each entry needs its own `ack`.
#[test]
fn f7_external_fills_do_not_grow_the_mismatch_list_without_bound() {
    let mut h = H::ready();
    let n = 300;
    for i in 0..n {
        let ts = h.now;
        let side = if i % 2 == 0 { Side::Buy } else { Side::Sell };
        h.venue(VenueInput::Fill(fill_in(&format!("x{i}"), None, &format!("z{i}"), side, "1", "100", ts)));
    }
    assert!(
        h.core.mismatches.len() < n,
        "{} unacknowledged mismatches, one operator ack each",
        h.core.mismatches.len()
    );
}

// ---- F8: own order listed without a client id is called foreign ---------------------

/// decide.rs notes that the venue's open-order list "may not carry our
/// client id". Fills in that situation are parked while an unidentified
/// order of ours could own them; a listed order is flagged at once.
#[test]
fn f8_own_unacknowledged_order_listed_without_client_id_is_not_flagged_foreign() {
    let mut h = H::ready();
    let a = h.accept("a", "k1", Side::Buy, "4", "100");
    let session = h.session;
    let queried_at = h.now + 1;
    h.venue(VenueInput::OpenOrders {
        session,
        queried_at,
        orders: vec![VenueOpenOrder {
            venue_oid: void("v1"),
            order_id: None,
            instrument: iid("X"),
            side: Side::Buy,
            remaining: Some(q("4")),
        }],
    });
    assert_eq!(h.unacked_mismatches(), vec![], "the send result for order a is simply still on its way");
    h.placed_open(a, "v1");
    assert_eq!(h.core.holds(None), vec![]);
}

// ---- F9: minimum notional is checked with a value rounded up -------------------------

/// num.rs: "rounding up is the conservative direction for every use: the
/// result is compared against a maximum". It is also compared against the
/// minimum, where rounding up admits an order below it.
#[test]
fn f9_min_notional_is_not_passed_by_rounding_up() {
    let mut cfg = config();
    let spec = cfg.instruments.get_mut(&iid("X")).unwrap();
    spec.qty_step = q("0.00000001");
    spec.px_tick = px("0.00000001");
    spec.px_max_sig_figs = None;
    spec.max_order_qty = q("1000000000");
    spec.account_pos_max = sq("1000000000");
    for s in cfg.strategies.values_mut() {
        s.limits.get_mut(&iid("X")).unwrap().max = sq("1000000000");
    }
    let mut h = H::ready_with(cfg);
    // 0.00000003 * 333333333.33333333 = 9.9999999999999999 < 10
    let why = h.reject("a", "k1", Side::Buy, "333333333.33333333", "0.00000003");
    assert_eq!(why, vec![RejectReason::BelowMinNotional]);
}

// ---- F10: a documented "must" of the policy is not validated ------------------------

/// config.rs says `unmatched_fill_wait_us` "must exceed the time an in-doubt
/// order takes to resolve (expiry horizon plus absence margin), or a fill of
/// ours is booked as external". `validate` accepts any value.
#[test]
fn f10_config_validation_enforces_the_parked_fill_wait_rule() {
    let mut cfg = config();
    cfg.policy.unmatched_fill_wait_us = SEC; // horizon 60 s + margin 10 s
    assert!(cfg.validate().is_err());
}
