//! The published read model.
//!
//! This is a copy of the domain state built only from records that are
//! already durable, plus bookkeeping the dashboard and the local API read.
//! Only the commit stage writes it. Readers take a read lock, copy what they
//! need and release; they never touch the authoritative core, and nothing
//! read here is fed back into an order decision.
//!
//! The copy can lag the authoritative state by the records that are still on
//! their way to disk. It never runs ahead of the journal.

use crate::util::{Hist, HistSummary};
use oms_api::EventMsg;
use oms_core::*;
use serde::Serialize;
use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex, RwLock, RwLockReadGuard};
use tokio::sync::mpsc;

/// Most trace entries kept per order.
const TRACE_PER_ORDER: usize = 200;
/// Most recent records kept for the activity view.
const RECENT: usize = 2000;
/// Most recent inputs kept that changed nothing (duplicates and the like).
const NOOPS: usize = 500;

/// One journal record with what the execution layer observed around it.
#[derive(Clone, Debug, Serialize)]
pub struct TraceEntry {
    pub record: Record,
    /// Wall-clock time at which the record was known durable.
    pub durable_at: Ts,
    /// Commands released to the gateway because of this record.
    pub commands: Vec<VenueCommand>,
    /// Reply released to the requester because of this record.
    pub reply: Option<Reply>,
    pub queue_us: u64,
    pub core_us: u64,
    pub persist_us: u64,
}

/// An input that produced no event. Not journaled; kept only as a bounded,
/// lossy diagnostic.
#[derive(Clone, Debug, Serialize)]
pub struct NoopEntry {
    pub at: Ts,
    pub input: Input,
    pub reply: Option<Reply>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct OrderTrace {
    pub entries: Vec<Arc<TraceEntry>>,
    /// True if older entries were dropped to respect the per-order cap. The
    /// journal still has them.
    pub truncated: bool,
}

#[derive(Clone, Debug, Default)]
pub struct StageHists {
    pub queue: Hist,
    pub core: Hist,
    pub persist: Hist,
    pub total: Hist,
    pub sync: Hist,
    pub batch: Hist,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct PipelineStats {
    pub inputs: u64,
    pub records: u64,
    pub noop_inputs: u64,
    pub commits: u64,
    pub journal_bytes: u64,
    pub commands_released: u64,
    pub replies_released: u64,
    pub events_published: u64,
    pub subscribers_dropped: u64,
    pub last_batch: u64,
    pub max_batch: u64,
    pub last_commit_at: Ts,
    pub last_sync_us: u64,
    pub snapshots: u64,
    pub last_snapshot_seq: u64,
    pub last_snapshot_at: Ts,
    pub last_snapshot_error: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct StageSummary {
    /// Waiting in the input queue.
    pub queue: HistSummary,
    /// Inside `Core::handle`.
    pub core: HistSummary,
    /// From the end of the decision to the record being durable.
    pub persist: HistSummary,
    /// From enqueue to durable.
    pub total: HistSummary,
    /// One journal write plus flush.
    pub sync: HistSummary,
    /// Records per commit.
    pub batch: HistSummary,
}

pub struct Inner {
    pub core: Core,
    pub durable_seq: u64,
    pub traces: BTreeMap<OrderId, OrderTrace>,
    pub recent: VecDeque<Arc<TraceEntry>>,
    pub noops: VecDeque<NoopEntry>,
    pub stats: PipelineStats,
    pub hists: StageHists,
}

struct Subscriber {
    /// `None` receives every strategy's events (admin socket).
    strategy: Option<StrategyId>,
    tx: mpsc::Sender<EventMsg>,
}

pub struct Published {
    inner: RwLock<Inner>,
    subs: Mutex<Vec<Subscriber>>,
}

impl Published {
    pub fn new(core: Core, durable_seq: u64) -> Published {
        Published {
            inner: RwLock::new(Inner {
                core,
                durable_seq,
                traces: BTreeMap::new(),
                recent: VecDeque::new(),
                noops: VecDeque::new(),
                stats: PipelineStats::default(),
                hists: StageHists::default(),
            }),
            subs: Mutex::new(Vec::new()),
        }
    }

    pub fn read(&self) -> RwLockReadGuard<'_, Inner> {
        self.inner.read().unwrap_or_else(|e| e.into_inner())
    }

    pub fn write(&self) -> std::sync::RwLockWriteGuard<'_, Inner> {
        self.inner.write().unwrap_or_else(|e| e.into_inner())
    }

    pub fn subscribe(&self, strategy: Option<StrategyId>, buffer: usize) -> mpsc::Receiver<EventMsg> {
        let (tx, rx) = mpsc::channel(buffer);
        self.subs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(Subscriber { strategy, tx });
        rx
    }

    /// Sends the order events of one durable record to the subscribers they
    /// concern. A subscriber whose buffer is full is dropped: its channel
    /// closes, the connection sees that and ends, and the client resynchronises
    /// by querying. Returns (delivered, dropped).
    pub fn fan_out(&self, rec: &Record, core: &Core) -> (u64, u64) {
        let mut subs = self.subs.lock().unwrap_or_else(|e| e.into_inner());
        if subs.is_empty() {
            return (0, 0);
        }
        let mut by_order: BTreeMap<OrderId, Vec<Event>> = BTreeMap::new();
        for ev in &rec.events {
            if let Some(id) = ev.order_id() {
                by_order.entry(id).or_default().push(ev.clone());
            }
        }
        let (mut delivered, mut dropped) = (0, 0);
        for (order_id, events) in by_order {
            let owner = core.order(order_id).map(|o| o.strategy.clone());
            let msg = EventMsg {
                seq: rec.seq,
                at: rec.at,
                order_id,
                events,
            };
            subs.retain(|s| {
                let wanted = match (&s.strategy, &owner) {
                    (None, _) => true,
                    (Some(a), Some(b)) => a == b,
                    (Some(_), None) => false,
                };
                if !wanted {
                    return !s.tx.is_closed();
                }
                match s.tx.try_send(msg.clone()) {
                    Ok(()) => {
                        delivered += 1;
                        true
                    }
                    // The connection ended; nothing was lost that anyone reads.
                    Err(mpsc::error::TrySendError::Closed(_)) => false,
                    Err(mpsc::error::TrySendError::Full(_)) => {
                        dropped += 1;
                        false
                    }
                }
            });
        }
        (delivered, dropped)
    }
}

impl Inner {
    /// Applies one durable record to the copy and files its trace.
    pub fn publish(&mut self, entry: TraceEntry) -> Result<(), CoreFault> {
        self.core.replay(&entry.record)?;
        self.durable_seq = entry.record.seq;
        let entry = Arc::new(entry);
        let mut ids: Vec<OrderId> = entry.record.events.iter().filter_map(|e| e.order_id()).collect();
        ids.sort();
        ids.dedup();
        for id in ids {
            let t = self.traces.entry(id).or_default();
            if t.entries.len() >= TRACE_PER_ORDER {
                t.entries.remove(1.min(t.entries.len() - 1));
                t.truncated = true;
            }
            t.entries.push(entry.clone());
        }
        for ev in &entry.record.events {
            if let Event::Purged { order_ids, .. } = ev {
                for id in order_ids {
                    self.traces.remove(id);
                }
            }
        }
        if self.recent.len() >= RECENT {
            self.recent.pop_front();
        }
        self.recent.push_back(entry);
        Ok(())
    }

    pub fn note_noop(&mut self, n: NoopEntry) {
        if self.noops.len() >= NOOPS {
            self.noops.pop_front();
        }
        self.noops.push_back(n);
        self.stats.noop_inputs += 1;
    }

    pub fn stage_summary(&self) -> StageSummary {
        StageSummary {
            queue: self.hists.queue.summary(),
            core: self.hists.core.summary(),
            persist: self.hists.persist.summary(),
            total: self.hists.total.summary(),
            sync: self.hists.sync.summary(),
            batch: self.hists.batch.summary(),
        }
    }
}
