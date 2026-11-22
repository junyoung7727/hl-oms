//! Operational status, shared by the local API and the dashboard.
//!
//! Everything here is read from the published (durable) state and from
//! counters. Each value says where it comes from; `as_of` is the wall-clock
//! time at which this process assembled the answer, not a venue time.

use crate::server::ApiCtx;
use crate::util::wall_us;
use oms_core::*;
use serde::Serialize;
use std::sync::atomic::Ordering;

#[derive(Clone, Debug, Serialize)]
pub struct NodeInfo {
    pub version: String,
    /// Digest of the source tree this binary was built from.
    pub source_digest: String,
    pub pid: u32,
    pub started_at: Ts,
    /// "sim", "hyperliquid:<network>" or "bybit:<network>".
    pub venue: String,
    /// The started gateway's form of the client order id (ADR-022).
    pub wire_id_form: WireIdForm,
    pub data_dir: String,
    pub recovery: serde_json::Value,
    pub kill_marker: KillMarkerInfo,
}

/// The kill marker of this process (C-HOLD): process state, read once at
/// the start.
#[derive(Clone, Debug, Default, Serialize)]
pub struct KillMarkerInfo {
    pub configured: bool,
    pub path: Option<String>,
    /// Something was at the path, or the path could not be examined, when
    /// this process started.
    pub present_at_boot: bool,
    /// The path the admin handler removes after a durable clear.
    #[serde(skip)]
    pub file: Option<std::path::PathBuf>,
}

pub fn status(ctx: &ApiCtx, strategy: Option<&StrategyId>) -> serde_json::Value {
    let p = ctx.shared.published.read();
    let core = &p.core;
    let now = wall_us();
    let in_doubt: Vec<serde_json::Value> = core
        .open
        .iter()
        .filter_map(|id| core.order(*id))
        .filter(|o| o.doubt.is_some())
        .map(|o| {
            serde_json::json!({
                "order_id": o.id,
                "strategy": o.strategy,
                "instrument": o.instrument,
                "side": o.side,
                "qty": o.qty,
                "since": o.doubt.as_ref().map(|d| d.since),
                "reason": o.doubt.as_ref().map(|d| d.reason),
                "detail": o.doubt.as_ref().map(|d| d.detail.clone()),
                "expires_at": o.expires_at,
            })
        })
        .collect();
    let mismatches: Vec<&Mismatch> = core.mismatches.values().filter(|m| m.acked.is_none()).collect();
    let phases = |ph: Phase| {
        core.open
            .iter()
            .filter(|id| core.order(**id).is_some_and(|o| o.phase == ph))
            .count()
    };
    let strategies: serde_json::Map<String, serde_json::Value> = core
        .config
        .strategies
        .keys()
        .filter(|s| strategy.is_none_or(|own| own == *s))
        .map(|s| {
            (
                s.to_string(),
                serde_json::json!({
                    "holds": core.holds(Some(s)),
                    "open_orders": core.strategy_open_orders(s),
                    "books": core.strategy_books.get(s),
                }),
            )
        })
        .collect();
    // Margin rules (C-MARGIN): null without a margin policy. The age is
    // this process's clock now minus the query time of the last report.
    let margin = core.margin_view(now);
    serde_json::json!({
        "as_of": now,
        "source": "published state (durable records only) and process counters",
        "node": ctx.info,
        "fatal": ctx.shared.fatal.reason(),
        // Why the gateway keeps the link down (Bybit: auth_rejected,
        // ip_banned, account, unchecked; ADR-028). Null when nothing blocks
        // it, or when the gateway does not report one.
        "gateway": {"block": ctx.gateway_block.as_ref().and_then(|f| f())},
        // The direct alerts (C-ALERT): process counters, never the token or
        // the file path.
        "alerts": crate::alerts::status_json(ctx.alerts.as_deref()),
        "core": {
            "instance_id": format!("{:016x}", core.instance_id),
            "boot": core.boot,
            "config_version": core.config.version,
            "link": core.link,
            "sync": core.sync,
            "holds": core.holds(None),
            "needs": core.needs(),
            "orders_in_memory": core.orders.len(),
            "open_orders": core.open.len(),
            "pending_send": phases(Phase::PendingSend),
            "working": phases(Phase::Working),
            "closing": phases(Phase::Closing),
            "inflight_sends": core.inflight_sends,
            "parked_fills": core.parked_fills.len(),
            "in_doubt": in_doubt,
            "mismatches_unacked": mismatches,
            "operator_halt_account": core.halt_account,
            "operator_halt_strategy": core.halt_strategy,
            "storage_low": core.storage_low,
            // The holds of C-HOLD; null when not held.
            "kill": core.kill,
            "watcher_mismatch": core.watcher_mismatch,
            "venue_change": core.venue_change,
            "fill_floor": core.fill_floor,
            "margin": margin,
            "stats": core.stats,
        },
        "account": if strategy.is_none() { serde_json::to_value(&core.account).ok() } else { None },
        "unattributed": if strategy.is_none() { serde_json::to_value(&core.unattributed).ok() } else { None },
        "strategies": strategies,
        "journal": {
            "durable_seq": p.durable_seq,
            "last_commit_at": p.stats.last_commit_at,
            "last_commit_age_us": now.saturating_sub(p.stats.last_commit_at),
            "last_sync_us": p.stats.last_sync_us,
            "bytes_written": p.stats.journal_bytes,
            "last_snapshot_seq": p.stats.last_snapshot_seq,
            "last_snapshot_at": p.stats.last_snapshot_at,
            "last_snapshot_error": p.stats.last_snapshot_error,
            "records_since_snapshot": p.durable_seq.saturating_sub(p.stats.last_snapshot_seq),
        },
        "pipeline": {
            "stats": p.stats,
            "stage_us": p.stage_summary(),
            "queues": ctx.shared.queues.stats(),
            "queue_lanes": ["venue", "control", "orders"],
            "ingress_rate_limited": ctx.rate_limited.load(Ordering::Relaxed),
            "ingress_refused_overload": ctx.refused_overload.load(Ordering::Relaxed),
        },
    })
}
