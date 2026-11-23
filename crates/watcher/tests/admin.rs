//! Sending confirmed differences to `admin.sock` (C-WATCH part 7, U19a7):
//! the resend rule of the engine (owed until a `"held"` answer, due only
//! while the hold is off at a cap sampled after the latest answer, one
//! send per check in flight, the resend pace), on the engine rig, whose
//! OMS answers `watcher_mismatch` and writes `hold_set` to the journal;
//! and the `admin.sock` client against a fake socket.

#![allow(clippy::unwrap_used)]

mod common;

use common::rig::*;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::time::{Duration, Instant};
use watcher::engine::{Action, Check};

/// The rounds at `T0 + k s` for each `k`.
fn rounds(r: &mut Rig, ks: std::ops::Range<u64>) {
    for k in ks {
        r.at(T0 + k * 1000);
    }
}

fn w1(r: &Rig) -> Vec<(u64, String)> {
    r.mismatches
        .iter()
        .filter(|m| m.1 == Check::W1)
        .map(|m| (m.0, m.2.clone()))
        .collect()
}

/// An order of the journal, closed there, still open at the venue.
fn closed_order(r: &mut Rig) {
    r.write(T0 - 10_000, &open(1));
    r.write(T0 - 10_000, &[of("order_closed", 1)]);
    r.venue.orders = vec![ours(1, "BTCUSDT", "Buy")];
}

fn closed_detail() -> String {
    format!(
        "W1 closed_in_journal {}: venue open, journal closed; seen 2 rounds, journal entry 11.0 s old",
        link(1)
    )
}

fn foreign_detail(order_id: &str, link: &str, age: &str) -> String {
    format!(
        "W1 foreign {order_id}: venue BTCUSDT Buy orderLinkId \"{link}\", journal no such order; seen 2 rounds, first seen {age} ago"
    )
}

fn config_with(extra: &str) -> watcher::config::Config {
    watcher::config::Config::from_toml(&format!(
        r#"name = "w"
network = "testnet"
credentials_file = "/nonexistent-watcher-engine/creds.toml"
sub_account_uid = 1
symbols = ["BTCUSDT"]
ip_echo_url = "https://ip.example/"
data_dir = "/nonexistent-watcher-engine/data"
status_url = "http://127.0.0.1:8188/api/status"
host = "h"
{extra}"#
    ))
    .unwrap()
}

#[test]
fn a_confirmed_mismatch_is_sent_once_and_again_after_a_clear() {
    let mut r = Rig::new("admin-once-and-again");
    closed_order(&mut r);
    rounds(&mut r, 0..31);
    // Sent at the confirmation, answered "held" (the OMS wrote hold_set);
    // every later round reads the hold and sends nothing.
    assert_eq!(w1(&r), vec![(T0 + 1000, closed_detail())]);
    assert!(r.held);
    assert_eq!(r.gauge("watcher_mismatch_active", "W1"), Some(1.0));
    // An operator clears the hold while the difference persists: the next
    // round reads the clear and sends again, with the detail of the
    // confirmation; the hold comes back.
    r.clear(T0 + 30_500);
    rounds(&mut r, 31..61);
    let again = vec![(T0 + 1000, closed_detail()), (T0 + 31_000, closed_detail())];
    assert_eq!(w1(&r), again);
    assert!(r.held);
    // The difference ends; after the next clear nothing is sent.
    r.venue.orders.clear();
    rounds(&mut r, 61..63);
    assert_eq!(r.gauge("watcher_mismatch_active", "W1"), Some(0.0));
    r.clear(T0 + 63_500);
    rounds(&mut r, 64..90);
    assert_eq!(w1(&r), again);
    assert_eq!(r.gauge("watcher_mismatches_total", "W1"), Some(1.0));
}

#[test]
fn a_confirmed_mismatch_is_sent_until_answered_even_after_the_difference_is_gone() {
    let mut r = Rig::new("admin-owed");
    r.venue.orders = vec![row("x", "v900", "BTCUSDT", "Buy")];
    // The first send fails: nothing is paid.
    r.admin_script
        .push_back(Err("unavailable: the OMS is stopping".into()));
    rounds(&mut r, 0..3);
    let d = foreign_detail("v900", "x", "2.0 s");
    assert_eq!(w1(&r), vec![(T0 + 2000, d.clone())]);
    assert_eq!(
        r.lines_with("warn: W1 mismatch send failed: unavailable"),
        1
    );
    // The order leaves the venue: the difference is gone, still owed.
    r.venue.orders.clear();
    rounds(&mut r, 3..12);
    assert_eq!(w1(&r).len(), 1);
    assert_eq!(r.gauge("watcher_mismatch_active", "W1"), Some(1.0));
    // 10 s after the first send's start it goes again, as it was at the
    // confirmation, and is answered "held".
    rounds(&mut r, 12..13);
    assert_eq!(
        w1(&r),
        vec![(T0 + 2000, d.clone()), (T0 + 12_000, d.clone())]
    );
    assert_eq!(r.gauge("watcher_mismatch_active", "W1"), Some(0.0));
    // Paid: neither the hold nor a clear brings it back.
    rounds(&mut r, 13..40);
    r.clear(T0 + 40_500);
    rounds(&mut r, 41..70);
    assert_eq!(w1(&r).len(), 2);
}

#[test]
fn a_difference_confirmed_under_a_hold_is_sent_after_the_clear() {
    let mut r = Rig::new("admin-under-hold");
    closed_order(&mut r);
    // A hold set by another send (W2) before the W1 difference.
    let _ = r.hold(Check::W2, "W2 position BTCUSDT: …");
    rounds(&mut r, 0..6);
    // Confirmed at T0 + 1 s while the journal shows the hold: owed, not sent.
    assert!(w1(&r).is_empty());
    assert_eq!(r.gauge("watcher_mismatches_total", "W1"), Some(1.0));
    assert_eq!(r.gauge("watcher_mismatch_active", "W1"), Some(1.0));
    // The operator clears it and another send sets it again before the
    // watcher reads that: the watcher's state shows the hold off, it sends,
    // and the OMS answers "already held", which pays nothing.
    r.clear(T0 + 5_500);
    r.cap = Some(r.journal.next_seq - 1);
    let _ = r.hold(Check::W2, "W2 position BTCUSDT: …");
    rounds(&mut r, 6..7);
    assert_eq!(w1(&r), vec![(T0 + 6000, closed_detail())]);
    r.cap = None;
    // The order is closed at the venue meanwhile: the difference is gone,
    // still owed; the hold is read and nothing is sent.
    r.venue.orders.clear();
    rounds(&mut r, 7..20);
    assert_eq!(w1(&r).len(), 1);
    assert_eq!(r.gauge("watcher_mismatch_active", "W1"), Some(1.0));
    // After the clear it is sent once, answered "held", and paid.
    r.clear(T0 + 20_000);
    rounds(&mut r, 20..40);
    let want = vec![(T0 + 6000, closed_detail()), (T0 + 20_000, closed_detail())];
    assert_eq!(w1(&r), want);
    assert_eq!(r.gauge("watcher_mismatch_active", "W1"), Some(0.0));
    r.clear(T0 + 40_000);
    rounds(&mut r, 40..60);
    assert_eq!(w1(&r), want);
}

#[test]
fn a_second_send_waits_for_the_first_answer() {
    let cfg =
        config_with("[rules]\nmismatch_resend_ms = 1000\n[http]\nrequest_timeout_ms = 10000\n");
    let mut r = Rig::with_config("admin-one-in-flight", &cfg);
    r.oms = Oms::Silent;
    r.venue.orders = vec![row("a", "va", "BTCUSDT", "Buy")];
    rounds(&mut r, 0..3);
    let a = foreign_detail("va", "a", "2.0 s");
    assert_eq!(w1(&r), vec![(T0 + 2000, a.clone())]);
    // A second difference is confirmed while the first send has no answer;
    // the resend pace (1 s) has long passed, but no second send starts.
    r.venue.orders.push(row("b", "vb", "BTCUSDT", "Buy"));
    rounds(&mut r, 3..10);
    assert_eq!(w1(&r).len(), 1);
    assert_eq!(r.gauge("watcher_mismatches_total", "W1"), Some(2.0));
    // The first send's "held" pays what it carried, not the second.
    let held = r.hold(Check::W1, &a);
    let more = r
        .engine
        .on_admin(T0 + 9_500, Check::W1, r.send_ids[0], held);
    assert!(more.is_empty(), "{more:?}");
    r.venue.orders.retain(|o| o["orderId"] == "va");
    rounds(&mut r, 10..15);
    assert_eq!(w1(&r).len(), 1);
    assert_eq!(r.gauge("watcher_mismatch_active", "W1"), Some(2.0));
    // After the clear the owed one goes first, with the still-seen one.
    r.clear(T0 + 15_000);
    rounds(&mut r, 15..17);
    let b = foreign_detail("vb", "b", "2.0 s");
    assert_eq!(w1(&r)[1], (T0 + 15_000, format!("{b}; and 1 more")));
    // A send without a report fails at emission + 2 × request_timeout_ms
    // + 1 s, and the next one starts then, not before.
    rounds(&mut r, 17..36);
    assert_eq!(w1(&r).len(), 2);
    rounds(&mut r, 36..37);
    assert_eq!(w1(&r)[2], (T0 + 36_000, format!("{b}; and 1 more")));
    assert_eq!(r.lines_with("warn: W1 mismatch send has no report"), 1);
}

#[test]
fn a_late_report_of_one_send_pays_only_what_that_send_carried() {
    let cfg =
        config_with("[rules]\nmismatch_resend_ms = 1000\n[http]\nrequest_timeout_ms = 10000\n");
    let mut r = Rig::with_config("admin-late-report", &cfg);
    r.oms = Oms::Silent;
    r.venue.orders = vec![row("a", "va", "BTCUSDT", "Buy")];
    rounds(&mut r, 0..3);
    let a = foreign_detail("va", "a", "2.0 s");
    assert_eq!(w1(&r), vec![(T0 + 2000, a.clone())]);
    // B is confirmed while send 1 (A) has no report; send 1 is taken as
    // failed at emission + 2 × request_timeout_ms + 1 s, and send 2 then
    // carries A and B.
    r.venue.orders.push(row("b", "vb", "BTCUSDT", "Buy"));
    rounds(&mut r, 3..24);
    assert_eq!(w1(&r).len(), 2);
    assert_eq!(w1(&r)[1], (T0 + 23_000, format!("{a}; and 1 more")));
    // Send 1's report comes now: "held", the hold's note names A only. It
    // belongs to no send in flight and pays nothing, not what send 2
    // carried.
    let held = r.hold(Check::W1, &a);
    assert_eq!(held, Ok("held".to_string()));
    assert_eq!(r.send_ids, vec![1, 2]);
    let more = r.engine.on_admin(T0 + 23_100, Check::W1, 1, held);
    assert!(more.is_empty(), "{more:?}");
    assert_eq!(r.gauge("watcher_mismatch_active", "W1"), Some(2.0));
    assert_eq!(
        r.lines_with("debug: an answer to W1 send 1, which is not the send in flight"),
        1
    );
    // Send 2's own report: "already held", which pays nothing either.
    let already = r.hold(Check::W1, &a);
    let more = r.engine.on_admin(T0 + 23_200, Check::W1, 2, already);
    assert!(more.is_empty(), "{more:?}");
    assert_eq!(r.gauge("watcher_mismatch_active", "W1"), Some(2.0));
    // Both orders leave the venue; the hold is read, nothing is sent.
    r.venue.orders.clear();
    rounds(&mut r, 24..30);
    assert_eq!(w1(&r).len(), 2);
    assert_eq!(r.gauge("watcher_mismatch_active", "W1"), Some(2.0));
    // After the clear A and B go once more, and the new hold names A and B.
    r.clear(T0 + 30_000);
    rounds(&mut r, 30..31);
    assert_eq!(w1(&r)[2], (T0 + 30_000, format!("{a}; and 1 more")));
    let held = r.hold(Check::W1, &w1(&r)[2].1);
    assert!(
        r.engine
            .on_admin(T0 + 30_100, Check::W1, 3, held)
            .is_empty()
    );
    assert_eq!(r.gauge("watcher_mismatch_active", "W1"), Some(0.0));
    r.clear(T0 + 31_000);
    rounds(&mut r, 31..40);
    assert_eq!(w1(&r).len(), 3);
}

#[test]
fn a_hold_seen_off_at_a_cap_sampled_before_the_answer_does_not_send() {
    let cfg = config_with("[rules]\nmismatch_resend_ms = 1000\n");
    let mut r = Rig::with_config("admin-early-cap", &cfg);
    r.oms = Oms::Silent;
    closed_order(&mut r);
    rounds(&mut r, 0..2);
    assert_eq!(w1(&r).len(), 1);
    // A round asks for its status before the answer...
    let a = r.engine.poll(T0 + 2000);
    let Some(Action::Venue {
        req_id,
        path,
        params,
    }) = a.into_iter().next()
    else {
        panic!()
    };
    let page = r.venue.answer(path, &params);
    let early = match &r.engine.on_venue(T0 + 2000, req_id, page)[..] {
        [Action::Status { req_id }] => *req_id,
        a => panic!("{a:?}"),
    };
    let cap_before = r.journal.next_seq - 1;
    // ...the send is answered "held" (hold_set written)...
    let held = r.hold(Check::W1, &closed_detail());
    let id = r.send_ids[0];
    assert!(r.engine.on_admin(T0 + 2100, Check::W1, id, held).is_empty());
    // ...and the early status's cap predates the hold_set: the hold looks
    // off there, but that cap proves nothing about the answer.
    r.cap = Some(cap_before);
    let s = r.status();
    let out = r.engine.on_status(T0 + 2200, early, Ok(s));
    assert!(out.is_empty(), "{out:?}");
    r.cap = None;
    rounds(&mut r, 3..10);
    assert_eq!(w1(&r).len(), 1);
    // A clear read at a later cap opens the way again.
    r.clear(T0 + 10_000);
    rounds(&mut r, 10..11);
    assert_eq!(w1(&r)[1], (T0 + 10_000, closed_detail()));
}

/// A fake `admin.sock`: answers each accepted connection with `answer`
/// (given the request lines), records the lines.
fn fake_admin(
    path: &std::path::Path,
    answer: impl Fn(&[String]) -> Option<String> + Send + 'static,
) -> std::sync::Arc<std::sync::Mutex<Vec<Vec<String>>>> {
    let _ = std::fs::remove_file(path);
    let l = UnixListener::bind(path).unwrap();
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let s = seen.clone();
    std::thread::spawn(move || {
        for c in l.incoming() {
            let Ok(c) = c else { return };
            let mut lines = Vec::new();
            let mut rd = BufReader::new(c.try_clone().unwrap());
            let mut w = c;
            for _ in 0..2 {
                let mut line = String::new();
                if rd.read_line(&mut line).unwrap_or(0) == 0 {
                    break;
                }
                lines.push(line.trim_end().to_string());
                let v: serde_json::Value = serde_json::from_str(&lines[lines.len() - 1]).unwrap();
                let reply = if v["op"] == "hello" {
                    Some(format!(
                        r#"{{"id":{},"ok":true,"result":{{"instance_id":"0000000000000007"}}}}"#,
                        v["id"]
                    ))
                } else {
                    answer(&lines)
                };
                if let Some(reply) = reply {
                    let _ = w.write_all(format!("{reply}\n").as_bytes());
                }
            }
            // Keep the connection until the client closes it.
            let mut rest = String::new();
            let _ = rd.read_line(&mut rest);
            s.lock().unwrap().push(lines);
        }
    });
    seen
}

#[test]
fn the_admin_client_says_hello_and_reads_both_replies() {
    let dir = common::test_dir("admin-client");
    let sock = dir.join("admin.sock");
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    // A missing socket fails at once.
    let r = rt.block_on(watcher::oms::admin_send(&sock, "w", Check::W1, "d", 1000));
    assert!(r.unwrap_err().starts_with("not sent:"), "missing socket");
    let answers = [
        (
            r#"{"id":2,"ok":true,"result":"held"}"#,
            Ok("held".to_string()),
        ),
        (
            r#"{"id":2,"ok":true,"result":"already held"}"#,
            Ok("already held".to_string()),
        ),
        (
            r#"{"id":2,"ok":false,"error":{"code":"bad_request","message":"detail is empty"}}"#,
            Err("bad_request: detail is empty".to_string()),
        ),
        (
            r#"{"id":2,"ok":false,"error":{"code":"overloaded","message":"queue full"}}"#,
            Err("overloaded: queue full".to_string()),
        ),
    ];
    for (reply, want) in answers {
        let reply = reply.to_string();
        let seen = fake_admin(&sock, move |_| Some(reply.clone()));
        let got = rt.block_on(watcher::oms::admin_send(
            &sock,
            "w-1",
            Check::W2,
            "W2 position X",
            1000,
        ));
        assert_eq!(got, want);
        // The fake records a connection when the client has closed it.
        let end = Instant::now() + Duration::from_secs(5);
        while seen.lock().unwrap().is_empty() && Instant::now() < end {
            std::thread::sleep(Duration::from_millis(5));
        }
        let lines = seen.lock().unwrap().clone();
        assert_eq!(
            lines.last().unwrap(),
            &vec![
                r#"{"id":1,"op":"hello","client":"watcher/w-1"}"#.to_string(),
                r#"{"id":2,"op":"watcher_mismatch","check":"W2","detail":"W2 position X"}"#
                    .to_string(),
            ]
        );
    }
    // No reply: the whole send ends within 2 × the timeout.
    let _seen = fake_admin(&sock, |_| None);
    let t = Instant::now();
    let got = rt.block_on(watcher::oms::admin_send(&sock, "w", Check::W1, "d", 300));
    assert!(got.unwrap_err().contains("no reply"), "timeout");
    assert!(
        t.elapsed() < Duration::from_millis(1500),
        "{:?}",
        t.elapsed()
    );
}
