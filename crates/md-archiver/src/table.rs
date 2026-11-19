//! The five L1 tables, described by the vendored contract files
//! (`contracts/lake/l1/<table>.table.json`): column names, order, types and
//! nullability are read from them, never written here a second time.

use std::sync::OnceLock;

use serde_json::Value;

/// A column type of the contract (lake contract 3.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColType {
    String,
    Bool,
    Int8,
    Int16,
    Int32,
    Int64,
    /// `list<int64>`, elements never null.
    ListInt64,
}

impl ColType {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "string" => Self::String,
            "bool" => Self::Bool,
            "int8" => Self::Int8,
            "int16" => Self::Int16,
            "int32" => Self::Int32,
            "int64" => Self::Int64,
            "list<int64>" => Self::ListInt64,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Column {
    pub name: String,
    pub ty: ColType,
    pub nullable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableSchema {
    pub name: String,
    pub contract_ver: i64,
    pub columns: Vec<Column>,
    /// The key-value metadata keys, in the contract's order.
    pub metadata: Vec<String>,
}

impl TableSchema {
    pub fn index(&self, column: &str) -> Option<usize> {
        self.columns.iter().position(|c| c.name == column)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Table {
    BookMsg,
    Trade,
    Ticker,
    Gap,
    Instrument,
}

impl Table {
    pub const ALL: [Table; 5] = [
        Table::BookMsg,
        Table::Trade,
        Table::Ticker,
        Table::Gap,
        Table::Instrument,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Table::BookMsg => "book_msg",
            Table::Trade => "trade",
            Table::Ticker => "ticker",
            Table::Gap => "gap",
            Table::Instrument => "instrument",
        }
    }

    /// The vendored contract file of the table.
    pub fn contract_json(self) -> &'static str {
        match self {
            Table::BookMsg => include_str!("../../../contracts/lake/l1/book_msg.table.json"),
            Table::Trade => include_str!("../../../contracts/lake/l1/trade.table.json"),
            Table::Ticker => include_str!("../../../contracts/lake/l1/ticker.table.json"),
            Table::Gap => include_str!("../../../contracts/lake/l1/gap.table.json"),
            Table::Instrument => include_str!("../../../contracts/lake/l1/instrument.table.json"),
        }
    }

    /// The schema of the table. The vendored files are checked by the tests,
    /// so a file that does not parse is a build defect and panics.
    pub fn schema(self) -> &'static TableSchema {
        static SCHEMAS: OnceLock<Vec<TableSchema>> = OnceLock::new();
        let all = SCHEMAS.get_or_init(|| Table::ALL.iter().map(|t| load(*t)).collect());
        &all[self as usize]
    }
}

#[allow(clippy::expect_used)]
fn load(table: Table) -> TableSchema {
    let v: Value = serde_json::from_str(table.contract_json()).expect("vendored table json");
    let s = |v: &Value| v.as_str().expect("string").to_string();
    TableSchema {
        name: s(&v["table"]),
        contract_ver: v["contract_ver"].as_i64().expect("contract_ver"),
        columns: v["columns"]
            .as_array()
            .expect("columns")
            .iter()
            .map(|c| Column {
                name: s(&c["name"]),
                ty: ColType::parse(c["type"].as_str().expect("type")).expect("known type"),
                nullable: c["nullable"].as_bool().expect("nullable"),
            })
            .collect(),
        metadata: v["metadata"]
            .as_array()
            .expect("metadata")
            .iter()
            .map(s)
            .collect(),
    }
}
