//! The projector's configuration file (TOML). Unknown keys are refused.
//!
//! ```toml
//! data_dir = "/var/lib/hl-oms/data"         # the OMS data directory
//! status_url = "http://127.0.0.1:8080/api/status"
//! venue = "bybit"                          # label of every series and line
//! cursor_file = "/var/lib/hl-oms/projector.cursor"
//! # host = "tokyo-1"                       # default: /etc/hostname
//!
//! [telemetry]                              # absent: nothing is pushed
//! vm_url = "http://127.0.0.1:8428"
//! vl_url = "http://127.0.0.1:9428"
//! ```

use serde::Deserialize;
use std::path::PathBuf;

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub data_dir: PathBuf,
    pub status_url: String,
    pub venue: String,
    pub cursor_file: PathBuf,
    #[serde(default)]
    pub host: Option<String>,
    #[serde(default)]
    pub telemetry: Option<Telemetry>,
    /// Period of one step (status query, tail read, metrics).
    #[serde(default = "default_poll_ms")]
    pub poll_ms: u64,
    /// Time without a status answer after which the OMS counts as dead,
    /// when no process holds the journal lock either.
    #[serde(default = "default_dead_after_ms")]
    pub dead_after_ms: u64,
    #[serde(default = "default_disk_every_ms")]
    pub disk_every_ms: u64,
    /// Records read in one step.
    #[serde(default = "default_max_records")]
    pub max_records: usize,
    /// Log lines pushed but not yet taken by the store: no more are pushed
    /// above it (one record alone may pass it), and the journal is not read
    /// further while this many lines wait.
    #[serde(default = "default_max_pending_lines")]
    pub max_pending_lines: usize,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Telemetry {
    pub vm_url: Option<String>,
    pub vl_url: Option<String>,
    #[serde(default = "default_queue_lines")]
    pub queue_lines: usize,
}

fn default_poll_ms() -> u64 {
    1000
}
fn default_dead_after_ms() -> u64 {
    10_000
}
fn default_disk_every_ms() -> u64 {
    10_000
}
fn default_max_records() -> usize {
    1000
}
fn default_max_pending_lines() -> usize {
    20_000
}
fn default_queue_lines() -> usize {
    100_000
}

impl Config {
    pub fn parse(text: &str) -> Result<Config, String> {
        let c: Config = toml::from_str(text).map_err(|e| e.to_string())?;
        c.check()?;
        Ok(c)
    }

    fn check(&self) -> Result<(), String> {
        let url = reqwest::Url::parse(&self.status_url).map_err(|e| format!("status_url: {e}"))?;
        let host = url
            .host_str()
            .unwrap_or("")
            .trim_start_matches('[')
            .trim_end_matches(']');
        let loopback = host == "localhost"
            || host
                .parse::<std::net::IpAddr>()
                .is_ok_and(|a| a.is_loopback());
        if !loopback {
            return Err(
                "status_url must be a loopback address: the OMS status API has no authentication"
                    .into(),
            );
        }
        if self.venue.is_empty() {
            return Err("venue must not be empty".into());
        }
        if self.poll_ms < 100 {
            return Err("poll_ms must be at least 100".into());
        }
        if self.dead_after_ms < self.poll_ms {
            return Err("dead_after_ms must be at least poll_ms".into());
        }
        if self.max_records == 0 || self.max_pending_lines == 0 {
            return Err("max_records and max_pending_lines must be positive".into());
        }
        if let Some(t) = &self.telemetry {
            // At most max_pending_lines are pushed and not taken (one record
            // alone may pass it); the rest of the queue is room for
            // unconfirmed lines and the projector's own lines.
            if t.queue_lines < 2 * self.max_pending_lines {
                return Err("telemetry.queue_lines must be at least twice max_pending_lines, or the queue can drop lines the cursor waits for".into());
            }
        }
        Ok(())
    }
}
