//! One Parquet writer for every L1 table, driven by the table's contract
//! schema (spec D6). Rows are pushed as cells in the contract's column order;
//! a cell of the wrong type, a null in a required column or an integer that
//! does not fit its column is refused, never cast.
//!
//! The file holds no build time and no host name: the same rows give the
//! same bytes.

use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::Path;
use std::sync::Arc;

use parquet::basic::{Compression, LogicalType, Repetition, Type as Physical, ZstdLevel};
use parquet::data_type::{BoolType, ByteArray, ByteArrayType, Int32Type, Int64Type};
use parquet::errors::ParquetError;
use parquet::file::metadata::KeyValue;
use parquet::file::properties::WriterProperties;
use parquet::file::writer::SerializedFileWriter;
use parquet::schema::types::Type;

use crate::table::{ColType, Table, TableSchema};

/// One value of a row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cell<'a> {
    Null,
    Str(&'a str),
    Bool(bool),
    /// Any integer column; checked against the column's width.
    Int(i64),
    List(Vec<i64>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WriteSettings {
    /// A row group is closed when its buffered values reach about this many
    /// bytes, so that the memory of one open file is bounded (judgement).
    /// The count includes each string's heap block and the levels (final
    /// review F3). Up to 24 hour files of a channel are open at once.
    pub row_group_bytes: usize,
}

impl Default for WriteSettings {
    fn default() -> Self {
        Self {
            row_group_bytes: 16 * 1024 * 1024,
        }
    }
}

#[derive(Debug)]
pub enum WriteError {
    Io(io::Error),
    Parquet(ParquetError),
    /// A row that does not fit the schema: a defect of the caller.
    Row(String),
}

impl std::fmt::Display for WriteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WriteError::Io(e) => write!(f, "{e}"),
            WriteError::Parquet(e) => write!(f, "parquet: {e}"),
            WriteError::Row(e) => write!(f, "row: {e}"),
        }
    }
}

impl std::error::Error for WriteError {}

impl From<io::Error> for WriteError {
    fn from(e: io::Error) -> Self {
        WriteError::Io(e)
    }
}

impl From<ParquetError> for WriteError {
    fn from(e: ParquetError) -> Self {
        WriteError::Parquet(e)
    }
}

/// The buffered values of one column of the open row group.
enum Buf {
    Bytes(Vec<ByteArray>, Vec<i16>),
    Bool(Vec<bool>, Vec<i16>),
    I32(Vec<i32>, Vec<i16>),
    I64(Vec<i64>, Vec<i16>),
    /// Values, definition levels, repetition levels.
    List(Vec<i64>, Vec<i16>, Vec<i16>),
}

impl Buf {
    fn new(ty: ColType) -> Self {
        match ty {
            ColType::String => Buf::Bytes(Vec::new(), Vec::new()),
            ColType::Bool => Buf::Bool(Vec::new(), Vec::new()),
            ColType::Int8 | ColType::Int16 | ColType::Int32 => Buf::I32(Vec::new(), Vec::new()),
            ColType::Int64 => Buf::I64(Vec::new(), Vec::new()),
            ColType::ListInt64 => Buf::List(Vec::new(), Vec::new(), Vec::new()),
        }
    }
}

fn int_range(ty: ColType) -> Option<(i64, i64)> {
    match ty {
        ColType::Int8 => Some((i8::MIN.into(), i8::MAX.into())),
        ColType::Int16 => Some((i16::MIN.into(), i16::MAX.into())),
        ColType::Int32 => Some((i32::MIN.into(), i32::MAX.into())),
        ColType::Int64 => Some((i64::MIN, i64::MAX)),
        _ => None,
    }
}

/// The Parquet schema of a table (spec D6).
pub fn parquet_schema(schema: &TableSchema) -> Result<Type, ParquetError> {
    let mut fields = Vec::with_capacity(schema.columns.len());
    for c in &schema.columns {
        let rep = if c.nullable {
            Repetition::OPTIONAL
        } else {
            Repetition::REQUIRED
        };
        let int = |bit_width| Some(LogicalType::integer(bit_width, true));
        let prim = |physical, logical| {
            Type::primitive_type_builder(&c.name, physical)
                .with_repetition(rep)
                .with_logical_type(logical)
                .build()
        };
        let field = match c.ty {
            ColType::String => prim(Physical::BYTE_ARRAY, Some(LogicalType::String))?,
            ColType::Bool => prim(Physical::BOOLEAN, None)?,
            ColType::Int8 => prim(Physical::INT32, int(8))?,
            ColType::Int16 => prim(Physical::INT32, int(16))?,
            ColType::Int32 => prim(Physical::INT32, None)?,
            ColType::Int64 => prim(Physical::INT64, None)?,
            ColType::ListInt64 => {
                let element = Type::primitive_type_builder("element", Physical::INT64)
                    .with_repetition(Repetition::REQUIRED)
                    .build()?;
                let list = Type::group_type_builder("list")
                    .with_repetition(Repetition::REPEATED)
                    .with_fields(vec![Arc::new(element)])
                    .build()?;
                Type::group_type_builder(&c.name)
                    .with_repetition(rep)
                    .with_logical_type(Some(LogicalType::List))
                    .with_fields(vec![Arc::new(list)])
                    .build()?
            }
        };
        fields.push(Arc::new(field));
    }
    Type::group_type_builder("schema")
        .with_fields(fields)
        .build()
}

/// An open L1 file. Push rows in the file's sort order, then `finish`.
pub struct FileWriter {
    table: Table,
    schema: &'static TableSchema,
    writer: SerializedFileWriter<BufWriter<File>>,
    bufs: Vec<Buf>,
    rows: usize,
    bytes: usize,
    settings: WriteSettings,
}

impl FileWriter {
    /// Creates (truncates) `path`. The metadata `contract_ver`, `parser_ver`
    /// and `table` are set here; table-specific keys come with `finish`.
    pub fn create(
        path: &Path,
        table: Table,
        parser_ver: &str,
        settings: WriteSettings,
    ) -> Result<Self, WriteError> {
        let schema = table.schema();
        let props = WriterProperties::builder()
            .set_compression(Compression::ZSTD(ZstdLevel::try_new(9)?))
            .set_key_value_metadata(Some(vec![
                KeyValue::new("contract_ver".into(), schema.contract_ver.to_string()),
                KeyValue::new("parser_ver".into(), parser_ver.to_string()),
                KeyValue::new("table".into(), schema.name.clone()),
            ]))
            .build();
        let file = BufWriter::new(File::create(path)?);
        let writer =
            SerializedFileWriter::new(file, Arc::new(parquet_schema(schema)?), Arc::new(props))?;
        Ok(Self {
            table,
            schema,
            writer,
            bufs: schema.columns.iter().map(|c| Buf::new(c.ty)).collect(),
            rows: 0,
            bytes: 0,
            settings,
        })
    }

    pub fn table(&self) -> Table {
        self.table
    }

    /// Appends one row. On an error nothing of the row is buffered.
    pub fn push(&mut self, row: &[Cell]) -> Result<(), WriteError> {
        let cols = &self.schema.columns;
        if row.len() != cols.len() {
            return Err(WriteError::Row(format!(
                "{}: {} cells for {} columns",
                self.schema.name,
                row.len(),
                cols.len()
            )));
        }
        // Check every cell before buffering any, so a refused row leaves the
        // buffers in step.
        for (c, cell) in cols.iter().zip(row) {
            let ok = match (cell, c.ty) {
                (Cell::Null, _) => c.nullable,
                (Cell::Str(_), ColType::String) | (Cell::Bool(_), ColType::Bool) => true,
                (Cell::List(_), ColType::ListInt64) => true,
                (Cell::Int(i), ty) => int_range(ty).is_some_and(|(lo, hi)| lo <= *i && *i <= hi),
                _ => false,
            };
            if !ok {
                return Err(WriteError::Row(format!(
                    "{}.{}: {cell:?} does not fit {:?}{}",
                    self.schema.name,
                    c.name,
                    c.ty,
                    if c.nullable { " (nullable)" } else { "" }
                )));
            }
        }
        for ((c, cell), buf) in cols.iter().zip(row).zip(&mut self.bufs) {
            let def = i16::from(c.nullable);
            match (buf, cell) {
                (Buf::Bytes(_, d), Cell::Null)
                | (Buf::Bool(_, d), Cell::Null)
                | (Buf::I32(_, d), Cell::Null)
                | (Buf::I64(_, d), Cell::Null) => d.push(0),
                (Buf::List(_, d, r), Cell::Null) => {
                    d.push(0);
                    r.push(0);
                }
                (Buf::Bytes(v, d), Cell::Str(s)) => {
                    // The string, its ByteArray (24) and heap block header, a level.
                    self.bytes += s.len() + 24 + 16 + 2;
                    v.push(ByteArray::from(s.as_bytes().to_vec()));
                    d.push(def);
                }
                (Buf::Bool(v, d), Cell::Bool(b)) => {
                    self.bytes += 3;
                    v.push(*b);
                    d.push(def);
                }
                (Buf::I32(v, d), Cell::Int(i)) => {
                    self.bytes += 6;
                    // Range checked above.
                    v.push(*i as i32);
                    d.push(def);
                }
                (Buf::I64(v, d), Cell::Int(i)) => {
                    self.bytes += 10;
                    v.push(*i);
                    d.push(def);
                }
                (Buf::List(v, d, r), Cell::List(items)) => {
                    // Levels of a three-level list: def = (nullable) + 1 for
                    // an element; an empty list has one entry one level less.
                    if items.is_empty() {
                        d.push(def);
                        r.push(0);
                    } else {
                        for (k, x) in items.iter().enumerate() {
                            v.push(*x);
                            d.push(def + 1);
                            r.push(i16::from(k > 0));
                        }
                    }
                    self.bytes += 12 * items.len().max(1);
                }
                _ => unreachable!("cells are checked above"),
            }
        }
        self.rows += 1;
        if self.bytes >= self.settings.row_group_bytes {
            self.flush_row_group()?;
        }
        Ok(())
    }

    fn flush_row_group(&mut self) -> Result<(), WriteError> {
        if self.rows == 0 {
            return Ok(());
        }
        let mut rg = self.writer.next_row_group()?;
        for (c, buf) in self.schema.columns.iter().zip(&mut self.bufs) {
            let mut col = rg.next_column()?.ok_or_else(|| {
                WriteError::Row(format!(
                    "{}: no column writer for {}",
                    self.schema.name, c.name
                ))
            })?;
            let nullable = c.nullable;
            fn defs(nullable: bool, d: &[i16]) -> Option<&[i16]> {
                if nullable { Some(d) } else { None }
            }
            match buf {
                Buf::Bytes(v, d) => {
                    col.typed::<ByteArrayType>()
                        .write_batch(v, defs(nullable, d), None)?;
                }
                Buf::Bool(v, d) => {
                    col.typed::<BoolType>()
                        .write_batch(v, defs(nullable, d), None)?;
                }
                Buf::I32(v, d) => {
                    col.typed::<Int32Type>()
                        .write_batch(v, defs(nullable, d), None)?;
                }
                Buf::I64(v, d) => {
                    col.typed::<Int64Type>()
                        .write_batch(v, defs(nullable, d), None)?;
                }
                Buf::List(v, d, r) => {
                    col.typed::<Int64Type>().write_batch(v, Some(d), Some(r))?;
                }
            }
            col.close()?;
            *buf = Buf::new(c.ty);
        }
        rg.close()?;
        self.rows = 0;
        self.bytes = 0;
        Ok(())
    }

    /// Writes the last row group, the table-specific metadata (for
    /// `book_msg`: `first_update_id`, `last_update_id`) and the footer, and
    /// fsyncs the file.
    pub fn finish(mut self, extra: Vec<(String, String)>) -> Result<(), WriteError> {
        self.flush_row_group()?;
        for (k, v) in extra {
            self.writer.append_key_value_metadata(KeyValue::new(k, v));
        }
        let mut out = self.writer.into_inner()?;
        out.flush()?;
        out.get_ref().sync_all()?;
        Ok(())
    }
}
