//! The watcher's journal state (C-WATCH part 2), built event by event from
//! the records of its own reader. Every rule is a row of the C-TAIL table
//! "이벤트가 바꾸는 상태" (`docs/contracts/tail.md`); the fields are those of
//! the C-TAIL field table. The watcher uses no OMS code. The rows for the
//! ack, doubt and terminal marks, the venue-change and funding keys, the
//! fill floor, the holds and the day base were added to C-TAIL in U19a2
//! review 1 (M1); the conformance test checks them against the core.
//!
//! What it keeps: the order info from `order_accepted` until `purged` (not
//! removed at `order_closed`: a fill may come after the close), the open
//! set, the fill, venue-change and funding keys until the fill floor passes
//! them, the position per instrument, the change times W1 and W2 judge by,
//! the holds, the day base and the fill floor.
//!
//! A record that breaks a rule (a fill of an order the journal never
//! accepted, a purge of an open order, a field that is missing or not of
//! its documented form, a record out of order) is `JournalError::State`.
//! It latches: the state may hold part of that record, so every later
//! `apply` returns the same error until the process restarts.

use crate::decimal::Dec;
use crate::journal::{JournalError, Record, Tail};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Buy,
    Sell,
}

/// One order of the journal, from `order_accepted` until `purged`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OrderInfo {
    pub strategy: String,
    pub instrument: String,
    pub side: Side,
    pub qty: Dec,
    pub client_order_id: String,
    pub venue_oid: Option<String>,
    /// Σ `fill_booked.qty` of the order, also after its close.
    pub filled: Dec,
    /// No `order_closed` yet.
    pub open: bool,
    pub acked: bool,
    pub in_doubt: bool,
    /// A `venue_terminal` was recorded: the venue ended the order. The OMS
    /// keeps it open until `order_closed` (C-TERM).
    pub terminal: bool,
    /// Record time (µs) of the latest `order_accepted`, `order_acked`,
    /// `order_in_doubt`, `venue_terminal` or `order_closed` of the order:
    /// the events that change what W1 compares. Other events of the order
    /// do not move it, so an order that keeps receiving events cannot keep
    /// a W1 difference young.
    pub fact_at: u64,
}

/// A booked fill (`fill_booked`, `external_fill_booked`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fill {
    pub qty: Dec,
    pub side: Side,
    pub instrument: String,
    /// µs.
    pub venue_ts: u64,
}

/// A change the venue made to a position on its own (`venue_position_changed`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VenueChange {
    pub instrument: String,
    /// Signed: positive adds to a long.
    pub qty: Dec,
    /// `adl`, `takeover`, `delivery` or `settle`.
    pub kind: String,
    pub venue_ts: u64,
}

/// A booked funding payment (`funding_booked`): cash only, no quantity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Funding {
    pub instrument: String,
    pub venue_ts: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Holds {
    pub watcher_mismatch: bool,
    pub kill: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DayBase {
    /// UTC day number.
    pub day: u64,
    pub value: Dec,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct JournalState {
    /// From the segment headers (`read_from`); `None` before the first read.
    pub instance_id: Option<u64>,
    /// The last record applied; 0 before the first.
    pub last_seq: u64,
    /// Its record time, µs.
    pub last_at: u64,
    pub orders: BTreeMap<u64, OrderInfo>,
    /// Accepted and not closed.
    pub open: BTreeSet<u64>,
    /// Fill key → fill, until the fill floor passes its venue time.
    pub fills: BTreeMap<String, Fill>,
    pub venue_changes: BTreeMap<String, VenueChange>,
    pub funding: BTreeMap<String, Funding>,
    /// Instrument → Σ booked fills (buy +, sell −) + venue changes +
    /// adjustments. An instrument the journal never changed is absent.
    pub positions: BTreeMap<String, Dec>,
    /// Instrument → record time of its latest `fill_booked`,
    /// `external_fill_booked`, `venue_position_changed` or
    /// `position_adjusted`. Absent: the journal never changed it (W2 takes
    /// that as old).
    pub last_change_at: BTreeMap<String, u64>,
    pub holds: Holds,
    pub day_base: Option<DayBase>,
    /// Venue time (µs) below which the OMS has forgotten its fill keys; it
    /// only ever goes up (`booted`, `purged`).
    pub fill_floor: u64,
    latched: Option<JournalError>,
}

impl JournalState {
    /// The journal position of an instrument; 0 when it never changed.
    pub fn position(&self, instrument: &str) -> Dec {
        self.positions.get(instrument).copied().unwrap_or(Dec::ZERO)
    }

    /// The error that stopped this state until a restart, if any.
    pub fn latched(&self) -> Option<&JournalError> {
        self.latched.as_ref()
    }

    /// Reads the next records from `tail` up to `cap` (at most
    /// `max_records`) and applies them. Returns the number applied.
    pub fn read_from(
        &mut self,
        tail: &mut Tail,
        cap: u64,
        max_records: usize,
    ) -> Result<usize, JournalError> {
        if let Some(e) = &self.latched {
            return Err(e.clone());
        }
        let recs = tail.read(cap, max_records)?;
        self.instance_id = tail.instance_id().or(self.instance_id);
        for r in &recs {
            self.apply(r)?;
        }
        Ok(recs.len())
    }

    /// Applies one record: the record after the last one applied.
    pub fn apply(&mut self, rec: &Record) -> Result<(), JournalError> {
        if let Some(e) = &self.latched {
            return Err(e.clone());
        }
        match self.apply_events(rec) {
            Ok(()) => {
                self.last_seq = rec.seq;
                self.last_at = rec.at;
                Ok(())
            }
            Err(why) => {
                let e = JournalError::State { seq: rec.seq, why };
                self.latched = Some(e.clone());
                Err(e)
            }
        }
    }

    fn apply_events(&mut self, rec: &Record) -> Result<(), String> {
        if rec.seq != self.last_seq + 1 {
            return Err(format!("record {} after record {}", rec.seq, self.last_seq));
        }
        for (name, f) in &rec.events {
            self.apply_event(rec.at, name, f)
                .map_err(|why| format!("{name}: {why}"))?;
        }
        Ok(())
    }

    fn apply_event(&mut self, at: u64, name: &str, f: &Value) -> Result<(), String> {
        match name {
            "order_accepted" => {
                let id = u64_at(f, "order_id")?;
                if self.orders.contains_key(&id) {
                    return Err(format!("order {id} accepted twice"));
                }
                let info = OrderInfo {
                    strategy: str_at(f, "request.strategy")?.to_string(),
                    instrument: str_at(f, "request.instrument")?.to_string(),
                    side: side_at(f, "request.side")?,
                    qty: dec_at(f, "request.qty")?,
                    client_order_id: str_at(f, "request.client_order_id")?.to_string(),
                    venue_oid: None,
                    filled: Dec::ZERO,
                    open: true,
                    acked: false,
                    in_doubt: false,
                    terminal: false,
                    fact_at: at,
                };
                self.orders.insert(id, info);
                self.open.insert(id);
            }
            "order_acked" => {
                let venue_oid = match field(f, "venue_oid")? {
                    Value::Null => None,
                    Value::String(s) => Some(s.clone()),
                    _ => return Err("field venue_oid is not a string or null".into()),
                };
                let o = self.order_mut(f)?;
                o.acked = true;
                o.in_doubt = false;
                if venue_oid.is_some() {
                    o.venue_oid = venue_oid;
                }
                o.fact_at = at;
            }
            "venue_oid_learned" => {
                let v = str_at(f, "venue_oid")?.to_string();
                self.order_mut(f)?.venue_oid = Some(v);
            }
            "order_in_doubt" => {
                let o = self.order_mut(f)?;
                o.in_doubt = true;
                o.fact_at = at;
            }
            "venue_terminal" => {
                let o = self.order_mut(f)?;
                o.terminal = true;
                o.in_doubt = false;
                o.fact_at = at;
            }
            "order_closed" => {
                let o = self.order_mut(f)?;
                o.open = false;
                o.fact_at = at;
                let id = u64_at(f, "order_id")?;
                self.open.remove(&id);
            }
            "fill_booked" => {
                let key = str_at(f, "key")?.to_string();
                let qty = dec_at(f, "qty")?;
                let venue_ts = u64_at(f, "venue_ts")?;
                let o = self.order_mut(f)?;
                o.filled = o.filled.checked_add(qty).map_err(|e| e.to_string())?;
                let (side, instrument) = (o.side, o.instrument.clone());
                self.book(at, &instrument, signed(side, qty)?)?;
                self.fills.insert(
                    key,
                    Fill {
                        qty,
                        side,
                        instrument,
                        venue_ts,
                    },
                );
            }
            "external_fill_booked" => {
                let key = str_at(f, "fill.key")?.to_string();
                let fill = Fill {
                    qty: dec_at(f, "fill.qty")?,
                    side: side_at(f, "fill.side")?,
                    instrument: str_at(f, "fill.instrument")?.to_string(),
                    venue_ts: u64_at(f, "fill.venue_ts")?,
                };
                self.book(at, &fill.instrument, signed(fill.side, fill.qty)?)?;
                self.fills.insert(key, fill);
            }
            "venue_position_changed" => {
                let key = str_at(f, "change.key")?.to_string();
                let kind = str_at(f, "change.kind")?;
                if !matches!(kind, "adl" | "takeover" | "delivery" | "settle") {
                    return Err(format!("unknown venue change kind {kind:?}"));
                }
                let change = VenueChange {
                    instrument: str_at(f, "change.instrument")?.to_string(),
                    qty: dec_at(f, "change.qty")?,
                    kind: kind.to_string(),
                    venue_ts: u64_at(f, "change.venue_ts")?,
                };
                self.book(at, &change.instrument, change.qty)?;
                self.venue_changes.insert(key, change);
            }
            "position_adjusted" => {
                let instrument = str_at(f, "instrument")?.to_string();
                let delta = dec_at(f, "delta")?;
                self.book(at, &instrument, delta)?;
            }
            "funding_booked" => {
                let key = str_at(f, "funding.key")?.to_string();
                let funding = Funding {
                    instrument: str_at(f, "funding.instrument")?.to_string(),
                    venue_ts: u64_at(f, "funding.venue_ts")?,
                };
                self.funding.insert(key, funding);
            }
            "purged" => {
                let ids = field(f, "order_ids")?
                    .as_array()
                    .ok_or("field order_ids is not a list")?;
                for id in ids {
                    let id = id.as_u64().ok_or("an order id is not a u64")?;
                    match self.orders.get(&id) {
                        None => return Err(format!("purge of unknown order {id}")),
                        Some(o) if o.open => return Err(format!("purge of open order {id}")),
                        Some(_) => {
                            self.orders.remove(&id);
                        }
                    }
                }
                self.raise_floor(u64_at(f, "fill_floor")?);
                let floor = self.fill_floor;
                self.fills.retain(|_, x| x.venue_ts >= floor);
                self.venue_changes.retain(|_, x| x.venue_ts >= floor);
                self.funding.retain(|_, x| x.venue_ts >= floor);
            }
            "booted" => self.raise_floor(u64_at(f, "fill_floor")?),
            "hold_set" | "hold_cleared" => {
                let on = name == "hold_set";
                match (str_at(f, "hold")?, on) {
                    ("watcher_mismatch", _) => self.holds.watcher_mismatch = on,
                    ("kill", _) => self.holds.kill = on,
                    // Cleared by the operator; derived in the OMS from the
                    // events that record the change, never set by hold_set.
                    ("venue_change", false) => {}
                    (h, _) => return Err(format!("unknown hold {h:?}")),
                }
            }
            "day_base_set" => {
                self.day_base = Some(DayBase {
                    day: u64_at(f, "day")?,
                    value: dec_at(f, "value")?,
                });
            }
            _ => {}
        }
        Ok(())
    }

    fn order_mut(&mut self, f: &Value) -> Result<&mut OrderInfo, String> {
        let id = u64_at(f, "order_id")?;
        self.orders
            .get_mut(&id)
            .ok_or_else(|| format!("order {id} is not in the journal"))
    }

    /// Adds a signed quantity to the position and moves its change time.
    fn book(&mut self, at: u64, instrument: &str, qty: Dec) -> Result<(), String> {
        let p = self.positions.entry(instrument.to_string()).or_default();
        *p = p.checked_add(qty).map_err(|e| e.to_string())?;
        self.last_change_at.insert(instrument.to_string(), at);
        Ok(())
    }

    fn raise_floor(&mut self, floor: u64) {
        self.fill_floor = self.fill_floor.max(floor);
    }
}

fn signed(side: Side, qty: Dec) -> Result<Dec, String> {
    match side {
        Side::Buy => Ok(qty),
        Side::Sell => qty.checked_neg().map_err(|e| e.to_string()),
    }
}

/// The value at a dotted path of the event's fields.
fn field<'a>(f: &'a Value, path: &str) -> Result<&'a Value, String> {
    path.split('.')
        .try_fold(f, |v, k| v.get(k))
        .ok_or_else(|| format!("field {path} is missing"))
}

fn str_at<'a>(f: &'a Value, path: &str) -> Result<&'a str, String> {
    field(f, path)?
        .as_str()
        .ok_or_else(|| format!("field {path} is not a string"))
}

fn u64_at(f: &Value, path: &str) -> Result<u64, String> {
    field(f, path)?
        .as_u64()
        .ok_or_else(|| format!("field {path} is not a u64"))
}

fn dec_at(f: &Value, path: &str) -> Result<Dec, String> {
    Dec::parse(str_at(f, path)?).map_err(|e| format!("field {path}: {e}"))
}

fn side_at(f: &Value, path: &str) -> Result<Side, String> {
    match str_at(f, path)? {
        "buy" => Ok(Side::Buy),
        "sell" => Ok(Side::Sell),
        s => Err(format!("field {path} is not buy or sell: {s:?}")),
    }
}
