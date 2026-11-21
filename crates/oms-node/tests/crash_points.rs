//! The node is stopped at one named boundary of the commit sequence
//! (docs/contracts/commit.md, "경계별 장애 결과") and restarted. Each test
//! checks the row of that table: what the journal holds, what the strategy
//! saw, what reached the venue, what the restarted node does, and what a
//! repeated request gets.
//!
//! The stop is `abort()` from inside the commit stage, so the operating
//! system keeps what was written: this is a process crash, not a power loss.
//! Needs `--features failpoints`; the shipped binary has no such code.

#![allow(clippy::unwrap_used)]

mod common;

use common::*;
use oms_api::client::ClientError;
use oms_core::*;
use serde_json::json;
use std::collections::BTreeSet;
use std::time::Duration;

/// Starts a node that aborts at `point` when it commits a new order, sends
/// one order, waits for the process to die, and restarts it without the
/// failpoint. Returns the environment, the request (for repeating it
/// unchanged) and what the strategy saw.
fn crash_then_restart(name: &str, point: &str) -> (Env, oms_api::Op, Result<NewOrderReply, ClientError>) {
    let mut env = Env::new(name);
    env.node_env = vec![("OMS_CRASH_AT".into(), point.into())];
    env.start_node();
    env.wait_ready();
    let op = order_op("k1", Side::Buy, "1", "100", Tif::Gtc, 1_500);
    let mut c = env.client("a");
    let seen = c.new_order(op.clone());
    let exit = env.wait_exit(Duration::from_secs(5));
    assert!(env.node.is_none(), "the node did not stop at {point}");
    assert_ne!(exit, Some(0));
    assert!(env.log_tail(5).contains(point), "{}", env.log_tail(5));
    // Nothing reached the venue at any of these boundaries.
    assert_eq!(env.venue_dump()["place_requests"], 0);

    env.node_env.clear();
    env.start_node();
    (env, op, seen)
}

fn settle_as_not_placed(env: &mut Env, id: OrderId) {
    // The restarted node knows the order, does not know whether it was sent,
    // and holds new orders until it is resolved.
    assert!(env.wait(Duration::from_secs(5), || env.try_status().is_some()));
    let mut c = env.client("a");
    assert!(env.wait(Duration::from_secs(5), || c.get_order(id).is_ok_and(|o| o.is_some())));
    // After the send window plus the margin, a query that finds nothing
    // closes it. It was never sent again.
    assert!(
        env.wait(Duration::from_secs(10), || c
            .get_order(id)
            .unwrap()
            .is_some_and(|o| matches!(o.close, Some(Close { reason: CloseReason::NotPlaced { .. }, .. })))),
        "order: {:?}",
        c.get_order(id)
    );
    env.wait_ready();
    let dump = env.venue_dump();
    assert_eq!(dump["place_requests"], 0, "the order was sent after the restart");
    assert_eq!(env.status()["core"]["stats"]["accepted"], 1);
}

/// ④~⑤: stopped before anything was written.
#[test]
fn stopped_before_the_write_the_request_never_happened() {
    let (mut env, op, seen) = crash_then_restart("crash-before-write", "before_write");
    assert!(seen.is_err(), "no reply may leave before the record is durable: {seen:?}");
    env.wait_ready();
    let st = env.status();
    assert_eq!(st["core"]["orders_in_memory"], 0);
    assert_eq!(st["core"]["stats"]["accepted"], 0);
    // The strategy repeats the same request: it is evaluated afresh, once.
    let mut c = env.client("a");
    let again = order_op("k1", Side::Buy, "1", "100", Tif::Gtc, 5_000);
    let _ = op;
    assert!(matches!(c.new_order(again).unwrap(), NewOrderReply::Accepted { .. }));
    assert!(env.wait(Duration::from_secs(5), || env.status()["core"]["working"] == 1));
    assert_eq!(env.venue_dump()["place_requests"], 1);
}

/// ⑤~⑥: the bytes were written, the flush was not confirmed. The process
/// died, the operating system did not, so the record is there.
#[test]
fn stopped_after_the_write_the_order_exists_and_is_resolved_without_a_send() {
    let (mut env, op, seen) = crash_then_restart("crash-after-write", "after_write");
    assert!(seen.is_err(), "no reply may leave before the flush is confirmed: {seen:?}");
    assert!(env.wait(Duration::from_secs(5), || env.try_status().is_some()));
    let mut c = env.client("a");
    // The same request again is the same order, not a second one.
    let NewOrderReply::Duplicate { order_id, .. } = c.new_order(op).unwrap() else {
        panic!("the repeated request was not recognised as a duplicate")
    };
    let o = c.get_order(order_id).unwrap().unwrap();
    assert!(o.close.is_some() || o.doubt.as_ref().is_some_and(|d| d.reason == DoubtReason::Restart), "{o:?}");
    settle_as_not_placed(&mut env, order_id);
}

/// ⑥~⑧: durable, nothing released.
#[test]
fn stopped_after_the_flush_the_order_is_in_doubt_and_never_sent() {
    let (mut env, op, seen) = crash_then_restart("crash-after-sync", "after_sync");
    assert!(seen.is_err(), "the reply is released after the read model, which did not happen: {seen:?}");
    assert!(env.wait(Duration::from_secs(5), || env.try_status().is_some()));
    let mut c = env.client("a");
    let NewOrderReply::Duplicate { order_id, .. } = c.new_order(op).unwrap() else {
        panic!("the repeated request was not recognised as a duplicate")
    };
    settle_as_not_placed(&mut env, order_id);
}

/// ⑧~⑨: the strategy has its answer, the gateway never got the command.
#[test]
fn stopped_after_the_reply_the_accepted_order_is_never_sent_and_closes_as_not_placed() {
    let (mut env, op, seen) = crash_then_restart("crash-after-reply", "after_reply");
    let NewOrderReply::Accepted { order_id } = seen.expect("the reply left before the stop") else {
        panic!("not accepted")
    };
    assert!(env.wait(Duration::from_secs(5), || env.try_status().is_some()));
    let mut c = env.client("a");
    assert!(matches!(c.new_order(op).unwrap(), NewOrderReply::Duplicate { order_id: again, .. } if again == order_id));
    settle_as_not_placed(&mut env, order_id);
}

/// The flush reports an error and the process lives: nothing of the batch is
/// released, the node stops with status 2, and the restart finds the record
/// that did reach the file.
#[test]
fn a_flush_error_after_the_write_stops_the_node_and_the_record_may_survive() {
    let mut env = Env::new("crash-sync-error");
    env.node_env = vec![("OMS_CRASH_AT".into(), "sync_error_after_write".into())];
    env.start_node();
    env.wait_ready();
    let op = order_op("k1", Side::Buy, "1", "100", Tif::Gtc, 1_500);
    let mut c = env.client("a");
    let seen = c.new_order(op.clone());
    assert!(!matches!(seen, Ok(NewOrderReply::Accepted { .. })), "accepted without a confirmed flush: {seen:?}");
    assert_eq!(env.wait_exit(Duration::from_secs(5)), Some(2), "{}", env.log_tail(5));
    assert_eq!(env.venue_dump()["place_requests"], 0);

    env.node_env.clear();
    env.start_node();
    assert!(env.wait(Duration::from_secs(5), || env.try_status().is_some()));
    let mut c = env.client("a");
    let NewOrderReply::Duplicate { order_id, .. } = c.new_order(op).unwrap() else {
        panic!("the record written before the failed flush was not found")
    };
    settle_as_not_placed(&mut env, order_id);
}

// ---- C-MARGIN: the latch and the day base at the commit boundaries ---------------

/// The node aborts at `point` when it commits a margin report that produced
/// the event `on` (`reducing_entered` or `day_base_set`).
fn crash_on(point: &str, on: &str) -> Vec<(String, String)> {
    vec![("OMS_CRASH_AT".into(), point.into()), ("OMS_CRASH_ON".into(), on.into())]
}

/// Waits for the node to abort at `point`.
fn wait_abort(env: &mut Env, point: &str) {
    let exit = env.wait_exit(Duration::from_secs(5));
    assert!(env.node.is_none(), "the node did not stop at {point}: {}", env.log_tail(5));
    assert_ne!(exit, Some(0));
    assert!(env.log_tail(5).contains(point), "{}", env.log_tail(5));
}

/// A node with the day base 100 and two plain orders resting at the venue.
/// Returns the venue client ids of the two orders.
fn two_resting_plain_orders(env: &mut Env) -> BTreeSet<String> {
    env.ctl(json!({"ref_price": {"instrument": "X", "px": "100"}}));
    let mut c = env.client("a");
    let bid = rest(env, &mut c, "bid", Side::Buy, "90", false);
    let ask = rest(env, &mut c, "ask", Side::Sell, "110", false);
    assert!(env.wait(Duration::from_secs(5), || env.status()["core"]["working"] == 2));
    BTreeSet::from([env.cloid(bid), env.cloid(ask)])
}

/// The restarted node is in a new link session and has taken margin reports
/// in it.
fn wait_restarted(env: &Env, boot: u64, holds: serde_json::Value) {
    let ok = env.wait(Duration::from_secs(20), || {
        env.try_status().is_some_and(|st| {
            let core = &st["core"];
            core["boot"] == boot
                && core["holds"] == holds
                && core["margin"]["session"] == core["link"]["session"]
                && core["margin"]["seq"].as_u64().is_some_and(|n| n >= 2)
        })
    });
    assert!(ok, "after the restart: {:?}\n{}", env.try_status().map(|st| st["core"].to_string()), env.log_tail(20));
}

/// G4, "중단과 재실행" row 1. The node decides to enter REDUCING and stops
/// before the record is written. After the restart there is no latch, no
/// cancel request and no record of either; the day base is the old one; the
/// two orders still rest at the venue. The next report that breaches the
/// limit decides again: REDUCING is entered once and both orders are
/// cancelled.
#[test]
fn stopped_before_the_write_of_a_reducing_decision_the_next_report_decides_again() {
    let mut env = margin_node_with("crash-reducing-before-write", "100", crash_on("before_write", "reducing_entered"));
    let base = env.status()["core"]["margin"]["day_base"].clone();
    assert_eq!(base["value"], "100");
    let resting = two_resting_plain_orders(&mut env);

    env.ctl(json!({"account_value": {"value": "94"}}));
    wait_abort(&mut env, "before_write");
    assert_eq!(open_at_venue(&mut env), resting, "a cancel reached the venue before the record was written");
    assert_eq!(env.journal_count("reducing_entered"), 0);
    assert_eq!(env.journal_count("cancel_requested"), 0);

    // The venue states 100 again, so the restarted node has no reason to
    // latch: what it shows is what the journal holds.
    env.ctl(json!({"account_value": {"value": "100"}}));
    env.node_env.clear();
    env.start_node();
    wait_restarted(&env, 2, json!([]));
    let st = env.status();
    assert_eq!(st["core"]["margin"]["reducing"], serde_json::Value::Null);
    assert_eq!(st["core"]["margin"]["day_base"], base);
    assert_eq!(st["core"]["open_orders"], 2);
    assert_eq!(open_at_venue(&mut env), resting);
    assert_eq!(env.journal_count("reducing_entered"), 0);
    assert_eq!(env.journal_count("cancel_requested"), 0);

    // The same loss again: this time the decision is recorded and acted on.
    lose_to(&mut env, "94");
    assert!(
        env.wait(Duration::from_secs(10), || env.status()["core"]["open_orders"] == 0),
        "the orders were not cancelled: {}",
        env.status()["core"]
    );
    assert_eq!(open_at_venue(&mut env), BTreeSet::new());
    assert_eq!(env.journal_count("reducing_entered"), 1);
    assert_eq!(env.journal_count("cancel_requested"), 2);
    assert_eq!(env.status()["core"]["margin"]["day_base"], base);
}

/// G4, "중단과 재실행" row 2. The record that enters REDUCING and asks for
/// the cancels reached the file, and the node stops before a cancel left
/// for the venue: after the write, after the flush, and after the read
/// model learned the record. After the restart the latch and the cancel
/// requests are restored from the journal (the venue states 100 again, so
/// nothing is decided anew), the cancels that were never sent are sent, and
/// the reduce-only order is left open. Nothing is requested twice.
#[test]
fn stopped_after_the_reducing_decision_is_durable_the_cancels_are_sent_after_restart() {
    for point in ["after_write", "after_sync", "after_reply"] {
        let mut env = margin_node_with(&format!("crash-reducing-{point}"), "100", crash_on(point, "reducing_entered"));
        let base = env.status()["core"]["margin"]["day_base"].clone();
        let plain = two_resting_plain_orders(&mut env);
        let mut c = env.client("a");
        let flagged = rest(&mut env, &mut c, "flagged", Side::Sell, "120", true);
        let flagged = BTreeSet::from([env.cloid(flagged)]);
        assert!(env.wait(Duration::from_secs(5), || env.status()["core"]["working"] == 3));
        drop(c);
        let all: BTreeSet<String> = plain.union(&flagged).cloned().collect();

        env.ctl(json!({"account_value": {"value": "94"}}));
        wait_abort(&mut env, point);
        assert_eq!(open_at_venue(&mut env), all, "{point}: a cancel reached the venue before the stop");
        assert_eq!(env.journal_count("reducing_entered"), 1, "{point}");
        assert_eq!(env.journal_count("cancel_requested"), 2, "{point}");

        env.ctl(json!({"account_value": {"value": "100"}}));
        env.node_env.clear();
        env.start_node();
        wait_restarted(&env, 2, json!(["reducing"]));
        let st = env.status();
        assert_eq!(st["core"]["margin"]["reducing"]["rule"], "daily_loss", "{point}");
        assert_eq!(
            st["core"]["margin"]["reducing"]["detail"],
            "account value 94 is more than 500 bps below the day base 100",
            "{point}"
        );
        assert_eq!(st["core"]["margin"]["day_base"], base, "{point}");

        // The cancels recorded before the stop reach the venue now.
        let mut left = open_at_venue(&mut env);
        let deadline = std::time::Instant::now() + Duration::from_secs(15);
        while left != flagged && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
            left = open_at_venue(&mut env);
        }
        assert_eq!(left, flagged, "{point}: a recorded rule cancel was never sent, or the reduce-only order was cancelled");
        assert!(
            env.wait(Duration::from_secs(10), || env.status()["core"]["open_orders"] == 1),
            "{point}: {}",
            env.status()["core"]
        );
        assert_eq!(env.journal_count("reducing_entered"), 1, "{point}");
        assert_eq!(env.journal_count("cancel_requested"), 2, "{point}");
        assert_eq!(env.status()["core"]["mismatches_unacked"], json!([]), "{point}");
    }
}

/// G4, "중단과 재실행" row 3. The first margin report of a new node sets the
/// day base, and the node stops at each commit boundary of that record.
/// Stopped before the write, the base does not exist after the restart and
/// the next report sets it, to its own value (the venue states 105 by
/// then). Stopped at any later boundary, the base of 100 is in the journal
/// and the restarted node keeps it. In every case the journal holds exactly
/// one `day_base_set`, also after further reports, and its day and value
/// are those of the report in the same record.
#[test]
fn stopped_around_a_day_base_set_the_base_is_set_exactly_once() {
    for (point, written, expected) in [
        ("before_write", 0, "105"),
        ("after_write", 1, "100"),
        ("after_sync", 1, "100"),
        ("after_reply", 1, "100"),
    ] {
        let mut env = Env::new(&format!("crash-day-base-{point}"));
        env.extra_runtime = "margin_poll_ms = 100".into();
        env.extra_policy = "\n[core.policy.margin]\naccount_modes_allowed = [\"standard\"]\n".into();
        env.node_env = crash_on(point, "day_base_set");
        env.ctl(json!({"account_value": {"value": "100"}}));
        env.start_node();
        wait_abort(&mut env, point);
        assert_eq!(env.journal_count("day_base_set"), written, "{point}");

        env.ctl(json!({"account_value": {"value": "105"}}));
        env.node_env.clear();
        env.start_node();
        wait_restarted(&env, 2, json!([]));
        let shown = env.status()["core"]["margin"]["day_base"].clone();
        assert_eq!(shown["value"], expected, "{point}: {shown}");
        assert_eq!(shown["set_by"], "report", "{point}");

        // Further reports do not set it again.
        let seq = env.status()["core"]["margin"]["seq"].as_u64().expect("seq");
        wait_margin(&env, "three more reports", |m| m["seq"].as_u64().is_some_and(|n| n >= seq + 3));
        assert_eq!(env.status()["core"]["margin"]["day_base"], shown, "{point}");
        let sets: Vec<(Input, Event)> = env
            .journal_records()
            .into_iter()
            .flat_map(|r| {
                let input = r.input.clone();
                r.events.into_iter().filter(|e| e.name() == "day_base_set").map(move |e| (input.clone(), e))
            })
            .collect();
        assert_eq!(sets.len(), 1, "{point}: the day base was set {} times", sets.len());
        // The one event carries the day and the value of its own report.
        let (Input::Venue(VenueInput::MarginReport(report)), Event::DayBaseSet { day, value, set_by }) = &sets[0] else {
            panic!("{point}: {:?}", sets[0])
        };
        assert_eq!(*value, report.account_value, "{point}");
        assert_eq!(value.to_string(), expected, "{point}");
        assert_eq!(*day, report.queried_at / 86_400_000_000, "{point}");
        assert_eq!(*set_by, BaseSetBy::Report, "{point}");
        assert_eq!(shown["day"], *day, "{point}");
    }
}

// ---- C-ALERT: the fatal message and the stop notice ---------------------------

/// Writes a token file into the test directory, the `[alert]` table that
/// names it, and points the node's alerts at the loopback fake
/// (`OMS_TEST_TELEGRAM_API`, failpoints builds only).
fn with_alerts(env: &mut Env, f: &http_fake::FakeHttp) {
    let token = env.dir.join("telegram.json");
    std::fs::write(&token, r#"{"token":"123456:ABCdef_ghi-JKL","chat_id":"42"}"#).unwrap();
    std::fs::set_permissions(&token, std::os::unix::fs::PermissionsExt::from_mode(0o600)).unwrap();
    env.extra_tables = format!("[alert]\ntelegram = \"{}\"\n", token.display());
    env.node_env.push(("OMS_TEST_TELEGRAM_API".into(), f.url()));
}

fn telegram_texts(f: &http_fake::FakeHttp) -> Vec<String> {
    f.requests()
        .iter()
        .map(|r| serde_json::from_slice::<serde_json::Value>(&r.body).unwrap()["text"].as_str().unwrap_or_default().to_string())
        .collect()
}

/// C-ALERT. A clean stop (SIGTERM, exit 0) sends the stop notice with the
/// number of orders left at the venue, after the start notice and before
/// the exit.
#[test]
fn a_clean_stop_sends_the_stop_notice_with_the_open_orders() {
    let f = http_fake::FakeHttp::start(http_fake::Answer::Reply(200, r#"{"ok":true,"result":{}}"#.into()));
    let mut env = Env::new("alert-stop");
    with_alerts(&mut env, &f);
    env.start_node();
    env.wait_ready();
    let mut c = env.client("a");
    rest(&mut env, &mut c, "left", Side::Buy, "90", false);
    assert!(env.wait(Duration::from_secs(5), || env.status()["core"]["working"] == 1));
    drop(c);
    assert!(env.wait(Duration::from_secs(5), || !telegram_texts(&f).is_empty()));
    assert_eq!(env.stop_node(), 0, "{}", env.log_tail(10));
    let all = telegram_texts(&f);
    assert_eq!(all.len(), 2, "{all:?}");
    assert!(all[0].contains("OMS 시작 (boot 1, config 1). 활성 조건 "), "{}", all[0]);
    assert!(all[1].starts_with("[OMS P1] sim ") && all[1].ends_with("\nOMS 정상 종료. 걸린 주문 1개는 거래소에 남음"), "{}", all[1]);
    assert!(env.log_tail(10).contains("alerts delivered before the exit"), "{}", env.log_tail(10));
}

/// C-ALERT, done criterion. The flush of the record that enters REDUCING
/// fails (two plain orders rest, the margin report breaches the daily
/// loss) and the node stops with status 2. A loopback fake stands in for
/// Telegram (`OMS_TEST_TELEGRAM_API`): it gets the start notice and the
/// fatal message, and nothing about REDUCING. Its first answer to the fatal
/// message is 500, so the message arrives only if the exit waits for the
/// retry (the exit flush); the node still exits within exit_flush_ms + 5 s.
#[test]
fn a_flush_failure_sends_one_fatal_alert_and_nothing_of_the_batch() {
    let ok = || http_fake::Answer::Reply(200, r#"{"ok":true,"result":{}}"#.into());
    let f = http_fake::FakeHttp::start(ok());
    f.script([ok(), http_fake::Answer::Reply(500, r#"{"ok":false,"description":"down"}"#.into())]);
    let mut env = Env::new("alert-fatal");
    with_alerts(&mut env, &f);
    env.node_env.extend(crash_on("sync_error_after_write", "reducing_entered"));
    env.extra_runtime = "margin_poll_ms = 100".into();
    env.extra_policy = "\n[core.policy.margin]\naccount_modes_allowed = [\"standard\"]\n".into();
    env.ctl(json!({"account_value": {"value": "100"}}));
    env.start_node();
    env.wait_ready();
    wait_margin(&env, "the account value", |m| m["account_value"] == "100");
    two_resting_plain_orders(&mut env);
    let texts = || telegram_texts(&f);
    assert!(env.wait(Duration::from_secs(5), || texts().len() == 1), "{:?}", texts());
    assert!(texts()[0].contains("[OMS P1] sim ") && texts()[0].contains("OMS 시작 (boot 1, config 1)"), "{:?}", texts());
    let st = env.status();
    assert_eq!(st["alerts"]["configured"], true, "{}", st["alerts"]);
    assert_eq!(st["alerts"]["queued"], 0, "{}", st["alerts"]);
    assert!(!st.to_string().contains("ABCdef") && !st.to_string().contains("telegram.json"));
    let seq = st["journal"]["durable_seq"].as_u64().unwrap();

    let t = std::time::Instant::now();
    env.ctl(json!({"account_value": {"value": "94"}}));
    assert_eq!(env.wait_exit(Duration::from_secs(25)), Some(2), "{}", env.log_tail(10));
    assert!(t.elapsed() < Duration::from_secs(15 + 5), "{:?}", t.elapsed());
    assert_eq!(env.journal_count("reducing_entered"), 1, "the record reached the file before the failed flush");

    let all = texts();
    assert_eq!(all.len(), 3, "start, fatal (500), fatal again: {all:?}");
    assert_eq!(all[1], all[2]);
    let fatal = &all[1];
    assert!(fatal.contains("치명 종료: commit failed: ") && fatal.contains("노드가 멈춤(exit 2)"), "{fatal}");
    // The last durable sequence: the margin reports after `seq` are
    // durable, the failed record is not.
    let shown: u64 = fatal.split("마지막 내구 seq ").nth(1).and_then(|r| r.split('.').next()).and_then(|n| n.parse().ok()).unwrap();
    assert!(shown >= seq, "{fatal}");
    let failed = env.journal_records().into_iter().find(|r| r.events.iter().any(|e| e.name() == "reducing_entered")).unwrap();
    assert!(shown < failed.seq, "{fatal}");
    for t in &all {
        assert!(!t.contains("REDUCING") && !t.contains("감소 전용") && !t.contains("daily_loss"), "{t}");
    }
    assert!(env.log_tail(10).contains("alerts delivered before the exit"), "{}", env.log_tail(10));
}

/// C-ALERT, review S1. The first journal flush after the start (the Boot
/// record, `OMS_CRASH_ON=boot`) fails, before the configuration is active.
/// The fatal stop exits with status 2, not 3, and the exit flush delivers
/// exactly one fatal message; no start notice was ever due.
#[test]
fn a_flush_failure_at_startup_exits_2_and_sends_the_fatal_message() {
    let f = http_fake::FakeHttp::start(http_fake::Answer::Reply(200, r#"{"ok":true,"result":{}}"#.into()));
    let mut env = Env::new("alert-fatal-boot");
    with_alerts(&mut env, &f);
    env.node_env.extend(crash_on("sync_error_after_write", "boot"));
    env.start_node();
    assert_eq!(env.wait_exit(Duration::from_secs(25)), Some(2), "{}", env.log_tail(10));
    let all = telegram_texts(&f);
    assert_eq!(all.len(), 1, "{all:?}");
    assert!(all[0].starts_with("[OMS P1] sim "), "{}", all[0]);
    assert!(all[0].contains("치명 종료: commit failed: ") && all[0].contains("노드가 멈춤(exit 2)"), "{}", all[0]);
    assert!(!all[0].contains("OMS 시작"), "{}", all[0]);
    assert!(env.log_tail(10).contains("alerts delivered before the exit"), "{}", env.log_tail(10));
}

/// C-ALERT, review N5. Exit 3 after the configuration is active (here the
/// trading socket cannot be bound: its directory does not exist) sends one
/// start-failure notice with the error kind only, no path and no token,
/// and the exit waits for its delivery.
#[test]
fn an_exit_3_after_activation_sends_one_start_failure_notice() {
    let f = http_fake::FakeHttp::start(http_fake::Answer::Reply(200, r#"{"ok":true,"result":{}}"#.into()));
    let mut env = Env::new("alert-exit3");
    with_alerts(&mut env, &f);
    env.extra_api = "socket = \"no-such-dir/oms.sock\"".into();
    env.start_node();
    assert_eq!(env.wait_exit(Duration::from_secs(25)), Some(3), "{}", env.log_tail(10));
    assert!(env.log_tail(10).contains("cannot open the trading socket"), "{}", env.log_tail(10));
    let all = telegram_texts(&f);
    let failed: Vec<&String> = all.iter().filter(|t| t.contains("OMS 시작 실패")).collect();
    assert_eq!(failed.len(), 1, "{all:?}");
    assert!(failed[0].starts_with("[OMS P1] sim ") && failed[0].ends_with("\nOMS 시작 실패(exit 3): trading_socket"), "{}", failed[0]);
    // Only the start notice may have gone out before it.
    for t in &all {
        assert!(t.contains("OMS 시작 실패") || t.contains("OMS 시작 (boot 1, config 1)"), "{t}");
        assert!(!t.contains("no-such-dir") && !t.contains("oms.sock") && !t.contains("ABCdef") && !t.contains("telegram.json"), "{t}");
    }
    assert!(env.log_tail(10).contains("alerts delivered before the exit"), "{}", env.log_tail(10));
}
