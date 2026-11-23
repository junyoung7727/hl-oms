//! The watcher's config file (C-WATCH part 4): `watcher --config <path>`,
//! TOML, unknown keys refused. Every field, its default and its validation
//! follow the spec's table; the ranges are judgments (판단(모델 아님)), the
//! defaults are design values (§7.5) unless the table says otherwise.
//! Nothing here touches the network; [`Config::load`] reads the config
//! file only, and the credentials file is read by [`crate::creds::load`].

use std::path::{Component, Path, PathBuf};

use serde::Deserialize;

use crate::rest::ClientConfig;

/// The venue network. Only these two exist.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Network {
    Testnet,
    Mainnet,
}

impl Network {
    /// The REST base, the same hosts as the OMS.
    pub fn rest_base(self) -> &'static str {
        match self {
            Network::Testnet => "https://api-testnet.bybit.com",
            Network::Mainnet => "https://api.bybit.com",
        }
    }
}

/// The loaded and validated config.
#[derive(Debug, Clone)]
pub struct Config {
    /// The heartbeat label and the `watcher` metric label.
    pub name: String,
    pub network: Network,
    pub mainnet_confirmed: bool,
    /// The TOML file with `api_key` and `api_secret` ([`crate::creds`]).
    pub credentials_file: PathBuf,
    /// The key's `userID` must equal it (U19a4b).
    pub sub_account_uid: u64,
    /// The traded symbols, each settling in USDT (checked at start, U19a4b).
    pub symbols: Vec<String>,
    pub ip_echo_url: String,
    /// The OMS data directory; the watcher only reads it.
    pub data_dir: PathBuf,
    /// `http://127.0.0.1:<port>/api/status` or `http://[::1]:<port>/api/status`.
    pub status_url: String,
    pub admin_socket: PathBuf,
    pub monitor_socket: PathBuf,
    /// The `host` metric label.
    pub host: String,
    pub intervals: Intervals,
    pub rules: Rules,
    pub w3: W3,
    pub http: Http,
    pub telemetry: Telemetry,
}

/// `[intervals]`: how often each check, the heartbeat, the IP recheck and
/// the metrics push run.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Intervals {
    pub w1_ms: u64,
    pub w2_ms: u64,
    pub w3_ms: u64,
    pub w4_ms: u64,
    pub heartbeat_ms: u64,
    pub ip_check_ms: u64,
    pub export_ms: u64,
}

impl Default for Intervals {
    fn default() -> Intervals {
        Intervals {
            w1_ms: 1000,
            w2_ms: 1000,
            w3_ms: 2000,
            w4_ms: 5000,
            heartbeat_ms: 5000,
            ip_check_ms: 60_000,
            export_ms: 1000,
        }
    }
}

/// `[rules]`: the age rule, the W4 sweep and the resend pace.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Rules {
    pub min_age_ms: u64,
    pub w4_overlap_ms: u64,
    pub w4_max_pages: u64,
    pub w4_initial_lookback_ms: u64,
    pub mismatch_resend_ms: u64,
}

impl Default for Rules {
    fn default() -> Rules {
        Rules {
            min_age_ms: 2000,
            w4_overlap_ms: 10_000,
            w4_max_pages: 10,
            w4_initial_lookback_ms: 600_000,
            mismatch_resend_ms: 10_000,
        }
    }
}

/// `[w3]`: the OMS's own margin limits (C-MARGIN).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct W3 {
    pub account_mm_rate_max_bps: u64,
    pub daily_loss_bps: u64,
}

impl Default for W3 {
    fn default() -> W3 {
        W3 {
            account_mm_rate_max_bps: 5000,
            daily_loss_bps: 500,
        }
    }
}

/// `[http]`: the Bybit client's windows.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Http {
    pub recv_window_ms: u64,
    pub request_timeout_ms: u64,
}

impl Default for Http {
    fn default() -> Http {
        Http {
            recv_window_ms: 5000,
            request_timeout_ms: 2000,
        }
    }
}

/// `[telemetry]`: where the metrics go; nothing is exported without it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Telemetry {
    pub vm_url: Option<String>,
}

/// The file as written: the optional fields stay optional until defaults
/// are applied, so an explicit empty value is not taken for the default.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    name: String,
    network: Network,
    #[serde(default)]
    mainnet_confirmed: bool,
    credentials_file: PathBuf,
    sub_account_uid: u64,
    symbols: Vec<String>,
    ip_echo_url: String,
    data_dir: PathBuf,
    status_url: String,
    admin_socket: Option<PathBuf>,
    monitor_socket: Option<PathBuf>,
    host: Option<String>,
    #[serde(default)]
    intervals: Intervals,
    #[serde(default)]
    rules: Rules,
    #[serde(default)]
    w3: W3,
    #[serde(default)]
    http: Http,
    #[serde(default)]
    telemetry: Telemetry,
}

/// The default `host` label: `/etc/hostname`, else `"unknown"`.
pub fn host_name() -> String {
    std::fs::read_to_string("/etc/hostname")
        .ok()
        .map(|h| h.trim().to_string())
        .filter(|h| !h.is_empty())
        .unwrap_or_else(|| "unknown".into())
}

impl Config {
    /// Reads and validates `path`. Every refusal is one line that names
    /// the file and the reason.
    pub fn load(path: &Path) -> Result<Config, String> {
        let text =
            std::fs::read_to_string(path).map_err(|e| format!("config {}: {e}", path.display()))?;
        Config::from_toml(&text).map_err(|e| format!("config {}: {e}", path.display()))
    }

    /// Parses and validates the text of a config file.
    pub fn from_toml(text: &str) -> Result<Config, String> {
        let f: File = toml::from_str(text).map_err(|e| one_line(text, &e))?;
        let cfg = Config {
            admin_socket: f
                .admin_socket
                .unwrap_or_else(|| f.data_dir.join("admin.sock")),
            monitor_socket: f
                .monitor_socket
                .unwrap_or_else(|| f.data_dir.join("monitor.sock")),
            host: f.host.unwrap_or_else(host_name),
            name: f.name,
            network: f.network,
            mainnet_confirmed: f.mainnet_confirmed,
            credentials_file: f.credentials_file,
            sub_account_uid: f.sub_account_uid,
            symbols: f.symbols,
            ip_echo_url: f.ip_echo_url,
            data_dir: f.data_dir,
            status_url: f.status_url,
            intervals: f.intervals,
            rules: f.rules,
            w3: f.w3,
            http: f.http,
            telemetry: f.telemetry,
        };
        cfg.validate()?;
        Ok(cfg)
    }

    /// What the Bybit client needs.
    pub fn client_config(&self) -> ClientConfig {
        ClientConfig {
            base: self.network.rest_base().to_string(),
            recv_window_ms: self.http.recv_window_ms,
            request_timeout_ms: self.http.request_timeout_ms,
        }
    }

    fn validate(&self) -> Result<(), String> {
        let chars = self.name.chars().count();
        if chars == 0 || chars > 64 || self.name.chars().any(char::is_control) {
            return Err(
                "name must be 1 to 64 characters without control characters (the heartbeat label)"
                    .into(),
            );
        }
        if self.network == Network::Mainnet && !self.mainnet_confirmed {
            return Err("mainnet requires mainnet_confirmed = true".into());
        }
        if !self.data_dir.is_absolute() {
            return Err("data_dir must be an absolute path".into());
        }
        check_credentials_path(&self.credentials_file, &self.data_dir)?;
        if self.sub_account_uid == 0 {
            return Err("sub_account_uid must be the sub-account's UID (> 0)".into());
        }
        self.check_symbols()?;
        if !plain(&self.ip_echo_url) {
            return Err(
                "ip_echo_url must be written without white space or control characters".into(),
            );
        }
        // An `https` URL always has a host: parsing refuses `https://`.
        let echo =
            reqwest::Url::parse(&self.ip_echo_url).map_err(|e| format!("ip_echo_url: {e}"))?;
        if echo.scheme() != "https" {
            return Err(
                "ip_echo_url must be https: a forged answer would pass the IP check".into(),
            );
        }
        if !echo.username().is_empty() || echo.password().is_some() {
            return Err("ip_echo_url must have no user or password".into());
        }
        check_status_url(&self.status_url)?;
        for (field, p) in [
            ("admin_socket", &self.admin_socket),
            ("monitor_socket", &self.monitor_socket),
        ] {
            if !p.is_absolute() {
                return Err(format!("{field} must be an absolute path"));
            }
        }
        if resolved(&self.admin_socket) == resolved(&self.monitor_socket) {
            return Err("admin_socket and monitor_socket must be different".into());
        }
        let chars = self.host.chars().count();
        if chars == 0 || chars > 255 || self.host.chars().any(char::is_control) {
            return Err("host must be 1 to 255 characters without control characters".into());
        }
        let i = &self.intervals;
        let r = &self.rules;
        for (table, field, v, lo, hi) in [
            ("intervals", "w1_ms", i.w1_ms, 500, 10_000),
            ("intervals", "w2_ms", i.w2_ms, 500, 10_000),
            ("intervals", "w3_ms", i.w3_ms, 1000, 30_000),
            ("intervals", "w4_ms", i.w4_ms, 1000, 60_000),
            ("intervals", "heartbeat_ms", i.heartbeat_ms, 1000, 10_000),
            ("intervals", "ip_check_ms", i.ip_check_ms, 10_000, 3_600_000),
            ("intervals", "export_ms", i.export_ms, 1000, 60_000),
            ("rules", "min_age_ms", r.min_age_ms, 500, 10_000),
            ("rules", "w4_overlap_ms", r.w4_overlap_ms, 0, 60_000),
            ("rules", "w4_max_pages", r.w4_max_pages, 1, 50),
            (
                "rules",
                "w4_initial_lookback_ms",
                r.w4_initial_lookback_ms,
                0,
                86_400_000,
            ),
            (
                "rules",
                "mismatch_resend_ms",
                r.mismatch_resend_ms,
                1000,
                600_000,
            ),
            (
                "w3",
                "account_mm_rate_max_bps",
                self.w3.account_mm_rate_max_bps,
                1,
                9999,
            ),
            ("w3", "daily_loss_bps", self.w3.daily_loss_bps, 1, 10_000),
            (
                "http",
                "recv_window_ms",
                self.http.recv_window_ms,
                1000,
                20_000,
            ),
            (
                "http",
                "request_timeout_ms",
                self.http.request_timeout_ms,
                200,
                10_000,
            ),
        ] {
            if !(lo..=hi).contains(&v) {
                return Err(format!("[{table}] {field} must be between {lo} and {hi}"));
            }
        }
        if let Some(u) = &self.telemetry.vm_url {
            check_vm_url(u)?;
        }
        Ok(())
    }

    fn check_symbols(&self) -> Result<(), String> {
        if self.symbols.is_empty() || self.symbols.len() > 50 {
            return Err("symbols must list 1 to 50 names".into());
        }
        let mut seen = std::collections::BTreeSet::new();
        for s in &self.symbols {
            let shape = (1..=32).contains(&s.len())
                && s.bytes()
                    .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit());
            if !shape {
                return Err(format!("symbols: {s:?} must be 1 to 32 of A-Z and 0-9"));
            }
            if !seen.insert(s) {
                return Err(format!("symbols: {s} is listed twice"));
            }
        }
        Ok(())
    }
}

/// The path rules of `credentials_file`: absolute, no `..`, outside the
/// data directory (compared by where both point, symlinks followed where
/// they exist). [`crate::creds::load`] applies them again at the read.
pub fn check_credentials_path(file: &Path, data_dir: &Path) -> Result<(), String> {
    if !file.is_absolute() {
        return Err("credentials_file must be an absolute path".into());
    }
    if file.components().any(|c| c == Component::ParentDir) {
        return Err("credentials_file must not hold `..`".into());
    }
    if resolved(file).starts_with(resolved(data_dir)) {
        return Err("credentials_file must be outside the data directory".into());
    }
    Ok(())
}

/// `http://127.0.0.1:<port>/api/status` or `http://[::1]:<port>/api/status`
/// with the port written in decimal, 1–65535 but not 80: the dashboard
/// binds loopback only, and it refuses a `Host` header without the port,
/// which the client sends for the default port 80.
fn check_status_url(u: &str) -> Result<(), String> {
    let refuse = || {
        Err(format!(
            "status_url must be http://127.0.0.1:<port>/api/status or http://[::1]:<port>/api/status (the OMS dashboard is loopback only), not {u:?}"
        ))
    };
    let Some(rest) = u
        .strip_prefix("http://127.0.0.1:")
        .or_else(|| u.strip_prefix("http://[::1]:"))
    else {
        return refuse();
    };
    let Some(port) = rest.strip_suffix("/api/status") else {
        return refuse();
    };
    if port.is_empty()
        || port.len() > 5
        || port.starts_with('0')
        || !port.bytes().all(|b| b.is_ascii_digit())
    {
        return refuse();
    }
    match port.parse::<u32>() {
        Ok(80) => Err("status_url: port 80 is left out of the Host header, which the OMS dashboard refuses; use another port".into()),
        Ok(1..=65_535) => Ok(()),
        _ => refuse(),
    }
}

/// No white space and no control character anywhere. URL parsing drops
/// them at the ends (and tabs and line feeds inside), so the text kept
/// would not be the URL that is checked: the exporter appends its path
/// to the text.
fn plain(u: &str) -> bool {
    !u.chars().any(|c| c.is_whitespace() || c.is_control())
}

/// `http://` to a loopback address literal, no user, query or fragment,
/// written without white space. A refusal quotes the URL unless it may
/// hold a password.
fn check_vm_url(u: &str) -> Result<(), String> {
    let refuse = || {
        if u.contains('@') {
            return Err(
                "[telemetry] vm_url must be http:// to a loopback address, without a user or password"
                    .into(),
            );
        }
        Err(format!(
            "[telemetry] vm_url must be http:// to a loopback address, not {u:?}"
        ))
    };
    if !plain(u) {
        return refuse();
    }
    let Ok(url) = reqwest::Url::parse(u) else {
        return refuse();
    };
    if url.scheme() != "http"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return refuse();
    }
    let host = url.host_str().unwrap_or("");
    let host = host.trim_start_matches('[').trim_end_matches(']');
    match host.parse::<std::net::IpAddr>() {
        Ok(ip) if ip.is_loopback() => Ok(()),
        _ => refuse(),
    }
}

/// A TOML error as one line: its message, the line it starts at and the
/// key written on that line (the error's own text spans several lines
/// with a quote of the file, and a wrong value does not name its key).
/// A missing field names itself; its span is the whole table, so its
/// first line and key would point at another, correct line.
fn one_line(text: &str, e: &toml::de::Error) -> String {
    let msg = e.message().split_whitespace().collect::<Vec<_>>().join(" ");
    let Some(s) = e.span() else {
        return msg;
    };
    if msg.starts_with("missing field ") {
        return msg;
    }
    let start = s.start.min(text.len());
    let line = text[..start].matches('\n').count() + 1;
    let key = text
        .lines()
        .nth(line - 1)
        .and_then(|l| l.split_once('='))
        .map(|(k, _)| k.trim())
        .filter(|k| {
            !k.is_empty()
                && k.chars()
                    .all(|c| c.is_ascii_alphanumeric() || "_-.\"".contains(c))
        });
    match key {
        Some(k) => format!("{msg} at line {line} (key {k})"),
        None => format!("{msg} at line {line}"),
    }
}

/// `p` absolute with `.` and `..` folded and symlinks followed where the
/// path exists: the longest prefix that exists is resolved by the file
/// system, the rest is folded by text. Compares paths by where they point.
pub(crate) fn resolved(p: &Path) -> PathBuf {
    let parts: Vec<Component> = p.components().collect();
    for n in (1..=parts.len()).rev() {
        let head: PathBuf = parts[..n].iter().collect();
        if let Ok(mut r) = head.canonicalize() {
            for c in &parts[n..] {
                match c {
                    Component::ParentDir => {
                        r.pop();
                    }
                    Component::CurDir => {}
                    c => r.push(c),
                }
            }
            return r;
        }
    }
    p.to_path_buf()
}
