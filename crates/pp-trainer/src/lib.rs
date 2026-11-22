//! Trainers behind one non-blocking interface.
//!
//! The game calls [`Trainer::latest_sample`] and [`Trainer::send`] from its
//! frame loop. Neither call waits on Bluetooth. [`SimTrainer`] is the keyboard
//! rider. [`BleTrainer`] talks to a trainer on BlueZ from a background thread,
//! using FTMS when the trainer has it, then Tacx FE-C, Wahoo's legacy
//! trainer control, or the Zwift trainer service.
//!
//! A sample is fresh while its age is less than or equal to [`SAMPLE_STALENESS`]
//! (3 seconds). Older readings are `None` so a dropped link does not leave the
//! last wattage on the HUD.

#![forbid(unsafe_code)]

mod ble;
mod config;
mod error;
mod heart;
mod heart_ble;
mod heart_sim;
mod session;
mod sim;
mod trace;

pub use ble::{BleTrainer, DiscoveredTrainer};
pub use config::{SessionMode, TrainerConfig};
pub use error::TrainerError;
pub use heart_sim::SimHeartRate;
pub use sim::SimTrainer;

use std::sync::atomic::{AtomicBool, Ordering};
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
    /// Looking for trainers. A device is listed when it advertises FTMS,
    /// Cycling Power, or the Tacx FE-C service. A Zwift Play, Click, or Ride
    /// controller advertises `0xFC82` without those and is not listed.
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

    /// True when the connected trainer exposes the Zwift `0xFC82` write
    /// characteristic for virtual gear-ratio updates.
    fn supports_virtual_shifting(&self) -> bool {
        false
    }

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

/// A heart-rate strap seen while scanning.
///
/// A trainer that also advertises the Heart Rate Service can appear here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredSensor {
    /// Bluetooth address, for example `AA:BB:CC:DD:EE:FF`.
    pub id: String,
    /// Advertised name, or the address when the peripheral has none.
    pub name: String,
    /// Received signal strength, in dBm, when the adapter reported it.
    pub rssi_dbm: Option<i16>,
}

/// A read-only heart-rate strap.
///
/// The link never writes a measurement or a control point. The only radio
/// write is enabling notifications on Heart Rate Measurement. It is independent
/// of [`Trainer`]: either connection can drop and reconnect on its own.
///
/// [`Self::bpm`] is `None` when nothing has arrived within [`SAMPLE_STALENESS`],
/// and when the strap reports that it supports skin contact and none is detected.
pub trait HeartRateLink: Send {
    /// The current lifecycle state. [`ConnectionState::Scanning`] means the
    /// strap list is being filled.
    fn state(&self) -> ConnectionState;

    /// Straps advertising the Heart Rate Service (0x180D). Does not block.
    fn discovered(&self) -> Vec<DiscoveredSensor>;

    /// Connect to an address or a case-insensitive name substring.
    ///
    /// An empty query or an unknown strap becomes [`ConnectionState::Failed`].
    /// The call returns when the request is queued.
    fn connect(&mut self, id_or_name: &str);

    /// Drop the strap and do not reconnect.
    fn disconnect(&mut self);

    /// The latest heart rate, in beats per minute.
    ///
    /// `None` when the link is down, the sample is older than [`SAMPLE_STALENESS`],
    /// or the strap reports no skin contact.
    fn bpm(&self) -> Option<u16>;

    /// The connected strap's name.
    fn name(&self) -> Option<String>;
}

/// Start a Bluetooth heart-rate scan on its own BlueZ session.
///
/// The trainer link is left alone. A missing adapter shows up later as
/// [`ConnectionState::Failed`] on the returned link.
///
/// # Errors
///
/// Returns an error when the background thread cannot be started.
pub fn spawn_ble_heart_rate() -> Result<Box<dyn HeartRateLink>, String> {
    heart_ble::BleHeartRate::spawn().map(|link| Box::new(link) as Box<dyn HeartRateLink>)
}

pub(crate) fn sample_is_fresh(age: Duration, staleness: Duration) -> bool {
    age <= staleness
}

pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|err| err.into_inner())
}

static BLUETOOTH_SETUP: AtomicBool = AtomicBool::new(false);

/// One BlueZ session may open at a time.
///
/// Opening two D-Bus connections in the same instant makes one of them fail
/// with "Remote peer disconnected". The trainer and the heart-rate strap each
/// keep their own session after this returns.
pub(crate) struct BluetoothSetup {
    _private: (),
}

impl Drop for BluetoothSetup {
    fn drop(&mut self) {
        BLUETOOTH_SETUP.store(false, Ordering::Release);
    }
}

pub(crate) async fn enter_bluetooth_setup() -> BluetoothSetup {
    while BLUETOOTH_SETUP
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    BluetoothSetup { _private: () }
}
