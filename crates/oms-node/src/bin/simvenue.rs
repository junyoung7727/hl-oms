//! The simulated venue as a separate process.
//!
//! `simvenue <socket-path>` listens on a Unix socket and speaks the protocol
//! in `oms_node::venue::simproto`. Its order state lives in memory and
//! survives restarts of the OMS, which is what lets a test kill the OMS and
//! check what it reconciles afterwards.
//!
//! This is a test tool. It is not a market model and proves nothing about a
//! real venue.

use oms_core::*;
use oms_node::venue::simproto::*;
use oms_sim::venue::{PlaceReq, PlaceResp, SimFill, SimOrder, SimStatus, SimVenue, StreamEvent};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::mpsc;

struct State {
    venue: SimVenue,
    streams: Vec<mpsc::UnboundedSender<String>>,
    drop_responses: u32,
    drop_requests: u32,
    delay_ms: u64,
    auto_fill: AutoFill,
    refuse_streams: bool,
    requests: u64,
    margin_queries: u64,
    /// Margin mode and leverage per instrument, where a control changed
    /// them. Every other instrument is isolated at leverage 2.
    venue_settings: std::collections::BTreeMap<InstrumentId, VenueSettingResult>,
    account_mode: String,
    venue_setting_queries: u64,
    account_mode_queries: u64,
}

fn now_us() -> Ts {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_micros() as Ts)
        .unwrap_or(0)
}

fn wire_order(o: &SimOrder) -> WireOrder {
    let (status, reason) = match &o.status {
        SimStatus::Open => (WireStatus::Open, String::new()),
        SimStatus::Filled => (WireStatus::Filled, String::new()),
        SimStatus::Canceled(r) => (WireStatus::Canceled, r.clone()),
    };
    WireOrder {
        oid: o.oid,
        client: o.client.map(|c| c.to_hex()),
        instrument: o.instrument.clone(),
        side: o.side,
        qty: o.qty,
        filled: o.filled,
        status,
        reason,
        ts: o.updated_at,
    }
}

fn wire_fill(f: &SimFill) -> WireFill {
    WireFill {
        tid: f.tid,
        oid: f.oid,
        client: f.client.map(|c| c.to_hex()),
        instrument: f.instrument.clone(),
        side: f.side,
        qty: f.qty,
        px: f.px,
        fee: f.fee,
        ts: f.ts,
    }
}

impl State {
    /// Sends everything the venue emitted to every stream connection.
    fn pump(&mut self) {
        for ev in self.venue.take_stream() {
            let msg = match ev {
                StreamEvent::Order { oid, .. } => match self.venue.orders.get(&oid) {
                    Some(o) => StreamMsg::Order(wire_order(o)),
                    None => continue,
                },
                StreamEvent::Fill(f) => StreamMsg::Fill(wire_fill(&f)),
            };
            self.broadcast(&StreamLine { stream: msg });
        }
    }

    fn broadcast(&mut self, line: &StreamLine) {
        let Ok(text) = serde_json::to_string(line) else { return };
        self.streams.retain(|tx| tx.send(text.clone()).is_ok());
    }

    fn immediate(&self, qty: Qty, tif: Tif) -> Qty {
        match (self.auto_fill, tif) {
            (_, Tif::Alo) => Qty::ZERO,
            (AutoFill::None, _) => Qty::ZERO,
            (AutoFill::Full, _) => qty,
            (AutoFill::Half, _) => Qty(qty.0 / 2),
        }
    }

    fn handle(&mut self, op: SimOp) -> Result<serde_json::Value, String> {
        let now = now_us();
        let json = |v: &dyn erased::Ser| v.to_value();
        match op {
            SimOp::Place {
                client,
                instrument,
                side,
                qty,
                px,
                tif,
                expires_at,
            } => {
                let client = VenueClientId::parse_hex(&client).ok_or("bad client id")?;
                let immediate = self.immediate(qty, tif);
                let req = PlaceReq {
                    client,
                    instrument,
                    side,
                    qty,
                    px,
                    tif,
                    expires_at,
                    // The wire of this process does not carry the flag: the
                    // venue process treats every order as a plain one.
                    reduce_only: false,
                };
                let result = match self.venue.place(now, &req, immediate) {
                    PlaceResp::Placed { oid, .. } => PlaceResult::Placed(wire_order(&self.venue.orders[&oid])),
                    PlaceResp::Rejected { reason } => PlaceResult::Rejected { reason },
                };
                self.pump();
                Ok(json(&result))
            }
            SimOp::Cancel { client } => {
                let client = VenueClientId::parse_hex(&client).ok_or("bad client id")?;
                let r = self.venue.cancel(now, client);
                self.pump();
                r.map(|()| serde_json::Value::Null)
            }
            SimOp::Status { client } => {
                let client = VenueClientId::parse_hex(&client).ok_or("bad client id")?;
                let r = match self.venue.status(client) {
                    Some(o) => StatusResult::Found(wire_order(o)),
                    None => StatusResult::NotFound,
                };
                Ok(json(&r))
            }
            SimOp::Fills { since } => Ok(json(&FillsResult {
                fills: self.venue.fills_since(since).iter().map(wire_fill).collect(),
                venue_now: now,
            })),
            SimOp::OpenOrders => {
                let v: Vec<WireOrder> = self.venue.open_orders().into_iter().map(wire_order).collect();
                Ok(json(&v))
            }
            SimOp::Positions => {
                let v: Vec<WirePosition> = self
                    .venue
                    .positions
                    .iter()
                    .map(|(i, q)| WirePosition {
                        instrument: i.clone(),
                        qty: SQty(*q),
                    })
                    .collect();
                Ok(json(&v))
            }
            SimOp::Margin => {
                self.margin_queries += 1;
                Ok(json(&MarginResult {
                    account_value: self.venue.account_value(),
                }))
            }
            SimOp::VenueSetting { instrument } => {
                self.venue_setting_queries += 1;
                let setting = self.venue_settings.get(&instrument).cloned().unwrap_or(VenueSettingResult {
                    isolated: true,
                    leverage: 2,
                });
                Ok(json(&setting))
            }
            SimOp::AccountMode => {
                self.account_mode_queries += 1;
                Ok(json(&AccountModeResult {
                    mode: self.account_mode.clone(),
                }))
            }
            SimOp::Subscribe => Err("subscribe is handled by the connection".into()),
            SimOp::Ctl { action } => self.ctl(action, now),
        }
    }

    fn ctl(&mut self, action: Ctl, now: Ts) -> Result<serde_json::Value, String> {
        match action {
            Ctl::Fill { client, qty } => {
                let client = VenueClientId::parse_hex(&client).ok_or("bad client id")?;
                let o = self.venue.status(client).ok_or("no such order")?;
                let (oid, qty) = (o.oid, qty.unwrap_or(o.remaining()));
                let f = self.venue.fill(now, oid, qty).ok_or("order is not open")?;
                self.pump();
                Ok(serde_json::to_value(wire_fill(&f)).unwrap_or_default())
            }
            Ctl::VenueCancel { client } => {
                let client = VenueClientId::parse_hex(&client).ok_or("bad client id")?;
                let oid = self.venue.status(client).ok_or("no such order")?.oid;
                let done = self.venue.venue_cancel(now, oid, "cancelled by venue");
                self.pump();
                Ok(serde_json::json!({ "cancelled": done }))
            }
            Ctl::ExternalFill {
                instrument,
                side,
                qty,
                px,
            } => {
                let f = self.venue.external_fill(now, &instrument, side, qty, px);
                self.pump();
                Ok(serde_json::to_value(wire_fill(&f)).unwrap_or_default())
            }
            Ctl::DropResponses { n } => {
                self.drop_responses = n;
                Ok(serde_json::Value::Null)
            }
            Ctl::DropRequests { n } => {
                self.drop_requests = n;
                Ok(serde_json::Value::Null)
            }
            Ctl::DelayMs { ms } => {
                self.delay_ms = ms;
                Ok(serde_json::Value::Null)
            }
            Ctl::AutoFill { mode } => {
                self.auto_fill = mode;
                Ok(serde_json::Value::Null)
            }
            Ctl::KickStreams => {
                let n = self.streams.len();
                self.streams.clear();
                Ok(serde_json::json!({ "closed": n }))
            }
            Ctl::RefuseStreams { on } => {
                self.refuse_streams = on;
                Ok(serde_json::Value::Null)
            }
            Ctl::RefPrice { instrument, px } => {
                self.broadcast(&StreamLine {
                    stream: StreamMsg::RefPrice { instrument, px, ts: now },
                });
                Ok(serde_json::Value::Null)
            }
            Ctl::AccountValue { value } => {
                self.venue.set_account_value(value);
                Ok(serde_json::Value::Null)
            }
            Ctl::VenueSetting {
                instrument,
                isolated,
                leverage,
            } => {
                self.venue_settings.insert(instrument, VenueSettingResult { isolated, leverage });
                Ok(serde_json::Value::Null)
            }
            Ctl::AccountMode { mode } => {
                self.account_mode = mode;
                Ok(serde_json::Value::Null)
            }
            Ctl::Dump => Ok(serde_json::json!({
                "orders": self.venue.orders.values().map(wire_order).collect::<Vec<_>>(),
                "positions": self.venue.positions.iter().map(|(i, q)| (i.to_string(), SQty(*q).to_string())).collect::<std::collections::BTreeMap<_, _>>(),
                "fills": self.venue.fills.len(),
                "place_requests": self.venue.place_requests.values().map(|n| u64::from(*n)).sum::<u64>(),
                "repeated_place_requests": self.venue.place_requests.values().filter(|n| **n > 1).count(),
                "duplicate_placements": self.venue.duplicate_placements.len(),
                "requests": self.requests,
                "margin_queries": self.margin_queries,
                "venue_setting_queries": self.venue_setting_queries,
                "account_mode_queries": self.account_mode_queries,
                "streams": self.streams.len(),
            })),
        }
    }
}

/// Tiny helper so `handle` can turn any serialisable value into JSON without
/// repeating the conversion at every arm.
mod erased {
    pub trait Ser {
        fn to_value(&self) -> serde_json::Value;
    }
    impl<T: serde::Serialize> Ser for T {
        fn to_value(&self) -> serde_json::Value {
            serde_json::to_value(self).unwrap_or_default()
        }
    }
}

async fn connection(stream: UnixStream, state: Arc<Mutex<State>>) {
    let (read, mut write) = stream.into_split();
    let mut lines = BufReader::new(read).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        let Ok(req) = serde_json::from_str::<SimReq>(&line) else {
            continue;
        };
        if matches!(req.op, SimOp::Subscribe) {
            let (tx, mut rx) = mpsc::unbounded_channel::<String>();
            let refused = {
                let mut s = state.lock().unwrap_or_else(|e| e.into_inner());
                if s.refuse_streams {
                    true
                } else {
                    s.streams.push(tx);
                    false
                }
            };
            let resp = SimResp {
                id: req.id,
                ok: !refused,
                result: serde_json::Value::Null,
                error: refused.then(|| "streams are refused".to_string()),
            };
            let mut out = serde_json::to_vec(&resp).unwrap_or_default();
            out.push(b'\n');
            if write.write_all(&out).await.is_err() || refused {
                return;
            }
            // From here on the connection only carries stream lines. It ends
            // when the venue drops the sender (KickStreams) or the peer goes.
            while let Some(text) = rx.recv().await {
                if write.write_all(text.as_bytes()).await.is_err() || write.write_all(b"\n").await.is_err() {
                    return;
                }
            }
            return;
        }

        let is_trade = matches!(req.op, SimOp::Place { .. } | SimOp::Cancel { .. });
        let (delay, drop_request) = {
            let mut s = state.lock().unwrap_or_else(|e| e.into_inner());
            s.requests += 1;
            let drop_request = is_trade && s.drop_requests > 0;
            if drop_request {
                s.drop_requests -= 1;
            }
            (if is_trade { s.delay_ms } else { 0 }, drop_request)
        };
        if drop_request {
            continue;
        }
        if delay > 0 {
            tokio::time::sleep(Duration::from_millis(delay)).await;
        }
        let (resp, swallow) = {
            let mut s = state.lock().unwrap_or_else(|e| e.into_inner());
            let r = s.handle(req.op);
            let swallow = is_trade && s.drop_responses > 0;
            if swallow {
                s.drop_responses -= 1;
            }
            let resp = match r {
                Ok(result) => SimResp {
                    id: req.id,
                    ok: true,
                    result,
                    error: None,
                },
                Err(e) => SimResp {
                    id: req.id,
                    ok: false,
                    result: serde_json::Value::Null,
                    error: Some(e),
                },
            };
            (resp, swallow)
        };
        if swallow {
            continue;
        }
        let mut out = serde_json::to_vec(&resp).unwrap_or_default();
        out.push(b'\n');
        if write.write_all(&out).await.is_err() {
            return;
        }
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let Some(path) = std::env::args().nth(1) else {
        eprintln!("usage: simvenue <socket-path>");
        std::process::exit(2);
    };
    let _ = std::fs::remove_file(&path);
    let listener = match UnixListener::bind(&path) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("bind {path}: {e}");
            std::process::exit(1);
        }
    };
    let state = Arc::new(Mutex::new(State {
        venue: SimVenue::new(true),
        streams: Vec::new(),
        drop_responses: 0,
        drop_requests: 0,
        delay_ms: 0,
        auto_fill: AutoFill::None,
        refuse_streams: false,
        requests: 0,
        margin_queries: 0,
        venue_settings: std::collections::BTreeMap::new(),
        account_mode: SIM_STANDARD_MODE.into(),
        venue_setting_queries: 0,
        account_mode_queries: 0,
    }));
    eprintln!("simvenue listening on {path}");
    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                tokio::spawn(connection(stream, state.clone()));
            }
            Err(e) => {
                eprintln!("accept: {e}");
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        }
    }
}
