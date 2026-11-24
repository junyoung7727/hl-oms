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

// Money and decision code: no float arithmetic, no `expect`, no `as` that
// can cut, wrap or lose a sign, no clock call (clippy.toml), and an
// exception only as `#[expect(lint, reason = "..")]` (ADR-033 part 3).
#![deny(
    clippy::float_arithmetic,
    clippy::expect_used,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    clippy::disallowed_methods,
    clippy::allow_attributes,
    clippy::allow_attributes_without_reason
)]
#![cfg_attr(test, expect(clippy::unwrap_used, reason = "a failed unwrap is a failed test"))]

pub mod config;
pub mod core;
mod decide;
pub mod event;
mod exposure;
pub mod ids;
pub mod input;
mod margin_view;
pub mod model;
pub mod needs;
pub mod num;
pub mod rate;
pub mod record;

pub use crate::core::{Core, CoreFault};
pub use config::*;
pub use event::*;
pub use exposure::Exposure;
pub use ids::*;
pub use input::*;
pub use margin_view::{AccountModeView, MarginView, VenueSettingView};
pub use model::*;
pub use needs::Need;
pub use num::{Amount, Px, Qty, Ratio, SQty};
pub use rate::{RateBudget, RateClass, Slot};
pub use record::Record;
