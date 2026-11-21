//! Ride orchestration for Penguin Pass.
//!
//! [`Session`] owns the course [`pp_course::Ride`], the active trainer, the
//! optional TV dashboard, and a copy of [`Settings`]. Godot calls
//! [`Session::tick`] once per frame. That call does not wait on Bluetooth or
//! the network.
//!
//! ```
//! use pp_session::{ActiveSource, Session, Settings};
//!
//! let settings = Settings {
//!     tv_dashboard_enabled: false,
//!     ..Settings::default()
//! };
//! let mut session = Session::new(settings);
//! assert!(matches!(session.status().source, ActiveSource::Simulated));
//! let frame = session.tick(0.0);
//! assert_eq!(frame.elapsed_s, 0.0);
//! ```

#![warn(missing_docs)]
#![forbid(unsafe_code)]

mod connection;
mod events;
mod link;
mod readout;
mod session;
mod settings;
mod signal;
mod stats;
mod zones;

pub use connection::{ConnectionStatus, LinkPhase, ResistanceMode, plain_failure};
pub use events::{EVENT_QUEUE_CAP, RideEvent};
pub use link::{BleTrainerFactory, TrainerFactory, TrainerLink};
pub use pp_core::{RiderProfile, Telemetry};
pub use pp_course::Course;
pub use pp_trainer::{ConnectionState, DiscoveredTrainer, SessionMode, Trainer, TrainerError};
pub use readout::{LiveReadout, MAX_POWER_WINDOW_S, POWER_AVERAGE_WINDOW_S};
pub use session::{
    ActiveSource, AheadPoint, CourseFacts, DASHBOARD_PROFILE_STEP_M, DashboardRun, PAIRING_HOLD_S,
    PairingEntry, SIM_PEDAL_CADENCE_RPM, SIM_POWER_MAX_W, SIM_POWER_STEP_W, Session,
    SessionOptions, SessionStatus,
};
pub use settings::{
    DEFAULT_BIKE_MASS_KG, DEFAULT_DASHBOARD_PORT, DEFAULT_FTP_W, DEFAULT_RIDER_MASS_KG,
    DEFAULT_TRAINER_DIFFICULTY, LoadedSettings, MAX_BIKE_MASS_KG, MAX_DASHBOARD_PORT, MAX_FTP_W,
    MAX_RIDER_MASS_KG, MAX_TRAINER_DIFFICULTY, MIN_BIKE_MASS_KG, MIN_DASHBOARD_PORT, MIN_FTP_W,
    MIN_RIDER_MASS_KG, MIN_TRAINER_DIFFICULTY, Settings, SettingsError,
};
pub use signal::{
    SIGNAL_FAIR_DBM, SIGNAL_GOOD_DBM, SIGNAL_STRONG_DBM, SIGNAL_WEAK_DBM, signal_bars, signal_word,
};
pub use stats::{LapRecord, MOVING_SPEED_MPS, RideSummary};
pub use zones::{PowerZone, power_zone};
