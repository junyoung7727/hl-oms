//! Core configuration: instruments, strategies, limits and policy values.
//!
//! Configuration is domain state. It enters the core as an input, is recorded
//! in the journal as a `ConfigActivated` event with its full content, and is
//! restored by replay. The configuration file is never read during recovery.

use crate::ids::*;
use crate::num::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoreConfig {
    /// Chosen by the operator; must increase with every activation.
    pub version: u32,
    pub instruments: BTreeMap<InstrumentId, InstrumentSpec>,
    pub strategies: BTreeMap<StrategyId, StrategySpec>,
    pub policy: Policy,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstrumentSpec {
    /// Order quantity must be a positive multiple of this.
    pub qty_step: Qty,
    /// Order price must be a positive multiple of this.
    pub px_tick: Px,
    /// If set, a price may not have more significant digits than this, unless
    /// it is an integer (Hyperliquid's price rule).
    pub px_max_sig_figs: Option<u32>,
    pub min_notional: Amount,
    pub max_order_qty: Qty,
    pub max_order_notional: Amount,
    /// If set, a limit price further than this from the reference price is
    /// rejected. Requires a fresh reference price.
    pub price_band_bps: Option<u32>,
    /// Account-wide bounds on the signed position, including every fill that
    /// could still happen on open orders.
    pub account_pos_min: SQty,
    pub account_pos_max: SQty,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PosLimit {
    pub min: SQty,
    pub max: SQty,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StrategySpec {
    pub enabled: bool,
    pub max_open_orders: u32,
    /// A strategy may trade only the instruments listed here.
    pub limits: BTreeMap<InstrumentId, PosLimit>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Policy {
    /// Account-wide cap on orders that are not closed.
    pub max_open_orders: u32,
    /// Cap on orders released to the gateway without venue evidence yet. This
    /// bounds the gateway's outbound queue by construction.
    pub max_inflight_sends: u32,
    /// Cap on orders kept in memory, closed ones included.
    pub max_orders_in_memory: u32,
    /// A request may not ask for an expiry further ahead than this.
    pub max_expiry_horizon_us: u64,
    pub ref_price_max_age_us: u64,
    /// How long a closed order stays queryable in memory.
    pub closed_retention_us: u64,
    /// Extra time an idempotency key is remembered after the request expired.
    pub dedup_grace_us: u64,
    /// A released send without any result for this long becomes in-doubt.
    pub send_result_timeout_us: u64,
    /// Whether the venue refuses to place a request after its expiry time.
    /// Only then can "not found after expiry" close an in-doubt order.
    pub venue_enforces_expiry: bool,
    /// Margin added to the expiry before absence is accepted as proof.
    pub absence_margin_us: u64,
    /// How many "not found" answers, each to a query sent after expiry plus
    /// margin and later than the one before, close an in-doubt order (U13c).
    /// One for Hyperliquid; two for Bybit, whose open-order list drops old
    /// closed orders. Not written while one.
    #[serde(skip_serializing_if = "is_one")]
    pub absence_not_found_needed: u32,
    pub cancel_retry_backoff_us: u64,
    pub cancel_max_attempts: u32,
    /// Longest a fill that names only an unknown venue order id waits for one
    /// of our unidentified orders to turn out to be its owner. Must exceed
    /// the time an in-doubt order takes to resolve (expiry horizon plus
    /// absence margin), or a fill of ours is booked as external.
    pub unmatched_fill_wait_us: u64,
    pub max_parked_fills: u32,
    pub purge_interval_us: u64,
    /// How far behind the fill-sync watermark fill keys are remembered. A
    /// fill older than that is treated as a re-delivery.
    pub fill_dedup_retention_us: u64,
    /// Margin rules (ADR-018, C-MARGIN). Absent means the rules are off and
    /// every hold refuses every new order, as before the rules existed. An
    /// absent section is not serialised, so a journal of a run without the
    /// rules keeps the bytes it had before the section existed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub margin: Option<MarginPolicy>,
}

/// Values of the margin rules. The section is optional; see `Policy::margin`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MarginPolicy {
    /// Account exposure may not exceed this many times the account value.
    pub max_account_leverage: Ratio,
    /// Loss against the day base, in basis points of the base, beyond which
    /// the account only reduces.
    pub daily_loss_bps: u32,
    /// Distance kept above the maintenance margin rate, in basis points of
    /// the position value.
    pub mm_buffer_bps: u32,
    /// A margin report older than this is not fresh.
    pub margin_max_age_us: u64,
    /// Without an accepted margin report for longer than this, resting plain
    /// orders are cancelled.
    pub stale_cancel_us: u64,
    /// Leverage every instrument must have at the venue.
    pub venue_leverage: u32,
    /// Margin mode every instrument must have at the venue.
    pub isolated: bool,
    /// Account modes accepted besides none. `unifiedAccount` and
    /// `portfolioMargin` are never accepted, also when listed.
    pub account_modes_allowed: Vec<String>,
}

impl Default for MarginPolicy {
    fn default() -> Self {
        MarginPolicy {
            max_account_leverage: Ratio(2 * SCALE),
            daily_loss_bps: 500,
            mm_buffer_bps: 25,
            margin_max_age_us: 30_000_000,
            stale_cancel_us: 60_000_000,
            venue_leverage: 2,
            isolated: true,
            account_modes_allowed: Vec::new(),
        }
    }
}

fn is_one(v: &u32) -> bool {
    *v == 1
}

impl Default for Policy {
    fn default() -> Self {
        Policy {
            max_open_orders: 500,
            max_inflight_sends: 64,
            max_orders_in_memory: 100_000,
            max_expiry_horizon_us: 60_000_000,
            ref_price_max_age_us: 15_000_000,
            closed_retention_us: 3_600_000_000,
            dedup_grace_us: 60_000_000,
            send_result_timeout_us: 30_000_000,
            venue_enforces_expiry: true,
            absence_margin_us: 10_000_000,
            absence_not_found_needed: 1,
            cancel_retry_backoff_us: 1_000_000,
            cancel_max_attempts: 5,
            unmatched_fill_wait_us: 120_000_000,
            max_parked_fills: 1024,
            purge_interval_us: 60_000_000,
            fill_dedup_retention_us: 86_400_000_000,
            margin: None,
        }
    }
}

impl CoreConfig {
    /// A configuration with no instruments and no strategies. Every order is
    /// rejected until a real configuration is activated.
    pub fn empty() -> CoreConfig {
        CoreConfig {
            version: 0,
            instruments: BTreeMap::new(),
            strategies: BTreeMap::new(),
            policy: Policy::default(),
        }
    }

    /// Checks internal consistency. The result lists every problem found.
    pub fn validate(&self) -> Result<(), Vec<String>> {
        let mut errs = Vec::new();
        if self.version == 0 {
            errs.push("version must be at least 1".to_string());
        }
        for (id, spec) in &self.instruments {
            if spec.qty_step.0 <= 0 {
                errs.push(format!("{id}: qty_step must be positive"));
            }
            if spec.px_tick.0 <= 0 {
                errs.push(format!("{id}: px_tick must be positive"));
            }
            if spec.max_order_qty.0 <= 0 {
                errs.push(format!("{id}: max_order_qty must be positive"));
            }
            if spec.max_order_notional.0 <= 0 {
                errs.push(format!("{id}: max_order_notional must be positive"));
            }
            if spec.min_notional.0 < 0 {
                errs.push(format!("{id}: min_notional must not be negative"));
            }
            if spec.account_pos_min > spec.account_pos_max {
                errs.push(format!("{id}: account_pos_min exceeds account_pos_max"));
            }
            if let Some(n) = spec.px_max_sig_figs {
                if n == 0 || n > 18 {
                    errs.push(format!("{id}: px_max_sig_figs must be 1..=18"));
                }
            }
            if let Some(b) = spec.price_band_bps {
                if b == 0 || b >= 10_000 {
                    errs.push(format!("{id}: price_band_bps must be 1..=9999"));
                }
            }
        }
        for (sid, s) in &self.strategies {
            for (iid, lim) in &s.limits {
                if !self.instruments.contains_key(iid) {
                    errs.push(format!("strategy {sid}: unknown instrument {iid}"));
                }
                if lim.min > lim.max {
                    errs.push(format!("strategy {sid}/{iid}: min exceeds max"));
                }
            }
        }
        let p = &self.policy;
        if p.max_open_orders == 0 || p.max_inflight_sends == 0 || p.max_orders_in_memory == 0 {
            errs.push("policy: capacity limits must be positive".to_string());
        }
        if p.max_orders_in_memory < p.max_open_orders {
            errs.push("policy: max_orders_in_memory must be at least max_open_orders".to_string());
        }
        if p.max_expiry_horizon_us == 0 {
            errs.push("policy: max_expiry_horizon_us must be positive".to_string());
        }
        if p.cancel_max_attempts == 0 {
            errs.push("policy: cancel_max_attempts must be positive".to_string());
        }
        if p.absence_not_found_needed == 0 {
            errs.push("policy: absence_not_found_needed must be positive".to_string());
        }
        for (name, v) in [
            ("send_result_timeout_us", p.send_result_timeout_us),
            ("absence_margin_us", p.absence_margin_us),
            ("cancel_retry_backoff_us", p.cancel_retry_backoff_us),
            ("purge_interval_us", p.purge_interval_us),
            ("fill_dedup_retention_us", p.fill_dedup_retention_us),
            ("ref_price_max_age_us", p.ref_price_max_age_us),
        ] {
            if v == 0 {
                errs.push(format!("policy: {name} must be positive"));
            }
        }
        // A fill that names only an unknown venue order id waits for one of
        // our unidentified orders. Such an order can stay unidentified for as
        // long as an in-doubt order takes to resolve; a shorter wait would
        // book our own fill as external.
        if p.unmatched_fill_wait_us <= p.max_expiry_horizon_us.saturating_add(p.absence_margin_us) {
            errs.push("policy: unmatched_fill_wait_us must exceed max_expiry_horizon_us + absence_margin_us".to_string());
        }
        if let Some(m) = &p.margin {
            // The 설정 table of C-MARGIN.
            if m.max_account_leverage.0 <= 0 {
                errs.push("policy.margin: max_account_leverage must be greater than 0".to_string());
            }
            if m.daily_loss_bps == 0 {
                errs.push("policy.margin: daily_loss_bps must be greater than 0".to_string());
            }
            if m.margin_max_age_us == 0 {
                errs.push("policy.margin: margin_max_age_us must be greater than 0".to_string());
            }
            if m.stale_cancel_us <= m.margin_max_age_us {
                errs.push("policy.margin: stale_cancel_us must be greater than margin_max_age_us".to_string());
            }
            if m.venue_leverage == 0 {
                errs.push("policy.margin: venue_leverage must be at least 1".to_string());
            }
        }
        if errs.is_empty() { Ok(()) } else { Err(errs) }
    }
}
