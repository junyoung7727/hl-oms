//! Alertmanager rules (ADR-035 D8): R10 the route tree, the receivers and
//! the inhibit rule; R11 secrets only as file paths under `/etc/`, and no
//! token-shaped string or chat id in any deploy file.

use std::collections::BTreeSet;

use serde_json::Value;

use crate::tree::{Finding, Parsed, Tree, finding};

pub const AM: &str = "deploy/observability/alertmanager/alertmanager.json";

/// R10: the receiver of everything no child route takes (P3 and anything
/// else): dashboard and weekly review only, so no integration.
const DEFAULT_RECEIVER: &str = "null";
/// R10: the labels every message is grouped by (inherited by every child).
const GROUP_BY: &[&str] = &["alertname", "venue"];
/// R10: the child routes, in this order: the one matcher, the receiver.
/// The `LakeAuditFailed` route comes before the P2 route: that alert ends
/// by itself after 26 h while its file may still fail, so its receiver
/// sends no resolved message.
const ROUTES: &[(&str, &str)] = &[
    ("alertname=\"Watchdog\"", "deadman"),
    ("severity=\"P1\"", "telegram-p1"),
    ("alertname=\"LakeAuditFailed\"", "telegram-p2-noresolve"),
    ("severity=\"P2\"", "telegram-p2"),
];
/// R10: keys a route may have. The root alone groups and has children;
/// a child has one matcher and no `continue` (the `LakeAuditFailed` alert
/// would go on to the P2 route and its resolved message), no muting and
/// no children of its own.
const ROOT_KEYS: &[&str] = &[
    "receiver",
    "group_by",
    "routes",
    "group_wait",
    "group_interval",
    "repeat_interval",
];
const CHILD_KEYS: &[&str] = &[
    "matchers",
    "receiver",
    "group_wait",
    "group_interval",
    "repeat_interval",
];
/// R10: the Telegram receivers: message prefix, silent (P2,
/// `disable_notifications`), resolved message. Each has one Telegram
/// config, in plain text (`parse_mode` empty: no HTML from label values).
const TELEGRAM: &[(&str, &str, bool, bool)] = &[
    ("telegram-p1", "[MON P1] ", false, true),
    ("telegram-p2", "[MON P2] ", true, true),
    ("telegram-p2-noresolve", "[MON P2] ", true, false),
];

/// A matcher `name op "value"` of a route or an inhibit rule, read as
/// Alertmanager reads it (spaces around the operator, value quoted or not).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Matcher {
    pub name: String,
    pub op: &'static str,
    pub value: String,
}

pub fn matcher(s: &str) -> Option<Matcher> {
    let s = s.trim();
    let end = s
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .unwrap_or(s.len());
    let (name, rest) = s.split_at(end);
    let rest = rest.trim_start();
    let op = ["=~", "!~", "!=", "="]
        .into_iter()
        .find(|op| rest.starts_with(op))?;
    let v = rest[op.len()..].trim();
    let value = match v.strip_prefix('"') {
        Some(q) => q.strip_suffix('"')?,
        None => v,
    };
    if name.is_empty() || value.contains('"') || value.contains('\\') {
        return None;
    }
    Some(Matcher {
        name: name.to_string(),
        op,
        value: value.to_string(),
    })
}

/// The matchers of a list of strings; `None` when one does not read.
fn matchers(v: Option<&Value>) -> Option<Vec<Matcher>> {
    v?.as_array()?
        .iter()
        .map(|m| m.as_str().and_then(matcher))
        .collect()
}

fn strs(v: Option<&Value>) -> Option<Vec<&str>> {
    v?.as_array()?.iter().map(Value::as_str).collect()
}

/// A receiver's integrations: every key but its name.
fn integrations(r: &Value) -> Vec<&str> {
    r.as_object()
        .into_iter()
        .flatten()
        .map(|(k, _)| k.as_str())
        .filter(|k| *k != "name")
        .collect()
}

pub fn check(_t: &Tree, p: &Parsed, out: &mut Vec<Finding>) {
    let Some(doc) = p.json.get(AM) else {
        finding(out, AM, "R10", "missing");
        return;
    };
    r10_routes(doc, out);
}

fn r10_routes(doc: &Value, out: &mut Vec<Finding>) {
    let receivers: Vec<&Value> = doc
        .get("receivers")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .collect();
    let mut names = BTreeSet::new();
    for r in &receivers {
        let name = r.get("name").and_then(Value::as_str).unwrap_or("");
        if !names.insert(name) {
            finding(out, AM, "R10", format!("receiver {name:?} is given twice"));
        }
    }
    let receiver = |name: &str| {
        receivers
            .iter()
            .find(|r| r.get("name").and_then(Value::as_str) == Some(name))
    };
    let Some(root) = doc.get("route").filter(|r| r.is_object()) else {
        finding(out, AM, "R10", "no route tree");
        return;
    };
    // Every receiver a route names exists (the tree is finite: each route
    // is visited once).
    let mut todo = vec![root];
    while let Some(r) = todo.pop() {
        let name = r.get("receiver").and_then(Value::as_str).unwrap_or("");
        if receiver(name).is_none() {
            finding(
                out,
                AM,
                "R10",
                format!("a route names receiver {name:?}, which does not exist"),
            );
        }
        todo.extend(
            r.get("routes")
                .and_then(Value::as_array)
                .into_iter()
                .flatten(),
        );
    }
    let root_receiver = root.get("receiver").and_then(Value::as_str);
    if root_receiver != Some(DEFAULT_RECEIVER) {
        finding(
            out,
            AM,
            "R10",
            format!("route.receiver must be {DEFAULT_RECEIVER} (found {root_receiver:?})"),
        );
    }
    if let Some(r) = receiver(DEFAULT_RECEIVER) {
        let i = integrations(r);
        if !i.is_empty() {
            finding(
                out,
                AM,
                "R10",
                format!("the default receiver {DEFAULT_RECEIVER} must send nowhere (found {i:?})"),
            );
        }
    }
    let group_by = strs(root.get("group_by")).unwrap_or_default();
    if GROUP_BY.iter().any(|g| !group_by.contains(g)) || group_by.contains(&"...") {
        finding(
            out,
            AM,
            "R10",
            format!("route.group_by must hold {GROUP_BY:?} (found {group_by:?})"),
        );
    }
    for (k, _) in root.as_object().into_iter().flatten() {
        if !ROOT_KEYS.contains(&k.as_str()) {
            finding(out, AM, "R10", format!("route: key {k} is not allowed"));
        }
    }
    let children: Vec<&Value> = root
        .get("routes")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .collect();
    if children.len() != ROUTES.len() {
        finding(
            out,
            AM,
            "R10",
            format!(
                "route.routes must be exactly {} routes (found {})",
                ROUTES.len(),
                children.len()
            ),
        );
    }
    for (i, (c, &(want, to))) in children.iter().zip(ROUTES).enumerate() {
        let got = matchers(c.get("matchers"));
        let want_m = matcher(want).map(|m| vec![m]);
        let got_to = c.get("receiver").and_then(Value::as_str);
        if got != want_m || got_to != Some(to) {
            finding(
                out,
                AM,
                "R10",
                format!(
                    "route {} must match only {want} and go to {to} (found {:?} -> {got_to:?})",
                    i + 1,
                    c.get("matchers")
                ),
            );
        }
        for (k, _) in c.as_object().into_iter().flatten() {
            if !CHILD_KEYS.contains(&k.as_str()) {
                finding(
                    out,
                    AM,
                    "R10",
                    format!("route {}: key {k} is not allowed", i + 1),
                );
            }
        }
    }
    for &(name, prefix, silent, resolved) in TELEGRAM {
        let Some(r) = receiver(name) else {
            finding(out, AM, "R10", format!("receiver {name} is missing"));
            continue;
        };
        let configs: Vec<&Value> = r
            .get("telegram_configs")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .collect();
        let [c] = configs[..] else {
            finding(
                out,
                AM,
                "R10",
                format!(
                    "{name} must have one telegram config (found {})",
                    configs.len()
                ),
            );
            continue;
        };
        let message = c.get("message").and_then(Value::as_str).unwrap_or("");
        // Alertmanager's defaults: notifications on, resolved messages on.
        let got_silent = c.get("disable_notifications").map(Value::as_bool);
        let got_resolved = c.get("send_resolved").map_or(Some(true), Value::as_bool);
        let plain = c.get("parse_mode").and_then(Value::as_str) == Some("");
        if !message.starts_with(prefix)
            || got_silent.unwrap_or(Some(false)) != Some(silent)
            || got_resolved != Some(resolved)
            || !plain
        {
            finding(
                out,
                AM,
                "R10",
                format!(
                    "{name}: message must start with {prefix:?}, disable_notifications {silent}, send_resolved {resolved}, parse_mode \"\""
                ),
            );
        }
    }
}

// Tests.

use serde_json::json;

use crate::assert_rules;

/// The real tree with the Alertmanager config changed through its JSON
/// value.
fn edit(t: &mut Tree, f: impl FnOnce(&mut Value)) {
    let mut v: Value = serde_json::from_str(&t.files[AM]).unwrap();
    f(&mut v);
    t.set(AM, &serde_json::to_string_pretty(&v).unwrap());
}

fn plant(f: impl FnOnce(&mut Value), rules: &[&str]) {
    let mut t = Tree::real();
    edit(&mut t, f);
    assert_rules(&crate::check(&t), rules);
}

fn receiver_mut<'a>(v: &'a mut Value, name: &str) -> &'a mut Value {
    v["receivers"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|r| r["name"] == name)
        .unwrap_or_else(|| panic!("no receiver {name}"))
}

fn telegram_mut<'a>(v: &'a mut Value, name: &str) -> &'a mut Value {
    &mut receiver_mut(v, name)["telegram_configs"][0]
}

fn routes_mut(v: &mut Value) -> &mut Vec<Value> {
    v["route"]["routes"].as_array_mut().unwrap()
}

#[test]
fn the_routes_send_watchdog_p1_p2_where_they_belong() {
    // The Watchdog route after the P1 route.
    plant(|v| routes_mut(v).swap(0, 1), &["R10"]);
    // The Watchdog to Telegram.
    plant(
        |v| routes_mut(v)[0]["receiver"] = json!("telegram-p1"),
        &["R10"],
    );
    // The Watchdog route matches more than the Watchdog.
    plant(
        |v| routes_mut(v)[0]["matchers"] = json!(["alertname=~\"Watch.*\""]),
        &["R10"],
    );
    plant(
        |v| routes_mut(v)[0]["matchers"] = json!(["alertname=\"Watchdog\"", "venue=\"bybit\""]),
        &["R10"],
    );
    // P1 by a regex, or to the P2 receiver.
    plant(
        |v| routes_mut(v)[1]["matchers"] = json!(["severity=~\"P1\""]),
        &["R10"],
    );
    plant(
        |v| routes_mut(v)[1]["receiver"] = json!("telegram-p2"),
        &["R10"],
    );
    // A route that goes on to the next one, a nested route, a muting key.
    plant(|v| routes_mut(v)[0]["continue"] = json!(true), &["R10"]);
    plant(
        |v| routes_mut(v)[1]["routes"] = json!([{"receiver": "null"}]),
        &["R10"],
    );
    plant(
        |v| routes_mut(v)[1]["mute_time_intervals"] = json!(["night"]),
        &["R10"],
    );
    // A fifth route, a missing one.
    plant(
        |v| routes_mut(v).push(json!({"matchers": ["severity=\"P3\""], "receiver": "null"})),
        &["R10"],
    );
    plant(
        |v| {
            routes_mut(v).pop();
        },
        &["R10"],
    );
    // The default receiver sends somewhere, or is another receiver.
    plant(
        |v| {
            let tg = telegram_mut(v, "telegram-p2").clone();
            receiver_mut(v, "null")["telegram_configs"] = json!([tg]);
        },
        &["R10"],
    );
    plant(|v| v["route"]["receiver"] = json!("telegram-p2"), &["R10"]);
    // A receiver a route names does not exist; a name given twice.
    plant(
        |v| receiver_mut(v, "deadman")["name"] = json!("dead-man"),
        &["R10"],
    );
    plant(
        |v| {
            v["receivers"]
                .as_array_mut()
                .unwrap()
                .push(json!({"name": "deadman"}))
        },
        &["R10"],
    );
    // Grouping without the venue: one message for every venue's alerts.
    plant(|v| v["route"]["group_by"] = json!(["alertname"]), &["R10"]);
    plant(|v| v["route"]["group_by"] = json!(["..."]), &["R10"]);
    // A child that groups by something else.
    plant(
        |v| routes_mut(v)[3]["group_by"] = json!(["alertname"]),
        &["R10"],
    );
    // Message prefixes, P2 silent and P1 loud, plain text.
    plant(
        |v| {
            let m = telegram_mut(v, "telegram-p1")["message"]
                .as_str()
                .unwrap()
                .replacen("[MON P1] ", "[MON P2] ", 1);
            telegram_mut(v, "telegram-p1")["message"] = json!(m);
        },
        &["R10"],
    );
    plant(
        |v| {
            let m = telegram_mut(v, "telegram-p2")["message"]
                .as_str()
                .unwrap()
                .replacen("[MON P2] ", "[MON P2]", 1);
            telegram_mut(v, "telegram-p2")["message"] = json!(m);
        },
        &["R10"],
    );
    plant(
        |v| telegram_mut(v, "telegram-p2")["disable_notifications"] = json!(false),
        &["R10"],
    );
    plant(
        |v| {
            telegram_mut(v, "telegram-p2")
                .as_object_mut()
                .unwrap()
                .remove("disable_notifications");
        },
        &["R10"],
    );
    plant(
        |v| telegram_mut(v, "telegram-p1")["disable_notifications"] = json!(true),
        &["R10"],
    );
    plant(
        |v| telegram_mut(v, "telegram-p1")["parse_mode"] = json!("HTML"),
        &["R10"],
    );
    plant(
        |v| {
            telegram_mut(v, "telegram-p1")
                .as_object_mut()
                .unwrap()
                .remove("parse_mode");
        },
        &["R10"],
    );
    // A second Telegram config on the P1 receiver.
    plant(
        |v| {
            let tg = telegram_mut(v, "telegram-p1").clone();
            receiver_mut(v, "telegram-p1")["telegram_configs"]
                .as_array_mut()
                .unwrap()
                .push(tg);
        },
        &["R10"],
    );
    // No route tree at all; the file gone (its install entry then names a
    // missing file: U14).
    plant(
        |v| {
            v.as_object_mut().unwrap().remove("route");
        },
        &["R10"],
    );
    let mut t = Tree::real();
    t.remove(AM);
    assert_rules(&crate::check(&t), &["R10", "U14"]);
    // Spaces around a matcher's operator are the same matcher.
    plant(
        |v| routes_mut(v)[1]["matchers"] = json!(["severity = \"P1\""]),
        &[],
    );
}
