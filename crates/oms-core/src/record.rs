//! The journal record: one input together with the events it produced.
//!
//! Recovery applies `events` in order with record time `at` and ignores
//! `input`. The input is kept for audit and for the determinism check, which
//! re-decides the input on the pre-state and compares the result with the
//! recorded events.

use crate::core::{Core, CoreFault};
use crate::event::Event;
use crate::ids::Ts;
use crate::input::Input;
use serde::{Deserialize, Serialize};

/// Version of the record schema written by this build. A record with a
/// different version is refused by recovery rather than guessed at.
pub const RECORD_SCHEMA: u16 = 1;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    /// Position in the journal, starting at 1, without gaps.
    pub seq: u64,
    /// Time assigned by the sequencer, microseconds since the Unix epoch.
    pub at: Ts,
    pub input: Input,
    pub events: Vec<Event>,
}

impl Core {
    /// Applies a recorded transition. Produces no command and no reply.
    pub fn replay(&mut self, rec: &Record) -> Result<(), CoreFault> {
        for ev in &rec.events {
            self.apply(rec.at, ev)?;
        }
        Ok(())
    }
}
