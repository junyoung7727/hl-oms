//! Record schema 2 (U13b): a journal written with record schema 1 is still
//! read, and records of both schemas replay to the live state.

#![allow(clippy::unwrap_used)]

use oms_core::record::{READABLE_RECORD_SCHEMAS, RECORD_SCHEMA};
use oms_core::*;
use serde_json::json;
use std::path::PathBuf;

const INSTANCE: u64 = 7;
const SEC: u64 = 1_000_000;

fn data_dir(name: &str) -> PathBuf {
    let base = std::env::var("OMS_TEST_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_TARGET_TMPDIR")));
    let dir = base.join(format!("schema-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("INSTANCE"), format!("{INSTANCE:016x}\n")).unwrap();
    dir
}

fn config() -> CoreConfig {
    serde_json::from_value(json!({
        "version": 1,
        "instruments": {"X": {
            "qty_step": "0.001", "px_tick": "0.1", "px_max_sig_figs": null,
            "min_notional": "0", "max_order_qty": "100", "max_order_notional": "100000000",
            "price_band_bps": null, "account_pos_min": "-1000", "account_pos_max": "1000"
        }},
        "strategies": {"a": {"enabled": true, "max_open_orders": 10, "limits": {"X": {"min": "-1000", "max": "1000"}}}},
        "policy": {}
    }))
    .unwrap()
}

fn iid(s: &str) -> InstrumentId {
    InstrumentId::new(s).unwrap()
}

fn fkey(s: &str) -> FillKey {
    FillKey::new(s).unwrap()
}

/// A live core and the records it produced, numbered from `next_seq`.
struct Run {
    core: Core,
    at: Ts,
    records: Vec<Record>,
    next_seq: u64,
}

impl Run {
    fn input(&mut self, input: Input) -> Output {
        self.at += SEC;
        let out = self.core.handle(self.at, &input).unwrap();
        if !out.events.is_empty() {
            self.records.push(Record {
                seq: self.next_seq,
                at: self.at,
                input,
                events: out.events.clone(),
            });
            self.next_seq += 1;
        }
        out
    }

    fn venue(&mut self, v: VenueInput) -> Output {
        self.input(Input::Venue(v))
    }

    fn take(&mut self) -> Vec<Record> {
        std::mem::take(&mut self.records)
    }
}

/// Inputs whose events all exist in record schema 1: boot, configuration,
/// link and sync, an order filled in full, an operator adjustment and a
/// halt.
fn schema_1_history(run: &mut Run) {
    run.input(Input::Boot {
        binary: "test".into(),
        kill_marker: None,
    });
    run.input(Input::Config(Box::new(config())));
    run.venue(VenueInput::Link {
        up: true,
        session: 1,
        reason: "test".into(),
    });
    let t = run.at;
    run.venue(VenueInput::FillsSynced {
        session: 1,
        up_to: t,
    });
    run.venue(VenueInput::OpenOrders {
        session: 1,
        queried_at: t,
        orders: vec![],
    });
    run.venue(VenueInput::Positions {
        session: 1,
        queried_at: t,
        positions: vec![],
    });
    let out = run.input(Input::NewOrder(NewOrderReq {
        strategy: StrategyId::new("a").unwrap(),
        client_order_id: ClientOrderId::new("k1").unwrap(),
        instrument: iid("X"),
        side: Side::Buy,
        qty: Qty::parse("1").unwrap(),
        px: Px::parse("100").unwrap(),
        tif: Tif::Gtc,
        reduce_only: false,
        expires_at: run.at + 10 * SEC,
        market_ref: None,
    }));
    let Some(Reply::NewOrder(NewOrderReply::Accepted { order_id })) = out.reply else {
        panic!("not accepted: {:?}", out.reply)
    };
    run.venue(VenueInput::SendResult {
        order_id,
        outcome: SendOutcome::Placed {
            venue_oid: VenueOrderId::new("v1").unwrap(),
            status: StatusIn {
                status: VenueStatus::Open,
                cum: None,
                venue_ts: run.at,
            },
        },
        timing: GatewayTiming::default(),
    });
    let ts = run.at;
    run.venue(VenueInput::Fill(FillIn {
        key: fkey("t1"),
        order_id: Some(order_id),
        venue_oid: None,
        instrument: iid("X"),
        side: Side::Buy,
        qty: Qty::parse("1").unwrap(),
        px: Px::parse("100").unwrap(),
        fee: Amount::ZERO,
        venue_ts: ts,
        is_maker: None,
        recv_ns: None,
    }));
    run.input(Input::Operator(OperatorCmd::AdjustPosition {
        instrument: iid("X"),
        ledger: Ledger::Unattributed,
        delta: SQty::parse("0.1").unwrap(),
        note: "test".into(),
    }));
    run.input(Input::Operator(OperatorCmd::Halt {
        scope: Scope::Account,
        note: "test".into(),
    }));
}

fn write(dir: &std::path::Path, record_schema: u16, records: &[Record]) {
    let mut opts = oms_node::recovery::journal_options(INSTANCE, 1 << 30, "test");
    opts.record_schema = record_schema;
    let after = records.first().map(|r| r.seq - 1).unwrap_or(0);
    let (mut w, _) = oms_journal::recover(dir, &opts, after, |_, _, _| Ok(())).unwrap();
    for rec in records {
        w.append(rec.seq, &serde_json::to_vec(rec).unwrap())
            .unwrap();
    }
    w.sync(1).unwrap();
}

#[test]
fn this_build_writes_schema_2_and_reads_1_and_2() {
    assert_eq!(RECORD_SCHEMA, 2);
    assert_eq!(READABLE_RECORD_SCHEMAS, [1, 2]);
}

#[test]
fn a_schema_1_journal_and_a_schema_2_tail_replay_to_the_live_state() {
    let dir = data_dir("v1-then-v2");
    let mut run = Run {
        core: Core::new(INSTANCE),
        at: 1_000 * SEC,
        records: Vec::new(),
        next_seq: 1,
    };
    schema_1_history(&mut run);
    let v1 = run.take();
    for rec in &v1 {
        let text = serde_json::to_string(rec).unwrap();
        assert!(
            !text.contains("\"reason\":\"snapshot_after_venue_change\""),
            "{text}"
        );
    }
    // Two writers of schema 1 share one segment, so the second segment
    // below is opened by the schema change and not by each writer.
    let (head, tail) = v1.split_at(v1.len() / 2);
    write(&dir, 1, head);
    write(&dir, 1, tail);
    assert_eq!(oms_journal::list_segments(&dir).unwrap().len(), 1);

    let recovered = oms_node::recovery::recover(&dir, 1 << 30, "test").unwrap();
    assert_eq!(
        recovered.core.digest(),
        run.core.digest(),
        "schema 1 replays to the live state"
    );
    assert_eq!(recovered.core, run.core);
    drop(recovered);

    // Schema-2 records after the schema-1 ones: a venue change and the
    // positions answer that settles it. The writer opens a new segment.
    let t = run.at;
    run.venue(VenueInput::PositionChange(VenuePositionChangeIn {
        key: fkey("adl1"),
        kind: VenueChangeKind::Adl,
        instrument: iid("X"),
        qty: SQty::parse("-0.2").unwrap(),
        px: Px::parse("100").unwrap(),
        venue_ts: t,
    }));
    let after = run.at + 1;
    run.venue(VenueInput::Positions {
        session: 1,
        queried_at: after,
        positions: vec![VenuePosition {
            instrument: iid("X"),
            qty: SQty::parse("0.85").unwrap(),
        }],
    });
    let v2 = run.take();
    assert!(
        v2.iter()
            .any(|r| r.events.iter().any(|e| e.name() == "position_adjusted"))
    );
    write(&dir, 2, &v2);
    assert_eq!(
        oms_journal::list_segments(&dir).unwrap().len(),
        2,
        "one segment per schema"
    );

    let recovered = oms_node::recovery::recover(&dir, 1 << 30, "test").unwrap();
    assert_eq!(
        recovered.core.digest(),
        run.core.digest(),
        "both schemas replay to the live state"
    );
    drop(recovered);
    let report = oms_node::recovery::verify(&dir, true).unwrap();
    assert_eq!(report.records, (v1.len() + v2.len()) as u64);
    let _ = std::fs::remove_dir_all(&dir);
}

/// U17d. The account numbers of a margin report (`account_mm`) and the
/// policy's `account_mm_rate_max_bps` are optional fields that are not
/// written when absent, so the record schema stays 2: a schema-2 journal
/// written before them reads unchanged. A journal whose reports carry them
/// and whose latch is `account_mm` recovers to the live state, beside one
/// whose reports do not (Hyperliquid), and `verify --re-decide` passes.
#[test]
fn margin_reports_with_and_without_account_mm_recover_to_the_live_state() {
    assert_eq!(RECORD_SCHEMA, 2, "no record schema rise for an optional field");
    for limit in [None, Some(5000)] {
        let dir = data_dir(&format!("account-mm-{limit:?}"));
        let mut run = Run {
            core: Core::new(INSTANCE),
            at: 1_000 * SEC,
            records: Vec::new(),
            next_seq: 1,
        };
        let mut cfg = config();
        cfg.policy.margin = Some(MarginPolicy {
            account_mm_rate_max_bps: limit,
            ..MarginPolicy::default()
        });
        run.input(Input::Boot {
            binary: "test".into(),
            kill_marker: None,
        });
        run.input(Input::Config(Box::new(cfg)));
        run.venue(VenueInput::Link {
            up: true,
            session: 1,
            reason: "test".into(),
        });
        let numbers = |maintenance: &str| {
            limit.map(|_| AccountMm {
                maintenance: Amount::parse(maintenance).unwrap(),
                margin_balance: Amount::parse("1000").unwrap(),
            })
        };
        for (seq, maintenance) in [(1, "499.99"), (2, "500")] {
            let queried_at = run.at;
            run.venue(VenueInput::MarginReport(MarginReport {
                session: 1,
                seq,
                queried_at,
                account_value: Amount::parse("100").unwrap(),
                positions: vec![],
                account_mm: numbers(maintenance),
            }));
        }
        let records = run.take();
        let text = serde_json::to_string(&records).unwrap();
        let expected_rule = limit.map(|_| RuleId::AccountMm);
        assert_eq!(
            run.core.margin.reducing.as_ref().map(|r| r.rule),
            expected_rule
        );
        assert_eq!(text.contains("account_mm"), limit.is_some(), "{text}");
        write(&dir, RECORD_SCHEMA, &records);
        let recovered = oms_node::recovery::recover(&dir, 1 << 30, "test").unwrap();
        assert_eq!(recovered.core.digest(), run.core.digest());
        assert_eq!(recovered.core, run.core);
        drop(recovered);
        let report = oms_node::recovery::verify(&dir, true).unwrap();
        assert_eq!(report.records, records.len() as u64);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// U18b1. The kill marker in `Boot`, the watcher's inputs, `hold_set`,
/// `hold_cleared`, `kill_loop_ended` and the three holds of C-HOLD in the
/// state are optional fields and new kinds, not a schema rise: a journal
/// with them recovers to the live state and `verify --re-decide` passes,
/// and a state without them keeps the bytes it had.
#[test]
fn kill_holds_liveness_and_heartbeats_recover_to_the_live_state() {
    assert_eq!(RECORD_SCHEMA, 2, "no record schema rise for new kinds and optional fields");
    let dir = data_dir("holds");
    let mut run = Run {
        core: Core::new(INSTANCE),
        at: 1_000 * SEC,
        records: Vec::new(),
        next_seq: 1,
    };
    let plain = {
        let mut c = Core::new(INSTANCE);
        c.handle(SEC, &Input::Boot { binary: "test".into(), kill_marker: None }).unwrap();
        serde_json::to_string(&c).unwrap()
    };
    for field in ["\"kill\"", "\"watcher_mismatch\"", "\"venue_change\"", "kill_marker"] {
        assert!(!plain.contains(field), "{field} written while unset: {plain}");
    }
    run.input(Input::Boot {
        binary: "test".into(),
        kill_marker: Some("marker: W5".into()),
    });
    run.input(Input::Config(Box::new(config())));
    run.venue(VenueInput::Link {
        up: true,
        session: 1,
        reason: "test".into(),
    });
    run.venue(VenueInput::KillLoopEnded {
        outcome: KillOutcome::Unsupported,
        rounds: 0,
        cancel_all_calls: 0,
        detail: "no cancel-all".into(),
    });
    run.input(Input::Watcher(WatcherInput::Mismatch {
        check: WatcherCheck::W1,
        detail: "fills differ".into(),
    }));
    let t = run.at;
    run.venue(VenueInput::UnclassifiedExecution {
        key: fkey("e1"),
        instrument: iid("X"),
        exec_type: "Odd".into(),
        detail: "d".into(),
        venue_ts: t,
    });
    for reason in [HoldKind::Kill, HoldKind::WatcherMismatch, HoldKind::VenueChange] {
        let out = run.input(Input::Operator(OperatorCmd::ClearHold {
            reason,
            note: "checked".into(),
        }));
        assert_eq!(out.reply, Some(Reply::Operator(Ok("cleared".into()))), "{reason:?}");
    }
    // A second latch, left set, so the recovered state holds one.
    run.input(Input::Watcher(WatcherInput::Kill { detail: "W5 again".into() }));
    assert!(run.core.kill.is_some());
    let records = run.take();
    let names: Vec<&str> = records.iter().flat_map(|r| r.events.iter().map(Event::name)).collect();
    for kind in ["hold_set", "hold_cleared", "kill_loop_ended"] {
        assert!(names.contains(&kind), "{kind}: {names:?}");
    }
    write(&dir, RECORD_SCHEMA, &records);
    let recovered = oms_node::recovery::recover(&dir, 1 << 30, "test").unwrap();
    assert_eq!(recovered.core.digest(), run.core.digest());
    assert_eq!(recovered.core, run.core);
    drop(recovered);
    let report = oms_node::recovery::verify(&dir, true).unwrap();
    assert_eq!(report.records, records.len() as u64);
    let _ = std::fs::remove_dir_all(&dir);
}

/// U18b1, review S3. A snapshot written by a build before U18b (state
/// schema 1) holds no `venue_change`, although a full replay of the same
/// journal latches it from the venue change. This build reads state schema
/// 2 only: the old snapshot is skipped with its reason and the journal is
/// replayed from the start, so the hold does not depend on whether a
/// snapshot happened to exist.
#[test]
fn a_snapshot_of_state_schema_1_is_skipped_and_the_journal_replays_the_venue_change() {
    assert_eq!(oms_node::pipeline::STATE_SCHEMA, 2);
    let dir = data_dir("state-1");
    let mut run = Run {
        core: Core::new(INSTANCE),
        at: 1_000 * SEC,
        records: Vec::new(),
        next_seq: 1,
    };
    run.input(Input::Boot { binary: "old".into(), kill_marker: None });
    run.input(Input::Config(Box::new(config())));
    run.venue(VenueInput::Link { up: true, session: 1, reason: "test".into() });
    let t = run.at;
    run.venue(VenueInput::UnclassifiedExecution {
        key: fkey("e1"),
        instrument: iid("X"),
        exec_type: "Odd".into(),
        detail: "d".into(),
        venue_ts: t,
    });
    assert!(run.core.venue_change.is_some());
    let records = run.take();
    write(&dir, RECORD_SCHEMA, &records);

    // What the old build saved after the change: the same state without
    // the hold, with a digest that matches it.
    let mut old = run.core.clone();
    old.venue_change = None;
    let payload = serde_json::to_vec(&oms_node::pipeline::SnapshotState { core: old.clone(), last_at: run.at }).unwrap();
    let info = oms_journal::SnapshotInfo {
        format: 0,
        state_schema: 1,
        seq: records.last().unwrap().seq,
        len: 0,
        crc32: 0,
        created_at_us: 1,
        writer: "old".into(),
        state_digest: old.digest(),
    };
    oms_journal::write_snapshot(&dir, info, &payload).unwrap();

    let recovered = oms_node::recovery::recover(&dir, 1 << 30, "test").unwrap();
    assert_eq!(recovered.report.snapshot_seq, None);
    assert_eq!(recovered.report.snapshots_skipped.len(), 1);
    assert!(
        recovered.report.snapshots_skipped[0].1.contains("state schema 1 (this build reads 2)"),
        "{:?}",
        recovered.report.snapshots_skipped
    );
    assert_eq!(recovered.report.records_replayed, records.len() as u64);
    assert!(recovered.core.venue_change.is_some(), "the hold of the full replay");
    assert_eq!(recovered.core, run.core);
    drop(recovered);
    let _ = std::fs::remove_dir_all(&dir);
}
