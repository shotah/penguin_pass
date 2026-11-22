//! Ride phase and the course the rider picked.
//!
//! A new session still starts [`RidePhase::Riding`] so an existing game keeps
//! moving. The title screen calls [`crate::Session::hold_at_start`] and later
//! [`crate::Session::begin_ride`].

use std::sync::mpsc::{self, Receiver};
use std::sync::{Mutex, OnceLock};
use std::thread::JoinHandle;

use pp_course::{Course, CourseMeta, is_route_id};
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

/// A route loaded from a JSON file (`res://routes/` or `user://routes/`).
#[derive(Clone, Debug)]
pub struct RouteEntry {
    /// Stable slug derived from folder or file stem (e.g. `"valley_loop"`).
    pub id: String,
    /// Validated course geometry and metadata.
    pub course: Course,
    /// True when loaded from the user's custom routes directory.
    pub user_route: bool,
}

/// Built-in loop, a custom JSON route, or a generated route.
///
/// Stored in the settings file. A missing key loads as [`Self::PenguinPass`].
#[derive(Clone, Debug, Default, PartialEq)]
pub enum CourseChoice {
    /// The built-in Penguin Pass loop.
    #[default]
    PenguinPass,
    /// The built-in Lonely Road loop.
    LonelyRoad,
    /// A JSON route registered at runtime (folder name under `routes/`).
    Custom {
        /// Route id (e.g. `valley_loop`).
        id: String,
    },
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
            Self::LonelyRoad => "farmland",
            Self::Generated {
                terrain: TerrainPreset::Desert,
                ..
            } => "desert",
            Self::Custom { .. } => "alpine",
            _ => "alpine",
        }
    }

    /// Short name for a menu row, such as `Penguin Pass` or `Desert 10 km`.
    #[must_use]
    pub fn label(&self) -> String {
        match self {
            Self::PenguinPass => Course::penguin_pass().name().to_owned(),
            Self::LonelyRoad => Course::lonely_road().name().to_owned(),
            Self::Custom { id } => id.replace('_', " ").to_owned(),
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

    /// True when the course is ridden without pacing bots.
    #[must_use]
    pub fn is_solitary(&self) -> bool {
        matches!(self, Self::LonelyRoad)
    }

    /// Stable route id when this choice refers to a JSON route.
    #[must_use]
    pub fn route_id(&self) -> Option<&str> {
        match self {
            Self::PenguinPass => Some("penguin_pass"),
            Self::LonelyRoad => Some("lonely_road"),
            Self::Custom { id } => Some(id),
            Self::Generated { .. } => None,
        }
    }

    /// Bundled ride-music mood id (`energetic`, `cruising`, or `solitary`).
    #[must_use]
    pub fn mood_name(&self) -> &'static str {
        match self {
            Self::LonelyRoad => "solitary",
            Self::PenguinPass => "energetic",
            Self::Generated {
                terrain: TerrainPreset::Mountain,
                ..
            } => "energetic",
            Self::Generated {
                terrain: TerrainPreset::Flat | TerrainPreset::Rolling | TerrainPreset::Desert,
                ..
            } => "cruising",
            Self::Custom { .. } => "energetic",
        }
    }

    /// True when the catalog row can be rerolled from the course picker.
    #[must_use]
    pub fn is_rerollable(&self) -> bool {
        matches!(self, Self::Generated { .. })
    }

    /// One sentence for the course picker overview.
    #[must_use]
    pub fn description(&self) -> &'static str {
        match self {
            Self::PenguinPass => {
                "Hand-built 5 km alpine climb with steady hairpins to the pass, a rolling ridge, and a fast plunge back to the valley."
            }
            Self::LonelyRoad => {
                "An 18 km solitary highway across open wheatfields, red barns, and vast flat horizons — no bots, just you and the road."
            }
            Self::Generated {
                terrain: TerrainPreset::Flat,
                ..
            } => {
                "Fast, level roads with sweeping bends built for steady tempo cruises, sprints, and big-ring spins."
            }
            Self::Generated {
                terrain: TerrainPreset::Rolling,
                ..
            } => {
                "Undulating country hills and punchy short climbs winding through green meadows and pine valleys."
            }
            Self::Generated {
                terrain: TerrainPreset::Mountain,
                ..
            } => {
                "Sustained alpine climbing, tight switchbacks, and high mountain ridges with sweeping descents."
            }
            Self::Generated {
                terrain: TerrainPreset::Desert,
                ..
            } => {
                "Sun-baked red rock straights, wide canyon bends, and long desert drags beneath open skies."
            }
            Self::Custom { .. } => "",
        }
    }

    pub(crate) fn from_toml(value: &toml::Value) -> Option<Self> {
        if let Some(text) = value.as_str() {
            return match text {
                "penguin_pass" => Some(Self::PenguinPass),
                "lonely_road" => Some(Self::LonelyRoad),
                _ if is_route_id(text) => Some(Self::Custom {
                    id: text.to_owned(),
                }),
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
            Self::LonelyRoad => serializer.serialize_str("lonely_road"),
            Self::Custom { id } => serializer.serialize_str(id),
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
    /// Theme file stem from the route JSON or generator preset.
    pub theme: String,
    /// Ride-music mood id from the route JSON or generator preset.
    pub mood: String,
    /// Whether pacing bots ride this course.
    pub bots: bool,
    /// Thumbnail samples, `(distance_m, elevation_m)`, at most 64 points.
    pub profile: Vec<(f32, f32)>,
    /// Course picker blurb.
    pub description: String,
    /// Top-down route samples in `[0, 1] × [0, 1]`, `(x, z)`.
    pub map_points: Vec<(f32, f32)>,
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
const MAP_POINTS: usize = 96;

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

pub(crate) fn build_course_dir(
    choice: &CourseChoice,
    reversed: bool,
    custom: &[RouteEntry],
) -> Result<Course, String> {
    let course = build_course(choice, custom)?;
    Ok(if reversed { course.reversed() } else { course })
}

pub(crate) fn build_course(choice: &CourseChoice, custom: &[RouteEntry]) -> Result<Course, String> {
    match choice {
        CourseChoice::PenguinPass => Ok(Course::penguin_pass()),
        CourseChoice::LonelyRoad => Ok(Course::lonely_road()),
        CourseChoice::Custom { id } => custom
            .iter()
            .find(|entry| entry.id == *id)
            .map(|entry| entry.course.clone())
            .ok_or_else(|| format!("unknown route '{id}'")),
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
                .map(|course| {
                    course.with_meta(CourseMeta {
                        theme: choice.theme_name().to_owned(),
                        mood: choice.mood_name().to_owned(),
                        description: choice.description().to_owned(),
                        bots: true,
                    })
                })
        }
    }
}

pub(crate) fn open_course(
    choice: &CourseChoice,
    reversed: bool,
    custom: &[RouteEntry],
) -> (Course, CourseChoice, Option<String>) {
    match build_course_dir(choice, reversed, custom) {
        Ok(course) => (course, choice.clone(), None),
        Err(detail) => {
            let course = Course::penguin_pass();
            let course = if reversed { course.reversed() } else { course };
            if matches!(choice, CourseChoice::Custom { .. }) {
                (course, choice.clone(), Some(detail))
            } else {
                (course, CourseChoice::PenguinPass, Some(detail))
            }
        }
    }
}

pub(crate) fn option_from_course(choice: CourseChoice, course: &Course) -> CourseOption {
    let (min_grade_pct, max_grade_pct) = grade_span(course);
    CourseOption {
        choice: choice.clone(),
        name: course.name().to_owned(),
        length_m: course.length_m(),
        ascent_m: f64::from(course.total_ascent_m()),
        min_grade_pct,
        max_grade_pct,
        theme: course.theme().to_owned(),
        mood: course.mood().to_owned(),
        bots: course.bots(),
        profile: thumbnail(course),
        description: course.description().to_owned(),
        map_points: route_map(course),
    }
}

pub(crate) fn course_option(
    choice: &CourseChoice,
    custom: &[RouteEntry],
) -> Result<CourseOption, String> {
    let course = build_course(choice, custom)?;
    Ok(option_from_course(choice.clone(), &course))
}

fn default_choices() -> [CourseChoice; 9] {
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
        CourseChoice::LonelyRoad,
    ]
}

fn build_default_catalog() -> Vec<CourseOption> {
    default_choices()
        .into_iter()
        .map(|choice| {
            course_option(&choice, &[])
                .unwrap_or_else(|err| panic!("proof-of-concept route: {err}"))
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

fn route_map(course: &Course) -> Vec<(f32, f32)> {
    let length = course.length_m();
    if !length.is_finite() || length <= 0.0 {
        return vec![(0.5, 0.5)];
    }
    let steps = MAP_POINTS.saturating_sub(1).max(1);
    let raw: Vec<(f32, f32)> = (0..MAP_POINTS)
        .map(|index| {
            let distance = length * index as f64 / steps as f64;
            let position = course.position_at(distance);
            (position[0], position[2])
        })
        .collect();
    let min_x = raw
        .iter()
        .map(|point| point.0)
        .fold(f32::INFINITY, f32::min);
    let max_x = raw
        .iter()
        .map(|point| point.0)
        .fold(f32::NEG_INFINITY, f32::max);
    let min_z = raw
        .iter()
        .map(|point| point.1)
        .fold(f32::INFINITY, f32::min);
    let max_z = raw
        .iter()
        .map(|point| point.1)
        .fold(f32::NEG_INFINITY, f32::max);
    let span = (max_x - min_x).max(max_z - min_z).max(1.0);
    let center_x = (min_x + max_x) * 0.5;
    let center_z = (min_z + max_z) * 0.5;
    raw.into_iter()
        .map(|(x, z)| {
            let nx = 0.5 + (x - center_x) / span;
            let nz = 0.5 + (z - center_z) / span;
            (nx.clamp(0.0, 1.0), nz.clamp(0.0, 1.0))
        })
        .collect()
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
        let forward = build_course_dir(&choice, false, &[]).expect("forward");
        let backward = build_course_dir(&choice, true, &[]).expect("reverse");
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
        assert_eq!(catalog.len(), 9);
        assert_eq!(catalog[6].name, "Rolling 20 km");
        assert_eq!(catalog[7].name, "Desert 20 km");
        assert_eq!(catalog[7].theme, "desert");
        assert_eq!(catalog[8].name, "Lonely Road");
        assert_eq!(catalog[8].theme, "farmland");
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

    #[test]
    fn catalog_descriptions_and_maps() {
        let catalog = build_default_catalog();
        for option in &catalog {
            assert!(!option.description.is_empty());
            assert!(option.map_points.len() >= 16);
            for &(x, z) in &option.map_points {
                assert!((0.0..=1.0).contains(&x), "{x}");
                assert!((0.0..=1.0).contains(&z), "{z}");
            }
        }
    }

    #[test]
    fn course_mood_names_match_playlists() {
        assert_eq!(CourseChoice::LonelyRoad.mood_name(), "solitary");
        assert_eq!(CourseChoice::PenguinPass.mood_name(), "energetic");
        assert_eq!(
            CourseChoice::Generated {
                terrain: TerrainPreset::Mountain,
                distance_km: 10,
                seed: MOUNTAIN_SEED,
            }
            .mood_name(),
            "energetic"
        );
        for terrain in [
            TerrainPreset::Desert,
            TerrainPreset::Flat,
            TerrainPreset::Rolling,
        ] {
            assert_eq!(
                CourseChoice::Generated {
                    terrain,
                    distance_km: 10,
                    seed: 1,
                }
                .mood_name(),
                "cruising"
            );
        }
    }

    #[test]
    fn custom_route_serializes_as_plain_id() {
        let custom = CourseChoice::Custom {
            id: "valley_loop".to_owned(),
        };
        let value = toml::Value::try_from(&custom).expect("serialize");
        assert_eq!(value.as_str(), Some("valley_loop"));
        let back = CourseChoice::from_toml(&value).expect("parse");
        assert_eq!(back, custom);
        assert!(CourseChoice::from_toml(&toml::Value::String("bad id".to_owned())).is_none());
    }

    #[test]
    fn lonely_road_serializes_and_flags() {
        let lonely = CourseChoice::LonelyRoad;
        assert!(lonely.is_solitary());
        assert!(!lonely.is_rerollable());
        assert!(!CourseChoice::PenguinPass.is_solitary());
        assert!(
            CourseChoice::Generated {
                terrain: TerrainPreset::Flat,
                distance_km: 10,
                seed: 1,
            }
            .is_rerollable()
        );

        let value = toml::Value::try_from(&lonely).expect("serialize");
        assert_eq!(value.as_str(), Some("lonely_road"));
        let back = CourseChoice::from_toml(&value).expect("parse");
        assert_eq!(back, lonely);
        assert_eq!(back.theme_name(), "farmland");
    }
}
