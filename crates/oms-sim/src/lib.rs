//! Deterministic simulation of the OMS core against a simulated venue.
//!
//! See `world.rs` for what is real and what is modelled, and
//! docs/verification/simulation.md for the properties checked and their
//! limits.

pub mod rng;
pub mod venue;
pub mod world;

pub use venue::SimVenue;
pub use world::{Profile, SimStats, Violation, World, run};
