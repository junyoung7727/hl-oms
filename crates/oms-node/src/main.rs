//! `oms-node` command line.
//!
//! ```text
//! oms-node run     --config <file>
//! oms-node verify  --data-dir <dir> [--re-decide]
//! oms-node trace   --data-dir <dir> --order <id>
//! oms-node dashboard [--static-dir <dir>] [--listen 127.0.0.1:8188]
//! oms-node version
//! ```
//!
//! `verify` and `trace` read a data directory without locking or changing
//! it, so they can run next to a live node.

use oms_core::*;
use oms_node::config::NodeConfig;
use std::path::PathBuf;

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

fn usage() -> i32 {
    eprintln!(
        "usage:\n  oms-node run --config <file>\n  oms-node verify --data-dir <dir> [--re-decide]\n  oms-node trace --data-dir <dir> --order <id>\n  oms-node dashboard [--static-dir <dir>] [--listen 127.0.0.1:8188]\n  oms-node version"
    );
    2
}

fn main() {
    // The build aborts on panic (see the workspace profiles). Say why in the
    // same one-line form as the rest of the log before that happens.
    std::panic::set_hook(Box::new(|info| {
        let line = serde_json::json!({
            "ts": oms_node::util::wall_us(),
            "level": "error",
            "msg": "fatal",
            "fields": { "reason": format!("panic: {info}") },
        });
        eprintln!("{line}");
    }));
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match args.first().map(String::as_str) {
        Some("run") => match arg(&args, "--config") {
            Some(path) => match NodeConfig::load(&PathBuf::from(path)) {
                Ok(cfg) => oms_node::node::run(cfg),
                Err(e) => {
                    eprintln!("configuration error: {e}");
                    2
                }
            },
            None => usage(),
        },
        Some("verify") => match arg(&args, "--data-dir") {
            Some(dir) => {
                let re_decide = args.iter().any(|a| a == "--re-decide");
                match oms_node::recovery::verify(&PathBuf::from(dir), re_decide) {
                    Ok(report) => {
                        println!("{}", serde_json::to_string_pretty(&report).unwrap_or_default());
                        0
                    }
                    Err(e) => {
                        eprintln!("verification failed: {e}");
                        1
                    }
                }
            }
            None => usage(),
        },
        Some("trace") => match (arg(&args, "--data-dir"), arg(&args, "--order").and_then(|s| s.parse().ok())) {
            (Some(dir), Some(order)) => trace(&PathBuf::from(dir), OrderId(order)),
            _ => usage(),
        },
        Some("dashboard") => {
            // Documents and verification results only: no node is attached.
            let listen = arg(&args, "--listen").unwrap_or_else(|| "127.0.0.1:8188".into());
            let dir = PathBuf::from(arg(&args, "--static-dir").unwrap_or_else(|| "dashboard".into()));
            match listen.parse::<std::net::SocketAddr>() {
                Ok(addr) => {
                    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build();
                    match rt {
                        Ok(rt) => {
                            eprintln!("dashboard (no node attached): http://{addr}/");
                            match rt.block_on(oms_node::http::serve(addr, dir, None)) {
                                Ok(()) => 0,
                                Err(e) => {
                                    eprintln!("dashboard: {e}");
                                    1
                                }
                            }
                        }
                        Err(e) => {
                            eprintln!("runtime: {e}");
                            1
                        }
                    }
                }
                Err(e) => {
                    eprintln!("--listen: {e}");
                    2
                }
            }
        }
        Some("version") => {
            println!("oms-node {} src:{}", oms_node::VERSION, oms_node::SOURCE_DIGEST);
            0
        }
        _ => usage(),
    };
    std::process::exit(code);
}

/// Prints every journal record that concerns one order.
fn trace(dir: &std::path::Path, order: OrderId) -> i32 {
    let mut found = 0u64;
    let result = oms_journal::verify_all(dir, |seq, _schema, payload| {
        let rec: Record = serde_json::from_slice(payload).map_err(|e| format!("record {seq}: {e}"))?;
        if rec.events.iter().any(|e| e.order_id() == Some(order)) {
            found += 1;
            println!("{}", String::from_utf8_lossy(payload));
        }
        Ok(())
    });
    match result {
        Ok(_) if found > 0 => 0,
        Ok(_) => {
            eprintln!("no record mentions order {order}");
            1
        }
        Err(e) => {
            eprintln!("journal error: {e}");
            1
        }
    }
}
