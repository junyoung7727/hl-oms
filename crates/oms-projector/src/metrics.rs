//! The `oms_*` and `projector_*` series (ADR-026, design §5.4.5).
//!
//! Every series has the label `venue`; per-instrument series add `symbol`;
//! `oms_limit` and `oms_limit_distance` add `kind` (and `symbol` for the
//! per-instrument kinds). No order id is ever a label. Values are taken from
//! the projector's replayed state, so they show the durable journal only.
//!
//! | series | value |
//! |---|---|
//! | `oms_up` | 1 when the last status query was answered |
//! | `oms_heartbeat_ts` | `as_of` of the last answer, seconds (absent before the first) |
//! | `oms_journal_durable_seq` | the durable sequence the projector reads up to |
//! | `oms_halt` | 1 while any hold refuses new exposure (`Core::holds`) |
//! | `oms_holds{reason}` | 1 while that hold reason is on; 0 for a reason seen earlier in this run |
//! | `oms_open_orders`, `oms_in_doubt_orders`, `oms_parked_fills` | counts |
//! | `oms_position_qty{symbol}` | account book position |
//! | `oms_realized_pnl{symbol}`, `oms_upnl{symbol}` | average-cost method ([`crate::pnl`]); `oms_upnl` at the reference price, absent without one |
//! | `oms_account_value` | of the last margin report |
//! | `oms_margin_ratio{symbol}` | margin used ÷ position value of the last margin report |
//! | `oms_limit`, `oms_limit_distance` | see [`limits`] |
//! | `projector_lag_records` | durable sequence − last applied record |
//! | `projector_cursor_seq`, `projector_pending_lines`, `projector_unconfirmed_records`, `projector_tail_error`, `projector_caught_up`, `projector_rejected_lines` | the projector's own state |
//! | `journal_disk_free_bytes` | `statvfs` of the journal file system, every `disk_every_ms` |
//!
//! Until the replay first reaches the read limit (`projector_caught_up` = 0)
//! only `oms_up`, `oms_heartbeat_ts`, `oms_journal_durable_seq`, the
//! `projector_*` series and `journal_disk_free_bytes` are pushed: the state
//! is then a prefix of the journal and would show, for example, a flat
//! position.
//!
//! `oms_rate_tokens` is not here: the OMS has no venue rate budget yet (U17).

use crate::pnl::Pnl;
use obs_push::Sample;
use oms_core::num::SCALE;
use oms_core::{Core, LeverageType};
use std::collections::{BTreeMap, BTreeSet};

/// Everything a metrics pass reads.
pub struct View<'a> {
    pub core: &'a Core,
    pub pnl: &'a BTreeMap<String, Pnl>,
    pub venue: &'a str,
    pub now_us: u64,
    pub up: bool,
    pub heartbeat_us: Option<u64>,
    pub durable_seq: u64,
    pub applied_seq: u64,
    pub cursor: u64,
    pub pending_lines: usize,
    pub unconfirmed: usize,
    pub tail_error: bool,
    /// Hold reasons seen in this run (a reason that went away is pushed as 0).
    pub holds_seen: &'a BTreeSet<String>,
    /// The replay has reached the read limit. Before that the state is a
    /// prefix of the journal and no `oms_*` state series is pushed.
    pub caught_up: bool,
    /// Lines the log store refused for good in this run.
    pub rejected_lines: u64,
}

fn dec(v: i64) -> f64 {
    v as f64 / SCALE as f64
}

struct Out<'a> {
    venue: &'a str,
    ts_ms: i64,
    v: Vec<Sample>,
}

impl Out<'_> {
    fn push(&mut self, name: &str, extra: &[(&str, &str)], value: f64) {
        let mut labels = BTreeMap::new();
        labels.insert("venue".to_string(), self.venue.to_string());
        for (k, val) in extra {
            labels.insert((*k).to_string(), (*val).to_string());
        }
        self.v.push(Sample {
            name: name.to_string(),
            labels,
            value,
            ts_ms: self.ts_ms,
        });
    }
}

fn b(x: bool) -> f64 {
    if x { 1.0 } else { 0.0 }
}

/// The name of a hold reason: its string, or the variant of a one-key
/// object (`venue_setting`).
pub fn hold_name(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Object(m) => m.keys().next().cloned().unwrap_or_default(),
        other => other.to_string(),
    }
}

/// The hold reasons on now, by name.
pub fn holds_now(core: &Core) -> BTreeSet<String> {
    core.holds(None)
        .iter()
        .filter_map(|h| serde_json::to_value(h).ok())
        .map(|v| hold_name(&v))
        .collect()
}

pub fn samples(v: &View<'_>) -> Vec<Sample> {
    let core = v.core;
    let mut o = Out {
        venue: v.venue,
        ts_ms: i64::try_from(v.now_us / 1000).unwrap_or(i64::MAX),
        v: Vec::new(),
    };
    o.push("oms_up", &[], b(v.up));
    if let Some(hb) = v.heartbeat_us {
        o.push("oms_heartbeat_ts", &[], hb as f64 / 1e6);
    }
    o.push("oms_journal_durable_seq", &[], v.durable_seq as f64);
    o.push(
        "projector_lag_records",
        &[],
        v.durable_seq.saturating_sub(v.applied_seq) as f64,
    );
    o.push("projector_cursor_seq", &[], v.cursor as f64);
    o.push("projector_pending_lines", &[], v.pending_lines as f64);
    o.push("projector_unconfirmed_records", &[], v.unconfirmed as f64);
    o.push("projector_tail_error", &[], b(v.tail_error));
    o.push("projector_caught_up", &[], b(v.caught_up));
    o.push("projector_rejected_lines", &[], v.rejected_lines as f64);
    if !v.caught_up {
        return o.v;
    }
    let holds = holds_now(core);
    o.push("oms_halt", &[], b(!holds.is_empty()));
    for reason in v.holds_seen.union(&holds) {
        o.push(
            "oms_holds",
            &[("reason", reason)],
            b(holds.contains(reason)),
        );
    }
    o.push("oms_open_orders", &[], core.open.len() as f64);
    let in_doubt = core
        .open
        .iter()
        .filter_map(|id| core.order(*id))
        .filter(|o| o.doubt.is_some())
        .count();
    o.push("oms_in_doubt_orders", &[], in_doubt as f64);
    o.push("oms_parked_fills", &[], core.parked_fills.len() as f64);

    let mut symbols: BTreeSet<String> = core
        .config
        .instruments
        .keys()
        .map(|i| i.to_string())
        .collect();
    symbols.extend(core.account.keys().map(|i| i.to_string()));
    symbols.extend(v.pnl.keys().cloned());
    for sym in &symbols {
        let book = core
            .account
            .iter()
            .find(|(i, _)| i.as_str() == sym.as_str())
            .map(|(_, b)| b);
        o.push(
            "oms_position_qty",
            &[("symbol", sym)],
            book.map_or(0.0, |b| dec(b.pos.0)),
        );
        let pnl = v.pnl.get(sym).cloned().unwrap_or_default();
        o.push("oms_realized_pnl", &[("symbol", sym)], pnl.realized);
        let mark = core
            .ref_px
            .iter()
            .find(|(i, _)| i.as_str() == sym.as_str())
            .map(|(_, r)| dec(r.px.0));
        if let Some(u) = mark.and_then(|m| pnl.upnl(m)) {
            o.push("oms_upnl", &[("symbol", sym)], u);
        }
    }
    if let Some(report) = &core.margin.report {
        o.push("oms_account_value", &[], dec(report.account_value.0));
        for p in &report.positions {
            if p.position_value.0 > 0 {
                let sym = p.instrument.to_string();
                o.push(
                    "oms_margin_ratio",
                    &[("symbol", &sym)],
                    dec(p.margin_used.0) / dec(p.position_value.0),
                );
            }
        }
    }
    for l in limits(core) {
        let mut labels: Vec<(&str, &str)> = vec![("kind", l.kind)];
        if let Some(s) = &l.symbol {
            labels.push(("symbol", s));
        }
        o.push("oms_limit", &labels, l.limit);
        o.push("oms_limit_distance", &labels, l.distance);
    }
    o.v
}

/// One limit and how far the state is from it, in the limit's unit. A
/// negative distance is a limit already passed.
#[derive(Clone, Debug, PartialEq)]
pub struct Limit {
    pub kind: &'static str,
    pub symbol: Option<String>,
    pub limit: f64,
    pub distance: f64,
}

/// The account-level limits the OMS enforces (per-strategy limits are not
/// shown), as the projector can compute them:
///
/// | kind | limit | distance |
/// |---|---|---|
/// | `open_orders` | `policy.max_open_orders` | limit − open orders |
/// | `position_max{symbol}` | `account_pos_max` | limit − (position + open buys) |
/// | `position_min{symbol}` | `account_pos_min` | (position − open sells) − limit |
/// | `leverage` | `max_account_leverage` × account value (quote) | limit − account exposure (C-MARGIN leverage gate) |
/// | `daily_loss` | `daily_loss_bps` of the day base (quote) | limit − (day base − account value) |
/// | `mm_buffer{symbol}` | (1 ÷ (2 × max leverage) + `mm_buffer_bps`) × position value (quote), isolated positions | margin used − limit |
///
/// The margin kinds need a margin policy and a report (and a day base for
/// `daily_loss`); without them they are absent.
pub fn limits(core: &Core) -> Vec<Limit> {
    let mut out = Vec::new();
    let policy = &core.config.policy;
    out.push(Limit {
        kind: "open_orders",
        symbol: None,
        limit: f64::from(policy.max_open_orders),
        distance: f64::from(policy.max_open_orders) - core.open.len() as f64,
    });
    for (instrument, spec) in &core.config.instruments {
        let book = core.account_book(instrument);
        let sym = Some(instrument.to_string());
        let max = dec(spec.account_pos_max.0);
        let min = dec(spec.account_pos_min.0);
        let pos = dec(book.pos.0);
        out.push(Limit {
            kind: "position_max",
            symbol: sym.clone(),
            limit: max,
            distance: max - (pos + dec(book.buy_open.0)),
        });
        out.push(Limit {
            kind: "position_min",
            symbol: sym,
            limit: min,
            distance: (pos - dec(book.sell_open.0)) - min,
        });
    }
    let (Some(margin), Some(report)) = (policy.margin.as_ref(), core.margin.report.as_ref()) else {
        return out;
    };
    let value = dec(report.account_value.0);
    let lev_limit = dec(margin.max_account_leverage.0) * value;
    if let Some(exposure) = core.account_exposure() {
        out.push(Limit {
            kind: "leverage",
            symbol: None,
            limit: lev_limit,
            distance: lev_limit - exposure.0 as f64 / (SCALE as f64 * SCALE as f64),
        });
    }
    if let Some(base) = &core.margin.day_base {
        let base_v = dec(base.value.0);
        let allowed = base_v * f64::from(margin.daily_loss_bps) / 10_000.0;
        out.push(Limit {
            kind: "daily_loss",
            symbol: None,
            limit: allowed,
            distance: allowed - (base_v - value),
        });
    }
    for p in &report.positions {
        if p.leverage_type != LeverageType::Isolated
            || p.position_value.0 <= 0
            || p.max_leverage == 0
        {
            continue;
        }
        let pv = dec(p.position_value.0);
        let line = pv
            * (1.0 / (2.0 * f64::from(p.max_leverage))
                + f64::from(margin.mm_buffer_bps) / 10_000.0);
        out.push(Limit {
            kind: "mm_buffer",
            symbol: Some(p.instrument.to_string()),
            limit: line,
            distance: dec(p.margin_used.0) - line,
        });
    }
    out
}
