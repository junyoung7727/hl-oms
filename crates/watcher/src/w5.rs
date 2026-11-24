//! W5, the OMS silence check (C-WATCH part 12, ADR-032, U19b3a), sans-IO:
//! the runtime gives it the instant, the probe answers and the kill's end;
//! it returns the probes to send, the one `Fire` that starts a kill, the
//! owed halt and the P1 texts of its outcome. Nothing here sleeps, sends
//! or reads a clock: the runtime (U19b3c) polls it at every tick, in every
//! state, with `clock::Steady` (a clock that does not step back).
//!
//! W5 sees a dead OMS process or a silent HTTP task only: `/api/status` is
//! built from the published state, not by the core loop, so a hung core
//! behind an answering HTTP task is not a W5 kill (known limit).

use serde_json::Value;

use crate::config::Config;
use crate::kill::{TEXT_MAX, cut};
use crate::log::Log;
use crate::oms;

/// A probe without an answer at emission + `request_timeout_ms` + this is
/// a failed probe (판단(모델 아님)).
pub const PROBE_EXTRA_MS: u64 = 1000;

/// W5's timings: `[w5]` and the client's `request_timeout_ms`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timing {
    pub probe_ms: u64,
    pub silence_ms: u64,
    pub request_timeout_ms: u64,
}

impl Timing {
    /// The timings of a config with `[kill]`; `None` without it (W5 off).
    pub fn of(cfg: &Config) -> Option<Timing> {
        cfg.kill.as_ref().map(|_| Timing {
            probe_ms: cfg.w5.probe_ms,
            silence_ms: cfg.w5.silence_ms,
            request_timeout_ms: cfg.http.request_timeout_ms,
        })
    }
}

/// W5's state (C-WATCH part 12).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// No `[kill]`: W5 never probes and never fires.
    Off,
    /// Before the end of the start checks: a cancel-all must never go to
    /// an unverified account.
    Unarmed,
    /// Watching: the last good answer at `last_good_ms`.
    Armed { last_good_ms: u64 },
    /// A kill runs: no probe, no silence judged.
    Killing,
    /// After a kill, until one good answer; with `halt_owed` that answer
    /// first owes a `watcher_kill`.
    Rearming { halt_owed: bool },
}

/// Send one `/api/status` request; answer with [`W5::on_answer`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Probe {
    pub req_id: u64,
}

/// The OMS was silent: start the kill. `silent_ms` = now − `last_good_ms`
/// on W5's clock (texts take their instants from the raw clock).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fire {
    pub last_good_ms: u64,
    pub silent_ms: u64,
}

/// The OMS answers after a kill that owes the halt: send one
/// `watcher_kill` and report its outcome with [`W5::owed_halt_done`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OwedHalt;

/// Is a `/api/status` answer good: 2xx JSON (`oms::Status::get` gives
/// `Ok` only for that) with a 16-hex `core.instance_id`, the boot rule
/// without the cap.
pub fn good_answer(answer: &Result<Value, String>) -> bool {
    answer.as_ref().is_ok_and(|v| {
        v.pointer("/core/instance_id")
            .and_then(Value::as_str)
            .is_some_and(|s| s.len() == 16 && s.bytes().all(|b| b.is_ascii_hexdigit()))
    })
}

#[derive(Debug, Clone, Copy)]
struct Open {
    req_id: u64,
    emitted: u64,
    deadline: u64,
}

/// The sans-IO W5.
pub struct W5 {
    timing: Option<Timing>,
    state: State,
    log: Log,
    next_id: u64,
    /// The probe in flight.
    open: Option<Open>,
    /// The emission of the previous probe (the next is due `probe_ms`
    /// after it).
    last_emitted: Option<u64>,
    /// The emission of the latest failed probe.
    failed_emitted: Option<u64>,
    /// The instant of the previous `poll`, `fire` or answer (gap rule).
    prev_seen: Option<u64>,
    /// An owed `watcher_kill` is in flight.
    halt_in_flight: bool,
    /// The text of the first `늦은 정지 실패` P1 since the kill ended.
    halt_failed: Option<String>,
}

impl W5 {
    /// `None` (no `[kill]`) gives an `Off` W5.
    pub fn new(timing: Option<Timing>, log: Log) -> W5 {
        W5 {
            state: if timing.is_some() {
                State::Unarmed
            } else {
                State::Off
            },
            timing,
            log,
            next_id: 1,
            open: None,
            last_emitted: None,
            failed_emitted: None,
            prev_seen: None,
            halt_in_flight: false,
            halt_failed: None,
        }
    }

    pub fn state(&self) -> State {
        self.state
    }

    /// `watcher_w5_armed`: 1 in `Armed` only.
    pub fn armed(&self) -> bool {
        matches!(self.state, State::Armed { .. })
    }

    /// `watcher_oms_silence_s` × 1000 while `Armed`.
    pub fn silent_ms(&self, now: u64) -> Option<u64> {
        match self.state {
            State::Armed { last_good_ms } => Some(now.saturating_sub(last_good_ms)),
            _ => None,
        }
    }

    /// W5's share of `watcher_kill_halt_owed`: `Rearming {halt_owed: true}`.
    pub fn halt_owed(&self) -> bool {
        self.state == State::Rearming { halt_owed: true }
    }

    /// The id of the probe in flight.
    pub fn probe_in_flight(&self) -> Option<u64> {
        self.open.map(|o| o.req_id)
    }

    /// The start checks ended (`Done` or `StayUpNoRequests`): watch from
    /// `now`, whether or not the OMS ever answered in this process.
    pub fn arm(&mut self, now: u64) {
        if self.state == State::Unarmed {
            self.state = State::Armed { last_good_ms: now };
            self.prev_seen = Some(now);
        }
    }

    /// The gap rule (review 2 S3): two instants more than `2 × probe_ms +
    /// request_timeout_ms` apart were not observed by the watcher (its
    /// process stopped or starved, a clock step forward). Every instant W5
    /// holds moves forward by the gap, so unobserved time is not silence.
    fn gap(&mut self, now: u64) {
        let Some(t) = self.timing else { return };
        let prev = self.prev_seen.replace(now);
        let Some(prev) = prev else { return };
        let gap = now.saturating_sub(prev);
        let threshold = 2 * t.probe_ms + t.request_timeout_ms;
        if gap <= threshold {
            return;
        }
        if let State::Armed { last_good_ms } = &mut self.state {
            *last_good_ms += gap;
        }
        if let Some(o) = &mut self.open {
            o.emitted += gap;
            o.deadline += gap;
        }
        for t in [&mut self.last_emitted, &mut self.failed_emitted]
            .into_iter()
            .flatten()
        {
            *t += gap;
        }
        (self.log)(&format!(
            "warn: W5: {gap} ms between two polls (more than {threshold} ms): the watcher did not observe that time, so it is not OMS silence"
        ));
    }

    /// The probe in flight past its deadline is a failed probe.
    fn expire(&mut self, now: u64) {
        if let Some(o) = self.open
            && now >= o.deadline
        {
            self.open = None;
            self.failed_emitted = Some(o.emitted);
        }
    }

    /// At every tick: in `Armed` and `Rearming` one probe every `probe_ms`
    /// from the previous probe's emission, never two in flight.
    pub fn poll(&mut self, now: u64) -> Option<Probe> {
        let t = self.timing?;
        self.gap(now);
        self.expire(now);
        if !matches!(self.state, State::Armed { .. } | State::Rearming { .. })
            || self.open.is_some()
        {
            return None;
        }
        if self
            .last_emitted
            .is_some_and(|e| now < e.saturating_add(t.probe_ms))
        {
            return None;
        }
        let req_id = self.next_id;
        self.next_id += 1;
        self.open = Some(Open {
            req_id,
            emitted: now,
            deadline: now + t.request_timeout_ms + PROBE_EXTRA_MS,
        });
        self.last_emitted = Some(now);
        Some(Probe { req_id })
    }

    /// The answer to probe `req_id`, taken when it arrives (also at or
    /// after its deadline if no poll applied the deadline yet: the truth
    /// about the OMS first). `good` = [`good_answer`]. An answer to a
    /// probe that is not in flight is ignored.
    pub fn on_answer(&mut self, now: u64, req_id: u64, good: bool) -> Option<OwedHalt> {
        self.gap(now);
        let o = self.open.filter(|o| o.req_id == req_id)?;
        self.open = None;
        if !good {
            self.failed_emitted = Some(o.emitted);
            return None;
        }
        match self.state {
            State::Armed { .. } | State::Rearming { halt_owed: false } => {
                self.state = State::Armed { last_good_ms: now };
            }
            State::Rearming { halt_owed: true } if !self.halt_in_flight => {
                self.halt_in_flight = true;
                return Some(OwedHalt);
            }
            _ => {}
        }
        None
    }

    /// In `Armed`: `silence_ms` without a good answer and a failed probe
    /// emitted at or after the last good answer → `Killing`, once. The
    /// second condition never delays a real kill under the config bound
    /// `silence_ms ≥ 3 × probe_ms + 3 × request_timeout_ms + 1000`.
    pub fn fire(&mut self, now: u64) -> Option<Fire> {
        let t = self.timing?;
        self.gap(now);
        self.expire(now);
        let State::Armed { last_good_ms } = self.state else {
            return None;
        };
        let silent_ms = now.saturating_sub(last_good_ms);
        if silent_ms < t.silence_ms || self.failed_emitted.is_none_or(|e| e < last_good_ms) {
            return None;
        }
        self.state = State::Killing;
        Some(Fire {
            last_good_ms,
            silent_ms,
        })
    }

    /// The kill ended (or was aborted): wait for one good answer before
    /// watching again, so an OMS the watcher stopped is not killed again
    /// every `silence_ms`; with `halt_owed` that answer owes a
    /// `watcher_kill` first.
    pub fn kill_ended(&mut self, halt_owed: bool) {
        if self.state == State::Killing {
            self.state = State::Rearming { halt_owed };
            self.halt_in_flight = false;
            self.halt_failed = None;
        }
    }

    /// The outcome of the owed `watcher_kill` (`oms::admin_kill`): a reply
    /// that latched → `Armed {now}` and the `늦은 정지` P1 (with a `해소`
    /// line after an earlier failure); anything else → still owed, tried
    /// again at the next good answer, with the `늦은 정지 실패` P1 at the
    /// first failure only. `None`: no P1.
    pub fn owed_halt_done(&mut self, now: u64, outcome: Result<String, String>) -> Option<String> {
        if !self.halt_in_flight || !self.halt_owed() {
            return None;
        }
        self.halt_in_flight = false;
        let why = match outcome {
            Ok(reply) if oms::latched(&reply) => {
                self.state = State::Armed { last_good_ms: now };
                let text = format!(
                    "늦은 정지: OMS가 다시 답해 watcher_kill로 래치함({})",
                    cut(&reply, TEXT_MAX)
                );
                return Some(match self.halt_failed.take() {
                    Some(failed) => format!("{text}\n해소: {failed}"),
                    None => text,
                });
            }
            Ok(reply) => reply,
            Err(why) => why,
        };
        if self.halt_failed.is_some() {
            return None;
        }
        let text = format!(
            "늦은 정지 실패: OMS가 답하지만 watcher_kill이 안 됨({}). OMS가 래치 없이 주문을 낼 수 있음",
            cut(&why, TEXT_MAX)
        );
        self.halt_failed = Some(text.clone());
        Some(text)
    }
}
