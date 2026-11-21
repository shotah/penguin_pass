//! Lifetime totals, badges, and the kits they unlock.
//!
//! A ride shorter than [`MIN_RIDE_S`] is ignored. The day streak counts a
//! local calendar day only when the ride lasted at least [`STREAK_RIDE_S`].
//! The file is TOML. A missing field loads as zero, so an older file still
//! opens. Kits are not stored; they follow the badges.

use std::fs;
use std::path::{Path, PathBuf};

use serde::de::{self, Deserializer, Visitor};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Ride length, in seconds, below which [`Profile::apply_ride`] changes nothing.
pub const MIN_RIDE_S: f64 = 60.0;

/// Ride length, in seconds, that counts as a day in the streak.
pub const STREAK_RIDE_S: f64 = 600.0;

/// Ride length, in seconds, that earns the hour badge.
pub const HOUR_RIDE_S: f64 = 3_600.0;

/// Course ids that count toward the every-course badge, in a stable order.
///
/// [`RideOutcome::course_id`] should be one of these. Penguin Pass is
/// `penguin_pass`, matching the settings file. A generated route uses the
/// terrain name: `flat`, `rolling`, `mountain`, or `desert`. Any other id is
/// remembered and does not count toward the badge.
pub const COURSES: &[&str] = &["penguin_pass", "flat", "rolling", "mountain", "desert"];

/// Kit unlocked by the first counted ride.
pub const KIT_SUNRISE: &str = "kit-sunrise";

/// Kit unlocked by 1,000 m of climbing.
pub const KIT_GLACIER: &str = "kit-glacier";

/// Kit unlocked by 8,849 m of climbing.
pub const KIT_KOM_DOTS: &str = "kit-kom-dots";

const VERSION: u32 = 1;

const HEADER: &str = "\
# Penguin Pass arcade profile
#
# Counts are lifetime totals. Distances and ascent are meters. Time is seconds.
# A ride under one minute is not recorded. The day streak counts a local
# calendar day when the ride lasted at least ten minutes.
# A missing field loads as zero, so an older file still opens.
# Kits are not stored; they follow the badges.
";

/// What [`Profile::progress`] is counting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BadgeUnit {
    /// Finished rides of at least a minute.
    Rides,
    /// Laps of Penguin Pass.
    Laps,
    /// Meters. Distance badges use 1,000 m per kilometre. Climb badges use vertical meters.
    Meters,
    /// Fish collected.
    Fish,
    /// Best combo.
    Combo,
    /// Current streak, in days.
    Days,
    /// Courses from [`COURSES`] that have been ridden.
    Courses,
    /// Rides that set a segment record.
    Records,
    /// Seconds. The hour badge's target is 3,600.
    Seconds,
}

/// A stable badge id. The text does not change once shipped.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct BadgeId(String);

impl BadgeId {
    /// The id text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for BadgeId {
    fn from(id: &str) -> Self {
        Self(id.to_owned())
    }
}

impl AsRef<str> for BadgeId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl PartialEq<&str> for BadgeId {
    fn eq(&self, other: &&str) -> bool {
        self.0 == *other
    }
}

/// One badge definition.
///
/// Take them from [`Badge::catalog`]. The id, name, and description are stable.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Badge {
    /// Id that never changes.
    pub id: &'static str,
    /// Short name.
    pub name: &'static str,
    /// One line.
    pub description: &'static str,
    /// Kit unlocked when the badge is earned. `None` when the badge is only a badge.
    pub kit: Option<&'static str>,
    goal: Goal,
}

impl Badge {
    /// Every badge, in the order [`Profile::apply_ride`] awards them.
    #[must_use]
    pub fn catalog() -> &'static [Self] {
        CATALOG
    }

    /// The catalog entry for `id`.
    #[must_use]
    pub fn get(id: &str) -> Option<&'static Self> {
        CATALOG.iter().find(|badge| badge.id == id)
    }

    /// The progress target, in [`Self::unit`].
    #[must_use]
    pub fn target(self) -> f64 {
        self.goal.target()
    }

    /// The unit of [`Self::target`] and of [`Profile::progress`].
    #[must_use]
    pub fn unit(self) -> BadgeUnit {
        self.goal.unit()
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Goal {
    Rides(u32),
    PenguinLaps(u32),
    DistanceM(f64),
    AscentM(f64),
    Fish(u32),
    Combo(u32),
    Streak(u32),
    EveryCourse,
    Segment,
    Hour(f64),
}

impl Goal {
    fn target(self) -> f64 {
        match self {
            Self::Rides(n)
            | Self::PenguinLaps(n)
            | Self::Combo(n)
            | Self::Streak(n)
            | Self::Fish(n) => f64::from(n),
            Self::DistanceM(meters) | Self::AscentM(meters) | Self::Hour(meters) => meters,
            Self::EveryCourse => 5.0,
            Self::Segment => 1.0,
        }
    }

    fn unit(self) -> BadgeUnit {
        match self {
            Self::Rides(_) => BadgeUnit::Rides,
            Self::PenguinLaps(_) => BadgeUnit::Laps,
            Self::DistanceM(_) | Self::AscentM(_) => BadgeUnit::Meters,
            Self::Fish(_) => BadgeUnit::Fish,
            Self::Combo(_) => BadgeUnit::Combo,
            Self::Streak(_) => BadgeUnit::Days,
            Self::EveryCourse => BadgeUnit::Courses,
            Self::Segment => BadgeUnit::Records,
            Self::Hour(_) => BadgeUnit::Seconds,
        }
    }
}

const CATALOG: &[Badge] = &[
    Badge {
        id: "first-ride",
        name: "First splash",
        description: "Finish a ride of at least a minute.",
        kit: Some(KIT_SUNRISE),
        goal: Goal::Rides(1),
    },
    Badge {
        id: "first-loop",
        name: "Round the mountain",
        description: "Finish a lap of Penguin Pass.",
        kit: None,
        goal: Goal::PenguinLaps(1),
    },
    Badge {
        id: "distance-10",
        name: "10 km",
        description: "Ride 10 km in total.",
        kit: None,
        goal: Goal::DistanceM(10_000.0),
    },
    Badge {
        id: "distance-50",
        name: "50 km",
        description: "Ride 50 km in total.",
        kit: None,
        goal: Goal::DistanceM(50_000.0),
    },
    Badge {
        id: "distance-100",
        name: "100 km",
        description: "Ride 100 km in total.",
        kit: None,
        goal: Goal::DistanceM(100_000.0),
    },
    Badge {
        id: "distance-500",
        name: "500 km",
        description: "Ride 500 km in total.",
        kit: None,
        goal: Goal::DistanceM(500_000.0),
    },
    Badge {
        id: "distance-1000",
        name: "1,000 km",
        description: "Ride 1,000 km in total.",
        kit: None,
        goal: Goal::DistanceM(1_000_000.0),
    },
    Badge {
        id: "climb-1000",
        name: "Glacier line",
        description: "Climb 1,000 m in total.",
        kit: Some(KIT_GLACIER),
        goal: Goal::AscentM(1_000.0),
    },
    Badge {
        id: "climb-5000",
        name: "High country",
        description: "Climb 5,000 m in total.",
        kit: None,
        goal: Goal::AscentM(5_000.0),
    },
    Badge {
        id: "climb-8849",
        name: "Above Everest",
        description: "Climb 8,849 m in total.",
        kit: Some(KIT_KOM_DOTS),
        goal: Goal::AscentM(8_849.0),
    },
    Badge {
        id: "fish-100",
        name: "A handful",
        description: "Collect 100 fish.",
        kit: None,
        goal: Goal::Fish(100),
    },
    Badge {
        id: "fish-1000",
        name: "A shoal",
        description: "Collect 1,000 fish.",
        kit: None,
        goal: Goal::Fish(1_000),
    },
    Badge {
        id: "fish-10000",
        name: "The school",
        description: "Collect 10,000 fish.",
        kit: None,
        goal: Goal::Fish(10_000),
    },
    Badge {
        id: "combo-25",
        name: "Clean line",
        description: "Collect 25 fish without a miss.",
        kit: None,
        goal: Goal::Combo(25),
    },
    Badge {
        id: "streak-3",
        name: "Three days",
        description: "Ride ten minutes on three days in a row.",
        kit: None,
        goal: Goal::Streak(3),
    },
    Badge {
        id: "streak-7",
        name: "One week",
        description: "Ride ten minutes on seven days in a row.",
        kit: None,
        goal: Goal::Streak(7),
    },
    Badge {
        id: "streak-30",
        name: "A month",
        description: "Ride ten minutes on thirty days in a row.",
        kit: None,
        goal: Goal::Streak(30),
    },
    Badge {
        id: "every-course",
        name: "Every road",
        description: "Ride Penguin Pass, flat, rolling, mountain, and desert.",
        kit: None,
        goal: Goal::EveryCourse,
    },
    Badge {
        id: "segment-record",
        name: "Segment record",
        description: "Set a segment record.",
        kit: None,
        goal: Goal::Segment,
    },
    Badge {
        id: "hour-ride",
        name: "One hour",
        description: "Finish a ride of at least an hour.",
        kit: None,
        goal: Goal::Hour(HOUR_RIDE_S),
    },
];

/// A calendar day in the rider's local timezone.
///
/// Month is 1 to 12. [`LocalDate::new`] rejects days that are not on the
/// calendar. A date built field by field that is not a real day does not
/// move the streak.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalDate {
    /// Year, such as 2026.
    pub year: i32,
    /// Month from 1 to 12.
    pub month: u8,
    /// Day of the month, from 1.
    pub day: u8,
}

impl LocalDate {
    /// A real calendar day, or `None` when the day does not exist.
    #[must_use]
    pub fn new(year: i32, month: u8, day: u8) -> Option<Self> {
        let date = Self { year, month, day };
        date.epoch_day().map(|_| date)
    }

    fn epoch_day(self) -> Option<i64> {
        if !is_valid_date(self.year, self.month, self.day) {
            return None;
        }
        Some(days_from_civil(
            self.year,
            u32::from(self.month),
            u32::from(self.day),
        ))
    }
}

/// What one finished ride did.
///
/// The session fills this from the ride and from [`crate::FishRun`].
#[derive(Clone, Debug, PartialEq, Default)]
pub struct RideOutcome {
    /// Elapsed time, in seconds.
    pub duration_s: f64,
    /// Distance ridden, in meters.
    pub distance_m: f64,
    /// Ascent gained, in meters.
    pub ascent_m: f64,
    /// Fish collected.
    pub fish: u64,
    /// Golden fish collected.
    pub golden_fish: u64,
    /// Best combo on the ride.
    pub best_combo: u32,
    /// One of [`COURSES`], or any other course id the session wants remembered.
    pub course_id: String,
    /// Laps of the built-in Penguin Pass loop finished on this ride.
    pub penguin_pass_laps: u32,
    /// The rider set at least one segment record.
    pub segment_record: bool,
}

/// A badge and the local day it was earned.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EarnedBadge {
    /// Badge id. Unknown ids are kept so a newer file still round-trips.
    pub id: BadgeId,
    /// The local day of the ride that earned it.
    pub earned: LocalDate,
}

/// Lifetime arcade record.
///
/// Construct with [`Profile::default`] or [`Profile::load`]. Fields update
/// through [`Profile::apply_ride`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Profile {
    #[serde(default = "current_version", deserialize_with = "de_u32")]
    version: u32,
    #[serde(default, deserialize_with = "de_u32")]
    rides: u32,
    #[serde(default, deserialize_with = "de_f64")]
    time_s: f64,
    #[serde(default, deserialize_with = "de_f64")]
    distance_m: f64,
    #[serde(default, deserialize_with = "de_f64")]
    ascent_m: f64,
    #[serde(default, deserialize_with = "de_u64")]
    fish: u64,
    #[serde(default, deserialize_with = "de_u64")]
    golden_fish: u64,
    #[serde(default, deserialize_with = "de_u32")]
    best_combo: u32,
    #[serde(default, deserialize_with = "de_f64")]
    longest_ride_s: f64,
    #[serde(default, deserialize_with = "de_u32")]
    streak_days: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    streak_last: Option<LocalDate>,
    #[serde(default, deserialize_with = "de_u32")]
    penguin_pass_laps: u32,
    #[serde(default, deserialize_with = "de_u32")]
    segment_records: u32,
    #[serde(default)]
    courses: Vec<String>,
    #[serde(default)]
    badges: Vec<EarnedBadge>,
}

impl Default for Profile {
    fn default() -> Self {
        Self {
            version: current_version(),
            rides: 0,
            time_s: 0.0,
            distance_m: 0.0,
            ascent_m: 0.0,
            fish: 0,
            golden_fish: 0,
            best_combo: 0,
            longest_ride_s: 0.0,
            streak_days: 0,
            streak_last: None,
            penguin_pass_laps: 0,
            segment_records: 0,
            courses: Vec::new(),
            badges: Vec::new(),
        }
    }
}

fn current_version() -> u32 {
    VERSION
}

impl Profile {
    /// Finished rides of at least a minute.
    #[must_use]
    pub fn rides(&self) -> u32 {
        self.rides
    }

    /// Total time of counted rides, in seconds.
    #[must_use]
    pub fn time_s(&self) -> f64 {
        self.time_s
    }

    /// Total distance of counted rides, in meters.
    #[must_use]
    pub fn distance_m(&self) -> f64 {
        self.distance_m
    }

    /// Total ascent of counted rides, in meters.
    #[must_use]
    pub fn ascent_m(&self) -> f64 {
        self.ascent_m
    }

    /// Fish collected on counted rides.
    #[must_use]
    pub fn fish(&self) -> u64 {
        self.fish
    }

    /// Golden fish collected on counted rides.
    #[must_use]
    pub fn golden_fish(&self) -> u64 {
        self.golden_fish
    }

    /// Best combo on any counted ride.
    #[must_use]
    pub fn best_combo(&self) -> u32 {
        self.best_combo
    }

    /// Longest counted ride, in seconds.
    #[must_use]
    pub fn longest_ride_s(&self) -> f64 {
        self.longest_ride_s
    }

    /// Current run of local days with a ride of at least ten minutes.
    #[must_use]
    pub fn streak_days(&self) -> u32 {
        self.streak_days
    }

    /// The local day of the last ride that counted toward the streak.
    #[must_use]
    pub fn streak_last(&self) -> Option<LocalDate> {
        self.streak_last
    }

    /// Penguin Pass laps finished on counted rides.
    #[must_use]
    pub fn penguin_pass_laps(&self) -> u32 {
        self.penguin_pass_laps
    }

    /// Counted rides that set a segment record.
    #[must_use]
    pub fn segment_records(&self) -> u32 {
        self.segment_records
    }

    /// Course ids ridden on counted rides, sorted.
    #[must_use]
    pub fn courses(&self) -> &[String] {
        &self.courses
    }

    /// Badges earned, in the order they were earned.
    ///
    /// A ride that earns several at once appends them in catalog order.
    #[must_use]
    pub fn badges(&self) -> &[EarnedBadge] {
        &self.badges
    }

    /// Whether `id` has been earned, including an id this version does not know.
    #[must_use]
    pub fn has_badge(&self, id: &str) -> bool {
        self.badges.iter().any(|badge| badge.id.as_str() == id)
    }

    /// Kits unlocked by earned badges, in catalog order.
    #[must_use]
    pub fn kits(&self) -> Vec<&'static str> {
        CATALOG
            .iter()
            .filter_map(|badge| {
                if self.has_badge(badge.id) {
                    badge.kit
                } else {
                    None
                }
            })
            .collect()
    }

    /// `(current, target)` for a progress screen.
    ///
    /// `current` is the lifetime total in [`Badge::unit`] and is not capped
    /// at `target`. A badge this profile does not know returns `(0, 1)`.
    #[must_use]
    pub fn progress(&self, badge: &Badge) -> (f64, f64) {
        (self.current(badge.goal), badge.target())
    }

    /// Add a ride and return the badges it just earned, in catalog order.
    ///
    /// A ride under [`MIN_RIDE_S`], or a non-finite duration, returns an empty
    /// list and leaves the profile unchanged. Negative distances and counts
    /// count as zero. The streak moves only when the ride lasted at least
    /// [`STREAK_RIDE_S`] and `date` is a real day after the previous streak
    /// day: the next day continues it, a later day starts it at one, and an
    /// earlier day leaves it alone. A shorter counted ride does not consume
    /// the day.
    #[must_use]
    pub fn apply_ride(&mut self, outcome: &RideOutcome, date: LocalDate) -> Vec<BadgeId> {
        let Some(duration) = counted_duration(outcome.duration_s) else {
            return Vec::new();
        };
        self.record(outcome, duration);
        self.note_streak(duration, date);
        self.award(date)
    }

    /// Read a profile from `path`.
    ///
    /// A missing file is a fresh profile and is not created. An empty file or
    /// a file of only comments is a fresh profile and is left in place. A
    /// file that is not TOML is moved to `<name>.bad` and replaced with a
    /// fresh profile. Unknown keys are ignored. Missing keys use zero. This
    /// does not panic.
    #[must_use]
    pub fn load(path: impl AsRef<Path>) -> Self {
        let path = path.as_ref();
        let text = match fs::read_to_string(path) {
            Ok(text) => text,
            Err(_) => return Self::default(),
        };
        match parse_profile(&text) {
            Ok(profile) => profile,
            Err(()) => {
                if quarantine(path) {
                    let fresh = Self::default();
                    let _ = fresh.save(path);
                    fresh
                } else {
                    Self::default()
                }
            }
        }
    }

    /// Write this profile to `path`.
    ///
    /// The bytes go to a temporary file beside `path` and are then renamed
    /// over it. The parent directory is created when it is missing. The
    /// temporary file is removed if the rename fails.
    ///
    /// # Errors
    ///
    /// Returns [`ProfileError`] when the directory cannot be created, the
    /// bytes cannot be written, or the rename fails.
    pub fn save(&self, path: impl AsRef<Path>) -> Result<(), ProfileError> {
        let path = path.as_ref();
        let mut profile = self.clone();
        profile.normalize();
        let body = encode(&profile)?;
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent).map_err(ProfileError::CreateDir)?;
        }
        let tmp = temp_path(path);
        if let Err(err) = fs::write(&tmp, body.as_bytes()) {
            let _ = fs::remove_file(&tmp);
            return Err(ProfileError::Write(err));
        }
        if let Err(err) = fs::rename(&tmp, path) {
            let _ = fs::remove_file(&tmp);
            return Err(ProfileError::Rename(err));
        }
        Ok(())
    }

    fn record(&mut self, outcome: &RideOutcome, duration: f64) {
        self.rides = self.rides.saturating_add(1);
        self.time_s += duration;
        self.distance_m += nonneg(outcome.distance_m);
        self.ascent_m += nonneg(outcome.ascent_m);
        self.fish = self.fish.saturating_add(outcome.fish);
        self.golden_fish = self.golden_fish.saturating_add(outcome.golden_fish);
        self.best_combo = self.best_combo.max(outcome.best_combo);
        self.longest_ride_s = self.longest_ride_s.max(duration);
        self.penguin_pass_laps = self
            .penguin_pass_laps
            .saturating_add(outcome.penguin_pass_laps);
        if outcome.segment_record {
            self.segment_records = self.segment_records.saturating_add(1);
        }
        self.remember_course(&outcome.course_id);
    }

    fn remember_course(&mut self, id: &str) {
        let id = id.trim();
        if id.is_empty() || self.courses.iter().any(|got| got == id) {
            return;
        }
        self.courses.push(id.to_owned());
        self.courses.sort();
    }

    fn note_streak(&mut self, duration_s: f64, date: LocalDate) {
        if duration_s < STREAK_RIDE_S {
            return;
        }
        let Some(today) = date.epoch_day() else {
            return;
        };
        match self.streak_last.and_then(LocalDate::epoch_day) {
            Some(last) if today == last => {}
            Some(last) if today == last + 1 => {
                self.streak_days = self.streak_days.saturating_add(1);
                self.streak_last = Some(date);
            }
            Some(last) if today > last => {
                self.streak_days = 1;
                self.streak_last = Some(date);
            }
            Some(_) => {}
            None => {
                self.streak_days = 1;
                self.streak_last = Some(date);
            }
        }
    }

    fn award(&mut self, date: LocalDate) -> Vec<BadgeId> {
        let mut earned = Vec::new();
        for badge in CATALOG {
            if self.has_badge(badge.id) || !self.meets(badge.goal) {
                continue;
            }
            self.badges.push(EarnedBadge {
                id: BadgeId::from(badge.id),
                earned: date,
            });
            earned.push(BadgeId::from(badge.id));
        }
        earned
    }

    fn meets(&self, goal: Goal) -> bool {
        match goal {
            Goal::Rides(n) => self.rides >= n,
            Goal::PenguinLaps(n) => self.penguin_pass_laps >= n,
            Goal::DistanceM(meters) => self.distance_m >= meters,
            Goal::AscentM(meters) => self.ascent_m >= meters,
            Goal::Fish(n) => self.fish >= u64::from(n),
            Goal::Combo(n) => self.best_combo >= n,
            Goal::Streak(n) => self.streak_days >= n,
            Goal::EveryCourse => COURSES
                .iter()
                .all(|id| self.courses.iter().any(|got| got == id)),
            Goal::Segment => self.segment_records > 0,
            Goal::Hour(seconds) => self.longest_ride_s >= seconds,
        }
    }

    fn current(&self, goal: Goal) -> f64 {
        match goal {
            Goal::Rides(_) => f64::from(self.rides),
            Goal::PenguinLaps(_) => f64::from(self.penguin_pass_laps),
            Goal::DistanceM(_) => self.distance_m,
            Goal::AscentM(_) => self.ascent_m,
            Goal::Fish(_) => self.fish as f64,
            Goal::Combo(_) => f64::from(self.best_combo),
            Goal::Streak(_) => f64::from(self.streak_days),
            Goal::EveryCourse => {
                let n = COURSES
                    .iter()
                    .filter(|id| self.courses.iter().any(|got| got == *id))
                    .count();
                u32::try_from(n).map_or(0.0, f64::from)
            }
            Goal::Segment => f64::from(self.segment_records),
            Goal::Hour(_) => self.longest_ride_s,
        }
    }

    fn normalize(&mut self) {
        self.version = current_version();
        self.time_s = nonneg(self.time_s);
        self.distance_m = nonneg(self.distance_m);
        self.ascent_m = nonneg(self.ascent_m);
        self.longest_ride_s = nonneg(self.longest_ride_s);
        let mut courses: Vec<String> = self
            .courses
            .iter()
            .map(|id| id.trim().to_owned())
            .filter(|id| !id.is_empty())
            .collect();
        courses.sort();
        courses.dedup();
        self.courses = courses;
        let mut seen = Vec::new();
        self.badges.retain(|badge| {
            let id = badge.id.as_str();
            if id.is_empty() || seen.iter().any(|got: &String| got == id) {
                return false;
            }
            seen.push(id.to_owned());
            true
        });
    }
}

/// Why [`Profile::save`] could not replace the file.
#[derive(Debug, Error)]
pub enum ProfileError {
    /// The parent directory could not be created.
    #[error("could not create the profile directory: {0}")]
    CreateDir(#[source] std::io::Error),
    /// The temporary file could not be written.
    #[error("could not write the profile: {0}")]
    Write(#[source] std::io::Error),
    /// The temporary file could not be renamed over the destination.
    #[error("could not replace the profile: {0}")]
    Rename(#[source] std::io::Error),
    /// The profile could not be encoded as TOML.
    #[error("could not encode the profile: {0}")]
    Encode(#[source] toml::ser::Error),
}

fn counted_duration(duration_s: f64) -> Option<f64> {
    if duration_s.is_finite() && duration_s >= MIN_RIDE_S {
        Some(duration_s)
    } else {
        None
    }
}

fn nonneg(value: f64) -> f64 {
    if value.is_finite() && value > 0.0 {
        value
    } else {
        0.0
    }
}

fn encode(profile: &Profile) -> Result<String, ProfileError> {
    let body = toml::to_string_pretty(profile).map_err(ProfileError::Encode)?;
    Ok(format!("{HEADER}\n{body}"))
}

fn parse_profile(text: &str) -> Result<Profile, ()> {
    if text.trim().is_empty() {
        return Ok(Profile::default());
    }
    let mut profile: Profile = toml::from_str(text).map_err(|_| ())?;
    profile.normalize();
    Ok(profile)
}

fn quarantine(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    fs::rename(path, bad_path(path)).is_ok()
}

fn bad_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".bad");
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.join(name),
        _ => PathBuf::from(name),
    }
}

fn temp_path(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .unwrap_or(std::ffi::OsStr::new("arcade.toml"));
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let unique = format!(
        ".{}.{}.{}.tmp",
        file_name.to_string_lossy(),
        std::process::id(),
        nanos
    );
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.join(unique),
        _ => PathBuf::from(unique),
    }
}

fn is_valid_date(year: i32, month: u8, day: u8) -> bool {
    let Some(last) = days_in_month(year, month) else {
        return false;
    };
    day >= 1 && day <= last
}

fn days_in_month(year: i32, month: u8) -> Option<u8> {
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap(year) => 29,
        2 => 28,
        _ => return None,
    };
    Some(days)
}

fn is_leap(year: i32) -> bool {
    let year = i64::from(year);
    year.rem_euclid(4) == 0 && (year.rem_euclid(100) != 0 || year.rem_euclid(400) == 0)
}

/// Howard Hinnant's `days_from_civil`. Day 0 is 1970-01-01.
fn days_from_civil(year: i32, month: u32, day: u32) -> i64 {
    let mut year = i64::from(year);
    if month <= 2 {
        year -= 1;
    }
    let era = year.div_euclid(400);
    let yoe = u64::try_from(year - era * 400).unwrap_or(0);
    let month_prime = if month > 2 { month - 3 } else { month + 9 };
    let day_of_year = (153 * u64::from(month_prime) + 2) / 5 + u64::from(day) - 1;
    let day_of_era = yoe * 365 + yoe / 4 - yoe / 100 + day_of_year;
    era * 146097 + i64::try_from(day_of_era).unwrap_or(0) - 719468
}

struct LooseF64;

impl Visitor<'_> for LooseF64 {
    type Value = f64;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a number")
    }

    fn visit_i64<E: de::Error>(self, value: i64) -> Result<f64, E> {
        Ok(value as f64)
    }

    fn visit_u64<E: de::Error>(self, value: u64) -> Result<f64, E> {
        Ok(value as f64)
    }

    fn visit_f64<E: de::Error>(self, value: f64) -> Result<f64, E> {
        Ok(value)
    }
}

fn de_f64<'de, D: Deserializer<'de>>(deserializer: D) -> Result<f64, D::Error> {
    deserializer.deserialize_any(LooseF64)
}

fn de_u32<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u32, D::Error> {
    Ok(to_u32(deserializer.deserialize_any(LooseF64)?))
}

fn de_u64<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u64, D::Error> {
    Ok(to_u64(deserializer.deserialize_any(LooseF64)?))
}

fn to_u32(value: f64) -> u32 {
    if !value.is_finite() || value <= 0.0 {
        return 0;
    }
    if value >= f64::from(u32::MAX) {
        return u32::MAX;
    }
    let rounded = value.round();
    if rounded >= f64::from(u32::MAX) {
        u32::MAX
    } else {
        // `rounded` is in `0.0 .. u32::MAX` and those integers are exact in f64.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        {
            rounded as u32
        }
    }
}

fn to_u64(value: f64) -> u64 {
    if !value.is_finite() || value <= 0.0 {
        return 0;
    }
    if value >= 1.0e15 {
        return 1_000_000_000_000_000;
    }
    let rounded = value.round();
    if rounded >= 1.0e15 {
        1_000_000_000_000_000
    } else {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        {
            rounded as u64
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(year: i32, month: u8, day: u8) -> LocalDate {
        LocalDate { year, month, day }
    }

    fn ride(seconds: f64) -> RideOutcome {
        RideOutcome {
            duration_s: seconds,
            ..RideOutcome::default()
        }
    }

    fn badge(id: &str) -> &'static Badge {
        Badge::get(id).unwrap_or_else(|| panic!("missing badge {id}"))
    }

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "pp-arcade-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .expect("clock")
                    .as_nanos()
            ));
            fs::create_dir_all(&path).expect("temp dir");
            Self(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn the_catalog_is_stable_and_every_badge_has_a_target() {
        assert_eq!(Badge::catalog().len(), 20);
        let mut ids = Vec::new();
        for badge in Badge::catalog() {
            assert!(!badge.id.is_empty());
            assert!(!badge.name.is_empty());
            assert!(!badge.description.is_empty());
            assert!(badge.target() > 0.0);
            assert!(ids.iter().all(|id| *id != badge.id));
            ids.push(badge.id);
            if let Some(kit) = badge.kit {
                assert!(kit == KIT_SUNRISE || kit == KIT_GLACIER || kit == KIT_KOM_DOTS);
            }
        }
        assert_eq!(COURSES.len(), 5);
        assert_eq!(badge("every-course").target(), 5.0);
        assert_eq!(badge("hour-ride").target(), HOUR_RIDE_S);
        assert_eq!(badge("climb-8849").target(), 8_849.0);
        assert_eq!(badge("first-ride").kit, Some(KIT_SUNRISE));
        assert_eq!(badge("climb-1000").kit, Some(KIT_GLACIER));
        assert_eq!(badge("climb-8849").kit, Some(KIT_KOM_DOTS));
    }

    #[test]
    fn a_ride_under_one_minute_changes_nothing() {
        let mut profile = Profile::default();
        let earned = profile.apply_ride(
            &RideOutcome {
                duration_s: 59.9,
                distance_m: 100_000.0,
                ascent_m: 5_000.0,
                fish: 500,
                golden_fish: 4,
                best_combo: 40,
                course_id: "penguin_pass".to_owned(),
                penguin_pass_laps: 3,
                segment_record: true,
            },
            date(2026, 1, 1),
        );
        assert!(earned.is_empty());
        assert_eq!(profile, Profile::default());
        assert!(
            profile
                .apply_ride(&ride(f64::NAN), date(2026, 1, 1))
                .is_empty()
        );
        assert_eq!(profile.rides(), 0);
    }

    #[test]
    fn one_long_ride_earns_every_total_badge_once_and_in_order() {
        let mut profile = Profile::default();
        let outcome = RideOutcome {
            duration_s: HOUR_RIDE_S,
            distance_m: 1_000_000.0,
            ascent_m: 8_849.0,
            fish: 10_000,
            golden_fish: 3,
            best_combo: 25,
            course_id: "penguin_pass".to_owned(),
            penguin_pass_laps: 1,
            segment_record: true,
        };
        let earned = profile.apply_ride(&outcome, date(2026, 1, 1));
        let expect: Vec<&str> = Badge::catalog()
            .iter()
            .map(|item| item.id)
            .filter(|id| !id.starts_with("streak") && *id != "every-course")
            .collect();
        let got: Vec<&str> = earned.iter().map(BadgeId::as_str).collect();
        assert_eq!(got, expect);
        assert_eq!(profile.kits(), vec![KIT_SUNRISE, KIT_GLACIER, KIT_KOM_DOTS]);
        let again = profile.apply_ride(&outcome, date(2026, 1, 2));
        assert!(again.is_empty(), "{again:?}");
        assert_eq!(profile.rides(), 2);
        assert_eq!(profile.distance_m(), 2_000_000.0);
        assert_eq!(profile.streak_days(), 2);
        assert!(!profile.has_badge("streak-3"));
        assert!(!profile.has_badge("every-course"));
    }

    #[test]
    fn partial_totals_do_not_earn_the_next_badge() {
        let mut profile = Profile::default();
        let _ = profile.apply_ride(
            &RideOutcome {
                duration_s: 120.0,
                distance_m: 5_000.0,
                ascent_m: 250.0,
                fish: 40,
                best_combo: 10,
                ..RideOutcome::default()
            },
            date(2026, 3, 1),
        );
        assert_eq!(profile.progress(badge("distance-10")), (5_000.0, 10_000.0));
        assert_eq!(profile.progress(badge("climb-1000")), (250.0, 1_000.0));
        assert_eq!(profile.progress(badge("fish-100")), (40.0, 100.0));
        assert_eq!(profile.progress(badge("combo-25")), (10.0, 25.0));
        assert_eq!(profile.progress(badge("first-ride")), (1.0, 1.0));
        assert_eq!(profile.progress(badge("hour-ride")), (120.0, HOUR_RIDE_S));
        assert!(!profile.has_badge("distance-10"));
        assert!(!profile.has_badge("hour-ride"));
        let _ = profile.apply_ride(
            &RideOutcome {
                duration_s: 3_599.0,
                distance_m: 4_999.0,
                ascent_m: 749.0,
                fish: 59,
                best_combo: 14,
                ..RideOutcome::default()
            },
            date(2026, 3, 1),
        );
        assert!(!profile.has_badge("distance-10"));
        assert!(!profile.has_badge("climb-1000"));
        assert!(!profile.has_badge("fish-100"));
        assert!(!profile.has_badge("combo-25"));
        assert!(!profile.has_badge("hour-ride"));
    }

    #[test]
    fn every_course_needs_the_five_known_roads() {
        let mut profile = Profile::default();
        for (index, course) in COURSES.iter().enumerate() {
            let earned = profile.apply_ride(
                &RideOutcome {
                    duration_s: 60.0,
                    course_id: (*course).to_owned(),
                    ..RideOutcome::default()
                },
                date(2026, 6, 1),
            );
            if index == 0 {
                assert_eq!(earned, vec![BadgeId::from("first-ride")]);
            } else if index + 1 == COURSES.len() {
                assert_eq!(earned, vec![BadgeId::from("every-course")]);
            } else {
                assert!(earned.is_empty(), "{earned:?}");
            }
        }
        assert_eq!(profile.progress(badge("every-course")), (5.0, 5.0));
        let mut other = Profile::default();
        for course in ["penguin_pass", "flat", "rolling", "mountain", "volcano"] {
            let _ = other.apply_ride(
                &RideOutcome {
                    duration_s: 60.0,
                    course_id: course.to_owned(),
                    ..RideOutcome::default()
                },
                date(2026, 6, 2),
            );
        }
        assert!(!other.has_badge("every-course"));
        assert_eq!(other.progress(badge("every-course")), (4.0, 5.0));
    }

    #[test]
    fn streaks_cross_month_ends_year_ends_and_a_skipped_day() {
        let mut profile = Profile::default();
        let _ = profile.apply_ride(&ride(600.0), date(2026, 1, 31));
        let earned = profile.apply_ride(&ride(600.0), date(2026, 2, 1));
        assert!(earned.is_empty());
        assert_eq!(profile.streak_days(), 2);

        let mut year = Profile::default();
        let _ = year.apply_ride(&ride(600.0), date(2025, 12, 31));
        let _ = year.apply_ride(&ride(600.0), date(2026, 1, 1));
        assert_eq!(year.streak_days(), 2);

        let mut leap = Profile::default();
        let _ = leap.apply_ride(&ride(600.0), date(2024, 2, 28));
        let _ = leap.apply_ride(&ride(600.0), date(2024, 2, 29));
        let earned = leap.apply_ride(&ride(600.0), date(2024, 3, 1));
        assert!(earned.iter().any(|id| id.as_str() == "streak-3"));
        assert_eq!(leap.streak_days(), 3);

        let mut plain = Profile::default();
        let _ = plain.apply_ride(&ride(600.0), date(2025, 2, 28));
        let _ = plain.apply_ride(&ride(600.0), date(2025, 3, 1));
        assert_eq!(plain.streak_days(), 2);
        assert!(LocalDate::new(2025, 2, 29).is_none());
        assert!(LocalDate::new(2024, 2, 29).is_some());

        let mut skipped = Profile::default();
        let _ = skipped.apply_ride(&ride(600.0), date(2026, 4, 1));
        let _ = skipped.apply_ride(&ride(600.0), date(2026, 4, 2));
        let _ = skipped.apply_ride(&ride(600.0), date(2026, 4, 3));
        assert!(skipped.has_badge("streak-3"));
        let _ = skipped.apply_ride(&ride(600.0), date(2026, 4, 5));
        assert_eq!(skipped.streak_days(), 1);
        assert!(skipped.has_badge("streak-3"));
        assert_eq!(skipped.streak_last(), Some(date(2026, 4, 5)));
    }

    #[test]
    fn a_short_ride_does_not_consume_or_reset_the_streak_day() {
        let mut profile = Profile::default();
        let _ = profile.apply_ride(&ride(600.0), date(2026, 5, 1));
        let _ = profile.apply_ride(
            &RideOutcome {
                duration_s: 540.0,
                distance_m: 1_000.0,
                ..RideOutcome::default()
            },
            date(2026, 5, 2),
        );
        assert_eq!(profile.streak_days(), 1);
        assert_eq!(profile.streak_last(), Some(date(2026, 5, 1)));
        assert_eq!(profile.rides(), 2);
        assert_eq!(profile.distance_m(), 1_000.0);
        let _ = profile.apply_ride(&ride(600.0), date(2026, 5, 2));
        assert_eq!(profile.streak_days(), 2);

        let mut blocked = Profile::default();
        let _ = blocked.apply_ride(&ride(600.0), date(2026, 6, 1));
        let _ = blocked.apply_ride(
            &RideOutcome {
                duration_s: 30.0,
                distance_m: 9_000.0,
                ..RideOutcome::default()
            },
            date(2026, 6, 2),
        );
        assert_eq!(blocked.rides(), 1);
        assert_eq!(blocked.distance_m(), 0.0);
        let _ = blocked.apply_ride(&ride(600.0), date(2026, 6, 2));
        assert_eq!(blocked.streak_days(), 2);

        let mut backwards = Profile::default();
        let _ = backwards.apply_ride(&ride(600.0), date(2026, 7, 5));
        let _ = backwards.apply_ride(&ride(600.0), date(2026, 7, 4));
        assert_eq!(backwards.streak_days(), 1);
        assert_eq!(backwards.streak_last(), Some(date(2026, 7, 5)));
        assert_eq!(backwards.rides(), 2);

        let mut impossible = Profile::default();
        let _ = impossible.apply_ride(&ride(600.0), date(2026, 8, 1));
        let _ = impossible.apply_ride(&ride(600.0), date(2026, 2, 30));
        assert_eq!(impossible.rides(), 2);
        assert_eq!(impossible.streak_days(), 1);
        assert_eq!(impossible.streak_last(), Some(date(2026, 8, 1)));
    }

    #[test]
    fn thirty_days_earn_the_streak_badges_on_the_day() {
        let mut profile = Profile::default();
        for day in 1..=30 {
            let earned = profile.apply_ride(&ride(600.0), date(2026, 1, day));
            match day {
                1 => assert_eq!(earned, vec![BadgeId::from("first-ride")]),
                3 => assert_eq!(earned, vec![BadgeId::from("streak-3")]),
                7 => assert_eq!(earned, vec![BadgeId::from("streak-7")]),
                30 => assert_eq!(earned, vec![BadgeId::from("streak-30")]),
                _ => assert!(earned.is_empty(), "day {day}: {earned:?}"),
            }
        }
        assert_eq!(profile.streak_days(), 30);
        assert_eq!(profile.progress(badge("streak-3")), (30.0, 3.0));
        let again = profile.apply_ride(&ride(600.0), date(2026, 1, 30));
        assert!(again.is_empty());
        assert_eq!(profile.streak_days(), 30);
    }

    #[test]
    fn calendar_steps_match_the_civil_day() {
        let step =
            |from: LocalDate, to: LocalDate| to.epoch_day().unwrap() - from.epoch_day().unwrap();
        assert_eq!(step(date(2026, 1, 31), date(2026, 2, 1)), 1);
        assert_eq!(step(date(2025, 12, 31), date(2026, 1, 1)), 1);
        assert_eq!(step(date(2024, 2, 28), date(2024, 2, 29)), 1);
        assert_eq!(step(date(2024, 2, 29), date(2024, 3, 1)), 1);
        assert_eq!(step(date(2025, 2, 28), date(2025, 3, 1)), 1);
        assert_eq!(step(date(2026, 1, 1), date(2026, 1, 3)), 2);
        assert_eq!(date(1970, 1, 1).epoch_day(), Some(0));
        assert!(date(2026, 2, 30).epoch_day().is_none());
    }

    #[test]
    fn a_profile_round_trips_and_an_older_file_still_loads() {
        let dir = TempDir::new();
        let path = dir.0.join("nested").join("arcade.toml");
        let mut profile = Profile::default();
        let _ = profile.apply_ride(
            &RideOutcome {
                duration_s: 600.0,
                distance_m: 12_000.0,
                ascent_m: 400.0,
                fish: 30,
                golden_fish: 2,
                best_combo: 8,
                course_id: "desert".to_owned(),
                penguin_pass_laps: 0,
                segment_record: false,
            },
            date(2026, 11, 20),
        );
        profile.save(&path).expect("save");
        let text = fs::read_to_string(&path).expect("read");
        assert!(text.starts_with('#'), "{text}");
        assert!(text.contains("ten minutes"), "{text}");
        assert!(tmp_files(path.parent().expect("parent")).is_empty());
        let loaded = Profile::load(&path);
        assert_eq!(loaded, profile);

        let old = dir.0.join("old.toml");
        fs::write(
            &old,
            "\
rides = 4
distance_m = 1500
fish = 9
best_combo = 6
future = true

[[badges]]
id = \"first-ride\"
earned = { year = 2020, month = 5, day = 2 }
",
        )
        .expect("old file");
        let loaded = Profile::load(&old);
        assert_eq!(loaded.rides(), 4);
        assert_eq!(loaded.distance_m(), 1_500.0);
        assert_eq!(loaded.fish(), 9);
        assert_eq!(loaded.best_combo(), 6);
        assert_eq!(loaded.golden_fish(), 0);
        assert_eq!(loaded.streak_days(), 0);
        assert!(loaded.has_badge("first-ride"));
        assert_eq!(loaded.kits(), vec![KIT_SUNRISE]);
    }

    #[test]
    fn an_older_file_does_not_award_a_badge_twice_and_keeps_unknown_ids() {
        let dir = TempDir::new();
        let path = dir.0.join("partial.toml");
        fs::write(
            &path,
            "\
rides = 1
time_s = 90

[[badges]]
id = \"first-ride\"
earned = { year = 2020, month = 5, day = 2 }

[[badges]]
id = \"future-badge\"
earned = { year = 2024, month = 1, day = 1 }
",
        )
        .expect("partial");
        let mut loaded = Profile::load(&path);
        assert!(loaded.has_badge("future-badge"));
        let earned = loaded.apply_ride(&ride(90.0), date(2026, 1, 2));
        assert!(!earned.iter().any(|id| id.as_str() == "first-ride"));
        assert!(loaded.has_badge("future-badge"));
        loaded.save(&path).expect("save");
        let again = Profile::load(&path);
        assert!(again.has_badge("future-badge"));
        assert!(again.has_badge("first-ride"));
        assert_eq!(again.rides(), 2);
    }

    #[test]
    fn a_missing_file_is_fresh_and_a_corrupt_file_is_set_aside() {
        let dir = TempDir::new();
        let missing = dir.0.join("missing.toml");
        assert_eq!(Profile::load(&missing), Profile::default());
        assert!(!missing.exists());

        let empty = dir.0.join("empty.toml");
        fs::write(&empty, "  \n# only a comment\n").expect("empty");
        assert_eq!(Profile::load(&empty), Profile::default());
        assert!(!dir.0.join("empty.toml.bad").exists());
        assert!(
            fs::read_to_string(&empty)
                .expect("kept")
                .contains("comment")
        );

        let broken = dir.0.join("arcade.toml");
        let text = "rides = [1, 2\n";
        fs::write(&broken, text).expect("broken");
        assert_eq!(Profile::load(&broken), Profile::default());
        let bad = dir.0.join("arcade.toml.bad");
        assert_eq!(fs::read_to_string(&bad).expect("bad copy"), text);
        let fresh = fs::read_to_string(&broken).expect("replaced");
        assert!(fresh.contains("rides"), "{fresh}");
        assert_eq!(Profile::load(&broken), Profile::default());
        assert_eq!(fs::read_to_string(&bad).expect("still aside"), text);
        assert!(tmp_files(&dir.0).is_empty());
    }

    #[test]
    fn save_creates_the_parent_and_removes_a_failed_temp_file() {
        let dir = TempDir::new();
        let path = dir.0.join("a").join("b").join("arcade.toml");
        Profile::default().save(&path).expect("save");
        assert!(path.is_file());
        assert!(tmp_files(path.parent().expect("parent")).is_empty());

        let blocked = dir.0.join("blocked.toml");
        fs::create_dir(&blocked).expect("dir");
        assert!(Profile::default().save(&blocked).is_err());
        assert!(tmp_files(&dir.0).is_empty());
    }

    fn tmp_files(dir: &Path) -> Vec<String> {
        fs::read_dir(dir)
            .expect("read dir")
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.contains(".tmp"))
            .collect()
    }
}
