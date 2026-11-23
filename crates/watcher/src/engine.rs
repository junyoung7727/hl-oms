//! The check engine (C-WATCH part 5, U19a5), sans-IO on the injected
//! clock: it neither sleeps nor sends. `poll(now)` starts the rounds and
//! heartbeats that are due and returns them as [`Action`]s; the runtime
//! (U19a7) carries them out and gives every answer back (`on_venue`,
//! `on_status`). The engine reads the journal itself through [`Tail`].
//!
//! A round of a check: (1) its venue pages, one after another; (2) one
//! `/api/status` request of its own, emitted only after the last venue
//! answer; (3) the journal read up to that answer's `journal.durable_seq`;
//! (4) the comparison. The venue is read before the journal: the OMS sends
//! nothing before its record is durable (C-COMMIT), so whatever the venue
//! shows that the OMS made is in the journal read after it.
//!
//! The two-round rule: a difference `(check, kind, subject, value)` is
//! confirmed when the previous completed round of the check had it too
//! and, at this round, its time is at least `min_age_ms` old. A first
//! sighting is pending (the round may still succeed); a second sighting
//! that is still young is deferred (the round is not a success). What a
//! check compares is its [`Logic`]: W1 in U19a5, W2 in U19a6, the others
//! later.
//!
//! Sending (U19a7): a confirmed difference is **owed** until a send that
//! carried it is answered `"held"` (`on_admin`); nothing else pays it. A
//! send of a check is due while the hold is off (the journal state shows
//! no `watcher_mismatch` hold, read up to the cap of a status request
//! emitted after the latest `"held"`/`"already held"` answer of that
//! check), the check owes a difference or (W1–W3) still sees a confirmed
//! one, `mismatch_resend_ms` passed since the previous send started, and
//! no send of the check is in flight. One send carries all of them.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde_json::Value;

use crate::config::Config;
use crate::journal::{JournalError, MAX_RECORDS, Tail};
use crate::log::Log;
use crate::metrics::Metrics;
use crate::rest::RestError;
use crate::state::JournalState;
use crate::w1::W1;

/// A request without an answer fails its round at its emission +
/// `request_timeout_ms` + this: the guard's 10 s window plus 1 s
/// (판단(모델 아님)).
pub const DEADLINE_EXTRA_MS: u64 = 11_000;
/// `watcher_mismatch.detail` is at most this many bytes (`api.md`).
pub const DETAIL_MAX: usize = 1024;
/// A send without a report is taken as failed at its emission +
/// 2 × `request_timeout_ms` (the client's bound of a whole send) + this
/// (판단(모델 아님)).
pub const SEND_EXTRA_MS: u64 = 1_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Check {
    W1,
    W2,
    W3,
    W4,
}

impl Check {
    pub const ALL: [Check; 4] = [Check::W1, Check::W2, Check::W3, Check::W4];

    pub fn name(self) -> &'static str {
        match self {
            Check::W1 => "W1",
            Check::W2 => "W2",
            Check::W3 => "W3",
            Check::W4 => "W4",
        }
    }

    /// A confirmed difference that a round still sees is sent again after
    /// an operator's clear (W1–W3); W4's are dismissed instead (U19a8).
    fn resends_while_seen(self) -> bool {
        self != Check::W4
    }
}

/// A signed GET of Bybit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VenueRequest {
    pub path: &'static str,
    pub params: Vec<(&'static str, String)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Venue {
        req_id: u64,
        path: &'static str,
        params: Vec<(&'static str, String)>,
    },
    /// `GET /api/status`; the answer goes to `on_status` with this id.
    Status { req_id: u64 },
    /// `watcher_mismatch` to `admin.sock`; its answer goes to `on_admin`
    /// with this `send_id` (review 1 S1 of U19a7).
    Mismatch {
        check: Check,
        send_id: u64,
        detail: String,
    },
    /// `heartbeat` to `monitor.sock`: each check's last success (ms).
    Heartbeat { checks: Vec<(Check, Option<u64>)> },
}

/// What a check makes of one venue page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Page {
    Next(VenueRequest),
    /// The venue read is complete.
    Done,
    /// retCode 0, but the page cannot be read: counted as `unreadable`.
    Unreadable(String),
    /// The round fails for another reason (too many pages).
    Fail(String),
}

/// The time a difference's age is judged from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Since {
    /// A known instant, ms.
    At(u64),
    /// The first round that saw the difference (an order the journal does
    /// not know).
    FirstSeen,
    /// Not known: taken as old.
    Unknown,
}

/// A difference one round saw.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diff {
    pub kind: &'static str,
    pub subject: String,
    /// Part of the identity: another value is another difference.
    pub value: String,
    /// `venue …, journal …` for the detail.
    pub what: String,
    pub since: Since,
}

/// What a check adds to the engine's rounds.
pub trait Logic {
    fn check(&self) -> Check;
    /// The first request of a round; the check forgets the last read.
    fn first(&mut self) -> VenueRequest;
    /// The `result` of a retCode 0 answer to the last request.
    fn page(&mut self, now_ms: u64, result: &Value) -> Page;
    /// The differences between the complete venue read and the journal.
    fn compare(&mut self, now_ms: u64, state: &JournalState) -> Vec<Diff>;
    /// A known time younger than `min_age_ms` defers the round even at a
    /// difference's first sighting (W2, design R5); W1 waits for the
    /// second.
    fn young_defers_at_first_sighting(&self) -> bool {
        false
    }
}

/// `(kind, subject, value)`; the check is the slot's.
type Ident = (&'static str, String, String);

#[derive(Clone, Copy)]
enum Wait {
    Venue(&'static str),
    /// `last_seq` of the journal state when the request was emitted.
    Status {
        seq_at_send: u64,
    },
}

/// The one request of a round in flight.
struct Open {
    req_id: u64,
    wait: Wait,
    deadline: u64,
}

/// The one send of a check in flight: what it carried.
struct Sending {
    id: u64,
    carried: Vec<Ident>,
    deadline: u64,
}

struct Slot {
    logic: Box<dyn Logic>,
    interval_ms: u64,
    next_due: u64,
    round: Option<Open>,
    success_ms: Option<u64>,
    /// The differences of the last completed round → their first sighting.
    seen: BTreeMap<Ident, u64>,
    /// Confirmed and still seen → the detail at the confirmation.
    confirmed: BTreeMap<Ident, String>,
    /// Confirmed and not yet paid by a `"held"` answer, in the order of
    /// confirmation, with the detail at the confirmation.
    owed: Vec<(Ident, String)>,
    sending: Option<Sending>,
    /// When the previous send started.
    last_send_ms: Option<u64>,
    /// The first request id after the latest `"held"`/`"already held"`
    /// answer: only a status request from it on shows the hold off.
    hold_answer_req: Option<u64>,
    /// The reason of the last failed round, until a round succeeds.
    failing: Option<String>,
}

pub struct Engine {
    tail: Tail,
    state: JournalState,
    metrics: Arc<Metrics>,
    log: Log,
    min_age_ms: u64,
    request_timeout_ms: u64,
    resend_ms: u64,
    heartbeat_ms: u64,
    boot_ms: u64,
    start_ms: u64,
    next_heartbeat: u64,
    next_req: u64,
    next_send: u64,
    slots: Vec<Slot>,
    latched: Option<JournalError>,
    journal_failing: bool,
    /// The newest status request whose cap the journal state reached.
    caught_up_req: u64,
}

impl Engine {
    /// The engine after the boot replay: `tail` and `state` read up to the
    /// boot's cap. `boot_ms` is the clock at config load (the check ages
    /// count from it before a first success); the first rounds and the
    /// first heartbeat are due at `start_ms`. Runs W1.
    pub fn new(
        cfg: &Config,
        tail: Tail,
        state: JournalState,
        metrics: Arc<Metrics>,
        log: Log,
        boot_ms: u64,
        start_ms: u64,
    ) -> Engine {
        let mut e = Engine {
            tail,
            state,
            metrics,
            log,
            min_age_ms: cfg.rules.min_age_ms,
            request_timeout_ms: cfg.http.request_timeout_ms,
            resend_ms: cfg.rules.mismatch_resend_ms,
            heartbeat_ms: cfg.intervals.heartbeat_ms,
            boot_ms,
            start_ms,
            next_heartbeat: start_ms,
            next_req: 1,
            next_send: 1,
            slots: Vec::new(),
            latched: None,
            journal_failing: false,
            caught_up_req: 0,
        };
        e.add_check(Box::new(W1::default()), cfg.intervals.w1_ms);
        e
    }

    /// Adds a check whose first round is due at `start_ms`.
    pub fn add_check(&mut self, logic: Box<dyn Logic>, interval_ms: u64) {
        let c = [("check", logic.check().name())];
        self.metrics.set("watcher_differences", &c, 0.0);
        self.metrics.set("watcher_mismatch_active", &c, 0.0);
        self.slots.push(Slot {
            logic,
            interval_ms,
            next_due: self.start_ms,
            round: None,
            success_ms: None,
            seen: BTreeMap::new(),
            confirmed: BTreeMap::new(),
            owed: Vec::new(),
            sending: None,
            last_send_ms: None,
            hold_answer_req: None,
            failing: None,
        });
    }

    pub fn state(&self) -> &JournalState {
        &self.state
    }

    /// The last success time of `check` (ms).
    pub fn success(&self, check: Check) -> Option<u64> {
        self.slot(check).and_then(|s| s.success_ms)
    }

    fn slot(&self, check: Check) -> Option<&Slot> {
        self.slots.iter().find(|s| s.logic.check() == check)
    }

    /// Fails the rounds whose request passed its deadline, starts the
    /// rounds that are due (never two of one check: a late round runs
    /// once) and the heartbeat.
    pub fn poll(&mut self, now: u64) -> Vec<Action> {
        let mut out = Vec::new();
        for i in 0..self.slots.len() {
            if self.slots[i]
                .round
                .as_ref()
                .is_some_and(|r| now >= r.deadline)
            {
                self.fail(
                    i,
                    "no answer before the request's deadline (taken as no_answer)".into(),
                );
            }
            let s = &mut self.slots[i];
            if s.round.is_some() || now < s.next_due {
                continue;
            }
            s.next_due = now + s.interval_ms;
            // A latched reader judges nothing: the round asks nothing.
            if let Some(e) = &self.latched {
                let why = format!("the journal reader stopped: {e}");
                self.fail(i, why);
            } else {
                let req = s.logic.first();
                self.emit_venue(i, now, req, &mut out);
            }
        }
        for i in 0..self.slots.len() {
            let s = &mut self.slots[i];
            if s.sending.as_ref().is_some_and(|x| now >= x.deadline) {
                s.sending = None;
                (self.log)(&format!(
                    "warn: {} mismatch send has no report before its deadline (taken as failed)",
                    s.logic.check().name()
                ));
            }
            self.send(i, now, &mut out);
        }
        if now >= self.next_heartbeat {
            self.next_heartbeat = now + self.heartbeat_ms;
            let checks = Check::ALL.map(|c| (c, self.success(c))).to_vec();
            out.push(Action::Heartbeat { checks });
        }
        for s in &self.slots {
            let since = s.success_ms.unwrap_or(self.boot_ms);
            let age = now.saturating_sub(since) as f64 / 1000.0;
            let c = [("check", s.logic.check().name())];
            self.metrics.set("watcher_check_age_s", &c, age);
        }
        out
    }

    /// The answer to a `Venue` request.
    pub fn on_venue(
        &mut self,
        now: u64,
        req_id: u64,
        answer: Result<Value, RestError>,
    ) -> Vec<Action> {
        let mut out = Vec::new();
        let Some((i, Wait::Venue(path))) = self.owner(req_id) else {
            self.stray(req_id);
            return out;
        };
        let page = match answer {
            Err(e) => Page::Fail(e.to_string()),
            Ok(v) => self.slots[i].logic.page(now, &v),
        };
        match page {
            Page::Next(req) => self.emit_venue(i, now, req, &mut out),
            Page::Done => {
                let req_id = self.next_id();
                self.slots[i].round = Some(Open {
                    req_id,
                    wait: Wait::Status {
                        seq_at_send: self.state.last_seq,
                    },
                    deadline: self.deadline(now),
                });
                out.push(Action::Status { req_id });
            }
            Page::Unreadable(why) => {
                let l = [("endpoint", path), ("class", "unreadable")];
                self.metrics.add("watcher_request_errors_total", &l, 1.0);
                self.fail(i, format!("{path}: unreadable: {why}"));
            }
            Page::Fail(why) => self.fail(i, format!("{path}: {why}")),
        }
        out
    }

    /// The answer to the send `send_id` of `check`: the `result` of
    /// `watcher_mismatch`, or why there is none (`<code>: <message>` for an
    /// error reply). Only `"held"` pays what the send carried. The answer
    /// of a send that is not the one in flight (its deadline passed, and a
    /// later send may carry more) is ignored and pays nothing.
    pub fn on_admin(
        &mut self,
        now: u64,
        check: Check,
        send_id: u64,
        answer: Result<String, String>,
    ) -> Vec<Action> {
        let mut out = Vec::new();
        let Some(i) = self.slots.iter().position(|s| s.logic.check() == check) else {
            return out;
        };
        let after = self.next_req;
        let s = &mut self.slots[i];
        if s.sending.as_ref().is_none_or(|x| x.id != send_id) {
            (self.log)(&format!(
                "debug: an answer to {} send {send_id}, which is not the send in flight, is ignored",
                check.name()
            ));
            return out;
        }
        let Some(sent) = s.sending.take() else {
            return out;
        };
        match answer.as_deref() {
            Ok("held") => {
                s.owed.retain(|(id, _)| !sent.carried.contains(id));
                s.hold_answer_req = Some(after);
            }
            Ok("already held") => s.hold_answer_req = Some(after),
            Ok(other) => (self.log)(&format!(
                "error: {} mismatch send: unexpected answer {other:?}; sent again later",
                check.name()
            )),
            Err(e) if e.starts_with("bad_request") => (self.log)(&format!(
                "error: {} mismatch send refused ({e}): a watcher bug; sent again later",
                check.name()
            )),
            Err(e) => (self.log)(&format!(
                "warn: {} mismatch send failed: {e}; sent again later",
                check.name()
            )),
        }
        self.set_active(i);
        self.send(i, now, &mut out);
        out
    }

    /// The hold is off for the sends of slot `i`: the journal state shows
    /// none, read up to the cap of a status request emitted after the
    /// latest `"held"`/`"already held"` answer of the check.
    fn hold_off(&self, i: usize) -> bool {
        !self.state.holds.watcher_mismatch
            && self.slots[i]
                .hold_answer_req
                .is_none_or(|a| self.caught_up_req >= a)
    }

    /// Starts the send of slot `i` if it is due.
    fn send(&mut self, i: usize, now: u64, out: &mut Vec<Action>) {
        if self.slots[i].sending.is_some() || !self.hold_off(i) {
            return;
        }
        let timeout = self.request_timeout_ms;
        let s = &mut self.slots[i];
        if s.last_send_ms.is_some_and(|t| now.saturating_sub(t) < self.resend_ms) {
            return;
        }
        let check = s.logic.check();
        let send_id = self.next_send;
        let mut carried: Vec<(Ident, String)> = s.owed.clone();
        if check.resends_while_seen() {
            for (id, d) in &s.confirmed {
                if !carried.iter().any(|(c, _)| c == id) {
                    carried.push((id.clone(), d.clone()));
                }
            }
        }
        let Some((_, first)) = carried.first() else {
            return;
        };
        let mut detail = first.clone();
        if carried.len() > 1 {
            detail.push_str(&format!("; and {} more", carried.len() - 1));
        }
        s.last_send_ms = Some(now);
        self.next_send += 1;
        s.sending = Some(Sending {
            id: send_id,
            carried: carried.into_iter().map(|(id, _)| id).collect(),
            deadline: now + 2 * timeout + SEND_EXTRA_MS,
        });
        out.push(Action::Mismatch {
            check,
            send_id,
            detail: cut(detail, DETAIL_MAX),
        });
    }

    /// `watcher_mismatch_active`: the confirmed differences a round still
    /// sees and the owed ones.
    fn set_active(&self, i: usize) {
        let s = &self.slots[i];
        let owed_only = s
            .owed
            .iter()
            .filter(|(id, _)| !s.confirmed.contains_key(id))
            .count();
        let active = (s.confirmed.len() + owed_only) as f64;
        let c = [("check", s.logic.check().name())];
        self.metrics.set("watcher_mismatch_active", &c, active);
    }

    /// The answer to a `Status` request: the body of `/api/status` as
    /// JSON, or why there is none (the OMS did not answer, not JSON).
    pub fn on_status(
        &mut self,
        now: u64,
        req_id: u64,
        answer: Result<Value, String>,
    ) -> Vec<Action> {
        let mut out = Vec::new();
        let Some((i, Wait::Status { seq_at_send })) = self.owner(req_id) else {
            self.stray(req_id);
            return out;
        };
        match self.read_to(answer, seq_at_send) {
            Ok(()) => {
                self.caught_up_req = self.caught_up_req.max(req_id);
                self.judge(i, now);
                for j in 0..self.slots.len() {
                    self.send(j, now, &mut out);
                }
            }
            Err(why) => self.fail(i, why),
        }
        out
    }

    /// The slot whose open request is `req_id`, and what it waits for.
    fn owner(&self, req_id: u64) -> Option<(usize, Wait)> {
        self.slots.iter().enumerate().find_map(|(i, s)| {
            s.round
                .as_ref()
                .filter(|r| r.req_id == req_id)
                .map(|r| (i, r.wait))
        })
    }

    fn stray(&self, req_id: u64) {
        (self.log)(&format!(
            "debug: an answer to request {req_id}, which no open request owns, is ignored"
        ));
    }

    fn next_id(&mut self) -> u64 {
        self.next_req += 1;
        self.next_req - 1
    }

    fn deadline(&self, now: u64) -> u64 {
        now + self.request_timeout_ms + DEADLINE_EXTRA_MS
    }

    fn emit_venue(&mut self, i: usize, now: u64, req: VenueRequest, out: &mut Vec<Action>) {
        let req_id = self.next_id();
        self.slots[i].round = Some(Open {
            req_id,
            wait: Wait::Venue(req.path),
            deadline: self.deadline(now),
        });
        out.push(Action::Venue {
            req_id,
            path: req.path,
            params: req.params,
        });
    }

    /// Ends the round of slot `i` without a success; one line per change
    /// of the reason.
    fn fail(&mut self, i: usize, why: String) {
        let s = &mut self.slots[i];
        s.round = None;
        if s.failing.as_deref() != Some(why.as_str()) {
            (self.log)(&format!(
                "warn: {} round failed: {why}",
                s.logic.check().name()
            ));
            s.failing = Some(why);
        }
    }

    /// Checks the status answer and reads the journal up to its cap.
    fn read_to(&mut self, answer: Result<Value, String>, seq_at_send: u64) -> Result<(), String> {
        let v = answer.map_err(|e| format!("/api/status: {e}"))?;
        let cap = v
            .pointer("/journal/durable_seq")
            .and_then(Value::as_u64)
            .ok_or("/api/status: journal.durable_seq is not a u64")?;
        let instance = v
            .pointer("/core/instance_id")
            .and_then(Value::as_str)
            .filter(|s| s.len() == 16 && s.bytes().all(|b| b.is_ascii_hexdigit()))
            .and_then(|s| u64::from_str_radix(s, 16).ok())
            .ok_or("/api/status: core.instance_id is not 16 hex digits")?;
        // Another round may have read further: a cap at or below what the
        // state holds is no error, only one below what it held when this
        // round asked.
        if cap < seq_at_send {
            self.latch(JournalError::WentBack {
                cap,
                read: seq_at_send,
            });
        }
        self.check_instance(instance);
        if let Some(e) = &self.latched {
            return Err(format!("the journal reader stopped: {e}"));
        }
        match self.state.read_from(&mut self.tail, cap, MAX_RECORDS) {
            Ok(_) => {}
            Err(e) if e.is_retried() => {
                if !self.journal_failing {
                    (self.log)(&format!(
                        "error: {e}; the journal is read again at the next round"
                    ));
                }
                self.journal_failing = true;
                self.metrics.set("watcher_journal_error", &[], 1.0);
                return Err(e.to_string());
            }
            Err(e) => {
                self.latch(e.clone());
                return Err(format!("the journal reader stopped: {e}"));
            }
        }
        self.check_instance(instance);
        let last = self.state.last_seq;
        self.metrics.set("watcher_journal_seq", &[], last as f64);
        if let Some(e) = &self.latched {
            return Err(format!("the journal reader stopped: {e}"));
        }
        if last < cap {
            return Err(format!(
                "the journal read stopped at record {last}, below the cap {cap}"
            ));
        }
        if self.journal_failing {
            (self.log)("the journal is read again without error");
        }
        self.journal_failing = false;
        self.metrics.set("watcher_journal_error", &[], 0.0);
        Ok(())
    }

    /// The status must name the instance of the journal's segments.
    fn check_instance(&mut self, status: u64) {
        if let Some(journal) = self.state.instance_id.filter(|&j| j != status) {
            self.latch(JournalError::Instance {
                expected: status,
                found: journal,
            });
        }
    }

    /// Stops the reader until restart; one error line.
    fn latch(&mut self, e: JournalError) {
        if self.latched.is_none() {
            (self.log)(&format!(
                "error: {e}; the journal reader stops until the watcher restarts"
            ));
            self.latched = Some(e);
        }
        self.journal_failing = true;
        self.metrics.set("watcher_journal_error", &[], 1.0);
    }

    /// The comparison of a round and the two-round rule.
    fn judge(&mut self, i: usize, now: u64) {
        let min_age = self.min_age_ms;
        let s = &mut self.slots[i];
        s.round = None;
        let check = s.logic.check();
        let strict = s.logic.young_defers_at_first_sighting();
        let mut seen = BTreeMap::new();
        let (mut open, mut deferred) = (0usize, false);
        let mut confirmed = Vec::new();
        for d in s.logic.compare(now, &self.state) {
            let id: Ident = (d.kind, d.subject.clone(), d.value.clone());
            if seen.contains_key(&id) {
                continue;
            }
            let before = s.seen.get(&id).copied();
            seen.insert(id.clone(), before.unwrap_or(now));
            if s.confirmed.contains_key(&id) {
                continue;
            }
            open += 1;
            // A strict check (W2) defers a young first sighting too.
            deferred |= before.is_none()
                && strict
                && matches!(d.since, Since::At(at) if now.saturating_sub(at) < min_age);
            // A first sighting is pending.
            let Some(first) = before else { continue };
            let since = match d.since {
                Since::At(t) => Some(t),
                Since::FirstSeen => Some(first),
                Since::Unknown => None,
            };
            if since.is_some_and(|t| now.saturating_sub(t) < min_age) {
                deferred = true;
                continue;
            }
            open -= 1;
            confirmed.push((id, detail(check, &d, since.map(|t| now - t.min(now)))));
        }
        s.seen = seen;
        let still = &s.seen;
        s.confirmed.retain(|id, _| still.contains_key(id));
        let c = [("check", check.name())];
        for (id, line) in confirmed {
            s.confirmed.insert(id.clone(), line.clone());
            if !s.owed.iter().any(|(o, _)| *o == id) {
                s.owed.push((id, line.clone()));
            }
            self.metrics.add("watcher_mismatches_total", &c, 1.0);
            (self.log)(&format!("error: confirmed difference: {line}"));
        }
        self.metrics.set("watcher_differences", &c, open as f64);
        self.set_active(i);
        let s = &mut self.slots[i];
        if !deferred {
            s.success_ms = Some(now);
            if s.failing.take().is_some() {
                (self.log)(&format!("{} rounds succeed again", check.name()));
            }
        }
    }
}

/// `"<check> <kind> <subject>: <what>; seen 2 rounds, <age>"`.
fn detail(check: Check, d: &Diff, age_ms: Option<u64>) -> String {
    let secs = |ms: u64| format!("{}.{} s", ms / 1000, ms % 1000 / 100);
    let age = match (d.since, age_ms) {
        (Since::FirstSeen, Some(a)) => format!("first seen {} ago", secs(a)),
        (_, Some(a)) => format!("journal entry {} old", secs(a)),
        (_, None) => "change time unknown".to_string(),
    };
    format!(
        "{} {} {}: {}; seen 2 rounds, {age}",
        check.name(),
        d.kind,
        d.subject,
        d.what
    )
}

/// `s` cut to at most `max` bytes at a char boundary.
pub fn cut(mut s: String, max: usize) -> String {
    if s.len() > max {
        let mut n = max;
        while !s.is_char_boundary(n) {
            n -= 1;
        }
        s.truncate(n);
    }
    s
}
