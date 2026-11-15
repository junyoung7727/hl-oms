//! The OMS process: everything around the deterministic core.
//!
//! | module      | responsibility                                               |
//! |-------------|--------------------------------------------------------------|
//! | `queues`    | bounded input lanes in front of the core                     |
//! | `pipeline`  | core thread (decide) and commit stage (persist, then release) |
//! | `recovery`  | snapshot + journal replay at startup; offline verification   |
//! | `published` | read model built from durable records; subscriber fan-out    |
//! | `server`    | local socket API for strategies and operators                |
//! | `venue`     | gateways and the reconciler                                  |
//! | `status`    | operational status document                                  |
//! | `http`      | read-only dashboard over loopback HTTP                       |
//! | `node`      | wiring, startup and shutdown                                 |

#![cfg_attr(test, allow(clippy::unwrap_used))]

pub mod config;
pub mod http;
pub mod node;
pub mod pipeline;
pub mod published;
pub mod queues;
pub mod recovery;
pub mod server;
pub mod status;
pub mod util;
pub mod venue;

/// Digest of the workspace sources at build time (see build.rs).
pub const SOURCE_DIGEST: &str = env!("OMS_SOURCE_DIGEST");
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
