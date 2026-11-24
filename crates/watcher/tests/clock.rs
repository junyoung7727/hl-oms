//! `clock::Steady` (U19b3a, review 4 S3): the clock of the kill path does
//! not step back. A wall clock that steps back by Δ would hold W5 and a
//! running kill for Δ; `Steady` turns the step into a pause of zero length
//! and passes a forward step through.

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use watcher::clock::{Clock, ManualClock, Steady};

#[test]
fn a_steady_clock_does_not_step_back() {
    let inner = Arc::new(ManualClock::new(10_000));
    let steady = Steady::new(inner.clone());
    assert_eq!(steady.now_ms(), 10_000);
    // The inner clock steps back 6,000 ms: Steady still reads 10,000.
    inner.set(4000);
    assert_eq!(steady.now_ms(), 10_000);
    assert_eq!(steady.now_ms(), 10_000);
    // Then it goes on from there with the inner clock's progress.
    inner.advance(500);
    assert_eq!(steady.now_ms(), 10_500);
    // A forward step passes through (W5's gap rule handles it).
    inner.advance(60_000);
    assert_eq!(steady.now_ms(), 70_500);
    // A second step back, and one that lands between: no value goes down.
    inner.set(0);
    assert_eq!(steady.now_ms(), 70_500);
    inner.set(100);
    assert_eq!(steady.now_ms(), 70_600);
    inner.set(50);
    assert_eq!(steady.now_ms(), 70_600);
    inner.set(1050);
    assert_eq!(steady.now_ms(), 71_600);
    // The raw clock is not changed by Steady.
    assert_eq!(inner.now_ms(), 1050);

    // Two threads read one timeline while the inner clock steps back and
    // forth: each thread's reads never go down, and a read after both is
    // at least every value either thread saw (one offset for both).
    let inner = Arc::new(ManualClock::new(1_000_000));
    let steady = Arc::new(Steady::new(inner.clone()));
    let readers: Vec<_> = (0..2)
        .map(|_| {
            let s = steady.clone();
            std::thread::spawn(move || {
                let mut seen = Vec::with_capacity(20_000);
                for _ in 0..20_000 {
                    seen.push(s.now_ms());
                }
                seen
            })
        })
        .collect();
    let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
    for _ in 0..20_000 {
        // xorshift: steps back up to 5,000 ms or forward up to 5,000 ms.
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        let now = inner.now_ms();
        let step = x % 5000;
        if x & 1 == 0 {
            inner.set(now.saturating_sub(step));
        } else {
            inner.set(now + step);
        }
    }
    let seen: Vec<Vec<u64>> = readers.into_iter().map(|h| h.join().unwrap()).collect();
    let after = steady.now_ms();
    for s in &seen {
        assert!(s.windows(2).all(|w| w[0] <= w[1]), "a read went down");
        assert!(s.iter().all(|v| *v <= after), "a later read is lower");
    }
}
