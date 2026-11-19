//! Push to VictoriaMetrics and VictoriaLogs (unit U6 spec D2), bounded and
//! non-blocking. The caller's [`ObsHandle::metric`] and [`ObsHandle::log`]
//! only take a lock and append; one thread named `obs-push` encodes and
//! sends. A full queue drops its oldest line and counts it; a failed request
//! puts its batch back at the front and is retried after a backoff, so the
//! hot path never waits for a store and memory stays bounded.
//!
//! Formats (facts file V17, L12):
//! - VictoriaMetrics `/api/v1/import`, one JSON line per sample:
//!   `{"metric":{"__name__":..,labels},"values":[v],"timestamps":[ms]}`.
//! - VictoriaLogs `/insert/jsonline?_stream_fields=service,host,venue`, one
//!   JSON line per entry with `_time` (RFC 3339, ns) and `_msg`.

use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

/// One VictoriaMetrics sample.
#[derive(Debug, Clone, PartialEq)]
pub struct Sample {
    pub name: String,
    pub labels: BTreeMap<String, String>,
    pub value: f64,
    pub ts_ms: i64,
}

/// One VictoriaLogs entry.
#[derive(Debug, Clone, PartialEq)]
pub struct LogLine {
    pub time_ns: i64,
    pub msg: String,
    pub service: String,
    pub host: String,
    pub venue: String,
    pub fields: serde_json::Map<String, serde_json::Value>,
}

fn quote(s: &str) -> String {
    serde_json::Value::String(s.to_string()).to_string()
}

/// The JSON line of a sample; `None` for a value that is not finite.
pub fn vm_line(s: &Sample) -> Option<String> {
    if !s.value.is_finite() {
        return None;
    }
    let mut out = String::with_capacity(64 + 32 * s.labels.len());
    out.push_str(r#"{"metric":{"__name__":"#);
    out.push_str(&quote(&s.name));
    for (k, v) in s.labels.iter().filter(|(k, _)| k.as_str() != "__name__") {
        out.push(',');
        out.push_str(&quote(k));
        out.push(':');
        out.push_str(&quote(v));
    }
    out.push_str(r#"},"values":["#);
    out.push_str(&serde_json::Value::from(s.value).to_string());
    out.push_str(r#"],"timestamps":["#);
    out.push_str(&s.ts_ms.to_string());
    out.push_str("]}");
    Some(out)
}

/// RFC 3339 UTC with nanoseconds, e.g. `2025-11-19T05:40:02.123456789Z`.
pub fn rfc3339_ns(ns: i64) -> String {
    let secs = ns.div_euclid(1_000_000_000);
    let frac = ns.rem_euclid(1_000_000_000);
    let days = secs.div_euclid(86_400);
    let sec = secs.rem_euclid(86_400);
    // Civil date from days since 1970-01-01 (H. Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{frac:09}Z",
        sec / 3600,
        sec / 60 % 60,
        sec % 60
    )
}

/// Keys the line itself sets; a field with one of these names is written as
/// `fields.<name>` so that it cannot replace them.
const RESERVED: [&str; 5] = ["_time", "_msg", "service", "host", "venue"];

/// The JSON line of a log entry: fixed keys first, then the fields sorted.
pub fn vl_line(l: &LogLine) -> String {
    let mut out = String::with_capacity(128);
    out.push_str(r#"{"_time":"#);
    out.push_str(&quote(&rfc3339_ns(l.time_ns)));
    for (k, v) in [
        ("_msg", &l.msg),
        ("service", &l.service),
        ("host", &l.host),
        ("venue", &l.venue),
    ] {
        out.push(',');
        out.push_str(&quote(k));
        out.push(':');
        out.push_str(&quote(v));
    }
    let mut fields: Vec<(String, &serde_json::Value)> = l
        .fields
        .iter()
        .map(|(k, v)| {
            if RESERVED.contains(&k.as_str()) {
                (format!("fields.{k}"), v)
            } else {
                (k.clone(), v)
            }
        })
        .collect();
    fields.sort_by(|a, b| a.0.cmp(&b.0));
    for (k, v) in fields {
        out.push(',');
        out.push_str(&quote(&k));
        out.push(':');
        out.push_str(&v.to_string());
    }
    out.push('}');
    out
}

#[derive(Debug, Clone)]
pub struct ObsConfig {
    /// e.g. `http://127.0.0.1:8428`; `None`: metrics are not pushed.
    pub vm_url: Option<String>,
    /// e.g. `http://127.0.0.1:9428`; `None`: logs are not pushed.
    pub vl_url: Option<String>,
    /// Lines each queue holds.
    pub queue_lines: usize,
    /// Lines per request.
    pub batch_lines: usize,
    pub timeout: Duration,
    /// How often the sender looks at the queues.
    pub flush_every: Duration,
    pub backoff_min: Duration,
    pub backoff_max: Duration,
    /// How often `obs_push_dropped_total` and `obs_push_queue_lines` are pushed.
    pub self_every: Duration,
    /// Labels of the self metrics (e.g. service, venue).
    pub self_labels: BTreeMap<String, String>,
}

impl ObsConfig {
    pub fn new(vm_url: Option<String>, vl_url: Option<String>) -> ObsConfig {
        ObsConfig {
            vm_url,
            vl_url,
            queue_lines: 100_000,
            batch_lines: 10_000,
            timeout: Duration::from_secs(5),
            flush_every: Duration::from_secs(1),
            backoff_min: Duration::from_secs(1),
            backoff_max: Duration::from_secs(30),
            self_every: Duration::from_secs(10),
            self_labels: BTreeMap::new(),
        }
    }
}

/// One endpoint's counters.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EndpointState {
    pub queued: usize,
    pub sent: u64,
    pub dropped: u64,
    pub failed_requests: u64,
    /// Samples not pushed because the value was not finite.
    pub invalid: u64,
    /// Lines the store refused for good (400, 413, 422): dropped (R11).
    pub rejected: u64,
    /// Lines in a request now.
    pub in_flight: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ObsState {
    pub vm: EndpointState,
    /// The small VictoriaMetrics queue sent first (heartbeat, self metrics).
    pub vm_priority: EndpointState,
    pub vl: EndpointState,
}

/// Lines the priority queue holds (R11).
pub const PRIORITY_LINES: usize = 1000;

struct Queue<T> {
    on: bool,
    items: VecDeque<T>,
    limit: usize,
    st: EndpointState,
    retry_at: Option<Instant>,
    backoff: Duration,
}

impl<T> Queue<T> {
    fn new(on: bool, limit: usize, backoff: Duration) -> Queue<T> {
        Queue {
            on,
            items: VecDeque::new(),
            limit: limit.max(1),
            st: EndpointState::default(),
            retry_at: None,
            backoff,
        }
    }

    fn push(&mut self, item: T) {
        if !self.on {
            return;
        }
        self.items.push_back(item);
        self.trim();
    }

    fn trim(&mut self) {
        while self.items.len() > self.limit {
            self.items.pop_front();
            self.st.dropped += 1;
        }
    }

    fn take(&mut self, n: usize) -> Vec<T> {
        let n = n.min(self.items.len());
        let batch: Vec<T> = self.items.drain(..n).collect();
        self.st.in_flight = batch.len();
        batch
    }

    /// A failed batch goes back to the front; overflow drops the oldest.
    fn put_back(&mut self, batch: Vec<T>) {
        for item in batch.into_iter().rev() {
            self.items.push_front(item);
        }
        self.trim();
    }

    fn state(&self) -> EndpointState {
        EndpointState {
            queued: self.items.len(),
            ..self.st.clone()
        }
    }
}

struct Inner {
    vm: Queue<Sample>,
    vmp: Queue<Sample>,
    vl: Queue<LogLine>,
    wake: bool,
}

struct Shared {
    inner: Mutex<Inner>,
    cv: Condvar,
    stop: AtomicBool,
}

/// The push handle. Dropping it stops the sender thread after its request.
pub struct ObsHandle {
    shared: Arc<Shared>,
    batch: usize,
}

pub struct ObsPush;

impl ObsPush {
    pub fn start(cfg: ObsConfig) -> Result<ObsHandle, String> {
        let client = reqwest::Client::builder()
            .timeout(cfg.timeout)
            .connect_timeout(cfg.timeout)
            .no_proxy()
            .build()
            .map_err(|e| format!("cannot build the push client: {}", e.without_url()))?;
        let vm_url = match &cfg.vm_url {
            Some(u) => Some(endpoint(u, "api/v1/import")?),
            None => None,
        };
        let vl_url = match &cfg.vl_url {
            Some(u) => Some(endpoint(
                u,
                "insert/jsonline?_stream_fields=service,host,venue",
            )?),
            None => None,
        };
        let shared = Arc::new(Shared {
            inner: Mutex::new(Inner {
                vm: Queue::new(vm_url.is_some(), cfg.queue_lines, cfg.backoff_min),
                vmp: Queue::new(vm_url.is_some(), PRIORITY_LINES, cfg.backoff_min),
                vl: Queue::new(vl_url.is_some(), cfg.queue_lines, cfg.backoff_min),
                wake: false,
            }),
            cv: Condvar::new(),
            stop: AtomicBool::new(false),
        });
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| format!("cannot start the push runtime: {e}"))?;
        let s = shared.clone();
        let batch = cfg.batch_lines.max(1);
        std::thread::Builder::new()
            .name("obs-push".into())
            .spawn(move || run(rt, s, client, vm_url, vl_url, cfg))
            .map_err(|e| format!("cannot start the push thread: {e}"))?;
        Ok(ObsHandle { shared, batch })
    }
}

fn endpoint(base: &str, path: &str) -> Result<reqwest::Url, String> {
    let b = base.trim_end_matches('/');
    reqwest::Url::parse(&format!("{b}/{path}")).map_err(|e| format!("bad url {b}: {e}"))
}

impl ObsHandle {
    fn with(&self, f: impl FnOnce(&mut Inner) -> bool) {
        if let Ok(mut i) = self.shared.inner.lock() {
            if f(&mut i) {
                i.wake = true;
                self.shared.cv.notify_all();
            }
        }
    }

    /// Queues one sample (never waits).
    pub fn metric(&self, s: Sample) {
        let batch = self.batch;
        self.with(|i| {
            if !i.vm.on {
                return false;
            }
            if !s.value.is_finite() {
                i.vm.st.invalid += 1;
                return false;
            }
            i.vm.push(s);
            i.vm.items.len() == batch
        });
    }

    /// Queues one sample in the small queue that is sent first (R11): the
    /// heartbeat must not wait behind thousands of summary lines.
    pub fn priority(&self, s: Sample) {
        self.with(|i| {
            if !i.vmp.on {
                return false;
            }
            if !s.value.is_finite() {
                i.vmp.st.invalid += 1;
                return false;
            }
            i.vmp.push(s);
            true
        });
    }

    pub fn metrics(&self, v: Vec<Sample>) {
        let batch = self.batch;
        self.with(|i| {
            if !i.vm.on {
                return false;
            }
            for s in v {
                if s.value.is_finite() {
                    i.vm.push(s);
                } else {
                    i.vm.st.invalid += 1;
                }
            }
            i.vm.items.len() >= batch
        });
    }

    /// Queues one log entry (never waits).
    pub fn log(&self, l: LogLine) {
        let batch = self.batch;
        self.with(|i| {
            i.vl.push(l);
            i.vl.on && i.vl.items.len() == batch
        });
    }

    pub fn state(&self) -> ObsState {
        match self.shared.inner.lock() {
            Ok(i) => ObsState {
                vm: i.vm.state(),
                vm_priority: i.vmp.state(),
                vl: i.vl.state(),
            },
            Err(_) => ObsState::default(),
        }
    }

    /// Waits until both queues are empty and nothing is in flight, or the
    /// time is up; `true` when everything was sent.
    pub fn flush_for(&self, d: Duration) -> bool {
        let end = Instant::now() + d;
        let Ok(mut i) = self.shared.inner.lock() else {
            return false;
        };
        i.wake = true;
        self.shared.cv.notify_all();
        loop {
            let idle = |q: &EndpointState, n: usize| n == 0 && q.in_flight == 0;
            if idle(&i.vm.st, i.vm.items.len())
                && idle(&i.vmp.st, i.vmp.items.len())
                && idle(&i.vl.st, i.vl.items.len())
            {
                return true;
            }
            let now = Instant::now();
            if now >= end {
                return false;
            }
            match self
                .shared
                .cv
                .wait_timeout(i, (end - now).min(Duration::from_millis(20)))
            {
                Ok((g, _)) => {
                    i = g;
                    i.wake = true;
                    self.shared.cv.notify_all();
                }
                Err(_) => return false,
            }
        }
    }
}

impl Drop for ObsHandle {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        self.shared.cv.notify_all();
    }
}

#[derive(Clone, Copy)]
enum Which {
    Vm,
    VmPriority,
    Vl,
}

fn run(
    rt: tokio::runtime::Runtime,
    shared: Arc<Shared>,
    client: reqwest::Client,
    vm_url: Option<reqwest::Url>,
    vl_url: Option<reqwest::Url>,
    cfg: ObsConfig,
) {
    let mut next_self = Instant::now() + cfg.self_every;
    loop {
        {
            let Ok(mut i) = shared.inner.lock() else {
                return;
            };
            if !i.wake {
                match shared.cv.wait_timeout(i, cfg.flush_every) {
                    Ok((g, _)) => i = g,
                    Err(_) => return,
                }
            }
            i.wake = false;
        }
        if shared.stop.load(Ordering::SeqCst) {
            return;
        }
        if vm_url.is_some() && Instant::now() >= next_self {
            next_self = Instant::now() + cfg.self_every;
            push_self_metrics(&shared, &cfg.self_labels);
        }
        for (which, url) in [
            (Which::VmPriority, &vm_url),
            (Which::Vm, &vm_url),
            (Which::Vl, &vl_url),
        ] {
            let Some(url) = url else { continue };
            // Send batches until the queue is empty or a request fails.
            loop {
                let body = {
                    let Ok(mut i) = shared.inner.lock() else {
                        return;
                    };
                    let now = Instant::now();
                    match which {
                        Which::Vm => take_vm(&mut i.vm, cfg.batch_lines, now),
                        Which::VmPriority => take_vm(&mut i.vmp, cfg.batch_lines, now),
                        Which::Vl => take_vl(&mut i.vl, cfg.batch_lines, now),
                    }
                };
                let Some((n, body, batch)) = body else { break };
                let ok = rt.block_on(post(&client, url.clone(), body));
                let Ok(mut i) = shared.inner.lock() else {
                    return;
                };
                let done = match (which, batch) {
                    (Which::Vm, Batch::Vm(b)) => finish(&mut i.vm, b, n, ok, &cfg),
                    (Which::VmPriority, Batch::Vm(b)) => finish(&mut i.vmp, b, n, ok, &cfg),
                    (Which::Vl, Batch::Vl(b)) => finish(&mut i.vl, b, n, ok, &cfg),
                    _ => false,
                };
                shared.cv.notify_all();
                if !done || shared.stop.load(Ordering::SeqCst) {
                    break;
                }
            }
        }
    }
}

enum Batch {
    Vm(Vec<Sample>),
    Vl(Vec<LogLine>),
}

fn ready<T>(q: &Queue<T>, now: Instant) -> bool {
    !q.items.is_empty() && q.retry_at.is_none_or(|t| now >= t)
}

fn take_vm(q: &mut Queue<Sample>, n: usize, now: Instant) -> Option<(usize, String, Batch)> {
    if !ready(q, now) {
        return None;
    }
    let batch = q.take(n);
    let body: Vec<String> = batch.iter().filter_map(vm_line).collect();
    Some((batch.len(), body.join("\n") + "\n", Batch::Vm(batch)))
}

fn take_vl(q: &mut Queue<LogLine>, n: usize, now: Instant) -> Option<(usize, String, Batch)> {
    if !ready(q, now) {
        return None;
    }
    let batch = q.take(n);
    let body: Vec<String> = batch.iter().map(vl_line).collect();
    Some((batch.len(), body.join("\n") + "\n", Batch::Vl(batch)))
}

/// Applies a request result; `true` when the next batch may go now.
fn finish<T>(q: &mut Queue<T>, batch: Vec<T>, n: usize, r: Post, cfg: &ObsConfig) -> bool {
    q.st.in_flight = 0;
    match r {
        Post::Ok => {
            q.st.sent += n as u64;
            q.retry_at = None;
            q.backoff = cfg.backoff_min;
            true
        }
        Post::Rejected(status) => {
            // Sending it again cannot help and would block every later line.
            q.st.rejected += n as u64;
            q.retry_at = None;
            q.backoff = cfg.backoff_min;
            eprintln!(
                "{}",
                serde_json::json!({"level": "warn", "msg": "store refused a batch", "fields": {"status": status, "lines": n}})
            );
            true
        }
        Post::Failed => {
            q.st.failed_requests += 1;
            q.put_back(batch);
            q.retry_at = Some(Instant::now() + q.backoff);
            q.backoff = (q.backoff * 2).min(cfg.backoff_max);
            false
        }
    }
}

fn push_self_metrics(shared: &Shared, labels: &BTreeMap<String, String>) {
    let ts_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0);
    let Ok(mut i) = shared.inner.lock() else {
        return;
    };
    let ends = [
        ("vm", i.vm.st.dropped, i.vm.items.len()),
        ("vm_priority", i.vmp.st.dropped, i.vmp.items.len()),
        ("vl", i.vl.st.dropped, i.vl.items.len()),
    ];
    for (endpoint, dropped, queued) in ends {
        let mut l = labels.clone();
        l.insert("endpoint".into(), endpoint.into());
        for (name, v) in [
            ("obs_push_dropped_total", dropped as f64),
            ("obs_push_queue_lines", queued as f64),
        ] {
            i.vmp.push(Sample {
                name: name.into(),
                labels: l.clone(),
                value: v,
                ts_ms,
            });
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Post {
    Ok,
    /// 400, 413 or 422: about this batch, not the store (R11).
    Rejected(u16),
    Failed,
}

async fn post(client: &reqwest::Client, url: reqwest::Url, body: String) -> Post {
    match client
        .post(url)
        .header("content-type", "application/stream+json")
        .body(body)
        .send()
        .await
    {
        Ok(r) => {
            let status = r.status().as_u16();
            let _ = r.bytes().await;
            match status {
                200..=299 => Post::Ok,
                400 | 413 | 422 => Post::Rejected(status),
                _ => Post::Failed,
            }
        }
        Err(_) => Post::Failed,
    }
}
