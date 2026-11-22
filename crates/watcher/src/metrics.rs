//! The watcher's in-process metrics registry and its own minimal
//! VictoriaMetrics import client (C-WATCH part 3). It does not use the
//! workspace's `obs-push`: the watcher depends on no workspace crate.
//!
//! Every series carries `venue="bybit"`, `host` and `watcher`; labels are
//! sorted. An export is one POST to `<vm_url>/api/v1/import` with one JSON
//! line per sample, `{"metric":{"__name__":…,labels},"values":[v],
//! "timestamps":[ms]}`, all at the export time. `watcher_up 1` is in every
//! export. There is no queue: a failed push is dropped (gauges are sent
//! fresh next time, counters are cumulative) and logged once per minute.
//! The push timer belongs to the runtime (U19a7).

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::Duration;

use crate::log::Log;

type Labels = BTreeMap<String, String>;

/// Gauges and counters by name and labels.
pub struct Metrics {
    base: Labels,
    series: Mutex<BTreeMap<(String, Labels), f64>>,
}

fn labels(l: &[(&str, &str)]) -> Labels {
    l.iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

fn quote(s: &str) -> String {
    serde_json::Value::String(s.to_string()).to_string()
}

impl Metrics {
    pub fn new(host: &str, watcher: &str) -> Metrics {
        Metrics {
            base: labels(&[("venue", "bybit"), ("host", host), ("watcher", watcher)]),
            series: Mutex::new(BTreeMap::new()),
        }
    }

    /// Sets a gauge.
    pub fn set(&self, name: &str, l: &[(&str, &str)], v: f64) {
        if let Ok(mut s) = self.series.lock() {
            s.insert((name.to_string(), labels(l)), v);
        }
    }

    /// Adds to a counter (absent = 0).
    pub fn add(&self, name: &str, l: &[(&str, &str)], v: f64) {
        if let Ok(mut s) = self.series.lock() {
            *s.entry((name.to_string(), labels(l))).or_insert(0.0) += v;
        }
    }

    /// Removes a gauge: it is not exported until set again.
    pub fn remove(&self, name: &str, l: &[(&str, &str)]) {
        if let Ok(mut s) = self.series.lock() {
            s.remove(&(name.to_string(), labels(l)));
        }
    }

    pub fn get(&self, name: &str, l: &[(&str, &str)]) -> Option<f64> {
        let s = self.series.lock().ok()?;
        s.get(&(name.to_string(), labels(l))).copied()
    }

    /// The import body at `ts_ms`: `watcher_up` first, then every series
    /// with a finite value, sorted by name and labels; one line each.
    pub fn import_lines(&self, ts_ms: u64) -> String {
        let mut out = String::new();
        self.line(&mut out, "watcher_up", &Labels::new(), 1.0, ts_ms);
        if let Ok(s) = self.series.lock() {
            for ((name, l), v) in s.iter() {
                if name != "watcher_up" && v.is_finite() {
                    self.line(&mut out, name, l, *v, ts_ms);
                }
            }
        }
        out
    }

    fn line(&self, out: &mut String, name: &str, l: &Labels, v: f64, ts_ms: u64) {
        // The fixed labels win over a series label of the same name.
        let mut all = l.clone();
        all.retain(|k, _| k != "__name__");
        all.extend(self.base.clone());
        out.push_str(r#"{"metric":{"__name__":"#);
        out.push_str(&quote(name));
        for (k, val) in &all {
            out.push(',');
            out.push_str(&quote(k));
            out.push(':');
            out.push_str(&quote(val));
        }
        out.push_str(r#"},"values":["#);
        out.push_str(&serde_json::Value::from(v).to_string());
        out.push_str(r#"],"timestamps":["#);
        out.push_str(&ts_ms.to_string());
        out.push_str("]}\n");
    }
}

/// How often a failing push is logged.
pub const PUSH_FAIL_LOG_MS: u64 = 60_000;

/// The VM import client.
pub struct Exporter {
    url: String,
    http: reqwest::Client,
    log: Log,
    last_fail_log_ms: Mutex<Option<u64>>,
}

impl Exporter {
    /// `vm_url` e.g. `http://127.0.0.1:8428` (the config allows loopback
    /// `http` only, U19a4); `timeout_ms` = `request_timeout_ms`.
    pub fn new(vm_url: &str, timeout_ms: u64, log: Log) -> Result<Exporter, String> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_millis(timeout_ms))
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Exporter {
            url: format!("{}/api/v1/import", vm_url.trim_end_matches('/')),
            http,
            log,
            last_fail_log_ms: Mutex::new(None),
        })
    }

    /// One POST of the current values at `now_ms`. A failure is returned
    /// and dropped; the first failure and then at most one a minute are
    /// logged.
    pub async fn push(&self, metrics: &Metrics, now_ms: u64) -> Result<(), String> {
        let body = metrics.import_lines(now_ms);
        let res = match self.http.post(&self.url).body(body).send().await {
            Ok(r) if r.status().is_success() => Ok(()),
            Ok(r) => Err(format!("HTTP {}", r.status().as_u16())),
            Err(e) => Err(e.to_string()),
        };
        if let Err(e) = &res {
            if let Ok(mut last) = self.last_fail_log_ms.lock() {
                let due = last.is_none_or(|t| now_ms.saturating_sub(t) >= PUSH_FAIL_LOG_MS);
                if due {
                    *last = Some(now_ms);
                    (self.log)(&format!(
                        "metrics push failed and was dropped: {e} (logged at most once a minute)"
                    ));
                }
            }
        }
        res
    }
}
