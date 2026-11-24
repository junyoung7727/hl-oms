//! `watcher --config <path>` and `watcher --version`. This file only reads
//! the flags and the config file and wires SIGTERM and SIGINT to the
//! shutdown input of [`watcher::runtime::run`]; everything else is the
//! library's. The REST base comes only from the config's `network`.

use std::process::ExitCode;
use std::sync::Arc;

use watcher::config::Config;
use watcher::runtime::{Endpoints, run};

const USAGE: &str = "usage: watcher --config <path> | watcher --version";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let path = match args.as_slice() {
        [v] if v == "--version" => {
            println!("watcher {}", watcher::VERSION);
            return ExitCode::SUCCESS;
        }
        [c, path] if c == "--config" => path.clone(),
        _ => {
            eprintln!("watcher: {USAGE}");
            return ExitCode::from(2);
        }
    };
    let cfg = match Config::load(std::path::Path::new(&path)) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("watcher: {e}");
            return ExitCode::from(2);
        }
    };
    let rt = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("watcher: the tokio runtime: {e}");
            return ExitCode::from(2);
        }
    };
    let code = rt.block_on(async {
        use tokio::signal::unix::{SignalKind, signal};
        let (term, int) = (signal(SignalKind::terminate()), signal(SignalKind::interrupt()));
        let (Ok(mut term), Ok(mut int)) = (term, int) else {
            eprintln!("watcher: cannot listen for SIGTERM and SIGINT");
            return 2;
        };
        let shutdown = async move {
            tokio::select! {
                _ = term.recv() => {}
                _ = int.recv() => {}
            }
        };
        let ep = Endpoints::of(&cfg);
        let clock = Arc::new(watcher::clock::SystemClock);
        run(cfg, ep, clock, watcher::log::stderr(), shutdown).await
    });
    // Not a drop: a dropped runtime waits for every `spawn_blocking` task,
    // and a kill marker write hung on the disk must not hold the exit.
    rt.shutdown_timeout(std::time::Duration::from_millis(
        watcher::runtime::SHUTDOWN_MS,
    ));
    ExitCode::from(code)
}
