//! Ride status published beside each telemetry snapshot.
//!
//! The dashboard works before the first status arrives: power zones stay
//! uncoloured and the connection chip stays neutral. Workout, segment,
//! draft, fish, powerup, and reverse are optional. Left at
//! [`RideStatus::default`], the page draws nothing for them.

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

/// One block of a structured workout, when a plan is running.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkoutStatus {
    /// Plan name, for example `Sweet spot`.
    pub name: String,
    /// Current block, for example `Interval 2 of 4`.
    pub block_label: String,
    /// Target power, in watts.
    pub target_w: u16,
    /// Seconds left in this block.
    pub remaining_s: u32,
    /// Overall workout progress, from 0.0 to 1.0.
    pub progress: f32,
    /// The trainer is holding [`Self::target_w`].
    pub erg_active: bool,
}

/// A timed course segment that is open now.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SegmentStatus {
    /// Segment name, for example `Summit climb`.
    pub name: String,
    /// Seconds since the segment opened.
    pub elapsed_s: f32,
    /// Progress through the segment, from 0.0 to 1.0.
    pub progress: f32,
    /// Seconds versus the personal best.
    ///
    /// Negative is ahead. Positive is behind. `None` is the first try.
    pub delta_s: Option<f32>,
}

/// A powerup that is currently changing the bike.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PowerupStatus {
    /// Short name, for example `Feather`, `Aero`, or `Burst`.
    pub name: String,
    /// Seconds left on the effect.
    pub remaining_s: f32,
}

/// Facts the dashboard needs that are not in a telemetry snapshot.
///
/// Construct it with struct update from [`Self::default`] when a field is
/// not known yet. `ftp_w: None` leaves the power number uncoloured.
/// The workout, segment, draft, fish, and powerup fields default to empty,
/// so a payload from before those fields still deserializes.
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
    /// Structured workout, when one is running.
    #[serde(default)]
    pub workout: Option<WorkoutStatus>,
    /// Open timed segment, when the rider is inside one.
    #[serde(default)]
    pub segment: Option<SegmentStatus>,
    /// Draft saving against a rider ahead, in percent.
    ///
    /// `None` or `0` hides the draft pill.
    #[serde(default)]
    pub draft_saving_pct: Option<u8>,
    /// Fish collected on this ride. `None` or `0` hides the fish pill.
    #[serde(default)]
    pub fish_collected: Option<u32>,
    /// Current fish combo. Shown beside [`Self::fish_collected`] when both
    /// are above zero.
    #[serde(default)]
    pub fish_combo: Option<u32>,
    /// Powerup that is active right now.
    #[serde(default)]
    pub active_powerup: Option<PowerupStatus>,
    /// The course is being ridden in reverse.
    #[serde(default)]
    pub course_reversed: bool,
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
            workout: None,
            segment: None,
            draft_saving_pct: None,
            fish_collected: None,
            fish_combo: None,
            active_powerup: None,
            course_reversed: false,
        }
    }
}
