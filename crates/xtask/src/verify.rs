//! Acceptance suite runner.
//!
//! Runs every test suite, records each test's result together with the
//! versions and environment it ran under, and writes
//! `evidence/latest/results.json` plus one log file per suite. Document
//! generation reads that file; a claim counts as passed only if every test it
//! names ran and passed for the same source digest.
//!
//! A test that did not run is recorded as not run. Nothing is assumed.

use crate::perf::environment;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

pub struct Suite {
    pub name: &'static str,
    pub what: &'static str,
    pub args: &'static [&'static str],
    pub env: Vec<(&'static str, String)>,
}

fn env(pairs: &[(&'static str, &str)]) -> Vec<(&'static str, String)> {
    pairs.iter().map(|(k, v)| (*k, v.to_string())).collect()
}

/// The suites, in the order they run. `quick` leaves out the long ones.
pub fn suites(quick: bool, live: bool, extended: Option<&str>) -> Vec<Suite> {
    let mut v = vec![
        Suite {
            name: "core",
            what: "domain core: number handling, identifiers, business scenarios, the review's regression and gap tests, random input",
            args: &["test", "--release", "-p", "oms-core"],
            env: env(&[]),
        },
        Suite {
            name: "journal",
            what: "journal and snapshot files at their failure boundaries",
            args: &["test", "-p", "oms-journal"],
            env: env(&[]),
        },
        Suite {
            name: "api",
            what: "local socket protocol examples",
            args: &["test", "-p", "oms-api"],
            env: env(&[]),
        },
        Suite {
            name: "sim",
            what: "deterministic simulation, 7 fault profiles and 2 broken-assumption runs",
            args: &["test", "--release", "-p", "oms-sim", "--", "--nocapture"],
            env: env(&[("OMS_SIM_SEEDS", if quick { "40" } else { "400" })]),
        },
        Suite {
            name: "node-unit",
            what: "queues, histogram, Hyperliquid signing vectors and message translation",
            args: &["test", "-p", "oms-node", "--lib"],
            env: env(&[]),
        },
        Suite {
            name: "pipeline",
            what: "commit gate with an injected flush failure (real threads, real file); the process stopped at each commit boundary",
            args: &[
                "test", "-p", "oms-node", "--features", "failpoints", "--test", "pipeline", "--test", "crash_points", "--", "--nocapture",
            ],
            env: env(&[]),
        },
        Suite {
            name: "e2e",
            what: "real oms-node and simvenue processes: kills, lost messages, link loss, journal damage, the review's process tests",
            args: &["test", "-p", "oms-node", "--test", "e2e", "--test", "review_node", "--", "--nocapture", "--test-threads=4"],
            env: env(&[("OMS_KILL_ROUNDS", if quick { "6" } else { "20" })]),
        },
    ];
    if live {
        v.push(Suite {
            name: "hl-live",
            what: "read-only queries and streams against the real Hyperliquid testnet",
            args: &["test", "-p", "oms-node", "--test", "hl_live", "--", "--nocapture"],
            env: env(&[("OMS_LIVE_TESTNET", "1")]),
        });
    }
    if let Some(dir) = extended {
        // Longer runs of two suites, kept as their own results so that a
        // figure quoted in a document can be traced to a stored run.
        v.push(Suite {
            name: "sim-extended",
            what: "deterministic simulation with 3000 seeds per profile",
            args: &["test", "--release", "-p", "oms-sim", "--", "--nocapture"],
            env: env(&[("OMS_SIM_SEEDS", "3000")]),
        });
        v.push(Suite {
            name: "kills-extended",
            what: "60 rounds of SIGKILL under load with the journal on the given directory",
            args: &["test", "-p", "oms-node", "--test", "e2e", "random_kills", "--", "--nocapture"],
            env: env(&[("OMS_KILL_ROUNDS", "60"), ("OMS_TEST_DIR", dir)]),
        });
    }
    v
}

fn parse_tests(output: &str) -> Vec<serde_json::Value> {
    let mut tests = Vec::new();
    for line in output.lines() {
        let Some(rest) = line.strip_prefix("test ") else { continue };
        let Some((name, result)) = rest.rsplit_once(" ... ") else { continue };
        let result = match result.trim() {
            "ok" => "passed",
            "FAILED" => "failed",
            r if r.starts_with("ignored") => "ignored",
            _ => continue,
        };
        tests.push(serde_json::json!({ "name": name.trim(), "result": result }));
    }
    tests
}

/// Lines a suite prints to describe what it actually exercised.
fn notes(output: &str) -> Vec<String> {
    output
        .lines()
        .filter(|l| {
            l.starts_with("profile=")
                || l.starts_with("caught as expected")
                || l.starts_with("rounds=")
                || l.starts_with("verify:")
                || l.starts_with("accepted=")
                || l.starts_with("40 requests")
                || l.starts_with("testnet BTC")
                || l.starts_with("skipped:")
        })
        .map(|l| l.chars().take(600).collect())
        .collect()
}

pub fn run(root: &Path, quick: bool, live: bool, extended: Option<String>) -> i32 {
    let out_dir = root.join("evidence/latest");
    let _ = std::fs::create_dir_all(&out_dir);
    let digest = crate::digest::source_digest(root);
    let started = oms_api::client::now_us();
    let mut results = Vec::new();
    let mut failed = false;

    // The core must not be able to reach a clock, a file or a socket.
    let purity = crate::digest::core_purity(root);
    if let Err(found) = &purity {
        failed = true;
        eprintln!("core purity check failed: {found:?}");
    }

    // The tests run with their crate as working directory, so the directory
    // for the extended runs has to be absolute.
    let extended = extended.map(|dir| {
        let _ = std::fs::create_dir_all(&dir);
        std::fs::canonicalize(&dir).map(|p| p.display().to_string()).unwrap_or(dir)
    });
    for s in suites(quick, live, extended.as_deref()) {
        eprintln!("== {} : {}", s.name, s.what);
        let t = Instant::now();
        let mut cmd = Command::new("cargo");
        cmd.args(s.args).current_dir(root);
        for (k, v) in &s.env {
            cmd.env(k, v);
        }
        let out = match cmd.output() {
            Ok(o) => o,
            Err(e) => {
                eprintln!("cannot run cargo: {e}");
                return 2;
            }
        };
        let text = format!(
            "{}\n{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        let log = out_dir.join(format!("{}.log", s.name));
        let _ = std::fs::write(&log, &text);
        let tests = parse_tests(&text);
        let n_failed = tests.iter().filter(|t| t["result"] == "failed").count();
        let n_passed = tests.iter().filter(|t| t["result"] == "passed").count();
        let ok = out.status.success() && n_failed == 0;
        failed |= !ok;
        eprintln!(
            "   {} passed, {} failed, exit {:?}, {:.1} s",
            n_passed,
            n_failed,
            out.status.code(),
            t.elapsed().as_secs_f64()
        );
        results.push(serde_json::json!({
            "name": s.name,
            "what": s.what,
            "command": format!("{}cargo {}", s.env.iter().map(|(k, v)| format!("{k}={v} ")).collect::<String>(), s.args.join(" ")),
            "exit_code": out.status.code(),
            "ok": ok,
            "seconds": (t.elapsed().as_secs_f64() * 10.0).round() / 10.0,
            "tests": tests,
            "notes": notes(&text),
            "log": format!("evidence/latest/{}.log", s.name),
        }));
    }
    let report = serde_json::json!({
        "source_digest": digest,
        "started_at_us": started,
        "finished_at_us": oms_api::client::now_us(),
        "mode": if quick { "quick" } else { "full" },
        "live_testnet": live,
        "extended": extended,
        "core_purity": match &purity { Ok(()) => serde_json::json!("ok"), Err(f) => serde_json::json!(f) },
        "environment": environment(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")),
        "test_data_dir": option_env!("CARGO_TARGET_TMPDIR").unwrap_or("target tmp dir of the build"),
        "suites": results,
    });
    let path = out_dir.join("results.json");
    if let Err(e) = std::fs::write(&path, serde_json::to_string_pretty(&report).unwrap_or_default()) {
        eprintln!("write {}: {e}", path.display());
        return 2;
    }
    eprintln!("written {} (source {digest})", path.display());
    i32::from(failed)
}
