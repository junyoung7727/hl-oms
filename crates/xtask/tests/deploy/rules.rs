//! Rule rules (ADR-035 D7): the vmalert rule files and the scrape config.
//! R1 every metric name is in the catalogue, R3 every label is known, R4
//! every function is allowed, R5 every alert has a severity and a summary
//! and one Watchdog exists, R6 the liveness, lake-gauge and audit-age rules
//! of every configured venue, R7 the design's lake, disk and journal rules and the
//! intervals of the groups that hold the pinned rules, R8 log groups are
//! `vlogs` and metric groups are not, R9 the scrape jobs, the job names the
//! rules use and the memory alert's caps; U8 also covers scrape targets.

use std::collections::BTreeSet;

use serde_json::Value;

use crate::catalogue::Catalogue;
use crate::expr::{self, FUNCTIONS};
use crate::tree::{Finding, Parsed, Tree, finding};
use crate::units::{MEMORY_MAX, flag_values, loopback_port, size};

const METRICS_DIR: &str = "deploy/observability/vmalert/metrics/";
const LOGS_DIR: &str = "deploy/observability/vmalert/logs/";
const SCRAPE: &str = "deploy/observability/victoriametrics/scrape.json";
const VENUES: &str = "deploy/observability/venues.toml";

const SEVERITIES: &[&str] = &["P1", "P2", "P3"];
const WATCHDOG: &str = "Watchdog";
/// R5: the Watchdog's file and group (R7 pins the group's interval).
const WATCHDOG_AT: (&str, &str) = ("system.rules.json", "watchdog");

/// R6: per configured venue, one alert of each name, with this severity,
/// in this file and group, no `for`, and this expression with `<V>`
/// replaced by the venue.
const PER_VENUE: &[PerVenue] = &[
    (
        "MdCollectorDown",
        "P1",
        "market-data.rules.json",
        "market-data",
        "time() - max by (venue) (md_collector_heartbeat_ts{venue=\"<V>\"}) > 60 or absent_over_time(md_collector_heartbeat_ts{venue=\"<V>\"}[2m])",
    ),
    (
        "MdLakeGaugesMissing",
        "P2",
        "market-data.rules.json",
        "market-data",
        "absent_over_time(lake_disk_free_bytes{venue=\"<V>\"}[10m]) or absent_over_time(lake_seal_lag_hours{venue=\"<V>\"}[10m])",
    ),
    (
        "LakeAuditStale",
        "P2",
        "lake.rules.json",
        "lake-audit",
        "time() - last_over_time(lake_audit_last_run_ts{venue=\"<V>\"}[3d]) > 129600 or absent_over_time(lake_audit_last_run_ts{venue=\"<V>\"}[36h])",
    ),
];

/// R6: the same, per trading venue (`trading` in `venues.toml`): the OMS's
/// liveness (design §8.3 B3), the projector that is its only witness, and
/// the journal gauge the days-left rules read (design R4 SHOULD 2).
const TRADING_PER_VENUE: &[PerVenue] = &[
    (
        "JournalDiskGaugeMissing",
        "P2",
        "oms.rules.json",
        "oms",
        "absent_over_time(journal_disk_free_bytes{venue=\"<V>\"}[5m])",
    ),
    (
        "OmsDown",
        "P1",
        "oms.rules.json",
        "oms",
        "max by (venue) (max_over_time(oms_up{venue=\"<V>\"}[30s])) == 0",
    ),
    (
        "ProjectorDown",
        "P2",
        "oms.rules.json",
        "oms",
        "absent_over_time(projector_caught_up{venue=\"<V>\"}[2m])",
    ),
];
/// R13: the file whose literal `("venue", "<V>")` labels the watcher's
/// series; a trading venue the watcher does not label has no watcher.
const WATCHER_METRICS: &str = "crates/watcher/src/metrics.rs";

/// R7: the design's lake and disk rules: (file, group, alert or record,
/// expr, for, severity).
type DesignRule = (
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    Option<&'static str>,
    Option<&'static str>,
);
const DESIGN: &[DesignRule] = &[
    (
        "lake.rules.json",
        "lake-recording",
        "lake_disk_days_left",
        "lake_disk_free_bytes / clamp_min(-deriv(lake_disk_free_bytes[24h]) * 86400, 1)",
        None,
        None,
    ),
    (
        "lake.rules.json",
        "lake",
        "LakeDiskDaysLeftLow",
        "lake_disk_days_left < 7 and on(venue) (time() - tfirst_over_time(lake_disk_free_bytes[24h])) > 43200",
        Some("10m"),
        Some("P2"),
    ),
    (
        "lake.rules.json",
        "lake",
        "LakeDiskDaysLeftCritical",
        "lake_disk_days_left < 2 and on(venue) (time() - tfirst_over_time(lake_disk_free_bytes[24h])) > 43200",
        Some("10m"),
        Some("P1"),
    ),
    (
        "lake.rules.json",
        "lake",
        "LakeL0WriteError",
        "max by (venue) (lake_l0_write_errors_total) > 0 or max by (venue) (increase(lake_l0_write_errors_total[10m])) > 0",
        None,
        Some("P1"),
    ),
    (
        "lake.rules.json",
        "lake",
        "LakeSealLag",
        "max by (venue) (lake_seal_lag_hours) > 2",
        None,
        Some("P2"),
    ),
    (
        "lake.rules.json",
        "lake-audit",
        "LakeAuditFailed",
        "max by (venue) (last_over_time(lake_audit_fail_total[26h]) - (last_over_time(lake_audit_fail_total[30d] offset 26h) or last_over_time(lake_audit_fail_total[26h]) * 0)) > 0",
        None,
        Some("P2"),
    ),
    // The journal file system (design R4 SHOULD 2): the lake rules' shape.
    (
        "oms.rules.json",
        "journal-recording",
        "journal_disk_days_left",
        "journal_disk_free_bytes / clamp_min(-deriv(journal_disk_free_bytes[24h]) * 86400, 1)",
        None,
        None,
    ),
    (
        "oms.rules.json",
        "oms",
        "JournalDiskDaysLeftLow",
        "journal_disk_days_left < 7 and on(venue) (time() - tfirst_over_time(journal_disk_free_bytes[24h])) > 43200",
        Some("10m"),
        Some("P2"),
    ),
    (
        "oms.rules.json",
        "oms",
        "JournalDiskDaysLeftCritical",
        "journal_disk_days_left < 2 and on(venue) (time() - tfirst_over_time(journal_disk_free_bytes[24h])) > 43200",
        Some("10m"),
        Some("P1"),
    ),
];
/// R7: the groups that hold an R6, R7 or Watchdog rule, and their
/// interval. `None`: no interval, or the default it stands for.
const GROUP_INTERVALS: &[(&str, &str, Option<&str>)] = &[
    ("market-data.rules.json", "market-data", Some("10s")),
    ("lake.rules.json", "lake-recording", Some("1m")),
    ("lake.rules.json", "lake", None),
    ("lake.rules.json", "lake-audit", Some("1m")),
    ("system.rules.json", "watchdog", None),
    ("oms.rules.json", "journal-recording", Some("1m")),
    ("oms.rules.json", "oms", None),
    ("oms.rules.json", "projector", None),
];
/// R12: the hold alert classes (design §8.3). Every reason of `HoldReason`
/// that `Core::holds(None)` returns is in exactly one class, and each class
/// is the reason list of one pinned alert. Latched: only a person clears
/// it. The gate (`unmonitored`): on at every start until the watcher's
/// first heartbeat. Passing: clears by itself. Operator: a person did it.
const LATCHED: &[&str] = &[
    "kill_latched",
    "watcher_mismatch",
    "venue_change",
    "reducing",
    "mismatch",
    "storage_low",
];
const GATE: &[&str] = &["unmonitored"];
const PASSING: &[&str] = &[
    "link_down",
    "resync",
    "in_doubt",
    "no_config",
    "venue_setting",
    "account_mode",
];
const OPERATOR: &[&str] = &["operator_account"];
/// Reasons only a strategy's holds carry (`core.rs` adds them for a given
/// strategy): the projector, which asks for the account, never pushes them.
const STRATEGY_ONLY: &[&str] = &["operator_strategy", "strategy_link"];
/// (alert, class, for, severity, gated on `oms_up` = 1). The passing
/// classes alert only for a running OMS: `oms_holds` keeps the journal's
/// last state while the OMS is down, and a planned stop leaves
/// `unmonitored` in it.
type HoldAlert = (&'static str, &'static [&'static str], Option<&'static str>, &'static str, bool);
const HOLD_ALERTS: &[HoldAlert] = &[
    ("OmsHoldLatched", LATCHED, None, "P1", false),
    ("OmsUnmonitored", GATE, Some("2m"), "P1", true),
    ("OmsHoldLasting", PASSING, Some("5m"), "P1", true),
    ("OmsOperatorHalt", OPERATOR, None, "P3", false),
];
const OMS_UP_GATE: &str = " and on(venue) max by (venue) (oms_up) == 1";
const MODEL: &str = "crates/oms-core/src/model.rs";

/// vmalert-metrics' `-evaluationInterval` (U11 pins it): what a group
/// without an interval is evaluated at.
const DEFAULT_INTERVAL: &str = "10s";

/// R9: the scrape jobs. Each reads the listener of its unit `<job>.service`.
pub const SCRAPE_JOBS: &[&str] = &[
    "victoriametrics",
    "victorialogs",
    "vmalert-metrics",
    "vmalert-logs",
    "alertmanager",
];
const SCRAPE_INTERVAL: &str = "15s";
/// R9: the alert that compares each scraped job's memory with its unit's
/// `MemoryMax`.
const MEMORY_ALERT: &str = "ObsMemoryNearCap";

/// One rule of a rule file.
pub struct Rule<'a> {
    pub file: &'a str,
    pub group: &'a str,
    /// A LogsQL rule: its expression is not scanned.
    pub vlogs: bool,
    pub v: &'a Value,
}

impl<'a> Rule<'a> {
    fn str(&self, k: &str) -> Option<&'a str> {
        self.v.get(k).and_then(Value::as_str)
    }
    pub fn alert(&self) -> Option<&'a str> {
        self.str("alert")
    }
    pub fn record(&self) -> Option<&'a str> {
        self.str("record")
    }
    pub fn expr(&self) -> Option<&'a str> {
        self.str("expr")
    }
    fn name(&self) -> &'a str {
        self.alert().or(self.record()).unwrap_or("")
    }
    fn label(&self, k: &str) -> Option<&'a str> {
        self.v
            .get("labels")
            .and_then(|l| l.get(k))
            .and_then(Value::as_str)
    }
    pub fn label_keys(&self) -> Vec<String> {
        self.v
            .get("labels")
            .and_then(Value::as_object)
            .map(|m| m.keys().cloned().collect())
            .unwrap_or_default()
    }
    fn summary(&self) -> Option<&'a str> {
        self.v
            .get("annotations")
            .and_then(|a| a.get("summary"))
            .and_then(Value::as_str)
    }
}

/// `Some(true)` for a log rule file, `Some(false)` for a metric rule file:
/// the `.json` files directly in the two directories vmalert reads.
fn rule_file(path: &str) -> Option<bool> {
    for (dir, logs) in [(METRICS_DIR, false), (LOGS_DIR, true)] {
        if let Some(rest) = path.strip_prefix(dir) {
            return (!rest.contains('/') && rest.ends_with(".json")).then_some(logs);
        }
    }
    None
}

pub fn norm(e: &str) -> String {
    e.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Every well-formed rule of the rule files, and the R5 and R8 findings of
/// their shape.
pub fn collect(p: &Parsed) -> (Vec<Rule<'_>>, Vec<Finding>) {
    let (mut rules, mut out) = (Vec::new(), Vec::new());
    let mut names = BTreeSet::new();
    for (path, doc) in &p.json {
        let Some(logs) = rule_file(path) else {
            continue;
        };
        let Some(groups) = doc.get("groups").and_then(Value::as_array) else {
            finding(&mut out, path, "R5", "no groups list");
            continue;
        };
        for (gi, g) in groups.iter().enumerate() {
            let name = g.get("name").and_then(Value::as_str).unwrap_or("");
            let list = g.get("rules").and_then(Value::as_array);
            if name.is_empty() || list.is_none() {
                finding(
                    &mut out,
                    path,
                    "R5",
                    format!("group {} needs a name and a rules list", gi + 1),
                );
                continue;
            }
            if !names.insert(name) {
                finding(&mut out, path, "R5", format!("group {name} is given twice"));
            }
            let kind = g.get("type");
            let vlogs = logs || kind.and_then(Value::as_str) == Some("vlogs");
            match (logs, kind) {
                (true, Some(k)) if k == "vlogs" => {}
                (true, _) => finding(
                    &mut out,
                    path,
                    "R8",
                    format!("group {name} under {LOGS_DIR} must have \"type\": \"vlogs\""),
                ),
                (false, Some(k)) => finding(
                    &mut out,
                    path,
                    "R8",
                    format!("group {name} under {METRICS_DIR} must have no type (found {k})"),
                ),
                (false, None) => {}
            }
            for (ri, v) in list.into_iter().flatten().enumerate() {
                let r = Rule {
                    file: path,
                    group: name,
                    vlogs,
                    v,
                };
                let one_kind = r.alert().is_some() != r.record().is_some();
                if !one_kind || r.expr().is_none_or(|e| e.trim().is_empty()) {
                    finding(
                        &mut out,
                        path,
                        "R5",
                        format!(
                            "rule {} of group {name} needs alert or record (one of them) and an expr",
                            ri + 1
                        ),
                    );
                    continue;
                }
                rules.push(r);
            }
        }
    }
    (rules, out)
}

pub fn check(t: &Tree, p: &Parsed, cat: &Catalogue, out: &mut Vec<Finding>) {
    let (rules, shape) = collect(p);
    out.extend(shape);
    names_labels_functions(&rules, cat, out);
    r5_alerts(&rules, out);
    // A missing, empty or malformed venue list is U13's finding, a trading
    // list R13's.
    if let Some(v) = venues(p, "venues").filter(|v| !v.is_empty()) {
        r6_per_venue(PER_VENUE, v, &rules, out);
    }
    if let Some(v) = r13_trading(t, p, out) {
        r6_per_venue(TRADING_PER_VENUE, v, &rules, out);
    }
    r7_design(p, &rules, out);
    r12_holds(t, &rules, out);
    r9_scrape(p, &rules, out);
}

fn names_labels_functions(rules: &[Rule], cat: &Catalogue, out: &mut Vec<Finding>) {
    for r in rules.iter().filter(|r| !r.vlogs) {
        let s = expr::scan(r.expr().unwrap_or(""));
        let name = r.name();
        for n in &s.names {
            if !cat.has(n) {
                finding(
                    out,
                    r.file,
                    "R1",
                    format!("{name}: {n} is in no part of the metric catalogue"),
                );
            }
        }
        for l in &s.labels {
            if !cat.knows_label(l) {
                finding(
                    out,
                    r.file,
                    "R3",
                    format!("{name}: label {l} is a label of no catalogue entry"),
                );
            }
        }
        for f in &s.functions {
            if !FUNCTIONS.contains(&f.as_str()) {
                finding(
                    out,
                    r.file,
                    "R4",
                    format!("{name}: function {f} is not allowed"),
                );
            }
        }
        for e in &s.errors {
            finding(out, r.file, "R4", format!("{name}: {e}"));
        }
    }
}

fn r5_alerts(rules: &[Rule], out: &mut Vec<Finding>) {
    let mut watchdogs = 0;
    for r in rules {
        let Some(name) = r.alert() else { continue };
        if name == WATCHDOG {
            watchdogs += 1;
            let (file, group) = WATCHDOG_AT;
            if norm(r.expr().unwrap_or("")) != "vector(1)"
                || r.label("severity") != Some("watchdog")
                || r.v.get("for").is_some()
                || r.file != format!("{METRICS_DIR}{file}")
                || r.group != group
            {
                finding(
                    out,
                    r.file,
                    "R5",
                    format!(
                        "Watchdog must have expr vector(1), severity watchdog and no for, in {file} group {group}"
                    ),
                );
            }
            continue;
        }
        match r.label("severity") {
            Some(s) if SEVERITIES.contains(&s) => {}
            Some(s) => finding(
                out,
                r.file,
                "R5",
                format!("{name}: severity {s} is not P1, P2 or P3"),
            ),
            None => finding(out, r.file, "R5", format!("{name} has no labels.severity")),
        }
        if r.summary().is_none_or(|s| s.trim().is_empty()) {
            finding(
                out,
                r.file,
                "R5",
                format!("{name} has no annotations.summary"),
            );
        }
    }
    if watchdogs != 1 {
        finding(
            out,
            METRICS_DIR,
            "R5",
            format!("exactly one Watchdog alert must exist (found {watchdogs})"),
        );
    }
}

fn venues(p: &Parsed, key: &str) -> Option<Vec<String>> {
    p.toml
        .get(VENUES)?
        .get(key)?
        .as_array()?
        .iter()
        .map(|v| v.as_str().map(str::to_string))
        .collect()
}

/// The trading venues, when each is the watcher's venue label.
fn r13_trading(t: &Tree, p: &Parsed, out: &mut Vec<Finding>) -> Option<Vec<String>> {
    let Some(venues) = venues(p, "trading").filter(|v| !v.is_empty()) else {
        finding(
            out,
            VENUES,
            "R13",
            "trading must be a non-empty list of the venues an OMS on this PC trades",
        );
        return None;
    };
    for v in &venues {
        let literal = format!("(\"venue\", \"{v}\")");
        if !t.files.get(WATCHER_METRICS).is_some_and(|s| s.contains(&literal)) {
            finding(
                out,
                VENUES,
                "R13",
                format!(
                    "trading venue {v}: {WATCHER_METRICS} has no {literal}, so no watcher reconciles it"
                ),
            );
        }
    }
    Some(venues)
}

type PerVenue = (&'static str, &'static str, &'static str, &'static str, &'static str);

fn r6_per_venue(table: &[PerVenue], venues: Vec<String>, rules: &[Rule], out: &mut Vec<Finding>) {
    for &(name, severity, file, group, template) in table {
        let path = format!("{METRICS_DIR}{file}");
        let found: Vec<&Rule> = rules
            .iter()
            .filter(|r| !r.vlogs && r.alert() == Some(name))
            .collect();
        for v in &venues {
            let mine: Vec<&&Rule> = found
                .iter()
                .filter(|r| r.label("venue") == Some(v.as_str()))
                .collect();
            let [r] = mine[..] else {
                finding(
                    out,
                    METRICS_DIR,
                    "R6",
                    format!(
                        "venue {v} needs exactly one {name} alert with labels.venue = {v} (found {})",
                        mine.len()
                    ),
                );
                continue;
            };
            let want = template.replace("<V>", v);
            if norm(r.expr().unwrap_or("")) != want
                || r.label("severity") != Some(severity)
                || r.v.get("for").is_some()
                || r.file != path
                || r.group != group
            {
                finding(
                    out,
                    r.file,
                    "R6",
                    format!(
                        "{name} of {v} must be in {path} group {group} with severity {severity}, no for, and expr {want}"
                    ),
                );
            }
        }
        for r in &found {
            if !r
                .label("venue")
                .is_some_and(|x| venues.iter().any(|v| v == x))
            {
                finding(
                    out,
                    r.file,
                    "R6",
                    format!(
                        "{name} for venue {:?}, which venues.toml does not list",
                        r.label("venue")
                    ),
                );
            }
        }
    }
}

/// The rule `name` exists once, in this file and group, with exactly this
/// expression, `for` and severity.
fn pin(rules: &[Rule], rule: &'static str, d: DesignRule, want: &str, out: &mut Vec<Finding>) {
    let (file, group, name, _, want_for, severity) = d;
    let path = format!("{METRICS_DIR}{file}");
    let found: Vec<&Rule> = rules.iter().filter(|r| r.name() == name).collect();
    let [r] = found[..] else {
        finding(
            out,
            &path,
            rule,
            format!(
                "the design rule {name} must exist once (found {})",
                found.len()
            ),
        );
        return;
    };
    let got_for = r.v.get("for").and_then(Value::as_str);
    if r.file != path
        || r.group != group
        || norm(r.expr().unwrap_or("")) != want
        || got_for != want_for
        || r.label("severity") != severity
    {
        finding(
            out,
            r.file,
            rule,
            format!(
                "{name} must be in {path} group {group} with expr {want}, for {want_for:?}, severity {severity:?}"
            ),
        );
    }
}

fn r7_design(p: &Parsed, rules: &[Rule], out: &mut Vec<Finding>) {
    for &d in DESIGN {
        pin(rules, "R7", d, d.3, out);
    }
    for &(file, group, interval) in GROUP_INTERVALS {
        let path = format!("{METRICS_DIR}{file}");
        // A missing group leaves its pinned rules in another group: their
        // R5, R6 or R7 finding.
        let got = p
            .json
            .get(&path)
            .and_then(|d| d.get("groups"))
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .find(|g| g.get("name").and_then(Value::as_str) == Some(group))
            .and_then(|g| g.get("interval"));
        let ok = match interval {
            Some(i) => got.and_then(Value::as_str) == Some(i),
            None => got.is_none_or(|x| x.as_str() == Some(DEFAULT_INTERVAL)),
        };
        if !ok {
            let want = interval.unwrap_or("absent (the default 10s)");
            finding(
                out,
                &path,
                "R7",
                format!("group {group} must have interval {want} (found {got:?})"),
            );
        }
    }
}

/// The variants of `pub enum HoldReason { … }` in snake_case: the
/// identifiers that begin a line of its body and are followed by `,` or `(`
/// (`rename_all = "snake_case"`).
fn hold_reasons(text: &str) -> Option<Vec<String>> {
    let body = &text[text.find("pub enum HoldReason {")?..];
    let body = &body[..body.find("\n}")?];
    let mut out = Vec::new();
    for line in body.lines().skip(1) {
        let l = line.trim_start();
        let end = l
            .find(|c: char| !c.is_ascii_alphanumeric() && c != '_')
            .unwrap_or(l.len());
        let (id, rest) = l.split_at(end);
        if id.starts_with(|c: char| c.is_ascii_uppercase())
            && (rest.starts_with(',') || rest.starts_with('('))
        {
            let mut snake = String::new();
            for (i, c) in id.chars().enumerate() {
                if c.is_ascii_uppercase() && i > 0 {
                    snake.push('_');
                }
                snake.push(c.to_ascii_lowercase());
            }
            out.push(snake);
        }
    }
    Some(out)
}

/// The R12 partition: every reason of `reasons` but the strategy-only ones
/// is in exactly one class of `alerts`, and every class name is such a
/// reason.
fn partition(reasons: &[String], alerts: &[HoldAlert]) -> Vec<String> {
    let classed: Vec<&str> = alerts.iter().flat_map(|a| a.1.iter().copied()).collect();
    let mut out = Vec::new();
    for r in reasons.iter().filter(|r| !STRATEGY_ONLY.contains(&r.as_str())) {
        let n = classed.iter().filter(|c| **c == r.as_str()).count();
        if n != 1 {
            out.push(format!("hold reason {r} is in {n} hold alert classes (exactly one)"));
        }
    }
    for c in &classed {
        if !reasons.iter().any(|r| r == c) || STRATEGY_ONLY.contains(c) {
            out.push(format!(
                "{c} is in a hold alert class but Core::holds(None) has no such reason"
            ));
        }
    }
    out
}

fn r12_holds(t: &Tree, rules: &[Rule], out: &mut Vec<Finding>) {
    match t.files.get(MODEL).and_then(|s| hold_reasons(s)) {
        None => finding(
            out,
            MODEL,
            "R12",
            "no `pub enum HoldReason { … }` to read the hold reasons from",
        ),
        Some(reasons) => {
            for text in partition(&reasons, HOLD_ALERTS) {
                finding(out, MODEL, "R12", text);
            }
        }
    }
    if GATE != ["unmonitored"] {
        finding(
            out,
            METRICS_DIR,
            "R12",
            "the gate class must be exactly unmonitored (on at every start)",
        );
    }
    for &(name, class, want_for, severity, gated) in HOLD_ALERTS {
        let mut want = format!(
            "max by (venue, reason) (oms_holds{{reason=~\"{}\"}}) > 0",
            class.join("|")
        );
        if gated {
            want.push_str(OMS_UP_GATE);
        }
        let d = ("oms.rules.json", "oms", name, "", want_for, Some(severity));
        pin(rules, "R12", d, &want, out);
    }
}

fn r9_scrape(p: &Parsed, rules: &[Rule], out: &mut Vec<Finding>) {
    let Some(doc) = p.json.get(SCRAPE) else {
        finding(out, SCRAPE, "R9", "missing");
        return;
    };
    let interval = doc
        .get("global")
        .and_then(|g| g.get("scrape_interval"))
        .and_then(Value::as_str);
    if interval != Some(SCRAPE_INTERVAL) {
        finding(
            out,
            SCRAPE,
            "R9",
            format!("global.scrape_interval must be {SCRAPE_INTERVAL} (found {interval:?})"),
        );
    }
    let mut jobs = Vec::new();
    for c in doc
        .get("scrape_configs")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let job = c.get("job_name").and_then(Value::as_str).unwrap_or("");
        let targets: Vec<&str> = c
            .get("static_configs")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|s| s.get("targets").and_then(Value::as_array))
            .flatten()
            .map(|t| t.as_str().unwrap_or(""))
            .collect();
        for t in &targets {
            if !loopback_port(t) {
                finding(
                    out,
                    SCRAPE,
                    "U8",
                    format!("job {job}: target {t} must be 127.0.0.1:<port>"),
                );
            }
        }
        jobs.push(job);
        // The target is the job's own unit's listener.
        let unit = format!("{job}.service");
        let listen: Vec<String> = p
            .units
            .iter()
            .filter(|u| u.name == unit)
            .flat_map(|u| {
                let mut v = flag_values(u, "httpListenAddr");
                v.extend(flag_values(u, "web.listen-address"));
                v
            })
            .collect();
        // An address off loopback, a missing unit or a listener given other
        // than once is a U rule's finding only.
        let comparable = targets.iter().all(|t| loopback_port(t))
            && listen.len() == 1
            && loopback_port(&listen[0]);
        if SCRAPE_JOBS.contains(&job) && comparable && targets != listen {
            finding(
                out,
                SCRAPE,
                "R9",
                format!(
                    "job {job}: targets {targets:?} must be the listener of {unit} ({listen:?})"
                ),
            );
        }
    }
    let got: BTreeSet<&str> = jobs.iter().copied().collect();
    let want: BTreeSet<&str> = SCRAPE_JOBS.iter().copied().collect();
    if got != want || got.len() != jobs.len() {
        finding(
            out,
            SCRAPE,
            "R9",
            format!("the scrape jobs must be {SCRAPE_JOBS:?}, each once (found {jobs:?})"),
        );
    }
    // A job name no scrape writes would keep a rule silent.
    for r in rules.iter().filter(|r| !r.vlogs) {
        for (label, op, value) in expr::scan(r.expr().unwrap_or("")).matchers {
            if label != "job" || (op != "=" && op != "=~") {
                continue;
            }
            for j in value.split('|') {
                if !SCRAPE_JOBS.contains(&j) {
                    finding(
                        out,
                        r.file,
                        "R9",
                        format!("{}: job {j:?} is not a scrape job", r.name()),
                    );
                }
            }
        }
    }
    let mem: Vec<&Rule> = rules
        .iter()
        .filter(|r| !r.vlogs && r.alert() == Some(MEMORY_ALERT))
        .collect();
    let [r] = mem[..] else {
        finding(
            out,
            METRICS_DIR,
            "R9",
            format!("{MEMORY_ALERT} must exist once (found {})", mem.len()),
        );
        return;
    };
    // The caps are the design table's (rule U6 ties each unit to it). Whole
    // terms: a cap with more digits holds the right one as a prefix.
    let e = norm(r.expr().unwrap_or(""));
    let mut terms: BTreeSet<&str> = e.split(" or ").collect();
    for job in SCRAPE_JOBS {
        let unit = format!("{job}.service");
        let Some(cap) = MEMORY_MAX
            .iter()
            .find(|(n, _)| *n == unit)
            .and_then(|(_, c)| size(c))
        else {
            continue;
        };
        let want = format!("process_resident_memory_bytes{{job=\"{job}\"}} > 0.9 * {cap}");
        if !terms.remove(want.as_str()) {
            finding(
                out,
                r.file,
                "R9",
                format!("{MEMORY_ALERT} must hold {want} (the MemoryMax of {unit})"),
            );
        }
    }
    for t in terms {
        finding(
            out,
            r.file,
            "R9",
            format!("{MEMORY_ALERT}: term {t} is not a cap of the design table"),
        );
    }
}

// Tests.

use crate::assert_rules;

const MD: &str = "deploy/observability/vmalert/metrics/market-data.rules.json";
const LAKE: &str = "deploy/observability/vmalert/metrics/lake.rules.json";
const SYSTEM: &str = "deploy/observability/vmalert/metrics/system.rules.json";
const LOGS: &str = "deploy/observability/vmalert/logs/market-data-logs.rules.json";

/// The real tree with one rule file changed through its JSON value.
fn edit(t: &mut Tree, file: &str, f: impl FnOnce(&mut Value)) {
    let mut v: Value = serde_json::from_str(&t.files[file]).unwrap();
    f(&mut v);
    t.set(file, &serde_json::to_string_pretty(&v).unwrap());
}

/// The rule named `name` (alert or record) in a rule file's value.
fn rule_mut<'a>(v: &'a mut Value, name: &str) -> &'a mut Value {
    v["groups"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .flat_map(|g| g["rules"].as_array_mut().unwrap().iter_mut())
        .find(|r| r["alert"] == name || r["record"] == name)
        .unwrap_or_else(|| panic!("no rule {name}"))
}

fn plant_rule(file: &str, name: &str, f: impl FnOnce(&mut Value), rules: &[&str]) -> Vec<Finding> {
    let mut t = Tree::real();
    edit(&mut t, file, |v| f(rule_mut(v, name)));
    let found = crate::check(&t);
    assert_rules(&found, rules);
    found
}

fn set_expr(file: &str, name: &str, e: &str, rules: &[&str]) -> Vec<Finding> {
    let e = e.to_string();
    plant_rule(file, name, move |r| r["expr"] = Value::String(e), rules)
}

const GAPS: &str = "sum by (venue, market) (increase(md_gaps_total[10m])) > 20";

#[test]
fn a_rule_with_an_unknown_metric_fails() {
    for e in [
        "sum by (venue, market) (increase(md_gap_total[10m])) > 20",
        &format!("{GAPS} or md_no_such_metric > 0"),
        &format!("{GAPS} and on(venue) md_no_such_metric > 0"),
        &format!("{GAPS} unless md_no_such_metric"),
        &format!("md_no_such_metric{{venue=\"bybit\"}} > 0 or {GAPS}"),
        &format!("{GAPS} or absent_over_time(md_no_such_metric[5m])"),
    ] {
        let f = set_expr(MD, "MdGapsHigh", e, &["R1"]);
        assert_eq!(f.len(), 1, "{e}: {f:?}");
    }
    // In a pinned design rule the same name is R1 and R7.
    set_expr(
        LAKE,
        "LakeSealLag",
        "max by (venue) (lake_seal_lag_hour) > 2",
        &["R1", "R7"],
    );
    // LogsQL is not scanned: field names are not metric names.
    let mut t = Tree::real();
    edit(&mut t, LOGS, |v| {
        rule_mut(v, "MdErrorLogs")["expr"] =
            Value::String("_time:5m no_such_field:=1 | stats count() as n".to_string());
    });
    assert_rules(&crate::check(&t), &[]);
}

const OMS: &str = "deploy/observability/vmalert/metrics/oms.rules.json";

#[test]
fn a_rule_with_an_unknown_oms_metric_fails() {
    // A name the projector does not push (it pushes oms_position_qty).
    let f = set_expr(
        OMS,
        "OmsParkedFills",
        "max by (venue) (oms_position_value) > 0",
        &["R1"],
    );
    assert!(f[0].text.contains("oms_position_value"), "{f:?}");
    // In a pinned rule it is R1 and the pin's rule.
    set_expr(
        OMS,
        "JournalDiskDaysLeftLow",
        "journal_disk_days_lft < 7 and on(venue) (time() - tfirst_over_time(journal_disk_free_bytes[24h])) > 43200",
        &["R1", "R7"],
    );
}

#[test]
fn an_unknown_label_or_function_fails() {
    for (e, rule) in [
        (
            "sum by (venu, market) (increase(md_gaps_total[10m])) > 20",
            "R3",
        ),
        (
            "sum by (venue, market) (increase(md_gaps_total{chanel=\"x\"}[10m])) > 20",
            "R3",
        ),
        (
            "sum by (venue, market) (increase_pure(md_gaps_total[10m])) > 20",
            "R4",
        ),
        (
            "sum by (venue, market) (md_gaps_total) > 20 or histogram_quantile(0.9, md_gaps_total)",
            "R4",
        ),
        ("md_gaps_total by (venue) > 20", "R4"),
        (
            "sum by (venue, market) (increase(md_gaps_total{venue=bybit}[10m])) > 20",
            "R4",
        ),
    ] {
        set_expr(MD, "MdGapsHigh", e, &[rule]);
    }
    // job and instance are known without a catalogue row naming them.
    set_expr(
        MD,
        "MdGapsHigh",
        "sum by (venue, market, instance) (increase(md_gaps_total[10m])) > 20",
        &[],
    );
}

/// A change to one rule's JSON value.
type Edit = Box<dyn FnOnce(&mut Value)>;

#[test]
fn an_alert_without_a_severity_or_summary_fails() {
    let cases: Vec<Edit> = vec![
        Box::new(|r| {
            r["labels"].as_object_mut().unwrap().remove("severity");
        }),
        Box::new(|r| {
            r.as_object_mut().unwrap().remove("labels");
        }),
        Box::new(|r| r["labels"]["severity"] = Value::String("P4".into())),
        Box::new(|r| r["labels"]["severity"] = Value::String("p2".into())),
        Box::new(|r| r["annotations"]["summary"] = Value::String(" ".into())),
        Box::new(|r| {
            r.as_object_mut().unwrap().remove("annotations");
        }),
        Box::new(|r| r["record"] = Value::String("md_gaps_high".into())),
        Box::new(|r| {
            r.as_object_mut().unwrap().remove("expr");
        }),
    ];
    for f in cases {
        plant_rule(MD, "MdGapsHigh", f, &["R5"]);
    }
    // A log alert is held to the same rule.
    plant_rule(
        LOGS,
        "MdErrorLogs",
        |r| {
            r["labels"].as_object_mut().unwrap().remove("severity");
        },
        &["R5"],
    );
}

#[test]
fn exactly_one_watchdog_exists() {
    let mut t = Tree::real();
    edit(&mut t, SYSTEM, |v| {
        v["groups"][0]["rules"].as_array_mut().unwrap().clear();
    });
    let f = crate::check(&t);
    assert_rules(&f, &["R5"]);
    assert!(f[0].text.contains("found 0"), "{f:?}");
    // A second one, in another file.
    let mut t = Tree::real();
    let watchdog = rule_mut(
        &mut serde_json::from_str(&t.files[SYSTEM]).unwrap(),
        WATCHDOG,
    )
    .clone();
    edit(&mut t, MD, |v| {
        v["groups"][0]["rules"]
            .as_array_mut()
            .unwrap()
            .push(watchdog);
    });
    // The copy is also outside the Watchdog's group.
    let f = crate::check(&t);
    assert_rules(&f, &["R5"]);
    assert!(f.iter().any(|x| x.text.contains("found 2")), "{f:?}");
    plant_rule(
        SYSTEM,
        WATCHDOG,
        |r| r["expr"] = Value::String("vector(0)".into()),
        &["R5"],
    );
    plant_rule(
        SYSTEM,
        WATCHDOG,
        |r| r["labels"]["severity"] = Value::String("P1".into()),
        &["R5"],
    );
}

#[test]
fn log_rules_are_vlogs_and_metric_rules_are_not() {
    let mut t = Tree::real();
    edit(&mut t, LOGS, |v| {
        v["groups"][0].as_object_mut().unwrap().remove("type");
    });
    assert_rules(&crate::check(&t), &["R8"]);
    for kind in ["vlogs", "prometheus"] {
        let mut t = Tree::real();
        edit(&mut t, SYSTEM, |v| {
            v["groups"][0]["type"] = Value::String(kind.into());
        });
        assert_rules(&crate::check(&t), &["R8"]);
    }
    // A file in a subdirectory is not read by vmalert's `*.json` and not
    // by the check.
    let mut t = Tree::real();
    t.set(&format!("{METRICS_DIR}old/x.json"), "{\"groups\": 1}");
    assert!(crate::check(&t).iter().all(|x| x.rule != "R5"));
}

/// The real tree with a second venue added to `venues.toml` and to both
/// registries (so U13 passes).
fn with_okx() -> Tree {
    let mut t = Tree::real();
    t.replace(
        VENUES,
        "venues = [\"bybit\"]",
        "venues = [\"bybit\", \"okx\"]",
    );
    for registry in [
        "crates/md-collector/src/registry.rs",
        "crates/md-archiver/src/registry.rs",
    ] {
        let text = format!("{}\n// \"okx\" => Some(&Okx),\n", t.files[registry]);
        t.set(registry, &text);
    }
    t
}

/// Copies the bybit rule `name` as a rule for venue `v`, into the group
/// that holds it.
fn copy_for(t: &mut Tree, name: &str, v: &str) {
    let file = match name {
        n if n.starts_with("Lake") => LAKE,
        n if n.starts_with("Md") => MD,
        _ => OMS,
    };
    edit(t, file, |doc| {
        let rules = doc["groups"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .map(|g| g["rules"].as_array_mut().unwrap())
            .find(|rs| rs.iter().any(|r| r["alert"] == name))
            .unwrap_or_else(|| panic!("no rule {name}"));
        let mut r = rules.iter().find(|r| r["alert"] == name).unwrap().clone();
        let e = r["expr"]
            .as_str()
            .unwrap()
            .replace("\"bybit\"", &format!("\"{v}\""));
        r["expr"] = Value::String(e);
        r["labels"]["venue"] = Value::String(v.into());
        rules.push(r);
    });
}

#[test]
fn every_configured_venue_has_a_collector_liveness_rule() {
    // A venue without its rules.
    let f = crate::check(&with_okx());
    assert_rules(&f, &["R6"]);
    assert!(
        f.iter().any(|x| x
            .text
            .contains("venue okx needs exactly one MdCollectorDown")),
        "{f:?}"
    );
    // With its three rules the venue passes.
    let mut t = with_okx();
    copy_for(&mut t, "MdCollectorDown", "okx");
    copy_for(&mut t, "MdLakeGaugesMissing", "okx");
    copy_for(&mut t, "LakeAuditStale", "okx");
    assert_rules(&crate::check(&t), &[]);
    // The absent branch dropped (the dead collector that pushes nothing).
    set_expr(
        MD,
        "MdCollectorDown",
        "time() - max by (venue) (md_collector_heartbeat_ts{venue=\"bybit\"}) > 60",
        &["R6"],
    );
    // The age branch dropped, the threshold raised, a for, P2.
    set_expr(
        MD,
        "MdCollectorDown",
        "absent_over_time(md_collector_heartbeat_ts{venue=\"bybit\"}[2m])",
        &["R6"],
    );
    set_expr(
        MD,
        "MdCollectorDown",
        "time() - max by (venue) (md_collector_heartbeat_ts{venue=\"bybit\"}) > 600 or absent_over_time(md_collector_heartbeat_ts{venue=\"bybit\"}[2m])",
        &["R6"],
    );
    plant_rule(
        MD,
        "MdCollectorDown",
        |r| r["for"] = Value::String("1m".into()),
        &["R6"],
    );
    plant_rule(
        MD,
        "MdCollectorDown",
        |r| r["labels"]["severity"] = Value::String("P2".into()),
        &["R6"],
    );
    // A rule for a venue that venues.toml does not list.
    let mut t = Tree::real();
    copy_for(&mut t, "MdCollectorDown", "okx");
    let f = crate::check(&t);
    assert_rules(&f, &["R6"]);
    assert!(f[0].text.contains("does not list"), "{f:?}");
    // Two rules for one venue.
    let mut t = Tree::real();
    copy_for(&mut t, "MdCollectorDown", "bybit");
    assert_rules(&crate::check(&t), &["R6"]);
}

#[test]
fn every_configured_venue_has_a_lake_gauges_missing_rule() {
    let mut t = with_okx();
    copy_for(&mut t, "MdCollectorDown", "okx");
    copy_for(&mut t, "LakeAuditStale", "okx");
    let f = crate::check(&t);
    assert_rules(&f, &["R6"]);
    assert!(
        f.len() == 1
            && f[0]
                .text
                .contains("venue okx needs exactly one MdLakeGaugesMissing"),
        "{f:?}"
    );
    for e in [
        "absent_over_time(lake_disk_free_bytes{venue=\"bybit\"}[10m])",
        "absent_over_time(lake_seal_lag_hours{venue=\"bybit\"}[10m])",
        "absent_over_time(lake_disk_free_bytes{venue=\"bybit\"}[1h]) or absent_over_time(lake_seal_lag_hours{venue=\"bybit\"}[10m])",
    ] {
        set_expr(MD, "MdLakeGaugesMissing", e, &["R6"]);
    }
    plant_rule(
        MD,
        "MdLakeGaugesMissing",
        |r| r["labels"]["severity"] = Value::String("P3".into()),
        &["R6"],
    );
}

#[test]
fn every_configured_venue_has_an_audit_stale_rule() {
    let mut t = with_okx();
    copy_for(&mut t, "MdCollectorDown", "okx");
    copy_for(&mut t, "MdLakeGaugesMissing", "okx");
    let f = crate::check(&t);
    assert_rules(&f, &["R6"]);
    assert!(
        f.len() == 1
            && f[0]
                .text
                .contains("venue okx needs exactly one LakeAuditStale"),
        "{f:?}"
    );
    plant_rule(
        LAKE,
        "LakeAuditStale",
        |r| r["alert"] = Value::String("LakeAuditStaleX".into()),
        &["R6"],
    );
    for e in [
        "time() - last_over_time(lake_audit_last_run_ts{venue=\"bybit\"}[3d]) > 129600",
        "time() - last_over_time(lake_audit_last_run_ts{venue=\"bybit\"}[3d]) > 1296000 or absent_over_time(lake_audit_last_run_ts{venue=\"bybit\"}[36h])",
    ] {
        set_expr(LAKE, "LakeAuditStale", e, &["R6"]);
    }
    plant_rule(
        LAKE,
        "LakeAuditStale",
        |r| r["for"] = Value::String("1h".into()),
        &["R6"],
    );
    plant_rule(
        LAKE,
        "LakeAuditStale",
        |r| r["labels"]["severity"] = Value::String("P3".into()),
        &["R6"],
    );
    // Moved to the lake group.
    let mut t = Tree::real();
    edit(&mut t, LAKE, |v| {
        let rules = v["groups"][2]["rules"].as_array_mut().unwrap();
        let i = rules
            .iter()
            .position(|r| r["alert"] == "LakeAuditStale")
            .unwrap();
        let r = rules.remove(i);
        v["groups"][1]["rules"].as_array_mut().unwrap().push(r);
    });
    assert_rules(&crate::check(&t), &["R6"]);
}

/// The real tree with a second trading venue whose watcher labels it.
fn with_trading_okx() -> Tree {
    let mut t = Tree::real();
    t.replace(VENUES, "trading = [\"bybit\"]", "trading = [\"bybit\", \"okx\"]");
    let w = "crates/watcher/src/metrics.rs";
    let text = format!("{}\n// labels(&[(\"venue\", \"okx\")])\n", t.files[w]);
    t.set(w, &text);
    t
}

#[test]
fn every_trading_venue_has_its_liveness_rules() {
    const LIVENESS: [&str; 3] = ["JournalDiskGaugeMissing", "OmsDown", "ProjectorDown"];
    let f = crate::check(&with_trading_okx());
    assert_rules(&f, &["R6"]);
    for name in LIVENESS {
        let want = format!("venue okx needs exactly one {name}");
        assert!(f.iter().any(|x| x.text.contains(&want)), "{name}: {f:?}");
    }
    // OmsDown alone is not enough: the journal gauge and the projector.
    let mut t = with_trading_okx();
    copy_for(&mut t, "OmsDown", "okx");
    let f = crate::check(&t);
    assert_rules(&f, &["R6"]);
    assert_eq!(f.len(), 2, "{f:?}");
    let mut t = with_trading_okx();
    for name in LIVENESS {
        copy_for(&mut t, name, "okx");
    }
    assert_rules(&crate::check(&t), &[]);
    // A trading venue the watcher does not label; the bybit rules are then
    // for a venue the list does not hold.
    let mut t = Tree::real();
    t.replace(VENUES, "trading = [\"bybit\"]", "trading = [\"okx\"]");
    let f = crate::check(&t);
    assert_rules(&f, &["R6", "R13"]);
    assert!(
        f.iter()
            .any(|x| x.rule == "R13" && x.text.contains("(\"venue\", \"okx\")")),
        "{f:?}"
    );
    // No trading list, or an empty one.
    for to in ["", "trading = []"] {
        let mut t = Tree::real();
        t.replace(VENUES, "trading = [\"bybit\"]", to);
        assert_rules(&crate::check(&t), &["R13"]);
    }
    // The window, the severity and a for.
    for (name, e) in [
        (
            "OmsDown",
            "max by (venue) (max_over_time(oms_up{venue=\"bybit\"}[5m])) == 0",
        ),
        (
            "OmsDown",
            "max by (venue) (oms_up{venue=\"bybit\"}) == 0",
        ),
        (
            "ProjectorDown",
            "absent_over_time(projector_caught_up{venue=\"bybit\"}[1h])",
        ),
        (
            "JournalDiskGaugeMissing",
            "absent_over_time(journal_disk_free_bytes{venue=\"bybit\"}[1h])",
        ),
    ] {
        set_expr(OMS, name, e, &["R6"]);
    }
    plant_rule(
        OMS,
        "OmsDown",
        |r| r["labels"]["severity"] = Value::String("P2".into()),
        &["R6"],
    );
    plant_rule(
        OMS,
        "ProjectorDown",
        |r| r["for"] = Value::String("10m".into()),
        &["R6"],
    );
    // A rule for a venue that the trading list does not hold.
    let mut t = Tree::real();
    copy_for(&mut t, "OmsDown", "okx");
    let f = crate::check(&t);
    assert_rules(&f, &["R6"]);
    assert!(f[0].text.contains("does not list"), "{f:?}");
}

const LATCHED_EXPR: &str = "max by (venue, reason) (oms_holds{reason=~\"kill_latched|watcher_mismatch|venue_change|reducing|mismatch|storage_low\"}) > 0";

#[test]
fn every_hold_reason_has_an_alert_class() {
    let model = "crates/oms-core/src/model.rs";
    // A new hold variant without a class.
    let mut t = Tree::real();
    t.replace(
        model,
        "orders are cancelled.\n    Unmonitored,\n}",
        "orders are cancelled.\n    Unmonitored,\n    /// A new hold.\n    MarginCall(InstrumentId),\n}",
    );
    let f = crate::check(&t);
    assert_rules(&f, &["R12"]);
    assert!(f[0].text.contains("margin_call is in 0"), "{f:?}");
    // A variant renamed: the new name has no class, the old one is no reason.
    let mut t = Tree::real();
    t.replace(model, "    KillLatched,\n", "    KillSwitch,\n");
    let f = crate::check(&t);
    assert_rules(&f, &["R12"]);
    assert_eq!(f.len(), 2, "{f:?}");
    // The enum is not found.
    let mut t = Tree::real();
    t.replace(model, "pub enum HoldReason {", "pub enum HoldCause {");
    assert_rules(&crate::check(&t), &["R12"]);
    // A reason in two classes: a passing hold that pages at once.
    set_expr(
        OMS,
        "OmsHoldLatched",
        &LATCHED_EXPR.replace("storage_low", "storage_low|link_down"),
        &["R12"],
    );
    // A latched reason left out: a kill latch that pages nobody.
    set_expr(
        OMS,
        "OmsHoldLatched",
        &LATCHED_EXPR.replace("kill_latched|", ""),
        &["R12"],
    );
    // unmonitored back in the latched class: P1 at every normal start.
    let mut t = Tree::real();
    edit(&mut t, OMS, |v| {
        rule_mut(v, "OmsHoldLatched")["expr"] =
            Value::String(LATCHED_EXPR.replace("storage_low", "storage_low|unmonitored"));
        let rules = v["groups"][1]["rules"].as_array_mut().unwrap();
        rules.retain(|r| r["alert"] != "OmsUnmonitored");
    });
    let f = crate::check(&t);
    assert_rules(&f, &["R12"]);
    assert_eq!(f.len(), 2, "{f:?}");
    // The oms_up gate dropped: a stopped OMS's last state pages every 30 min.
    for name in ["OmsUnmonitored", "OmsHoldLasting"] {
        plant_rule(
            OMS,
            name,
            |r| {
                let e = r["expr"].as_str().unwrap();
                let e = e.replace(" and on(venue) max by (venue) (oms_up) == 1", "");
                r["expr"] = Value::String(e);
            },
            &["R12"],
        );
    }
    // The gate on a latched class: a latch of a stopped OMS goes quiet.
    set_expr(
        OMS,
        "OmsHoldLatched",
        &format!("{LATCHED_EXPR} and on(venue) max by (venue) (oms_up) == 1"),
        &["R12"],
    );
    // The for, the severity, another group.
    plant_rule(
        OMS,
        "OmsUnmonitored",
        |r| r["for"] = Value::String("30s".into()),
        &["R12"],
    );
    plant_rule(
        OMS,
        "OmsOperatorHalt",
        |r| r["labels"]["severity"] = Value::String("P1".into()),
        &["R12"],
    );
    let mut t = Tree::real();
    edit(&mut t, OMS, |v| {
        let rules = v["groups"][1]["rules"].as_array_mut().unwrap();
        let i = rules
            .iter()
            .position(|r| r["alert"] == "OmsHoldLasting")
            .unwrap();
        let r = rules.remove(i);
        v["groups"][2]["rules"].as_array_mut().unwrap().push(r);
    });
    assert_rules(&crate::check(&t), &["R12"]);
    // The class table itself: a reason in two classes, or in none.
    let reasons = hold_reasons(&Tree::real().files[model]).unwrap();
    assert!(partition(&reasons, HOLD_ALERTS).is_empty());
    let mut two = HOLD_ALERTS.to_vec();
    two[1].1 = &["unmonitored", "link_down"];
    let f = partition(&reasons, &two);
    assert!(f.len() == 1 && f[0].contains("link_down is in 2"), "{f:?}");
    two[1].1 = &[];
    let f = partition(&reasons, &two);
    assert!(f.len() == 1 && f[0].contains("unmonitored is in 0"), "{f:?}");
}

#[test]
fn the_design_lake_and_disk_rules_are_present() {
    // LakeAuditFailed back to the increase of the first rounds (the D3
    // panel that shows its value no longer matches it either).
    set_expr(
        LAKE,
        "LakeAuditFailed",
        "max by (venue) (increase(lake_audit_fail_total[26h])) > 0",
        &["R7", "G8"],
    );
    set_expr(
        LAKE,
        "LakeDiskDaysLeftLow",
        "lake_disk_days_left < 7",
        &["R7"],
    );
    set_expr(
        LAKE,
        "LakeL0WriteError",
        "max by (venue) (increase(lake_l0_write_errors_total[10m])) > 0",
        &["R7"],
    );
    set_expr(
        LAKE,
        "lake_disk_days_left",
        "lake_disk_free_bytes / clamp_min(-deriv(lake_disk_free_bytes[1h]) * 86400, 1)",
        &["R7"],
    );
    for (name, f) in [
        ("LakeDiskDaysLeftCritical", "for"),
        ("LakeDiskDaysLeftLow", "severity"),
        ("LakeSealLag", "for"),
        ("LakeL0WriteError", "severity"),
    ] {
        plant_rule(
            LAKE,
            name,
            |r| {
                if f == "for" {
                    if r.get("for").is_some() {
                        r.as_object_mut().unwrap().remove("for");
                    } else {
                        r["for"] = Value::String("5m".into());
                    }
                } else {
                    r["labels"]["severity"] = Value::String("P3".into());
                }
            },
            &["R7"],
        );
    }
    // Removed (the collector's inhibit rule then names a stale alert),
    // moved to another group, a group's interval changed.
    let mut t = Tree::real();
    edit(&mut t, LAKE, |v| {
        let rules = v["groups"][1]["rules"].as_array_mut().unwrap();
        rules.retain(|r| r["alert"] != "LakeSealLag");
    });
    assert_rules(&crate::check(&t), &["R7", "R10"]);
    let mut t = Tree::real();
    edit(&mut t, LAKE, |v| {
        let r = v["groups"][1]["rules"].as_array_mut().unwrap().remove(0);
        v["groups"][2]["rules"].as_array_mut().unwrap().push(r);
    });
    assert_rules(&crate::check(&t), &["R7"]);
    let mut t = Tree::real();
    edit(&mut t, LAKE, |v| {
        v["groups"][2]["interval"] = Value::String("10s".into());
    });
    assert_rules(&crate::check(&t), &["R7"]);
}

#[test]
fn the_design_journal_and_key_rules_are_present() {
    set_expr(
        OMS,
        "JournalDiskDaysLeftLow",
        "journal_disk_days_left < 8 and on(venue) (time() - tfirst_over_time(journal_disk_free_bytes[24h])) > 43200",
        &["R7"],
    );
    // Without the warm-up: the first day of a new journal pages.
    set_expr(
        OMS,
        "JournalDiskDaysLeftCritical",
        "journal_disk_days_left < 2",
        &["R7"],
    );
    set_expr(
        OMS,
        "journal_disk_days_left",
        "journal_disk_free_bytes / clamp_min(-deriv(journal_disk_free_bytes[1h]) * 86400, 1)",
        &["R7"],
    );
    for (name, f) in [
        ("JournalDiskDaysLeftLow", "for"),
        ("JournalDiskDaysLeftCritical", "severity"),
    ] {
        plant_rule(
            OMS,
            name,
            |r| {
                if f == "for" {
                    r.as_object_mut().unwrap().remove("for");
                } else {
                    r["labels"]["severity"] = Value::String("P2".into());
                }
            },
            &["R7"],
        );
    }
    // The recording group evaluated every hour; the OMS group every minute.
    for (group, interval) in [("journal-recording", "1h"), ("oms", "1m"), ("projector", "5m")] {
        let mut t = Tree::real();
        edit(&mut t, OMS, |v| {
            let g = v["groups"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .find(|g| g["name"] == group)
                .unwrap();
            g["interval"] = Value::String(interval.into());
        });
        assert_rules(&crate::check(&t), &["R7"]);
    }
}

#[test]
fn the_scrape_config_reads_the_stack_on_loopback() {
    for (from, to, rules) in [
        ("\"127.0.0.1:8428\"", "\"0.0.0.0:8428\"", &["U8"][..]),
        ("\"127.0.0.1:9428\"", "\"localhost:9428\"", &["U8"][..]),
        ("\"127.0.0.1:8880\"", "\"127.0.0.1:8881\"", &["R9"][..]),
        ("\"15s\"", "\"1m\"", &["R9"][..]),
        (
            "\"job_name\": \"alertmanager\"",
            "\"job_name\": \"am\"",
            &["R9"][..],
        ),
    ] {
        let mut t = Tree::real();
        t.replace(SCRAPE, from, to);
        assert_rules(&crate::check(&t), rules);
    }
    let mut t = Tree::real();
    edit(&mut t, SCRAPE, |v| {
        v["scrape_configs"].as_array_mut().unwrap().pop();
    });
    assert_rules(&crate::check(&t), &["R9"]);
    let mut t = Tree::real();
    edit(&mut t, SCRAPE, |v| {
        let c = v["scrape_configs"][0].clone();
        v["scrape_configs"].as_array_mut().unwrap().push(c);
    });
    assert_rules(&crate::check(&t), &["R9"]);
    // A rule that names a job no scrape writes.
    set_expr(
        SYSTEM,
        "ObsComponentDown",
        "up{job=~\"victorialogs|vmalert-log|alertmanager\"} == 0",
        &["R9"],
    );
    let mut t = Tree::real();
    t.remove(SCRAPE);
    let f = crate::check(&t);
    assert!(
        f.iter().any(|x| x.rule == "R9" && x.text == "missing"),
        "{f:?}"
    );
}

#[test]
fn memory_alerts_follow_the_unit_caps() {
    let mut t = Tree::real();
    t.replace(
        SYSTEM,
        "process_resident_memory_bytes{job=\\\"vmalert-metrics\\\"} > 0.9 * 209715200",
        "process_resident_memory_bytes{job=\\\"vmalert-metrics\\\"} > 0.9 * 314572800",
    );
    assert_rules(&crate::check(&t), &["R9"]);
    set_expr(
        SYSTEM,
        "ObsMemoryNearCap",
        "process_resident_memory_bytes{job=\"victoriametrics\"} > 0.9 * 1073741824",
        &["R9"],
    );
    // A cap ten times too large: the right number is a prefix of it.
    for (from, to) in [
        ("0.9 * 104857600\"", "0.9 * 1048576000\""),
        (
            "{job=\\\"victoriametrics\\\"} > 0.9 * 1073741824",
            "{job=\\\"victoriametrics\\\"} > 0.9 * 10737418240",
        ),
        ("0.9 * 104857600\"", "0.9 * 104857600.5\""),
    ] {
        let mut t = Tree::real();
        t.replace(SYSTEM, from, to);
        assert_rules(&crate::check(&t), &["R9"]);
    }
    // A sixth term is not one of the caps.
    let mut t = Tree::real();
    t.replace(
        SYSTEM,
        "0.9 * 104857600\"",
        "0.9 * 104857600 or process_resident_memory_bytes{job=\\\"alertmanager\\\"} > 0.99 * 104857600\"",
    );
    assert_rules(&crate::check(&t), &["R9"]);
}

#[test]
fn the_pinned_rules_are_evaluated_at_the_design_interval() {
    // A group's interval changed: the P1 rules of market-data and lake, the
    // Watchdog's group.
    for (file, group, interval) in [
        (MD, "market-data", "1h"),
        (MD, "market-data", "1m"),
        (LAKE, "lake", "6h"),
        (SYSTEM, "watchdog", "1h"),
    ] {
        let mut t = Tree::real();
        edit(&mut t, file, |v| {
            let g = v["groups"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .find(|g| g["name"] == group)
                .unwrap();
            g["interval"] = Value::String(interval.into());
        });
        assert_rules(&crate::check(&t), &["R7"]);
    }
    // A group evaluated at the default may also name it.
    let mut t = Tree::real();
    edit(&mut t, LAKE, |v| {
        v["groups"][1]["interval"] = Value::String("10s".into());
    });
    assert_rules(&crate::check(&t), &[]);
    // The Watchdog with a for, or in a group of its own.
    plant_rule(
        SYSTEM,
        WATCHDOG,
        |r| r["for"] = Value::String("1h".into()),
        &["R5"],
    );
    let mut t = Tree::real();
    edit(&mut t, SYSTEM, |v| {
        let r = v["groups"][0]["rules"].as_array_mut().unwrap().remove(0);
        v["groups"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({"name": "late", "interval": "1h", "rules": [r]}));
    });
    assert_rules(&crate::check(&t), &["R5"]);
    // A per-venue rule in a group of its own.
    let mut t = Tree::real();
    edit(&mut t, MD, |v| {
        let r = v["groups"][0]["rules"].as_array_mut().unwrap().remove(0);
        assert_eq!(r["alert"], "MdCollectorDown");
        v["groups"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({"name": "late", "interval": "1h", "rules": [r]}));
    });
    assert_rules(&crate::check(&t), &["R6"]);
}
