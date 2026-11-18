//! A ticker delta carries only the fields that changed. Merging it must keep
//! every other field of the state, or the 1 s summary would show nulls.

use md_core::{StreamKey, Ticker};

fn empty(is_snapshot: bool) -> Ticker {
    Ticker {
        key: StreamKey::new("tickers", "BTCUSDT"),
        is_snapshot,
        venue_ts_ns: 0,
        recv_ns: 0,
        last_px: None,
        mark_px: None,
        index_px: None,
        funding_rate: None,
        next_funding_ns: None,
        open_interest: None,
        best_bid_px: None,
        best_bid_qty: None,
        best_ask_px: None,
        best_ask_qty: None,
        px_exp: -1,
        ref_px_exp: -3,
        qty_exp: -3,
    }
}

#[test]
fn merge_overwrites_only_the_fields_the_delta_carries() {
    let mut state = Ticker {
        venue_ts_ns: 10,
        recv_ns: 11,
        last_px: Some(420001),
        mark_px: Some(42000000),
        index_px: Some(41999900),
        funding_rate: Some(10000),
        next_funding_ns: Some(5),
        open_interest: Some(1234567),
        best_bid_px: Some(419999),
        best_bid_qty: Some(2000),
        best_ask_px: Some(420002),
        best_ask_qty: Some(3000),
        ..empty(true)
    };
    let delta = Ticker {
        venue_ts_ns: 20,
        recv_ns: 21,
        last_px: Some(420002),
        best_bid_qty: Some(2500),
        ..empty(false)
    };
    let mut want = state.clone();
    want.venue_ts_ns = 20;
    want.recv_ns = 21;
    want.last_px = Some(420002);
    want.best_bid_qty = Some(2500);
    assert!(state.merge(&delta));
    assert_eq!(state, want);
    let other_tick = Ticker {
        px_exp: -2,
        ref_px_exp: -4,
        last_px: Some(1),
        ..empty(false)
    };
    assert!(!state.merge(&other_tick));
    assert_eq!(
        state, want,
        "a delta of another instrument row changes nothing"
    );
    assert!(state.is_snapshot, "the merged state stays a full state");
}
