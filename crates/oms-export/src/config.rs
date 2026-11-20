//! The configuration file (TOML).

use serde::Deserialize;
use std::path::PathBuf;

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// The OMS data directory.
    pub data_dir: PathBuf,
    /// The lake root.
    pub lake_root: PathBuf,
    /// The OMS `/api/status` URL; loopback only (the API has no
    /// authentication).
    pub status_url: String,
    /// The venue and market of every row (one OMS trades on one venue).
    pub venue: String,
    pub market: String,
    #[serde(default = "default_timeout_ms")]
    pub status_timeout_ms: u64,
    #[serde(default = "default_batch")]
    pub batch: usize,
}

fn default_timeout_ms() -> u64 {
    2_000
}
fn default_batch() -> usize {
    1000
}

impl Config {
    pub fn parse(text: &str) -> Result<Config, String> {
        let c: Config = toml::from_str(text).map_err(|e| e.to_string())?;
        let url = reqwest::Url::parse(&c.status_url).map_err(|e| format!("status_url: {e}"))?;
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
        if c.venue.is_empty() || c.market.is_empty() {
            return Err("venue and market must not be empty".into());
        }
        if c.batch == 0 {
            return Err("batch must be positive".into());
        }
        Ok(c)
    }
}
