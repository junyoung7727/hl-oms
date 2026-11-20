//! `oms-export --config <file>`: one export run (ADR-027). Prints one JSON
//! summary line. Exit 0 done, 3 no read limit (the OMS that owns the
//! journal did not answer; nothing written), 1 any other error.

use oms_export::{Config, ExportError, Settings, export};
use oms_projector::HttpStatus;
use std::time::Duration;

fn main() {
    let code = match run() {
        Ok(line) => {
            println!("{line}");
            0
        }
        Err((code, e)) => {
            eprintln!(
                "{}",
                serde_json::json!({"level": "error", "msg": "oms-export failed", "error": e})
            );
            code
        }
    };
    std::process::exit(code);
}

fn run() -> Result<String, (i32, String)> {
    let args: Vec<String> = std::env::args().collect();
    let path = match args.as_slice() {
        [_, flag, path] if flag == "--config" => path,
        _ => return Err((1, "usage: oms-export --config <file>".into())),
    };
    let text = std::fs::read_to_string(path).map_err(|e| (1, format!("read {path}: {e}")))?;
    let cfg = Config::parse(&text).map_err(|e| (1, e))?;
    let mut status = HttpStatus::new(&cfg.status_url, Duration::from_millis(cfg.status_timeout_ms))
        .map_err(|e| (1, e))?;
    let s = Settings {
        data_dir: cfg.data_dir,
        lake_root: cfg.lake_root,
        venue: cfg.venue,
        market: cfg.market,
        batch: cfg.batch,
    };
    match export(&s, &mut status) {
        Ok(sum) => serde_json::to_string(&sum).map_err(|e| (1, e.to_string())),
        Err(e @ ExportError::NoReadLimit(_)) => Err((3, e.to_string())),
        Err(e) => Err((1, e.to_string())),
    }
}
