//! The fake Bybit parts shared by the node's process tests: the fake
//! WebSockets ([`ws`]), the readers of the journal files on disk
//! ([`journal`]), and the stateful fake account ([`book`], sans-IO) with
//! its servers ([`venue`]). A directory without `main.rs`, so not a test
//! target of its own; a test target takes it with `mod bybit_fake;` and may
//! use a part only.

#![allow(dead_code)]

pub mod book;
pub mod journal;
pub mod venue;
pub mod ws;
