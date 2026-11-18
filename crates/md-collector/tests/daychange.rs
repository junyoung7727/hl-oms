//! Ticket 05: the UTC day change, staggered overlapping switches (spec D7).

mod common;
#[path = "common/fake_bybit.rs"]
mod fake;
#[path = "common/run.rs"]
mod run;
#[path = "common/slowfs.rs"]
mod slowfs;

use std::sync::Arc;
use std::time::Duration;

use common::*;
use fake::Behaviour;
use md_collector::collector::{Collector, Deps, WallClock};
use md_collector::daychange;
use md_lake::qraw::Src;
use run::*;
use slowfs::SetFree;

#[test]
fn day_change_times_default() {
    let midnight = 1_767_398_400_000_000_000; // 2026-01-03 00:00:00 UTC
    assert_eq!(
        daychange::times(midnight, 4, 5000, 2000),
        vec![
            midnight + 5 * S,
            midnight + 7 * S,
            midnight + 9 * S,
            midnight + 11 * S
        ]
    );
    assert!(daychange::times(midnight, 0, 5000, 2000).is_empty());
}

/// The real clock moved so that the next UTC midnight is `lead` away.
struct NearMidnight(i64);

impl NearMidnight {
    fn new(lead: Duration) -> NearMidnight {
        let now = md_collector::wall_ns();
        let next = md_lake::time::day_start(now) + DAY;
        let lead = i64::try_from(lead.as_nanos()).expect("lead");
        NearMidnight(next - lead - now)
    }
}

impl WallClock for NearMidnight {
    fn now_ns(&self) -> i64 {
        md_collector::wall_ns() + self.0
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn day_change_switches_in_order() {
    let b = Behaviour {
        stream_every: Some(Duration::from_millis(20)),
        ..Behaviour::default()
    };
    let s = setup(
        "daychange",
        b,
        pages(&[("linear", &["BTCUSDT", "ETHUSDT"])]),
    )
    .await;
    // Traded BTCUSDT book on its own connection, ETHUSDT book on the other.
    let cfg = s.config(
        &[(
            "linear",
            "symbols = [\"BTCUSDT\", \"ETHUSDT\"]\nchannels = [\"orderbook.50\"]\nconn_budget = 4",
        )],
        "[day_change]\nstart_offset_ms = 400\nstagger_ms = 500\n\n[[traded_channels]]\nmarket = \"linear\"\nsymbol = \"BTCUSDT\"\nchannel = \"orderbook.50\"\n",
    );
    let deps = Deps {
        fs: s.fs.clone(),
        free_space: SetFree::new(u64::MAX),
        clock: Arc::new(NearMidnight::new(Duration::from_millis(1500))),
    };
    let started = std::time::Instant::now();
    let h = Collector::start(cfg, deps).await.expect("start");
    eventually("two switches", || h.state().switches_done == 2).await;
    eventually("the old connections closed", || {
        let c = s.ws.conns();
        c.len() == 4 && c[0].ended && c[1].ended
    })
    .await;
    // Let the new connections stream a little after the switch.
    tokio::time::sleep(Duration::from_millis(200)).await;
    h.shutdown().await.expect("shutdown");
    let c = s.ws.conns();
    let topic = |i: usize| fake::topic_of(&c[i].data[0]).1;
    // The two first connections start together; find the traded one.
    let (btc, eth) = if topic(0) == "BTCUSDT" {
        (0, 1)
    } else {
        (1, 0)
    };
    assert_eq!(
        (topic(btc), topic(eth)),
        ("BTCUSDT".into(), "ETHUSDT".into())
    );
    // The traded slot switches first, then the other slot, 500 ms apart,
    // starting 400 ms after the (moved) midnight.
    assert_eq!(
        (topic(2), topic(3)),
        ("BTCUSDT".into(), "ETHUSDT".into()),
        "slot order"
    );
    let at = |i: usize| c[i].accepted_at.expect("accepted").duration_since(started);
    assert!(
        at(2) >= Duration::from_millis(1800),
        "first switch at {:?}",
        at(2)
    );
    let gap = at(3) - at(2);
    assert!(
        gap >= Duration::from_millis(450) && gap <= Duration::from_millis(1000),
        "stagger {gap:?}"
    );
    // Each new connection was open before the old one of its slot closed.
    for (old, new) in [(btc, 2), (eth, 3)] {
        let ended = c[old].ended_at.expect("ended");
        assert!(
            c[new].accepted_at.expect("accepted") < ended,
            "slot of {old}"
        );
    }
    // Everything each connection sent is in L0 under one conn_id: the old
    // one's frames up to its close, the new one's snapshot and the deltas
    // after it.
    let recs: Vec<md_lake::qraw::Record> = records_under(s.fs.as_ref(), &s.dir, "/_open/")
        .into_iter()
        .filter(|r| r.src == Src::WebSocket)
        .collect();
    let ids: std::collections::BTreeSet<u32> = recs.iter().map(|r| r.conn_id).collect();
    assert_eq!(ids.len(), 4, "{ids:?}");
    for (i, fake_conn) in c.iter().enumerate() {
        let id = ids
            .iter()
            .copied()
            .find(|id| {
                recs.iter()
                    .find(|r| r.conn_id == *id)
                    .is_some_and(|r| r.payload == fake_conn.data[0].as_bytes())
            })
            .unwrap_or_else(|| panic!("no conn_id for fake connection {i}"));
        let got: Vec<String> = texts(
            &recs
                .iter()
                .filter(|r| r.conn_id == id)
                .cloned()
                .collect::<Vec<_>>(),
        );
        assert_eq!(got[..], fake_conn.data[..got.len()], "connection {i}");
        assert!(
            got[0].contains("\"type\":\"snapshot\""),
            "{i} starts with its snapshot"
        );
        assert!(got.len() > 3, "deltas after the snapshot of {i}");
    }
}
