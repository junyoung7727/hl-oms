//! Node configuration file (TOML).
//!
//! The `[core]` table is the domain configuration. It reaches the core as an
//! input and is journaled; the rest configures the execution layer of this
//! process and is not part of the domain state.

use oms_core::CoreConfig;
use serde::Deserialize;
use std::path::{Path, PathBuf};

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

fn default_fills_lag_ms() -> u64 {
    2_000
}

fn default_ref_price_ms() -> u64 {
    5_000
}

fn default_key_env() -> String {
    "HL_AGENT_PRIVATE_KEY".into()
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
        Ok(cfg)
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
        }
        Ok(())
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
        let margin = margin.map(|m| format!("[core.policy.margin]\n{m}")).unwrap_or_default();
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

[core.strategies.a]
enabled = true
max_open_orders = 50
[core.strategies.a.limits.X]
min = "-10"
max = "10"

[core.policy]

{margin}
"#
        );
        let cfg: NodeConfig = toml::from_str(&text).map_err(|e| format!("parse: {e}"))?;
        cfg.validate()
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
            })
        );
        assert_eq!((cfg.runtime.margin_poll_ms, cfg.runtime.venue_setting_poll_ms), (5_000, 60_000));
        assert!(matches!(&cfg.venue, VenueConfig::Hyperliquid(h) if h.network == "testnet"));

        let sim = shipped("sim.toml").expect("the shipped simulated-venue example must load");
        assert_eq!(sim.core.policy.margin, None);
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
}
