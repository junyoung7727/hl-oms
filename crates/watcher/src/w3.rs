//! W3 wallet and margin (C-WATCH part 9, U19a9): the unified account of
//! `/v5/account/wallet-balance?accountType=UNIFIED` judged by the OMS's own
//! margin rules (C-MARGIN), with the limits of `[w3]` in the watcher's
//! config.
//!
//! Two violations, each a difference whose identity is its kind alone (the
//! amounts are in the detail, not in the identity):
//! - `account_mm_rate`: `maintenance > 0` and `maintenance × 10 000 ≥
//!   account_mm_rate_max_bps × margin_balance` (`totalMaintenanceMargin`,
//!   `totalMarginBalance`);
//! - `daily_loss`: the journal's day base is of today's UTC day and
//!   `(base − totalEquity) × 10 000 > daily_loss_bps × base`. Without a
//!   base of today this part is not judged.
//!
//! A violation is confirmed on two rounds in a row by the engine's
//! two-round rule; there is no age rule (the time is unknown, so old). A
//! product beyond the decimal's range counts as a violation (the safe
//! side). An answer without exactly one `UNIFIED` account, a field that is
//! not a journal decimal or a negative maintenance (the OMS refuses that
//! report too) is unreadable: the round fails.

use serde_json::Value;

use crate::config;
use crate::decimal::Dec;
use crate::engine::{Check, Diff, Logic, Page, Since, VenueRequest};
use crate::state::JournalState;

pub const PATH: &str = "/v5/account/wallet-balance";
/// A UTC day, ms (the OMS's day number is `queried_at / DAY_US`).
pub const DAY_MS: u64 = 86_400_000;
/// The subject of every W3 difference: the one unified account.
pub const SUBJECT: &str = "account";

/// The unified account of one round.
struct Wallet {
    equity: Dec,
    maintenance: Dec,
    balance: Dec,
}

pub struct W3 {
    max_bps: u64,
    loss_bps: u64,
    wallet: Option<Wallet>,
}

impl W3 {
    pub fn new(cfg: &config::W3) -> W3 {
        W3 {
            max_bps: cfg.account_mm_rate_max_bps,
            loss_bps: cfg.daily_loss_bps,
            wallet: None,
        }
    }

    fn read(result: &Value) -> Result<Wallet, String> {
        let list = result
            .get("list")
            .and_then(Value::as_array)
            .ok_or("result.list is not a list")?;
        let unified: Vec<&Value> = list
            .iter()
            .filter(|a| a.get("accountType").and_then(Value::as_str) == Some("UNIFIED"))
            .collect();
        let [account] = unified.as_slice() else {
            return Err(format!("{} UNIFIED accounts, not one", unified.len()));
        };
        let dec = |field: &str| -> Result<Dec, String> {
            let s = account
                .get(field)
                .and_then(Value::as_str)
                .ok_or_else(|| format!("{field} is not a string"))?;
            Dec::parse(s).map_err(|_| format!("{field} {s:?} is not a decimal"))
        };
        let w = Wallet {
            equity: dec("totalEquity")?,
            maintenance: dec("totalMaintenanceMargin")?,
            balance: dec("totalMarginBalance")?,
        };
        if w.maintenance.0 < 0 {
            return Err(format!(
                "totalMaintenanceMargin {} is negative",
                w.maintenance
            ));
        }
        Ok(w)
    }
}

/// `a × k ≥ b × l` in exact integers; a product out of range is `None`.
fn at_or_above(a: Dec, k: u64, b: Dec, l: u64) -> Option<bool> {
    let left = a.0.checked_mul(i128::from(k))?;
    let right = b.0.checked_mul(i128::from(l))?;
    Some(left >= right)
}

impl Logic for W3 {
    fn check(&self) -> Check {
        Check::W3
    }

    fn first(&mut self, _now_ms: u64, _state: &JournalState) -> VenueRequest {
        self.wallet = None;
        VenueRequest {
            path: PATH,
            params: vec![("accountType", "UNIFIED".to_string())],
        }
    }

    fn page(&mut self, _now_ms: u64, result: &Value) -> Page {
        match W3::read(result) {
            Ok(w) => {
                self.wallet = Some(w);
                Page::Done
            }
            Err(why) => Page::Unreadable(why),
        }
    }

    /// The design's "한도 위반 2회 연속": never at the first round.
    fn at_once(&self, _kind: &str) -> bool {
        false
    }

    fn compare(&mut self, now_ms: u64, state: &JournalState) -> Vec<Diff> {
        let Some(w) = self.wallet.take() else {
            return Vec::new();
        };
        let diff = |kind, what| Diff {
            kind,
            subject: SUBJECT.to_string(),
            value: String::new(),
            what,
            since: Since::Unknown,
        };
        let mut out = Vec::new();
        // C-MARGIN 계좌 유지증거금률: at the limit is a violation.
        if w.maintenance.0 > 0
            && at_or_above(w.maintenance, 10_000, w.balance, self.max_bps).unwrap_or(true)
        {
            out.push(diff(
                "account_mm_rate",
                format!(
                    "venue totalMaintenanceMargin {}, totalMarginBalance {}, at or above {} bps",
                    w.maintenance, w.balance, self.max_bps
                ),
            ));
        }
        // C-MARGIN 일 손실: beyond the limit, against today's base only.
        if let Some(b) = state.day_base.filter(|b| b.day == now_ms / DAY_MS) {
            let beyond = match b.value.checked_sub(w.equity) {
                Ok(loss) => !at_or_above(b.value, self.loss_bps, loss, 10_000).unwrap_or(false),
                Err(_) => true,
            };
            if beyond {
                out.push(diff(
                    "daily_loss",
                    format!(
                        "venue totalEquity {}, journal day base {} of day {}, more than {} bps below",
                        w.equity, b.value, b.day, self.loss_bps
                    ),
                ));
            }
        }
        out
    }
}
