//! L0 lake files (lake contract §2): QRAW1, the collector's open files and
//! session files, the hour seal and the day seal, and verify.
//!
//! No venue knowledge: the seal takes the venue's topic classifier as a
//! function. Paths are relative to a lake root.

pub mod dayseal;
pub mod fs;
pub mod paths;
pub mod qraw;
pub mod read;
pub mod seal;
pub mod time;
pub mod writer;
pub mod zst;
