//! The telemetry model (U6 spec D4, R6–R9), with synthetic books and events.

use std::collections::BTreeMap;

use md_collector::telemetry::{BookInfo, BookSource, LagHist, Period, Telemetry, TopicKey};
use md_core::StreamKey;
use md_feed::FeedEvent;
use obs_push::Sample;

const S: i64 = 1_000_000_000;
const MS: i64 = 1_000_000;

struct Books(BTreeMap<TopicKey, BookInfo>);

impl BookSource for Books {
    fn book(&self, t: &TopicKey) -> Option<BookInfo> {
        self.0.get(t).cloned()
    }
    fn exps(&self, _market: &str, symbol: &str) -> Option<(i8, i8)> {
        match symbol {
            "BTCUSDT" => Some((-1, -3)),
            _ => Some((-4, 0)),
        }
    }
}

fn none() -> Books {
    Books(BTreeMap::new())
}

fn k(ch: &str, sym: &str) -> StreamKey {
    StreamKey::new(ch, sym)
}

fn tk(market: &str, ch: &str, sym: &str) -> TopicKey {
    TopicKey::new(market, &k(ch, sym))
}

fn find<'a>(v: &'a [Sample], name: &str, kv: &[(&str, &str)]) -> Vec<&'a Sample> {
    v.iter()
        .filter(|s| {
            s.name == name
                && kv
                    .iter()
                    .all(|(a, b)| s.labels.get(*a).map(String::as_str) == Some(*b))
        })
        .collect()
}

fn one(v: &[Sample], name: &str, kv: &[(&str, &str)]) -> f64 {
    let f = find(v, name, kv);
    assert_eq!(f.len(), 1, "{name} {kv:?}: {f:?}");
    f[0].value
}

#[test]
fn summary_from_synthetic_book() {
    let t = Telemetry::new("bybit");
    t.register_slot("linear", 0, &[k("orderbook.50", "BTCUSDT")], true, 0);
    // Book in price units 10^-1 and qty units 10^-3: bid 42000.0, ask 42000.2.
    let info = BookInfo {
        spread_bp: Some(0.047_618_480_6),
        mid: Some(420_001.0),
        depth: [12_500, 30_000, 7_000, 9_000],
        stale: false,
        crossed: false,
    };
    let key = tk("linear", "orderbook.50", "BTCUSDT");
    let books = Books(BTreeMap::from([(key.clone(), info.clone())]));
    let v = t.summary(10 * S, Period::Fast, &books);
    let l = [
        ("venue", "bybit"),
        ("market", "linear"),
        ("symbol", "BTCUSDT"),
        ("channel", "orderbook.50"),
    ];
    assert_eq!(one(&v, "md_spread_bp", &l), 0.047_618_480_6);
    assert!((one(&v, "md_mid_px", &l) - 42_000.1).abs() < 1e-9);
    let d = |side, levels| {
        let mut kv = l.to_vec();
        kv.push(("side", side));
        kv.push(("levels", levels));
        one(&v, "md_depth_qty", &kv)
    };
    assert!((d("bid", "10") - 12.5).abs() < 1e-12);
    assert!((d("bid", "50") - 30.0).abs() < 1e-12);
    assert!((d("ask", "10") - 7.0).abs() < 1e-12);
    assert!((d("ask", "50") - 9.0).abs() < 1e-12);
    assert!(v.iter().all(|s| s.ts_ms == 10_000));

    // A stale or crossed book gives no spread or mid (depth stays).
    for (stale, crossed) in [(true, false), (false, true)] {
        let mut b = info.clone();
        b.stale = stale;
        b.crossed = crossed;
        let books = Books(BTreeMap::from([(key.clone(), b)]));
        let v = t.summary(11 * S, Period::Fast, &books);
        assert!(find(&v, "md_spread_bp", &l).is_empty());
        assert!(find(&v, "md_mid_px", &l).is_empty());
        assert_eq!(find(&v, "md_depth_qty", &l).len(), 4);
    }
}

#[test]
fn lag_quantiles() {
    let mut h = LagHist::default();
    assert_eq!(h.quantile(0.5), None);
    for ms in 1..=100 {
        h.add(f64::from(ms));
    }
    let p50 = h.quantile(0.5).expect("p50");
    let p99 = h.quantile(0.99).expect("p99");
    assert!((p50 / 50.0 - 1.0).abs() < 0.13, "{p50}");
    assert!((p99 / 99.0 - 1.0).abs() < 0.13, "{p99}");

    let t = Telemetry::new("bybit");
    t.register_slot("linear", 0, &[k("publicTrade", "BTCUSDT")], true, 0);
    let key = k("publicTrade", "BTCUSDT");
    let l = [("symbol", "BTCUSDT"), ("channel", "publicTrade")];
    // No sample in a period: no lag series.
    let v = t.summary(S, Period::Fast, &none());
    assert!(find(&v, "md_recv_lag_ms", &l).is_empty());
    t.lag("linear", &key, 10 * S, 10 * S + 20 * MS);
    t.lag("linear", &key, 10 * S, 10 * S - 3 * MS);
    let v = t.summary(11 * S, Period::Fast, &none());
    let mut q99 = l.to_vec();
    q99.push(("quantile", "0.99"));
    assert!((one(&v, "md_recv_lag_ms", &q99) / 20.0 - 1.0).abs() < 0.13);
    assert_eq!(one(&v, "md_recv_lag_negative_total", &l), 1.0);
    // The histogram is per period.
    let v = t.summary(12 * S, Period::Fast, &none());
    assert!(find(&v, "md_recv_lag_ms", &l).is_empty());
}

#[test]
fn fast_slow_split() {
    let t = Telemetry::new("bybit");
    t.register_slot(
        "linear",
        0,
        &[k("orderbook.50", "BTCUSDT"), k("orderbook.50", "ETHUSDT")],
        true,
        0,
    );
    t.register_slot(
        "linear",
        1,
        &[k("orderbook.50", "AUSDT"), k("orderbook.50", "BUSDT")],
        false,
        0,
    );
    t.register_slot("spot", 1 << 28, &[k("orderbook.50", "CUSDT")], false, 0);
    let mut per_symbol: BTreeMap<String, u32> = BTreeMap::new();
    for i in 1..=10 {
        for s in t.summary(i * S, Period::Fast, &none()) {
            if s.name == "md_last_msg_age_s" {
                *per_symbol.entry(s.labels["symbol"].clone()).or_default() += 1;
            }
        }
        if i == 10 {
            for s in t.summary(i * S, Period::Slow, &none()) {
                if s.name == "md_last_msg_age_s" {
                    *per_symbol.entry(s.labels["symbol"].clone()).or_default() += 1;
                }
            }
        }
    }
    assert_eq!(per_symbol["BTCUSDT"], 10);
    assert_eq!(per_symbol["ETHUSDT"], 10);
    assert_eq!(per_symbol["AUSDT"], 1);
    assert_eq!(per_symbol["BUSDT"], 1);
    assert_eq!(per_symbol["CUSDT"], 1);
}

#[test]
fn msgs_bytes_per_conn_channel_and_last_msg_age() {
    let t = Telemetry::new("bybit");
    t.register_slot(
        "linear",
        3,
        &[k("orderbook.50", "BTCUSDT"), k("publicTrade", "BTCUSDT")],
        false,
        5 * S,
    );
    let v = t.summary(7 * S, Period::Slow, &none());
    let l = [("symbol", "BTCUSDT"), ("channel", "orderbook.50")];
    assert_eq!(
        one(&v, "md_last_msg_age_s", &l),
        2.0,
        "age since the slot started (R9)"
    );
    for i in 0..3 {
        t.frame("linear", &k("orderbook.50", "BTCUSDT"), 3, 100, 8 * S + i);
    }
    t.frame("linear", &k("publicTrade", "BTCUSDT"), 3, 40, 8 * S);
    let v = t.summary(10 * S, Period::Slow, &none());
    let c = [
        ("market", "linear"),
        ("channel", "orderbook.50"),
        ("conn", "3"),
    ];
    assert_eq!(one(&v, "md_msgs", &c), 3.0);
    assert_eq!(one(&v, "md_bytes", &c), 300.0);
    let c2 = [("channel", "publicTrade"), ("conn", "3")];
    assert_eq!(one(&v, "md_bytes", &c2), 40.0);
    assert!((one(&v, "md_last_msg_age_s", &l) - (2.0 - 2e-9)).abs() < 1e-6);
    // Period sums reset.
    let v = t.summary(20 * S, Period::Slow, &none());
    assert_eq!(one(&v, "md_msgs", &c), 0.0);
    assert!(
        find(&v, "md_msgs", &[("conn", "3")])
            .iter()
            .all(|s| s.labels.get("conn_id").is_none())
    );
}

#[test]
fn gaps_reconnects_totals() {
    let t = Telemetry::new("bybit");
    let topics = [k("orderbook.50", "BTCUSDT"), k("orderbook.50", "ETHUSDT")];
    t.register_slot("linear", 0, &topics, true, 0);
    let ev = |e: FeedEvent| t.event("linear", &e);
    ev(FeedEvent::Connected {
        conn_index: 0,
        conn_id: 1,
    });
    ev(FeedEvent::Gap {
        conn_id: 1,
        key: k("orderbook.50", "BTCUSDT"),
        gap: md_core::GapEvent {
            last_good: Some(1),
            next: Some(3),
            recv_ns: 0,
        },
        resync: md_core::adapter::Resync::Resubscribe,
    });
    // A day-change switch: candidate 2 connects, takes over, 1 ends.
    ev(FeedEvent::Connected {
        conn_index: 0,
        conn_id: 2,
    });
    ev(FeedEvent::SwitchDone {
        conn_index: 0,
        from_conn_id: 1,
        to_conn_id: 2,
    });
    ev(FeedEvent::Disconnected {
        conn_index: 0,
        conn_id: 1,
        reason: "switched".into(),
    });
    // A candidate that fails: its Disconnected comes before SwitchFailed.
    ev(FeedEvent::Connected {
        conn_index: 0,
        conn_id: 3,
    });
    ev(FeedEvent::Disconnected {
        conn_index: 0,
        conn_id: 3,
        reason: "x".into(),
    });
    ev(FeedEvent::SwitchFailed {
        conn_index: 0,
        conn_id: 3,
        reason: "x".into(),
    });
    let v = t.summary(S, Period::Fast, &none());
    let btc = [("symbol", "BTCUSDT")];
    assert_eq!(
        one(&v, "md_reconnects_total", &btc),
        0.0,
        "switches are not reconnects"
    );
    assert_eq!(one(&v, "md_gaps_total", &btc), 1.0);
    assert_eq!(one(&v, "md_gaps_total", &[("symbol", "ETHUSDT")]), 0.0);
    // A real drop of the active connection, then two failed attempts, then up.
    ev(FeedEvent::Disconnected {
        conn_index: 0,
        conn_id: 2,
        reason: "eof".into(),
    });
    ev(FeedEvent::Disconnected {
        conn_index: 0,
        conn_id: 4,
        reason: "refused".into(),
    });
    ev(FeedEvent::Disconnected {
        conn_index: 0,
        conn_id: 5,
        reason: "refused".into(),
    });
    ev(FeedEvent::Connected {
        conn_index: 0,
        conn_id: 6,
    });
    let v = t.summary(2 * S, Period::Fast, &none());
    assert_eq!(
        one(&v, "md_reconnects_total", &btc),
        1.0,
        "one outage, one reconnect"
    );
    assert_eq!(
        one(&v, "md_reconnects_total", &[("symbol", "ETHUSDT")]),
        1.0
    );
}

#[test]
fn book_info_from_a_real_order_book() {
    use md_core::{BookUpdate, Level, OrderBook};
    let lv = |px, qty| Level { px, qty };
    let mut bids: Vec<Level> = (0..60).map(|i| lv(420_000 - i, 1_000 + i)).collect();
    bids.reverse();
    let asks: Vec<Level> = (0..60).map(|i| lv(420_002 + i, 2_000)).collect();
    let u = BookUpdate {
        key: k("orderbook.200", "BTCUSDT"),
        venue_ts_ns: 0,
        recv_ns: 0,
        update_id: Some(1),
        prev_update_id: None,
        first_update_id: None,
        venue_seq: None,
        bids,
        asks,
        px_exp: -1,
        qty_exp: -3,
    };
    let mut b = OrderBook::new(None);
    b.apply_snapshot(&u);
    let i = BookInfo::from_book(&b, false);
    // bid 42000.0, ask 42000.2: spread 0.2 / 42000.1 × 10^4 bp.
    assert!((i.spread_bp.expect("spread") - 0.2 / 42_000.1 * 1e4).abs() < 1e-9);
    assert_eq!(i.mid, Some(420_001.0));
    // Best 10 bids: qty 1000..=1009; best 50: 1000..=1049.
    assert_eq!(i.depth[0], (1_000..1_010).sum::<i128>());
    assert_eq!(i.depth[1], (1_000..1_050).sum::<i128>());
    assert_eq!(i.depth[2], 20_000);
    assert_eq!(i.depth[3], 100_000);
    assert!(!i.crossed && !i.stale);
}
