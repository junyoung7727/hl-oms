//! The core thread and the commit stage.
//!
//! ```text
//! input queues ──► core thread ──► commit stage ──► replies, gateway, read model
//!                  (decide+apply)   (write, flush,
//!                                    then release)
//! ```
//!
//! The core thread is the only writer of the domain state. It takes a batch
//! of inputs, decides each one, and hands the resulting records and outputs to
//! the commit stage. It performs no I/O.
//!
//! The commit stage writes the batch's records to the journal and flushes
//! them. Only after the flush has succeeded does it release anything: it
//! updates the read model, sends replies and hands commands to the gateway.
//! An output is therefore never visible outside this process unless the
//! record that caused it is durable. This ordering is contract C-COMMIT.
//!
//! The in-memory state of the core runs ahead of the journal by the batches
//! in flight. If the flush fails, or the process dies, that lead is lost
//! together with the outputs that depended on it; none of them was released.

use crate::published::{NoopEntry, Published, TraceEntry};
use crate::queues::{CoreMsg, InputQueues};
use crate::util::{Fatal, wall_us};
use crate::{error, info};
use oms_core::*;
use oms_journal::{SnapshotInfo, Writer};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::thread::JoinHandle;
use std::time::Instant;
use tokio::sync::{mpsc, oneshot};

/// Version of the snapshot payload written by this build.
pub const STATE_SCHEMA: u16 = 1;

/// Snapshot payload.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct SnapshotState {
    pub core: Core,
    /// Record time of the last record applied.
    pub last_at: Ts,
}

/// What the core thread hands over for one input.
pub struct Item {
    pub input: Option<Input>,
    pub at: Ts,
    /// The record and its encoding, if the input produced events.
    pub record: Option<(Record, Vec<u8>)>,
    pub commands: Vec<VenueCommand>,
    pub reply: Option<(oneshot::Sender<Reply>, Reply)>,
    pub enqueued: Instant,
    pub core_start: Instant,
    pub core_end: Instant,
}

pub struct Batch {
    pub items: Vec<Item>,
}

pub struct Shared {
    pub queues: Arc<InputQueues>,
    pub published: Arc<Published>,
    pub fatal: Arc<Fatal>,
    /// Set to ask the commit stage for a snapshot at its next commit.
    pub snapshot_requested: AtomicBool,
    /// Highest durable sequence, for cheap polling.
    pub durable_seq: AtomicU64,
    /// Makes the next journal flush fail. Exists only in builds with the
    /// `failpoints` feature, which the shipped binary does not enable.
    #[cfg(feature = "failpoints")]
    pub fail_next_sync: AtomicBool,
}

/// Test-only: the commit boundary named in `OMS_CRASH_AT`, if this batch
/// holds a newly accepted order. Exists only with the `failpoints` feature,
/// which the shipped binary does not enable.
#[cfg(feature = "failpoints")]
fn crash_point(batch: &Batch) -> Option<String> {
    let point = std::env::var("OMS_CRASH_AT").ok()?;
    batch
        .items
        .iter()
        .any(|i| !i.commands.is_empty() && i.record.as_ref().is_some_and(|(r, _)| matches!(r.input, Input::NewOrder(_))))
        .then_some(point)
}

#[cfg(feature = "failpoints")]
fn crash_now(point: &str) -> ! {
    eprintln!("{}", serde_json::json!({"level": "warn", "msg": "failpoint: aborting", "fields": {"at": point}}));
    std::process::abort()
}

pub struct Committer {
    writer: Writer,
    shared: Arc<Shared>,
    commands: mpsc::Sender<VenueCommand>,
    snapshot_tx: SyncSender<(u64, Ts, Core)>,
    snapshot_every: u64,
    last_snapshot_seq: u64,
    last_at: Ts,
}

impl Committer {
    /// Marks the process as stopped and closes the input queues. Called at
    /// the point of failure, while the batch is still held: a requester whose
    /// reply channel is about to be dropped must not be able to observe the
    /// failure before the stop is visible.
    fn fail(&self, why: String) -> String {
        self.shared.fatal.trip(format!("commit failed: {why}"));
        self.shared.queues.close();
        why
    }

    /// Writes, flushes, then releases. On error nothing of the batch has been
    /// released, the stop is already visible, and the caller must stop.
    pub fn commit(&mut self, batch: Batch) -> Result<(), String> {
        #[cfg(feature = "failpoints")]
        let crash = crash_point(&batch);
        #[cfg(feature = "failpoints")]
        if crash.as_deref() == Some("before_write") {
            crash_now("before_write");
        }
        let mut records = 0u64;
        let mut bytes = 0u64;
        for item in &batch.items {
            if let Some((rec, encoded)) = &item.record {
                if let Err(e) = self.writer.append(rec.seq, encoded) {
                    return Err(self.fail(e.to_string()));
                }
                records += 1;
                bytes += encoded.len() as u64;
            }
        }
        let sync_started = Instant::now();
        #[cfg(feature = "failpoints")]
        if self.shared.fail_next_sync.swap(false, Ordering::SeqCst) {
            self.writer.inject_sync_failure();
        }
        #[cfg(feature = "failpoints")]
        if matches!(crash.as_deref(), Some("after_write" | "sync_error_after_write")) {
            // The bytes reach the file and the flush is not confirmed.
            self.writer.inject_sync_failure_after_write();
        }
        if records > 0 {
            let synced = self.writer.sync(wall_us()).map_err(|e| e.to_string());
            #[cfg(feature = "failpoints")]
            if synced.is_err() && crash.as_deref() == Some("after_write") {
                crash_now("after_write");
            }
            synced.map_err(|e| self.fail(e))?;
        }
        #[cfg(feature = "failpoints")]
        if crash.as_deref() == Some("after_sync") {
            crash_now("after_sync");
        }
        let durable = Instant::now();
        let durable_at = wall_us();
        let sync_us = durable.duration_since(sync_started).as_micros() as u64;
        self.shared.durable_seq.store(self.writer.durable_seq(), Ordering::SeqCst);

        // 1. The read model learns the records, in order.
        let mut pending_out = Vec::with_capacity(batch.items.len());
        {
            let mut p = self.shared.published.write();
            for item in batch.items {
                let queue_us = item.core_start.duration_since(item.enqueued).as_micros() as u64;
                let core_us = item.core_end.duration_since(item.core_start).as_micros() as u64;
                let persist_us = durable.duration_since(item.core_end).as_micros() as u64;
                p.stats.inputs += 1;
                p.hists.queue.record(queue_us);
                p.hists.core.record(core_us);
                let reply_copy = item.reply.as_ref().map(|(_, r)| r.clone());
                match item.record {
                    Some((record, _)) => {
                        self.last_at = record.at;
                        p.hists.persist.record(persist_us);
                        p.hists.total.record(durable.duration_since(item.enqueued).as_micros() as u64);
                        let entry = TraceEntry {
                            record: record.clone(),
                            durable_at,
                            commands: item.commands.clone(),
                            reply: reply_copy,
                            queue_us,
                            core_us,
                            persist_us,
                        };
                        p.publish(entry).map_err(|e| self.fail(format!("read model: {e}")))?;
                        pending_out.push((Some(record), item.commands, item.reply));
                    }
                    None => {
                        if let Some(input) = item.input {
                            if !matches!(input, Input::Tick | Input::Storage { .. }) {
                                p.note_noop(NoopEntry {
                                    at: item.at,
                                    input,
                                    reply: reply_copy,
                                });
                            }
                        }
                        pending_out.push((None, item.commands, item.reply));
                    }
                }
            }
            p.stats.records += records;
            p.stats.journal_bytes += bytes;
            if records > 0 {
                p.stats.commits += 1;
                p.stats.last_batch = records;
                p.stats.max_batch = p.stats.max_batch.max(records);
                p.stats.last_commit_at = durable_at;
                p.stats.last_sync_us = sync_us;
                p.hists.sync.record(sync_us);
                p.hists.batch.record(records);
            }
        }

        // 2. Replies, gateway commands and subscriber events leave.
        let (mut replies, mut commands, mut delivered, mut dropped) = (0u64, 0u64, 0u64, 0u64);
        for (record, cmds, reply) in pending_out {
            if let Some((tx, reply)) = reply {
                // The requester may have gone away; the record stands anyway.
                let _ = tx.send(reply);
                replies += 1;
            }
            #[cfg(feature = "failpoints")]
            if crash.as_deref() == Some("after_reply") && !cmds.is_empty() {
                // Long enough for the reply to reach the client's socket.
                std::thread::sleep(std::time::Duration::from_millis(300));
                crash_now("after_reply");
            }
            for cmd in cmds {
                // The gateway queue is sized from the core's own cap on
                // outstanding sends, so this waits only if that sizing is
                // wrong; waiting is still safe, dropping would not be.
                if self.commands.blocking_send(cmd).is_err() {
                    return Err(self.fail("the venue gateway is gone".into()));
                }
                commands += 1;
            }
            if let Some(rec) = record {
                let p = self.shared.published.read();
                let (d, x) = self.shared.published.fan_out(&rec, &p.core);
                delivered += d;
                dropped += x;
            }
        }
        {
            let mut p = self.shared.published.write();
            p.stats.replies_released += replies;
            p.stats.commands_released += commands;
            p.stats.events_published += delivered;
            p.stats.subscribers_dropped += dropped;
        }

        let requested = self.shared.snapshot_requested.swap(false, Ordering::SeqCst);
        let due = self.writer.durable_seq().saturating_sub(self.last_snapshot_seq) >= self.snapshot_every;
        if requested || due {
            self.request_snapshot();
        }
        Ok(())
    }

    /// Hands a copy of the durable state to the snapshot thread. The copy is
    /// taken from the read model, which holds exactly the durable records.
    fn request_snapshot(&mut self) {
        let (seq, core) = {
            let p = self.shared.published.read();
            (p.durable_seq, p.core.clone())
        };
        if seq == self.last_snapshot_seq {
            return;
        }
        // If a snapshot is still being written this one is skipped; the next
        // interval tries again.
        if self.snapshot_tx.try_send((seq, self.last_at, core)).is_ok() {
            self.last_snapshot_seq = seq;
        }
    }

    pub fn durable_seq(&self) -> u64 {
        self.writer.durable_seq()
    }
}

fn snapshot_thread(dir: PathBuf, writer_name: String, shared: Arc<Shared>, rx: Receiver<(u64, Ts, Core)>) {
    while let Ok((seq, last_at, core)) = rx.recv() {
        let started = Instant::now();
        let digest = core.digest();
        let result = serde_json::to_vec(&SnapshotState { core, last_at })
            .map_err(|e| e.to_string())
            .and_then(|payload| {
                let info = SnapshotInfo {
                    format: 0,
                    state_schema: STATE_SCHEMA,
                    seq,
                    len: 0,
                    crc32: 0,
                    created_at_us: wall_us(),
                    writer: writer_name.clone(),
                    state_digest: digest,
                };
                oms_journal::write_snapshot(&dir, info, &payload).map_err(|e| e.to_string())
            });
        let mut p = shared.published.write();
        match result {
            Ok(path) => {
                p.stats.snapshots += 1;
                p.stats.last_snapshot_seq = seq;
                p.stats.last_snapshot_at = wall_us();
                p.stats.last_snapshot_error = None;
                drop(p);
                info!("snapshot written", "seq": seq, "path": path.display().to_string(), "ms": started.elapsed().as_millis() as u64);
            }
            Err(e) => {
                // A failed snapshot costs recovery time, not correctness: the
                // journal is complete without it.
                p.stats.last_snapshot_error = Some(e.clone());
                drop(p);
                error!("snapshot failed", "seq": seq, "error": e);
            }
        }
    }
}

pub struct PipelineConfig {
    pub max_batch: usize,
    pub snapshot_every: u64,
    pub inline_commit: bool,
    pub data_dir: PathBuf,
    pub writer_name: String,
}

pub struct Pipeline {
    pub core_thread: JoinHandle<()>,
    pub commit_thread: Option<JoinHandle<()>>,
    pub snapshot_thread: JoinHandle<()>,
}

/// Where the core thread sends a finished batch.
enum Sink {
    /// The default layout: a separate commit thread, so the core keeps
    /// deciding while the previous batch is being flushed.
    Thread(SyncSender<Batch>),
    /// The simplest layout: the core thread flushes each batch itself.
    Inline(Box<Committer>),
}

/// Starts the core thread and the commit stage. `core` is the recovered
/// state and `last_at` the time of the last recovered record.
pub fn spawn(
    core: Core,
    writer: Writer,
    last_at: Ts,
    shared: Arc<Shared>,
    commands: mpsc::Sender<VenueCommand>,
    cfg: PipelineConfig,
) -> Pipeline {
    let (snapshot_tx, snapshot_rx) = sync_channel::<(u64, Ts, Core)>(1);
    let snap_shared = shared.clone();
    let (dir, name) = (cfg.data_dir.clone(), cfg.writer_name.clone());
    let snapshot_thread = std::thread::Builder::new()
        .name("oms-snapshot".into())
        .spawn(move || snapshot_thread(dir, name, snap_shared, snapshot_rx))
        .expect("spawn snapshot thread");

    let last_seq = writer.durable_seq();
    let last_snapshot_seq = shared.published.read().stats.last_snapshot_seq;
    let mut committer = Committer {
        writer,
        shared: shared.clone(),
        commands,
        snapshot_tx,
        snapshot_every: cfg.snapshot_every,
        last_snapshot_seq,
        last_at,
    };

    let (sink, commit_thread) = if cfg.inline_commit {
        (Sink::Inline(Box::new(committer)), None)
    } else {
        // A few batches may wait here; beyond that the core thread blocks,
        // which stops it taking input, which fills the input queues, which
        // makes the ingress refuse. Nothing grows without bound.
        let (tx, rx) = sync_channel::<Batch>(8);
        let fatal = shared.fatal.clone();
        let queues = shared.queues.clone();
        let handle = std::thread::Builder::new()
            .name("oms-commit".into())
            .spawn(move || {
                while let Ok(mut batch) = rx.recv() {
                    // Group commit: everything already waiting goes into the
                    // same write and the same flush.
                    while batch.items.len() < 4096 {
                        match rx.try_recv() {
                            Ok(more) => batch.items.extend(more.items),
                            Err(_) => break,
                        }
                    }
                    if let Err(e) = committer.commit(batch) {
                        fatal.trip(format!("commit failed: {e}"));
                        queues.close();
                        return;
                    }
                }
            })
            .expect("spawn commit thread");
        (Sink::Thread(tx), Some(handle))
    };

    let core_shared = shared.clone();
    let max_batch = cfg.max_batch;
    let core_thread = std::thread::Builder::new()
        .name("oms-core".into())
        .spawn(move || core_loop(core, last_seq, last_at, core_shared, max_batch, sink))
        .expect("spawn core thread");

    Pipeline {
        core_thread,
        commit_thread,
        snapshot_thread,
    }
}

fn core_loop(mut core: Core, mut seq: u64, mut last_at: Ts, shared: Arc<Shared>, max_batch: usize, mut sink: Sink) {
    let mut msgs: Vec<CoreMsg> = Vec::with_capacity(max_batch);
    loop {
        msgs.clear();
        if !shared.queues.pop_batch(max_batch, &mut msgs) {
            break;
        }
        if shared.fatal.is_tripped() {
            break;
        }
        let mut batch = Batch {
            items: Vec::with_capacity(msgs.len()),
        };
        for msg in msgs.drain(..) {
            // Record time never goes backwards, even if the wall clock does.
            let at = wall_us().max(last_at);
            last_at = at;
            let core_start = Instant::now();
            let out = match core.handle(at, &msg.input) {
                Ok(out) => out,
                Err(fault) => {
                    // The state is no longer trustworthy and nothing of this
                    // batch has been released. Stop; the journal still holds
                    // the last consistent state.
                    shared.fatal.trip(format!("{fault} while handling {}", msg.input.name()));
                    shared.queues.close();
                    return;
                }
            };
            let core_end = Instant::now();
            let reply = match (msg.reply, out.reply) {
                (Some(tx), Some(r)) => Some((tx, r)),
                _ => None,
            };
            let (record, input) = if out.events.is_empty() {
                (None, Some(msg.input))
            } else {
                seq += 1;
                let rec = Record {
                    seq,
                    at,
                    input: msg.input,
                    events: out.events,
                };
                match serde_json::to_vec(&rec) {
                    Ok(bytes) => (Some((rec, bytes)), None),
                    Err(e) => {
                        shared.fatal.trip(format!("record {seq} could not be encoded: {e}"));
                        shared.queues.close();
                        return;
                    }
                }
            };
            batch.items.push(Item {
                input,
                at,
                record,
                commands: out.commands,
                reply,
                enqueued: msg.enqueued,
                core_start,
                core_end,
            });
        }
        match &mut sink {
            Sink::Thread(tx) => {
                if tx.send(batch).is_err() {
                    // The commit stage stopped; it has tripped the latch.
                    break;
                }
            }
            Sink::Inline(committer) => {
                if let Err(e) = committer.commit(batch) {
                    shared.fatal.trip(format!("commit failed: {e}"));
                    shared.queues.close();
                    return;
                }
            }
        }
    }
    info!("core thread stopped", "last_seq": seq);
}
