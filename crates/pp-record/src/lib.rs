//! Ride recording for Penguin Pass.
//!
//! [`Recorder`] turns a stream of ride samples into one FIT activity.
//! The game calls [`Recorder::push`] every frame. The recorder keeps one
//! record per second. [`Activity::write`] puts a `.fit` file in a directory
//! the caller chooses.
//!
//! The file is an indoor virtual ride: sport cycling, sub-sport virtual
//! activity. It has no GPS position. Training sites show the ride without
//! a map until a course has real coordinates.
//!
//! ```
//! use pp_record::{Recorder, RideMeta, RideSample};
//! use std::time::{Duration, SystemTime};
//!
//! let meta = RideMeta {
//!     started_at: SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000),
//!     course_name: "Penguin Pass".to_owned(),
//!     rider_mass_kg: 75.0,
//!     bike_mass_kg: 9.0,
//!     ftp_w: 200,
//!     trainer_name: None,
//!     game_version: "0.1.0".to_owned(),
//!     utc_offset_s: 0,
//! };
//! let mut recorder = Recorder::new(meta);
//! recorder.push(RideSample {
//!     elapsed_s: 0.0,
//!     power_w: Some(180.0),
//!     cadence_rpm: Some(85.0),
//!     heart_rate_bpm: None,
//!     speed_mps: 5.0,
//!     distance_m: 0.0,
//!     altitude_m: 0.0,
//!     grade_pct: 0.0,
//!     lap: 1,
//! });
//! let activity = recorder.finish();
//! assert_eq!(activity.records.len(), 1);
//! ```

#![warn(missing_docs)]
#![forbid(unsafe_code)]

mod fit;
mod recorder;

pub use recorder::{
    Activity, ActivitySummary, LapSummary, MAX_MOVING_GAP_S, MIN_WRITTEN_DISTANCE_M,
    MIN_WRITTEN_ELAPSED_S, MOVING_SPEED_MPS, NORMALIZED_POWER_WINDOW_S, RecordedSecond, Recorder,
    RideMeta, RideSample, SkipReason, WriteOutcome,
};
