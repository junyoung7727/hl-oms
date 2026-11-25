//! Grafana rules (ADR-035 D9): G1 the listener, G2 alerting and anonymous
//! access off, G3 the admin password from a file, G4 the two datasources,
//! G5 the dashboard provider and the provisioning path, G6 the panels'
//! datasources and targets, G7 the dashboards' expressions against the
//! metric catalogue, G8 the two dashboards, G9 Grafana's data on the lake
//! disk.

use std::collections::BTreeSet;

use serde_json::Value;

use crate::catalogue::Catalogue;
use crate::expr::{self, FUNCTIONS};
use crate::ini::Ini;
use crate::tree::{Finding, Parsed, Tree, finding};

pub const INI: &str = "deploy/observability/grafana/grafana.ini";
const DATASOURCES: &str = "deploy/observability/grafana/provisioning/datasources/hl-oms.yaml";
const PROVIDERS: &str = "deploy/observability/grafana/provisioning/dashboards/hl-oms.yaml";
const DASHBOARDS: &str = "deploy/observability/grafana/dashboards/";
/// The install map's sources of the dashboards and of the provisioning
/// files (under `deploy/observability/`).
const DASHBOARD_SRC: &str = "grafana/dashboards/";
const PROVISIONING_SRC: &str = "grafana/provisioning/";
/// The unit that starts Grafana, and its one command (ADR-035 D2).
const UNIT: &str = "grafana.service";
const EXEC: &str = "/opt/grafana/bin/grafana server --homepath /opt/grafana --config /etc/hl-oms-obs/grafana/grafana.ini";

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

/// G6: the datasource uids a panel, a target or a variable may name (G4's).
const DATASOURCE_UIDS: &[&str] = &["vm", "vl"];
/// G6, G7: the VictoriaMetrics datasource. Its targets and the variables
/// are MetricsQL; VictoriaLogs targets are LogsQL and are not scanned.
const METRICS_UID: &str = "vm";
/// G6: D3's one text panel: the daily command for the failures that no
/// metric shows (an audit date's write error after the push, a failed
/// verify, a killed archiver run; no exporter of unit state exists).
const SYSTEM_UID: &str = "d3-system";
const FAILED_JOBS_TITLE: &str = "매일 확인: 실패한 일일 작업";
const FAILED_JOBS_COMMAND: &str = "systemctl --failed 'md-*' 'crypto-lake-audit*'";
/// G8: the dashboards of design §8.4 (uid, title: the one question each
/// answers).
const BOARDS: &[(&str, &str)] = &[
    (
        "d2-market-data",
        "D2 시세 수집: 어느 거래소·종목이 비거나 늦나",
    ),
    ("d3-system", "D3 시스템: 서비스가 살아 있고 자원이 남았나"),
];
/// G8: panels that show the value an alert compares with its threshold,
/// so the panel and the alert cannot drift apart: (dashboard uid, alert,
/// threshold, the dashboard's variable filter put first in each selector).
/// D3: `LakeAuditFailed`, new failures in 26 h. D2: `MdTopicsStale`, the
/// share of stale order-book topics per market and channel.
const PINNED: &[(&str, &str, &str, &str)] = &[
    (SYSTEM_UID, "LakeAuditFailed", " > 0", ""),
    (
        "d2-market-data",
        "MdTopicsStale",
        " > 0.1",
        "venue=~\"$venue\", ",
    ),
];
/// G8: D3's memory panel divides each scraped component's memory by its
/// unit's `MemoryMax` (rule U6), one term per scrape job.
const MEMORY_METRIC: &str = "process_resident_memory_bytes";

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

pub fn check(_t: &Tree, p: &Parsed, cat: &Catalogue, out: &mut Vec<Finding>) {
    let boards: Vec<(&str, &Value)> = p
        .json
        .iter()
        .filter(|(k, _)| k.starts_with(DASHBOARDS))
        .map(|(k, v)| (k.as_str(), v))
        .collect();
    g6_panels(&boards, out);
    g7_expressions(&boards, cat, out);
    g8_boards(p, &boards, out);
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
    // over its file, and `grafana server` has flags that do the same
    // (`--configOverrides`) or open a listener (`--profile-addr`): the
    // check reads only the file, so the unit sets no such variable and has
    // exactly the one command.
    for u in p.units.iter().filter(|u| u.name == UNIT) {
        let mut exec = Vec::new();
        for (_, k, v) in u.ini.entries.iter().filter(|(s, _, _)| s == "Service") {
            if k.ends_with("Environment") && v.contains("GF_") {
                finding(
                    out,
                    &u.path,
                    "G1",
                    format!("{k}: no GF_* variable (settings only in grafana.ini)"),
                );
            }
            if k.starts_with("Exec") {
                exec.push((k.as_str(), v.as_str()));
            }
        }
        if exec != [("ExecStart", EXEC)] {
            finding(
                out,
                &u.path,
                "G1",
                format!(
                    "the one Exec line must be ExecStart={EXEC} (found {} Exec lines or another command)",
                    exec.len()
                ),
            );
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

fn ds_uid(v: &Value) -> Option<&str> {
    v.get("datasource")?.as_object()?.get("uid")?.as_str()
}

/// Every panel of a dashboard, the panels of its rows included (the JSON
/// is a finite tree: each panel is visited once).
fn panels(d: &Value) -> Vec<&Value> {
    let mut all = Vec::new();
    let mut todo: Vec<&Value> = d
        .get("panels")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .collect();
    while let Some(p) = todo.pop() {
        all.push(p);
        todo.extend(
            p.get("panels")
                .and_then(Value::as_array)
                .into_iter()
                .flatten(),
        );
    }
    all
}

fn targets(p: &Value) -> Vec<&Value> {
    p.get("targets")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .collect()
}

fn variables(d: &Value) -> Vec<&Value> {
    d.get("templating")
        .and_then(|t| t.get("list"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .collect()
}

/// A variable's query: the object form of Grafana 13 or the older string.
fn variable_query(v: &Value) -> Option<&str> {
    match v.get("query")? {
        Value::String(q) => Some(q),
        q => q.get("query")?.as_str(),
    }
}

fn panel_type(p: &Value) -> &str {
    p.get("type").and_then(Value::as_str).unwrap_or("")
}

fn g6_panels(boards: &[(&str, &Value)], out: &mut Vec<Finding>) {
    let mut uids = BTreeSet::new();
    for &(path, d) in boards {
        let uid = d.get("uid").and_then(Value::as_str).unwrap_or("");
        if uid.is_empty() || !uids.insert(uid) {
            finding(
                out,
                path,
                "G6",
                format!("uid {uid:?} must be set and unique"),
            );
        }
        let mut texts = Vec::new();
        for p in panels(d) {
            let title = p.get("title").and_then(Value::as_str).unwrap_or("");
            if title.is_empty() {
                finding(out, path, "G6", "every panel needs a title");
            }
            let kind = panel_type(p);
            if kind == "text" || kind == "row" {
                if kind == "text" {
                    texts.push(p);
                }
                if (kind == "text" && p.get("datasource").is_some()) || !targets(p).is_empty() {
                    finding(
                        out,
                        path,
                        "G6",
                        format!("{title}: a {kind} panel has no datasource and no target"),
                    );
                }
                continue;
            }
            let ds = ds_uid(p).filter(|u| DATASOURCE_UIDS.contains(u));
            if ds.is_none() {
                finding(
                    out,
                    path,
                    "G6",
                    format!(
                        "{title}: the panel's datasource uid must be one of {DATASOURCE_UIDS:?}"
                    ),
                );
            }
            let ts = targets(p);
            if ts.is_empty() {
                finding(out, path, "G6", format!("{title}: no target"));
            }
            for t in ts {
                let e = t.get("expr").and_then(Value::as_str).unwrap_or("");
                if ds.is_none() || ds_uid(t) != ds || e.trim().is_empty() {
                    finding(
                        out,
                        path,
                        "G6",
                        format!(
                            "{title}: every target needs the panel's datasource uid and a non-empty expr"
                        ),
                    );
                }
            }
        }
        for v in variables(d) {
            let name = v.get("name").and_then(Value::as_str).unwrap_or("");
            if v.get("type").and_then(Value::as_str) != Some("query")
                || ds_uid(v) != Some(METRICS_UID)
                || variable_query(v).is_none_or(|q| q.trim().is_empty())
            {
                finding(
                    out,
                    path,
                    "G6",
                    format!(
                        "variable {name}: a query variable on datasource {METRICS_UID} with a query"
                    ),
                );
            }
        }
        if uid == SYSTEM_UID {
            let ok = matches!(texts[..], [t] if t.get("title").and_then(Value::as_str)
                == Some(FAILED_JOBS_TITLE)
                && t.get("options")
                    .and_then(|o| o.get("content"))
                    .and_then(Value::as_str)
                    .is_some_and(|c| c.contains(FAILED_JOBS_COMMAND)));
            if !ok {
                finding(
                    out,
                    path,
                    "G6",
                    format!(
                        "{SYSTEM_UID} must have exactly one text panel, {FAILED_JOBS_TITLE:?}, holding {FAILED_JOBS_COMMAND}"
                    ),
                );
            }
        }
    }
}

/// `label_values(<selector>, <label>)`: the selector and the label.
fn label_values(q: &str) -> Option<(&str, &str)> {
    let inner = q.trim().strip_prefix("label_values(")?.strip_suffix(')')?;
    let (sel, label) = inner.rsplit_once(',')?;
    let label = label.trim();
    let ident = !label.is_empty() && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    ident.then_some((sel.trim(), label))
}

/// Rules R1, R3 and R4 of the rule files, for one dashboard expression.
fn scan_findings(s: &expr::Scan, cat: &Catalogue) -> Vec<String> {
    let mut f = Vec::new();
    for n in s.names.iter().filter(|n| !cat.has(n)) {
        f.push(format!("{n} is in no part of the metric catalogue"));
    }
    for l in s.labels.iter().filter(|l| !cat.knows_label(l)) {
        f.push(format!("label {l} is a label of no catalogue entry"));
    }
    for x in s
        .functions
        .iter()
        .filter(|x| !FUNCTIONS.contains(&x.as_str()))
    {
        f.push(format!("function {x} is not allowed"));
    }
    f.extend(s.errors.iter().cloned());
    f
}

fn g7_expressions(boards: &[(&str, &Value)], cat: &Catalogue, out: &mut Vec<Finding>) {
    for &(path, d) in boards {
        for p in panels(d) {
            let title = p.get("title").and_then(Value::as_str).unwrap_or("");
            for t in targets(p)
                .into_iter()
                .filter(|t| ds_uid(t) == Some(METRICS_UID))
            {
                let target_expr = t.get("expr").and_then(Value::as_str).unwrap_or("");
                let s = expr::scan(target_expr);
                for f in scan_findings(&s, cat) {
                    finding(out, path, "G7", format!("{title}: {f}"));
                }
            }
        }
        for v in variables(d) {
            let name = v.get("name").and_then(Value::as_str).unwrap_or("");
            let q = variable_query(v).unwrap_or("");
            let Some((sel, label)) = label_values(q) else {
                finding(
                    out,
                    path,
                    "G7",
                    format!("variable {name}: the query must be label_values(<metric>, <label>)"),
                );
                continue;
            };
            let s = expr::scan(sel);
            let mut f = scan_findings(&s, cat);
            match (&s.names[..], &s.functions[..]) {
                ([m], []) => {
                    let has = cat.metrics.get(m).is_some_and(|(_, l)| l.contains(label));
                    if cat.has(m) && !has {
                        f.push(format!("{m} has no label {label}"));
                    }
                }
                _ => f.push("label_values names one metric selector".to_string()),
            }
            for f in f {
                finding(out, path, "G7", format!("variable {name}: {f}"));
            }
        }
    }
}

fn g8_boards(p: &Parsed, boards: &[(&str, &Value)], out: &mut Vec<Finding>) {
    let uid = |d: &Value| d.get("uid").and_then(Value::as_str).map(str::to_string);
    for &(want_uid, want_title) in BOARDS {
        let found: Vec<&&Value> = boards
            .iter()
            .map(|(_, d)| d)
            .filter(|d| uid(d).as_deref() == Some(want_uid))
            .collect();
        let ok =
            matches!(found[..], [d] if d.get("title").and_then(Value::as_str) == Some(want_title));
        if !ok {
            finding(
                out,
                DASHBOARDS,
                "G8",
                format!("dashboard {want_uid} must exist once with title {want_title:?}"),
            );
        }
    }
    let board_exprs = |want_uid: &str| -> Option<(&str, Vec<String>)> {
        let &(path, d) = boards
            .iter()
            .find(|(_, d)| uid(d).as_deref() == Some(want_uid))?;
        let exprs = panels(d)
            .into_iter()
            .flat_map(targets)
            .filter_map(|t| t.get("expr").and_then(Value::as_str))
            .map(crate::rules::norm)
            .collect();
        Some((path, exprs))
    };
    // The panel follows the alert's current expression. A changed
    // `LakeAuditFailed` is also rule R7's finding; a missing alert is no
    // finding here (R7 pins `LakeAuditFailed`, nothing pins `MdTopicsStale`).
    let (rules, _) = crate::rules::collect(p);
    for &(board, name, threshold, filter) in PINNED {
        let Some((path, exprs)) = board_exprs(board) else {
            continue;
        };
        let alert = rules
            .iter()
            .find(|r| r.alert() == Some(name))
            .and_then(|r| r.expr())
            .map(crate::rules::norm);
        if let Some(a) = alert {
            let want = a
                .strip_suffix(threshold)
                .unwrap_or(&a)
                .replace('{', &format!("{{{filter}"));
            if !exprs.iter().any(|e| *e == want) {
                finding(
                    out,
                    path,
                    "G8",
                    format!("a panel must show {want} ({name} without{threshold})"),
                );
            }
        }
    }
    let Some((path, exprs)) = board_exprs(SYSTEM_UID) else {
        return;
    };
    let want: BTreeSet<String> = crate::rules::SCRAPE_JOBS
        .iter()
        .filter_map(|job| {
            let unit = format!("{job}.service");
            let cap = crate::units::MEMORY_MAX
                .iter()
                .find(|(n, _)| *n == unit)
                .and_then(|(_, c)| crate::units::size(c))?;
            Some(format!("{MEMORY_METRIC}{{job=\"{job}\"}} / {cap}"))
        })
        .collect();
    let mem: Vec<&String> = exprs.iter().filter(|e| e.contains(MEMORY_METRIC)).collect();
    let ok = matches!(mem[..], [e] if e.split(" or ").map(str::to_string).collect::<BTreeSet<_>>() == want);
    if !ok {
        finding(
            out,
            path,
            "G8",
            format!(
                "one panel target must be exactly the terms {want:?} (each scraped component's memory over its unit's MemoryMax)"
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
    // Other flags of `grafana server` that change the settings or open a
    // listener, and a second command.
    for extra in [
        " --configOverrides server.http_addr=0.0.0.0",
        " --profile --profile-addr 0.0.0.0",
    ] {
        plant(
            UNIT_PATH,
            "--config /etc/hl-oms-obs/grafana/grafana.ini\n",
            &format!("--config /etc/hl-oms-obs/grafana/grafana.ini{extra}\n"),
            &["G1"],
        );
    }
    plant(
        UNIT_PATH,
        wd,
        "WorkingDirectory=/opt/grafana\nExecStartPost=/opt/grafana/bin/grafana cli admin reset-admin-password x\n",
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

/// One planted change to a JSON file.
type Edit = Box<dyn FnOnce(&mut Value)>;

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
    let cases: Vec<Edit> = vec![
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
    let cases: Vec<Edit> = vec![
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

const D2: &str = "deploy/observability/grafana/dashboards/d2-market-data.json";
const D3: &str = "deploy/observability/grafana/dashboards/d3-system.json";

fn panel_mut<'a>(v: &'a mut Value, title: &str) -> &'a mut Value {
    v["panels"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|p| p["title"] == title)
        .unwrap_or_else(|| panic!("no panel {title}"))
}

fn var_mut<'a>(v: &'a mut Value, name: &str) -> &'a mut Value {
    v["templating"]["list"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|x| x["name"] == name)
        .unwrap_or_else(|| panic!("no variable {name}"))
}

const MSGS: &str = "초당 메시지 (시장별)";
const QUEUE: &str = "연결별 큐 바이트";
const TEXT_TITLE: &str = "매일 확인: 실패한 일일 작업";
const AUDIT_PANEL: &str = "26시간 안의 새 감사 실패";
const MEMORY_PANEL: &str = "메모리 / 단위 상한 (구성 요소별)";
const STALE_PANEL: &str = "멈춘 호가 주제 비율 (시장·채널별)";

#[test]
fn a_dashboard_panel_with_an_unknown_datasource_fails() {
    let cases: Vec<Edit> = vec![
        // A panel or a target on a datasource that is not provisioned, by
        // name, mixed, or left to Grafana's default.
        Box::new(|v| panel_mut(v, MSGS)["datasource"]["uid"] = json!("prom")),
        Box::new(|v| panel_mut(v, MSGS)["targets"][0]["datasource"]["uid"] = json!("x")),
        Box::new(|v| panel_mut(v, MSGS)["datasource"] = json!("VictoriaMetrics")),
        Box::new(|v| panel_mut(v, MSGS)["datasource"] = json!({"uid": "-- Mixed --"})),
        Box::new(|v| {
            panel_mut(v, MSGS)
                .as_object_mut()
                .unwrap()
                .remove("datasource");
        }),
        Box::new(|v| {
            panel_mut(v, MSGS)["targets"][0]
                .as_object_mut()
                .unwrap()
                .remove("datasource");
        }),
        // A target on the other datasource than its panel.
        Box::new(|v| panel_mut(v, MSGS)["targets"][0]["datasource"]["uid"] = json!("vl")),
        // A target without a query, a panel without a target or a title.
        Box::new(|v| panel_mut(v, MSGS)["targets"][0]["expr"] = json!("")),
        Box::new(|v| {
            panel_mut(v, MSGS)["targets"][0]
                .as_object_mut()
                .unwrap()
                .remove("expr");
        }),
        Box::new(|v| panel_mut(v, MSGS)["targets"] = json!([])),
        Box::new(|v| panel_mut(v, QUEUE)["title"] = json!("")),
        // A variable on another datasource or of another kind.
        Box::new(|v| var_mut(v, "venue")["datasource"]["uid"] = json!("x")),
        Box::new(|v| var_mut(v, "venue")["type"] = json!("custom")),
        // A panel nested in a row is checked as well.
        Box::new(|v| {
            let panels = v["panels"].as_array_mut().unwrap();
            let i = panels.iter().position(|p| p["title"] == QUEUE).unwrap();
            let mut child = panels.remove(i);
            child["datasource"]["uid"] = json!("x");
            panels.push(json!({
                "type": "row", "title": "행", "collapsed": true, "panels": [child]
            }));
        }),
    ];
    for f in cases {
        plant_json(D2, f, &["G6"]);
    }
    // The text panel holds no query.
    plant_json(
        D3,
        |v| panel_mut(v, TEXT_TITLE)["datasource"] = json!({"type": "prometheus", "uid": "vm"}),
        &["G6"],
    );
    plant_json(
        D3,
        |v| panel_mut(v, TEXT_TITLE)["targets"] = json!([{"refId": "A", "expr": "up"}]),
        &["G6"],
    );
    // Two dashboards with one uid: Grafana loads one of them.
    plant_json(D2, |v| v["uid"] = json!("d3-system"), &["G6", "G8"]);
}

#[test]
fn the_system_dashboard_has_the_failed_jobs_text_panel() {
    let text = |v: &mut Value| panel_mut(v, TEXT_TITLE).clone();
    let cases: Vec<Edit> = vec![
        Box::new(|v| {
            v["panels"]
                .as_array_mut()
                .unwrap()
                .retain(|p| p["type"] != "text");
        }),
        Box::new(move |v| {
            let mut t = text(v);
            t["id"] = json!(99);
            v["panels"].as_array_mut().unwrap().push(t);
        }),
        Box::new(|v| {
            let c = panel_mut(v, TEXT_TITLE)["options"]["content"]
                .as_str()
                .unwrap()
                .replace(
                    "systemctl --failed 'md-*' 'crypto-lake-audit*'",
                    "systemctl --failed",
                );
            panel_mut(v, TEXT_TITLE)["options"]["content"] = json!(c);
        }),
        Box::new(|v| panel_mut(v, TEXT_TITLE)["title"] = json!("일일 작업")),
    ];
    for f in cases {
        plant_json(D3, f, &["G6"]);
    }
    // Another dashboard may have text panels.
    let mut t = Tree::real();
    let mut panel = serde_json::Value::Null;
    edit(&mut t, D3, |v| panel = panel_mut(v, TEXT_TITLE).clone());
    edit(&mut t, D2, |v| {
        v["panels"].as_array_mut().unwrap().push(panel)
    });
    assert_rules(&crate::check(&t), &[]);
}

#[test]
fn a_dashboard_with_an_unknown_metric_fails() {
    let set = |title: &'static str, e: &'static str| -> Edit {
        Box::new(move |v| panel_mut(v, title)["targets"][0]["expr"] = json!(e))
    };
    let query = |name: &'static str, q: &'static str| -> Edit {
        Box::new(move |v| var_mut(v, name)["query"]["query"] = json!(q))
    };
    let cases: Vec<Edit> = vec![
        // A name in no part of the catalogue, after `or`, inside a function.
        set(
            MSGS,
            "sum by (market) (sum_over_time(md_msgz{venue=~\"$venue\"}[1m])) / 60",
        ),
        set(
            QUEUE,
            "max by (conn) (md_queue_bytes) or lake_audit_runs_total",
        ),
        // A label no entry has, a function not allowed, a shape the
        // scanner does not read.
        set(QUEUE, "max by (conn) (md_queue_bytes{venu=~\"$venue\"})"),
        set(QUEUE, "histogram_quantile(0.99, md_queue_bytes)"),
        set(QUEUE, "md_queue_bytes by (conn)"),
        // Templating: an unknown metric, a label the metric lacks, the
        // one-argument form, another query function, the string form.
        query("venue", "label_values(md_heartbeat_ts, venue)"),
        query("venue", "label_values(md_collector_heartbeat_ts, symbol)"),
        query("venue", "label_values(venue)"),
        query("venue", "query_result(up)"),
        query(
            "symbol",
            "label_values(md_spread_bp{venu=~\"$venue\"}, symbol)",
        ),
        Box::new(|v| var_mut(v, "venue")["query"] = json!("label_values(nosuch, venue)")),
    ];
    for f in cases {
        plant_json(D2, f, &["G7"]);
    }
    // A VictoriaLogs panel's LogsQL is not read as MetricsQL.
    let mut t = Tree::real();
    edit(&mut t, D2, |v| {
        let p = panel_mut(v, QUEUE);
        p["datasource"] = json!({"type": "victoriametrics-logs-datasource", "uid": "vl"});
        p["targets"] = json!([{
            "refId": "A",
            "datasource": {"type": "victoriametrics-logs-datasource", "uid": "vl"},
            "expr": "_time:5m service:=\"md-collector\" level:=error | stats by (venue) count() errors"
        }]);
    });
    assert_rules(&crate::check(&t), &[]);
}

#[test]
fn the_market_data_and_system_dashboards_exist() {
    let mut t = Tree::real();
    t.remove(D2);
    assert_rules(&crate::check(&t), &["U14", "G8"]);
    plant_json(D3, |v| v["uid"] = json!("d3"), &["G8"]);
    plant_json(D2, |v| v["title"] = json!("D2 시세"), &["G8"]);
    // The audit panel shows the alert's own value, not `increase`.
    plant_json(
        D3,
        |v| {
            panel_mut(v, AUDIT_PANEL)["targets"][0]["expr"] =
                json!("max by (venue) (increase(lake_audit_fail_total[26h]))")
        },
        &["G8"],
    );
    // The stale-topic panel shows `MdTopicsStale`'s value: not every
    // channel (quiet trade channels are always stale), not summed over
    // markets (one market's stale book hides under the other's).
    let stale = |e: &'static str| -> Edit {
        Box::new(move |v| panel_mut(v, STALE_PANEL)["targets"][0]["expr"] = json!(e))
    };
    for f in [
        stale(
            "count by (channel) (md_last_msg_age_s{venue=~\"$venue\"} > 60) / count by (channel) (md_last_msg_age_s{venue=~\"$venue\"})",
        ),
        stale(
            "count by (channel) (md_last_msg_age_s{venue=~\"$venue\", channel=~\"orderbook.*\"} > 60) / count by (channel) (md_last_msg_age_s{venue=~\"$venue\", channel=~\"orderbook.*\"})",
        ),
        stale(
            "count by (venue, market, channel) (md_last_msg_age_s{venue=~\"$venue\", channel=~\"orderbook.*\"} > 30) / count by (venue, market, channel) (md_last_msg_age_s{venue=~\"$venue\", channel=~\"orderbook.*\"})",
        ),
    ] {
        plant_json(D2, f, &["G8"]);
    }
    // `MdTopicsStale` changed without its panel.
    plant(
        "deploy/observability/vmalert/metrics/market-data.rules.json",
        "(md_last_msg_age_s{channel=~\\\"orderbook.*\\\"} > 60)",
        "(md_last_msg_age_s{channel=~\\\"orderbook.*\\\"} > 90)",
        &["G8"],
    );
    // An alert changed without its panel (R7 pins the alert).
    let lake = "deploy/observability/vmalert/metrics/lake.rules.json";
    plant(
        lake,
        "last_over_time(lake_audit_fail_total[26h]) * 0)) > 0",
        "last_over_time(lake_audit_fail_total[25h]) * 0)) > 0",
        &["R7", "G8"],
    );
    // The memory panel's caps are the units' MemoryMax (rule U6).
    let mem = |from: &'static str, to: &'static str| -> Edit {
        Box::new(move |v| {
            let e = panel_mut(v, MEMORY_PANEL)["targets"][0]["expr"]
                .as_str()
                .unwrap()
                .to_string();
            assert_eq!(e.matches(from).count(), 1, "{from}");
            panel_mut(v, MEMORY_PANEL)["targets"][0]["expr"] = json!(e.replacen(from, to, 1));
        })
    };
    for f in [
        mem(
            "{job=\"alertmanager\"} / 104857600",
            "{job=\"alertmanager\"} / 1048576000",
        ),
        mem(
            " or process_resident_memory_bytes{job=\"alertmanager\"} / 104857600",
            "",
        ),
        mem(
            "{job=\"alertmanager\"} / 104857600",
            "{job=\"alertmanager\"} / 104857600 or process_resident_memory_bytes{job=\"grafana\"} / 536870912",
        ),
    ] {
        plant_json(D3, f, &["G8"]);
    }
}
