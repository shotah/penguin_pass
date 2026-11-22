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

/// What the wheels are rolling on.
///
/// A trainer that can shake maps this to road feel. [`Self::Smooth`] is
/// pavement and turns the effect off. A course span (a bridge deck, a gravel
/// trail) names a surface directly. The lane is the other source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoadSurface {
    /// Smooth pavement. Road feel is off.
    Smooth,
    /// Concrete plates.
    ConcretePlates,
    /// Cattle grid.
    CattleGrid,
    /// Hard cobblestones.
    CobblestonesHard,
    /// Soft cobblestones.
    CobblestonesSoft,
    /// Brick road.
    BrickRoad,
    /// Compact dirt.
    OffRoad,
    /// Loose gravel.
    Gravel,
    /// Ice.
    Ice,
    /// Plank deck. A bridge uses this.
    WoodenBoards,
}

impl Serialize for RoadSurface {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.label())
    }
}

impl<'de> Deserialize<'de> for RoadSurface {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let label = String::deserialize(deserializer)?;
        Self::from_label(&label)
            .ok_or_else(|| serde::de::Error::custom(format!("unknown road surface {label:?}")))
    }
}

impl RoadSurface {
    #[must_use]
    fn from_label(label: &str) -> Option<Self> {
        match label {
            "smooth" => Some(Self::Smooth),
            "concrete-plates" => Some(Self::ConcretePlates),
            "cattle-grid" => Some(Self::CattleGrid),
            "cobblestones-hard" => Some(Self::CobblestonesHard),
            "cobblestones-soft" => Some(Self::CobblestonesSoft),
            "brick-road" => Some(Self::BrickRoad),
            "off-road" => Some(Self::OffRoad),
            "gravel" => Some(Self::Gravel),
            "ice" => Some(Self::Ice),
            "wooden-boards" => Some(Self::WoodenBoards),
            _ => None,
        }
    }

    /// Stable name for logs and course data.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Smooth => "smooth",
            Self::ConcretePlates => "concrete-plates",
            Self::CattleGrid => "cattle-grid",
            Self::CobblestonesHard => "cobblestones-hard",
            Self::CobblestonesSoft => "cobblestones-soft",
            Self::BrickRoad => "brick-road",
            Self::OffRoad => "off-road",
            Self::Gravel => "gravel",
            Self::Ice => "ice",
            Self::WoodenBoards => "wooden-boards",
        }
    }
}

/// Lane slot where the gravel right shoulder takes over.
///
/// Slots `0.0` through `7.0` are the paved road (`pp-view` slots 0..=7).
/// Slot `8.0` is the gravel shoulder, past the right edge of the asphalt.
/// Feel starts halfway through the move onto that shoulder.
pub const GRAVEL_SHOULDER_SLOT: f32 = 7.5;

/// Surface under the wheels.
///
/// `span` is a course feature at this distance. It wins over the lane, so a
/// wooden bridge stays wooden in the shoulder slot and a gravel trail is
/// gravel in every lane. [`Some`]`(`[`RoadSurface::Smooth`]`)` is pavement,
/// not “no span.” `None` is today's roads: slot [`GRAVEL_SHOULDER_SLOT`] and
/// above is gravel, and the rest is smooth. A non-finite slot with no span
/// is smooth.
#[must_use]
pub fn surface_under(lane_slot: f32, span: Option<RoadSurface>) -> RoadSurface {
    if let Some(surface) = span {
        return surface;
    }
    if !lane_slot.is_finite() {
        return RoadSurface::Smooth;
    }
    if lane_slot >= GRAVEL_SHOULDER_SLOT {
        RoadSurface::Gravel
    } else {
        RoadSurface::Smooth
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
    /// Set the Zwift virtual gear over trainer service `0xFC82`.
    ///
    /// Valid gears are `1`..=`24` (`1` is easiest, `24` is hardest). Values
    /// outside that range are clamped by the trainer layer before they hit the
    /// radio.
    SetVirtualGear {
        /// Requested gear, before clamping.
        gear: u8,
    },
    /// Shake the trainer to match the surface under the wheels.
    ///
    /// [`RoadSurface::Smooth`] turns the effect off. Trainers that cannot shake
    /// ignore it. It does not change grade or target power.
    SetRoadFeel {
        /// Surface to simulate under the wheels.
        surface: RoadSurface,
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
    fn surface_under_defaults_to_pavement_then_gravel_shoulder() {
        assert_eq!(surface_under(0.0, None), RoadSurface::Smooth);
        assert_eq!(surface_under(6.0, None), RoadSurface::Smooth);
        assert_eq!(surface_under(7.49, None), RoadSurface::Smooth);
        assert_eq!(
            surface_under(GRAVEL_SHOULDER_SLOT, None),
            RoadSurface::Gravel
        );
        assert_eq!(surface_under(8.0, None), RoadSurface::Gravel);
        assert_eq!(surface_under(f32::NAN, None), RoadSurface::Smooth);
        assert_eq!(surface_under(f32::INFINITY, None), RoadSurface::Smooth);
        assert_eq!(
            surface_under(8.0, Some(RoadSurface::WoodenBoards)),
            RoadSurface::WoodenBoards
        );
        assert_eq!(
            surface_under(2.0, Some(RoadSurface::Gravel)),
            RoadSurface::Gravel
        );
        assert_eq!(
            surface_under(8.0, Some(RoadSurface::Smooth)),
            RoadSurface::Smooth
        );
    }

    #[test]
    fn road_surface_labels_are_kebab_case() {
        assert_eq!(RoadSurface::Smooth.label(), "smooth");
        assert_eq!(RoadSurface::ConcretePlates.label(), "concrete-plates");
        assert_eq!(RoadSurface::CattleGrid.label(), "cattle-grid");
        assert_eq!(RoadSurface::CobblestonesHard.label(), "cobblestones-hard");
        assert_eq!(RoadSurface::CobblestonesSoft.label(), "cobblestones-soft");
        assert_eq!(RoadSurface::BrickRoad.label(), "brick-road");
        assert_eq!(RoadSurface::OffRoad.label(), "off-road");
        assert_eq!(RoadSurface::Gravel.label(), "gravel");
        assert_eq!(RoadSurface::Ice.label(), "ice");
        assert_eq!(RoadSurface::WoodenBoards.label(), "wooden-boards");
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
