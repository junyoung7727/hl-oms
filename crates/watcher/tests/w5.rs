//! W5, the OMS silence check (C-WATCH part 12, U19b3a), sans-IO: a probe of
//! `/api/status` every `probe_ms`, never two in flight, a lost probe failed
//! at emission + `request_timeout_ms` + 1,000 ms; `Fire` once when
//! `silence_ms` passed without a good answer and a probe emitted at or
//! after the last good answer failed; the gap rule (unobserved time is not
//! silence); after a kill one good answer before W5 watches again; the
//! owed halt. The clock is walked by hand in polls of 100 ms or less,
//! except where a test makes a gap.

#![allow(clippy::unwrap_used)]

use std::sync::{Arc, Mutex};

use http_fake::{Answer, FakeHttp};
use watcher::clock::{Clock, ManualClock, Steady};
use watcher::oms::Status;
use watcher::w5::{Fire, OwedHalt, State, Timing, W5, good_answer};

const T0: u64 = 1_800_000_000_000;

/// The defaults: 1 s probe, 10 s silence, 2 s request timeout.
const DEFAULT: Timing = Timing {
    probe_ms: 1000,
    silence_ms: 10_000,
    request_timeout_ms: 2000,
};

/// What the fake OMS does with the probe emitted at an instant: answer
/// after a delay, good or not, or never.
type Oms = dyn Fn(u64) -> Option<(u64, bool)>;

/// W5 with the runtime's order in one turn: the answers that arrived
/// (`on_answer`), then `poll`, then `fire`.
struct Sim {
    w: W5,
    lines: Arc<Mutex<Vec<String>>>,
    /// `(arrives at, req_id, good)`.
    pending: Vec<(u64, u64, bool)>,
    /// `(emitted at, req_id)`.
    probes: Vec<(u64, u64)>,
    fires: Vec<(u64, Fire)>,
    owed: Vec<u64>,
}

impl Sim {
    fn new(t: Timing) -> Sim {
        let (log, lines) = watcher::log::collect();
        Sim {
            w: W5::new(Some(t), log),
            lines,
            pending: Vec::new(),
            probes: Vec::new(),
            fires: Vec::new(),
            owed: Vec::new(),
        }
    }

    /// Armed at `T0` with one good answer at `T0` (the probe emitted then).
    fn armed_good_at_t0(t: Timing) -> Sim {
        let mut s = Sim::new(t);
        s.w.arm(T0);
        let p = s.w.poll(T0).unwrap();
        s.probes.push((T0, p.req_id));
        assert_eq!(s.w.on_answer(T0, p.req_id, true), None);
        assert_eq!(s.w.state(), State::Armed { last_good_ms: T0 });
        s
    }

    /// One turn at `now`.
    fn at(&mut self, now: u64, oms: &Oms) {
        self.pending.sort();
        let due: Vec<_> = self
            .pending
            .iter()
            .filter(|p| p.0 <= now)
            .copied()
            .collect();
        self.pending.retain(|p| p.0 > now);
        for (_, id, good) in due {
            if let Some(OwedHalt) = self.w.on_answer(now, id, good) {
                self.owed.push(now);
            }
        }
        if let Some(p) = self.w.poll(now) {
            self.probes.push((now, p.req_id));
            if let Some((delay, good)) = oms(now) {
                self.pending.push((now + delay, p.req_id, good));
            }
        }
        if let Some(f) = self.w.fire(now) {
            self.fires.push((now, f));
        }
    }

    /// Turns at `from`, `from + step`, … up to `to`, and at every instant
    /// an answer arrives in between.
    fn walk(&mut self, from: u64, to: u64, step: u64, oms: &Oms) {
        let mut now = from;
        let mut grid = from;
        let mut n = 0;
        while now <= to {
            n += 1;
            assert!(n < 1_000_000, "the walk has a bound");
            self.at(now, oms);
            while grid <= now {
                grid += step;
            }
            now = self
                .pending
                .iter()
                .map(|p| p.0)
                .filter(|a| *a > now && *a < grid)
                .min()
                .unwrap_or(grid);
        }
    }

    fn emitted(&self) -> Vec<u64> {
        self.probes.iter().map(|p| p.0 - T0).collect()
    }

    fn fired_at(&self) -> Vec<u64> {
        self.fires.iter().map(|f| f.0 - T0).collect()
    }
}

fn silent(_: u64) -> Option<(u64, bool)> {
    None
}

#[test]
fn w5_is_not_armed_before_arm_and_off_without_kill() {
    // Without [kill]: Off, whatever happens.
    let (log, _) = watcher::log::collect();
    let mut off = W5::new(None, log);
    assert_eq!(off.state(), State::Off);
    off.arm(T0);
    for k in 0..1000 {
        let now = T0 + k * 100;
        assert_eq!(off.poll(now), None);
        assert_eq!(off.fire(now), None);
    }
    assert_eq!(off.state(), State::Off);
    assert!(!off.armed());
    assert_eq!(off.silent_ms(T0 + 100_000), None);
    off.kill_ended(true);
    assert_eq!(off.state(), State::Off);
    assert!(!off.halt_owed());

    // With [kill] but before `arm` (the start checks have not ended): no
    // probe and no fire, however long the OMS is silent.
    let mut s = Sim::new(DEFAULT);
    assert_eq!(s.w.state(), State::Unarmed);
    s.walk(T0, T0 + 60_000, 100, &silent);
    assert!(s.probes.is_empty() && s.fires.is_empty());
    assert!(!s.w.armed());
    // `arm` starts the watch at its instant: a probe at once, and a fire
    // 10 s later even though the OMS never answered in this process
    // (owner question 1's default).
    let a = T0 + 60_000;
    s.w.arm(a);
    assert_eq!(s.w.state(), State::Armed { last_good_ms: a });
    assert!(s.w.armed());
    s.walk(a, a + 20_000, 100, &silent);
    assert_eq!(s.probes[0].0, a);
    assert_eq!(s.fired_at(), vec![70_000]);
    assert_eq!(
        s.fires[0].1,
        Fire {
            last_good_ms: a,
            silent_ms: 10_000
        }
    );
    // A second arm does nothing (it arms an Unarmed W5 only).
    s.w.arm(a + 30_000);
    assert_eq!(s.w.state(), State::Killing);
}

#[test]
fn probes_are_one_per_probe_ms_and_never_two_in_flight() {
    // The OMS answers each probe after 50 ms: one probe per probe_ms from
    // the previous probe's emission.
    let mut s = Sim::new(DEFAULT);
    s.w.arm(T0);
    let quick = |_: u64| Some((50, true));
    s.walk(T0, T0 + 5000, 100, &quick);
    assert_eq!(s.emitted(), vec![0, 1000, 2000, 3000, 4000, 5000]);
    // Every answer is good: silence never passes 1,050 ms.
    assert!(s.fires.is_empty());
    assert_eq!(s.w.silent_ms(T0 + 5000), Some(950));

    // An answer after 2,500 ms (slower than probe_ms, inside the deadline
    // of 3,000 ms): the next probe goes when the open one is answered, so
    // two are never in flight.
    let mut s = Sim::new(DEFAULT);
    s.w.arm(T0);
    let slow = |_: u64| Some((2500, true));
    s.walk(T0, T0 + 10_000, 100, &slow);
    assert_eq!(s.emitted(), vec![0, 2500, 5000, 7500, 10_000]);
    // While one is open, a poll at its due instant emits nothing.
    assert!(s.w.probe_in_flight().is_some());
    assert_eq!(s.w.poll(T0 + 11_000), None);

    // An answer to a probe that is not in flight is ignored: an unknown
    // id, and the same id twice.
    let mut s = Sim::new(DEFAULT);
    s.w.arm(T0);
    let p = s.w.poll(T0).unwrap();
    assert_eq!(s.w.on_answer(T0 + 10, p.req_id + 7, true), None);
    assert_eq!(s.w.state(), State::Armed { last_good_ms: T0 });
    assert_eq!(s.w.probe_in_flight(), Some(p.req_id));
    assert_eq!(s.w.on_answer(T0 + 20, p.req_id, true), None);
    assert_eq!(
        s.w.state(),
        State::Armed {
            last_good_ms: T0 + 20
        }
    );
    assert_eq!(s.w.on_answer(T0 + 30, p.req_id, true), None);
    assert_eq!(
        s.w.state(),
        State::Armed {
            last_good_ms: T0 + 20
        }
    );
}

#[test]
fn a_lost_probe_is_failed_at_its_deadline() {
    // The deadline is emission + request_timeout_ms + 1,000 ms = 3,000 ms.
    let mut s = Sim::new(DEFAULT);
    s.w.arm(T0);
    s.at(T0, &silent);
    let first = s.probes[0].1;
    s.walk(T0 + 100, T0 + 2900, 100, &silent);
    s.at(T0 + 2999, &silent);
    assert_eq!(s.w.probe_in_flight(), Some(first), "open 1 ms before");
    s.at(T0 + 3000, &silent);
    // Failed at its deadline, and the next probe goes in the same turn.
    assert_ne!(s.w.probe_in_flight(), Some(first));
    assert_eq!(s.emitted(), vec![0, 3000]);
    // With no answer at all: one probe per deadline.
    s.walk(T0 + 3100, T0 + 9000, 100, &silent);
    assert_eq!(s.emitted(), vec![0, 3000, 6000, 9000]);
    // A late good answer to a failed probe is ignored.
    assert_eq!(s.w.on_answer(T0 + 9050, first, true), None);
    assert_eq!(s.w.state(), State::Armed { last_good_ms: T0 });

    // A probe answer is taken when it arrives (the truth about the OMS
    // first), also at the deadline when no poll has applied it yet.
    let mut s = Sim::new(DEFAULT);
    s.w.arm(T0);
    let p = s.w.poll(T0).unwrap();
    assert_eq!(s.w.on_answer(T0 + 3000, p.req_id, true), None);
    assert_eq!(
        s.w.state(),
        State::Armed {
            last_good_ms: T0 + 3000
        }
    );

    // The deadline follows request_timeout_ms (a long silence_ms, so that
    // no fire ends the probes).
    let mut s = Sim::new(Timing {
        probe_ms: 500,
        silence_ms: 60_000,
        request_timeout_ms: 1500,
    });
    s.w.arm(T0);
    s.walk(T0, T0 + 5000, 100, &silent);
    assert_eq!(s.emitted(), vec![0, 2500, 5000]);
}

#[test]
fn ten_seconds_without_a_good_status_answer_fire_once() {
    // The last good answer at T0, then silence: no fire at +9,999 ms, a
    // fire at +10,000 ms, once (the polls 100 ms apart: no gap).
    let mut s = Sim::armed_good_at_t0(DEFAULT);
    s.walk(T0 + 100, T0 + 9900, 100, &silent);
    s.at(T0 + 9999, &silent);
    assert!(s.fires.is_empty(), "no fire at +9,999 ms");
    assert_eq!(s.w.silent_ms(T0 + 9999), Some(9999));
    s.at(T0 + 10_000, &silent);
    assert_eq!(s.fired_at(), vec![10_000]);
    assert_eq!(
        s.fires[0].1,
        Fire {
            last_good_ms: T0,
            silent_ms: 10_000
        }
    );
    assert_eq!(s.w.state(), State::Killing);
    assert!(!s.w.armed());
    // Once: no second fire and no probe while the kill runs.
    let probes = s.probes.len();
    s.walk(T0 + 10_100, T0 + 120_000, 100, &silent);
    assert_eq!(s.fired_at(), vec![10_000]);
    assert_eq!(s.probes.len(), probes);

    // A good answer at +9,000 ms moves it: the fire comes 10 s later.
    let mut s = Sim::armed_good_at_t0(DEFAULT);
    // The probe emitted at +7,000 ms is answered at +9,000 ms.
    let late = |e: u64| (e == T0 + 7000).then_some((2000, true));
    s.walk(T0 + 100, T0 + 18_900, 100, &late);
    s.at(T0 + 18_999, &late);
    assert!(s.fires.is_empty(), "no fire at +18,999 ms");
    assert_eq!(s.w.silent_ms(T0 + 18_999), Some(9999));
    s.at(T0 + 19_000, &late);
    assert_eq!(s.fired_at(), vec![19_000]);
    assert_eq!(s.fires[0].1.last_good_ms, T0 + 9000);
}

#[test]
fn w5_fires_only_after_a_failed_probe() {
    // Timings outside the config bound (W5 takes them as given), so that
    // the silence passes while the one probe in flight is still open: a
    // deadline of 9,000 ms against 3,000 ms of silence.
    let t = Timing {
        probe_ms: 1000,
        silence_ms: 3000,
        request_timeout_ms: 8000,
    };
    let mut s = Sim::armed_good_at_t0(t);
    s.walk(T0 + 100, T0 + 9900, 100, &silent);
    s.at(T0 + 9999, &silent);
    assert!(s.fires.is_empty(), "silence passed, no probe failed yet");
    assert_eq!(s.emitted(), vec![0, 1000]);
    // The probe emitted at +1,000 ms fails at its deadline: fire.
    s.at(T0 + 10_000, &silent);
    assert_eq!(s.fired_at(), vec![10_000]);
    assert_eq!(s.fires[0].1.silent_ms, 10_000);

    // An answer that is not good fails the probe at once.
    let mut s = Sim::armed_good_at_t0(t);
    let bad = |e: u64| (e == T0 + 1000).then_some((500, false));
    s.walk(T0 + 100, T0 + 2900, 100, &bad);
    assert!(s.fires.is_empty());
    s.at(T0 + 2999, &bad);
    assert!(s.fires.is_empty());
    s.at(T0 + 3000, &bad);
    assert_eq!(s.fired_at(), vec![3000]);

    // A probe emitted in the turn of the good answer (emission equal to
    // the last good instant) that fails counts (review 3 N1).
    let mut s = Sim::new(t);
    s.w.arm(T0);
    s.at(T0, &|_| Some((1000, true)));
    // At +1,000 ms the answer arrives, then the next probe is emitted in
    // the same turn; the OMS answers no more.
    s.at(T0 + 1000, &silent);
    assert_eq!(
        s.w.state(),
        State::Armed {
            last_good_ms: T0 + 1000
        }
    );
    assert_eq!(s.emitted(), vec![0, 1000]);
    s.walk(T0 + 1100, T0 + 9900, 100, &silent);
    assert!(s.fires.is_empty());
    s.at(T0 + 10_000, &silent);
    assert_eq!(s.fired_at(), vec![10_000]);
    assert_eq!(
        s.fires[0].1,
        Fire {
            last_good_ms: T0 + 1000,
            silent_ms: 9000
        }
    );

    // A probe emitted before the last good answer does not count: the
    // OMS answered the probe of +1,000 ms at +1,500 ms, so the probe that
    // fails afterwards must be one emitted at or after +1,500 ms.
    let mut s = Sim::new(t);
    s.w.arm(T0);
    let p = s.w.poll(T0).unwrap();
    // The first probe gets a bad answer at +100 ms (failed, emitted T0).
    assert_eq!(s.w.on_answer(T0 + 100, p.req_id, false), None);
    let q = s.w.poll(T0 + 1000).unwrap();
    assert_eq!(s.w.on_answer(T0 + 1500, q.req_id, true), None);
    // The next probe (emitted at +2,000 ms) stays open until +11,000 ms.
    s.walk(T0 + 2000, T0 + 10_900, 100, &silent);
    assert!(s.fires.is_empty(), "the failed probe of T0 is older");
    s.at(T0 + 11_000, &silent);
    assert_eq!(s.fired_at(), vec![11_000]);
}

#[test]
fn one_lost_probe_and_a_slow_answer_at_the_silence_bound_do_not_fire() {
    // probe_ms 500, request_timeout_ms 1,500, silence_ms at its lower
    // bound 500 + 2 × 1,500 + 1,000 = 4,500 (review 1 S6). The last good
    // answer at T0; the next probe (+500 ms) is lost and fails at
    // +3,000 ms; the one after (+3,000 ms) answers after 1,499 ms.
    let t = Timing {
        probe_ms: 500,
        silence_ms: 4500,
        request_timeout_ms: 1500,
    };
    let mut s = Sim::armed_good_at_t0(t);
    let oms = |e: u64| {
        if e == T0 + 500 {
            None
        } else if e == T0 + 3000 {
            Some((1499, true))
        } else {
            Some((10, true))
        }
    };
    s.walk(T0 + 1, T0 + 20_000, 1, &oms);
    assert!(s.fires.is_empty(), "{:?}", s.fires);
    assert_eq!(&s.emitted()[..3], &[0, 500, 3000]);
    // The good answer at +4,499 ms came 1 ms before the bound.
    assert!(s.w.silent_ms(T0 + 20_000).unwrap() < 500);
}

#[test]
fn a_gap_in_the_watchers_own_ticks_is_not_oms_silence() {
    // The OMS answers every probe after 50 ms, the watcher polls every
    // 100 ms; then one poll 15,000 ms later (the watcher's process was
    // stopped): the gap threshold is 2 × 1,000 + 2,000 = 4,000 ms.
    let mut s = Sim::new(DEFAULT);
    s.w.arm(T0);
    let quick = |_: u64| Some((50, true));
    s.walk(T0, T0 + 4000, 100, &quick);
    // At +4,000 ms the probe of +4,000 ms is in flight; its answer would
    // come at +4,050 ms, but the next poll is at +19,000 ms.
    let p1 = T0 + 4000;
    assert!(s.w.probe_in_flight().is_some());
    assert_eq!(
        s.w.state(),
        State::Armed {
            last_good_ms: T0 + 3050
        }
    );
    s.pending.clear();
    let gap_end = p1 + 15_000;
    s.at(gap_end, &silent);
    assert!(s.fires.is_empty(), "a gap is not silence");
    // last_good_ms and the probe in flight moved forward by the gap.
    assert_eq!(
        s.w.state(),
        State::Armed {
            last_good_ms: T0 + 3050 + 15_000
        }
    );
    assert!(s.w.probe_in_flight().is_some(), "its deadline moved too");
    {
        let lines = s.lines.lock().unwrap();
        assert_eq!(
            lines.as_slice(),
            &["warn: W5: 15000 ms between two polls (more than 4000 ms): the watcher did not observe that time, so it is not OMS silence".to_string()]
        );
    }

    // The OMS answers again: no fire; then a forward clock step of
    // 15,000 ms between two 100 ms ticks is the same gap.
    s.walk(gap_end + 100, gap_end + 5000, 100, &quick);
    assert!(s.fires.is_empty());
    let before = s.w.silent_ms(gap_end + 5000).unwrap();
    s.pending.clear();
    let step_end = gap_end + 5000 + 15_000;
    s.at(step_end, &silent);
    assert!(s.fires.is_empty(), "a clock step forward is not silence");
    assert_eq!(s.w.silent_ms(step_end), Some(before));
    assert_eq!(s.lines.lock().unwrap().len(), 2);

    // Real silence after a gap is counted from the gap's end: with the
    // last good answer at the last poll before the gap, W5 fires
    // silence_ms after the gap's end.
    let mut s = Sim::new(DEFAULT);
    s.w.arm(T0);
    s.walk(T0, T0 + 900, 100, &|_| Some((1000, true)));
    // At +1,000 ms the answer to the probe of T0 arrives and the next
    // probe is emitted in the same turn; then nothing for 15 s.
    s.at(T0 + 1000, &silent);
    assert_eq!(
        s.w.state(),
        State::Armed {
            last_good_ms: T0 + 1000
        }
    );
    let g = T0 + 1000 + 15_000;
    s.at(g, &silent);
    assert!(s.fires.is_empty());
    s.walk(g + 100, g + 9900, 100, &silent);
    assert!(
        s.fires.is_empty(),
        "no fire before silence_ms after the gap"
    );
    s.at(g + 10_000, &silent);
    assert_eq!(
        s.fires.iter().map(|f| f.0).collect::<Vec<_>>(),
        vec![g + 10_000]
    );
    assert_eq!(s.fires[0].1.silent_ms, 10_000);

    // A gap exactly at the threshold is not a gap.
    let mut s = Sim::armed_good_at_t0(DEFAULT);
    s.at(T0 + 4000, &silent);
    assert_eq!(s.w.state(), State::Armed { last_good_ms: T0 });
    assert!(s.lines.lock().unwrap().is_empty());
}

#[test]
fn a_clock_step_back_does_not_blind_w5() {
    // W5 polled through Steady over a manual clock that steps back
    // 60,000 ms after a good answer, then the OMS is silent: Fire after
    // silence_ms of the inner clock's progress, not 70 s.
    let inner = Arc::new(ManualClock::new(T0));
    let steady = Steady::new(inner.clone());
    let mut s = Sim::armed_good_at_t0(DEFAULT);
    assert_eq!(steady.now_ms(), T0);
    // The step back, seen at a tick (W5 reads the clock at every tick).
    inner.set(T0 - 60_000);
    s.at(steady.now_ms(), &silent);
    let mut progress = 0;
    while s.fires.is_empty() {
        assert!(progress <= 80_000, "no fire within 80 s of progress");
        inner.advance(100);
        progress += 100;
        s.at(steady.now_ms(), &silent);
    }
    assert_eq!(progress, 10_000);
    assert_eq!(s.fires[0].1.silent_ms, 10_000);
    // The raw clock is 50 s behind the instant W5 fired at.
    assert_eq!(inner.now_ms(), T0 - 50_000);
    assert!(s.lines.lock().unwrap().is_empty(), "a step back is no gap");
}

#[test]
fn after_a_kill_w5_waits_for_a_good_answer_before_it_watches_again() {
    let mut s = Sim::armed_good_at_t0(DEFAULT);
    s.walk(T0 + 100, T0 + 10_000, 100, &silent);
    assert_eq!(s.fired_at(), vec![10_000]);
    // The kill runs 60 s: no probe meanwhile.
    let probes = s.probes.len();
    s.walk(T0 + 10_100, T0 + 70_000, 100, &silent);
    assert_eq!(s.probes.len(), probes);
    // The kill ended with the OMS stopped and nothing owed: Rearming.
    s.w.kill_ended(false);
    assert_eq!(s.w.state(), State::Rearming { halt_owed: false });
    assert!(!s.w.armed() && !s.w.halt_owed());
    assert_eq!(s.w.silent_ms(T0 + 70_000), None);
    // The OMS stays down for 2 minutes: probes go on, no new kill.
    s.walk(T0 + 70_100, T0 + 190_000, 100, &silent);
    assert!(s.probes.len() > probes);
    assert_eq!(s.fired_at(), vec![10_000], "no kill of a stopped OMS");
    // The OMS answers: W5 watches again from that answer.
    let back = T0 + 190_100;
    s.at(back, &silent);
    let open = s.w.probe_in_flight().unwrap();
    assert_eq!(s.w.on_answer(back + 50, open, true), None);
    assert_eq!(
        s.w.state(),
        State::Armed {
            last_good_ms: back + 50
        }
    );
    // And kills again after a new silence, at the first tick at or after
    // back + 50 + 10,000 ms.
    s.walk(back + 100, back + 20_000, 100, &silent);
    assert_eq!(s.fired_at(), vec![10_000, 200_200]);
    assert_eq!(s.fires[1].1.last_good_ms, back + 50);
    // `kill_ended` acts only on a running kill.
    s.w.kill_ended(true);
    s.w.kill_ended(false);
    assert_eq!(s.w.state(), State::Rearming { halt_owed: true });
}

#[test]
fn an_owed_halt_is_sent_at_the_first_good_answer() {
    let fired = || {
        let mut s = Sim::armed_good_at_t0(DEFAULT);
        s.walk(T0 + 100, T0 + 10_000, 100, &silent);
        assert_eq!(s.fired_at(), vec![10_000]);
        s.w.kill_ended(true);
        assert_eq!(s.w.state(), State::Rearming { halt_owed: true });
        assert!(s.w.halt_owed());
        s
    };
    // The OMS stays silent: nothing owed is sent, no new kill.
    let mut s = fired();
    s.walk(T0 + 10_100, T0 + 60_000, 100, &silent);
    assert!(s.owed.is_empty());
    assert_eq!(s.fired_at(), vec![10_000]);

    // The OMS answers: the first good answer gives OwedHalt, not Armed.
    let back = |e: u64| (e >= T0 + 60_000).then_some((50, true));
    s.walk(T0 + 60_100, T0 + 63_000, 100, &back);
    assert_eq!(s.owed, vec![T0 + 61_050], "one, never two in flight");
    assert_eq!(s.w.state(), State::Rearming { halt_owed: true });
    // The halt latched: one 늦은 정지 P1, and W5 watches again.
    let p1 = s.w.owed_halt_done(T0 + 63_000, Ok("latched".into()));
    assert_eq!(
        p1.as_deref(),
        Some("늦은 정지: OMS가 다시 답해 watcher_kill로 래치함(latched)")
    );
    assert_eq!(
        s.w.state(),
        State::Armed {
            last_good_ms: T0 + 63_000
        }
    );
    assert!(!s.w.halt_owed());
    // A second outcome is ignored.
    assert_eq!(s.w.owed_halt_done(T0 + 63_100, Ok("latched".into())), None);

    // A failure: one 늦은 정지 실패 P1, still owed, retried at the next
    // good answer without a second P1; then the rearm reply latches.
    let mut s = fired();
    let back = |e: u64| (e >= T0 + 19_000).then_some((50, true));
    s.walk(T0 + 10_100, T0 + 21_000, 100, &back);
    assert_eq!(s.owed.len(), 1);
    let p1 = s.w.owed_halt_done(
        T0 + 21_000,
        Err("not sent: /run/hl-oms/admin.sock: Connection refused (os error 111)".into()),
    );
    assert_eq!(
        p1.as_deref(),
        Some(
            "늦은 정지 실패: OMS가 답하지만 watcher_kill이 안 됨(not sent: /run/hl-oms/admin.sock: Connection refused (os error 111)). OMS가 래치 없이 주문을 낼 수 있음"
        )
    );
    assert_eq!(s.w.state(), State::Rearming { halt_owed: true });
    // Not latched while owed: no silence is judged (no new kill).
    s.walk(T0 + 21_100, T0 + 22_500, 100, &back);
    assert_eq!(s.owed.len(), 2);
    // A reply that does not latch is a failure too: no second P1.
    assert_eq!(s.w.owed_halt_done(T0 + 22_500, Ok("held".into())), None);
    assert_eq!(s.w.state(), State::Rearming { halt_owed: true });
    s.walk(T0 + 22_600, T0 + 24_000, 100, &back);
    assert_eq!(s.owed.len(), 3);
    let p1 = s.w.owed_halt_done(
        T0 + 24_000,
        Ok("already latched; the earlier confirmation is void and a new kill loop starts".into()),
    );
    assert_eq!(
        p1.as_deref(),
        Some(
            "늦은 정지: OMS가 다시 답해 watcher_kill로 래치함(already latched; the earlier confirmation is void and a new kill loop starts)\n해소: 늦은 정지 실패: OMS가 답하지만 watcher_kill이 안 됨(not sent: /run/hl-oms/admin.sock: Connection refused (os error 111)). OMS가 래치 없이 주문을 낼 수 있음"
        )
    );
    assert_eq!(
        s.w.state(),
        State::Armed {
            last_good_ms: T0 + 24_000
        }
    );
    assert_eq!(s.fired_at(), vec![10_000]);

    // The texts are cut to 200 characters on one line.
    let mut s = fired();
    s.walk(T0 + 10_100, T0 + 21_000, 100, &back);
    let long = format!("a\nb{}", "x".repeat(400));
    let p1 = s.w.owed_halt_done(T0 + 21_000, Ok(long)).unwrap();
    assert!(!p1.contains('\n'), "{p1}");
    assert!(p1.contains(&format!("(a b{}).", "x".repeat(197))), "{p1}");
}

/// Builds a fake `/api/status` and asks it once.
fn status_of(body: Answer) -> Result<serde_json::Value, String> {
    let fake = FakeHttp::start(body);
    let url = format!("http://{}/api/status", fake.addr());
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let s = Status::new(&url, 2000).unwrap();
    rt.block_on(s.get())
}

#[test]
fn only_a_2xx_status_with_a_16_hex_instance_id_is_good() {
    let json = |v: serde_json::Value| Answer::Reply(200, v.to_string());
    let good = serde_json::json!({"core": {"instance_id": "00000000000000aF"}, "journal": {}});
    assert!(good_answer(&status_of(json(good))));
    // The cap is not needed (the boot rule without the cap).
    let no_cap = serde_json::json!({"core": {"instance_id": "0123456789abcdef"}});
    assert!(good_answer(&status_of(json(no_cap))));
    for bad in [
        Answer::Reply(
            503,
            serde_json::json!({"core": {"instance_id": "0123456789abcdef"}}).to_string(),
        ),
        Answer::Reply(404, "{}".into()),
        Answer::Reply(200, "not json".into()),
        json(serde_json::json!({"core": {"instance_id": "0123456789abcde"}})),
        json(serde_json::json!({"core": {"instance_id": "0123456789abcdef0"}})),
        json(serde_json::json!({"core": {"instance_id": "0123456789abcdeg"}})),
        json(serde_json::json!({"core": {"instance_id": 81985529216486895u64}})),
        json(serde_json::json!({"core": {}})),
        json(serde_json::json!({})),
    ] {
        assert!(!good_answer(&status_of(bad.clone())), "{bad:?}");
    }
    assert!(!good_answer(&Err("no answer: connection refused".into())));
}
