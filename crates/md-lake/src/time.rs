//! UTC dates and hours of nanosecond times (no time-zone database needed).

pub const HOUR_NS: i64 = 3_600_000_000_000;
pub const DAY_NS: i64 = 24 * HOUR_NS;

pub fn hour_start(ns: i64) -> i64 {
    ns - ns.rem_euclid(HOUR_NS)
}

pub fn day_start(ns: i64) -> i64 {
    ns - ns.rem_euclid(DAY_NS)
}

/// `YYYY-MM-DD` of a UTC day number (days since 1970-01-01), by the
/// civil-from-days algorithm of H. Hinnant.
fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// The UTC date of a time, `YYYY-MM-DD`.
pub fn date(ns: i64) -> String {
    let (y, m, d) = civil(ns.div_euclid(DAY_NS));
    format!("{y:04}-{m:02}-{d:02}")
}

/// The UTC hour (0–23) of a time.
pub fn hour(ns: i64) -> u8 {
    (ns.rem_euclid(DAY_NS) / HOUR_NS) as u8
}

/// The start of a UTC date `YYYY-MM-DD` in ns, by days-from-civil.
pub fn date_start(date: &str) -> Option<i64> {
    let y: i64 = date.get(0..4)?.parse().ok()?;
    let m: i64 = date.get(5..7)?.parse().ok()?;
    let d: i64 = date.get(8..10)?.parse().ok()?;
    let y2 = if m <= 2 { y - 1 } else { y };
    let era = y2.div_euclid(400);
    let yoe = y2.rem_euclid(400);
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some((era * 146_097 + doe - 719_468) * DAY_NS)
}
