//! Selective mutation check.
//!
//! Each entry removes or weakens one protective piece of logic in a copy of
//! the workspace and runs the tests that are supposed to depend on it. The
//! tests must fail. If they pass, the protection is not actually guarded by
//! those tests, and the mutant is reported as survived.
//!
//! This is a hand-picked list, not exhaustive mutation testing. It answers
//! one question per entry: "would we notice if this were gone?"

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

struct Mutant {
    id: &'static str,
    what: &'static str,
    file: &'static str,
    find: &'static str,
    replace: &'static str,
    /// Arguments after `cargo test`.
    test: &'static [&'static str],
    guards: &'static str,
}

const MUTANTS: &[Mutant] = &[
    Mutant {
        id: "M01-ignore-flush-failure",
        what: "the commit stage ignores a failed flush and releases outputs anyway",
        file: "crates/oms-node/src/pipeline.rs",
        find: "            synced.map_err(|e| self.fail(e))?;",
        replace: "            let _ = synced;",
        test: &["-p", "oms-node", "--features", "failpoints", "--test", "pipeline", "outputs_follow_durability_with_a_commit_thread"],
        guards: "I-WAL / C-COMMIT",
    },
    Mutant {
        id: "M02-no-duplicate-lookup",
        what: "a new-order request no longer checks whether its key was already used",
        file: "crates/oms-core/src/decide.rs",
        find: "if let Some(id) = self.core.lookup_client(&r.strategy, &r.client_order_id) {",
        replace: "if let Some(id) = None::<OrderId> {",
        test: &["-p", "oms-core", "--test", "scenarios", "same_key"],
        guards: "I-KEY / C-IDEM",
    },
    Mutant {
        id: "M03-no-account-limit",
        what: "the account position bound is not checked for buys",
        file: "crates/oms-core/src/decide.rs",
        find: "Side::Buy => v <= inst.account_pos_max.0,",
        replace: "Side::Buy => true,",
        test: &["-p", "oms-core", "--test", "scenarios", "fia_7_1"],
        guards: "I-QTY / C-NEW",
    },
    Mutant {
        id: "M04-absence-without-waiting",
        what: "'not found' closes an in-doubt order without waiting for expiry plus margin",
        file: "crates/oms-core/src/decide.rs",
        find: "if queried_at < threshold {",
        replace: "if false {",
        test: &["-p", "oms-core", "--test", "scenarios", "not_found_closes"],
        guards: "C-DOUBT",
    },
    Mutant {
        id: "M05-restart-forgets-outstanding-sends",
        what: "after a restart, sends without a recorded result are not marked in doubt",
        file: "crates/oms-core/src/decide.rs",
        find: "if pending_without_result {",
        replace: "if false {",
        test: &["-p", "oms-core", "--test", "scenarios", "restart_puts"],
        guards: "C-BOOT",
    },
    Mutant {
        id: "M06-no-fill-dedup",
        what: "a fill whose key was already booked is booked again",
        file: "crates/oms-core/src/decide.rs",
        find: "if self.core.seen_fills.contains_key(&f.key) || self.core.parked_fills.iter().any(|p| p.fill.key == f.key) {",
        replace: "if false {",
        test: &["-p", "oms-core", "--test", "scenarios", "duplicate_fill"],
        guards: "I-FILL / C-FILL",
    },
    Mutant {
        id: "M07-holds-not-enforced",
        what: "hold reasons no longer cause a rejection",
        file: "crates/oms-core/src/decide.rs",
        find: "why.push(R::Hold(h));",
        replace: "let _ = h;",
        test: &["-p", "oms-core", "--test", "scenarios", "unknown_send_outcome"],
        guards: "C-NEW / holds",
    },
    Mutant {
        id: "M08-close-without-fill-sync",
        what: "an order with unknown final quantity is closed before fills are known to be complete",
        file: "crates/oms-core/src/decide.rs",
        find: "None if synced => self.emit(Event::OrderClosed { order_id: id, reason }),",
        replace: "None => self.emit(Event::OrderClosed { order_id: id, reason }),",
        test: &["-p", "oms-core", "--test", "scenarios", "terminal_without_final_quantity"],
        guards: "C-TERM",
    },
    Mutant {
        id: "M09-key-forgotten-before-expiry",
        what: "a closed order is purged without waiting for its request to expire",
        file: "crates/oms-core/src/decide.rs",
        find: "&& self.at >= o.expires_at.saturating_add(policy.dedup_grace_us)",
        replace: "",
        test: &["-p", "oms-core", "--test", "scenarios", "a_key_is_forgotten"],
        guards: "C-IDEM",
    },
    Mutant {
        id: "M10-stale-position-snapshot-judged",
        what: "a position snapshot older than the last booked fill is compared anyway",
        file: "crates/oms-core/src/decide.rs",
        find: "if self.core.last_fill_at >= queried_at {",
        replace: "if false {",
        test: &["-p", "oms-core", "--test", "scenarios", "a_position_snapshot_older"],
        guards: "C-RECON",
    },
    Mutant {
        id: "M11-mid-journal-damage-truncated",
        what: "damage in the middle of the journal is treated as an incomplete tail",
        file: "crates/oms-journal/src/segment.rs",
        find: "if !is_last || later.is_some() {",
        replace: "if false {",
        test: &["-p", "oms-journal", "--test", "journal", "damage_in_the_middle"],
        guards: "C-JOURNAL",
    },
    Mutant {
        id: "M12-checksum-not-verified",
        what: "frame checksums are not verified",
        file: "crates/oms-journal/src/frame.rs",
        find: "if h.finalize() != crc {",
        replace: "if false {",
        test: &["-p", "oms-journal", "--lib", "every_truncation_and_every_bit_flip"],
        guards: "C-JOURNAL",
    },
    Mutant {
        id: "M13-sim-cancel-releases-reservation",
        what: "a cancel request releases the order's reservation at once (caught by the simulation)",
        file: "crates/oms-core/src/core.rs",
        find: "            Event::CancelRequested { order_id, by } => {\n                let o = self.order_mut(*order_id)?;",
        replace: "            Event::CancelRequested { order_id, by } => {\n                let r = self.order_mut(*order_id)?.reserved;\n                self.release(*order_id, r)?;\n                let o = self.order_mut(*order_id)?;",
        test: &["--release", "-p", "oms-sim", "--test", "sim", "profile_calm"],
        guards: "I-RES / I-BRIDGE (simulation)",
    },
    Mutant {
        id: "M14-sim-resend-on-timeout",
        what: "the simulated gateway model reports 'not sent' for a request that timed out (caught by the simulation)",
        file: "crates/oms-sim/src/world.rs",
        find: "outcome: SendOutcome::Unknown { reason: reason.to_string() }, timing",
        replace: "outcome: SendOutcome::NotSent { reason: reason.to_string() }, timing",
        test: &["--release", "-p", "oms-sim", "--test", "sim", "profile_lossy"],
        guards: "G-2 of C-GW (simulation)",
    },
    Mutant {
        id: "M15-gateway-timeout-as-not-sent",
        what: "the real gateway reports 'not sent' when no answer arrived (process test)",
        file: "crates/oms-node/src/venue/simnet.rs",
        find: "Err(CallError::NoAnswer(e)) => SendOutcome::Unknown { reason: e },",
        replace: "Err(CallError::NoAnswer(e)) => SendOutcome::NotSent { reason: e },",
        test: &["-p", "oms-node", "--test", "e2e", "a_lost_request_is_closed_only_after_its_send_window"],
        guards: "G-2 of C-GW (process)",
    },
    // Protections added after the gap review of 2026-10-02.
    Mutant {
        id: "M16-parked-fill-not-drained",
        what: "fills waiting for a venue order id are not booked when another fill reveals that id",
        file: "crates/oms-core/src/decide.rs",
        find: "if identified {",
        replace: "if false {",
        test: &["-p", "oms-core", "--test", "review_core", "f2_parked_fill"],
        guards: "C-FILL (review F2)",
    },
    Mutant {
        id: "M17-cancel-budget-never-restarts",
        what: "a cancel whose attempts ran out can never be requested again",
        file: "crates/oms-core/src/decide.rs",
        find: ".is_some_and(|c| !c.inflight && !c.confirmed && c.used() >= max)",
        replace: ".is_some_and(|_| false)",
        test: &["-p", "oms-core", "--test", "review_core", "f1_cancel_can_be_reissued"],
        guards: "C-CANCEL (review F1)",
    },
    Mutant {
        id: "M18-stale-link-down-accepted",
        what: "a link-down notice for an older session takes the current session down",
        file: "crates/oms-core/src/decide.rs",
        find: "} else if self.core.link.up && session >= self.core.link.session {",
        replace: "} else if self.core.link.up {",
        test: &["-p", "oms-core", "--test", "review_core", "f5_stale_link_down"],
        guards: "C-LINK (review F5)",
    },
    Mutant {
        id: "M19-fills-complete-on-a-possibly-full-page",
        what: "the Hyperliquid gateway takes any page shorter than 2000 fills as the last one",
        file: "crates/oms-node/src/venue/hyperliquid/mod.rs",
        find: "if page_len < SMALLEST_FILL_PAGE {",
        replace: "if page_len < 2000 {",
        test: &["-p", "oms-node", "--lib", "a_fill_page_proves_completeness"],
        guards: "G-7 of C-GW",
    },
    Mutant {
        id: "M20-one-connection-pool",
        what: "the admin socket shares its connection pool with the trading socket",
        file: "crates/oms-node/src/server.rs",
        find: "let pool = if admin { &ctx.admin_connections } else { &ctx.connections };",
        replace: "let pool = &ctx.connections;",
        test: &["-p", "oms-node", "--test", "review_node", "idle_trading_connections"],
        guards: "C-API / C-OVERLOAD",
    },
];

fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for e in std::fs::read_dir(from)? {
        let e = e?;
        let (src, dst) = (e.path(), to.join(e.file_name()));
        if src.is_dir() {
            if e.file_name() == "target" {
                continue;
            }
            copy_tree(&src, &dst)?;
        } else if e.file_type()?.is_file() {
            // Sockets and the like (left behind by a test run) are not source.
            std::fs::copy(&src, &dst)?;
        }
    }
    Ok(())
}

pub fn run(root: &Path, only: Option<&str>) -> i32 {
    // The copy and its build output (several GiB) go next to this build's
    // own output, wherever the build configuration put that: the directory
    // two levels above the running executable is the target directory. The
    // source tree's disk may be the one the journal is measured on.
    let target = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().and_then(|p| p.parent()).map(|p| p.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target"));
    let work = target.join("mutants-work");
    let _ = std::fs::remove_dir_all(work.join("crates"));
    for item in ["crates", "Cargo.toml", "Cargo.lock"] {
        let (src, dst) = (root.join(item), work.join(item));
        let r = if src.is_dir() { copy_tree(&src, &dst) } else { std::fs::create_dir_all(&work).and_then(|()| std::fs::copy(&src, &dst).map(|_| ())) };
        if let Err(e) = r {
            eprintln!("copy {item}: {e}");
            return 2;
        }
    }
    let target_dir = work.join("target");
    let digest = crate::digest::source_digest(root);
    let mut results = Vec::new();
    let mut survived = 0;
    for m in MUTANTS.iter().filter(|m| only.is_none_or(|o| m.id.starts_with(o))) {
        let path = work.join(m.file);
        let original = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("{}: {e}", path.display());
                return 2;
            }
        };
        let n = original.matches(m.find).count();
        if n != 1 {
            eprintln!("{}: the text to mutate occurs {n} times in {}; the list is out of date", m.id, m.file);
            results.push(serde_json::json!({ "id": m.id, "what": m.what, "guards": m.guards, "killed": false, "note": "not applicable: source changed" }));
            survived += 1;
            continue;
        }
        let _ = std::fs::write(&path, original.replacen(m.find, m.replace, 1));
        let t = Instant::now();
        let out = Command::new("cargo")
            .arg("test")
            .args(m.test)
            .current_dir(&work)
            .env("CARGO_TARGET_DIR", &target_dir)
            .env("OMS_SIM_SEEDS", "60")
            .output();
        let _ = std::fs::write(&path, &original);
        let (killed, note) = match out {
            Err(e) => (false, format!("cargo did not run: {e}")),
            Ok(o) => {
                let text = format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
                let compiled = !text.contains("could not compile");
                let ran = text.contains("test result:");
                if !compiled {
                    // A mutant that does not compile proves nothing.
                    (false, "the mutant did not compile".to_string())
                } else if !ran {
                    (false, "no test ran".to_string())
                } else {
                    (!o.status.success(), String::new())
                }
            }
        };
        if !killed {
            survived += 1;
        }
        eprintln!("{} {:<44} {:.0} s  {}", if killed { "killed  " } else { "SURVIVED" }, m.id, t.elapsed().as_secs_f64(), note);
        results.push(serde_json::json!({
            "id": m.id, "what": m.what, "guards": m.guards, "file": m.file,
            "test": format!("cargo test {}", m.test.join(" ")),
            "killed": killed, "note": note,
        }));
    }
    let report = serde_json::json!({
        "source_digest": digest,
        "taken_at_us": oms_api::client::now_us(),
        "mutants": results,
    });
    if only.is_none() {
        let path = root.join("evidence/mutants.json");
        let _ = std::fs::write(&path, serde_json::to_string_pretty(&report).unwrap_or_default());
        eprintln!("written {}", path.display());
    }
    i32::from(survived > 0)
}
