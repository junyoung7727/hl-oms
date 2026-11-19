//! Test helpers. Data directories are under `OMS_TEST_DIR`, else
//! `CARGO_TARGET_TMPDIR` (as md-lake): never `std::env::temp_dir()`.
#![allow(dead_code)]

pub mod lake;

use std::fs;
use std::path::{Path, PathBuf};

use parquet::file::reader::{FileReader, SerializedFileReader};
use parquet::record::{Field, ListAccessor};
use serde_json::{Map, Value};

pub fn golden_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../contracts/lake/golden")
}

/// A fresh directory for one test.
pub fn test_dir(name: &str) -> PathBuf {
    let base = std::env::var("OMS_TEST_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_TARGET_TMPDIR")));
    let dir = base
        .join("md-archiver")
        .join(format!("{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create test dir");
    dir
}

/// The rows of a Parquet file as JSON objects, in file order.
pub fn read_rows(path: &Path) -> Vec<Map<String, Value>> {
    let r = SerializedFileReader::new(fs::File::open(path).expect("open")).expect("parquet");
    r.get_row_iter(None)
        .expect("rows")
        .map(|row| {
            let row = row.expect("row");
            row.get_column_iter()
                .map(|(name, field)| (name.clone(), json(field)))
                .collect()
        })
        .collect()
}

/// The JSON of a field of the L1 column types (the `json` feature of
/// `parquet` is not built).
fn json(f: &Field) -> Value {
    match f {
        Field::Null => Value::Null,
        Field::Bool(b) => Value::from(*b),
        Field::Byte(i) => Value::from(*i),
        Field::Short(i) => Value::from(*i),
        Field::Int(i) => Value::from(*i),
        Field::Long(i) => Value::from(*i),
        Field::Str(s) => Value::from(s.as_str()),
        Field::ListInternal(l) => Value::Array(
            (0..l.len())
                .map(|i| Value::from(l.get_long(i).expect("list<int64> element")))
                .collect(),
        ),
        other => panic!("not an L1 column type: {other:?}"),
    }
}
