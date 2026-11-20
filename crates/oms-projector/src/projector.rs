//! One projector step (design §8.2 P1–P5): ask the status, read the journal
//! up to the durable sequence, replay, push lines and metrics, move the
//! cursor when the log store has taken the lines.

use crate::cursor;
use crate::lines::{self, Ctx, OrderInfo};
use crate::metrics::{self, View};
use crate::pnl::Pnl;
use crate::probe::{FreeSpace, LockProbe, StatusSource};
use obs_push::{LogLine, ObsHandle, Sample};
use oms_core::record::READABLE_RECORD_SCHEMAS;
use oms_core::{Core, Event, OrderId, Record, Side, WireIdForm};
use oms_journal::{Tail, TailRecord};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::PathBuf;

/// What a projector is told at start.
#[derive(Clone, Debug)]
pub struct Settings {
    pub data_dir: PathBuf,
    pub cursor_file: PathBuf,
    pub venue: String,
    pub host: String,
    pub max_records: usize,
    pub max_pending_lines: usize,
    pub dead_after_us: u64,
    pub disk_every_us: u64,
    /// Whether log lines go to a store. Without one the cursor follows the
    /// applied records at once.
    pub logs_on: bool,
}

/// What one step did, for tests and the process log.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StepReport {
    pub applied: usize,
    pub lines: usize,
    pub unconfirmed_lines: usize,
    pub cursor: u64,
    pub dead: bool,
    pub tail_error: Option<String>,
    pub fatal: Option<String>,
}

struct Pending {
    seq: u64,
    lines: Vec<LogLine>,
    /// `None` until the lines are pushed. Then: lines pushed in this run
    /// when the last line of this record was pushed; the record is taken
    /// when the store has processed that many.
    end_mark: Option<u64>,
}

/// Longest time one step spends replaying records an earlier run emitted
/// (judgement: the step period is 1 s).
const REPLAY_BUDGET: std::time::Duration = std::time::Duration::from_millis(300);

pub struct Projector {
    s: Settings,
    instance_id: u64,
    core: Core,
    pnl: BTreeMap<String, Pnl>,
    tail: Tail,
    status: Box<dyn StatusSource>,
    locks: Box<dyn LockProbe>,
    disk: Box<dyn FreeSpace>,
    obs: Option<ObsHandle>,
    /// The cursor in the file.
    cursor: u64,
    stored_cursor: u64,
    /// Records up to this one were emitted by an earlier run: replayed
    /// without lines.
    emit_after: u64,
    applied: u64,
    durable: u64,
    /// The replay has reached the read limit once: from then on the state
    /// series show the durable journal, not a prefix of it.
    caught_up: bool,
    up: bool,
    last_answer_us: u64,
    heartbeat_us: Option<u64>,
    wire_id_form: Option<WireIdForm>,
    /// Records after the cursor, in order: pushed ones first, then the
    /// ones waiting to be pushed.
    pending: VecDeque<Pending>,
    /// Lines of every pending record.
    pending_lines: usize,
    /// Lines of the pushed pending records.
    outstanding_lines: usize,
    pushed: u64,
    seen_dropped: u64,
    seen_rejected: u64,
    tail_error: Option<String>,
    fatal: Option<String>,
    /// Highest sequence shown unconfirmed in the current dead period.
    unconfirmed_max: u64,
    unconfirmed_count: usize,
    holds_seen: BTreeSet<String>,
    last_disk_us: Option<u64>,
    lock_probe_error: Option<String>,
    status_error: Option<String>,
    warned_cursor_ahead: bool,
    /// The push queue dropped lines: push the pending records again once
    /// the queue is empty.
    rewind_wanted: bool,
}

/// Reads the instance id of a data directory (`INSTANCE`, 16 hex digits).
pub fn read_instance_id(dir: &std::path::Path) -> Result<u64, String> {
    let path = dir.join("INSTANCE");
    let text =
        std::fs::read_to_string(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
    u64::from_str_radix(text.trim(), 16).map_err(|e| format!("{}: {e}", path.display()))
}

impl Projector {
    pub fn new(
        s: Settings,
        status: Box<dyn StatusSource>,
        locks: Box<dyn LockProbe>,
        disk: Box<dyn FreeSpace>,
        obs: Option<ObsHandle>,
        now_us: u64,
    ) -> Result<Projector, String> {
        let instance_id = read_instance_id(&s.data_dir)?;
        let cursor = cursor::load(&s.cursor_file, instance_id)?;
        let tail = Tail::new(&s.data_dir, 0);
        Ok(Projector {
            instance_id,
            core: Core::new(instance_id),
            pnl: BTreeMap::new(),
            tail,
            status,
            locks,
            disk,
            obs,
            cursor,
            stored_cursor: cursor,
            emit_after: cursor,
            applied: 0,
            durable: 0,
            caught_up: false,
            up: false,
            last_answer_us: now_us,
            heartbeat_us: None,
            wire_id_form: None,
            pending: VecDeque::new(),
            pending_lines: 0,
            outstanding_lines: 0,
            pushed: 0,
            seen_dropped: 0,
            seen_rejected: 0,
            tail_error: None,
            fatal: None,
            unconfirmed_max: 0,
            unconfirmed_count: 0,
            holds_seen: BTreeSet::new(),
            last_disk_us: None,
            lock_probe_error: None,
            status_error: None,
            warned_cursor_ahead: false,
            rewind_wanted: false,
            s,
        })
    }

    pub fn core(&self) -> &Core {
        &self.core
    }

    pub fn cursor(&self) -> u64 {
        self.cursor
    }

    pub fn applied_seq(&self) -> u64 {
        self.applied
    }

    pub fn pnl(&self) -> &BTreeMap<String, Pnl> {
        &self.pnl
    }

    /// One pass. Call it every `poll_ms`.
    pub fn step(&mut self, now_us: u64) -> StepReport {
        let mut rep = StepReport::default();
        // P1: the durable sequence, from the OMS of this journal only.
        self.up = false;
        if let Some(st) = self.status.poll() {
            if st.instance_id == Some(self.instance_id) {
                self.status_error = None;
                self.up = true;
                self.last_answer_us = now_us;
                self.heartbeat_us = Some(st.as_of_us);
                self.durable = self.durable.max(st.durable_seq);
                if st.wire_id_form.is_some() {
                    self.wire_id_form = st.wire_id_form;
                }
                if st.durable_seq < self.cursor && !self.warned_cursor_ahead {
                    self.warned_cursor_ahead = true;
                    let mut f = serde_json::Map::new();
                    f.insert("cursor".into(), self.cursor.into());
                    f.insert("durable_seq".into(), st.durable_seq.into());
                    self.own_log(now_us, "warn", "the cursor is past the durable sequence of the journal; records up to the cursor are replayed without lines", f);
                }
            } else {
                // Another OMS (or none that says which): its durable
                // sequence says nothing about this journal.
                let text = format!(
                    "status_url answers for instance {}, the journal is instance {:016x}",
                    st.instance_id
                        .map_or("unknown".to_string(), |i| format!("{i:016x}")),
                    self.instance_id
                );
                if self.status_error.as_deref() != Some(text.as_str()) {
                    let mut f = serde_json::Map::new();
                    f.insert("error".into(), text.clone().into());
                    self.own_log(
                        now_us,
                        "error",
                        "status answer ignored: not the OMS of this journal",
                        f,
                    );
                }
                self.status_error = Some(text);
            }
        }
        let dead = !self.up
            && now_us.saturating_sub(self.last_answer_us) >= self.s.dead_after_us
            && self.lock_is_free();
        rep.dead = dead;
        // P2–P4: confirmed records.
        let cap = self.durable.max(self.emit_after);
        if self.fatal.is_none() {
            // Records an earlier run emitted: replayed without lines, as
            // many as fit in the budget.
            let started = std::time::Instant::now();
            while self.fatal.is_none()
                && self.tail.next_seq() <= self.emit_after
                && started.elapsed() < REPLAY_BUDGET
            {
                if !self.read_once(now_us, self.emit_after, &mut rep) {
                    break;
                }
            }
            if self.fatal.is_none()
                && self.tail.next_seq() > self.emit_after
                && self.pending_lines < self.s.max_pending_lines
            {
                self.read_once(now_us, cap, &mut rep);
            }
            if self.tail_error.is_none() && self.tail.next_seq() > cap {
                self.caught_up = true;
            }
        }
        // Past the durable sequence, only while the OMS is dead.
        if dead && self.fatal.is_none() && self.tail_error.is_none() && self.tail.next_seq() > cap {
            rep.unconfirmed_lines = self.show_unconfirmed();
        } else if !dead {
            self.unconfirmed_max = 0;
            self.unconfirmed_count = 0;
        }
        // P5: the cursor, then the next lines.
        self.confirm(now_us);
        self.send_pending();
        self.push_metrics(now_us);
        rep.cursor = self.cursor;
        rep.tail_error = self.tail_error.clone();
        rep.fatal = self.fatal.clone();
        rep
    }

    /// One `Tail::read` up to `cap`. `false` when it returned nothing or
    /// stopped on an error.
    fn read_once(&mut self, now_us: u64, cap: u64, rep: &mut StepReport) -> bool {
        match self.tail.read(cap, self.s.max_records) {
            Ok(recs) => {
                if self.tail_error.take().is_some() {
                    self.own_log(
                        now_us,
                        "info",
                        "journal tail readable again",
                        serde_json::Map::new(),
                    );
                }
                let any = !recs.is_empty();
                for tr in recs {
                    if let Err(e) = self.take(&tr, rep) {
                        self.fail(now_us, e);
                        return false;
                    }
                }
                any
            }
            Err(e) => {
                let text = e.to_string();
                if self.tail_error.as_deref() != Some(text.as_str()) {
                    let mut f = serde_json::Map::new();
                    f.insert("error".into(), text.clone().into());
                    f.insert("next_seq".into(), self.tail.next_seq().into());
                    self.own_log(now_us, "error", "journal tail stopped on damage at or below the durable sequence; the cursor stays", f);
                }
                self.tail_error = Some(text);
                false
            }
        }
    }

    fn lock_is_free(&mut self) -> bool {
        match self.locks.held() {
            Ok(held) => {
                self.lock_probe_error = None;
                !held
            }
            Err(e) => {
                // Cannot tell: the OMS counts as alive.
                let text = e.to_string();
                if self.lock_probe_error.as_deref() != Some(text.as_str()) {
                    eprintln!(
                        "{}",
                        serde_json::json!({"level": "warn", "msg": "lock probe failed; the OMS counts as alive", "error": text})
                    );
                }
                self.lock_probe_error = Some(text);
                false
            }
        }
    }

    fn parse(tr: &TailRecord) -> Result<Record, String> {
        if !READABLE_RECORD_SCHEMAS.contains(&tr.record_schema) {
            return Err(format!(
                "record {} has record schema {}, this build reads {:?}",
                tr.seq, tr.record_schema, READABLE_RECORD_SCHEMAS
            ));
        }
        let rec: Record = serde_json::from_slice(&tr.payload)
            .map_err(|e| format!("record {} does not parse: {e}", tr.seq))?;
        if rec.seq != tr.seq {
            return Err(format!("frame {} holds record {}", tr.seq, rec.seq));
        }
        Ok(rec)
    }

    fn infos(&self, rec: &Record) -> Vec<Option<OrderInfo>> {
        rec.events
            .iter()
            .map(|ev| match ev {
                Event::FillBooked { order_id, .. } => lines::order_info(&self.core, *order_id),
                _ => None,
            })
            .collect()
    }

    fn take(&mut self, tr: &TailRecord, rep: &mut StepReport) -> Result<(), String> {
        let rec = Self::parse(tr)?;
        let infos = self.infos(&rec);
        self.core
            .replay(&rec)
            .map_err(|e| format!("record {} does not apply: {}", rec.seq, e.0))?;
        self.book_pnl(&rec, &infos);
        self.applied = rec.seq;
        rep.applied += 1;
        if rec.seq <= self.emit_after {
            return Ok(());
        }
        let ctx = self.ctx();
        let out = lines::record_lines(&ctx, &rec, Some(&self.core), &infos, false);
        rep.lines += out.len();
        if self.s.logs_on {
            self.pending_lines += out.len();
            self.pending.push_back(Pending {
                seq: rec.seq,
                lines: out,
                end_mark: None,
            });
        } else {
            // No store: taken at once.
            self.pending.push_back(Pending {
                seq: rec.seq,
                lines: Vec::new(),
                end_mark: Some(0),
            });
        }
        Ok(())
    }

    fn ctx(&self) -> Ctx<'_> {
        Ctx {
            host: &self.s.host,
            venue: &self.s.venue,
            instance_id: self.instance_id,
            wire_id_form: self.wire_id_form,
        }
    }

    fn book_pnl(&mut self, rec: &Record, infos: &[Option<OrderInfo>]) {
        let f = |v: i64| v as f64 / oms_core::num::SCALE as f64;
        let signed = |side: Side, q: i64| match side {
            Side::Buy => i128::from(q),
            Side::Sell => -i128::from(q),
        };
        for (i, ev) in rec.events.iter().enumerate() {
            match ev {
                Event::FillBooked { qty, px, fee, .. } => {
                    if let Some(info) = infos.get(i).and_then(Option::as_ref) {
                        let p = self.pnl.entry(info.instrument.clone()).or_default();
                        p.trade(signed(info.side, qty.0), f(px.0), f(fee.0));
                    }
                }
                Event::ExternalFillBooked { fill, .. } => {
                    let p = self.pnl.entry(fill.instrument.to_string()).or_default();
                    p.trade(signed(fill.side, fill.qty.0), f(fill.px.0), f(fill.fee.0));
                }
                Event::VenuePositionChanged { change } => {
                    let p = self.pnl.entry(change.instrument.to_string()).or_default();
                    p.trade(i128::from(change.qty.0), f(change.px.0), 0.0);
                }
                Event::PositionAdjusted {
                    instrument, delta, ..
                } => {
                    self.pnl
                        .entry(instrument.to_string())
                        .or_default()
                        .shift(i128::from(delta.0));
                }
                Event::FundingBooked { funding } => {
                    self.pnl
                        .entry(funding.instrument.to_string())
                        .or_default()
                        .funding(f(funding.amount.0));
                }
                _ => {}
            }
        }
    }

    fn show_unconfirmed(&mut self) -> usize {
        let peeked = match self.tail.peek_beyond(self.s.max_records) {
            Ok(p) => p,
            Err(_) => return 0,
        };
        self.unconfirmed_count = peeked.len();
        let mut n = 0;
        for tr in peeked {
            if tr.seq <= self.unconfirmed_max {
                continue;
            }
            let Ok(rec) = Self::parse(&tr) else { break };
            let infos = self.infos(&rec);
            let ctx = self.ctx();
            let out = lines::record_lines(&ctx, &rec, None, &infos, true);
            n += out.len();
            if self.s.logs_on {
                for l in out {
                    self.push_line(l);
                }
            }
            self.unconfirmed_max = rec.seq;
        }
        n
    }

    fn push_line(&mut self, l: LogLine) {
        if let Some(obs) = &self.obs {
            obs.log(l);
            self.pushed += 1;
        }
    }

    fn own_log(
        &mut self,
        now_us: u64,
        level: &str,
        msg: &str,
        mut fields: serde_json::Map<String, serde_json::Value>,
    ) {
        fields.insert("level".into(), level.into());
        let mut line = serde_json::Map::new();
        line.insert("msg".into(), msg.into());
        line.extend(fields.clone());
        eprintln!("{}", serde_json::Value::Object(line));
        if self.s.logs_on {
            self.push_line(LogLine {
                time_ns: i64::try_from(now_us.saturating_mul(1000)).unwrap_or(i64::MAX),
                msg: msg.to_string(),
                service: "oms-projector".into(),
                host: self.s.host.clone(),
                venue: self.s.venue.clone(),
                fields,
            });
        }
    }

    fn fail(&mut self, now_us: u64, e: String) {
        let mut f = serde_json::Map::new();
        f.insert("error".into(), e.clone().into());
        self.own_log(now_us, "error", "projector stopped: a durable record cannot be replayed; restart after fixing the build", f);
        self.fatal = Some(e);
    }

    fn confirm(&mut self, now_us: u64) {
        let processed = match (&self.obs, self.s.logs_on) {
            (Some(obs), true) => {
                let st = obs.state().vl;
                if st.rejected > self.seen_rejected {
                    // Refused for good (400, 413, 422): sending again gets
                    // the same answer. Counted as processed, so the cursor
                    // passes them; the loss is logged and counted.
                    let mut f = serde_json::Map::new();
                    f.insert(
                        "rejected_lines".into(),
                        (st.rejected - self.seen_rejected).into(),
                    );
                    f.insert("rejected_total".into(), st.rejected.into());
                    self.seen_rejected = st.rejected;
                    self.own_log(
                        now_us,
                        "error",
                        "the log store refused lines for good; they are not sent again",
                        f,
                    );
                }
                if st.dropped > self.seen_dropped {
                    // A line the cursor waits for may be among the dropped.
                    // Which ones is not known: nothing is taken until the
                    // queue is empty, then every pending record is pushed
                    // again (duplicates are told apart by seq).
                    self.seen_dropped = st.dropped;
                    self.rewind_wanted = true;
                }
                if self.rewind_wanted {
                    if st.queued == 0 && st.in_flight == 0 {
                        self.rewind(now_us);
                    }
                    return;
                }
                st.sent + st.rejected + st.dropped
            }
            _ => u64::MAX,
        };
        while self
            .pending
            .front()
            .is_some_and(|p| p.end_mark.is_some_and(|m| m <= processed))
        {
            if let Some(p) = self.pending.pop_front() {
                self.pending_lines -= p.lines.len();
                self.outstanding_lines -= p.lines.len();
                self.cursor = p.seq;
            }
        }
        if self.cursor != self.stored_cursor {
            match cursor::store(&self.s.cursor_file, self.instance_id, self.cursor) {
                Ok(()) => self.stored_cursor = self.cursor,
                Err(e) => {
                    let mut f = serde_json::Map::new();
                    f.insert("error".into(), e.into());
                    self.own_log(
                        now_us,
                        "error",
                        "cursor file not written; retried next step",
                        f,
                    );
                }
            }
        }
    }

    /// Pushes waiting records in order while the pushed-but-not-taken lines
    /// stay within `max_pending_lines` (one record alone may pass it). So
    /// the push queue, which holds more than that, does not drop them.
    fn send_pending(&mut self) {
        if self.rewind_wanted || !self.s.logs_on {
            return;
        }
        let mut i = self
            .pending
            .iter()
            .position(|p| p.end_mark.is_none())
            .unwrap_or(self.pending.len());
        while i < self.pending.len() {
            let n = self.pending[i].lines.len();
            if self.outstanding_lines > 0 && self.outstanding_lines + n > self.s.max_pending_lines {
                break;
            }
            let lines = self.pending[i].lines.clone();
            for l in lines {
                self.push_line(l);
            }
            self.pending[i].end_mark = Some(self.pushed);
            self.outstanding_lines += n;
            i += 1;
        }
    }

    /// The queue dropped lines and is now empty: every pending record is
    /// pushed again, within the same limit as the first time.
    fn rewind(&mut self, now_us: u64) {
        for p in self.pending.iter_mut() {
            p.end_mark = None;
        }
        self.outstanding_lines = 0;
        self.rewind_wanted = false;
        let mut f = serde_json::Map::new();
        f.insert("cursor".into(), self.cursor.into());
        f.insert("pending_records".into(), (self.pending.len() as u64).into());
        self.own_log(
            now_us,
            "warn",
            "the push queue dropped log lines; pushing the records after the cursor again",
            f,
        );
    }

    fn push_metrics(&mut self, now_us: u64) {
        let holds = if self.caught_up {
            metrics::holds_now(&self.core)
        } else {
            BTreeSet::new()
        };
        let mut out = metrics::samples(&View {
            core: &self.core,
            pnl: &self.pnl,
            venue: &self.s.venue,
            now_us,
            up: self.up,
            heartbeat_us: self.heartbeat_us,
            durable_seq: self.durable,
            applied_seq: self.applied,
            cursor: self.cursor,
            pending_lines: self.pending_lines,
            unconfirmed: self.unconfirmed_count,
            tail_error: self.tail_error.is_some() || self.fatal.is_some(),
            holds_seen: &self.holds_seen,
            caught_up: self.caught_up,
            rejected_lines: self.seen_rejected,
        });
        self.holds_seen.extend(holds);
        if self
            .last_disk_us
            .is_none_or(|t| now_us.saturating_sub(t) >= self.s.disk_every_us)
        {
            self.last_disk_us = Some(now_us);
            match self.disk.free_bytes() {
                Ok(bytes) => out.push(Sample {
                    name: "journal_disk_free_bytes".into(),
                    labels: BTreeMap::from([("venue".to_string(), self.s.venue.clone())]),
                    value: bytes as f64,
                    ts_ms: i64::try_from(now_us / 1000).unwrap_or(i64::MAX),
                }),
                Err(e) => eprintln!(
                    "{}",
                    serde_json::json!({"level": "warn", "msg": "statvfs of the journal failed", "error": e.to_string()})
                ),
            }
        }
        if let Some(obs) = &self.obs {
            obs.metrics(out);
        }
    }

    /// Whether the order of `id` is known to the replayed state.
    pub fn knows_order(&self, id: u64) -> bool {
        self.core.order(OrderId(id)).is_some()
    }
}
