//! The watcher's config file (C-WATCH part 4): every field of the spec's
//! table, its default and its validation; unknown keys refused; mainnet
//! only with `mainnet_confirmed = true`; every refusal is one line.

#![allow(clippy::unwrap_used)]

mod common;

use std::path::{Path, PathBuf};

use watcher::config::{Config, Kill, Network};

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
    let m = ok(&format!("{confirmed}{TELEGRAM}"));
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
    let m = ok(&format!("{mainnet}{TELEGRAM}"));
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

#[test]
fn kill_loop_fields_are_validated() {
    // Without the section: no kill (the key rules of step 2 are not read).
    assert_eq!(ok(MIN).kill, None);
    // An empty section takes the defaults (table 7-8 C4, C5).
    let k = ok(&format!("{MIN}\n[kill]\n")).kill.unwrap();
    assert_eq!(
        k,
        Kill {
            round_ms: 1000,
            watch_ms: 30_000,
            watch_every_ms: 5000,
            notice_ms: 60_000,
        }
    );
    let kill = |body: &str| format!("{MIN}\n[kill]\n{body}\n");
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
}
