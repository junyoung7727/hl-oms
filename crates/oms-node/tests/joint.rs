//! The OMS and watcher joint tests: the real `oms-node` (a `failpoints`
//! build with the Bybit gateway) and the real watcher (its
//! `test-endpoints` build) run together against one stateful fake Bybit,
//! a fake Telegram, a fake VictoriaMetrics and a fake `systemctl`,
//! loopback only ([`joint_rig`]). Needs `--features joint` and the
//! variables `OMS_TEST_DIR`, `JOINT_WATCHER_BIN` and `JOINT_OMSCTL_BIN`;
//! `cargo run -p xtask -- joint` builds the two other binaries and sets
//! them. One test at a time (`--test-threads=1`): each world holds several
//! processes and the scenarios run in real time.

#![allow(clippy::unwrap_used)]

mod bybit_fake;
mod joint_rig;

use bybit_fake::book::{OKEY, SYMBOL, WKEY};
use joint_rig::*;
use oms_core::*;
use serde_json::json;
use std::time::{Duration, Instant};

/// The watcher's four check requests at the fake.
const CHECKS: [(&str, &str); 4] = [
    ("W1", "/v5/order/realtime"),
    ("W2", "/v5/position/list"),
    ("W3", "/v5/account/wallet-balance"),
    ("W4", "/v5/execution/list"),
];

fn now_ms() -> u64 {
    oms_api::client::now_us() / 1_000
}

/// The order's state as the strategy sees it.
fn phase(oms: &Oms, id: OrderId) -> Phase {
    oms.order(id).phase
}

/// The order's close reason in the journal, once it is closed.
fn closed(oms: &Oms, id: OrderId) -> Option<CloseReason> {
    oms.events("order_closed")
        .into_iter()
        .find_map(|(_, e)| match e {
            Event::OrderClosed { order_id, reason } if order_id == id => Some(reason),
            _ => None,
        })
}

fn fills_of(oms: &Oms, id: OrderId) -> usize {
    oms.events("fill_booked")
        .iter()
        .filter(|(_, e)| matches!(e, Event::FillBooked { order_id, .. } if *order_id == id))
        .count()
}

/// What the watcher must never have caused: a `watcher_mismatch` hold or a
/// kill from it in the journal (a `watcher_mismatch` or `watcher_kill`
/// request on `admin.sock` shows there), a `[WATCH P1]`, a `confirmed
/// difference` line, the kill marker. The watcher logs no line of its own
/// for those two requests, so the greps for their names only catch an
/// alert text that names them.
fn assert_watcher_quiet(w: &World, oms: &Oms, watcher: &Watcher) {
    let holds: Vec<Event> = oms
        .events("hold_set")
        .into_iter()
        .map(|(_, e)| e)
        .filter(|e| {
            matches!(
                e,
                Event::HoldSet {
                    hold: HoldKind::WatcherMismatch,
                    ..
                } | Event::HoldSet {
                    source: HoldSource::Watcher,
                    ..
                }
            )
        })
        .collect();
    let watch: Vec<_> = w
        .telegram_messages()
        .into_iter()
        .filter(|(p, _)| p == "[WATCH P1]")
        .collect();
    let lines = [
        watcher.log_containing("confirmed difference"),
        watcher.log_containing("watcher_mismatch"),
        watcher.log_containing("watcher_kill"),
    ]
    .concat();
    if !holds.is_empty() || !watch.is_empty() || !lines.is_empty() || w.marker().exists() {
        w.fail(&format!(
            "the watcher raised something: holds {holds:?}, telegram {watch:?}, lines {lines:?}, marker {}",
            w.marker().exists()
        ));
    }
}

/// The rig's guard refuses a binary that is not the watcher's test build
/// before any watcher configuration is written; the real test build
/// started without `WATCHER_TEST_ENDPOINTS` exits 2 with its one line
/// and sends nothing to the fake.
#[test]
fn j0_the_rig_refuses_a_watcher_built_without_test_endpoints_and_a_test_build_without_wiring_exits_2()
 {
    let w = World::new("j0", Timers::Short, Duration::from_secs(20));
    let script = w.dir.join("bin/watcher");
    std::fs::write(&script, "#!/bin/sh\necho 'watcher 0.1.0'\n").unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
    let refused = w.watcher_with(&script).err().unwrap_or_default();
    assert!(
        refused.contains("printed \"watcher 0.1.0\": not the watcher's test build"),
        "the guard took a default build: {refused:?}"
    );
    assert!(
        !w.dir.join("w/watcher.toml").exists(),
        "a configuration was written for a binary the guard refused"
    );

    let mut watcher = w.watcher();
    watcher.start_unwired();
    w.wait(
        "the unwired test build exits",
        Duration::from_secs(15),
        || watcher.exit().is_some(),
    );
    let (status, _) = watcher.exit().unwrap();
    let lines = watcher.log_lines("");
    assert_eq!(status.code(), Some(2), "{lines:?}");
    assert_eq!(
        lines,
        ["this build is for tests: WATCHER_TEST_ENDPOINTS is required"],
        "the refusal line"
    );
    let seen = w.fake.with(|b, _| b.log.len());
    assert_eq!(seen, 0, "the unwired watcher reached the fake");
    assert!(w.telegram.requests().is_empty() && w.vm.requests().is_empty());
}

/// J1 at the deployed timers (the code defaults): the OMS starts
/// unmonitored and its gate opens on the first heartbeat with fresh W1 and
/// W2; an order is partly then fully filled and a second one is
/// cancelled; afterwards at least three rounds of each check and
/// `3 × w4_ms + min_age_ms` of quiet pass with no mismatch, no hold from
/// the watcher, no `[WATCH P1]`; every heartbeat record's check times are
/// within 15 s of its record time; the venue's book and the journal agree
/// on the position and on zero open orders.
#[test]
fn j1_a_whole_life_cycle_at_the_deployed_timers_has_no_mismatch_and_opens_the_gate() {
    let w = World::new("j1", Timers::Deployed, Duration::from_secs(120));
    let mut oms = w.oms();
    oms.start(&[]);
    w.wait(
        "the OMS is up, linked and unmonitored",
        Duration::from_secs(20),
        || {
            oms.status_admin().is_some_and(|s| {
                s["core"]["link"]["up"] == true
                    && s["core"]["boot"].as_u64().unwrap_or(0) > 0
                    && s["core"]["needs"] == json!([])
                    && s["core"]["holds"] == json!(["unmonitored"])
            })
        },
    );
    let NewOrderReply::Rejected { reasons } =
        oms.new_order("early", Side::Buy, "0.001", "30000", false)
    else {
        w.fail("a plain order was accepted before any heartbeat")
    };
    assert!(
        reasons.contains(&RejectReason::Hold(HoldReason::Unmonitored)),
        "{reasons:?}"
    );

    // The deployed timers: the OMS's gate policy is its code default.
    let policy = oms.status_admin().unwrap()["core"]["monitor"]["policy"].clone();
    assert_eq!(
        policy,
        json!({"heartbeat_max_age_us": 30_000_000, "check_max_age_us": 10_000_000, "unmonitored_cancel_us": 60_000_000}),
        "not the deployed gate policy"
    );

    let mut watcher = w.watcher();
    watcher.start();
    w.wait("the gate opens", Duration::from_secs(40), || {
        oms.status_http().is_some_and(|s| {
            s["core"]["monitor"]["monitored"] == true
                && s["core"]["monitor"]["watcher"] == WATCHER_NAME
        })
    });
    // The gate opened in the record of a heartbeat with fresh W1 and W2,
    // and not before the first heartbeat.
    let recs = oms.records();
    let opened = recs
        .iter()
        .find(|r| {
            r.events.iter().any(|e| {
                matches!(
                    e,
                    Event::MonitorChanged {
                        monitored: true,
                        ..
                    }
                )
            })
        })
        .unwrap_or_else(|| w.fail("no monitor_changed{monitored: true} in the journal"));
    let first_beat = recs
        .iter()
        .find(|r| r.events.iter().any(|e| e.name() == "watcher_heartbeat"))
        .unwrap();
    assert!(
        first_beat.seq <= opened.seq,
        "the gate opened before any heartbeat"
    );
    let fresh = opened.events.iter().any(|e| match e {
        Event::WatcherHeartbeat { watcher, checks } => {
            watcher == WATCHER_NAME
                && ["W1", "W2"].iter().all(|c| {
                    checks
                        .get(*c)
                        .copied()
                        .flatten()
                        .is_some_and(|t| opened.at.saturating_sub(t) <= 10_000_000)
                })
        }
        _ => false,
    });
    assert!(
        fresh,
        "the gate opened without a heartbeat with fresh W1 and W2: {:?}",
        opened.events
    );

    w.wait(
        "a reference price from the ticker",
        Duration::from_secs(15),
        || !oms.events("ref_price_updated").is_empty(),
    );
    let NewOrderReply::Accepted { order_id: a } =
        oms.new_order("a1", Side::Buy, "0.002", "30000", false)
    else {
        w.fail("order A was not accepted")
    };
    w.wait("order A is working", Duration::from_secs(10), || {
        phase(&oms, a) == Phase::Working
    });
    let venue_a = w
        .fake
        .with(|b, _| b.by_link(&oms.link(a)).map(|o| o.order_id.clone()))
        .unwrap_or_else(|| w.fail("order A is not at the venue"));
    w.fake.fill(&venue_a, "0.001", "30000");
    w.wait(
        "the partial fill is booked",
        Duration::from_secs(10),
        || fills_of(&oms, a) == 1,
    );
    assert_eq!(
        phase(&oms, a),
        Phase::Working,
        "order A after its partial fill"
    );
    w.fake.fill(&venue_a, "0.001", "30000");
    w.wait("order A is closed Filled", Duration::from_secs(10), || {
        closed(&oms, a) == Some(CloseReason::Filled)
    });
    assert_eq!(fills_of(&oms, a), 2);

    let NewOrderReply::Accepted { order_id: b } =
        oms.new_order("b1", Side::Buy, "0.001", "29000", false)
    else {
        w.fail("order B was not accepted")
    };
    w.wait("order B is working", Duration::from_secs(10), || {
        phase(&oms, b) == Phase::Working
    });
    assert!(
        matches!(oms.cancel(b), CancelReply::Requested { .. }),
        "the cancel of B"
    );
    w.wait(
        "order B is closed Cancelled",
        Duration::from_secs(10),
        || matches!(closed(&oms, b), Some(CloseReason::Canceled { .. })),
    );
    assert!(
        w.fake
            .with(|b2, _| b2.by_link(&oms.link(b)).map(|o| o.status))
            == Some("Cancelled"),
        "order B at the venue"
    );

    // Quiet: three rounds of every check after the last order event, and
    // 3 × w4_ms + min_age_ms.
    let last = now_ms();
    let quiet = Duration::from_millis(3 * w.timers.w4_ms() + w.timers.min_age_ms());
    let started = Instant::now();
    w.wait(
        "three rounds of W1..W4 and the quiet time",
        Duration::from_secs(60),
        || {
            started.elapsed() >= quiet
                && CHECKS.iter().all(|(_, path)| {
                    w.requests(WKEY, path).iter().filter(|t| **t > last).count() >= 3
                })
        },
    );
    assert_watcher_quiet(&w, &oms, &watcher);
    assert!(
        watcher.exit().is_none(),
        "the watcher ended: {:?}",
        watcher.log_lines("")
    );

    // Every heartbeat record: its check times within 15 s of the record
    // time and not after it; the last one has all four.
    let beats: Vec<(Ts, std::collections::BTreeMap<String, Option<Ts>>)> = oms
        .records()
        .into_iter()
        .flat_map(|r| {
            r.events.into_iter().filter_map(move |e| match e {
                Event::WatcherHeartbeat { checks, .. } => Some((r.at, checks)),
                _ => None,
            })
        })
        .collect();
    assert!(beats.len() >= 3, "{} heartbeat records", beats.len());
    for (at, checks) in &beats {
        for (c, t) in checks {
            if let Some(t) = t {
                assert!(
                    *t <= *at && at - t <= 15_000_000,
                    "{c} at {t} in the heartbeat recorded at {at}"
                );
            }
        }
    }
    // The watcher's deployed heartbeat_ms (5,000): on average at least 4 s
    // between heartbeat records, and no two closer than 2.5 s, so one
    // record late by up to 2.5 s does not fail the test (both margins
    // 판단(모델 아님)).
    let span = beats.last().unwrap().0 - beats[0].0;
    let mean = span / (beats.len() as u64 - 1);
    assert!(
        mean >= 4_000_000,
        "heartbeats {mean} µs apart on average: not the deployed heartbeat_ms"
    );
    for pair in beats.windows(2) {
        assert!(
            pair[1].0 - pair[0].0 >= 2_500_000,
            "heartbeats {} µs apart: not the deployed heartbeat_ms",
            pair[1].0 - pair[0].0
        );
    }
    let (_, newest) = beats.last().unwrap();
    assert!(
        CHECKS
            .iter()
            .all(|(c, _)| newest.get(*c).copied().flatten().is_some()),
        "the last heartbeat: {newest:?}"
    );

    // The venue and the journal agree: +0.002, nothing open.
    let status = oms.status_http().unwrap();
    let (venue_size, venue_open) = w.fake.with(|b, _| (b.size, b.open_orders().len()));
    assert_eq!((venue_size, venue_open), (2, 0), "the book");
    assert_eq!(status["core"]["open_orders"], 0, "{}", status["core"]);
    assert_eq!(
        status["account"][SYMBOL]["pos"]
            .as_str()
            .map(str::to_string),
        Some("0.002".to_string()),
        "the journal's position: {}",
        status["account"]
    );
    w.assert_venue_invariants();
}

/// The OMS and the watcher of `w` started in the documented order (OMS
/// first, unmonitored), the gate open and a reference price there.
fn start_both(w: &World) -> (Oms, Watcher) {
    start_both_with(w, &[])
}

/// [`start_both`] with the OMS's `extra_env` (J5b's failpoint).
fn start_both_with(w: &World, extra_env: &[(&str, &str)]) -> (Oms, Watcher) {
    let mut oms = w.oms();
    oms.start(extra_env);
    w.wait(
        "the OMS is up, linked and unmonitored",
        Duration::from_secs(20),
        || {
            oms.status_admin().is_some_and(|s| {
                s["core"]["link"]["up"] == true
                    && s["core"]["boot"].as_u64().unwrap_or(0) > 0
                    && s["core"]["needs"] == json!([])
                    && s["core"]["holds"] == json!(["unmonitored"])
            })
        },
    );
    let mut watcher = w.watcher();
    watcher.start();
    w.wait("the gate opens", Duration::from_secs(30), || {
        monitored(&oms) == Some(true)
    });
    w.wait(
        "a reference price from the ticker",
        Duration::from_secs(15),
        || !oms.events("ref_price_updated").is_empty(),
    );
    (oms, watcher)
}

/// The gate's verdict in `/api/status`.
fn monitored(oms: &Oms) -> Option<bool> {
    oms.status_http()
        .and_then(|s| s["core"]["monitor"]["monitored"].as_bool())
}

/// A plain buy of 0.001 at `px`, accepted and working.
fn working(w: &World, oms: &Oms, key: &str, px: &str) -> OrderId {
    let NewOrderReply::Accepted { order_id } = oms.new_order(key, Side::Buy, "0.001", px, false)
    else {
        w.fail(&format!("the plain order {key} was not accepted"))
    };
    w.wait(
        &format!("order {key} is working"),
        Duration::from_secs(10),
        || phase(oms, order_id) == Phase::Working,
    );
    order_id
}

/// `core.holds` in `/api/status`.
fn status_holds(oms: &Oms) -> Vec<String> {
    oms.status_http()
        .and_then(|s| {
            s["core"]["holds"].as_array().map(|a| {
                a.iter()
                    .filter_map(|h| h.as_str().map(str::to_string))
                    .collect()
            })
        })
        .unwrap_or_default()
}

/// The rejection reasons of a new plain order, or a failure if it was
/// accepted.
fn refused(w: &World, oms: &Oms, key: &str) -> Vec<RejectReason> {
    match oms.new_order(key, Side::Buy, "0.001", "29000", false) {
        NewOrderReply::Rejected { reasons } => reasons,
        r => w.fail(&format!("the plain order {key} was not refused: {r:?}")),
    }
}

/// Every `hold_set` of `hold`: record time (µs), source, note.
fn holds_of(oms: &Oms, hold: HoldKind) -> Vec<(Ts, HoldSource, String)> {
    oms.events("hold_set")
        .into_iter()
        .filter_map(|(at, e)| match e {
            Event::HoldSet {
                hold: h,
                source,
                note,
            } if h == hold => Some((at, source, note)),
            _ => None,
        })
        .collect()
}

/// The record times (µs) of `monitor_changed{monitored: m}`.
fn monitor_changes(oms: &Oms, m: bool) -> Vec<Ts> {
    oms.events("monitor_changed")
        .into_iter()
        .filter_map(|(at, e)| match e {
            Event::MonitorChanged { monitored, .. } if monitored == m => Some(at),
            _ => None,
        })
        .collect()
}

/// The fake Telegram's message bodies with `prefix` that hold `text`.
fn telegram_with(w: &World, prefix: &str, text: &str) -> Vec<String> {
    w.telegram_messages()
        .into_iter()
        .filter(|(p, b)| p == prefix && b.contains(text))
        .map(|(_, b)| b)
        .collect()
}

/// The OMS's direct alert of a `watcher_mismatch` hold
/// (`alerts.rs`, `HoldReason::WatcherMismatch`).
const OMS_WATCHER_HOLD: &str = "워처가 불일치를 확정함";

/// `omsctl clear-hold --reason <reason> --note <note>`: it must answer
/// `cleared`.
fn clear_hold(w: &World, oms: &Oms, reason: &str, note: &str) {
    let (code, out) = oms.omsctl(&["clear-hold", "--reason", reason, "--note", note]);
    if code != Some(0) || !out.contains("cleared") {
        w.fail(&format!("clear-hold {reason}: exit {code:?}, {out:?}"));
    }
}

/// J2a: a venue-only order (made on the venue's web page) is confirmed by
/// W1 only after it was seen in two completed rounds and `min_age_ms`
/// after it was first seen; the OMS then holds plain orders
/// (`hold_set{watcher_mismatch}` from W1, both direct alerts), refuses a
/// new one and still accepts a cancel. After the operator cancelled the
/// order at the venue and two W1 rounds no longer saw it, `clear-hold`
/// restores trading and no hold comes back.
#[test]
fn j2a_a_venue_only_order_holds_plain_orders_after_two_rounds_and_two_seconds_until_clear_hold() {
    let w = World::new("j2a", Timers::Short, Duration::from_secs(60));
    let min_age = w.watcher_timer("min_age_ms").unwrap();
    let (oms, watcher) = start_both(&w);
    let r = working(&w, &oms, "r1", "29000");

    // F, planted under the book's lock: every W1 answer after `from`
    // holds it.
    let (from, f) = w.fake.with(|b, now| {
        let from = b.log.len();
        (
            from,
            b.plant_order(now, "manual-1", "Buy", "0.001", "29500"),
        )
    });
    w.wait(
        "W1 confirms F and the OMS holds",
        Duration::from_secs(15),
        || !holds_of(&oms, HoldKind::WatcherMismatch).is_empty(),
    );
    let holds = holds_of(&oms, HoldKind::WatcherMismatch);
    let (at, source, note) = holds[0].clone();
    let seen = w.answers_after(WKEY, "/v5/order/realtime", from);
    let (first, second) = (seen[0], *seen.get(1).unwrap_or(&u64::MAX));
    let earliest = (first + min_age).max(second);
    let at_ms = at / 1_000;
    assert!(
        at_ms >= earliest,
        "the hold at {at_ms} ms, before first_seen {first} + {min_age} ms or the second W1 answer {second}"
    );
    assert!(
        at_ms <= earliest + 5_000,
        "the hold at {at_ms} ms, more than 5 s after {earliest}"
    );
    println!(
        "j2a: hold {} ms after the first W1 answer that held F",
        at_ms - first
    );
    assert_eq!(holds.len(), 1, "{holds:?}");
    assert_eq!(source, HoldSource::W1, "{note}");
    assert!(note.starts_with("W1 foreign "), "the hold's note: {note}");
    // `/api/status` shows the published state, a moment after the record.
    w.wait(
        "/api/status holds watcher_mismatch",
        Duration::from_secs(2),
        || status_holds(&oms).contains(&"watcher_mismatch".to_string()),
    );
    let lines = watcher.log_lines("error: confirmed difference: W1 foreign ");
    assert_eq!(lines.len(), 1, "{lines:?}");
    w.wait("both direct alerts", Duration::from_secs(10), || {
        !telegram_with(&w, "[WATCH P1]", "워처 W1 불일치 확정").is_empty()
            && !telegram_with(&w, "[OMS P1]", OMS_WATCHER_HOLD).is_empty()
    });
    let watch = telegram_with(&w, "[WATCH P1]", "불일치 확정");
    assert_eq!(watch.len(), 1, "{watch:?}");

    // Under the hold: a new plain order is refused, the cancel of R is not.
    let reasons = refused(&w, &oms, "under-hold");
    assert!(
        reasons.contains(&RejectReason::Hold(HoldReason::WatcherMismatch)),
        "{reasons:?}"
    );
    assert!(
        matches!(oms.cancel(r), CancelReply::Requested { .. }),
        "the cancel of R under the hold"
    );
    w.wait("R is closed Cancelled", Duration::from_secs(10), || {
        matches!(closed(&oms, r), Some(CloseReason::Canceled { .. }))
    });
    assert_eq!(
        w.fake
            .with(|b, _| b.by_link(&oms.link(r)).map(|o| o.status)),
        Some("Cancelled"),
        "R at the venue"
    );

    // The operator cancels F on the venue's page (no frame); two W1
    // rounds without it, then the release.
    w.fake.cancel_at_venue(&f, false);
    let gone = w.log_len();
    w.wait("two W1 rounds without F", Duration::from_secs(10), || {
        w.answers_after(WKEY, "/v5/order/realtime", gone).len() >= 2
    });
    clear_hold(
        &w,
        &oms,
        "watcher_mismatch",
        "manual-1 cancelled at the venue",
    );
    let cleared = oms.events("hold_cleared").into_iter().any(|(_, e)| {
        matches!(
            e,
            Event::HoldCleared {
                hold: HoldKind::WatcherMismatch,
                ..
            }
        )
    });
    assert!(
        cleared,
        "no hold_cleared{{watcher_mismatch}} in the journal"
    );
    let after = working(&w, &oms, "after-clear", "29000");
    assert!(
        w.fake.with(|b, _| b.by_link(&oms.link(after)).is_some()),
        "the order after the release is not at the venue"
    );

    // No hold comes back: mismatch_resend_ms and two more W1 rounds.
    let resend = Duration::from_millis(w.watcher_timer("mismatch_resend_ms").unwrap());
    let (since, idx) = (Instant::now(), w.log_len());
    w.wait(
        "mismatch_resend_ms and two W1 rounds after the release",
        Duration::from_secs(10),
        || since.elapsed() >= resend && w.answers_after(WKEY, "/v5/order/realtime", idx).len() >= 2,
    );
    let holds = holds_of(&oms, HoldKind::WatcherMismatch);
    assert_eq!(holds.len(), 1, "a hold came back: {holds:?}");
    assert_eq!(
        watcher.log_lines("error: confirmed difference:").len(),
        1,
        "{:?}",
        watcher.log_lines("")
    );
    assert!(watcher.exit().is_none(), "the watcher ended");
    w.assert_venue_invariants();
}

/// The OMS's `[venue] fills_lag_ms` as the rig wrote it: its fill query
/// ends that long before now. `Timers::Short` writes the line; a
/// configuration without it is a test error, so the guard never compares
/// against a copy of the code default.
fn fills_lag_ms(w: &World) -> u64 {
    std::fs::read_to_string(w.dir.join("o/oms.toml"))
        .unwrap()
        .lines()
        .find_map(|l| l.strip_prefix("fills_lag_ms = "))
        .map(|v| v.trim().parse().unwrap())
        .unwrap_or_else(|| panic!("the OMS configuration the rig wrote has no fills_lag_ms line"))
}

/// The times (ms) of the watcher's W4 answers after log index `from` whose
/// window (`startTime..=endTime`) holds `exec_time`.
fn w4_answers_holding(w: &World, from: usize, exec_time: u64) -> Vec<u64> {
    w.fake.with(|b, _| {
        b.log
            .iter()
            .skip(from)
            .filter(|l| l.key == WKEY && l.path == "/v5/execution/list" && l.answer == "ok")
            .filter(|l| {
                let p = |k: &str| {
                    l.query
                        .split('&')
                        .find_map(|kv| kv.strip_prefix(k))
                        .and_then(|v| v.parse::<u64>().ok())
                };
                p("startTime=").is_some_and(|a| a <= exec_time)
                    && p("endTime=").is_some_and(|e| exec_time <= e)
            })
            .map(|l| l.at_ms)
            .collect()
    })
}

/// The unacknowledged mismatches in `/api/status`: (id, kind, subject).
fn unacked(oms: &Oms) -> Vec<(u64, String, String)> {
    oms.status_http()
        .and_then(|s| s["core"]["mismatches_unacked"].as_array().cloned())
        .unwrap_or_default()
        .iter()
        .map(|m| {
            (
                m["id"].as_u64().unwrap_or(0),
                m["kind"].as_str().unwrap_or("").to_string(),
                m["subject"].as_str().unwrap_or("").to_string(),
            )
        })
        .collect()
}

/// J2b: a venue-only fill (a `Trade` execution with a foreign link,
/// back-dated past `min_age_ms`) is confirmed by W2 (or W4) only after
/// the second W2 answer that showed the planted size, or the second W4
/// sweep that returned the row, whichever is first; the OMS holds plain
/// orders, refuses a new one and accepts a cancel. The OMS books the fill
/// itself (at the latest after a reconnect of its private stream:
/// `external_fill_booked` and its own `ExternalFill` mismatch, at most one
/// `PositionOutOfRange` besides); W2 and W4 then agree. `omsctl ack` and
/// `clear-hold` restore trading, no hold comes back, and the venue's
/// position equals the journal's.
#[test]
fn j2b_a_venue_only_fill_holds_after_two_w2_rounds_and_is_released_after_the_oms_books_it() {
    let mut w = World::new("j2b", Timers::Short, Duration::from_secs(75));
    w.set_watcher("w4_ms", 5_000);
    let min_age = w.watcher_timer("min_age_ms").unwrap();
    let (oms, watcher) = start_both(&w);
    let r = working(&w, &oms, "r1", "29000");

    // The back-date must stay above the OMS's fill-query lag, else the
    // reconnect's one fill query cannot read the row.
    let back = min_age + 500;
    let lag = fills_lag_ms(&w);
    assert!(
        back > lag,
        "the plant is back-dated {back} ms (min_age_ms + 500), not above the OMS's fills_lag_ms {lag} ms"
    );
    // Below the first boot's fill floor the OMS drops a fill unrecorded.
    let floor_ms = oms
        .events("booted")
        .into_iter()
        .find_map(|(_, e)| match e {
            Event::Booted { fill_floor, .. } => Some(fill_floor / 1_000),
            _ => None,
        })
        .unwrap();
    w.wait(
        "an execTime 1 s above the fill floor",
        Duration::from_secs(10),
        || now_ms().saturating_sub(back) >= floor_ms + 1_000,
    );
    let (from, exec_time) = w.fake.with(|b, now| {
        let from = b.log.len();
        let t = now - back;
        b.plant_execution("Trade", "Buy", "0.001", "30000", "manual-fill-1", t);
        (from, t)
    });
    assert_eq!(
        w.fake.with(|b, _| b.position_updated_ms),
        exec_time,
        "the position's updatedTime"
    );
    w.wait(
        "W2 or W4 confirms and the OMS holds",
        Duration::from_secs(15),
        || !holds_of(&oms, HoldKind::WatcherMismatch).is_empty(),
    );
    let holds = holds_of(&oms, HoldKind::WatcherMismatch);
    let (at, source, note) = holds[0].clone();
    let at_ms = at / 1_000;
    let w2 = w.answers_after(WKEY, "/v5/position/list", from);
    let w4 = w4_answers_holding(&w, from, exec_time);
    let earliest = w2
        .get(1)
        .copied()
        .unwrap_or(u64::MAX)
        .min(w4.get(1).copied().unwrap_or(u64::MAX));
    assert!(
        at_ms >= earliest,
        "the hold at {at_ms} ms, before the second W2 answer {:?} and the second W4 sweep {:?} that held the fill",
        w2.get(1),
        w4.get(1)
    );
    println!(
        "j2b: hold {} ms after the plant, {source:?}",
        at_ms - from_ms(&w, from)
    );
    assert_eq!(holds.len(), 1, "{holds:?}");
    assert!(
        matches!(source, HoldSource::W2 | HoldSource::W4),
        "{source:?}: {note}"
    );
    assert!(
        note.starts_with("W2 ") || note.starts_with("W4 "),
        "the hold's note: {note}"
    );
    assert!(
        !watcher
            .log_lines("error: confirmed difference: ")
            .is_empty(),
        "no confirmed difference line"
    );
    w.wait("both direct alerts", Duration::from_secs(10), || {
        !telegram_with(&w, "[WATCH P1]", "불일치 확정").is_empty()
            && !telegram_with(&w, "[OMS P1]", OMS_WATCHER_HOLD).is_empty()
    });
    let reasons = refused(&w, &oms, "under-hold");
    assert!(
        reasons.contains(&RejectReason::Hold(HoldReason::WatcherMismatch)),
        "{reasons:?}"
    );
    assert!(
        matches!(oms.cancel(r), CancelReply::Requested { .. }),
        "the cancel of R under the hold"
    );
    w.wait("R is closed Cancelled", Duration::from_secs(10), || {
        matches!(closed(&oms, r), Some(CloseReason::Canceled { .. }))
    });

    // The OMS books the fill: perhaps already at the cancel of R, at the
    // latest in the reconnect's reconcile round.
    println!(
        "j2b: the fill was booked before the stream drop: {}",
        !oms.events("external_fill_booked").is_empty()
    );
    w.fake.private.drop_connection();
    w.wait(
        "the OMS books the fill and is linked",
        Duration::from_secs(20),
        || {
            !oms.events("external_fill_booked").is_empty()
                && oms.status_http().is_some_and(|s| {
                    s["core"]["link"]["up"] == true && s["core"]["needs"] == json!([])
                })
                && unacked(&oms).iter().any(|(_, k, _)| k == "external_fill")
        },
    );
    let mismatches = unacked(&oms);
    let subject = "instrument:BTCUSDT";
    let fill = mismatches
        .iter()
        .filter(|(_, k, s)| k == "external_fill" && s == subject)
        .count();
    let range = mismatches
        .iter()
        .filter(|(_, k, s)| k == "position_out_of_range" && s == subject)
        .count();
    assert!(
        fill == 1 && range <= 1 && fill + range == mismatches.len(),
        "the OMS's unacknowledged mismatches: {mismatches:?}"
    );
    let raised = oms.events("mismatch_raised").len();
    assert_eq!(raised, mismatches.len(), "mismatches raised in the journal");

    // W2 and W4 agree with the journal now: after one W4 answer that may
    // have judged an older journal, no new confirmed difference for two
    // rounds of W2 and two sweeps of W4.
    let idx = w.log_len();
    w.wait(
        "a W4 sweep after the booking",
        Duration::from_secs(10),
        || !w.answers_after(WKEY, "/v5/execution/list", idx).is_empty(),
    );
    let confirmed = watcher.log_lines("error: confirmed difference: ");
    let idx = w.log_len();
    w.wait(
        "two W2 rounds and two W4 sweeps",
        Duration::from_secs(15),
        || {
            w.answers_after(WKEY, "/v5/position/list", idx).len() >= 2
                && w.answers_after(WKEY, "/v5/execution/list", idx).len() >= 2
        },
    );
    assert_eq!(
        watcher.log_lines("error: confirmed difference: "),
        confirmed,
        "a difference after the OMS booked the fill"
    );
    let checks: std::collections::BTreeSet<String> = confirmed
        .iter()
        .filter_map(|l| {
            l.strip_prefix("error: confirmed difference: ")
                .and_then(|d| d.split(' ').next())
                .map(str::to_string)
        })
        .collect();
    assert!(
        !checks.is_empty() && checks.iter().all(|c| c == "W2" || c == "W4"),
        "{confirmed:?}"
    );
    let watch = telegram_with(&w, "[WATCH P1]", "불일치 확정");
    assert!(
        (1..=2).contains(&watch.len())
            && watch
                .iter()
                .all(|b| b.contains("워처 W2 불일치") || b.contains("워처 W4 불일치")),
        "{watch:?}"
    );

    // The release: every OMS mismatch acknowledged, then the hold cleared.
    for (id, kind, _) in &mismatches {
        let (code, out) = oms.omsctl(&[
            "ack",
            "--id",
            &id.to_string(),
            "--note",
            &format!("{kind}: manual-fill-1 on the venue's page"),
        ]);
        assert_eq!(code, Some(0), "ack {id}: {out}");
    }
    w.wait("no unacknowledged mismatch", Duration::from_secs(5), || {
        unacked(&oms).is_empty()
    });
    clear_hold(
        &w,
        &oms,
        "watcher_mismatch",
        "manual-fill-1 booked by the OMS",
    );
    let resend = Duration::from_millis(w.watcher_timer("mismatch_resend_ms").unwrap());
    let (since, idx) = (Instant::now(), w.log_len());
    w.wait(
        "mismatch_resend_ms and two W2 rounds after the release",
        Duration::from_secs(10),
        || since.elapsed() >= resend && w.answers_after(WKEY, "/v5/position/list", idx).len() >= 2,
    );
    assert_eq!(
        holds_of(&oms, HoldKind::WatcherMismatch).len(),
        1,
        "a hold came back"
    );
    let _ = working(&w, &oms, "after-clear", "29000");

    // The venue's position is the journal's: +0.001.
    let status = oms.status_http().unwrap();
    assert_eq!(w.fake.with(|b, _| b.size), 1, "the book's position");
    assert_eq!(
        status["account"][SYMBOL]["pos"].as_str(),
        Some("0.001"),
        "the journal's position: {}",
        status["account"]
    );
    assert!(watcher.exit().is_none(), "the watcher ended");
    w.assert_venue_invariants();
}

/// The time (ms) of the fake's log entry at `idx`, or now if there is none
/// yet.
fn from_ms(w: &World, idx: usize) -> u64 {
    w.fake
        .with(|b, now| b.log.get(idx).map_or(now, |l| l.at_ms))
}

/// The OMS's Short `[core.policy.monitor]` and `[runtime] tick_ms`
/// (`joint_rig::Timers::Short`), in µs.
const HEARTBEAT_MAX_AGE_US: u64 = 10_000_000;
const CHECK_MAX_AGE_US: u64 = 5_000_000;
const UNMONITORED_CANCEL_US: u64 = 10_000_000;
const TICK_US: u64 = 50_000;

/// The watcher's heartbeat records: record time (µs) and check times.
fn heartbeats(oms: &Oms) -> Vec<(Ts, std::collections::BTreeMap<String, Option<Ts>>)> {
    oms.records()
        .into_iter()
        .flat_map(|r| {
            r.events.into_iter().filter_map(move |e| match e {
                Event::WatcherHeartbeat { checks, .. } => Some((r.at, checks)),
                _ => None,
            })
        })
        .collect()
}

/// The times (ms) of the OMS's cancels of `link` at the fake (WS Trade or
/// REST, `OKEY`).
fn okey_cancels(w: &World, link: &str) -> Vec<u64> {
    w.fake.with(|b, _| {
        b.log
            .iter()
            .filter(|l| l.key == OKEY && (l.path == "order.cancel" || l.path == "/v5/order/cancel"))
            .filter(|l| {
                serde_json::from_str::<serde_json::Value>(&l.body)
                    .ok()
                    .is_some_and(|v| v["orderLinkId"] == link)
            })
            .map(|l| l.at_ms)
            .collect()
    })
}

/// J6a: the watcher stopped (SIGTERM) sends no heartbeat; the OMS stays
/// monitored for `heartbeat_max_age` after the last heartbeat record, then
/// closes the gate within a tick and a second, refuses plain orders and,
/// `unmonitored_cancel` after the close record (`monitor_changed`),
/// cancels the resting plain order once. The restarted watcher opens the gate again within 10 s of
/// its boot replay, and a plain order is accepted.
#[test]
fn j6a_a_stopped_watcher_closes_the_gate_after_the_heartbeat_limit_and_its_return_opens_it() {
    let w = World::new("j6a", Timers::Short, Duration::from_secs(90));
    let (oms, mut watcher) = start_both(&w);
    let r = working(&w, &oms, "r1", "29000");
    let link = oms.link(r);

    watcher.sigterm();
    w.wait("the watcher ends", Duration::from_secs(5), || {
        watcher.exit().is_some()
    });
    assert_eq!(
        watcher.exit().unwrap().0.code(),
        Some(0),
        "the watcher's exit"
    );
    w.wait("the gate closes", Duration::from_secs(15), || {
        !monitor_changes(&oms, false).is_empty()
    });
    let closed_at = monitor_changes(&oms, false)[0];
    let last_beat = heartbeats(&oms)
        .into_iter()
        .map(|(at, _)| at)
        .filter(|at| *at <= closed_at)
        .max()
        .unwrap();
    let delay = closed_at - last_beat;
    println!("gate closed after {} ms", delay / 1_000);
    assert!(
        delay > HEARTBEAT_MAX_AGE_US,
        "the gate closed {delay} µs after the last heartbeat record"
    );
    assert!(
        delay <= HEARTBEAT_MAX_AGE_US + TICK_US + 1_000_000,
        "the gate closed {delay} µs after the last heartbeat record"
    );
    assert!(
        heartbeats(&oms).iter().all(|(at, _)| *at <= closed_at),
        "a heartbeat after the watcher ended"
    );
    let reasons = refused(&w, &oms, "unmonitored");
    assert!(
        reasons.contains(&RejectReason::Hold(HoldReason::Unmonitored)),
        "{reasons:?}"
    );

    // Stage 2: one cancel of R, strictly after unmonitored_cancel.
    let due_ms = (closed_at + UNMONITORED_CANCEL_US) / 1_000;
    let by_ms = due_ms + (TICK_US + 1_000_000) / 1_000;
    w.wait("the stage-2 cancel of R", Duration::from_secs(15), || {
        !okey_cancels(&w, &link).is_empty()
    });
    let cancels = okey_cancels(&w, &link);
    assert!(
        cancels[0] >= due_ms && cancels[0] <= by_ms,
        "the cancel of R at {} ms; due after {due_ms}, by {by_ms}",
        cancels[0]
    );
    println!(
        "stage-2 cancel {} ms after the gate closed",
        cancels[0] - closed_at / 1_000
    );
    w.wait("R is closed Cancelled", Duration::from_secs(10), || {
        matches!(closed(&oms, r), Some(CloseReason::Canceled { .. }))
    });
    assert_eq!(
        okey_cancels(&w, &link).len(),
        1,
        "R was cancelled more than once"
    );

    // The watcher returns.
    watcher.start();
    w.wait("the watcher's boot replay", Duration::from_secs(20), || {
        watcher.log_containing("boot replay done").len() >= 2
    });
    let replayed = Instant::now();
    w.wait(
        "the gate opens within 10 s of the boot replay",
        Duration::from_secs(10),
        || monitored(&oms) == Some(true),
    );
    println!(
        "gate opened {} ms after the boot replay line was seen",
        replayed.elapsed().as_millis()
    );
    let _ = working(&w, &oms, "back", "29000");
    assert_watcher_quiet(&w, &oms, &watcher);
    w.assert_venue_invariants();
}

/// J6b: the watcher keeps sending heartbeats while its W1 and W2 requests
/// fail (503 for its key only); the heartbeats carry the last success
/// times unchanged, and the OMS closes the gate at the first heartbeat
/// recorded more than `check_max_age` after the older of them, and
/// refuses a plain order; its own requests are still answered (its cancel
/// of a resting order goes through). When the requests succeed again the
/// gate opens within two heartbeats. Failed rounds judge nothing: no
/// mismatch throughout.
#[test]
fn j6b_failing_watcher_checks_close_the_gate_after_the_check_age_limit() {
    let w = World::new("j6b", Timers::Short, Duration::from_secs(60));
    let heartbeat_ms = w.watcher_timer("heartbeat_ms").unwrap();
    let (oms, watcher) = start_both(&w);
    let r = working(&w, &oms, "r1", "29000");
    let paths = ["/v5/order/realtime", "/v5/position/list"];
    let from = w.fake.with(|b, _| {
        for p in paths {
            b.fault(WKEY, p, http_fake::Answer::Reply(503, "{}".into()));
        }
        b.log.len()
    });
    w.wait("the gate closes", Duration::from_secs(10), || {
        !monitor_changes(&oms, false).is_empty()
    });
    let closed_at = monitor_changes(&oms, false)[0];
    // Two more heartbeats after the close: the checks stay frozen.
    w.wait(
        "two heartbeats after the close",
        Duration::from_secs(5),
        || {
            heartbeats(&oms)
                .iter()
                .filter(|(at, _)| *at > closed_at)
                .count()
                >= 2
        },
    );
    let beats = heartbeats(&oms);
    let (_, at_close) = beats
        .iter()
        .find(|(at, _)| *at == closed_at)
        .cloned()
        .unwrap_or_else(|| w.fail("the gate closed in a record without a heartbeat"));
    let w1w2 =
        |c: &std::collections::BTreeMap<String, Option<Ts>>| (c["W1"].unwrap(), c["W2"].unwrap());
    let (t1, t2) = w1w2(&at_close);
    for (at, c) in beats.iter().filter(|(at, _)| *at > closed_at) {
        assert_eq!(w1w2(c), (t1, t2), "W1/W2 moved in the heartbeat at {at}");
    }
    let older = t1.min(t2);
    let delay = closed_at - older;
    println!(
        "gate closed {} ms after the older of the last W1/W2 successes",
        delay / 1_000
    );
    assert!(delay > CHECK_MAX_AGE_US, "closed {delay} µs after {older}");
    assert!(
        delay <= CHECK_MAX_AGE_US + heartbeat_ms * 1_000 + 1_000_000,
        "closed {delay} µs after {older}"
    );
    let reasons = refused(&w, &oms, "stale-checks");
    assert!(
        reasons.contains(&RejectReason::Hold(HoldReason::Unmonitored)),
        "{reasons:?}"
    );
    // The OMS's own requests are answered: its cancel of R goes through.
    assert!(
        matches!(oms.cancel(r), CancelReply::Requested { .. }),
        "the cancel of R while unmonitored"
    );
    w.wait("R is closed Cancelled", Duration::from_secs(10), || {
        matches!(closed(&oms, r), Some(CloseReason::Canceled { .. }))
    });
    let theirs = w.fake.with(|b, _| {
        b.log
            .iter()
            .skip(from)
            .filter(|l| l.key == OKEY)
            .map(|l| l.answer.clone())
            .collect::<Vec<_>>()
    });
    assert!(
        !theirs.is_empty() && theirs.iter().all(|a| a == "ok"),
        "the OMS's requests during the fault: {theirs:?}"
    );
    let failed = w.fake.with(|b, _| {
        b.log
            .iter()
            .skip(from)
            .filter(|l| l.key == WKEY && paths.contains(&l.path.as_str()) && l.answer == "fault")
            .count()
    });
    assert!(failed >= 4, "{failed} faulted W1/W2 requests");
    println!(
        "j6b: {failed} faulted W1/W2 requests, {} OMS requests answered meanwhile",
        theirs.len()
    );

    // The requests succeed again: the gate opens within two heartbeats.
    w.fake.with(|b, _| {
        for p in paths {
            b.clear_fault(WKEY, p);
        }
    });
    let cleared = Instant::now();
    w.wait(
        "the gate opens within two heartbeats",
        Duration::from_millis(2 * heartbeat_ms + 1_000),
        || monitored(&oms) == Some(true),
    );
    println!(
        "gate opened {} ms after the faults were removed",
        cleared.elapsed().as_millis()
    );
    let _ = working(&w, &oms, "fresh", "29000");
    assert_watcher_quiet(&w, &oms, &watcher);
    w.assert_venue_invariants();
}

/// Every `kill_loop_ended`: record time (µs) and outcome.
fn kill_loop_ends(oms: &Oms) -> Vec<(Ts, KillOutcome)> {
    oms.events("kill_loop_ended")
        .into_iter()
        .filter_map(|(at, e)| match e {
            Event::KillLoopEnded { outcome, .. } => Some((at, outcome)),
            _ => None,
        })
        .collect()
}

/// The OMS's Short `[venue] kill_after_ms` and `kill_watch_ms`
/// (`joint_rig::Timers::Short`).
const KILL_AFTER_MS: u64 = 10_000;
const KILL_WATCH_MS: u64 = 10_000;

/// What the watcher must not have said about a difference: a
/// `watcher_mismatch` hold, a `confirmed difference` line, a `[WATCH P1]`
/// mismatch message.
fn assert_no_mismatch(w: &World, oms: &Oms, watcher: &Watcher) {
    let holds = holds_of(oms, HoldKind::WatcherMismatch);
    let lines = watcher.log_containing("confirmed difference");
    let watch = telegram_with(w, "[WATCH P1]", "불일치");
    if !holds.is_empty() || !lines.is_empty() || !watch.is_empty() {
        w.fail(&format!(
            "a mismatch: holds {holds:?}, lines {lines:?}, telegram {watch:?}"
        ));
    }
}

/// J4: the private stream goes half-open (no pong, no reconnect) while two
/// plain orders rest. The OMS latches its kill (`hold_set{kill, source:
/// stream}`) between 9 s and 11 s after the last pong, its cancel-alls
/// empty the book and its loop confirms zero after `kill_watch_ms`; its
/// reconciler is off while the link is down, so the journal keeps both
/// orders open and acked. The watcher meanwhile goes on with its rounds and
/// heartbeats, W5 does not fire (`/api/status` answers), and it raises no
/// mismatch through the outage and three W1 rounds after the
/// confirmation: an order missing at the venue during a kill is not a
/// difference (W1, C-WATCH part 5). When the stream answers again the
/// reconcile closes both orders, `clear-hold kill` is taken, a plain order
/// is accepted, and still no mismatch for two rounds.
#[test]
fn j4_a_ten_second_stream_outage_kills_in_the_oms_and_the_watcher_raises_nothing() {
    let w = World::new("j4", Timers::Short, Duration::from_secs(90));
    let (oms, watcher) = start_both(&w);
    let r1 = working(&w, &oms, "r1", "29000");
    let r2 = working(&w, &oms, "r2", "28900");
    std::thread::sleep(Duration::from_millis(1_500));

    w.fake
        .private
        .silent
        .store(true, std::sync::atomic::Ordering::SeqCst);
    std::thread::sleep(Duration::from_millis(50));
    let last = w
        .fake
        .private
        .last_pong
        .lock()
        .unwrap()
        .expect("a pong before the outage");
    let last_ms = now_ms() - last.elapsed().as_millis() as u64;
    let at_9 = last + Duration::from_millis(KILL_AFTER_MS - 1_000);
    w.never(
        "the OMS latched its kill or sent a cancel-all before 9 s",
        at_9.saturating_duration_since(Instant::now()),
        || {
            !holds_of(&oms, HoldKind::Kill).is_empty()
                || !w.fake.with(|b, _| b.cancel_alls(OKEY)).is_empty()
        },
    );
    w.wait(
        "the OMS latches its kill by 11 s after the last pong",
        (last + Duration::from_millis(KILL_AFTER_MS + 1_000))
            .saturating_duration_since(Instant::now()),
        || !holds_of(&oms, HoldKind::Kill).is_empty(),
    );
    let kills = holds_of(&oms, HoldKind::Kill);
    assert_eq!(kills.len(), 1, "{kills:?}");
    let (latched_at, source, note) = kills[0].clone();
    assert_eq!(source, HoldSource::Stream, "{note}");
    println!(
        "j4: kill latched {} ms after the last pong",
        latched_at / 1_000 - last_ms
    );
    w.wait(
        "the OMS's cancel-alls empty the book",
        Duration::from_secs(5),
        || {
            w.fake
                .with(|b, _| !b.cancel_alls(OKEY).is_empty() && b.open_orders().is_empty())
        },
    );
    // The reconciler is off while the link is down: both orders stay open
    // in the journal.
    assert_eq!(
        (closed(&oms, r1), closed(&oms, r2)),
        (None, None),
        "an order closed in the journal while the link is down"
    );
    w.wait(
        "the OMS's kill loop confirms zero",
        Duration::from_secs(KILL_WATCH_MS / 1_000 + 10),
        || !kill_loop_ends(&oms).is_empty(),
    );
    let ends = kill_loop_ends(&oms);
    assert_eq!(ends.len(), 1, "{ends:?}");
    let (ended_at, outcome) = ends[0];
    assert_eq!(outcome, KillOutcome::Confirmed);
    println!(
        "j4: the loop confirmed {} ms after the latch",
        (ended_at - latched_at) / 1_000
    );
    assert!(
        ended_at - latched_at >= KILL_WATCH_MS * 1_000,
        "the loop confirmed {} µs after the latch",
        ended_at - latched_at
    );
    // The loop confirmed, but the journal still holds both orders open:
    // the release is refused, so W1's kill exemption cannot outlive the
    // OMS's catch-up.
    let (code, _, err) = oms.omsctl_full(&[
        "clear-hold",
        "--reason",
        "kill",
        "--note",
        "before the reconcile",
    ]);
    assert!(
        code != Some(0) && err.contains("kill_orders_open: 2 open orders"),
        "clear-hold with the orders open in the journal: exit {code:?}, {err:?}"
    );
    // Three W1 rounds after the confirmation, the stream still down.
    let idx = w.log_len();
    w.wait(
        "three W1 rounds after the confirmation",
        Duration::from_secs(5),
        || w.answers_after(WKEY, "/v5/order/realtime", idx).len() >= 3,
    );
    assert_eq!(
        (closed(&oms, r1), closed(&oms, r2)),
        (None, None),
        "an order closed in the journal while the link is down"
    );
    // The watcher saw the outage through: rounds and heartbeats went on,
    // W5 did not fire.
    let latched_ms = latched_at / 1_000;
    let rounds = w
        .requests(WKEY, "/v5/order/realtime")
        .iter()
        .filter(|t| **t > latched_ms)
        .count();
    let beats = heartbeats(&oms)
        .iter()
        .filter(|(at, _)| *at > latched_at)
        .count();
    assert!(
        rounds >= 10 && beats >= 5,
        "{rounds} W1 requests and {beats} heartbeats after the latch"
    );
    assert!(
        watcher.log_containing("W5").is_empty() && !w.marker().exists(),
        "W5 fired: {:?}",
        watcher.log_containing("W5")
    );
    assert!(w.systemctl_log().is_empty(), "{:?}", w.systemctl_log());
    assert_no_mismatch(&w, &oms, &watcher);

    // The stream answers again: the link comes up and the reconcile closes
    // both orders.
    w.fake
        .private
        .silent
        .store(false, std::sync::atomic::Ordering::SeqCst);
    w.wait(
        "the link is up and both orders are closed",
        Duration::from_secs(15),
        || {
            oms.status_admin()
                .is_some_and(|s| s["core"]["link"]["up"] == true)
                && matches!(closed(&oms, r1), Some(CloseReason::Canceled { .. }))
                && matches!(closed(&oms, r2), Some(CloseReason::Canceled { .. }))
        },
    );
    clear_hold(
        &w,
        &oms,
        "kill",
        "stream back, venue checked, zero open orders",
    );
    w.wait(
        "/api/status holds no kill_latched",
        Duration::from_secs(2),
        || !status_holds(&oms).contains(&"kill_latched".to_string()),
    );
    let after = working(&w, &oms, "after-kill", "29000");
    assert!(
        w.fake.with(|b, _| b.by_link(&oms.link(after)).is_some()),
        "the order after the release is not at the venue"
    );
    let idx = w.log_len();
    w.wait(
        "two W1 rounds after the release",
        Duration::from_secs(5),
        || w.answers_after(WKEY, "/v5/order/realtime", idx).len() >= 2,
    );
    assert_watcher_quiet(&w, &oms, &watcher);
    assert!(watcher.exit().is_none(), "the watcher ended");
    w.assert_venue_invariants();
}

/// The wall-clock time (ms) of the file's last change.
fn mtime_ms(path: &std::path::Path) -> u64 {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_millis() as u64)
}

/// J3: the OMS hangs (SIGSTOP) with two plain orders resting. The
/// watcher's W5 fires `silence_ms` after the last good `/api/status`
/// answer, not before: K1 writes the marker, K2's `hello` gets no reply
/// from the stopped process, K3 stops the unit with the fake `systemctl`
/// (the marker is there at the call) and the OMS ends on the SIGTERM path
/// within `stop_wait_ms`; then the watcher's cancel-alls (the fake leaves
/// one order after the first) empty the book, and the kill ends after
/// `watch_ms` of zero, with a start and a final `[WATCH P1]` notice. The
/// restarted OMS reads the marker at boot and is latched
/// (`hold_set{kill, source: marker}` in its boot record): plain and
/// reduce-only orders are refused, `clear-hold kill` is refused until its
/// own kill loop confirmed zero, then taken, the marker is removed and a
/// plain order is accepted. The watcher across the restart: no second
/// kill, W5 armed again, no mismatch for the orders the restarted OMS
/// still had open in its journal while it was latched.
#[test]
fn j3_a_hung_oms_is_killed_by_the_watcher_and_boots_latched_until_the_operator_clears_it() {
    let mut w = World::new("j3", Timers::Short, Duration::from_secs(120));
    w.set_watcher("silence_ms", 6_000);
    // The OMS's own stream kill must not latch when the stopped process
    // resumes (spec J3): 30 s instead of 10 s.
    w.set_oms("kill_after_ms", 30_000);
    let silence = w.watcher_timer("silence_ms").unwrap();
    let probe = w.watcher_timer("probe_ms").unwrap();
    let timeout = w.watcher_timer("request_timeout_ms").unwrap();
    let stop_wait = w.watcher_timer("stop_wait_ms").unwrap();
    let watch = w.watcher_timer("watch_ms").unwrap();
    let (mut oms, watcher) = start_both(&w);
    let r1 = working(&w, &oms, "r1", "29000");
    let r2 = working(&w, &oms, "r2", "28900");
    w.wait("W5 is armed", Duration::from_secs(10), || {
        metric(&w.vm, "watcher_w5_armed") == Some(1.0)
    });
    w.fake.with(|b, _| b.leave_one_once(WKEY));

    // 1. Nothing before silence_ms after the last good probe, which can
    // precede the SIGSTOP by probe_ms plus the time the OMS took to answer.
    let stopped = now_ms();
    oms.sigstop();
    let quiet_until = stopped + silence - probe - timeout;
    w.never(
        "the marker or a WKEY cancel-all before silence_ms after the last good probe",
        Duration::from_millis(quiet_until.saturating_sub(now_ms())),
        || w.marker().exists() || !w.fake.with(|b, _| b.cancel_alls(WKEY)).is_empty(),
    );

    // 2. K1 then K3, both before the first cancel-all.
    // The systemctl call is waited for first, so a stop without the
    // marker shows as `marker=absent` in it.
    w.wait("the systemctl stop", Duration::from_secs(15), || {
        !w.systemctl_log().is_empty()
    });
    let call = w.systemctl_log()[0].clone();
    let want = format!("--no-ask-password stop -- {UNIT} marker=present ");
    assert!(call.starts_with(&want), "the systemctl call: {call:?}");
    let called_ms: u64 = call.rsplit(' ').next().unwrap().parse().unwrap();
    let marker_ms = mtime_ms(&w.marker());
    let text = std::fs::read_to_string(w.marker()).unwrap_or_default();
    assert!(
        text.starts_with(&format!("watcher {WATCHER_NAME} W5")),
        "the marker: {text:?}"
    );
    assert!(
        marker_ms <= called_ms,
        "the marker at {marker_ms} ms, after the systemctl call at {called_ms}"
    );
    println!("j3: marker {} ms after the SIGSTOP", marker_ms - stopped);

    // 3. The OMS ends on the SIGTERM path within stop_wait_ms.
    w.wait(
        "the OMS ends",
        Duration::from_millis(stop_wait + 1_000),
        || oms.exit().is_some(),
    );
    let ended_ms = now_ms();
    assert_eq!(oms.exit().unwrap().0.code(), Some(0), "the OMS's exit");
    assert!(
        ended_ms <= called_ms + stop_wait,
        "the OMS ended {} ms after the systemctl call",
        ended_ms - called_ms
    );
    assert!(
        oms.log().contains("\"msg\":\"stopped cleanly\""),
        "not the SIGTERM path (drain, final snapshot, exit 0)"
    );

    // 4. The watcher's cancel-all loop: the first leaves one order, the
    // count sees it, a second empties the book; the kill ends after
    // watch_ms of zero.
    w.wait(
        "two WKEY cancel-alls and an empty book",
        Duration::from_secs(10),
        || {
            w.fake
                .with(|b, _| b.cancel_alls(WKEY).len() >= 2 && b.open_orders().is_empty())
        },
    );
    let cancel_alls = w.fake.with(|b, _| b.cancel_alls(WKEY));
    assert!(
        cancel_alls[0] >= called_ms,
        "a WKEY cancel-all at {} ms, before the systemctl call at {called_ms}",
        cancel_alls[0]
    );
    let counted = w
        .requests(WKEY, "/v5/order/realtime")
        .iter()
        .filter(|t| **t >= cancel_alls[0] && **t <= cancel_alls[1])
        .count();
    assert!(counted >= 1, "no count between the two cancel-alls");
    let mut kill_ended_ms = 0;
    w.wait(
        "the watcher's kill ends",
        Duration::from_millis(watch + 10_000),
        || {
            kill_ended_ms = now_ms();
            !watcher
                .log_containing("the kill ended: zero open orders confirmed")
                .is_empty()
        },
    );
    assert!(
        kill_ended_ms >= cancel_alls[1] + watch,
        "the kill ended {} ms after the second cancel-all, watch_ms {watch}",
        kill_ended_ms - cancel_alls[1]
    );
    println!(
        "j3: systemctl {} ms after the marker, the OMS ended {} ms after the call, the kill ended {} ms after the second cancel-all",
        called_ms - marker_ms,
        ended_ms - called_ms,
        kill_ended_ms - cancel_alls[1]
    );
    let ended = watcher.log_containing("the kill ended:");
    assert_eq!(ended.len(), 1, "{ended:?}");
    assert!(ended[0].contains("the halt is not owed"), "{ended:?}");

    // 5. The start and final notices, each with the position block.
    w.wait("the final kill notice", Duration::from_secs(5), || {
        !telegram_with(&w, "[WATCH P1]", "워처 킬 정리 완료").is_empty()
    });
    let start = telegram_with(&w, "[WATCH P1]", "워처 킬 시작(");
    assert_eq!(start.len(), 1, "{start:?}");
    assert!(
        start[0].contains("킬 마커: 씀.")
            && start[0].contains(&format!("systemctl stop {UNIT}: 정지됨"))
            && start[0].contains("포지션(거래소"),
        "the start notice: {}",
        start[0]
    );
    let last = telegram_with(&w, "[WATCH P1]", "워처 킬 정리 완료");
    assert_eq!(last.len(), 1, "{last:?}");
    assert!(
        last[0].contains("다음 시작은 킬 마커로 래치됨") && last[0].contains("포지션(거래소"),
        "the final notice: {}",
        last[0]
    );

    // 6. The operator restarts the OMS: latched by the marker.
    let boot_from = oms.records().len();
    oms.start(&[]);
    let s = oms.status_admin().unwrap();
    assert_eq!(
        s["node"]["kill_marker"]["present_at_boot"], true,
        "{}",
        s["node"]
    );
    assert!(
        status_holds(&oms).contains(&"kill_latched".to_string()),
        "{}",
        s["core"]
    );
    let kills = holds_of(&oms, HoldKind::Kill);
    assert_eq!(kills.len(), 1, "{kills:?}");
    let (at, source, note) = kills[0].clone();
    assert_eq!(source, HoldSource::Marker, "{note}");
    assert!(
        note.starts_with(&format!("marker: watcher {WATCHER_NAME} W5")),
        "{note}"
    );
    let boot_record = oms
        .records()
        .into_iter()
        .skip(boot_from)
        .find(|r| r.at == at);
    assert!(
        boot_record.is_some_and(|r| r.events.iter().any(|e| e.name() == "booted")),
        "the kill hold is not in the boot record"
    );
    let reasons = refused(&w, &oms, "latched");
    assert!(
        reasons.contains(&RejectReason::Hold(HoldReason::KillLatched)),
        "{reasons:?}"
    );
    let NewOrderReply::Rejected { reasons } =
        oms.new_order("latched-ro", Side::Sell, "0.001", "31000", true)
    else {
        w.fail("a reduce-only order was accepted under the kill latch")
    };
    assert!(
        reasons.contains(&RejectReason::Hold(HoldReason::KillLatched)),
        "{reasons:?}"
    );
    let (code, _, err) =
        oms.omsctl_full(&["clear-hold", "--reason", "kill", "--note", "too early"]);
    assert!(
        code != Some(0) && err.contains("kill_unconfirmed"),
        "clear-hold before the loop confirmed: exit {code:?}, {err:?}"
    );
    assert!(
        w.marker().exists(),
        "a refused clear-hold removed the marker"
    );
    w.wait(
        "the restarted OMS's kill loop confirms zero and both orders are closed",
        Duration::from_secs(25),
        || {
            kill_loop_ends(&oms)
                .iter()
                .any(|(_, o)| *o == KillOutcome::Confirmed)
                && closed(&oms, r1).is_some()
                && closed(&oms, r2).is_some()
        },
    );
    clear_hold(
        &w,
        &oms,
        "kill",
        "watcher kill: venue checked, zero open orders",
    );
    w.wait("the marker is removed", Duration::from_secs(2), || {
        !w.marker().exists()
    });
    let ref_from = oms.events("ref_price_updated").len();
    w.wait(
        "a ticker and the gate open after the restart",
        Duration::from_secs(20),
        || oms.events("ref_price_updated").len() > ref_from && monitored(&oms) == Some(true),
    );
    let after = working(&w, &oms, "after-kill", "29000");
    assert!(
        w.fake.with(|b, _| b.by_link(&oms.link(after)).is_some()),
        "the order after the release is not at the venue"
    );

    // 7. The watcher across the restart.
    w.wait("W5 armed again", Duration::from_secs(10), || {
        metric(&w.vm, "watcher_w5_armed") == Some(1.0)
    });
    let idx = w.log_len();
    w.wait(
        "two W1 rounds after the release",
        Duration::from_secs(5),
        || w.answers_after(WKEY, "/v5/order/realtime", idx).len() >= 2,
    );
    assert_eq!(
        watcher.log_containing("the kill ended:").len(),
        1,
        "a second kill"
    );
    assert_eq!(w.systemctl_log().len(), 1, "{:?}", w.systemctl_log());
    assert_eq!(
        telegram_with(&w, "[WATCH P1]", "워처 킬 시작(").len(),
        1,
        "a second kill notice"
    );
    let from_watcher: Vec<_> = holds_of(&oms, HoldKind::Kill)
        .into_iter()
        .filter(|(_, s, _)| *s != HoldSource::Marker)
        .collect();
    assert!(from_watcher.is_empty(), "{from_watcher:?}");
    assert_no_mismatch(&w, &oms, &watcher);
    assert!(watcher.exit().is_none(), "the watcher ended");
    w.assert_venue_invariants();
}

/// The OMS's Short `[core.policy] absence_margin_us`
/// (`joint_rig::Timers::Short`).
const ABSENCE_MARGIN_US: u64 = 300_000;

/// The journal's events about order `id` named `name`, with their record
/// times.
fn events_of(oms: &Oms, name: &str, id: OrderId) -> Vec<(Ts, Event)> {
    oms.events(name)
        .into_iter()
        .filter(|(_, e)| e.order_id() == Some(id))
        .collect()
}

/// The one order the journal accepted (a crashed node's reply may never
/// have left).
fn the_accepted_order(w: &World, oms: &Oms) -> OrderId {
    let ids: Vec<OrderId> = oms
        .events("order_accepted")
        .into_iter()
        .filter_map(|(_, e)| e.order_id())
        .collect();
    match ids[..] {
        [id] => id,
        _ => w.fail(&format!("not one accepted order in the journal: {ids:?}")),
    }
}

/// The OKEY lookups at the fake after log index `from` of `path` whose
/// query names `orderLinkId=<link>`, with their answers.
fn lookups_by_link(w: &World, from: usize, path: &str, link: &str) -> Vec<String> {
    let by = format!("orderLinkId={link}");
    w.fake.with(|b, _| {
        b.log
            .iter()
            .skip(from)
            .filter(|l| l.key == OKEY && l.path == path && l.query.contains(&by))
            .map(|l| l.answer.clone())
            .collect()
    })
}

/// What a restart of the OMS must not have caused in the watcher: besides
/// [`assert_watcher_quiet`], no W5 kill line and no `systemctl` call; the
/// watcher still runs. Waits first for `min_age_ms` and three W1 rounds
/// after log index `from`, so a difference seen then would be confirmed.
fn assert_watcher_quiet_after(w: &World, oms: &Oms, watcher: &Watcher, from: usize) {
    let start = Instant::now();
    w.wait(
        "min_age_ms and three W1 rounds",
        Duration::from_secs(10),
        || {
            start.elapsed() >= Duration::from_millis(w.timers.min_age_ms())
                && w.answers_after(WKEY, "/v5/order/realtime", from).len() >= 3
        },
    );
    assert_watcher_quiet(w, oms, watcher);
    let w5 = watcher.log_containing("error: W5");
    assert!(w5.is_empty(), "W5 fired: {w5:?}");
    assert!(w.systemctl_log().is_empty(), "{:?}", w.systemctl_log());
    assert!(watcher.exit().is_none(), "the watcher ended");
}

/// J5a: the node is killed (SIGKILL) at the moment its WS Trade create
/// reaches the venue: the fake books the order, sends the signal from its
/// own thread and neither answers nor pushes an `order` frame, so the
/// journal holds the accepted order without a send result. The restarted
/// node marks it `order_in_doubt{Restart}` in its boot record, finds it
/// open by its `orderLinkId` and works it without sending it again: one
/// create for that link over WS Trade and REST across both processes. A
/// cancel then closes it at the venue and in the journal. The watcher
/// raises nothing across the restart (W1 skips orders in doubt and unacked
/// ones; W5's silence is longer than the restart).
#[test]
fn j5a_a_node_killed_while_its_create_is_at_the_venue_restarts_into_one_order_and_no_mismatch() {
    use std::os::unix::process::ExitStatusExt;
    let mut w = World::new("j5a", Timers::Short, Duration::from_secs(60));
    w.set_watcher("silence_ms", 20_000);
    let (mut oms, watcher) = start_both(&w);
    let first_pid = oms.pid().unwrap();

    w.fake.silent_for_next_create(oms.sigkill_hook());
    let seen = oms.try_new_order(
        "j5a",
        Side::Buy,
        "0.001",
        "29000",
        false,
        oms_api::client::now_us() + 30_000_000,
    );
    w.wait(
        "the fake's create hook kills the OMS",
        Duration::from_secs(10),
        || oms.exit().is_some(),
    );
    let (status, _) = oms.exit().unwrap();
    assert_eq!(status.signal(), Some(9), "not killed by SIGKILL: {status:?}");
    let id = the_accepted_order(&w, &oms);
    // The reply leaves before the gateway gets the command, but the
    // signal may still beat its write to the socket.
    match &seen {
        Ok(NewOrderReply::Accepted { order_id }) => assert_eq!(*order_id, id),
        Ok(r) => w.fail(&format!("the order was not accepted: {r:?}")),
        Err(_) => {}
    }
    let link = oms.link(id);
    assert_eq!(
        w.fake
            .with(|b, _| b.by_link(&link).map(|o| (o.is_open(), b.creates_for(&link)))),
        Some((true, 1)),
        "the create did not reach the venue once"
    );
    for name in ["order_acked", "order_in_doubt", "order_closed"] {
        let e = events_of(&oms, name, id);
        assert!(e.is_empty(), "the killed node recorded {name}: {e:?}");
    }

    // The operator restarts the OMS.
    let from = w.log_len();
    oms.start(&[]);
    assert_ne!(oms.pid(), Some(first_pid));
    let doubt = events_of(&oms, "order_in_doubt", id);
    assert!(
        matches!(
            doubt[..],
            [(
                _,
                Event::OrderInDoubt {
                    reason: DoubtReason::Restart,
                    ..
                }
            )]
        ),
        "{doubt:?}"
    );
    let boots: Vec<Ts> = oms.events("booted").into_iter().map(|(at, _)| at).collect();
    assert_eq!(boots.len(), 2, "{boots:?}");
    assert_eq!(doubt[0].0, boots[1], "the doubt is not in the boot record");
    let up = Instant::now();
    w.wait(
        "the restarted OMS finds the order open and works it",
        Duration::from_secs(10),
        || {
            let creates = w.fake.with(|b, _| b.creates_for(&link));
            if creates > 1 {
                w.fail(&format!("{creates} creates for {link} after the restart"));
            }
            let o = oms.order(id);
            o.phase == Phase::Working && o.doubt.is_none()
        },
    );
    println!(
        "j5a: the order worked {} ms after the restarted OMS was up",
        up.elapsed().as_millis()
    );
    let found = lookups_by_link(&w, from, "/v5/order/realtime", &link);
    assert!(
        found.first().is_some_and(|a| a == "ok"),
        "no answered realtime lookup by orderLinkId after the restart: {found:?}"
    );
    assert!(
        events_of(&oms, "order_closed", id).is_empty(),
        "the order was closed while it is open at the venue"
    );
    w.wait("the gate opens again", Duration::from_secs(20), || {
        monitored(&oms) == Some(true)
    });

    assert!(
        matches!(oms.cancel(id), CancelReply::Requested { .. }),
        "the cancel was not taken"
    );
    w.wait(
        "the cancel closes the order in the journal and at the venue",
        Duration::from_secs(10),
        || {
            matches!(closed(&oms, id), Some(CloseReason::Canceled { .. }))
                && w.fake
                    .with(|b, _| b.by_link(&link).map(|o| o.status))
                    == Some("Cancelled")
        },
    );
    assert_watcher_quiet_after(&w, &oms, &watcher, w.log_len());
    assert_eq!(
        w.fake.with(|b, _| (b.creates_for(&link), b.open_orders().len())),
        (1, 0),
        "creates for the link, open orders at the venue"
    );
    w.assert_venue_invariants();
}

/// J5b: the node stops itself (the failpoint `after_sync`, an abort as a
/// SIGKILL would be) after the flush of the accepted order and before the
/// reply and the send. Nothing reaches the venue. The restarted node marks
/// the order `order_in_doubt{Restart}`; it sends no create, and only
/// after `expires_at + absence_margin_us` does it count not-found answers
/// to its lookups by `orderLinkId` (realtime, then history): after two,
/// at least the margin apart, `order_closed{NotPlaced}`. Trading goes on
/// (a plain order is accepted). The watcher raises nothing.
#[test]
fn j5b_a_node_stopped_after_the_flush_before_the_send_closes_the_order_as_not_placed() {
    let mut w = World::new("j5b", Timers::Short, Duration::from_secs(60));
    w.set_watcher("silence_ms", 20_000);
    let (mut oms, watcher) = start_both_with(&w, &[("OMS_CRASH_AT", "after_sync")]);
    let first_pid = oms.pid().unwrap();

    let expires_at = oms_api::client::now_us() + 5_000_000;
    let seen = oms.try_new_order("j5b", Side::Buy, "0.001", "29000", false, expires_at);
    assert!(
        seen.is_err(),
        "a reply left before the read model learned the record: {seen:?}"
    );
    w.wait(
        "the node stops at the failpoint",
        Duration::from_secs(10),
        || oms.exit().is_some(),
    );
    let (status, _) = oms.exit().unwrap();
    assert!(!status.success(), "{status:?}");
    let log = oms.log();
    assert!(
        log.contains("failpoint: aborting") && log.contains("after_sync"),
        "not stopped at after_sync"
    );
    let id = the_accepted_order(&w, &oms);
    let link = oms.link(id);
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(w.fake.with(|b, _| b.creates_for(&link)), 0, "sent before the stop");

    // The operator restarts the OMS.
    let from = w.log_len();
    oms.start(&[]);
    assert_ne!(oms.pid(), Some(first_pid));
    let doubt = events_of(&oms, "order_in_doubt", id);
    assert!(
        matches!(
            doubt[..],
            [(
                _,
                Event::OrderInDoubt {
                    reason: DoubtReason::Restart,
                    ..
                }
            )]
        ),
        "{doubt:?}"
    );
    w.wait(
        "the restarted OMS closes the order as not placed",
        Duration::from_secs(20),
        || closed(&oms, id).is_some(),
    );
    let Some(CloseReason::NotPlaced { evidence }) = closed(&oms, id) else {
        w.fail(&format!("closed as {:?}", closed(&oms, id)))
    };
    let close_at = events_of(&oms, "order_closed", id)[0].0;
    let noted: Vec<(Ts, Ts)> = events_of(&oms, "absence_noted", id)
        .into_iter()
        .filter_map(|(at, e)| match e {
            Event::AbsenceNoted { queried_at, .. } => Some((at, queried_at)),
            _ => None,
        })
        .collect();
    let threshold = expires_at + ABSENCE_MARGIN_US;
    assert!(
        noted.len() >= 2
            && noted.iter().all(|(at, q)| *at <= close_at && *q >= threshold)
            && noted
                .windows(2)
                .all(|p| p[1].1 >= p[0].1 + ABSENCE_MARGIN_US),
        "not two not-found answers after expires_at + margin ({threshold}), a margin apart, before the close at {close_at}: {noted:?}; {evidence}"
    );
    println!(
        "j5b: not placed {} ms after expires_at; not-found answers queried at +{:?} ms",
        close_at.saturating_sub(expires_at) / 1_000,
        noted
            .iter()
            .map(|(_, q)| q.saturating_sub(expires_at) / 1_000)
            .collect::<Vec<_>>()
    );
    let realtime = lookups_by_link(&w, from, "/v5/order/realtime", &link);
    let history = lookups_by_link(&w, from, "/v5/order/history", &link);
    assert!(
        realtime.len() >= 2 && history.len() >= 4,
        "lookups by orderLinkId: realtime {realtime:?}, history {history:?}"
    );
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(
        w.fake.with(|b, _| b.creates_for(&link)),
        0,
        "the order was sent after the restart"
    );

    w.wait("the gate opens again", Duration::from_secs(20), || {
        monitored(&oms) == Some(true)
    });
    let after = working(&w, &oms, "after", "28900");
    assert_watcher_quiet_after(&w, &oms, &watcher, w.log_len());
    assert_eq!(
        w.fake.with(|b, _| (b.creates_for(&link), b.creates_for(&oms.link(after)))),
        (0, 1),
        "creates for the stopped order and for the order after"
    );
    w.assert_venue_invariants();
}

/// The fake `systemctl` logs each call with the wall clock in ms, also
/// in the first 100 ms of a second (where a `date +%s%3N` without its
/// leading zeros goes wrong); a call that is not the rig's stop exits 1
/// and signals nothing.
#[test]
fn the_fake_systemctl_logs_the_wall_clock_in_milliseconds() {
    let w = World::new("js", Timers::Short, Duration::from_secs(20));
    // Bounded: at most one second to reach the first 50 ms of a second.
    w.wait(
        "the first 50 ms of a second",
        Duration::from_secs(2),
        || now_ms() % 1_000 < 50,
    );
    let before = now_ms();
    let out = std::process::Command::new(w.systemctl())
        .args(["status", "x"])
        .output()
        .unwrap();
    let after = now_ms();
    assert_eq!(out.status.code(), Some(1), "a call that is not the stop");
    let log = w.systemctl_log();
    assert_eq!(log.len(), 1, "{log:?}");
    assert!(log[0].starts_with("status x marker=absent "), "{log:?}");
    let at: u64 = log[0].rsplit(' ').next().unwrap().parse().unwrap();
    assert!(
        before <= at && at <= after,
        "logged {at} ms, called between {before} and {after}"
    );
}

/// The rig's own rules: a base directory inside a git work tree, a
/// relative one and a socket path of 100 bytes or more are refused; a
/// start of the OMS with an `OMS_TEST_` variable, and a watcher timer
/// override of a line the timers do not write, are test errors, before
/// any process starts.
#[test]
fn the_rig_refuses_a_base_in_a_git_work_tree_a_long_socket_path_and_an_oms_test_variable() {
    let mut w = World::new("jr", Timers::Short, Duration::from_secs(20));
    let tree = w.dir.join("repo");
    std::fs::create_dir_all(tree.join(".git")).unwrap();
    let inside = tree.join("missing/base");
    let e = check_base(&inside, &inside.join("j1-1")).unwrap_err();
    assert!(e.contains("inside the git work tree"), "{e}");
    assert!(
        check_base(
            std::path::Path::new("rel"),
            std::path::Path::new("rel/j1-1")
        )
        .is_err()
    );
    // Made-up absolute paths, so the lengths do not depend on where the
    // suite runs (check_base only measures them and looks for `.git`).
    let base = std::path::Path::new("/joint-rig-base");
    // The longest socket path is `<dir>/o/d/monitor.sock`, 17 bytes more.
    let fits = base.join("x".repeat(SOCKET_PATH_MAX - 1 - 17 - base.as_os_str().len() - 1));
    assert_eq!(check_base(base, &fits), Ok(()));
    let long = base.join("x".repeat(SOCKET_PATH_MAX - 17 - base.as_os_str().len() - 1));
    let e = check_base(base, &long).unwrap_err();
    assert!(e.contains("has 100 bytes, at most 99 allowed"), "{e}");

    let mut oms = w.oms();
    let refused = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        oms.start(&[("OMS_TEST_BYBIT_WIRING", "{}")]);
    }));
    assert!(refused.is_err(), "Oms::start took an OMS_TEST_ variable");
    assert!(oms.pid().is_none(), "an OMS was started");

    w.set_watcher("no_such_ms", 1);
    let e = panic_of(|| drop(w.watcher()));
    assert!(
        e.contains("the watcher's timers write no line no_such_ms"),
        "{e}"
    );
    assert!(
        !w.dir.join("w/watcher.toml").exists(),
        "a watcher configuration was written"
    );
}

/// The panic message of `f`, which must panic.
fn panic_of(f: impl FnOnce()) -> String {
    let e = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).expect_err("no panic");
    e.downcast_ref::<String>().cloned().unwrap_or_default()
}

/// The `[http]` port rule of the rig: a port taken at the OMS's first
/// start is moved once (the dashboard's bind fails while the node keeps
/// running, so the rig reads the bind error in the log, not an exit); a
/// port taken again after the move, or taken when a journal or a watcher
/// configuration already exists, is a precondition failure, and no OMS
/// is left running.
#[test]
fn the_rig_moves_a_taken_http_port_once_and_a_second_taken_port_is_a_precondition() {
    let w = World::new("jp", Timers::Short, Duration::from_secs(90));
    let held = w.port();
    let _taken = std::net::TcpListener::bind(("127.0.0.1", held)).unwrap();
    let mut oms = w.oms();
    oms.start(&[]);
    assert!(oms.exit().is_none(), "the OMS ended: {}", oms.log());
    assert_ne!(oms.port(), held, "the port was not moved");
    assert_eq!(w.port(), oms.port(), "the world's port is not the OMS's");
    assert!(
        oms.status_http().is_some(),
        "no /api/status on the new port"
    );
    w.wait("the boot record", Duration::from_secs(10), || {
        !oms.events("booted").is_empty()
    });
    let boots = oms.events("booted").len();
    assert_eq!(boots, 1, "the journal of the first attempt was kept");
    drop(oms);

    // A later start: the journal exists, so the port is not moved.
    let _taken_again = std::net::TcpListener::bind(("127.0.0.1", w.port())).unwrap();
    let mut again = w.oms();
    let e = panic_of(|| again.start(&[]));
    assert!(
        e.starts_with(PRECONDITION) && e.contains("already holds it"),
        "{e}"
    );
    assert!(again.pid().is_none(), "an OMS was left running");

    // The first start, but the port of the retry is taken too.
    let w2 = World::new("jq", Timers::Short, Duration::from_secs(30));
    let _first = std::net::TcpListener::bind(("127.0.0.1", w2.port())).unwrap();
    let second = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let mut oms = w2.oms();
    oms.retry_on_port(second.local_addr().unwrap().port());
    let e = panic_of(|| oms.start(&[]));
    assert!(
        e.starts_with(PRECONDITION) && e.contains("still in use after a retry"),
        "{e}"
    );
    assert!(oms.pid().is_none(), "an OMS was left running");

    // The first start after the watcher's configuration was written: its
    // status_url names the port, so it is not moved.
    let w3 = World::new("js", Timers::Short, Duration::from_secs(30));
    let _watcher = w3.watcher();
    let _held = std::net::TcpListener::bind(("127.0.0.1", w3.port())).unwrap();
    let mut oms = w3.oms();
    let e = panic_of(|| oms.start(&[]));
    assert!(
        e.starts_with(PRECONDITION) && e.contains("already holds it"),
        "{e}"
    );
    assert!(oms.pid().is_none(), "an OMS was left running");

    // The holder of the port answers `/api/status` with JSON, as another
    // OMS does: the rig still sees the taken port and moves it.
    let w4 = World::new("jh", Timers::Short, Duration::from_secs(30));
    let holder = std::net::TcpListener::bind(("127.0.0.1", w4.port())).unwrap();
    let held = w4.port();
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let answers = std::thread::spawn({
        let stop = stop.clone();
        move || answer_status_until(&holder, &stop)
    });
    let mut oms = w4.oms();
    oms.start(&[]);
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    answers.join().unwrap();
    assert_ne!(
        oms.port(),
        held,
        "a port held by an answering server was not moved"
    );
    let pid = u64::from(oms.pid().unwrap());
    assert_eq!(oms.status_http().unwrap()["node"]["pid"], pid);
}

/// Answers every connection on `l` with `200` and `{}` until `stop` is
/// set (at most 60 s).
fn answer_status_until(l: &std::net::TcpListener, stop: &std::sync::atomic::AtomicBool) {
    use std::io::{Read, Write};
    l.set_nonblocking(true).unwrap();
    let end = Instant::now() + Duration::from_secs(60);
    while !stop.load(std::sync::atomic::Ordering::Relaxed) && Instant::now() < end {
        match l.accept() {
            Ok((mut s, _)) => {
                s.set_nonblocking(false).unwrap();
                s.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
                let _ = s.read(&mut [0; 1024]);
                let _ = s.write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}",
                );
            }
            Err(_) => std::thread::sleep(Duration::from_millis(5)),
        }
    }
}

/// `Oms::start`'s wait: a holder of the port that answers `/api/status`
/// while the admin socket already answers does not end it; the bind
/// error in the log does, in the next round. An OMS that answers on both
/// ends it at once, as does an exit.
#[test]
fn the_rig_waits_for_the_bind_error_when_a_holder_answers_api_status() {
    let taken = "bind 127.0.0.1:1: Address already in use";
    let rounds = std::cell::Cell::new(0);
    let end = Instant::now() + Duration::from_secs(5);
    let ours = || Some(json!({"node": {"pid": 7}}));
    let log = || {
        rounds.set(rounds.get() + 1);
        if rounds.get() > 1 {
            taken.to_string()
        } else {
            String::new()
        }
    };
    let holder = || Some(json!({}));
    assert!(port_taken(7, taken, log, || false, ours, holder, end));
    assert_eq!(rounds.get(), 2, "the wait ended in the first round");

    rounds.set(0);
    assert!(!port_taken(7, taken, log, || false, ours, ours, end));
    assert_eq!(rounds.get(), 1);
    rounds.set(0);
    assert!(!port_taken(7, taken, log, || true, || None, || None, end));
    assert_eq!(rounds.get(), 1);
}

/// `Oms::start`'s rule for an OMS that is up: both statuses name the
/// started process; another server's `/api/status` does not count.
#[test]
fn the_rig_takes_an_oms_as_up_only_when_the_admin_socket_and_api_status_name_its_pid() {
    let ours = json!({"node": {"pid": 7}});
    let other = json!({"node": {"pid": 8}});
    assert!(up(7, Some(ours.clone()), || Some(ours.clone())));
    assert!(!up(7, Some(ours.clone()), || None), "no /api/status");
    assert!(
        !up(7, Some(ours.clone()), || Some(json!({}))),
        "/api/status of no node"
    );
    assert!(
        !up(7, Some(ours.clone()), || Some(other.clone())),
        "/api/status of another node"
    );
    assert!(
        !up(7, Some(other.clone()), || Some(ours.clone())),
        "admin of another node"
    );
    assert!(
        !up(7, None, || panic!(
            "/api/status asked before the admin socket answered"
        )),
        "no admin socket"
    );
}
