//! The holds of C-HOLD through a real node (U18b1): the kill marker read at
//! boot, `clear-hold` and the marker's removal, and the watcher's inputs on
//! the admin socket. Real `oms-node` and `simvenue` processes; the
//! simulated venue has no cancel-all and answers the kill `unsupported`.

#![allow(clippy::unwrap_used)]

mod common;
use common::*;
use oms_api::client::ClientError;
use oms_api::*;
use oms_core::*;
use serde_json::{Value, json};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// A node whose `[kill] marker` is `<test dir>/watch/kill.marker`, with a
/// margin policy when `margin` is set. Neither started. Returns the marker
/// path; its directory exists.
fn marker_env(name: &str, margin: bool) -> (Env, PathBuf) {
    let mut env = Env::new(name);
    let watch = env.dir.join("watch");
    std::fs::create_dir_all(&watch).unwrap();
    let marker = watch.join("kill.marker");
    env.extra_tables = format!("[kill]\nmarker = \"{}\"\n", marker.display());
    if margin {
        env.extra_runtime = "margin_poll_ms = 100".into();
        env.extra_policy =
            "\n[core.policy.margin]\naccount_modes_allowed = [\"standard\"]\n".into();
        env.ctl(json!({"account_value": {"value": "100000"}}));
    }
    (env, marker)
}

fn set_mode(p: &Path, mode: u32) {
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(mode)).unwrap();
}

/// Waits until the core holds exactly `holds`, the link is up and nothing
/// is needed; returns the status.
fn wait_holds(env: &Env, holds: Value) -> Value {
    let ok = env.wait(Duration::from_secs(20), || {
        env.try_status().is_some_and(|s| {
            s["core"]["holds"] == holds
                && s["core"]["link"]["up"] == true
                && s["core"]["needs"].as_array().is_some_and(|n| n.is_empty())
        })
    });
    assert!(
        ok,
        "never held {holds}: {}\n{}",
        env.try_status()
            .map(|s| s["core"].to_string())
            .unwrap_or_default(),
        env.log_tail(20)
    );
    env.status()
}

/// Waits until the kill latch is confirmed in this process (the simulated
/// venue's `unsupported` answer); returns the status.
fn wait_confirmed(env: &Env) -> Value {
    let ok = env.wait(Duration::from_secs(20), || {
        env.try_status()
            .is_some_and(|s| s["core"]["kill"]["confirmed_at"].is_u64())
    });
    assert!(
        ok,
        "the kill was never confirmed: {}\n{}",
        env.try_status()
            .map(|s| s["core"].to_string())
            .unwrap_or_default(),
        env.log_tail(20)
    );
    env.status()
}

fn new_order(
    c: &mut oms_api::client::Client,
    key: &str,
    px: &str,
    reduce_only: bool,
) -> NewOrderReply {
    let Op::NewOrder {
        client_order_id,
        instrument,
        side,
        qty,
        px,
        tif,
        expires_at,
        ..
    } = order_op(key, Side::Buy, "1", px, Tif::Gtc, 60_000)
    else {
        unreachable!()
    };
    let side = if reduce_only { Side::Sell } else { side };
    c.new_order(Op::NewOrder {
        client_order_id,
        instrument,
        side,
        qty,
        px,
        tif,
        reduce_only,
        expires_at,
        market_ref: None,
    })
    .unwrap()
}

fn refused_for(r: NewOrderReply, hold: HoldReason) {
    match r {
        NewOrderReply::Rejected { reasons } => assert!(
            reasons.contains(&RejectReason::Hold(hold.clone())),
            "{hold:?}: {reasons:?}"
        ),
        other => panic!("{hold:?}: accepted: {other:?}"),
    }
}

/// The text of an operator reply, or of its refusal.
fn admin_call(env: &Env, op: Op) -> Result<String, (ErrorCode, String)> {
    match env.admin().call(op) {
        Ok(v) => Ok(v.as_str().unwrap_or_default().to_string()),
        Err(ClientError::Api(e)) => Err((e.code, e.message)),
        Err(e) => panic!("{e}"),
    }
}

fn omsctl(env: &Env, args: &[&str]) -> Result<String, String> {
    let mut all = vec![
        "--socket".to_string(),
        env.admin_sock().display().to_string(),
    ];
    all.extend(args.iter().map(|a| a.to_string()));
    oms_api::ctl::run(&all, &mut std::io::sink())
}

/// Done criterion (U18b): with the kill marker the OMS boots with
/// KillLatched and accepts no order, reduce-only included, until
/// `clear_hold` kill, which removes the marker; the next boot is
/// unlatched and an order is accepted.
#[test]
fn a_kill_marker_at_boot_refuses_every_order_until_clear_hold_kill_removes_it() {
    let (mut env, marker) = marker_env("kill-marker", true);
    std::fs::write(&marker, "W5 killed: the OMS was silent for 30 s\n").unwrap();
    env.start_node();
    let st = wait_confirmed(&env);
    let st = if st["core"]["holds"] == json!(["kill_latched"]) {
        st
    } else {
        wait_holds(&env, json!(["kill_latched"]))
    };
    assert_eq!(
        st["node"]["kill_marker"],
        json!({"configured": true, "path": marker.display().to_string(), "present_at_boot": true})
    );
    let k = &st["core"]["kill"];
    assert_eq!(
        (k["source"].as_str(), k["note"].as_str()),
        (
            Some("marker"),
            Some("marker: W5 killed: the OMS was silent for 30 s")
        )
    );
    assert_eq!(k["outcome"], "unsupported");
    assert_eq!(st["core"]["watcher_mismatch"], Value::Null);
    assert_eq!(st["core"]["venue_change"], Value::Null);

    // The latch is in the boot record itself, before any order; the
    // gateway's answer comes after it.
    let records = env.journal_records();
    let boot = records
        .iter()
        .find(|r| {
            matches!(
                &r.input,
                Input::Boot {
                    kill_marker: Some(_),
                    ..
                }
            )
        })
        .expect("the boot record");
    assert_eq!(
        boot.events.iter().map(Event::name).collect::<Vec<_>>(),
        ["booted", "hold_set"]
    );
    let ended = records
        .iter()
        .find(|r| r.events.iter().any(|e| e.name() == "kill_loop_ended"))
        .expect("kill_loop_ended");
    assert!(ended.seq > boot.seq);
    assert!(matches!(
        ended.input,
        Input::Venue(VenueInput::KillLoopEnded {
            outcome: KillOutcome::Unsupported,
            ..
        })
    ));

    // Every order is refused, reduce-only included.
    env.ctl(json!({"ref_price": {"instrument": "X", "px": "100"}}));
    let mut a = env.client("a");
    refused_for(
        new_order(&mut a, "plain", "90", false),
        HoldReason::KillLatched,
    );
    refused_for(
        new_order(&mut a, "reduce", "110", true),
        HoldReason::KillLatched,
    );

    // A refused clear leaves the marker where it is.
    let (code, why) = admin_call(
        &env,
        Op::ClearHold {
            reason: HoldKind::Kill,
            note: " ".into(),
        },
    )
    .unwrap_err();
    assert_eq!(code, ErrorCode::BadRequest);
    assert!(why.starts_with("note_required"), "{why}");
    assert!(marker.exists(), "a refused clear removed the marker");
    let (code, _) = admin_call(
        &env,
        Op::ClearHold {
            reason: HoldKind::WatcherMismatch,
            note: "n".into(),
        },
    )
    .unwrap_err();
    assert_eq!(code, ErrorCode::BadRequest);
    assert!(marker.exists());
    // Not on the trading socket.
    assert_eq!(
        api_error(a.call(Op::ClearHold {
            reason: HoldKind::Kill,
            note: "n".into()
        })),
        ErrorCode::Forbidden
    );

    // omsctl clear-hold: cleared, and the marker is gone.
    let out = omsctl(
        &env,
        &[
            "clear-hold",
            "--reason",
            "kill",
            "--note",
            "orders checked at the venue",
        ],
    )
    .unwrap();
    assert!(out.contains("cleared; kill marker removed"), "{out}");
    assert!(!marker.exists());
    assert_eq!(env.journal_count("hold_cleared"), 1);
    wait_holds(&env, json!([]));
    rest(&mut env, &mut a, "after-clear", Side::Buy, "90", false);

    // The next boot is not latched.
    assert_eq!(env.stop_node(), 0);
    env.start_node();
    env.wait_ready();
    let st = env.status();
    assert_eq!(st["node"]["kill_marker"]["present_at_boot"], false);
    assert_eq!(st["core"]["kill"], Value::Null);
    let mut a = env.client("a");
    rest(&mut env, &mut a, "after-restart", Side::Buy, "90", false);
    assert_eq!(env.journal_count("hold_set"), 1);
}

/// C-HOLD. A marker the node cannot remove after a durable clear: the
/// clear stands and the reply says so; the next start latches again.
#[test]
fn an_unremovable_marker_is_reported_and_latches_the_next_boot() {
    if rustix::process::getuid().is_root() {
        return;
    }
    let (mut env, marker) = marker_env("kill-marker-stays", false);
    std::fs::write(&marker, "killed").unwrap();
    env.start_node();
    wait_confirmed(&env);
    let watch = marker.parent().unwrap().to_path_buf();
    set_mode(&watch, 0o500);
    let reply = admin_call(
        &env,
        Op::ClearHold {
            reason: HoldKind::Kill,
            note: "checked".into(),
        },
    );
    set_mode(&watch, 0o700);
    assert_eq!(
        reply,
        Ok(
            "cleared; kill marker NOT removed (PermissionDenied): the next start latches again"
                .to_string()
        )
    );
    assert!(marker.exists());
    wait_holds(&env, json!([]));

    assert_eq!(env.stop_node(), 0);
    env.start_node();
    let st = wait_confirmed(&env);
    assert_eq!(st["core"]["kill"]["source"], "marker");
    assert!(
        st["core"]["holds"]
            .as_array()
            .unwrap()
            .contains(&json!("kill_latched"))
    );
    assert_eq!(st["node"]["kill_marker"]["present_at_boot"], true);
    assert_eq!(
        admin_call(
            &env,
            Op::ClearHold {
                reason: HoldKind::Kill,
                note: "checked".into()
            }
        ),
        Ok("cleared; kill marker removed".to_string())
    );
    assert!(!marker.exists());
}

/// C-HOLD. A marker path the node cannot examine latches (fail-safe): the
/// note names the error.
#[test]
fn a_marker_path_that_cannot_be_examined_latches() {
    if rustix::process::getuid().is_root() {
        return;
    }
    let (mut env, marker) = marker_env("kill-marker-shut", false);
    let watch = marker.parent().unwrap().to_path_buf();
    set_mode(&watch, 0o000);
    env.start_node();
    let ok = env.wait(Duration::from_secs(20), || {
        env.try_status()
            .is_some_and(|s| s["core"]["kill"].is_object())
    });
    set_mode(&watch, 0o700);
    assert!(ok, "not latched: {}", env.log_tail(20));
    let st = wait_confirmed(&env);
    assert_eq!(
        st["core"]["kill"]["note"],
        "marker unreadable: PermissionDenied"
    );
    assert_eq!(st["node"]["kill_marker"]["present_at_boot"], true);
    let mut a = env.client("a");
    refused_for(
        new_order(&mut a, "plain", "90", false),
        HoldReason::KillLatched,
    );
}

/// Done criterion (U18b): under WatcherMismatch a reduce-only order passes
/// and a plain one is refused. The watcher's two inputs on the admin
/// socket, their replies, and what is refused without being recorded.
#[test]
fn watcher_mismatch_over_the_admin_socket_refuses_plain_orders_only() {
    let mut env = margin_node("watcher-mismatch", "100000");
    env.ctl(json!({"ref_price": {"instrument": "X", "px": "100"}}));
    assert_eq!(
        env.status()["node"]["kill_marker"],
        json!({"configured": false, "path": null, "present_at_boot": false})
    );
    let mut a = env.client("a");

    let mismatch = |check, detail: &str| Op::WatcherMismatch {
        check,
        detail: detail.into(),
    };
    assert_eq!(
        admin_call(
            &env,
            mismatch(WatcherCheck::W2, "position X: OMS 1, venue 0")
        ),
        Ok("held".to_string())
    );
    assert_eq!(
        admin_call(&env, mismatch(WatcherCheck::W1, "again")),
        Ok("already held".to_string())
    );
    // An empty detail, a strategy connection, an unknown check: refused,
    // nothing recorded.
    let (code, _) = admin_call(&env, mismatch(WatcherCheck::W3, " ")).unwrap_err();
    assert_eq!(code, ErrorCode::BadRequest);
    assert_eq!(
        api_error(a.call(mismatch(WatcherCheck::W3, "d"))),
        ErrorCode::Forbidden
    );
    assert_eq!(
        api_error(a.call(Op::WatcherKill { detail: "d".into() })),
        ErrorCode::Forbidden
    );
    let mut raw = std::os::unix::net::UnixStream::connect(env.admin_sock()).unwrap();
    use std::io::{BufRead, Write};
    raw.write_all(b"{\"id\":1,\"op\":\"hello\",\"strategy\":null}\n{\"id\":2,\"op\":\"watcher_mismatch\",\"check\":\"W5\",\"detail\":\"d\"}\n").unwrap();
    let mut lines = std::io::BufReader::new(raw.try_clone().unwrap()).lines();
    lines.next().unwrap().unwrap();
    let answer: Value = serde_json::from_str(&lines.next().unwrap().unwrap()).unwrap();
    assert_eq!(answer["error"]["code"], "bad_request", "{answer}");
    assert_eq!(env.journal_count("hold_set"), 1);

    let st = wait_holds(&env, json!(["watcher_mismatch"]));
    let w = &st["core"]["watcher_mismatch"];
    assert_eq!(
        (w["source"].as_str(), w["note"].as_str()),
        (Some("W2"), Some("position X: OMS 1, venue 0"))
    );
    refused_for(
        new_order(&mut a, "plain", "90", false),
        HoldReason::WatcherMismatch,
    );
    rest(&mut env, &mut a, "reduce", Side::Sell, "120", true);

    assert_eq!(
        omsctl(
            &env,
            &[
                "clear-hold",
                "--reason",
                "watcher_mismatch",
                "--note",
                "checked"
            ]
        )
        .map(|t| t.contains("\"cleared\"")),
        Ok(true)
    );
    wait_holds(&env, json!([]));
    rest(&mut env, &mut a, "plain-after", Side::Buy, "90", false);

    // The watcher's kill: answered once durable; the simulated venue
    // answers unsupported, and the operator can clear it.
    assert_eq!(
        admin_call(
            &env,
            Op::WatcherKill {
                detail: "W5: no heartbeat".into()
            }
        ),
        Ok("latched".to_string())
    );
    assert_eq!(
        admin_call(
            &env,
            Op::WatcherKill {
                detail: "again".into()
            }
        ),
        Ok("already latched".to_string())
    );
    let st = wait_confirmed(&env);
    assert_eq!(st["core"]["kill"]["source"], "watcher");
    refused_for(
        new_order(&mut a, "reduce-killed", "120", true),
        HoldReason::KillLatched,
    );
    assert_eq!(
        admin_call(
            &env,
            Op::ClearHold {
                reason: HoldKind::Kill,
                note: "checked".into()
            }
        ),
        Ok("cleared".to_string())
    );
    wait_holds(&env, json!([]));
    assert_eq!(env.journal_count("hold_set"), 2);
    assert_eq!(env.journal_count("hold_cleared"), 2);
}
