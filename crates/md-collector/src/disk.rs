//! Free space of the lake's file system (spec D8).

use std::io;
use std::path::PathBuf;

pub trait FreeSpace: Send + Sync {
    fn free_bytes(&self) -> io::Result<u64>;
}

/// `statvfs` of a directory (as oms-node): bytes available to this user.
pub struct StatVfs(pub PathBuf);

impl FreeSpace for StatVfs {
    fn free_bytes(&self) -> io::Result<u64> {
        let s = rustix::fs::statvfs(&self.0).map_err(io::Error::from)?;
        Ok(s.f_bavail.saturating_mul(s.f_frsize))
    }
}

/// The disk floor checker (spec D8): below `floor_bytes` the other (not
/// traded) feeds stop, with one `disk_floor` note per dropped topic; at
/// `floor_bytes + margin_bytes` they start again on new connections.
pub(crate) async fn run(inner: std::sync::Arc<crate::collector::Inner>) {
    use std::sync::atomic::Ordering;

    use serde_json::json;

    use crate::collector::Inner;
    use crate::notes::Note;

    let d = inner.cfg.disk;
    let mut t = tokio::time::interval(std::time::Duration::from_millis(d.check_ms.max(10)));
    let mut dropped = false;
    loop {
        t.tick().await;
        let free = match inner.free_space.free_bytes() {
            Ok(f) => f,
            Err(e) => {
                inner.log(
                    "warn",
                    "free space unknown",
                    json!({"error": e.to_string()}),
                );
                continue;
            }
        };
        inner.counters.free_bytes.store(free, Ordering::Relaxed);
        inner.counters.free_known.store(true, Ordering::Relaxed);
        let floor_text = format!(
            "디스크 하한: 여유 {free} 바이트 < 하한 {} 바이트 — 거래 종목만 수집 중",
            d.floor_bytes
        );
        if !dropped && free < d.floor_bytes {
            // The alert first: stopping the feeds takes time (review 7).
            if let Some(a) = &inner.alert {
                a.set("disk_floor", Some(floor_text.clone()));
            }
            dropped = true;
            inner.counters.disk_floor.store(true, Ordering::Relaxed);
            inner.log(
                "warn",
                "disk floor: keeping only the traded topics",
                json!({"free_bytes": free}),
            );
            let mut groups = inner.groups.lock().await;
            for r in groups
                .iter_mut()
                .filter(|r| !r.group.traded && r.handle.is_some())
            {
                // The note starts when the gate closes: frames after it are
                // not written (review finding 1).
                let now = crate::wall_ns();
                inner.stop_group(r).await;
                for topic in Inner::topics_of(&r.group) {
                    inner.note(&Note {
                        note: "disk_floor",
                        conn_id: None,
                        topics: Some(std::slice::from_ref(&topic)),
                        start_recv_ns: now,
                        end_recv_ns: None,
                        detail: json!({"text": format!("free {free} bytes < floor {}", d.floor_bytes)}),
                    });
                }
            }
        } else if dropped && free >= d.floor_bytes.saturating_add(d.margin_bytes) {
            inner.log(
                "info",
                "disk floor: collecting every topic again",
                json!({"free_bytes": free}),
            );
            let mut groups = inner.groups.lock().await;
            let mut all = true;
            for r in groups
                .iter_mut()
                .filter(|r| !r.group.traded && r.handle.is_none())
            {
                match inner.start_group(r.group.clone()) {
                    Ok(new) => *r = new,
                    Err(e) => {
                        all = false;
                        inner.log("error", "feed not restarted", json!({"error": e}));
                    }
                }
            }
            if all {
                dropped = false;
                inner.counters.disk_floor.store(false, Ordering::Relaxed);
            }
        }
        // U6 D8: start, every 5 min while it lasts, clear (alert-direct).
        if let Some(a) = &inner.alert {
            let text = match (dropped, free < d.floor_bytes) {
                (false, _) => None,
                (true, true) => Some(floor_text),
                // Above the floor but a feed did not start again (review 7).
                (true, false) => Some(format!(
                    "디스크 하한 해소(여유 {free} 바이트)했지만 피드 재시작 실패 — 일부 종목 수집 중단"
                )),
            };
            a.set("disk_floor", text);
        }
    }
}
