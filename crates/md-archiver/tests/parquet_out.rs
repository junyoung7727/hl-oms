//! The schema-driven Parquet writer (spec D6): the file read back has the
//! contract's columns, types and metadata, and equal rows give equal bytes.

mod common;

use md_archiver::parquet_out::{Cell, FileWriter, WriteSettings};
use md_archiver::table::{ColType, Table};
use parquet::basic::{LogicalType, Repetition, Type as Physical};
use parquet::file::reader::{FileReader, SerializedFileReader};
use serde_json::{Value, json};

fn read(path: &std::path::Path) -> SerializedFileReader<std::fs::File> {
    SerializedFileReader::new(std::fs::File::open(path).expect("open")).expect("parquet")
}

/// The physical and logical type the contract type maps to (spec D6).
fn expected(ty: ColType) -> (Physical, Option<LogicalType>) {
    let int = |bit_width| Some(LogicalType::integer(bit_width, true));
    match ty {
        ColType::String => (Physical::BYTE_ARRAY, Some(LogicalType::String)),
        ColType::Bool => (Physical::BOOLEAN, None),
        ColType::Int8 => (Physical::INT32, int(8)),
        ColType::Int16 => (Physical::INT32, int(16)),
        ColType::Int32 => (Physical::INT32, None),
        ColType::Int64 => (Physical::INT64, None),
        ColType::ListInt64 => (Physical::INT64, None),
    }
}

/// One row of `table`: every column set, lists non-empty, nullable columns
/// null when `nulls`.
fn sample_row(table: Table, nulls: bool, n: i64) -> Vec<Cell<'static>> {
    table
        .schema()
        .columns
        .iter()
        .map(|c| match (c.ty, nulls && c.nullable) {
            (_, true) => Cell::Null,
            (ColType::String, _) => Cell::Str(if n % 2 == 0 { "BTCUSDT" } else { "ETHUSDT" }),
            (ColType::Bool, _) => Cell::Bool(n % 2 == 0),
            (ColType::ListInt64, _) => Cell::List(if n % 3 == 0 {
                Vec::new()
            } else {
                vec![n, n + 1, -n]
            }),
            (ColType::Int8, _) => Cell::Int(n % 100 - 50),
            (ColType::Int16, _) => Cell::Int(n % 1000),
            (ColType::Int32, _) => Cell::Int(n),
            (ColType::Int64, _) => Cell::Int(1_767_312_000_000_000_000 + n),
        })
        .collect()
}

fn write(table: Table, rows: &[Vec<Cell>], settings: WriteSettings, path: &std::path::Path) {
    let mut w = FileWriter::create(path, table, "bybit-1", settings).expect("create");
    for r in rows {
        w.push(r).expect("push");
    }
    let extra = match table {
        Table::BookMsg => vec![
            ("first_update_id".to_string(), "100".to_string()),
            ("last_update_id".to_string(), String::new()),
        ],
        _ => Vec::new(),
    };
    w.finish(extra).expect("finish");
}

#[test]
fn schema_read_back_equals_contract() {
    let dir = common::test_dir("schema_read_back");
    for table in Table::ALL {
        let contract: Value = serde_json::from_str(table.contract_json()).expect("json");
        for rows in [0, 1] {
            let path = dir.join(format!("{}-{rows}.parquet", table.name()));
            let data: Vec<_> = (0..rows).map(|n| sample_row(table, false, n)).collect();
            write(table, &data, WriteSettings::default(), &path);
            let r = read(&path);
            let meta = r.metadata().file_metadata();
            let fields = meta.schema().get_fields();
            let cols = contract["columns"].as_array().expect("columns");
            assert_eq!(fields.len(), cols.len(), "{}", table.name());
            for (f, c) in fields.iter().zip(cols) {
                assert_eq!(f.name(), c["name"].as_str().expect("name"));
                let nullable = c["nullable"].as_bool().expect("nullable");
                let info = f.get_basic_info();
                assert_eq!(
                    info.repetition(),
                    if nullable {
                        Repetition::OPTIONAL
                    } else {
                        Repetition::REQUIRED
                    },
                    "{}.{}",
                    table.name(),
                    f.name()
                );
                let ty = ColType::parse(c["type"].as_str().expect("type")).expect("known type");
                let (physical, logical) = expected(ty);
                if ty == ColType::ListInt64 {
                    assert_eq!(info.logical_type_ref(), Some(&LogicalType::List));
                    let list = &f.get_fields()[0];
                    assert_eq!(list.name(), "list");
                    assert_eq!(list.get_basic_info().repetition(), Repetition::REPEATED);
                    let element = &list.get_fields()[0];
                    assert_eq!(element.name(), "element");
                    assert_eq!(
                        element.get_basic_info().repetition(),
                        Repetition::REQUIRED,
                        "list elements are never null"
                    );
                    assert_eq!(element.get_physical_type(), physical);
                } else {
                    assert_eq!(f.get_physical_type(), physical, "{}", f.name());
                    assert_eq!(info.logical_type_ref(), logical.as_ref(), "{}", f.name());
                }
            }
            let kv: Vec<(String, String)> = meta
                .key_value_metadata()
                .expect("metadata")
                .iter()
                .map(|k| (k.key.clone(), k.value.clone().unwrap_or_default()))
                .collect();
            let keys: Vec<&str> = kv.iter().map(|(k, _)| k.as_str()).collect();
            let want: Vec<&str> = contract["metadata"]
                .as_array()
                .expect("metadata")
                .iter()
                .map(|v| v.as_str().expect("key"))
                .collect();
            assert_eq!(keys, want, "{}", table.name());
            let get = |k: &str| kv.iter().find(|(x, _)| x == k).map(|(_, v)| v.as_str());
            assert_eq!(get("contract_ver"), Some("1"));
            assert_eq!(get("parser_ver"), Some("bybit-1"));
            assert_eq!(get("table"), Some(table.name()));
            assert_eq!(r.metadata().file_metadata().num_rows(), rows);
        }
    }
}

#[test]
fn rows_read_back_equal() {
    let dir = common::test_dir("rows_read_back");
    for table in Table::ALL {
        let rows: Vec<_> = (0..7).map(|n| sample_row(table, n % 2 == 1, n)).collect();
        let path = dir.join(format!("{}.parquet", table.name()));
        write(table, &rows, WriteSettings::default(), &path);
        let back = common::read_rows(&path);
        assert_eq!(back.len(), rows.len());
        let schema = table.schema();
        for (got, want) in back.iter().zip(&rows) {
            for (c, cell) in schema.columns.iter().zip(want) {
                let w = match cell {
                    Cell::Null => Value::Null,
                    Cell::Str(s) => json!(s),
                    Cell::Bool(b) => json!(b),
                    Cell::Int(i) => json!(i),
                    Cell::List(v) => json!(v),
                };
                assert_eq!(got[&c.name], w, "{}.{}", table.name(), c.name);
            }
        }
    }
}

#[test]
fn writer_is_deterministic() {
    let dir = common::test_dir("deterministic");
    for table in Table::ALL {
        let rows: Vec<_> = (0..50).map(|n| sample_row(table, n % 5 == 0, n)).collect();
        let a = dir.join(format!("{}-a.parquet", table.name()));
        let b = dir.join(format!("{}-b.parquet", table.name()));
        write(table, &rows, WriteSettings::default(), &a);
        write(table, &rows, WriteSettings::default(), &b);
        assert_eq!(
            std::fs::read(&a).expect("a"),
            std::fs::read(&b).expect("b"),
            "{}",
            table.name()
        );
    }
}

#[test]
fn row_groups_are_cut() {
    let dir = common::test_dir("row_groups");
    let rows: Vec<_> = (0..1000)
        .map(|n| sample_row(Table::BookMsg, false, n))
        .collect();
    let path = dir.join("cut.parquet");
    write(
        Table::BookMsg,
        &rows,
        WriteSettings {
            row_group_bytes: 16 * 1024,
        },
        &path,
    );
    let r = read(&path);
    assert!(r.metadata().num_row_groups() > 3, "several row groups");
    let back = common::read_rows(&path);
    let want: Vec<i64> = (0..1000).map(|n| 1_767_312_000_000_000_000 + n).collect();
    let got: Vec<i64> = back
        .iter()
        .map(|r| r["recv_ns"].as_i64().expect("recv_ns"))
        .collect();
    assert_eq!(got, want, "rows keep their order across row groups");
}

#[test]
fn a_cell_of_the_wrong_type_is_refused() {
    let dir = common::test_dir("wrong_type");
    let mut w = FileWriter::create(
        &dir.join("bad.parquet"),
        Table::Gap,
        "bybit-1",
        WriteSettings::default(),
    )
    .expect("create");
    let mut row = sample_row(Table::Gap, false, 1);
    row[0] = Cell::Int(1);
    assert!(w.push(&row).is_err(), "an integer for a string column");
    let mut row = sample_row(Table::Gap, false, 1);
    row.pop();
    assert!(w.push(&row).is_err(), "a short row");
    let mut row = sample_row(Table::Instrument, false, 1);
    row[0] = Cell::Null;
    let mut w = FileWriter::create(
        &dir.join("bad2.parquet"),
        Table::Instrument,
        "bybit-1",
        WriteSettings::default(),
    )
    .expect("create");
    assert!(w.push(&row).is_err(), "null in a required column");
    let mut row = sample_row(Table::BookMsg, false, 1);
    let kind = Table::BookMsg
        .schema()
        .columns
        .iter()
        .position(|c| c.name == "kind")
        .expect("kind");
    row[kind] = Cell::Int(300);
    let mut w = FileWriter::create(
        &dir.join("bad3.parquet"),
        Table::BookMsg,
        "bybit-1",
        WriteSettings::default(),
    )
    .expect("create");
    assert!(w.push(&row).is_err(), "300 does not fit int8");
}
