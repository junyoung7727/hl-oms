//! Open-loop load generator for the local socket API.
//!
//! Every request has an *intended* send time fixed before the run starts:
//! `start + i / rate`. Latency is measured from that intended time to the
//! arrival of the response. If the generator or the server falls behind, the
//! delay is charged to the requests that were supposed to go out, instead of
//! silently lowering the offered rate (coordinated omission). The time from
//! the actual send to the response is reported separately, and so is how far
//! the generator itself ran behind its schedule.
//!
//! All samples are kept and percentiles are exact.

use crate::bench::summary;
use oms_api::*;
use oms_core::*;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub struct LoadCfg {
    pub socket: String,
    pub rate: u64,
    pub seconds: u64,
    pub connections: u64,
    pub strategy: String,
    pub run_id: String,
    /// Every n-th request repeats the key of the previous one.
    pub duplicate_every: u64,
    pub ttl_ms: u64,
}

#[derive(Default)]
struct ConnResult {
    intended_to_resp_us: Vec<u64>,
    sent_to_resp_us: Vec<u64>,
    send_lag_us: Vec<u64>,
    accepted: u64,
    rejected: u64,
    duplicate: u64,
    conflict: u64,
    errors: std::collections::BTreeMap<String, u64>,
    sent: u64,
    unanswered: u64,
}

fn now_us() -> u64 {
    oms_api::client::now_us()
}

fn connection(cfg: &LoadCfg, index: u64, start: Instant, per_conn: u64, interval: Duration) -> Result<ConnResult, String> {
    let stream = UnixStream::connect(Path::new(&cfg.socket)).map_err(|e| format!("connect: {e}"))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .map_err(|e| e.to_string())?;
    let mut writer = stream.try_clone().map_err(|e| e.to_string())?;
    let mut reader = BufReader::new(stream);
    let strategy = StrategyId::new(cfg.strategy.clone()).map_err(|e| e.to_string())?;
    let hello = serde_json::to_string(&Request {
        id: 0,
        op: Op::Hello {
            strategy: Some(strategy),
            client: "loadgen".into(),
        },
    })
    .map_err(|e| e.to_string())?;
    writer
        .write_all(format!("{hello}\n").as_bytes())
        .map_err(|e| e.to_string())?;
    let mut line = String::new();
    reader.read_line(&mut line).map_err(|e| e.to_string())?;
    if !line.contains("\"ok\":true") {
        return Err(format!("hello refused: {line}"));
    }

    // intended and actual send instants, indexed by request id - 1.
    type SendTimes = Vec<(Instant, Option<Instant>)>;
    let times: Arc<Mutex<SendTimes>> = Arc::new(Mutex::new(Vec::with_capacity(per_conn as usize)));
    let result = Arc::new(Mutex::new(ConnResult::default()));

    let (r_times, r_result) = (times.clone(), result.clone());
    let reader_thread = std::thread::spawn(move || {
        let mut line = String::new();
        let mut seen = 0u64;
        while seen < per_conn {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) | Err(_) => break,
                Ok(_) => {}
            }
            let got = Instant::now();
            let Ok(resp) = serde_json::from_str::<Response>(&line) else { continue };
            if resp.id == 0 {
                continue;
            }
            seen += 1;
            let (intended, sent) = {
                let t = r_times.lock().unwrap_or_else(|e| e.into_inner());
                match t.get(resp.id as usize - 1) {
                    Some(x) => *x,
                    None => continue,
                }
            };
            let mut r = r_result.lock().unwrap_or_else(|e| e.into_inner());
            r.intended_to_resp_us.push(got.saturating_duration_since(intended).as_micros() as u64);
            if let Some(sent) = sent {
                r.sent_to_resp_us.push(got.saturating_duration_since(sent).as_micros() as u64);
            }
            match (resp.ok, resp.result, resp.error) {
                (true, Some(v), _) => match serde_json::from_value::<NewOrderReply>(v) {
                    Ok(NewOrderReply::Accepted { .. }) => r.accepted += 1,
                    Ok(NewOrderReply::Rejected { .. }) => r.rejected += 1,
                    Ok(NewOrderReply::Duplicate { .. }) => r.duplicate += 1,
                    Ok(NewOrderReply::Conflict { .. }) => r.conflict += 1,
                    Err(_) => *r.errors.entry("unreadable".into()).or_default() += 1,
                },
                (_, _, Some(e)) => *r.errors.entry(format!("{:?}", e.code)).or_default() += 1,
                _ => *r.errors.entry("unreadable".into()).or_default() += 1,
            }
        }
    });

    let x = InstrumentId::new("X").map_err(|e| e.to_string())?;
    let mut last_key = String::new();
    for i in 0..per_conn {
        // Connections are staggered so that their sends interleave evenly.
        let intended = start + interval * (i as u32) + interval.mul_f64(index as f64 / cfg.connections as f64);
        let now = Instant::now();
        if intended > now {
            std::thread::sleep(intended - now);
        }
        let key = if cfg.duplicate_every > 0 && i % cfg.duplicate_every == cfg.duplicate_every - 1 && !last_key.is_empty() {
            last_key.clone()
        } else {
            format!("{}-{index}-{i}", cfg.run_id)
        };
        last_key = key.clone();
        let req = Request {
            id: i + 1,
            op: Op::NewOrder {
                client_order_id: ClientOrderId::new(key).map_err(|e| e.to_string())?,
                instrument: x.clone(),
                side: if i % 2 == 0 { Side::Buy } else { Side::Sell },
                qty: Qty(1_000_000),
                px: Px(100 * 100_000_000),
                tif: Tif::Gtc,
                reduce_only: false,
                expires_at: now_us() + cfg.ttl_ms * 1000,
            },
        };
        let mut bytes = serde_json::to_vec(&req).map_err(|e| e.to_string())?;
        bytes.push(b'\n');
        times.lock().unwrap_or_else(|e| e.into_inner()).push((intended, None));
        if writer.write_all(&bytes).is_err() {
            break;
        }
        let sent = Instant::now();
        {
            let mut t = times.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(slot) = t.get_mut(i as usize) {
                slot.1 = Some(sent);
            }
        }
        let mut r = result.lock().unwrap_or_else(|e| e.into_inner());
        r.sent += 1;
        r.send_lag_us.push(sent.saturating_duration_since(intended).as_micros() as u64);
    }
    let _ = reader_thread.join();
    let mut r = std::mem::take(&mut *result.lock().unwrap_or_else(|e| e.into_inner()));
    r.unanswered = r.sent - r.intended_to_resp_us.len() as u64;
    Ok(r)
}

pub fn run(cfg: LoadCfg) -> Result<serde_json::Value, String> {
    let total = cfg.rate * cfg.seconds;
    let per_conn = total / cfg.connections;
    // Each connection sends at rate / connections.
    let interval = Duration::from_nanos(1_000_000_000 * cfg.connections / cfg.rate.max(1));
    let cfg = Arc::new(cfg);
    let start = Instant::now() + Duration::from_millis(200);
    let wall_start = now_us();
    let handles: Vec<_> = (0..cfg.connections)
        .map(|index| {
            let cfg = cfg.clone();
            std::thread::spawn(move || connection(&cfg, index, start, per_conn, interval))
        })
        .collect();
    let mut all = ConnResult::default();
    for h in handles {
        let r = h.join().map_err(|_| "worker panicked".to_string())??;
        all.intended_to_resp_us.extend(r.intended_to_resp_us);
        all.sent_to_resp_us.extend(r.sent_to_resp_us);
        all.send_lag_us.extend(r.send_lag_us);
        all.accepted += r.accepted;
        all.rejected += r.rejected;
        all.duplicate += r.duplicate;
        all.conflict += r.conflict;
        all.sent += r.sent;
        all.unanswered += r.unanswered;
        for (k, v) in r.errors {
            *all.errors.entry(k).or_default() += v;
        }
    }
    let elapsed = start.elapsed().as_secs_f64();
    println!(
        "# offered {} req/s for {} s over {} connections: sent {} answered {} in {:.2} s",
        cfg.rate,
        cfg.seconds,
        cfg.connections,
        all.sent,
        all.intended_to_resp_us.len(),
        elapsed
    );
    let lat = summary("intended send -> response", all.intended_to_resp_us.clone(), "us");
    let svc = summary("actual send -> response", all.sent_to_resp_us.clone(), "us");
    let lag = summary("generator lag (actual - intended send)", all.send_lag_us.clone(), "us");
    Ok(serde_json::json!({
        "offered_rate": cfg.rate,
        "seconds": cfg.seconds,
        "connections": cfg.connections,
        "duplicate_every": cfg.duplicate_every,
        "wall_start_us": wall_start,
        "sent": all.sent,
        "answered": all.intended_to_resp_us.len(),
        "unanswered": all.unanswered,
        "achieved_rate": (all.intended_to_resp_us.len() as f64 / elapsed).round(),
        "accepted": all.accepted,
        "rejected": all.rejected,
        "duplicate": all.duplicate,
        "conflict": all.conflict,
        "errors": all.errors,
        "latency_intended_to_response": lat,
        "latency_sent_to_response": svc,
        "generator_lag": lag,
    }))
}
