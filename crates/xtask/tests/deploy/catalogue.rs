//! The metric catalogue (ADR-035 D7): `deploy/observability/metrics.toml`,
//! the pinned qdp list `contracts/lake/xrepo/metrics.json` and the `record`
//! names of the rule files. Rules R2 (a row names a file that has the name
//! as a literal) and R2b (every reserved lake name has a writer, and one
//! name has one writer).

use std::collections::{BTreeMap, BTreeSet};

use crate::expr;
use crate::rules;
use crate::tree::{Finding, Parsed, Tree, finding};

const CATALOGUE: &str = "deploy/observability/metrics.toml";
const QDP: &str = "contracts/lake/xrepo/metrics.json";

/// Who writes a name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Writer {
    /// A `[[metric]]` row: hl-oms pushes it.
    Hl,
    /// A `[[stack]]` row: a tool of the stack exports it.
    Stack,
    /// The qdp auditor (the pinned `metrics` list).
    Qdp,
    /// A recording rule of a rule file.
    Record,
}

#[derive(Default)]
pub struct Catalogue {
    /// Name → (writer, labels).
    pub metrics: BTreeMap<String, (Writer, BTreeSet<String>)>,
}

impl Catalogue {
    pub fn has(&self, name: &str) -> bool {
        self.metrics.contains_key(name)
    }

    /// A label is known when some entry has it; `job` and `instance` are
    /// added by the scrape (and by vmalert's queries) to every stack series.
    pub fn knows_label(&self, label: &str) -> bool {
        label == "job"
            || label == "instance"
            || self.metrics.values().any(|(_, l)| l.contains(label))
    }

    fn add(&mut self, name: &str, w: Writer, labels: BTreeSet<String>, out: &mut Vec<Finding>) {
        if let Some((prev, _)) = self.metrics.get(name) {
            let file = match w {
                Writer::Record => "deploy/observability/vmalert/metrics/",
                _ => CATALOGUE,
            };
            finding(
                out,
                file,
                "R2b",
                format!("{name} has two writers ({prev:?} and {w:?}): one name, one writer"),
            );
            return;
        }
        self.metrics.insert(name.to_string(), (w, labels));
    }
}

fn strings(v: Option<&toml::Value>) -> Option<Vec<String>> {
    v?.as_array()?
        .iter()
        .map(|x| x.as_str().map(str::to_string))
        .collect()
}

fn json_strings(v: Option<&serde_json::Value>) -> Option<Vec<String>> {
    v?.as_array()?
        .iter()
        .map(|x| x.as_str().map(str::to_string))
        .collect()
}

pub fn build(t: &Tree, p: &Parsed, out: &mut Vec<Finding>) -> Catalogue {
    let mut c = Catalogue::default();
    let Some(doc) = p.toml.get(CATALOGUE) else {
        finding(out, CATALOGUE, "R2", "missing");
        return c;
    };
    let row = |v: &toml::Value, keys: &[&str]| -> Option<Vec<String>> {
        keys.iter()
            .map(|k| v.get(*k).and_then(|x| x.as_str()).map(str::to_string))
            .collect()
    };
    for (i, m) in doc
        .get("metric")
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
        .enumerate()
    {
        let (Some(f), Some(labels)) = (row(m, &["name", "source"]), strings(m.get("labels")))
        else {
            finding(
                out,
                CATALOGUE,
                "R2",
                format!("[[metric]] {} needs name, labels and source", i + 1),
            );
            continue;
        };
        let (name, source) = (&f[0], &f[1]);
        let literal = format!("\"{name}\"");
        match t.files.get(source.as_str()) {
            None => finding(
                out,
                CATALOGUE,
                "R2",
                format!("{name}: source {source} is not a file the check reads (tree::ANCHORS)"),
            ),
            Some(text) if !text.contains(&literal) => finding(
                out,
                CATALOGUE,
                "R2",
                format!("{name}: {source} has no literal {literal}"),
            ),
            Some(_) => {}
        }
        c.add(name, Writer::Hl, labels.into_iter().collect(), out);
    }
    for (i, m) in doc
        .get("stack")
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
        .enumerate()
    {
        let f = row(m, &["name", "component", "version"]);
        let labels = strings(m.get("labels"));
        match (f, labels) {
            (Some(f), Some(labels)) if f.iter().all(|x| !x.trim().is_empty()) => {
                c.add(&f[0], Writer::Stack, labels.into_iter().collect(), out);
            }
            _ => finding(
                out,
                CATALOGUE,
                "R2",
                format!(
                    "[[stack]] {} needs name, labels and a non-empty component and version",
                    i + 1
                ),
            ),
        }
    }
    // The pinned list is not under `deploy/`, so parse rule P1 does not read
    // it; a copy that is not JSON is the vendored-contract test's finding.
    let qdp: Option<serde_json::Value> =
        t.files.get(QDP).and_then(|s| serde_json::from_str(s).ok());
    let qdp = qdp.as_ref();
    let reserved = json_strings(qdp.and_then(|v| v.get("reserved"))).unwrap_or_default();
    for m in qdp
        .and_then(|v| v.get("metrics"))
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
    {
        let (Some(name), Some(labels)) = (
            m.get("name").and_then(|v| v.as_str()),
            json_strings(m.get("labels")),
        ) else {
            finding(out, QDP, "R2b", "a metric needs a name and labels");
            continue;
        };
        if c.metrics.get(name).is_some_and(|(w, _)| *w == Writer::Hl) {
            finding(
                out,
                CATALOGUE,
                "R2b",
                format!("{name} is a qdp metric and a [[metric]] row: one name, one writer"),
            );
            continue;
        }
        c.add(name, Writer::Qdp, labels.into_iter().collect(), out);
    }
    // A record has the labels of the metrics its expression reads and the
    // labels the rule sets.
    for r in rules::collect(p).0.iter().filter(|r| !r.vlogs) {
        let Some(name) = r.record() else { continue };
        let mut labels: BTreeSet<String> = BTreeSet::new();
        for n in expr::scan(r.expr().unwrap_or("")).names {
            if let Some((_, l)) = c.metrics.get(&n) {
                labels.extend(l.iter().cloned());
            }
        }
        labels.extend(r.label_keys());
        c.add(name, Writer::Record, labels, out);
    }
    for name in &reserved {
        if !c
            .metrics
            .get(name)
            .is_some_and(|(w, _)| matches!(w, Writer::Hl | Writer::Record))
        {
            finding(
                out,
                CATALOGUE,
                "R2b",
                format!(
                    "{name} is reserved for hl-oms in {QDP} but no [[metric]] row or recording rule writes it"
                ),
            );
        }
    }
    c
}

// Tests.

use crate::{assert_rules, check};

const LAKE: &str = "deploy/observability/vmalert/metrics/lake.rules.json";
const SYSTEM: &str = "deploy/observability/vmalert/metrics/system.rules.json";

#[test]
fn a_name_in_no_part_of_the_catalogue_fails() {
    // In none of the three parts: not pushed by hl-oms, not in the
    // pinned qdp list, not a record.
    let mut t = Tree::real();
    t.replace(
        SYSTEM,
        "increase(vmalert_iteration_missed_total[10m]) > 0",
        "increase(lake_audit_runs_total[10m]) > 0",
    );
    let f = check(&t);
    assert_rules(&f, &["R1"]);
    assert!(f[0].text.contains("lake_audit_runs_total"), "{f:?}");
    // Each part on its own makes a name known.
    let t = Tree::real();
    let mut out = Vec::new();
    let p = crate::tree::parse(&t, &mut out);
    let c = build(&t, &p, &mut out);
    assert!(out.is_empty(), "{out:?}");
    for (name, w) in [
        ("md_msgs", Writer::Hl),
        ("up", Writer::Stack),
        ("lake_audit_fail_total", Writer::Qdp),
        ("lake_disk_days_left", Writer::Record),
    ] {
        assert_eq!(c.metrics.get(name).map(|x| x.0), Some(w), "{name}");
    }
    assert!(!c.has("lake_audit_runs_total"));
}

#[test]
fn a_removed_recording_rule_makes_its_name_unknown() {
    let mut t = Tree::real();
    t.replace(
        LAKE,
        "\"record\": \"lake_disk_days_left\"",
        "\"record\": \"lake_disk_days_left_v2\"",
    );
    // The two days-left alerts and the D3 panel read a name nobody writes;
    // the reserved name has no writer; the design's record is gone.
    let f = check(&t);
    assert_rules(&f, &["R1", "R2b", "R7", "G7"]);
    assert_eq!(f.iter().filter(|x| x.rule == "R1").count(), 2, "{f:?}");
}

#[test]
fn a_catalogue_row_without_its_source_literal_fails() {
    let telemetry = "crates/md-collector/src/telemetry.rs";
    // The source no longer has the name.
    let mut t = Tree::real();
    t.replace(
        telemetry,
        "push(\"md_msgs\", l.clone(), cs.msgs as f64);",
        "push(\"md_messages\", l.clone(), cs.msgs as f64);",
    );
    assert_rules(&check(&t), &["R2"]);
    // The row names a file that does not have it.
    let mut t = Tree::real();
    t.replace(
        CATALOGUE,
        "name = \"md_msgs\"\nlabels = [\"venue\", \"market\", \"channel\", \"conn\"]\nsource = \"crates/md-collector/src/telemetry.rs\"",
        "name = \"md_msgs\"\nlabels = [\"venue\", \"market\", \"channel\", \"conn\"]\nsource = \"crates/md-collector/src/obs.rs\"",
    );
    assert_rules(&check(&t), &["R2"]);
    // A file the check does not read.
    let mut t = Tree::real();
    t.replace(
        CATALOGUE,
        "source = \"crates/obs-push/src/lib.rs\"\n\n[[metric]]\nname = \"obs_push_queue_lines\"",
        "source = \"crates/obs-push/src/main.rs\"\n\n[[metric]]\nname = \"obs_push_queue_lines\"",
    );
    assert_rules(&check(&t), &["R2"]);
    // A row without a source, a stack row without a version.
    let mut t = Tree::real();
    t.replace(
        CATALOGUE,
        "name = \"obs_push_queue_lines\"\nlabels = [\"service\", \"venue\", \"endpoint\"]\nsource = \"crates/obs-push/src/lib.rs\"\n",
        "name = \"obs_push_queue_lines\"\nlabels = [\"service\", \"venue\", \"endpoint\"]\n",
    );
    assert_rules(&check(&t), &["R2"]);
    let mut t = Tree::real();
    t.replace(
        CATALOGUE,
        "name = \"vmalert_execution_errors_total\"\nlabels = [\"job\", \"instance\"]\ncomponent = \"vmalert\"\nversion = \"v1.157.0\"",
        "name = \"vmalert_execution_errors_total\"\nlabels = [\"job\", \"instance\"]\ncomponent = \"vmalert\"\nversion = \" \"",
    );
    assert_rules(&check(&t), &["R2"]);
}

#[test]
fn a_reserved_lake_name_nobody_produces_fails() {
    // lake_l0_open_files is reserved for hl-oms and no rule reads it.
    let mut t = Tree::real();
    t.replace(
        CATALOGUE,
        "[[metric]]\nname = \"lake_l0_open_files\"\nlabels = [\"venue\"]\nsource = \"crates/md-collector/src/obs.rs\"\n",
        "",
    );
    let f = check(&t);
    assert_rules(&f, &["R2b"]);
    assert!(f[0].text.contains("lake_l0_open_files"), "{f:?}");
    // A qdp metric that is also a [[metric]] row: two writers.
    let mut t = Tree::real();
    let text = format!(
        "{}\n[[metric]]\nname = \"lake_audit_fail_total\"\nlabels = [\"venue\"]\nsource = \"contracts/lake/xrepo/metrics.json\"\n",
        t.files[CATALOGUE]
    );
    t.set(CATALOGUE, &text);
    assert_rules(&check(&t), &["R2b"]);
    // A record named like a pushed metric: two writers.
    let mut t = Tree::real();
    t.replace(
        LAKE,
        "\"record\": \"lake_disk_days_left\"",
        "\"record\": \"lake_seal_lag_hours\"",
    );
    let f = check(&t);
    assert!(
        f.iter()
            .any(|x| x.rule == "R2b" && x.text.contains("lake_seal_lag_hours has two writers")),
        "{f:?}"
    );
}

/// The series the projector pushes and the labels it gives them
/// (`crates/oms-projector/src/metrics.rs`; `journal_disk_free_bytes` in
/// `projector.rs`).
const PROJECTOR: &[(&str, &[&str])] = &[
    ("oms_up", &["venue"]),
    ("oms_heartbeat_ts", &["venue"]),
    ("oms_journal_durable_seq", &["venue"]),
    ("projector_lag_records", &["venue"]),
    ("projector_cursor_seq", &["venue"]),
    ("projector_pending_lines", &["venue"]),
    ("projector_unconfirmed_records", &["venue"]),
    ("projector_tail_error", &["venue"]),
    ("projector_caught_up", &["venue"]),
    ("projector_rejected_lines", &["venue"]),
    ("oms_halt", &["venue"]),
    ("oms_holds", &["venue", "reason"]),
    ("oms_open_orders", &["venue"]),
    ("oms_in_doubt_orders", &["venue"]),
    ("oms_parked_fills", &["venue"]),
    ("oms_position_qty", &["venue", "symbol"]),
    ("oms_realized_pnl", &["venue", "symbol"]),
    ("oms_upnl", &["venue", "symbol"]),
    ("oms_account_value", &["venue"]),
    ("oms_margin_ratio", &["venue", "symbol"]),
    ("oms_limit", &["venue", "kind", "symbol"]),
    ("oms_limit_distance", &["venue", "kind", "symbol"]),
    ("journal_disk_free_bytes", &["venue"]),
];

/// The series the watcher exports and the labels it gives them, besides
/// `venue`, `host` and `watcher` on every series
/// (`crates/watcher/src/metrics.rs`), with the file that holds the name.
const WATCHER: &[(&str, &[&str], &str)] = &[
    ("watcher_up", &[], "metrics.rs"),
    ("watcher_differences", &["check"], "engine.rs"),
    ("watcher_mismatch_active", &["check"], "engine.rs"),
    ("watcher_check_age_s", &["check"], "engine.rs"),
    ("watcher_request_errors_total", &["endpoint", "class"], "rest.rs"),
    ("watcher_journal_error", &[], "engine.rs"),
    ("watcher_journal_seq", &[], "engine.rs"),
    ("watcher_mismatches_total", &["check"], "engine.rs"),
    ("watcher_heartbeat_ok_ts", &[], "oms.rs"),
    ("watcher_auth_rejected", &[], "rest.rs"),
    ("watcher_requests_total", &["endpoint"], "rest.rs"),
    ("watcher_alerts_queued", &[], "alert.rs"),
    ("watcher_alerts_delivered_total", &[], "alert.rs"),
    ("watcher_alerts_dropped_total", &[], "alert.rs"),
    ("watcher_alert_failures_total", &[], "alert.rs"),
    ("watcher_api_key_days_left", &[], "start.rs"),
    ("watcher_public_ip_ok", &[], "start.rs"),
    ("watcher_w5_armed", &[], "runtime.rs"),
    ("watcher_kill_active", &[], "runtime.rs"),
    ("watcher_kills_total", &[], "runtime.rs"),
    ("watcher_kill_cancel_all_total", &[], "runtime.rs"),
    ("watcher_kill_halt_owed", &[], "runtime.rs"),
    ("watcher_kill_step", &["step"], "runtime.rs"),
    ("watcher_kill_round", &[], "runtime.rs"),
    ("watcher_kill_remaining", &[], "runtime.rs"),
    ("watcher_oms_silence_s", &[], "runtime.rs"),
];

#[test]
fn every_projector_and_watcher_series_is_catalogued() {
    let t = Tree::real();
    let mut out = Vec::new();
    let p = crate::tree::parse(&t, &mut out);
    let c = build(&t, &p, &mut out);
    assert!(out.is_empty(), "{out:?}");
    for (name, labels) in PROJECTOR {
        let want: BTreeSet<String> = labels.iter().map(|l| l.to_string()).collect();
        assert_eq!(c.metrics.get(*name), Some(&(Writer::Hl, want)), "{name}");
    }
    let doc = &p.toml[CATALOGUE];
    for (name, labels, file) in WATCHER {
        let mut want: BTreeSet<String> = labels.iter().map(|l| l.to_string()).collect();
        want.extend(["venue", "host", "watcher"].map(String::from));
        assert_eq!(c.metrics.get(*name), Some(&(Writer::Hl, want)), "{name}");
        let row = doc["metric"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["name"].as_str() == Some(name))
            .unwrap();
        let source = format!("crates/watcher/src/{file}");
        assert_eq!(row["source"].as_str(), Some(source.as_str()), "{name}");
    }
    let hl = c
        .metrics
        .iter()
        .filter(|(n, (w, _))| *w == Writer::Hl && n.starts_with("watcher_"))
        .count();
    assert_eq!(hl, WATCHER.len());
    // A row removed: the rule that reads it names an unknown metric.
    let mut t = Tree::real();
    t.replace(
        CATALOGUE,
        "[[metric]]\nname = \"oms_parked_fills\"\nlabels = [\"venue\"]\nsource = \"crates/oms-projector/src/metrics.rs\"\n",
        "",
    );
    let f = check(&t);
    assert_rules(&f, &["R1", "G7"]);
    assert!(f[0].text.starts_with("OmsParkedFills: oms_parked_fills"), "{f:?}");
    // A watcher row removed: the rule that reads it names an unknown metric.
    let mut t = Tree::real();
    t.replace(
        CATALOGUE,
        "[[metric]]\nname = \"watcher_kill_halt_owed\"\nlabels = [\"venue\", \"host\", \"watcher\"]\nsource = \"crates/watcher/src/runtime.rs\"\n",
        "",
    );
    let f = check(&t);
    assert_rules(&f, &["R1"]);
    assert!(f[0].text.starts_with("WatcherKillHaltOwed: watcher_kill_halt_owed"), "{f:?}");
    // The projector's file no longer has the name.
    let mut t = Tree::real();
    t.replace(
        "crates/oms-projector/src/metrics.rs",
        "o.push(\"oms_halt\",",
        "o.push(\"oms_halted\",",
    );
    assert_rules(&check(&t), &["R2"]);
    // Nor the watcher's.
    let mut t = Tree::real();
    t.replace(
        "crates/watcher/src/start.rs",
        "pub const DAYS_LEFT: &str = \"watcher_api_key_days_left\";",
        "pub const DAYS_LEFT: &str = \"watcher_key_days_left\";",
    );
    assert_rules(&check(&t), &["R2"]);
}
