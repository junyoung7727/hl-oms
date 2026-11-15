//! Read-only checks of the Hyperliquid gateway against the real testnet.
//!
//! Runs only with `OMS_LIVE_TESTNET=1`; otherwise every test returns at once.
//! Nothing here places, cancels or signs anything that is sent: the gateway
//! is driven only through its query path and its public streams, for an
//! address that has no account. The signing key is the well-known key from
//! the SDK's own tests; it controls nothing.
//!
//! What this shows: the real venue answers the gateway's queries in the
//! shapes the gateway parses, and the stream handshake works. What it does
//! not show: anything about order placement, cancels, fills or rate limits.

#![allow(clippy::unwrap_used)]

use oms_core::*;
use oms_node::config::HyperliquidConfig;
use oms_node::queues::{CoreMsg, InputQueues};
use oms_node::venue::{GatewayPorts, QueryReq, VenueInputTx};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

fn live() -> bool {
    std::env::var("OMS_LIVE_TESTNET").is_ok_and(|v| v == "1")
}

fn collect(queues: &Arc<InputQueues>, for_: Duration) -> Vec<VenueInput> {
    let deadline = Instant::now() + for_;
    let mut out = Vec::new();
    let mut batch: Vec<CoreMsg> = Vec::new();
    while Instant::now() < deadline {
        if queues.stats().depth.iter().sum::<usize>() == 0 {
            std::thread::sleep(Duration::from_millis(50));
            continue;
        }
        batch.clear();
        queues.pop_batch(64, &mut batch);
        for m in batch.drain(..) {
            if let Input::Venue(v) = m.input {
                out.push(v);
            }
        }
    }
    out
}

#[test]
fn the_gateway_reads_the_real_testnet() {
    if !live() {
        eprintln!("skipped: set OMS_LIVE_TESTNET=1 to run");
        return;
    }
    let key_var = "OMS_TEST_AGENT_KEY_FOR_LIVE_READ";
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let queues = Arc::new(InputQueues::new(4096, 16, 16));
    let (_cmd_tx, cmd_rx) = mpsc::channel::<VenueCommand>(4);
    let (query_tx, query_rx) = mpsc::channel::<QueryReq>(16);

    // BTC on testnet has 5 size decimals (observed 2026-10-02), so one price
    // decimal is allowed.
    let btc = InstrumentId::new("BTC").unwrap();
    let mut instruments = BTreeMap::new();
    instruments.insert(btc.clone(), InstrumentSpec {
        qty_step: Qty::parse("0.00001").unwrap(),
        px_tick: Px::parse("1").unwrap(),
        px_max_sig_figs: Some(5),
        min_notional: Amount::parse("10").unwrap(),
        max_order_qty: Qty::parse("1").unwrap(),
        max_order_notional: Amount::parse("100000").unwrap(),
        price_band_bps: Some(500),
        account_pos_min: SQty::parse("-1").unwrap(),
        account_pos_max: SQty::parse("1").unwrap(),
    });
    let cfg: HyperliquidConfig = toml::from_str(&format!(
        r#"
network = "testnet"
account_address = "0x00000000000000000000000000000000000000a1"
agent_key_env = "{key_var}"
request_timeout_ms = 8000
ref_price_interval_ms = 1000
"#
    ))
    .unwrap();
    let ports = GatewayPorts {
        commands: cmd_rx,
        queries: query_rx,
        input: VenueInputTx::new(queues.clone()),
        instance_id: 0xfeed,
        boot: 1,
    };
    // The key from the SDK's own tests.
    let name = {
        let _guard = rt.enter();
        oms_node::venue::hyperliquid::spawn_with_key(
            cfg,
            "0x0123456789012345678901234567890123456789012345678901234567890123",
            instruments,
            ports,
            4,
        )
        .unwrap()
    };
    assert_eq!(name, "hyperliquid:testnet");

    // 1. Metadata loads, the instrument checks pass, the streams subscribe.
    let first = collect(&queues, Duration::from_secs(30));
    let session = first
        .iter()
        .find_map(|v| match v {
            VenueInput::Link { up: true, session, .. } => Some(*session),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no link-up within 30 s; got {first:?}"));
    assert_eq!(session >> 32, 1, "the boot number is the high half of the session");

    // 2. Queries. In the node the reconciler repeats a query until its
    // answer arrives; one lost request must not decide this test either, so
    // the four are asked again, a few times, until each has answered.
    let mut got = first;
    for _round in 0..4 {
        for q in [
            QueryReq::Status { order_id: OrderId(1), venue_oid: None },
            QueryReq::OpenOrders,
            QueryReq::Positions,
            QueryReq::Fills { since: oms_node::util::wall_us() - 3_600_000_000 },
        ] {
            query_tx.blocking_send(q).unwrap();
        }
        got.extend(collect(&queues, Duration::from_secs(8)));
        let answered = got.iter().any(|v| matches!(v, VenueInput::StatusNotFound { .. }))
            && got.iter().any(|v| matches!(v, VenueInput::OpenOrders { .. }))
            && got.iter().any(|v| matches!(v, VenueInput::Positions { .. }))
            && got.iter().any(|v| matches!(v, VenueInput::FillsSynced { .. }));
        if answered {
            break;
        }
    }

    // An order this instance never placed is "unknownOid".
    assert!(
        got.iter().any(|v| matches!(v, VenueInput::StatusNotFound { order_id: OrderId(1), .. })),
        "no not-found answer in {got:?}"
    );
    // An address without an account has no open orders and no positions.
    assert!(got.iter().any(|v| matches!(v, VenueInput::OpenOrders { orders, .. } if orders.is_empty())));
    assert!(got.iter().any(|v| matches!(v, VenueInput::Positions { positions, .. } if positions.is_empty())));
    // No fills, and the gateway declares them complete up to a time in the past.
    let now = oms_node::util::wall_us();
    assert!(got.iter().any(|v| matches!(v, VenueInput::FillsSynced { up_to, .. } if *up_to < now)));
    assert!(!got.iter().any(|v| matches!(v, VenueInput::Fill(_))));
    // Reference prices arrive for the configured instrument.
    let px = got.iter().find_map(|v| match v {
        VenueInput::RefPrice { instrument, px, .. } if *instrument == btc => Some(*px),
        _ => None,
    });
    println!("testnet BTC mid seen by the gateway: {:?}; inputs: {}", px, got.len());
    assert!(px.is_some_and(|p| p.0 > 0), "no reference price in {got:?}");
    rt.shutdown_timeout(Duration::from_millis(200));
}
