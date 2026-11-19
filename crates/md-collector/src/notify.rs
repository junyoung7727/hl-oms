//! The systemd notify protocol (U6 spec D5, R10): `READY=1` once after the
//! start, `WATCHDOG=1` on every writer beat. A datagram to the socket in
//! `$NOTIFY_SOCKET` (a leading `@` is the Linux abstract namespace); the
//! socket is non-blocking and a failed send is counted, never waited on, so
//! a busy service manager cannot stop the writer.

use std::os::unix::net::UnixDatagram;
use std::sync::atomic::{AtomicU64, Ordering};

pub trait Notifier: Send + Sync {
    fn notify(&self, msg: &str);
}

pub struct SdNotify {
    sock: UnixDatagram,
    path: String,
    pub failed: AtomicU64,
}

impl SdNotify {
    /// `None` without `NOTIFY_SOCKET` (not run by systemd with `Type=notify`).
    pub fn from_env() -> Option<SdNotify> {
        let path = std::env::var("NOTIFY_SOCKET").ok()?;
        SdNotify::new(&path).ok()
    }

    pub fn new(path: &str) -> std::io::Result<SdNotify> {
        let sock = UnixDatagram::unbound()?;
        sock.set_nonblocking(true)?;
        Ok(SdNotify {
            sock,
            path: path.to_string(),
            failed: AtomicU64::new(0),
        })
    }

    fn send(&self, msg: &str) -> std::io::Result<usize> {
        match self.path.strip_prefix('@') {
            Some(name) => {
                use std::os::linux::net::SocketAddrExt;
                let addr = std::os::unix::net::SocketAddr::from_abstract_name(name.as_bytes())?;
                self.sock.send_to_addr(msg.as_bytes(), &addr)
            }
            None => self.sock.send_to(msg.as_bytes(), &self.path),
        }
    }
}

impl Notifier for SdNotify {
    fn notify(&self, msg: &str) {
        if self.send(msg).is_err() {
            self.failed.fetch_add(1, Ordering::Relaxed);
        }
    }
}
