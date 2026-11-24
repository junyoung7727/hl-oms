//! The watcher's config file (C-WATCH part 4): every field of the spec's
//! table, its default and its validation; unknown keys refused; mainnet
//! only with `mainnet_confirmed = true`; every refusal is one line.

#![allow(clippy::unwrap_used)]

mod common;

use std::path::{Path, PathBuf};

use watcher::config::{Config, Kill, Network, Systemd, W5};

/// The required fields and nothing else.
const MIN: &str = r#"name = "w1"
network = "testnet"
credentials_file = "/etc/watcher/creds.toml"
sub_account_uid = 4242
symbols = ["BTCUSDT", "ETHUSDT"]
ip_echo_url = "https://ip.example/"
data_dir = "/srv/oms/data"
status_url = "http://127.0.0.1:8188/api/status"
"#;

/// `MIN` with the top-level line of `key` replaced by `key = value`, or
/// removed when `value` is `None`, or added when `MIN` has no such line.
fn set(key: &str, value: Option<&str>) -> String {
    let mut out = String::new();
    let mut found = false;
    for line in MIN.lines() {
        if line.split(" = ").next() == Some(key) {
            found = true;
            if let Some(v) = value {
                out.push_str(&format!("{key} = {v}\n"));
            }
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    if !found {
        if let Some(v) = value {
            out = format!("{key} = {v}\n{out}");
        }
    }
    out
}

/// A `[telegram]` section with a good path (U19b1a), for mainnet configs.
const TELEGRAM: &str = "\n[telegram]\ntoken_file = \"/etc/watcher/telegram.json\"\n";

fn ok(text: &str) -> Config {
    match Config::from_toml(text) {
        Ok(c) => c,
        Err(e) => panic!("refused: {e}\n--- config ---\n{text}"),
    }
}

/// Refused, with one line that contains `needle`.
fn refused(text: &str, needle: &str) {
    match Config::from_toml(text) {
        Ok(_) => panic!("accepted, expected a refusal with {needle:?}\n--- config ---\n{text}"),
        Err(e) => {
            assert!(
                e.contains(needle),
                "{e:?} lacks {needle:?}\n--- config ---\n{text}"
            );
            assert!(!e.contains('\n'), "not one line: {e:?}");
        }
    }
}

fn host_name() -> String {
    std::fs::read_to_string("/etc/hostname")
        .ok()
        .map(|h| h.trim().to_string())
        .filter(|h| !h.is_empty())
        .unwrap_or_else(|| "unknown".into())
}

#[test]
fn the_config_is_validated_and_mainnet_needs_confirmation() {
    // The required fields alone: the defaults of the table.
    let c = ok(MIN);
    assert_eq!(c.name, "w1");
    assert_eq!(c.network, Network::Testnet);
    assert!(!c.mainnet_confirmed);
    assert_eq!(c.credentials_file, Path::new("/etc/watcher/creds.toml"));
    assert_eq!(c.sub_account_uid, 4242);
    assert_eq!(c.symbols, ["BTCUSDT", "ETHUSDT"]);
    assert_eq!(c.ip_echo_url, "https://ip.example/");
    assert_eq!(c.data_dir, Path::new("/srv/oms/data"));
    assert_eq!(c.status_url, "http://127.0.0.1:8188/api/status");
    assert_eq!(c.admin_socket, Path::new("/srv/oms/data/admin.sock"));
    assert_eq!(c.monitor_socket, Path::new("/srv/oms/data/monitor.sock"));
    assert_eq!(c.host, host_name());
    let i = &c.intervals;
    assert_eq!(
        (
            i.w1_ms,
            i.w2_ms,
            i.w3_ms,
            i.w4_ms,
            i.heartbeat_ms,
            i.ip_check_ms,
            i.export_ms
        ),
        (1000, 1000, 2000, 5000, 5000, 60_000, 1000)
    );
    let r = &c.rules;
    assert_eq!(
        (
            r.min_age_ms,
            r.w4_overlap_ms,
            r.w4_max_pages,
            r.w4_initial_lookback_ms,
            r.mismatch_resend_ms
        ),
        (2000, 10_000, 10, 600_000, 10_000)
    );
    assert_eq!(
        (c.w3.account_mm_rate_max_bps, c.w3.daily_loss_bps),
        (5000, 500)
    );
    assert_eq!(
        (c.http.recv_window_ms, c.http.request_timeout_ms),
        (5000, 2000)
    );
    assert_eq!(c.telemetry.vm_url, None);
    let cc = c.client_config();
    assert_eq!(cc.base, "https://api-testnet.bybit.com");
    assert_eq!((cc.recv_window_ms, cc.request_timeout_ms), (5000, 2000));

    // Mainnet only with the confirmation; then the mainnet host.
    refused(
        &set("network", Some("\"mainnet\"")),
        "mainnet requires mainnet_confirmed = true",
    );
    let unconfirmed = set("network", Some("\"mainnet\""))
        .replace("name = ", "mainnet_confirmed = false\nname = ");
    refused(&unconfirmed, "mainnet requires mainnet_confirmed = true");
    let confirmed =
        set("network", Some("\"mainnet\"")).replace("name = ", "mainnet_confirmed = true\nname = ");
    let m = ok(&format!("{confirmed}{TELEGRAM}{}", kill_section("config-mainnet")));
    assert_eq!((m.network, m.mainnet_confirmed), (Network::Mainnet, true));
    assert_eq!(m.client_config().base, "https://api.bybit.com");
    // The confirmation does not change a testnet config.
    let t = ok(&set("mainnet_confirmed", Some("true")));
    assert_eq!(t.client_config().base, "https://api-testnet.bybit.com");
    refused(&set("network", Some("\"devnet\"")), "network");

    // Unknown keys, at the top and in every table, and unknown tables.
    refused(&set("bogus", Some("1")), "bogus");
    for table in ["intervals", "rules", "w3", "http", "telemetry"] {
        refused(&format!("{MIN}\n[{table}]\nbogus_ms = 1\n"), "bogus_ms");
    }
    refused(&format!("{MIN}\n[venue]\nkind = \"bybit\"\n"), "venue");
    // Every required field.
    for key in [
        "name",
        "network",
        "credentials_file",
        "sub_account_uid",
        "symbols",
        "ip_echo_url",
        "data_dir",
        "status_url",
    ] {
        // The TOML error of a missing field spans the whole file: its
        // line and the key on it would point at another, correct line.
        let text = set(key, None);
        refused(&text, &format!("missing field `{key}`"));
        let e = Config::from_toml(&text).unwrap_err();
        assert!(
            !e.contains(" at line ") && !e.contains("(key "),
            "a missing field names another line: {e:?}"
        );
    }
    // A parse error is one line too (TOML errors span several).
    refused("name = \n", "line 1");
    refused(
        &format!("{MIN}\n[intervals]\nw1_ms = \"fast\"\n"),
        "line 11",
    );

    // name: 1–64 characters (not bytes), no control characters.
    refused(&set("name", Some("\"\"")), "name");
    let n64 = "é".repeat(64);
    assert_eq!(ok(&set("name", Some(&format!("\"{n64}\"")))).name, n64);
    refused(
        &set("name", Some(&format!("\"{}\"", "a".repeat(65)))),
        "name",
    );
    refused(&set("name", Some("\"a\\tb\"")), "name");
    refused(&set("name", Some("\"a\\u007fb\"")), "name");

    // sub_account_uid > 0.
    refused(&set("sub_account_uid", Some("0")), "sub_account_uid");
    refused(&set("sub_account_uid", Some("-1")), "sub_account_uid");
    assert_eq!(ok(&set("sub_account_uid", Some("1"))).sub_account_uid, 1);

    // symbols: 1–50 names, each 1–32 of [A-Z0-9], none twice.
    refused(&set("symbols", Some("[]")), "symbols");
    let names = |n: usize| {
        (0..n)
            .map(|i| format!("\"S{i}USDT\""))
            .collect::<Vec<_>>()
            .join(", ")
    };
    assert_eq!(
        ok(&set("symbols", Some(&format!("[{}]", names(50)))))
            .symbols
            .len(),
        50
    );
    refused(
        &set("symbols", Some(&format!("[{}]", names(51)))),
        "symbols",
    );
    assert_eq!(
        ok(&set("symbols", Some(&format!("[\"{}\"]", "A".repeat(32))))).symbols[0].len(),
        32
    );
    for bad in [
        "\"\"",
        "\"btcusdt\"",
        "\"BTC-USDT\"",
        "\"BTC USDT\"",
        "\"BTCÜSDT\"",
    ] {
        refused(&set("symbols", Some(&format!("[{bad}]"))), "symbols");
    }
    refused(
        &set("symbols", Some(&format!("[\"{}\"]", "A".repeat(33)))),
        "symbols",
    );
    refused(
        &set("symbols", Some("[\"BTCUSDT\", \"BTCUSDT\"]")),
        "BTCUSDT",
    );

    // ip_echo_url: https only.
    refused(
        &set("ip_echo_url", Some("\"http://ip.example/\"")),
        "ip_echo_url must be https",
    );
    refused(&set("ip_echo_url", Some("\"not a url\"")), "ip_echo_url");
    refused(&set("ip_echo_url", Some("\"https://\"")), "ip_echo_url");
    // No user, no white space or control character (URL parsing drops
    // them at the ends, so the text kept would not be the URL used).
    for bad in [
        "\"https://u:pw@ip.example/\"",
        "\" https://ip.example/\"",
        "\"https://ip.example/ \"",
        "\"https://ip.example/\\n\"",
        "\"https://ip.exa\\tmple/\"",
    ] {
        refused(&set("ip_echo_url", Some(bad)), "ip_echo_url");
    }

    // data_dir absolute; credentials_file absolute, no `..`, outside the
    // data directory (the directory itself included).
    refused(
        &set("data_dir", Some("\"srv/oms/data\"")),
        "data_dir must be an absolute path",
    );
    refused(
        &set("credentials_file", Some("\"creds.toml\"")),
        "credentials_file must be an absolute path",
    );
    refused(
        &set(
            "credentials_file",
            Some("\"/etc/watcher/../watcher/creds.toml\""),
        ),
        "must not hold `..`",
    );
    for inside in [
        "/srv/oms/data/creds.toml",
        "/srv/oms/data/sub/creds.toml",
        "/srv/oms/data",
        "/srv/oms/./data/creds.toml",
    ] {
        refused(
            &set("credentials_file", Some(&format!("\"{inside}\""))),
            "credentials_file must be outside the data directory",
        );
    }
    let beside = ok(&set(
        "credentials_file",
        Some("\"/srv/oms/data-creds.toml\""),
    ));
    assert_eq!(
        beside.credentials_file,
        Path::new("/srv/oms/data-creds.toml")
    );
    // A data directory written with `..` still holds the file.
    refused(
        &set("data_dir", Some("\"/etc/x/../watcher\"")),
        "credentials_file must be outside the data directory",
    );

    // status_url: the loopback dashboard only, with its port.
    for good in [
        "http://127.0.0.1:8188/api/status",
        "http://[::1]:8188/api/status",
        "http://127.0.0.1:65535/api/status",
    ] {
        assert_eq!(
            ok(&set("status_url", Some(&format!("\"{good}\"")))).status_url,
            good
        );
    }
    for bad in [
        "http://localhost:8188/api/status",
        "https://127.0.0.1:8188/api/status",
        "http://10.0.0.1:8188/api/status",
        "http://127.0.0.2:8188/api/status",
        "http://127.0.0.1/api/status",
        "http://127.0.0.1:0/api/status",
        "http://127.0.0.1:80/api/status",
        "http://127.0.0.1:08188/api/status",
        "http://127.0.0.1:65536/api/status",
        "http://127.0.0.1:8188/api/statuses",
        "http://127.0.0.1:8188/api/status/",
        "http://127.0.0.1:8188/api/status?x=1",
        "http://127.0.0.1:8188/api/status#x",
        "http://user@127.0.0.1:8188/api/status",
        "http://127.0.0.1:8188",
    ] {
        refused(
            &set("status_url", Some(&format!("\"{bad}\""))),
            "status_url",
        );
    }

    // The sockets: defaults under data_dir, absolute, different.
    let socks = ok(
        &set("admin_socket", Some("\"/run/oms/admin.sock\"")).replace(
            "name = ",
            "monitor_socket = \"/run/oms/monitor.sock\"\nname = ",
        ),
    );
    assert_eq!(socks.admin_socket, Path::new("/run/oms/admin.sock"));
    assert_eq!(socks.monitor_socket, Path::new("/run/oms/monitor.sock"));
    refused(
        &set("admin_socket", Some("\"admin.sock\"")),
        "admin_socket must be an absolute path",
    );
    refused(
        &set("monitor_socket", Some("\"monitor.sock\"")),
        "monitor_socket must be an absolute path",
    );
    refused(
        &set("monitor_socket", Some("\"/srv/oms/data/admin.sock\"")),
        "admin_socket and monitor_socket must be different",
    );
    refused(
        &set("admin_socket", Some("\"/srv/oms/data/./monitor.sock\"")),
        "admin_socket and monitor_socket must be different",
    );
    // Compared by where they point: through a symlinked directory and
    // through `..`, which `Path` equality does not fold.
    let dir = common::test_dir("config-sockets");
    let real = dir.join("real");
    std::fs::create_dir_all(&real).unwrap();
    let alias = dir.join("alias");
    std::os::unix::fs::symlink(&real, &alias).unwrap();
    let pair = |admin: &Path, monitor: &Path| {
        set("admin_socket", Some(&format!("'{}'", admin.display()))).replace(
            "name = ",
            &format!("monitor_socket = '{}'\nname = ", monitor.display()),
        )
    };
    let same = real.join("oms.sock");
    for admin in [
        alias.join("oms.sock"),
        real.join("sub").join("..").join("oms.sock"),
        alias.join("sub").join("..").join("oms.sock"),
    ] {
        refused(
            &pair(&admin, &same),
            "admin_socket and monitor_socket must be different",
        );
    }
    let apart = ok(&pair(&alias.join("admin.sock"), &same));
    assert_eq!(apart.admin_socket, alias.join("admin.sock"));
    let _ = std::fs::remove_dir_all(&dir);

    // host: a label, given or /etc/hostname.
    assert_eq!(ok(&set("host", Some("\"tokyo-1\""))).host, "tokyo-1");
    refused(&set("host", Some("\"\"")), "host");
    refused(&set("host", Some("\"a\\nb\"")), "host");
    let h255 = "h".repeat(255);
    assert_eq!(ok(&set("host", Some(&format!("\"{h255}\"")))).host, h255);
    refused(
        &set("host", Some(&format!("\"{}\"", "h".repeat(256)))),
        "host",
    );

    // Every range at both bounds and one past each.
    let ranges: [(&str, &str, u64, u64); 16] = [
        ("intervals", "w1_ms", 500, 10_000),
        ("intervals", "w2_ms", 500, 10_000),
        ("intervals", "w3_ms", 1000, 30_000),
        ("intervals", "w4_ms", 1000, 60_000),
        ("intervals", "heartbeat_ms", 1000, 10_000),
        ("intervals", "ip_check_ms", 10_000, 3_600_000),
        ("intervals", "export_ms", 1000, 60_000),
        ("rules", "min_age_ms", 500, 10_000),
        ("rules", "w4_overlap_ms", 0, 60_000),
        ("rules", "w4_max_pages", 1, 50),
        ("rules", "w4_initial_lookback_ms", 0, 86_400_000),
        ("rules", "mismatch_resend_ms", 1000, 600_000),
        ("w3", "account_mm_rate_max_bps", 1, 9999),
        ("w3", "daily_loss_bps", 1, 10_000),
        ("http", "recv_window_ms", 1000, 20_000),
        ("http", "request_timeout_ms", 200, 10_000),
    ];
    for (table, field, lo, hi) in ranges {
        let with = |v: u64| format!("{MIN}\n[{table}]\n{field} = {v}\n");
        for v in [lo, hi] {
            let c = ok(&with(v));
            assert_eq!(value_of(&c, field), v, "{table}.{field} = {v}");
        }
        if lo > 0 {
            refused(
                &with(lo - 1),
                &format!("[{table}] {field} must be between {lo} and {hi}"),
            );
        }
        refused(
            &with(hi + 1),
            &format!("[{table}] {field} must be between {lo} and {hi}"),
        );
    }
    refused(
        &format!("{MIN}\n[rules]\nw4_overlap_ms = -1\n"),
        "w4_overlap_ms",
    );
    // Two fields of one table at once: each bound is its own.
    let two = ok(&format!("{MIN}\n[intervals]\nw1_ms = 500\nw2_ms = 10000\n"));
    assert_eq!((two.intervals.w1_ms, two.intervals.w2_ms), (500, 10_000));
    assert_eq!(two.intervals.w3_ms, 2000);
    refused(
        &format!("{MIN}\n[intervals]\nw1_ms = 500\nw2_ms = 10001\n"),
        "[intervals] w2_ms must be between 500 and 10000",
    );
    refused(
        &format!("{MIN}\n[rules]\nw4_max_pages = 51\nmin_age_ms = 500\n"),
        "[rules] w4_max_pages must be between 1 and 50",
    );

    // vm_url: loopback http only; absent = nothing exported.
    for good in [
        "http://127.0.0.1:8428",
        "http://[::1]:8428/",
        "http://127.0.0.1:8428/vm",
    ] {
        let c = ok(&format!("{MIN}\n[telemetry]\nvm_url = \"{good}\"\n"));
        assert_eq!(c.telemetry.vm_url.as_deref(), Some(good));
    }
    for bad in [
        "https://127.0.0.1:8428",
        "http://10.0.0.5:8428",
        "http://localhost:8428",
        "http://vm.example:8428",
        "http://user:pw@127.0.0.1:8428",
        "http://127.0.0.1:8428/?db=x",
        "http://127.0.0.1:8428/#x",
        "127.0.0.1:8428",
        // The exporter appends `/api/v1/import` to the text: a trailing
        // space would give a URL that never parses.
        "http://127.0.0.1:8428 ",
        " http://127.0.0.1:8428",
        "http://127.0.0.1:84\\t28",
        "http://127.0.0.1:8428\\n",
    ] {
        refused(
            &format!("{MIN}\n[telemetry]\nvm_url = \"{bad}\"\n"),
            "vm_url",
        );
    }
    // A refused URL with a user is not quoted (it may hold a password).
    let e = Config::from_toml(&format!(
        "{MIN}\n[telemetry]\nvm_url = \"http://u:hunter2@10.0.0.1:8428\"\n"
    ))
    .unwrap_err();
    assert!(e.contains("vm_url") && !e.contains("hunter2"), "{e}");
}

fn value_of(c: &Config, field: &str) -> u64 {
    let (i, r) = (&c.intervals, &c.rules);
    match field {
        "w1_ms" => i.w1_ms,
        "w2_ms" => i.w2_ms,
        "w3_ms" => i.w3_ms,
        "w4_ms" => i.w4_ms,
        "heartbeat_ms" => i.heartbeat_ms,
        "ip_check_ms" => i.ip_check_ms,
        "export_ms" => i.export_ms,
        "min_age_ms" => r.min_age_ms,
        "w4_overlap_ms" => r.w4_overlap_ms,
        "w4_max_pages" => r.w4_max_pages,
        "w4_initial_lookback_ms" => r.w4_initial_lookback_ms,
        "mismatch_resend_ms" => r.mismatch_resend_ms,
        "account_mm_rate_max_bps" => c.w3.account_mm_rate_max_bps,
        "daily_loss_bps" => c.w3.daily_loss_bps,
        "recv_window_ms" => c.http.recv_window_ms,
        "request_timeout_ms" => c.http.request_timeout_ms,
        f => panic!("no field {f}"),
    }
}

#[test]
fn the_config_file_is_read_and_its_refusals_name_the_file() {
    let dir = common::test_dir("config-load");
    let p: PathBuf = dir.join("watcher.toml");
    std::fs::write(&p, MIN).unwrap();
    assert_eq!(Config::load(&p).unwrap().name, "w1");
    std::fs::write(&p, set("network", Some("\"mainnet\""))).unwrap();
    let e = Config::load(&p).unwrap_err();
    assert!(
        e.contains(&p.display().to_string()) && e.contains("mainnet_confirmed"),
        "{e}"
    );
    assert!(!e.contains('\n'), "{e:?}");
    std::fs::write(&p, "name = \n").unwrap();
    let e = Config::load(&p).unwrap_err();
    assert!(
        e.contains(&p.display().to_string()) && e.contains("line 1"),
        "{e}"
    );
    assert!(!e.contains('\n'), "{e:?}");
    let missing = dir.join("missing.toml");
    let e = Config::load(&missing).unwrap_err();
    assert!(e.contains(&missing.display().to_string()), "{e}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn telegram_is_validated_and_required_on_mainnet() {
    // Optional on testnet: no section, no sender.
    assert_eq!(ok(MIN).telegram, None);
    let t = ok(&format!("{MIN}{TELEGRAM}"));
    assert_eq!(
        t.telegram.as_ref().map(|t| t.token_file.as_path()),
        Some(Path::new("/etc/watcher/telegram.json"))
    );

    // Mainnet refuses a config without it, with the spec's line, and
    // takes the same config with it.
    let mainnet = set("network", Some("\"mainnet\""))
        .replace("name = ", "mainnet_confirmed = true\nname = ");
    refused(
        &mainnet,
        "mainnet requires [telegram]: the watcher's P1 must reach a person",
    );
    let m = ok(&format!(
        "{mainnet}{TELEGRAM}{}",
        kill_section("config-telegram-mainnet")
    ));
    assert_eq!(m.network, Network::Mainnet);
    assert!(m.telegram.is_some());
    // An unconfirmed mainnet is still refused for the confirmation first.
    refused(
        &format!("{}{TELEGRAM}", set("network", Some("\"mainnet\""))),
        "mainnet requires mainnet_confirmed = true",
    );

    // `token_file` is required in the section; unknown keys are refused.
    refused(&format!("{MIN}\n[telegram]\n"), "missing field `token_file`");
    refused(
        &format!("{MIN}{TELEGRAM}chat_id = 1\n"),
        "unknown field `chat_id`",
    );
    refused(
        &format!("{MIN}{TELEGRAM}token = \"1:a\"\n"),
        "unknown field `token`",
    );
    let path = |p: &str| format!("{MIN}\n[telegram]\ntoken_file = \"{p}\"\n");
    // Absolute, no `..`, outside the data directory (by where it points).
    refused(&path("telegram.json"), "[telegram] token_file must be an absolute path");
    refused(
        &path("/etc/watcher/../watcher/telegram.json"),
        "[telegram] token_file must not hold `..`",
    );
    for inside in [
        "/srv/oms/data/telegram.json",
        "/srv/oms/data",
        "/srv/oms/data/sub/t.json",
        "/srv/oms/./data/t.json",
    ] {
        refused(
            &path(inside),
            "[telegram] token_file must be outside the data directory",
        );
    }
    let dir = common::test_dir("config-telegram");
    let data = dir.join("data");
    std::fs::create_dir_all(&data).unwrap();
    let alias = dir.join("alias");
    std::os::unix::fs::symlink(&data, &alias).unwrap();
    let with_data = |tok: &Path| {
        set("data_dir", Some(&format!("\"{}\"", data.display())))
            + &format!("\n[telegram]\ntoken_file = \"{}\"\n", tok.display())
    };
    refused(
        &with_data(&alias.join("t.json")),
        "[telegram] token_file must be outside the data directory",
    );
    assert!(ok(&with_data(&dir.join("t.json"))).telegram.is_some());
    let _ = std::fs::remove_dir_all(&dir);
}

/// A whole `[kill]` section (mainnet needs one since U19b3a) whose marker
/// directory is a fresh test directory named `name`.
fn kill_section(name: &str) -> String {
    format!("\n[kill]\n{}", kill_required(&common::test_dir(name)))
}

/// The two fields a `[kill]` section must name (U19b3a): a marker in an
/// existing directory outside the data directory, and the OMS's unit.
fn kill_required(dir: &Path) -> String {
    format!(
        "marker = \"{}\"\noms_unit = \"watcher-test-no-such-unit.service\"\n",
        dir.join("kill.marker").display()
    )
}

#[test]
fn kill_loop_fields_are_validated() {
    let dir = common::test_dir("config-kill-loop");
    let req = kill_required(&dir);
    // Without the section: no kill (the key rules of step 2 are not read).
    assert_eq!(ok(MIN).kill, None);
    // A section with only the required fields takes the defaults (table
    // 7-8 C4, C5).
    let k = ok(&format!("{MIN}\n[kill]\n{req}")).kill.unwrap();
    assert_eq!(
        k,
        Kill {
            round_ms: 1000,
            watch_ms: 30_000,
            watch_every_ms: 5000,
            notice_ms: 60_000,
            marker: dir.join("kill.marker"),
            oms_unit: "watcher-test-no-such-unit.service".into(),
            systemd: Systemd::System,
            stop_wait_ms: 10_000,
        }
    );
    let kill = |body: &str| format!("{MIN}\n[kill]\n{req}{body}\n");
    // Every bound itself is accepted, one step beyond it refused.
    for (field, lo, hi) in [
        ("round_ms", 1000, 10_000),
        ("watch_ms", 10_000, 300_000),
        ("notice_ms", 60_000, 600_000),
    ] {
        for v in [lo, hi] {
            ok(&kill(&format!("{field} = {v}\nwatch_every_ms = 1000")));
        }
        for v in [lo - 1, hi + 1] {
            refused(
                &kill(&format!("{field} = {v}\nwatch_every_ms = 1000")),
                &format!("[kill] {field} must be between {lo} and {hi}"),
            );
        }
    }
    // watch_every_ms: 1,000 up to watch_ms.
    ok(&kill("watch_every_ms = 1000"));
    ok(&kill("watch_ms = 20000\nwatch_every_ms = 20000"));
    refused(
        &kill("watch_every_ms = 999"),
        "[kill] watch_every_ms must be between 1000 and 30000",
    );
    refused(
        &kill("watch_ms = 20000\nwatch_every_ms = 20001"),
        "[kill] watch_every_ms must be between 1000 and 20000",
    );
    let k = ok(&kill("round_ms = 2000\nwatch_ms = 60000\nwatch_every_ms = 10000\nnotice_ms = 120000"))
        .kill
        .unwrap();
    assert_eq!(
        (k.round_ms, k.watch_ms, k.watch_every_ms, k.notice_ms),
        (2000, 60_000, 10_000, 120_000)
    );
    // Unknown keys and wrong types are refused.
    refused(&kill("bogus = 1"), "bogus");
    refused(&kill("round_ms = \"1000\""), "round_ms");
    refused(&kill("round_ms = -1"), "round_ms");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn kill_and_w5_are_validated_and_kill_is_required_on_mainnet() {
    let dir = common::test_dir("config-kill-w5");
    let req = kill_required(&dir);
    let kill = |body: &str| format!("{MIN}\n[kill]\n{body}\n");

    // `marker` and `oms_unit` are required in the section (U19b3a).
    refused(
        &kill("oms_unit = \"hl-oms.service\""),
        "missing field `marker`",
    );
    refused(
        &kill(&format!("marker = \"{}\"", dir.join("m").display())),
        "missing field `oms_unit`",
    );

    // The marker: the OMS's own rule (absolute, no `..`, outside the data
    // directory by where it points, its directory exists at load).
    let unit = "oms_unit = \"hl-oms.service\"";
    let marker = |p: &str| kill(&format!("marker = \"{p}\"\n{unit}"));
    refused(&marker("kill.marker"), "[kill] marker must be an absolute path");
    refused(
        &marker(&format!("{}/../x/kill.marker", dir.display())),
        "[kill] marker must not hold `..`",
    );
    for inside in ["/srv/oms/data/kill.marker", "/srv/oms/./data/k"] {
        refused(&marker(inside), "[kill] marker must be outside the data directory");
    }
    let missing = dir.join("no-such-dir").join("kill.marker");
    refused(
        &marker(&missing.display().to_string()),
        &format!("[kill] marker: the directory of {} does not exist", missing.display()),
    );
    let data = dir.join("data");
    std::fs::create_dir_all(&data).unwrap();
    let alias = dir.join("alias");
    std::os::unix::fs::symlink(&data, &alias).unwrap();
    let with_data = |m: &Path| {
        set("data_dir", Some(&format!("\"{}\"", data.display())))
            + &format!("\n[kill]\nmarker = \"{}\"\n{unit}\n", m.display())
    };
    refused(
        &with_data(&alias.join("kill.marker")),
        "[kill] marker must be outside the data directory",
    );
    let k = ok(&with_data(&dir.join("kill.marker"))).kill.unwrap();
    assert_eq!(k.marker, dir.join("kill.marker"));

    // The unit: a systemd unit name, no glob, no space, no option.
    let m = format!("marker = \"{}\"", dir.join("kill.marker").display());
    let named = |u: &str| kill(&format!("{m}\noms_unit = {u:?}"));
    const UNIT: &str = "[kill] oms_unit must be a systemd unit name: 1 to 255 of A-Z a-z 0-9 : _ . @ -, ending in .service, not starting with -";
    for good in [
        "hl-oms.service",
        "hl-oms@bybit.service",
        "a:b_c.d-e.service",
        &format!("{}.service", "a".repeat(247)),
    ] {
        assert_eq!(ok(&named(good)).kill.unwrap().oms_unit, good);
    }
    for bad in [
        "",
        ".service",
        "hl-oms",
        "hl-oms.target",
        "hl oms.service",
        "hl-oms*.service",
        "hl-oms?.service",
        "hl-[o]ms.service",
        "-hl-oms.service",
        "--user.service",
        "hl-oms.service ",
        "hl/oms.service",
        "hl-öms.service",
        "hl-oms.service\n",
        &format!("{}.service", "a".repeat(248)),
    ] {
        refused(&named(bad), UNIT);
    }

    // `systemd`: "system" (the default) or "user"; `stop_wait_ms` in range.
    assert_eq!(ok(&kill(&req)).kill.unwrap().systemd, Systemd::System);
    let user = ok(&kill(&format!("{req}systemd = \"user\""))).kill.unwrap();
    assert_eq!(user.systemd, Systemd::User);
    refused(&kill(&format!("{req}systemd = \"session\"")), "systemd");
    for v in [1000, 120_000] {
        let k = ok(&kill(&format!("{req}stop_wait_ms = {v}"))).kill.unwrap();
        assert_eq!(k.stop_wait_ms, v);
    }
    for v in [999, 120_001] {
        refused(
            &kill(&format!("{req}stop_wait_ms = {v}")),
            "[kill] stop_wait_ms must be between 1000 and 120000",
        );
    }
    refused(&kill(&format!("{req}unit = \"x.service\"")), "unknown field `unit`");

    // `[w5]`: absent = the design's 1 s probe and 10 s silence.
    assert_eq!(
        ok(MIN).w5,
        W5 {
            probe_ms: 1000,
            silence_ms: 10_000,
        }
    );
    let w5 = |body: &str| format!("{MIN}\n[w5]\n{body}\n");
    assert_eq!(ok(&w5("")).w5, ok(MIN).w5);
    for v in [500, 5000] {
        assert_eq!(ok(&w5(&format!("probe_ms = {v}"))).w5.probe_ms, v);
    }
    for v in [499, 5001] {
        refused(
            &w5(&format!("probe_ms = {v}")),
            "[w5] probe_ms must be between 500 and 5000",
        );
    }
    for v in [10_000, 60_000] {
        assert_eq!(ok(&w5(&format!("silence_ms = {v}"))).w5.silence_ms, v);
    }
    refused(
        &w5("silence_ms = 60001"),
        "[w5] silence_ms must be between 3000 and 60000",
    );
    // The bound: silence_ms ≥ 3 × probe_ms + 3 × request_timeout_ms +
    // 1,000, so neither one lost probe and a slow answer (review 1 S6)
    // nor a stall of the watcher shorter than a gap (U19b3a review 1 S1)
    // can fire W5. It holds where W5 runs, with [kill].
    let http = |rt: u64, body: &str| {
        format!("{MIN}\n[http]\nrequest_timeout_ms = {rt}\n\n[w5]\n{body}\n\n[kill]\n{req}")
    };
    let bound = |n: u64| {
        format!("[w5] silence_ms must be at least 3 × probe_ms + 3 × request_timeout_ms + 1000 = {n}: one lost probe or a stall of the watcher shorter than a gap must not fire W5")
    };
    let w = ok(&http(1500, "probe_ms = 500\nsilence_ms = 7000")).w5;
    assert_eq!((w.probe_ms, w.silence_ms), (500, 7000));
    refused(&http(1500, "probe_ms = 500\nsilence_ms = 6999"), &bound(7000));
    // The lost-probe bound alone (probe_ms + 2 × request_timeout_ms +
    // 1,000 = 4,500) is no longer enough.
    refused(&http(1500, "probe_ms = 500\nsilence_ms = 4500"), &bound(7000));
    // The defaults sit at the bound: 3 × 1,000 + 3 × 2,000 + 1,000.
    ok(&http(2000, ""));
    refused(&http(2000, "silence_ms = 9999"), &bound(10_000));
    refused(&http(2000, "probe_ms = 1001"), &bound(10_003));
    // The review's case: probe_ms 5,000 with the default silence_ms
    // (gap threshold 12,000 ms > silence_ms) is refused.
    refused(&http(2000, "probe_ms = 5000"), &bound(22_000));
    refused(&http(2000, "probe_ms = 5000\nsilence_ms = 21999"), &bound(22_000));
    ok(&http(2000, "probe_ms = 5000\nsilence_ms = 22000"));
    refused(&http(10_000, ""), &bound(34_000));
    // Without [kill] W5 is off and the bound is not read: the longest
    // request_timeout_ms keeps the default [w5].
    let off = format!("{MIN}\n[http]\nrequest_timeout_ms = 10000\n");
    assert_eq!(ok(&off).w5.silence_ms, 10_000);
    refused(
        &http(200, "probe_ms = 500\nsilence_ms = 2999"),
        "[w5] silence_ms must be between 3000 and 60000",
    );
    refused(&w5("bogus = 1"), "unknown field `bogus`");
    refused(&w5("probe_ms = \"1000\""), "probe_ms");

    // Mainnet refuses a config without [kill] (owner question 9), with
    // the spec's line, and takes it with the section.
    let mainnet = set("network", Some("\"mainnet\""))
        .replace("name = ", "mainnet_confirmed = true\nname = ")
        + TELEGRAM;
    refused(
        &mainnet,
        "mainnet requires [kill]: without it the watcher cannot stop the OMS or cancel its orders",
    );
    let m = ok(&format!("{mainnet}\n[kill]\n{req}"));
    assert_eq!(m.network, Network::Mainnet);
    assert!(m.kill.is_some());
    // Testnet takes a config without it (W5 is then off).
    assert_eq!(ok(MIN).kill, None);
    let _ = std::fs::remove_dir_all(&dir);
}
