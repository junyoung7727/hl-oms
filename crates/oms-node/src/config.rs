//! Node configuration file (TOML).
//!
//! The `[core]` table is the domain configuration. It reaches the core as an
//! input and is journaled; the rest configures the execution layer of this
//! process and is not part of the domain state.

use crate::venue::hyperliquid::wire::{WEIGHT_MARGIN_QUERY, WEIGHT_OTHER_INFO_QUERY};
use oms_core::CoreConfig;
use serde::Deserialize;
use std::path::{Path, PathBuf};

/// Most weight per minute the steady-state queries may use against
/// Hyperliquid: half of the documented 1200 per minute and IP.
pub const QUERY_WEIGHT_BUDGET: u64 = 600;
/// One reference-price query (`allMids`, documented weight 2).
const WEIGHT_REF_PRICE: u64 = 2;
/// One sweep: fills (20), open orders (20) and positions (2), 42 in all.
const WEIGHT_SWEEP: u64 = 2 * WEIGHT_OTHER_INFO_QUERY as u64 + WEIGHT_MARGIN_QUERY as u64;

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeConfig {
    pub data_dir: PathBuf,
    #[serde(default)]
    pub api: ApiConfig,
    #[serde(default)]
    pub http: HttpConfig,
    #[serde(default)]
    pub runtime: RuntimeConfig,
    #[serde(default)]
    pub limits: IngressLimits,
    pub venue: VenueConfig,
    pub core: CoreConfig,
    /// Direct Telegram alerts (C-ALERT). Absent: none.
    pub alert: Option<AlertSection>,
    /// The kill marker (C-HOLD). Absent: no marker is looked for.
    pub kill: Option<KillSection>,
}

/// The `[kill]` table (C-HOLD).
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KillSection {
    /// The file the watcher writes when it kills (U19b). Read once, before
    /// the boot record; anything there latches the kill switch. Absolute,
    /// outside the data directory, in a directory that exists.
    pub marker: PathBuf,
}

impl KillSection {
    fn validate(&self, data_dir: &Path) -> Result<(), String> {
        if !self.marker.is_absolute() {
            return Err("[kill] marker must be an absolute path".into());
        }
        if self.marker.components().any(|c| c == std::path::Component::ParentDir) {
            return Err("[kill] marker must not hold `..`".into());
        }
        if resolved(&self.marker).starts_with(resolved(data_dir)) {
            return Err("[kill] marker must be outside the data directory".into());
        }
        Ok(())
    }

    /// The marker's directory must exist when the configuration is loaded:
    /// a marker that cannot be written there would never latch.
    fn check_directory(&self) -> Result<(), String> {
        match self.marker.parent() {
            Some(dir) if dir.is_dir() => Ok(()),
            _ => Err(format!("[kill] marker: the directory of {} does not exist", self.marker.display())),
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ApiConfig {
    /// Trading socket. A relative path is resolved against `data_dir`.
    pub socket: PathBuf,
    /// Admin socket.
    pub admin_socket: PathBuf,
    pub max_connections: usize,
    /// Connections on the admin socket, counted separately so that trading
    /// clients can never keep the operator out.
    pub admin_max_connections: usize,
    /// A connection that has not sent a valid `hello` within this time is
    /// closed. Zero disables the check.
    pub hello_timeout_ms: u64,
    /// Requests one connection may have outstanding at once.
    pub window: usize,
    /// User ids, besides this process's own, allowed on the trading socket.
    pub allowed_uids: Vec<u32>,
    /// Events buffered per subscriber before it is disconnected.
    pub subscriber_buffer: usize,
}

impl Default for ApiConfig {
    fn default() -> Self {
        ApiConfig {
            socket: "oms.sock".into(),
            admin_socket: "admin.sock".into(),
            max_connections: 64,
            admin_max_connections: 8,
            hello_timeout_ms: 10_000,
            window: 32,
            allowed_uids: Vec::new(),
            subscriber_buffer: 4096,
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct HttpConfig {
    /// `ip:port` of the read-only dashboard, loopback only. Empty disables it.
    pub listen: String,
    /// Directory with the dashboard's static files and generated registry.
    pub static_dir: PathBuf,
}

/// The `[alert]` table (C-ALERT, ADR-029).
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AlertSection {
    /// The bot token file (alert-direct format, mode 0600, outside any
    /// repository). Absolute path.
    pub telegram: PathBuf,
    /// How long a passing state (link down, resync, in doubt, ...) must
    /// hold before its start is sent.
    #[serde(default = "default_grace_ms")]
    pub grace_ms: u64,
    /// The conditions are evaluated at least this often.
    #[serde(default = "default_eval_ms")]
    pub eval_ms: u64,
    /// A lasting condition is sent again this often (design 7.7).
    #[serde(default = "default_repeat_ms")]
    pub repeat_ms: u64,
    /// How long the exit waits for queued alerts.
    #[serde(default = "default_exit_flush_ms")]
    pub exit_flush_ms: u64,
    /// Only "none" until a second channel is chosen (owner decision D3).
    #[serde(default = "default_second_channel")]
    pub second_channel: String,
    /// Telegram failing for longer than this is told on the second channel.
    #[serde(default = "default_second_after_ms")]
    pub second_after_ms: u64,
    /// The token file, loaded by `NodeConfig::load`.
    #[serde(skip)]
    pub loaded: Option<alert_direct::TelegramConfig>,
    /// The loopback Bot API base of `OMS_TEST_TELEGRAM_API` (failpoints
    /// builds only).
    #[serde(skip)]
    pub test_api: Option<String>,
}

fn default_grace_ms() -> u64 {
    30_000
}

fn default_eval_ms() -> u64 {
    1_000
}

fn default_repeat_ms() -> u64 {
    alert_direct::REPEAT_MS as u64
}

fn default_exit_flush_ms() -> u64 {
    15_000
}

fn default_second_channel() -> String {
    "none".into()
}

fn default_second_after_ms() -> u64 {
    alert_direct::SECOND_AFTER_MS as u64
}

/// The environment variable that replaces the Telegram routes with one
/// loopback URL, read only by a build with the `failpoints` feature.
pub const TEST_TELEGRAM_API: &str = "OMS_TEST_TELEGRAM_API";

/// The value of `OMS_TEST_TELEGRAM_API` if it is an `http://` URL on a
/// loopback address; any other host is refused.
pub fn test_api_base(value: &str) -> Result<String, String> {
    let url = reqwest::Url::parse(value).map_err(|e| format!("{TEST_TELEGRAM_API}: {e}"))?;
    let host = url.host_str().unwrap_or_default().trim_start_matches('[').trim_end_matches(']');
    let loopback = host.parse::<std::net::IpAddr>().is_ok_and(|ip| ip.is_loopback());
    if url.scheme() != "http" || !loopback {
        return Err(format!("{TEST_TELEGRAM_API} must be http://<loopback address>:<port>"));
    }
    Ok(value.to_string())
}

impl AlertSection {
    fn validate(&self) -> Result<(), String> {
        // A relative path would resolve against the configuration's
        // directory, often a checkout.
        if !self.telegram.is_absolute() {
            return Err("[alert] telegram must be an absolute path".into());
        }
        let bounds: [(&str, u64, u64, u64); 5] = [
            ("grace_ms", self.grace_ms, 0, 600_000),
            ("eval_ms", self.eval_ms, 100, 10_000),
            ("repeat_ms", self.repeat_ms, 60_000, 3_600_000),
            ("exit_flush_ms", self.exit_flush_ms, 1_000, 30_000),
            ("second_after_ms", self.second_after_ms, 10_000, 600_000),
        ];
        for (name, v, lo, hi) in bounds {
            if !(lo..=hi).contains(&v) {
                return Err(format!("[alert] {name} must be between {lo} and {hi}"));
            }
        }
        if self.second_channel != "none" {
            return Err(format!(
                "[alert] second_channel {:?}: only \"none\" until a second channel is chosen (D3)",
                self.second_channel
            ));
        }
        Ok(())
    }

    /// Loads the token file with every rule of alert-direct; with a test
    /// API the repository rule is skipped (the test directory may be inside
    /// a checkout). Errors never hold the token.
    fn load(&mut self, test_api: Option<String>) -> Result<(), String> {
        let tg = alert_direct::TelegramConfig::load_with(&self.telegram, test_api.is_none())
            .map_err(|e| format!("[alert] telegram: {e}"))?;
        self.loaded = Some(tg);
        self.test_api = test_api;
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RuntimeConfig {
    pub orders_queue: usize,
    pub control_queue: usize,
    pub venue_queue: usize,
    /// Most inputs the core takes in one batch.
    pub max_batch: usize,
    pub tick_ms: u64,
    /// A snapshot is written after this many records since the last one.
    pub snapshot_every: u64,
    pub segment_max_bytes: u64,
    /// New orders are refused below this much free space on the journal's
    /// file system.
    pub storage_low_bytes: u64,
    pub reconcile_ms: u64,
    pub sweep_ms: u64,
    /// Time between margin queries while the link is up. Used only with a
    /// margin policy (`[core.policy.margin]`).
    pub margin_poll_ms: u64,
    /// Time between venue-setting queries for an instrument without a
    /// position, while the link is up. Used only with a margin policy.
    pub venue_setting_poll_ms: u64,
    pub worker_threads: usize,
    /// Run journal writes on the core thread instead of a separate commit
    /// thread. For the layout comparison in ADR-012 only.
    pub inline_commit: bool,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        RuntimeConfig {
            orders_queue: 1024,
            control_queue: 256,
            venue_queue: 4096,
            max_batch: 256,
            tick_ms: 250,
            snapshot_every: 50_000,
            segment_max_bytes: 64 * 1024 * 1024,
            storage_low_bytes: 1 << 30,
            reconcile_ms: 500,
            sweep_ms: 15_000,
            margin_poll_ms: 5_000,
            venue_setting_poll_ms: 60_000,
            worker_threads: 2,
            inline_commit: false,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct IngressLimits {
    /// Sustained new-order requests per second per strategy.
    pub new_orders_per_sec: u32,
    pub new_orders_burst: u32,
    /// Cancels have their own, larger budget so that a flood of new orders
    /// cannot crowd them out.
    pub cancels_per_sec: u32,
    pub cancels_burst: u32,
}

impl Default for IngressLimits {
    fn default() -> Self {
        IngressLimits {
            new_orders_per_sec: 50,
            new_orders_burst: 100,
            cancels_per_sec: 200,
            cancels_burst: 400,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum VenueConfig {
    /// The simulated venue process (`simvenue`), reached over its socket.
    Sim {
        socket: PathBuf,
        #[serde(default = "default_timeout_ms")]
        request_timeout_ms: u64,
    },
    Hyperliquid(HyperliquidConfig),
    Bybit(BybitConfig),
}

fn default_timeout_ms() -> u64 {
    3_000
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HyperliquidConfig {
    /// "testnet" or "mainnet".
    pub network: String,
    /// Address of the account that holds the funds.
    pub account_address: String,
    /// Name of the environment variable that holds the agent wallet's private
    /// key. The key itself never appears in a file this program reads.
    #[serde(default = "default_key_env")]
    pub agent_key_env: String,
    #[serde(default = "default_timeout_ms")]
    pub request_timeout_ms: u64,
    /// Must be true to place orders on mainnet.
    #[serde(default)]
    pub mainnet_confirmed: bool,
    /// If positive, the gateway keeps the venue's scheduled cancel this many
    /// seconds ahead. Should this process stop, the venue cancels every open
    /// order at that time. The venue allows ten such triggers per day.
    #[serde(default)]
    pub schedule_cancel_secs: u64,
    /// How far in the past a fill query ends. Fills are declared complete
    /// only up to that time, so it must exceed the venue's query lag plus the
    /// clock difference.
    #[serde(default = "default_fills_lag_ms")]
    pub fills_lag_ms: u64,
    /// Minimum time between reference price updates per instrument.
    #[serde(default = "default_ref_price_ms")]
    pub ref_price_interval_ms: u64,
    /// Whether an answer `{"status":"err", …}` to an order proves that the
    /// order was not placed. The documentation shows no such answer; the
    /// SDKs treat it as a refusal of the whole action. Off: the outcome is
    /// unknown and is resolved by query after the send window. Turn it on
    /// only after observing the venue's behaviour.
    #[serde(default)]
    pub trust_top_level_error: bool,
}

/// The Bybit gateway (ADR-028): a unified trading account's sub-account,
/// category linear, USDT settled, one-way mode, cross margin.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BybitConfig {
    /// "testnet" or "mainnet".
    pub network: String,
    /// TOML file with `api_key` and `api_secret`, owner-only (mode 0600),
    /// outside the data directory. Absolute path.
    pub credentials_file: PathBuf,
    /// The sub-account (UID) the key must belong to.
    pub sub_account_uid: u64,
    /// An HTTPS service whose answer body is the caller's public IP. The
    /// key must be bound to that address.
    pub ip_echo_url: String,
    #[serde(default = "default_ip_check_secs")]
    pub ip_check_secs: u64,
    #[serde(default = "default_recv_window_ms")]
    pub recv_window_ms: u64,
    #[serde(default = "default_timeout_ms")]
    pub request_timeout_ms: u64,
    /// Must be true to place orders on mainnet.
    #[serde(default)]
    pub mainnet_confirmed: bool,
    /// How far in the past a fill query ends (as for Hyperliquid).
    #[serde(default = "default_fills_lag_ms")]
    pub fills_lag_ms: u64,
    /// The private stream sends a ping this often; no pong within the same
    /// time means the stream is down.
    #[serde(default = "default_ping_interval_ms")]
    pub ping_interval_ms: u64,
    /// Minimum time between reference price updates per instrument, read
    /// from the public ticker stream (U17c).
    #[serde(default = "default_ref_price_ms")]
    pub ref_price_interval_ms: u64,
}

fn default_ping_interval_ms() -> u64 {
    5_000
}

fn default_ip_check_secs() -> u64 {
    60
}

fn default_recv_window_ms() -> u64 {
    5_000
}

fn default_fills_lag_ms() -> u64 {
    2_000
}

fn default_ref_price_ms() -> u64 {
    5_000
}

fn default_key_env() -> String {
    "HL_AGENT_PRIVATE_KEY".into()
}

/// `p` as an absolute path with `.` and `..` folded and, where the path
/// exists, symlinks followed. Compares paths by where they point.
fn resolved(p: &Path) -> PathBuf {
    if let Ok(c) = p.canonicalize() {
        return c;
    }
    let abs = std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf());
    let mut out = PathBuf::new();
    for c in abs.components() {
        match c {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            c => out.push(c),
        }
    }
    // The longest existing prefix may itself be a symlink.
    let mut head = out.clone();
    let mut tail = Vec::new();
    while !head.exists() {
        match head.file_name() {
            Some(n) => tail.push(n.to_os_string()),
            None => return out,
        }
        if !head.pop() {
            return out;
        }
    }
    let mut r = head.canonicalize().unwrap_or(head);
    for n in tail.into_iter().rev() {
        r.push(n);
    }
    r
}

impl NodeConfig {
    pub fn load(path: &Path) -> Result<NodeConfig, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
        let mut cfg: NodeConfig = toml::from_str(&text).map_err(|e| format!("parse {}: {e}", path.display()))?;
        let base = path.parent().unwrap_or(Path::new("."));
        if cfg.data_dir.is_relative() {
            cfg.data_dir = base.join(&cfg.data_dir);
        }
        if cfg.http.static_dir.as_os_str().is_empty() {
            cfg.http.static_dir = "dashboard".into();
        }
        if cfg.http.static_dir.is_relative() {
            cfg.http.static_dir = base.join(&cfg.http.static_dir);
        }
        cfg.validate()?;
        if let Some(k) = &cfg.kill {
            k.check_directory()?;
        }
        // Only a build with the failpoints feature (the node tests) reads
        // it; the shipped binary has no such code.
        #[cfg(feature = "failpoints")]
        let test_api = std::env::var(TEST_TELEGRAM_API).ok().map(|v| test_api_base(&v)).transpose()?;
        #[cfg(not(feature = "failpoints"))]
        let test_api: Option<String> = None;
        if let Some(a) = &mut cfg.alert {
            a.load(test_api)?;
        }
        Ok(cfg)
    }

    /// Whether this configuration trades real money: a mainnet venue with
    /// `mainnet_confirmed`.
    pub fn is_mainnet(&self) -> bool {
        match &self.venue {
            VenueConfig::Sim { .. } => false,
            VenueConfig::Hyperliquid(h) => h.network == "mainnet" && h.mainnet_confirmed,
            VenueConfig::Bybit(b) => b.network == "mainnet" && b.mainnet_confirmed,
        }
    }

    /// The venue's name as `status.node.venue` shows it, worked out from
    /// the configuration so that the alerts start before the gateway.
    pub fn venue_label(&self) -> String {
        match &self.venue {
            VenueConfig::Sim { .. } => "sim".into(),
            VenueConfig::Hyperliquid(h) => format!("hyperliquid:{}", h.network),
            VenueConfig::Bybit(b) => format!("bybit:{}", b.network),
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        self.core.validate().map_err(|e| format!("[core] {}", e.join("; ")))?;
        let r = &self.runtime;
        if r.orders_queue == 0 || r.control_queue == 0 || r.venue_queue == 0 || r.max_batch == 0 {
            return Err("[runtime] queue sizes and max_batch must be positive".into());
        }
        if r.margin_poll_ms == 0 {
            return Err("[runtime] margin_poll_ms must be greater than 0".into());
        }
        if r.venue_setting_poll_ms == 0 {
            return Err("[runtime] venue_setting_poll_ms must be greater than 0".into());
        }
        if self.api.window == 0 || self.api.max_connections == 0 || self.api.admin_max_connections == 0 {
            return Err("[api] window, max_connections and admin_max_connections must be positive".into());
        }
        if !self.http.listen.is_empty() {
            let addr: std::net::SocketAddr = self
                .http
                .listen
                .parse()
                .map_err(|e| format!("[http] listen: {e}"))?;
            if !addr.ip().is_loopback() {
                return Err("[http] listen must be a loopback address; the dashboard has no authentication".into());
            }
        }
        if let VenueConfig::Hyperliquid(h) = &self.venue {
            match h.network.as_str() {
                "testnet" => {}
                // Real money is not traded without the margin rules.
                "mainnet" if h.mainnet_confirmed && self.core.policy.margin.is_none() => {
                    return Err("[venue] mainnet requires [core.policy.margin] (the margin rules, C-MARGIN)".into());
                }
                "mainnet" if h.mainnet_confirmed => {}
                "mainnet" => return Err("[venue] mainnet requires mainnet_confirmed = true".into()),
                other => return Err(format!("[venue] unknown network {other}")),
            }
            if h.schedule_cancel_secs != 0 && h.schedule_cancel_secs < 15 {
                return Err("[venue] schedule_cancel_secs must be 0 or at least 15".into());
            }
            // With the margin rules the queries must leave half of the
            // venue's limit to orders, cancels and everything that is not
            // steady. Without them nothing is refused that loaded before.
            if let (Some(weight), true) = (self.steady_state_query_weight(), self.core.policy.margin.is_some()) {
                if weight > QUERY_WEIGHT_BUDGET {
                    return Err(format!(
                        "[runtime] steady-state query weight {weight} per minute is above {QUERY_WEIGHT_BUDGET}, half of the venue's limit of 1200: use fewer instruments or longer intervals (margin_poll_ms, venue_setting_poll_ms, sweep_ms, ref_price_interval_ms)"
                    ));
                }
            }
        }
        if let VenueConfig::Bybit(b) = &self.venue {
            self.validate_bybit(b)?;
        } else if self.core.policy.margin.as_ref().is_some_and(|m| m.account_mm_rate_max_bps.is_some()) {
            // The other venues report isolated positions and no account
            // numbers: the rule would refuse every report (U17d).
            return Err("[core.policy.margin] account_mm_rate_max_bps is for a cross-margin account (Bybit) only".into());
        }
        if let Some(a) = &self.alert {
            a.validate()?;
        }
        // Real money is not traded without the trading-safety alerts
        // (C-ALERT; owner question 1 of U18a, default: refuse).
        if self.is_mainnet() && self.alert.is_none() {
            return Err("[venue] mainnet requires [alert] (direct alerts, C-ALERT)".into());
        }
        if let Some(k) = &self.kill {
            k.validate(&self.data_dir)?;
        }
        // Nor without the kill marker (C-HOLD; owner question 1 of U18b,
        // default: refuse).
        if self.is_mainnet() && self.kill.is_none() {
            return Err("[venue] mainnet requires [kill] marker (the kill marker, C-HOLD)".into());
        }
        Ok(())
    }

    fn validate_bybit(&self, b: &BybitConfig) -> Result<(), String> {
        match b.network.as_str() {
            "testnet" => {}
            "mainnet" if b.mainnet_confirmed && self.core.policy.margin.is_none() => {
                return Err("[venue] mainnet requires [core.policy.margin] (the margin rules, C-MARGIN)".into());
            }
            "mainnet" if b.mainnet_confirmed => {}
            "mainnet" => return Err("[venue] mainnet requires mainnet_confirmed = true".into()),
            other => return Err(format!("[venue] unknown network {other}")),
        }
        if !b.credentials_file.is_absolute() {
            return Err("[venue] credentials_file must be an absolute path".into());
        }
        if b
            .credentials_file
            .components()
            .any(|c| c == std::path::Component::ParentDir)
        {
            return Err("[venue] credentials_file must not hold `..`".into());
        }
        if resolved(&b.credentials_file).starts_with(resolved(&self.data_dir)) {
            return Err("[venue] credentials_file must be outside the data directory".into());
        }
        if b.sub_account_uid == 0 {
            return Err("[venue] sub_account_uid must be the sub-account's UID".into());
        }
        let echo = reqwest::Url::parse(&b.ip_echo_url).map_err(|e| format!("[venue] ip_echo_url: {e}"))?;
        if echo.scheme() != "https" {
            return Err("[venue] ip_echo_url must be https: a forged answer would pass the IP check".into());
        }
        if b.ip_check_secs < 10 {
            return Err("[venue] ip_check_secs must be at least 10".into());
        }
        if !(1_000..=20_000).contains(&b.recv_window_ms) {
            return Err("[venue] recv_window_ms must be between 1000 and 20000".into());
        }
        // The venue recommends a ping every 20 s (F20 Q2.11).
        if !(1_000..=20_000).contains(&b.ping_interval_ms) {
            return Err("[venue] ping_interval_ms must be between 1000 and 20000".into());
        }
        // Judgment (U17c): one lost update must not make the price stale.
        let p = &self.core.policy;
        if b.ref_price_interval_ms < 1_000 || b.ref_price_interval_ms.saturating_mul(2_000) > p.ref_price_max_age_us {
            return Err(
                "[venue] ref_price_interval_ms must be at least 1000 and at most half of [core.policy] ref_price_max_age_us".into(),
            );
        }
        // ADR-024: Bybit enforces expiry through the receive window, and
        // its realtime query can miss an order for a moment.
        let p = &self.core.policy;
        if !p.venue_enforces_expiry || p.absence_not_found_needed != 2 {
            return Err(
                "[core.policy] Bybit needs venue_enforces_expiry = true and absence_not_found_needed = 2 (ADR-024)".into(),
            );
        }
        // The account is cross margin (ADR-028); the per-instrument rule
        // would refuse every order.
        if let Some(m) = &p.margin {
            if m.isolated {
                return Err("[core.policy.margin] Bybit trades cross margin: set isolated = false".into());
            }
            // The gateway passes `marginMode` on as the account mode
            // (ADR-028 U16c); only cross margin keeps the rules meaningful.
            if m.account_modes_allowed.iter().any(|a| a != "REGULAR_MARGIN") {
                return Err("[core.policy.margin] Bybit allows only the account mode REGULAR_MARGIN".into());
            }
            // The MM-buffer rule judges isolated positions only; the cross
            // account is judged as a whole (C-MARGIN, U17d).
            if m.account_mm_rate_max_bps.is_none() {
                return Err("[core.policy.margin] Bybit needs account_mm_rate_max_bps (the account maintenance-margin limit; the decided value is 5000)".into());
            }
        }
        Ok(())
    }

    /// Weight per minute of the queries this configuration sends to the
    /// venue in a steady state, against the venue's request limit. `None`
    /// for a venue without such a limit.
    ///
    /// The terms (C-MARGIN, 조회 weight 예산): the margin report and, per
    /// instrument, the venue setting, both only with a margin policy; the
    /// reference prices; the sweep. The venue setting is counted for every
    /// instrument, the worst case of no position anywhere. The reference
    /// prices are counted as one weight-2 query per interval although this
    /// gateway reads them from the stream, which leaves that much room.
    /// Not counted, because they are not steady: the account-mode query
    /// (20 once per link session), order-status queries, the queries
    /// repeated each second while an answer is missing, orders and cancels.
    pub fn steady_state_query_weight(&self) -> Option<u64> {
        let VenueConfig::Hyperliquid(h) = &self.venue else {
            return None;
        };
        // A part of a unit counts as a whole one.
        let per_minute = |weight: u64, every_ms: u64| (weight * 60_000).div_ceil(every_ms.max(1));
        let r = &self.runtime;
        let mut weight = per_minute(WEIGHT_REF_PRICE, h.ref_price_interval_ms) + per_minute(WEIGHT_SWEEP, r.sweep_ms);
        if self.core.policy.margin.is_some() {
            let instruments = self.core.instruments.len() as u64;
            weight += per_minute(u64::from(WEIGHT_MARGIN_QUERY), r.margin_poll_ms);
            weight += per_minute(u64::from(WEIGHT_OTHER_INFO_QUERY) * instruments, r.venue_setting_poll_ms);
        }
        Some(weight)
    }

    pub fn socket_path(&self) -> PathBuf {
        self.data_dir.join(&self.api.socket)
    }

    pub fn admin_socket_path(&self) -> PathBuf {
        self.data_dir.join(&self.api.admin_socket)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIM: &str = "kind = \"sim\"\nsocket = \"venue.sock\"";

    fn hyperliquid(network: &str) -> String {
        format!("kind = \"hyperliquid\"\nnetwork = \"{network}\"\naccount_address = \"0xTEST\"\nmainnet_confirmed = true")
    }

    /// Parses and validates a configuration with the given venue table,
    /// `[runtime]` lines and, if `margin` is given, a `[core.policy.margin]`
    /// table with those lines.
    fn check(venue: &str, runtime: &str, margin: Option<&str>) -> Result<(), String> {
        parse(venue, runtime, margin, 1)?.validate()
    }

    /// The same configuration, not yet validated, with `instruments`
    /// instruments (X, X2, X3, ...).
    fn parse(venue: &str, runtime: &str, margin: Option<&str>, instruments: usize) -> Result<NodeConfig, String> {
        let margin = margin.map(|m| format!("[core.policy.margin]\n{m}")).unwrap_or_default();
        let more: String = (2..=instruments)
            .map(|i| {
                format!(
                    "[core.instruments.X{i}]\nqty_step = \"0.001\"\npx_tick = \"0.1\"\nmin_notional = \"10\"\nmax_order_qty = \"100\"\nmax_order_notional = \"1000000\"\naccount_pos_min = \"-10\"\naccount_pos_max = \"10\"\n\n"
                )
            })
            .collect();
        let text = format!(
            r#"
data_dir = "d"

[runtime]
{runtime}

[venue]
{venue}

[core]
version = 1

[core.instruments.X]
qty_step = "0.001"
px_tick = "0.1"
min_notional = "10"
max_order_qty = "100"
max_order_notional = "1000000"
account_pos_min = "-10"
account_pos_max = "10"

{more}
[core.strategies.a]
enabled = true
max_open_orders = 50
[core.strategies.a.limits.X]
min = "-10"
max = "10"

[core.policy]

{margin}

[alert]
telegram = "/etc/hl-oms/telegram.json"

[kill]
marker = "/var/lib/hl-oms/kill.marker"
"#
        );
        toml::from_str(&text).map_err(|e| format!("parse: {e}"))
    }

    fn refused(venue: &str, runtime: &str, margin: Option<&str>, because: &str) {
        match check(venue, runtime, margin) {
            Ok(()) => panic!("loaded, but must be refused ({because}): margin {margin:?}, runtime {runtime:?}"),
            Err(e) => assert!(e.contains(because), "refused for another reason than {because:?}: {e}"),
        }
    }

    fn shipped(name: &str) -> Result<NodeConfig, String> {
        NodeConfig::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../config").join(name))
    }

    /// S21. The shipped Hyperliquid example passes every check of
    /// `validate` and carries a margin policy with the owner's values
    /// written out: limit 2.0, daily loss 5 %, MM buffer 0.25 %p, 30 s and
    /// 60 s, venue leverage 2 isolated. Its allow-list is empty on purpose
    /// (T10). The simulated-venue example keeps loading without a policy.
    #[test]
    fn example_config_with_a_margin_policy_loads() {
        let cfg = shipped("hyperliquid-testnet.example.toml").expect("the shipped example must load");
        assert_eq!(
            cfg.core.policy.margin,
            Some(oms_core::MarginPolicy {
                max_account_leverage: oms_core::Ratio(200_000_000),
                daily_loss_bps: 500,
                mm_buffer_bps: 25,
                margin_max_age_us: 30_000_000,
                stale_cancel_us: 60_000_000,
                venue_leverage: 2,
                isolated: true,
                account_modes_allowed: vec![],
                account_mm_rate_max_bps: None,
            })
        );
        assert_eq!((cfg.runtime.margin_poll_ms, cfg.runtime.venue_setting_poll_ms), (5_000, 60_000));
        assert!(matches!(&cfg.venue, VenueConfig::Hyperliquid(h) if h.network == "testnet"));

        let sim = shipped("sim.toml").expect("the shipped simulated-venue example must load");
        assert_eq!(sim.core.policy.margin, None);
    }

    /// G7. The query weight per minute is worked out from the
    /// configuration, and a configuration with a margin policy whose weight
    /// is above 600 (half of the venue's 1200 per minute and IP) does not
    /// load. Every expected number is worked by hand:
    ///
    /// - margin report: weight 2 every 5 s = 2 x 12 = 24
    /// - reference prices: weight 2 every 5 s = 24
    /// - sweep: fills 20 + open orders 20 + positions 2 = 42 every 15 s = 42 x 4 = 168
    /// - venue setting: weight 20 per instrument every 60 s = 20 per instrument
    ///
    /// 24 + 24 + 168 = 216, plus 20 per instrument.
    #[test]
    fn steady_state_query_weight_is_computed_from_config_and_a_budget_above_600_is_refused() {
        let testnet = hyperliquid("testnet");
        let weight = |runtime: &str, margin: Option<&str>, instruments: usize| {
            parse(&testnet, runtime, margin, instruments).unwrap().steady_state_query_weight()
        };
        // The shipped example: one instrument.
        assert_eq!(shipped("hyperliquid-testnet.example.toml").unwrap().steady_state_query_weight(), Some(236));
        assert_eq!(weight("", Some(""), 1), Some(236));
        assert_eq!(weight("", Some(""), 2), Some(256));
        // 216 + 19 x 20 = 596 loads; 216 + 20 x 20 = 616 does not.
        assert_eq!(weight("", Some(""), 19), Some(596));
        assert_eq!(parse(&testnet, "", Some(""), 19).unwrap().validate(), Ok(()));
        assert_eq!(weight("", Some(""), 20), Some(616));
        let refused = parse(&testnet, "", Some(""), 20).unwrap().validate().unwrap_err();
        assert!(refused.contains("query weight 616 per minute is above 600"), "{refused}");

        // Each interval moves its own term.
        // Margin every 1 s: 2 x 60 = 120; 120 + 24 + 168 + 20 = 332.
        assert_eq!(weight("margin_poll_ms = 1000", Some(""), 1), Some(332));
        // Sweep every 5 s: 42 x 12 = 504; 24 + 24 + 504 + 20 = 572.
        assert_eq!(weight("sweep_ms = 5000", Some(""), 1), Some(572));
        // Reference prices every 1 s: 2 x 60 = 120; 24 + 120 + 168 + 20 = 332.
        let fast_ref = format!("{testnet}\nref_price_interval_ms = 1000");
        assert_eq!(parse(&fast_ref, "", Some(""), 1).unwrap().steady_state_query_weight(), Some(332));
        // Venue setting every 20 s: 20 x 3 = 60; 216 + 60 = 276.
        assert_eq!(weight("venue_setting_poll_ms = 20000", Some(""), 1), Some(276));

        // Exactly 600 loads. Venue setting every 3.125 s is 20 x 19.2 = 384
        // and 216 + 384 = 600. One millisecond faster is 384.1..., counted
        // as 385 (a part of a unit is a whole one): 601 does not load.
        assert_eq!(weight("venue_setting_poll_ms = 3125", Some(""), 1), Some(600));
        assert_eq!(check(&testnet, "venue_setting_poll_ms = 3125", Some("")), Ok(()));
        assert_eq!(weight("venue_setting_poll_ms = 3124", Some(""), 1), Some(601));
        self::refused(&testnet, "venue_setting_poll_ms = 3124", Some(""), "query weight 601 per minute is above 600");
        // Sweep every 4 s: 42 x 15 = 630; 24 + 24 + 630 + 20 = 698.
        self::refused(&testnet, "sweep_ms = 4000", Some(""), "query weight 698 per minute is above 600");
        // Mainnet is held to the same budget.
        self::refused(&hyperliquid("mainnet"), "sweep_ms = 4000", Some(""), "query weight 698 per minute is above 600");

        // Without a margin policy the margin and venue-setting queries are
        // not sent (24 + 168 = 192 for any number of instruments), and the
        // budget is not enforced: such a configuration loads as it did
        // before the margin rules.
        assert_eq!(weight("", None, 1), Some(192));
        assert_eq!(weight("", None, 20), Some(192));
        assert_eq!(check(&testnet, "sweep_ms = 4000", None), Ok(()));
        // The simulated venue has no request limit.
        assert_eq!(parse(SIM, "sweep_ms = 4000", Some(""), 20).unwrap().steady_state_query_weight(), None);
        assert_eq!(check(SIM, "sweep_ms = 4000", Some("")), Ok(()));
    }

    /// S21. Mainnet does not start without a margin policy, and a margin
    /// policy with a value outside the contract's 설정 table does not load
    /// on any venue.
    #[test]
    fn mainnet_is_refused_without_a_margin_policy_and_bad_margin_values_are_refused() {
        // Mainnet: the section must be there. Empty is enough: the defaults
        // of the contract apply.
        refused(&hyperliquid("mainnet"), "", None, "mainnet requires [core.policy.margin]");
        assert_eq!(check(&hyperliquid("mainnet"), "", Some("")), Ok(()));
        // Elsewhere the section stays optional.
        assert_eq!(check(&hyperliquid("testnet"), "", None), Ok(()));
        assert_eq!(check(SIM, "", None), Ok(()));
        assert_eq!(check(SIM, "", Some("")), Ok(()));

        // A limit of zero.
        refused(SIM, "", Some("max_account_leverage = \"0\""), "max_account_leverage must be greater than 0");
        refused(SIM, "", Some("daily_loss_bps = 0"), "daily_loss_bps must be greater than 0");
        refused(SIM, "", Some("margin_max_age_us = 0"), "margin_max_age_us must be greater than 0");
        refused(SIM, "", Some("venue_leverage = 0"), "venue_leverage must be at least 1");
        refused(SIM, "margin_poll_ms = 0", Some(""), "margin_poll_ms must be greater than 0");
        refused(SIM, "venue_setting_poll_ms = 0", Some(""), "venue_setting_poll_ms must be greater than 0");
        assert_eq!(check(SIM, "venue_setting_poll_ms = 1", Some("")), Ok(()));
        // The MM buffer may be zero.
        assert_eq!(check(SIM, "", Some("mm_buffer_bps = 0")), Ok(()));

        // The stale-cancel limit must be greater than the freshness limit:
        // equal is refused, one microsecond more loads.
        let because = "stale_cancel_us must be greater than margin_max_age_us";
        refused(SIM, "", Some("margin_max_age_us = 30000000\nstale_cancel_us = 30000000"), because);
        refused(SIM, "", Some("stale_cancel_us = 29999999"), because);
        assert_eq!(check(SIM, "", Some("margin_max_age_us = 30000000\nstale_cancel_us = 30000001")), Ok(()));

        // The same bad values are refused on mainnet too.
        refused(&hyperliquid("mainnet"), "", Some("daily_loss_bps = 0"), "daily_loss_bps must be greater than 0");
    }

    /// U17d. With a margin policy Bybit needs the account maintenance-margin
    /// limit, 1..=9999 bps; every other venue refuses it (Hyperliquid and
    /// the simulated venue judge isolated positions; their reports carry no
    /// account numbers, so every report would be refused).
    #[test]
    fn the_account_mm_limit_is_required_on_bybit_and_refused_elsewhere() {
        let cross = "isolated = false\naccount_modes_allowed = [\"REGULAR_MARGIN\"]";
        let with = |limit: &str| format!("{cross}\naccount_mm_rate_max_bps = {limit}");
        for limit in ["1", "5000", "9999"] {
            assert_eq!(check_bybit(&bybit("testnet", ""), Some(&with(limit))), Ok(()), "{limit}");
        }
        for (margin, because) in [
            (cross.to_string(), "Bybit needs account_mm_rate_max_bps"),
            (with("0"), "account_mm_rate_max_bps must be 1..=9999"),
            (with("10000"), "account_mm_rate_max_bps must be 1..=9999"),
        ] {
            match check_bybit(&bybit("testnet", ""), Some(&margin)) {
                Ok(()) => panic!("loaded, must be refused ({because}): {margin}"),
                Err(e) => assert!(e.contains(because), "{because}: {e}"),
            }
        }
        // Without a margin policy there is nothing to require.
        assert_eq!(check_bybit(&bybit("testnet", ""), None), Ok(()));

        let because = "account_mm_rate_max_bps is for a cross-margin account (Bybit) only";
        refused(&hyperliquid("testnet"), "", Some("account_mm_rate_max_bps = 5000"), because);
        refused(&hyperliquid("mainnet"), "", Some("account_mm_rate_max_bps = 5000"), because);
        refused(SIM, "", Some("account_mm_rate_max_bps = 5000"), because);
        assert_eq!(check(&hyperliquid("testnet"), "", Some("")), Ok(()));
    }

    /// C-ALERT. The `[alert]` table: an absolute token path, each bound at
    /// both ends, only `second_channel = "none"`; mainnet (Hyperliquid and
    /// Bybit) does not start without the table, elsewhere it is optional.
    #[test]
    fn the_alert_table_is_validated_and_required_on_mainnet() {
        let with = |venue: &str, margin: Option<&str>, edit: &dyn Fn(&mut AlertSection)| {
            let mut cfg = parse(venue, "", margin, 1).unwrap();
            cfg.data_dir = "/var/lib/hl-oms/data".into();
            if venue.contains("bybit") {
                cfg.core.policy.absence_not_found_needed = 2;
            }
            edit(cfg.alert.as_mut().expect("the helper writes [alert]"));
            cfg.validate()
        };
        let a = parse(SIM, "", None, 1).unwrap().alert.unwrap();
        assert_eq!(
            (a.grace_ms, a.eval_ms, a.repeat_ms, a.exit_flush_ms, a.second_channel.as_str(), a.second_after_ms),
            (30_000, 1_000, 300_000, 15_000, "none", 60_000)
        );
        assert_eq!(with(SIM, None, &|_| {}), Ok(()));

        let because = "telegram must be an absolute path";
        for rel in ["telegram.json", "./t.json", "secrets/t.json"] {
            let e = with(SIM, None, &|a| a.telegram = rel.into()).unwrap_err();
            assert!(e.contains(because), "{rel}: {e}");
        }
        type Field = fn(&mut AlertSection) -> &mut u64;
        let fields: [(&str, Field, u64, u64); 5] = [
            ("grace_ms", |a| &mut a.grace_ms, 0, 600_000),
            ("eval_ms", |a| &mut a.eval_ms, 100, 10_000),
            ("repeat_ms", |a| &mut a.repeat_ms, 60_000, 3_600_000),
            ("exit_flush_ms", |a| &mut a.exit_flush_ms, 1_000, 30_000),
            ("second_after_ms", |a| &mut a.second_after_ms, 10_000, 600_000),
        ];
        for (name, field, lo, hi) in fields {
            assert_eq!(with(SIM, None, &|a| *field(a) = lo), Ok(()), "{name} = {lo}");
            assert_eq!(with(SIM, None, &|a| *field(a) = hi), Ok(()), "{name} = {hi}");
            let mut outside = vec![hi + 1];
            if lo > 0 {
                outside.push(lo - 1);
            }
            for v in outside {
                let e = with(SIM, None, &|a| *field(a) = v).unwrap_err();
                assert!(e.contains(&format!("[alert] {name} must be between {lo} and {hi}")), "{name} = {v}: {e}");
            }
        }
        for other in ["pushover", "", "None"] {
            let e = with(SIM, None, &|a| a.second_channel = other.into()).unwrap_err();
            assert!(e.contains("second_channel") && e.contains("D3"), "{other}: {e}");
        }
        // An unknown key in the table.
        let text = "data_dir = \"d\"\n[venue]\nkind = \"sim\"\nsocket = \"v.sock\"\n[core]\nversion = 1\n[alert]\ntelegram = \"/t.json\"\ntoken = \"x\"\n";
        assert!(toml::from_str::<NodeConfig>(text).is_err());

        // Mainnet: required. Testnet and the simulated venue: optional.
        let without = |venue: &str, margin: Option<&str>| {
            let mut cfg = parse(venue, "", margin, 1).unwrap();
            cfg.data_dir = "/var/lib/hl-oms/data".into();
            cfg.core.policy.absence_not_found_needed = if venue.contains("bybit") { 2 } else { 1 };
            cfg.alert = None;
            cfg.validate()
        };
        let because = "mainnet requires [alert]";
        let bybit_margin = Some("isolated = false\naccount_mm_rate_max_bps = 5000");
        for (venue, margin) in [(hyperliquid("mainnet"), Some("")), (bybit("mainnet", "mainnet_confirmed = true"), bybit_margin)] {
            let e = without(&venue, margin).unwrap_err();
            assert!(e.contains(because), "{venue}: {e}");
            assert_eq!(with(&venue, margin, &|_| {}), Ok(()), "{venue}");
        }
        assert_eq!(without(&hyperliquid("testnet"), None), Ok(()));
        assert_eq!(without(&bybit("testnet", ""), None), Ok(()));
        assert_eq!(without(SIM, None), Ok(()));
        // Mainnet without mainnet_confirmed is refused for that reason first.
        let e = without(&bybit("mainnet", ""), None).unwrap_err();
        assert!(e.contains("mainnet_confirmed"), "{e}");
    }

    /// C-HOLD. The `[kill]` table: an absolute marker path outside the data
    /// directory, in a directory that exists at load; mainnet (Hyperliquid
    /// and Bybit) does not start without it, elsewhere it is optional.
    #[test]
    fn the_kill_marker_is_validated_and_required_on_mainnet() {
        let with = |venue: &str, margin: Option<&str>, marker: Option<&str>| {
            let mut cfg = parse(venue, "", margin, 1).unwrap();
            cfg.data_dir = "/var/lib/hl-oms/data".into();
            cfg.core.policy.absence_not_found_needed = if venue.contains("bybit") { 2 } else { 1 };
            cfg.kill = marker.map(|m| KillSection { marker: m.into() });
            cfg.validate()
        };
        assert_eq!(parse(SIM, "", None, 1).unwrap().kill.unwrap().marker, PathBuf::from("/var/lib/hl-oms/kill.marker"));
        for (marker, because) in [
            ("kill.marker", "absolute"),
            ("./kill.marker", "absolute"),
            ("/var/lib/hl-oms/data/kill.marker", "outside the data directory"),
            ("/var/lib/hl-oms/data", "outside the data directory"),
            ("/srv/x/../../var/lib/hl-oms/data/k", "`..`"),
        ] {
            let e = with(SIM, None, Some(marker)).unwrap_err();
            assert!(e.contains(because), "{marker}: {e}");
        }
        assert_eq!(with(SIM, None, Some("/var/lib/hl-oms/kill.marker")), Ok(()));

        let because = "mainnet requires [kill]";
        let bybit_margin = Some("isolated = false\naccount_mm_rate_max_bps = 5000");
        for (venue, margin) in [(hyperliquid("mainnet"), Some("")), (bybit("mainnet", "mainnet_confirmed = true"), bybit_margin)] {
            let e = with(&venue, margin, None).unwrap_err();
            assert!(e.contains(because), "{venue}: {e}");
            assert_eq!(with(&venue, margin, Some("/var/lib/hl-oms/kill.marker")), Ok(()), "{venue}");
        }
        assert_eq!(with(&hyperliquid("testnet"), None, None), Ok(()));
        assert_eq!(with(&bybit("testnet", ""), None, None), Ok(()));
        assert_eq!(with(SIM, None, None), Ok(()));
        // An unknown key in the table.
        let text = "data_dir = \"d\"\n[venue]\nkind = \"sim\"\nsocket = \"v.sock\"\n[core]\nversion = 1\n[kill]\nmarker = \"/k\"\npoll_ms = 1\n";
        assert!(toml::from_str::<NodeConfig>(text).is_err());

        // At load the marker's directory must exist.
        let tmp = std::env::temp_dir().join(format!("kill-cfg-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        let sim = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../config/sim.toml")).unwrap();
        let load = |marker: &Path| {
            let text = format!("{sim}\n[kill]\nmarker = \"{}\"\n", marker.display());
            let path = tmp.join("oms.toml");
            std::fs::write(&path, text).unwrap();
            NodeConfig::load(&path)
        };
        let cfg = load(&tmp.join("kill.marker")).expect("loads");
        assert_eq!(cfg.kill.unwrap().marker, tmp.join("kill.marker"));
        let e = load(&tmp.join("missing/kill.marker")).unwrap_err();
        assert!(e.contains("does not exist"), "{e}");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// C-ALERT. `OMS_TEST_TELEGRAM_API` is taken only as an http URL on a
    /// loopback address.
    #[test]
    fn the_test_telegram_api_must_be_loopback_http() {
        assert_eq!(TEST_TELEGRAM_API, "OMS_TEST_TELEGRAM_API");
        for ok in ["http://127.0.0.1:8080", "http://127.0.0.1:9/", "http://[::1]:8080"] {
            assert_eq!(test_api_base(ok), Ok(ok.to_string()), "{ok}");
        }
        for bad in ["https://api.telegram.org", "http://api.telegram.org", "http://10.0.0.1:80", "http://localhost:80", "https://127.0.0.1:443", "127.0.0.1:80", ""] {
            assert!(test_api_base(bad).is_err(), "{bad} was taken");
        }
    }

    fn bybit(network: &str, more: &str) -> String {
        format!(
            "kind = \"bybit\"\nnetwork = \"{network}\"\ncredentials_file = \"/etc/hl-oms/bybit.toml\"\nsub_account_uid = 4242\nip_echo_url = \"https://ip.example/\"\n{more}"
        )
    }

    /// A Bybit configuration with the ADR-024 policy values set, validated.
    fn check_bybit(venue: &str, margin: Option<&str>) -> Result<(), String> {
        let mut cfg = parse(venue, "", margin, 1)?;
        cfg.data_dir = "/var/lib/hl-oms/data".into();
        cfg.core.policy.absence_not_found_needed = 2;
        cfg.validate()
    }

    #[test]
    fn a_bybit_configuration_loads_and_each_unsafe_value_is_refused() {
        assert_eq!(check_bybit(&bybit("testnet", ""), None), Ok(()));
        assert_eq!(check_bybit(&bybit("mainnet", "mainnet_confirmed = true"), Some("isolated = false\naccount_mm_rate_max_bps = 5000")), Ok(()));
        assert_eq!(
            check_bybit(&bybit("testnet", ""), Some("isolated = false\naccount_modes_allowed = [\"REGULAR_MARGIN\"]\naccount_mm_rate_max_bps = 5000")),
            Ok(())
        );
        let cases = [
            (bybit("mainnet", ""), None, "mainnet_confirmed"),
            (bybit("mainnet", "mainnet_confirmed = true"), None, "requires [core.policy.margin]"),
            (bybit("mainnet", "mainnet_confirmed = true"), Some(""), "isolated = false"),
            (bybit("demo", ""), None, "unknown network"),
            (bybit("testnet", "").replace("/etc/hl-oms/bybit.toml", "bybit.toml"), None, "absolute"),
            (bybit("testnet", "").replace("/etc/hl-oms/bybit.toml", "/var/lib/hl-oms/data/k.toml"), None, "outside the data directory"),
            (bybit("testnet", "").replace("4242", "0"), None, "sub_account_uid"),
            (bybit("testnet", "").replace("https://ip.example/", "http://ip.example/"), None, "https"),
            (bybit("testnet", "ip_check_secs = 5"), None, "ip_check_secs"),
            (bybit("testnet", "recv_window_ms = 60000"), None, "recv_window_ms"),
            (bybit("testnet", "ping_interval_ms = 500"), None, "ping_interval_ms"),
            (bybit("testnet", ""), Some("isolated = false\naccount_modes_allowed = [\"REGULAR_MARGIN\", \"PORTFOLIO_MARGIN\"]\naccount_mm_rate_max_bps = 5000"), "only the account mode REGULAR_MARGIN"),
        ];
        for (venue, margin, because) in cases {
            match check_bybit(&venue, margin) {
                Ok(()) => panic!("loaded, must be refused ({because})"),
                Err(e) => assert!(e.contains(because), "{because}: {e}"),
            }
        }
        // `..`, a symlinked directory and a relative data directory do not
        // hide a credentials file inside the data directory.
        assert!(check_bybit(&bybit("testnet", "").replace("/etc/hl-oms/bybit.toml", "/srv/x/../../var/lib/hl-oms/data/k.toml"), None)
            .unwrap_err()
            .contains("`..`"));
        let tmp = std::env::temp_dir().join(format!("bybit-cfg-{}", std::process::id()));
        let data = tmp.join("data");
        std::fs::create_dir_all(&data).unwrap();
        std::os::unix::fs::symlink(&data, tmp.join("alias")).unwrap();
        let mut cfg = parse(&bybit("testnet", "").replace("/etc/hl-oms/bybit.toml", &tmp.join("alias/k.toml").display().to_string()), "", None, 1).unwrap();
        cfg.data_dir = data.clone();
        cfg.core.policy.absence_not_found_needed = 2;
        assert!(cfg.validate().unwrap_err().contains("outside the data directory"), "symlinked directory");
        let cwd = std::env::current_dir().unwrap();
        let mut cfg = parse(&bybit("testnet", "").replace("/etc/hl-oms/bybit.toml", &cwd.join("d/k.toml").display().to_string()), "", None, 1).unwrap();
        cfg.data_dir = "d".into();
        cfg.core.policy.absence_not_found_needed = 2;
        assert!(cfg.validate().unwrap_err().contains("outside the data directory"), "relative data directory");
        let _ = std::fs::remove_dir_all(&tmp);
        // Without the ADR-024 absence count.
        let mut cfg = parse(&bybit("testnet", ""), "", None, 1).unwrap();
        cfg.data_dir = "/var/lib/hl-oms/data".into();
        assert!(cfg.validate().unwrap_err().contains("absence_not_found_needed = 2"));
        // An unknown key in the venue table.
        assert!(parse(&bybit("testnet", "api_secret = \"x\""), "", None, 1).is_err());
    }

    /// The reference price interval (U17c): at least 1 s, and at most half
    /// of the freshness limit, so one lost update does not make the price
    /// stale. The default is 5 s against the default limit of 15 s.
    #[test]
    fn the_bybit_reference_price_interval_is_bounded_by_one_second_and_half_the_freshness_limit() {
        let with = |interval: Option<u64>, max_age_us: Option<u64>| {
            let more = interval.map(|ms| format!("ref_price_interval_ms = {ms}")).unwrap_or_default();
            let mut cfg = parse(&bybit("testnet", &more), "", None, 1).unwrap();
            cfg.data_dir = "/var/lib/hl-oms/data".into();
            cfg.core.policy.absence_not_found_needed = 2;
            if let Some(us) = max_age_us {
                cfg.core.policy.ref_price_max_age_us = us;
            }
            let VenueConfig::Bybit(b) = &cfg.venue else { panic!("not bybit") };
            (b.ref_price_interval_ms, cfg.validate())
        };
        assert_eq!(oms_core::Policy::default().ref_price_max_age_us, 15_000_000);
        assert_eq!(with(None, None), (5_000, Ok(())));
        assert_eq!(with(Some(1_000), None), (1_000, Ok(())));
        assert_eq!(with(Some(7_500), None), (7_500, Ok(())));
        assert_eq!(with(Some(2_000), Some(4_000_000)), (2_000, Ok(())));
        for (interval, max_age) in [(999, None), (0, None), (7_501, None), (2_001, Some(4_000_000)), (5_000, Some(9_999_999))] {
            let (_, r) = with(Some(interval), max_age);
            let e = r.expect_err(&format!("{interval} ms loaded"));
            assert!(e.contains("ref_price_interval_ms"), "{interval}: {e}");
        }
    }
}
