//! Unit rules: memory caps, timers, the collector's restart and protection
//! settings, the trading slice, schedules, the daily-job lock, listeners,
//! storage paths, store and vmalert flags, venues, the install map and
//! service identity (ADR-035).

use std::collections::BTreeSet;

use crate::tree::{Finding, Parsed, Tree, Unit, file_name, finding};

const OBS: &str = "deploy/observability/";

/// Units that the kernel must keep alive under memory pressure (U3,
/// protected group).
pub const PROTECTED: &[&str] = &["md-collector@.service"];

/// The observability stack's own services (U15: they run as `hlobs`).
const STACK: &[&str] = &[
    "victoriametrics.service",
    "victorialogs.service",
    "vmalert-metrics.service",
    "vmalert-logs.service",
    "alertmanager.service",
    "grafana.service",
];

/// The `MemoryMax` of each unit (U6, design §8.5).
const MEMORY_MAX: &[(&str, &str)] = &[
    ("victoriametrics.service", "1G"),
    ("victorialogs.service", "1G"),
    ("vmalert-metrics.service", "200M"),
    ("vmalert-logs.service", "200M"),
    ("alertmanager.service", "100M"),
    ("grafana.service", "512M"),
    ("md-collector@.service", "1G"),
    ("md-lake-seal@.service", "1G"),
    ("md-archiver@.service", "4G"),
];

/// Timers and their `OnCalendar` (U5).
const TIMERS: &[(&str, &str)] = &[
    ("md-lake-seal@.timer", "*-*-* *:05:00 UTC"),
    ("md-archiver@.timer", "*-*-* 01:10:00 UTC"),
];

/// Daily jobs, which run one at a time under one lock (U7).
const DAILY: &[&str] = &["md-archiver@.service"];
const LOCK: &str = "/usr/bin/flock /run/lock/hl-oms-daily.lock ";

/// Install paths that a unit's command line names before their files exist
/// (U14). `pending_install_paths_are_still_missing` fails once one exists.
pub const PENDING_DST: &[&str] = &[
    "/etc/hl-oms-obs/victoriametrics/scrape.json",
    "/etc/hl-oms-obs/vmalert/metrics/*.json",
    "/etc/hl-oms-obs/vmalert/logs/*.json",
    "/etc/hl-oms-obs/alertmanager/alertmanager.json",
    "/etc/hl-oms-obs/grafana/grafana.ini",
];

/// Files under `deploy/observability/` that are not installed (U14).
const NOT_INSTALLED: &[&str] = &["README.md", "install.toml", "venues.toml", "metrics.toml"];
const NOT_INSTALLED_DIRS: &[&str] = &["vmalert/tests/"];

#[derive(Clone, Copy)]
enum Want {
    Is(&'static str),
    Set,
    Absent,
}
use Want::{Absent, Is, Set};

/// U3, collector group: restarted at once, forever, unless its
/// configuration was refused.
const COLLECTOR: &[(&str, &str, Want)] = &[
    ("Service", "Type", Is("notify")),
    ("Service", "Restart", Is("always")),
    ("Service", "RestartSec", Is("2")),
    ("Service", "WatchdogSec", Is("30s")),
    ("Service", "NotifyAccess", Is("main")),
    ("Service", "RestartPreventExitStatus", Is("2")),
    ("Unit", "StartLimitIntervalSec", Is("0")),
    ("Service", "RestartSteps", Absent),
    ("Service", "RestartMaxDelaySec", Absent),
];

/// U3, protected group (plus `MemoryMin` = half of `MemoryMax`).
const PROTECTION: &[(&str, &str, Want)] = &[
    ("Service", "Slice", Is("trading.slice")),
    ("Service", "OOMScoreAdjust", Is("-900")),
    ("Service", "MemorySwapMax", Is("0")),
    ("Service", "MemoryMin", Set),
    ("Service", "IOWeight", Is("1000")),
];

pub fn check(t: &Tree, p: &Parsed, out: &mut Vec<Finding>) {
    u1_memory_max(p, out);
    u2_timers(p, out);
    u3_collector(p, out);
    u4_slice(p, out);
    u5_schedules(p, out);
    u6_design_caps(p, out);
    u7_daily_lock(p, out);
    u8_listeners(p, out);
    u9_stores(p, out);
    u10_u11_flags(p, out);
    u13_venues(t, p, out);
    u14_install_map(t, p, out);
    u15_identity(p, out);
}

/// A systemd size: bytes, or a number with `K`, `M` or `G` (base 1024).
fn size(v: &str) -> Option<u64> {
    let (n, mult) = match v.char_indices().last()? {
        (i, 'K') => (&v[..i], 1u64 << 10),
        (i, 'M') => (&v[..i], 1 << 20),
        (i, 'G') => (&v[..i], 1 << 30),
        _ => (v, 1),
    };
    if n.is_empty() || !n.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    n.parse::<u64>().ok()?.checked_mul(mult)
}

fn named<'a>(p: &'a Parsed, name: &'a str) -> impl Iterator<Item = &'a Unit> + 'a {
    p.units.iter().filter(move |u| u.name == name)
}

/// Reports a unit of `names` that does not exist; returns the units found.
fn required<'a>(
    p: &'a Parsed,
    name: &'a str,
    rule: &'static str,
    out: &mut Vec<Finding>,
) -> Vec<&'a Unit> {
    let found: Vec<&Unit> = named(p, name).collect();
    if found.is_empty() {
        finding(out, &format!("{OBS}systemd/{name}"), rule, "missing");
    }
    found
}

fn expect(
    u: &Unit,
    rule: &'static str,
    (section, key, want): (&str, &str, Want),
    out: &mut Vec<Finding>,
) {
    let got = u.ini.values(section, key);
    let ok = match want {
        Is(v) => got == [v],
        Set => got.len() == 1,
        Absent => got.is_empty(),
    };
    if !ok {
        let need = match want {
            Is(v) => format!("exactly once as {v}"),
            Set => "exactly once".to_string(),
            Absent => "absent".to_string(),
        };
        finding(
            out,
            &u.path,
            rule,
            format!("[{section}] {key} must be {need} (found {got:?})"),
        );
    }
}

/// The command lines of a unit: every `Exec*` value of `[Service]`.
fn exec_lines(u: &Unit) -> Vec<&str> {
    u.ini
        .entries
        .iter()
        .filter(|(s, k, _)| s == "Service" && k.starts_with("Exec"))
        .map(|(_, _, v)| v.as_str())
        .collect()
}

fn exec_tokens(u: &Unit) -> Vec<&str> {
    exec_lines(u)
        .into_iter()
        .flat_map(str::split_whitespace)
        .collect()
}

/// The values of every command-line token `flag=value` of a unit.
fn flag_values<'a>(u: &'a Unit, flag: &str) -> Vec<&'a str> {
    exec_tokens(u)
        .into_iter()
        .filter_map(|t| t.strip_prefix(flag))
        .collect()
}

/// `NAME=value` pairs of the unit's `Environment=` lines.
fn environment(u: &Unit) -> Vec<(&str, &str)> {
    u.ini
        .values("Service", "Environment")
        .into_iter()
        .flat_map(str::split_whitespace)
        .filter_map(|kv| kv.split_once('='))
        .collect()
}

fn u1_memory_max(p: &Parsed, out: &mut Vec<Finding>) {
    for u in p.units.iter().filter(|u| u.name.ends_with(".service")) {
        let v = u.ini.values("Service", "MemoryMax");
        if v.len() != 1 || size(v[0]).is_none() {
            finding(
                out,
                &u.path,
                "U1",
                format!(
                    "[Service] MemoryMax must be given once as a size, not infinity (found {v:?})"
                ),
            );
        }
    }
}

fn u2_timers(p: &Parsed, out: &mut Vec<Finding>) {
    let names: BTreeSet<&str> = p.units.iter().map(|u| u.name.as_str()).collect();
    for u in p
        .units
        .iter()
        .filter(|u| u.ini.values("Service", "Type") == ["oneshot"])
    {
        let timer = format!("{}.timer", u.name.trim_end_matches(".service"));
        if !names.contains(timer.as_str()) {
            finding(
                out,
                &u.path,
                "U2",
                format!("a oneshot service without its timer {timer}"),
            );
        }
    }
    for u in p.units.iter().filter(|u| u.name.ends_with(".timer")) {
        let service = format!("{}.service", u.name.trim_end_matches(".timer"));
        if !names.contains(service.as_str()) {
            finding(
                out,
                &u.path,
                "U2",
                format!("a timer without its service {service}"),
            );
        }
        if !u.ini.any("Unit").is_empty() {
            finding(
                out,
                &u.path,
                "U2",
                "a timer starts the service of its own name: no Unit=",
            );
        }
    }
}

fn u3_collector(p: &Parsed, out: &mut Vec<Finding>) {
    for u in required(p, "md-collector@.service", "U3", out) {
        for &row in COLLECTOR {
            expect(u, "U3", row, out);
        }
    }
    for name in PROTECTED {
        for u in required(p, name, "U3", out) {
            for &row in PROTECTION {
                expect(u, "U3", row, out);
            }
            let max = u.ini.one("Service", "MemoryMax").and_then(size);
            let min = u.ini.one("Service", "MemoryMin").and_then(size);
            if !matches!((min, max), (Some(min), Some(max)) if min.checked_mul(2) == Some(max)) {
                finding(out, &u.path, "U3", "MemoryMin must be half of MemoryMax");
            }
        }
    }
}

fn u4_slice(p: &Parsed, out: &mut Vec<Finding>) {
    let need = p
        .units
        .iter()
        .filter(|u| u.ini.values("Service", "Slice").contains(&"trading.slice"))
        .map(|u| {
            u.ini
                .one("Service", "MemoryMin")
                .and_then(size)
                .unwrap_or(0)
        })
        .sum::<u64>();
    for u in required(p, "trading.slice", "U4", out) {
        expect(u, "U4", ("Slice", "CPUWeight", Is("1000")), out);
        let have = u.ini.one("Slice", "MemoryMin").and_then(size);
        if have.is_none_or(|h| h < need) {
            finding(
                out,
                &u.path,
                "U4",
                format!(
                    "[Slice] MemoryMin must reserve at least the {need} bytes of MemoryMin of its units"
                ),
            );
        }
    }
}

fn u5_schedules(p: &Parsed, out: &mut Vec<Finding>) {
    for &(name, when) in TIMERS {
        for u in required(p, name, "U5", out) {
            expect(u, "U5", ("Timer", "OnCalendar", Is(when)), out);
            expect(u, "U5", ("Timer", "Persistent", Is("true")), out);
        }
    }
}

fn u6_design_caps(p: &Parsed, out: &mut Vec<Finding>) {
    for &(name, cap) in MEMORY_MAX {
        for u in required(p, name, "U6", out) {
            if u.ini.one("Service", "MemoryMax").and_then(size) != size(cap) {
                finding(
                    out,
                    &u.path,
                    "U6",
                    format!("MemoryMax must be the design cap {cap}"),
                );
            }
        }
    }
}

fn u7_daily_lock(p: &Parsed, out: &mut Vec<Finding>) {
    for name in DAILY {
        for u in required(p, name, "U7", out) {
            if u.ini.values("Service", "ExecStart").is_empty() {
                finding(out, &u.path, "U7", "no ExecStart");
            }
            for key in ["ExecStart", "ExecStartPre", "ExecStartPost"] {
                for v in u.ini.values("Service", key) {
                    if !v.starts_with(LOCK) {
                        finding(
                            out,
                            &u.path,
                            "U7",
                            format!("{key} must begin with {LOCK:?}"),
                        );
                    }
                }
            }
        }
    }
}

fn loopback_port(v: &str) -> bool {
    v.strip_prefix("127.0.0.1:")
        .is_some_and(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
}

fn u8_listeners(p: &Parsed, out: &mut Vec<Finding>) {
    const FLAGS: &[&str] = &["-httpListenAddr=", "--web.listen-address="];
    for u in &p.units {
        for flag in FLAGS {
            for v in flag_values(u, flag) {
                if !loopback_port(v) {
                    finding(
                        out,
                        &u.path,
                        "U8",
                        format!("{flag}{v} must be 127.0.0.1:<port>"),
                    );
                }
            }
        }
    }
    // A listener without the flag binds every interface: each stack
    // listener names its address once.
    for &(name, flag) in &[
        ("victoriametrics.service", "-httpListenAddr="),
        ("victorialogs.service", "-httpListenAddr="),
        ("vmalert-metrics.service", "-httpListenAddr="),
        ("vmalert-logs.service", "-httpListenAddr="),
        ("alertmanager.service", "--web.listen-address="),
    ] {
        for u in named(p, name) {
            if flag_values(u, flag).len() != 1 {
                finding(out, &u.path, "U8", format!("{flag} must be given once"));
            }
        }
    }
    for u in named(p, "alertmanager.service") {
        if flag_values(u, "--cluster.listen-address=") != [""] {
            finding(
                out,
                &u.path,
                "U8",
                "--cluster.listen-address= must be given once with an empty value (no gossip port)",
            );
        }
    }
}

fn u9_stores(p: &Parsed, out: &mut Vec<Finding>) {
    for u in &p.units {
        let env = environment(u);
        let mut values: Vec<(String, &str)> = Vec::new();
        for flag in ["-storageDataPath=", "--storage.path=", "--lake="] {
            values.extend(
                flag_values(u, flag)
                    .into_iter()
                    .map(|v| (flag.to_string(), v)),
            );
        }
        let tokens = exec_tokens(u);
        for w in tokens.windows(2) {
            if w[0] == "--lake" {
                values.push(("--lake ".to_string(), w[1]));
            }
        }
        values.extend(
            env.iter()
                .filter(|(k, _)| *k == "LAKE")
                .map(|(_, v)| ("LAKE=".to_string(), *v)),
        );
        for (what, v) in values {
            let resolved = match v
                .strip_prefix("${")
                .and_then(|r| r.strip_suffix('}'))
                .or_else(|| v.strip_prefix('$'))
            {
                Some(name) => match env.iter().rev().find(|(k, _)| *k == name) {
                    Some((_, val)) => *val,
                    None => {
                        finding(
                            out,
                            &u.path,
                            "U9",
                            format!("{what}{v}: the unit does not set {name}"),
                        );
                        continue;
                    }
                },
                None => v,
            };
            if !resolved.starts_with("/mnt/nvme/") {
                finding(
                    out,
                    &u.path,
                    "U9",
                    format!("{what}{v} = {resolved} must be under /mnt/nvme/"),
                );
            }
        }
    }
}

/// (unit, rule, flags with an exact value, flags that need some value).
type FlagRule = (
    &'static str,
    &'static str,
    &'static [(&'static str, &'static str)],
    &'static [&'static str],
);

fn u10_u11_flags(p: &Parsed, out: &mut Vec<Finding>) {
    const FLAGS: &[FlagRule] = &[
        (
            "victoriametrics.service",
            "U10",
            &[("-retentionPeriod=", "1y")],
            &["-memory.allowedBytes="],
        ),
        (
            "victorialogs.service",
            "U10",
            &[("-retentionPeriod=", "1y")],
            &[],
        ),
        (
            "vmalert-metrics.service",
            "U11",
            &[
                ("-datasource.url=", "http://127.0.0.1:8428"),
                ("-evaluationInterval=", "10s"),
            ],
            &["-notifier.url=", "-remoteWrite.url="],
        ),
        (
            "vmalert-logs.service",
            "U11",
            &[
                ("-datasource.url=", "http://127.0.0.1:9428"),
                ("-rule.defaultRuleType=", "vlogs"),
                ("-evaluationInterval=", "30s"),
            ],
            &["-notifier.url="],
        ),
    ];
    for &(name, rule, exact, any) in FLAGS {
        for u in required(p, name, rule, out) {
            for &(flag, want) in exact {
                if flag_values(u, flag) != [want] {
                    finding(
                        out,
                        &u.path,
                        rule,
                        format!("{flag}{want} must be given once"),
                    );
                }
            }
            for flag in any {
                if !matches!(flag_values(u, flag).as_slice(), [v] if !v.is_empty()) {
                    finding(
                        out,
                        &u.path,
                        rule,
                        format!("{flag} must be given once with a value"),
                    );
                }
            }
        }
    }
}

fn u13_venues(t: &Tree, p: &Parsed, out: &mut Vec<Finding>) {
    let path = format!("{OBS}venues.toml");
    let venues: Option<Vec<&str>> = p
        .toml
        .get(&path)
        .and_then(|v| v.get("venues"))
        .and_then(|v| v.as_array())
        .and_then(|a| a.iter().map(|v| v.as_str()).collect());
    let Some(venues) = venues.filter(|v| !v.is_empty()) else {
        finding(
            out,
            &path,
            "U13",
            "venues must be a non-empty list of venue names",
        );
        return;
    };
    for v in venues {
        for registry in [
            "crates/md-collector/src/registry.rs",
            "crates/md-archiver/src/registry.rs",
        ] {
            let literal = format!("\"{v}\" => Some(");
            if !t.files.get(registry).is_some_and(|s| s.contains(&literal)) {
                finding(
                    out,
                    &path,
                    "U13",
                    format!("{v} is not a registered venue in {registry}"),
                );
            }
        }
    }
}

/// `pattern` with `*` matching any run of characters other than `/`.
fn glob(pattern: &str, s: &str) -> bool {
    match pattern.split_once('*') {
        None => pattern == s,
        Some((head, tail)) => {
            let Some(rest) = s.strip_prefix(head) else {
                return false;
            };
            (0..=rest.len())
                .filter(|&i| rest.is_char_boundary(i))
                .take_while(|&i| !rest[..i].contains('/'))
                .any(|i| glob(tail, &rest[i..]))
        }
    }
}

/// Every `/etc/hl-oms-obs/…` and `/usr/local/lib/hl-oms-obs/…` path or glob
/// that a unit's command line or environment names.
fn install_paths(u: &Unit) -> Vec<&str> {
    let mut lines = exec_lines(u);
    lines.extend(u.ini.values("Service", "Environment"));
    let mut found = Vec::new();
    for line in lines {
        for prefix in ["/etc/hl-oms-obs/", "/usr/local/lib/hl-oms-obs/"] {
            for (i, _) in line.match_indices(prefix) {
                let rest = &line[i..];
                let end = rest
                    .find(|c: char| c.is_whitespace() || c == '"' || c == '\'')
                    .unwrap_or(rest.len());
                found.push(&rest[..end]);
            }
        }
    }
    found
}

fn not_installed(src: &str) -> bool {
    NOT_INSTALLED.contains(&src) || NOT_INSTALLED_DIRS.iter().any(|d| src.starts_with(d))
}

/// The install path a source file must have, and its mode.
fn install_target(src: &str) -> (String, &'static str) {
    let name = file_name(src);
    if [".service", ".timer", ".slice"]
        .iter()
        .any(|e| name.ends_with(e))
    {
        (format!("/etc/systemd/system/{name}"), "0644")
    } else if src.starts_with("bin/") {
        (format!("/usr/local/lib/hl-oms-obs/{name}"), "0755")
    } else {
        (format!("/etc/hl-oms-obs/{src}"), "0644")
    }
}

/// `(src, dst, mode)` of every entry of the install map; a malformed entry
/// is a finding and is left out.
fn install_entries(p: &Parsed, out: &mut Vec<Finding>) -> Vec<(String, String, String)> {
    let path = format!("{OBS}install.toml");
    let Some(files) = p
        .toml
        .get(&path)
        .and_then(|v| v.get("file"))
        .and_then(|v| v.as_array())
    else {
        finding(out, &path, "U14", "no [[file]] entries");
        return Vec::new();
    };
    let mut entries = Vec::new();
    for (i, f) in files.iter().enumerate() {
        let get = |k: &str| f.get(k).and_then(|v| v.as_str()).map(str::to_string);
        match (get("src"), get("dst"), get("mode")) {
            (Some(s), Some(d), Some(m)) => entries.push((s, d, m)),
            _ => finding(
                out,
                &path,
                "U14",
                format!("entry {} needs src, dst and mode as strings", i + 1),
            ),
        }
    }
    entries
}

fn u14_install_map(t: &Tree, p: &Parsed, out: &mut Vec<Finding>) {
    let path = format!("{OBS}install.toml");
    let entries = install_entries(p, out);
    let mut dsts = BTreeSet::new();
    for (src, dst, mode) in &entries {
        if !t.files.contains_key(&format!("{OBS}{src}")) {
            finding(out, &path, "U14", format!("src {src} does not exist"));
        }
        if not_installed(src) {
            finding(out, &path, "U14", format!("{src} is not installed"));
        }
        if !dsts.insert(dst.as_str()) {
            finding(out, &path, "U14", format!("dst {dst} is given twice"));
        }
        let (want_dst, want_mode) = install_target(src);
        if *dst != want_dst || mode != want_mode {
            finding(
                out,
                &path,
                "U14",
                format!(
                    "{src} must install to {want_dst} with mode {want_mode} (found {dst} {mode})"
                ),
            );
        }
    }
    let srcs: BTreeSet<&str> = entries.iter().map(|(s, _, _)| s.as_str()).collect();
    for file in t.files.keys().filter_map(|k| k.strip_prefix(OBS)) {
        if !not_installed(file) && !srcs.contains(file) {
            finding(out, &path, "U14", format!("{file} has no install entry"));
        }
    }
    for u in &p.units {
        for want in install_paths(u) {
            if !dsts.iter().any(|d| glob(want, d)) && !PENDING_DST.contains(&want) {
                finding(
                    out,
                    &u.path,
                    "U14",
                    format!("{want} is no install path of the install map"),
                );
            }
        }
    }
}

fn u15_identity(p: &Parsed, out: &mut Vec<Finding>) {
    for u in &p.units {
        if u.ini.values("Service", "User").contains(&"root") {
            finding(
                out,
                &u.path,
                "U15",
                "User=root is not written out: a unit without User= runs as root",
            );
        }
    }
    for name in STACK {
        for u in named(p, name) {
            expect(u, "U15", ("Service", "User", Is("hlobs")), out);
        }
    }
}

// Tests.

use crate::assert_rules;

const SYS: &str = "deploy/observability/systemd/";
const INSTALL: &str = "deploy/observability/install.toml";

fn unit(name: &str) -> String {
    format!("{SYS}{name}")
}

/// The real tree with `from` replaced by `to` in one unit.
fn with(name: &str, from: &str, to: &str) -> Tree {
    let mut t = Tree::real();
    t.replace(&unit(name), from, to);
    t
}

fn plant(name: &str, from: &str, to: &str, rules: &[&str]) {
    assert_rules(&crate::check(&with(name, from, to)), rules);
}

/// Adds a file and its install entry.
fn add_installed(t: &mut Tree, src: &str, text: &str, dst: &str, mode: &str) {
    t.set(&format!("{OBS}{src}"), text);
    let mut map = t.files[INSTALL].clone();
    map.push_str(&format!(
        "\n[[file]]\nsrc = \"{src}\"\ndst = \"{dst}\"\nmode = \"{mode}\"\n"
    ));
    t.set(INSTALL, &map);
}

#[test]
fn a_collector_unit_without_restart_always_fails() {
    plant("md-collector@.service", "Restart=always\n", "", &["U3"]);
    plant(
        "md-collector@.service",
        "Restart=always\n",
        "Restart=on-failure\n",
        &["U3"],
    );
    plant(
        "md-collector@.service",
        "Restart=always\n",
        "Restart=always\nRestart=no\n",
        &["U3"],
    );
    plant(
        "md-collector@.service",
        "RestartSec=2\n",
        "RestartSec=5\n",
        &["U3"],
    );
    plant(
        "md-collector@.service",
        "RestartPreventExitStatus=2\n",
        "",
        &["U3"],
    );
    plant(
        "md-collector@.service",
        "Type=notify\n",
        "Type=simple\n",
        &["U3"],
    );
    plant(
        "md-collector@.service",
        "NotifyAccess=main\n",
        "NotifyAccess=all\n",
        &["U3"],
    );
    // The unit itself gone.
    let mut t = Tree::real();
    t.remove(&unit("md-collector@.service"));
    let f = crate::check(&t);
    assert!(
        f.iter().any(|x| x.rule == "U3" && x.text == "missing"),
        "{f:?}"
    );
}

#[test]
fn a_collector_unit_without_watchdog_sec_fails() {
    plant("md-collector@.service", "WatchdogSec=30s\n", "", &["U3"]);
    plant(
        "md-collector@.service",
        "WatchdogSec=30s\n",
        "WatchdogSec=0\n",
        &["U3"],
    );
}

#[test]
fn a_collector_unit_with_a_start_limit_fails() {
    plant(
        "md-collector@.service",
        "StartLimitIntervalSec=0\n",
        "",
        &["U3"],
    );
    plant(
        "md-collector@.service",
        "StartLimitIntervalSec=0\n",
        "StartLimitIntervalSec=10s\n",
        &["U3"],
    );
    // In [Service] the key does not lift the limit.
    plant(
        "md-collector@.service",
        "StartLimitIntervalSec=0\n\n[Service]\n",
        "\n[Service]\nStartLimitIntervalSec=0\n",
        &["U3"],
    );
}

#[test]
fn a_collector_unit_with_a_restart_back_off_fails() {
    plant(
        "md-collector@.service",
        "RestartSec=2\n",
        "RestartSec=2\nRestartSteps=5\n",
        &["U3"],
    );
    plant(
        "md-collector@.service",
        "RestartSec=2\n",
        "RestartSec=2\nRestartMaxDelaySec=60\n",
        &["U3"],
    );
}

#[test]
fn a_collector_unit_without_oom_score_adjust_fails() {
    plant(
        "md-collector@.service",
        "OOMScoreAdjust=-900\n",
        "",
        &["U3"],
    );
    plant(
        "md-collector@.service",
        "OOMScoreAdjust=-900\n",
        "OOMScoreAdjust=0\n",
        &["U3"],
    );
    plant(
        "md-collector@.service",
        "OOMScoreAdjust=-900\n",
        "OOMScoreAdjust=-500\n",
        &["U3"],
    );
}

#[test]
fn a_collector_unit_without_memory_swap_max_zero_fails() {
    plant("md-collector@.service", "MemorySwapMax=0\n", "", &["U3"]);
    plant(
        "md-collector@.service",
        "MemorySwapMax=0\n",
        "MemorySwapMax=1G\n",
        &["U3"],
    );
}

#[test]
fn a_collector_unit_with_wrong_memory_min_or_io_weight_fails() {
    // A smaller reservation still fits the slice: U3 alone.
    plant(
        "md-collector@.service",
        "MemoryMin=512M\n",
        "MemoryMin=256M\n",
        &["U3"],
    );
    // A larger one is also more than the slice reserves.
    plant(
        "md-collector@.service",
        "MemoryMin=512M\n",
        "MemoryMin=1G\n",
        &["U3", "U4"],
    );
    plant("md-collector@.service", "MemoryMin=512M\n", "", &["U3"]);
    plant(
        "md-collector@.service",
        "IOWeight=1000\n",
        "IOWeight=100\n",
        &["U3"],
    );
    plant("md-collector@.service", "IOWeight=1000\n", "", &["U3"]);
    // Outside the slice its reservation is not counted there.
    plant(
        "md-collector@.service",
        "Slice=trading.slice\n",
        "Slice=system.slice\n",
        &["U3"],
    );
    // Half of the cap, in bytes: 512M of 1024M is the same as 512M of 1G.
    plant(
        "md-collector@.service",
        "MemoryMax=1G\n",
        "MemoryMax=1024M\n",
        &[],
    );
}

#[test]
fn the_trading_slice_reserves_its_units_memory() {
    // A second unit in the slice: 512M + 100M > 512M.
    plant(
        "vmalert-metrics.service",
        "MemoryMax=200M\n",
        "MemoryMax=200M\nSlice=trading.slice\nMemoryMin=100M\n",
        &["U4"],
    );
    plant(
        "trading.slice",
        "MemoryMin=512M\n",
        "MemoryMin=256M\n",
        &["U4"],
    );
    plant("trading.slice", "MemoryMin=512M\n", "", &["U4"]);
    plant(
        "trading.slice",
        "CPUWeight=1000\n",
        "CPUWeight=100\n",
        &["U4"],
    );
    plant("trading.slice", "MemoryMin=512M\n", "MemoryMin=1G\n", &[]);
    let mut t = Tree::real();
    t.remove(&unit("trading.slice"));
    assert_rules(&crate::check(&t), &["U4", "U14"]);
}

#[test]
fn a_service_without_memory_max_fails() {
    // A plain unit and a template unit.
    plant(
        "victoriametrics.service",
        "MemoryMax=1G\n",
        "",
        &["U1", "U6"],
    );
    plant("md-lake-seal@.service", "MemoryMax=1G\n", "", &["U1", "U6"]);
    plant(
        "md-archiver@.service",
        "MemoryMax=4G\n",
        "MemoryMax=infinity\n",
        &["U1", "U6"],
    );
    plant(
        "grafana.service",
        "MemoryMax=512M\n",
        "MemoryMax=50%\n",
        &["U1", "U6"],
    );
    plant(
        "alertmanager.service",
        "MemoryMax=100M\n",
        "MemoryMax=100M\nMemoryMax=200M\n",
        &["U1", "U6"],
    );
    // A service of no design row still needs a cap.
    let mut t = Tree::real();
    let text = "[Unit]\nDescription=x\n\n[Service]\nType=simple\nUser=hlobs\nExecStart=/bin/true\n";
    add_installed(
        &mut t,
        "systemd/extra@.service",
        text,
        "/etc/systemd/system/extra@.service",
        "0644",
    );
    assert_rules(&crate::check(&t), &["U1"]);
}

#[test]
fn memory_max_follows_the_design_table() {
    plant(
        "vmalert-logs.service",
        "MemoryMax=200M\n",
        "MemoryMax=300M\n",
        &["U6"],
    );
    plant(
        "md-archiver@.service",
        "MemoryMax=4G\n",
        "MemoryMax=8G\n",
        &["U6"],
    );
    plant(
        "md-lake-seal@.service",
        "MemoryMax=1G\n",
        "MemoryMax=2G\n",
        &["U6"],
    );
    plant(
        "md-archiver@.service",
        "MemoryMax=4G\n",
        "MemoryMax=4096M\n",
        &[],
    );
    for &(name, _) in MEMORY_MAX {
        let mut t = Tree::real();
        t.remove(&unit(name));
        let f = crate::check(&t);
        assert!(
            f.iter()
                .any(|x| x.rule == "U6" && x.file == unit(name) && x.text == "missing"),
            "{name}: {f:?}"
        );
    }
}

#[test]
fn a_oneshot_without_a_timer_fails() {
    plant(
        "victorialogs.service",
        "Type=simple\n",
        "Type=oneshot\n",
        &["U2"],
    );
    let mut t = Tree::real();
    t.remove(&unit("md-archiver@.timer"));
    assert_rules(&crate::check(&t), &["U2", "U5", "U14"]);
}

#[test]
fn a_timer_without_its_service_fails() {
    let mut t = Tree::real();
    let text = "[Unit]\nDescription=x\n\n[Timer]\nOnCalendar=daily\nPersistent=true\n";
    add_installed(
        &mut t,
        "systemd/orphan@.timer",
        text,
        "/etc/systemd/system/orphan@.timer",
        "0644",
    );
    assert_rules(&crate::check(&t), &["U2"]);
    plant(
        "md-lake-seal@.timer",
        "Persistent=true\n",
        "Persistent=true\nUnit=md-archiver@%i.service\n",
        &["U2"],
    );
    let mut t = Tree::real();
    t.remove(&unit("md-lake-seal@.service"));
    // The seal service gone: its timer is an orphan, and its design row
    // is missing.
    assert_rules(&crate::check(&t), &["U2", "U6", "U14"]);
}

#[test]
fn the_seal_archiver_and_audit_timers_keep_their_schedule() {
    for &(name, when) in TIMERS {
        let mut t = Tree::real();
        t.remove(&unit(name));
        assert_rules(&crate::check(&t), &["U2", "U5", "U14"]);
        plant(
            name,
            &format!("OnCalendar={when}\n"),
            "OnCalendar=hourly\n",
            &["U5"],
        );
        plant(
            name,
            &format!("OnCalendar={when}\n"),
            &format!("OnCalendar={when}\nOnCalendar=daily\n"),
            &["U5"],
        );
        plant(name, "Persistent=true\n", "", &["U5"]);
        plant(name, "Persistent=true\n", "Persistent=false\n", &["U5"]);
    }
    plant("md-lake-seal@.timer", "*:05:00", "*:02:00", &["U5"]);
}

#[test]
fn a_daily_job_without_the_lock_fails() {
    plant(
        "md-archiver@.service",
        "ExecStart=/usr/bin/flock /run/lock/hl-oms-daily.lock ",
        "ExecStart=",
        &["U7"],
    );
    plant(
        "md-archiver@.service",
        "ExecStart=/usr/bin/flock /run/lock/hl-oms-daily.lock ",
        "ExecStart=/usr/bin/flock /run/lock/other.lock ",
        &["U7"],
    );
    plant(
        "md-archiver@.service",
        "Nice=10\n",
        "Nice=10\nExecStartPre=/bin/true\n",
        &["U7"],
    );
    plant(
        "md-archiver@.service",
        "Nice=10\n",
        "Nice=10\nExecStartPost=/bin/true\n",
        &["U7"],
    );
    let mut t = Tree::real();
    let path = unit("md-archiver@.service");
    let text: String = t.files[&path]
        .lines()
        .filter(|l| !l.starts_with("ExecStart="))
        .map(|l| format!("{l}\n"))
        .collect();
    t.set(&path, &text);
    assert_rules(&crate::check(&t), &["U7"]);
}

#[test]
fn a_listener_off_loopback_fails() {
    plant(
        "victoriametrics.service",
        "-httpListenAddr=127.0.0.1:8428",
        "-httpListenAddr=0.0.0.0:8428",
        &["U8"],
    );
    plant(
        "victoriametrics.service",
        "-httpListenAddr=127.0.0.1:8428",
        "-httpListenAddr=:8428",
        &["U8"],
    );
    plant(
        "victorialogs.service",
        "-httpListenAddr=127.0.0.1:9428",
        "-httpListenAddr=127.0.0.1:",
        &["U8"],
    );
    plant(
        "vmalert-logs.service",
        " -httpListenAddr=127.0.0.1:8881",
        "",
        &["U8"],
    );
    plant(
        "vmalert-metrics.service",
        "-httpListenAddr=127.0.0.1:8880",
        "-httpListenAddr=127.0.0.1:8880 -httpListenAddr=:8882",
        &["U8"],
    );
    plant(
        "alertmanager.service",
        "--web.listen-address=127.0.0.1:9093",
        "--web.listen-address=:9093",
        &["U8"],
    );
    plant(
        "alertmanager.service",
        " --cluster.listen-address=",
        "",
        &["U8"],
    );
    plant(
        "alertmanager.service",
        "--cluster.listen-address=",
        "--cluster.listen-address=0.0.0.0:9094",
        &["U8"],
    );
}

#[test]
fn a_store_off_the_lake_disk_fails() {
    plant(
        "victoriametrics.service",
        "-storageDataPath=/mnt/nvme/observability/victoria-metrics",
        "-storageDataPath=/var/lib/vm",
        &["U9"],
    );
    plant(
        "victorialogs.service",
        "-storageDataPath=/mnt/nvme/observability/victoria-logs",
        "-storageDataPath=victoria-logs",
        &["U9"],
    );
    plant(
        "alertmanager.service",
        "--storage.path=/mnt/nvme/observability/alertmanager",
        "--storage.path=/tmp/am",
        &["U9"],
    );
    // The variable's value is checked: LAKE= and --lake ${LAKE} both.
    plant(
        "md-lake-seal@.service",
        "Environment=LAKE=/mnt/nvme/lake\n",
        "Environment=LAKE=/home/x\n",
        &["U9"],
    );
    // --lake ${LAKE} with no Environment=LAKE=.
    plant(
        "md-archiver@.service",
        "Environment=LAKE=/mnt/nvme/lake\n",
        "",
        &["U9"],
    );
    plant(
        "md-archiver@.service",
        "--lake ${LAKE}",
        "--lake /home/x",
        &["U9"],
    );
    plant(
        "md-archiver@.service",
        "--lake ${LAKE}",
        "--lake=$LAKE",
        &[],
    );
    plant(
        "md-archiver@.service",
        "--lake ${LAKE}",
        "--lake $OTHER",
        &["U9"],
    );
    let f = crate::check(&with(
        "md-lake-seal@.service",
        "Environment=LAKE=/mnt/nvme/lake\n",
        "Environment=LAKE=/home/x\n",
    ));
    assert_eq!(f.len(), 2, "both the variable and the flag: {f:?}");
}

#[test]
fn vmalert_units_carry_their_sources_and_intervals() {
    plant(
        "vmalert-metrics.service",
        "-datasource.url=http://127.0.0.1:8428",
        "-datasource.url=http://127.0.0.1:9428",
        &["U11"],
    );
    plant(
        "vmalert-metrics.service",
        "-evaluationInterval=10s",
        "-evaluationInterval=1m",
        &["U11"],
    );
    plant(
        "vmalert-metrics.service",
        "-evaluationInterval=10s",
        "-evaluationInterval=10s -evaluationInterval=1m",
        &["U11"],
    );
    plant(
        "vmalert-metrics.service",
        " -remoteWrite.url=http://127.0.0.1:8428",
        "",
        &["U11"],
    );
    plant(
        "vmalert-metrics.service",
        " -notifier.url=http://127.0.0.1:9093",
        " -notifier.url=",
        &["U11"],
    );
    plant(
        "vmalert-logs.service",
        "-datasource.url=http://127.0.0.1:9428",
        "-datasource.url=http://127.0.0.1:8428",
        &["U11"],
    );
    plant(
        "vmalert-logs.service",
        " -rule.defaultRuleType=vlogs",
        "",
        &["U11"],
    );
    plant(
        "vmalert-logs.service",
        "-evaluationInterval=30s",
        "-evaluationInterval=10s",
        &["U11"],
    );
    plant(
        "vmalert-logs.service",
        " -notifier.url=http://127.0.0.1:9093",
        "",
        &["U11"],
    );
}

#[test]
fn the_stores_keep_their_retention_and_memory_flags() {
    plant(
        "victoriametrics.service",
        "-retentionPeriod=1y",
        "-retentionPeriod=1",
        &["U10"],
    );
    plant(
        "victoriametrics.service",
        " -retentionPeriod=1y",
        "",
        &["U10"],
    );
    plant(
        "victoriametrics.service",
        " -memory.allowedBytes=600MiB",
        "",
        &["U10"],
    );
    plant(
        "victoriametrics.service",
        "-memory.allowedBytes=600MiB",
        "-memory.allowedBytes=",
        &["U10"],
    );
    plant(
        "victorialogs.service",
        "-retentionPeriod=1y",
        "-retentionPeriod=7d",
        &["U10"],
    );
}

#[test]
fn an_unregistered_venue_fails() {
    let venues = format!("{OBS}venues.toml");
    for text in [
        "venues = [\"bybit\", \"okx\"]\n",
        "venues = []\n",
        "venue = [\"bybit\"]\n",
        "venues = [\"bybit\", 1]\n",
    ] {
        let mut t = Tree::real();
        t.set(&venues, text);
        assert_rules(&crate::check(&t), &["U13"]);
    }
    for registry in [
        "crates/md-collector/src/registry.rs",
        "crates/md-archiver/src/registry.rs",
    ] {
        let mut t = Tree::real();
        t.replace(registry, "\"bybit\" => Some(", "\"bybit2\" => Some(");
        assert_rules(&crate::check(&t), &["U13"]);
    }
    let mut t = Tree::real();
    t.remove(&venues);
    assert_rules(&crate::check(&t), &["U13"]);
}

#[test]
fn the_install_map_covers_every_file_and_path() {
    let entry = "\n[[file]]\nsrc = \"systemd/trading.slice\"\ndst = \"/etc/systemd/system/trading.slice\"\nmode = \"0644\"\n";
    let mut t = Tree::real();
    t.replace(INSTALL, entry, "");
    assert_rules(&crate::check(&t), &["U14"]);
    for (from, to) in [
        (
            "src = \"systemd/trading.slice\"",
            "src = \"systemd/nothing.slice\"",
        ),
        (
            "dst = \"/etc/systemd/system/trading.slice\"",
            "dst = \"/etc/systemd/system/md-archiver@.timer\"",
        ),
        (
            "dst = \"/etc/systemd/system/trading.slice\"",
            "dst = \"/etc/hl-oms-obs/systemd/trading.slice\"",
        ),
        (
            "dst = \"/etc/systemd/system/trading.slice\"",
            "dst = \"etc/systemd/system/trading.slice\"",
        ),
        (
            "dst = \"/etc/systemd/system/trading.slice\"\nmode = \"0644\"",
            "dst = \"/etc/systemd/system/trading.slice\"\nmode = \"0755\"",
        ),
        (
            "dst = \"/etc/systemd/system/trading.slice\"\nmode = \"0644\"",
            "dst = \"/etc/systemd/system/trading.slice\"\nmode = 644",
        ),
    ] {
        let mut t = Tree::real();
        t.replace(INSTALL, from, to);
        assert_rules(&crate::check(&t), &["U14"]);
    }
    // A file that is not installed listed as a src.
    let mut t = Tree::real();
    let map = format!(
        "{}\n[[file]]\nsrc = \"venues.toml\"\ndst = \"/etc/hl-oms-obs/venues.toml\"\nmode = \"0644\"\n",
        t.files[INSTALL]
    );
    t.set(INSTALL, &map);
    assert_rules(&crate::check(&t), &["U14"]);
    // A new config file without an entry, then with its entry.
    let mut t = Tree::real();
    t.set(&format!("{OBS}victoriametrics/extra.json"), "{}");
    assert_rules(&crate::check(&t), &["U14"]);
    let mut t = Tree::real();
    add_installed(
        &mut t,
        "victoriametrics/extra.json",
        "{}",
        "/etc/hl-oms-obs/victoriametrics/extra.json",
        "0644",
    );
    assert_rules(&crate::check(&t), &[]);
    // Not installed: no entry needed.
    let mut t = Tree::real();
    t.set(&format!("{OBS}vmalert/tests/x.test.json"), "{}");
    assert_rules(&crate::check(&t), &[]);
    // A script under bin/ is installed executable, to the library dir.
    for (dst, mode, rules) in [
        ("/usr/local/lib/hl-oms-obs/x.sh", "0755", &[][..]),
        ("/usr/local/lib/hl-oms-obs/x.sh", "0644", &["U14"][..]),
        ("/etc/hl-oms-obs/bin/x.sh", "0755", &["U14"][..]),
    ] {
        let mut t = Tree::real();
        add_installed(&mut t, "bin/x.sh", "#!/bin/sh\n", dst, mode);
        assert_rules(&crate::check(&t), rules);
    }
    let mut t = Tree::real();
    t.set(INSTALL, "[[file]]\nsrc = \"x\"\n");
    assert!(
        crate::check(&t)
            .iter()
            .any(|x| x.rule == "U14" && x.text.contains("needs src, dst and mode"))
    );
}

#[test]
fn a_unit_path_without_an_install_dst_fails() {
    let tmpl = "-rule.templates=/etc/hl-oms-obs/vmalert/templates/*.tmpl";
    plant(
        "vmalert-metrics.service",
        "-evaluationInterval=10s",
        &format!("-evaluationInterval=10s {tmpl}"),
        &["U14"],
    );
    plant(
        "md-archiver@.service",
        "Nice=10\n",
        "Nice=10\nEnvironment=HOOK=/usr/local/lib/hl-oms-obs/hook.sh\n",
        &["U14"],
    );
    // With a file that the glob matches, the path is covered.
    let mut t = with(
        "vmalert-metrics.service",
        "-evaluationInterval=10s",
        &format!("-evaluationInterval=10s {tmpl}"),
    );
    add_installed(
        &mut t,
        "vmalert/templates/a.tmpl",
        "",
        "/etc/hl-oms-obs/vmalert/templates/a.tmpl",
        "0644",
    );
    let f = crate::check(&t);
    assert!(f.iter().all(|x| x.rule != "U14"), "{f:?}");
    // A glob does not reach into a subdirectory.
    let mut t = with(
        "vmalert-metrics.service",
        "-evaluationInterval=10s",
        &format!("-evaluationInterval=10s {tmpl}"),
    );
    add_installed(
        &mut t,
        "vmalert/templates/sub/a.tmpl",
        "",
        "/etc/hl-oms-obs/vmalert/templates/sub/a.tmpl",
        "0644",
    );
    assert!(crate::check(&t).iter().any(|x| x.rule == "U14"));
}

#[test]
fn pending_install_paths_are_still_missing() {
    let t = Tree::real();
    let mut out = Vec::new();
    let p = crate::tree::parse(&t, &mut out);
    let dsts: Vec<String> = install_entries(&p, &mut out)
        .into_iter()
        .map(|(_, d, _)| d)
        .collect();
    let named: BTreeSet<&str> = p.units.iter().flat_map(install_paths).collect();
    for pending in PENDING_DST {
        assert!(
            !dsts.iter().any(|d| glob(pending, d)),
            "{pending} has an install entry now: remove it from PENDING_DST"
        );
        assert!(
            named.contains(pending),
            "{pending} is named by no unit: remove it from PENDING_DST"
        );
    }
}

#[test]
fn stack_units_run_as_hlobs_and_never_as_root() {
    plant(
        "md-collector@.service",
        "Type=notify\n",
        "Type=notify\nUser=root\n",
        &["U15"],
    );
    plant(
        "md-archiver@.service",
        "Type=oneshot\n",
        "Type=oneshot\nUser=root\n",
        &["U15"],
    );
    plant("grafana.service", "User=hlobs\n", "", &["U15"]);
    plant(
        "victoriametrics.service",
        "User=hlobs\n",
        "User=nobody\n",
        &["U15"],
    );
}

#[test]
fn sizes_and_globs_read_as_systemd_and_vmalert_do() {
    assert_eq!(size("512M"), Some(512 << 20));
    assert_eq!(size("1G"), size("1024M"));
    assert_eq!(size("4096"), Some(4096));
    assert_eq!(size("2K"), Some(2048));
    for bad in ["infinity", "", "M", "1.5G", "50%", "-1G", "1T"] {
        assert_eq!(size(bad), None, "{bad}");
    }
    assert!(glob("/a/*.json", "/a/x.json"));
    assert!(glob("/a/*.json", "/a/.json"));
    assert!(!glob("/a/*.json", "/a/b/x.json"));
    assert!(!glob("/a/*.json", "/a/x.yaml"));
    assert!(glob("/a/x", "/a/x"));
}
