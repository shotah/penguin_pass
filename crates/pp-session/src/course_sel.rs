//! Ride phase and the course the rider picked.
//!
//! A new session still starts [`RidePhase::Riding`] so an existing game keeps
//! moving. The title screen calls [`crate::Session::hold_at_start`] and later
//! [`crate::Session::begin_ride`].

use std::sync::mpsc::{self, Receiver};
use std::sync::{Mutex, OnceLock};
use std::thread::JoinHandle;

use pp_course::Course;
use pp_routegen::{RouteSpec, Terrain, TerrainPreset, generate};
use serde::de::{self, Deserializer};
use serde::ser::{SerializeMap, Serializer};
use serde::{Deserialize, Serialize};

/// Where the ride is on the title screen, on the road, or on the summary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RidePhase {
    /// At the start line. The trainer and the dashboard stay live.
    ///
    /// Distance, the ride clock, laps, and the summary do not advance, and
    /// the trainer is held at grade 0.
    Ready,
    /// The ride clock is running.
    Riding,
    /// [`crate::Session::end_ride`] has frozen the summary.
    Ended,
}

/// Built-in loop or a generated route.
///
/// Stored in the settings file. A missing key loads as [`Self::PenguinPass`].
#[derive(Clone, Debug, Default, PartialEq)]
pub enum CourseChoice {
    /// The built-in Penguin Pass loop.
    #[default]
    PenguinPass,
    /// A route from `pp-routegen`.
    Generated {
        /// Terrain preset.
        terrain: TerrainPreset,
        /// Target length, in kilometres. The generator accepts 2 to 100.
        distance_km: u16,
        /// Seed. The same seed and distance rebuild the same road.
        seed: u64,
    },
}

impl CourseChoice {
    /// `desert` for [`TerrainPreset::Desert`], otherwise `alpine`.
    #[must_use]
    pub fn theme_name(&self) -> &'static str {
        match self {
            Self::Generated {
                terrain: TerrainPreset::Desert,
                ..
            } => "desert",
            _ => "alpine",
        }
    }

    /// Short name for a menu row, such as `Penguin Pass` or `Desert 10 km`.
    #[must_use]
    pub fn label(&self) -> String {
        match self {
            Self::PenguinPass => "Penguin Pass".to_owned(),
            Self::Generated {
                terrain,
                distance_km,
                ..
            } => format!("{} {distance_km} km", terrain_title(*terrain)),
        }
    }

    /// [`Self::label`], with ` (Reverse)` appended when `reversed` is true.
    ///
    /// The menu row stays [`Self::label`]. The toast and the HUD use this so
    /// the active direction is visible.
    #[must_use]
    pub fn label_with_direction(&self, reversed: bool) -> String {
        let name = self.label();
        if reversed {
            format!("{name} (Reverse)")
        } else {
            name
        }
    }

    pub(crate) fn from_toml(value: &toml::Value) -> Option<Self> {
        if let Some(text) = value.as_str() {
            return match text {
                "penguin_pass" => Some(Self::PenguinPass),
                _ => None,
            };
        }
        let table = value.as_table()?;
        let terrain = TerrainPreset::parse(table.get("terrain")?.as_str()?)?;
        let distance_km = u16::try_from(positive_int(table.get("distance_km")?)?).ok()?;
        let seed = positive_int(table.get("seed")?)?;
        Some(Self::Generated {
            terrain,
            distance_km,
            seed,
        })
    }
}

impl Serialize for CourseChoice {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::PenguinPass => serializer.serialize_str("penguin_pass"),
            Self::Generated {
                terrain,
                distance_km,
                seed,
            } => {
                let mut map = serializer.serialize_map(Some(3))?;
                map.serialize_entry("terrain", terrain.as_str())?;
                map.serialize_entry("distance_km", distance_km)?;
                map.serialize_entry("seed", seed)?;
                map.end()
            }
        }
    }
}

impl<'de> Deserialize<'de> for CourseChoice {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = toml::Value::deserialize(deserializer).map_err(de::Error::custom)?;
        Ok(Self::from_toml(&value).unwrap_or(Self::PenguinPass))
    }
}

/// One row of the course picker.
#[derive(Clone, Debug)]
pub struct CourseOption {
    /// Choice passed to [`crate::Session::set_course`].
    pub choice: CourseChoice,
    /// Menu name. Matches [`CourseChoice::label`].
    pub name: String,
    /// Lap length, in meters.
    pub length_m: f64,
    /// Total ascent of one lap, in meters.
    pub ascent_m: f64,
    /// Steepest downhill grade, in percent. Negative is downhill.
    pub min_grade_pct: f32,
    /// Steepest uphill grade, in percent.
    pub max_grade_pct: f32,
    /// [`CourseChoice::theme_name`].
    pub theme: &'static str,
    /// Thumbnail samples, `(distance_m, elevation_m)`, at most 64 points.
    pub profile: Vec<(f32, f32)>,
}

/// Documented rolling seed. See `docs/route-generator.md`.
const ROLLING_SEED: u64 = 3;
/// Documented mountain seed.
const MOUNTAIN_SEED: u64 = 11;
/// Documented desert seed.
const DESERT_SEED: u64 = 7;
/// Flat seed from the 2 km fingerprint, used here for the 10 km row.
const FLAT_SEED: u64 = 1;

const THUMBNAIL_POINTS: usize = 64;

fn terrain_title(terrain: TerrainPreset) -> &'static str {
    match terrain {
        TerrainPreset::Flat => "Flat",
        TerrainPreset::Rolling => "Rolling",
        TerrainPreset::Mountain => "Mountain",
        TerrainPreset::Desert => "Desert",
    }
}

fn positive_int(value: &toml::Value) -> Option<u64> {
    let number = value.as_integer()?;
    u64::try_from(number).ok()
}

pub(crate) fn build_course_dir(choice: &CourseChoice, reversed: bool) -> Result<Course, String> {
    let course = build_course(choice)?;
    Ok(if reversed { course.reversed() } else { course })
}

pub(crate) fn build_course(choice: &CourseChoice) -> Result<Course, String> {
    match choice {
        CourseChoice::PenguinPass => Ok(Course::penguin_pass()),
        CourseChoice::Generated {
            terrain,
            distance_km,
            seed,
        } => {
            let distance_m = f64::from(*distance_km) * 1_000.0;
            let spec = RouteSpec::new(choice.label(), *seed, distance_m, Terrain::Preset(*terrain));
            generate(&spec)
                .map(|route| route.course().clone())
                .map_err(|err| format!("could not build {}: {err}", choice.label()))
        }
    }
}

pub(crate) fn open_course(
    choice: &CourseChoice,
    reversed: bool,
) -> (Course, CourseChoice, Option<String>) {
    match build_course_dir(choice, reversed) {
        Ok(course) => (course, choice.clone(), None),
        Err(detail) => {
            let course = Course::penguin_pass();
            let course = if reversed { course.reversed() } else { course };
            (course, CourseChoice::PenguinPass, Some(detail))
        }
    }
}

pub(crate) fn course_option(choice: &CourseChoice) -> Result<CourseOption, String> {
    let course = build_course(choice)?;
    let (min_grade_pct, max_grade_pct) = grade_span(&course);
    Ok(CourseOption {
        choice: choice.clone(),
        name: choice.label(),
        length_m: course.length_m(),
        ascent_m: f64::from(course.total_ascent_m()),
        min_grade_pct,
        max_grade_pct,
        theme: choice.theme_name(),
        profile: thumbnail(&course),
    })
}

fn default_choices() -> [CourseChoice; 8] {
    [
        CourseChoice::PenguinPass,
        CourseChoice::Generated {
            terrain: TerrainPreset::Rolling,
            distance_km: 5,
            seed: ROLLING_SEED,
        },
        CourseChoice::Generated {
            terrain: TerrainPreset::Rolling,
            distance_km: 10,
            seed: ROLLING_SEED,
        },
        CourseChoice::Generated {
            terrain: TerrainPreset::Mountain,
            distance_km: 10,
            seed: MOUNTAIN_SEED,
        },
        CourseChoice::Generated {
            terrain: TerrainPreset::Desert,
            distance_km: 10,
            seed: DESERT_SEED,
        },
        CourseChoice::Generated {
            terrain: TerrainPreset::Flat,
            distance_km: 10,
            seed: FLAT_SEED,
        },
        CourseChoice::Generated {
            terrain: TerrainPreset::Rolling,
            distance_km: 20,
            seed: ROLLING_SEED,
        },
        CourseChoice::Generated {
            terrain: TerrainPreset::Desert,
            distance_km: 20,
            seed: DESERT_SEED,
        },
    ]
}

fn build_default_catalog() -> Vec<CourseOption> {
    default_choices()
        .into_iter()
        .map(|choice| {
            course_option(&choice).unwrap_or_else(|err| panic!("proof-of-concept route: {err}"))
        })
        .collect()
}

struct CatalogCache {
    ready: Option<Vec<CourseOption>>,
    rx: Option<Receiver<Vec<CourseOption>>>,
    _thread: Option<JoinHandle<()>>,
}

fn cache() -> &'static Mutex<CatalogCache> {
    static CACHE: OnceLock<Mutex<CatalogCache>> = OnceLock::new();
    CACHE.get_or_init(|| {
        let (tx, rx) = mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("pp-session-catalog".to_owned())
            .spawn(move || {
                let _ = tx.send(build_default_catalog());
            })
            .ok();
        Mutex::new(CatalogCache {
            ready: None,
            rx: spawned.as_ref().map(|_| rx),
            _thread: spawned,
        })
    })
}

/// Start the catalog build if it has not started. Does not wait.
pub(crate) fn warm_catalog() {
    let _ = cache();
}

pub(crate) fn shared_catalog() -> Vec<CourseOption> {
    let mut guard = cache().lock().unwrap_or_else(|err| err.into_inner());
    if let Some(ready) = &guard.ready {
        return ready.clone();
    }
    let built = match guard.rx.take() {
        Some(rx) => rx.recv().unwrap_or_else(|_| build_default_catalog()),
        None => build_default_catalog(),
    };
    if let Some(handle) = guard._thread.take() {
        let _ = handle.join();
    }
    guard.ready = Some(built.clone());
    built
}

fn grade_span(course: &Course) -> (f32, f32) {
    let length = course.length_m();
    if length <= 0.0 {
        return (0.0, 0.0);
    }
    let mut min_grade = f32::MAX;
    let mut max_grade = f32::MIN;
    let mut distance = 0.0;
    while distance <= length {
        let grade = course.grade_pct_at(distance);
        min_grade = min_grade.min(grade);
        max_grade = max_grade.max(grade);
        distance += 1.0;
    }
    (min_grade, max_grade)
}

fn thumbnail(course: &Course) -> Vec<(f32, f32)> {
    let length = course.length_m();
    if !length.is_finite() || length <= 0.0 {
        return vec![(0.0, course.elevation_at(0.0))];
    }
    let step = (length / (THUMBNAIL_POINTS - 1) as f64).max(1.0);
    let mut points: Vec<(f32, f32)> = course
        .profile(step)
        .into_iter()
        .take(THUMBNAIL_POINTS)
        .map(|point| (point.distance_m as f32, point.elevation_m))
        .collect();
    if points.is_empty() {
        points.push((0.0, course.elevation_at(0.0)));
    }
    points
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn reverse_label_and_build_flip_penguin_pass() {
        let choice = CourseChoice::PenguinPass;
        assert_eq!(choice.label(), "Penguin Pass");
        assert_eq!(choice.label_with_direction(false), "Penguin Pass");
        assert_eq!(choice.label_with_direction(true), "Penguin Pass (Reverse)");
        let forward = build_course_dir(&choice, false).expect("forward");
        let backward = build_course_dir(&choice, true).expect("reverse");
        assert!(!forward.is_reversed());
        assert!(backward.is_reversed());
        let length = forward.length_m();
        let sample = length * 0.37;
        let mirrored = (length - sample).rem_euclid(length);
        let gap = (backward.elevation_at(sample) - forward.elevation_at(mirrored)).abs();
        assert!(gap < 1.0e-3_f32, "{gap}");
    }

    #[test]
    fn fresh_catalog_cost() {
        let started = Instant::now();
        let catalog = build_default_catalog();
        let elapsed = started.elapsed();
        eprintln!("CATALOG_BUILD {elapsed:?}");
        assert_eq!(catalog.len(), 8);
        assert_eq!(catalog[6].name, "Rolling 20 km");
        assert_eq!(catalog[7].name, "Desert 20 km");
        assert_eq!(catalog[7].theme, "desert");
        for option in &catalog {
            eprintln!(
                "CATALOG_ROW {} len {:.1} ascent {:.1} grade {:.2}..{:.2} theme {} points {}",
                option.name,
                option.length_m,
                option.ascent_m,
                option.min_grade_pct,
                option.max_grade_pct,
                option.theme,
                option.profile.len()
            );
            assert!(option.profile.len() <= 64);
        }
    }
}
