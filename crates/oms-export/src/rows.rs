//! Journal records to rows of the three trading tables (lake contract 4.1–4.3).
//!
//! Decimal strings and the event payload come from the record's JSON as the
//! journal holds it; the typed record decides which rows a record makes.

use std::collections::HashMap;

use md_archiver::table::TableSchema;
use oms_core::{Event, OrderId, Record, Side, Tif, VenueClientId, WireIdForm};
use serde_json::{Map, Value};

use crate::tables;

/// One cell, owned.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum V {
    Null,
    Str(String),
    Bool(bool),
    Int(i64),
}

impl V {
    fn opt_str(v: Option<&str>) -> V {
        v.map_or(V::Null, |s| V::Str(s.to_string()))
    }
}

/// One row: cells in the contract's column order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    /// Journal time of the record, µs: the row's date (contract 4).
    pub at_us: u64,
    pub cells: Vec<V>,
}

/// Puts named cells in the schema's column order. Every column must be
/// named once and every name must be a column.
fn row(schema: &TableSchema, at_us: u64, named: Vec<(&str, V)>) -> Result<Row, String> {
    let mut cells: Vec<Option<V>> = vec![None; schema.columns.len()];
    for (name, v) in named {
        let i = schema
            .index(name)
            .ok_or_else(|| format!("{}: no column {name}", schema.name))?;
        if cells[i].replace(v).is_some() {
            return Err(format!("{}: column {name} given twice", schema.name));
        }
    }
    let cells = cells
        .into_iter()
        .zip(&schema.columns)
        .map(|(c, col)| c.ok_or_else(|| format!("{}: column {} not given", schema.name, col.name)))
        .collect::<Result<_, _>>()?;
    Ok(Row { at_us, cells })
}

/// The rows of one record.
#[derive(Debug, Default)]
pub struct RecordRows {
    pub orders: Vec<Row>,
    pub fills: Vec<Row>,
    pub events: Vec<Row>,
}

struct OrderInfo {
    instrument: String,
    side: Side,
}

/// Where an event names its instrument (C-TAIL field table). A test checks
/// this list against every `instrument` field of the table.
pub const INSTRUMENT_PATHS: [&str; 5] = [
    "instrument",
    "request.instrument",
    "fill.instrument",
    "funding.instrument",
    "change.instrument",
];

/// Maps records in journal order. Holds the order info that later fills
/// and events of an order need.
pub struct Mapper {
    venue: String,
    market: String,
    instance_id: u64,
    wire_id_form: Option<WireIdForm>,
    orders: HashMap<OrderId, OrderInfo>,
}

fn side_cell(s: Side) -> V {
    V::Int(match s {
        Side::Buy => 1,
        Side::Sell => -1,
    })
}

fn tif_name(t: Tif) -> &'static str {
    match t {
        Tif::Gtc => "GTC",
        Tif::Ioc => "IOC",
        Tif::Alo => "PostOnly",
    }
}

fn int(v: u64, what: &str) -> Result<i64, String> {
    i64::try_from(v).map_err(|_| format!("{what} {v} does not fit int64"))
}

/// The field object of the `i`-th event of a record's JSON.
fn raw_fields(raw: &Value, i: usize) -> Result<(&str, &Map<String, Value>), String> {
    let ev = raw
        .get("events")
        .and_then(|e| e.get(i))
        .and_then(Value::as_object)
        .ok_or_else(|| format!("event {i} is not an object"))?;
    let mut it = ev.iter();
    match (it.next(), it.next()) {
        (Some((name, Value::Object(fields))), None) => Ok((name, fields)),
        _ => Err(format!("event {i} is not one name with a field object")),
    }
}

fn str_at<'a>(fields: &'a Map<String, Value>, path: &str) -> Result<&'a str, String> {
    let mut v: Option<&Value> = None;
    for (k, part) in path.split('.').enumerate() {
        v = if k == 0 {
            fields.get(part)
        } else {
            v.and_then(|x| x.get(part))
        };
    }
    v.and_then(Value::as_str)
        .ok_or_else(|| format!("{path} is not a string"))
}

/// The venue's execution id in a fill key (C-GW G-8). The Bybit gateway's
/// key is `<symbol>:<execId>` (`docs/contracts/gateway.md`); the contract
/// column is the `execId`. Other venues' keys are the column as they are.
pub fn exec_id(venue: &str, symbol: &str, key: &str) -> Result<String, String> {
    if venue != "bybit" {
        return Ok(key.to_string());
    }
    match key.strip_prefix(symbol).and_then(|r| r.strip_prefix(':')) {
        Some(id) if !id.is_empty() => Ok(id.to_string()),
        _ => Err(format!("Bybit fill key {key:?} is not {symbol}:<execId>")),
    }
}

/// The value with the keys of every object sorted (contract 4.3).
pub fn sorted(v: &Value) -> Value {
    match v {
        Value::Object(m) => {
            let mut keys: Vec<&String> = m.keys().collect();
            keys.sort();
            let mut out = Map::new();
            for k in keys {
                out.insert(k.clone(), sorted(&m[k]));
            }
            Value::Object(out)
        }
        Value::Array(a) => Value::Array(a.iter().map(sorted).collect()),
        other => other.clone(),
    }
}

impl Mapper {
    pub fn new(
        venue: &str,
        market: &str,
        instance_id: u64,
        wire_id_form: Option<WireIdForm>,
    ) -> Mapper {
        Mapper {
            venue: venue.to_string(),
            market: market.to_string(),
            instance_id,
            wire_id_form,
            orders: HashMap::new(),
        }
    }

    /// The rows of `rec`, whose JSON as the journal holds it is `raw`.
    pub fn record(
        &mut self,
        rec: &Record,
        raw: &Value,
        record_schema: u16,
    ) -> Result<RecordRows, String> {
        let mut out = RecordRows::default();
        for (i, ev) in rec.events.iter().enumerate() {
            let (name, fields) = raw_fields(raw, i)?;
            if name != ev.name() {
                return Err(format!(
                    "record {}: event {i} is {name} in the JSON and {} typed",
                    rec.seq,
                    ev.name()
                ));
            }
            match ev {
                Event::OrderAccepted {
                    order_id, request, ..
                } => {
                    self.orders.insert(
                        *order_id,
                        OrderInfo {
                            instrument: request.instrument.to_string(),
                            side: request.side,
                        },
                    );
                    out.orders.push(self.order_row(
                        rec,
                        *order_id,
                        request.tif,
                        request.reduce_only,
                        request.side,
                        fields,
                        record_schema,
                    )?);
                }
                Event::FillBooked {
                    order_id, venue_ts, ..
                } => {
                    out.fills
                        .push(self.fill_row(rec, i, *order_id, *venue_ts, fields)?);
                }
                _ => {}
            }
            out.events
                .push(self.event_row(rec, i, ev, name, fields, record_schema)?);
        }
        Ok(out)
    }

    #[allow(clippy::too_many_arguments)]
    fn order_row(
        &self,
        rec: &Record,
        order_id: OrderId,
        tif: Tif,
        reduce_only: bool,
        side: Side,
        fields: &Map<String, Value>,
        record_schema: u16,
    ) -> Result<Row, String> {
        let wire_id = self.wire_id_form.map(|f| {
            f.encode(VenueClientId {
                instance_id: self.instance_id,
                order_id,
            })
        });
        let mref = fields
            .get("request")
            .and_then(|r| r.get("market_ref"))
            .filter(|m| !m.is_null());
        let ref_str = |k: &str| -> Result<V, String> {
            match mref {
                None => Ok(V::Null),
                Some(m) => m
                    .get(k)
                    .and_then(Value::as_str)
                    .map(|s| V::Str(s.to_string()))
                    .ok_or_else(|| format!("market_ref.{k} is not a string")),
            }
        };
        let ref_int = |k: &str, nullable: bool| -> Result<V, String> {
            match mref {
                None => Ok(V::Null),
                Some(m) => match m.get(k) {
                    Some(Value::Null) | None if nullable => Ok(V::Null),
                    Some(x) => x
                        .as_i64()
                        .map(V::Int)
                        .ok_or_else(|| format!("market_ref.{k} is not an int64")),
                    None => Err(format!("market_ref.{k} missing")),
                },
            }
        };
        row(
            tables::orders(),
            rec.at,
            vec![
                ("order_id", V::Str(order_id.0.to_string())),
                ("seq", V::Int(int(rec.seq, "seq")?)),
                ("at_us", V::Int(int(rec.at, "record time")?)),
                (
                    "strategy",
                    V::Str(str_at(fields, "request.strategy")?.to_string()),
                ),
                (
                    "client_order_id",
                    V::Str(str_at(fields, "request.client_order_id")?.to_string()),
                ),
                ("venue", V::Str(self.venue.clone())),
                ("market", V::Str(self.market.clone())),
                (
                    "symbol",
                    V::Str(str_at(fields, "request.instrument")?.to_string()),
                ),
                ("side", side_cell(side)),
                ("qty", V::Str(str_at(fields, "request.qty")?.to_string())),
                ("px", V::Str(str_at(fields, "request.px")?.to_string())),
                ("tif", V::Str(tif_name(tif).to_string())),
                ("reduce_only", V::Bool(reduce_only)),
                ("wire_id", V::opt_str(wire_id.as_deref())),
                ("market_ref_venue", ref_str("venue")?),
                ("market_ref_market", ref_str("market")?),
                ("market_ref_symbol", ref_str("symbol")?),
                ("market_ref_channel", ref_str("channel")?),
                ("market_ref_update_id", ref_int("update_id", true)?),
                ("market_ref_venue_ts_ns", ref_int("venue_ts_ns", false)?),
                ("market_ref_recv_ns", ref_int("recv_ns", false)?),
                ("record_schema", V::Int(record_schema.into())),
            ],
        )
    }

    fn fill_row(
        &self,
        rec: &Record,
        i: usize,
        order_id: OrderId,
        venue_ts: u64,
        fields: &Map<String, Value>,
    ) -> Result<Row, String> {
        let info = self.orders.get(&order_id).ok_or_else(|| {
            format!(
                "record {}: fill of order {} the journal never accepted",
                rec.seq, order_id.0
            )
        })?;
        let venue_ts_ns = venue_ts
            .checked_mul(1000)
            .ok_or_else(|| format!("venue_ts {venue_ts} µs overflows ns"))?;
        row(
            tables::fills(),
            rec.at,
            vec![
                ("venue", V::Str(self.venue.clone())),
                ("symbol", V::Str(info.instrument.clone())),
                (
                    "exec_id",
                    V::Str(exec_id(
                        &self.venue,
                        &info.instrument,
                        str_at(fields, "key")?,
                    )?),
                ),
                ("order_id", V::Str(order_id.0.to_string())),
                ("seq", V::Int(int(rec.seq, "seq")?)),
                ("event_idx", V::Int(int(i as u64, "event index")?)),
                ("at_us", V::Int(int(rec.at, "record time")?)),
                ("side", side_cell(info.side)),
                ("px", V::Str(str_at(fields, "px")?.to_string())),
                ("qty", V::Str(str_at(fields, "qty")?.to_string())),
                ("fee", V::Str(str_at(fields, "fee")?.to_string())),
                // Null in journals written before U16a (contract 4.2).
                (
                    "is_maker",
                    fields
                        .get("is_maker")
                        .and_then(Value::as_bool)
                        .map_or(V::Null, V::Bool),
                ),
                ("venue_ts_ns", V::Int(int(venue_ts_ns, "venue_ts_ns")?)),
                (
                    "recv_ns",
                    fields
                        .get("recv_ns")
                        .and_then(Value::as_i64)
                        .map_or(V::Null, V::Int),
                ),
            ],
        )
    }

    fn event_row(
        &self,
        rec: &Record,
        i: usize,
        ev: &Event,
        name: &str,
        fields: &Map<String, Value>,
        record_schema: u16,
    ) -> Result<Row, String> {
        let order_id = ev.order_id();
        let own = INSTRUMENT_PATHS.iter().find_map(|p| str_at(fields, p).ok());
        let symbol = own.map(str::to_string).or_else(|| {
            order_id
                .and_then(|o| self.orders.get(&o))
                .map(|o| o.instrument.clone())
        });
        let venue = symbol.as_ref().map(|_| self.venue.as_str());
        let payload = serde_json::to_string(&sorted(&Value::Object(fields.clone())))
            .map_err(|e| format!("payload: {e}"))?;
        row(
            tables::events(),
            rec.at,
            vec![
                ("seq", V::Int(int(rec.seq, "seq")?)),
                ("event_idx", V::Int(int(i as u64, "event index")?)),
                ("at_us", V::Int(int(rec.at, "record time")?)),
                ("kind", V::Str(name.to_string())),
                (
                    "order_id",
                    V::opt_str(order_id.map(|o| o.0.to_string()).as_deref()),
                ),
                ("venue", V::opt_str(venue)),
                ("symbol", V::opt_str(symbol.as_deref())),
                ("payload", V::Str(payload)),
                ("record_schema", V::Int(record_schema.into())),
            ],
        )
    }
}
