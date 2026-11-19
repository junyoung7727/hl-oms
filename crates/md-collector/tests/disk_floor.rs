//! Ticket 06: the disk floor (spec D8, R5).

mod common;
#[path = "common/fake_bybit.rs"]
mod fake;
#[path = "common/run.rs"]
mod run;
#[path = "common/slowfs.rs"]
mod slowfs;

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use common::*;
use fake::Behaviour;
use md_collector::collector::{Collector, Deps, SystemClock};
use md_lake::qraw::Src;
use run::*;
use slowfs::SetFree;

fn locked(dir: &std::path::Path, p: &str) -> bool {
    let f = std::fs::File::open(dir.join(p)).expect("open");
    matches!(f.try_lock(), Err(std::fs::TryLockError::WouldBlock))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn disk_floor_drops_and_restores() {
    let b = Behaviour {
        stream_every: Some(Duration::from_millis(20)),
        ..Behaviour::default()
    };
    let s = setup(
        "disk-floor",
        b,
        pages(&[("linear", &["BTCUSDT", "ETHUSDT"])]),
    )
    .await;
    let cfg = s.config(
        &[(
            "linear",
            "symbols = [\"BTCUSDT\", \"ETHUSDT\"]\nchannels = [\"orderbook.50\", \"publicTrade\"]\nconn_budget = 6",
        )],
        "[disk]\nfloor_bytes = 1000\nmargin_bytes = 1000\ncheck_ms = 50\n\n[[traded_channels]]\nmarket = \"linear\"\nsymbol = \"BTCUSDT\"\nchannel = \"orderbook.50\"\n",
    );
    let free = SetFree::new(10_000);
    let deps = Deps {
        fs: s.fs.clone(),
        free_space: free.clone(),
        clock: Arc::new(SystemClock),
    };
    let h = Collector::start(cfg, deps).await.expect("start");
    // Traded: BTCUSDT book. Other: BTCUSDT trades; ETHUSDT book and trades
    // (one channel set, one connection) → 2 other connections.
    eventually("three connections streaming", || {
        let c = s.ws.conns();
        c.len() == 3 && c.iter().all(|c| c.data.len() > 3)
    })
    .await;
    let traded =
        s.ws.conns()
            .iter()
            .position(|c| c.data.iter().all(|f| f.contains("orderbook.50.BTCUSDT")))
            .expect("the traded connection");

    free.set(500);
    eventually("the other connections closed", || {
        let c = s.ws.conns();
        h.state().disk_floor && (0..3).filter(|i| *i != traded).all(|i| c[i].ended)
    })
    .await;
    assert!(!s.ws.conns()[traded].ended, "the traded connection stays");
    // R5: the stopped connections' open files are closed (unlocked) while
    // the collector runs; the traded one's is still held.
    eventually("the dropped files unlocked", || {
        let open: Vec<String> = files_with(&s.dir, "/_open/")
            .into_iter()
            .filter(|p| p.ends_with(".partial") && !p.ends_with("-c0.qraw.zst.partial"))
            .collect();
        open.iter().filter(|p| !locked(&s.dir, p)).count() == 2
            && open.iter().filter(|p| locked(&s.dir, p)).count() == 1
    })
    .await;

    // Between the floor and floor + margin: nothing changes.
    free.set(1500);
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert_eq!(s.ws.accepted.load(Ordering::SeqCst), 3);
    assert!(h.state().disk_floor);

    free.set(2500);
    eventually("the other topics subscribed again", || {
        s.ws.conns().len() == 5 && !h.state().disk_floor
    })
    .await;
    eventually("the new connections stream", || {
        s.ws.conns()[3..].iter().all(|c| c.data.len() > 2)
    })
    .await;
    h.shutdown().await.expect("shutdown");

    let sess = records_under(s.fs.as_ref(), &s.dir, "/_session/");
    let mut notes: Vec<(String, String)> = sess
        .iter()
        .filter(|r| r.src == Src::Note)
        .map(|r| serde_json::from_slice::<serde_json::Value>(&r.payload).expect("note"))
        .filter(|n| n["note"] == "disk_floor")
        .map(|n| {
            assert_eq!(n["conn_id"], serde_json::Value::Null);
            assert_eq!(n["end_recv_ns"], serde_json::Value::Null);
            let t = &n["topics"].as_array().expect("topics")[..];
            assert_eq!(t.len(), 1, "one note per topic");
            assert_eq!(t[0]["market"], "linear");
            (
                t[0]["symbol"].as_str().expect("s").to_string(),
                t[0]["channel"].as_str().expect("c").to_string(),
            )
        })
        .collect();
    notes.sort();
    assert_eq!(
        notes,
        vec![
            ("BTCUSDT".into(), "publicTrade".into()),
            ("ETHUSDT".into(), "orderbook.50".into()),
            ("ETHUSDT".into(), "publicTrade".into()),
        ]
    );
    // The traded connection lost nothing.
    let c = s.ws.conns();
    let recs = records_under(s.fs.as_ref(), &s.dir, "/_open/");
    let first = c[traded].data[0].as_bytes();
    let id = recs
        .iter()
        .find(|r| r.payload == first)
        .map(|r| r.conn_id)
        .expect("traded conn_id");
    let got = texts(
        &recs
            .iter()
            .filter(|r| r.conn_id == id && r.src == Src::WebSocket)
            .cloned()
            .collect::<Vec<_>>(),
    );
    assert_eq!(got[..], c[traded].data[..got.len()]);
    assert!(
        got.len() + 2 >= c[traded].data.len(),
        "{} of {}",
        got.len(),
        c[traded].data.len()
    );
}
