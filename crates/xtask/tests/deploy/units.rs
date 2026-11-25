//! Unit rules: memory caps, timers, the collector's restart and protection
//! settings, the trading slice, schedules, the daily-job lock, listeners,
//! storage paths, store and vmalert flags, venues, the install map, service
//! identity, the audit's checkout, state and lake writes, the text of command
//! lines, the stack units' restart and sandbox settings, and the bounds of
//! the seal, archiver and audit runs (ADR-035).

use std::collections::BTreeSet;

use crate::tree::{Finding, Parsed, Tree, Unit, file_name, finding};

const OBS: &str = "deploy/observability/";

/// Units that the kernel must keep alive under memory pressure (U3,
/// protected group).
pub const PROTECTED: &[&str] = &["md-collector@.service"];

/// The observability stack's own services (U15: they run as `hlobs`) and
/// the one directory each may write (U18).
const STACK: &[(&str, Want)] = &[
    (
        "victoriametrics.service",
        Is("/mnt/nvme/observability/victoria-metrics"),
    ),
    (
        "victorialogs.service",
        Is("/mnt/nvme/observability/victoria-logs"),
    ),
    ("vmalert-metrics.service", Absent),
    ("vmalert-logs.service", Absent),
    (
        "alertmanager.service",
        Is("/mnt/nvme/observability/alertmanager"),
    ),
    ("grafana.service", Is("/mnt/nvme/observability/grafana")),
];

/// U18: restarted after a failure, sandboxed, and stopped by no other unit
/// (a restart of a store must not stop vmalert).
const STACK_SETTINGS: &[(&str, &str, Want)] = &[
    ("Service", "Restart", Is("on-failure")),
    ("Service", "RestartSec", Is("5")),
    ("Service", "NoNewPrivileges", Is("yes")),
    ("Service", "ProtectSystem", Is("strict")),
    ("Service", "ProtectHome", Is("yes")),
    ("Service", "PrivateTmp", Is("yes")),
    ("Unit", "Requires", Absent),
    ("Unit", "Requisite", Absent),
    ("Unit", "BindsTo", Absent),
    ("Unit", "PartOf", Absent),
];

/// Store path flags (U9) and the units that must give theirs once.
const STORE_FLAGS: &[&str] = &["storageDataPath", "storage.path", "lake"];
const STORES: &[(&str, &str)] = &[
    ("victoriametrics.service", "storageDataPath"),
    ("victorialogs.service", "storageDataPath"),
    ("alertmanager.service", "storage.path"),
    ("md-lake-seal@.service", "lake"),
    ("md-archiver@.service", "lake"),
];

/// Oneshot runs that hold the daily lock or the next hour's seal while they
/// run: each ends after at most this time (U19).
const BOUNDED: &[&str] = &[
    "md-lake-seal@.service",
    "md-archiver@.service",
    "crypto-lake-audit@.service",
];
const RUN_TIMEOUT: &str = "6h";

/// The `MemoryMax` of each unit (U6, design §8.5).
pub const MEMORY_MAX: &[(&str, &str)] = &[
    ("victoriametrics.service", "1G"),
    ("victorialogs.service", "1G"),
    ("vmalert-metrics.service", "200M"),
    ("vmalert-logs.service", "200M"),
    ("alertmanager.service", "100M"),
    ("grafana.service", "512M"),
    ("md-collector@.service", "1G"),
    ("md-lake-seal@.service", "1G"),
    ("md-archiver@.service", "4G"),
    ("crypto-lake-audit@.service", "4G"),
];

/// Timers and their `OnCalendar` (U5).
const TIMERS: &[(&str, &str)] = &[
    ("md-lake-seal@.timer", "*-*-* *:05:00 UTC"),
    ("md-archiver@.timer", "*-*-* 01:10:00 UTC"),
    ("crypto-lake-audit@.timer", "*-*-* 02:30:00 UTC"),
];

/// Daily jobs, which run one at a time under one lock (U7).
const DAILY: &[&str] = &["md-archiver@.service", "crypto-lake-audit@.service"];
const LOCK: &str = "/usr/bin/flock /run/lock/hl-oms-daily.lock ";

/// The audit unit, its script, and the directories that verify and the
/// auditor write inside the read-only lake (U12, U16).
const AUDIT: &str = "crypto-lake-audit@.service";
const AUDIT_SCRIPT: &str = "crypto-lake-audit.sh";
const L0: &str = "/mnt/nvme/lake/crypto/l0";
const VERIFY_DIR: &str = "/mnt/nvme/lake/crypto/l0/_verify";
const AUDIT_DIR: &str = "/mnt/nvme/lake/crypto/audit";
/// The KR pipeline's checkouts (with `-official` and any other suffix).
const KR_CHECKOUT: &str = "/mnt/nvme/gitproject/quant-data-platform";
/// The auditor's state file in the script: the unit's state directory.
const AUDIT_STATE: &str = "--state \"$STATE_DIRECTORY/audit-$venue.json\"";
/// The auditor pushes to the local VictoriaMetrics only.
const AUDIT_VM_URL: &str = "--vm-url http://127.0.0.1:8428";
/// The script lines that decide which dates are verified again and what the
/// unit's status says (D4), each once. The script has no test of its own in
/// CI (D11 runs it by hand), so its text is pinned (U12).
const AUDIT_SCRIPT_LINES: &[&str] = &[
    "while [ \"$i\" -le 9 ]; do",
    "future=$(find \"$stamp\" -newermt \"@$now\" -print) || future=1",
    "if ! changed=$(sealed \"$d\" -cnewer \"$stamp\"); then",
    "rm -f \"$stamp.new\"",
    "exit 5",
];
/// The stamp is taken, then a second passes, then verify lists the files,
/// then the stamp is kept: these lines once each, in this order.
const AUDIT_STAMP_ORDER: &[&str] = &[
    "if ! touch \"$stamp.new\"; then",
    "sleep 1",
    "if \"$MD_LAKE\" verify --venue \"$venue\" --lake \"$LAKE\" --date \"$d\" --json >/dev/null; then",
    "if ! mv -f \"$stamp.new\" \"$stamp\"; then",
];
/// The lake the audit unit's path lines name; the script works in `$LAKE`.
const AUDIT_LAKE: &str = "/mnt/nvme/lake";
/// The audit unit's two path lines, exactly.
const AUDIT_PATHS: &[(&str, &str, Want)] = &[
    (
        "Service",
        "ReadOnlyPaths",
        Is("/mnt/nvme/lake/crypto/l0 /mnt/nvme/lake/crypto/l1"),
    ),
    (
        "Service",
        "ReadWritePaths",
        Is("/mnt/nvme/lake/crypto/l0/_verify /mnt/nvme/lake/crypto/audit"),
    ),
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
    // The older name, and the keys in [Service] (still read there), would
    // set a limit again.
    ("Unit", "StartLimitInterval", Absent),
    ("Service", "StartLimitIntervalSec", Absent),
    ("Service", "StartLimitInterval", Absent),
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
    ("Service", "MemoryHigh", Absent),
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
    u12_audit_checkout_and_state(t, p, out);
    u13_venues(t, p, out);
    u14_install_map(t, p, out);
    u15_identity(p, out);
    u16_lake_writes(p, out);
    u17_exec_text(p, out);
    u18_stack_settings(p, out);
    u19_bounded_runs(p, out);
}

/// A systemd size: bytes, or a number with `K`, `M` or `G` (base 1024).
pub fn size(v: &str) -> Option<u64> {
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

/// The words of a command or `Environment=` line as systemd splits them: at
/// whitespace, with `"…"` and `'…'` quoting.
fn words(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut word = String::new();
    let mut quote = None;
    let mut started = false;
    for c in line.chars() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => word.push(c),
            None if c == '"' || c == '\'' => {
                quote = Some(c);
                started = true;
            }
            None if c.is_whitespace() => {
                if started {
                    out.push(std::mem::take(&mut word));
                    started = false;
                }
            }
            None => {
                word.push(c);
                started = true;
            }
        }
    }
    if started {
        out.push(word);
    }
    out
}

/// Every value of the command-line flag `name` (written without dashes), as
/// the Go, kingpin and clap parsers read it: one or two dashes, the value
/// after `=` or in the next word. A flag at the end with no value reads as
/// empty.
pub fn flag_values(u: &Unit, name: &str) -> Vec<String> {
    let mut found = Vec::new();
    for line in exec_lines(u) {
        let w = words(line);
        for (i, word) in w.iter().enumerate() {
            let Some(flag) = word.strip_prefix("--").or_else(|| word.strip_prefix('-')) else {
                continue;
            };
            match flag.split_once('=') {
                Some((n, v)) if n == name => found.push(v.to_string()),
                None if flag == name => {
                    found.push(w.get(i + 1).cloned().unwrap_or_default());
                }
                _ => {}
            }
        }
    }
    found
}

/// `NAME=value` pairs of the unit's `Environment=` lines, in order; an empty
/// `Environment=` clears the ones before it, as in systemd.
fn environment(u: &Unit) -> Vec<(String, String)> {
    let mut env = Vec::new();
    for line in u.ini.values("Service", "Environment") {
        if line.trim().is_empty() {
            env.clear();
        }
        env.extend(words(line).into_iter().filter_map(|kv| {
            kv.split_once('=')
                .map(|(k, v)| (k.to_string(), v.to_string()))
        }));
    }
    env
}

/// Every value the unit's environment gives `name`.
fn env_values<'a>(env: &'a [(String, String)], name: &str) -> Vec<&'a str> {
    env.iter()
        .filter(|(k, _)| k == name)
        .map(|(_, v)| v.as_str())
        .collect()
}

/// `v` with a leading `${NAME}` or `$NAME` replaced by the last value the
/// unit sets for it; `Err(NAME)` when the unit does not set it.
fn resolve<'a>(env: &'a [(String, String)], v: &'a str) -> Result<&'a str, &'a str> {
    match v
        .strip_prefix("${")
        .and_then(|r| r.strip_suffix('}'))
        .or_else(|| v.strip_prefix('$'))
    {
        Some(name) => env_values(env, name).last().copied().ok_or(name),
        None => Ok(v),
    }
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

pub fn loopback_port(v: &str) -> bool {
    v.strip_prefix("127.0.0.1:")
        .is_some_and(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
}

fn u8_listeners(p: &Parsed, out: &mut Vec<Finding>) {
    // Every spelling of a listener flag counts: the tools accept a repeated
    // one as a second listener (VictoriaMetrics, Alertmanager) or let the
    // last value win.
    for u in &p.units {
        for flag in ["httpListenAddr", "web.listen-address"] {
            for v in flag_values(u, flag) {
                if !loopback_port(&v) {
                    finding(
                        out,
                        &u.path,
                        "U8",
                        format!("-{flag} {v} must be 127.0.0.1:<port>"),
                    );
                }
            }
        }
    }
    // A listener without the flag binds every interface: each stack
    // listener names its address once.
    for &(name, flag) in &[
        ("victoriametrics.service", "httpListenAddr"),
        ("victorialogs.service", "httpListenAddr"),
        ("vmalert-metrics.service", "httpListenAddr"),
        ("vmalert-logs.service", "httpListenAddr"),
        ("alertmanager.service", "web.listen-address"),
    ] {
        for u in named(p, name) {
            if flag_values(u, flag).len() != 1 {
                finding(out, &u.path, "U8", format!("-{flag} must be given once"));
            }
        }
    }
    for u in named(p, "alertmanager.service") {
        if flag_values(u, "cluster.listen-address") != [""] {
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
        let mut values: Vec<(String, String)> = Vec::new();
        for flag in STORE_FLAGS {
            values.extend(
                flag_values(u, flag)
                    .into_iter()
                    .map(|v| (format!("-{flag} "), v)),
            );
        }
        values.extend(
            env_values(&env, "LAKE")
                .into_iter()
                .map(|v| ("LAKE=".to_string(), v.to_string())),
        );
        for (what, v) in &values {
            match resolve(&env, v) {
                Err(name) => finding(
                    out,
                    &u.path,
                    "U9",
                    format!("{what}{v}: the unit does not set {name}"),
                ),
                Ok(resolved) if !resolved.starts_with("/mnt/nvme/") => finding(
                    out,
                    &u.path,
                    "U9",
                    format!("{what}{v} = {resolved} must be under /mnt/nvme/"),
                ),
                Ok(_) => {}
            }
        }
    }
    // Without the flag the tool writes to a relative default.
    for &(name, flag) in STORES {
        for u in named(p, name) {
            if flag_values(u, flag).len() != 1 {
                finding(out, &u.path, "U9", format!("-{flag} must be given once"));
            }
        }
    }
}

/// (unit, rule, flags (without dashes) with an exact value, flags that need
/// some value).
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
            &[("retentionPeriod", "1y")],
            &["memory.allowedBytes"],
        ),
        (
            "victorialogs.service",
            "U10",
            &[("retentionPeriod", "1y")],
            &[],
        ),
        (
            "vmalert-metrics.service",
            "U11",
            &[
                ("datasource.url", "http://127.0.0.1:8428"),
                ("evaluationInterval", "10s"),
            ],
            &["notifier.url", "remoteWrite.url"],
        ),
        (
            "vmalert-logs.service",
            "U11",
            &[
                ("datasource.url", "http://127.0.0.1:9428"),
                ("rule.defaultRuleType", "vlogs"),
                ("evaluationInterval", "30s"),
            ],
            &["notifier.url"],
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
                        format!("-{flag}={want} must be given once"),
                    );
                }
            }
            for flag in any {
                if !matches!(flag_values(u, flag).as_slice(), [v] if !v.is_empty()) {
                    finding(
                        out,
                        &u.path,
                        rule,
                        format!("-{flag} must be given once with a value"),
                    );
                }
            }
        }
    }
}

fn u12_audit_checkout_and_state(t: &Tree, p: &Parsed, out: &mut Vec<Finding>) {
    for (path, text) in t.files.iter().filter(|(k, _)| k.starts_with("deploy/")) {
        if text.contains(KR_CHECKOUT) {
            finding(
                out,
                path,
                "U12",
                format!("names {KR_CHECKOUT}: the audit runs from its own qdp checkout"),
            );
        }
    }
    let script = format!("{OBS}bin/{AUDIT_SCRIPT}");
    match t.files.get(&script) {
        None => finding(out, &script, "U12", "missing"),
        Some(text) => {
            let lines: Vec<&str> = text.lines().map(str::trim).collect();
            if lines.iter().filter(|l| l.starts_with("venue=")).count() != 1
                || !lines.contains(&"venue=$1")
            {
                finding(
                    out,
                    &script,
                    "U12",
                    "the venue is set once, as venue=$1 (the unit's instance)",
                );
            }
            let states: Vec<&str> = text
                .match_indices("--state")
                .map(|(i, _)| &text[i..])
                .collect();
            if states.is_empty() || !states.iter().all(|r| r.starts_with(AUDIT_STATE)) {
                finding(
                    out,
                    &script,
                    "U12",
                    format!("every --state must be {AUDIT_STATE}, outside the lake"),
                );
            }
            if text.contains("STATE_DIRECTORY=") {
                finding(
                    out,
                    &script,
                    "U12",
                    "STATE_DIRECTORY is set by systemd only",
                );
            }
            let urls: Vec<&str> = text
                .match_indices("--vm-url")
                .map(|(i, _)| &text[i..])
                .collect();
            if urls.is_empty()
                || !urls.iter().all(|r| {
                    r.strip_prefix(AUDIT_VM_URL)
                        .is_some_and(|rest| rest.chars().next().is_none_or(char::is_whitespace))
                })
            {
                finding(
                    out,
                    &script,
                    "U12",
                    format!("every --vm-url must be {AUDIT_VM_URL}"),
                );
            }
            let mut at = Vec::new();
            for &line in AUDIT_SCRIPT_LINES.iter().chain(AUDIT_STAMP_ORDER) {
                let found: Vec<usize> = (0..lines.len()).filter(|&i| lines[i] == line).collect();
                if found.len() != 1 {
                    finding(
                        out,
                        &script,
                        "U12",
                        format!("must hold the line {line:?} once (found {})", found.len()),
                    );
                }
                if AUDIT_STAMP_ORDER.contains(&line) {
                    at.push(found.first().copied());
                }
            }
            if !at.windows(2).all(|w| w[0] < w[1]) {
                finding(
                    out,
                    &script,
                    "U12",
                    format!("the lines {AUDIT_STAMP_ORDER:?} must come in this order"),
                );
            }
        }
    }
    for u in required(p, AUDIT, "U12", out) {
        expect(
            u,
            "U12",
            ("Service", "StateDirectory", Is("qdp-crypto-audit")),
            out,
        );
        if !env_values(&environment(u), "STATE_DIRECTORY").is_empty() {
            finding(
                out,
                &u.path,
                "U12",
                "STATE_DIRECTORY is set by systemd only",
            );
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
pub fn install_entries(p: &Parsed, out: &mut Vec<Finding>) -> Vec<(String, String, String)> {
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
            if !dsts.iter().any(|d| glob(want, d)) {
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
    for (name, _) in STACK {
        for u in named(p, name) {
            expect(u, "U15", ("Service", "User", Is("hlobs")), out);
        }
    }
}

/// The paths of a `ReadOnlyPaths=`/`ReadWritePaths=` key, without systemd's
/// `-` (ignore if missing) and `+` prefixes and a trailing `/`.
fn path_list(u: &Unit, key: &str) -> Vec<String> {
    u.ini
        .values("Service", key)
        .into_iter()
        .flat_map(words)
        .map(|w| {
            let p = w.trim_start_matches(['-', '+']);
            match p.trim_end_matches('/') {
                "" => "/".to_string(),
                q => q.to_string(),
            }
        })
        .collect()
}

/// `dir` is `path` or one of its parents.
fn covers(dir: &str, path: &str) -> bool {
    dir == "/" || dir == path || path.starts_with(&format!("{dir}/"))
}

/// Whether a command line runs `md-lake verify` or the audit script.
fn runs(u: &Unit) -> (bool, bool) {
    let (mut verify, mut audit) = (false, false);
    for line in exec_lines(u) {
        let w = words(line);
        for (i, word) in w.iter().enumerate() {
            let name = file_name(word);
            verify |= name == "md-lake" && w.get(i + 1).is_some_and(|n| n == "verify");
            audit |= name == AUDIT_SCRIPT;
        }
    }
    (verify, audit)
}

fn u16_lake_writes(p: &Parsed, out: &mut Vec<Finding>) {
    for u in &p.units {
        let (verify, audit) = runs(u);
        if !verify && !audit {
            continue;
        }
        let mut read_only = path_list(u, "ReadOnlyPaths");
        if u.ini.values("Service", "ProtectSystem").contains(&"strict") {
            read_only.push("/".to_string());
        }
        let writes = path_list(u, "ReadWritePaths");
        if read_only.iter().any(|d| covers(d, L0)) && !writes.iter().any(|w| w == VERIFY_DIR) {
            finding(
                out,
                &u.path,
                "U16",
                format!("md-lake verify writes {VERIFY_DIR}: list it in ReadWritePaths="),
            );
        }
        if audit && !writes.iter().any(|w| w == AUDIT_DIR) {
            finding(
                out,
                &u.path,
                "U16",
                format!("the auditor writes {AUDIT_DIR}: list it in ReadWritePaths="),
            );
        }
    }
    for u in named(p, AUDIT) {
        for &row in AUDIT_PATHS {
            expect(u, "U16", row, out);
        }
        if env_values(&environment(u), "LAKE") != [AUDIT_LAKE] {
            finding(
                out,
                &u.path,
                "U16",
                format!("LAKE is set once, as {AUDIT_LAKE}: the lake its path lines name"),
            );
        }
    }
}

/// The first `%` specifier of `line` that is not `%i`, `%I`, `%n` or `%%`.
fn bad_specifier(line: &str) -> Option<String> {
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        if c == '%' {
            match chars.next() {
                Some('i' | 'I' | 'n' | '%') => {}
                Some(x) => return Some(format!("%{x}")),
                None => return Some("%".to_string()),
            }
        }
    }
    None
}

fn u17_exec_text(p: &Parsed, out: &mut Vec<Finding>) {
    for u in &p.units {
        for line in exec_lines(u) {
            if line.contains("$(") || line.contains('`') {
                finding(
                    out,
                    &u.path,
                    "U17",
                    "an Exec* line runs no shell: no $( or backtick",
                );
            }
            if let Some(spec) = bad_specifier(line) {
                finding(
                    out,
                    &u.path,
                    "U17",
                    format!("{spec} in an Exec* line: only %i, %I, %n and %%"),
                );
            }
        }
    }
}

fn u18_stack_settings(p: &Parsed, out: &mut Vec<Finding>) {
    for &(name, writes) in STACK {
        for u in required(p, name, "U18", out) {
            for &row in STACK_SETTINGS {
                expect(u, "U18", row, out);
            }
            expect(u, "U18", ("Service", "ReadWritePaths", writes), out);
        }
    }
}

fn u19_bounded_runs(p: &Parsed, out: &mut Vec<Finding>) {
    for name in BOUNDED {
        for u in required(p, name, "U19", out) {
            expect(
                u,
                "U19",
                ("Service", "TimeoutStartSec", Is(RUN_TIMEOUT)),
                out,
            );
            expect(u, "U19", ("Service", "TimeoutSec", Absent), out);
        }
    }
    for u in named(p, "md-archiver@.service") {
        // L1 is never deleted; one topic-day in memory per thread.
        let env = environment(u);
        if env_values(&env, "RETAIN_ARGS") != [""] {
            finding(
                out,
                &u.path,
                "U19",
                "Environment= must set an empty RETAIN_ARGS= once",
            );
        }
        let threads = flag_values(u, "threads");
        if !matches!(threads.as_slice(), [v] if resolve(&env, v) == Ok("1")) {
            finding(out, &u.path, "U19", "--threads must be given once as 1");
        }
        if !flag_values(u, "retain-l1-days").is_empty() {
            finding(out, &u.path, "U19", "--retain-l1-days must not be given");
        }
    }
    // Every value the check reads comes from the unit file itself.
    for u in &p.units {
        if !u.ini.any("EnvironmentFile").is_empty() {
            finding(
                out,
                &u.path,
                "U19",
                "no EnvironmentFile=: the check cannot read its values",
            );
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
    // The older name, or the key again in [Service], brings a limit back.
    for to in [
        "StartLimitIntervalSec=0\nStartLimitInterval=10s\n",
        "StartLimitIntervalSec=0\n\n[Service]\nStartLimitIntervalSec=10s\n[Unit]\n",
        "StartLimitIntervalSec=0\n\n[Service]\nStartLimitInterval=10s\n[Unit]\n",
    ] {
        plant(
            "md-collector@.service",
            "StartLimitIntervalSec=0\n",
            to,
            &["U3"],
        );
    }
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
    // A throttle below the cap would stall it before the kernel protects it.
    plant(
        "md-collector@.service",
        "IOWeight=1000\n",
        "IOWeight=1000\nMemoryHigh=64M\n",
        &["U3"],
    );
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
    plant(
        "crypto-lake-audit@.service",
        "MemoryMax=4G\n",
        "MemoryMax=8G\n",
        &["U6"],
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
    // and its run bound are missing.
    assert_rules(&crate::check(&t), &["U2", "U6", "U14", "U19"]);
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
    // The audit runs after the archiver's 01:10.
    plant("crypto-lake-audit@.timer", "02:30:00", "01:00:00", &["U5"]);
    plant("crypto-lake-audit@.timer", "Persistent=true\n", "", &["U5"]);
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
    // The audit takes the same lock, before the script.
    plant(
        "crypto-lake-audit@.service",
        "ExecStart=/usr/bin/flock /run/lock/hl-oms-daily.lock ",
        "ExecStart=",
        &["U7"],
    );
    plant(
        "crypto-lake-audit@.service",
        "Nice=10\n",
        "Nice=10\nExecStartPre=/usr/local/bin/md-lake verify --venue %i\n",
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
    // With the command go its --lake (U9) and --threads (U19).
    assert_rules(&crate::check(&t), &["U7", "U9", "U19"]);
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
    // The tools read a flag with one or two dashes, `name=value` or
    // `name value`, and quoted: a second listener in another spelling.
    plant(
        "victoriametrics.service",
        "-httpListenAddr=127.0.0.1:8428",
        "-httpListenAddr=127.0.0.1:8428 --httpListenAddr=0.0.0.0:8429",
        &["U8"],
    );
    plant(
        "victoriametrics.service",
        "-httpListenAddr=127.0.0.1:8428",
        "-httpListenAddr=127.0.0.1:8428 \"-httpListenAddr=0.0.0.0:8429\"",
        &["U8"],
    );
    plant(
        "alertmanager.service",
        "--web.listen-address=127.0.0.1:9093",
        "--web.listen-address=127.0.0.1:9093 --web.listen-address 0.0.0.0:9094",
        &["U8"],
    );
    plant(
        "alertmanager.service",
        "--cluster.listen-address=",
        "--cluster.listen-address= --cluster.listen-address 0.0.0.0:9094",
        &["U8"],
    );
    // Every value counts, also in a unit with no listener rule of its own.
    let mut t = Tree::real();
    let text = "[Unit]\nDescription=x\n\n[Service]\nType=simple\nMemoryMax=1G\nExecStart=/usr/local/bin/x -httpListenAddr=127.0.0.1:1 --httpListenAddr 0.0.0.0:2\n";
    add_installed(
        &mut t,
        "systemd/extra.service",
        text,
        "/etc/systemd/system/extra.service",
        "0644",
    );
    assert_rules(&crate::check(&t), &["U8"]);
    // The same loopback address in another spelling passes.
    plant(
        "victoriametrics.service",
        "-httpListenAddr=127.0.0.1:8428",
        "--httpListenAddr 127.0.0.1:8428",
        &[],
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
    // Every store names its path once, in any spelling; without it the
    // tool writes to a relative default.
    for (name, from, to) in [
        (
            "victoriametrics.service",
            " -storageDataPath=/mnt/nvme/observability/victoria-metrics",
            "",
        ),
        (
            "victoriametrics.service",
            "-storageDataPath=/mnt/nvme/observability/victoria-metrics",
            "--storageDataPath=/var/lib/vm",
        ),
        (
            "victoriametrics.service",
            "-storageDataPath=/mnt/nvme/observability/victoria-metrics",
            "-storageDataPath /var/lib/vm",
        ),
        (
            "victorialogs.service",
            "-storageDataPath=/mnt/nvme/observability/victoria-logs",
            "-storageDataPath=/mnt/nvme/observability/victoria-logs -storageDataPath=/mnt/nvme/x",
        ),
        (
            "alertmanager.service",
            " --storage.path=/mnt/nvme/observability/alertmanager",
            "",
        ),
        (
            "alertmanager.service",
            "--storage.path=/mnt/nvme/observability/alertmanager",
            "--storage.path /var/lib/am",
        ),
        ("md-lake-seal@.service", " --lake ${LAKE}", ""),
        // An empty Environment= clears the variables before it.
        (
            "md-archiver@.service",
            "Environment=LAKE=/mnt/nvme/lake\n",
            "Environment=LAKE=/mnt/nvme/lake\nEnvironment=\n",
        ),
    ] {
        plant(name, from, to, &["U9"]);
    }
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
    // A listed venue also needs its liveness rules (R6).
    let mut t = Tree::real();
    t.set(&venues, "venues = [\"bybit\", \"okx\"]\n");
    assert_rules(&crate::check(&t), &["R6", "U13"]);
    for text in [
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
    // The same entry twice: only the second dst is the finding.
    let mut t = Tree::real();
    let map = format!("{}{entry}", t.files[INSTALL]);
    t.set(INSTALL, &map);
    let f = crate::check(&t);
    assert_rules(&f, &["U14"]);
    assert!(f.len() == 1 && f[0].text.contains("given twice"), "{f:?}");
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
    // Grafana would read another file than the one the check reads (G1
    // pins Grafana's command line too).
    plant(
        "grafana.service",
        "--config /etc/hl-oms-obs/grafana/grafana.ini",
        "--config /etc/hl-oms-obs/grafana/custom.ini",
        &["U14", "G1"],
    );
    // No path is pending: an install entry dropped leaves the unit's path
    // without a dst.
    let mut t = Tree::real();
    t.replace(
        INSTALL,
        "[[file]]\nsrc = \"grafana/grafana.ini\"\ndst = \"/etc/hl-oms-obs/grafana/grafana.ini\"\nmode = \"0644\"\n",
        "",
    );
    let f = crate::check(&t);
    assert!(
        f.iter()
            .any(|x| x.rule == "U14" && x.file == unit("grafana.service")),
        "{f:?}"
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
fn stack_units_keep_their_restart_and_sandbox_settings() {
    for (name, from, to) in [
        ("victoriametrics.service", "Restart=on-failure\n", ""),
        (
            "victorialogs.service",
            "Restart=on-failure\n",
            "Restart=on-failure\nRestart=no\n",
        ),
        ("alertmanager.service", "RestartSec=5\n", "RestartSec=60\n"),
        ("grafana.service", "ProtectSystem=strict\n", ""),
        (
            "grafana.service",
            "ProtectSystem=strict\n",
            "ProtectSystem=full\n",
        ),
        ("vmalert-logs.service", "NoNewPrivileges=yes\n", ""),
        (
            "vmalert-metrics.service",
            "ProtectHome=yes\n",
            "ProtectHome=no\n",
        ),
        ("victorialogs.service", "PrivateTmp=yes\n", ""),
        // Each store writes its own directory only; vmalert writes none.
        (
            "victoriametrics.service",
            "ReadWritePaths=/mnt/nvme/observability/victoria-metrics\n",
            "ReadWritePaths=/mnt/nvme\n",
        ),
        (
            "alertmanager.service",
            "ReadWritePaths=/mnt/nvme/observability/alertmanager\n",
            "",
        ),
        (
            "vmalert-metrics.service",
            "PrivateTmp=yes\n",
            "PrivateTmp=yes\nReadWritePaths=/mnt/nvme/lake\n",
        ),
        // A restart of a store must not stop vmalert.
        (
            "vmalert-metrics.service",
            "After=victoriametrics.service",
            "Requires=victoriametrics.service\nAfter=victoriametrics.service",
        ),
        (
            "vmalert-logs.service",
            "After=victoriametrics.service",
            "BindsTo=victorialogs.service\nAfter=victoriametrics.service",
        ),
        (
            "vmalert-logs.service",
            "After=victoriametrics.service",
            "PartOf=victorialogs.service\nAfter=victoriametrics.service",
        ),
    ] {
        plant(name, from, to, &["U18"]);
    }
}

#[test]
fn the_seal_and_archiver_runs_are_bounded() {
    for (name, from, to) in [
        // A oneshot has no start timeout by default: a hung run holds the
        // daily lock, or stops sealing, for good.
        ("md-archiver@.service", "TimeoutStartSec=6h\n", ""),
        ("md-lake-seal@.service", "TimeoutStartSec=6h\n", ""),
        ("crypto-lake-audit@.service", "TimeoutStartSec=6h\n", ""),
        (
            "crypto-lake-audit@.service",
            "TimeoutStartSec=6h\n",
            "TimeoutStartSec=6h\nTimeoutSec=0\n",
        ),
        (
            "md-lake-seal@.service",
            "TimeoutStartSec=6h\n",
            "TimeoutStartSec=6h\nTimeoutSec=infinity\n",
        ),
        (
            "md-archiver@.service",
            "TimeoutStartSec=6h\n",
            "TimeoutStartSec=24h\n",
        ),
        // One topic-day per thread: four threads pass the 4G cap.
        (
            "md-archiver@.service",
            "Environment=THREADS=1\n",
            "Environment=THREADS=4\n",
        ),
        (
            "md-archiver@.service",
            "Environment=THREADS=1\n",
            "Environment=THREADS=1 THREADS=2\n",
        ),
        (
            "md-archiver@.service",
            "--threads ${THREADS}",
            "--threads 4",
        ),
        ("md-archiver@.service", " --threads ${THREADS}", ""),
        // L1 deletion stays off.
        (
            "md-archiver@.service",
            "Environment=RETAIN_ARGS=\n",
            "Environment=RETAIN_ARGS=--retain-l1-days 14\n",
        ),
        (
            "md-archiver@.service",
            "Environment=RETAIN_ARGS=\n",
            "Environment=\"RETAIN_ARGS=--retain-l1-days 14\"\n",
        ),
        (
            "md-archiver@.service",
            "$RETAIN_ARGS",
            "--retain-l1-days=14 $RETAIN_ARGS",
        ),
        // Values from a file the check cannot read.
        (
            "md-archiver@.service",
            "Nice=10\n",
            "Nice=10\nEnvironmentFile=-/etc/default/md-archiver\n",
        ),
        (
            "md-collector@.service",
            "TimeoutStopSec=30\n",
            "TimeoutStopSec=30\nEnvironmentFile=/etc/default/md-collector\n",
        ),
    ] {
        plant(name, from, to, &["U19"]);
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

#[test]
fn the_audit_unit_can_write_its_verify_results() {
    let audit = "crypto-lake-audit@.service";
    let rw = "ReadWritePaths=/mnt/nvme/lake/crypto/l0/_verify /mnt/nvme/lake/crypto/audit\n";
    for to in [
        // verify could not write its result inside the read-only l0.
        "ReadWritePaths=/mnt/nvme/lake/crypto/audit\n",
        // The auditor could not write its result.
        "ReadWritePaths=/mnt/nvme/lake/crypto/l0/_verify\n",
        "",
        // Wider than the two directories the runs write.
        "ReadWritePaths=/mnt/nvme/lake\n",
        "ReadWritePaths=/mnt/nvme/lake/crypto/l0/_verify /mnt/nvme/lake/crypto/audit\nReadWritePaths=/mnt/nvme/lake/crypto/l1\n",
    ] {
        plant(audit, rw, to, &["U16"]);
    }
    // The read-only lines are exact too: L1 stays read-only.
    plant(
        audit,
        "ReadOnlyPaths=/mnt/nvme/lake/crypto/l0 /mnt/nvme/lake/crypto/l1\n",
        "ReadOnlyPaths=/mnt/nvme/lake/crypto/l0\n",
        &["U16"],
    );
    // Any unit that runs md-lake verify under a read-only l0, or a parent of
    // it, needs the _verify directory (the general rule, not the audit
    // unit's pinned lines).
    let verify = |extra: &str| {
        format!(
            "[Unit]\nDescription=x\n\n[Service]\nType=simple\nMemoryMax=1G\n{extra}ExecStart=/usr/local/bin/md-lake verify --venue %i --lake /mnt/nvme/lake --date 2026-12-01 --json\n"
        )
    };
    for (extra, rules) in [
        ("ReadOnlyPaths=/mnt/nvme/lake\n", &["U16"][..]),
        ("ReadOnlyPaths=-/mnt/nvme/lake/crypto/l0\n", &["U16"][..]),
        ("ReadOnlyPaths=/mnt\n", &["U16"][..]),
        // ProtectSystem=strict makes the whole file system read-only.
        ("ProtectSystem=strict\n", &["U16"][..]),
        (
            "ReadOnlyPaths=/mnt/nvme/lake\nReadWritePaths=/mnt/nvme/lake/crypto/l0/_verify\n",
            &[][..],
        ),
        // Nothing read-only: nothing to open up.
        ("", &[][..]),
        ("ReadOnlyPaths=/mnt/nvme/lake/crypto/l1\n", &[][..]),
    ] {
        let mut t = Tree::real();
        add_installed(
            &mut t,
            "systemd/md-lake-verify@.service",
            &verify(extra),
            "/etc/systemd/system/md-lake-verify@.service",
            "0644",
        );
        let f = crate::check(&t);
        assert_rules(&f, rules);
    }
    // Another unit that runs the audit script needs the audit directory
    // too, not only the audit unit's pinned lines.
    let script = |rw: &str| {
        format!(
            "[Unit]\nDescription=x\n\n[Service]\nType=simple\nMemoryMax=1G\nReadOnlyPaths=/mnt/nvme/lake/crypto/l0\n{rw}ExecStart=/usr/local/lib/hl-oms-obs/crypto-lake-audit.sh %i\n"
        )
    };
    for (rw, rules) in [
        (
            "ReadWritePaths=/mnt/nvme/lake/crypto/l0/_verify\n",
            &["U16"][..],
        ),
        (
            "ReadWritePaths=/mnt/nvme/lake/crypto/l0/_verify /mnt/nvme/lake/crypto/audit\n",
            &[][..],
        ),
    ] {
        let mut t = Tree::real();
        add_installed(
            &mut t,
            "systemd/lake-audit-copy@.service",
            &script(rw),
            "/etc/systemd/system/lake-audit-copy@.service",
            "0644",
        );
        let f = crate::check(&t);
        assert_rules(&f, rules);
    }
    // The script reads and writes $LAKE: the lake the two path lines name.
    let lake = "Environment=LAKE=/mnt/nvme/lake\n";
    for to in [
        "Environment=LAKE=/mnt/nvme/other\n",
        "Environment=LAKE=/mnt/nvme/lake\nEnvironment=LAKE=/mnt/nvme/other\n",
    ] {
        plant(audit, lake, to, &["U16"]);
    }
}

#[test]
fn the_audit_script_keeps_its_verify_rules() {
    // The lines that decide which dates are verified again and what the
    // unit's status says (D4); each change below once passed every test.
    let script = format!("{OBS}bin/crypto-lake-audit.sh");
    let touch = "\tif ! touch \"$stamp.new\"; then\n\t\techo \"crypto-lake-audit: cannot write $stamp.new\" >&2\n\t\tverify_failed=1\n\t\tcontinue\n\tfi\n";
    let verify = "\tif \"$MD_LAKE\" verify --venue \"$venue\" --lake \"$LAKE\" --date \"$d\" --json >/dev/null; then\n";
    for (from, to) in [
        // mtime instead of ctime: a renamed file is missed.
        ("-cnewer \"$stamp\"", "-newer \"$stamp\""),
        // A shorter window than the day seal's look-back.
        ("\"$i\" -le 9", "\"$i\" -le 2"),
        // Coarse timestamps: a file sealed in the same second as the stamp.
        ("\tsleep 1\n", ""),
        // The stamp taken after verify listed the files (the race).
        (
            &format!("{touch}\tsleep 1\n{verify}"),
            &format!("\tsleep 1\n{verify}{touch}"),
        ),
        // A failed verify replaces the stamp.
        (
            "\t\trm -f \"$stamp.new\"\n",
            "\t\tmv -f \"$stamp.new\" \"$stamp\"\n",
        ),
        // The new stamp removed before verify: mv then fails every day.
        ("\tsleep 1\n", "\tsleep 1\n\trm -f \"$stamp.new\"\n"),
        // A failed verify leaves the unit green.
        ("\texit 5\n", "\texit 0\n"),
        // A stamp from the future hides the files sealed since.
        ("-newermt \"@$now\"", "-newermt \"@0\""),
        // Pushes off loopback.
        (
            "--vm-url http://127.0.0.1:8428",
            "--vm-url http://10.0.0.1:8428",
        ),
        (
            "--vm-url http://127.0.0.1:8428",
            "--vm-url http://127.0.0.1:8428 --vm-url http://10.0.0.1:8428",
        ),
        (
            "--vm-url http://127.0.0.1:8428",
            "--vm-url http://127.0.0.1:84280",
        ),
    ] {
        let mut t = Tree::real();
        t.replace(&script, from, to);
        let f = crate::check(&t);
        assert_rules(&f, &["U12"]);
    }
}

#[test]
fn the_audit_never_points_at_the_kr_checkouts() {
    let audit = "crypto-lake-audit@.service";
    let script = format!("{OBS}bin/crypto-lake-audit.sh");
    let kr = "/mnt/nvme/gitproject/quant-data-platform";
    // The KR pipeline's checkouts, in the unit or any other deploy file.
    for to in [
        format!("Environment=QDP_CHECKOUT={kr}\n"),
        format!("Environment=QDP_CHECKOUT={kr}-official\n"),
    ] {
        plant(
            audit,
            "Environment=QDP_CHECKOUT=/opt/qdp-crypto\n",
            &to,
            &["U12"],
        );
    }
    let mut t = Tree::real();
    t.replace(&script, "venue=$1\n", &format!("venue=$1\ncd {kr}/x\n"));
    assert_rules(&crate::check(&t), &["U12"]);
    // A comment counts too: the path is not written anywhere under deploy/.
    plant(
        "md-archiver@.timer",
        "Persistent=true\n",
        &format!("Persistent=true\n# not {kr}\n"),
        &["U12"],
    );
    // The state file stays outside the lake: the unit's state directory, the
    // instance's venue.
    let state = "--state \"$STATE_DIRECTORY/audit-$venue.json\"";
    for (from, to) in [
        (state, "--state \"$LAKE/crypto/audit-$venue.json\""),
        (state, "--state \"$STATE_DIRECTORY/audit.json\""),
        (state, "--state=\"$STATE_DIRECTORY/audit-$venue.json\""),
        (state, ""),
        ("venue=$1\n", "venue=${1:-bybit}\n"),
        ("venue=$1\n", "venue=$1\nvenue=bybit\n"),
        ("venue=$1\n", "venue=$1\nSTATE_DIRECTORY=/mnt/nvme/lake\n"),
    ] {
        let mut t = Tree::real();
        t.replace(&script, from, to);
        let f = crate::check(&t);
        assert_rules(&f, &["U12"]);
    }
    plant(audit, "StateDirectory=qdp-crypto-audit\n", "", &["U12"]);
    plant(
        audit,
        "StateDirectory=qdp-crypto-audit\n",
        "StateDirectory=qdp-crypto-audit\nEnvironment=STATE_DIRECTORY=/mnt/nvme/lake/state\n",
        &["U12"],
    );
    plant(
        audit,
        "StateDirectory=qdp-crypto-audit\n",
        "StateDirectory=other\n",
        &["U12"],
    );
    // The script itself gone: U12 cannot see the state path, U14 its file.
    let mut t = Tree::real();
    t.remove(&script);
    assert_rules(&crate::check(&t), &["U12", "U14"]);
}

#[test]
fn an_exec_line_with_shell_substitution_fails() {
    let audit = "crypto-lake-audit@.service";
    let script = "/usr/local/lib/hl-oms-obs/crypto-lake-audit.sh %i";
    // systemd runs no shell: $( and backticks reach the program as text.
    for to in [
        "/usr/local/lib/hl-oms-obs/crypto-lake-audit.sh $(date +%%F)",
        "/usr/local/lib/hl-oms-obs/crypto-lake-audit.sh `date`",
        // A specifier other than %i, %I, %n and %% is expanded or refused.
        "/usr/local/lib/hl-oms-obs/crypto-lake-audit.sh %h",
        "/usr/local/lib/hl-oms-obs/crypto-lake-audit.sh %i %",
        "/usr/local/lib/hl-oms-obs/crypto-lake-audit.sh 100%",
    ] {
        plant(audit, script, to, &["U17"]);
    }
    plant(
        "md-archiver@.service",
        "Nice=10\n",
        "Nice=10\nExecStopPost=/bin/echo $(id -u)\n",
        &["U17"],
    );
    plant(
        "victoriametrics.service",
        "-retentionPeriod=1y",
        "-retentionPeriod=1y -x=%t",
        &["U17"],
    );
    // %%, %i, %I and %n pass.
    plant(
        audit,
        script,
        "/usr/local/lib/hl-oms-obs/crypto-lake-audit.sh %i %I %n 100%%",
        &[],
    );
    // Outside Exec* lines the rule does not apply.
    plant(
        audit,
        "Description=Lake verify and qdp audit for %i\n",
        "Description=Lake verify and qdp audit for %i on %H\n",
        &[],
    );
}

#[test]
fn the_audit_script_is_installed_executable() {
    let dst = "dst = \"/usr/local/lib/hl-oms-obs/crypto-lake-audit.sh\"";
    for (from, to) in [
        (
            &format!("{dst}\nmode = \"0755\""),
            &format!("{dst}\nmode = \"0644\"")[..],
        ),
        (
            &dst.to_string(),
            "dst = \"/etc/hl-oms-obs/bin/crypto-lake-audit.sh\"",
        ),
        // The unit's ExecStart path then has no dst either.
        (
            &dst.to_string(),
            "dst = \"/usr/local/lib/hl-oms-obs/audit.sh\"",
        ),
    ] {
        let mut t = Tree::real();
        t.replace(INSTALL, from, to);
        assert_rules(&crate::check(&t), &["U14"]);
    }
    // Run by sh: the first line says so.
    let script = format!("{OBS}bin/crypto-lake-audit.sh");
    for first in ["#!/bin/bash\n", "#!/usr/bin/env sh\n", "\n"] {
        let mut t = Tree::real();
        t.replace(&script, "#!/bin/sh\n", first);
        assert_rules(&crate::check(&t), &["P1"]);
    }
}
