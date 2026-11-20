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
    /// Lines pushed in this run when the last line of this record was
    /// pushed. The record is taken when the store has processed that many.
    end_mark: u64,
}

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
    up: bool,
    last_answer_us: u64,
    heartbeat_us: Option<u64>,
    wire_id_form: Option<WireIdForm>,
    pending: VecDeque<Pending>,
    pending_lines: usize,
    pushed: u64,
    seen_dropped: u64,
    tail_error: Option<String>,
    fatal: Option<String>,
    /// Highest sequence shown unconfirmed in the current dead period.
    unconfirmed_max: u64,
    unconfirmed_count: usize,
    holds_seen: BTreeSet<String>,
    last_disk_us: Option<u64>,
    lock_probe_error: Option<String>,
    /// The push queue dropped lines: rewind to the cursor once it is empty.
    rewind_wanted: bool,
    /// After a rewind, records up to this one are emitted one at a time,
    /// each after the store took the one before, so the queue cannot
    /// overflow again on the same records.
    careful_until: u64,
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
            up: false,
            last_answer_us: now_us,
            heartbeat_us: None,
            wire_id_form: None,
            pending: VecDeque::new(),
            pending_lines: 0,
            pushed: 0,
            seen_dropped: 0,
            tail_error: None,
            fatal: None,
            unconfirmed_max: 0,
            unconfirmed_count: 0,
            holds_seen: BTreeSet::new(),
            last_disk_us: None,
            lock_probe_error: None,
            rewind_wanted: false,
            careful_until: 0,
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
        // P1: the durable sequence.
        match self.status.poll() {
            Some(st) => {
                self.up = true;
                self.last_answer_us = now_us;
                self.heartbeat_us = Some(st.as_of_us);
                self.durable = self.durable.max(st.durable_seq);
                if st.wire_id_form.is_some() {
                    self.wire_id_form = st.wire_id_form;
                }
            }
            None => self.up = false,
        }
        let dead = !self.up
            && now_us.saturating_sub(self.last_answer_us) >= self.s.dead_after_us
            && self.lock_is_free();
        rep.dead = dead;
        // P2–P4: confirmed records.
        let cap = self.durable.max(self.emit_after);
        let next = self.tail.next_seq();
        let careful = next > self.emit_after && next <= self.careful_until;
        let (read_cap, read_max, may_read) = if next <= self.emit_after {
            // Replay of what an earlier pass emitted: no lines.
            (self.emit_after, self.s.max_records, true)
        } else if careful {
            (cap, 1, self.pending_lines == 0)
        } else {
            (
                cap,
                self.s.max_records,
                self.pending_lines < self.s.max_pending_lines,
            )
        };
        if self.fatal.is_none() && !self.rewind_wanted && may_read {
            match self.tail.read(read_cap, read_max) {
                Ok(recs) => {
                    if self.tail_error.take().is_some() {
                        self.own_log(
                            now_us,
                            "info",
                            "journal tail readable again",
                            serde_json::Map::new(),
                        );
                    }
                    for tr in recs {
                        if let Err(e) = self.take(&tr, &mut rep) {
                            self.fail(now_us, e);
                            break;
                        }
                    }
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
                }
            }
        }
        // Past the durable sequence, only while the OMS is dead.
        if dead && self.fatal.is_none() && self.tail_error.is_none() && self.tail.next_seq() > cap {
            rep.unconfirmed_lines = self.show_unconfirmed();
        } else if !dead {
            self.unconfirmed_max = 0;
            self.unconfirmed_count = 0;
        }
        // P5: the cursor.
        self.confirm(now_us);
        self.push_metrics(now_us);
        rep.cursor = self.cursor;
        rep.tail_error = self.tail_error.clone();
        rep.fatal = self.fatal.clone();
        rep
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
            for l in &out {
                self.push_line(l.clone());
            }
            self.pending_lines += out.len();
            self.pending.push_back(Pending {
                seq: rec.seq,
                lines: out,
                end_mark: self.pushed,
            });
        } else {
            self.pending.push_back(Pending {
                seq: rec.seq,
                lines: Vec::new(),
                end_mark: 0,
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
                if st.dropped > self.seen_dropped {
                    // A line the cursor waits for may be among the dropped.
                    // Which ones is not known: nothing is taken until the
                    // queue is empty, then everything after the cursor is
                    // emitted again (duplicates are told apart by seq).
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
            .is_some_and(|p| p.end_mark <= processed)
        {
            if let Some(p) = self.pending.pop_front() {
                self.pending_lines -= p.lines.len();
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

    /// Starts over from the first record, emitting after the cursor, as a
    /// restart does. Records that were pending are then emitted one at a
    /// time.
    fn rewind(&mut self, now_us: u64) {
        let last_pending = self.pending.back().map_or(self.applied, |p| p.seq);
        self.careful_until = self.careful_until.max(last_pending).max(self.applied);
        self.core = Core::new(self.instance_id);
        self.pnl.clear();
        self.tail = Tail::new(&self.s.data_dir, 0);
        self.emit_after = self.cursor;
        self.applied = 0;
        self.pending.clear();
        self.pending_lines = 0;
        self.rewind_wanted = false;
        let mut f = serde_json::Map::new();
        f.insert("cursor".into(), self.cursor.into());
        f.insert("careful_until".into(), self.careful_until.into());
        self.own_log(
            now_us,
            "warn",
            "the push queue dropped log lines; emitting again from the cursor",
            f,
        );
    }

    fn push_metrics(&mut self, now_us: u64) {
        let holds = metrics::holds_now(&self.core);
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
