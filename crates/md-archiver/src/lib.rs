//! md-archiver: the L1 Parquet tables of the lake contract (section 3), built
//! from sealed L0 files with the same venue parser as the live path (U7).

pub mod build;
pub mod commit;
pub mod input;
pub mod parquet_out;
pub mod registry;
pub mod retain;
pub mod ro;
pub mod rows;
pub mod run;
pub mod table;

/// The parser version on every L1 row and file (lake contract 6). Raise it
/// when the same L0 bytes would give other L1 rows; then rebuild.
pub const PARSER_VER_BYBIT: &str = "bybit-1";
