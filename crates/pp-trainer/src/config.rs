//! Tunables for grade writes, staleness, and reconnect backoff.

use std::time::Duration;

use pp_ftms::{DEFAULT_CRR, DEFAULT_CW_KG_PER_M, DEFAULT_WIND_SPEED_MPS};

/// How a session treats the Fitness Machine Control Point.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionMode {
    /// Request control, start the session, then accept grade and power commands.
    Control,
    /// Subscribe to measurements. Do not write the control point.
    ReadOnly,
}

/// Timing and simulation defaults for [`crate::BleTrainer`].
///
/// The game loop may call [`crate::Trainer::send`] every frame. Grade writes
/// are coalesced: a new grade is sent only when it differs from the last sent
/// grade by at least [`Self::min_grade_delta_pct`] percentage points, and at
/// most one control-point command goes out per [`Self::min_command_interval`].
/// The newest command wins.
#[derive(Debug, Clone, PartialEq)]
pub struct TrainerConfig {
    /// How long [`crate::Trainer::latest_sample`] keeps a reading.
    ///
    /// A sample is fresh while its age is less than or equal to this. The
    /// default is [`crate::SAMPLE_STALENESS`] (3 seconds), which covers the
    /// usual one-notification-per-second indoor bike plus one skipped packet.
    pub staleness: Duration,
    /// Minimum grade change, in percentage points, that produces a write.
    ///
    /// The default is 0.1, matching the 0.01% wire resolution with a little
    /// slack so a 60 fps caller does not refresh an unchanged slope.
    pub min_grade_delta_pct: f32,
    /// Minimum time between control-point commands after the handshake.
    ///
    /// The default is 500 ms, so a frame loop cannot exceed about two writes
    /// per second.
    pub min_command_interval: Duration,
    /// First delay before a reconnect attempt.
    pub reconnect_initial: Duration,
    /// Cap on the exponential reconnect delay.
    pub reconnect_max: Duration,
    /// How long to wait for a control-point response indication.
    pub control_timeout: Duration,
    /// Wind speed sent with every grade, in meters per second.
    pub wind_speed_mps: f32,
    /// Coefficient of rolling resistance, dimensionless, sent with every grade.
    pub crr: f32,
    /// Wind resistance coefficient, in kilograms per meter, sent with every grade.
    pub cw_kg_per_m: f32,
    /// Whether the session requests control.
    pub mode: SessionMode,
    /// Rider mass, in kilograms, sent in an FE-C user-configuration page.
    ///
    /// `None` sends the invalid weight, so the trainer keeps its own rider mass.
    /// The session crate owns the rider profile and copies it here at spawn.
    pub rider_mass_kg: Option<f32>,
    /// Bicycle mass, in kilograms, not including the rider.
    ///
    /// `None` sends the invalid bicycle weight on the FE-C user-configuration page.
    pub bike_mass_kg: Option<f32>,
}

impl Default for TrainerConfig {
    fn default() -> Self {
        Self {
            staleness: crate::SAMPLE_STALENESS,
            min_grade_delta_pct: 0.1,
            min_command_interval: Duration::from_millis(500),
            reconnect_initial: Duration::from_secs(1),
            reconnect_max: Duration::from_secs(30),
            control_timeout: Duration::from_secs(2),
            wind_speed_mps: DEFAULT_WIND_SPEED_MPS,
            crr: DEFAULT_CRR,
            cw_kg_per_m: DEFAULT_CW_KG_PER_M,
            mode: SessionMode::Control,
            rider_mass_kg: None,
            bike_mass_kg: None,
        }
    }
}

pub(crate) fn duration_ms(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}
