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
/// link and sync, an order with a fill, an operator adjustment and a halt.
fn schema_1_history(run: &mut Run) {
    run.input(Input::Boot { binary: "test".into() });
    run.input(Input::Config(Box::new(config())));
    run.venue(VenueInput::Link {
        up: true,
        session: 1,
        reason: "test".into(),
    });
    let t = run.at;
    run.venue(VenueInput::FillsSynced { session: 1, up_to: t });
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
        qty: Qty::parse("0.4").unwrap(),
        px: Px::parse("100").unwrap(),
        fee: Amount::ZERO,
        venue_ts: ts,
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
        w.append(rec.seq, &serde_json::to_vec(rec).unwrap()).unwrap();
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
        assert!(!text.contains("\"reason\":\"snapshot_after_venue_change\""), "{text}");
    }
    write(&dir, 1, &v1);

    let recovered = oms_node::recovery::recover(&dir, 1 << 30, "test").unwrap();
    assert_eq!(recovered.core.digest(), run.core.digest(), "schema 1 replays to the live state");
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
            qty: SQty::parse("0.25").unwrap(),
        }],
    });
    let v2 = run.take();
    assert!(v2.iter().any(|r| r.events.iter().any(|e| e.name() == "position_adjusted")));
    write(&dir, 2, &v2);
    assert_eq!(oms_journal::list_segments(&dir).unwrap().len(), 2, "one segment per schema");

    let recovered = oms_node::recovery::recover(&dir, 1 << 30, "test").unwrap();
    assert_eq!(recovered.core.digest(), run.core.digest(), "both schemas replay to the live state");
    drop(recovered);
    let report = oms_node::recovery::verify(&dir, true).unwrap();
    assert_eq!(report.records, (v1.len() + v2.len()) as u64);
    let _ = std::fs::remove_dir_all(&dir);
}
