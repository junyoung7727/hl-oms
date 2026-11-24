//! The watcher's kill sequence (C-WATCH part 12, U19b3b): the sans-IO
//! `Kill` (K1 marker → K2 `watcher_kill` → K3 `systemctl stop` → the
//! loop, each step with a deadline, the owed halt retried during the loop,
//! `Kill::abort` in every step), driven by hand on its own clock (no task,
//! no socket, no program), and the marker writer on a directory under
//! `OMS_TEST_DIR`.

#![allow(clippy::unwrap_used)]

mod common;

use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use serde_json::{Value, json};
use watcher::alert::{MSG_LIMIT, message, split};
use watcher::clock::{Clock, ManualClock, Steady};
use watcher::config::Kill as KillCfg;
use watcher::kill::{
    AbortWhy, Action, Arrived, CancelAnswer, DETAIL_MAX, Kill, MARKER_TEXT_MAX, MarkerOutcome,
    NOT_READ_ON_ABORT, NotSent, Notice, Positions, Report, Setup, Stage, Step, StopOutcome,
    halt_detail, late_halt_detail, marker_text, notice_text, position_block, spawn_marker, utc_ms,
    write_marker,
};
use watcher::rest::CancelAll;
use watcher::stop::Outcome;
use watcher::w5::Fire;

/// 2023-11-14T22:13:20Z.
const T0: u64 = 1_700_000_000_000;
const UNIT: &str = "watcher-test-no-such-unit.service";

fn kill_cfg(marker: PathBuf) -> KillCfg {
    KillCfg {
        marker,
        oms_unit: UNIT.to_string(),
        ..KillCfg::default()
    }
}

// ---- Texts ----

#[test]
fn the_marker_text_and_detail_are_cut_at_a_character_boundary() {
    assert_eq!(utc_ms(T0), "2023-11-14T22:13:20.000Z");
    assert_eq!(utc_ms(T0 + 1_234), "2023-11-14T22:13:21.234Z");
    assert_eq!(utc_ms(951_782_400_000), "2000-02-29T00:00:00.000Z");
    assert_eq!(
        marker_text("w-1", T0, 10_100, true),
        "watcher w-1 W5 2023-11-14T22:13:20.000Z: /api/status gave no good answer for 10.1 s (last good answer 2023-11-14T22:13:09.900Z); the watcher stops the OMS and cancels every order with its own key. Clear with omsctl clear-hold --reason kill."
    );
    assert!(
        marker_text("w-1", T0, 10_000, false)
            .contains("for 10 s (last good answer none in this process);"),
    );
    assert_eq!(
        halt_detail("w-1", 10_000),
        "W5: /api/status gave no good answer for 10 s; watcher w-1"
    );
    assert_eq!(
        late_halt_detail("w-1"),
        "W5: 늦은 정지: watcher w-1의 킬이 OMS 래치나 정지를 확인하지 못함"
    );

    // A 64-character name of 4-byte characters (the config's longest)
    // fits whole; a longer one (Setup takes any) is cut.
    let name = "𝕏".repeat(64);
    let fits = marker_text(&name, T0, 10_000, true);
    assert!(fits.len() <= MARKER_TEXT_MAX && fits.ends_with("--reason kill."));
    let longer = "𝕏".repeat(100);
    let text = marker_text(&longer, T0, 10_000, true);
    let full = format!(
        "watcher {longer} W5 2023-11-14T22:13:20.000Z: /api/status gave no good answer for 10 s (last good answer 2023-11-14T22:13:10.000Z); the watcher stops the OMS and cancels every order with its own key. Clear with omsctl clear-hold --reason kill."
    );
    assert!(full.len() > MARKER_TEXT_MAX, "the case must cut");
    assert!(text.len() <= MARKER_TEXT_MAX, "{} bytes", text.len());
    assert!(full.starts_with(&text));
    // Cut at a whole character, as late as possible.
    let next = full[text.len()..].chars().next().unwrap();
    assert!(text.len() + next.len_utf8() > MARKER_TEXT_MAX);
    // The OMS reads 512 bytes and decodes them lossily: nothing is lost.
    assert!(!String::from_utf8_lossy(&text.as_bytes()[..text.len()]).contains('\u{FFFD}'));

    for d in [halt_detail(&name, 10_000), late_halt_detail(&name)] {
        assert!(d.len() <= DETAIL_MAX && d.contains(&name), "{d}");
    }
    // A name longer than the config allows still gives a detail within
    // the OMS's 1024 bytes, cut at a whole character.
    let long = "가".repeat(1000);
    for (d, full) in [
        (
            halt_detail(&long, 10_000),
            format!("W5: /api/status gave no good answer for 10 s; watcher {long}"),
        ),
        (
            late_halt_detail(&long),
            format!("W5: 늦은 정지: watcher {long}의 킬이 OMS 래치나 정지를 확인하지 못함"),
        ),
    ] {
        assert!(
            d.len() <= DETAIL_MAX && full.starts_with(&d),
            "{} bytes",
            d.len()
        );
        let next = full[d.len()..].chars().next().unwrap();
        assert!(d.len() + next.len_utf8() > DETAIL_MAX);
    }
}

// ---- The marker writer ----

fn names(dir: &std::path::Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    v.sort();
    v
}

#[tokio::test]
async fn a_marker_file_is_what_a_marker_rule_reads() {
    let dir = common::test_dir("kseq-marker-file");
    let marker = dir.join("oms.kill");
    let text = marker_text("w-1", T0, 10_000, true);
    // A temporary file of this process left by an earlier run (the same
    // pid after a reboot) is removed first, never the marker.
    let stale = dir.join(format!(".oms.kill.watcher-{}.tmp", std::process::id()));
    std::fs::write(&stale, "stale").unwrap();

    // The runtime runs the write on spawn_blocking.
    let out = spawn_marker(marker.clone(), text.clone()).await.unwrap();
    assert_eq!(out, MarkerOutcome::Written);
    let meta = std::fs::symlink_metadata(&marker).unwrap();
    assert!(meta.file_type().is_file());
    assert_eq!(meta.permissions().mode() & 0o777, 0o600);
    assert_eq!(std::fs::read_to_string(&marker).unwrap(), text);
    // In the marker's directory, and no temporary file left.
    assert_eq!(names(&dir), vec!["oms.kill".to_string()]);
}

#[test]
fn an_existing_marker_is_left_as_it_is() {
    let dir = common::test_dir("kseq-marker-existing");
    // A file, a dangling symlink and a directory at the marker path: each
    // is a marker already there, not touched and not replaced.
    let file = dir.join("a.kill");
    std::fs::write(&file, "operator's marker").unwrap();
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(write_marker(&file, "new"), MarkerOutcome::AlreadyPresent);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "operator's marker");
    assert_eq!(
        std::fs::metadata(&file).unwrap().permissions().mode() & 0o777,
        0o644
    );

    let link = dir.join("b.kill");
    std::os::unix::fs::symlink(dir.join("nowhere"), &link).unwrap();
    assert_eq!(write_marker(&link, "new"), MarkerOutcome::AlreadyPresent);
    assert!(
        std::fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert!(!dir.join("nowhere").exists());

    let sub = dir.join("c.kill");
    std::fs::create_dir(&sub).unwrap();
    assert_eq!(write_marker(&sub, "new"), MarkerOutcome::AlreadyPresent);
    assert!(sub.is_dir() && names(&sub).is_empty());
    assert_eq!(names(&dir), vec!["a.kill", "b.kill", "c.kill"]);
}

#[test]
fn a_marker_that_cannot_be_written_does_not_stop_the_sequence() {
    let dir = common::test_dir("kseq-marker-fails");
    // The directory is gone: the io kind, nothing written.
    let gone = dir.join("gone").join("oms.kill");
    assert_eq!(
        write_marker(&gone, "x"),
        MarkerOutcome::Failed("NotFound".to_string())
    );

    // The sequence goes on: the halt follows the failed write at once.
    let mut r = Run::new(true);
    let steps = r.marker(T0 + 50, MarkerOutcome::Failed("NotFound".into()));
    assert!(matches!(steps.as_slice(), [Step::Halt { .. }]), "{steps:?}");
    assert!(
        r.k.header().contains("킬 마커: 쓰지 못함(NotFound)."),
        "{}",
        r.k.header()
    );
    r.halt(T0 + 100, Err("no socket"));
    let steps = r.stop(T0 + 200, Outcome::Stopped);
    assert!(cancel_all(&steps), "{steps:?}");
    assert_eq!(r.k.outcome(), StopOutcome::StoppedUnmarked);
}

// ---- The sequence (K1 → K2 → K3 → loop) ----

const FIRE: Fire = Fire {
    last_good_ms: T0 - 10_000,
    silent_ms: 10_000,
};
const MARKER: &str = "/srv/oms/oms.kill";
/// `request_timeout_ms` of the tests: K2's deadline is 2 × 2,000 + 1,000.
const RT: u64 = 2000;
const K2_MS: u64 = 2 * RT + 1000;
const X8: &str = ". OMS 정지 실패: OMS가 새 주문을 낼 수 있음. OMS 키 폐기(X8 절차)를 검토";

fn setup(checked: bool) -> Setup {
    Setup {
        name: "w-1".to_string(),
        kill: kill_cfg(PathBuf::from(MARKER)),
        request_timeout_ms: RT,
        marker_checked: checked,
        answered: true,
        raw_now_ms: T0,
    }
}

fn cancel_all(steps: &[Step]) -> bool {
    steps
        .iter()
        .any(|s| matches!(s, Step::Loop(Action::CancelAll { .. })))
}

fn halt_id(steps: &[Step]) -> Option<u64> {
    steps.iter().find_map(|s| match s {
        Step::Halt { id, .. } => Some(*id),
        _ => None,
    })
}

/// A kill driven by hand; every step it emitted, with its instant.
struct Run {
    k: Kill,
    log: Vec<(u64, Step)>,
    /// The loop's requests not yet answered by `drive`.
    pending: Vec<Step>,
}

impl Run {
    fn new(checked: bool) -> Run {
        Run::with(setup(checked))
    }

    fn with(s: Setup) -> Run {
        Run::fired(FIRE, s)
    }

    fn fired(fire: Fire, s: Setup) -> Run {
        let (k, steps) = Kill::new(fire, s, T0);
        let mut r = Run {
            k,
            log: Vec::new(),
            pending: Vec::new(),
        };
        r.keep(T0, steps);
        r
    }

    fn keep(&mut self, now: u64, steps: Vec<Step>) -> Vec<Step> {
        self.log.extend(steps.iter().cloned().map(|s| (now, s)));
        self.pending.extend(
            steps
                .iter()
                .filter(|s| {
                    matches!(
                        s,
                        Step::Loop(Action::CancelAll { .. } | Action::Count { .. })
                    )
                })
                .cloned(),
        );
        steps
    }

    fn marker(&mut self, now: u64, o: MarkerOutcome) -> Vec<Step> {
        let steps = self.k.marker_done(now, o);
        self.keep(now, steps)
    }

    /// The reply to the open halt attempt.
    fn halt(&mut self, now: u64, r: Result<&str, &str>) -> Vec<Step> {
        let (id, _) = self.k.halt_open().expect("a halt is open");
        let r = r.map(str::to_string).map_err(str::to_string);
        let steps = self.k.halt_done(now, id, r);
        self.keep(now, steps)
    }

    fn stop(&mut self, now: u64, o: Outcome) -> Vec<Step> {
        let steps = self.k.stop_done(now, o);
        self.keep(now, steps)
    }

    fn poll(&mut self, now: u64) -> Vec<Step> {
        let steps = self.k.poll(now);
        self.keep(now, steps)
    }

    fn arrived(&mut self, now: u64, a: Arrived) -> Vec<Step> {
        let steps = self.k.arrived(now, a);
        self.keep(now, steps)
    }

    fn index(&self, f: impl Fn(&Step) -> bool) -> Option<usize> {
        self.log.iter().position(|(_, s)| f(s))
    }

    fn notices(&self) -> Vec<Notice> {
        self.log
            .iter()
            .filter_map(|(_, s)| match s {
                Step::Loop(Action::Notice(n)) => Some(n.clone()),
                _ => None,
            })
            .collect()
    }

    fn halts(&self) -> Vec<u64> {
        self.log
            .iter()
            .filter_map(|(t, s)| matches!(s, Step::Halt { .. }).then_some(*t))
            .collect()
    }

    /// Answers every request of the loop at once (cancel-alls `cancel`,
    /// counts `open`) and every halt retry with `halt`, polling every
    /// 100 ms from `now` until the kill ends or `until`; step-bounded.
    fn drive(
        &mut self,
        now: u64,
        until: u64,
        cancel: impl Fn() -> CancelAnswer,
        open: usize,
        halt: Result<&'static str, &'static str>,
    ) -> u64 {
        self.drive_with(now, until, cancel, open, |_, _| Some(halt))
    }

    /// As `drive`, but the open halt attempt is answered with what
    /// `halt(now, emitted)` gives at each step (`None`: not yet).
    fn drive_with(
        &mut self,
        mut now: u64,
        until: u64,
        cancel: impl Fn() -> CancelAnswer,
        open: usize,
        mut halt: impl FnMut(u64, u64) -> Option<Result<&'static str, &'static str>>,
    ) -> u64 {
        for _ in 0..20_000 {
            if self.k.stage() == Stage::Ended || now >= until {
                return now;
            }
            self.poll(now);
            while let Some(s) = self.pending.pop() {
                match s {
                    Step::Loop(Action::CancelAll { req_id }) => self.arrived(
                        now,
                        Arrived::Cancel {
                            req_id,
                            answer: cancel(),
                        },
                    ),
                    Step::Loop(Action::Count { req_id }) => self.arrived(
                        now,
                        Arrived::Counted {
                            req_id,
                            result: Ok(open),
                        },
                    ),
                    _ => Vec::new(),
                };
            }
            if self.k.halt_open().is_some() {
                let emitted = *self.halts().last().unwrap();
                if let Some(r) = halt(now, emitted) {
                    let more = self.halt(now, r);
                    assert!(
                        !more.iter().any(|s| matches!(s, Step::Halt { .. })),
                        "{more:?}"
                    );
                }
            }
            now += 100;
        }
        panic!("the step bound was reached");
    }

    fn ended(&self) -> Option<(Report, bool)> {
        self.log.iter().find_map(|(_, s)| match s {
            Step::Ended { report, halt_owed } => Some((report.clone(), *halt_owed)),
            _ => None,
        })
    }
}

fn zero() -> CancelAnswer {
    CancelAnswer::Done(CancelAll { cancelled: 0 })
}

#[test]
fn the_marker_and_the_halt_come_before_the_first_cancel_all() {
    // (marker, halt reply, stop outcome or none when the halt latched)
    let cases: [(MarkerOutcome, Result<&str, &str>, Option<Outcome>); 4] = [
        (MarkerOutcome::Written, Ok("latched"), None),
        (
            MarkerOutcome::Written,
            Err("no socket"),
            Some(Outcome::Stopped),
        ),
        (
            MarkerOutcome::Written,
            Err("no socket"),
            Some(Outcome::Failed("exit 1".into())),
        ),
        (
            MarkerOutcome::Failed("PermissionDenied".into()),
            Ok("latched"),
            None,
        ),
    ];
    for (marker, reply, stop) in cases {
        let mut r = Run::new(true);
        assert!(matches!(r.log.as_slice(), [(T0, Step::WriteMarker { .. })]));
        // Nothing but the write until its outcome (or its deadline).
        assert!(r.poll(T0 + 100).is_empty());
        let steps = r.marker(T0 + 200, marker.clone());
        assert!(
            halt_id(&steps).is_some() && !cancel_all(&steps),
            "{steps:?}"
        );
        assert!(r.poll(T0 + 300).is_empty());
        let steps = r.halt(T0 + 400, reply);
        match &stop {
            None => assert!(cancel_all(&steps), "{steps:?}"),
            Some(o) => {
                assert!(
                    matches!(steps.as_slice(), [Step::StopUnit { deadline }] if *deadline == T0 + 400 + 10_000),
                    "{steps:?}"
                );
                assert!(r.poll(T0 + 500).is_empty());
                let steps = r.stop(T0 + 600, o.clone());
                assert!(cancel_all(&steps), "{steps:?}");
            }
        }
        let first_cancel = r
            .index(|s| matches!(s, Step::Loop(Action::CancelAll { .. })))
            .unwrap();
        let marker_at = r.index(|s| matches!(s, Step::WriteMarker { .. })).unwrap();
        let halt_at = r.index(|s| matches!(s, Step::Halt { .. })).unwrap();
        assert!(marker_at < halt_at && halt_at < first_cancel);
        match r.index(|s| matches!(s, Step::StopUnit { .. })) {
            Some(stop_at) => assert!(stop.is_some() && halt_at < stop_at && stop_at < first_cancel),
            None => assert!(stop.is_none(), "the stop must run when the halt failed"),
        }
        // The start notice comes with the first cancel-all, never before
        // the steps' outcomes.
        let start = r
            .index(|s| matches!(s, Step::Loop(Action::Notice(Notice::Start { .. }))))
            .unwrap();
        assert!(start < first_cancel && halt_at < start);
        assert_eq!(r.k.stage(), Stage::Loop);
    }
}

#[test]
fn a_marker_write_that_does_not_return_does_not_hold_the_halt() {
    let mut r = Run::new(true);
    assert_eq!(r.k.next_wake(), Some(T0 + 2000));
    assert!(r.poll(T0 + 1999).is_empty());
    let steps = r.poll(T0 + 2000);
    assert!(halt_id(&steps).is_some(), "{steps:?}");
    assert!(
        r.k.header()
            .contains("킬 마커: 쓰지 못함(시간 초과; 뒤늦게 써질 수 있음). OMS 정지: 진행 중"),
        "{}",
        r.k.header()
    );
    // The late outcome is ignored: the header still says it timed out.
    assert!(r.marker(T0 + 2500, MarkerOutcome::Written).is_empty());
    assert!(r.k.header().contains("쓰지 못함(시간 초과"));
    r.halt(T0 + 2600, Ok("latched"));
    assert_eq!(r.k.outcome(), StopOutcome::Latched);
}

#[test]
fn a_halt_without_an_outcome_goes_to_the_stop_at_its_deadline() {
    let mut r = Run::new(true);
    let t1 = T0 + 100;
    r.marker(t1, MarkerOutcome::Written);
    assert_eq!(r.k.halt_open().map(|(_, d)| d), Some(t1 + K2_MS));
    assert_eq!(r.k.next_wake(), Some(t1 + K2_MS));
    assert!(r.poll(t1 + K2_MS - 1).is_empty());
    let steps = r.poll(t1 + K2_MS);
    assert!(
        matches!(steps.as_slice(), [Step::StopUnit { deadline }] if *deadline == t1 + K2_MS + 10_000),
        "{steps:?}"
    );
    // The runtime aborts the task: no attempt is open any more.
    assert_eq!(r.k.halt_open(), None);
    assert!(
        r.k.header().ends_with(&format!(
            "OMS 정지: watcher_kill 실패(정지 응답 없음(시간 초과)) → systemctl stop {UNIT}: 진행 중"
        )),
        "{}",
        r.k.header()
    );
    // A late reply (even one that latched) changes nothing.
    let id = halt_id(&r.log.iter().map(|(_, s)| s.clone()).collect::<Vec<_>>()).unwrap();
    assert!(
        r.k.halt_done(t1 + K2_MS + 1, id, Ok("latched".into()))
            .is_empty()
    );
    assert_eq!(r.k.stage(), Stage::Stop);

    // A reply handled at the deadline before any poll applied it is taken
    // (the truth about the OMS first).
    let mut r = Run::new(true);
    r.marker(t1, MarkerOutcome::Written);
    let steps = r.halt(t1 + K2_MS, Ok("latched"));
    assert!(cancel_all(&steps), "{steps:?}");
    assert_eq!(r.k.outcome(), StopOutcome::Latched);
}

#[test]
fn a_marker_written_before_the_path_check_says_so() {
    for (checked, marker, text) in [
        (true, MarkerOutcome::Written, "킬 마커: 씀. "),
        (
            false,
            MarkerOutcome::Written,
            "킬 마커: 씀(OMS가 읽는 경로인지 확인 못 함). ",
        ),
        (true, MarkerOutcome::AlreadyPresent, "킬 마커: 이미 있음. "),
        (
            false,
            MarkerOutcome::AlreadyPresent,
            "킬 마커: 이미 있음(OMS가 읽는 경로인지 확인 못 함). ",
        ),
    ] {
        let mut r = Run::new(checked);
        r.marker(T0 + 100, marker);
        assert!(r.k.header().contains(text), "{}", r.k.header());
    }
}

#[test]
fn already_latched_and_the_rearm_reply_count_as_halted() {
    let rearm = "already latched; the earlier confirmation is void and a new kill loop starts";
    for reply in ["latched", "already latched", rearm] {
        let mut r = Run::new(true);
        r.marker(T0 + 100, MarkerOutcome::Written);
        let steps = r.halt(T0 + 200, Ok(reply));
        assert!(cancel_all(&steps), "{reply}: {steps:?}");
        assert!(r.index(|s| matches!(s, Step::StopUnit { .. })).is_none());
        assert_eq!(r.k.outcome(), StopOutcome::Latched);
        assert!(!r.k.halt_owed());
        assert_eq!(
            r.k.kill_loop().header(),
            format!(
                "W5: OMS /api/status 무응답 10초, 마지막 정상 응답 11-15 07:13:10 KST. 킬 마커: 씀. OMS 정지: admin.sock watcher_kill → {reply}"
            )
        );
    }

    // A latch reply with the OMS's long stored detail after it: cut to
    // 200 UTF-16 units at a whole character, control characters as
    // spaces (`already latched \n\u{7}` is 18 units; review 2 S2).
    let mut r = Run::new(true);
    r.marker(T0 + 100, MarkerOutcome::Written);
    let reply = format!("already latched \n\u{7}{}", "𝕏".repeat(300));
    let steps = r.halt(T0 + 200, Ok(&reply));
    assert!(cancel_all(&steps), "{steps:?}");
    assert_eq!(r.k.outcome(), StopOutcome::Latched);
    let header = r.k.kill_loop().header();
    assert!(!header.chars().any(char::is_control), "{header}");
    assert!(
        header.ends_with(&format!(
            "OMS 정지: admin.sock watcher_kill → already latched   {}",
            "𝕏".repeat(91)
        )),
        "{header}"
    );
}

#[test]
fn a_halt_answered_with_an_error_goes_to_the_stop() {
    for (reply, why) in [
        (Err("bad_request: detail"), "bad_request: detail"),
        (Err("unauthorized: uid"), "unauthorized: uid"),
        // A reply that is no latch is not halted either.
        (Ok("held"), "held"),
        (Ok("not latched"), "not latched"),
    ] {
        let mut r = Run::new(true);
        r.marker(T0 + 100, MarkerOutcome::Written);
        let steps = r.halt(T0 + 200, reply);
        assert!(
            matches!(steps.as_slice(), [Step::StopUnit { deadline }] if *deadline == T0 + 200 + 10_000),
            "{why}: {steps:?}"
        );
        assert!(
            r.k.header().ends_with(&format!(
                "OMS 정지: watcher_kill 실패({why}) → systemctl stop {UNIT}: 진행 중"
            )),
            "{}",
            r.k.header()
        );
    }
}

#[test]
fn a_stop_past_stop_wait_ms_is_unconfirmed_and_the_loop_starts() {
    let unconfirmed = format!(
        "W5: OMS /api/status 무응답 10초, 마지막 정상 응답 11-15 07:13:10 KST. 킬 마커: 씀. OMS 정지: watcher_kill 실패(no socket) → systemctl stop {UNIT}: 정지 확인 못 함(10000 ms 안에 systemctl이 끝나지 않음){X8}"
    );
    // The kill's own deadline, and the stopper's Unconfirmed: one text.
    for by_stopper in [false, true] {
        let mut r = Run::new(true);
        r.marker(T0 + 100, MarkerOutcome::Written);
        r.halt(T0 + 200, Err("no socket"));
        let t3 = T0 + 200;
        assert_eq!(r.k.next_wake(), Some(t3 + 10_000));
        assert!(r.poll(t3 + 9_999).is_empty());
        let steps = if by_stopper {
            r.stop(t3 + 9_999, Outcome::Unconfirmed)
        } else {
            r.poll(t3 + 10_000)
        };
        assert!(cancel_all(&steps), "{steps:?}");
        assert_eq!(r.k.kill_loop().header(), unconfirmed);
        assert_eq!(r.k.outcome(), StopOutcome::Unconfirmed);
        assert!(r.k.halt_owed());
        // A late outcome of the client is ignored.
        assert!(r.stop(t3 + 10_500, Outcome::Stopped).is_empty());
        assert_eq!(r.k.outcome(), StopOutcome::Unconfirmed);
    }
}

#[test]
fn both_failed_says_oms_stop_failed_in_the_start_notice() {
    let mut r = Run::new(true);
    r.marker(T0 + 100, MarkerOutcome::Written);
    r.halt(T0 + 200, Err("no socket"));
    r.stop(
        T0 + 300,
        Outcome::Failed("exit 4: Access denied".to_string()),
    );
    let header = r.k.kill_loop().header().to_string();
    assert_eq!(
        header,
        format!(
            "W5: OMS /api/status 무응답 10초, 마지막 정상 응답 11-15 07:13:10 KST. 킬 마커: 씀. OMS 정지: watcher_kill 실패(no socket) → systemctl stop {UNIT}: 실패(exit 4: Access denied){X8}"
        )
    );
    let start = notice_text(&r.notices()[0], &header);
    assert!(
        start.starts_with(&format!("워처 킬 시작({header}). ")),
        "{start}"
    );

    // No X8 line when the halt latched or the stop was confirmed.
    let mut r = Run::new(true);
    r.marker(T0 + 100, MarkerOutcome::Written);
    r.halt(T0 + 200, Err("no socket"));
    r.stop(T0 + 300, Outcome::Stopped);
    assert!(
        r.k.kill_loop()
            .header()
            .ends_with(&format!("systemctl stop {UNIT}: 정지됨")),
        "{}",
        r.k.kill_loop().header()
    );
    let mut r = Run::new(true);
    r.marker(T0 + 100, MarkerOutcome::Written);
    r.halt(T0 + 200, Ok("latched"));
    assert!(!r.k.kill_loop().header().contains("X8"));

    // A watcher that never saw a good answer says so.
    let mut s = setup(false);
    s.answered = false;
    let r = Run::with(s);
    assert!(
        r.k.header()
            .starts_with("W5: OMS /api/status 무응답 10초, 마지막 정상 응답 이 프로세스에서 없음. 킬 마커: 진행 중. OMS 정지: 진행 중"),
        "{}",
        r.k.header()
    );
}

#[test]
fn a_kill_without_a_confirmed_stop_owes_the_halt() {
    type Case = (
        bool,
        MarkerOutcome,
        Result<&'static str, &'static str>,
        Option<Outcome>,
        StopOutcome,
        bool,
    );
    let failed = || Some(Outcome::Failed("exit 1".into()));
    let cases: Vec<Case> = vec![
        // halted → not owed
        (
            false,
            MarkerOutcome::Failed("x".into()),
            Ok("latched"),
            None,
            StopOutcome::Latched,
            false,
        ),
        // stopped with a checked marker (written or already there) → not owed
        (
            true,
            MarkerOutcome::Written,
            Err("e"),
            Some(Outcome::Stopped),
            StopOutcome::StoppedMarked,
            false,
        ),
        (
            true,
            MarkerOutcome::AlreadyPresent,
            Err("e"),
            Some(Outcome::Stopped),
            StopOutcome::StoppedMarked,
            false,
        ),
        // stopped with a marker at an unchecked path → owed
        (
            false,
            MarkerOutcome::Written,
            Err("e"),
            Some(Outcome::Stopped),
            StopOutcome::StoppedUncheckedMarker,
            true,
        ),
        (
            false,
            MarkerOutcome::AlreadyPresent,
            Err("e"),
            Some(Outcome::Stopped),
            StopOutcome::StoppedUncheckedMarker,
            true,
        ),
        // stopped without a marker → owed
        (
            true,
            MarkerOutcome::Failed("x".into()),
            Err("e"),
            Some(Outcome::Stopped),
            StopOutcome::StoppedUnmarked,
            true,
        ),
        // neither halted nor stopped → owed
        (
            true,
            MarkerOutcome::Written,
            Err("e"),
            failed(),
            StopOutcome::Unconfirmed,
            true,
        ),
        (
            true,
            MarkerOutcome::Written,
            Err("e"),
            Some(Outcome::Unconfirmed),
            StopOutcome::Unconfirmed,
            true,
        ),
    ];
    for (checked, marker, reply, stop, outcome, owed) in cases {
        let what = format!("{checked} {marker:?} {reply:?} {stop:?}");
        let mut r = Run::new(checked);
        r.marker(T0 + 100, marker);
        r.halt(T0 + 200, reply);
        if let Some(o) = stop {
            r.stop(T0 + 300, o);
        }
        assert_eq!(r.k.outcome(), outcome, "{what}");
        assert_eq!(r.k.halt_owed(), owed, "{what}");
        assert_eq!(r.k.kill_loop().outcome(), outcome, "{what}");
        // The loop runs to its end (the halt retries refused): the final
        // line and Report agree.
        r.drive(T0 + 300, T0 + 120_000, zero, 0, Err("refused"));
        let finals: Vec<StopOutcome> = r
            .notices()
            .iter()
            .filter_map(|n| match n {
                Notice::Final { outcome, .. } => Some(*outcome),
                _ => None,
            })
            .collect();
        assert_eq!(finals, vec![outcome], "{what}");
        let (report, halt_owed) = r.ended().expect("ended");
        assert!(!report.aborted);
        assert_eq!(halt_owed, owed, "{what}");
        assert_eq!(r.k.stage(), Stage::Ended);
    }
}

// ---- The halt retried during the loop ----

/// K2 refused and K3 failed: the loop starts at T0 + 300 with the halt
/// owed; K2 ended at T0 + 200.
fn unhalted() -> Run {
    let mut r = Run::new(true);
    r.marker(T0 + 100, MarkerOutcome::Written);
    r.halt(T0 + 200, Err("no socket"));
    let steps = r.stop(T0 + 300, Outcome::Failed("exit 1".into()));
    assert!(cancel_all(&steps));
    assert!(r.k.halt_owed());
    r
}

#[test]
fn an_oms_that_answers_during_the_loop_is_latched_at_once() {
    // The watcher key is latched (scenario C): every cancel-all is not
    // sent, so the loop cannot end; the retries need no Bybit request.
    let latched = || CancelAnswer::NotSent(NotSent::Latched);
    let mut r = unhalted();
    // 5,000 ms after the failed K2, then 5,000 ms after each failure; an
    // attempt without an answer is never joined by a second one and ends
    // at its deadline (2 × 2,000 + 1,000), the next 5,000 ms after it.
    let now = r.drive_with(T0 + 300, T0 + 25_000, latched, 1, |now, emitted| {
        match emitted - T0 {
            5_200 if now >= T0 + 6_000 => Some(Err("refused")),
            5_200 => None,
            // Never answered: its deadline is T0 + 16,000.
            11_000 => None,
            21_000 if now >= T0 + 21_500 => Some(Ok("latched")),
            _ => None,
        }
    });
    let halts: Vec<u64> = r.halts().iter().map(|t| t - T0).collect();
    assert_eq!(halts, vec![100, 5_200, 11_000, 21_000]);
    assert!(!r.k.halt_owed());
    assert_eq!(r.k.outcome(), StopOutcome::Latched);
    assert_eq!(r.k.kill_loop().outcome(), StopOutcome::Latched);
    let late: Vec<Notice> = r
        .notices()
        .into_iter()
        .filter(|n| matches!(n, Notice::LateHalt { .. }))
        .collect();
    assert_eq!(
        late,
        vec![Notice::LateHalt {
            reply: "latched".into()
        }]
    );
    assert_eq!(
        notice_text(&late[0], ""),
        "늦은 정지: OMS가 다시 답해 watcher_kill로 래치함(latched)"
    );
    // No further halt, however long the loop runs; no cancel-all left.
    let now = r.drive(now, now + 60_000, latched, 1, Err("unexpected"));
    assert_eq!(r.halts().len(), 4);
    assert_eq!(r.k.kill_loop().cancel_all_calls(), 0);
    // Then the key works again and the venue is empty: the final line
    // names the latch, nothing is owed.
    r.drive(now, now + 60_000, zero, 0, Err("unexpected"));
    let finals: Vec<StopOutcome> = r
        .notices()
        .iter()
        .filter_map(|n| match n {
            Notice::Final { outcome, .. } => Some(*outcome),
            _ => None,
        })
        .collect();
    assert_eq!(finals, vec![StopOutcome::Latched]);
    assert_eq!(r.ended().map(|(_, owed)| owed), Some(false));

    // A reply to a closed attempt is ignored while a newer one is open:
    // it neither closes the new attempt nor latches.
    let mut r = unhalted();
    r.drive_with(T0 + 300, T0 + 15_300, latched, 1, |_, _| None);
    let halts: Vec<u64> = r.halts().iter().map(|t| t - T0).collect();
    assert_eq!(halts, vec![100, 5_200, 15_200]);
    let ids: Vec<u64> = r
        .log
        .iter()
        .filter_map(|(_, s)| match s {
            Step::Halt { id, .. } => Some(*id),
            _ => None,
        })
        .collect();
    let open = r.k.halt_open().unwrap();
    assert_eq!(open.0, ids[2]);
    assert!(
        r.k.halt_done(T0 + 15_300, ids[1], Ok("latched".into()))
            .is_empty()
    );
    assert_eq!(r.k.halt_open(), Some(open));
    assert!(r.k.halt_owed());
}

#[test]
fn no_halt_is_retried_when_the_halt_is_not_owed() {
    let one_open = || CancelAnswer::Done(CancelAll { cancelled: 1 });
    // K2 latched.
    let mut r = Run::new(true);
    r.marker(T0 + 100, MarkerOutcome::Written);
    r.halt(T0 + 200, Ok("latched"));
    r.drive(T0 + 200, T0 + 120_000, one_open, 1, Err("unexpected"));
    assert_eq!(r.halts(), vec![T0 + 100]);
    // K3 stopped with a marker at a checked path.
    let mut r = Run::new(true);
    r.marker(T0 + 100, MarkerOutcome::AlreadyPresent);
    r.halt(T0 + 200, Err("no socket"));
    r.stop(T0 + 300, Outcome::Stopped);
    assert!(!r.k.halt_owed());
    r.drive(T0 + 300, T0 + 120_000, one_open, 1, Err("unexpected"));
    assert_eq!(r.halts(), vec![T0 + 100]);
    // The same stop with the path not checked owes it: retried.
    let mut r = Run::new(false);
    r.marker(T0 + 100, MarkerOutcome::AlreadyPresent);
    r.halt(T0 + 200, Err("no socket"));
    r.stop(T0 + 300, Outcome::Stopped);
    r.drive(T0 + 300, T0 + 20_000, one_open, 1, Err("refused"));
    let halts: Vec<u64> = r.halts().iter().map(|t| t - T0).collect();
    assert_eq!(halts, vec![100, 5_200, 10_200, 15_200]);
}

#[test]
fn the_first_retry_after_a_halt_timeout_counts_from_its_deadline() {
    // K2 emitted at T0 + 100 is never answered: it ends at its deadline
    // T0 + 5,100, K3 fails 1,000 ms later and the loop starts owing the
    // halt; the first retry is 5,000 ms after K2's deadline (review 1 S2).
    let one_open = || CancelAnswer::Done(CancelAll { cancelled: 1 });
    let mut r = Run::new(true);
    r.marker(T0 + 100, MarkerOutcome::Written);
    assert!(r.poll(T0 + 100 + K2_MS - 1).is_empty());
    let steps = r.poll(T0 + 100 + K2_MS);
    assert!(
        matches!(steps.as_slice(), [Step::StopUnit { .. }]),
        "{steps:?}"
    );
    let steps = r.stop(T0 + 6_100, Outcome::Failed("exit 1".into()));
    assert!(cancel_all(&steps) && halt_id(&steps).is_none(), "{steps:?}");
    assert!(r.k.halt_owed());
    r.drive(T0 + 6_100, T0 + 16_000, one_open, 1, Err("refused"));
    let halts: Vec<u64> = r.halts().iter().map(|t| t - T0).collect();
    assert_eq!(halts, vec![100, 10_100, 15_100]);
}

#[test]
fn a_retry_in_flight_at_the_loops_end_is_given_up() {
    // The venue is empty at once, so the loop ends 30 s after its start
    // (T0 + 30,300); the retries are refused at once until the one of
    // T0 + 30,200, which is not answered. The loop ends while it is open:
    // it is given up (the runtime aborts its task by `halt_open`), the
    // halt stays owed, a late reply to it is ignored (review 1 S2).
    let mut r = unhalted();
    r.drive_with(T0 + 300, T0 + 600_000, zero, 0, |_, emitted| {
        (emitted < T0 + 30_000).then_some(Err("refused"))
    });
    assert_eq!(r.k.stage(), Stage::Ended);
    let end = r
        .log
        .iter()
        .find_map(|(t, s)| matches!(s, Step::Ended { .. }).then_some(*t))
        .unwrap();
    let (last, id) = r
        .log
        .iter()
        .filter_map(|(t, s)| match s {
            Step::Halt { id, .. } => Some((*t, *id)),
            _ => None,
        })
        .next_back()
        .unwrap();
    // The case's premise: the last attempt was still open at the end.
    assert_eq!((last, end), (T0 + 30_200, T0 + 30_300));
    assert!(end < last + K2_MS);
    assert_eq!(r.k.halt_open(), None);
    assert_eq!(r.ended().map(|(_, owed)| owed), Some(true));
    assert!(
        r.k.halt_done(end + 100, id, Ok("latched".into()))
            .is_empty()
    );
    assert!(r.k.halt_owed());
    assert_eq!(r.k.outcome(), StopOutcome::Unconfirmed);
    assert!(r.k.poll(end + 60_000).is_empty());
}

// ---- Abort in every step ----

fn abort_text(steps: &[Step]) -> String {
    match steps {
        [
            Step::Loop(Action::Notice(n @ Notice::AbortBefore { .. })),
            Step::Ended { report, .. },
        ] => {
            assert!(report.aborted && report.cancel_all_calls == 0, "{report:?}");
            notice_text(n, "")
        }
        _ => panic!("{steps:?}"),
    }
}

#[test]
fn abort_before_the_loop_tells_the_steps_done() {
    let silence = "W5: OMS /api/status 무응답 10초, 마지막 정상 응답 11-15 07:13:10 KST";
    let tail = "로 워처 킬 중단(일괄 취소 전). OMS가 아직 주문을 낼 수 있음";
    // During K1.
    let mut r = Run::new(true);
    let steps = r.k.abort(T0 + 100, AbortWhy::Shutdown);
    let text = abort_text(&steps);
    assert_eq!(
        text,
        format!(
            "{silence}. 킬 마커: 진행 중. OMS 정지: 진행 중.\n워처 종료{tail}\n{NOT_READ_ON_ABORT}"
        )
    );
    assert_eq!(r.k.stage(), Stage::Ended);
    assert!(matches!(
        steps[1],
        Step::Ended {
            halt_owed: true,
            ..
        }
    ));
    // Nothing after it: the late write, the deadlines, a second abort.
    assert!(r.marker(T0 + 200, MarkerOutcome::Written).is_empty());
    assert!(r.poll(T0 + 60_000).is_empty());
    assert!(r.k.abort(T0 + 60_000, AbortWhy::Shutdown).is_empty());

    // During K2, by a start refusal: the open attempt is given up.
    let mut r = Run::new(true);
    r.marker(T0 + 100, MarkerOutcome::Written);
    let (id, _) = r.k.halt_open().unwrap();
    let steps = r.k.abort(
        T0 + 200,
        AbortWhy::StartRefused("the OMS reads its kill marker at none".into()),
    );
    assert_eq!(
        abort_text(&steps),
        format!(
            "{silence}. 킬 마커: 씀. OMS 정지: 진행 중.\n워처 시작 거부: the OMS reads its kill marker at none{tail}\n{NOT_READ_ON_ABORT}"
        )
    );
    assert_eq!(r.k.halt_open(), None);
    assert!(r.k.halt_done(T0 + 300, id, Ok("latched".into())).is_empty());

    // During K3.
    let mut r = Run::new(true);
    r.marker(T0 + 100, MarkerOutcome::Failed("PermissionDenied".into()));
    r.halt(T0 + 200, Err("no socket"));
    let text = abort_text(&r.k.abort(T0 + 300, AbortWhy::Shutdown));
    assert!(
        text.starts_with(&format!(
            "{silence}. 킬 마커: 쓰지 못함(PermissionDenied). OMS 정지: watcher_kill 실패(no socket) → systemctl stop {UNIT}: 진행 중.\n워처 종료{tail}"
        )),
        "{text}"
    );
    assert!(r.stop(T0 + 400, Outcome::Stopped).is_empty());
    assert!(
        r.index(|s| matches!(s, Step::Loop(Action::CancelAll { .. })))
            .is_none()
    );

    // In the loop: the loop's own abort notice, and the debt reported.
    for (reply, owed) in [(Ok("latched"), false), (Err("no socket"), true)] {
        let mut r = Run::new(true);
        r.marker(T0 + 100, MarkerOutcome::Written);
        r.halt(T0 + 200, reply);
        if owed {
            r.stop(T0 + 300, Outcome::Failed("exit 1".into()));
        }
        let steps = r.k.abort(T0 + 400, AbortWhy::Shutdown);
        assert!(
            matches!(
                steps.as_slice(),
                [
                    Step::Loop(Action::Notice(Notice::Abort { .. })),
                    Step::Ended { report, halt_owed }
                ] if report.aborted && *halt_owed == owed
            ),
            "{steps:?}"
        );
        assert!(r.poll(T0 + 2_000).is_empty());
    }
}

// ---- The header's bound ----

/// 20 positions of a plausible shape (a 12-character symbol, the
/// venue's usual decimals).
fn rows(n: usize) -> Vec<Value> {
    (0..n)
        .map(|i| {
            json!({
                "symbol": format!("{i:02}00PEPEUSDT"), "side": "Sell",
                "size": "12345678.9", "unrealisedPnl": "-12345.6789",
                "markPrice": "0.0123456", "liqPrice": "0.0150001",
            })
        })
        .collect()
}

/// The longest rows the block can show: a 30-character symbol and eight
/// decimals in every number.
fn longest_rows(n: usize) -> Vec<Value> {
    (0..n)
        .map(|i| {
            json!({
                "symbol": format!("{i:02}{}USDT", "X".repeat(24)), "side": "Sell",
                "size": "12345.12345678", "unrealisedPnl": "-123456.12345678",
                "markPrice": "123456.12345678", "liqPrice": "100000.5",
            })
        })
        .collect()
}

#[test]
fn a_start_notice_with_the_longest_header_fits_one_part() {
    // The config's longest name (64 characters of 2 UTF-16 units) and
    // unit (255 characters); every free text far over its cut, with
    // control characters in it, of 1-unit characters and of characters
    // outside the BMP (2 units each; review 1 M1).
    // The silence's seconds text adds one unit per character: `10` at
    // the tests' 10 s, milliseconds below 60 s and below 1,000 s, and the
    // longest a `u64` gives (review 2 S1).
    let name = "𝕏".repeat(64);
    let unit = format!("{}.service", "u".repeat(247));
    let silences = [
        (10_000, "10"),
        (59_999, "59.999"),
        (999_999, "999.999"),
        (u64::MAX, "18446744073709551.615"),
    ];
    for ((silent_ms, secs), c) in silences
        .into_iter()
        .flat_map(|s| ['가', '𝕏'].map(|c| (s, c)))
    {
        let mut s = setup(true);
        s.name = name.clone();
        s.kill.oms_unit = unit.clone();
        let fire = Fire {
            last_good_ms: T0.saturating_sub(silent_ms),
            silent_ms,
        };
        let mut r = Run::fired(fire, s);
        let noise = |n: usize| format!("a\n\u{7}b\r{}", c.to_string().repeat(n));
        r.marker(T0 + 100, MarkerOutcome::Failed(noise(1000)));
        // A 64 KiB reply that is no latch, and 4 KiB of systemctl's stderr.
        r.halt(T0 + 200, Ok(&*noise(64 * 1024).leak()));
        r.stop(
            T0 + 300,
            Outcome::Failed(format!("exit 1: {}", noise(4096))),
        );
        let header = r.k.kill_loop().header().to_string();
        assert!(!header.chars().any(char::is_control), "{header}");
        assert!(header.contains(&format!("systemctl stop {unit}: 실패(")));
        // Each free text is cut to 200 UTF-16 units, at a whole character
        // (the prefix `a  b ` is 5 units, `exit 1: a  b ` 13).
        let fill = |prefix: usize| c.to_string().repeat((200 - prefix) / c.len_utf16());
        for cut in [
            format!("쓰지 못함(a  b {}).", fill(5)),
            format!("watcher_kill 실패(a  b {}) →", fill(5)),
            format!("실패(exit 1: a  b {}).", fill(13)),
        ] {
            assert!(header.contains(&cut), "{cut}: {header}");
        }
        assert!(header.starts_with(&format!("W5: OMS /api/status 무응답 {secs}초, ")));
        let units = header.encode_utf16().count();
        // 1,029 units at `10`, one more per further character; 1,048 at
        // the longest.
        let most = 1027 + secs.len();
        assert!(
            units <= most && units <= 1048,
            "the header is {units} units"
        );
        if c == '가' {
            assert_eq!(units, most);
        }
        eprintln!("header ({c}, {secs} s): {units} UTF-16 units");

        let start = notice_text(&r.notices()[0], &header);
        let body = format!(
            "{start}\n{}",
            position_block(&Positions::Rows(rows(20)), "11-15 07:13:20 KST")
        );
        let msg = message(&name, T0, &body);
        let units = msg.encode_utf16().count();
        eprintln!("start notice ({c}, {secs} s) with 20 plausible rows: {units} UTF-16 units");
        assert!(units <= MSG_LIMIT, "{units} units");
        assert_eq!(split(&msg, MSG_LIMIT).len(), 1);

        // Known limit (C-WATCH part 12): with this header and 20 rows of
        // the longest shape the block can show, the start notice is cut
        // into two parts, never more.
        let body = format!(
            "{start}\n{}",
            position_block(&Positions::Rows(longest_rows(20)), "11-15 07:13:20 KST")
        );
        let msg = message(&name, T0, &body);
        eprintln!(
            "start notice ({c}, {secs} s) with 20 longest rows: {} UTF-16 units",
            msg.encode_utf16().count()
        );
        assert_eq!(split(&msg, MSG_LIMIT).len(), 2);
    }
}

// ---- A clock that steps back ----

#[test]
fn a_clock_step_back_does_not_hold_the_loop() {
    // The kill reads Steady over a wall clock that stepped back 60 s
    // before the fire (so Steady is 60 s ahead of it) and steps back 60 s
    // again during the loop with an order open; the texts read the raw
    // clock (review 1 S1).
    let raw = std::sync::Arc::new(ManualClock::new(T0 + 60_000));
    let steady = Steady::new(raw.clone());
    assert_eq!(steady.now_ms(), T0 + 60_000);
    raw.set(T0);
    assert_eq!(steady.now_ms(), T0 + 60_000);
    let mut s = setup(true);
    s.raw_now_ms = raw.now_ms();
    let (k, steps) = Kill::new(FIRE, s, steady.now_ms());
    let mut r = Run {
        k,
        log: Vec::new(),
        pending: Vec::new(),
    };
    r.keep(T0, steps);
    match &r.log[0].1 {
        Step::WriteMarker { text } => assert!(
            text.contains(
                "W5 2023-11-14T22:13:20.000Z: /api/status gave no good answer for 10 s (last good answer 2023-11-14T22:13:10.000Z);"
            ),
            "{text}"
        ),
        s => panic!("{s:?}"),
    }
    raw.advance(100);
    r.marker(steady.now_ms(), MarkerOutcome::Written);
    raw.advance(100);
    r.halt(steady.now_ms(), Err("no socket"));
    raw.advance(100);
    r.stop(steady.now_ms(), Outcome::Failed("exit 1".into()));
    assert!(
        r.k.kill_loop()
            .header()
            .contains("마지막 정상 응답 11-15 07:13:10 KST"),
        "{}",
        r.k.kill_loop().header()
    );

    // The test's own count of real progress: 100 ms per step. The clock
    // steps back 60 s at step 250 (25 s of progress).
    let one_open = || CancelAnswer::Done(CancelAll { cancelled: 1 });
    let mut progress = 300u64;
    let mut cancels = Vec::new();
    let mut halts = Vec::new();
    let mut marks = Vec::new();
    for step in 0..1000 {
        if step == 250 {
            raw.set(raw.now_ms() - 60_000);
        } else {
            raw.advance(100);
            progress += 100;
        }
        let now = steady.now_ms();
        let before = r.log.len();
        r.poll(now);
        while let Some(st) = r.pending.pop() {
            match st {
                Step::Loop(Action::CancelAll { req_id }) => r.arrived(
                    now,
                    Arrived::Cancel {
                        req_id,
                        answer: one_open(),
                    },
                ),
                Step::Loop(Action::Count { req_id }) => r.arrived(
                    now,
                    Arrived::Counted {
                        req_id,
                        result: Ok(1),
                    },
                ),
                _ => Vec::new(),
            };
        }
        if r.k.halt_open().is_some() {
            r.halt(now, Err("refused"));
        }
        for (_, st) in r.log[before..].iter() {
            match st {
                Step::Loop(Action::CancelAll { .. }) => cancels.push(progress),
                Step::Halt { .. } => halts.push(progress),
                Step::Loop(Action::Notice(Notice::Progress { .. })) => marks.push(progress),
                _ => {}
            }
        }
    }
    // Every cancel-all round_ms of progress after the previous one, the
    // halt retries every 5,000 ms, the progress marks every notice_ms,
    // across the step back.
    assert!(cancels.len() > 90, "{cancels:?}");
    assert!(
        cancels.windows(2).all(|w| w[1] - w[0] == 1000),
        "{cancels:?}"
    );
    assert!(
        halts.len() > 15 && halts.windows(2).all(|w| w[1] - w[0] == 5000),
        "{halts:?}"
    );
    assert_eq!(marks, vec![60_300]);
}
