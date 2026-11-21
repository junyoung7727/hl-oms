//! Venue gateways.
//!
//! A gateway implements [`VenueGateway`] and is started by [`start`], which
//! owns the command and query loops. The two implementations (the simulated
//! venue and Hyperliquid) are chosen once at startup (ADR-021). What they
//! must honour is this message contract, documented in
//! docs/contracts/gateway.md:
//!
//! - For every `SendOrder` it receives, a gateway delivers exactly one
//!   `SendResult`, and it classifies the outcome conservatively: `NotSent`
//!   only if no byte of the request can have left the host, `Rejected` only
//!   on the venue's explicit refusal, `Unknown` for everything else.
//! - It never repeats an order request on its own.
//! - For every `SendCancel` it delivers exactly one `CancelResult`.
//! - It reports `Link` up with a session number larger than any before, and
//!   `Link` down when the event stream is lost.
//! - A query is answered with ordinary venue inputs, or not at all. A
//!   `FillsSynced { up_to }` is sent only after every fill with venue time
//!   up to `up_to` has been delivered.
//! - Venue input is never dropped: a full input queue makes the gateway wait.
//! - For a `Kill` it runs its kill loop and delivers one `KillLoopEnded`
//!   when the loop ends; a gateway without cancel-all delivers one
//!   `KillLoopEnded { outcome: Unsupported }` at once (C-HOLD).

pub mod bybit;
pub mod hyperliquid;
pub mod reconciler;
pub mod simnet;
pub mod simproto;

use crate::queues::{CoreMsg, InputQueues};
use oms_core::*;
use std::sync::Arc;
use tokio::sync::{Semaphore, mpsc};

#[derive(Clone, Debug)]
pub enum QueryReq {
    Status {
        order_id: OrderId,
        venue_oid: Option<VenueOrderId>,
        /// `Need::OrderStatus::accepted_at`: the venue created the order no
        /// earlier than this.
        accepted_at: Ts,
    },
    Fills {
        since: Ts,
    },
    OpenOrders,
    Positions,
    /// The account's margin report (C-MARGIN). `seq` numbers the query; the
    /// answer carries it back so that the core can tell a late answer from
    /// a new one. Answered with `MarginReport`, with `MarginReportInvalid`
    /// if the answer cannot be read as a whole, or not at all.
    Margin {
        seq: u64,
    },
    /// The margin mode and the leverage of one instrument at the venue
    /// (C-MARGIN). Answered with `VenueSetting`, or not at all. A gateway
    /// only reads the setting; it never changes it.
    VenueSetting {
        instrument: InstrumentId,
    },
    /// The account-wide margin scheme at the venue (C-MARGIN). Answered
    /// with `AccountMode`, or not at all.
    AccountMode,
}

/// The gateway's side of the channels.
pub struct GatewayPorts {
    pub commands: mpsc::Receiver<VenueCommand>,
    pub queries: mpsc::Receiver<QueryReq>,
    pub input: VenueInputTx,
    pub instance_id: u64,
    pub boot: u32,
}

/// What the driver needs from a gateway. Static dispatch: the methods
/// return futures, so the trait is not used as `dyn` (ADR-021).
pub trait VenueGateway: Send + Sync + 'static {
    /// How many queries run at the same time.
    const QUERY_CONCURRENCY: usize;

    /// The form of the client order id on the wire (ADR-022). The status API
    /// prints the id in this form.
    const WIRE_ID: WireIdForm;

    /// Handles one `VenueCommand::SendOrder`. Delivers exactly one
    /// `SendResult` (G-1).
    fn send_order(&self, cmd: VenueCommand) -> impl Future<Output = ()> + Send;

    /// Handles one `VenueCommand::SendCancel`. Delivers exactly one
    /// `CancelResult` (G-5).
    fn send_cancel(&self, cmd: VenueCommand) -> impl Future<Output = ()> + Send;

    /// Answers one query with venue inputs, or not at all.
    fn query(&self, q: QueryReq) -> impl Future<Output = ()> + Send;

    /// Handles `VenueCommand::Kill` and `VenueCommand::KillStop` (C-HOLD).
    /// Returns at once: the kill loop runs on its own task, not under a
    /// command permit, so that no number of sends can delay it.
    fn kill(&self, cmd: VenueCommand);

    /// Starts the long-running tasks: the event stream and anything else the
    /// gateway keeps running. `start` calls it once, synchronously, before it
    /// spawns the command loop.
    fn spawn_tasks(self: &Arc<Self>, boot: u32);
}

/// Starts a gateway: its long-running tasks, then the command loop, then the
/// query loop. Each command and each query runs as its own task, at most
/// `max_concurrent` commands and `G::QUERY_CONCURRENCY` queries at once.
/// The core already bounds outstanding sends; the semaphore bounds tasks
/// even if that sizing were wrong.
pub fn start<G: VenueGateway>(gw: Arc<G>, mut ports: GatewayPorts, max_concurrent: usize) {
    gw.spawn_tasks(ports.boot);

    let cmd_gw = gw.clone();
    tokio::spawn(async move {
        let permits = Arc::new(Semaphore::new(max_concurrent));
        while let Some(cmd) = ports.commands.recv().await {
            if matches!(cmd, VenueCommand::Kill { .. } | VenueCommand::KillStop) {
                cmd_gw.kill(cmd);
                continue;
            }
            let Ok(permit) = permits.clone().acquire_owned().await else {
                return;
            };
            let gw = cmd_gw.clone();
            tokio::spawn(async move {
                match &cmd {
                    VenueCommand::SendOrder { .. } => gw.send_order(cmd).await,
                    VenueCommand::SendCancel { .. } => gw.send_cancel(cmd).await,
                    // Handled above, without a permit.
                    VenueCommand::Kill { .. } | VenueCommand::KillStop => {}
                }
                drop(permit);
            });
        }
    });

    tokio::spawn(async move {
        let permits = Arc::new(Semaphore::new(G::QUERY_CONCURRENCY));
        while let Some(q) = ports.queries.recv().await {
            let Ok(permit) = permits.clone().acquire_owned().await else {
                return;
            };
            let gw = gw.clone();
            tokio::spawn(async move {
                gw.query(q).await;
                drop(permit);
            });
        }
    });
}

#[derive(Clone)]
pub struct VenueInputTx {
    queues: Arc<InputQueues>,
}

impl VenueInputTx {
    pub fn new(queues: Arc<InputQueues>) -> VenueInputTx {
        VenueInputTx { queues }
    }

    /// Delivers one venue input to the core, waiting if its queue is full.
    /// Returns false if the process is shutting down.
    pub async fn send(&self, v: VenueInput) -> bool {
        self.queues.push_venue(CoreMsg::new(Input::Venue(v))).await
    }
}

/// What a gateway without cancel-all does with a kill command (C-HOLD): it
/// answers `Kill` with `KillLoopEnded { outcome: Unsupported }`, so that the
/// operator can clear the latch, and logs that the orders resting at the
/// venue are the operator's to cancel. `KillStop` has no loop to end.
pub fn kill_unsupported(input: &VenueInputTx, venue: &'static str, cmd: VenueCommand) {
    match cmd {
        VenueCommand::Kill { source, note, .. } => {
            crate::warn!("kill latched, but this gateway has no cancel-all: cancel the resting orders with omsctl cancel-all", "venue": venue, "source": format!("{source:?}"), "note": note);
            // Test-only: `OMS_HOLD_KILL_ANSWER` keeps the answer back, so a
            // test sees a latch that is not confirmed. Only a build with the
            // failpoints feature (the node tests) reads it.
            #[cfg(feature = "failpoints")]
            if std::env::var_os("OMS_HOLD_KILL_ANSWER").is_some() {
                crate::warn!("failpoint: the kill answer is held back", "venue": venue);
                return;
            }
            let input = input.clone();
            tokio::spawn(async move {
                input
                    .send(VenueInput::KillLoopEnded {
                        outcome: KillOutcome::Unsupported,
                        rounds: 0,
                        cancel_all_calls: 0,
                        detail: KILL_UNSUPPORTED.into(),
                    })
                    .await;
            });
        }
        VenueCommand::KillStop => crate::info!("kill cleared", "venue": venue),
        VenueCommand::SendOrder { .. } | VenueCommand::SendCancel { .. } => {}
    }
}

/// The detail of the `unsupported` answer.
pub const KILL_UNSUPPORTED: &str = "this gateway has no cancel-all; cancel with omsctl cancel-all";

/// Session numbers must grow across restarts as well as reconnects, so the
/// boot number forms the high part.
pub fn session_number(boot: u32, connection: u32) -> u64 {
    (u64::from(boot) << 32) | u64::from(connection)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;
    use tokio::sync::Semaphore;

    /// A third gateway: records what the driver hands it. Every operation
    /// waits at `gate` until the test opens it.
    struct Fake {
        log: Mutex<Vec<String>>,
        gate: Semaphore,
        running: AtomicUsize,
        peak: AtomicUsize,
        done: AtomicUsize,
    }

    impl Fake {
        fn new() -> Arc<Fake> {
            Arc::new(Fake {
                log: Mutex::new(Vec::new()),
                gate: Semaphore::new(0),
                running: AtomicUsize::new(0),
                peak: AtomicUsize::new(0),
                done: AtomicUsize::new(0),
            })
        }

        async fn op(&self, what: String) {
            let now = self.running.fetch_add(1, Ordering::SeqCst) + 1;
            self.peak.fetch_max(now, Ordering::SeqCst);
            if let Ok(p) = self.gate.acquire().await {
                p.forget();
            }
            self.log.lock().unwrap().push(what);
            self.running.fetch_sub(1, Ordering::SeqCst);
            self.done.fetch_add(1, Ordering::SeqCst);
        }

        fn log(&self) -> Vec<String> {
            self.log.lock().unwrap().clone()
        }
    }

    impl VenueGateway for Fake {
        const QUERY_CONCURRENCY: usize = 2;
        const WIRE_ID: WireIdForm = WireIdForm::Base32Pair;

        async fn send_order(&self, cmd: VenueCommand) {
            let VenueCommand::SendOrder { order_id, .. } = cmd else {
                self.log.lock().unwrap().push("send_order got a cancel".into());
                return;
            };
            self.op(format!("order {}", order_id.0)).await;
        }

        async fn send_cancel(&self, cmd: VenueCommand) {
            let VenueCommand::SendCancel { order_id, .. } = cmd else {
                self.log.lock().unwrap().push("send_cancel got an order".into());
                return;
            };
            self.op(format!("cancel {}", order_id.0)).await;
        }

        async fn query(&self, q: QueryReq) {
            self.op(format!("query {q:?}")).await;
        }

        fn kill(&self, cmd: VenueCommand) {
            let what = if matches!(cmd, VenueCommand::KillStop) { "kill_stop" } else { "kill" };
            self.log.lock().unwrap().push(what.into());
        }

        fn spawn_tasks(self: &Arc<Self>, boot: u32) {
            // Slow on purpose: if the command loop were spawned first, the
            // queued commands would be handled while this sleeps.
            std::thread::sleep(Duration::from_millis(20));
            self.log.lock().unwrap().push(format!("tasks boot={boot}"));
        }
    }

    fn order(id: u64) -> VenueCommand {
        VenueCommand::SendOrder {
            order_id: OrderId(id),
            instance_id: 1,
            instrument: InstrumentId::new("BTC").unwrap(),
            side: Side::Buy,
            qty: Qty(1),
            px: Px(1),
            tif: Tif::Gtc,
            reduce_only: false,
            expires_at: 0,
        }
    }

    fn cancel(id: u64) -> VenueCommand {
        VenueCommand::SendCancel {
            order_id: OrderId(id),
            instance_id: 1,
            attempt: 1,
            instrument: InstrumentId::new("BTC").unwrap(),
            venue_oid: None,
        }
    }

    struct Wired {
        commands: mpsc::Sender<VenueCommand>,
        queries: mpsc::Sender<QueryReq>,
    }

    fn ports(boot: u32) -> (GatewayPorts, Wired) {
        let (cmd_tx, cmd_rx) = mpsc::channel(64);
        let (q_tx, q_rx) = mpsc::channel(64);
        let ports = GatewayPorts {
            commands: cmd_rx,
            queries: q_rx,
            input: VenueInputTx::new(Arc::new(InputQueues::new(16, 16, 16))),
            instance_id: 1,
            boot,
        };
        let wired = Wired {
            commands: cmd_tx,
            queries: q_tx,
        };
        (ports, wired)
    }

    async fn until(what: &str, f: impl Fn() -> bool) {
        for _ in 0..500 {
            if f() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        panic!("timed out waiting for {what}");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_gateway_plugged_in_through_the_trait_gets_each_command_and_query_once() {
        let gw = Fake::new();
        gw.gate.add_permits(1000);
        let (p, w) = ports(7);
        // Queued before the start, so a command loop that ran before the
        // long-running tasks would log first.
        w.commands.send(order(1)).await.unwrap();
        w.commands.send(cancel(2)).await.unwrap();
        w.commands.send(order(3)).await.unwrap();
        w.queries.send(QueryReq::Positions).await.unwrap();
        w.queries.send(QueryReq::Margin { seq: 9 }).await.unwrap();
        start(gw.clone(), p, 4);
        until("five operations", || gw.done.load(Ordering::SeqCst) == 5).await;
        tokio::time::sleep(Duration::from_millis(20)).await;
        let mut log = gw.log();
        assert_eq!(log.remove(0), "tasks boot=7", "spawn_tasks runs once, before the command loop");
        log.sort();
        assert_eq!(
            log,
            vec!["cancel 2", "order 1", "order 3", "query Margin { seq: 9 }", "query Positions"]
        );
    }

    #[tokio::test]
    async fn the_driver_bounds_concurrent_commands_and_queries_and_drops_none() {
        let gw = Fake::new();
        let (p, w) = ports(1);
        start(gw.clone(), p, 3);
        for i in 0..10 {
            w.commands.send(order(i)).await.unwrap();
        }
        until("three commands running", || gw.running.load(Ordering::SeqCst) == 3).await;
        tokio::time::sleep(Duration::from_millis(30)).await;
        assert_eq!(gw.peak.load(Ordering::SeqCst), 3, "max_concurrent bounds the commands");
        gw.gate.add_permits(10);
        until("ten commands done", || gw.done.load(Ordering::SeqCst) == 10).await;

        gw.peak.store(0, Ordering::SeqCst);
        for _ in 0..6 {
            w.queries.send(QueryReq::OpenOrders).await.unwrap();
        }
        until("two queries running", || gw.running.load(Ordering::SeqCst) == 2).await;
        tokio::time::sleep(Duration::from_millis(30)).await;
        assert_eq!(gw.peak.load(Ordering::SeqCst), Fake::QUERY_CONCURRENCY, "the gateway's constant bounds the queries");
        gw.gate.add_permits(6);
        until("six queries done", || gw.done.load(Ordering::SeqCst) == 16).await;
        assert_eq!(gw.log().len(), 1 + 16, "nothing was dropped");
    }

    /// Both gateways of today keep the hex form they always wrote, so no
    /// Hyperliquid or simulated-venue request changes (ADR-022).
    #[test]
    fn the_gateways_of_today_write_the_hex_form() {
        assert_eq!(<super::simnet::Gateway as VenueGateway>::WIRE_ID, WireIdForm::Hex128);
        assert_eq!(<super::hyperliquid::Gateway as VenueGateway>::WIRE_ID, WireIdForm::Hex128);
    }
}
