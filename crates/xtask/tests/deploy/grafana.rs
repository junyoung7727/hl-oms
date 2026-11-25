//! Grafana rules (ADR-035 D9): G1 the listener, G2 alerting and anonymous
//! access off, G3 the admin password from a file, G4 the two datasources,
//! G5 the dashboard provider and the provisioning path, G6 the panels'
//! datasources and targets, G7 the dashboards' expressions against the
//! metric catalogue, G8 the two dashboards, G9 Grafana's data on the lake
//! disk.

use crate::catalogue::Catalogue;
use crate::ini::Ini;
use crate::tree::{Finding, Parsed, Tree, finding};

pub const INI: &str = "deploy/observability/grafana/grafana.ini";
/// The unit that starts Grafana.
const UNIT: &str = "grafana.service";

/// G1: the address in the repository. RFC 5737 TEST-NET-1 is never
/// assigned to this PC, so Grafana refuses to start until the operator
/// writes this PC's Tailscale address (100.64.0.0/10): refuse rather than
/// listen on every interface, which an empty or missing `http_addr` does.
const PLACEHOLDER: &str = "192.0.2.1";

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
