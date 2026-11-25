//! The test build's endpoints (`Endpoints::for_test_build`, the feature
//! `test-endpoints`): every endpoint from `WATCHER_TEST_ENDPOINTS` alone,
//! each REST and Telegram URL `http://127.0.0.1:<port>`, `systemctl` an
//! absolute executable file; refused on mainnet and without the variable.
//! Nothing here starts a binary or sends a request.

#![allow(clippy::unwrap_used)]

mod common;

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use watcher::alert::Route;
use watcher::config::Config;
use watcher::runtime::{Endpoints, TEST_ENDPOINTS};

fn config(network: &str, dir: &Path) -> Config {
    let mut text = format!(
        r#"name = "joint"
network = "{network}"
credentials_file = "/etc/watcher/creds.toml"
sub_account_uid = 4242
symbols = ["BTCUSDT"]
ip_echo_url = "https://ip.example/"
data_dir = "/srv/oms/data"
status_url = "http://127.0.0.1:8188/api/status"
"#
    );
    if network == "mainnet" {
        text = format!(
            "mainnet_confirmed = true\n{text}\n[telegram]\ntoken_file = \"/etc/watcher/telegram.json\"\n\n[kill]\nmarker = \"{}\"\noms_unit = \"watcher-test-no-such-unit.service\"\n",
            dir.join("kill.marker").display()
        );
    }
    Config::from_toml(&text).unwrap()
}

/// An executable script in `dir` (never run).
fn script(dir: &Path, name: &str, mode: u32) -> PathBuf {
    let p = dir.join(name);
    std::fs::write(&p, "#!/bin/sh\nexit 1\n").unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(mode)).unwrap();
    p
}

fn good(systemctl: &Path) -> Value {
    json!({
        "rest": "http://127.0.0.1:41001",
        "ip_echo_url": "http://127.0.0.1:41002/ip",
        "telegram": "http://127.0.0.1:41003",
        "systemctl": systemctl.display().to_string(),
    })
}

/// Refused with one line that names the variable.
fn refused(wiring: &Value, cfg: &Config) -> String {
    match Endpoints::for_test_build(Some(&wiring.to_string()), cfg) {
        Ok(ep) => panic!("{wiring} taken: {ep:?}"),
        Err(e) => {
            assert!(e.starts_with(TEST_ENDPOINTS), "{e}");
            assert!(!e.contains('\n'), "not one line: {e:?}");
            e
        }
    }
}

#[test]
fn a_good_wiring_gives_every_endpoint_from_the_wiring() {
    let dir = common::test_dir("te-good");
    let sc = script(&dir, "systemctl", 0o700);
    let cfg = config("testnet", &dir);
    let w = good(&sc);
    let ep = Endpoints::for_test_build(Some(&w.to_string()), &cfg).unwrap();
    assert_eq!(ep.rest_base, "http://127.0.0.1:41001");
    assert_eq!(ep.ip_echo_url, "http://127.0.0.1:41002/ip");
    assert_eq!(ep.telegram, vec![Route::direct("http://127.0.0.1:41003")]);
    assert_eq!(ep.systemctl, sc);
    // Nothing comes from the config or the production values.
    let real = Endpoints::of(&cfg);
    assert_ne!(ep.rest_base, real.rest_base);
    assert_ne!(ep.ip_echo_url, real.ip_echo_url);
    assert!(ep.telegram.iter().all(|r| !real.telegram.contains(r)));
    assert_ne!(ep.systemctl, real.systemctl);
    assert_eq!(TEST_ENDPOINTS, "WATCHER_TEST_ENDPOINTS");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_wiring_off_127_0_0_1_is_refused() {
    let dir = common::test_dir("te-host");
    let sc = script(&dir, "systemctl", 0o700);
    let cfg = config("testnet", &dir);
    for key in ["rest", "ip_echo_url", "telegram"] {
        for bad in [
            "https://api-testnet.bybit.com",
            "https://api.telegram.org/",
            "https://127.0.0.1:41001",
            "http://localhost:41001",
            "http://127.0.0.2:41001",
            "http://[::1]:41001",
            "http://0.0.0.0:41001",
            "http://127.0.0.1@api.bybit.com/",
            "http://127.0.0.1.example:41001",
            "http://127.0.0.1",
            "ws://127.0.0.1:41001",
            "not a url",
            "",
        ] {
            let mut w = good(&sc);
            w[key] = json!(bad);
            let e = refused(&w, &cfg);
            assert!(e.contains(key), "{e}");
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_test_build_refuses_mainnet() {
    let dir = common::test_dir("te-mainnet");
    let sc = script(&dir, "systemctl", 0o700);
    let cfg = config("mainnet", &dir);
    let e = Endpoints::for_test_build(Some(&good(&sc).to_string()), &cfg).unwrap_err();
    assert_eq!(e, "a test build never runs on mainnet");
    let e = Endpoints::for_test_build(None, &cfg).unwrap_err();
    assert_eq!(e, "a test build never runs on mainnet");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_test_build_without_wiring_is_refused() {
    let dir = common::test_dir("te-unset");
    let cfg = config("testnet", &dir);
    let e = Endpoints::for_test_build(None, &cfg).unwrap_err();
    assert_eq!(
        e,
        "this build is for tests: WATCHER_TEST_ENDPOINTS is required"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn unknown_and_missing_fields_are_refused() {
    let dir = common::test_dir("te-fields");
    let sc = script(&dir, "systemctl", 0o700);
    let cfg = config("testnet", &dir);
    for key in ["rest", "ip_echo_url", "telegram", "systemctl"] {
        let mut w = good(&sc);
        w.as_object_mut().unwrap().remove(key);
        refused(&w, &cfg);
    }
    let mut w = good(&sc);
    w["private_ws"] = json!("ws://127.0.0.1:41004");
    refused(&w, &cfg);
    for text in ["", "{", "[]", "\"http://127.0.0.1:1\""] {
        let e = Endpoints::for_test_build(Some(text), &cfg).unwrap_err();
        assert!(e.starts_with(TEST_ENDPOINTS) && !e.contains('\n'), "{e}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_systemctl_must_be_an_absolute_executable_file() {
    let dir = common::test_dir("te-systemctl");
    let cfg = config("testnet", &dir);
    let sc = script(&dir, "systemctl", 0o700);
    let plain = script(&dir, "plain", 0o600);
    std::fs::create_dir_all(dir.join("sub")).unwrap();
    for bad in [
        "systemctl".to_string(),
        "bin/systemctl".to_string(),
        dir.join("sub/../systemctl").display().to_string(),
        dir.join("missing").display().to_string(),
        dir.join("sub").display().to_string(),
        plain.display().to_string(),
        String::new(),
    ] {
        let mut w = good(&sc);
        w["systemctl"] = json!(bad);
        let e = refused(&w, &cfg);
        assert!(e.contains("systemctl"), "{e}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}
