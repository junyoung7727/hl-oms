//! A plan over Bybit's limits is refused or banned at start: 21,000 args
//! characters per connection, 500 new connections per 5 minutes counted over
//! every market of `stream.bybit.com`.

use std::collections::BTreeSet;

use md_bybit::{BybitAdapter, Market};
use md_core::StreamKey;
use md_core::adapter::MarketDataAdapter;
use serde_json::Value;

const CHANNELS: [&str; 3] = ["orderbook.50", "publicTrade", "tickers"];

fn symbols(prefix: &str, n: usize) -> Vec<String> {
    // Long names, as the longest real ones (for example 1000000MOGUSDT).
    (0..n).map(|i| format!("{prefix}{i:07}LONGUSDT")).collect()
}

fn channels() -> Vec<String> {
    CHANNELS.iter().map(|c| c.to_string()).collect()
}

#[test]
fn plans_for_all_three_markets_stay_within_the_limits() {
    let markets = [
        (Market::Linear, 932, 0, 100),
        (Market::Spot, 509, 100, 100),
        (Market::Inverse, 26, 200, 50),
    ];
    let mut used = BTreeSet::new();
    let mut total = 0;
    for (market, n, first, max) in markets {
        let syms = symbols(market.category(), n);
        let plan = BybitAdapter::new(market, &[])
            .with_conn_budget(first, max)
            .plan(&syms, &channels(), 50)
            .expect("plan");
        total += plan.len();
        let mut seen = Vec::new();
        for conn in &plan {
            assert!(
                (first..first + max).contains(&conn.conn_index),
                "{market:?} conn {}",
                conn.conn_index
            );
            assert!(used.insert(conn.conn_index), "conn_index used twice");
            assert_eq!(conn.depth, 50);
            assert!(conn.topics.len() <= 200);
            let chars: usize = conn
                .topics
                .iter()
                .map(|k| k.channel.len() + 1 + k.symbol.len() + 3)
                .sum();
            assert!(chars <= 21_000, "{chars} args characters");
            let syms_here: BTreeSet<_> = conn.topics.iter().map(|k| &k.symbol).collect();
            for s in syms_here {
                assert_eq!(
                    conn.topics.iter().filter(|k| &k.symbol == s).count(),
                    3,
                    "all channels of {s} on one connection"
                );
            }
            seen.extend(conn.topics.iter().cloned());
        }
        let want: Vec<StreamKey> = syms
            .iter()
            .flat_map(|s| CHANNELS.iter().map(move |c| StreamKey::new(*c, s.clone())))
            .collect();
        assert_eq!(seen, want, "{market:?}: every topic once, in order");
    }
    assert!(total <= 250, "{total} connections");
}

#[test]
fn the_args_limit_splits_connections_before_the_topic_cap() {
    // 120-character symbols: 3 topics ≈ 400 characters, so about 52 symbols fit
    // 21,000 characters, well under 200 topics.
    let syms: Vec<String> = (0..200).map(|i| format!("{i:0>120}")).collect();
    let plan = BybitAdapter::new(Market::Linear, &[])
        .plan(&syms, &channels(), 50)
        .expect("plan");
    assert!(plan.len() >= 4);
    for conn in &plan {
        let chars: usize = conn
            .topics
            .iter()
            .map(|k| k.channel.len() + 1 + k.symbol.len() + 3)
            .sum();
        assert!(chars <= 21_000);
        assert!(chars > 20_000 || conn.conn_index == plan.last().expect("a plan").conn_index);
    }
}

#[test]
fn plan_refuses_what_bybit_would_refuse() {
    let a = BybitAdapter::new(Market::Linear, &[]);
    let one = vec!["orderbook.50".to_string()];
    let over = a
        .clone()
        .with_max_topics_per_conn(1)
        .with_conn_budget(0, 10);
    assert!(
        over.plan(&symbols("x", 11), &one, 50).is_err(),
        "11 connections, budget 10"
    );
    assert_eq!(
        over.plan(&symbols("x", 10), &one, 50).map(|p| p.len()),
        Ok(10)
    );
    assert!(
        a.plan(&symbols("x", 1), &["orderbook.7".into()], 7)
            .is_err()
    );
    assert!(
        a.plan(&symbols("x", 1), &one, 200).is_err(),
        "depth differs"
    );
    assert!(a.plan(&symbols("x", 1), &["kline.1".into()], 50).is_err());
    // u32::from_str accepts these; Bybit has no such topic.
    for odd in ["orderbook.+50", "orderbook.050", "orderbook. 50"] {
        assert!(
            a.plan(&symbols("x", 1), &[odd.into()], 50).is_err(),
            "{odd}"
        );
    }
    let near_max = a
        .clone()
        .with_max_topics_per_conn(1)
        .with_conn_budget(u32::MAX - 1, 5);
    assert!(
        near_max
            .plan(&symbols("x", 3), &["tickers".into()], 50)
            .is_err(),
        "connection numbers would repeat past u32::MAX"
    );
    let dup = vec!["BTCUSDT".to_string(), "BTCUSDT".to_string()];
    assert!(a.plan(&dup, &one, 50).is_err(), "duplicate symbol");
    assert!(
        a.clone()
            .with_max_topics_per_conn(2)
            .plan(&symbols("x", 1), &channels(), 50)
            .is_err(),
        "one symbol's channels do not fit one connection"
    );
    assert_eq!(a.plan(&[], &one, 50), Ok(vec![]));
}

#[test]
fn subscribe_frames_hold_at_most_ten_args() {
    let a = BybitAdapter::new(Market::Spot, &[]).with_conn_budget(7, 5);
    let plan = a.plan(&symbols("s", 9), &channels(), 50).expect("plan");
    assert_eq!(plan.len(), 1);
    let frames = a.subscribe_frames(&plan[0]);
    assert_eq!(frames.len(), 3, "27 topics in frames of 10");
    let mut args = Vec::new();
    let mut ids = BTreeSet::new();
    for f in &frames {
        let v: Value = serde_json::from_str(f).expect("json");
        assert_eq!(v["op"], "subscribe");
        assert!(ids.insert(v["req_id"].as_str().expect("req_id").to_string()));
        let a = v["args"].as_array().expect("args");
        assert!(a.len() <= 10);
        args.extend(a.iter().map(|x| x.as_str().expect("topic").to_string()));
    }
    assert!(ids.iter().all(|id| id.starts_with("c7-")));
    let want: Vec<String> = plan[0]
        .topics
        .iter()
        .map(|k| format!("{}.{}", k.channel, k.symbol))
        .collect();
    assert_eq!(args, want);
}

#[test]
fn keepalive_is_a_ping_every_20_seconds() {
    let k = BybitAdapter::new(Market::Linear, &[]).keepalive();
    assert_eq!(k.interval_ms, 20_000);
    assert_eq!(k.frame.as_deref(), Some(r#"{"op":"ping"}"#));
}

#[test]
fn resubscribe_frames_unsubscribe_then_subscribe_one_topic() {
    use md_core::adapter::ConnPlan;
    let key = StreamKey::new("orderbook.50", "BTCUSDT");
    let plan = ConnPlan {
        conn_index: 7,
        topics: vec![key.clone(), StreamKey::new("publicTrade", "BTCUSDT")],
        depth: 50,
    };
    let frames = BybitAdapter::new(Market::Linear, &[]).resubscribe_frames(&plan, &key);
    let v: Vec<serde_json::Value> = frames
        .iter()
        .map(|f| serde_json::from_str(f).expect("json"))
        .collect();
    assert_eq!(v.len(), 2);
    assert_eq!(v[0]["op"], "unsubscribe");
    assert_eq!(v[1]["op"], "subscribe");
    for f in &v {
        assert_eq!(f["args"], serde_json::json!(["orderbook.50.BTCUSDT"]));
    }
    assert_ne!(v[0]["req_id"], v[1]["req_id"]);
    // A topic that is not in the plan has nothing to resubscribe.
    let other = StreamKey::new("orderbook.50", "ETHUSDT");
    assert!(BybitAdapter::new(Market::Linear, &[]).resubscribe_frames(&plan, &other).is_empty());
}
