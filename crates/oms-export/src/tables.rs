//! The three trading tables, from the vendored contract files
//! (`contracts/lake/trading/<table>.table.json`).

use std::sync::OnceLock;

use md_archiver::table::{TableSchema, parse_vendored};

/// The exporter's version (contract 6): raised when the same journal would
/// give different rows. Every file carries it, so a rise rewrites every
/// date.
pub const EXPORT_VER: &str = "oms-export-1";

pub const ORDERS_JSON: &str = include_str!("../../../contracts/lake/trading/orders.table.json");
pub const FILLS_JSON: &str = include_str!("../../../contracts/lake/trading/fills.table.json");
pub const EVENTS_JSON: &str = include_str!("../../../contracts/lake/trading/events.table.json");

pub fn orders() -> &'static TableSchema {
    static S: OnceLock<TableSchema> = OnceLock::new();
    S.get_or_init(|| parse_vendored(ORDERS_JSON))
}

pub fn fills() -> &'static TableSchema {
    static S: OnceLock<TableSchema> = OnceLock::new();
    S.get_or_init(|| parse_vendored(FILLS_JSON))
}

pub fn events() -> &'static TableSchema {
    static S: OnceLock<TableSchema> = OnceLock::new();
    S.get_or_init(|| parse_vendored(EVENTS_JSON))
}

/// The tables in the order the exporter writes them.
pub fn all() -> [&'static TableSchema; 3] {
    [orders(), fills(), events()]
}
