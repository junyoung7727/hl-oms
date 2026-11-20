//! One VictoriaLogs line per journal event (ADR-026, design §5.4.5).
//!
//! The fields are read from the event's JSON form (C-TAIL field table) by
//! name, so a new event kind gets a line with the fields it shares with the
//! others and no code change. Fields that an event does not have are left
//! out of its line.
//!
//! | field | from |
//! |---|---|
//! | `seq`, `event_index` | the record and the event's position in it |
//! | `kind` | the event name |
//! | `order_id` | `order_id`, `fill.order_id` |
//! | `wire_id` | the order's client id in the gateway's form (`node.wire_id_form` of the status) |
//! | `symbol` | `request.instrument`, `instrument`, `fill.instrument`, `change.instrument`, `funding.instrument`; for `fill_booked` the order's instrument |
//! | `side` | `request.side`, `fill.side`; for `fill_booked` the order's side |
//! | `qty`, `px` | `qty`/`request.qty`/`fill.qty`/`change.qty`/`delta`, `px`/`request.px`/`fill.px`/`change.px` (decimal strings as written) |
//! | `phase` | the order's phase after the record (confirmed lines only) |
//! | `reason` | `reason`, `reasons`, `rule`: a string, the variant name of an enum object, or a comma list |
//! | `fill_key` | `key`, `fill.key`, `change.key`, `funding.key` |
//! | `recv_ns` | `request.market_ref.recv_ns` |
//! | `venue_ts_ns` | `venue_ts`, `fill.venue_ts`, `change.venue_ts`, `funding.venue_ts` × 1000 |
//! | `unconfirmed` | `true` on a line past the durable sequence; absent otherwise |

use obs_push::LogLine;
use oms_core::{Core, Event, OrderId, Phase, Record, Side, VenueClientId, WireIdForm};
use serde_json::{Map, Value};

/// What the line builder needs from outside the event.
pub struct Ctx<'a> {
    pub host: &'a str,
    pub venue: &'a str,
    pub instance_id: u64,
    pub wire_id_form: Option<WireIdForm>,
}

/// Instrument and side of an order, looked up before its record is applied
/// (a fill does not carry them).
#[derive(Clone, Debug)]
pub struct OrderInfo {
    pub instrument: String,
    pub side: Side,
}

pub fn order_info(core: &Core, id: OrderId) -> Option<OrderInfo> {
    core.order(id).map(|o| OrderInfo {
        instrument: o.instrument.to_string(),
        side: o.side,
    })
}

fn first<'a>(obj: &'a Value, paths: &[&str]) -> Option<&'a Value> {
    paths
        .iter()
        .find_map(|p| obj.pointer(p))
        .filter(|v| !v.is_null())
}

/// A string, the variant name of a one-key object, or a comma list of those.
fn reason_text(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Object(m) if m.len() == 1 => m.keys().next().cloned(),
        Value::Array(a) => {
            let parts: Vec<String> = a.iter().filter_map(reason_text).collect();
            (!parts.is_empty()).then(|| parts.join(","))
        }
        _ => None,
    }
}

fn phase_name(p: Phase) -> Option<String> {
    serde_json::to_value(p)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
}

/// The lines of one record. `core` is the state after the record when the
/// line is confirmed, `None` for an unconfirmed line (no phase). `infos`
/// holds the order info looked up before the record, by event index.
pub fn record_lines(
    ctx: &Ctx<'_>,
    rec: &Record,
    core: Option<&Core>,
    infos: &[Option<OrderInfo>],
    unconfirmed: bool,
) -> Vec<LogLine> {
    rec.events
        .iter()
        .enumerate()
        .map(|(i, ev)| {
            event_line(
                ctx,
                rec,
                i,
                ev,
                core,
                infos.get(i).and_then(Option::as_ref),
                unconfirmed,
            )
        })
        .collect()
}

fn event_line(
    ctx: &Ctx<'_>,
    rec: &Record,
    index: usize,
    ev: &Event,
    core: Option<&Core>,
    info: Option<&OrderInfo>,
    unconfirmed: bool,
) -> LogLine {
    let json = serde_json::to_value(ev).unwrap_or(Value::Null);
    let (kind, body) = match &json {
        Value::Object(m) if m.len() == 1 => m
            .iter()
            .next()
            .map(|(k, v)| (k.clone(), v.clone()))
            .unwrap_or_default(),
        Value::String(s) => (s.clone(), Value::Null),
        _ => ("unknown".to_string(), Value::Null),
    };
    let mut f = Map::new();
    f.insert("seq".into(), rec.seq.into());
    f.insert("event_index".into(), (index as u64).into());
    f.insert("kind".into(), kind.clone().into());
    let order_id = first(&body, &["/order_id", "/fill/order_id"]).and_then(Value::as_u64);
    if let Some(id) = order_id {
        f.insert("order_id".into(), id.into());
        if let Some(form) = ctx.wire_id_form {
            let wire = form.encode(VenueClientId {
                instance_id: ctx.instance_id,
                order_id: OrderId(id),
            });
            f.insert("wire_id".into(), wire.into());
        }
        if let Some(phase) = core
            .and_then(|c| c.order(OrderId(id)))
            .and_then(|o| phase_name(o.phase))
        {
            f.insert("phase".into(), phase.into());
        }
    }
    let symbol = first(
        &body,
        &[
            "/request/instrument",
            "/instrument",
            "/fill/instrument",
            "/change/instrument",
            "/funding/instrument",
        ],
    )
    .and_then(Value::as_str)
    .map(str::to_string)
    .or_else(|| info.map(|i| i.instrument.clone()));
    if let Some(s) = &symbol {
        f.insert("symbol".into(), s.clone().into());
    }
    let side = first(&body, &["/request/side", "/fill/side"])
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            info.and_then(|i| {
                serde_json::to_value(i.side)
                    .ok()?
                    .as_str()
                    .map(str::to_string)
            })
        });
    if let Some(s) = &side {
        f.insert("side".into(), s.clone().into());
    }
    let qty = first(
        &body,
        &["/qty", "/request/qty", "/fill/qty", "/change/qty", "/delta"],
    )
    .cloned();
    if let Some(q) = &qty {
        f.insert("qty".into(), q.clone());
    }
    let px = first(&body, &["/px", "/request/px", "/fill/px", "/change/px"]).cloned();
    if let Some(p) = &px {
        f.insert("px".into(), p.clone());
    }
    let reason = first(&body, &["/reason", "/reasons", "/rule"]).and_then(reason_text);
    if let Some(r) = &reason {
        f.insert("reason".into(), r.clone().into());
    }
    if let Some(k) = first(&body, &["/key", "/fill/key", "/change/key", "/funding/key"]) {
        f.insert("fill_key".into(), k.clone());
    }
    if let Some(r) = first(&body, &["/request/market_ref/recv_ns"]) {
        f.insert("recv_ns".into(), r.clone());
    }
    let venue_ts = first(
        &body,
        &[
            "/venue_ts",
            "/fill/venue_ts",
            "/change/venue_ts",
            "/funding/venue_ts",
        ],
    )
    .and_then(Value::as_u64);
    if let Some(us) = venue_ts {
        f.insert("venue_ts_ns".into(), us.saturating_mul(1000).into());
    }
    if unconfirmed {
        f.insert("unconfirmed".into(), true.into());
    }
    let mut msg = format!("{kind} seq={}", rec.seq);
    if let Some(id) = order_id {
        msg.push_str(&format!(" order={id}"));
    }
    for v in [&symbol, &side].into_iter().flatten() {
        msg.push(' ');
        msg.push_str(v);
    }
    for (name, v) in [("qty", &qty), ("px", &px)] {
        if let Some(Value::String(s)) = v {
            msg.push_str(&format!(" {name}={s}"));
        }
    }
    if let Some(r) = &reason {
        msg.push_str(&format!(" reason={r}"));
    }
    if unconfirmed {
        msg.push_str(" (unconfirmed)");
    }
    LogLine {
        time_ns: i64::try_from(rec.at.saturating_mul(1000)).unwrap_or(i64::MAX),
        msg,
        service: "oms".into(),
        host: ctx.host.to_string(),
        venue: ctx.venue.to_string(),
        fields: f,
    }
}
