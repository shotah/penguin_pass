//! Per-second reduction and the ride summary.

use std::time::SystemTime;

/// Rides shorter than this are not written, in seconds.
pub const MIN_WRITTEN_ELAPSED_S: f64 = 60.0;

/// Rides shorter than this are not written, in meters.
pub const MIN_WRITTEN_DISTANCE_M: f64 = 100.0;

/// Speed above which a sample gap counts as moving, in meters per second.
///
/// About 1.8 km/h. A stop does not add moving time. A slow roll does.
pub const MOVING_SPEED_MPS: f32 = 0.5;

/// Sample gaps longer than this do not count as moving time, in seconds.
///
/// A pause of minutes stays in the elapsed clock and does not become a
/// string of invented moving seconds.
pub const MAX_MOVING_GAP_S: f64 = 5.0;

/// Rolling window for normalised power, in seconds.
pub const NORMALIZED_POWER_WINDOW_S: u32 = 30;

/// Who rode, and which course, for the file name and the activity header.
#[derive(Debug, Clone, PartialEq)]
pub struct RideMeta {
    /// Wall clock at the start of the ride, UTC.
    pub started_at: SystemTime,
    /// Course title. The file name is a slug of this.
    pub course_name: String,
    /// Rider mass, in kilograms. Kept for the caller. Not a FIT field.
    pub rider_mass_kg: f32,
    /// Bicycle mass, in kilograms, without the rider. Not a FIT field.
    pub bike_mass_kg: f32,
    /// Functional threshold power, in watts. Written as the session threshold.
    pub ftp_w: u16,
    /// Trainer the rider was connected to, when one was connected.
    pub trainer_name: Option<String>,
    /// Game version string. Written on the creator device info.
    pub game_version: String,
}

/// One game tick.
///
/// Missing sensors stay `None`. The recorder does not invent a value.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RideSample {
    /// Ride clock, in seconds, from the start of the ride.
    pub elapsed_s: f64,
    /// Instantaneous power, in watts.
    pub power_w: Option<f32>,
    /// Cadence, in revolutions per minute.
    pub cadence_rpm: Option<f32>,
    /// Heart rate, in beats per minute.
    pub heart_rate_bpm: Option<u16>,
    /// Game speed, in meters per second.
    pub speed_mps: f32,
    /// Odometer, in meters.
    pub distance_m: f64,
    /// Elevation above the course datum, in meters.
    pub altitude_m: f32,
    /// Road grade, in percent. Positive is uphill.
    pub grade_pct: f32,
    /// Course lap. The first lap is `1`. `0` is before the ride starts.
    pub lap: u32,
}

/// One stored second of the ride.
///
/// Power, cadence, and heart rate are means of the samples in that second.
/// Speed, distance, altitude, and grade are the last finite sample.
#[derive(Debug, Clone, PartialEq)]
pub struct RecordedSecond {
    /// Whole seconds from the start of the ride. The FIT timestamp is the
    /// start time plus this many seconds.
    pub elapsed_s: u64,
    /// Mean power, in watts. `None` when the second had no power sample.
    pub power_w: Option<f32>,
    /// Mean cadence, in revolutions per minute.
    pub cadence_rpm: Option<f32>,
    /// Mean heart rate, in beats per minute.
    pub heart_rate_bpm: Option<f32>,
    /// Last speed, in meters per second.
    pub speed_mps: Option<f32>,
    /// Last odometer reading, in meters.
    pub distance_m: Option<f64>,
    /// Last altitude, in meters.
    pub altitude_m: Option<f32>,
    /// Last grade, in percent.
    pub grade_pct: Option<f32>,
    /// Lap of the last sample in the second.
    pub lap: u32,
}

/// One course lap, closed when the lap number changes or the ride ends.
#[derive(Debug, Clone, PartialEq)]
pub struct LapSummary {
    /// Lap number from the samples.
    pub lap: u32,
    /// Ride clock when this lap opened, in seconds.
    pub start_s: f64,
    /// Ride clock when the next lap opened, or when the ride ended.
    pub end_s: f64,
    /// Time spent moving on this lap, in seconds.
    pub moving_s: f64,
    /// Odometer gain on this lap, in meters.
    pub distance_m: f64,
    /// Elevation gained on this lap, in meters.
    pub ascent_m: f32,
    /// Mean of the one-second power values, in watts.
    pub average_power_w: Option<f32>,
    /// Highest one-second power, in watts.
    pub max_power_w: Option<f32>,
    /// Mean of the one-second cadence values, in revolutions per minute.
    pub average_cadence_rpm: Option<f32>,
    /// Highest one-second cadence, in revolutions per minute.
    pub max_cadence_rpm: Option<f32>,
    /// Mean of the one-second heart-rate values, in beats per minute.
    pub average_heart_rate_bpm: Option<f32>,
    /// Highest one-second heart rate, in beats per minute.
    pub max_heart_rate_bpm: Option<f32>,
    /// Lap distance divided by moving time, in meters per second.
    pub average_speed_mps: Option<f32>,
    /// Highest one-second speed, in meters per second.
    pub max_speed_mps: Option<f32>,
}

/// Totals for the session message.
#[derive(Debug, Clone, PartialEq)]
pub struct ActivitySummary {
    /// Last accepted sample's ride clock, in seconds.
    pub elapsed_s: f64,
    /// Time spent above [`MOVING_SPEED_MPS`], in seconds.
    pub moving_s: f64,
    /// Last recorded odometer reading, in meters.
    pub distance_m: f64,
    /// Elevation gained between successive one-second altitudes, in meters.
    pub ascent_m: f32,
    /// Mean of the one-second power values, in watts.
    pub average_power_w: Option<f32>,
    /// Highest one-second power, in watts.
    pub max_power_w: Option<f32>,
    /// Mean cadence, in revolutions per minute.
    pub average_cadence_rpm: Option<f32>,
    /// Highest one-second cadence, in revolutions per minute.
    pub max_cadence_rpm: Option<f32>,
    /// Mean heart rate, in beats per minute.
    pub average_heart_rate_bpm: Option<f32>,
    /// Highest one-second heart rate, in beats per minute.
    pub max_heart_rate_bpm: Option<f32>,
    /// Distance divided by moving time, in meters per second.
    ///
    /// `None` when the rider never counted as moving.
    pub average_speed_mps: Option<f32>,
    /// Highest one-second speed, in meters per second.
    pub max_speed_mps: Option<f32>,
    /// Normalised power, in watts.
    ///
    /// The fourth-power mean of 30-second rolling averages. `None` when the
    /// ride is shorter than [`NORMALIZED_POWER_WINDOW_S`], or when no 30
    /// consecutive recorded seconds have power.
    pub normalized_power_w: Option<f32>,
}

/// Why [`Activity::write`] did not create a file.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SkipReason {
    /// Elapsed time was under [`MIN_WRITTEN_ELAPSED_S`].
    TooShort {
        /// Elapsed time of the ride, in seconds.
        elapsed_s: f64,
    },
    /// Distance was under [`MIN_WRITTEN_DISTANCE_M`].
    ///
    /// Checked only after the ride is long enough.
    TooShortDistance {
        /// Distance of the ride, in meters.
        distance_m: f64,
    },
}

/// Result of a successful [`Activity::write`] call.
///
/// I/O failures are the `Err` side. A ride that is too short is [`Skipped`](Self::Skipped).
#[derive(Debug)]
#[must_use]
pub enum WriteOutcome {
    /// The activity file is at this path.
    Written(std::path::PathBuf),
    /// The ride was under a skip threshold. Nothing was written.
    Skipped(SkipReason),
}

/// A finished ride, ready to write.
#[derive(Debug, Clone, PartialEq)]
pub struct Activity {
    /// The meta passed to [`Recorder::new`].
    pub meta: RideMeta,
    /// One entry per recorded second, in order.
    pub records: Vec<RecordedSecond>,
    /// One entry per course lap that received a sample.
    pub laps: Vec<LapSummary>,
    /// Session totals.
    pub summary: ActivitySummary,
}

impl Activity {
    /// Write the FIT file into `dir`.
    ///
    /// The directory is created if it is missing. The write goes to a
    /// temporary name in that directory, is flushed, then renamed. An
    /// existing file is left in place; the new file gets `-2`, `-3`, and so on.
    ///
    /// # Errors
    ///
    /// Returns an error when the directory cannot be created, the file cannot
    /// be written, or the start time cannot be stored in a FIT timestamp.
    /// A short ride is [`WriteOutcome::Skipped`], not an error.
    pub fn write(&self, dir: &std::path::Path) -> std::io::Result<WriteOutcome> {
        crate::fit::write_activity(self, dir)
    }
}

/// Collects samples and reduces them to one record per second.
#[derive(Debug)]
pub struct Recorder {
    meta: RideMeta,
    last_elapsed: Option<f64>,
    moving_s: f64,
    bucket: Option<Bucket>,
    records: Vec<RecordedSecond>,
    open_lap: Option<LapSpan>,
    laps: Vec<LapSpan>,
}

impl Recorder {
    /// Start a ride. `push` may be called on every game tick.
    #[must_use]
    pub fn new(meta: RideMeta) -> Self {
        Self {
            meta,
            last_elapsed: None,
            moving_s: 0.0,
            bucket: None,
            records: Vec::new(),
            open_lap: None,
            laps: Vec::new(),
        }
    }

    /// Fold one tick into the open second.
    ///
    /// A non-finite or negative clock, or a clock that moves backwards, is
    /// ignored. A gap of minutes does not invent the missing seconds.
    pub fn push(&mut self, sample: RideSample) {
        if !sample.elapsed_s.is_finite() || sample.elapsed_s < 0.0 {
            return;
        }
        if self
            .last_elapsed
            .is_some_and(|previous| sample.elapsed_s < previous)
        {
            return;
        }
        let Some(second) = second_index(sample.elapsed_s) else {
            return;
        };
        let previous = self.last_elapsed;
        self.note_lap(sample.lap, sample.elapsed_s);
        if let Some(previous) = previous {
            self.add_moving(sample.elapsed_s - previous, sample.speed_mps);
        }
        self.last_elapsed = Some(sample.elapsed_s);
        self.fill_bucket(second, &sample);
    }

    /// Close the open second and the open lap, and compute the summary.
    #[must_use]
    pub fn finish(mut self) -> Activity {
        self.flush_bucket();
        if let Some(lap) = self.open_lap.take() {
            self.laps.push(lap);
        }
        let summary = summarize(self.last_elapsed, self.moving_s, &self.records);
        let laps = build_laps(&self.laps, &self.records);
        Activity {
            meta: self.meta,
            records: self.records,
            laps,
            summary,
        }
    }

    fn note_lap(&mut self, lap: u32, elapsed_s: f64) {
        match self.open_lap.as_mut() {
            Some(open) if open.lap == lap => open.end_s = elapsed_s,
            Some(_) => {
                if let Some(mut open) = self.open_lap.take() {
                    open.end_s = elapsed_s;
                    self.laps.push(open);
                }
                self.open_lap = Some(LapSpan::open(lap, elapsed_s));
            }
            None => self.open_lap = Some(LapSpan::open(lap, elapsed_s)),
        }
    }

    fn add_moving(&mut self, dt_s: f64, speed_mps: f32) {
        if !dt_s.is_finite() || dt_s <= 0.0 || dt_s > MAX_MOVING_GAP_S {
            return;
        }
        let Some(speed) = non_negative(speed_mps) else {
            return;
        };
        if speed <= MOVING_SPEED_MPS {
            return;
        }
        self.moving_s += dt_s;
        if let Some(lap) = self.open_lap.as_mut() {
            lap.moving_s += dt_s;
        }
    }

    fn fill_bucket(&mut self, second: u64, sample: &RideSample) {
        if self
            .bucket
            .as_ref()
            .is_some_and(|bucket| second < bucket.second)
        {
            return;
        }
        if self
            .bucket
            .as_ref()
            .is_some_and(|bucket| second > bucket.second)
        {
            self.flush_bucket();
        }
        if self.bucket.is_none() {
            self.bucket = Some(Bucket::new(second, sample.lap));
        }
        let Some(bucket) = self.bucket.as_mut() else {
            return;
        };
        bucket.lap = sample.lap;
        if let Some(power) = sample.power_w.and_then(non_negative) {
            bucket.power.push(f64::from(power));
        }
        if let Some(cadence) = sample.cadence_rpm.and_then(non_negative) {
            bucket.cadence.push(f64::from(cadence));
        }
        if let Some(heart) = sample.heart_rate_bpm.filter(|bpm| *bpm > 0) {
            bucket.heart.push(f64::from(heart));
        }
        if let Some(speed) = non_negative(sample.speed_mps) {
            bucket.speed_mps = Some(speed);
        }
        if sample.distance_m.is_finite() && sample.distance_m >= 0.0 {
            bucket.distance_m = Some(sample.distance_m);
        }
        if sample.altitude_m.is_finite() {
            bucket.altitude_m = Some(sample.altitude_m);
        }
        if sample.grade_pct.is_finite() {
            bucket.grade_pct = Some(sample.grade_pct);
        }
    }

    fn flush_bucket(&mut self) {
        if let Some(bucket) = self.bucket.take() {
            self.records.push(bucket.finish());
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct LapSpan {
    lap: u32,
    start_s: f64,
    end_s: f64,
    moving_s: f64,
}

impl LapSpan {
    fn open(lap: u32, elapsed_s: f64) -> Self {
        Self {
            lap,
            start_s: elapsed_s,
            end_s: elapsed_s,
            moving_s: 0.0,
        }
    }
}

#[derive(Debug)]
struct Accum {
    sum: f64,
    count: u32,
}

impl Accum {
    fn new() -> Self {
        Self { sum: 0.0, count: 0 }
    }

    fn push(&mut self, value: f64) {
        self.sum += value;
        self.count = self.count.saturating_add(1);
    }

    fn mean(&self) -> Option<f32> {
        if self.count == 0 {
            None
        } else {
            Some((self.sum / f64::from(self.count)) as f32)
        }
    }
}

#[derive(Debug)]
struct Bucket {
    second: u64,
    lap: u32,
    power: Accum,
    cadence: Accum,
    heart: Accum,
    speed_mps: Option<f32>,
    distance_m: Option<f64>,
    altitude_m: Option<f32>,
    grade_pct: Option<f32>,
}

impl Bucket {
    fn new(second: u64, lap: u32) -> Self {
        Self {
            second,
            lap,
            power: Accum::new(),
            cadence: Accum::new(),
            heart: Accum::new(),
            speed_mps: None,
            distance_m: None,
            altitude_m: None,
            grade_pct: None,
        }
    }

    fn finish(self) -> RecordedSecond {
        RecordedSecond {
            elapsed_s: self.second,
            power_w: self.power.mean(),
            cadence_rpm: self.cadence.mean(),
            heart_rate_bpm: self.heart.mean(),
            speed_mps: self.speed_mps,
            distance_m: self.distance_m,
            altitude_m: self.altitude_m,
            grade_pct: self.grade_pct,
            lap: self.lap,
        }
    }
}

struct Group {
    lap: u32,
    start: usize,
    end: usize,
}

fn second_index(elapsed_s: f64) -> Option<u64> {
    let floored = elapsed_s.floor();
    if floored < 0.0 || floored > 1.0e12 {
        None
    } else {
        Some(floored as u64)
    }
}

fn non_negative(value: f32) -> Option<f32> {
    (value.is_finite() && value >= 0.0).then_some(value)
}

fn mean_and_max(values: impl Iterator<Item = f32>) -> (Option<f32>, Option<f32>) {
    let mut sum = 0.0f64;
    let mut count = 0u32;
    let mut max = None;
    for value in values.filter(|value| value.is_finite()) {
        sum += f64::from(value);
        count = count.saturating_add(1);
        max = Some(max.map_or(value, |seen: f32| seen.max(value)));
    }
    let mean = (count > 0).then(|| (sum / f64::from(count)) as f32);
    (mean, max)
}

fn summarize(
    last_elapsed: Option<f64>,
    moving_s: f64,
    records: &[RecordedSecond],
) -> ActivitySummary {
    let elapsed_s = last_elapsed.unwrap_or(0.0);
    let distance_m = records
        .iter()
        .rev()
        .find_map(|record| record.distance_m)
        .unwrap_or(0.0);
    let (average_power_w, max_power_w) =
        mean_and_max(records.iter().filter_map(|record| record.power_w));
    let (average_cadence_rpm, max_cadence_rpm) =
        mean_and_max(records.iter().filter_map(|record| record.cadence_rpm));
    let (average_heart_rate_bpm, max_heart_rate_bpm) =
        mean_and_max(records.iter().filter_map(|record| record.heart_rate_bpm));
    let max_speed_mps = records
        .iter()
        .filter_map(|record| record.speed_mps)
        .reduce(f32::max);
    let average_speed_mps = (moving_s > 0.0).then(|| (distance_m / moving_s) as f32);
    ActivitySummary {
        elapsed_s,
        moving_s,
        distance_m,
        ascent_m: ascent(records, &mut None),
        average_power_w,
        max_power_w,
        average_cadence_rpm,
        max_cadence_rpm,
        average_heart_rate_bpm,
        max_heart_rate_bpm,
        average_speed_mps,
        max_speed_mps,
        normalized_power_w: normalized_power(records, elapsed_s),
    }
}

fn ascent(records: &[RecordedSecond], previous: &mut Option<f32>) -> f32 {
    let mut gained = 0.0f32;
    for record in records {
        match record.altitude_m {
            Some(altitude) => {
                if let Some(earlier) = *previous {
                    let rise = altitude - earlier;
                    if rise.is_finite() && rise > 0.0 {
                        gained += rise;
                    }
                }
                *previous = Some(altitude);
            }
            None => *previous = None,
        }
    }
    gained
}

fn normalized_power(records: &[RecordedSecond], elapsed_s: f64) -> Option<f32> {
    let window_s = NORMALIZED_POWER_WINDOW_S;
    if elapsed_s < f64::from(window_s) {
        return None;
    }
    let Ok(window) = usize::try_from(window_s) else {
        return None;
    };
    if window == 0 {
        return None;
    }
    let powers: Vec<(u64, f64)> = records
        .iter()
        .filter_map(|record| {
            record
                .power_w
                .map(|power| (record.elapsed_s, f64::from(power)))
        })
        .collect();
    if powers.len() < window {
        return None;
    }
    let mut fourth = 0.0f64;
    let mut count = 0u32;
    let span = u64::from(window_s - 1);
    for end in (window - 1)..powers.len() {
        let start = end + 1 - window;
        if powers[end].0.wrapping_sub(powers[start].0) != span {
            continue;
        }
        let sum: f64 = powers[start..=end].iter().map(|(_, power)| *power).sum();
        let average = sum / f64::from(window_s);
        let square = average * average;
        fourth += square * square;
        count = count.saturating_add(1);
    }
    if count == 0 {
        return None;
    }
    let value = (fourth / f64::from(count)).powf(0.25);
    value.is_finite().then_some(value as f32)
}

fn group_records(records: &[RecordedSecond]) -> Vec<Group> {
    let mut groups: Vec<Group> = Vec::new();
    for (index, record) in records.iter().enumerate() {
        if let Some(last) = groups.last_mut()
            && last.lap == record.lap
        {
            last.end = index + 1;
        } else {
            groups.push(Group {
                lap: record.lap,
                start: index,
                end: index + 1,
            });
        }
    }
    groups
}

fn build_laps(spans: &[LapSpan], records: &[RecordedSecond]) -> Vec<LapSummary> {
    let groups = group_records(records);
    let mut group_index = 0;
    let mut distance_cursor = 0.0f64;
    let mut previous_altitude = None;
    let mut laps = Vec::new();
    for span in spans {
        let slice = matched_slice(records, &groups, &mut group_index, span.lap);
        let (distance_m, next_cursor) = span_distance(slice, distance_cursor);
        if !slice.is_empty() {
            distance_cursor = next_cursor;
        }
        let ascent_m = ascent(slice, &mut previous_altitude);
        laps.push(lap_summary(span, slice, distance_m, ascent_m));
    }
    while group_index < groups.len() {
        let group = &groups[group_index];
        group_index += 1;
        let slice = &records[group.start..group.end];
        let start_s = slice.first().map_or(0.0, |record| record.elapsed_s as f64);
        let end_s = slice
            .last()
            .map_or(start_s, |record| record.elapsed_s as f64);
        let span = LapSpan {
            lap: group.lap,
            start_s,
            end_s,
            moving_s: 0.0,
        };
        let (distance_m, next_cursor) = span_distance(slice, distance_cursor);
        distance_cursor = next_cursor;
        let ascent_m = ascent(slice, &mut previous_altitude);
        laps.push(lap_summary(&span, slice, distance_m, ascent_m));
    }
    laps
}

fn matched_slice<'a>(
    records: &'a [RecordedSecond],
    groups: &[Group],
    group_index: &mut usize,
    lap: u32,
) -> &'a [RecordedSecond] {
    if *group_index < groups.len() && groups[*group_index].lap == lap {
        let group = &groups[*group_index];
        *group_index += 1;
        &records[group.start..group.end]
    } else {
        &[]
    }
}

fn span_distance(slice: &[RecordedSecond], cursor: f64) -> (f64, f64) {
    let Some(end) = slice.iter().rev().find_map(|record| record.distance_m) else {
        return (0.0, cursor);
    };
    ((end - cursor).max(0.0), end)
}

fn lap_summary(
    span: &LapSpan,
    slice: &[RecordedSecond],
    distance_m: f64,
    ascent_m: f32,
) -> LapSummary {
    let (average_power_w, max_power_w) =
        mean_and_max(slice.iter().filter_map(|record| record.power_w));
    let (average_cadence_rpm, max_cadence_rpm) =
        mean_and_max(slice.iter().filter_map(|record| record.cadence_rpm));
    let (average_heart_rate_bpm, max_heart_rate_bpm) =
        mean_and_max(slice.iter().filter_map(|record| record.heart_rate_bpm));
    let max_speed_mps = slice
        .iter()
        .filter_map(|record| record.speed_mps)
        .reduce(f32::max);
    let average_speed_mps = (span.moving_s > 0.0).then(|| (distance_m / span.moving_s) as f32);
    LapSummary {
        lap: span.lap,
        start_s: span.start_s,
        end_s: span.end_s,
        moving_s: span.moving_s,
        distance_m,
        ascent_m,
        average_power_w,
        max_power_w,
        average_cadence_rpm,
        max_cadence_rpm,
        average_heart_rate_bpm,
        max_heart_rate_bpm,
        average_speed_mps,
        max_speed_mps,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn meta() -> RideMeta {
        RideMeta {
            started_at: SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000),
            course_name: "Penguin Pass".to_owned(),
            rider_mass_kg: 75.0,
            bike_mass_kg: 9.0,
            ftp_w: 200,
            trainer_name: None,
            game_version: "0.1.0".to_owned(),
        }
    }

    fn sample(elapsed_s: f64, lap: u32) -> RideSample {
        RideSample {
            elapsed_s,
            power_w: Some(100.0),
            cadence_rpm: Some(80.0),
            heart_rate_bpm: Some(120),
            speed_mps: 5.0,
            distance_m: elapsed_s * 5.0,
            altitude_m: 0.0,
            grade_pct: 0.0,
            lap,
        }
    }

    #[test]
    fn one_second_keeps_the_mean_and_the_last_sample() {
        let mut recorder = Recorder::new(meta());
        recorder.push(RideSample {
            elapsed_s: 0.0,
            power_w: Some(100.0),
            cadence_rpm: Some(80.0),
            heart_rate_bpm: Some(100),
            speed_mps: 4.0,
            distance_m: 0.0,
            altitude_m: 1.0,
            grade_pct: 1.0,
            lap: 1,
        });
        recorder.push(RideSample {
            elapsed_s: 0.4,
            power_w: Some(300.0),
            cadence_rpm: Some(100.0),
            heart_rate_bpm: Some(140),
            speed_mps: 9.0,
            distance_m: 8.0,
            altitude_m: 6.0,
            grade_pct: -2.0,
            lap: 1,
        });
        recorder.push(RideSample {
            elapsed_s: 1.2,
            power_w: None,
            cadence_rpm: None,
            heart_rate_bpm: None,
            speed_mps: 3.0,
            distance_m: 11.0,
            altitude_m: 6.0,
            grade_pct: 0.0,
            lap: 1,
        });
        let activity = recorder.finish();
        assert_eq!(activity.records.len(), 2);
        let first = &activity.records[0];
        assert_eq!(first.elapsed_s, 0);
        assert_eq!(first.power_w, Some(200.0));
        assert_eq!(first.cadence_rpm, Some(90.0));
        assert_eq!(first.heart_rate_bpm, Some(120.0));
        assert_eq!(first.speed_mps, Some(9.0));
        assert_eq!(first.distance_m, Some(8.0));
        assert_eq!(first.altitude_m, Some(6.0));
        assert_eq!(first.grade_pct, Some(-2.0));
        let second = &activity.records[1];
        assert_eq!(second.elapsed_s, 1);
        assert_eq!(second.power_w, None);
        assert_eq!(second.cadence_rpm, None);
        assert_eq!(second.heart_rate_bpm, None);
        assert_eq!(second.speed_mps, Some(3.0));
        assert_eq!(second.distance_m, Some(11.0));
    }

    #[test]
    fn summary_matches_a_hand_computed_ride() {
        let mut recorder = Recorder::new(meta());
        let mut tick = |elapsed, power, cadence, heart, speed, distance, altitude, lap| {
            recorder.push(RideSample {
                elapsed_s: elapsed,
                power_w: power,
                cadence_rpm: cadence,
                heart_rate_bpm: heart,
                speed_mps: speed,
                distance_m: distance,
                altitude_m: altitude,
                grade_pct: 0.0,
                lap,
            });
        };
        tick(0.0, Some(100.0), Some(80.0), Some(120), 5.0, 0.0, 0.0, 1);
        tick(1.0, Some(200.0), Some(100.0), Some(140), 5.0, 5.0, 3.0, 1);
        tick(2.0, Some(300.0), None, None, 0.0, 5.0, 3.0, 1);
        tick(3.0, Some(100.0), Some(90.0), Some(130), 5.0, 10.0, 4.0, 2);
        let activity = recorder.finish();
        let summary = &activity.summary;
        assert_eq!(summary.elapsed_s, 3.0);
        assert_eq!(summary.moving_s, 2.0);
        assert_eq!(summary.distance_m, 10.0);
        assert_eq!(summary.ascent_m, 4.0);
        assert_eq!(summary.average_power_w, Some(175.0));
        assert_eq!(summary.max_power_w, Some(300.0));
        assert_eq!(summary.average_cadence_rpm, Some(90.0));
        assert_eq!(summary.max_cadence_rpm, Some(100.0));
        assert_eq!(summary.average_heart_rate_bpm, Some(130.0));
        assert_eq!(summary.max_heart_rate_bpm, Some(140.0));
        assert_eq!(summary.average_speed_mps, Some(5.0));
        assert_eq!(summary.max_speed_mps, Some(5.0));
        assert_eq!(summary.normalized_power_w, None);
        assert_eq!(activity.laps.len(), 2);
        assert_eq!(activity.laps[0].lap, 1);
        assert_eq!(activity.laps[0].start_s, 0.0);
        assert_eq!(activity.laps[0].end_s, 3.0);
        assert_eq!(activity.laps[0].moving_s, 1.0);
        assert_eq!(activity.laps[0].distance_m, 5.0);
        assert_eq!(activity.laps[0].ascent_m, 3.0);
        assert_eq!(activity.laps[0].average_power_w, Some(200.0));
        assert_eq!(activity.laps[1].lap, 2);
        assert_eq!(activity.laps[1].start_s, 3.0);
        assert_eq!(activity.laps[1].end_s, 3.0);
        assert_eq!(activity.laps[1].moving_s, 1.0);
        assert_eq!(activity.laps[1].distance_m, 5.0);
        assert_eq!(activity.laps[1].ascent_m, 1.0);
        assert_eq!(activity.laps[1].average_power_w, Some(100.0));
        assert_eq!(activity.laps[1].average_speed_mps, Some(5.0));
    }

    #[test]
    fn missing_sensors_stay_absent() {
        let mut recorder = Recorder::new(meta());
        recorder.push(RideSample {
            power_w: Some(150.0),
            cadence_rpm: None,
            heart_rate_bpm: Some(0),
            ..sample(0.0, 1)
        });
        recorder.push(RideSample {
            power_w: Some(250.0),
            cadence_rpm: None,
            heart_rate_bpm: None,
            ..sample(1.0, 1)
        });
        let activity = recorder.finish();
        assert_eq!(activity.records[0].cadence_rpm, None);
        assert_eq!(activity.records[0].heart_rate_bpm, None);
        assert_eq!(activity.summary.average_power_w, Some(200.0));
        assert_eq!(activity.summary.average_cadence_rpm, None);
        assert_eq!(activity.summary.max_cadence_rpm, None);
        assert_eq!(activity.summary.average_heart_rate_bpm, None);
        assert_eq!(activity.summary.max_heart_rate_bpm, None);
    }

    #[test]
    fn gaps_do_not_invent_records() {
        let mut recorder = Recorder::new(meta());
        recorder.push(sample(1.0, 1));
        recorder.push(sample(2.0, 1));
        recorder.push(RideSample {
            elapsed_s: 180.0,
            distance_m: 20.0,
            ..sample(180.0, 1)
        });
        let activity = recorder.finish();
        let seconds: Vec<u64> = activity
            .records
            .iter()
            .map(|record| record.elapsed_s)
            .collect();
        assert_eq!(seconds, vec![1, 2, 180]);
        assert_eq!(activity.summary.moving_s, 1.0);
        assert_eq!(activity.summary.elapsed_s, 180.0);
    }

    #[test]
    fn odd_input_does_not_panic() {
        let mut recorder = Recorder::new(meta());
        recorder.push(RideSample {
            elapsed_s: f64::NAN,
            power_w: Some(f32::NAN),
            cadence_rpm: Some(-4.0),
            heart_rate_bpm: Some(0),
            speed_mps: f32::NEG_INFINITY,
            distance_m: f64::NAN,
            altitude_m: f32::NAN,
            grade_pct: f32::INFINITY,
            lap: 1,
        });
        recorder.push(RideSample {
            elapsed_s: -3.0,
            ..sample(0.0, 1)
        });
        recorder.push(RideSample {
            elapsed_s: 1.0,
            power_w: Some(f32::NAN),
            cadence_rpm: Some(-1.0),
            heart_rate_bpm: Some(0),
            speed_mps: -2.0,
            distance_m: -10.0,
            altitude_m: f32::NAN,
            grade_pct: f32::NAN,
            lap: 1,
        });
        recorder.push(sample(0.4, 1));
        recorder.push(RideSample {
            elapsed_s: 1.0,
            power_w: Some(80.0),
            cadence_rpm: None,
            heart_rate_bpm: None,
            speed_mps: f32::NAN,
            distance_m: f64::NAN,
            altitude_m: f32::NAN,
            grade_pct: f32::NAN,
            lap: 1,
        });
        let activity = recorder.finish();
        assert_eq!(activity.records.len(), 1);
        assert_eq!(activity.records[0].elapsed_s, 1);
        assert_eq!(activity.records[0].power_w, Some(80.0));
        assert_eq!(activity.records[0].cadence_rpm, None);
        assert_eq!(activity.records[0].speed_mps, None);
        assert_eq!(activity.records[0].distance_m, None);
        assert_eq!(activity.summary.elapsed_s, 1.0);
        let empty = Recorder::new(meta()).finish();
        assert!(empty.records.is_empty());
        assert_eq!(empty.summary.elapsed_s, 0.0);
        assert_eq!(empty.summary.distance_m, 0.0);
    }

    #[test]
    fn normalised_power_is_the_fourth_power_mean() {
        let mut steady = Recorder::new(meta());
        for second in 0..=30 {
            steady.push(RideSample {
                power_w: Some(200.0),
                ..sample(f64::from(second), 1)
            });
        }
        let steady = steady.finish();
        let normalized = steady.summary.normalized_power_w.expect("30 s of power");
        assert!((normalized - 200.0).abs() < 0.01, "{normalized}");

        let mut varying = Recorder::new(meta());
        for second in 0..=30 {
            let power = if second < 15 { 100.0 } else { 300.0 };
            varying.push(RideSample {
                power_w: Some(power),
                ..sample(f64::from(second), 1)
            });
        }
        let mean_a = 200.0_f64;
        let mean_b = 6200.0_f64 / 30.0;
        let expected = ((mean_a.powi(4) + mean_b.powi(4)) / 2.0).powf(0.25) as f32;
        let actual = varying
            .finish()
            .summary
            .normalized_power_w
            .expect("two windows");
        assert!((actual - expected).abs() < 0.05, "{actual} vs {expected}");

        let mut short = Recorder::new(meta());
        for second in 0..30 {
            short.push(RideSample {
                power_w: Some(200.0),
                ..sample(f64::from(second), 1)
            });
        }
        assert_eq!(short.finish().summary.normalized_power_w, None);

        let mut gapped = Recorder::new(meta());
        for second in 0..=20 {
            gapped.push(RideSample {
                power_w: Some(200.0),
                ..sample(f64::from(second), 1)
            });
        }
        for second in 100..=120 {
            gapped.push(RideSample {
                power_w: Some(200.0),
                distance_m: f64::from(second),
                ..sample(f64::from(second), 1)
            });
        }
        assert_eq!(gapped.finish().summary.normalized_power_w, None);
    }
}
