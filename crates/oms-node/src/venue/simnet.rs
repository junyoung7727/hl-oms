//! Gateway to the simulated venue process.
//!
//! It has the same shape as a real gateway: a long-lived stream connection
//! whose state is the link, and request/response exchanges with a timeout for
//! orders, cancels and queries. Timeouts and connection losses here are real.

use super::simproto::*;
use super::{GatewayPorts, QueryReq, VenueInputTx, session_number};
use crate::util::wall_us;
use crate::{info, warn};
use oms_core::*;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::net::unix::OwnedWriteHalf;
use tokio::sync::{Mutex, Semaphore, oneshot};

/// Why an exchange with the venue produced no answer.
enum CallError {
    /// The connection could not be established: nothing was sent.
    Connect(String),
    /// The request may have been sent; no answer arrived.
    NoAnswer(String),
}

type Pending = Arc<std::sync::Mutex<HashMap<u64, oneshot::Sender<SimResp>>>>;

struct Conn {
    writer: OwnedWriteHalf,
    pending: Pending,
}

/// A request connection that multiplexes calls by id and reconnects lazily.
struct Requests {
    socket: PathBuf,
    timeout: Duration,
    conn: Mutex<Option<Conn>>,
    next_id: AtomicU64,
}

impl Requests {
    async fn connect(&self) -> Result<Conn, String> {
        let stream = UnixStream::connect(&self.socket).await.map_err(|e| e.to_string())?;
        let (read, writer) = stream.into_split();
        let pending: Pending = Arc::default();
        let p = pending.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(read).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if let Ok(resp) = serde_json::from_str::<SimResp>(&line) {
                    let waiter = p.lock().unwrap_or_else(|e| e.into_inner()).remove(&resp.id);
                    if let Some(tx) = waiter {
                        let _ = tx.send(resp);
                    }
                }
            }
            // The connection ended: everyone still waiting gets no answer.
            p.lock().unwrap_or_else(|e| e.into_inner()).clear();
        });
        Ok(Conn { writer, pending })
    }

    async fn call(&self, op: SimOp) -> Result<SimResp, CallError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let mut line = serde_json::to_vec(&SimReq { id, op }).map_err(|e| CallError::Connect(e.to_string()))?;
        line.push(b'\n');
        let (tx, rx) = oneshot::channel();
        let pending: Pending;
        {
            let mut guard = self.conn.lock().await;
            if guard.is_none() {
                *guard = Some(self.connect().await.map_err(CallError::Connect)?);
            }
            let Some(conn) = guard.as_mut() else {
                return Err(CallError::Connect("no connection".into()));
            };
            conn.pending.lock().unwrap_or_else(|e| e.into_inner()).insert(id, tx);
            pending = conn.pending.clone();
            if let Err(e) = conn.writer.write_all(&line).await {
                // Part of the line may have gone out.
                *guard = None;
                return Err(CallError::NoAnswer(format!("write failed: {e}")));
            }
        }
        match tokio::time::timeout(self.timeout, rx).await {
            Ok(Ok(resp)) => Ok(resp),
            Ok(Err(_)) => {
                *self.conn.lock().await = None;
                Err(CallError::NoAnswer("connection closed before the response".into()))
            }
            Err(_) => {
                // Stop waiting for it; a late answer is discarded.
                pending.lock().unwrap_or_else(|e| e.into_inner()).remove(&id);
                Err(CallError::NoAnswer(format!("no response within {} ms", self.timeout.as_millis())))
            }
        }
    }
}

fn wire_status(o: &WireOrder, report_cum: bool) -> StatusIn {
    let status = match o.status {
        WireStatus::Open => VenueStatus::Open,
        WireStatus::Filled => VenueStatus::Filled,
        WireStatus::Canceled => VenueStatus::Canceled { reason: o.reason.clone() },
    };
    StatusIn {
        status,
        cum: report_cum.then_some(o.filled),
        venue_ts: o.ts,
    }
}

fn venue_oid(oid: u64) -> Option<VenueOrderId> {
    VenueOrderId::new(format!("v{oid}")).ok()
}

fn our_order(client: &Option<String>, instance_id: u64) -> Option<OrderId> {
    client
        .as_deref()
        .and_then(VenueClientId::parse_hex)
        .filter(|c| c.instance_id == instance_id)
        .map(|c| c.order_id)
}

fn fill_input(f: &WireFill, instance_id: u64) -> Option<VenueInput> {
    Some(VenueInput::Fill(FillIn {
        key: FillKey::new(format!("t{}", f.tid)).ok()?,
        order_id: our_order(&f.client, instance_id),
        venue_oid: venue_oid(f.oid),
        instrument: f.instrument.clone(),
        side: f.side,
        qty: f.qty,
        px: f.px,
        fee: f.fee,
        venue_ts: f.ts,
    }))
}

struct Gateway {
    requests: Requests,
    input: VenueInputTx,
    instance_id: u64,
    session: AtomicU64,
}

impl Gateway {
    async fn send_order(&self, cmd: VenueCommand) {
        let VenueCommand::SendOrder {
            order_id,
            instance_id,
            instrument,
            side,
            qty,
            px,
            tif,
            expires_at,
            ..
        } = cmd
        else {
            return;
        };
        let started_at = wall_us();
        let client = VenueClientId { instance_id, order_id }.to_hex();
        let outcome = if started_at >= expires_at {
            SendOutcome::NotSent {
                reason: "the send window had passed before the gateway could send".into(),
            }
        } else {
            let op = SimOp::Place {
                client: client.clone(),
                instrument,
                side,
                qty,
                px,
                tif,
                expires_at,
            };
            match self.requests.call(op).await {
                Err(CallError::Connect(e)) => SendOutcome::NotSent {
                    reason: format!("could not connect: {e}"),
                },
                Err(CallError::NoAnswer(e)) => SendOutcome::Unknown { reason: e },
                Ok(resp) if !resp.ok => SendOutcome::Unknown {
                    reason: format!("venue error: {}", resp.error.unwrap_or_default()),
                },
                Ok(resp) => match serde_json::from_value::<PlaceResult>(resp.result) {
                    Ok(PlaceResult::Placed(o)) => match venue_oid(o.oid) {
                        Some(v) => SendOutcome::Placed {
                            venue_oid: v,
                            status: wire_status(&o, true),
                        },
                        None => SendOutcome::Unknown {
                            reason: "unusable venue order id".into(),
                        },
                    },
                    Ok(PlaceResult::Rejected { reason }) => SendOutcome::Rejected { reason },
                    // An answer we cannot read tells us nothing certain.
                    Err(e) => SendOutcome::Unknown {
                        reason: format!("unreadable response: {e}"),
                    },
                },
            }
        };
        let timing = GatewayTiming {
            started_at,
            finished_at: wall_us(),
            wire_id: client,
        };
        self.input
            .send(VenueInput::SendResult {
                order_id,
                outcome,
                timing,
            })
            .await;
    }

    async fn send_cancel(&self, cmd: VenueCommand) {
        let VenueCommand::SendCancel {
            order_id,
            instance_id,
            attempt,
            ..
        } = cmd
        else {
            return;
        };
        let started_at = wall_us();
        let client = VenueClientId { instance_id, order_id }.to_hex();
        let outcome = match self.requests.call(SimOp::Cancel { client: client.clone() }).await {
            Err(CallError::Connect(e)) => CancelOutcome::NotSent {
                reason: format!("could not connect: {e}"),
            },
            Err(CallError::NoAnswer(e)) => CancelOutcome::Unknown { reason: e },
            Ok(resp) if resp.ok => CancelOutcome::Confirmed,
            Ok(resp) => CancelOutcome::Rejected {
                reason: resp.error.unwrap_or_default(),
            },
        };
        let timing = GatewayTiming {
            started_at,
            finished_at: wall_us(),
            wire_id: client,
        };
        self.input
            .send(VenueInput::CancelResult {
                order_id,
                attempt,
                outcome,
                timing,
            })
            .await;
    }

    async fn query(&self, q: QueryReq) {
        let session = self.session.load(Ordering::SeqCst);
        let queried_at = wall_us();
        match q {
            QueryReq::Status { order_id, .. } => {
                let client = VenueClientId {
                    instance_id: self.instance_id,
                    order_id,
                }
                .to_hex();
                let Ok(resp) = self.requests.call(SimOp::Status { client }).await else {
                    return;
                };
                match serde_json::from_value::<StatusResult>(resp.result) {
                    Ok(StatusResult::Found(o)) => {
                        self.input
                            .send(VenueInput::OrderStatus(OrderStatusIn {
                                session,
                                order_id: Some(order_id),
                                venue_oid: venue_oid(o.oid),
                                status: wire_status(&o, true),
                                source: EvidenceSource::Query,
                            }))
                            .await;
                    }
                    Ok(StatusResult::NotFound) => {
                        self.input
                            .send(VenueInput::StatusNotFound {
                                session,
                                order_id,
                                queried_at,
                            })
                            .await;
                    }
                    Err(_) => {}
                }
            }
            QueryReq::Fills { since } => {
                let Ok(resp) = self.requests.call(SimOp::Fills { since }).await else {
                    return;
                };
                let Ok(r) = serde_json::from_value::<FillsResult>(resp.result) else {
                    return;
                };
                for f in &r.fills {
                    if let Some(i) = fill_input(f, self.instance_id) {
                        self.input.send(i).await;
                    }
                }
                // The answer was computed at `venue_now`; a fill can still
                // happen at that same instant afterwards.
                self.input
                    .send(VenueInput::FillsSynced {
                        session,
                        up_to: r.venue_now.saturating_sub(1),
                    })
                    .await;
            }
            QueryReq::OpenOrders => {
                let Ok(resp) = self.requests.call(SimOp::OpenOrders).await else {
                    return;
                };
                let Ok(list) = serde_json::from_value::<Vec<WireOrder>>(resp.result) else {
                    return;
                };
                let orders = list
                    .iter()
                    .filter_map(|o| {
                        Some(VenueOpenOrder {
                            venue_oid: venue_oid(o.oid)?,
                            order_id: our_order(&o.client, self.instance_id),
                            instrument: o.instrument.clone(),
                            side: o.side,
                            remaining: Some(Qty(o.qty.0 - o.filled.0)),
                        })
                    })
                    .collect();
                self.input
                    .send(VenueInput::OpenOrders {
                        session,
                        queried_at,
                        orders,
                    })
                    .await;
            }
            QueryReq::Positions => {
                let Ok(resp) = self.requests.call(SimOp::Positions).await else {
                    return;
                };
                let Ok(list) = serde_json::from_value::<Vec<WirePosition>>(resp.result) else {
                    return;
                };
                let positions = list
                    .into_iter()
                    .map(|p| VenuePosition {
                        instrument: p.instrument,
                        qty: p.qty,
                    })
                    .collect();
                self.input
                    .send(VenueInput::Positions {
                        session,
                        queried_at,
                        positions,
                    })
                    .await;
            }
            QueryReq::Margin { seq } => {
                // No answer: no input. The last report keeps ageing.
                let Ok(resp) = self.requests.call(SimOp::Margin).await else {
                    return;
                };
                let input = match serde_json::from_value::<MarginResult>(resp.result) {
                    Ok(r) => VenueInput::MarginReport(MarginReportIn {
                        session,
                        seq,
                        queried_at,
                        account_value: r.account_value,
                        positions: Vec::new(),
                    }),
                    // An answer that cannot be read is refused as a whole.
                    Err(e) => VenueInput::MarginReportInvalid {
                        session,
                        seq,
                        queried_at,
                        reason: format!("unreadable margin answer: {e}"),
                    },
                };
                self.input.send(input).await;
            }
        }
    }

    /// Keeps the event stream connected. The stream's state is the link.
    async fn stream_loop(self: Arc<Self>, boot: u32, socket: PathBuf) {
        let mut connection = 0u32;
        let mut backoff = Duration::from_millis(100);
        loop {
            match self.run_stream(boot, &mut connection, &socket).await {
                Ok(()) => backoff = Duration::from_millis(100),
                Err(e) => {
                    warn!("venue stream unavailable", "error": e);
                }
            }
            tokio::time::sleep(backoff).await;
            backoff = (backoff * 2).min(Duration::from_secs(2));
        }
    }

    async fn run_stream(&self, boot: u32, connection: &mut u32, socket: &PathBuf) -> Result<(), String> {
        let stream = UnixStream::connect(socket).await.map_err(|e| e.to_string())?;
        let (read, mut write) = stream.into_split();
        let mut line = serde_json::to_vec(&SimReq {
            id: 0,
            op: SimOp::Subscribe,
        })
        .map_err(|e| e.to_string())?;
        line.push(b'\n');
        write.write_all(&line).await.map_err(|e| e.to_string())?;
        let mut lines = BufReader::new(read).lines();
        // The first line is the response to `subscribe`.
        let first = lines
            .next_line()
            .await
            .map_err(|e| e.to_string())?
            .ok_or("stream closed before the subscription was confirmed")?;
        let resp: SimResp = serde_json::from_str(&first).map_err(|e| e.to_string())?;
        if !resp.ok {
            return Err(resp.error.unwrap_or_else(|| "subscription refused".into()));
        }
        *connection += 1;
        let session = session_number(boot, *connection);
        self.session.store(session, Ordering::SeqCst);
        info!("venue stream connected", "session": session);
        self.input
            .send(VenueInput::Link {
                up: true,
                session,
                reason: "stream subscribed".into(),
            })
            .await;
        let reason = loop {
            match lines.next_line().await {
                Ok(Some(line)) => {
                    let Ok(msg) = serde_json::from_str::<StreamLine>(&line) else {
                        continue;
                    };
                    let input = match msg.stream {
                        StreamMsg::Order(o) => Some(VenueInput::OrderStatus(OrderStatusIn {
                            session,
                            order_id: our_order(&o.client, self.instance_id),
                            venue_oid: venue_oid(o.oid),
                            status: wire_status(&o, true),
                            source: EvidenceSource::Stream,
                        })),
                        StreamMsg::Fill(f) => fill_input(&f, self.instance_id),
                        StreamMsg::RefPrice { instrument, px, ts } => Some(VenueInput::RefPrice {
                            instrument,
                            px,
                            venue_ts: ts,
                        }),
                    };
                    if let Some(i) = input {
                        self.input.send(i).await;
                    }
                }
                Ok(None) => break "stream closed by the venue".to_string(),
                Err(e) => break format!("stream read failed: {e}"),
            }
        };
        self.input
            .send(VenueInput::Link {
                up: false,
                session,
                reason: reason.clone(),
            })
            .await;
        Err(reason)
    }
}

/// Starts the gateway tasks.
pub fn spawn(socket: PathBuf, timeout: Duration, mut ports: GatewayPorts, max_concurrent: usize) {
    let gw = Arc::new(Gateway {
        requests: Requests {
            socket: socket.clone(),
            timeout,
            conn: Mutex::new(None),
            next_id: AtomicU64::new(1),
        },
        input: ports.input.clone(),
        instance_id: ports.instance_id,
        session: AtomicU64::new(0),
    });

    tokio::spawn(gw.clone().stream_loop(ports.boot, socket));

    // Commands: each runs as its own task, at most `max_concurrent` at once.
    // The core already bounds outstanding sends; the semaphore bounds tasks
    // even if that sizing were wrong.
    let cmd_gw = gw.clone();
    tokio::spawn(async move {
        let permits = Arc::new(Semaphore::new(max_concurrent));
        while let Some(cmd) = ports.commands.recv().await {
            let Ok(permit) = permits.clone().acquire_owned().await else {
                return;
            };
            let gw = cmd_gw.clone();
            tokio::spawn(async move {
                match &cmd {
                    VenueCommand::SendOrder { .. } => gw.send_order(cmd).await,
                    VenueCommand::SendCancel { .. } => gw.send_cancel(cmd).await,
                }
                drop(permit);
            });
        }
    });

    let q_gw = gw;
    tokio::spawn(async move {
        let permits = Arc::new(Semaphore::new(8));
        while let Some(q) = ports.queries.recv().await {
            let Ok(permit) = permits.clone().acquire_owned().await else {
                return;
            };
            let gw = q_gw.clone();
            tokio::spawn(async move {
                gw.query(q).await;
                drop(permit);
            });
        }
    });
}
