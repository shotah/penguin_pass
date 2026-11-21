//! Persisted rider and game preferences.
//!
//! The file is TOML so a person can edit it. [`Settings::load`] never fails
//! the game: a missing file is the default settings, and a broken file is the
//! default settings plus a warning. The broken file is left in place until
//! the next [`Settings::save`].

use std::fs;
use std::path::{Path, PathBuf};

use pp_core::RiderProfile;
use serde::de::{self, Deserializer, Visitor};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::course_sel::CourseChoice;

/// Lowest accepted rider mass, in kilograms.
pub const MIN_RIDER_MASS_KG: f32 = 30.0;
/// Highest accepted rider mass, in kilograms.
pub const MAX_RIDER_MASS_KG: f32 = 200.0;
/// Default rider mass, in kilograms.
pub const DEFAULT_RIDER_MASS_KG: f32 = 75.0;

/// Lowest accepted bicycle mass, in kilograms. Does not include the rider.
pub const MIN_BIKE_MASS_KG: f32 = 4.0;
/// Highest accepted bicycle mass, in kilograms. Does not include the rider.
pub const MAX_BIKE_MASS_KG: f32 = 30.0;
/// Default bicycle mass, in kilograms. Does not include the rider.
pub const DEFAULT_BIKE_MASS_KG: f32 = 9.0;

/// Lowest accepted functional threshold power, in watts.
pub const MIN_FTP_W: u16 = 50;
/// Highest accepted functional threshold power, in watts.
pub const MAX_FTP_W: u16 = 600;
/// Default functional threshold power, in watts.
pub const DEFAULT_FTP_W: u16 = 200;

/// Lowest trainer difficulty. `0.0` sends a flat grade to the trainer.
pub const MIN_TRAINER_DIFFICULTY: f32 = 0.0;
/// Highest trainer difficulty. `1.0` sends the real road grade.
pub const MAX_TRAINER_DIFFICULTY: f32 = 1.0;
/// Default trainer difficulty, as a fraction of the real road grade.
pub const DEFAULT_TRAINER_DIFFICULTY: f32 = 0.5;

/// Lowest TCP port the TV dashboard may bind.
pub const MIN_DASHBOARD_PORT: u16 = 1024;
/// Highest TCP port the TV dashboard may bind.
pub const MAX_DASHBOARD_PORT: u16 = 65535;
/// Default TCP port for the TV dashboard.
pub const DEFAULT_DASHBOARD_PORT: u16 = 7878;

/// Lowest music volume. `0` is silent.
pub const MIN_MUSIC_VOLUME: u8 = 0;
/// Highest music volume.
pub const MAX_MUSIC_VOLUME: u8 = 100;
/// Default music volume, on a scale of 0 to 100.
pub const DEFAULT_MUSIC_VOLUME: u8 = 60;

const HEADER: &str = "\
# Penguin Pass settings
#
# Masses are kilograms. ftp_w is watts. trainer_difficulty is the fraction
# of the real road grade sent to the trainer (0.0 to 1.0). On-screen speed
# always uses the real grade. This only scales what the legs feel.
# tv_dashboard_port is a TCP port from 1024 to 65535.
# control_trainer: when false, the game only reads the trainer and never sends grade.
# Delete last_trainer_id to forget the remembered trainer.
# course: \"penguin_pass\", or { terrain = \"desert\", distance_km = 10, seed = 7 }.
# terrain is flat, rolling, mountain, or desert. A missing course is Penguin Pass.
# A saved course that cannot be built falls back to Penguin Pass.
# music_volume, ambient_volume, and effects_volume are 0 to 100.
# music_folder is the player's own folder of music files.
# rider_breathing: when true, a quiet breath follows the effort. Off when missing.
# Delete music_folder to choose no folder. A folder that is not on disk is kept.
# rider is the on-screen rider name, such as nils or sigrid. Unknown names are kept.
# zwift_controllers: when true, the game looks for Zwift controllers.
# heart_rate_id and heart_rate_name remember a heart-rate strap. Delete them to forget it.
# rides_folder is where FIT ride files are written. Delete it to use a rides folder
# next to the settings file.
# arcade: when true (the default), the ride has fish and powerups.
# ghost: when true (the default), the ride shows the stored ghost. Recording continues either way.
# kit is default, or a kit id such as kit-sunrise. Missing or blank loads as default.
# time_of_day is day, auto, dawn, sunset, or night. Missing loads as day.
# weather is clear, auto, cloudy, rain, snow, or fog. Missing loads as clear.
# bots_count is how many computer riders to put on the road, from 0 to 24. 0 turns them off.
# strava_auto_upload: when true (the default), a saved ride is sent to Strava if the rider is connected.
";

/// Rider, trainer, and display preferences.
///
/// Construct with [`Settings::default`], [`Settings::load`], or a struct
/// update. Values outside the documented range are corrected by
/// [`Settings::normalized`], [`Settings::load`], [`Settings::save`], and
/// [`crate::Session::apply_settings`]. A finite value outside the range is
/// moved to the nearest bound. NaN is replaced with the field default,
/// because it has no nearest bound. `+infinity` uses the upper bound and
/// `-infinity` uses the lower bound.
///
/// `trainer_difficulty` is the fraction of the real road grade sent to the
/// trainer. On-screen speed always uses the real grade. This field only
/// scales what the legs feel.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    /// Rider mass, in kilograms. Valid range [`MIN_RIDER_MASS_KG`] to [`MAX_RIDER_MASS_KG`].
    #[serde(
        default = "default_rider_mass_kg",
        deserialize_with = "de_rider_mass_kg"
    )]
    pub rider_mass_kg: f32,
    /// Bicycle mass, in kilograms, not including the rider.
    ///
    /// Valid range [`MIN_BIKE_MASS_KG`] to [`MAX_BIKE_MASS_KG`].
    #[serde(default = "default_bike_mass_kg", deserialize_with = "de_bike_mass_kg")]
    pub bike_mass_kg: f32,
    /// Functional threshold power, in watts. Valid range [`MIN_FTP_W`] to [`MAX_FTP_W`].
    #[serde(default = "default_ftp_w", deserialize_with = "de_ftp_w")]
    pub ftp_w: u16,
    /// Fraction of the real road grade sent to the trainer, from `0.0` to `1.0`.
    ///
    /// On-screen speed always uses the real grade. This only scales what the
    /// legs feel. `0.0` holds the trainer flat. `1.0` sends the road grade
    /// unchanged. The default is [`DEFAULT_TRAINER_DIFFICULTY`].
    #[serde(
        default = "default_trainer_difficulty",
        deserialize_with = "de_trainer_difficulty"
    )]
    pub trainer_difficulty: f32,
    /// Bluetooth address or name fragment to reconnect on the next launch.
    ///
    /// `None` when the rider has not connected a trainer yet. Blank text is
    /// stored as `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_trainer_id: Option<String>,
    /// Whether the LAN TV dashboard is served.
    #[serde(default = "default_true")]
    pub tv_dashboard_enabled: bool,
    /// TCP port for the TV dashboard. Valid range [`MIN_DASHBOARD_PORT`] to [`MAX_DASHBOARD_PORT`].
    #[serde(
        default = "default_dashboard_port",
        deserialize_with = "de_dashboard_port"
    )]
    pub tv_dashboard_port: u16,
    /// Whether the game window starts in fullscreen. The session stores this;
    /// Godot applies it to the window.
    #[serde(default = "default_true")]
    pub fullscreen: bool,
    /// Whether the game controls the trainer's resistance.
    ///
    /// When `true` (the default), the session asks for the Fitness Machine
    /// control point and sends the scaled road grade. When `false`, the game
    /// only reads power, cadence, and heart rate. It never takes control and
    /// never sends a grade, so the trainer stays in whatever mode the rider
    /// set on it.
    #[serde(default = "default_true")]
    pub control_trainer: bool,
    /// Course to load on the next launch.
    ///
    /// Missing from older files, which load as [`CourseChoice::PenguinPass`].
    /// A value that does not parse also loads as Penguin Pass, and the warning
    /// names it. Building the course happens when the session starts.
    #[serde(default)]
    pub course: CourseChoice,
    /// Playback volume for the rider's music, from 0 to 100.
    ///
    /// The default is [`DEFAULT_MUSIC_VOLUME`]. Godot applies this to the
    /// player. The session only stores it.
    #[serde(default = "default_music_volume", deserialize_with = "de_music_volume")]
    pub music_volume: u8,
    /// Ambience bus, from 0 to 100. Same curve as [`Self::music_volume`].
    ///
    /// Missing from older files, which load as [`DEFAULT_MUSIC_VOLUME`].
    #[serde(default = "default_music_volume", deserialize_with = "de_music_volume")]
    pub ambient_volume: u8,
    /// Effects bus, from 0 to 100. Same curve as [`Self::music_volume`].
    ///
    /// Missing from older files, which load as [`DEFAULT_MUSIC_VOLUME`].
    #[serde(default = "default_music_volume", deserialize_with = "de_music_volume")]
    pub effects_volume: u8,
    /// Quiet breathing that follows effort. Off unless the rider turns it on.
    ///
    /// Missing from older files, which load as `false`.
    #[serde(default)]
    pub rider_breathing: bool,
    /// Folder of music files the rider picked.
    ///
    /// `None` when no folder has been chosen. A path that is not on disk is
    /// kept. [`Settings::missing_music_folder`] reports that case.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub music_folder: Option<PathBuf>,
    /// On-screen rider name.
    ///
    /// The usual names are `nils` and `sigrid`. Any other text is kept.
    /// Missing from older files, which load as `nils`.
    #[serde(default = "default_rider_name")]
    pub rider: String,
    /// Whether the game should look for Zwift controllers.
    ///
    /// Missing from older files, which load as `false`.
    #[serde(default)]
    pub zwift_controllers: bool,
    /// Bluetooth address of the remembered heart-rate strap.
    ///
    /// `None` when the rider has not connected a strap, or has forgotten it.
    /// Blank text is stored as `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heart_rate_id: Option<String>,
    /// Advertised name of the remembered heart-rate strap.
    ///
    /// `None` when no strap is remembered. Blank text is stored as `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heart_rate_name: Option<String>,
    /// Directory for saved FIT rides.
    ///
    /// `None` uses a `rides` folder next to the settings file. A missing
    /// directory is created on the first save. Blank text is stored as `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rides_folder: Option<PathBuf>,
    /// Fish and powerups.
    ///
    /// Missing from older files, which load as `true`.
    #[serde(default = "default_true")]
    pub arcade: bool,
    /// Show the stored ghost while recording still runs.
    ///
    /// Missing from older files, which load as `true`. `false` hides the ghost
    /// and still records one.
    #[serde(default = "default_true")]
    pub ghost: bool,
    /// Kit laid over the chosen rider. `default` keeps that rider's colours.
    ///
    /// Missing from older files, which load as `default`.
    #[serde(default = "default_kit")]
    pub kit: String,
    /// Sky preset. `day`, `auto`, `dawn`, `sunset`, or `night`.
    ///
    /// Missing from older files, which load as `day`.
    #[serde(default = "default_time_of_day")]
    pub time_of_day: String,
    /// Weather preset. `clear`, `auto`, `cloudy`, `rain`, `snow`, or `fog`.
    ///
    /// Missing from older files, which load as `clear`.
    #[serde(default = "default_weather")]
    pub weather: String,
    /// Computer riders on the road. `0` turns them off. Clamped to `0..=24`.
    ///
    /// Missing from older files, which load as 8.
    #[serde(default = "default_bots_count")]
    pub bots_count: u8,
    /// Upload a saved FIT ride when Strava is connected.
    ///
    /// Missing from older files, which load as `true`.
    #[serde(default = "default_true")]
    pub strava_auto_upload: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            rider_mass_kg: default_rider_mass_kg(),
            bike_mass_kg: default_bike_mass_kg(),
            ftp_w: default_ftp_w(),
            trainer_difficulty: default_trainer_difficulty(),
            last_trainer_id: None,
            tv_dashboard_enabled: default_true(),
            tv_dashboard_port: default_dashboard_port(),
            fullscreen: default_true(),
            control_trainer: default_true(),
            course: CourseChoice::PenguinPass,
            music_volume: default_music_volume(),
            ambient_volume: default_music_volume(),
            effects_volume: default_music_volume(),
            rider_breathing: false,
            music_folder: None,
            rider: default_rider_name(),
            zwift_controllers: false,
            heart_rate_id: None,
            heart_rate_name: None,
            rides_folder: None,
            arcade: default_true(),
            ghost: default_true(),
            kit: default_kit(),
            time_of_day: default_time_of_day(),
            weather: default_weather(),
            bots_count: default_bots_count(),
            strava_auto_upload: default_true(),
        }
    }
}

const fn default_rider_mass_kg() -> f32 {
    DEFAULT_RIDER_MASS_KG
}

const fn default_bike_mass_kg() -> f32 {
    DEFAULT_BIKE_MASS_KG
}

const fn default_ftp_w() -> u16 {
    DEFAULT_FTP_W
}

const fn default_trainer_difficulty() -> f32 {
    DEFAULT_TRAINER_DIFFICULTY
}

const fn default_dashboard_port() -> u16 {
    DEFAULT_DASHBOARD_PORT
}

const fn default_true() -> bool {
    true
}

const fn default_music_volume() -> u8 {
    DEFAULT_MUSIC_VOLUME
}

fn clamp_volume(volume: u8) -> u8 {
    u8::try_from(round_u16(
        f64::from(volume),
        u16::from(DEFAULT_MUSIC_VOLUME),
        u16::from(MIN_MUSIC_VOLUME),
        u16::from(MAX_MUSIC_VOLUME),
    ))
    .unwrap_or(DEFAULT_MUSIC_VOLUME)
}

fn default_kit() -> String {
    "default".to_owned()
}

fn default_time_of_day() -> String {
    "day".to_owned()
}

fn default_weather() -> String {
    "clear".to_owned()
}

const fn default_bots_count() -> u8 {
    8
}

fn default_rider_name() -> String {
    "nils".to_owned()
}

impl Settings {
    /// Copy with every field inside its valid range.
    ///
    /// Finite out-of-range numbers move to the nearest bound. NaN becomes the
    /// field default. Infinities move to the corresponding bound. A blank
    /// `last_trainer_id` becomes `None`.
    #[must_use]
    pub fn normalized(&self) -> Self {
        let mut settings = self.clone();
        settings.rider_mass_kg = clamp_f32(
            f64::from(settings.rider_mass_kg),
            DEFAULT_RIDER_MASS_KG,
            MIN_RIDER_MASS_KG,
            MAX_RIDER_MASS_KG,
        );
        settings.bike_mass_kg = clamp_f32(
            f64::from(settings.bike_mass_kg),
            DEFAULT_BIKE_MASS_KG,
            MIN_BIKE_MASS_KG,
            MAX_BIKE_MASS_KG,
        );
        settings.ftp_w = round_u16(
            f64::from(settings.ftp_w),
            DEFAULT_FTP_W,
            MIN_FTP_W,
            MAX_FTP_W,
        );
        settings.trainer_difficulty = clamp_f32(
            f64::from(settings.trainer_difficulty),
            DEFAULT_TRAINER_DIFFICULTY,
            MIN_TRAINER_DIFFICULTY,
            MAX_TRAINER_DIFFICULTY,
        );
        settings.tv_dashboard_port = round_u16(
            f64::from(settings.tv_dashboard_port),
            DEFAULT_DASHBOARD_PORT,
            MIN_DASHBOARD_PORT,
            MAX_DASHBOARD_PORT,
        );
        if settings
            .last_trainer_id
            .as_ref()
            .is_some_and(|id| id.trim().is_empty())
        {
            settings.last_trainer_id = None;
        }
        settings.music_volume = clamp_volume(settings.music_volume);
        settings.ambient_volume = clamp_volume(settings.ambient_volume);
        settings.effects_volume = clamp_volume(settings.effects_volume);
        if settings.music_folder.as_ref().is_some_and(|path| {
            path.as_os_str().is_empty() || path.to_string_lossy().trim().is_empty()
        }) {
            settings.music_folder = None;
        }
        if settings.rider.trim().is_empty() {
            settings.rider = default_rider_name();
        }
        if settings.kit.trim().is_empty() {
            settings.kit = default_kit();
        }
        if settings.time_of_day.trim().is_empty() {
            settings.time_of_day = default_time_of_day();
        } else {
            settings.time_of_day = settings.time_of_day.trim().to_owned();
        }
        if settings.weather.trim().is_empty() {
            settings.weather = default_weather();
        } else {
            settings.weather = settings.weather.trim().to_owned();
        }
        settings.bots_count = settings.bots_count.min(24);
        if settings
            .heart_rate_id
            .as_ref()
            .is_some_and(|id| id.trim().is_empty())
        {
            settings.heart_rate_id = None;
        }
        if settings
            .heart_rate_name
            .as_ref()
            .is_some_and(|name| name.trim().is_empty())
        {
            settings.heart_rate_name = None;
        }
        if settings.rides_folder.as_ref().is_some_and(|path| {
            path.as_os_str().is_empty() || path.to_string_lossy().trim().is_empty()
        }) {
            settings.rides_folder = None;
        }
        settings
    }

    /// The chosen music folder when it is not on disk.
    ///
    /// `None` when no folder is set or the path exists. The stored path is
    /// left as it is either way, so the menu can say the folder is missing
    /// without forgetting the rider's choice.
    #[must_use]
    pub fn missing_music_folder(&self) -> Option<&Path> {
        self.music_folder.as_deref().filter(|path| !path.exists())
    }

    /// Rider profile for [`pp_course::Ride`], using these masses and FTP.
    ///
    /// CdA and Crr stay at [`RiderProfile`]'s defaults. The numbers are
    /// normalized first, so a NaN mass does not reach the physics.
    #[must_use]
    pub fn rider_profile(&self) -> RiderProfile {
        let settings = self.normalized();
        let defaults = RiderProfile::default();
        RiderProfile {
            rider_mass_kg: settings.rider_mass_kg,
            bike_mass_kg: settings.bike_mass_kg,
            cda_m2: defaults.cda_m2,
            crr: defaults.crr,
            ftp_w: settings.ftp_w,
        }
    }

    /// Read settings from `path`.
    ///
    /// A missing file yields [`Settings::default`] and no warning. An
    /// unreadable or malformed file yields defaults and a warning, and the
    /// file is not rewritten. Unknown keys are ignored. Missing keys use
    /// their defaults. Out-of-range and non-finite values are corrected as
    /// in [`Settings::normalized`], and the warning names each correction.
    #[must_use]
    pub fn load(path: impl AsRef<Path>) -> LoadedSettings {
        let path = path.as_ref();
        let text = match fs::read_to_string(path) {
            Ok(text) => text,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                return LoadedSettings {
                    settings: Settings::default(),
                    warning: None,
                };
            }
            Err(err) => {
                return LoadedSettings {
                    settings: Settings::default(),
                    warning: Some(format!("could not read {}: {err}", path.display())),
                };
            }
        };
        match parse_settings(&text) {
            Ok((settings, notes)) => LoadedSettings {
                settings,
                warning: join_notes(&notes),
            },
            Err(err) => LoadedSettings {
                settings: Settings::default(),
                warning: Some(format!("could not parse {}: {err}", path.display())),
            },
        }
    }

    /// Write these settings to `path`, creating the parent directory if needed.
    ///
    /// The write goes to a temporary file in the same directory and is then
    /// renamed over `path`, so a crash cannot leave a half-written settings
    /// file. The temporary file is removed if the rename fails. Values are
    /// normalized before they are written. This is the only file I/O the
    /// session performs on the caller's thread.
    ///
    /// # Errors
    ///
    /// Returns [`SettingsError`] when the directory cannot be created, the
    /// bytes cannot be written, or the rename fails.
    pub fn save(&self, path: impl AsRef<Path>) -> Result<(), SettingsError> {
        let path = path.as_ref();
        let body = encode(&self.normalized())?;
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent).map_err(SettingsError::CreateDir)?;
        }
        let tmp = temp_path(path);
        if let Err(err) = fs::write(&tmp, body.as_bytes()) {
            let _ = fs::remove_file(&tmp);
            return Err(SettingsError::Write(err));
        }
        if let Err(err) = fs::rename(&tmp, path) {
            let _ = fs::remove_file(&tmp);
            return Err(SettingsError::Rename(err));
        }
        Ok(())
    }
}

/// Settings read from disk, plus a warning when the file was not used as written.
#[derive(Debug, Clone, PartialEq)]
pub struct LoadedSettings {
    /// Settings to use. Defaults when the file was missing or unusable.
    pub settings: Settings,
    /// Why the file was missing data or could not be trusted.
    ///
    /// `None` when the file was missing, empty, or fully usable. Corrections
    /// of out-of-range or non-finite fields are reported here and are not
    /// applied back to the file until [`Settings::save`].
    pub warning: Option<String>,
}

/// Why [`Settings::save`] could not replace the file.
#[derive(Debug, Error)]
pub enum SettingsError {
    /// The parent directory could not be created.
    #[error("could not create settings directory: {0}")]
    CreateDir(#[source] std::io::Error),
    /// The temporary file could not be written.
    #[error("could not write settings file: {0}")]
    Write(#[source] std::io::Error),
    /// The temporary file could not be renamed over the destination.
    #[error("could not replace settings file: {0}")]
    Rename(#[source] std::io::Error),
    /// The settings could not be encoded as TOML.
    #[error("could not encode settings: {0}")]
    Encode(#[source] toml::ser::Error),
}

fn encode(settings: &Settings) -> Result<String, SettingsError> {
    let body = toml::to_string_pretty(settings).map_err(SettingsError::Encode)?;
    Ok(format!("{HEADER}\n{body}"))
}

fn join_notes(notes: &[String]) -> Option<String> {
    if notes.is_empty() {
        None
    } else {
        Some(notes.join("; "))
    }
}

fn parse_settings(text: &str) -> Result<(Settings, Vec<String>), String> {
    if text.trim().is_empty() {
        return Ok((Settings::default(), Vec::new()));
    }
    let quoted = quote_non_finite(text);
    let value: toml::Value = toml::from_str(&quoted).map_err(|err| err.to_string())?;
    let notes = notes_from(&value)?;
    let settings: Settings = toml::from_str(&quoted).map_err(|err| err.to_string())?;
    Ok((settings.normalized(), notes))
}

fn notes_from(value: &toml::Value) -> Result<Vec<String>, String> {
    let Some(table) = value.as_table() else {
        return Err("settings file must be a TOML table".to_owned());
    };
    let mut notes = Vec::new();
    for (key, item) in table {
        match key.as_str() {
            "rider_mass_kg" => note_f32(
                &mut notes,
                key,
                item,
                DEFAULT_RIDER_MASS_KG,
                MIN_RIDER_MASS_KG,
                MAX_RIDER_MASS_KG,
            )?,
            "bike_mass_kg" => note_f32(
                &mut notes,
                key,
                item,
                DEFAULT_BIKE_MASS_KG,
                MIN_BIKE_MASS_KG,
                MAX_BIKE_MASS_KG,
            )?,
            "trainer_difficulty" => note_f32(
                &mut notes,
                key,
                item,
                DEFAULT_TRAINER_DIFFICULTY,
                MIN_TRAINER_DIFFICULTY,
                MAX_TRAINER_DIFFICULTY,
            )?,
            "ftp_w" => note_u16(&mut notes, key, item, DEFAULT_FTP_W, MIN_FTP_W, MAX_FTP_W)?,
            "tv_dashboard_port" => note_u16(
                &mut notes,
                key,
                item,
                DEFAULT_DASHBOARD_PORT,
                MIN_DASHBOARD_PORT,
                MAX_DASHBOARD_PORT,
            )?,
            "tv_dashboard_enabled"
            | "fullscreen"
            | "control_trainer"
            | "arcade"
            | "ghost"
            | "rider_breathing"
            | "strava_auto_upload" => {
                expect_bool(key, item)?;
            }
            "last_trainer_id" => expect_string(key, item)?,
            "course" if CourseChoice::from_toml(item).is_none() => {
                notes.push("course could not be read; using Penguin Pass".to_owned());
            }
            "music_volume" | "ambient_volume" | "effects_volume" => note_u16(
                &mut notes,
                key,
                item,
                u16::from(DEFAULT_MUSIC_VOLUME),
                u16::from(MIN_MUSIC_VOLUME),
                u16::from(MAX_MUSIC_VOLUME),
            )?,
            "music_folder" => expect_string(key, item)?,
            "rider" | "heart_rate_id" | "heart_rate_name" | "rides_folder" | "time_of_day"
            | "weather" => {
                expect_string(key, item)?;
            }
            "bots_count" => note_u16(&mut notes, key, item, 8, 0, 24)?,
            "zwift_controllers" => expect_bool(key, item)?,
            _ => {}
        }
    }
    Ok(notes)
}

fn note_f32(
    notes: &mut Vec<String>,
    key: &str,
    value: &toml::Value,
    default: f32,
    min: f32,
    max: f32,
) -> Result<(), String> {
    let number = as_number(value).ok_or_else(|| format!("{key} must be a number"))?;
    if let Some(note) = f32_note(key, number, default, min, max) {
        notes.push(note);
    }
    Ok(())
}

fn note_u16(
    notes: &mut Vec<String>,
    key: &str,
    value: &toml::Value,
    default: u16,
    min: u16,
    max: u16,
) -> Result<(), String> {
    let number = as_number(value).ok_or_else(|| format!("{key} must be a number"))?;
    if let Some(note) = u16_note(key, number, default, min, max) {
        notes.push(note);
    }
    Ok(())
}

fn f32_note(key: &str, value: f64, default: f32, min: f32, max: f32) -> Option<String> {
    let clamped = clamp_f32(value, default, min, max);
    adjustment(key, value, f64::from(clamped), default)
}

fn u16_note(key: &str, value: f64, default: u16, min: u16, max: u16) -> Option<String> {
    let clamped = round_u16(value, default, min, max);
    adjustment(key, value, f64::from(clamped), default)
}

fn adjustment(
    key: &str,
    value: f64,
    clamped: f64,
    default: impl std::fmt::Display,
) -> Option<String> {
    if value.is_nan() {
        return Some(format!("{key} was not a number and was reset to {default}"));
    }
    if (clamped - value).abs() < 1e-4 {
        return None;
    }
    Some(format!("{key} {value} was clamped to {clamped}"))
}

fn expect_bool(key: &str, value: &toml::Value) -> Result<(), String> {
    if value.as_bool().is_some() {
        Ok(())
    } else {
        Err(format!("{key} must be true or false"))
    }
}

fn expect_string(key: &str, value: &toml::Value) -> Result<(), String> {
    if value.as_str().is_some() {
        Ok(())
    } else {
        Err(format!("{key} must be a string"))
    }
}

fn as_number(value: &toml::Value) -> Option<f64> {
    match value {
        toml::Value::Float(number) => Some(*number),
        toml::Value::Integer(number) => Some(*number as f64),
        toml::Value::String(text) => parse_loose_str(text),
        _ => None,
    }
}

fn parse_loose_str(text: &str) -> Option<f64> {
    let text = text.trim();
    match text.to_ascii_lowercase().as_str() {
        "nan" | "+nan" | "-nan" => Some(f64::NAN),
        "inf" | "+inf" | "infinity" | "+infinity" => Some(f64::INFINITY),
        "-inf" | "-infinity" => Some(f64::NEG_INFINITY),
        _ => text.parse().ok(),
    }
}

fn clamp_f32(value: f64, default: f32, min: f32, max: f32) -> f32 {
    if value.is_nan() {
        return default;
    }
    if value == f64::INFINITY {
        return max;
    }
    if value == f64::NEG_INFINITY {
        return min;
    }
    let value = value as f32;
    if value.is_nan() {
        return default;
    }
    if value == f32::INFINITY {
        return max;
    }
    if value == f32::NEG_INFINITY {
        return min;
    }
    value.clamp(min, max)
}

fn round_u16(value: f64, default: u16, min: u16, max: u16) -> u16 {
    if value.is_nan() {
        return default;
    }
    if value == f64::INFINITY {
        return max;
    }
    if value == f64::NEG_INFINITY || value <= 0.0 {
        return min;
    }
    if value >= f64::from(u16::MAX) {
        return max;
    }
    let rounded = value.round();
    if rounded >= f64::from(u16::MAX) {
        return max;
    }
    let whole = rounded as u16;
    whole.clamp(min, max)
}

fn temp_path(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .unwrap_or(std::ffi::OsStr::new("settings.toml"));
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

struct LooseVisitor;

impl Visitor<'_> for LooseVisitor {
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

    fn visit_str<E: de::Error>(self, value: &str) -> Result<f64, E> {
        parse_loose_str(value).ok_or_else(|| E::custom(format!("invalid number {value:?}")))
    }

    fn visit_string<E: de::Error>(self, value: String) -> Result<f64, E> {
        self.visit_str(&value)
    }
}

fn de_f32<'de, D>(deserializer: D, default: f32, min: f32, max: f32) -> Result<f32, D::Error>
where
    D: Deserializer<'de>,
{
    let value = deserializer.deserialize_any(LooseVisitor)?;
    Ok(clamp_f32(value, default, min, max))
}

fn de_u16<'de, D>(deserializer: D, default: u16, min: u16, max: u16) -> Result<u16, D::Error>
where
    D: Deserializer<'de>,
{
    let value = deserializer.deserialize_any(LooseVisitor)?;
    Ok(round_u16(value, default, min, max))
}

fn de_rider_mass_kg<'de, D: Deserializer<'de>>(deserializer: D) -> Result<f32, D::Error> {
    de_f32(
        deserializer,
        DEFAULT_RIDER_MASS_KG,
        MIN_RIDER_MASS_KG,
        MAX_RIDER_MASS_KG,
    )
}

fn de_bike_mass_kg<'de, D: Deserializer<'de>>(deserializer: D) -> Result<f32, D::Error> {
    de_f32(
        deserializer,
        DEFAULT_BIKE_MASS_KG,
        MIN_BIKE_MASS_KG,
        MAX_BIKE_MASS_KG,
    )
}

fn de_trainer_difficulty<'de, D: Deserializer<'de>>(deserializer: D) -> Result<f32, D::Error> {
    de_f32(
        deserializer,
        DEFAULT_TRAINER_DIFFICULTY,
        MIN_TRAINER_DIFFICULTY,
        MAX_TRAINER_DIFFICULTY,
    )
}

fn de_ftp_w<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u16, D::Error> {
    de_u16(deserializer, DEFAULT_FTP_W, MIN_FTP_W, MAX_FTP_W)
}

fn de_dashboard_port<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u16, D::Error> {
    de_u16(
        deserializer,
        DEFAULT_DASHBOARD_PORT,
        MIN_DASHBOARD_PORT,
        MAX_DASHBOARD_PORT,
    )
}

fn de_music_volume<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u8, D::Error> {
    let volume = de_u16(
        deserializer,
        u16::from(DEFAULT_MUSIC_VOLUME),
        u16::from(MIN_MUSIC_VOLUME),
        u16::from(MAX_MUSIC_VOLUME),
    )?;
    Ok(u8::try_from(volume).unwrap_or(DEFAULT_MUSIC_VOLUME))
}

/// Quote bare `nan` and `inf` tokens. Standard TOML rejects them, and a hand
/// edit should be clamped rather than treated as a broken file.
fn quote_non_finite(src: &str) -> String {
    let chars: Vec<char> = src.chars().collect();
    let mut out = String::with_capacity(src.len());
    let mut index = 0;
    while index < chars.len() {
        match chars[index] {
            '#' => index = copy_comment(&chars, index, &mut out),
            '"' => index = copy_basic_string(&chars, index, &mut out),
            '\'' => index = copy_literal_string(&chars, index, &mut out),
            _ => {
                if let Some((quoted, len)) = non_finite_at(&chars, index) {
                    out.push_str(quoted);
                    index += len;
                } else {
                    out.push(chars[index]);
                    index += 1;
                }
            }
        }
    }
    out
}

fn copy_comment(chars: &[char], start: usize, out: &mut String) -> usize {
    copy_while(chars, start, out, |ch| ch != '\n')
}

fn copy_basic_string(chars: &[char], start: usize, out: &mut String) -> usize {
    let mut index = start;
    out.push(chars[index]);
    index += 1;
    while index < chars.len() {
        let ch = chars[index];
        out.push(ch);
        index += 1;
        if ch == '\\' {
            if index < chars.len() {
                out.push(chars[index]);
                index += 1;
            }
            continue;
        }
        if ch == '"' {
            break;
        }
    }
    index
}

fn copy_literal_string(chars: &[char], start: usize, out: &mut String) -> usize {
    let mut index = start + 1;
    out.push('\'');
    while index < chars.len() {
        let ch = chars[index];
        out.push(ch);
        index += 1;
        if ch == '\'' {
            break;
        }
    }
    index
}

fn copy_while(
    chars: &[char],
    start: usize,
    out: &mut String,
    pred: impl Fn(char) -> bool,
) -> usize {
    let mut index = start;
    while index < chars.len() && pred(chars[index]) {
        out.push(chars[index]);
        index += 1;
    }
    index
}

fn non_finite_at(chars: &[char], index: usize) -> Option<(&'static str, usize)> {
    const WORDS: &[(&str, &str)] = &[
        ("+infinity", "\"+inf\""),
        ("-infinity", "\"-inf\""),
        ("infinity", "\"+inf\""),
        ("+nan", "\"nan\""),
        ("-nan", "\"nan\""),
        ("+inf", "\"+inf\""),
        ("-inf", "\"-inf\""),
        ("nan", "\"nan\""),
        ("inf", "\"+inf\""),
    ];
    for (word, quoted) in WORDS {
        let len = word.len();
        if starts_with_ignore_ascii_case(chars, index, word) && token_edges(chars, index, len) {
            return Some((quoted, len));
        }
    }
    None
}

fn starts_with_ignore_ascii_case(chars: &[char], index: usize, word: &str) -> bool {
    let bytes = word.as_bytes();
    if chars.len().saturating_sub(index) < bytes.len() {
        return false;
    }
    chars[index..index + bytes.len()]
        .iter()
        .zip(bytes)
        .all(|(ch, byte)| {
            ch.is_ascii() && ch.to_ascii_lowercase() as u8 == byte.to_ascii_lowercase()
        })
}

fn token_edges(chars: &[char], start: usize, len: usize) -> bool {
    let before_ok = start == 0 || is_token_edge(chars[start - 1]);
    let end = start + len;
    let after_ok = end >= chars.len() || is_token_edge(chars[end]);
    before_ok && after_ok
}

fn is_token_edge(ch: char) -> bool {
    !ch.is_ascii_alphanumeric() && ch != '_'
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "pp-session-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .expect("clock")
                    .as_nanos()
            ));
            fs::create_dir_all(&path).expect("temp dir");
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn write(dir: &TempDir, name: &str, text: &str) -> PathBuf {
        let path = dir.path().join(name);
        fs::write(&path, text).expect("write fixture");
        path
    }

    fn tmp_files(dir: &Path) -> Vec<String> {
        fs::read_dir(dir)
            .expect("read dir")
            .map(|entry| {
                entry
                    .expect("dir entry")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .filter(|name| name.contains(".tmp"))
            .collect()
    }

    #[test]
    fn defaults_match_a_typical_rider() {
        let settings = Settings::default();
        assert_eq!(settings.rider_mass_kg, 75.0);
        assert_eq!(settings.bike_mass_kg, 9.0);
        assert_eq!(settings.ftp_w, 200);
        assert_eq!(settings.trainer_difficulty, 0.5);
        assert_eq!(settings.last_trainer_id, None);
        assert!(settings.tv_dashboard_enabled);
        assert_eq!(settings.tv_dashboard_port, 7878);
        assert!(settings.fullscreen);
        assert!(settings.control_trainer);
        assert_eq!(settings.music_volume, 60);
        assert_eq!(settings.music_folder, None);
        assert_eq!(settings.rider, "nils");
        assert!(!settings.zwift_controllers);
        assert_eq!(settings.heart_rate_id, None);
        assert_eq!(settings.heart_rate_name, None);
        assert_eq!(settings.rides_folder, None);
        assert!(settings.arcade);
        assert!(settings.ghost);
        assert_eq!(settings.time_of_day, "day");
        assert_eq!(settings.weather, "clear");
        assert_eq!(settings.bots_count, 8);
        assert!(settings.strava_auto_upload);

        let profile = settings.rider_profile();
        let defaults = RiderProfile::default();
        assert_eq!(profile.rider_mass_kg, 75.0);
        assert_eq!(profile.bike_mass_kg, 9.0);
        assert_eq!(profile.ftp_w, 200);
        assert_eq!(profile.cda_m2, defaults.cda_m2);
        assert_eq!(profile.crr, defaults.crr);
    }

    #[test]
    fn round_trip_through_a_temp_file() {
        let dir = TempDir::new();
        let path = dir.path().join("nested").join("penguin-pass.toml");
        let settings = Settings {
            rider_mass_kg: 82.5,
            bike_mass_kg: 8.0,
            ftp_w: 275,
            trainer_difficulty: 0.8,
            last_trainer_id: Some("AA:BB:CC:DD:EE:FF".to_owned()),
            tv_dashboard_enabled: false,
            tv_dashboard_port: 9000,
            fullscreen: false,
            control_trainer: false,
            course: CourseChoice::Generated {
                terrain: pp_routegen::TerrainPreset::Desert,
                distance_km: 10,
                seed: 7,
            },
            music_volume: 40,
            ambient_volume: 25,
            effects_volume: 80,
            rider_breathing: true,
            music_folder: Some(PathBuf::from("/music/rides")),
            rider: "sigrid".to_owned(),
            zwift_controllers: true,
            heart_rate_id: Some("AA:BB:CC:DD:EE:FF".to_owned()),
            heart_rate_name: Some("HRM-Dual".to_owned()),
            rides_folder: Some(PathBuf::from("/tmp/penguin-rides")),
            arcade: false,
            ghost: false,
            kit: "kit-sunrise".to_owned(),
            time_of_day: "sunset".to_owned(),
            weather: "rain".to_owned(),
            bots_count: 3,
            strava_auto_upload: false,
        };

        settings.save(&path).expect("save");
        let text = fs::read_to_string(&path).expect("read back");
        assert!(text.starts_with('#'), "{text}");
        assert!(text.contains("what the legs feel"), "{text}");
        assert!(text.contains("trainer_difficulty"), "{text}");
        assert!(text.contains("terrain"), "{text}");
        assert!(text.contains("penguin_pass"), "{text}");
        assert!(text.contains("music_volume"), "{text}");
        assert!(text.contains("music_folder"), "{text}");
        assert!(text.contains("sigrid"), "{text}");
        assert!(text.contains("zwift_controllers"), "{text}");
        assert!(text.contains("heart_rate_id"), "{text}");
        assert!(text.contains("arcade"), "{text}");
        assert!(text.contains("ghost"), "{text}");
        assert!(text.contains("kit-sunrise"), "{text}");
        assert!(text.contains("sunset"), "{text}");
        assert!(text.contains("rain"), "{text}");
        assert!(text.contains("bots_count"), "{text}");
        assert!(text.contains("strava_auto_upload"), "{text}");
        assert!(tmp_files(path.parent().expect("parent")).is_empty());

        let loaded = Settings::load(&path);
        assert_eq!(loaded.warning, None);
        assert_eq!(loaded.settings, settings.normalized());
    }

    #[test]
    fn a_missing_kit_loads_as_default_and_a_chosen_kit_round_trips() {
        let dir = TempDir::new();
        let older = dir.path().join("older.toml");
        fs::write(&older, "rider = \"nils\"\n").expect("write");
        let loaded = Settings::load(&older);
        assert_eq!(loaded.settings.kit, "default");

        let settings = Settings {
            kit: "kit-glacier".to_owned(),
            ..Settings::default()
        };
        let path = dir.path().join("kit.toml");
        settings.save(&path).expect("save");
        let again = Settings::load(&path);
        assert_eq!(again.settings.kit, "kit-glacier");

        let blank = Settings {
            kit: "  ".to_owned(),
            ..Settings::default()
        };
        assert_eq!(blank.normalized().kit, "default");
    }

    #[test]
    fn missing_file_yields_defaults() {
        let dir = TempDir::new();
        let loaded = Settings::load(dir.path().join("missing.toml"));
        assert_eq!(loaded.settings, Settings::default());
        assert_eq!(loaded.warning, None);
    }

    #[test]
    fn malformed_file_yields_defaults_and_is_left_untouched() {
        let dir = TempDir::new();
        let syntax = write(&dir, "broken.toml", "rider_mass_kg = [\nthis is not toml\n");
        let before = fs::read_to_string(&syntax).expect("fixture");
        let loaded = Settings::load(&syntax);
        assert_eq!(loaded.settings, Settings::default());
        assert!(loaded.warning.is_some(), "expected a warning");
        assert_eq!(fs::read_to_string(&syntax).expect("still there"), before);

        let typed = write(&dir, "typed.toml", "fullscreen = \"yes\"\nftp_w = 240\n");
        let before = fs::read_to_string(&typed).expect("fixture");
        let loaded = Settings::load(&typed);
        assert_eq!(loaded.settings, Settings::default());
        assert!(
            loaded
                .warning
                .as_deref()
                .is_some_and(|warning| warning.contains("fullscreen")),
            "{:?}",
            loaded.warning
        );
        assert_eq!(fs::read_to_string(&typed).expect("still there"), before);
    }

    #[test]
    fn unreadable_path_yields_defaults_and_a_warning() {
        let dir = TempDir::new();
        let loaded = Settings::load(dir.path());
        assert_eq!(loaded.settings, Settings::default());
        assert!(loaded.warning.is_some(), "reading a directory should warn");
    }

    #[test]
    fn partial_file_uses_defaults_for_missing_keys() {
        let dir = TempDir::new();
        let path = write(
            &dir,
            "partial.toml",
            "# the word nan in a comment is not a value\nftp_w = 250\nunknown_hud = true\n",
        );
        let loaded = Settings::load(&path);
        assert_eq!(loaded.warning, None, "{:?}", loaded.warning);
        assert_eq!(loaded.settings.ftp_w, 250);
        assert_eq!(loaded.settings.rider_mass_kg, 75.0);
        assert_eq!(loaded.settings.bike_mass_kg, 9.0);
        assert_eq!(loaded.settings.trainer_difficulty, 0.5);
        assert!(loaded.settings.tv_dashboard_enabled);
        assert_eq!(loaded.settings.tv_dashboard_port, 7878);
        assert!(loaded.settings.fullscreen);
        assert!(loaded.settings.control_trainer);
        assert_eq!(loaded.settings.last_trainer_id, None);
        assert_eq!(loaded.settings.course, CourseChoice::PenguinPass);
        assert_eq!(loaded.settings.music_volume, DEFAULT_MUSIC_VOLUME);
        assert_eq!(loaded.settings.music_folder, None);
        assert_eq!(loaded.settings.rider, "nils");
        assert!(!loaded.settings.zwift_controllers);
        assert_eq!(loaded.settings.heart_rate_id, None);
        assert_eq!(loaded.settings.heart_rate_name, None);
        assert_eq!(loaded.settings.rides_folder, None);
        assert!(loaded.settings.arcade);
        assert!(loaded.settings.ghost);
        assert_eq!(loaded.settings.time_of_day, "day");
        assert_eq!(loaded.settings.weather, "clear");
        assert_eq!(loaded.settings.bots_count, 8);
        assert!(loaded.settings.strava_auto_upload);
        assert!(loaded.settings.missing_music_folder().is_none());
    }

    #[test]
    fn bots_count_clamps_at_24() {
        let dir = TempDir::new();
        let path = write(&dir, "bots.toml", "bots_count = 40\n");
        let loaded = Settings::load(&path);
        assert_eq!(loaded.settings.bots_count, 24);
        assert!(
            loaded
                .warning
                .as_deref()
                .is_some_and(|warning| warning.contains("bots_count")),
            "{:?}",
            loaded.warning
        );
    }

    #[test]
    fn music_volume_clamps_and_a_missing_folder_is_kept() {
        let dir = TempDir::new();
        let path = write(
            &dir,
            "music.toml",
            "music_volume = 250\nmusic_folder = \"/no/such/penguin-pass-tunes\"\n",
        );
        let loaded = Settings::load(&path);
        assert_eq!(loaded.settings.music_volume, 100);
        assert_eq!(
            loaded.settings.music_folder.as_deref(),
            Some(Path::new("/no/such/penguin-pass-tunes"))
        );
        assert_eq!(
            loaded.settings.missing_music_folder(),
            Some(Path::new("/no/such/penguin-pass-tunes"))
        );
        let warning = loaded.warning.expect("volume was clamped");
        assert!(warning.contains("music_volume"), "{warning}");

        let present = dir.path().join("tunes");
        fs::create_dir(&present).expect("folder");
        let mut settings = loaded.settings;
        settings.music_folder = Some(present.clone());
        assert!(settings.missing_music_folder().is_none());
        settings.music_volume = 140;
        assert_eq!(settings.normalized().music_volume, 100);
        settings.music_folder = Some(PathBuf::from("   "));
        assert_eq!(settings.normalized().music_folder, None);
    }

    #[test]
    fn unreadable_course_keeps_the_rest_of_the_file() {
        let dir = TempDir::new();
        let path = write(&dir, "course.toml", "ftp_w = 250\ncourse = 4\n");
        let loaded = Settings::load(&path);
        assert_eq!(loaded.settings.ftp_w, 250);
        assert_eq!(loaded.settings.course, CourseChoice::PenguinPass);
        let warning = loaded.warning.expect("course warning");
        assert!(warning.contains("course"), "{warning}");
    }

    #[test]
    fn out_of_range_and_non_finite_values_are_clamped() {
        let dir = TempDir::new();
        let path = write(
            &dir,
            "wild.toml",
            "\
rider_mass_kg = 500
bike_mass_kg = -inf
ftp_w = nan
trainer_difficulty = inf
tv_dashboard_port = 80
fullscreen = false
",
        );
        let original = fs::read_to_string(&path).expect("fixture");
        let loaded = Settings::load(&path);
        assert_eq!(fs::read_to_string(&path).expect("untouched"), original);
        assert_eq!(loaded.settings.rider_mass_kg, 200.0);
        assert_eq!(loaded.settings.bike_mass_kg, 4.0);
        assert_eq!(loaded.settings.ftp_w, 200);
        assert_eq!(loaded.settings.trainer_difficulty, 1.0);
        assert_eq!(loaded.settings.tv_dashboard_port, 1024);
        assert!(!loaded.settings.fullscreen);
        let warning = loaded.warning.expect("corrections are reported");
        assert!(warning.contains("rider_mass_kg"), "{warning}");
        assert!(warning.contains("bike_mass_kg"), "{warning}");
        assert!(warning.contains("ftp_w"), "{warning}");
        assert!(warning.contains("trainer_difficulty"), "{warning}");
        assert!(warning.contains("tv_dashboard_port"), "{warning}");

        let in_memory = Settings {
            rider_mass_kg: f32::NAN,
            bike_mass_kg: f32::NEG_INFINITY,
            trainer_difficulty: f32::INFINITY,
            ..Settings::default()
        };
        let normalized = in_memory.normalized();
        assert_eq!(normalized.rider_mass_kg, 75.0);
        assert_eq!(normalized.bike_mass_kg, 4.0);
        assert_eq!(normalized.trainer_difficulty, 1.0);

        let saved = dir.path().join("saved.toml");
        in_memory.save(&saved).expect("save clamps");
        let round = Settings::load(&saved);
        assert_eq!(round.warning, None, "{:?}", round.warning);
        assert_eq!(round.settings.rider_mass_kg, 75.0);
        assert_eq!(round.settings.bike_mass_kg, 4.0);
        assert_eq!(round.settings.trainer_difficulty, 1.0);
    }

    #[test]
    fn save_is_atomic_and_creates_the_parent_directory() {
        let dir = TempDir::new();
        let path = dir.path().join("a").join("b").join("penguin-pass.toml");
        assert!(!path.parent().expect("parent").exists());
        Settings::default().save(&path).expect("save");
        assert!(path.is_file());
        assert!(tmp_files(path.parent().expect("parent")).is_empty());
        assert!(tmp_files(dir.path()).is_empty());
    }

    #[test]
    fn audio_settings_round_trip_and_a_missing_key_uses_the_default() {
        let dir = TempDir::new();
        let path = dir.path().join("audio.toml");
        let settings = Settings {
            ambient_volume: 25,
            effects_volume: 80,
            rider_breathing: true,
            ..Settings::default()
        };
        settings.save(&path).expect("save");
        let loaded = Settings::load(&path);
        assert_eq!(loaded.warning, None);
        assert_eq!(loaded.settings.ambient_volume, 25);
        assert_eq!(loaded.settings.effects_volume, 80);
        assert!(loaded.settings.rider_breathing);

        let older = write(&dir, "older-audio.toml", "music_volume = 10\n");
        let loaded = Settings::load(&older);
        assert_eq!(loaded.settings.music_volume, 10);
        assert_eq!(loaded.settings.ambient_volume, DEFAULT_MUSIC_VOLUME);
        assert_eq!(loaded.settings.effects_volume, DEFAULT_MUSIC_VOLUME);
        assert!(!loaded.settings.rider_breathing);

        let hot = write(
            &dir,
            "hot-audio.toml",
            "ambient_volume = 250\neffects_volume = 250\n",
        );
        let loaded = Settings::load(&hot);
        assert_eq!(loaded.settings.ambient_volume, 100);
        assert_eq!(loaded.settings.effects_volume, 100);
        let warning = loaded.warning.expect("out of range");
        assert!(warning.contains("ambient_volume"), "{warning}");
        assert!(warning.contains("effects_volume"), "{warning}");
    }

    #[test]
    fn failed_save_removes_the_temp_file() {
        let dir = TempDir::new();
        let path = dir.path().join("penguin-pass.toml");
        fs::create_dir(&path).expect("path is a directory");
        let error = Settings::default().save(&path);
        assert!(error.is_err(), "renaming onto a directory should fail");
        assert!(tmp_files(dir.path()).is_empty());
    }
}
