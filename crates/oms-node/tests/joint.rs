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

use bybit_fake::book::{SYMBOL, WKEY};
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
/// kill from it in the journal, a `[WATCH P1]`, a `confirmed difference`
/// line, a request on `admin.sock` (its log names `watcher_mismatch` or
/// `watcher_kill` when it sends one), the kill marker.
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
    // The watcher's deployed heartbeat_ms (5,000): no two heartbeat
    // records closer than 4 s (the margin 판단(모델 아님)).
    for pair in beats.windows(2) {
        assert!(
            pair[1].0 - pair[0].0 >= 4_000_000,
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

/// The rig's own rules: a base directory inside a git work tree, a
/// relative one and a socket path of 100 bytes or more are refused; a
/// start of the OMS with an `OMS_TEST_` variable is a test error, before
/// any process starts.
#[test]
fn the_rig_refuses_a_base_in_a_git_work_tree_a_long_socket_path_and_an_oms_test_variable() {
    let w = World::new("jr", Timers::Short, Duration::from_secs(20));
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
    let base = w.dir.join("ok");
    // The longest socket path is `<dir>/o/d/monitor.sock`, 17 bytes more.
    let fits = base.join("x".repeat(SOCKET_PATH_MAX - 1 - 17 - base.as_os_str().len() - 1));
    assert_eq!(check_base(&base, &fits), Ok(()));
    let long = base.join("x".repeat(SOCKET_PATH_MAX - 17 - base.as_os_str().len() - 1));
    let e = check_base(&base, &long).unwrap_err();
    assert!(e.contains("has 100 bytes, at most 99 allowed"), "{e}");

    let mut oms = w.oms();
    let refused = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        oms.start(&[("OMS_TEST_BYBIT_WIRING", "{}")]);
    }));
    assert!(refused.is_err(), "Oms::start took an OMS_TEST_ variable");
    assert!(oms.pid().is_none(), "an OMS was started");
}
