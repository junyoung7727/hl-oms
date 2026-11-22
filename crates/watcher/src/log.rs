//! Where the watcher's log lines go: one line per call. The runtime writes
//! to stderr; tests collect the lines.

use std::sync::{Arc, Mutex};

pub type Log = Arc<dyn Fn(&str) + Send + Sync>;

/// One line on stderr per call.
pub fn stderr() -> Log {
    Arc::new(|line: &str| eprintln!("watcher: {line}"))
}

/// A log that keeps its lines (tests), and the lines it kept.
pub fn collect() -> (Log, Arc<Mutex<Vec<String>>>) {
    let lines = Arc::new(Mutex::new(Vec::new()));
    let l = lines.clone();
    let log: Log = Arc::new(move |line: &str| {
        if let Ok(mut v) = l.lock() {
            v.push(line.to_string());
        }
    });
    (log, lines)
}
