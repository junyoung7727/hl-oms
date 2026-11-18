//! The market-data collector (design §6.2, unit U5b): one process per venue
//! that writes every received frame to the lake's stage-1 files, and the
//! `md-lake` commands that seal and verify them.

pub mod config;
pub mod plan;
pub mod registry;
