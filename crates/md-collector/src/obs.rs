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
    let st = inner.ingest.health();
    let venue = inner.cfg.venue.as_str();
    let ts = now_ns / 1_000_000;
    let v = [("venue", venue)];
    let mut out = vec![
        sample("lake_l0_open_files", &v, st.open_files as f64, ts),
        // Payload bytes before zstd (review 8: not the bytes on disk).
        sample("lake_l0_bytes_total", &v, st.bytes_total as f64, ts),
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
    use tokio::time::MissedTickBehavior::Delay;
    // The slow readers run on their own (review 6): chronyc may take 2 s
    // and the lake listing touches the disk.
    tokio::spawn(clock_offset_loop(inner.clone(), obs.clone(), t.slow_ms));
    tokio::spawn(seal_lag_loop(inner.clone(), obs.clone()));
    let mut fast = tokio::time::interval(Duration::from_millis(t.fast_ms));
    let mut slow = tokio::time::interval(Duration::from_millis(t.slow_ms));
    // After a delay, one summary, not a burst of them (review 6).
    fast.set_missed_tick_behavior(Delay);
    slow.set_missed_tick_behavior(Delay);
    loop {
        tokio::select! {
            _ = fast.tick() => {
                let samples = summary(&inner, Period::Fast).await;
                obs.metrics(samples);
            }
            _ = slow.tick() => {
                // The heartbeat and the lake metrics before the summary,
                // which waits for the feeds' lock (review 6).
                let (lake, hb) = lake_samples(&inner, crate::wall_ns());
                if let Some(hb) = hb {
                    obs.priority(hb);
                }
                obs.metrics(lake);
                let samples = summary(&inner, Period::Slow).await;
                obs.metrics(samples);
            }
        }
    }
}

async fn clock_offset_loop(inner: Arc<Inner>, obs: Arc<ObsHandle>, every_ms: u64) {
    let Some(c) = inner.clock_offset.clone() else {
        return;
    };
    let host = inner.cfg.host.clone();
    let mut t = tokio::time::interval(Duration::from_millis(every_ms));
    t.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        t.tick().await;
        let c = c.clone();
        let off = tokio::task::spawn_blocking(move || c.offset_ms())
            .await
            .ok()
            .flatten();
        if let Some(ms) = off {
            // One series per host (review 10: not per venue).
            let ts = crate::wall_ns() / 1_000_000;
            obs.metric(sample("node_clock_offset_ms", &[("host", &host)], ms, ts));
        }
    }
}

async fn seal_lag_loop(inner: Arc<Inner>, obs: Arc<ObsHandle>) {
    let venue = inner.cfg.venue.clone();
    let mut t = tokio::time::interval(Duration::from_secs(60));
    t.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        t.tick().await;
        let (fs, v) = (inner.fs.clone(), venue.clone());
        let now = crate::wall_ns();
        let r = tokio::task::spawn_blocking(move || {
            md_lake::seal::unsealed_hours(fs.as_ref(), &v, now, SEAL_LOOKBACK_HOURS)
        })
        .await;
        if let Ok(Ok(hours)) = r {
            let cur = md_lake::time::hour_start(now);
            let lag = hours
                .first()
                .map_or(0, |h| (cur - h) / md_lake::time::HOUR_NS);
            obs.metric(sample(
                "lake_seal_lag_hours",
                &[("venue", &venue)],
                lag as f64,
                now / 1_000_000,
            ));
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

/// A writer beat older than this is a stall alert (review 2: half the
/// 30 s watchdog, so the message goes before systemd kills the process).
pub const STALL_S: i64 = 15;

/// What the L0 write conditions are now (spec D8, R3, review 1, 2).
/// `ok_for_ms`: how long this session has run with beats and no failure;
/// the failure clears only after `clear_after_ms` of that (past the first
/// fsync), so a disk that fails at the first flush or fsync cannot send a
/// clear and a new start on every restart.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct L0Conditions {
    /// `None`: leave the condition as it is.
    pub l0_write: Option<Option<String>>,
    pub writer_stall: Option<String>,
}

pub fn l0_conditions(
    h: &crate::ingest::Health,
    now_s: i64,
    ok_for_ms: u64,
    clear_after_ms: u64,
) -> L0Conditions {
    let l0_write = match &h.failed {
        Some(e) => Some(Some(write_failure_text(
            h.failed_os_error.as_deref().unwrap_or(""),
            e,
        ))),
        None if h.beats > 0 && ok_for_ms >= clear_after_ms => Some(None),
        None => None,
    };
    let writer_stall = match (h.failed.is_none(), h.heartbeat_s) {
        (true, Some(hb)) if now_s - hb > STALL_S => Some(format!(
            "L0 쓰기 멈춤: 쓰기 루프가 {}초 동안 응답 없음 — 30초가 되면 systemd가 수집기를 다시 시작함",
            now_s - hb
        )),
        _ => None,
    };
    L0Conditions {
        l0_write,
        writer_stall,
    }
}

/// The alert task: every second, the L0 write conditions.
pub(crate) async fn alerts(inner: Arc<Inner>, alert: Arc<AlertHandle>) {
    let clear_after_ms = inner.cfg.write.fsync_ms.saturating_add(5000);
    let mut t = tokio::time::interval(Duration::from_secs(1));
    let mut ok_since: Option<Instant> = None;
    loop {
        t.tick().await;
        let h = inner.ingest.health();
        if h.failed.is_some() || h.beats == 0 {
            ok_since = None;
        } else if ok_since.is_none() {
            ok_since = Some(Instant::now());
        }
        let ok_for = ok_since.map_or(0, |s| s.elapsed().as_millis() as u64);
        let c = l0_conditions(&h, crate::wall_ns() / 1_000_000_000, ok_for, clear_after_ms);
        if let Some(v) = c.l0_write {
            alert.set("l0_write", v);
        }
        alert.set("writer_stall", c.writer_stall);
    }
}
