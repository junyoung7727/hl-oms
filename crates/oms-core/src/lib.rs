//! Deterministic OMS domain core.
//!
//! This crate owns order, risk and reconciliation state and every decision
//! about it. It has no dependency that can reach a network, a file system, a
//! clock or a random source; the only dependencies are `serde` and
//! `serde_json`. Time, configuration and venue information arrive as inputs.
//!
//! Entry points:
//! - [`Core::handle`] decides the events for one input and applies them.
//! - [`Core::apply`] applies one already-decided event (recovery).
//! - [`Core::needs`] lists the venue evidence the state is waiting for.

#![cfg_attr(test, allow(clippy::unwrap_used))]

pub mod config;
pub mod core;
mod decide;
pub mod event;
pub mod ids;
pub mod input;
pub mod model;
pub mod needs;
pub mod num;
pub mod record;

pub use crate::core::{Core, CoreFault};
pub use config::*;
pub use event::*;
pub use ids::*;
pub use input::*;
pub use model::*;
pub use needs::Need;
pub use num::{Amount, Px, Qty, Ratio, SQty};
pub use record::Record;
