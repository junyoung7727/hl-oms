//! The collector config (spec D2). One TOML file per venue; every check that
//! needs no network runs at load, before any file is opened.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::registry;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Symbols {
    /// Every instrument the venue lists as trading at the start.
    All,
    List(Vec<String>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Write {
    pub flush_ms: u64,
    pub fsync_ms: u64,
    /// Bytes a plan slot may have between receive and write.
    pub queue_bytes: usize,
    /// Bytes all slots together may have (spec R1; judgement, not a model:
    /// half of `MemoryMax=1G`).
    pub total_queue_bytes: usize,
}

impl Default for Write {
    fn default() -> Self {
        Self {
            flush_ms: 1000,
            fsync_ms: 5000,
            queue_bytes: 16 * 1024 * 1024,
            total_queue_bytes: 512 * 1024 * 1024,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Disk {
    pub floor_bytes: u64,
    pub margin_bytes: u64,
    pub check_ms: u64,
}

impl Default for Disk {
    /// Judgement, not a model: 20 GB is about one day of the full plan.
    fn default() -> Self {
        Self {
            floor_bytes: 20_000_000_000,
            margin_bytes: 20_000_000_000,
            check_ms: 10_000,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct DayChange {
    pub start_offset_ms: u64,
    pub stagger_ms: u64,
}

impl Default for DayChange {
    fn default() -> Self {
        Self {
            start_offset_ms: 5000,
            stagger_ms: 2000,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Instruments {
    pub refresh_ms: u64,
}

impl Default for Instruments {
    fn default() -> Self {
        Self {
            refresh_ms: 3_600_000,
        }
    }
}

/// Pushes to VictoriaMetrics and VictoriaLogs (U6 spec D11); absent: none.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Telemetry {
    pub vm_url: Option<String>,
    pub vl_url: Option<String>,
    /// Summary period of the traded topics.
    pub fast_ms: u64,
    /// Summary period of every other topic, and of the lake metrics.
    pub slow_ms: u64,
    /// Lines each push queue holds.
    pub queue_lines: usize,
}

impl Default for Telemetry {
    fn default() -> Self {
        Self {
            vm_url: None,
            vl_url: None,
            fast_ms: 1000,
            slow_ms: 10_000,
            queue_lines: 100_000,
        }
    }
}

/// Direct Telegram alerts (U6 spec D8, R3, R4); absent: none.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Alert {
    /// The token file (`{"token", "chat_id"}`, mode 0600, outside any repository).
    pub telegram: PathBuf,
    /// Where the repeat state survives restarts; default
    /// `$RUNTIME_DIRECTORY/alert-state.json` when that is set.
    pub state_file: Option<PathBuf>,
    /// Tests only: a loopback Bot API, honoured only with `loopback_only`.
    pub api_base: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Extra {
    pub channel: String,
    pub symbols: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarketConfig {
    pub name: String,
    pub symbols: Symbols,
    pub channels: Vec<String>,
    pub extra: Vec<Extra>,
    pub conn_budget: u32,
    pub max_topics_per_conn: Option<usize>,
    pub ws_url: Option<String>,
    pub rest_base: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TradedChannel {
    pub market: String,
    pub symbol: String,
    pub channel: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub venue: String,
    pub collector: String,
    pub lake_root: PathBuf,
    pub host: String,
    /// Tests only: refuse every address that is not loopback.
    pub loopback_only: bool,
    pub write: Write,
    pub disk: Disk,
    pub day_change: DayChange,
    pub instruments: Instruments,
    pub markets: Vec<MarketConfig>,
    pub traded_channels: Vec<TradedChannel>,
    pub telemetry: Option<Telemetry>,
    pub alert: Option<Alert>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum RawSymbols {
    Word(String),
    List(Vec<String>),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawMarket {
    name: String,
    symbols: RawSymbols,
    channels: Vec<String>,
    #[serde(default)]
    extra: Vec<Extra>,
    conn_budget: u32,
    max_topics_per_conn: Option<usize>,
    ws_url: Option<String>,
    rest_base: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Raw {
    venue: String,
    collector: Option<String>,
    lake_root: PathBuf,
    host: Option<String>,
    #[serde(default)]
    loopback_only: bool,
    #[serde(default)]
    write: Write,
    #[serde(default)]
    disk: Disk,
    #[serde(default)]
    day_change: DayChange,
    #[serde(default)]
    instruments: Instruments,
    markets: Vec<RawMarket>,
    #[serde(default)]
    traded_channels: Vec<TradedChannel>,
    telemetry: Option<Telemetry>,
    alert: Option<Alert>,
}

/// `http(s)://` with a host that is a loopback address or `localhost`.
fn is_loopback_url(u: &str) -> bool {
    let Some(rest) = u.strip_prefix("http://").or_else(|| u.strip_prefix("https://")) else {
        return false;
    };
    let host_port = rest.split('/').next().unwrap_or("");
    let host = match host_port.strip_prefix('[') {
        Some(v6) => v6.split(']').next().unwrap_or(""),
        None => host_port.rsplit_once(':').map_or(host_port, |(h, _)| h),
    };
    host == "localhost"
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
}

impl Alert {
    /// Loads the token file with the rules of alert-direct; with
    /// `loopback_only` (tests) the "outside any repository" rule is skipped,
    /// because test directories sit inside a checkout.
    pub fn load_telegram(
        &self,
        loopback_only: bool,
    ) -> Result<alert_direct::TelegramConfig, String> {
        alert_direct::TelegramConfig::load_with(&self.telegram, !loopback_only)
            .map_err(|e| format!("[alert] telegram: {e}"))
    }
}

impl MarketConfig {
    /// Whether this market collects `channel` for `symbol`. With
    /// `symbols = "all"` a main channel counts for every symbol.
    pub fn collects(&self, symbol: &str, channel: &str) -> bool {
        let listed = match &self.symbols {
            Symbols::All => true,
            Symbols::List(s) => s.iter().any(|x| x == symbol),
        };
        (listed && self.channels.iter().any(|c| c == channel))
            || self
                .extra
                .iter()
                .any(|e| e.channel == channel && e.symbols.iter().any(|s| s == symbol))
    }
}

impl Config {
    pub fn load(path: &Path) -> Result<Config, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        Self::from_toml(&text)
    }

    pub fn from_toml(text: &str) -> Result<Config, String> {
        let raw: Raw = toml::from_str(text).map_err(|e| e.to_string())?;
        let venue = registry::venue(&raw.venue)
            .ok_or_else(|| format!("venue {} is not registered", raw.venue))?;
        let collector = raw.collector.unwrap_or_else(|| "md_collector".into());
        if !md_lake::paths::valid_writer(&collector) {
            return Err(format!(
                "collector name {collector} is not ^[a-z0-9][a-z0-9_]*$"
            ));
        }
        if raw.markets.is_empty() {
            return Err("no [[markets]]".into());
        }
        let mut seen = BTreeSet::new();
        let mut budget: u64 = 0;
        let mut markets = Vec::new();
        for m in raw.markets {
            if venue.market_slot(&m.name).is_none() {
                return Err(format!("{} has no market {}", raw.venue, m.name));
            }
            if !seen.insert(m.name.clone()) {
                return Err(format!("market {} is listed twice", m.name));
            }
            budget += u64::from(m.conn_budget);
            let symbols = match m.symbols {
                RawSymbols::Word(w) if w == "all" => Symbols::All,
                RawSymbols::Word(w) => {
                    return Err(format!(
                        "market {}: symbols = \"{w}\": use \"all\" or a list",
                        m.name
                    ));
                }
                RawSymbols::List(l) => Symbols::List(l),
            };
            markets.push(MarketConfig {
                name: m.name,
                symbols,
                channels: m.channels,
                extra: m.extra,
                conn_budget: m.conn_budget,
                max_topics_per_conn: m.max_topics_per_conn,
                ws_url: m.ws_url,
                rest_base: m.rest_base,
            });
        }
        if budget > u64::from(venue.max_conn_budget()) {
            return Err(format!(
                "the markets' conn_budget sum {budget} is over {}'s limit {}",
                raw.venue,
                venue.max_conn_budget()
            ));
        }
        for t in &raw.traded_channels {
            let Some(m) = markets.iter().find(|m| m.name == t.market) else {
                return Err(format!(
                    "traded channel {}/{}/{}: market {} is not collected",
                    t.market, t.symbol, t.channel, t.market
                ));
            };
            if !m.collects(&t.symbol, &t.channel) {
                return Err(format!(
                    "traded channel {}/{}/{} is not collected (symbol {} channel {})",
                    t.market, t.symbol, t.channel, t.symbol, t.channel
                ));
            }
        }
        if let Some(t) = &raw.telemetry {
            if t.fast_ms < 100 || t.slow_ms < t.fast_ms {
                return Err(format!(
                    "[telemetry] needs fast_ms >= 100 and slow_ms >= fast_ms (got {} and {})",
                    t.fast_ms, t.slow_ms
                ));
            }
            for u in [&t.vm_url, &t.vl_url].into_iter().flatten() {
                if raw.loopback_only && !is_loopback_url(u) {
                    return Err(format!("[telemetry] {u} is not loopback (loopback_only)"));
                }
            }
        }
        if let Some(a) = &raw.alert {
            if a.api_base.is_some() && !raw.loopback_only {
                return Err("[alert] api_base is for tests only (needs loopback_only)".into());
            }
            if let Some(u) = &a.api_base {
                if !is_loopback_url(u) {
                    return Err(format!("[alert] api_base {u} is not loopback"));
                }
            }
            // R4: a configured file that cannot be loaded is a config error.
            a.load_telegram(raw.loopback_only)?;
        }
        Ok(Config {
            venue: raw.venue,
            collector,
            lake_root: raw.lake_root,
            host: raw.host.unwrap_or_else(crate::host_name),
            loopback_only: raw.loopback_only,
            write: raw.write,
            disk: raw.disk,
            day_change: raw.day_change,
            instruments: raw.instruments,
            markets,
            traded_channels: raw.traded_channels,
            telemetry: raw.telemetry,
            alert: raw.alert,
        })
    }
}
