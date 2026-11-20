//! Trainers behind one non-blocking interface.
//!
//! The game calls [`Trainer::latest_sample`] and [`Trainer::send`] from its
//! frame loop. Neither call waits on Bluetooth. [`SimTrainer`] is the keyboard
//! rider. [`BleTrainer`] talks to a Fitness Machine Service trainer on BlueZ
//! from a background thread.
//!
//! A sample is fresh while its age is less than or equal to [`SAMPLE_STALENESS`]
//! (3 seconds). Older readings are `None` so a dropped link does not leave the
//! last wattage on the HUD.

#![forbid(unsafe_code)]

mod ble;
mod config;
mod error;
mod session;
mod sim;

pub use ble::{BleTrainer, DiscoveredTrainer};
pub use config::{SessionMode, TrainerConfig};
pub use error::TrainerError;
pub use sim::SimTrainer;

use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use pp_core::{TrainerCommand, TrainerSample};

/// How long a trainer reading stays visible after it arrives.
///
/// Indoor bikes notify about once a second. Three seconds keeps a single
/// skipped packet on screen and then goes blank.
pub const SAMPLE_STALENESS: Duration = Duration::from_secs(3);

/// Where a trainer is in its connection lifecycle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionState {
    /// No link, and no reconnect is scheduled by the caller.
    ///
    /// [`BleTrainer`] also uses this after the link drops, before the backoff
    /// elapses and the state moves back to [`Self::Connecting`].
    Disconnected,
    /// Looking for peripherals that advertise the Fitness Machine Service.
    Scanning,
    /// A connection attempt is in progress, including the FTMS control handshake.
    Connecting,
    /// Measurements are arriving and, in control mode, commands can be queued.
    Connected,
    /// The last attempt failed. The string is a short reason for the log.
    Failed(String),
}

impl std::fmt::Display for ConnectionState {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Disconnected => write!(formatter, "disconnected"),
            Self::Scanning => write!(formatter, "scanning"),
            Self::Connecting => write!(formatter, "connecting"),
            Self::Connected => write!(formatter, "connected"),
            Self::Failed(reason) => write!(formatter, "failed: {reason}"),
        }
    }
}

/// What the frame loop needs from a trainer.
///
/// Methods return promptly. They do not wait on a radio, a socket, or a timer.
pub trait Trainer: Send {
    /// The newest reading, or `None` when nothing has arrived within [`SAMPLE_STALENESS`].
    fn latest_sample(&self) -> Option<TrainerSample>;

    /// Queue a grade or a power target.
    ///
    /// `Ok` means the command was accepted. A Bluetooth trainer may still
    /// coalesce it: grade writes go out only when the grade has changed by at
    /// least 0.1 percentage points, and at most about twice a second. The
    /// newest command wins. `Err` means it was not queued.
    fn send(&mut self, command: TrainerCommand) -> Result<(), TrainerError>;

    /// The current lifecycle state.
    fn state(&self) -> ConnectionState;

    /// A name for logs and the trainer picker.
    fn name(&self) -> String;

    /// The latest non-fatal control-point problem, such as a rejected grade.
    ///
    /// `None` when the session has not recorded one. A fatal failure is also
    /// visible on [`Trainer::state`].
    fn last_warning(&self) -> Option<String> {
        None
    }
}

pub(crate) fn sample_is_fresh(age: Duration, staleness: Duration) -> bool {
    age <= staleness
}

pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|err| err.into_inner())
}
