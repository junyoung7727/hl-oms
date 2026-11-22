//! The watcher's own Bybit client (C-WATCH part 3) against a loopback fake
//! (`http-fake`), on a manual clock: the request guard, the headers and the
//! signed query, the answer classes, the never-retried key rejection and
//! the 403 pause.

#![allow(clippy::unwrap_used)]

use watcher::rest::{GUARD_MAX, GUARD_WINDOW_MS, Guard};

const T0: u64 = 1_700_000_000_000;

#[test]
fn the_guard_holds_requests_beyond_60_in_10_s() {
    assert_eq!((GUARD_MAX, GUARD_WINDOW_MS), (60, 10_000));
    // 60 requests 100 ms apart are let through; the 61st waits for the
    // first to leave the window.
    let mut g = Guard::default();
    for i in 0..60 {
        assert_eq!(g.admit(T0 + i * 100), Ok(()), "request {i}");
    }
    assert_eq!(g.admit(T0 + 5_950), Err(T0 + 10_000));
    assert_eq!(g.admit(T0 + 9_999), Err(T0 + 10_000));
    // A refused request is not recorded: at the instant it was given it goes.
    assert_eq!(g.admit(T0 + 10_000), Ok(()));
    assert_eq!(g.admit(T0 + 10_000), Err(T0 + 10_100));
    assert_eq!(g.admit(T0 + 10_100), Ok(()));

    // A clock that steps back holds the requests until it passes the old
    // values again.
    let mut g = Guard::default();
    for _ in 0..60 {
        g.admit(T0).unwrap();
    }
    assert_eq!(g.admit(T0 - 5_000), Err(T0 + 10_000));
    assert_eq!(g.admit(T0 + 10_000), Ok(()));

    // A runaway caller that asks every 10 ms for a simulated minute, and
    // when refused waits for the given instant: never more than 60 in any
    // 10 s window, and every refusal names an instant at which it goes.
    let mut g = Guard::default();
    let mut sent: Vec<u64> = Vec::new();
    let mut now = T0;
    let mut steps = 0;
    let mut waited = false;
    while now < T0 + 60_000 {
        steps += 1;
        assert!(steps < 100_000, "bounded loop");
        match g.admit(now) {
            Ok(()) => {
                sent.push(now);
                now += 10;
                waited = false;
            }
            Err(at) => {
                assert!(!waited, "refused again at the instant it was given");
                assert!(at > now, "the instant {at} is after {now}");
                now = at;
                waited = true;
            }
        }
    }
    for (i, &t) in sent.iter().enumerate() {
        let in_window = sent[i..].iter().take_while(|&&u| u < t + 10_000).count();
        assert!(in_window <= 60, "{in_window} requests in the 10 s from {t}");
    }
    // Six full windows in a minute, no more.
    assert_eq!(sent.len(), 360);
}
