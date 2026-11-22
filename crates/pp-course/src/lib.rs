//! Course geometry and longitudinal bike physics for Penguin Pass.
//!
//! [`Course`] is a closed road centerline in meters, right-handed, Y up.
//! [`Ride`] integrates trainer power along that centerline. This crate does
//! not depend on Godot or Bluetooth.
//!
//! ```
//! use pp_core::{RiderProfile, TrainerSample};
//! use pp_course::{Course, Ride};
//!
//! let mut ride = Ride::new(Course::penguin_pass(), RiderProfile::default());
//! let sample = TrainerSample {
//!     power_w: Some(200),
//!     cadence_rpm: Some(85.0),
//!     heart_rate_bpm: Some(140),
//!     ..TrainerSample::default()
//! };
//! let frame = ride.step(1.0 / 60.0, &sample);
//! let _position = ride.course().position_at(frame.lap_distance_m);
//! assert!(frame.speed_mps >= 0.0);
//! ```

#![warn(missing_docs)]
#![forbid(unsafe_code)]

mod course;
mod physics;
mod ride;

pub use course::{Course, CourseError, CourseMeta, ProfilePoint, is_route_id};
pub use physics::{AIR_DENSITY_KG_PER_M3, GRAVITY_MPS2, LOW_SPEED_FLOOR_MPS, acceleration_mps2};
pub use ride::{MAX_DT_S, MAX_SPEED_MPS, MAX_STEP_S, Ride};
