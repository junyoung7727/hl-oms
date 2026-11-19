//! The collector's way out (U6 spec D4–D10): logs to stderr and
//! VictoriaLogs, receive lag from the feeds, the summary and lake metrics
//! to VictoriaMetrics, the clock offset, and the direct `[MD P1]` alerts.
//! Nothing here can block frame ingestion: obs-push and alert-direct only
//! take a lock, and the slow readers (chronyc, the lake listing) run on
//! blocking threads.

use std::collections::BTreeMap;
use std::io::Read;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use alert_direct::AlertHandle;
use md_core::{Normalized, StreamKey};
use md_feed::MessageSink;
use obs_push::{LogLine, ObsHandle, Sample};
use serde_json::{Value, json};

use crate::collector::{Inner, Running};
use crate::telemetry::{BookInfo, BookSource, Period, Telemetry, TopicKey};

/// The VictoriaLogs service name of the collector.
pub const SERVICE: &str = "md-collector";
/// How far back `lake_seal_lag_hours` looks (spec D7: the day seal's 7 days).
pub const SEAL_LOOKBACK_HOURS: u32 = 168;

/// Log lines: one JSON line on stderr (as oms-node), and the same entry to
/// VictoriaLogs when a push is configured (spec D10).
pub struct LogSink {
    pub obs: Option<Arc<ObsHandle>>,
    pub host: String,
    pub venue: String,
}

impl LogSink {
    pub fn stderr_only(venue: &str) -> LogSink {
        LogSink {
            obs: None,
            host: String::new(),
            venue: venue.to_string(),
        }
    }

    pub fn log(&self, level: &str, msg: &str, fields: Value) {
        let now = crate::wall_ns();
        eprintln!(
            "{}",
            json!({"ts": now / 1000, "level": level, "msg": msg, "fields": fields})
        );
        if let Some(o) = &self.obs {
            let mut f = serde_json::Map::new();
            f.insert("level".into(), level.into());
            match fields {
                Value::Object(m) => {
                    for (k, v) in m {
                        f.insert(k, v);
                    }
                }
                Value::Null => {}
                v => {
                    f.insert("fields".into(), v);
                }
            }
            o.log(LogLine {
                time_ns: now,
                msg: msg.to_string(),
                service: SERVICE.into(),
                host: self.host.clone(),
                venue: self.venue.clone(),
                fields: f,
            });
        }
    }
}

/// The message sink of one market's feeds: receive lag (spec D4).
pub struct LagSink {
    pub market: String,
    pub telemetry: Arc<Telemetry>,
}

impl MessageSink for LagSink {
    fn message(&self, _conn_id: u32, msg: &Normalized) {
        match msg {
            Normalized::Trade(t) => {
                self.telemetry
                    .lag(&self.market, &t.key, t.venue_ts_ns, t.recv_ns)
            }
            Normalized::Ticker(t) => {
                self.telemetry
                    .lag(&self.market, &t.key, t.venue_ts_ns, t.recv_ns)
            }
            _ => {}
        }
    }

    fn book_msg(&self, _conn_id: u32, key: &StreamKey, venue_ts_ns: i64, recv_ns: i64) {
        self.telemetry.lag(&self.market, key, venue_ts_ns, recv_ns);
    }
}

/// The books of the running feeds, for one summary (spec R6: called after
/// the telemetry shard is released).
pub(crate) struct GroupBooks<'a> {
    pub groups: &'a [Running],
    pub index: BTreeMap<TopicKey, usize>,
    pub exps: &'a BTreeMap<(String, String), (i8, i8)>,
}

impl<'a> GroupBooks<'a> {
    pub fn new(groups: &'a [Running], exps: &'a BTreeMap<(String, String), (i8, i8)>) -> Self {
        let mut index = BTreeMap::new();
        for (i, r) in groups.iter().enumerate() {
            if r.handle.is_none() {
                continue;
            }
            for k in r.group.topics() {
                index.insert(TopicKey::new(&r.group.market, &k), i);
            }
        }
        GroupBooks {
            groups,
            index,
            exps,
        }
    }
}

impl BookSource for GroupBooks<'_> {
    fn book(&self, t: &TopicKey) -> Option<BookInfo> {
        let r = self.groups.get(*self.index.get(t)?)?;
        let key = StreamKey::new(t.channel.clone(), t.symbol.clone());
        r.handle
            .as_ref()?
            .read_book(&key, |v| BookInfo::from_book(v.book, v.stale))
    }

    fn exps(&self, market: &str, symbol: &str) -> Option<(i8, i8)> {
        self.exps
            .get(&(market.to_string(), symbol.to_string()))
            .copied()
    }
}

/// The clock offset to NTP in ms (spec D6); `None` when unknown.
pub trait ClockOffset: Send + Sync {
    fn offset_ms(&self) -> Option<f64>;
}

/// `chronyc -c tracking`, field 5: the "System time" offset in seconds;
/// positive = the system clock is behind NTP (measured on the Linux PC,
/// 2026-11-19). Killed after 2 s.
pub struct Chrony;

impl ClockOffset for Chrony {
    fn offset_ms(&self) -> Option<f64> {
        let mut child = std::process::Command::new("chronyc")
            .args(["-c", "tracking"])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .ok()?;
        let end = Instant::now() + Duration::from_secs(2);
        loop {
            match child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) if Instant::now() < end => std::thread::sleep(Duration::from_millis(20)),
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
            }
        }
        let mut out = String::new();
        child.stdout.take()?.read_to_string(&mut out).ok()?;
        parse_chrony(&out)
    }
}

/// Field 5 of the CSV, in ms.
pub fn parse_chrony(csv: &str) -> Option<f64> {
    let v: f64 = csv.lines().next()?.split(',').nth(4)?.trim().parse().ok()?;
    v.is_finite().then_some(v * 1000.0)
}

fn sample(name: &str, labels: &[(&str, &str)], value: f64, ts_ms: i64) -> Sample {
    Sample {
        name: name.into(),
        labels: labels
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
        value,
        ts_ms,
    }
}

/// The lake and process metrics of the slow period (spec D4).
pub(crate) fn lake_samples(inner: &Inner, now_ns: i64) -> (Vec<Sample>, Option<Sample>) {
    let st = inner.ingest.state();
    let venue = inner.cfg.venue.as_str();
    let ts = now_ns / 1_000_000;
    let v = [("venue", venue)];
    let mut out = vec![
        sample("lake_l0_open_files", &v, st.open_files as f64, ts),
        sample(
            "lake_l0_bytes_total",
            &v,
            st.conns.values().map(|c| c.bytes_written).sum::<u64>() as f64,
            ts,
        ),
        sample(
            "lake_l0_write_errors_total",
            &v,
            f64::from(u8::from(st.failed.is_some())),
            ts,
        ),
    ];
    if inner.counters.free_known.load(Ordering::Relaxed) {
        out.push(sample(
            "lake_disk_free_bytes",
            &v,
            inner.counters.free_bytes.load(Ordering::Relaxed) as f64,
            ts,
        ));
    }
    for (slot, bytes) in &st.slot_bytes {
        out.push(sample(
            "md_queue_bytes",
            &[("venue", venue), ("conn", &slot.to_string())],
            *bytes as f64,
            ts,
        ));
    }
    let hb = st
        .heartbeat_s
        .map(|s| sample("md_collector_heartbeat_ts", &v, s as f64, ts));
    (out, hb)
}

/// The summary task: fast and slow periods, the clock offset, the seal lag.
pub(crate) async fn run(inner: Arc<Inner>) {
    let Some(obs) = inner.obs.clone() else { return };
    let Some(t) = inner.cfg.telemetry.clone() else {
        return;
    };
    let mut fast = tokio::time::interval(Duration::from_millis(t.fast_ms));
    let mut slow = tokio::time::interval(Duration::from_millis(t.slow_ms));
    let mut seal = tokio::time::interval(Duration::from_secs(60));
    let host = inner.cfg.host.clone();
    let venue = inner.cfg.venue.clone();
    loop {
        tokio::select! {
            _ = fast.tick() => {
                let samples = summary(&inner, Period::Fast).await;
                obs.metrics(samples);
            }
            _ = slow.tick() => {
                let mut samples = summary(&inner, Period::Slow).await;
                let now = crate::wall_ns();
                let (lake, hb) = lake_samples(&inner, now);
                samples.extend(lake);
                if let Some(hb) = hb {
                    obs.priority(hb);
                }
                if let Some(c) = inner.clock_offset.clone() {
                    let off = tokio::task::spawn_blocking(move || c.offset_ms()).await.ok().flatten();
                    if let Some(ms) = off {
                        samples.push(sample("node_clock_offset_ms", &[("venue", &venue), ("host", &host)], ms, now / 1_000_000));
                    }
                }
                obs.metrics(samples);
            }
            _ = seal.tick() => {
                let (fs, v) = (inner.fs.clone(), venue.clone());
                let now = crate::wall_ns();
                let r = tokio::task::spawn_blocking(move || {
                    md_lake::seal::unsealed_hours(fs.as_ref(), &v, now, SEAL_LOOKBACK_HOURS)
                }).await;
                if let Ok(Ok(hours)) = r {
                    let cur = md_lake::time::hour_start(now);
                    let lag = hours.first().map_or(0, |h| (cur - h) / md_lake::time::HOUR_NS);
                    obs.metric(sample("lake_seal_lag_hours", &[("venue", &venue)], lag as f64, now / 1_000_000));
                }
            }
        }
    }
}

async fn summary(inner: &Inner, p: Period) -> Vec<Sample> {
    let groups = inner.groups.lock().await;
    let books = GroupBooks::new(&groups, &inner.exps);
    inner.telemetry.summary(crate::wall_ns(), p, &books)
}

/// The text of an L0 write failure (spec D8, R7).
pub fn write_failure_text(os: &str, error: &str) -> String {
    format!("L0 쓰기 실패: {os} ({error}) — 수집기가 종료되고 systemd가 다시 시작함")
}

/// The alert task (spec D8, R3): every second, the L0 write condition. A
/// failure holds it; the first beat of a session without a failure clears
/// it (a restart that works).
pub(crate) async fn alerts(inner: Arc<Inner>, alert: Arc<AlertHandle>) {
    let mut t = tokio::time::interval(Duration::from_secs(1));
    loop {
        t.tick().await;
        let st = inner.ingest.state();
        match &st.failed {
            Some(e) => {
                let os = st.failed_os_error.clone().unwrap_or_default();
                alert.set("l0_write", Some(write_failure_text(&os, e)));
            }
            None if st.beats > 0 => alert.set("l0_write", None),
            None => {}
        }
    }
}
