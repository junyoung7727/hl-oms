//! Registry: review items, decisions, contracts, claims, assumptions.
//!
//! `cargo run -p xtask -- docs` loads `registry/*.toml`, checks that every
//! cross-reference points at something that exists (an id, a file, a test, an
//! event name), joins the claims with the latest verification results, and
//! writes the generated tables under `docs/generated/` and the data files the
//! dashboard reads under `dashboard/data/`.
//!
//! A claim's status is computed, never written by hand:
//!
//! - passed: every test it names ran and passed, for the current source digest
//! - failed: at least one of them failed
//! - not run: at least one of them has no result
//! - stale: the results belong to a different source digest

use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

fn load(root: &Path, file: &str, key: &str) -> Result<Vec<Value>, String> {
    let path = root.join("registry").join(file);
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let v: toml::Value = toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    let j = serde_json::to_value(v).map_err(|e| e.to_string())?;
    Ok(j.get(key).and_then(|a| a.as_array()).cloned().unwrap_or_default())
}

fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v.get(k).and_then(|x| x.as_str()).unwrap_or("")
}

fn list(v: &Value, k: &str) -> Vec<String> {
    v.get(k)
        .and_then(|x| x.as_array())
        .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
        .unwrap_or_default()
}

fn ids(items: &[Value], what: &str, errors: &mut Vec<String>) -> BTreeSet<String> {
    let mut set = BTreeSet::new();
    for i in items {
        let id = s(i, "id").to_string();
        if id.is_empty() {
            errors.push(format!("{what}: an entry has no id"));
        } else if !set.insert(id.clone()) {
            errors.push(format!("{what}: duplicate id {id}"));
        }
    }
    set
}

/// Event names, read from the `Event::name` function in the core.
fn event_names(root: &Path) -> BTreeSet<String> {
    let text = std::fs::read_to_string(root.join("crates/oms-core/src/event.rs")).unwrap_or_default();
    text.lines()
        .filter(|l| l.trim_start().starts_with("Event::"))
        .filter_map(|l| l.split("=> \"").nth(1))
        .filter_map(|r| r.split('"').next())
        .map(String::from)
        .collect()
}

/// All test function names found in the sources of one suite.
fn test_names(root: &Path, dirs: &[&str]) -> BTreeSet<String> {
    fn walk(p: &Path, out: &mut BTreeSet<String>) {
        let Ok(rd) = std::fs::read_dir(p) else { return };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().is_some_and(|x| x == "rs") {
                let text = std::fs::read_to_string(&p).unwrap_or_default();
                for line in text.lines() {
                    // `async fn` too: a test under `#[tokio::test]`.
                    let declaration = line.trim_start().trim_start_matches("async ");
                    if let Some(rest) = declaration.strip_prefix("fn ") {
                        if let Some(name) = rest.split(['(', '<']).next() {
                            out.insert(name.trim().to_string());
                        }
                    }
                }
            }
        }
    }
    let mut out = BTreeSet::new();
    for d in dirs {
        walk(&root.join(d), &mut out);
    }
    out
}

fn suite_dirs(suite: &str) -> Option<&'static [&'static str]> {
    Some(match suite {
        "core" => &["crates/oms-core/src", "crates/oms-core/tests"],
        "journal" => &["crates/oms-journal/src", "crates/oms-journal/tests"],
        "api" => &["crates/oms-api/src"],
        "sim" | "sim-extended" => &["crates/oms-sim/src", "crates/oms-sim/tests"],
        "node-unit" => &["crates/oms-node/src", "crates/oms-node/tests"],
        "alert" => &["crates/alert-direct/src", "crates/alert-direct/tests"],
        "watcher" => &["crates/watcher/src", "crates/watcher/tests", "crates/watcher-conformance/tests"],
        "pipeline" => &["crates/oms-node/tests"],
        "e2e" | "kills-extended" => &["crates/oms-node/tests"],
        "hl-live" => &["crates/oms-node/tests"],
        _ => return None,
    })
}

struct Evidence {
    digest: String,
    results: BTreeMap<String, String>,
    raw: Value,
    perf: Vec<Value>,
    mutants: Option<Value>,
}

fn read_json(path: &Path) -> Option<Value> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

fn load_evidence(root: &Path) -> Evidence {
    let raw = read_json(&root.join("evidence/latest/results.json")).unwrap_or(Value::Null);
    let mut results = BTreeMap::new();
    for suite in raw.get("suites").and_then(|x| x.as_array()).into_iter().flatten() {
        let name = s(suite, "name");
        for t in suite.get("tests").and_then(|x| x.as_array()).into_iter().flatten() {
            results.insert(format!("{name}/{}", s(t, "name")), s(t, "result").to_string());
        }
    }
    if let Some(p) = raw.get("core_purity") {
        results.insert(
            "source/core_purity".into(),
            if p == "ok" { "passed" } else { "failed" }.into(),
        );
    }
    if let Some(scripts) = raw.get("source_scripts").and_then(|x| x.as_object()) {
        for (name, r) in scripts {
            results.insert(format!("source/{name}"), if r == "ok" { "passed" } else { "failed" }.into());
        }
    }
    let mut perf = Vec::new();
    if let Ok(rd) = std::fs::read_dir(root.join("evidence/perf")) {
        let mut paths: Vec<_> = rd.flatten().map(|e| e.path()).collect();
        paths.sort();
        for p in paths {
            if let Some(v) = read_json(&p) {
                perf.push(v);
            }
        }
    }
    Evidence {
        digest: s(&raw, "source_digest").to_string(),
        results,
        raw,
        perf,
        mutants: read_json(&root.join("evidence/mutants.json")),
    }
}

/// Judges a measurement claim from a perf report. Returns (status, detail).
fn judge_perf(claim_id: &str, test: &str, ev: &Evidence, digest: &str) -> (String, String) {
    let label = test.trim_start_matches("perf/");
    let Some(report) = ev.perf.iter().find(|r| s(r, "label") == label) else {
        return ("not_run".into(), format!("no evidence/perf/{label}.json"));
    };
    let fresh = s(report, "build").contains(digest);
    let stale = |st: &str| if fresh { st.to_string() } else { "stale".to_string() };
    if label == "recovery" {
        let (n, ms) = (
            report.get("records_replayed").and_then(|x| x.as_u64()).unwrap_or(0),
            report.get("elapsed_ms").and_then(|x| x.as_u64()).unwrap_or(u64::MAX),
        );
        let ok = n >= 50_000 && ms <= 5_000;
        return (stale(if ok { "passed" } else { "failed" }), format!("{n} records replayed in {ms} ms"));
    }
    if label.starts_with("fsync-") || label == "core-bench" {
        // A measurement kept as evidence, not a criterion: it counts as
        // present when it was taken from this source.
        let text = report
            .get("results")
            .and_then(|x| x.as_array())
            .map(|rs| {
                rs.iter()
                    .map(|r| format!("{}: p50 {} p99 {} {}", s(r, "name"), r["p50"], r["p99"], s(r, "unit")))
                    .collect::<Vec<_>>()
                    .join("; ")
            })
            .unwrap_or_default();
        return (stale("passed"), text);
    }
    let runs = report.get("runs").and_then(|x| x.as_array()).cloned().unwrap_or_default();
    let default_limits = label.ends_with("default-limits");
    let mut detail = Vec::new();
    let mut ok = !runs.is_empty();
    let mut relevant = 0;
    for r in &runs {
        let load = &r["load"];
        let rate = load["offered_rate"].as_u64().unwrap_or(0);
        let p99 = load["latency_intended_to_response"]["p99"].as_u64().unwrap_or(u64::MAX);
        let unanswered = load["unanswered"].as_u64().unwrap_or(1);
        let errors = load["errors"].as_object().map(|o| o.len()).unwrap_or(0);
        let hw = r["queues"]["high_water"].as_array().cloned().unwrap_or_default();
        let cap = r["queues"]["capacity"].as_array().cloned().unwrap_or_default();
        let within = hw.iter().zip(cap.iter()).all(|(h, c)| h.as_u64() <= c.as_u64());
        match claim_id {
            "V-PERF-1" if rate <= 200 => {
                relevant += 1;
                ok &= p99 <= 20_000 && unanswered == 0 && errors == 0;
                detail.push(format!("{rate}/s: p99 {:.2} ms, unanswered {unanswered}, error kinds {errors}", p99 as f64 / 1000.0));
            }
            "V-PERF-2" if rate >= 2000 && default_limits => {
                // Shipped limits: the excess must be refused, answered, and
                // leave no record. The core's counters are rebuilt from the
                // journal, so they count exactly the recorded decisions.
                relevant += 1;
                let refused = load["errors"]["Overloaded"].as_u64().unwrap_or(0);
                let only_overloaded = load["errors"].as_object().is_some_and(|o| o.keys().all(|k| k == "Overloaded"));
                let decided = r["core_stats"]["accepted"].as_u64().unwrap_or(0) + r["core_stats"]["rejected"].as_u64().unwrap_or(0);
                let answered_by_core = load["accepted"].as_u64().unwrap_or(0) + load["rejected"].as_u64().unwrap_or(0);
                let no_record = decided == answered_by_core;
                ok &= unanswered == 0 && within && refused > 0 && only_overloaded && no_record;
                detail.push(format!(
                    "{rate}/s with shipped limits: {refused} refused as overloaded, {answered_by_core} decided by the core, {decided} decisions in the journal, unanswered {unanswered}, queues within capacity: {within}"
                ));
            }
            // A comparison or a plain measurement kept as evidence: no
            // criterion, only that it exists for this source.
            "V-PERF-4" => {
                relevant += 1;
                detail.push(format!("{rate}/s [{}]: p99 {:.2} ms, unanswered {unanswered}, error kinds {errors}", s(report, "commit_layout"), p99 as f64 / 1000.0));
            }
            "V-PERF-2" if rate >= 2000 && !default_limits => {
                relevant += 1;
                ok &= unanswered == 0 && within;
                detail.push(format!("{rate}/s [{}, limits lifted]: p99 {:.2} ms, unanswered {unanswered}, queues within capacity: {within}, error kinds {errors}", s(report, "commit_layout"), p99 as f64 / 1000.0));
            }
            _ => {}
        }
    }
    if relevant == 0 {
        return ("not_run".into(), "no run at the required rate".into());
    }
    (stale(if ok { "passed" } else { "failed" }), detail.join("; "))
}

fn claim_status(claim: &Value, ev: &Evidence, digest: &str) -> (String, Vec<Value>) {
    let mut per_test = Vec::new();
    let (mut failed, mut missing, mut stale) = (false, false, false);
    for t in list(claim, "tests") {
        let (result, detail) = if t.starts_with("perf/") {
            judge_perf(s(claim, "id"), &t, ev, digest)
        } else if t == "mutants/all" {
            match &ev.mutants {
                None => ("not_run".to_string(), "no evidence/mutants.json".to_string()),
                Some(m) => {
                    let all = m.get("mutants").and_then(|x| x.as_array()).cloned().unwrap_or_default();
                    let survived: Vec<&str> = all.iter().filter(|x| x["killed"] != true).map(|x| s(x, "id")).collect();
                    let fresh = s(m, "source_digest") == digest;
                    let st = if !fresh {
                        "stale"
                    } else if survived.is_empty() && !all.is_empty() {
                        "passed"
                    } else {
                        "failed"
                    };
                    (st.to_string(), format!("{} mutants, survived: {survived:?}", all.len()))
                }
            }
        } else {
            match ev.results.get(&t) {
                Some(r) if ev.digest != digest => ("stale".to_string(), format!("last result: {r}")),
                Some(r) => (r.clone(), String::new()),
                None => ("not_run".to_string(), String::new()),
            }
        };
        match result.as_str() {
            "failed" => failed = true,
            "not_run" | "ignored" => missing = true,
            "stale" => stale = true,
            _ => {}
        }
        per_test.push(json!({ "test": t, "result": result, "detail": detail }));
    }
    let status = if failed {
        "failed"
    } else if missing {
        "not_run"
    } else if stale {
        "stale"
    } else if per_test.is_empty() {
        "not_run"
    } else {
        "passed"
    };
    (status.to_string(), per_test)
}

fn cell(text: &str) -> String {
    text.replace('|', "\\|").replace('\n', " ")
}

fn ko_status(st: &str) -> &'static str {
    match st {
        "passed" => "통과",
        "failed" => "실패",
        "not_run" => "미실행",
        "stale" => "이전 버전 결과",
        _ => "알 수 없음",
    }
}

pub fn run(root: &Path) -> i32 {
    let mut errors: Vec<String> = Vec::new();
    let mut get = |file: &str, key: &str| match load(root, file, key) {
        Ok(v) => v,
        Err(e) => {
            errors.push(e);
            Vec::new()
        }
    };
    let sources = get("sources.toml", "source");
    let mut considerations = get("considerations-general.toml", "item");
    considerations.extend(get("considerations-domain.toml", "item"));
    considerations.extend(get("considerations-project.toml", "item"));
    let decisions = get("decisions.toml", "adr");
    let contracts = get("contracts.toml", "contract");
    let claims = get("claims.toml", "claim");
    let assumptions = get("assumptions.toml", "assumption");
    let user_decisions = get("user-decisions.toml", "decision");
    let paths = get("paths.toml", "path");
    let coverage = get("coverage.toml", "row");

    let source_ids = ids(&sources, "sources", &mut errors);
    ids(&considerations, "considerations", &mut errors);
    let adr_ids = ids(&decisions, "decisions", &mut errors);
    let contract_ids = ids(&contracts, "contracts", &mut errors);
    let claim_ids = ids(&claims, "claims", &mut errors);
    ids(&assumptions, "assumptions", &mut errors);
    ids(&user_decisions, "user decisions", &mut errors);

    let exists = |p: &str| root.join(p).exists();
    let check_refs = |owner: &str, item: &Value, errors: &mut Vec<String>| {
        for a in list(item, "adr") {
            if !adr_ids.contains(&a) {
                errors.push(format!("{owner}: unknown decision {a}"));
            }
        }
        for c in list(item, "contracts") {
            if !contract_ids.contains(&c) {
                errors.push(format!("{owner}: unknown contract {c}"));
            }
        }
        for c in list(item, "claims") {
            if !claim_ids.contains(&c) {
                errors.push(format!("{owner}: unknown claim {c}"));
            }
        }
        for c in list(item, "code") {
            if !exists(&c) {
                errors.push(format!("{owner}: no such file {c}"));
            }
        }
        for c in list(item, "sources") {
            if !source_ids.contains(&c) {
                errors.push(format!("{owner}: unknown source {c}"));
            }
        }
    };
    let consideration_ids: Vec<String> = considerations.iter().map(|i| s(i, "id").to_string()).collect();
    for (n, row) in coverage.iter().enumerate() {
        let owner = format!("coverage row {} ({} / {})", n + 1, s(row, "source"), s(row, "element"));
        if !source_ids.contains(s(row, "source")) {
            errors.push(format!("{owner}: unknown source"));
        }
        let status = s(row, "status");
        if !["다룸", "비적용", "보류", "미검토"].contains(&status) {
            errors.push(format!("{owner}: status {status:?} is not one of 다룸, 비적용, 보류, 미검토"));
        }
        let items = list(row, "items");
        for i in &items {
            if !consideration_ids.contains(i) {
                errors.push(format!("{owner}: unknown item {i}"));
            }
        }
        if status == "다룸" && items.is_empty() {
            errors.push(format!("{owner}: covered but names no item"));
        }
        if status != "다룸" && s(row, "note").is_empty() {
            errors.push(format!("{owner}: needs a note (reason, or impact and condition to resume)"));
        }
    }
    for i in &considerations {
        let id = s(i, "id");
        check_refs(id, i, &mut errors);
        for (field, allowed) in [
            ("layer", &["general", "domain", "project"][..]),
            ("applicability", &["적용", "조건부", "비적용"][..]),
            ("design", &["결정", "미결", "해당없음"][..]),
            ("impl", &["구현", "부분", "미구현", "해당없음"][..]),
            ("verify", &["검증", "부분", "미검증", "해당없음"][..]),
        ] {
            if !allowed.contains(&s(i, field)) {
                errors.push(format!("{id}: {field} = {:?} is not one of {allowed:?}", s(i, field)));
            }
        }
        // A status other than fully done must say what is open.
        let incomplete = ["미결", "부분", "미구현", "미검증"]
            .iter()
            .any(|x| [s(i, "design"), s(i, "impl"), s(i, "verify")].contains(x));
        if incomplete && s(i, "open").is_empty() {
            errors.push(format!("{id}: not fully done but has no `open` text"));
        }
        // The brief asks for a reason with every applicability judgement, and
        // for impact, resolution and stage with everything left open.
        if s(i, "reason").is_empty() {
            errors.push(format!("{id}: no `reason` for its applicability"));
        }
        let open = s(i, "open");
        if !open.is_empty() {
            for marker in ["영향:", "해소:", "단계:"] {
                if !open.contains(marker) {
                    errors.push(format!("{id}: `open` does not state {marker}"));
                }
            }
        }
        if s(i, "verify") == "검증" && list(i, "claims").is_empty() {
            errors.push(format!("{id}: marked verified but names no claim"));
        }
    }
    for d in &decisions {
        if !exists(s(d, "file")) {
            errors.push(format!("{}: no such file {}", s(d, "id"), s(d, "file")));
        }
    }
    let events = event_names(root);
    let mut event_index: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for c in &contracts {
        let id = s(c, "id");
        check_refs(id, c, &mut errors);
        if !exists(s(c, "file")) {
            errors.push(format!("{id}: no such file {}", s(c, "file")));
        }
        for e in list(c, "events") {
            if !events.contains(&e) {
                errors.push(format!("{id}: unknown event {e}"));
            }
            event_index.entry(e).or_default().push(id.to_string());
        }
    }
    for p in &paths {
        for e in list(p, "steps") {
            if !events.contains(&e) {
                errors.push(format!("{}: unknown event {e}", s(p, "id")));
            }
        }
    }
    for e in &events {
        if !event_index.contains_key(e) {
            errors.push(format!("event {e} is not covered by any contract"));
        }
    }
    let mut test_cache: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for c in &claims {
        let id = s(c, "id");
        for t in list(c, "tests") {
            let Some((suite, name)) = t.split_once('/') else {
                errors.push(format!("{id}: test {t} has no suite"));
                continue;
            };
            if matches!(suite, "perf" | "mutants" | "source") {
                continue;
            }
            let Some(dirs) = suite_dirs(suite) else {
                errors.push(format!("{id}: unknown suite {suite}"));
                continue;
            };
            let names = test_cache.entry(suite.to_string()).or_insert_with(|| test_names(root, dirs));
            let last = name.rsplit("::").next().unwrap_or(name);
            if !names.contains(last) {
                errors.push(format!("{id}: no test function {last} in suite {suite}"));
            }
        }
        for f in ["statement", "why", "level", "scope", "real", "simulated", "faults", "criterion", "repro", "supports", "not_covered"] {
            if s(c, f).is_empty() {
                errors.push(format!("{id}: field {f} is empty"));
            }
        }
    }
    if !errors.is_empty() {
        eprintln!("registry has {} problems:", errors.len());
        for e in &errors {
            eprintln!("  - {e}");
        }
        return 1;
    }

    // ---- join with evidence --------------------------------------------------
    let digest = crate::digest::source_digest(root);
    let ev = load_evidence(root);
    let mut claims_out = Vec::new();
    let mut claim_state: BTreeMap<String, String> = BTreeMap::new();
    for c in &claims {
        let (status, tests) = claim_status(c, &ev, &digest);
        claim_state.insert(s(c, "id").to_string(), status.clone());
        let mut c = c.clone();
        c["status"] = json!(status);
        c["results"] = json!(tests);
        claims_out.push(c);
    }
    // A review item marked verified whose claims did not all pass is reported.
    let mut inconsistent = Vec::new();
    for i in &considerations {
        if s(i, "verify") == "검증" {
            let bad: Vec<String> = list(i, "claims")
                .into_iter()
                .filter(|c| claim_state.get(c).is_some_and(|st| st != "passed"))
                .collect();
            if !bad.is_empty() {
                inconsistent.push(json!({ "item": s(i, "id"), "claims_not_passed": bad }));
            }
        }
    }

    let count = |field: &str, value: &str| considerations.iter().filter(|i| s(i, field) == value).count();
    let summary = json!({
        "considerations": considerations.len(),
        "by_layer": { "general": count("layer", "general"), "domain": count("layer", "domain"), "project": count("layer", "project") },
        "applicability": { "적용": count("applicability", "적용"), "조건부": count("applicability", "조건부"), "비적용": count("applicability", "비적용") },
        "design": { "결정": count("design", "결정"), "미결": count("design", "미결"), "해당없음": count("design", "해당없음") },
        "impl": { "구현": count("impl", "구현"), "부분": count("impl", "부분"), "미구현": count("impl", "미구현"), "해당없음": count("impl", "해당없음") },
        "verify": { "검증": count("verify", "검증"), "부분": count("verify", "부분"), "미검증": count("verify", "미검증"), "해당없음": count("verify", "해당없음") },
        "claims": claims.len(),
        "claims_by_status": {
            "passed": claim_state.values().filter(|x| *x == "passed").count(),
            "failed": claim_state.values().filter(|x| *x == "failed").count(),
            "not_run": claim_state.values().filter(|x| *x == "not_run").count(),
            "stale": claim_state.values().filter(|x| *x == "stale").count(),
        },
        "verified_items_with_unpassed_claims": inconsistent,
    });

    // ---- dashboard data ------------------------------------------------------
    let data_dir = root.join("dashboard/data");
    let _ = std::fs::create_dir_all(&data_dir);
    let registry = json!({
        "generated_at_us": oms_api::client::now_us(),
        "source_digest": digest,
        "evidence_digest": ev.digest,
        "summary": summary,
        "sources": sources,
        "considerations": considerations,
        "decisions": decisions,
        "contracts": contracts,
        "claims": claims_out,
        "assumptions": assumptions,
        "user_decisions": user_decisions,
        "paths": paths,
        "coverage": coverage,
        "event_index": event_index,
    });
    let write = |name: &str, v: &Value| {
        let _ = std::fs::write(data_dir.join(name), serde_json::to_string(v).unwrap_or_default());
    };
    write("registry.json", &registry);
    write("evidence.json", &ev.raw);
    write("perf.json", &json!(ev.perf));
    write("mutants.json", ev.mutants.as_ref().unwrap_or(&Value::Null));

    // ---- generated documents ---------------------------------------------------
    let gen_dir = root.join("docs/generated");
    let _ = std::fs::create_dir_all(&gen_dir);
    let header = format!(
        "> 자동 생성 문서다. 고치려면 `registry/*.toml`을 고치고 `cargo run -p xtask -- docs`를 실행한다.\n> 소스 다이제스트 `{digest}`, 검증 결과의 다이제스트 `{}`.\n\n",
        if ev.digest.is_empty() { "없음" } else { &ev.digest }
    );

    let mut md = format!("# 검토 항목 대응표\n\n{header}");
    md += &format!(
        "항목 {}건(일반 {}, Rust·LMAX·OMS {}, 프로젝트 {}).\n\n| 구분 | 값 |\n|---|---|\n| 적용 판단 | 적용 {} · 조건부 {} · 비적용 {} |\n| 설계 | 결정 {} · 미결 {} · 해당없음 {} |\n| 구현 | 구현 {} · 부분 {} · 미구현 {} · 해당없음 {} |\n| 검증 | 검증 {} · 부분 {} · 미검증 {} · 해당없음 {} |\n\n",
        considerations.len(),
        count("layer", "general"), count("layer", "domain"), count("layer", "project"),
        count("applicability", "적용"), count("applicability", "조건부"), count("applicability", "비적용"),
        count("design", "결정"), count("design", "미결"), count("design", "해당없음"),
        count("impl", "구현"), count("impl", "부분"), count("impl", "미구현"), count("impl", "해당없음"),
        count("verify", "검증"), count("verify", "부분"), count("verify", "미검증"), count("verify", "해당없음"),
    );
    md += "'검증'은 연결된 주장의 시험이 그 주장에 적힌 조건 안에서 통과했다는 뜻이다. 표준 준수를 뜻하지 않는다.\n\n";
    if !inconsistent.is_empty() {
        md += "**주의:** 다음 항목은 '검증'으로 적혀 있지만 연결된 주장이 현재 소스에서 모두 통과 상태는 아니다.\n\n| 항목 | 통과하지 않은 주장 |\n|---|---|\n";
        for x in &inconsistent {
            let claims: Vec<&str> = x["claims_not_passed"].as_array().map(|a| a.iter().filter_map(|c| c.as_str()).collect()).unwrap_or_default();
            md += &format!("| {} | {} |\n", x["item"].as_str().unwrap_or(""), claims.join(", "));
        }
        md += "\n";
    }
    for (layer, title) in [("general", "1. 일반 소프트웨어"), ("domain", "2. Rust·LMAX·OMS 특성"), ("project", "3. 프로젝트 특성")] {
        md += &format!("## {title}\n\n| ID | 영역 | 고려사항 | 필요성 | 출처 | 적용 | 설계 | 구현 | 검증 | 처리 방안 | 연결 | 미해결 |\n|---|---|---|---|---|---|---|---|---|---|---|---|\n");
        for i in considerations.iter().filter(|i| s(i, "layer") == layer) {
            let mut links = Vec::new();
            links.extend(list(i, "adr"));
            links.extend(list(i, "contracts"));
            links.extend(list(i, "claims"));
            let reason = s(i, "reason");
            md += &format!(
                "| {} | {} | {} | {} | {} | {}{} | {} | {} | {} | {} | {} | {} |\n",
                s(i, "id"), cell(s(i, "area")), cell(s(i, "consideration")), cell(s(i, "necessity")),
                list(i, "sources").join(", "), s(i, "applicability"),
                if reason.is_empty() { String::new() } else { format!(": {}", cell(reason)) },
                s(i, "design"), s(i, "impl"), s(i, "verify"), cell(s(i, "handling")), links.join(", "), cell(s(i, "open")),
            );
        }
        md += "\n";
    }
    let _ = std::fs::write(gen_dir.join("considerations.md"), md);

    let mut md = format!("# 검증 주장과 결과\n\n{header}");
    let st = &summary["claims_by_status"];
    md += &format!("주장 {}건: 통과 {}, 실패 {}, 미실행 {}, 이전 버전 결과 {}.\n\n", claims.len(), st["passed"], st["failed"], st["not_run"], st["stale"]);
    md += "| ID | 주장 | 수준 | 상태 | 재현 |\n|---|---|---|---|---|\n";
    for c in &claims_out {
        md += &format!("| {} | {} | {} | {} | `{}` |\n", s(c, "id"), cell(s(c, "statement")), s(c, "level"), ko_status(s(c, "status")), cell(s(c, "repro")));
    }
    md += "\n";
    for c in &claims_out {
        md += &format!(
            "## {} — {}\n\n- 상태: **{}**\n- 왜 중요한가: {}\n- 수준: {}\n- 전제와 범위: {}\n- 실제 구성요소: {}\n- 모의·시뮬레이션: {}\n- 주입한 장애: {}\n- 판정 기준과 근거: {}\n- 재현: `{}`\n- 뒷받침하는 것: {}\n- 확인하지 않은 범위: {}\n\n| 시험 | 결과 | 비고 |\n|---|---|---|\n",
            s(c, "id"), s(c, "statement"), ko_status(s(c, "status")), s(c, "why"), s(c, "level"), s(c, "scope"), s(c, "real"),
            s(c, "simulated"), s(c, "faults"), s(c, "criterion"), s(c, "repro"), s(c, "supports"), s(c, "not_covered"),
        );
        for r in c["results"].as_array().into_iter().flatten() {
            md += &format!("| `{}` | {} | {} |\n", s(r, "test"), ko_status(s(r, "result")), cell(s(r, "detail")));
        }
        md += "\n";
    }
    let _ = std::fs::write(gen_dir.join("claims.md"), md);

    let mut md = format!("# 계약 추적표\n\n{header}| 계약 | 제목 | 상태 전이(이벤트) | 설계 결정 | 코드 | 주장 |\n|---|---|---|---|---|---|\n");
    for c in &contracts {
        let claims_with_state: Vec<String> = list(c, "claims")
            .iter()
            .map(|id| format!("{id}({})", ko_status(claim_state.get(id).map(String::as_str).unwrap_or(""))))
            .collect();
        md += &format!(
            "| [{}](../../{}) | {} | {} | {} | {} | {} |\n",
            s(c, "id"), s(c, "file"), cell(s(c, "title")),
            list(c, "events").iter().map(|e| format!("`{e}`")).collect::<Vec<_>>().join(" "),
            list(c, "adr").join(", "),
            list(c, "code").iter().map(|e| format!("`{e}`")).collect::<Vec<_>>().join("<br>"),
            claims_with_state.join(", "),
        );
    }
    md += "\n모든 저널 이벤트는 위 표의 어느 계약에 속한다(생성기가 확인한다).\n";
    let _ = std::fs::write(gen_dir.join("traceability.md"), md);

    let mut md = format!("# 사용자 결정이 필요한 항목\n\n{header}");
    for d in &user_decisions {
        md += &format!(
            "## {} {}\n\n- 결정이 필요한 이유: {}\n- 추천안: {}\n- 대안: {}\n- 영향: {}\n- 미룰 수 있는 시점: {}\n\n",
            s(d, "id"), s(d, "title"), s(d, "why"), s(d, "recommendation"),
            if list(d, "alternatives").is_empty() { "—".to_string() } else { list(d, "alternatives").join(" / ") },
            s(d, "impact"), s(d, "defer_until"),
        );
    }
    let _ = std::fs::write(gen_dir.join("user-decisions.md"), md);

    let mut md = format!("# 외부에 의존하는 가정\n\n{header}| ID | 가정 | 의존 대상 | 근거 | 깨졌을 때 | 드러내는 수단 | 확인 여부 |\n|---|---|---|---|---|---|---|\n");
    for a in &assumptions {
        md += &format!(
            "| {} | {} | {} | {} | {} | {} | {} |\n",
            s(a, "id"), cell(s(a, "statement")), cell(s(a, "depends_on")), cell(s(a, "basis")),
            cell(s(a, "if_broken")), cell(s(a, "detection")), cell(s(a, "checked")),
        );
    }
    let _ = std::fs::write(gen_dir.join("assumptions.md"), md);

    let mut md = format!("# 검토 기준의 출처\n\n{header}| ID | 자료 | 판 | 역할 | 실제 접근한 범위 | 주의 |\n|---|---|---|---|---|---|\n");
    for x in &sources {
        md += &format!(
            "| {} | {} | {} | {} | {} {} | {} |\n",
            s(x, "id"), cell(s(x, "title")), cell(s(x, "version")), s(x, "role"), cell(s(x, "accessed")), cell(s(x, "secondary")), cell(s(x, "caution")),
        );
    }
    let _ = std::fs::write(gen_dir.join("sources.md"), md);

    let count_cov = |st: &str| coverage.iter().filter(|r| s(r, "status") == st).count();
    let mut md = format!(
        "# 출처 요소별 대응표\n\n{header}검토 기준의 모수다. 출처가 나누어 둔 요소마다 이 프로젝트가 어디에서 다뤘는지, 다루지 않았다면 왜인지를 적는다.\n\
         요소 목록은 [조사 기록](../research/standards-sources.md) §3과 [Hyperliquid 조사](../research/hyperliquid-protocol.md) §2에서 왔다. 표준의 본문을 읽지 못한 요소는 제목만으로 판단했고 그렇게 적었다.\n\n\
         요소 {}건: 다룸 {} · 비적용 {} · 보류 {} · 미검토 {}.\n\n\
         | 뜻 | |\n|---|---|\n| 다룸 | 검토 항목이 있다. 그 항목의 설계·구현·검증 상태는 [검토 항목 대응표](considerations.md)에 있다. 다뤘다는 것이 구현했다는 뜻은 아니다 |\n\
         | 비적용 | 이 프로젝트에 해당하지 않는다. 이유를 적는다 |\n| 보류 | 해당하지만 지금 다루지 않는다. 영향과 재개 조건을 적는다 |\n| 미검토 | 판단하지 못했다. 이유를 적는다 |\n\n\
         | 출처 | 요소 | 상태 | 검토 항목 | 비고 |\n|---|---|---|---|---|\n",
        coverage.len(), count_cov("다룸"), count_cov("비적용"), count_cov("보류"), count_cov("미검토"),
    );
    for r in &coverage {
        md += &format!(
            "| {} | {} | {} | {} | {} |\n",
            s(r, "source"), cell(s(r, "element")), s(r, "status"), list(r, "items").join(", "), cell(s(r, "note")),
        );
    }
    let _ = std::fs::write(gen_dir.join("coverage.md"), md);

    println!("{}", serde_json::to_string_pretty(&summary).unwrap_or_default());
    0
}
