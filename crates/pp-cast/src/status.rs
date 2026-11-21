//! Ride status published beside each telemetry snapshot.
//!
//! The dashboard works before the first status arrives: power zones stay
//! uncoloured and the connection chip stays neutral.

use serde::{Deserialize, Serialize};

/// Where the watts on the ride are coming from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RideSource {
    /// No status has been published yet.
    #[default]
    Unknown,
    /// Keyboard rider. There is no trainer.
    Simulated,
    /// A paired trainer.
    Trainer,
}

/// Whether the game still has the trainer link.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkPhase {
    /// No status has been published yet.
    #[default]
    Unknown,
    /// The source is reporting.
    Connected,
    /// The game lost the trainer and is trying again.
    ///
    /// This is the trainer link, not the browser's event stream. The page
    /// shows them as different chips.
    Reconnecting,
    /// The game is looking for a trainer.
    Searching,
}

/// What the trainer is doing with resistance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResistanceMode {
    /// The game is sending grade or target power to the trainer.
    Controlled,
    /// The trainer only reports power.
    ReadingOnly,
}

/// Facts the dashboard needs that are not in a telemetry snapshot.
///
/// Construct it with struct update from [`Self::default`] when a field is
/// not known yet. `ftp_w: None` leaves the power number uncoloured.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RideStatus {
    /// Functional threshold power, in watts.
    ///
    /// `None` until the game sends one. Zones stay off while it is `None`.
    pub ftp_w: Option<u16>,
    /// Simulated rider or a named trainer. [`RideSource::Unknown`] until the
    /// first real status.
    pub source: RideSource,
    /// Trainer name when [`Self::source`] is [`RideSource::Trainer`].
    pub trainer_name: Option<String>,
    /// Trainer link, independent of the browser stream.
    pub link: LinkPhase,
    /// `None` for the simulated rider and until the game says which mode.
    pub resistance: Option<ResistanceMode>,
    /// Three-second average power, in watts.
    ///
    /// The hero number uses this when it is `Some`. `None` falls back to the
    /// instantaneous telemetry power, except while [`Self::link`] is
    /// [`LinkPhase::Reconnecting`], where a missing average is shown as unknown.
    pub power_3s_w: Option<f32>,
}

impl Default for RideStatus {
    /// No FTP, no trainer, and no average. The page stays readable.
    fn default() -> Self {
        Self {
            ftp_w: None,
            source: RideSource::Unknown,
            trainer_name: None,
            link: LinkPhase::Unknown,
            resistance: None,
            power_3s_w: None,
        }
    }
}
