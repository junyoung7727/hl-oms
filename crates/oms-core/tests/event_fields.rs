//! The per-event field table of the tail contract (docs/contracts/tail.md)
//! lists every field the journal encoder writes (U14a). The watcher (U19a)
//! is built from that table alone, so a field missing there is a field the
//! watcher cannot read.

#![allow(clippy::unwrap_used)]

mod common;
use common::*;
use oms_core::*;
use serde_json::Value;
use std::collections::BTreeSet;

/// One instance of every event kind, with every optional field present.
fn every_event() -> Vec<Event> {
    let h = H::ready();
    let mut request = h.req("a", "k1", Side::Buy, "1", "100");
    request.market_ref = Some(MarketRef {
        venue: RefName::new("bybit").unwrap(),
        market: RefName::new("linear").unwrap(),
        symbol: RefName::new("BTCUSDT").unwrap(),
        channel: RefName::new("orderbook.50").unwrap(),
        update_id: Some(1),
        venue_ts_ns: 2,
        recv_ns: 3,
    });
    let fill = FillIn {
        key: fkey("t1"),
        order_id: Some(OrderId(1)),
        venue_oid: Some(void("v1")),
        instrument: iid("X"),
        side: Side::Buy,
        qty: q("1"),
        px: px("100"),
        fee: amt("0.01"),
        venue_ts: 5,
        is_maker: Some(true),
        recv_ns: Some(6),
    };
    let report = MarginReport {
        session: 1,
        seq: 1,
        queried_at: 5,
        account_value: amt("100"),
        positions: vec![MarginPosition {
            instrument: iid("X"),
            qty: sq("1"),
            position_value: amt("100"),
            margin_used: amt("10"),
            leverage_type: LeverageType::Isolated,
            leverage_value: 10,
            max_leverage: 50,
        }],
    };
    let id = OrderId(1);
    vec![
        Event::ConfigActivated {
            config: Box::new(config()),
        },
        Event::Booted {
            boot: 1,
            binary: "b".into(),
            fill_floor: 0,
        },
        Event::LinkChanged {
            up: true,
            session: 1,
            reason: "r".into(),
        },
        Event::RequestRejected {
            request: request.clone(),
            reasons: vec![RejectReason::QtyStep],
        },
        Event::OrderAccepted {
            order_id: id,
            request,
            config_version: 1,
            ref_px: Some(px("100")),
        },
        Event::OrderAcked {
            order_id: id,
            venue_oid: Some(void("v1")),
            via: EvidenceSource::Stream,
            session: 1,
        },
        Event::VenueOidLearned {
            order_id: id,
            venue_oid: void("v1"),
        },
        Event::OrderInDoubt {
            order_id: id,
            reason: DoubtReason::Transport,
            detail: "d".into(),
        },
        Event::StatusNoted {
            order_id: id,
            session: 1,
            reported_cum: q("1"),
            venue_ts: 5,
        },
        Event::OrderRecheck { order_id: id },
        Event::FillBooked {
            order_id: id,
            key: fkey("t1"),
            qty: q("1"),
            px: px("100"),
            fee: amt("0.01"),
            venue_ts: 5,
            is_maker: Some(false),
            recv_ns: Some(6),
        },
        Event::FillParked { fill: fill.clone() },
        Event::FillUnparked { key: fkey("t1") },
        Event::ExternalFillBooked { fill },
        Event::VenueTerminal {
            order_id: id,
            kind: VenueFinal::Filled,
            reason: "r".into(),
            final_cum: Some(q("1")),
            venue_ts: 5,
        },
        Event::MissingFillsFlagged { order_id: id },
        Event::OrderClosed {
            order_id: id,
            reason: CloseReason::Filled,
        },
        Event::CancelRequested {
            order_id: id,
            by: Actor::Operator,
        },
        Event::CancelSent {
            order_id: id,
            attempt: 1,
        },
        Event::CancelOutcomeNoted {
            order_id: id,
            attempt: 1,
            outcome: CancelOutcome::Confirmed,
        },
        Event::FillsSynced {
            session: 1,
            up_to: 5,
        },
        Event::OpenOrdersSwept { session: 1 },
        Event::PositionsVerified { session: 1 },
        Event::MismatchRaised {
            id: 1,
            kind: MismatchKind::ExternalFill,
            subject: "s".into(),
            detail: "d".into(),
        },
        Event::MismatchAcked {
            id: 1,
            note: "n".into(),
        },
        Event::OperatorHalted {
            scope: Scope::Account,
            note: "n".into(),
        },
        Event::OperatorResumed {
            scope: Scope::Account,
            note: "n".into(),
        },
        Event::StorageLowChanged {
            low: true,
            free_bytes: 1,
        },
        Event::RefPriceUpdated {
            instrument: iid("X"),
            px: px("100"),
            venue_ts: 5,
        },
        Event::PositionAdjusted {
            instrument: iid("X"),
            ledger: Ledger::Unattributed,
            delta: sq("1"),
            note: "n".into(),
            reason: AdjustReason::SnapshotAfterVenueChange,
        },
        Event::Purged {
            order_ids: vec![id],
            cutoff: 5,
            fill_floor: 5,
        },
        Event::MarginReported { report },
        Event::MarginReportRejected {
            session: 1,
            seq: 1,
            queried_at: 5,
            reason: "r".into(),
        },
        Event::DayBaseSet {
            day: 1,
            value: amt("100"),
            set_by: BaseSetBy::Report,
        },
        Event::ReducingEntered {
            rule: RuleId::DailyLoss,
            detail: "d".into(),
        },
        Event::VenueSettingNoted {
            session: 1,
            instrument: iid("X"),
            isolated: true,
            leverage: 10,
        },
        Event::AccountModeNoted {
            session: 1,
            mode: "m".into(),
        },
        Event::ReducingCleared {
            note: "n".into(),
            old_base: Some(amt("100")),
            new_base: amt("90"),
        },
        Event::FundingBooked {
            funding: FundingIn {
                key: fkey("f1"),
                instrument: iid("X"),
                amount: amt("0.1"),
                rate: "0.0001".into(),
                venue_ts: 5,
            },
        },
        Event::VenuePositionChanged {
            change: VenuePositionChangeIn {
                key: fkey("adl1"),
                kind: VenueChangeKind::Adl,
                instrument: iid("X"),
                qty: sq("-1"),
                px: px("100"),
                venue_ts: 5,
            },
        },
        Event::UnclassifiedExecutionNoted {
            key: fkey("e1"),
            instrument: iid("X"),
            exec_type: "T".into(),
            detail: "d".into(),
            venue_ts: 5,
        },
        Event::VenueChangeSettled {
            instrument: iid("X"),
        },
        Event::AbsenceNoted {
            order_id: id,
            queried_at: 5,
        },
    ]
}

/// Fails to compile when an event kind is added: add it to `every_event`
/// and to the table.
fn _every_kind_is_listed(e: &Event) {
    match e {
        Event::ConfigActivated { .. }
        | Event::Booted { .. }
        | Event::LinkChanged { .. }
        | Event::RequestRejected { .. }
        | Event::OrderAccepted { .. }
        | Event::OrderAcked { .. }
        | Event::VenueOidLearned { .. }
        | Event::OrderInDoubt { .. }
        | Event::StatusNoted { .. }
        | Event::OrderRecheck { .. }
        | Event::FillBooked { .. }
        | Event::FillParked { .. }
        | Event::FillUnparked { .. }
        | Event::ExternalFillBooked { .. }
        | Event::VenueTerminal { .. }
        | Event::MissingFillsFlagged { .. }
        | Event::OrderClosed { .. }
        | Event::CancelRequested { .. }
        | Event::CancelSent { .. }
        | Event::CancelOutcomeNoted { .. }
        | Event::FillsSynced { .. }
        | Event::OpenOrdersSwept { .. }
        | Event::PositionsVerified { .. }
        | Event::MismatchRaised { .. }
        | Event::MismatchAcked { .. }
        | Event::OperatorHalted { .. }
        | Event::OperatorResumed { .. }
        | Event::StorageLowChanged { .. }
        | Event::RefPriceUpdated { .. }
        | Event::PositionAdjusted { .. }
        | Event::Purged { .. }
        | Event::MarginReported { .. }
        | Event::MarginReportRejected { .. }
        | Event::DayBaseSet { .. }
        | Event::ReducingEntered { .. }
        | Event::VenueSettingNoted { .. }
        | Event::AccountModeNoted { .. }
        | Event::ReducingCleared { .. }
        | Event::FundingBooked { .. }
        | Event::VenuePositionChanged { .. }
        | Event::UnclassifiedExecutionNoted { .. }
        | Event::VenueChangeSettled { .. }
        | Event::AbsenceNoted { .. } => {}
    }
}

/// The rows of the field table: (event, field path, type). A type that
/// starts with `enum` or `object` is read as a whole; the check does not
/// look inside it.
fn table() -> Vec<(String, String, String)> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/contracts/tail.md");
    let text = std::fs::read_to_string(path).unwrap();
    let start = text
        .find("<!-- event-fields:begin -->")
        .expect("table start marker");
    let end = text
        .find("<!-- event-fields:end -->")
        .expect("table end marker");
    text[start..end]
        .lines()
        .filter(|l| l.starts_with("| `"))
        .map(|l| {
            let cells: Vec<&str> = l.split('|').map(str::trim).collect();
            let unquote = |s: &str| s.trim_matches('`').to_string();
            (unquote(cells[1]), unquote(cells[2]), cells[3].to_string())
        })
        .collect()
}

/// Every field path the encoder writes for one event, stopping at a path
/// the table reads as a whole.
fn paths(v: &Value, prefix: &str, whole: &BTreeSet<String>, out: &mut BTreeSet<String>) {
    match v {
        Value::Object(m) if !whole.contains(prefix) => {
            for (k, child) in m {
                let p = if prefix.is_empty() {
                    k.clone()
                } else {
                    format!("{prefix}.{k}")
                };
                paths(child, &p, whole, out);
            }
        }
        Value::Array(items) if !whole.contains(prefix) && items.iter().any(Value::is_object) => {
            for item in items {
                paths(item, &format!("{prefix}[]"), whole, out);
            }
        }
        _ => {
            out.insert(prefix.to_string());
        }
    }
}

#[test]
fn the_field_table_lists_every_field_the_encoder_writes() {
    let rows = table();
    let mut missing = Vec::new();
    let mut kinds = BTreeSet::new();
    for ev in every_event() {
        let name = ev.name();
        kinds.insert(name);
        let json = serde_json::to_value(&ev).unwrap();
        let body = json.get(name).cloned().unwrap_or(Value::Null);
        let whole: BTreeSet<String> = rows
            .iter()
            .filter(|(e, _, t)| e == name && (t.starts_with("enum") || t.starts_with("object")))
            .map(|(_, f, _)| f.clone())
            .collect();
        let mut written = BTreeSet::new();
        paths(&body, "", &whole, &mut written);
        for field in written {
            if !rows.iter().any(|(e, f, _)| e == name && *f == field) {
                missing.push(format!("{name}.{field}"));
            }
        }
    }
    assert!(
        missing.is_empty(),
        "fields the table does not list: {missing:#?}"
    );
    // And the table names no event kind that does not exist.
    for (e, _, _) in &rows {
        assert!(
            kinds.contains(e.as_str()),
            "the table lists an unknown event {e}"
        );
    }
    assert_eq!(kinds.len(), 43, "one instance of every event kind");
}

/// Whether a JSON value has the kind the type column names. "… 또는 null"
/// also allows null.
fn kind_matches(t: &str, v: &Value) -> bool {
    if v.is_null() {
        return t.contains("또는 null");
    }
    if t.contains("list") {
        return v.is_array();
    }
    if t.starts_with("decimal string") || t.starts_with("string") {
        return v.is_string();
    }
    if t.starts_with("u64") || t.starts_with("u32") || t.starts_with("i64") {
        return v.is_u64() || v.is_i64();
    }
    if t.starts_with("bool") {
        return v.is_boolean();
    }
    if t.starts_with("enum") {
        // A unit variant, or one key naming the variant.
        return v.is_string() || v.as_object().is_some_and(|m| m.len() == 1);
    }
    if t.starts_with("object") {
        return v.is_object();
    }
    false
}

/// Review 2026-11-20. A row read as a whole hides every field under it, so
/// only the configuration and enum values may be one: a row like
/// `order_accepted | request | object` would let the rows of the request
/// be dropped.
#[test]
fn only_the_configuration_and_enums_are_read_as_a_whole() {
    for (e, f, t) in table() {
        if t.starts_with("object") {
            assert_eq!(
                (e.as_str(), f.as_str()),
                ("config_activated", "config"),
                "{e}.{f} ({t})"
            );
        }
    }
}

#[test]
fn every_row_of_the_table_is_a_field_the_encoder_writes() {
    let rows = table();
    let events = every_event();
    for (e, field, t) in &rows {
        let ev = events.iter().find(|x| x.name() == e).unwrap();
        let json = serde_json::to_value(ev).unwrap();
        let mut v = json.get(e.as_str()).unwrap();
        for part in field.split('.') {
            let key = part.trim_end_matches("[]");
            v = v
                .get(key)
                .unwrap_or_else(|| panic!("{e}.{field} ({t}) is not written"));
            if part.ends_with("[]") {
                v = v
                    .get(0)
                    .unwrap_or_else(|| panic!("{e}.{field}: empty list"));
            }
        }
        assert!(
            kind_matches(t, v),
            "{e}.{field}: the table says {t}, the encoder writes {v}"
        );
    }
}
