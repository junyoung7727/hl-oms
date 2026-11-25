//! The Bybit gateway against answers recorded from the real testnet
//! (`crates/http-fake/recorded/bybit-testnet/`, written by the
//! `bybit-testnet` suite). The loopback fake serves the recorded bodies as
//! they came, so these tests show that the gateway reads what the venue
//! really sends. The `bybit-testnet` suite fails when the venue's answers no
//! longer have the recorded shape; then the files are recorded again and
//! these tests show whether the gateway still reads them.

#![allow(clippy::unwrap_used)]

use http_fake::{Answer as Http, FakeHttp};
use oms_core::*;
use oms_node::venue::QueryReq;
use oms_node::venue::bybit::answer::{self, Answer};
use oms_node::venue::bybit::query::Queries;
use oms_node::venue::bybit::rest::{Credentials, Rest};
use oms_node::venue::bybit::{cancel_all, exec, orders};
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;

fn recorded(name: &str) -> Value {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../http-fake/recorded/bybit-testnet")
        .join(format!("{name}.json"));
    serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap()
}

/// The recorded answer as the fake sends it: its HTTP status and body.
fn reply(name: &str) -> Http {
    let r = recorded(name);
    Http::Reply(
        r["http_status"].as_u64().unwrap() as u16,
        r["response"].to_string(),
    )
}

fn rest(fake: &FakeHttp) -> Rest {
    Rest::new(
        &fake.url(),
        Arc::new(Credentials::new("K", "S")),
        5_000,
        Duration::from_millis(500),
    )
    .unwrap()
}

/// The instance whose order link ids the recorded rows carry.
fn instance_of(link: &str) -> u64 {
    exec::WIRE_ID.decode(link).unwrap().instance_id
}

fn recorded_link(name: &str) -> String {
    recorded(name)["response"]["result"]["list"][0]["orderLinkId"]
        .as_str()
        .unwrap()
        .to_string()
}

#[test]
fn recorded_refusals_read_as_rejected() {
    for (name, code) in [
        ("create_stale", 10002),
        ("create_below_min_qty", 10001),
        ("cancel_unknown", 110001),
    ] {
        let r = recorded(name);
        let a = answer::classify(
            r["http_status"].as_u64().unwrap() as u16,
            &r["response"].to_string(),
        );
        assert!(
            matches!(a, Answer::Rejected { code: c, .. } if c == code),
            "{name}: {a:?}"
        );
    }
    let r = recorded("create_stale");
    let a = answer::classify(200, &r["response"].to_string());
    assert!(matches!(
        orders::send_outcome(a, 0),
        SendOutcome::Rejected { .. }
    ));
}

#[tokio::test]
async fn a_recorded_empty_cancel_all_reads_as_nothing_cancelled() {
    let fake = FakeHttp::start(reply("cancel_all_empty"));
    let c = cancel_all(&rest(&fake)).await.unwrap();
    assert_eq!(c.cancelled, 0);
    assert!(!c.maybe_more);
}

#[tokio::test]
async fn recorded_open_orders_positions_and_fills_become_core_inputs() {
    let link = recorded_link("order_realtime_one");
    let fake = FakeHttp::start(Http::Reply(404, "{}".into()));
    // A page with rows carries a cursor even when it is the last; the real
    // venue then answers the cursor with an empty page and an empty cursor
    // (seen on the testnet 2026-11-25). The fake answers in that order.
    fake.route(
        "/v5/order/realtime",
        [reply("order_realtime_one"), reply("order_realtime_empty")],
    );
    fake.route(
        "/v5/position/list",
        [reply("position_list_one"), reply("position_list_empty")],
    );
    fake.route(
        "/v5/execution/list",
        [reply("execution_list_rows"), reply("execution_list_empty")],
    );
    let q = Queries::new(rest(&fake), instance_of(&link), 2_000);

    let a = q.answer(QueryReq::OpenOrders, 1).await;
    assert_eq!(a.failed, None);
    let VenueInput::OpenOrders { orders, .. } = &a.inputs[0] else {
        panic!("{:?}", a.inputs)
    };
    assert_eq!(orders.len(), 1, "{orders:?}");

    let a = q.answer(QueryReq::Positions, 1).await;
    assert_eq!(a.failed, None);
    let VenueInput::Positions { positions, .. } = &a.inputs[0] else {
        panic!("{:?}", a.inputs)
    };
    assert_eq!(positions.len(), 1, "{positions:?}");

    let rows = recorded("execution_list_rows")["response"]["result"]["list"]
        .as_array()
        .unwrap()
        .clone();
    let fill_instance = instance_of(rows[0]["orderLinkId"].as_str().unwrap());
    let q = Queries::new(rest(&fake), fill_instance, 2_000);
    let since = rows
        .iter()
        .map(|r| r["execTime"].as_str().unwrap().parse::<u64>().unwrap())
        .min()
        .unwrap();
    let a = q
        .answer(
            QueryReq::Fills {
                since: (since - 1_000) * 1000,
            },
            1,
        )
        .await;
    assert_eq!(a.failed, None, "{:?}", a.inputs);
    let fills: Vec<&FillIn> = a
        .inputs
        .iter()
        .filter_map(|i| {
            if let VenueInput::Fill(f) = i {
                Some(f)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(fills.len(), rows.len(), "{:?}", a.inputs);
    for f in fills {
        assert!(f.order_id.is_some(), "the recorded fill is ours: {f:?}");
        assert!(f.fee.raw() > 0, "a taker fee is a cost: {f:?}");
    }
}

#[test]
fn recorded_execution_rows_classify_as_our_fills() {
    let rows = recorded("execution_list_rows")["response"]["result"]["list"]
        .as_array()
        .unwrap()
        .clone();
    assert!(!rows.is_empty());
    for row in &rows {
        let link = row["orderLinkId"].as_str().unwrap();
        let VenueInput::Fill(f) = exec::classify(row, instance_of(link), None).unwrap() else {
            panic!("not a fill: {row}")
        };
        assert_eq!(
            f.order_id,
            Some(exec::WIRE_ID.decode(link).unwrap().order_id)
        );
        assert_eq!(f.side, Side::Buy);
    }
}

#[tokio::test]
async fn the_recorded_margin_report_and_account_mode_are_read() {
    let fake = FakeHttp::start(Http::Reply(404, "{}".into()));
    fake.route("/v5/account/wallet-balance", [reply("wallet_balance")]);
    fake.route("/v5/account/info", [reply("account_info")]);
    // By symbol the venue answers one row even with no position (size 0).
    fake.route("/v5/position/list", [reply("position_list_one")]);
    fake.route("/v5/market/instruments-info", [reply("instruments_info")]);
    let q = Queries::new(rest(&fake), 1, 2_000);
    for req in [
        QueryReq::Margin { seq: 1 },
        QueryReq::AccountMode,
        QueryReq::VenueSetting {
            instrument: InstrumentId::new("BTCUSDT").unwrap(),
        },
    ] {
        let a = q.answer(req.clone(), 1).await;
        assert_eq!(a.failed, None, "{req:?}");
        assert!(!a.inputs.is_empty(), "{req:?}");
    }
}
