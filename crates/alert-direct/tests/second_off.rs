//! alert-direct without a second channel (U18a1): no `alert-second` thread
//! and no second state. Its own test binary, so that no other test's second
//! channel thread is in the process.

use std::sync::Arc;
use std::time::Duration;

use alert_direct::{AlertConfig, AlertDirect, Route, SystemClock, TelegramConfig};
use http_fake::{Answer, FakeHttp};

/// Names of this process's threads (Linux).
fn thread_names() -> Vec<String> {
    let mut v = Vec::new();
    for e in std::fs::read_dir("/proc/self/task")
        .expect("/proc/self/task")
        .flatten()
    {
        if let Ok(n) = std::fs::read_to_string(e.path().join("comm")) {
            v.push(n.trim().to_string());
        }
    }
    v
}

#[test]
fn second_off_by_default() {
    let f = FakeHttp::start(Answer::Reply(200, r#"{"ok":true,"result":{}}"#.into()));
    let tg = TelegramConfig::new("123456:ABCdef_ghi-JKL", "42").expect("config");
    let mut c = AlertConfig::new(tg, "[MD P1]", "bybit");
    assert!(c.second.is_none());
    assert_eq!(c.second_after_ms, 60_000);
    c.routes = vec![Route::direct(&f.url()).expect("route")];
    let h = AlertDirect::start(c, Arc::new(SystemClock)).expect("start");
    h.once("x");
    assert!(h.flush_for(Duration::from_secs(5)));
    let names = thread_names();
    assert!(names.iter().any(|n| n == "alert-direct"), "{names:?}");
    assert!(!names.iter().any(|n| n == "alert-second"), "{names:?}");
    let st = h.state();
    assert_eq!(st.second, None);
    assert_eq!(st.failing_since_ms, None);
    assert_eq!(st.delivered, 1);
}
