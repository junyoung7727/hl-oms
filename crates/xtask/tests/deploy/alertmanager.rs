//! Alertmanager rules (ADR-035 D8): R10 the route tree, the receivers and
//! the inhibit rules; R11 secrets only as file paths under `/etc/`, and no
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
/// R10: the child routes, in this order: the one matcher, the receiver,
/// `group_wait` / `group_interval` / `repeat_interval` (spec D8; the
/// timings are 판단(모델 아님), each one written out: a missing one would
/// take Alertmanager's default). The `LakeAuditFailed` route comes before
/// the P2 route: that alert ends by itself after 26 h while its file may
/// still fail, so its receiver sends no resolved message.
const ROUTES: &[(&str, &str, [&str; 3])] = &[
    ("alertname=\"Watchdog\"", "deadman", ["0s", "1m", "1m"]),
    ("severity=\"P1\"", "telegram-p1", ["10s", "1m", "30m"]),
    (
        "alertname=\"LakeAuditFailed\"",
        "telegram-p2-noresolve",
        ["30s", "5m", "4h"],
    ),
    ("severity=\"P2\"", "telegram-p2", ["30s", "5m", "4h"]),
];
const TIMINGS: [&str; 3] = ["group_wait", "group_interval", "repeat_interval"];
/// R10: the Watchdog's receiver. It sends nowhere until the owner chooses
/// the outside dead-man service (Q1); that change replaces this rule.
const DEADMAN: &str = "deadman";
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
/// R10: the inhibit rules, in this order (design §8.3 B3), each with
/// `equal: ["venue"]`: (source, targets). A dead collector holds back the
/// P2 and P3 alerts derived from its own pushes (`md_*`,
/// `lake_seal_lag_hours`, `lake_disk_free_bytes`, its error log lines); the
/// qdp audit alerts stay visible. A dead projector holds back the journal
/// gauge alert, whose only writer it is. Market-data alerts are not held
/// back while the OMS is down: the collector does not need the OMS
/// (ADR-037).
const INHIBITS: &[(&[&str], &[&str])] = &[
    (
        &["alertname=\"MdCollectorDown\""],
        &[
            "severity=~\"P2|P3\"",
            "alertname=~\"Md.*|LakeSealLag|LakeDiskDaysLeft.*\"",
        ],
    ),
    (
        &["alertname=\"ProjectorDown\""],
        &[
            "severity=~\"P2|P3\"",
            "alertname=~\"JournalDiskGaugeMissing\"",
        ],
    ),
];
const INHIBIT_NAMES: &str = "Md.*|LakeSealLag|LakeDiskDaysLeft.*";
/// Metrics the collector does not push: an alert on one of them is no
/// collector alert (`obs_push_*` only with `service="md-collector"`).
const NOT_COLLECTOR: &[&str] = &["lake_audit_", "oms_", "projector_", "watcher_", "journal_"];
const INHIBIT_EQUAL: &[&str] = &["venue"];
const INHIBIT_KEYS: &[&str] = &["source_matchers", "target_matchers", "equal"];
/// R11: keys that would hold a secret in the file, or send it elsewhere
/// (another API host, a proxy). A webhook address is a secret too (a
/// dead-man ping address keeps the dead-man quiet): only `url_file`.
const FORBIDDEN_KEYS: &[&str] = &[
    "bot_token",
    "chat_id",
    "api_url",
    "http_config",
    "telegram_api_url",
    "url",
];
/// R11: the keys of every Telegram config that name its secret files.
const SECRET_FILES: &[&str] = &["bot_token_file", "chat_id_file"];


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

/// Whether the anchored pattern `re` matches `name`, for the patterns the
/// pinned inhibit rule uses: alternatives of a literal or a literal and
/// `.*`.
pub fn regex_matches(re: &str, name: &str) -> bool {
    re.split('|').any(|alt| match alt.strip_suffix(".*") {
        Some(prefix) => name.starts_with(prefix),
        None => name == alt,
    })
}

/// `[0-9]{6,}:[A-Za-z0-9_-]{30,}` somewhere in `text`: a Telegram bot
/// token.
pub fn token_shape(text: &str) -> bool {
    let c: Vec<char> = text.chars().collect();
    (0..c.len()).filter(|&i| c[i] == ':').any(|i| {
        let digits = c[..i]
            .iter()
            .rev()
            .take_while(|x| x.is_ascii_digit())
            .count();
        let rest = c[i + 1..]
            .iter()
            .take_while(|x| x.is_ascii_alphanumeric() || **x == '_' || **x == '-')
            .count();
        digits >= 6 && rest >= 30
    })
}

/// A key holding `chat` (any case) whose value is 6 or more digits, with
/// an optional minus (a group chat id): `chat_id: -100…`, `"chat": "…"`,
/// `CHAT_ID=…`.
pub fn chat_id_value(text: &str) -> bool {
    let c: Vec<char> = text.to_ascii_lowercase().chars().collect();
    let key = ['c', 'h', 'a', 't'];
    (0..c.len()).filter(|&i| c[i..].starts_with(&key)).any(|i| {
        let mut j = i + key.len();
        let skip = |j: &mut usize, f: &dyn Fn(char) -> bool| {
            while *j < c.len() && f(c[*j]) {
                *j += 1;
            }
        };
        skip(&mut j, &|x| {
            x.is_ascii_alphanumeric() || x == '_' || x == '-'
        });
        skip(&mut j, &|x| x == '"' || x == '\'');
        skip(&mut j, &|x| x == ' ' || x == '\t');
        if j >= c.len() || (c[j] != ':' && c[j] != '=') {
            return false;
        }
        j += 1;
        skip(&mut j, &|x| x == ' ' || x == '\t');
        skip(&mut j, &|x| x == '"' || x == '\'');
        if j < c.len() && c[j] == '-' {
            j += 1;
        }
        let from = j;
        skip(&mut j, &|x| x.is_ascii_digit());
        j - from >= 6
    })
}

pub fn check(t: &Tree, p: &Parsed, out: &mut Vec<Finding>) {
    r11_text(t, out);
    let Some(doc) = p.json.get(AM) else {
        finding(out, AM, "R10", "missing");
        return;
    };
    r10_routes(doc, out);
    r10_inhibit(p, doc, out);
    r11_keys(doc, out);
    r11_files(doc, out);
}

/// No deploy file holds a token or a chat id. No R11 finding repeats the
/// value it found.
fn r11_text(t: &Tree, out: &mut Vec<Finding>) {
    for (path, text) in t.files.iter().filter(|(k, _)| k.starts_with("deploy/")) {
        if token_shape(text) {
            finding(
                out,
                path,
                "R11",
                "holds a Telegram token shape (digits:characters)",
            );
        }
        if chat_id_value(text) {
            finding(
                out,
                path,
                "R11",
                "holds a chat id (6 or more digits after a chat key)",
            );
        }
    }
}

fn r11_keys(doc: &Value, out: &mut Vec<Finding>) {
    let mut todo = vec![doc];
    while let Some(v) = todo.pop() {
        match v {
            Value::Object(m) => {
                for (k, x) in m {
                    if FORBIDDEN_KEYS.contains(&k.as_str()) {
                        finding(
                            out,
                            AM,
                            "R11",
                            format!(
                                "key {k} is not allowed: secrets only as *_file paths, the default API only"
                            ),
                        );
                    }
                    todo.push(x);
                }
            }
            Value::Array(a) => todo.extend(a),
            _ => {}
        }
    }
}

fn r11_files(doc: &Value, out: &mut Vec<Finding>) {
    let configs = doc
        .get("receivers")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|r| r.get("telegram_configs").and_then(Value::as_array))
        .flatten();
    for c in configs {
        for key in SECRET_FILES {
            let path = c.get(*key).and_then(Value::as_str);
            let ok = path.is_some_and(|p| {
                p.strip_prefix("/etc/").is_some_and(|rest| {
                    !rest.is_empty()
                        && !rest.ends_with('/')
                        && rest
                            .split('/')
                            .all(|s| !s.is_empty() && s != "." && s != "..")
                })
            });
            if !ok {
                finding(
                    out,
                    AM,
                    "R11",
                    // Not the value: it may be the secret itself.
                    format!(
                        "every telegram config needs {key}, a file path under /etc/ (the value is missing or is no such path)"
                    ),
                );
            }
        }
    }
}

fn sorted(v: Option<Vec<Matcher>>) -> Option<Vec<Matcher>> {
    v.map(|mut v| {
        v.sort();
        v
    })
}

fn r10_inhibit(p: &Parsed, doc: &Value, out: &mut Vec<Finding>) {
    let rules: Vec<&Value> = doc
        .get("inhibit_rules")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .collect();
    let want = |l: &[&str]| sorted(l.iter().map(|m| matcher(m)).collect());
    let one = |r: &Value, (source, target): (&[&str], &[&str])| {
        sorted(matchers(r.get("source_matchers"))) == want(source)
            && sorted(matchers(r.get("target_matchers"))) == want(target)
            && strs(r.get("equal")).as_deref() == Some(INHIBIT_EQUAL)
            && r.as_object()
                .into_iter()
                .flatten()
                .all(|(k, _)| INHIBIT_KEYS.contains(&k.as_str()))
    };
    let ok = rules.len() == INHIBITS.len() && rules.iter().zip(INHIBITS).all(|(r, &w)| one(r, w));
    if !ok {
        finding(
            out,
            AM,
            "R10",
            format!(
                "inhibit_rules must be exactly these rules, in this order, each with equal {INHIBIT_EQUAL:?}: {INHIBITS:?}"
            ),
        );
    }
    let (rules, _) = crate::rules::collect(p);
    let alerts: Vec<&str> = rules.iter().filter_map(|r| r.alert()).collect();
    // Every name of a pinned target pattern is an alert of the rule files.
    for (_, target) in INHIBITS {
        for m in target.iter().filter_map(|m| matcher(m)) {
            if m.name != "alertname" {
                continue;
            }
            for alt in m.value.split('|') {
                if !alerts.iter().any(|a| regex_matches(alt, a)) {
                    finding(
                        out,
                        AM,
                        "R10",
                        format!("inhibit target {alt} names no alert of the rule files"),
                    );
                }
            }
        }
    }
    // An alert on a metric the collector does not push, whose name the
    // collector's pattern matches, would go quiet while the collector is
    // down.
    for r in &rules {
        let Some(name) = r.alert() else { continue };
        if !regex_matches(INHIBIT_NAMES, name) || r.vlogs {
            continue;
        }
        let s = crate::expr::scan(r.expr().unwrap_or(""));
        let other = s
            .names
            .iter()
            .find(|n| NOT_COLLECTOR.iter().any(|p| n.starts_with(p)));
        let all_services = s.names.iter().any(|n| n.starts_with("obs_push_"))
            && !s
                .matchers
                .iter()
                .any(|(l, op, v)| l == "service" && op == "=" && v == "md-collector");
        if let Some(n) = other {
            finding(
                out,
                r.file,
                "R10",
                format!("{name} reads {n}, which the collector does not push, but the inhibit rule holds it back with the collector"),
            );
        } else if all_services {
            finding(
                out,
                r.file,
                "R10",
                format!("{name} reads obs_push_* without service=\"md-collector\": another service's drops would be held back with the collector"),
            );
        }
    }
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
    for name in [DEFAULT_RECEIVER, DEADMAN] {
        let Some(r) = receiver(name) else { continue };
        let i = integrations(r);
        if !i.is_empty() {
            finding(
                out,
                AM,
                "R10",
                format!("receiver {name} must send nowhere (found {i:?})"),
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
    for (i, (c, &(want, to, timings))) in children.iter().zip(ROUTES).enumerate() {
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
        for (key, value) in TIMINGS.iter().zip(timings) {
            let got = c.get(*key).and_then(Value::as_str);
            if got != Some(value) {
                finding(
                    out,
                    AM,
                    "R10",
                    format!("route {}: {key} must be {value} (found {got:?})", i + 1),
                );
            }
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
        let i = integrations(r);
        if i != ["telegram_configs"] {
            finding(
                out,
                AM,
                "R10",
                format!("{name} must have telegram_configs only (found {i:?})"),
            );
        }
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
    // The Watchdog route after the P1 route, with or without its timings.
    plant(|v| routes_mut(v).swap(0, 1), &["R10"]);
    plant(
        |v| {
            let r = routes_mut(v);
            for key in ["matchers", "receiver"] {
                let x = r[0][key].take();
                r[0][key] = std::mem::replace(&mut r[1][key], x);
            }
        },
        &["R10"],
    );
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
    // A route's timings differ from the design (spec D8): a dead
    // collector's P1 message 10 hours late, repeated every 1000 hours; a
    // missing timing takes Alertmanager's default (30s / 5m / 4h).
    plant(|v| routes_mut(v)[1]["group_wait"] = json!("10h"), &["R10"]);
    plant(
        |v| routes_mut(v)[1]["repeat_interval"] = json!("1000h"),
        &["R10"],
    );
    plant(
        |v| {
            routes_mut(v)[1]
                .as_object_mut()
                .unwrap()
                .remove("group_wait");
        },
        &["R10"],
    );
    plant(
        |v| routes_mut(v)[0]["repeat_interval"] = json!("1h"),
        &["R10"],
    );
    plant(
        |v| routes_mut(v)[2]["group_interval"] = json!("1m"),
        &["R10"],
    );
    plant(|v| routes_mut(v)[3]["group_wait"] = json!("30m"), &["R10"]);
    // A key on the root route; `...` next to the two labels (one group for
    // every label set).
    plant(
        |v| v["route"]["mute_time_intervals"] = json!(["night"]),
        &["R10"],
    );
    plant(
        |v| v["route"]["group_by"] = json!(["alertname", "venue", "..."]),
        &["R10"],
    );
    // The dead-man receiver sends somewhere before the owner chooses the
    // outside service (Q1): the Watchdog to Telegram every minute.
    plant(
        |v| {
            let tg = telegram_mut(v, "telegram-p2").clone();
            receiver_mut(v, "deadman")["telegram_configs"] = json!([tg]);
        },
        &["R10"],
    );
    // A Telegram receiver with a second kind of integration.
    plant(
        |v| {
            receiver_mut(v, "telegram-p1")["webhook_configs"] =
                json!([{"url_file": "/etc/hl-oms/other-url"}])
        },
        &["R10"],
    );
    // Spaces around a matcher's operator are the same matcher.
    plant(
        |v| routes_mut(v)[1]["matchers"] = json!(["severity = \"P1\""]),
        &[],
    );
}

#[test]
fn the_audit_failure_route_sends_no_resolved_message() {
    let noresolve = "telegram-p2-noresolve";
    plant(
        |v| telegram_mut(v, noresolve)["send_resolved"] = json!(true),
        &["R10"],
    );
    // Without the key Alertmanager sends resolved messages.
    plant(
        |v| {
            telegram_mut(v, noresolve)
                .as_object_mut()
                .unwrap()
                .remove("send_resolved");
        },
        &["R10"],
    );
    // The P2 route first would take LakeAuditFailed (severity P2).
    plant(|v| routes_mut(v).swap(2, 3), &["R10"]);
    plant(
        |v| routes_mut(v)[2]["receiver"] = json!("telegram-p2"),
        &["R10"],
    );
    plant(
        |v| {
            routes_mut(v).remove(2);
        },
        &["R10"],
    );
    // Its receiver is silent like every P2 receiver.
    plant(
        |v| telegram_mut(v, noresolve)["disable_notifications"] = json!(false),
        &["R10"],
    );
}

fn inhibit_mut(v: &mut Value) -> &mut Value {
    &mut v["inhibit_rules"][0]
}

/// The projector's inhibit rule.
fn projector(v: &mut Value) -> &mut Value {
    &mut v["inhibit_rules"][1]
}

#[test]
fn the_inhibit_rule_holds_back_only_collector_alerts() {
    // Widened to every alert: the audit alerts would go quiet with the
    // collector.
    plant(
        |v| inhibit_mut(v)["target_matchers"] = json!(["severity=~\"P2|P3\"", "alertname=~\".*\""]),
        &["R10"],
    );
    plant(
        |v| {
            inhibit_mut(v)["target_matchers"] =
                json!(["severity=~\"P2|P3\"", "alertname=~\"Md.*|Lake.*\""])
        },
        &["R10"],
    );
    // Without the alertname matcher, or holding back P1 alerts too.
    plant(
        |v| inhibit_mut(v)["target_matchers"] = json!(["severity=~\"P2|P3\""]),
        &["R10"],
    );
    plant(
        |v| {
            inhibit_mut(v)["target_matchers"] = json!([
                "severity=~\"P1|P2|P3\"",
                "alertname=~\"Md.*|LakeSealLag|LakeDiskDaysLeft.*\""
            ])
        },
        &["R10"],
    );
    // Another source; another venue's collector; no venue match at all.
    plant(
        |v| inhibit_mut(v)["source_matchers"] = json!(["alertname=\"LakeSealLag\""]),
        &["R10"],
    );
    plant(|v| inhibit_mut(v)["equal"] = json!([]), &["R10"]);
    plant(
        |v| {
            inhibit_mut(v).as_object_mut().unwrap().remove("equal");
        },
        &["R10"],
    );
    // An old-style matcher key next to the pinned ones; a second rule; none.
    plant(
        |v| inhibit_mut(v)["target_match_re"] = json!({"alertname": ".*"}),
        &["R10"],
    );
    plant(
        |v| {
            let r = inhibit_mut(v).clone();
            v["inhibit_rules"].as_array_mut().unwrap().push(r);
        },
        &["R10"],
    );
    plant(
        |v| {
            v.as_object_mut().unwrap().remove("inhibit_rules");
        },
        &["R10"],
    );
    // An alert on a qdp metric whose name the rule matches would be held
    // back during a collector outage.
    let md = "deploy/observability/vmalert/metrics/market-data.rules.json";
    let mut t = Tree::real();
    let mut v: Value = serde_json::from_str(&t.files[md]).unwrap();
    v["groups"][0]["rules"].as_array_mut().unwrap().push(json!({
        "alert": "MdAuditLate",
        "expr": "time() - max by (venue) (lake_audit_last_run_ts) > 129600",
        "labels": {"severity": "P2"},
        "annotations": {"summary": "x"}
    }));
    t.set(md, &serde_json::to_string_pretty(&v).unwrap());
    assert_rules(&crate::check(&t), &["R10"]);
    // The pinned pattern, read as Alertmanager reads it (anchored).
    let re = "Md.*|LakeSealLag|LakeDiskDaysLeft.*";
    for (name, held) in [
        ("MdGapsHigh", true),
        ("MdCollectorDown", true),
        ("LakeSealLag", true),
        ("LakeSealLagged", false),
        ("LakeDiskDaysLeftLow", true),
        ("LakeAuditFailed", false),
        ("LakeNotAudited", false),
        ("XMd", false),
    ] {
        assert_eq!(regex_matches(re, name), held, "{name}");
    }
}

/// The real tree with one rule file changed through its JSON value.
fn edit_rules(t: &mut Tree, file: &str, f: impl FnOnce(&mut Value)) {
    let mut v: Value = serde_json::from_str(&t.files[file]).unwrap();
    f(&mut v);
    t.set(file, &serde_json::to_string_pretty(&v).unwrap());
}

#[test]
fn the_oms_inhibit_rules_hold_back_only_derived_alerts() {
    // Widened to every P2 and P3 alert: a dead projector would hide the
    // collector's and the audit's alerts too.
    plant(
        |v| projector(v)["target_matchers"] = json!(["severity=~\"P2|P3\"", "alertname=~\".*\""]),
        &["R10"],
    );
    // P1 too; another source; no venue match.
    plant(
        |v| {
            projector(v)["target_matchers"] =
                json!(["severity=~\"P1|P2|P3\"", "alertname=~\"JournalDiskGaugeMissing\""])
        },
        &["R10"],
    );
    plant(
        |v| projector(v)["source_matchers"] = json!(["alertname=\"OmsDown\""]),
        &["R10"],
    );
    plant(|v| projector(v)["equal"] = json!([]), &["R10"]);
    // A third rule; the two in the other order; the projector's rule gone.
    plant(
        |v| {
            let r = projector(v).clone();
            v["inhibit_rules"].as_array_mut().unwrap().push(r);
        },
        &["R10"],
    );
    plant(
        |v| v["inhibit_rules"].as_array_mut().unwrap().swap(0, 1),
        &["R10"],
    );
    plant(
        |v| {
            v["inhibit_rules"].as_array_mut().unwrap().pop();
        },
        &["R10"],
    );
    // A stale name: the rule file's alert renamed (R6 sees the venue's
    // rule gone).
    let oms = "deploy/observability/vmalert/metrics/oms.rules.json";
    let mut t = Tree::real();
    t.replace(
        oms,
        "\"alert\": \"JournalDiskGaugeMissing\"",
        "\"alert\": \"JournalDiskGaugeGone\"",
    );
    let f = crate::check(&t);
    assert_rules(&f, &["R6", "R10"]);
    assert!(
        f.iter()
            .any(|x| x.rule == "R10" && x.text.contains("JournalDiskGaugeMissing")),
        "{f:?}"
    );
    // The collector's pattern widened: OMS alerts go quiet with the
    // collector.
    plant(
        |v| inhibit_mut(v)["target_matchers"] = json!(["severity=~\"P2|P3\"", "alertname=~\".*\""]),
        &["R10"],
    );
    // An Md* alert that reads an OMS, projector, journal or watcher series
    // (each prefix of the list), or every service's drops. The watcher's
    // series join the catalogue in a later part: one row is planted here.
    let md = "deploy/observability/vmalert/metrics/market-data.rules.json";
    for e in [
        "max by (venue) (oms_holds) > 0",
        "max by (venue) (projector_lag_records) > 100",
        "min by (venue) (journal_disk_free_bytes) < 1",
        "max by (venue) (watcher_up) == 0",
        "sum by (venue, endpoint) (increase(obs_push_dropped_total[10m])) > 0",
        "sum by (venue, endpoint) (increase(obs_push_dropped_total{service=\"oms-projector\"}[10m])) > 0",
    ] {
        let mut t = Tree::real();
        let catalogue = "deploy/observability/metrics.toml";
        let mut rows = t.files[catalogue].clone();
        rows.push_str(
            "\n[[metric]]\nname = \"watcher_up\"\nlabels = [\"venue\"]\nsource = \"crates/watcher/src/metrics.rs\"\n",
        );
        t.set(catalogue, &rows);
        edit_rules(&mut t, md, |v| {
            let r = v["groups"][0]["rules"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .find(|r| r["alert"] == "MdObsPushDropping")
                .unwrap();
            r["expr"] = json!(e);
        });
        assert_rules(&crate::check(&t), &["R10"]);
    }
}

/// The real tree with `line` added at the end of a deploy file.
fn append(path: &str, line: &str) -> Vec<Finding> {
    let mut t = Tree::real();
    let text = format!("{}{line}\n", t.files[path]);
    t.set(path, &text);
    crate::check(&t)
}

#[test]
fn an_inline_token_or_chat_id_fails() {
    // Inline secrets and a redirected API in the Alertmanager config. The
    // chat id has five digits, so only the key gives it away.
    for (key, value) in [
        ("bot_token", json!("x")),
        ("chat_id", json!(12345)),
        ("api_url", json!("http://127.0.0.1:1/")),
        ("http_config", json!({"proxy_url": "http://127.0.0.1:1/"})),
        ("url", json!("https://127.0.0.1:1/ping/x")),
    ] {
        plant(|v| telegram_mut(v, "telegram-p1")[key] = value, &["R11"]);
    }
    plant(
        |v| v["global"] = json!({"telegram_api_url": "http://127.0.0.1:1/"}),
        &["R11"],
    );
    // A dead-man ping address is a secret: whoever has it can keep the
    // dead-man quiet. Any integration on `deadman` is R10 until Q1.
    for webhook in [
        json!({"url_file": "/etc/hl-oms/deadman-url", "http_config": {"bearer_token": "x"}}),
        json!({"url": "https://127.0.0.1:1/ping/x"}),
    ] {
        plant(
            |v| receiver_mut(v, "deadman")["webhook_configs"] = json!([webhook]),
            &["R10", "R11"],
        );
    }
    // A token shape or a chat id in any deploy file, text or comment. Built
    // here so this file holds no such string.
    let token = format!("{}:{}", "7".repeat(10), "a".repeat(35));
    let chat = format!("chat_id: -{}", "1".repeat(10));
    let readme = "deploy/observability/README.md";
    let unit = "deploy/observability/systemd/alertmanager.service";
    for (path, line) in [
        (readme, token.clone()),
        (unit, format!("# bot {token}")),
        (readme, chat.clone()),
        (readme, format!("\"chat\": \"{}\"", "2".repeat(6))),
        (unit, format!("# CHAT_ID={}", "3".repeat(9))),
        // The shortest shapes.
        (readme, format!("{}:{}", "7".repeat(6), "Z_-9".repeat(8))),
    ] {
        let f = append(path, &line);
        assert!(
            f.iter().any(|x| x.rule == "R11" && x.file == path),
            "{path}: {line}: {f:?}"
        );
        assert_rules(&f, &["R11"]);
        // The finding does not repeat the secret.
        assert!(f.iter().all(|x| !x.text.contains(&line)), "{f:?}");
    }
    // One digit or one character short of the shapes.
    for line in [
        format!("{}:{}", "7".repeat(5), "a".repeat(35)),
        format!("{}:{}", "7".repeat(10), "a".repeat(29)),
        format!("chat_id: {}", "1".repeat(5)),
        format!("chat_id_file: /etc/{}", "1".repeat(9)),
    ] {
        assert_rules(&append(readme, &line), &[]);
    }
}

#[test]
fn secret_files_are_absolute_paths_under_etc() {
    for (key, value) in [
        ("bot_token_file", json!("alertmanager-telegram-token")),
        ("chat_id_file", json!("/home/x/chat-id")),
        ("bot_token_file", json!("/etc/../home/x/token")),
        ("chat_id_file", json!("/etc/")),
        ("bot_token_file", json!(1)),
    ] {
        plant(|v| telegram_mut(v, "telegram-p2")[key] = value, &["R11"]);
    }
    for key in ["bot_token_file", "chat_id_file"] {
        plant(
            |v| {
                telegram_mut(v, "telegram-p2-noresolve")
                    .as_object_mut()
                    .unwrap()
                    .remove(key);
            },
            &["R11"],
        );
    }
    // A secret pasted as the value of its file key: R11 says so without
    // repeating the value (the check's findings reach the CI log).
    let token = format!("{}:{}", "7".repeat(10), "a".repeat(35));
    let chat = format!("-{}", "1".repeat(13));
    for (key, secret) in [("bot_token_file", &token), ("chat_id_file", &chat)] {
        let mut t = Tree::real();
        edit(&mut t, |v| {
            telegram_mut(v, "telegram-p1")[key] = json!(secret)
        });
        let f = crate::check(&t);
        assert_rules(&f, &["R11"]);
        assert!(
            f.iter().any(|x| x.rule == "R11" && x.text.contains(key)),
            "{f:?}"
        );
        assert!(f.iter().all(|x| !x.text.contains(secret.as_str())), "{key}");
    }
    // Another file under /etc/ is the operator's choice.
    plant(
        |v| telegram_mut(v, "telegram-p1")["chat_id_file"] = json!("/etc/hl-oms/other-chat-id"),
        &[],
    );
}
