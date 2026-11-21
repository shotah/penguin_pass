//! Timed segments, personal records, and a best-lap ghost for Penguin Pass.
//!
//! [`detect`] finds the lap, the sustained climbs, and one sprint on a
//! [`Course`](pp_course::Course). [`SegmentTimer`] times them from the ride
//! clock. [`Records`] stores the best efforts and the ghost together.
//! [`GhostRecorder`] keeps the fastest first lap from a standing start.
//!
//! The crate does not link Godot, Bluetooth, or the network. See
//! `docs/segments.md` for the calls the session makes.
//!
//! ```
//! use pp_course::Course;
//! use pp_segments::{SegmentKind, detect};
//!
//! let segments = detect(&Course::penguin_pass());
//! assert!(segments.iter().any(|segment| segment.kind == SegmentKind::Lap));
//! ```

#![warn(missing_docs)]
#![forbid(unsafe_code)]

mod feed;
mod ghost;
mod records;
mod segment;
mod timer;

pub use feed::MAX_TICK_DISTANCE_M;
pub use ghost::{GhostGap, GhostLap, GhostRecorder, GhostTrace, MAX_GHOST_S, STANDING_START_M};
pub use records::{Records, RecordsError};
pub use segment::{Segment, SegmentKind, detect};
pub use timer::{SegmentEffort, SegmentEvent, SegmentProgress, SegmentTimer};
