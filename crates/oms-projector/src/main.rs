//! `oms-projector --config <file>`: runs [`oms_projector::Projector`] every
//! `poll_ms` until killed. A kill loses nothing: the next start replays up
//! to the cursor and emits from the record after it.

use oms_projector::{Config, HttpStatus, ProcLocks, Projector, Settings, StatVfs};
use std::time::Duration;

fn main() {
    if let Err(e) = run() {
        eprintln!(
            "{}",
            serde_json::json!({"level": "error", "msg": "oms-projector exits", "error": e})
        );
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    let path = match args.as_slice() {
        [_, flag, path] if flag == "--config" => path,
        _ => return Err("usage: oms-projector --config <file>".into()),
    };
    let text = std::fs::read_to_string(path).map_err(|e| format!("read {path}: {e}"))?;
    let cfg = Config::parse(&text)?;
    let host = cfg.host.clone().unwrap_or_else(oms_projector::host_name);
    let obs = match &cfg.telemetry {
        Some(t) => {
            let mut oc = obs_push::ObsConfig::new(t.vm_url.clone(), t.vl_url.clone());
            oc.queue_lines = t.queue_lines;
            oc.self_labels
                .insert("service".into(), "oms-projector".into());
            oc.self_labels.insert("venue".into(), cfg.venue.clone());
            Some(obs_push::ObsPush::start(oc)?)
        }
        None => None,
    };
    let logs_on = cfg.telemetry.as_ref().is_some_and(|t| t.vl_url.is_some());
    let settings = Settings {
        data_dir: cfg.data_dir.clone(),
        cursor_file: cfg.cursor_file.clone(),
        venue: cfg.venue.clone(),
        host,
        max_records: cfg.max_records,
        max_pending_lines: cfg.max_pending_lines,
        dead_after_us: cfg.dead_after_ms * 1000,
        disk_every_us: cfg.disk_every_ms * 1000,
        logs_on,
    };
    let status = HttpStatus::new(
        &cfg.status_url,
        Duration::from_millis(cfg.poll_ms.min(1000)),
    )?;
    let mut p = Projector::new(
        settings,
        Box::new(status),
        Box::new(ProcLocks::new(&cfg.data_dir)),
        Box::new(StatVfs(cfg.data_dir.clone())),
        obs,
        oms_projector::wall_us(),
    )?;
    let period = Duration::from_millis(cfg.poll_ms);
    loop {
        let started = std::time::Instant::now();
        let rep = p.step(oms_projector::wall_us());
        if rep.fatal.is_some() {
            // Keep pushing the metrics (projector_tail_error = 1) so the
            // stop is visible; the state does not move any more.
        }
        std::thread::sleep(period.saturating_sub(started.elapsed()));
    }
}
