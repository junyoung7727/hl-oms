//! A child process that holds an exclusive flock (Seam C): the test binary
//! runs itself with `--exact lock_holder_child --ignored`.

use std::fs::File;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};

const ENV: &str = "MD_LAKE_LOCK_PATH";

/// Holds the lock while alive; dropping it ends the child and frees the lock.
pub struct Holder(
    Option<Child>,
    Option<std::io::Lines<BufReader<std::process::ChildStdout>>>,
);

impl Drop for Holder {
    fn drop(&mut self) {
        if let Some(mut c) = self.0.take() {
            drop(c.stdin.take());
            if let Some(lines) = self.1.take() {
                for _ in lines {}
            }
            let _ = c.wait();
        }
    }
}

pub fn hold(path: &Path) -> Holder {
    if !path.exists() {
        std::fs::create_dir_all(path).expect("create dir to lock");
    }
    let mut child = Command::new(std::env::current_exe().expect("test exe"))
        .args([
            "--exact",
            "lock_holder_child",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(ENV, path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn lock holder");
    let out = child.stdout.take().expect("stdout");
    let mut lines = BufReader::new(out).lines();
    loop {
        match lines.next() {
            Some(Ok(l)) if l.trim_end().ends_with("MD_LAKE_LOCKED") => break,
            Some(Ok(_)) => continue,
            other => panic!("lock holder did not lock: {other:?}"),
        }
    }
    Holder(Some(child), Some(lines))
}

pub fn child_main() {
    let Ok(path) = std::env::var(ENV) else { return };
    let f = File::open(&path).expect("open to lock");
    f.lock().expect("lock");
    println!("MD_LAKE_LOCKED");
    std::io::stdout().flush().expect("flush");
    let mut sink = Vec::new();
    let _ = std::io::stdin().read_to_end(&mut sink);
    drop(f);
}
