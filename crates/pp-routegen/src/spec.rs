//! Route specification: distance, seed, name, and terrain character.

use std::fmt;

/// Shortest supported loop, in meters.
pub const MIN_DISTANCE_M: f64 = 2_000.0;

/// Longest supported loop, in meters.
pub const MAX_DISTANCE_M: f64 = 100_000.0;

/// Highest elevation above the start line, in meters.
pub const MAX_ELEVATION_M: f64 = 2_000.0;

/// Longest side of the map, as a fraction of the route length.
///
/// A circle is about 0.32. A long thin loop is rejected past this so a
/// 10 km route stays on a map a few kilometres across.
pub const MAX_FOOTPRINT_FRACTION: f64 = 0.46;

/// Shortest allowed longest-side, as a fraction of the route length.
///
/// This stops a tight spiral: the loop has to occupy a real patch of ground.
pub const MIN_FOOTPRINT_FRACTION: f64 = 0.03;

/// Minimum gap between non-consecutive centerline samples, in meters.
pub const CENTERLINE_CLEARANCE_M: f64 = 25.0;

/// Along-track distance treated as the same stretch of road, in meters.
///
/// Samples closer than this along the route are consecutive, including
/// across the start line. A hairpin's two exits fall inside this window;
/// the parallel legs further along do not.
pub const CLEARANCE_WINDOW_M: f64 = 160.0;

/// How many shapes are tried before [`crate::generate`] gives up.
pub const MAX_ATTEMPTS: u32 = 40;

/// How far a generated ascent may sit from the target, in meters.
///
/// The tolerance is 12% of the target, and at least 3 m so a short flat
/// loop is not rejected for a rounding error of a metre.
#[must_use]
pub fn ascent_tolerance_m(target_ascent_m: f64) -> f64 {
    (target_ascent_m.abs() * 0.12).max(3.0)
}

/// Named terrain character.
///
/// Each preset expands to a [`TerrainParams`] value. Those numbers are the
/// contract the generator builds to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerrainPreset {
    /// Almost no climbing, gentle curves.
    Flat,
    /// Short hills of a few percent, moderate curves.
    Rolling,
    /// One or a few long climbs, switchbacks, long descents.
    Mountain,
    /// Long straights, wide bends, shallow drags and an occasional ramp.
    Desert,
}

impl TerrainPreset {
    /// The numbers this preset expands to.
    #[must_use]
    pub fn params(self) -> TerrainParams {
        match self {
            Self::Flat => TerrainParams {
                ascent_per_km: 4.0,
                max_up_grade_pct: 2.0,
                max_down_grade_pct: 2.0,
                typical_climb_m: 2_000.0,
                min_radius_m: 55.0,
                switchback_radius_m: 55.0,
                twist: 0.22,
                switchbacks_per_10km: 0.0,
                max_grade_rate: 0.08,
            },
            Self::Rolling => TerrainParams {
                ascent_per_km: 15.0,
                max_up_grade_pct: 6.0,
                max_down_grade_pct: 6.0,
                typical_climb_m: 1_100.0,
                min_radius_m: 34.0,
                switchback_radius_m: 34.0,
                twist: 0.55,
                switchbacks_per_10km: 0.0,
                max_grade_rate: 0.18,
            },
            Self::Mountain => TerrainParams {
                ascent_per_km: 25.0,
                max_up_grade_pct: 10.0,
                max_down_grade_pct: 10.0,
                typical_climb_m: 8_000.0,
                min_radius_m: 42.0,
                switchback_radius_m: 26.0,
                twist: 0.80,
                switchbacks_per_10km: 6.0,
                max_grade_rate: 0.22,
            },
            Self::Desert => TerrainParams {
                ascent_per_km: 11.0,
                max_up_grade_pct: 7.0,
                max_down_grade_pct: 6.5,
                typical_climb_m: 7_000.0,
                min_radius_m: 85.0,
                switchback_radius_m: 85.0,
                twist: 0.18,
                switchbacks_per_10km: 0.0,
                max_grade_rate: 0.10,
            },
        }
    }

    /// Parse `flat`, `rolling`, `mountain`, or `desert`.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "flat" => Some(Self::Flat),
            "rolling" => Some(Self::Rolling),
            "mountain" => Some(Self::Mountain),
            "desert" => Some(Self::Desert),
            _ => None,
        }
    }

    /// Stable name used by the example binary.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Flat => "flat",
            Self::Rolling => "rolling",
            Self::Mountain => "mountain",
            Self::Desert => "desert",
        }
    }
}

impl fmt::Display for TerrainPreset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Numeric terrain character, in rider-facing units.
///
/// Grades are percent (rise over horizontal run). Lengths and radii are
/// meters. `twist` is dimensionless, from 0 (long straights) to 1 (twisty).
/// `switchbacks_per_10km` counts climb hairpins per 10 km of route.
/// `max_grade_rate` is the steepest change of grade, in percentage points
/// per meter of horizontal road.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TerrainParams {
    /// Target total ascent per kilometre of route distance, in meters.
    pub ascent_per_km: f64,
    /// Steepest uphill grade, in percent.
    pub max_up_grade_pct: f64,
    /// Steepest downhill grade, in percent. Stored as a positive number.
    pub max_down_grade_pct: f64,
    /// Typical length of one sustained climb, in meters.
    pub typical_climb_m: f64,
    /// Minimum centerline radius of an ordinary bend, in meters.
    pub min_radius_m: f64,
    /// Minimum centerline radius of a switchback hairpin, in meters.
    ///
    /// Hairpins may be tighter than [`Self::min_radius_m`]. Adjacent hairpin
    /// legs are about two radii apart, so this stays large enough for the
    /// centerline clearance.
    pub switchback_radius_m: f64,
    /// How much the plan bends, from 0 (straighter) to 1 (twisty).
    pub twist: f64,
    /// Climb hairpins per 10 km of route. Zero means none.
    pub switchbacks_per_10km: f64,
    /// Maximum change of grade, in percentage points per meter.
    pub max_grade_rate: f64,
}

impl TerrainParams {
    fn validate(&self, distance_m: f64) -> Result<(), SpecError> {
        let fields = [
            self.ascent_per_km,
            self.max_up_grade_pct,
            self.max_down_grade_pct,
            self.typical_climb_m,
            self.min_radius_m,
            self.switchback_radius_m,
            self.twist,
            self.switchbacks_per_10km,
            self.max_grade_rate,
        ];
        if fields.iter().any(|v| !v.is_finite()) {
            return Err(SpecError::Terrain("terrain numbers must be finite"));
        }
        if self.ascent_per_km < 0.0 {
            return Err(SpecError::Terrain("ascent per km must be >= 0"));
        }
        if !(0.5..=20.0).contains(&self.max_up_grade_pct)
            || !(0.5..=20.0).contains(&self.max_down_grade_pct)
        {
            return Err(SpecError::Terrain("max grade must be between 0.5% and 20%"));
        }
        if !(100.0..=50_000.0).contains(&self.typical_climb_m) {
            return Err(SpecError::Terrain(
                "typical climb must be between 100 m and 50 km",
            ));
        }
        if !(15.0..=2_000.0).contains(&self.min_radius_m) {
            return Err(SpecError::Terrain(
                "minimum radius must be between 15 m and 2 km",
            ));
        }
        if self.switchback_radius_m < 18.0 || self.switchback_radius_m > self.min_radius_m + 1e-9 {
            return Err(SpecError::Terrain(
                "switchback radius must be between 18 m and the minimum bend radius",
            ));
        }
        if !(0.0..=1.0).contains(&self.twist) {
            return Err(SpecError::Terrain("twist must be from 0 to 1"));
        }
        if !(0.0..=30.0).contains(&self.switchbacks_per_10km) {
            return Err(SpecError::Terrain(
                "switchbacks per 10 km must be from 0 to 30",
            ));
        }
        if !(0.02..=1.5).contains(&self.max_grade_rate) {
            return Err(SpecError::Terrain(
                "grade rate must be between 0.02 and 1.5 percentage points per meter",
            ));
        }
        // A circle of this radius is already most of the route, so no
        // smaller bend can close a loop either.
        if self.min_radius_m * std::f64::consts::TAU > distance_m * 0.80 {
            return Err(SpecError::Terrain(
                "minimum radius is too large for this distance",
            ));
        }
        let achievable = 0.40 * self.max_up_grade_pct.min(self.max_down_grade_pct) * 10.0;
        if self.ascent_per_km > achievable {
            return Err(SpecError::Terrain(
                "target ascent is too high for the grade limits",
            ));
        }
        Ok(())
    }
}

/// Preset or explicit terrain numbers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Terrain {
    /// A named preset.
    Preset(TerrainPreset),
    /// Caller-supplied numbers.
    Custom(TerrainParams),
}

impl Terrain {
    /// Resolved numbers, whether they came from a preset or the caller.
    #[must_use]
    pub fn params(self) -> TerrainParams {
        match self {
            Self::Preset(preset) => preset.params(),
            Self::Custom(params) => params,
        }
    }
}

/// What to generate.
///
/// `distance_m` is the 3D centerline length in meters. `seed` selects the
/// route. The same field values always produce the same course.
#[derive(Debug, Clone, PartialEq)]
pub struct RouteSpec {
    /// Deterministic route selector.
    pub seed: u64,
    /// Target 3D centerline length, in meters.
    pub distance_m: f64,
    /// Terrain character.
    pub terrain: Terrain,
    /// Course name written into the JSON. Trimmed during validation.
    pub name: String,
}

impl RouteSpec {
    /// A spec that still needs [`Self::validate`] before generation.
    #[must_use]
    pub fn new(name: impl Into<String>, seed: u64, distance_m: f64, terrain: Terrain) -> Self {
        Self {
            seed,
            distance_m,
            terrain,
            name: name.into(),
        }
    }

    /// Rejects a non-finite or out-of-range distance, a bad name, or terrain
    /// numbers that cannot make a rideable loop.
    ///
    /// # Errors
    ///
    /// Returns [`SpecError`] for any rejected field.
    pub fn validate(&self) -> Result<(), SpecError> {
        if !self.distance_m.is_finite() {
            return Err(SpecError::DistanceNotFinite);
        }
        if !(MIN_DISTANCE_M..=MAX_DISTANCE_M).contains(&self.distance_m) {
            return Err(SpecError::DistanceOutOfRange {
                distance_m: self.distance_m,
            });
        }
        let name = self.name.trim();
        if name.is_empty() {
            return Err(SpecError::NameEmpty);
        }
        if name.chars().count() > 64 {
            return Err(SpecError::NameTooLong {
                len: name.chars().count(),
            });
        }
        if name.chars().any(|c| c.is_control()) {
            return Err(SpecError::NameHasControlChar);
        }
        self.terrain.params().validate(self.distance_m)?;
        Ok(())
    }

    /// Trimmed name, after a successful [`Self::validate`].
    #[must_use]
    pub fn course_name(&self) -> &str {
        self.name.trim()
    }
}

/// Why a [`RouteSpec`] was rejected.
#[derive(Debug, Clone, PartialEq)]
pub enum SpecError {
    /// Distance was NaN or infinite.
    DistanceNotFinite,
    /// Distance was outside 2 km to 100 km.
    DistanceOutOfRange {
        /// The rejected distance, in meters.
        distance_m: f64,
    },
    /// Name was empty or only whitespace.
    NameEmpty,
    /// Name was longer than 64 characters.
    NameTooLong {
        /// Character count after trimming.
        len: usize,
    },
    /// Name contained a control character.
    NameHasControlChar,
    /// A terrain number was missing, out of range, or not achievable.
    Terrain(&'static str),
}

impl fmt::Display for SpecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DistanceNotFinite => write!(f, "distance must be finite"),
            Self::DistanceOutOfRange { distance_m } => write!(
                f,
                "distance {distance_m} m is outside {MIN_DISTANCE_M} m to {MAX_DISTANCE_M} m"
            ),
            Self::NameEmpty => write!(f, "name must not be empty"),
            Self::NameTooLong { len } => {
                write!(f, "name is {len} characters; the maximum is 64")
            }
            Self::NameHasControlChar => write!(f, "name must not contain control characters"),
            Self::Terrain(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for SpecError {}
