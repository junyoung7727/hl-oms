//! Grafana rules (ADR-035 D9): G1 the listener, G2 alerting and anonymous
//! access off, G3 the admin password from a file, G4 the two datasources,
//! G5 the dashboard provider and the provisioning path, G6 the panels'
//! datasources and targets, G7 the dashboards' expressions against the
//! metric catalogue, G8 the two dashboards, G9 Grafana's data on the lake
//! disk.

use serde_json::Value;

use crate::catalogue::Catalogue;
use crate::ini::Ini;
use crate::tree::{Finding, Parsed, Tree, finding};

pub const INI: &str = "deploy/observability/grafana/grafana.ini";
const DATASOURCES: &str = "deploy/observability/grafana/provisioning/datasources/hl-oms.yaml";
const PROVIDERS: &str = "deploy/observability/grafana/provisioning/dashboards/hl-oms.yaml";
/// The install map's sources of the dashboards and of the provisioning
/// files (under `deploy/observability/`).
const DASHBOARD_SRC: &str = "grafana/dashboards/";
const PROVISIONING_SRC: &str = "grafana/provisioning/";
/// The unit that starts Grafana.
const UNIT: &str = "grafana.service";

/// G1: the address in the repository. RFC 5737 TEST-NET-1 is never
/// assigned to this PC, so Grafana refuses to start until the operator
/// writes this PC's Tailscale address (100.64.0.0/10): refuse rather than
/// listen on every interface, which an empty or missing `http_addr` does.
const PLACEHOLDER: &str = "192.0.2.1";

/// G2: sections whose `enabled` must be `false`: Grafana's own alerting
/// (alerts are vmalert's and Alertmanager's, design table 8-5) and access
/// without a login.
const OFF: &[&str] = &["unified_alerting", "auth.anonymous"];

/// G3: the admin password is read from a file under `/etc/` (mode 0640,
/// group `hlobs`, created by the operator); the repository holds the path
/// only.
const SECRET_FILE: (&str, &str) = ("$__file{/etc/", "}");

/// G4: the two datasources, exactly: name, uid, type, URL (the listeners
/// of `victoriametrics.service` and `victorialogs.service`, U8), default.
/// Both are read by Grafana's server (`access: proxy`): a browser on the
/// iPhone cannot reach 127.0.0.1 of this PC. Not editable in the UI. No
/// other key: credentials and plugin settings stay out of the file.
const SOURCES: &[(&str, &str, &str, &str, bool)] = &[
    (
        "VictoriaMetrics",
        "vm",
        "prometheus",
        "http://127.0.0.1:8428",
        true,
    ),
    (
        "VictoriaLogs",
        "vl",
        "victoriametrics-logs-datasource",
        "http://127.0.0.1:9428",
        false,
    ),
];
const SOURCE_KEYS: &[&str] = &[
    "name",
    "uid",
    "type",
    "access",
    "url",
    "isDefault",
    "editable",
];
/// G4, G5: the provisioning format and the top-level keys of each file.
const API_VERSION: u64 = 1;
const SOURCES_TOP: &[&str] = &["apiVersion", "datasources"];
const PROVIDERS_TOP: &[&str] = &["apiVersion", "providers"];
/// G5: the one dashboard provider: files, not deleted from the UI, not
/// saved from the UI (the repository is the only source).
const PROVIDER_KEYS: &[&str] = &[
    "name",
    "type",
    "disableDeletion",
    "allowUiUpdates",
    "options",
];

/// G9: where Grafana keeps its data and plugins (ADR-035 D1).
const DATA_DISK: &str = "/mnt/nvme/";
const DATA_KEYS: &[&str] = &["data", "plugins"];

/// A canonical dotted IPv4 address in 100.64.0.0/10, the range Tailscale
/// assigns from.
fn tailscale(addr: &str) -> bool {
    let octets: Option<Vec<u8>> = addr
        .split('.')
        .map(|o| o.parse::<u8>().ok().filter(|n| n.to_string() == o))
        .collect();
    matches!(octets.as_deref(), Some([100, b, _, _]) if (64..128).contains(b))
}

/// An absolute path under `DATA_DISK` with no empty, `.` or `..` segment.
fn on_data_disk(path: &str) -> bool {
    path.strip_prefix(DATA_DISK).is_some_and(|rest| {
        rest.split('/')
            .all(|s| !s.is_empty() && s != "." && s != "..")
    })
}

pub fn check(_t: &Tree, p: &Parsed, _cat: &Catalogue, out: &mut Vec<Finding>) {
    let Some(ini) = p.ini.get(INI) else {
        finding(out, INI, "G1", "missing");
        return;
    };
    g1_listener(p, ini, out);
    g2_off(ini, out);
    g3_admin_password(ini, out);
    g4_datasources(p, out);
    g5_provider(p, ini, out);
    g9_data(ini, out);
}

fn g1_listener(p: &Parsed, ini: &Ini, out: &mut Vec<Finding>) {
    let addr = ini.values("server", "http_addr");
    let ok = matches!(addr[..], [a] if a == PLACEHOLDER || tailscale(a));
    if !ok {
        finding(
            out,
            INI,
            "G1",
            format!(
                "[server] http_addr must be given once, {PLACEHOLDER} or this PC's Tailscale address in 100.64.0.0/10 (found {addr:?})"
            ),
        );
    }
    // Grafana reads `GF_<SECTION>_<KEY>` variables and `cfg:` arguments
    // over its file: the check reads only the file.
    for u in p.units.iter().filter(|u| u.name == UNIT) {
        for (s, k, v) in &u.ini.entries {
            let env = k.ends_with("Environment") && v.contains("GF_");
            let arg = k.starts_with("Exec") && v.contains("cfg:");
            if s == "Service" && (env || arg) {
                finding(
                    out,
                    &u.path,
                    "G1",
                    format!(
                        "{k}: no GF_* variable and no cfg: argument (settings only in grafana.ini)"
                    ),
                );
            }
        }
    }
}

fn g2_off(ini: &Ini, out: &mut Vec<Finding>) {
    for section in OFF {
        let v = ini.values(section, "enabled");
        if v != ["false"] {
            finding(
                out,
                INI,
                "G2",
                format!("[{section}] enabled must be false, given once (found {v:?})"),
            );
        }
    }
}

fn g3_admin_password(ini: &Ini, out: &mut Vec<Finding>) {
    let v = ini.values("security", "admin_password");
    let ok = matches!(v[..], [value] if value
        .strip_prefix(SECRET_FILE.0)
        .and_then(|rest| rest.strip_suffix(SECRET_FILE.1))
        .is_some_and(|path| !path.contains('}') && !path.contains(char::is_whitespace)
            && path.split('/').all(|s| !s.is_empty() && s != "." && s != "..")));
    if !ok {
        // Not the value: it may be the password itself.
        finding(
            out,
            INI,
            "G3",
            "[security] admin_password must be given once as $__file{/etc/<path>} (the value is missing or is no such reference)",
        );
    }
}

fn keys_within(v: &Value, allowed: &[&str]) -> bool {
    v.as_object()
        .is_some_and(|m| m.keys().all(|k| allowed.contains(&k.as_str())))
}

fn g4_datasources(p: &Parsed, out: &mut Vec<Finding>) {
    let Some(doc) = p.json.get(DATASOURCES) else {
        finding(out, DATASOURCES, "G4", "missing");
        return;
    };
    if doc.get("apiVersion").and_then(Value::as_u64) != Some(API_VERSION)
        || !keys_within(doc, SOURCES_TOP)
    {
        finding(
            out,
            DATASOURCES,
            "G4",
            format!("the file must hold apiVersion {API_VERSION} and datasources only"),
        );
    }
    let list: Vec<&Value> = doc
        .get("datasources")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .collect();
    if list.len() != SOURCES.len() {
        finding(
            out,
            DATASOURCES,
            "G4",
            format!(
                "exactly {} datasources (found {})",
                SOURCES.len(),
                list.len()
            ),
        );
    }
    for &(name, uid, kind, url, default) in SOURCES {
        let found: Vec<&&Value> = list
            .iter()
            .filter(|d| d.get("uid").and_then(Value::as_str) == Some(uid))
            .collect();
        let ok = match found[..] {
            [d] => {
                let s = |k: &str| d.get(k).and_then(Value::as_str);
                s("name") == Some(name)
                    && s("type") == Some(kind)
                    && s("url") == Some(url)
                    && s("access") == Some("proxy")
                    && d.get("isDefault").map_or(Some(false), Value::as_bool) == Some(default)
                    && d.get("editable").and_then(Value::as_bool) == Some(false)
                    && keys_within(d, SOURCE_KEYS)
            }
            _ => false,
        };
        if !ok {
            finding(
                out,
                DATASOURCES,
                "G4",
                format!(
                    "datasource uid {uid} once: name {name}, type {kind}, url {url}, access proxy, isDefault {default}, editable false, keys {SOURCE_KEYS:?} only"
                ),
            );
        }
    }
}

fn g5_provider(p: &Parsed, ini: &Ini, out: &mut Vec<Finding>) {
    let entries = crate::units::install_entries(p, &mut Vec::new());
    let parent = |dst: &str| dst.rsplit_once('/').map(|(d, _)| d.to_string());
    // Every dashboard is installed into one directory, the provider's.
    let dirs: std::collections::BTreeSet<Option<String>> = entries
        .iter()
        .filter(|(src, _, _)| src.starts_with(DASHBOARD_SRC))
        .map(|(_, dst, _)| parent(dst))
        .collect();
    let Some(doc) = p.json.get(PROVIDERS) else {
        finding(out, PROVIDERS, "G5", "missing");
        return;
    };
    let providers: Vec<&Value> = doc
        .get("providers")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .collect();
    let ok = doc.get("apiVersion").and_then(Value::as_u64) == Some(API_VERSION)
        && keys_within(doc, PROVIDERS_TOP)
        && match providers[..] {
            [pr] => {
                let path = pr
                    .get("options")
                    .filter(|o| keys_within(o, &["path"]))
                    .and_then(|o| o.get("path"))
                    .and_then(Value::as_str);
                pr.get("type").and_then(Value::as_str) == Some("file")
                    && pr.get("disableDeletion").and_then(Value::as_bool) == Some(true)
                    && pr.get("allowUiUpdates").and_then(Value::as_bool) == Some(false)
                    && keys_within(pr, PROVIDER_KEYS)
                    && path.is_some()
                    && dirs.len() == 1
                    && dirs.iter().all(|d| d.as_deref() == path)
            }
            _ => false,
        };
    if !ok {
        finding(
            out,
            PROVIDERS,
            "G5",
            format!(
                "one file provider: disableDeletion true, allowUiUpdates false, options.path = the directory every dashboard installs to (found {dirs:?})"
            ),
        );
    }
    // Grafana reads its provisioning files below `[paths] provisioning`.
    let prov = ini.values("paths", "provisioning");
    let ok = match prov[..] {
        [dir] => entries
            .iter()
            .filter(|(src, _, _)| src.starts_with(PROVISIONING_SRC))
            .all(|(src, dst, _)| *dst == format!("{dir}/{}", &src[PROVISIONING_SRC.len()..])),
        _ => false,
    };
    if !ok {
        finding(
            out,
            INI,
            "G5",
            format!(
                "[paths] provisioning must be given once and be where the provisioning files install (found {prov:?})"
            ),
        );
    }
}

fn g9_data(ini: &Ini, out: &mut Vec<Finding>) {
    for key in DATA_KEYS {
        let v = ini.values("paths", key);
        if !matches!(v[..], [path] if on_data_disk(path)) {
            finding(
                out,
                INI,
                "G9",
                format!("[paths] {key} must be given once, under {DATA_DISK} (found {v:?})"),
            );
        }
    }
}

// Tests.

use crate::assert_rules;

fn plant(path: &str, from: &str, to: &str, rules: &[&str]) {
    let mut t = Tree::real();
    t.replace(path, from, to);
    assert_rules(&crate::check(&t), rules);
}

const UNIT_PATH: &str = "deploy/observability/systemd/grafana.service";

#[test]
fn grafana_listens_on_the_tailscale_placeholder_only() {
    let line = "http_addr = 192.0.2.1\n";
    // All interfaces in every spelling, loopback, a name, IPv6, and
    // addresses just outside 100.64.0.0/10 or not written canonically.
    for bad in [
        "0.0.0.0",
        "",
        "::",
        "[::]",
        "*",
        "127.0.0.1",
        "localhost",
        "100.128.0.1",
        "100.63.255.255",
        "100.64.0.256",
        "100.064.0.1",
        "100.64.0",
        "100.64.0.1.2",
        "fd7a:115c:a1e0::1",
    ] {
        plant(INI, line, &format!("http_addr = {bad}\n"), &["G1"]);
    }
    // This PC's Tailscale address, written by the operator.
    for good in ["100.64.0.1", "100.127.255.254", "100.101.102.103"] {
        let mut t = Tree::real();
        t.replace(INI, line, &format!("http_addr = {good}\n"));
        let f = crate::check(&t);
        assert!(f.is_empty(), "{good}: {f:?}");
    }
    // Missing (Grafana's default is every interface), or given twice.
    plant(INI, line, "", &["G1"]);
    plant(
        INI,
        line,
        "http_addr = 192.0.2.1\nhttp_addr = 0.0.0.0\n",
        &["G1"],
    );
    // An override the check does not read: the environment of the unit or
    // a `cfg:` argument on its command line.
    let wd = "WorkingDirectory=/opt/grafana\n";
    plant(
        UNIT_PATH,
        wd,
        "WorkingDirectory=/opt/grafana\nEnvironment=GF_SERVER_HTTP_ADDR=0.0.0.0\n",
        &["G1"],
    );
    plant(
        UNIT_PATH,
        wd,
        "WorkingDirectory=/opt/grafana\nEnvironment=\"A=1\" \"GF_AUTH_ANONYMOUS_ENABLED=true\"\n",
        &["G1"],
    );
    plant(
        UNIT_PATH,
        wd,
        "WorkingDirectory=/opt/grafana\nPassEnvironment=GF_SERVER_HTTP_ADDR\n",
        &["G1"],
    );
    plant(
        UNIT_PATH,
        "--config /etc/hl-oms-obs/grafana/grafana.ini\n",
        "--config /etc/hl-oms-obs/grafana/grafana.ini cfg:server.http_addr=0.0.0.0\n",
        &["G1"],
    );
}

#[test]
fn grafana_data_stays_on_the_lake_disk() {
    let data = "data = /mnt/nvme/observability/grafana\n";
    let plugins = "plugins = /mnt/nvme/observability/grafana/plugins\n";
    plant(INI, data, "data = /var/lib/grafana\n", &["G9"]);
    plant(INI, data, "data = /mnt/nvme/../var/lib/grafana\n", &["G9"]);
    plant(INI, data, "data = /mnt/nvmex/grafana\n", &["G9"]);
    plant(INI, data, "", &["G9"]);
    plant(INI, plugins, "plugins = /tmp/plugins\n", &["G9"]);
    plant(
        INI,
        data,
        "data = /mnt/nvme/observability/grafana\ndata = /var/lib/grafana\n",
        &["G9"],
    );
}

#[test]
fn grafana_alerting_and_anonymous_access_are_off() {
    let alerting = "[unified_alerting]\nenabled = false\n";
    let anonymous = "[auth.anonymous]\nenabled = false\n";
    for (from, to) in [
        (alerting, "[unified_alerting]\nenabled = true\n"),
        (alerting, "[unified_alerting]\n"),
        (alerting, ""),
        (
            alerting,
            "[unified_alerting]\nenabled = false\n[unified_alerting]\nenabled = true\n",
        ),
        (anonymous, "[auth.anonymous]\nenabled = true\n"),
        (anonymous, "[auth.anonymous]\nenabled = False\n"),
        (anonymous, ""),
    ] {
        plant(INI, from, to, &["G2"]);
    }
}

#[test]
fn the_admin_password_comes_from_a_file() {
    let line = "admin_password = $__file{/etc/hl-oms/grafana-admin-password}\n";
    for bad in [
        "admin_password = admin\n",
        "admin_password = $__env{GRAFANA_ADMIN}\n",
        "admin_password = $__file{/tmp/grafana-admin-password}\n",
        "admin_password = $__file{/etc/}\n",
        "admin_password = $__file{/etc/../tmp/x}\n",
        "admin_password = $__file{/etc/hl-oms/x} trailing\n",
        "",
    ] {
        plant(INI, line, bad, &["G3"]);
    }
}

use serde_json::json;

/// The real tree with one JSON file changed through its value.
fn edit(t: &mut Tree, path: &str, f: impl FnOnce(&mut Value)) {
    let mut v: Value = serde_json::from_str(&t.files[path]).unwrap();
    f(&mut v);
    t.set(path, &serde_json::to_string_pretty(&v).unwrap());
}

fn plant_json(path: &str, f: impl FnOnce(&mut Value), rules: &[&str]) {
    let mut t = Tree::real();
    edit(&mut t, path, f);
    assert_rules(&crate::check(&t), rules);
}

fn source_mut<'a>(v: &'a mut Value, uid: &str) -> &'a mut Value {
    v["datasources"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|d| d["uid"] == uid)
        .unwrap_or_else(|| panic!("no datasource {uid}"))
}

#[test]
fn the_datasources_are_vm_and_vl_on_loopback() {
    let cases: Vec<Box<dyn FnOnce(&mut Value)>> = vec![
        // Off loopback, another port, another uid or type.
        Box::new(|v| source_mut(v, "vm")["url"] = json!("http://0.0.0.0:8428")),
        Box::new(|v| source_mut(v, "vl")["url"] = json!("http://127.0.0.1:9429")),
        Box::new(|v| source_mut(v, "vm")["uid"] = json!("victoriametrics")),
        Box::new(|v| source_mut(v, "vl")["type"] = json!("loki")),
        // Editable in the UI, or editable by default.
        Box::new(|v| source_mut(v, "vm")["editable"] = json!(true)),
        Box::new(|v| {
            source_mut(v, "vl")
                .as_object_mut()
                .unwrap()
                .remove("editable");
        }),
        // The browser (the iPhone) would query 127.0.0.1 itself.
        Box::new(|v| source_mut(v, "vm")["access"] = json!("direct")),
        // Credentials or extra settings in the file.
        Box::new(|v| source_mut(v, "vm")["secureJsonData"] = json!({"password": "x"})),
        Box::new(|v| source_mut(v, "vm")["basicAuth"] = json!(true)),
        Box::new(|v| source_mut(v, "vl")["jsonData"] = json!({"maxLines": 1000})),
        // The default is VictoriaMetrics.
        Box::new(|v| source_mut(v, "vl")["isDefault"] = json!(true)),
        Box::new(|v| source_mut(v, "vm")["isDefault"] = json!(false)),
        // A third datasource, a missing one, another format, another key.
        Box::new(|v| {
            let mut d = source_mut(v, "vm").clone();
            d["uid"] = json!("vm2");
            d["isDefault"] = json!(false);
            v["datasources"].as_array_mut().unwrap().push(d);
        }),
        Box::new(|v| {
            v["datasources"].as_array_mut().unwrap().pop();
        }),
        Box::new(|v| v["apiVersion"] = json!(2)),
        Box::new(|v| v["deleteDatasources"] = json!([{"name": "x", "orgId": 1}])),
    ];
    for f in cases {
        plant_json(DATASOURCES, f, &["G4"]);
    }
    // The order of the two is free.
    let mut t = Tree::real();
    edit(&mut t, DATASOURCES, |v| {
        v["datasources"].as_array_mut().unwrap().reverse();
    });
    assert_rules(&crate::check(&t), &[]);
}

#[test]
fn the_dashboard_provider_reads_the_installed_dashboards() {
    let provider = |v: &mut Value| v["providers"][0].take();
    let cases: Vec<Box<dyn FnOnce(&mut Value)>> = vec![
        Box::new(|v| v["providers"][0]["options"]["path"] = json!("/var/lib/grafana/dashboards")),
        Box::new(|v| v["providers"][0]["allowUiUpdates"] = json!(true)),
        Box::new(|v| {
            v["providers"][0]
                .as_object_mut()
                .unwrap()
                .remove("allowUiUpdates");
        }),
        Box::new(|v| v["providers"][0]["disableDeletion"] = json!(false)),
        Box::new(|v| v["providers"][0]["type"] = json!("git")),
        Box::new(|v| v["providers"][0]["options"]["foldersFromFilesStructure"] = json!(true)),
        Box::new(move |v| {
            let p = provider(v);
            v["providers"] = json!([p.clone(), p]);
        }),
        Box::new(|v| v["providers"] = json!([])),
    ];
    for f in cases {
        plant_json(PROVIDERS, f, &["G5"]);
    }
    // Grafana would look for the provisioning files elsewhere.
    plant(
        INI,
        "provisioning = /etc/hl-oms-obs/grafana/provisioning\n",
        "provisioning = /etc/grafana/provisioning\n",
        &["G5"],
    );
    // A dashboard installed outside the provider's path.
    plant(
        "deploy/observability/install.toml",
        "dst = \"/etc/hl-oms-obs/grafana/dashboards/d3-system.json\"",
        "dst = \"/etc/hl-oms-obs/grafana/other/d3-system.json\"",
        &["U14", "G5"],
    );
}
