//! Ticket 01: config checks, plan groups, market slots (spec D1–D3).

use md_collector::config::{Config, Symbols};
use md_collector::plan;
use md_collector::registry;

const EXAMPLE: &str = r#"
venue = "bybit"
collector = "md_collector"
lake_root = "/srv/lake"

[[markets]]
name = "linear"
symbols = ["BTCUSDT", "ETHUSDT", "SOLUSDT"]
channels = ["orderbook.50", "publicTrade", "tickers"]
conn_budget = 100

[[markets.extra]]
channel = "orderbook.1000"
symbols = ["BTCUSDT"]

[[markets]]
name = "spot"
symbols = "all"
channels = ["orderbook.50", "publicTrade"]
conn_budget = 50

[[traded_channels]]
market = "linear"
symbol = "BTCUSDT"
channel = "orderbook.1000"

[[traded_channels]]
market = "linear"
symbol = "BTCUSDT"
channel = "publicTrade"
"#;

fn with(extra: &str) -> String {
    format!("{EXAMPLE}\n{extra}")
}

fn err(text: &str) -> String {
    match Config::from_toml(text) {
        Ok(_) => panic!("the config was accepted"),
        Err(e) => e,
    }
}

#[test]
fn config_example_loads() {
    let c = Config::from_toml(EXAMPLE).expect("example");
    assert_eq!(c.venue, "bybit");
    assert_eq!(c.collector, "md_collector");
    assert_eq!(c.write.flush_ms, 1000);
    assert_eq!(c.write.fsync_ms, 5000);
    assert_eq!(c.write.queue_bytes, 16 * 1024 * 1024);
    assert_eq!(c.disk.floor_bytes, 20_000_000_000);
    assert_eq!(c.disk.margin_bytes, 20_000_000_000);
    assert_eq!(c.day_change.start_offset_ms, 5000);
    assert_eq!(c.day_change.stagger_ms, 2000);
    assert_eq!(c.instruments.refresh_ms, 3_600_000);
    assert!(!c.loopback_only);
    assert_eq!(c.markets.len(), 2);
    assert_eq!(c.markets[1].symbols, Symbols::All);
    assert_eq!(c.traded_channels.len(), 2);
}

#[test]
fn unknown_field_refused() {
    assert!(
        err(&EXAMPLE.replace("conn_budget = 50", "conn_budget = 50\nspeed = 3")).contains("speed")
    );
}

#[test]
fn unknown_venue_refused() {
    assert!(err(&EXAMPLE.replace("venue = \"bybit\"", "venue = \"nope\"")).contains("nope"));
}

#[test]
fn unknown_market_refused() {
    assert!(err(&EXAMPLE.replace("name = \"spot\"", "name = \"option\"")).contains("option"));
}

#[test]
fn market_listed_twice_refused() {
    let text = EXAMPLE.replace("name = \"spot\"", "name = \"linear\"");
    assert!(err(&text).contains("twice"));
}

#[test]
fn conn_budget_sum_refused() {
    let text = EXAMPLE.replace("conn_budget = 50", "conn_budget = 151");
    assert!(err(&text).contains("250"));
}

#[test]
fn symbols_word_must_be_all() {
    assert!(err(&EXAMPLE.replace("symbols = \"all\"", "symbols = \"some\"")).contains("all"));
}

#[test]
fn traded_not_collected_refused() {
    // A channel the market does not collect.
    let e = err(&with(
        "[[traded_channels]]\nmarket = \"linear\"\nsymbol = \"ETHUSDT\"\nchannel = \"tickers.x\"\n",
    ));
    assert!(e.contains("tickers.x"), "{e}");
    // An extra channel for a symbol outside the extra's list.
    let e = err(&with(
        "[[traded_channels]]\nmarket = \"linear\"\nsymbol = \"ETHUSDT\"\nchannel = \"orderbook.1000\"\n",
    ));
    assert!(e.contains("ETHUSDT"), "{e}");
    // A symbol outside the market's list.
    let e = err(&with(
        "[[traded_channels]]\nmarket = \"linear\"\nsymbol = \"XRPUSDT\"\nchannel = \"publicTrade\"\n",
    ));
    assert!(e.contains("XRPUSDT"), "{e}");
    // A market that is not collected.
    let e = err(&with(
        "[[traded_channels]]\nmarket = \"inverse\"\nsymbol = \"BTCUSD\"\nchannel = \"publicTrade\"\n",
    ));
    assert!(e.contains("inverse"), "{e}");
    // With symbols = "all" the symbol is checked after the instrument fetch;
    // the channel is checked at load.
    assert!(
        Config::from_toml(&with(
            "[[traded_channels]]\nmarket = \"spot\"\nsymbol = \"ANY\"\nchannel = \"publicTrade\"\n",
        ))
        .is_ok()
    );
}

#[test]
fn plan_groups_split_traded() {
    let c = Config::from_toml(EXAMPLE).expect("example");
    let venue = registry::venue("bybit").expect("bybit");
    let m = &c.markets[0];
    let symbols: Vec<String> = match &m.symbols {
        Symbols::List(s) => s.clone(),
        Symbols::All => unreachable!(),
    };
    let groups = plan::groups(venue, m, 0, &c.traded_channels, &symbols).expect("groups");
    let traded: Vec<&plan::Group> = groups.iter().filter(|g| g.traded).collect();
    let other: Vec<&plan::Group> = groups.iter().filter(|g| !g.traded).collect();
    assert!(!traded.is_empty() && !other.is_empty());
    // Traded topics are alone on their connections.
    let mut traded_topics: Vec<(String, String)> = traded
        .iter()
        .flat_map(|g| g.plans.iter().flat_map(|p| p.topics.iter()))
        .map(|k| (k.symbol.clone(), k.channel.clone()))
        .collect();
    traded_topics.sort();
    assert_eq!(
        traded_topics,
        vec![
            ("BTCUSDT".to_string(), "orderbook.1000".to_string()),
            ("BTCUSDT".to_string(), "publicTrade".to_string()),
        ]
    );
    // Every collected topic is in exactly one plan.
    let mut all: Vec<(String, String)> = groups
        .iter()
        .flat_map(|g| g.plans.iter().flat_map(|p| p.topics.iter()))
        .map(|k| (k.symbol.clone(), k.channel.clone()))
        .collect();
    let n = all.len();
    all.sort();
    all.dedup();
    assert_eq!(all.len(), n, "a topic is in two plans");
    // 3 symbols × 3 channels + 1 extra.
    assert_eq!(n, 10);
    assert!(!all.contains(&("ETHUSDT".to_string(), "orderbook.1000".to_string())));
    // One book depth per plan.
    for g in &groups {
        for p in &g.plans {
            let books: std::collections::BTreeSet<&str> = p
                .topics
                .iter()
                .filter(|k| k.channel.starts_with("orderbook."))
                .map(|k| k.channel.as_str())
                .collect();
            assert!(books.len() <= 1, "{books:?}");
        }
    }
    // Consecutive conn_index values from the market's base, inside the budget.
    let mut idx: Vec<u32> = groups
        .iter()
        .flat_map(|g| g.plans.iter().map(|p| p.conn_index))
        .collect();
    let expect: Vec<u32> = (0..u32::try_from(idx.len()).expect("len")).collect();
    idx.sort();
    assert_eq!(idx, expect);
    let other_topics: usize = other.iter().map(|g| g.topics().len()).sum();
    assert_eq!(other_topics, 8);
    // A market base shifts every conn_index.
    let shifted = plan::groups(venue, m, 100, &c.traded_channels, &symbols).expect("groups");
    assert!(
        shifted
            .iter()
            .flat_map(|g| g.plans.iter())
            .all(|p| p.conn_index >= 100)
    );
}

#[test]
fn plan_groups_respect_budget() {
    let text = EXAMPLE.replace(
        "conn_budget = 100",
        "conn_budget = 2\nmax_topics_per_conn = 3",
    );
    let c = Config::from_toml(&text).expect("config");
    let venue = registry::venue("bybit").expect("bybit");
    let symbols = vec![
        "BTCUSDT".to_string(),
        "ETHUSDT".to_string(),
        "SOLUSDT".to_string(),
    ];
    let e = plan::groups(venue, &c.markets[0], 0, &c.traded_channels, &symbols)
        .expect_err("over budget");
    assert!(e.contains("budget"), "{e}");
}

#[test]
fn market_slots_pinned() {
    let v = registry::venue("bybit").expect("bybit");
    assert_eq!(v.market_slot("linear"), Some(0));
    assert_eq!(v.market_slot("inverse"), Some(1));
    assert_eq!(v.market_slot("spot"), Some(2));
    assert_eq!(v.market_slot("option"), None);
    assert_eq!(registry::conn_range(0), (1, 1 << 28));
    assert_eq!(registry::conn_range(1), (1 << 28, 1 << 29));
    assert_eq!(registry::conn_range(2), (1 << 29, 3 << 28));
    assert_eq!(registry::slot_of_conn_id((2 << 28) + 7), 2);
    assert!(registry::venue("nope").is_none());
}
