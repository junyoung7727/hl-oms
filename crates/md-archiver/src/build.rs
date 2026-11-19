//! The build of one date (spec D3–D6, R1–R11): replay of the date before
//! (carry-in, no rows) and of the date, per topic, into `.tmp` files next to
//! their final L1 paths. Committing the files is [`crate::commit`].

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io;
use std::path::{Path, PathBuf};

use md_core::adapter::{Instrument, MarketDataAdapter, Route};
use md_core::{BookUpdate, Normalized, OrderBook, SeqRule, Side, StreamKey};
use md_lake::fs::Fs;
use md_lake::qraw::{Record, Src};
use md_lake::time::{DAY_NS, HOUR_NS, date, date_start};
use serde_json::{Value, json};

use crate::input::{
    ConnInfo, ConnKey, DateState, FileKind, InputError, InstrumentsInEffect, L0File, NoteRec,
    Pass1, RecRef, Refusal, Response, Topic, date_state, for_each_record,
};
use crate::parquet_out::{FileWriter, WriteError, WriteSettings};
use crate::registry::{ChannelKind, Venue};
use crate::rows::{
    BookRow, Common, GapRow, InstrumentRow, L1Row, Lineage, TickerRow, TradeRow, cells,
};
use crate::table::Table;

/// Dates of instruments responses a record may use: its file's date and
/// the seven before (spec D3).
pub const INSTRUMENT_LOOKBACK_DAYS: i64 = 7;

/// Rows written between two disk-floor checks (judgement: about every
/// 30 MB of L1).
pub const DISK_CHECK_ROWS: u64 = 100_000;

/// Called before each new file: `Err` stops the build (spec R7).
#[derive(Clone)]
pub struct DiskCheck(pub std::sync::Arc<dyn Fn() -> Result<(), String> + Send + Sync>);

impl std::fmt::Debug for DiskCheck {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DiskCheck")
    }
}

#[derive(Debug, Clone)]
pub struct Settings {
    /// Channels written as one `day.parquet` per date (lake contract 3.1).
    pub small_channels: BTreeSet<String>,
    pub write: WriteSettings,
    /// Topics replayed at once; their rows are written in symbol order.
    pub threads: usize,
    pub disk_check: Option<DiskCheck>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            small_channels: ["publicTrade", "tickers"].map(String::from).into(),
            write: WriteSettings::default(),
            threads: 4,
            disk_check: None,
        }
    }
}

#[derive(Debug)]
pub enum BuildError {
    Io(io::Error),
    Refuse(Refusal),
    /// The date (or the date before) is not day-sealed yet.
    Wait(String),
    Write(WriteError),
    /// A defect of this crate (two gap rows with one key, a row the schema
    /// does not take). Nothing is committed.
    Defect(String),
    /// Free disk space fell below the floor (spec R7).
    Disk(String),
}

impl std::fmt::Display for BuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BuildError::Io(e) => write!(f, "{e}"),
            BuildError::Refuse(r) => write!(f, "refused: {}", r.0),
            BuildError::Wait(w) => write!(f, "waiting: {w}"),
            BuildError::Write(e) => write!(f, "{e}"),
            BuildError::Defect(e) => write!(f, "defect: {e}"),
            BuildError::Disk(e) => write!(f, "disk: {e}"),
        }
    }
}

impl std::error::Error for BuildError {}

impl From<io::Error> for BuildError {
    fn from(e: io::Error) -> Self {
        BuildError::Io(e)
    }
}

impl From<WriteError> for BuildError {
    fn from(e: WriteError) -> Self {
        BuildError::Write(e)
    }
}

impl From<InputError> for BuildError {
    fn from(e: InputError) -> Self {
        match e {
            InputError::Io(e) => BuildError::Io(e),
            InputError::Refuse(r) => BuildError::Refuse(r),
        }
    }
}

/// One file the build wrote under its `.tmp` name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuiltFile {
    pub path: String,
    pub table: Table,
    pub rows: u64,
}

/// What a build wrote: the files in path order, the gap file last.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Built {
    pub date: String,
    pub files: Vec<BuiltFile>,
    pub gap_rows_by_kind: BTreeMap<String, u64>,
    /// Trade rows dropped with no handover row of their connection pair.
    pub unattributed_trade_drops: u64,
    /// The L0 bytes of the date (for the disk check of spec R7).
    pub l0_bytes: u64,
}

pub fn tmp_path(path: &str) -> String {
    format!("{path}.tmp")
}

fn prev_date(d: &str) -> Option<String> {
    date_start(d).map(|s| date(s - DAY_NS))
}

/// The files of `date` and of the date before, or why they cannot be built.
pub fn inputs(fs: &dyn Fs, venue: &str, d: &str) -> Result<(Vec<L0File>, Vec<L0File>), BuildError> {
    let cur = match date_state(fs, venue, d)? {
        DateState::Sealed(f) => f,
        DateState::NoL0 => return Err(BuildError::Wait(format!("{d} has no L0"))),
        DateState::NotSealed(why) => return Err(BuildError::Wait(format!("{d}: {why}"))),
    };
    let p = prev_date(d).ok_or_else(|| BuildError::Defect(format!("date {d}")))?;
    let prev = match date_state(fs, venue, &p)? {
        DateState::Sealed(f) => f,
        DateState::NoL0 => Vec::new(),
        DateState::NotSealed(why) => {
            return Err(BuildError::Wait(format!(
                "the date before, {p}, is not day-sealed ({why})"
            )));
        }
    };
    Ok((prev, cur))
}

/// The instruments responses of `market` over the dates before `d` and `d`.
fn responses(
    fs: &dyn Fs,
    venue: &dyn Venue,
    d: &str,
    market: &str,
) -> Result<Vec<(String, Response)>, BuildError> {
    let mut out = Vec::new();
    let start = date_start(d).ok_or_else(|| BuildError::Defect(format!("date {d}")))?;
    for back in (0..=INSTRUMENT_LOOKBACK_DAYS + 1).rev() {
        let day = date(start - back * DAY_NS);
        let files = match date_state(fs, venue.name(), &day)? {
            DateState::Sealed(f) => f,
            _ => continue,
        };
        for f in files
            .iter()
            .filter(|f| matches!(&f.kind, FileKind::Instruments { market: m } if m == market))
        {
            for_each_record(fs, f, |rec_no, _, r| {
                if r.src != Src::Rest {
                    return;
                }
                // A body that does not parse is a parse_error row of its own
                // date's build; it gives no instrument row.
                if let Ok(rows) = venue.parse_instruments(market, &r.payload, r.recv_ns) {
                    out.push((
                        day.clone(),
                        Response {
                            at: RecRef {
                                l0_file_id: f.l0_file_id.clone(),
                                rec_no,
                                recv_ns: r.recv_ns,
                            },
                            rows,
                        },
                    ));
                }
            })?;
        }
    }
    Ok(out)
}

/// The instruments in effect for records of files dated `file_date`.
fn window(all: &[(String, Response)], file_date: &str) -> InstrumentsInEffect {
    let end = date_start(file_date).unwrap_or(0);
    let first = date(end - INSTRUMENT_LOOKBACK_DAYS * DAY_NS);
    InstrumentsInEffect::new(
        all.iter()
            .filter(|(d, _)| d.as_str() >= first.as_str() && d.as_str() <= file_date)
            .map(|(_, r)| r.clone())
            .collect(),
    )
}

fn compact(v: Value) -> String {
    // serde_json's Map is a BTreeMap here (no preserve_order feature), so
    // keys are sorted.
    v.to_string()
}

fn lin(file: &L0File, rec_no: u64, r: &Record) -> Lineage {
    Lineage {
        conn_id: i64::from(r.conn_id),
        frame_no: i64::try_from(r.frame_no).unwrap_or(i64::MAX),
        l0_file_id: file.l0_file_id.clone(),
        rec_no: i64::try_from(rec_no).unwrap_or(i64::MAX),
    }
}

/// The context of one date's build.
struct Ctx<'a> {
    venue: &'a dyn Venue,
    date: String,
    day_start: i64,
    pass1: Pass1,
    /// Connections that carry each topic.
    carriers: BTreeMap<Topic, Vec<ConnKey>>,
    /// Instruments in effect per `(market, file date)`.
    instruments: BTreeMap<(String, String), InstrumentsInEffect>,
}

impl Ctx<'_> {
    fn conn(&self, c: &ConnKey) -> &ConnInfo {
        &self.pass1.conns[c]
    }

    /// Whether another connection that carries `topic` is alive at `t`.
    fn alive_other(&self, topic: &Topic, not: &ConnKey, t: i64) -> Vec<&ConnKey> {
        self.carriers
            .get(topic)
            .map(|v| {
                v.iter()
                    .filter(|c| *c != not)
                    .filter(|c| {
                        let i = self.conn(c);
                        i.first_ns <= t && t <= i.last_ns
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// When the book of `topic` on followed connection `f` ends because no
    /// connection can carry it on (spec R2, final review F1): `f`'s last
    /// record, moved on only through a pending handover, that is a
    /// connection alive at that time that has sent nothing of the topic yet
    /// (its first record of the topic is later). A connection that already
    /// sends the topic is not applied to this book, so following it would
    /// freeze the book while rows go missing.
    fn chain_end(&self, topic: &Topic, f: &ConnKey) -> i64 {
        let mut seen: BTreeSet<&ConnKey> = BTreeSet::from([f]);
        let mut e = self.conn(f).last_ns;
        loop {
            let pending: Vec<&ConnKey> = self
                .alive_other(topic, f, e)
                .into_iter()
                .filter(|c| !seen.contains(*c))
                .filter(|c| {
                    self.conn(c)
                        .topics
                        .get(topic)
                        .is_some_and(|r| r.recv_ns > e)
                })
                .collect();
            let next = pending
                .iter()
                .map(|c| self.conn(c).last_ns)
                .fold(e, i64::max);
            seen.extend(pending.iter().copied());
            if pending.is_empty() || next == e {
                return e;
            }
            e = next;
        }
    }

    fn in_date(&self, file: &L0File) -> bool {
        file.date == self.date
    }

    fn hour_of(&self, recv_ns: i64) -> usize {
        (recv_ns - self.day_start).div_euclid(HOUR_NS).clamp(0, 23) as usize
    }
}

/// Times at which notes end the book state of each topic (spec D4).
fn note_ends(ctx: &Ctx, topics: &BTreeSet<Topic>) -> BTreeMap<Topic, Vec<i64>> {
    let mut out: BTreeMap<Topic, Vec<i64>> = BTreeMap::new();
    for n in &ctx.pass1.notes {
        let kind = n.body["note"].as_str().unwrap_or("");
        if !matches!(kind, "collector_down" | "disk_floor" | "stale") {
            continue;
        }
        let Some(start) = n.body["start_recv_ns"].as_i64() else {
            continue;
        };
        for t in covered(ctx, n, topics) {
            out.entry(t).or_default().push(start);
        }
    }
    for v in out.values_mut() {
        v.sort_unstable();
    }
    out
}

/// The topics a note is about (spec D5).
fn covered(ctx: &Ctx, n: &NoteRec, topics: &BTreeSet<Topic>) -> Vec<Topic> {
    if let Some(list) = n.body["topics"].as_array() {
        return list
            .iter()
            .filter_map(|t| {
                Some(Topic {
                    market: t["market"].as_str()?.to_string(),
                    channel: t["channel"].as_str()?.to_string(),
                    symbol: t["symbol"].as_str()?.to_string(),
                })
            })
            .collect();
    }
    match n.body["conn_id"]
        .as_u64()
        .and_then(|c| u32::try_from(c).ok())
    {
        Some(conn_id) => {
            let key = ConnKey {
                collector: n.writer.clone(),
                session_ns: n.writer_start_ns,
                conn_id,
            };
            ctx.pass1
                .conns
                .get(&key)
                .map(|i| i.topics.keys().cloned().collect())
                .unwrap_or_default()
        }
        None => topics.iter().cloned().collect(),
    }
}

/// The connection an open file of a `crash_truncate` note belongs to.
fn crashed_conn(file: &str) -> Option<ConnKey> {
    match md_lake::paths::L0Path::parse(file).ok()? {
        md_lake::paths::L0Path::Open {
            collector,
            session_ns,
            conn_id,
            ..
        } => Some(ConnKey {
            collector,
            session_ns,
            conn_id,
        }),
        _ => None,
    }
}

/// Gap rows of notes and of reconnects (spec D5, R1).
fn note_and_reconnect_rows(ctx: &Ctx, topics: &BTreeSet<Topic>, files: &[L0File]) -> Vec<GapRow> {
    let mut rows = Vec::new();
    let in_d: BTreeSet<&str> = files
        .iter()
        .filter(|f| ctx.in_date(f))
        .map(|f| f.l0_file_id.as_str())
        .collect();
    let mut downs: Vec<(Topic, i64, i64)> = Vec::new();
    for n in &ctx.pass1.notes {
        let kind = n.body["note"].as_str().unwrap_or("");
        let start = n.body["start_recv_ns"].as_i64().unwrap_or(n.at.recv_ns);
        let end = n.body["end_recv_ns"].as_i64();
        let cov = if kind == "crash_truncate" {
            match n.body["detail"]["file"].as_str().and_then(crashed_conn) {
                Some(c) => ctx
                    .pass1
                    .conns
                    .get(&c)
                    .map(|i| i.topics.keys().cloned().collect())
                    .unwrap_or_default(),
                // About a collector's session file: no row (contract 3.5).
                None => Vec::new(),
            }
        } else {
            covered(ctx, n, topics)
        };
        if kind == "collector_down" {
            for t in &cov {
                downs.push((t.clone(), start, end.unwrap_or(i64::MAX)));
            }
        }
        let kind: &'static str = match kind {
            "clock_step" => "clock_step",
            "crash_truncate" => "crash_truncate",
            "collector_down" => "collector_down",
            "overflow" => "overflow",
            "disk_floor" => "disk_floor",
            "stale" => "stale",
            _ => continue,
        };
        if !in_d.contains(n.at.l0_file_id.as_str()) {
            continue;
        }
        let conn_id = if kind == "crash_truncate" {
            n.body["detail"]["file"]
                .as_str()
                .and_then(crashed_conn)
                .map(|c| i64::from(c.conn_id))
        } else {
            n.body["conn_id"].as_i64()
        };
        let detail = match &n.body["detail"] {
            Value::Object(m) => compact(Value::Object(m.clone())),
            _ => "{}".into(),
        };
        for t in cov {
            rows.push(GapRow {
                market: t.market,
                symbol: t.symbol,
                channel: t.channel,
                kind,
                conn_id,
                start_recv_ns: start,
                end_recv_ns: end,
                last_good_update_id: None,
                next_update_id: None,
                detail: detail.clone(),
                l0_file_id: Some(n.at.l0_file_id.clone()),
                rec_no: Some(n.at.rec_no as i64),
            });
        }
    }
    // Reconnects (R1).
    for (topic, carriers) in &ctx.carriers {
        for a in carriers {
            let ai = ctx.conn(a);
            if !ctx.alive_other(topic, a, ai.last_ns).is_empty() {
                continue;
            }
            let Some(b) = carriers
                .iter()
                .filter(|c| ctx.conn(c).first_ns > ai.last_ns)
                .min_by_key(|c| (ctx.conn(c).first_ns, (*c).clone()))
            else {
                continue;
            };
            let ev = &ctx.conn(b).topics[topic];
            if !in_d.contains(ev.l0_file_id.as_str()) {
                continue;
            }
            let other_session =
                (a.collector.as_str(), a.session_ns) != (b.collector.as_str(), b.session_ns);
            if other_session
                && downs
                    .iter()
                    .any(|(t, s, e)| t == topic && *s <= ev.recv_ns && ai.last_ns <= *e)
            {
                continue;
            }
            rows.push(GapRow {
                market: topic.market.clone(),
                symbol: topic.symbol.clone(),
                channel: topic.channel.clone(),
                kind: "reconnect",
                conn_id: Some(i64::from(b.conn_id)),
                start_recv_ns: ai.last_ns,
                end_recv_ns: Some(ev.recv_ns),
                last_good_update_id: None,
                next_update_id: None,
                detail: compact(json!({
                    "from_conn_id": a.conn_id,
                    "from_session_ns": a.session_ns,
                    "to_conn_id": b.conn_id,
                    "to_session_ns": b.session_ns,
                })),
                l0_file_id: Some(ev.l0_file_id.clone()),
                rec_no: Some(ev.rec_no as i64),
            });
        }
    }
    rows
}

fn gap(topic: &Topic, kind: &'static str, lineage: &Lineage) -> GapRow {
    GapRow {
        market: topic.market.clone(),
        symbol: topic.symbol.clone(),
        channel: topic.channel.clone(),
        kind,
        conn_id: Some(lineage.conn_id),
        start_recv_ns: 0,
        end_recv_ns: None,
        last_good_update_id: None,
        next_update_id: None,
        detail: "{}".into(),
        l0_file_id: Some(lineage.l0_file_id.clone()),
        rec_no: Some(lineage.rec_no),
    }
}

fn parse_error(topic: &Topic, lineage: &Lineage, recv_ns: i64, why: &str) -> GapRow {
    GapRow {
        start_recv_ns: recv_ns,
        end_recv_ns: Some(recv_ns),
        detail: compact(json!({ "error": why })),
        ..gap(topic, "parse_error", lineage)
    }
}

/// The adapter for a symbol's instrument row, rebuilt when the row changes.
struct AdapterCache {
    market: String,
    inst: Option<Instrument>,
    adapter: Option<Box<dyn MarketDataAdapter + Send>>,
}

impl AdapterCache {
    fn new(market: &str) -> Self {
        Self {
            market: market.to_string(),
            inst: None,
            adapter: None,
        }
    }

    fn get(
        &mut self,
        venue: &dyn Venue,
        inst: &Instrument,
    ) -> Option<&(dyn MarketDataAdapter + Send)> {
        if self.inst.as_ref() != Some(inst) {
            self.adapter = venue.adapter(&self.market, std::slice::from_ref(inst));
            self.inst = Some(inst.clone());
        }
        self.adapter.as_deref()
    }
}

/// A parsed record of a topic file, or why it is a `parse_error`.
fn parse_record(
    ctx: &Ctx,
    cache: &mut AdapterCache,
    topic: &Topic,
    file: &L0File,
    r: &Record,
) -> Result<(Vec<Normalized>, Instrument), String> {
    if r.src != Src::WebSocket {
        return Err(format!("a src {} record in a topic file", r.src.byte()));
    }
    let inst = ctx
        .instruments
        .get(&(topic.market.clone(), file.date.clone()))
        .and_then(|w| w.row(&topic.symbol, r.recv_ns))
        .ok_or_else(|| format!("no instrument row for {}", topic.symbol))?
        .clone();
    let a = cache
        .get(ctx.venue, &inst)
        .ok_or_else(|| format!("no adapter for market {}", topic.market))?;
    let key = StreamKey::new(topic.channel.clone(), topic.symbol.clone());
    match a.route(&r.payload) {
        Route::Stream(k) if k == key => {}
        other => return Err(format!("the record routes to {other:?}")),
    }
    a.parse(&key, &r.payload, r.recv_ns)
        .map(|v| (v, inst))
        .map_err(|e| e.reason)
}

/// Rows of one topic, by output file, and its gap rows.
#[derive(Default)]
struct TopicOut {
    book: BTreeMap<usize, Vec<BookRow>>,
    trade: Vec<TradeRow>,
    ticker: Vec<TickerRow>,
    gaps: Vec<GapRow>,
    unattributed_drops: u64,
}

/// Per connection: sequence tracking (spec D4, R8).
#[derive(Default)]
struct ConnSeq {
    had_snapshot: bool,
    /// `Some` while tracking is alive: the last `update_id` and the
    /// `recv_ns` of the last good message.
    run: Option<(i64, i64)>,
}

struct Book {
    ob: OrderBook,
    last: BookRow,
}

/// Book replay of one topic (spec D4, R2, R3, R8, R9).
struct BookReplay<'c, 'a> {
    ctx: &'c Ctx<'a>,
    topic: &'c Topic,
    depth: i64,
    rule: SeqRule,
    hours: Vec<i64>,
    next_hour: usize,
    ends: &'c [i64],
    next_end: usize,
    seq: HashMap<ConnKey, ConnSeq>,
    followed: Option<ConnKey>,
    book: Option<Book>,
    out: TopicOut,
}

fn levels(l: &[md_core::Level]) -> (Vec<i64>, Vec<i64>) {
    (
        l.iter().map(|x| x.px).collect(),
        l.iter().map(|x| x.qty).collect(),
    )
}

fn book_row(u: &BookUpdate, kind: i64, depth: i64, lineage: Lineage) -> BookRow {
    let (bid_px, bid_qty) = levels(&u.bids);
    let (ask_px, ask_qty) = levels(&u.asks);
    BookRow {
        symbol: u.key.symbol.clone(),
        depth,
        kind,
        venue_ts_ns: u.venue_ts_ns,
        recv_ns: u.recv_ns,
        update_id: u.update_id,
        prev_update_id: u.prev_update_id,
        first_update_id: u.first_update_id,
        venue_seq: u.venue_seq,
        bid_px,
        bid_qty,
        ask_px,
        ask_qty,
        px_exp: i64::from(u.px_exp),
        qty_exp: i64::from(u.qty_exp),
        lineage,
    }
}

impl<'c, 'a> BookReplay<'c, 'a> {
    fn new(
        ctx: &'c Ctx<'a>,
        topic: &'c Topic,
        depth: i64,
        rule: SeqRule,
        small: bool,
        ends: &'c [i64],
    ) -> Self {
        let hours = if small {
            vec![ctx.day_start]
        } else {
            (0..24).map(|h| ctx.day_start + h * HOUR_NS).collect()
        };
        Self {
            ctx,
            topic,
            depth,
            rule,
            hours,
            next_hour: 0,
            ends,
            next_end: 0,
            seq: HashMap::new(),
            followed: None,
            book: None,
            out: TopicOut::default(),
        }
    }

    fn bucket(&self, recv_ns: i64) -> usize {
        if self.hours.len() == 1 {
            0
        } else {
            self.ctx.hour_of(recv_ns)
        }
    }

    /// Applies every hour start at or before `t` (a record at the hour start
    /// is not before it, lake contract 3.2) and every end of book state
    /// before `t`, in time order (an hour start before an end at the same
    /// time).
    fn advance(&mut self, t: i64) {
        loop {
            let hour = self.hours.get(self.next_hour).copied().filter(|h| *h <= t);
            let note = self.ends.get(self.next_end).copied().filter(|e| *e < t);
            let conn_end = match (&self.book, &self.followed) {
                (Some(_), Some(f)) => Some(self.ctx.chain_end(self.topic, f)).filter(|e| *e < t),
                _ => None,
            };
            let end = match (note, conn_end) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (a, b) => a.or(b),
            };
            match (hour, end) {
                (Some(h), e) if e.is_none_or(|e| h <= e) => {
                    self.checkpoint(h);
                    self.next_hour += 1;
                }
                (_, Some(e)) => {
                    self.book = None;
                    if note == Some(e) {
                        self.next_end += 1;
                    }
                }
                (None, None) => return,
                (Some(_), None) => unreachable!("handled by the first arm"),
            }
        }
    }

    fn checkpoint(&mut self, h: i64) {
        let Some(b) = &self.book else { return };
        let bids = b.ob.levels(md_core::BookSide::Bid);
        let asks = b.ob.levels(md_core::BookSide::Ask);
        let (bid_px, bid_qty) = levels(&bids);
        let (ask_px, ask_qty) = levels(&asks);
        let row = BookRow {
            kind: 2,
            recv_ns: h,
            bid_px,
            bid_qty,
            ask_px,
            ask_qty,
            ..b.last.clone()
        };
        let k = self.bucket(h);
        self.out.book.entry(k).or_default().push(row);
    }

    fn start_book(&mut self, c: &ConnKey, u: &BookUpdate, row: &BookRow) {
        let mut ob = OrderBook::new(None);
        ob.apply_snapshot(u);
        self.followed = Some(c.clone());
        self.book = Some(Book {
            ob,
            last: row.clone(),
        });
    }

    fn on_record(
        &mut self,
        file: &L0File,
        rec_no: u64,
        conn: Option<ConnKey>,
        r: &Record,
        parsed: Result<(Vec<Normalized>, Instrument), String>,
    ) {
        let t = r.recv_ns;
        self.advance(t);
        let emit = self.ctx.in_date(file);
        let lineage = lin(file, rec_no, r);
        let (msgs, conn) = match (parsed, conn) {
            (Ok((m, _)), Some(c)) => (m, c),
            (Ok(_), None) => {
                self.fail(emit, &lineage, t, "a WebSocket record without a connection");
                return;
            }
            (Err(why), _) => {
                self.fail(emit, &lineage, t, &why);
                return;
            }
        };
        for m in msgs {
            match m {
                Normalized::BookSnapshot(u) => self.snapshot(emit, &conn, &u, lineage.clone()),
                Normalized::BookDelta(u) => self.delta(emit, &conn, &u, lineage.clone()),
                other => {
                    self.fail(emit, &lineage, t, &format!("not a book message: {other:?}"));
                }
            }
        }
    }

    /// A `parse_error` of the symbol ends the book state (R3).
    fn fail(&mut self, emit: bool, lineage: &Lineage, t: i64, why: &str) {
        self.book = None;
        if emit {
            self.out.gaps.push(parse_error(self.topic, lineage, t, why));
        }
    }

    fn snapshot(&mut self, emit: bool, c: &ConnKey, u: &BookUpdate, lineage: Lineage) {
        let t = u.recv_ns;
        let row = book_row(u, 0, self.depth, lineage.clone());
        let s = self.seq.entry(c.clone()).or_default();
        let first = !s.had_snapshot;
        if s.run.is_some() && emit {
            self.out.gaps.push(GapRow {
                start_recv_ns: t,
                end_recv_ns: Some(t),
                ..gap(self.topic, "resnapshot", &lineage)
            });
        }
        s.had_snapshot = true;
        s.run = u.update_id.map(|id| (id, t)).or(Some((i64::MIN, t)));
        match self.followed.clone() {
            None => self.start_book(c, u, &row),
            Some(f) if &f == c => self.start_book(c, u, &row),
            Some(f) => {
                let (fi, ci) = (self.ctx.conn(&f), self.ctx.conn(c));
                if first && fi.first_ns < ci.first_ns && ci.first_ns <= fi.last_ns {
                    if emit {
                        self.out.gaps.push(GapRow {
                            start_recv_ns: t,
                            end_recv_ns: Some(t),
                            detail: compact(json!({
                                "from_conn_id": f.conn_id,
                                "to_conn_id": c.conn_id,
                            })),
                            ..gap(self.topic, "handover", &lineage)
                        });
                    }
                    self.start_book(c, u, &row);
                } else if fi.last_ns < t {
                    self.start_book(c, u, &row);
                }
            }
        }
        if emit {
            let k = self.bucket(t);
            self.out.book.entry(k).or_default().push(row);
        }
    }

    fn follows(&self, prev: i64, u: &BookUpdate) -> bool {
        match self.rule {
            SeqRule::PlusOne => u.update_id == prev.checked_add(1),
            SeqRule::PrevEquals => u.prev_update_id == Some(prev),
            SeqRule::None => true,
        }
    }

    fn delta(&mut self, emit: bool, c: &ConnKey, u: &BookUpdate, lineage: Lineage) {
        let t = u.recv_ns;
        let row = book_row(u, 1, self.depth, lineage.clone());
        let run = self.seq.get(c).and_then(|s| s.run);
        let mut broke = false;
        if let Some((prev, good_ns)) = run {
            if self.follows(prev, u) {
                if let Some(s) = self.seq.get_mut(c) {
                    s.run = Some((u.update_id.unwrap_or(prev), t));
                }
            } else {
                broke = true;
                if emit {
                    self.out.gaps.push(GapRow {
                        start_recv_ns: good_ns,
                        end_recv_ns: Some(t),
                        last_good_update_id: Some(prev).filter(|p| *p != i64::MIN),
                        next_update_id: u.update_id,
                        ..gap(self.topic, "seq_gap", &lineage)
                    });
                }
                if let Some(s) = self.seq.get_mut(c) {
                    s.run = None;
                }
                // R3: a break on any connection ends the book state.
                self.book = None;
            }
        }
        // A message of the topic on another connection after the followed
        // one ended, before any snapshot moved replay: the book missed it.
        if let Some(f) = self.followed.clone() {
            if &f != c && self.book.is_some() && self.ctx.conn(&f).last_ns < t {
                self.book = None;
            }
        }
        if !broke && run.is_some() && self.followed.as_ref() == Some(c) {
            if let Some(b) = &mut self.book {
                if (b.last.px_exp, b.last.qty_exp) != (row.px_exp, row.qty_exp) {
                    // R9: levels of two exponents never mix.
                    self.book = None;
                } else {
                    b.ob.apply_delta(u);
                    b.last = row.clone();
                }
            }
        }
        if emit {
            let k = self.bucket(t);
            self.out.book.entry(k).or_default().push(row);
        }
    }
}

fn side(s: Side) -> i64 {
    match s {
        Side::Buy => 1,
        Side::Sell => -1,
    }
}

/// One topic of the date: its rows and gap rows.
fn replay_topic(
    fs: &dyn Fs,
    ctx: &Ctx,
    settings: &Settings,
    topic: &Topic,
    files: &[&L0File],
    ends: &[i64],
) -> Result<TopicOut, BuildError> {
    let kind = ctx.venue.channel_kind(&topic.channel);
    let mut cache = AdapterCache::new(&topic.market);
    match kind {
        Some(ChannelKind::Book { depth }) => {
            let rule = {
                let inst = Instrument {
                    symbol: topic.symbol.clone(),
                    status: String::new(),
                    tick_size: "1".into(),
                    lot_size: "1".into(),
                    px_exp: 0,
                    qty_exp: 0,
                    contract_type: None,
                    settle_coin: None,
                    venue_ts_ns: None,
                };
                ctx.venue
                    .adapter(&topic.market, &[inst])
                    .map(|a| {
                        a.seq_rule(&StreamKey::new(topic.channel.clone(), topic.symbol.clone()))
                    })
                    .unwrap_or(SeqRule::None)
            };
            let small = settings.small_channels.contains(&topic.channel);
            let mut rp = BookReplay::new(ctx, topic, depth, rule, small, ends);
            for f in files {
                for_each_record(fs, f, |rec_no, conn, r| {
                    let parsed = parse_record(ctx, &mut cache, topic, f, &r);
                    rp.on_record(f, rec_no, conn, &r, parsed);
                })?;
            }
            rp.advance(ctx.day_start + DAY_NS);
            Ok(rp.out)
        }
        Some(ChannelKind::Trade) => replay_trades(fs, ctx, &mut cache, topic, files),
        Some(ChannelKind::Ticker) => {
            let mut out = TopicOut::default();
            for f in files.iter().filter(|f| ctx.in_date(f)) {
                for_each_record(fs, f, |rec_no, _, r| {
                    let lineage = lin(f, rec_no, &r);
                    match parse_record(ctx, &mut cache, topic, f, &r) {
                        Ok((msgs, _)) => {
                            for m in msgs {
                                match m {
                                    Normalized::Ticker(k) => out.ticker.push(TickerRow {
                                        symbol: k.key.symbol.clone(),
                                        kind: if k.is_snapshot { 0 } else { 1 },
                                        venue_ts_ns: k.venue_ts_ns,
                                        recv_ns: k.recv_ns,
                                        last_px: k.last_px,
                                        mark_px: k.mark_px,
                                        index_px: k.index_px,
                                        funding_rate: k.funding_rate,
                                        next_funding_ns: k.next_funding_ns,
                                        open_interest: k.open_interest,
                                        best_bid_px: k.best_bid_px,
                                        best_bid_qty: k.best_bid_qty,
                                        best_ask_px: k.best_ask_px,
                                        best_ask_qty: k.best_ask_qty,
                                        px_exp: i64::from(k.px_exp),
                                        qty_exp: i64::from(k.qty_exp),
                                        ref_px_exp: i64::from(k.ref_px_exp),
                                        lineage: lineage.clone(),
                                    }),
                                    other => out.gaps.push(parse_error(
                                        topic,
                                        &lineage,
                                        r.recv_ns,
                                        &format!("not a ticker message: {other:?}"),
                                    )),
                                }
                            }
                        }
                        Err(why) => out.gaps.push(parse_error(topic, &lineage, r.recv_ns, &why)),
                    }
                })?;
            }
            Ok(out)
        }
        Some(ChannelKind::Instruments) => Ok(TopicOut::default()),
        None => {
            let mut out = TopicOut::default();
            for f in files.iter().filter(|f| ctx.in_date(f)) {
                for_each_record(fs, f, |rec_no, _, r| {
                    let lineage = lin(f, rec_no, &r);
                    out.gaps.push(parse_error(
                        topic,
                        &lineage,
                        r.recv_ns,
                        &format!("channel {} is not parsed", topic.channel),
                    ));
                })?;
            }
            Ok(out)
        }
    }
}

/// Trades: the followed connection, handover rows and de-duplication over
/// the date before and the date (spec D4).
fn replay_trades(
    fs: &dyn Fs,
    ctx: &Ctx,
    cache: &mut AdapterCache,
    topic: &Topic,
    files: &[&L0File],
) -> Result<TopicOut, BuildError> {
    let mut out = TopicOut::default();
    let mut followed: Option<ConnKey> = None;
    // Every trade seen: key → (recv_ns, l0_file_id, rec_no, conn, row index in out.trade when in D).
    struct Seen {
        recv_ns: i64,
        file: String,
        rec_no: i64,
        conn: Option<ConnKey>,
        row: Option<usize>,
    }
    let mut all: Vec<(String, Seen)> = Vec::new();
    let mut handovers: Vec<(ConnKey, ConnKey, usize)> = Vec::new();
    for f in files {
        let emit = ctx.in_date(f);
        for_each_record(fs, f, |rec_no, conn, r| {
            let lineage = lin(f, rec_no, &r);
            let t = r.recv_ns;
            if let Some(c) = &conn {
                let first = ctx
                    .pass1
                    .conns
                    .get(c)
                    .and_then(|i| i.topics.get(topic))
                    .is_some_and(|ev| ev.l0_file_id == f.l0_file_id && ev.rec_no == rec_no);
                if first {
                    match followed.clone() {
                        None => followed = Some(c.clone()),
                        Some(fo) if &fo == c => {}
                        Some(fo) => {
                            let (fi, ci) = (ctx.conn(&fo), ctx.conn(c));
                            if fi.first_ns < ci.first_ns && ci.first_ns <= fi.last_ns {
                                if emit {
                                    handovers.push((fo.clone(), c.clone(), out.gaps.len()));
                                    out.gaps.push(GapRow {
                                        start_recv_ns: t,
                                        end_recv_ns: Some(t),
                                        detail: String::new(),
                                        ..gap(topic, "handover", &lineage)
                                    });
                                }
                                followed = Some(c.clone());
                            } else if fi.last_ns < t {
                                followed = Some(c.clone());
                            }
                        }
                    }
                }
            }
            match parse_record(ctx, cache, topic, f, &r) {
                Ok((msgs, _)) => {
                    for (idx, m) in msgs.into_iter().enumerate() {
                        match m {
                            Normalized::Trade(x) => {
                                let row = emit.then(|| {
                                    out.trade.push(TradeRow {
                                        symbol: x.key.symbol.clone(),
                                        venue_ts_ns: x.venue_ts_ns,
                                        recv_ns: x.recv_ns,
                                        trade_id: x.trade_id.clone(),
                                        side: side(x.side),
                                        px: x.px,
                                        qty: x.qty,
                                        px_exp: i64::from(x.px_exp),
                                        qty_exp: i64::from(x.qty_exp),
                                        venue_seq: x.venue_seq,
                                        flags: i64::from(x.flags),
                                        msg_idx: idx as i64,
                                        lineage: lineage.clone(),
                                    });
                                    out.trade.len() - 1
                                });
                                all.push((
                                    x.trade_id,
                                    Seen {
                                        recv_ns: t,
                                        file: f.l0_file_id.clone(),
                                        rec_no: rec_no as i64,
                                        conn: conn.clone(),
                                        row,
                                    },
                                ));
                            }
                            other => {
                                if emit {
                                    out.gaps.push(parse_error(
                                        topic,
                                        &lineage,
                                        t,
                                        &format!("not a trade message: {other:?}"),
                                    ));
                                }
                            }
                        }
                    }
                }
                Err(why) => {
                    if emit {
                        out.gaps.push(parse_error(topic, &lineage, t, &why));
                    }
                }
            }
        })?;
    }
    // De-duplication: keep the earliest (recv_ns, l0_file_id, rec_no).
    let mut best: HashMap<&str, usize> = HashMap::new();
    for (i, (id, s)) in all.iter().enumerate() {
        match best.get(id.as_str()) {
            Some(&j) => {
                let b = &all[j].1;
                if (s.recv_ns, &s.file, s.rec_no) < (b.recv_ns, &b.file, b.rec_no) {
                    best.insert(id, i);
                }
            }
            None => {
                best.insert(id, i);
            }
        }
    }
    let mut counts = vec![0u64; handovers.len()];
    let mut drop_rows = BTreeSet::new();
    for (i, (id, s)) in all.iter().enumerate() {
        let k = best[id.as_str()];
        if k == i {
            continue;
        }
        let Some(row) = s.row else { continue };
        drop_rows.insert(row);
        let kept = &all[k].1;
        let pair = |a: &ConnKey, b: &ConnKey| matches!((&kept.conn, &s.conn), (Some(x), Some(y)) if (x == a && y == b) || (x == b && y == a));
        match handovers.iter().position(|(a, b, _)| pair(a, b)) {
            Some(h) => counts[h] += 1,
            None => out.unattributed_drops += 1,
        }
    }
    for ((from, to, at), n) in handovers.iter().zip(counts) {
        out.gaps[*at].detail = compact(json!({
            "dropped_trade_rows": n,
            "from_conn_id": from.conn_id,
            "to_conn_id": to.conn_id,
        }));
    }
    let rows = std::mem::take(&mut out.trade);
    out.trade = rows
        .into_iter()
        .enumerate()
        .filter(|(i, _)| !drop_rows.contains(i))
        .map(|(_, r)| r)
        .collect();
    Ok(out)
}

/// The output files of one date under the lake root, written as `.tmp`.
struct Out<'a> {
    root: &'a Path,
    venue: &'a str,
    date: &'a str,
    parser_ver: &'a str,
    write: WriteSettings,
    disk_check: Option<DiskCheck>,
    pushed: u64,
    open: BTreeMap<String, (FileWriter, u64, Option<i64>, Option<i64>)>,
    done: Vec<BuiltFile>,
}

impl Out<'_> {
    fn path(&self, table: Table, market: &str, channel: &str, name: &str) -> String {
        let v = self.venue;
        let d = self.date;
        match table {
            Table::Gap => format!("crypto/l1/gap/venue={v}/date={d}/{name}"),
            Table::Instrument => {
                format!("crypto/l1/instrument/venue={v}/market={market}/date={d}/{name}")
            }
            t => format!(
                "crypto/l1/{}/venue={v}/market={market}/channel={channel}/date={d}/{name}",
                t.name()
            ),
        }
    }

    fn file(&self, path: &str) -> PathBuf {
        self.root.join(tmp_path(path))
    }

    fn push<R: L1Row>(
        &mut self,
        table: Table,
        path: String,
        market: &str,
        channel: &str,
        rows: &[R],
        update_ids: impl Fn(&R) -> Option<i64>,
    ) -> Result<(), BuildError> {
        if !self.open.contains_key(&path) {
            if let Some(DiskCheck(check)) = &self.disk_check {
                check().map_err(BuildError::Disk)?;
            }
            let file = self.file(&path);
            if let Some(dir) = file.parent() {
                crate::ro::create_dirs_synced(dir)?;
            }
            let w = FileWriter::create(&file, table, self.parser_ver, self.write)?;
            self.open.insert(path.clone(), (w, 0, None, None));
        }
        let common = Common {
            venue: self.venue,
            market,
            channel,
            parser_ver: self.parser_ver,
        };
        let schema = table.schema();
        let Some((w, n, first, last)) = self.open.get_mut(&path) else {
            return Err(BuildError::Defect(format!("{path} is not open")));
        };
        for r in rows {
            let c = cells(r, &common, schema).map_err(BuildError::Defect)?;
            w.push(&c)?;
            *n += 1;
            // Final review F6: the collector writes to the same disk, so
            // the floor is checked while a file grows too.
            self.pushed += 1;
            if self.pushed % DISK_CHECK_ROWS == 0 {
                if let Some(DiskCheck(check)) = &self.disk_check {
                    check().map_err(BuildError::Disk)?;
                }
            }
            if let Some(u) = update_ids(r) {
                first.get_or_insert(u);
                *last = Some(u);
            }
        }
        Ok(())
    }

    fn close_all(&mut self) -> Result<(), BuildError> {
        for (path, (w, rows, first, last)) in std::mem::take(&mut self.open) {
            let table = w.table();
            let extra = if table == Table::BookMsg {
                let s = |v: Option<i64>| v.map(|x| x.to_string()).unwrap_or_default();
                vec![
                    ("first_update_id".to_string(), s(first)),
                    ("last_update_id".to_string(), s(last)),
                ]
            } else {
                Vec::new()
            };
            w.finish(extra)?;
            self.done.push(BuiltFile { path, table, rows });
        }
        Ok(())
    }
}

/// Writes the rows of one topic to the open files of its channel.
fn write_topic(
    out: &mut Out,
    ctx: &Ctx,
    settings: &Settings,
    topic: &Topic,
    t: &mut TopicOut,
    gaps: &mut Vec<GapRow>,
    unattributed: &mut u64,
) -> Result<(), BuildError> {
    *unattributed += t.unattributed_drops;
    gaps.append(&mut t.gaps);
    let small = settings.small_channels.contains(&topic.channel);
    let name = |h: usize| {
        if small {
            "day.parquet".to_string()
        } else {
            format!("hour={h:02}.parquet")
        }
    };
    for (h, rows) in std::mem::take(&mut t.book) {
        let mut rows = rows;
        rows.sort_by(|a, b| a.sort_key().cmp(&b.sort_key()));
        let path = out.path(Table::BookMsg, &topic.market, &topic.channel, &name(h));
        out.push(
            Table::BookMsg,
            path,
            &topic.market,
            &topic.channel,
            &rows,
            |r| {
                if r.kind == 2 { None } else { r.update_id }
            },
        )?;
    }
    let mut trade: BTreeMap<usize, Vec<TradeRow>> = BTreeMap::new();
    for r in std::mem::take(&mut t.trade) {
        let h = if small { 0 } else { ctx.hour_of(r.recv_ns) };
        trade.entry(h).or_default().push(r);
    }
    for (h, mut rows) in trade {
        rows.sort_by(|a, b| a.sort_key().cmp(&b.sort_key()));
        let path = out.path(Table::Trade, &topic.market, &topic.channel, &name(h));
        out.push(
            Table::Trade,
            path,
            &topic.market,
            &topic.channel,
            &rows,
            |_| None,
        )?;
    }
    let mut ticker: BTreeMap<usize, Vec<TickerRow>> = BTreeMap::new();
    for r in std::mem::take(&mut t.ticker) {
        let h = if small { 0 } else { ctx.hour_of(r.recv_ns) };
        ticker.entry(h).or_default().push(r);
    }
    for (h, mut rows) in ticker {
        rows.sort_by(|a, b| a.sort_key().cmp(&b.sort_key()));
        let path = out.path(Table::Ticker, &topic.market, &topic.channel, &name(h));
        out.push(
            Table::Ticker,
            path,
            &topic.market,
            &topic.channel,
            &rows,
            |_| None,
        )?;
    }
    Ok(())
}

/// Builds `d` into `.tmp` files below `root` (the lake root of `fs`).
pub fn build_date(
    root: &Path,
    fs: &dyn Fs,
    venue: &dyn Venue,
    d: &str,
    settings: &Settings,
) -> Result<Built, BuildError> {
    let (prev, cur) = inputs(fs, venue.name(), d)?;
    let day_start = date_start(d).ok_or_else(|| BuildError::Defect(format!("date {d}")))?;
    let files: Vec<L0File> = prev.iter().chain(cur.iter()).cloned().collect();
    let pass1 = Pass1::scan(fs, &files)?;
    let mut carriers: BTreeMap<Topic, Vec<ConnKey>> = BTreeMap::new();
    for (c, i) in &pass1.conns {
        for t in i.topics.keys() {
            carriers.entry(t.clone()).or_default().push(c.clone());
        }
    }
    let markets: BTreeSet<String> = files
        .iter()
        .filter_map(|f| f.topic().map(|t| t.market))
        .collect();
    let mut instruments = BTreeMap::new();
    for m in &markets {
        let all = responses(fs, venue, d, m)?;
        for fd in [prev_date(d).unwrap_or_default(), d.to_string()] {
            instruments.insert((m.clone(), fd.clone()), window(&all, &fd));
        }
    }
    let ctx = Ctx {
        venue,
        date: d.to_string(),
        day_start,
        pass1,
        carriers,
        instruments,
    };
    // Topics with records in the files of D or D−1 (contract 3.5).
    let topics: BTreeSet<Topic> = files
        .iter()
        .filter(|f| f.records > 0)
        .filter_map(L0File::topic)
        .collect();
    let ends = note_ends(&ctx, &topics);
    let mut gaps = note_and_reconnect_rows(&ctx, &topics, &files);
    let mut out = Out {
        root,
        venue: venue.name(),
        date: d,
        parser_ver: venue.parser_ver(),
        write: settings.write,
        disk_check: settings.disk_check.clone(),
        pushed: 0,
        open: BTreeMap::new(),
        done: Vec::new(),
    };
    let mut unattributed = 0;
    let mut group: Option<(String, String)> = None;
    let order: Vec<&Topic> = topics
        .iter()
        .filter(|t| t.channel != "instruments")
        .collect();
    let no_ends = Vec::new();
    // R6: up to `threads` topics are replayed at once; their rows are
    // written in topic order, so every file stays in symbol order.
    for chunk in order.chunks(settings.threads.max(1)) {
        let results: Vec<Result<TopicOut, BuildError>> = std::thread::scope(|sc| {
            let handles: Vec<_> = chunk
                .iter()
                .map(|topic| {
                    let tfiles: Vec<&L0File> = files
                        .iter()
                        .filter(|f| f.topic().as_ref() == Some(*topic))
                        .collect();
                    let ends = ends.get(*topic).unwrap_or(&no_ends);
                    let ctx = &ctx;
                    sc.spawn(move || replay_topic(fs, ctx, settings, topic, &tfiles, ends))
                })
                .collect();
            handles
                .into_iter()
                .map(|h| {
                    h.join().unwrap_or_else(|_| {
                        Err(BuildError::Defect("a replay thread panicked".into()))
                    })
                })
                .collect()
        });
        for (topic, t) in chunk.iter().zip(results) {
            let mut t = t?;
            let g = (topic.market.clone(), topic.channel.clone());
            if group.as_ref() != Some(&g) {
                out.close_all()?;
                group = Some(g);
            }
            write_topic(
                &mut out,
                &ctx,
                settings,
                topic,
                &mut t,
                &mut gaps,
                &mut unattributed,
            )?;
        }
    }
    out.close_all()?;
    // Instruments of the date.
    for m in &markets {
        let mut rows = Vec::new();
        let topic = Topic {
            market: m.clone(),
            channel: "instruments".into(),
            symbol: "_all".into(),
        };
        for f in cur
            .iter()
            .filter(|f| matches!(&f.kind, FileKind::Instruments { market } if market == m))
        {
            for_each_record(fs, f, |rec_no, _, r| {
                let lineage = lin(f, rec_no, &r);
                if r.src != Src::Rest {
                    gaps.push(parse_error(
                        &topic,
                        &lineage,
                        r.recv_ns,
                        "not a REST record",
                    ));
                    return;
                }
                match venue.parse_instruments(m, &r.payload, r.recv_ns) {
                    Ok(list) => {
                        for i in list {
                            rows.push(InstrumentRow {
                                symbol: i.symbol,
                                recv_ns: r.recv_ns,
                                venue_ts_ns: i.venue_ts_ns,
                                status: i.status,
                                tick_size: i.tick_size,
                                lot_size: i.lot_size,
                                px_exp: i64::from(i.px_exp),
                                qty_exp: i64::from(i.qty_exp),
                                contract_type: i.contract_type,
                                settle_coin: i.settle_coin,
                                lineage: lineage.clone(),
                            });
                        }
                    }
                    Err(e) => gaps.push(parse_error(&topic, &lineage, r.recv_ns, &e.reason)),
                }
            })?;
        }
        if !rows.is_empty() {
            rows.sort_by(|a, b| a.sort_key().cmp(&b.sort_key()));
            let path = out.path(Table::Instrument, m, "instruments", "day.parquet");
            out.push(Table::Instrument, path, m, "instruments", &rows, |_| None)?;
        }
    }
    out.close_all()?;
    // The gap file, always, last (spec D2, D7).
    gaps.sort_by(|a, b| a.sort_key().cmp(&b.sort_key()));
    for w in gaps.windows(2) {
        if w[0].key() == w[1].key() {
            return Err(BuildError::Defect(format!(
                "two gap rows with key {:?}",
                w[0].key()
            )));
        }
    }
    let mut by_kind: BTreeMap<String, u64> = BTreeMap::new();
    for g in &gaps {
        *by_kind.entry(g.kind.to_string()).or_default() += 1;
    }
    let path = out.path(Table::Gap, "", "", "day.parquet");
    let file = out.file(&path);
    if let Some(dir) = file.parent() {
        crate::ro::create_dirs_synced(dir)?;
    }
    let w = FileWriter::create(&file, Table::Gap, venue.parser_ver(), settings.write)?;
    out.open.insert(path.clone(), (w, 0, None, None));
    out.push(Table::Gap, path, "", "", &gaps, |_| None)?;
    out.close_all()?;
    let mut done = out.done;
    let gap_at = done.iter().position(|f| f.table == Table::Gap);
    let gap_file = gap_at.map(|i| done.remove(i));
    done.sort_by(|a, b| a.path.cmp(&b.path));
    done.extend(gap_file);
    Ok(Built {
        date: d.to_string(),
        files: done,
        gap_rows_by_kind: by_kind,
        unattributed_trade_drops: unattributed,
        l0_bytes: cur.iter().map(|f| f.bytes).sum(),
    })
}
