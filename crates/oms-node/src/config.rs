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
        if b.credentials_file.starts_with(&self.data_dir) {
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
        assert_eq!(check_bybit(&bybit("mainnet", "mainnet_confirmed = true"), Some("isolated = false")), Ok(()));
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
        ];
        for (venue, margin, because) in cases {
            match check_bybit(&venue, margin) {
                Ok(()) => panic!("loaded, must be refused ({because})"),
                Err(e) => assert!(e.contains(because), "{because}: {e}"),
            }
        }
        // Without the ADR-024 absence count.
        let mut cfg = parse(&bybit("testnet", ""), "", None, 1).unwrap();
        cfg.data_dir = "/var/lib/hl-oms/data".into();
        assert!(cfg.validate().unwrap_err().contains("absence_not_found_needed = 2"));
        // An unknown key in the venue table.
        assert!(parse(&bybit("testnet", "api_secret = \"x\""), "", None, 1).is_err());
    }
}
