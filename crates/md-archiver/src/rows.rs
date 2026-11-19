//! The rows of the five L1 tables (lake contract 3.2–3.6) and their cells.
//! A row gives the cell of a column by the column's contract name, so the
//! column order comes only from the contract file.

use crate::parquet_out::Cell;
use crate::table::TableSchema;

/// Lineage of a row (lake contract 3.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lineage {
    pub conn_id: i64,
    pub frame_no: i64,
    pub l0_file_id: String,
    pub rec_no: i64,
}

fn lineage<'a>(l: &'a Lineage, col: &str) -> Option<Cell<'a>> {
    Some(match col {
        "conn_id" => Cell::Int(l.conn_id),
        "frame_no" => Cell::Int(l.frame_no),
        "l0_file_id" => Cell::Str(&l.l0_file_id),
        "rec_no" => Cell::Int(l.rec_no),
        _ => return None,
    })
}

fn opt(v: Option<i64>) -> Cell<'static> {
    v.map_or(Cell::Null, Cell::Int)
}

fn opt_str(v: &Option<String>) -> Cell<'_> {
    v.as_deref().map_or(Cell::Null, Cell::Str)
}

/// The columns shared by every table: the topic and `parser_ver`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Common<'a> {
    pub venue: &'a str,
    pub market: &'a str,
    pub channel: &'a str,
    pub parser_ver: &'a str,
}

pub trait L1Row {
    /// The cell of column `col`, `None` when the row has no such column.
    fn cell<'a>(&'a self, common: &'a Common<'a>, col: &str) -> Option<Cell<'a>>;
}

/// The cells of `row` in the order of `schema`. A column the row does not
/// know is a defect of this crate and an error.
pub fn cells<'a, R: L1Row>(
    row: &'a R,
    common: &'a Common<'a>,
    schema: &TableSchema,
) -> Result<Vec<Cell<'a>>, String> {
    schema
        .columns
        .iter()
        .map(|c| match c.name.as_str() {
            "venue" => Ok(Cell::Str(common.venue)),
            "parser_ver" => Ok(Cell::Str(common.parser_ver)),
            name => row
                .cell(common, name)
                .ok_or_else(|| format!("{}: the row has no column {name}", schema.name)),
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BookRow {
    pub symbol: String,
    pub depth: i64,
    /// 0 snapshot, 1 delta, 2 checkpoint.
    pub kind: i64,
    pub venue_ts_ns: i64,
    pub recv_ns: i64,
    pub update_id: Option<i64>,
    pub prev_update_id: Option<i64>,
    pub first_update_id: Option<i64>,
    pub venue_seq: Option<i64>,
    pub bid_px: Vec<i64>,
    pub bid_qty: Vec<i64>,
    pub ask_px: Vec<i64>,
    pub ask_qty: Vec<i64>,
    pub px_exp: i64,
    pub qty_exp: i64,
    pub lineage: Lineage,
}

impl BookRow {
    /// Sort key inside one symbol: `recv_ns`, checkpoint rows first,
    /// `l0_file_id`, `rec_no` (lake contract 3.2).
    pub fn sort_key(&self) -> (i64, bool, &str, i64) {
        (
            self.recv_ns,
            self.kind != 2,
            &self.lineage.l0_file_id,
            self.lineage.rec_no,
        )
    }
}

impl L1Row for BookRow {
    fn cell<'a>(&'a self, common: &'a Common<'a>, col: &str) -> Option<Cell<'a>> {
        Some(match col {
            "market" => Cell::Str(common.market),
            "channel" => Cell::Str(common.channel),
            "symbol" => Cell::Str(&self.symbol),
            "depth" => Cell::Int(self.depth),
            "kind" => Cell::Int(self.kind),
            "venue_ts_ns" => Cell::Int(self.venue_ts_ns),
            "recv_ns" => Cell::Int(self.recv_ns),
            "update_id" => opt(self.update_id),
            "prev_update_id" => opt(self.prev_update_id),
            "first_update_id" => opt(self.first_update_id),
            "venue_seq" => opt(self.venue_seq),
            "bid_px" => Cell::List(self.bid_px.clone()),
            "bid_qty" => Cell::List(self.bid_qty.clone()),
            "ask_px" => Cell::List(self.ask_px.clone()),
            "ask_qty" => Cell::List(self.ask_qty.clone()),
            "px_exp" => Cell::Int(self.px_exp),
            "qty_exp" => Cell::Int(self.qty_exp),
            c => return lineage(&self.lineage, c),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TradeRow {
    pub symbol: String,
    pub venue_ts_ns: i64,
    pub recv_ns: i64,
    pub trade_id: String,
    pub side: i64,
    pub px: i64,
    pub qty: i64,
    pub px_exp: i64,
    pub qty_exp: i64,
    pub venue_seq: Option<i64>,
    pub flags: i64,
    pub msg_idx: i64,
    pub lineage: Lineage,
}

impl TradeRow {
    /// `recv_ns`, `rec_no`, `msg_idx` (lake contract 3.3), then the file.
    pub fn sort_key(&self) -> (i64, i64, i64, &str) {
        (
            self.recv_ns,
            self.lineage.rec_no,
            self.msg_idx,
            &self.lineage.l0_file_id,
        )
    }
}

impl L1Row for TradeRow {
    fn cell<'a>(&'a self, common: &'a Common<'a>, col: &str) -> Option<Cell<'a>> {
        Some(match col {
            "market" => Cell::Str(common.market),
            "channel" => Cell::Str(common.channel),
            "symbol" => Cell::Str(&self.symbol),
            "venue_ts_ns" => Cell::Int(self.venue_ts_ns),
            "recv_ns" => Cell::Int(self.recv_ns),
            "trade_id" => Cell::Str(&self.trade_id),
            "side" => Cell::Int(self.side),
            "px" => Cell::Int(self.px),
            "qty" => Cell::Int(self.qty),
            "px_exp" => Cell::Int(self.px_exp),
            "qty_exp" => Cell::Int(self.qty_exp),
            "venue_seq" => opt(self.venue_seq),
            "flags" => Cell::Int(self.flags),
            "msg_idx" => Cell::Int(self.msg_idx),
            c => return lineage(&self.lineage, c),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TickerRow {
    pub symbol: String,
    pub kind: i64,
    pub venue_ts_ns: i64,
    pub recv_ns: i64,
    pub last_px: Option<i64>,
    pub mark_px: Option<i64>,
    pub index_px: Option<i64>,
    pub funding_rate: Option<i64>,
    pub next_funding_ns: Option<i64>,
    pub open_interest: Option<i64>,
    pub best_bid_px: Option<i64>,
    pub best_bid_qty: Option<i64>,
    pub best_ask_px: Option<i64>,
    pub best_ask_qty: Option<i64>,
    pub px_exp: i64,
    pub qty_exp: i64,
    pub ref_px_exp: i64,
    pub lineage: Lineage,
}

impl TickerRow {
    pub fn sort_key(&self) -> (i64, &str, i64) {
        (self.recv_ns, &self.lineage.l0_file_id, self.lineage.rec_no)
    }
}

impl L1Row for TickerRow {
    fn cell<'a>(&'a self, common: &'a Common<'a>, col: &str) -> Option<Cell<'a>> {
        Some(match col {
            "market" => Cell::Str(common.market),
            "channel" => Cell::Str(common.channel),
            "symbol" => Cell::Str(&self.symbol),
            "kind" => Cell::Int(self.kind),
            "venue_ts_ns" => Cell::Int(self.venue_ts_ns),
            "recv_ns" => Cell::Int(self.recv_ns),
            "last_px" => opt(self.last_px),
            "mark_px" => opt(self.mark_px),
            "index_px" => opt(self.index_px),
            "funding_rate" => opt(self.funding_rate),
            "next_funding_ns" => opt(self.next_funding_ns),
            "open_interest" => opt(self.open_interest),
            "best_bid_px" => opt(self.best_bid_px),
            "best_bid_qty" => opt(self.best_bid_qty),
            "best_ask_px" => opt(self.best_ask_px),
            "best_ask_qty" => opt(self.best_ask_qty),
            "px_exp" => Cell::Int(self.px_exp),
            "qty_exp" => Cell::Int(self.qty_exp),
            "ref_px_exp" => Cell::Int(self.ref_px_exp),
            c => return lineage(&self.lineage, c),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstrumentRow {
    pub symbol: String,
    pub recv_ns: i64,
    pub venue_ts_ns: Option<i64>,
    pub status: String,
    pub tick_size: String,
    pub lot_size: String,
    pub px_exp: i64,
    pub qty_exp: i64,
    pub contract_type: Option<String>,
    pub settle_coin: Option<String>,
    pub lineage: Lineage,
}

impl InstrumentRow {
    pub fn sort_key(&self) -> (&str, i64, &str, i64) {
        (
            &self.symbol,
            self.recv_ns,
            &self.lineage.l0_file_id,
            self.lineage.rec_no,
        )
    }
}

impl L1Row for InstrumentRow {
    fn cell<'a>(&'a self, common: &'a Common<'a>, col: &str) -> Option<Cell<'a>> {
        Some(match col {
            "market" => Cell::Str(common.market),
            "symbol" => Cell::Str(&self.symbol),
            "recv_ns" => Cell::Int(self.recv_ns),
            "venue_ts_ns" => opt(self.venue_ts_ns),
            "status" => Cell::Str(&self.status),
            "tick_size" => Cell::Str(&self.tick_size),
            "lot_size" => Cell::Str(&self.lot_size),
            "px_exp" => Cell::Int(self.px_exp),
            "qty_exp" => Cell::Int(self.qty_exp),
            "contract_type" => opt_str(&self.contract_type),
            "settle_coin" => opt_str(&self.settle_coin),
            c => return lineage(&self.lineage, c),
        })
    }
}

/// A gap row (lake contract 3.5). Its topic is its own, not the writer's
/// common topic: one gap file holds every topic of the venue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GapRow {
    pub market: String,
    pub symbol: String,
    pub channel: String,
    pub kind: &'static str,
    pub conn_id: Option<i64>,
    pub start_recv_ns: i64,
    pub end_recv_ns: Option<i64>,
    pub last_good_update_id: Option<i64>,
    pub next_update_id: Option<i64>,
    /// Compact JSON with sorted keys.
    pub detail: String,
    pub l0_file_id: Option<String>,
    pub rec_no: Option<i64>,
}

impl GapRow {
    /// `market`, `symbol`, `channel`, `start_recv_ns`, `kind`, `conn_id`
    /// (null first), then `(l0_file_id, rec_no)` (spec R11).
    #[allow(clippy::type_complexity)]
    pub fn sort_key(
        &self,
    ) -> (
        &str,
        &str,
        &str,
        i64,
        &str,
        Option<i64>,
        Option<&str>,
        Option<i64>,
    ) {
        (
            &self.market,
            &self.symbol,
            &self.channel,
            self.start_recv_ns,
            self.kind,
            self.conn_id,
            self.l0_file_id.as_deref(),
            self.rec_no,
        )
    }

    /// The contract key (lake contract 3.5).
    pub fn key(&self) -> (&str, &str, &str, &str, i64, Option<i64>) {
        (
            &self.market,
            &self.symbol,
            &self.channel,
            self.kind,
            self.start_recv_ns,
            self.conn_id,
        )
    }
}

impl L1Row for GapRow {
    fn cell<'a>(&'a self, _common: &'a Common<'a>, col: &str) -> Option<Cell<'a>> {
        Some(match col {
            "market" => Cell::Str(&self.market),
            "symbol" => Cell::Str(&self.symbol),
            "channel" => Cell::Str(&self.channel),
            "kind" => Cell::Str(self.kind),
            "conn_id" => opt(self.conn_id),
            "start_recv_ns" => Cell::Int(self.start_recv_ns),
            "end_recv_ns" => opt(self.end_recv_ns),
            "last_good_update_id" => opt(self.last_good_update_id),
            "next_update_id" => opt(self.next_update_id),
            "detail" => Cell::Str(&self.detail),
            "l0_file_id" => opt_str(&self.l0_file_id),
            "rec_no" => opt(self.rec_no),
            _ => return None,
        })
    }
}
