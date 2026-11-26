//! The fake Bybit parts shared by the node's process tests: the fake
//! WebSockets ([`ws`]) and the readers of the journal files on disk
//! ([`journal`]). A directory without `main.rs`, so not a test target of
//! its own; a test target takes it with `mod bybit_fake;` and may use a
//! part only.

#![allow(dead_code)]

pub mod journal;
pub mod ws;
