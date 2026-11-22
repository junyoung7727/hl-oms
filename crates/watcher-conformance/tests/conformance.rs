//! The watcher against the OMS's own code: its `Base32Pair` against the
//! core's, its journal reader against journals that `oms-journal` writes
//! from records the core decided, and its journal state against the core's
//! sets on such journals with every event kind. This package is the only one that
//! links the watcher with OMS code (`tools/ci/watcher_independent.sh`).

#![allow(clippy::unwrap_used)]

#[path = "../../oms-core/tests/common/mod.rs"]
mod common;

use common::*;
use oms_core::*;
use oms_journal::{Error, Options, Writer, recover};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use watcher::decimal::Dec;
use watcher::journal::{MAX_RECORDS, Tail};
use watcher::state::JournalState;

fn tmp(name: &str) -> PathBuf {
    let base = std::env::var("OMS_TEST_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir());
    let d = base.join(format!("watcher-conformance-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn opts(segment_max_bytes: u64) -> Options {
    Options {
        segment_max_bytes,
        truncate_torn_tail: true,
        record_schema: 2,
        readable_schemas: vec![1, 2],
        instance_id: 7,
        writer: "watcher-conformance".into(),
    }
}

/// A journal of the records in `h.log`, every one synced. The writer (and
/// so the journal lock) is returned.
fn journal(dir: &Path, h: &H, segment_max_bytes: u64) -> Writer {
    let (mut w, _) = recover(dir, &opts(segment_max_bytes), 0, |_, _, _| Ok(())).unwrap();
    for rec in &h.log {
        w.append(rec.seq, &serde_json::to_vec(rec).unwrap())
            .unwrap();
        w.sync(rec.at).unwrap();
    }
    w
}

/// Every directory entry with its length and modification time.
fn listing(dir: &Path) -> BTreeMap<String, (u64, SystemTime)> {
    std::fs::read_dir(dir)
        .unwrap()
        .map(|e| {
            let e = e.unwrap();
            let m = e.metadata().unwrap();
            (
                e.file_name().to_string_lossy().into_owned(),
                (m.len(), m.modified().unwrap()),
            )
        })
        .collect()
}

/// xorshift64 (Marsaglia), so that the random pairs need no dependency.
struct XorShift(u64);

impl XorShift {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
}

#[test]
fn the_watcher_base32_pair_equals_the_core_form() {
    let core = |i: u64, o: u64| {
        WireIdForm::Base32Pair.encode(VenueClientId {
            instance_id: i,
            order_id: OrderId(o),
        })
    };
    let core_decode = |s: &str| {
        WireIdForm::Base32Pair
            .decode(s)
            .map(|id| (id.instance_id, id.order_id.0))
    };
    let edges = [
        0,
        1,
        31,
        32,
        u64::MAX - 1,
        u64::MAX,
        0x0123_4567_89ab_cdef,
        42,
    ];
    let mut pairs: Vec<(u64, u64)> = edges
        .iter()
        .flat_map(|&i| edges.iter().map(move |&o| (i, o)))
        .collect();
    let mut rng = XorShift(0x9e37_79b9_7f4a_7c15);
    for _ in 0..10_000 {
        pairs.push((rng.next(), rng.next()));
    }
    for (i, o) in pairs {
        let s = watcher::wire_id::encode(i, o);
        assert_eq!(s, core(i, o), "{i} {o}");
        assert_eq!(watcher::wire_id::decode(&s), Some((i, o)));
        assert_eq!(core_decode(&s), Some((i, o)));
    }
    let malformed = [
        "",
        "-",
        "0000000000000",
        "0000000000000-",
        "-0000000000000",
        "0000000000000-000000000000",
        "0000000000000-00000000000000",
        "000000000000-00000000000000",
        "0000000000000_0000000000000",
        "0000000000000--000000000000",
        "0000000000000-0000000000000-",
        "000000000000A-0000000000000",
        "0000000000000-000000000000w",
        "0000000000000-000000000000z",
        "g000000000000-0000000000000",
        "0000000000000-g000000000000",
        "vvvvvvvvvvvvv-0000000000000",
        " 000000000000-0000000000000",
        "+000000000000-0000000000000",
        "028q5cu4qnjff-000000000001a\n",
        "028Q5CU4QNJFF-000000000001A",
        "0x0123456789abcdef000000000000002a",
        "000000000000é-0000000000000",
    ];
    for bad in malformed {
        assert_eq!(watcher::wire_id::decode(bad), None, "{bad:?}");
        assert_eq!(core_decode(bad), None, "{bad:?}");
    }
}

#[test]
fn the_watcher_reads_while_the_oms_writer_holds_the_lock() {
    let dir = tmp("lock");
    let h = H::ready();
    assert!(
        h.log.len() >= 3,
        "the harness wrote {} records",
        h.log.len()
    );
    // Small segments, so that the reader also crosses rotations.
    let w = journal(&dir, &h, 512);
    let last = h.log.last().unwrap().seq;
    let before = listing(&dir);
    assert!(before.contains_key("LOCK"));
    assert!(
        before.keys().filter(|k| k.starts_with("journal-")).count() >= 2,
        "{before:?}"
    );

    let mut t = Tail::open(&dir);
    let recs = t.read(last, MAX_RECORDS).unwrap();
    assert_eq!(t.instance_id(), Some(7));
    assert_eq!(recs.len(), h.log.len());
    for (mine, theirs) in recs.iter().zip(&h.log) {
        let v: Value = serde_json::to_value(theirs).unwrap();
        assert_eq!((mine.seq, mine.at), (theirs.seq, theirs.at));
        let events: Vec<(String, Value)> = v["events"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| {
                let (k, f) = e.as_object().unwrap().iter().next().unwrap();
                (k.clone(), f.clone())
            })
            .collect();
        assert_eq!(mine.events, events, "record {}", mine.seq);
    }
    assert_eq!(t.read(last, MAX_RECORDS).unwrap(), vec![]);

    // The reader changed nothing in the directory, and the writer still
    // holds the lock.
    assert_eq!(listing(&dir), before);
    assert!(matches!(
        recover(&dir, &opts(512), 0, |_, _, _| Ok(())),
        Err(Error::Locked(_))
    ));
    drop(w);
}

// ---- the journal state against the core (U19a2) ----------------------------

fn dec(s: impl ToString) -> Dec {
    Dec::parse(&s.to_string()).unwrap()
}

fn side(s: Side) -> watcher::state::Side {
    match s {
        Side::Buy => watcher::state::Side::Buy,
        Side::Sell => watcher::state::Side::Sell,
    }
}

/// The watcher's sets against the core's (spec "Conformance"): the open
/// set; for every open order its instrument, side, quantity, strategy,
/// client order id, venue order id, filled quantity, doubt and venue end;
/// the position per instrument (0 counts as absent); the fill,
/// venue-change and funding keys at or above the fill floor with their
/// venue times; the fill floor itself.
fn same(core: &Core, st: &JournalState, at: &str) {
    let open: Vec<u64> = core.open.iter().map(|id| id.0).collect();
    assert_eq!(
        st.open.iter().copied().collect::<Vec<_>>(),
        open,
        "{at}: open set"
    );
    for id in &core.open {
        let o = &core.orders[id];
        let w = st
            .orders
            .get(&id.0)
            .unwrap_or_else(|| panic!("{at}: no info for open order {id}"));
        assert_eq!(
            (
                w.instrument.as_str(),
                w.side,
                w.qty,
                w.strategy.as_str(),
                w.client_order_id.as_str(),
                w.venue_oid.as_deref(),
            ),
            (
                o.instrument.as_str(),
                side(o.side),
                dec(o.qty),
                o.strategy.as_str(),
                o.client_order_id.as_str(),
                o.venue_oid.as_ref().map(|v| v.as_str()),
            ),
            "{at}: order {id}"
        );
        assert_eq!(w.filled, dec(o.cum_qty), "{at}: filled of order {id}");
        assert_eq!(w.in_doubt, o.doubt.is_some(), "{at}: doubt of order {id}");
        assert_eq!(
            w.terminal,
            o.terminal.is_some(),
            "{at}: terminal of order {id}"
        );
        assert!(w.open, "{at}: order {id}");
    }
    let core_pos: BTreeMap<String, Dec> = core
        .account
        .iter()
        .map(|(i, b)| (i.as_str().to_string(), dec(b.pos)))
        .filter(|(_, p)| !p.is_zero())
        .collect();
    let mine: BTreeMap<String, Dec> = st
        .positions
        .iter()
        .filter(|(_, p)| !p.is_zero())
        .map(|(i, p)| (i.clone(), *p))
        .collect();
    assert_eq!(mine, core_pos, "{at}: positions");
    let floor = core.fill_floor;
    assert_eq!(st.fill_floor, floor, "{at}: fill floor");
    let core_keys: BTreeMap<String, u64> = core
        .seen_fills
        .iter()
        .filter(|(_, t)| **t >= floor)
        .map(|(k, t)| (k.as_str().to_string(), *t))
        .collect();
    let mut keys = BTreeMap::new();
    for (k, t) in st
        .fills
        .iter()
        .map(|(k, f)| (k, f.venue_ts))
        .chain(st.venue_changes.iter().map(|(k, c)| (k, c.venue_ts)))
        .chain(st.funding.iter().map(|(k, f)| (k, f.venue_ts)))
    {
        if t >= floor {
            assert!(keys.insert(k.clone(), t).is_none(), "{at}: key {k} twice");
        }
    }
    assert_eq!(keys, core_keys, "{at}: fill keys");
}

/// Writes the scenario's records with `oms-journal` (small segments, so
/// the reader also crosses rotations), then reads them one record at a
/// time (the cap is each record in turn) and compares the watcher with a
/// core that replayed the same records. Returns the event names written.
fn conforms(name: &str, h: &H) -> BTreeSet<&'static str> {
    let dir = tmp(&format!("state-{name}"));
    let w = journal(&dir, h, 4096);
    let mut core = Core::new(h.core.instance_id);
    let mut tail = Tail::open(&dir);
    let mut st = JournalState::default();
    for rec in &h.log {
        core.replay(rec).unwrap();
        let n = st
            .read_from(&mut tail, rec.seq, MAX_RECORDS)
            .unwrap_or_else(|e| panic!("{name}: record {}: {e}", rec.seq));
        assert_eq!(n, 1, "{name}: record {}", rec.seq);
        let events: Vec<_> = rec.events.iter().map(Event::name).collect();
        same(
            &core,
            &st,
            &format!("{name} after record {} {events:?}", rec.seq),
        );
    }
    assert_eq!(core, h.core, "{name}: the replayed core is the live one");
    assert_eq!(st.instance_id, Some(7));
    drop(w);
    let _ = std::fs::remove_dir_all(&dir);
    h.log
        .iter()
        .flat_map(|r| r.events.iter().map(Event::name))
        .collect()
}

/// Orders filled and closed on both sides, a rejected request, a cancel
/// confirmed by the venue, and a partial fill then a cancel.
fn plain_fills_closes_partial_fill_and_cancel() -> H {
    let mut h = H::ready();
    h.reject("a", "bad", Side::Buy, "0.0005", "100");
    h.long("a", "k0", "1");
    let s = h.accept("b", "ks", Side::Sell, "2", "100");
    h.placed_open(s, "vs");
    h.fill(s, "ts1", "1.5", "100");
    h.fill(s, "ts2", "0.5", "100");
    h.status(s, VenueStatus::Filled, Some("2"));
    let c = h.accept("a", "kc", Side::Buy, "1", "100");
    h.placed_open(c, "vc");
    assert_eq!(h.cancel("a", c), CancelReply::Requested { order_id: c });
    h.venue(VenueInput::CancelResult {
        order_id: c,
        attempt: 1,
        outcome: CancelOutcome::Confirmed,
        timing: Default::default(),
    });
    h.status(
        c,
        VenueStatus::Canceled {
            reason: "user".into(),
        },
        Some("0"),
    );
    let p = h.accept("a", "kp", Side::Buy, "4", "100");
    h.placed_open(p, "vp");
    h.fill(p, "tp1", "1.5", "100");
    assert_eq!(h.cancel("a", p), CancelReply::Requested { order_id: p });
    h.status(
        p,
        VenueStatus::Canceled {
            reason: "user".into(),
        },
        Some("1.5"),
    );
    h
}

/// A fill booked after its order closed (FillAfterClose): its side and
/// instrument come from the kept order info.
fn fill_after_close() -> H {
    let mut h = H::ready();
    let a = h.accept("a", "k1", Side::Sell, "4", "100");
    h.placed_open(a, "v1");
    h.status(
        a,
        VenueStatus::Canceled {
            reason: "canceled".into(),
        },
        Some("0"),
    );
    assert!(h.order(a).is_closed());
    h.fill(a, "t1", "1", "100");
    assert_eq!(h.unacked_mismatches(), vec![MismatchKind::FillAfterClose]);
    h
}

/// The operator acknowledges every open mismatch (they hold plain orders).
fn ack_all(h: &mut H) {
    let ids: Vec<u32> = h
        .core
        .mismatches
        .values()
        .filter(|m| m.acked.is_none())
        .map(|m| m.id)
        .collect();
    for id in ids {
        h.op(OperatorCmd::AckMismatch {
            id,
            note: "seen".into(),
        })
        .unwrap();
    }
}

fn fill_in(key: &str, venue_oid: &str, side: Side, qty: &str, at: Ts) -> FillIn {
    FillIn {
        key: fkey(key),
        order_id: None,
        venue_oid: Some(void(venue_oid)),
        instrument: iid("X"),
        side,
        qty: q(qty),
        px: px("100"),
        fee: Amount::ZERO,
        venue_ts: at,
        is_maker: None,
        recv_ns: None,
    }
}

/// Fills parked while an own order has no venue id: one unparked as an
/// external fill, one unparked to its order; an external fill booked at
/// once.
fn external_parked_and_unparked_fills() -> H {
    let mut h = H::ready();
    let a = h.accept("a", "k1", Side::Buy, "4", "100");
    let t = h.now;
    h.venue(VenueInput::Fill(fill_in("t1", "other", Side::Buy, "1", t)));
    h.tick();
    h.placed_open(a, "v1");
    h.tick();
    ack_all(&mut h);
    let b = h.accept("b", "k2", Side::Buy, "4", "100");
    let t = h.now;
    h.venue(VenueInput::Fill(fill_in("t2", "v2", Side::Buy, "1", t)));
    h.placed_open(b, "v2");
    // No open order of ours could own it: booked as external at once.
    h.status(a, VenueStatus::Canceled { reason: "c".into() }, Some("0"));
    h.status(b, VenueStatus::Canceled { reason: "c".into() }, Some("1"));
    let t = h.now;
    h.venue(VenueInput::Fill(fill_in(
        "t3",
        "nobody",
        Side::Sell,
        "2.5",
        t,
    )));
    h
}

/// An order in doubt and then absent at the venue; a venue id learned after
/// a stream ack; a recheck after an open-order sweep; missing fills.
fn in_doubt_recheck_and_missing_fills() -> H {
    let mut cfg = config();
    cfg.policy.absence_not_found_needed = 2;
    let mut h = H::ready_with(cfg);
    let a = h.accept("a", "k1", Side::Buy, "4", "100");
    let expires_at = h.order(a).expires_at;
    h.venue(VenueInput::SendResult {
        order_id: a,
        outcome: SendOutcome::Unknown {
            reason: "timeout".into(),
        },
        timing: Default::default(),
    });
    let session = h.session;
    h.venue(VenueInput::StatusNotFound {
        session,
        order_id: a,
        queried_at: expires_at + 10 * SEC,
    });
    h.venue(VenueInput::StatusNotFound {
        session,
        order_id: a,
        queried_at: expires_at + 20 * SEC,
    });
    assert!(h.order(a).is_closed());

    let b = h.accept("a", "kb", Side::Buy, "1", "100");
    h.status(b, VenueStatus::Open, Some("0"));
    h.placed_open(b, "vb");
    let c = h.accept("a", "kc", Side::Buy, "1", "100");
    h.placed_open(c, "vc");
    let (session, queried_at) = (h.session, h.now + 1);
    h.venue(VenueInput::OpenOrders {
        session,
        queried_at,
        orders: vec![],
    });
    h.status(c, VenueStatus::Open, Some("0"));

    // Ended at the venue with fills the journal does not have.
    h.status(
        c,
        VenueStatus::Canceled {
            reason: "user".into(),
        },
        Some("0.4"),
    );
    let (session, up_to) = (h.session, h.now);
    h.venue(VenueInput::FillsSynced { session, up_to });
    h
}

/// Venue changes of each kind, funding, an unclassified execution, a
/// snapshot that agrees and one that adjusts, and the venue-change hold
/// cleared.
fn venue_changes_funding_and_unclassified() -> H {
    let mut h = H::ready();
    h.long("a", "k0", "2");
    let chg = |key: &str, kind: VenueChangeKind, qty: &str, ts: Ts| {
        VenueInput::PositionChange(VenuePositionChangeIn {
            key: fkey(key),
            kind,
            instrument: iid("X"),
            qty: sq(qty),
            px: px("100"),
            venue_ts: ts,
        })
    };
    let pos = |h: &mut H, qty: &str| {
        let (session, queried_at) = (h.session, h.now + 1);
        h.venue(VenueInput::Positions {
            session,
            queried_at,
            positions: vec![VenuePosition {
                instrument: iid("X"),
                qty: sq(qty),
            }],
        })
    };
    let ts = h.now;
    h.venue(VenueInput::Funding(FundingIn {
        key: fkey("f1"),
        instrument: iid("X"),
        amount: amt("0.25"),
        rate: "0.0001".into(),
        venue_ts: ts,
    }));
    h.venue(chg("adl1", VenueChangeKind::Adl, "-0.5", ts));
    h.venue(chg("bust1", VenueChangeKind::Takeover, "1", ts));
    h.venue(chg("dlv1", VenueChangeKind::Delivery, "-1", ts));
    h.venue(chg("s1", VenueChangeKind::Settle, "1", ts));
    h.venue(VenueInput::UnclassifiedExecution {
        key: fkey("e1"),
        instrument: iid("X"),
        exec_type: "SomethingNew".into(),
        detail: "execType SomethingNew".into(),
        venue_ts: ts,
    });
    pos(&mut h, "2.5");
    h.venue(chg("adl2", VenueChangeKind::Adl, "-0.5", ts));
    pos(&mut h, "2.2");
    h.op(OperatorCmd::ClearHold {
        reason: HoldKind::VenueChange,
        note: "ADL seen at the venue".into(),
    })
    .unwrap();
    h
}

/// Operator halt and resume, storage low, a reference price, an operator
/// adjustment, a mismatch and its ack, a strategy link lost and restored,
/// a second configuration.
fn operator_storage_mismatch_and_links() -> H {
    let mut h = H::ready();
    h.inp(Input::Storage {
        low: true,
        free_bytes: 1,
    });
    h.inp(Input::Storage {
        low: false,
        free_bytes: 1 << 40,
    });
    h.op(OperatorCmd::Halt {
        scope: Scope::Account,
        note: "t".into(),
    })
    .unwrap();
    h.op(OperatorCmd::Resume {
        scope: Scope::Account,
        note: "t".into(),
    })
    .unwrap();
    h.ref_price("X", "100");
    h.op(OperatorCmd::AdjustPosition {
        instrument: iid("X"),
        ledger: Ledger::Unattributed,
        delta: sq("-1.5"),
        note: "manual trade on venue UI".into(),
    })
    .unwrap();
    let (session, queried_at) = (h.session, h.now + 1);
    h.venue(VenueInput::OpenOrders {
        session,
        queried_at,
        orders: vec![VenueOpenOrder {
            venue_oid: void("m1"),
            order_id: None,
            instrument: iid("X"),
            side: Side::Buy,
            remaining: Some(q("1")),
        }],
    });
    let id = *h.core.mismatches.keys().next().unwrap();
    h.op(OperatorCmd::AckMismatch {
        id,
        note: "cancelled by hand".into(),
    })
    .unwrap();
    h.inp(Input::Session(SessionInput::Lost {
        strategy: sid("a"),
        why: "connection closed".into(),
    }));
    h.inp(Input::Session(SessionInput::Hello { strategy: sid("a") }));
    let mut cfg = config();
    cfg.version = 2;
    let out = h.inp(Input::Config(Box::new(cfg)));
    assert!(
        matches!(out.reply, Some(Reply::Config(Ok(_)))),
        "{:?}",
        out.reply
    );
    h
}

/// The kill latch from the watcher, its loop end, a rearm and the clear;
/// the watcher-mismatch hold set and cleared; a kill marker at a start.
fn holds_kill_loop_and_rearm() -> H {
    let mut h = H::ready();
    let lend = |h: &mut H| {
        h.venue(VenueInput::KillLoopEnded {
            outcome: KillOutcome::Confirmed,
            rounds: 3,
            cancel_all_calls: 3,
            detail: "zero open orders for 30 s".into(),
        });
    };
    h.inp(Input::Watcher(WatcherInput::Kill {
        detail: "first".into(),
    }));
    lend(&mut h);
    h.advance(SEC);
    h.inp(Input::Watcher(WatcherInput::Kill {
        detail: "second".into(),
    }));
    lend(&mut h);
    h.op(OperatorCmd::ClearHold {
        reason: HoldKind::Kill,
        note: "venue checked".into(),
    })
    .unwrap();
    h.inp(Input::Watcher(WatcherInput::Mismatch {
        check: WatcherCheck::W2,
        detail: "d".into(),
    }));
    h.op(OperatorCmd::ClearHold {
        reason: HoldKind::WatcherMismatch,
        note: "checked".into(),
    })
    .unwrap();
    // A restart with a kill marker: booted, then the latch.
    h.inp(Input::Boot {
        binary: "test".into(),
        kill_marker: Some("marker: W5 killed".into()),
    });
    h
}

/// Margin: venue settings and account mode, reports, a refused report, the
/// day base, reducing entered and cleared.
fn margin_reports_day_base_and_reducing() -> H {
    let mut h = H::ready_with(config_with_margin());
    h.report_margin("1000");
    let (session, queried_at) = (h.session, h.now);
    h.venue(VenueInput::MarginReportInvalid {
        session,
        seq: 99,
        queried_at,
        reason: "marginSummary.accountValue: not a number".into(),
    });
    h.report_margin("900");
    h.op(OperatorCmd::ClearReduce {
        note: "loss understood".into(),
    })
    .unwrap();
    h.venue_setting("X", false, 1);
    h.account_mode(STANDARD_MODE);
    h
}

/// The monitoring gate: a fresh heartbeat opens it, silence closes it.
fn monitor_heartbeat_and_change() -> H {
    let mut cfg = config();
    cfg.policy.monitor = Some(MonitorPolicy::default());
    let mut h = H::configured(cfg);
    h.link_up_and_sync();
    let t = h.now + 1_000;
    let checks: BTreeMap<String, Option<Ts>> =
        [("W1".to_string(), Some(t)), ("W2".to_string(), Some(t))].into();
    h.inp(Input::Watcher(WatcherInput::Heartbeat {
        watcher: "watcher-1".into(),
        checks,
    }));
    h.advance(31 * SEC);
    h.tick();
    h
}

/// Purges of closed orders and old keys, and a restart whose boot raises
/// the fill floor; fills before and after.
fn purge_and_restart() -> H {
    let mut cfg = config();
    cfg.policy.closed_retention_us = SEC;
    cfg.policy.dedup_grace_us = SEC;
    cfg.policy.purge_interval_us = SEC;
    cfg.policy.fill_dedup_retention_us = SEC;
    let mut h = H::ready_with(cfg);
    h.long("a", "k0", "1");
    let a = h.accept("a", "k1", Side::Sell, "4", "100");
    h.placed_open(a, "v1");
    h.fill(a, "t1", "1", "100");
    h.status(
        a,
        VenueStatus::Canceled {
            reason: "canceled".into(),
        },
        Some("1"),
    );
    let t = h.now;
    h.venue(VenueInput::Fill(fill_in(
        "x1",
        "nobody",
        Side::Buy,
        "0.5",
        t,
    )));
    h.venue(VenueInput::Funding(FundingIn {
        key: fkey("f1"),
        instrument: iid("X"),
        amount: amt("0.1"),
        rate: "0.0001".into(),
        venue_ts: t,
    }));
    ack_all(&mut h);
    let open = h.accept("b", "k2", Side::Buy, "1", "100");
    h.placed_open(open, "v2");
    h.advance(100 * SEC);
    h.link_up_and_sync();
    h.tick();
    assert!(h.core.order(a).is_none(), "the closed order was purged");
    assert!(h.core.open.contains(&open), "the open order was not");
    h.status(
        open,
        VenueStatus::Canceled { reason: "c".into() },
        Some("0"),
    );
    h.advance(100 * SEC);
    h.inp(Input::Boot {
        binary: "test".into(),
        kill_marker: None,
    });
    h.link_up_and_sync();
    h.long("a", "k3", "1");
    h.advance(100 * SEC);
    h.tick();
    // The floor passed the old keys, so the comparison saw keys forgotten.
    assert!(h.core.fill_floor > t, "{} {t}", h.core.fill_floor);
    assert_eq!(
        h.core
            .seen_fills
            .keys()
            .map(|k| k.as_str())
            .collect::<Vec<_>>(),
        vec!["k3"]
    );
    h
}

/// The event names of the C-TAIL field table, read from the contract at
/// test time.
fn table_names() -> BTreeSet<String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/contracts/tail.md");
    let text = std::fs::read_to_string(&path).unwrap();
    let begin = text.find("<!-- event-fields:begin -->").unwrap();
    let end = text.find("<!-- event-fields:end -->").unwrap();
    text[begin..end]
        .lines()
        .filter_map(|l| l.strip_prefix("| `"))
        .filter_map(|l| l.split_once('`').map(|(n, _)| n.to_string()))
        .collect()
}

/// Drives the core through the harness and returns it with its records.
type Scenario = fn() -> H;

#[test]
fn the_watcher_sets_equal_the_core_on_journals_with_every_event_kind() {
    let scenarios: [(&str, Scenario); 10] = [
        ("plain", plain_fills_closes_partial_fill_and_cancel),
        ("after-close", fill_after_close),
        ("parked", external_parked_and_unparked_fills),
        ("doubt", in_doubt_recheck_and_missing_fills),
        ("venue-changes", venue_changes_funding_and_unclassified),
        ("operator", operator_storage_mismatch_and_links),
        ("holds", holds_kill_loop_and_rearm),
        ("margin", margin_reports_day_base_and_reducing),
        ("monitor", monitor_heartbeat_and_change),
        ("purge", purge_and_restart),
    ];
    let mut written = BTreeSet::new();
    for (name, run) in scenarios {
        written.extend(conforms(name, &run()));
    }
    let table = table_names();
    assert!(table.len() > 40, "the table was not read: {table:?}");
    let written: BTreeSet<String> = written.iter().map(|s| s.to_string()).collect();
    let missing: Vec<_> = table.difference(&written).collect();
    assert!(missing.is_empty(), "no scenario writes: {missing:?}");
    let extra: Vec<_> = written.difference(&table).collect();
    assert!(extra.is_empty(), "written but not in the table: {extra:?}");
}
