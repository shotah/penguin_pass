//! Structured workouts for Penguin Pass.
//!
//! [`Workout`] is a plan: warm-up, steady blocks, ramps, repeats, and open
//! sections, with power stored as a fraction of FTP. [`WorkoutRun`] walks
//! that plan from power samples. [`ErgGuide`] turns the current target into
//! the watts to send to a trainer. This crate does not draw, open a socket,
//! or talk to hardware.
//!
//! FTP is the rider's setting, ftp_w on the session settings. Zones are the
//! power_zone function in pp-session, the same one the ride console uses.
//! This crate does not keep a second zone table. See docs/workouts.md.
//!
//! ```
//! use pp_workout::{Library, WorkoutRun};
//!
//! let workout = Library::builtin()
//!     .workouts
//!     .into_iter()
//!     .find(|workout| workout.id == "endurance-20")
//!     .expect("built-in endurance ride");
//! let mut run = WorkoutRun::new(&workout, 200.0);
//! let events = run.tick(1.0, 130.0, Some(85.0));
//! assert!(!events.is_empty());
//! assert!(run.target_w().is_some());
//! ```

#![warn(missing_docs)]
#![forbid(unsafe_code)]

mod builtin;
mod erg;
mod library;
mod model;
mod run;
mod zwo;

pub use erg::{
    ERG_EASE_S, ERG_JUMP_W, ERG_MAX_W, ERG_MIN_INTERVAL_S, ERG_STOP_S, ErgCommand, ErgGuide,
};
pub use library::{Library, LoadWarning};
pub use model::{
    Cue, Intensity, IntervalStep, OpenStep, ProfileBar, RampStep, SteadyStep, Step, Workout,
};
pub use run::{
    BIAS_LIMIT, CUE_HOLD_S, ENDING_SOON_S, RAMP_TEST_FACTOR, RANGE_FLOOR_W, RANGE_FRACTION,
    RANGE_GRACE_S, StepResult, WorkoutEvent, WorkoutResult, WorkoutRun, WorkoutView,
};
pub use zwo::{ParsedZwo, ZwoError, parse_zwo};
