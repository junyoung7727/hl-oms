//! Receive → write (spec D4). The sink runs on md-feed's tasks and must not
//! block: it routes each frame, counts it against its connection's in-flight
//! bytes and hands it to one writer thread over a channel. A connection over
//! its byte budget is refused (`Overflow`: md-feed closes and reconnects it)
//! after one `overflow` note; nothing is dropped without a note.
//!
//! The writer thread owns the stage-1 files: one md-lake `OpenWriter` per
//! `conn_id` (`0` = the REST pseudo-connection) and the session file. It
//! writes `clock_step` notes, closes a connection's file on its close marker,
//! and ticks every writer (block flush, fsync, hour and date rotation).

use std::collections::{BTreeMap, BTreeSet};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use md_core::StreamKey;
use md_core::adapter::Route;
use md_feed::{FrameSink, Overflow, RawFrame, Src};
use md_lake::fs::Fs;
use md_lake::qraw::{self, Record};
use md_lake::writer::{OpenWriter, SessionWriter, WriterSettings};
use md_lake::zst::Contexts;
use serde_json::json;

use crate::notes::{self, Note};
use crate::notify::Notifier;
use crate::obs::LogSink;
use crate::telemetry::Telemetry;
use crate::registry::{self, Venue};

/// How often the writer ticks its files.
const TICK: Duration = Duration::from_millis(100);
/// How often the writer beats (heartbeat value and `WATCHDOG=1`, U6 D5).
const BEAT: Duration = Duration::from_secs(1);

/// `ENOSPC`, `EIO`, `EDQUOT`, ... for the alert text (U6 R7); else the kind.
pub fn os_error_name(e: &std::io::Error) -> String {
    match e.raw_os_error() {
        Some(28) => "ENOSPC".into(),
        Some(5) => "EIO".into(),
        Some(122) => "EDQUOT".into(),
        Some(30) => "EROFS".into(),
        Some(n) => format!("errno {n}"),
        None => format!("{:?}", e.kind()),
    }
}

/// Where a frame goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Dest {
    /// The open file of this `conn_id`.
    Open(u32),
    Session,
}

enum Item {
    /// `slot`: the plan slot the bytes are counted against (`None`: past the cap).
    Frame {
        dest: Dest,
        rec: Record,
        slot: Option<u32>,
        bytes: usize,
    },
    Note(Record),
    Close(u32),
    Stop,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConnSnapshot {
    /// Frames the sink took (each is written, or the writer failed).
    pub frames_accepted: u64,
    pub frames_written: u64,
    pub bytes_written: u64,
    pub overflows: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IngestSnapshot {
    pub conns: BTreeMap<u32, ConnSnapshot>,
    /// In-flight bytes per plan slot, and in total.
    pub slot_bytes: BTreeMap<u32, usize>,
    pub total_bytes: usize,
    pub clock_steps: u64,
    /// Frames of an already closed connection, kept in the session file.
    pub late_frames: u64,
    pub notes: u64,
    /// The writer stopped on an error (the collector must exit).
    pub failed: Option<String>,
    /// Its OS error name (`ENOSPC`, `EIO`, ...).
    pub failed_os_error: Option<String>,
    /// Unix seconds of the writer's last beat (`md_collector_heartbeat_ts`).
    pub heartbeat_s: Option<i64>,
    pub beats: u64,
    /// Open stage-1 writers (`lake_l0_open_files`).
    pub open_files: usize,
}

/// The receive queue budgets (spec D4, R1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Bytes one plan slot may have between receive and write.
    pub queue_bytes: usize,
    /// Bytes all slots together may have.
    pub total_bytes: usize,
}

#[derive(Default)]
struct Shared {
    /// In-flight bytes per plan slot: a reconnect inherits its slot's backlog.
    slots: BTreeMap<u32, usize>,
    total: usize,
    conns: BTreeMap<u32, ConnStats>,
    clock_steps: u64,
    late_frames: u64,
    notes: u64,
    failed: Option<String>,
    failed_os_error: Option<String>,
    heartbeat_s: Option<i64>,
    beats: u64,
    open_files: usize,
}

#[derive(Default)]
struct ConnStats {
    overflowed: bool,
    accepted: u64,
    frames: u64,
    bytes: u64,
    overflows: u64,
}

/// The collector's frame sink.
pub struct Ingest {
    tx: Mutex<mpsc::Sender<Item>>,
    venue: &'static dyn Venue,
    writer: String,
    limits: Limits,
    shared: Arc<Mutex<Shared>>,
    telemetry: Option<Arc<Telemetry>>,
}

/// The writer thread.
pub struct WriterThread {
    tx: mpsc::Sender<Item>,
    join: Option<JoinHandle<()>>,
    shared: Arc<Mutex<Shared>>,
}

/// Starts the writer thread.
pub fn start(
    fs: Arc<dyn Fs>,
    settings: WriterSettings,
    venue: &'static dyn Venue,
    limits: Limits,
) -> Result<(Arc<Ingest>, WriterThread), String> {
    start_with(fs, settings, venue, limits, Hooks::default())
}

/// What the ingest reports to (U6): the systemd notifier the writer beats
/// to, the log sink and the telemetry model.
#[derive(Default, Clone)]
pub struct Hooks {
    pub notifier: Option<Arc<dyn Notifier>>,
    pub log: Option<Arc<LogSink>>,
    pub telemetry: Option<Arc<Telemetry>>,
}

/// As [`start`], with the U6 hooks.
pub fn start_with(
    fs: Arc<dyn Fs>,
    settings: WriterSettings,
    venue: &'static dyn Venue,
    limits: Limits,
    hooks: Hooks,
) -> Result<(Arc<Ingest>, WriterThread), String> {
    let notifier = hooks.notifier.clone();
    let log = hooks
        .log
        .clone()
        .unwrap_or_else(|| Arc::new(LogSink::stderr_only(&settings.venue)));
    let (tx, rx) = mpsc::channel();
    let shared: Arc<Mutex<Shared>> = Arc::default();
    let writer = settings.collector.clone();
    let w = Writer {
        fs: fs.clone(),
        settings: settings.clone(),
        contexts: Contexts::default(),
        open: BTreeMap::new(),
        closed: BTreeSet::new(),
        last_recv: BTreeMap::new(),
        session: SessionWriter::new(fs, settings, &Contexts::default()),
        shared: shared.clone(),
        notifier,
        log: log.clone(),
    };
    let join = std::thread::Builder::new()
        .name("md-writer".into())
        .spawn(move || w.run(rx))
        .map_err(|e| format!("cannot start the writer thread: {e}"))?;
    let ingest = Arc::new(Ingest {
        tx: Mutex::new(tx.clone()),
        venue,
        writer,
        limits,
        shared: shared.clone(),
        telemetry: hooks.telemetry,
    });
    Ok((
        ingest,
        WriterThread {
            tx,
            join: Some(join),
            shared,
        },
    ))
}

fn snapshot(shared: &Mutex<Shared>) -> IngestSnapshot {
    let Ok(s) = shared.lock() else {
        return IngestSnapshot::default();
    };
    IngestSnapshot {
        conns: s
            .conns
            .iter()
            .map(|(id, c)| {
                (
                    *id,
                    ConnSnapshot {
                        frames_accepted: c.accepted,
                        frames_written: c.frames,
                        bytes_written: c.bytes,
                        overflows: c.overflows,
                    },
                )
            })
            .collect(),
        slot_bytes: s.slots.clone(),
        total_bytes: s.total,
        clock_steps: s.clock_steps,
        late_frames: s.late_frames,
        notes: s.notes,
        failed: s.failed.clone(),
        failed_os_error: s.failed_os_error.clone(),
        heartbeat_s: s.heartbeat_s,
        beats: s.beats,
        open_files: s.open_files,
    }
}

impl Ingest {
    fn send(&self, item: Item) -> bool {
        match self.tx.lock() {
            Ok(tx) => tx.send(item).is_ok(),
            Err(_) => false,
        }
    }

    fn dest(&self, f: &RawFrame) -> Dest {
        self.dest_topic(f).0
    }

    /// Where a frame goes, and its stream topic with its market (U6 R7:
    /// WebSocket frames carry no topic; the route names it).
    fn dest_topic(&self, f: &RawFrame) -> (Dest, Option<(&'static str, StreamKey)>) {
        match f.src {
            Src::Ws => {
                let slot = usize::try_from(registry::slot_of_conn_id(f.conn_id)).ok();
                let market = slot.and_then(|s| self.venue.markets().get(s)).copied();
                match market.map(|m| (m, self.venue.route(m, &f.payload))) {
                    Some((m, Route::Stream(k))) => (Dest::Open(f.conn_id), Some((m, k))),
                    _ => (Dest::Session, None),
                }
            }
            // A REST body that names a market of the venue is market data
            // (the instruments topic, D1); any other body is control.
            Src::Rest => match self.venue.rest_market(&f.payload) {
                Some(m) if self.venue.market_slot(&m).is_some() => (Dest::Open(0), None),
                _ => (Dest::Session, None),
            },
        }
    }

    /// Writes a note to the session file (never refused).
    pub fn note(&self, n: &Note<'_>) {
        let rec = notes::record(&self.writer, n, crate::wall_ns());
        self.send(Item::Note(rec));
    }

    /// A REST body the collector fetched itself (the instrument list): never
    /// refused (spec R6), routed as any REST body.
    pub fn rest_body(&self, recv_ns: i64, frame_no: u64, body: Vec<u8>) {
        let f = RawFrame {
            recv_ns,
            src: Src::Rest,
            conn_id: 0,
            frame_no,
            conn_index: 0,
            topic: None,
            for_conn_id: None,
            payload: body,
        };
        let dest = self.dest(&f);
        if let Ok(mut s) = self.shared.lock() {
            s.conns.entry(0).or_default().accepted += 1;
        }
        self.send(Item::Frame {
            dest,
            rec: record(f),
            slot: None,
            bytes: 0,
        });
    }

    pub fn state(&self) -> IngestSnapshot {
        snapshot(&self.shared)
    }
}

fn record(f: RawFrame) -> Record {
    Record {
        recv_ns: f.recv_ns,
        src: match f.src {
            Src::Ws => qraw::Src::WebSocket,
            Src::Rest => qraw::Src::Rest,
        },
        conn_id: f.conn_id,
        frame_no: f.frame_no,
        payload: f.payload,
    }
}

impl FrameSink for Ingest {
    fn frame(&self, f: RawFrame) -> Result<(), Overflow> {
        let (dest, topic) = self.dest_topic(&f);
        let bytes = qraw::FIXED_LEN + f.payload.len();
        let slot = f.conn_index;
        {
            let Ok(mut s) = self.shared.lock() else {
                return Err(Overflow);
            };
            let in_slot = s.slots.get(&slot).copied().unwrap_or(0);
            // A frame larger than a slot's whole budget is taken when the slot
            // is empty, so that it cannot make the connection loop
            // (judgement); the total cap holds for every frame.
            let full = (in_slot > 0 && in_slot + bytes > self.limits.queue_bytes)
                || s.total + bytes > self.limits.total_bytes;
            let c = s.conns.entry(f.conn_id).or_default();
            if full || (c.overflowed && f.src == Src::Ws) {
                // One note per refused connection; a refused REST body (the
                // pseudo-connection 0 never closes) gets one each time.
                let first = !c.overflowed || f.src == Src::Rest;
                if first {
                    c.overflows += 1;
                    if f.src == Src::Ws {
                        c.overflowed = true;
                    }
                    drop(s);
                    self.note(&Note {
                        note: "overflow",
                        conn_id: Some(f.conn_id),
                        topics: None,
                        start_recv_ns: f.recv_ns,
                        end_recv_ns: None,
                        detail: json!({"text": "receive queue full"}),
                    });
                }
                return Err(Overflow);
            }
            c.accepted += 1;
            *s.slots.entry(slot).or_default() += bytes;
            s.total += bytes;
        }
        if let (Some(t), Some((market, key))) = (&self.telemetry, &topic) {
            t.frame(market, key, slot, f.payload.len(), f.recv_ns);
        }
        if self.send(Item::Frame {
            dest,
            rec: record(f),
            slot: Some(slot),
            bytes,
        }) {
            Ok(())
        } else {
            // The writer has stopped: the process is ending.
            Err(Overflow)
        }
    }

    /// Called by the connection's own task after its last frame (md-feed),
    /// so the close marker follows its frames (spec R5).
    fn conn_ended(&self, conn_id: u32) {
        if conn_id != 0 {
            self.send(Item::Close(conn_id));
        }
    }
}

impl WriterThread {
    /// Closes every file (frame end, fsync, unlock) and joins the thread.
    pub fn stop(mut self) -> Result<(), String> {
        let _ = self.tx.send(Item::Stop);
        if let Some(j) = self.join.take() {
            j.join()
                .map_err(|_| "the writer thread panicked".to_string())?;
        }
        match snapshot(&self.shared).failed {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }

    /// The writer's error, if it stopped on one.
    pub fn failed(&self) -> Option<String> {
        snapshot(&self.shared).failed
    }
}

struct Writer {
    fs: Arc<dyn Fs>,
    settings: WriterSettings,
    contexts: Contexts,
    open: BTreeMap<u32, OpenWriter>,
    closed: BTreeSet<u32>,
    last_recv: BTreeMap<u32, i64>,
    session: SessionWriter,
    shared: Arc<Mutex<Shared>>,
    notifier: Option<Arc<dyn Notifier>>,
    log: Arc<LogSink>,
}

impl Writer {
    fn run(mut self, rx: mpsc::Receiver<Item>) {
        let mut last_tick = Instant::now();
        let mut last_beat = Instant::now();
        loop {
            let item = match rx.recv_timeout(TICK) {
                Ok(i) => Some(i),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => Some(Item::Stop),
            };
            let stop = matches!(item, Some(Item::Stop));
            let r = match item {
                Some(Item::Stop) => self.close_all(),
                Some(i) => self.handle(i),
                None => Ok(()),
            };
            let mut ticked = false;
            let r = r.and_then(|()| {
                if !stop && last_tick.elapsed() >= TICK {
                    last_tick = Instant::now();
                    ticked = true;
                    self.tick()
                } else {
                    Ok(())
                }
            });
            if let Err(e) = r {
                // Files are left as they are (no frame end): md-lake seal
                // cuts them back and notes it (crash_truncate). No beat
                // follows (U6 D5): the thread ends here.
                let os = os_error_name(&e);
                self.log.log(
                    "error",
                    "writer failed",
                    json!({"error": e.to_string(), "os_error": os}),
                );
                if let Ok(mut s) = self.shared.lock() {
                    s.failed = Some(e.to_string());
                    s.failed_os_error = Some(os);
                }
                return;
            }
            if ticked && !stop && last_beat.elapsed() >= BEAT {
                last_beat = Instant::now();
                self.beat();
            }
            if stop {
                return;
            }
        }
    }

    fn handle(&mut self, item: Item) -> std::io::Result<()> {
        match item {
            Item::Frame {
                dest,
                rec,
                slot,
                bytes,
            } => {
                let conn_id = rec.conn_id;
                let len = rec.payload.len() as u64;
                let r = self.frame(dest, rec);
                if let Ok(mut s) = self.shared.lock() {
                    if let Some(slot) = slot {
                        if let Some(b) = s.slots.get_mut(&slot) {
                            *b = b.saturating_sub(bytes);
                        }
                        s.total = s.total.saturating_sub(bytes);
                    }
                    let c = s.conns.entry(conn_id).or_default();
                    if r.is_ok() {
                        c.frames += 1;
                        c.bytes += len;
                    }
                }
                r
            }
            Item::Note(rec) => {
                if let Ok(mut s) = self.shared.lock() {
                    s.notes += 1;
                }
                self.session.append(&rec)
            }
            Item::Close(0) => Ok(()),
            Item::Close(conn_id) => {
                self.closed.insert(conn_id);
                self.last_recv.remove(&conn_id);
                match self.open.remove(&conn_id) {
                    Some(w) => w.close(),
                    None => Ok(()),
                }
            }
            Item::Stop => self.close_all(),
        }
    }

    fn frame(&mut self, dest: Dest, rec: Record) -> std::io::Result<()> {
        if rec.src == qraw::Src::WebSocket && rec.conn_id > 0 {
            if let Some(&prev) = self.last_recv.get(&rec.conn_id) {
                if rec.recv_ns < prev {
                    let n = notes::record(
                        &self.settings.collector,
                        &Note {
                            note: "clock_step",
                            conn_id: Some(rec.conn_id),
                            topics: None,
                            start_recv_ns: rec.recv_ns,
                            end_recv_ns: None,
                            detail: json!({"back_ns": prev - rec.recv_ns}),
                        },
                        crate::wall_ns(),
                    );
                    if let Ok(mut s) = self.shared.lock() {
                        s.clock_steps += 1;
                        s.notes += 1;
                    }
                    self.session.append(&n)?;
                }
            }
            self.last_recv.insert(rec.conn_id, rec.recv_ns);
        }
        match dest {
            Dest::Open(c) if self.closed.contains(&c) => {
                // Kept, never dropped: an open file once closed is not
                // reopened (the sealer may hold it).
                self.log
                    .log("warn", "frame of a closed connection", json!({"conn_id": c}));
                if let Ok(mut s) = self.shared.lock() {
                    s.late_frames += 1;
                }
                self.session.append(&rec)
            }
            Dest::Open(c) => {
                let (fs, settings, contexts) = (&self.fs, &self.settings, &self.contexts);
                let w = self
                    .open
                    .entry(c)
                    .or_insert_with(|| OpenWriter::new(fs.clone(), settings.clone(), c, contexts));
                // Never a file in an hour that has passed (spec R2).
                w.set_min_hour(crate::wall_ns());
                w.append(&rec)
            }
            Dest::Session => self.session.append(&rec),
        }
    }

    /// A beat (U6 D5): only after a tick in which every writer's tick
    /// returned `Ok`, at most once per second, also without frames.
    fn beat(&mut self) {
        let now_s = crate::wall_ns() / 1_000_000_000;
        if let Ok(mut s) = self.shared.lock() {
            s.heartbeat_s = Some(now_s);
            s.beats += 1;
        }
        if let Some(n) = &self.notifier {
            n.notify("WATCHDOG=1");
        }
    }

    fn tick(&mut self) -> std::io::Result<()> {
        let now = crate::wall_ns();
        for w in self.open.values_mut() {
            w.tick(now)?;
        }
        if let Ok(mut s) = self.shared.lock() {
            s.open_files = self.open.len();
        }
        self.session.tick(now)
    }

    fn close_all(&mut self) -> std::io::Result<()> {
        let mut first_err = None;
        for (_, w) in std::mem::take(&mut self.open) {
            if let Err(e) = w.close() {
                first_err.get_or_insert(e);
            }
        }
        let session = std::mem::replace(
            &mut self.session,
            SessionWriter::new(self.fs.clone(), self.settings.clone(), &self.contexts),
        );
        if let Err(e) = session.close() {
            first_err.get_or_insert(e);
        }
        match first_err {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }
}
