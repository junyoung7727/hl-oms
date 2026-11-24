//! The stopper (C-WATCH part 12, ADR-032, U19b3a): step K3 of a kill
//! stops the OMS's one unit when `watcher_kill` did not halt it. It runs
//! exactly one program with a fixed argument list,
//! `<systemctl> [--user] --no-ask-password stop -- <oms_unit>`; the unit
//! name passed the config's rule (no glob, no space, no leading `-`), and
//! `--` ends the options before it. This file holds the crate's only
//! process start (test `stop::the_watcher_starts_one_program_only`).
//!
//! Nothing here waits: the runtime is single-threaded, so the stopper is
//! the one process call made inline in the tick (spawn, `try_wait`,
//! `kill`, a non-blocking read). stderr is set non-blocking right after
//! the spawn and read only after `try_wait` gave the exit, at most 4 KiB;
//! `WouldBlock` or end of file ends the read, so a helper that inherited
//! the pipe and keeps it open cannot hold the tick. At the deadline on the
//! injected clock the client is killed (`SIGKILL`) and reaped by
//! `try_wait` at later ticks, never by a blocking `wait`; the outcome is
//! then `Unconfirmed` (systemd's stop job may still stop the OMS later).

use std::io::Read;
use std::os::unix::process::ExitStatusExt;
use std::path::Path;
use std::process::{Child, ChildStderr, ExitStatus, Stdio};

use rustix::fs::{OFlags, fcntl_getfl, fcntl_setfl};

use crate::config::{Kill, Systemd};

/// The program the binary runs (tests give a script under `OMS_TEST_DIR`).
pub const SYSTEMCTL: &str = "/usr/bin/systemctl";
/// At most this much of the client's stderr is read.
pub const STDERR_MAX: usize = 4096;

/// The argument list: `[--user] --no-ask-password stop -- <oms_unit>`.
pub fn args(cfg: &Kill) -> Vec<String> {
    let mut a = Vec::new();
    if cfg.systemd == Systemd::User {
        a.push("--user".to_string());
    }
    for s in ["--no-ask-password", "stop", "--", cfg.oms_unit.as_str()] {
        a.push(s.to_string());
    }
    a
}

/// What K3 learnt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The client exited 0.
    Stopped,
    /// Another exit (`exit <code>: <first stderr line>`, `signal <n>`).
    Failed(String),
    /// The deadline passed first; the client was killed.
    Unconfirmed,
}

/// One running `systemctl stop`.
#[derive(Debug)]
pub struct Stopper {
    child: Child,
    stderr: Option<ChildStderr>,
    deadline_ms: u64,
    /// The outcome was given.
    done: bool,
    /// `try_wait` gave the exit.
    reaped: bool,
}

impl Stopper {
    /// Starts the client; the deadline is on the injected clock (`now +
    /// stop_wait_ms` from the kill sequence). An error is a failed stop
    /// (nothing was started).
    pub fn start(systemctl: &Path, cfg: &Kill, deadline_ms: u64) -> Result<Stopper, String> {
        let mut child = std::process::Command::new(systemctl)
            .args(args(cfg))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("{}: {e}", systemctl.display()))?;
        // A pipe that cannot be made non-blocking is not read at all: a
        // read must never wait in the tick.
        let stderr = child.stderr.take().filter(|e| {
            fcntl_getfl(e)
                .and_then(|f| fcntl_setfl(e, f | OFlags::NONBLOCK))
                .is_ok()
        });
        Ok(Stopper {
            child,
            stderr,
            deadline_ms,
            done: false,
            reaped: false,
        })
    }

    /// At every tick: the outcome once (`None` while the client runs and
    /// after the outcome was given); after `Unconfirmed` later polls reap
    /// the killed client. Never waits.
    pub fn poll(&mut self, now: u64) -> Option<Outcome> {
        if self.reaped {
            return None;
        }
        match self.child.try_wait() {
            Ok(Some(status)) => {
                self.reaped = true;
                if self.done {
                    return None;
                }
                self.done = true;
                return Some(self.outcome(status));
            }
            Ok(None) => {}
            Err(e) => {
                if !self.done {
                    self.done = true;
                    let _ = self.child.kill();
                    return Some(Outcome::Failed(format!("try_wait: {e}")));
                }
                return None;
            }
        }
        if self.done || now < self.deadline_ms {
            return None;
        }
        self.done = true;
        let _ = self.child.kill();
        Some(Outcome::Unconfirmed)
    }

    /// The client's exit was seen (it is no longer a zombie).
    pub fn reaped(&self) -> bool {
        self.reaped
    }

    fn outcome(&mut self, status: ExitStatus) -> Outcome {
        if status.success() {
            return Outcome::Stopped;
        }
        let head = match status.code() {
            Some(code) => format!("exit {code}"),
            None => format!("signal {}", status.signal().unwrap_or(0)),
        };
        let text = self.read_stderr();
        match String::from_utf8_lossy(&text).lines().next().map(str::trim) {
            Some(line) if !line.is_empty() => Outcome::Failed(format!("{head}: {line}")),
            _ => Outcome::Failed(head),
        }
    }

    /// What the client wrote, up to [`STDERR_MAX`] bytes, without waiting.
    fn read_stderr(&mut self) -> Vec<u8> {
        let mut out = Vec::new();
        let Some(e) = self.stderr.as_mut() else {
            return out;
        };
        let mut buf = [0u8; 1024];
        while out.len() < STDERR_MAX {
            match e.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => out.extend_from_slice(&buf[..n.min(STDERR_MAX - out.len())]),
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(_) => break,
            }
        }
        out
    }
}
