//! The maker flag and the receive time of a fill (U16a): the gateway puts
//! them in the fill report, the core carries them to the booked fill, and a
//! journal written before them reads and writes the same bytes.

#![allow(clippy::unwrap_used)]

mod common;
use common::*;
use oms_core::*;

fn report(key: &str, order_id: Option<OrderId>, oid: &str, ts: Ts) -> FillIn {
    FillIn {
        key: fkey(key),
        order_id,
        venue_oid: Some(void(oid)),
        instrument: iid("X"),
        side: Side::Buy,
        qty: q("1"),
        px: px("100"),
        fee: amt("-0.01"),
        venue_ts: ts,
        is_maker: Some(true),
        recv_ns: Some(ts as i64 * 1000 + 7),
    }
}

fn booked(h: &H) -> Vec<(FillKey, Option<bool>, Option<i64>)> {
    h.log
        .iter()
        .flat_map(|r| r.events.iter())
        .filter_map(|e| match e {
            Event::FillBooked {
                key,
                is_maker,
                recv_ns,
                ..
            } => Some((key.clone(), *is_maker, *recv_ns)),
            _ => None,
        })
        .collect()
}

#[test]
fn the_booked_fill_keeps_the_reports_maker_flag_and_receive_time() {
    let mut h = H::ready();
    let a = h.accept("a", "k1", Side::Buy, "2", "100");
    h.placed_open(a, "v1");
    let ts = h.now;
    h.venue(VenueInput::Fill(report("t1", Some(a), "v1", ts)));
    assert_eq!(
        booked(&h),
        vec![(fkey("t1"), Some(true), Some(ts as i64 * 1000 + 7))]
    );
}

#[test]
fn a_parked_fill_keeps_them_until_it_is_booked() {
    let mut h = H::ready();
    let a = h.accept("a", "k1", Side::Buy, "2", "100"); // send result not in yet
    let ts = h.now;
    h.venue(VenueInput::Fill(report("t1", None, "v1", ts)));
    assert_eq!(h.core.parked_fills.len(), 1);
    assert_eq!(booked(&h), vec![]);
    h.placed_open(a, "v1"); // the venue order id identifies the parked fill
    assert_eq!(
        booked(&h),
        vec![(fkey("t1"), Some(true), Some(ts as i64 * 1000 + 7))]
    );
}

#[test]
fn absent_fields_are_not_written_and_old_records_keep_their_bytes() {
    let mut h = H::ready();
    let a = h.accept("a", "k1", Side::Buy, "2", "100");
    h.placed_open(a, "v1");
    h.fill(a, "t1", "1", "100"); // a report without the two fields
    for rec in &h.log {
        let text = serde_json::to_string(rec).unwrap();
        assert!(
            !text.contains("is_maker") && !text.contains("recv_ns"),
            "{text}"
        );
        let again: Record = serde_json::from_str(&text).unwrap();
        assert_eq!(
            serde_json::to_string(&again).unwrap(),
            text,
            "record {}",
            rec.seq
        );
    }
    assert_eq!(booked(&h), vec![(fkey("t1"), None, None)]);
}
