//! `unsealed_hours` (U6 spec D7).

mod common;

use md_lake::fs::RealFs;
use md_lake::seal::unsealed_hours;
use md_lake::time::{HOUR_NS, date, hour};

#[test]
fn unsealed_hours_lists_closed_hours_with_open_files() {
    let dir = common::test_dir("unsealed");
    let fs = RealFs::new(&dir);
    let now = 1_763_510_400_000_000_000 + 30 * 60 * 1_000_000_000; // 00:30 UTC
    let cur = now - 30 * 60 * 1_000_000_000;
    let put = |h: i64, venue: &str| {
        let d = dir.join(format!(
            "crypto/l0/_open/venue={venue}/date={}/hour={:02}",
            date(h),
            hour(h)
        ));
        std::fs::create_dir_all(&d).expect("mkdir");
        std::fs::write(d.join("x.qraw.zst"), b"").expect("write");
    };
    put(cur, "bybit"); // the current hour: not closed
    put(cur - 3 * HOUR_NS, "bybit");
    put(cur - 5 * HOUR_NS, "bybit");
    put(cur - 200 * HOUR_NS, "bybit"); // past the lookback
    put(cur - 2 * HOUR_NS, "okx"); // another venue
    // An emptied folder (sealed) does not count.
    let empty = cur - HOUR_NS;
    std::fs::create_dir_all(dir.join(format!(
        "crypto/l0/_open/venue=bybit/date={}/hour={:02}",
        date(empty),
        hour(empty)
    )))
    .expect("mkdir");
    let got = unsealed_hours(fs.as_ref(), "bybit", now, 168).expect("list");
    assert_eq!(got, vec![cur - 5 * HOUR_NS, cur - 3 * HOUR_NS]);
    assert!(
        unsealed_hours(fs.as_ref(), "kraken", now, 168)
            .expect("list")
            .is_empty()
    );
}
