//! The holds of C-HOLD (U18b1): the kill latch, the watcher's mismatch and
//! the venue's position change, how each is set, what it refuses, and how
//! only the operator's `clear-hold` releases it.

#![allow(clippy::unwrap_used)]

mod common;
use common::*;
use oms_core::*;

fn names(out: &Output) -> Vec<&'static str> {
    out.events.iter().map(Event::name).collect()
}

fn watcher(h: &mut H, w: WatcherInput) -> (Output, Result<String, String>) {
    let out = h.inp(Input::Watcher(w));
    let reply = match &out.reply {
        Some(Reply::Operator(r)) => r.clone(),
        other => panic!("unexpected reply {other:?}"),
    };
    (out, reply)
}

fn watcher_kill(h: &mut H, detail: &str) -> (Output, Result<String, String>) {
    watcher(
        h,
        WatcherInput::Kill {
            detail: detail.into(),
        },
    )
}

fn watcher_mismatch(h: &mut H, detail: &str) -> (Output, Result<String, String>) {
    watcher(
        h,
        WatcherInput::Mismatch {
            check: WatcherCheck::W2,
            detail: detail.into(),
        },
    )
}

fn kill_loop_ended(h: &mut H, outcome: KillOutcome) -> Output {
    h.venue(VenueInput::KillLoopEnded {
        outcome,
        rounds: 3,
        cancel_all_calls: 3,
        detail: "zero open orders for 30 s".into(),
    })
}

fn clear(h: &mut H, reason: HoldKind, note: &str) -> (Output, Result<String, String>) {
    let out = h.inp(Input::Operator(OperatorCmd::ClearHold {
        reason,
        note: note.into(),
    }));
    let reply = match &out.reply {
        Some(Reply::Operator(r)) => r.clone(),
        other => panic!("unexpected reply {other:?}"),
    };
    (out, reply)
}

fn unclassified(h: &mut H, key: &str) -> Output {
    let ts = h.now;
    h.venue(VenueInput::UnclassifiedExecution {
        key: fkey(key),
        instrument: iid("X"),
        exec_type: "SomethingNew".into(),
        detail: "execType SomethingNew".into(),
        venue_ts: ts,
    })
}

fn venue_change(h: &mut H, key: &str, qty: &str) -> Output {
    let ts = h.now;
    h.venue(VenueInput::PositionChange(VenuePositionChangeIn {
        key: fkey(key),
        kind: VenueChangeKind::Adl,
        instrument: iid("X"),
        qty: sq(qty),
        px: px("100"),
        venue_ts: ts,
    }))
}

fn kills(h: &H) -> Vec<VenueCommand> {
    h.commands
        .iter()
        .filter(|c| matches!(c, VenueCommand::Kill { .. } | VenueCommand::KillStop))
        .cloned()
        .collect()
}

/// A margin policy, a fresh report, a reference price and a long of 2 for
/// strategy `a`: a plain order and a reduce-only sell both pass.
fn trading() -> H {
    let mut h = H::ready_with(config_with_margin());
    h.report_margin("1000");
    h.ref_price("X", "100");
    h.long("a", "k0", "2");
    h
}

/// Whether a plain buy and a reduce-only sell pass now, and the reasons of
/// the plain one if it is refused.
fn plain_and_reduce_only(h: &mut H, key: &str) -> (Vec<RejectReason>, bool) {
    h.report_margin("1000");
    let plain = match h.new_order(h.req("a", &format!("{key}-p"), Side::Buy, "0.1", "100")) {
        NewOrderReply::Rejected { reasons } => reasons,
        NewOrderReply::Accepted { .. } => vec![],
        other => panic!("{other:?}"),
    };
    let r = h.reduce_only_req("a", &format!("{key}-r"), Side::Sell, "0.5", "100");
    let reduce_only_passes = match h.new_order(r) {
        NewOrderReply::Accepted { .. } => true,
        NewOrderReply::Rejected { .. } => false,
        other => panic!("{other:?}"),
    };
    (plain, reduce_only_passes)
}

/// TN table 7-7 with a margin policy: under the watcher's mismatch and the
/// venue change (and, U18b3, a lost strategy link) a reduce-only order
/// passes and a plain one is refused;
/// under the kill latch and the operator's halt both are refused.
#[test]
fn the_hold_table_of_table_7_7() {
    type Set = fn(&mut H);
    let rows: [(HoldReason, Set, bool); 5] = [
        (
            HoldReason::KillLatched,
            |h| {
                watcher_kill(h, "W5: the OMS is silent").1.unwrap();
            },
            false,
        ),
        (
            HoldReason::OperatorAccount,
            |h| {
                h.op(OperatorCmd::Halt {
                    scope: Scope::Account,
                    note: "n".into(),
                })
                .unwrap();
            },
            false,
        ),
        (
            HoldReason::WatcherMismatch,
            |h| {
                watcher_mismatch(h, "positions differ").1.unwrap();
            },
            true,
        ),
        (
            HoldReason::VenueChange,
            |h| {
                unclassified(h, "e1");
            },
            true,
        ),
        // U18b3 (C-LIVE): the hold of strategy `a`, whose orders these are.
        (
            HoldReason::StrategyLink,
            |h| {
                h.inp(Input::Session(SessionInput::Lost {
                    strategy: sid("a"),
                    why: "last trading connection closed".into(),
                }));
            },
            true,
        ),
    ];
    for (reason, set, reduce_only_passes) in rows {
        let mut h = trading();
        let (plain, passes) = plain_and_reduce_only(&mut h, "before");
        assert_eq!(
            (plain, passes),
            (vec![], true),
            "{reason:?}: nothing holds yet"
        );
        set(&mut h);
        assert!(h.core.holds(Some(&sid("a"))).contains(&reason), "{reason:?}");
        let (plain, passes) = plain_and_reduce_only(&mut h, "after");
        assert!(
            plain.contains(&RejectReason::Hold(reason.clone())),
            "{reason:?}: {plain:?}"
        );
        assert_eq!(passes, reduce_only_passes, "{reason:?}: reduce-only");
        assert_eq!(
            reason.refuses_only_plain_orders(),
            reduce_only_passes,
            "{reason:?}"
        );
    }

    // Without a margin policy every hold refuses every order, as before.
    let mut h = H::ready();
    h.long("a", "k0", "2");
    watcher_mismatch(&mut h, "d").1.unwrap();
    let r = h.reduce_only_req("a", "k1", Side::Sell, "0.5", "100");
    assert_eq!(
        h.new_order(r),
        NewOrderReply::Rejected {
            reasons: vec![RejectReason::Hold(HoldReason::WatcherMismatch)]
        }
    );
}

/// The holds of C-HOLD come after the others, so that an exact list of
/// the earlier ones keeps its order.
#[test]
fn the_new_holds_come_last_in_their_order() {
    let mut h = H::ready();
    h.op(OperatorCmd::Halt {
        scope: Scope::Account,
        note: "n".into(),
    })
    .unwrap();
    unclassified(&mut h, "e1");
    watcher_mismatch(&mut h, "d").1.unwrap();
    watcher_kill(&mut h, "k").1.unwrap();
    assert_eq!(
        h.core.holds(None),
        vec![
            HoldReason::Mismatch,
            HoldReason::OperatorAccount,
            HoldReason::KillLatched,
            HoldReason::WatcherMismatch,
            HoldReason::VenueChange,
        ]
    );
}

/// Each refusal code of `clear-hold`, in the order of the table; a refusal
/// writes nothing and sends nothing.
#[test]
fn clear_hold_refusals() {
    let mut h = H::ready();
    let refused = |h: &mut H, reason: HoldKind, note: &str, code: &str| {
        let records = h.log.len();
        let commands = h.commands.len();
        let (out, reply) = clear(h, reason, note);
        let text = reply.expect_err("refused");
        assert!(text.starts_with(&format!("{code}:")), "{reason:?}: {text}");
        assert!(out.events.is_empty(), "{reason:?}: {:?}", names(&out));
        assert_eq!((h.log.len(), h.commands.len()), (records, commands));
        text
    };
    for reason in [
        HoldKind::Kill,
        HoldKind::WatcherMismatch,
        HoldKind::VenueChange,
    ] {
        refused(&mut h, reason, "n", "not_held");
    }

    watcher_kill(&mut h, "k").1.unwrap();
    refused(&mut h, HoldKind::Kill, "", "note_required");
    refused(&mut h, HoldKind::Kill, "  \t", "note_required");
    let text = refused(&mut h, HoldKind::Kill, "n", "kill_unconfirmed");
    assert!(
        text.contains("the kill loop has not confirmed zero open orders in this process"),
        "{text}"
    );
    kill_loop_ended(&mut h, KillOutcome::Confirmed);
    let (out, reply) = clear(&mut h, HoldKind::Kill, "venue checked by hand");
    assert_eq!(reply, Ok("cleared".to_string()));
    assert_eq!(names(&out), ["hold_cleared"]);
    assert_eq!(out.commands, [VenueCommand::KillStop]);
    assert!(!h.core.holds(None).contains(&HoldReason::KillLatched));

    // A venue change: refused until the venue's position after it was read.
    venue_change(&mut h, "adl1", "-0.5");
    refused(&mut h, HoldKind::VenueChange, "", "note_required");
    refused(&mut h, HoldKind::VenueChange, "n", "venue_change_pending");
    let session = h.session;
    let after = h.now + 1;
    h.venue(VenueInput::Positions {
        session,
        queried_at: after,
        positions: vec![VenuePosition {
            instrument: iid("X"),
            qty: sq("-0.5"),
        }],
    });
    assert!(h.core.venue_change_wait.is_empty());
    let (out, reply) = clear(&mut h, HoldKind::VenueChange, "ADL seen at the venue");
    assert_eq!(reply, Ok("cleared".to_string()));
    assert_eq!(names(&out), ["hold_cleared"]);
    assert!(out.commands.is_empty());
    // The mismatch entry stays until it is acknowledged (two steps).
    assert_eq!(h.core.holds(None), vec![HoldReason::Mismatch]);

    watcher_mismatch(&mut h, "d").1.unwrap();
    refused(&mut h, HoldKind::WatcherMismatch, "", "note_required");
    assert_eq!(
        clear(&mut h, HoldKind::WatcherMismatch, "checked").1,
        Ok("cleared".to_string())
    );
    assert_eq!(h.rebuilt(), h.core);
}

/// C-HOLD (amends the U18b1 spec, review S1): `clear-hold kill` is refused
/// while the core's own book has open orders, whatever the kill loop
/// answered, and after the confirmation check. `cancel-all` is taken under
/// the latch (cancels are not new orders); once the venue has closed the
/// orders the clear is taken.
#[test]
fn a_kill_is_not_cleared_while_the_book_has_open_orders() {
    for outcome in [KillOutcome::Unsupported, KillOutcome::Confirmed] {
        let mut h = H::ready();
        let a = h.accept("a", "k1", Side::Buy, "1", "100");
        h.placed_open(a, "v1");
        let b = h.accept("a", "k2", Side::Sell, "1", "110");
        h.placed_open(b, "v2");
        watcher_kill(&mut h, "k").1.unwrap();
        let refused = |h: &mut H, code: &str| {
            let records = h.log.len();
            let (out, reply) = clear(h, HoldKind::Kill, "venue checked");
            let text = reply.expect_err("refused");
            assert!(text.starts_with(&format!("{code}:")), "{outcome:?}: {text}");
            assert!(out.events.is_empty() && out.commands.is_empty());
            assert_eq!(h.log.len(), records);
            text
        };
        // The confirmation is checked first.
        refused(&mut h, "kill_unconfirmed");
        kill_loop_ended(&mut h, outcome);
        let text = refused(&mut h, "kill_orders_open");
        assert!(
            text.contains("2 open orders") && text.contains("cancel-all"),
            "{text}"
        );

        // cancel-all is taken under the latch and asks the venue once per
        // order; until the venue answers, the orders are still open.
        assert_eq!(
            h.op(OperatorCmd::CancelAll {
                scope: Scope::Account,
                note: "kill".into(),
            }),
            Ok("cancel requested for 2 orders".to_string())
        );
        assert_eq!(h.cancels().len(), 2);
        assert!(h.core.holds(None).contains(&HoldReason::KillLatched));
        refused(&mut h, "kill_orders_open");
        h.status(
            a,
            VenueStatus::Canceled {
                reason: "canceled".into(),
            },
            Some("0"),
        );
        let text = refused(&mut h, "kill_orders_open");
        assert!(text.contains("1 open orders"), "{text}");
        h.status(
            b,
            VenueStatus::Canceled {
                reason: "canceled".into(),
            },
            Some("0"),
        );
        assert!(h.core.open.is_empty());

        let (out, reply) = clear(&mut h, HoldKind::Kill, "venue checked");
        assert_eq!(reply, Ok("cleared".to_string()), "{outcome:?}");
        assert_eq!(names(&out), ["hold_cleared"]);
        assert_eq!(out.commands, [VenueCommand::KillStop]);
        assert_eq!(h.rebuilt(), h.core);
    }
}

/// `resume` ends only an operator's halt: under a kill, a watcher mismatch
/// and an operator halt, `resume` leaves the first two.
#[test]
fn resume_leaves_kill_and_watcher_mismatch() {
    let mut h = H::ready();
    h.op(OperatorCmd::Halt {
        scope: Scope::Account,
        note: "n".into(),
    })
    .unwrap();
    watcher_kill(&mut h, "k").1.unwrap();
    watcher_mismatch(&mut h, "d").1.unwrap();
    assert_eq!(
        h.op(OperatorCmd::Resume {
            scope: Scope::Account,
            note: "n".into(),
        }),
        Ok("resumed".to_string())
    );
    assert_eq!(
        h.core.holds(None),
        vec![HoldReason::KillLatched, HoldReason::WatcherMismatch]
    );
    // Without an operator halt `resume` is refused, and the kill stays.
    assert!(
        h.op(OperatorCmd::Resume {
            scope: Scope::Account,
            note: "n".into(),
        })
        .is_err()
    );
    assert!(h.core.holds(None).contains(&HoldReason::KillLatched));
    assert_eq!(h.rebuilt(), h.core);
}

/// The venue-change hold is derived from the events that already record
/// the change: the event lists stay as they were (U13b), and the hold
/// latches from each of the three paths.
#[test]
fn a_venue_change_latches_without_a_new_event() {
    let mut h = H::ready();
    let out = venue_change(&mut h, "adl1", "-0.5");
    assert_eq!(names(&out), ["venue_position_changed", "mismatch_raised"]);
    assert!(h.core.holds(None).contains(&HoldReason::VenueChange));
    let note = h.core.venue_change.clone().unwrap();
    assert_eq!(note.source, HoldSource::Venue);
    assert!(note.note.contains("adl1"), "{}", note.note);

    let mut h = H::ready();
    let out = unclassified(&mut h, "e1");
    assert_eq!(
        names(&out),
        ["unclassified_execution_noted", "mismatch_raised"]
    );
    assert!(
        h.core
            .venue_change
            .as_ref()
            .unwrap()
            .note
            .contains("SomethingNew")
    );

    // The overflow path raises only the mismatch.
    let mut h = H::ready();
    h.op(OperatorCmd::AdjustPosition {
        instrument: iid("X"),
        ledger: Ledger::Unattributed,
        delta: SQty(i64::MAX - 1),
        note: "n".into(),
    })
    .unwrap();
    let out = venue_change(&mut h, "big1", "1");
    assert_eq!(names(&out), ["mismatch_raised"]);
    assert!(h.core.holds(None).contains(&HoldReason::VenueChange));

    // Acknowledging the mismatch leaves the hold; it needs its own clear.
    let id = *h.core.mismatches.keys().next().unwrap();
    h.op(OperatorCmd::AckMismatch {
        id,
        note: "seen".into(),
    })
    .unwrap();
    assert_eq!(h.core.holds(None), vec![HoldReason::VenueChange]);
    assert_eq!(h.rebuilt(), h.core);
}

/// A journal written before U18b, with one of the three events, replays
/// with the venue-change hold set: the hold is in `apply`, not in a
/// decision.
#[test]
fn a_journal_with_a_venue_change_latches_on_replay() {
    let start = H::configured(config());
    let change = VenuePositionChangeIn {
        key: fkey("adl1"),
        kind: VenueChangeKind::Adl,
        instrument: iid("X"),
        qty: sq("-0.5"),
        px: px("100"),
        venue_ts: start.now,
    };
    let cases = [
        Event::VenuePositionChanged { change },
        Event::UnclassifiedExecutionNoted {
            key: fkey("e1"),
            instrument: iid("X"),
            exec_type: "Odd".into(),
            detail: "d".into(),
            venue_ts: start.now,
        },
        Event::MismatchRaised {
            id: 1,
            kind: MismatchKind::VenuePositionChange,
            subject: "instrument:X".into(),
            detail: "overflows the booked position".into(),
        },
    ];
    for ev in cases {
        let mut c = start.rebuilt();
        assert_eq!(c.venue_change, None);
        let rec = Record {
            seq: start.log.len() as u64 + 1,
            at: start.now + SEC,
            input: Input::Tick,
            events: vec![ev.clone()],
        };
        c.replay(&rec).unwrap();
        assert!(
            c.holds(None).contains(&HoldReason::VenueChange),
            "{}",
            ev.name()
        );
        assert_eq!(c.venue_change.as_ref().unwrap().since, start.now + SEC);
    }
    // A mismatch of another kind does not set it.
    let mut c = start.rebuilt();
    c.replay(&Record {
        seq: start.log.len() as u64 + 1,
        at: start.now + SEC,
        input: Input::Tick,
        events: vec![Event::MismatchRaised {
            id: 1,
            kind: MismatchKind::ExternalFill,
            subject: "instrument:X".into(),
            detail: "d".into(),
        }],
    })
    .unwrap();
    assert_eq!(c.venue_change, None);
}

/// Each hold latches once: a second trigger while it is held emits
/// nothing, and the first note stays.
#[test]
fn a_second_trigger_keeps_the_first_note() {
    let mut h = H::ready();
    let (out, reply) = watcher_kill(&mut h, "first");
    assert_eq!(
        (names(&out), reply),
        (vec!["hold_set"], Ok("latched".to_string()))
    );
    let since = h.core.kill.as_ref().unwrap().since;
    let (out, reply) = watcher_kill(&mut h, "second");
    assert_eq!(reply, Ok("already latched".to_string()));
    assert!(out.events.is_empty() && out.commands.is_empty());
    // A marker at a later boot does not replace it either.
    let out = h.inp(Input::Boot {
        binary: "test".into(),
        kill_marker: Some("marker: third".into()),
    });
    assert_eq!(names(&out), ["booted"]);
    let k = h.core.kill.clone().unwrap();
    assert_eq!(
        (k.since, k.source, k.note.as_str()),
        (since, HoldSource::Watcher, "first")
    );

    let (out, reply) = watcher_mismatch(&mut h, "first");
    assert_eq!(
        (names(&out), reply),
        (vec!["hold_set"], Ok("held".to_string()))
    );
    let (out, reply) = watcher(
        &mut h,
        WatcherInput::Mismatch {
            check: WatcherCheck::W4,
            detail: "second".into(),
        },
    );
    assert_eq!(reply, Ok("already held".to_string()));
    assert!(out.events.is_empty());
    let w = h.core.watcher_mismatch.clone().unwrap();
    assert_eq!((w.source, w.note.as_str()), (HoldSource::W2, "first"));

    unclassified(&mut h, "e1");
    let first = h.core.venue_change.clone().unwrap();
    h.advance(SEC);
    venue_change(&mut h, "adl1", "1");
    assert_eq!(h.core.venue_change, Some(first));
    assert_eq!(h.rebuilt(), h.core);
}

/// The watcher's inputs need a detail of at most `WATCHER_DETAIL_MAX`
/// (1 KiB) bytes; an empty or longer one is refused and nothing is written
/// (review N8).
#[test]
fn a_watcher_input_without_a_detail_is_refused() {
    let mut h = H::ready();
    let long = "x".repeat(WATCHER_DETAIL_MAX + 1);
    for (out, reply) in [
        watcher_kill(&mut h, " "),
        watcher_mismatch(&mut h, ""),
        watcher_kill(&mut h, &long),
        watcher_mismatch(&mut h, &long),
    ] {
        assert!(reply.unwrap_err().starts_with("bad_request"));
        assert!(out.events.is_empty());
    }
    assert_eq!(h.core.holds(None), vec![]);
    assert_eq!(WATCHER_DETAIL_MAX, 1024);
    let longest = "x".repeat(WATCHER_DETAIL_MAX);
    assert_eq!(watcher_mismatch(&mut h, &longest).1, Ok("held".to_string()));
    assert_eq!(watcher_kill(&mut h, &longest).1, Ok("latched".to_string()));
}

/// The kill latch replays from `hold_set`, its confirmation from
/// `kill_loop_ended`; every boot resets the confirmation and asks the
/// gateway for a new loop with the latch's own fields.
#[test]
fn the_kill_latch_and_its_confirmation_survive_replay_and_a_boot_asks_for_a_new_loop() {
    let mut h = H::ready();
    // An answer without a latch is not recorded.
    assert!(
        kill_loop_ended(&mut h, KillOutcome::Confirmed)
            .events
            .is_empty()
    );

    watcher_kill(&mut h, "W5: no heartbeat").1.unwrap();
    let since = h.core.kill.as_ref().unwrap().since;
    let kill = VenueCommand::Kill {
        since,
        source: HoldSource::Watcher,
        note: "W5: no heartbeat".into(),
    };
    assert_eq!(kills(&h), std::slice::from_ref(&kill));
    assert_eq!(h.rebuilt().kill, h.core.kill);

    let out = kill_loop_ended(&mut h, KillOutcome::Confirmed);
    assert_eq!(names(&out), ["kill_loop_ended"]);
    let confirmed_at = h.core.kill.as_ref().unwrap().confirmed_at;
    assert_eq!(confirmed_at, Some(h.now));
    // A second answer in the same process is not recorded.
    assert!(
        kill_loop_ended(&mut h, KillOutcome::Confirmed)
            .events
            .is_empty()
    );
    let replayed = h.rebuilt();
    assert_eq!(replayed.kill.as_ref().unwrap().confirmed_at, confirmed_at);
    assert_eq!(replayed, h.core);

    // A restart: the latch stays, the confirmation does not, and the boot
    // record asks for a new loop.
    let out = h.inp(Input::Boot {
        binary: "test".into(),
        kill_marker: None,
    });
    assert_eq!(names(&out), ["booted"]);
    assert_eq!(out.commands, std::slice::from_ref(&kill));
    let k = h.core.kill.clone().unwrap();
    assert_eq!((k.since, k.confirmed_at, k.outcome), (since, None, None));
    assert!(h.core.holds(None).contains(&HoldReason::KillLatched));
    let (_, reply) = clear(&mut h, HoldKind::Kill, "n");
    assert!(reply.unwrap_err().starts_with("kill_unconfirmed"));

    h.link_up_and_sync();
    let out = kill_loop_ended(&mut h, KillOutcome::Unsupported);
    assert_eq!(names(&out), ["kill_loop_ended"]);
    assert_eq!(
        h.core.kill.as_ref().unwrap().outcome,
        Some(KillOutcome::Unsupported)
    );
    assert_eq!(
        clear(&mut h, HoldKind::Kill, "cancelled by hand").1,
        Ok("cleared".to_string())
    );
    assert_eq!(kills(&h), [kill.clone(), kill, VenueCommand::KillStop]);
    assert_eq!(h.core.holds(None), vec![]);
    // Unlatched, a boot asks for nothing.
    let out = h.inp(Input::Boot {
        binary: "test".into(),
        kill_marker: None,
    });
    assert!(out.commands.is_empty());
    h.link_up_and_sync();
    h.accept("a", "k1", Side::Buy, "1", "100");
    assert_eq!(h.rebuilt(), h.core);
}

/// The kill marker latches in the boot record itself, so that no order
/// can be decided before the latch; the `Kill` command follows from it.
#[test]
fn the_kill_marker_latches_in_the_boot_record() {
    let mut h = H::new();
    let out = h.inp(Input::Boot {
        binary: "test".into(),
        kill_marker: Some("marker: W5 killed at 12:00".into()),
    });
    assert_eq!(names(&out), ["booted", "hold_set"]);
    let Event::HoldSet { hold, source, note } = &out.events[1] else {
        unreachable!()
    };
    assert_eq!(
        (hold, source, note.as_str()),
        (
            &HoldKind::Kill,
            &HoldSource::Marker,
            "marker: W5 killed at 12:00"
        )
    );
    assert_eq!(
        out.commands,
        [VenueCommand::Kill {
            since: h.now,
            source: HoldSource::Marker,
            note: "marker: W5 killed at 12:00".into(),
        }]
    );
    h.inp(Input::Config(Box::new(config_with_margin())));
    h.link_up_and_sync();
    h.report_margin("1000");
    h.ref_price("X", "100");
    let (plain, reduce_only_passes) = plain_and_reduce_only(&mut h, "k");
    assert!(
        plain.contains(&RejectReason::Hold(HoldReason::KillLatched)),
        "{plain:?}"
    );
    assert!(!reduce_only_passes);
    assert_eq!(h.rebuilt(), h.core);

    // The boot record is written in schema 2 without a field for a run
    // without a marker.
    let json = serde_json::to_string(&Input::Boot {
        binary: "b".into(),
        kill_marker: None,
    })
    .unwrap();
    assert_eq!(json, r#"{"boot":{"binary":"b"}}"#);
}

/// Every gateway command is what `command_for` derives from the events of
/// its record, one event at a time on the replayed state: a command exists
/// only because of a recorded event, and leaves with that record.
#[test]
fn every_command_comes_from_command_for_on_a_recorded_event() {
    let mut h = trading();
    let id = h.accept("a", "k1", Side::Buy, "0.1", "100");
    h.placed_open(id, "v1");
    watcher_kill(&mut h, "k").1.unwrap();
    h.cancel("a", id);
    h.status(
        id,
        VenueStatus::Canceled {
            reason: "canceled".into(),
        },
        Some("0"),
    );
    kill_loop_ended(&mut h, KillOutcome::Confirmed);
    h.inp(Input::Boot {
        binary: "test".into(),
        kill_marker: None,
    });
    kill_loop_ended(&mut h, KillOutcome::Confirmed);
    clear(&mut h, HoldKind::Kill, "n").1.unwrap();

    let mut c = Core::new(h.core.instance_id);
    let mut derived = Vec::new();
    for rec in &h.log {
        for ev in &rec.events {
            c.apply(rec.at, ev).unwrap();
            derived.extend(c.command_for(ev));
        }
    }
    assert_eq!(derived, h.commands);
    assert_eq!(kills(&h).len(), 3, "{:?}", kills(&h));
}

// ---- U18b2: the stream trigger and a new trigger after the confirmation ----

fn stream_lost(h: &mut H, since: Ts, detail: &str) -> Output {
    h.venue(VenueInput::StreamLost {
        since,
        detail: detail.into(),
    })
}

/// C-KILL. `StreamLost` latches the kill with source `stream` and a note
/// that says how long the stream was down and why, in one record with the
/// `Kill` command; a second loss while the loop runs records nothing.
#[test]
fn a_stream_loss_latches_the_kill_and_asks_for_the_loop() {
    let mut h = H::ready();
    let since = h.now;
    // The harness adds 1 ms to the record time of every input.
    h.advance(10 * SEC);
    let out = stream_lost(&mut h, since, "no pong within 1000 ms of a ping");
    assert_eq!(names(&out), ["hold_set"]);
    let k = h.core.kill.clone().unwrap();
    assert_eq!((k.since, k.source), (h.now, HoldSource::Stream));
    assert_eq!(
        k.note,
        format!(
            "private stream down for 10001 ms (no proof of life since {since}): no pong within 1000 ms of a ping"
        )
    );
    assert_eq!(
        out.commands,
        [VenueCommand::Kill {
            since: k.since,
            source: HoldSource::Stream,
            note: k.note.clone(),
        }]
    );
    assert!(h.core.holds(None).contains(&HoldReason::KillLatched));
    // The loop has not confirmed: the running loop covers a new loss.
    h.advance(SEC);
    let out = stream_lost(&mut h, since, "closed by the venue");
    assert!(out.events.is_empty() && out.commands.is_empty());
    assert_eq!(h.core.kill, Some(k));
    assert_eq!(h.rebuilt(), h.core);
}

/// C-KILL (review N3 of U18b1). After the loop confirmed, a new trigger
/// (the watcher or the stream) voids the confirmation with `kill_rearmed`
/// and asks for a new loop; the latch keeps its first note. A clear is
/// refused until the new loop confirms. Replay gives the same state.
#[test]
fn a_new_trigger_after_the_confirmation_resets_it_and_asks_for_a_new_loop() {
    let mut h = H::ready();
    watcher_kill(&mut h, "first").1.unwrap();
    let first = h.core.kill.clone().unwrap();
    kill_loop_ended(&mut h, KillOutcome::Confirmed);
    assert!(h.core.kill.as_ref().unwrap().confirmed_at.is_some());

    h.advance(SEC);
    let (out, reply) = watcher_kill(&mut h, "second");
    assert_eq!(reply, Ok(KILL_REARMED_REPLY.to_string()));
    assert!(KILL_REARMED_REPLY.starts_with("already latched"));
    assert_eq!(names(&out), ["kill_rearmed"]);
    assert_eq!(
        out.commands,
        [VenueCommand::Kill {
            since: first.since,
            source: HoldSource::Watcher,
            note: "second".into(),
        }]
    );
    let k = h.core.kill.clone().unwrap();
    assert_eq!(
        (k.since, k.source, k.note.as_str(), k.confirmed_at, k.outcome),
        (first.since, HoldSource::Watcher, "first", None, None)
    );
    let (_, reply) = clear(&mut h, HoldKind::Kill, "n");
    assert!(reply.unwrap_err().starts_with("kill_unconfirmed"));
    // While the new loop runs, another trigger records nothing.
    let (out, reply) = watcher_kill(&mut h, "third");
    assert_eq!(reply, Ok("already latched".to_string()));
    assert!(out.events.is_empty());
    assert_eq!(h.rebuilt(), h.core);

    // The new loop confirms; a stream loss after it re-arms again.
    kill_loop_ended(&mut h, KillOutcome::Confirmed);
    h.advance(SEC);
    let since = h.now;
    h.advance(10 * SEC);
    let out = stream_lost(&mut h, since, "closed by the venue");
    assert_eq!(names(&out), ["kill_rearmed"]);
    assert!(
        matches!(&out.commands[..], [VenueCommand::Kill { source: HoldSource::Stream, note, .. }] if note.ends_with("closed by the venue")),
        "{:?}",
        out.commands
    );
    assert_eq!(h.core.kill.as_ref().unwrap().confirmed_at, None);
    kill_loop_ended(&mut h, KillOutcome::Confirmed);
    assert_eq!(
        clear(&mut h, HoldKind::Kill, "venue checked").1,
        Ok("cleared".to_string())
    );
    assert_eq!(kills(&h).len(), 4, "{:?}", kills(&h));
    assert_eq!(h.rebuilt(), h.core);

    // Every command still comes from `command_for` on a recorded event.
    let mut c = Core::new(h.core.instance_id);
    let mut derived = Vec::new();
    for rec in &h.log {
        for ev in &rec.events {
            c.apply(rec.at, ev).unwrap();
            derived.extend(c.command_for(ev));
        }
    }
    assert_eq!(derived, h.commands);
}

/// C-KILL. An `unsupported` answer confirms nothing at the venue: a new
/// trigger after it keeps the answer, records nothing and asks for no new
/// loop (the gateway would answer the same). The book check
/// (`kill_orders_open`) still holds the clear while orders are open.
#[test]
fn a_new_trigger_after_an_unsupported_answer_records_nothing() {
    let mut h = H::ready();
    watcher_kill(&mut h, "first").1.unwrap();
    kill_loop_ended(&mut h, KillOutcome::Unsupported);
    let before = h.core.kill.clone().unwrap();
    h.advance(SEC);
    let (out, reply) = watcher_kill(&mut h, "second");
    assert_eq!(reply, Ok("already latched".to_string()));
    assert!(out.events.is_empty() && out.commands.is_empty(), "{out:?}");
    h.advance(10 * SEC);
    let since = h.now - 10 * SEC;
    let out = stream_lost(&mut h, since, "closed by the venue");
    assert!(out.events.is_empty() && out.commands.is_empty(), "{out:?}");
    assert_eq!(h.core.kill.as_ref(), Some(&before));
    assert_eq!(h.rebuilt(), h.core);
}
