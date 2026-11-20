//! Shared types for Penguin Pass.
//!
//! Units are plain SI-ish numbers. The field name carries the unit. There is
//! no units library: other crates should use these fields as documented.

use serde::{Deserialize, Serialize};

/// One reading from a trainer.
///
/// Missing sensors stay `None`. The game does not invent a value for them.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct TrainerSample {
    /// Instantaneous power, in watts.
    pub power_w: Option<u16>,
    /// Cadence, in revolutions per minute.
    pub cadence_rpm: Option<f32>,
    /// Trainer-reported wheel speed, in kilometers per hour.
    ///
    /// Informational only. Game physics owns the speed used for the ride.
    pub speed_kph: Option<f32>,
    /// Heart rate, in beats per minute.
    pub heart_rate_bpm: Option<u8>,
}

/// Mass and drag of the rider, plus functional threshold power.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct RiderProfile {
    /// Rider mass, in kilograms.
    pub rider_mass_kg: f32,
    /// Bicycle mass, in kilograms. Does not include the rider.
    pub bike_mass_kg: f32,
    /// Aerodynamic drag area (Cd × A), in square meters.
    pub cda_m2: f32,
    /// Coefficient of rolling resistance, dimensionless.
    pub crr: f32,
    /// Functional threshold power, in watts.
    pub ftp_w: u16,
}

impl Default for RiderProfile {
    /// A typical road rider: 75 kg, 9 kg bike, CdA 0.32 m², Crr 0.004, FTP 200 W.
    fn default() -> Self {
        Self {
            rider_mass_kg: 75.0,
            bike_mass_kg: 9.0,
            cda_m2: 0.32,
            crr: 0.004,
            ftp_w: 200,
        }
    }
}

impl RiderProfile {
    /// Rider mass plus bicycle mass, in kilograms.
    pub fn total_mass_kg(&self) -> f32 {
        self.rider_mass_kg + self.bike_mass_kg
    }
}

/// Per-frame ride snapshot sent to the HUD and the TV dashboard.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Telemetry {
    /// Current power, in watts.
    pub power_w: f32,
    /// Cadence, in revolutions per minute. `None` when no sensor is reporting.
    pub cadence_rpm: Option<f32>,
    /// Heart rate, in beats per minute. `None` when no sensor is reporting.
    pub heart_rate_bpm: Option<u8>,
    /// Game physics speed, in meters per second.
    pub speed_mps: f32,
    /// Total distance ridden this session, in meters.
    pub distance_m: f64,
    /// Distance ridden on the current lap, in meters.
    pub lap_distance_m: f64,
    /// Current lap number. `0` before the ride starts; the first lap is `1`.
    pub lap: u32,
    /// Road grade, in percent, not a fraction. `5.0` is a 5% grade. Positive is uphill.
    pub grade_pct: f32,
    /// Elevation above the course datum, in meters.
    pub elevation_m: f32,
    /// Elapsed ride time, in seconds.
    pub elapsed_s: f64,
}

impl Telemetry {
    /// Game physics speed, in kilometers per hour.
    pub fn speed_kph(&self) -> f32 {
        self.speed_mps * 3.6
    }
}

/// What the game asks a trainer to do.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum TrainerCommand {
    /// Simulate this grade, in percent, not a fraction. Positive is uphill.
    SetGrade {
        /// Grade to hold, in percent.
        grade_pct: f32,
    },
    /// Hold this power target, in watts (ERG mode).
    SetTargetPower {
        /// Power target, in watts.
        watts: u16,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_rider_is_a_typical_road_rider() {
        let rider = RiderProfile::default();
        assert_eq!(rider.rider_mass_kg, 75.0);
        assert_eq!(rider.bike_mass_kg, 9.0);
        assert_eq!(rider.cda_m2, 0.32);
        assert_eq!(rider.crr, 0.004);
        assert_eq!(rider.ftp_w, 200);
        assert_eq!(rider.total_mass_kg(), 84.0);
    }

    #[test]
    fn total_mass_adds_rider_and_bike() {
        let rider = RiderProfile {
            rider_mass_kg: 1.5,
            bike_mass_kg: 2.5,
            ..RiderProfile::default()
        };
        assert_eq!(rider.total_mass_kg(), 4.0);
    }

    #[test]
    fn speed_kph_converts_from_meters_per_second() {
        let telemetry = Telemetry {
            speed_mps: 10.0,
            ..Telemetry::default()
        };
        assert_eq!(telemetry.speed_kph(), 36.0);
    }

    #[test]
    fn telemetry_json_round_trip() {
        let original = Telemetry {
            power_w: 250.5,
            cadence_rpm: Some(88.0),
            heart_rate_bpm: None,
            speed_mps: 9.5,
            distance_m: 4321.25,
            lap_distance_m: 321.0,
            lap: 3,
            grade_pct: -2.5,
            elevation_m: 48.0,
            elapsed_s: 905.5,
        };
        let json = serde_json::to_string(&original).unwrap();
        let decoded: Telemetry = serde_json::from_str(&json).unwrap();
        assert_eq!(original, decoded);
    }
}
