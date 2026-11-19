//! `md-collector`: writes every frame of one venue to the lake's stage-1
//! files (spec D2–D8).
//!
//! ```text
//! md-collector run --config <file>
//! md-collector check --config <file>
//! md-collector version
//! ```
//!
//! Exit codes: 0 stopped by SIGTERM or Ctrl-C, 1 start or write error that a
//! restart may cure (the files are left for `md-lake seal` to recover), 2
//! configuration error or a plan the config cannot give (no restart).

use std::path::PathBuf;
use std::time::Duration;

use md_collector::collector::{Collector, Deps, StartError};
use md_collector::config::Config;

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn usage() -> i32 {
    eprintln!(
        "usage:\n  md-collector run --config <file>\n  md-collector check --config <file>\n  md-collector version"
    );
    2
}

fn line(level: &str, msg: &str, fields: serde_json::Value) {
    let l = serde_json::json!({"ts": md_collector::wall_ns() / 1000, "level": level, "msg": msg, "fields": fields});
    eprintln!("{l}");
}

async fn run(cfg: Config) -> i32 {
    let deps = Deps::real(&cfg);
    let handle = match Collector::start(cfg, deps).await {
        Ok(h) => h,
        Err(e) => {
            line(
                "error",
                "start failed",
                serde_json::json!({"error": e.to_string()}),
            );
            return match e {
                StartError::Refused(_) => 2,
                StartError::Failed(_) => 1,
            };
        }
    };
    line("info", "collector started", serde_json::json!({}));
    let mut term = match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
        Ok(s) => s,
        Err(e) => {
            line(
                "error",
                "cannot install the signal handler",
                serde_json::json!({"error": e.to_string()}),
            );
            let _ = handle.shutdown().await;
            return 1;
        }
    };
    let mut check = tokio::time::interval(Duration::from_secs(1));
    let code = loop {
        tokio::select! {
            _ = term.recv() => break 0,
            _ = tokio::signal::ctrl_c() => break 0,
            _ = check.tick() => {
                if let Some(e) = handle.failed() {
                    line("error", "writer failed", serde_json::json!({"error": e}));
                    // U6 D8, R2: the [MD P1] goes out first (20 s at most,
                    // below WatchdogSec=30s).
                    if !handle.report_write_failure(Duration::from_secs(20)).await {
                        line("error", "the [MD P1] alert was not delivered", serde_json::json!({}));
                    }
                    // Exit without closing: the open files are recovered by
                    // md-lake seal (crash_truncate); a new session starts.
                    return 1;
                }
            }
        }
    };
    match handle.shutdown().await {
        Ok(()) => code,
        Err(e) => {
            line("error", "shutdown failed", serde_json::json!({"error": e}));
            1
        }
    }
}

fn main() {
    std::panic::set_hook(Box::new(|info| {
        line(
            "error",
            "fatal",
            serde_json::json!({"reason": format!("panic: {info}")}),
        );
    }));
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match args.first().map(String::as_str) {
        Some("version") => {
            println!("md-collector {}", env!("CARGO_PKG_VERSION"));
            0
        }
        Some(cmd @ ("run" | "check")) => match arg(&args, "--config") {
            None => usage(),
            Some(path) => match Config::load(&PathBuf::from(path)) {
                Err(e) => {
                    eprintln!("configuration error: {e}");
                    2
                }
                Ok(_) if cmd == "check" => 0,
                Ok(cfg) => match tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .build()
                {
                    Ok(rt) => rt.block_on(run(cfg)),
                    Err(e) => {
                        eprintln!("cannot start the runtime: {e}");
                        1
                    }
                },
            },
        },
        _ => usage(),
    };
    std::process::exit(code);
}
