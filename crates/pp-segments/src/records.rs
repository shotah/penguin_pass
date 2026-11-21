//! Personal records and the best-lap ghost, in one TOML file.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::ghost::{GhostTrace, MAX_GHOST_S, STANDING_START_M};
use crate::timer::SegmentEffort;

const LENGTH_EPS_M: f64 = 1.0e-3;

const HEADER: &str = "\
# Penguin Pass segment records
#
# length_m is the lap these efforts belong to. A course whose length does
# not match the one now loaded is ignored, so a rebuilt route does not
# inherit the old times.
# time_s is the best time in seconds. tenths_s is that effort at each tenth
# of the segment, from 10% through the finish.
# The ghost is the fastest first lap from a standing start.
# samples_m[i] is the distance in metres at i seconds. end_s is the lap time.
";

/// Why [`Records::save`] could not replace the file.
#[derive(Debug)]
pub enum RecordsError {
    /// The parent directory could not be created.
    CreateDir(std::io::Error),
    /// The temporary file could not be written.
    Write(std::io::Error),
    /// The temporary file could not be renamed over the destination.
    Rename(std::io::Error),
    /// The records could not be encoded as TOML.
    Encode(toml::ser::Error),
}

impl std::fmt::Display for RecordsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CreateDir(err) => write!(f, "could not create records directory: {err}"),
            Self::Write(err) => write!(f, "could not write records file: {err}"),
            Self::Rename(err) => write!(f, "could not replace records file: {err}"),
            Self::Encode(err) => write!(f, "could not encode records: {err}"),
        }
    }
}

impl std::error::Error for RecordsError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::CreateDir(err) | Self::Write(err) | Self::Rename(err) => Some(err),
            Self::Encode(err) => Some(err),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
struct CourseStore {
    length_m: f64,
    efforts: BTreeMap<String, SegmentEffort>,
    ghost: Option<GhostTrace>,
}

impl CourseStore {
    fn new(length_m: f64) -> Self {
        Self {
            length_m,
            efforts: BTreeMap::new(),
            ghost: None,
        }
    }
}

/// Best segment times and ghosts, keyed by the course name the caller chooses.
///
/// Records and the ghost for one course live in the same file, so a crash
/// cannot save one without the other. The file is TOML. A missing file is
/// empty records. A file that does not parse is moved to `<name>.bad` and
/// replaced with empty records.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Records {
    courses: BTreeMap<String, CourseStore>,
}

impl Records {
    /// Read records from `path`.
    ///
    /// A missing file is empty and is not created. A blank file is empty.
    /// A file that cannot be parsed is moved to `<name>.bad` and the path is
    /// replaced with an empty records file. An unreadable path yields empty
    /// records and is left in place.
    #[must_use]
    pub fn load(path: impl AsRef<Path>) -> Self {
        let path = path.as_ref();
        let text = match fs::read_to_string(path) {
            Ok(text) => text,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Self::default(),
            Err(_) => return Self::default(),
        };
        if text.trim().is_empty() {
            return Self::default();
        }
        match decode(&text) {
            Ok(records) => records,
            Err(_) => {
                if quarantine(path) {
                    let _ = Self::default().save(path);
                }
                Self::default()
            }
        }
    }

    /// Write these records to `path`, creating the parent directory if needed.
    ///
    /// The bytes go to a temporary file in the same directory and are then
    /// renamed over `path`. The temporary file is removed if the rename fails.
    ///
    /// # Errors
    ///
    /// Returns [`RecordsError`] when the directory cannot be created, the
    /// bytes cannot be written, or the rename fails.
    pub fn save(&self, path: impl AsRef<Path>) -> Result<(), RecordsError> {
        let path = path.as_ref();
        let body = encode(self)?;
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent).map_err(RecordsError::CreateDir)?;
        }
        let tmp = temp_path(path);
        if let Err(err) = fs::write(&tmp, body.as_bytes()) {
            let _ = fs::remove_file(&tmp);
            return Err(RecordsError::Write(err));
        }
        if let Err(err) = fs::rename(&tmp, path) {
            let _ = fs::remove_file(&tmp);
            return Err(RecordsError::Rename(err));
        }
        Ok(())
    }

    /// Stored efforts whose lap length matches `length_m` within a millimetre.
    ///
    /// A different length means the course changed shape. Those efforts are
    /// not returned. The vector is sorted by segment id.
    #[must_use]
    pub fn efforts(&self, key: &str, length_m: f64) -> Vec<SegmentEffort> {
        self.courses
            .get(key)
            .filter(|course| length_matches(course.length_m, length_m))
            .map(|course| course.efforts.values().cloned().collect())
            .unwrap_or_default()
    }

    /// The ghost for `key` when the stored lap length matches `length_m`.
    #[must_use]
    pub fn ghost(&self, key: &str, length_m: f64) -> Option<GhostTrace> {
        self.courses.get(key).and_then(|course| {
            if length_matches(course.length_m, length_m) {
                course.ghost.clone()
            } else {
                None
            }
        })
    }

    /// Store `effort` when it is strictly faster than the one already kept.
    ///
    /// A different `length_m` drops the previous efforts and ghost for `key`,
    /// because they belong to another shape of the course. Returns whether
    /// the effort was stored.
    #[must_use]
    pub fn set_effort(&mut self, key: &str, length_m: f64, effort: SegmentEffort) -> bool {
        if key.is_empty() || !length_ok(length_m) || !effort_ok(&effort) {
            return false;
        }
        self.prepare(key, length_m);
        let Some(course) = self.courses.get_mut(key) else {
            return false;
        };
        let keep = match course.efforts.get(&effort.segment_id) {
            Some(existing) => effort.time_s < existing.time_s,
            None => true,
        };
        if keep {
            course.efforts.insert(effort.segment_id.clone(), effort);
        }
        keep
    }

    /// Store `ghost` when its first lap is strictly faster than the one kept.
    ///
    /// The ghost's finish distance must be the lap length. A different
    /// `length_m` drops the previous efforts, same as [`Self::set_effort`].
    #[must_use]
    pub fn set_ghost(&mut self, key: &str, length_m: f64, ghost: GhostTrace) -> bool {
        if key.is_empty() || !length_ok(length_m) || !ghost_matches(&ghost, length_m) {
            return false;
        }
        self.prepare(key, length_m);
        let Some(course) = self.courses.get_mut(key) else {
            return false;
        };
        let keep = match course.ghost.as_ref() {
            Some(existing) => ghost.end_s() < existing.end_s(),
            None => true,
        };
        if keep {
            course.ghost = Some(ghost);
        }
        keep
    }

    fn prepare(&mut self, key: &str, length_m: f64) {
        match self.courses.entry(key.to_owned()) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(CourseStore::new(length_m));
            }
            std::collections::btree_map::Entry::Occupied(mut entry) => {
                if !length_matches(entry.get().length_m, length_m) {
                    entry.insert(CourseStore::new(length_m));
                }
            }
        }
    }
}

fn length_ok(length_m: f64) -> bool {
    length_m.is_finite() && length_m > 0.0
}

fn length_matches(stored: f64, current: f64) -> bool {
    stored.is_finite() && current.is_finite() && (stored - current).abs() <= LENGTH_EPS_M
}

fn effort_ok(effort: &SegmentEffort) -> bool {
    !effort.segment_id.is_empty()
        && effort.time_s.is_finite()
        && effort.time_s > 0.0
        && !effort.date.is_empty()
        && effort.tenths_s.iter().all(|t| t.is_finite() && *t >= 0.0)
}

fn ghost_matches(ghost: &GhostTrace, length_m: f64) -> bool {
    ghost.end_s().is_finite()
        && ghost.end_s() > 0.0
        && ghost.end_s() <= MAX_GHOST_S + 1.0e-6
        && (ghost.end_m() - length_m).abs() <= 1.0
        && !ghost.date().is_empty()
}

pub(crate) fn today() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    format_unix_date(secs)
}

pub(crate) fn format_unix_date(secs: u64) -> String {
    let (year, month, day) = civil_from_unix_days(i64::try_from(secs / 86_400).unwrap_or(0));
    format!("{year:04}-{month:02}-{day:02}")
}

fn civil_from_unix_days(unix_days: i64) -> (i32, u32, u32) {
    let z = unix_days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = u64::try_from(z - era * 146_097).unwrap_or(0);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let year = i64::from(u32::try_from(yoe).unwrap_or(0)) + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { year + 1 } else { year };
    let year = i32::try_from(year).unwrap_or(1970);
    let month = u32::try_from(month).unwrap_or(1);
    let day = u32::try_from(day).unwrap_or(1);
    (year, month, day)
}

fn encode(records: &Records) -> Result<String, RecordsError> {
    let body = FileBody::from(records);
    let text = toml::to_string_pretty(&body).map_err(RecordsError::Encode)?;
    Ok(format!("{HEADER}\n{text}"))
}

fn decode(text: &str) -> Result<Records, toml::de::Error> {
    let body: FileBody = toml::from_str(text)?;
    Ok(Records::from(body))
}

fn quarantine(path: &Path) -> bool {
    let Some(name) = path.file_name() else {
        return false;
    };
    let mut bad_name = name.to_os_string();
    bad_name.push(".bad");
    let bad = path.with_file_name(bad_name);
    if bad == path {
        return false;
    }
    let _ = fs::remove_file(&bad);
    fs::rename(path, bad).is_ok()
}

fn temp_path(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .unwrap_or(std::ffi::OsStr::new("segments.toml"));
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
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

#[derive(Debug, Serialize, Deserialize)]
struct FileBody {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    courses: Vec<CourseBody>,
}

#[derive(Debug, Serialize, Deserialize)]
struct CourseBody {
    key: String,
    length_m: f64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    efforts: Vec<EffortBody>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ghost: Option<GhostBody>,
}

#[derive(Debug, Serialize, Deserialize)]
struct EffortBody {
    id: String,
    time_s: f64,
    date: String,
    tenths_s: Vec<f64>,
}

#[derive(Debug, Serialize, Deserialize)]
struct GhostBody {
    end_s: f64,
    end_m: f64,
    date: String,
    samples_m: Vec<f64>,
}

impl From<&Records> for FileBody {
    fn from(records: &Records) -> Self {
        let mut courses: Vec<CourseBody> = records
            .courses
            .iter()
            .map(|(key, course)| CourseBody {
                key: key.clone(),
                length_m: course.length_m,
                efforts: course
                    .efforts
                    .values()
                    .map(|effort| EffortBody {
                        id: effort.segment_id.clone(),
                        time_s: effort.time_s,
                        date: effort.date.clone(),
                        tenths_s: effort.tenths_s.to_vec(),
                    })
                    .collect(),
                ghost: course.ghost.as_ref().map(|ghost| GhostBody {
                    end_s: ghost.end_s(),
                    end_m: ghost.end_m(),
                    date: ghost.date().to_owned(),
                    samples_m: ghost.samples_m().to_vec(),
                }),
            })
            .collect();
        courses.sort_by(|a, b| a.key.cmp(&b.key));
        Self { courses }
    }
}

impl From<FileBody> for Records {
    fn from(body: FileBody) -> Self {
        let mut courses = BTreeMap::new();
        for course in body.courses {
            if course.key.is_empty() || !length_ok(course.length_m) {
                continue;
            }
            let mut store = CourseStore::new(course.length_m);
            for effort in course.efforts {
                let Some(parsed) = effort_from(effort) else {
                    continue;
                };
                let replace = match store.efforts.get(&parsed.segment_id) {
                    Some(existing) => parsed.time_s < existing.time_s,
                    None => true,
                };
                if replace {
                    store.efforts.insert(parsed.segment_id.clone(), parsed);
                }
            }
            let length_m = course.length_m;
            if let Some(ghost) = course.ghost.and_then(|body| ghost_from(body, length_m)) {
                store.ghost = Some(ghost);
            }
            courses.insert(course.key, store);
        }
        Self { courses }
    }
}

fn effort_from(body: EffortBody) -> Option<SegmentEffort> {
    if body.tenths_s.len() != 10 {
        return None;
    }
    let mut tenths_s = [0.0; 10];
    for (slot, value) in tenths_s.iter_mut().zip(body.tenths_s) {
        *slot = value;
    }
    let effort = SegmentEffort {
        segment_id: body.id,
        time_s: body.time_s,
        tenths_s,
        date: body.date,
    };
    if effort_ok(&effort) {
        Some(effort)
    } else {
        None
    }
}

fn ghost_from(body: GhostBody, length_m: f64) -> Option<GhostTrace> {
    if body.samples_m.len() > MAX_GHOST_S as usize + 1 {
        return None;
    }
    if body
        .samples_m
        .first()
        .is_some_and(|d| *d > STANDING_START_M)
    {
        return None;
    }
    let ghost = GhostTrace::try_new(body.samples_m, body.end_s, body.end_m, body.date)?;
    if ghost_matches(&ghost, length_m) {
        Some(ghost)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ghost::GhostRecorder;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "pp-segments-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
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

    fn sample_effort(id: &str, time_s: f64) -> SegmentEffort {
        let mut tenths_s = [0.0; 10];
        for (k, tenth) in tenths_s.iter_mut().enumerate() {
            *tenth = time_s * (k + 1) as f64 / 10.0;
        }
        SegmentEffort {
            segment_id: id.to_owned(),
            time_s,
            tenths_s,
            date: "2026-11-20".to_owned(),
        }
    }

    fn sample_ghost() -> GhostTrace {
        let mut recorder = GhostRecorder::new(200.0, None);
        let _ = recorder.tick(0.0, 0.0);
        let mut finished = None;
        let mut t = 0.0;
        loop {
            t += 0.05;
            if let Some(event) = recorder.tick(t, t * 10.0) {
                finished = Some(event);
            }
            if t * 10.0 >= 200.0 {
                break;
            }
        }
        assert!(finished.expect("lap").record);
        recorder.trace().expect("ghost").clone()
    }

    #[test]
    fn unix_dates_match_the_civil_calendar() {
        assert_eq!(format_unix_date(0), "1970-01-01");
        assert_eq!(format_unix_date(86_400), "1970-01-02");
        assert_eq!(format_unix_date(946_684_800), "2000-01-01");
        assert_eq!(format_unix_date(1_704_067_200), "2024-01-01");
        assert_eq!(format_unix_date(1_704_067_200 + 59 * 86_400), "2024-02-29");
    }

    #[test]
    fn round_trip_keeps_efforts_and_the_ghost() {
        let dir = TempDir::new();
        let path = dir.path().join("segments.toml");
        let mut records = Records::default();
        assert!(records.set_effort("loop", 200.0, sample_effort("lap", 30.0)));
        assert!(records.set_effort("loop", 200.0, sample_effort("sprint", 8.0)));
        assert!(!records.set_effort("loop", 200.0, sample_effort("lap", 40.0)));
        assert!(records.set_effort("loop", 200.0, sample_effort("lap", 25.0)));
        let ghost = sample_ghost();
        assert!(records.set_ghost("loop", 200.0, ghost.clone()));
        records.save(&path).expect("save");
        let loaded = Records::load(&path);
        let efforts = loaded.efforts("loop", 200.0);
        assert_eq!(efforts.len(), 2);
        let lap = efforts
            .iter()
            .find(|effort| effort.segment_id == "lap")
            .expect("lap");
        assert!((lap.time_s - 25.0).abs() < 1e-9);
        let stored = loaded.ghost("loop", 200.0).expect("ghost");
        assert_eq!(stored.samples_m().len(), ghost.samples_m().len());
        assert!((stored.end_s() - ghost.end_s()).abs() < 1e-6);
        assert!((stored.end_m() - ghost.end_m()).abs() < 1e-6);
        for (left, right) in stored.samples_m().iter().zip(ghost.samples_m()) {
            assert!((left - right).abs() < 1e-4, "{left} vs {right}");
        }
        assert_eq!(stored.date(), ghost.date());
        let text = fs::read_to_string(&path).expect("read");
        assert!(text.starts_with("# Penguin Pass segment records"));
        assert!(text.contains("loop"));
        let names = fs::read_dir(dir.path())
            .expect("dir")
            .map(|entry| {
                entry
                    .expect("entry")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .filter(|name| name.contains(".tmp"))
            .collect::<Vec<_>>();
        assert!(names.is_empty(), "{names:?}");
    }

    #[test]
    fn a_missing_file_is_empty_and_a_bad_file_is_moved_aside() {
        let dir = TempDir::new();
        let missing = dir.path().join("nope.toml");
        let loaded = Records::load(&missing);
        assert!(loaded.efforts("x", 1.0).is_empty());
        assert!(!missing.exists());

        let corrupt = dir.path().join("segments.toml");
        fs::write(&corrupt, "[[[not toml\n").expect("write");
        let loaded = Records::load(&corrupt);
        assert!(loaded.efforts("x", 1.0).is_empty());
        let bad = dir.path().join("segments.toml.bad");
        assert_eq!(fs::read_to_string(&bad).expect("bad"), "[[[not toml\n");
        let replaced = fs::read_to_string(&corrupt).expect("replaced");
        assert!(Records::load(&corrupt).efforts("x", 1.0).is_empty());
        assert!(
            replaced.contains("courses") || replaced.trim().is_empty() || replaced.starts_with('#')
        );

        let mut records = Records::default();
        assert!(records.set_effort("k", 100.0, sample_effort("lap", 10.0)));
        let truncated_path = dir.path().join("cut.toml");
        records.save(&truncated_path).expect("save");
        let full = fs::read_to_string(&truncated_path).expect("full");
        let marker = "tenths_s = [";
        let cut_at = full.find(marker).expect("tenths field") + marker.len() + 4;
        let torn = &full[..cut_at];
        fs::write(&truncated_path, torn).expect("truncate");
        let loaded = Records::load(&truncated_path);
        assert!(loaded.efforts("k", 100.0).is_empty());
        let cut_bad = fs::read_to_string(dir.path().join("cut.toml.bad")).expect("cut bad");
        assert_eq!(cut_bad, torn);
    }

    #[test]
    fn a_different_length_hides_records_until_the_shape_is_replaced() {
        let mut records = Records::default();
        assert!(records.set_effort("roll", 5_000.0, sample_effort("lap", 800.0)));
        let ghost = sample_ghost();
        // sample ghost ends at 200 m, so it does not match a 5000 m lap.
        assert!(!records.set_ghost("roll", 5_000.0, ghost));
        let (recorder, _) = {
            let mut recorder = GhostRecorder::new(5_000.0, None);
            let _ = recorder.tick(0.0, 0.0);
            let mut t = 0.0;
            loop {
                t += 1.0;
                let _ = recorder.tick(t, t * 20.0);
                if t * 20.0 >= 5_000.0 {
                    break;
                }
            }
            (recorder, ())
        };
        let ghost = recorder.trace().expect("5 km").clone();
        assert!(records.set_ghost("roll", 5_000.0, ghost));
        assert!(records.efforts("roll", 5_001.0).is_empty());
        assert!(records.ghost("roll", 5_001.0).is_none());
        assert_eq!(records.efforts("roll", 5_000.0).len(), 1);
        assert!(records.set_effort("roll", 4_000.0, sample_effort("lap", 700.0)));
        assert!(records.efforts("roll", 5_000.0).is_empty());
        assert!(records.ghost("roll", 5_000.0).is_none());
        assert_eq!(records.efforts("roll", 4_000.0).len(), 1);
    }
}
