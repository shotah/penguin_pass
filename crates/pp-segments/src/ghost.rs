//! The rider's best first lap, played back as a ghost.

use crate::feed::{self, Sample, Step};
use crate::records::today;

/// Longest ghost that is stored, in seconds. Three hours.
pub const MAX_GHOST_S: f64 = 3.0 * 60.0 * 60.0;

/// A first sample farther than this from the start line is not a standing start.
pub const STANDING_START_M: f64 = 10.0;

const MAX_SAMPLES: usize = MAX_GHOST_S as usize + 1;

/// Distance along the course at each whole second of the best first lap.
///
/// Sample `i` is the distance at `i` seconds. The lap may finish between
/// seconds: [`Self::end_s`] is that time and [`Self::end_m`] is the lap length.
/// One hour is 3,600 samples, plus the finish when it falls between seconds.
#[derive(Debug, Clone, PartialEq)]
pub struct GhostTrace {
    samples_m: Vec<f64>,
    end_s: f64,
    end_m: f64,
    date: String,
}

/// How far the rider is from the ghost.
///
/// Both fields are positive when the rider is ahead.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GhostGap {
    /// Rider distance minus the ghost's distance at the same elapsed time, in metres.
    pub meters: f64,
    /// When the ghost reaches the rider's distance, minus the rider's elapsed time, in seconds.
    pub seconds: f64,
}

/// The first lap of this ride, from a standing start.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GhostLap {
    /// Lap time, in seconds.
    pub time_s: f64,
    /// The stored ghost's lap time, if there was one.
    pub previous_s: Option<f64>,
    /// `true` when this lap is strictly faster and should be stored.
    pub record: bool,
}

impl GhostTrace {
    pub(crate) fn try_new(
        samples_m: Vec<f64>,
        end_s: f64,
        end_m: f64,
        date: String,
    ) -> Option<Self> {
        if samples_m.is_empty()
            || samples_m.len() > MAX_SAMPLES
            || !end_s.is_finite()
            || end_s <= 0.0
            || end_s > MAX_GHOST_S + 1.0e-6
            || !end_m.is_finite()
            || end_m <= 0.0
            || date.is_empty()
        {
            return None;
        }
        let mut previous = f64::NEG_INFINITY;
        for sample in &samples_m {
            if !sample.is_finite() || *sample + 1.0e-3 < previous {
                return None;
            }
            previous = *sample;
        }
        if samples_m[0] > STANDING_START_M {
            return None;
        }
        Some(Self {
            samples_m,
            end_s,
            end_m,
            date,
        })
    }

    /// Lap time, in seconds.
    #[must_use]
    pub fn end_s(&self) -> f64 {
        self.end_s
    }

    /// Distance at the finish, in metres. This is the lap length.
    #[must_use]
    pub fn end_m(&self) -> f64 {
        self.end_m
    }

    /// UTC day the lap was ridden, `YYYY-MM-DD`.
    #[must_use]
    pub fn date(&self) -> &str {
        &self.date
    }

    /// Distance at each whole second, starting at 0.
    #[must_use]
    pub fn samples_m(&self) -> &[f64] {
        &self.samples_m
    }

    /// Distance at `elapsed_s`, interpolated between samples.
    ///
    /// `None` when the time is negative, non-finite, or past the finish.
    #[must_use]
    pub fn distance_at(&self, elapsed_s: f64) -> Option<f64> {
        if !elapsed_s.is_finite() || elapsed_s < 0.0 || elapsed_s > self.end_s + 1.0e-9 {
            return None;
        }
        if elapsed_s >= self.end_s - 1.0e-12 {
            return Some(self.end_m);
        }
        let index = elapsed_s.floor() as usize;
        let d0 = self.samples_m.get(index).copied()?;
        let t0 = index as f64;
        let (t1, d1) = if index + 1 < self.samples_m.len() {
            ((index + 1) as f64, self.samples_m[index + 1])
        } else {
            (self.end_s, self.end_m)
        };
        let span = t1 - t0;
        if span <= 1.0e-12 {
            return Some(d1);
        }
        let weight = (elapsed_s - t0) / span;
        Some(d0 + (d1 - d0) * weight)
    }

    /// Elapsed time at which the ghost was at `distance_m`.
    ///
    /// The earliest time is used when the ghost was stopped. `None` when the
    /// ghost never reached that distance.
    #[must_use]
    pub fn time_at(&self, distance_m: f64) -> Option<f64> {
        if !distance_m.is_finite() || distance_m < -1.0e-6 {
            return None;
        }
        let distance_m = distance_m.max(0.0);
        let first = self.samples_m.first().copied()?;
        if distance_m <= first + 1.0e-9 {
            return Some(0.0);
        }
        if distance_m > self.end_m + 1.0e-4 {
            return None;
        }
        if distance_m >= self.end_m - 1.0e-9 {
            return Some(self.end_s);
        }
        let mut prev_t = 0.0;
        let mut prev_d = first;
        for (index, sample) in self.samples_m.iter().copied().enumerate().skip(1) {
            if distance_m <= sample {
                return Some(interpolate(
                    prev_t,
                    prev_d,
                    index as f64,
                    sample,
                    distance_m,
                ));
            }
            prev_t = index as f64;
            prev_d = sample;
        }
        if self.end_s > prev_t {
            return Some(interpolate(
                prev_t, prev_d, self.end_s, self.end_m, distance_m,
            ));
        }
        None
    }

    /// Gap to the rider at this elapsed time and distance.
    ///
    /// `None` when the ghost has no position at `elapsed_s`, or the rider is
    /// already past the distance the ghost reached.
    #[must_use]
    pub fn gap(&self, elapsed_s: f64, rider_distance_m: f64) -> Option<GhostGap> {
        let ghost_distance_m = self.distance_at(elapsed_s)?;
        let ghost_time_s = self.time_at(rider_distance_m)?;
        Some(GhostGap {
            meters: rider_distance_m - ghost_distance_m,
            seconds: ghost_time_s - elapsed_s,
        })
    }
}

fn interpolate(t0: f64, d0: f64, t1: f64, d1: f64, distance_m: f64) -> f64 {
    let span = d1 - d0;
    if span.abs() <= 1.0e-12 {
        return t0;
    }
    let weight = ((distance_m - d0) / span).clamp(0.0, 1.0);
    t0 + weight * (t1 - t0)
}

/// Builds a [`GhostTrace`] from the same feed as [`SegmentTimer`](crate::SegmentTimer).
///
/// Only the first lap counts, and only from a standing start: the first
/// sample must be within [`STANDING_START_M`] of the start line. "Best" means
/// the fastest such lap. A later lap in the same ride is not compared.
#[derive(Debug)]
pub struct GhostRecorder {
    length_m: f64,
    best_s: Option<f64>,
    anchor: Option<Sample>,
    samples_m: Vec<f64>,
    next_second: u64,
    trace: Option<GhostTrace>,
    failed: bool,
    started: bool,
    reported: bool,
}

impl GhostRecorder {
    /// `existing` is the stored ghost when its course length matches.
    ///
    /// Only its lap time is kept, for the comparison at the line.
    #[must_use]
    pub fn new(length_m: f64, existing: Option<GhostTrace>) -> Self {
        let best_s = existing.and_then(|trace| {
            if trace.end_s().is_finite() && trace.end_s() > 0.0 {
                Some(trace.end_s())
            } else {
                None
            }
        });
        Self {
            length_m,
            best_s,
            anchor: None,
            samples_m: Vec::new(),
            next_second: 1,
            trace: None,
            failed: !(length_m.is_finite() && length_m > 0.0),
            started: false,
            reported: false,
        }
    }

    /// Feed one sample. Returns `Some` once, when the first lap finishes.
    #[must_use]
    pub fn tick(&mut self, elapsed_s: f64, total_distance_m: f64) -> Option<GhostLap> {
        if self.failed || self.reported {
            return None;
        }
        match feed::classify(self.anchor, elapsed_s, total_distance_m) {
            Step::Ignore => None,
            Step::Break(_) => {
                self.failed = true;
                self.anchor = None;
                None
            }
            Step::Start(sample) => {
                self.anchor = Some(sample);
                if sample.distance_m < 0.0
                    || sample.distance_m > STANDING_START_M
                    || sample.elapsed_s < 0.0
                {
                    self.failed = true;
                    return None;
                }
                self.started = true;
                self.samples_m.push(0.0);
                self.next_second = 1;
                let origin = Sample {
                    elapsed_s: 0.0,
                    distance_m: 0.0,
                };
                self.consume(origin, sample)
            }
            Step::Advance { prev, curr } => {
                if !self.started {
                    self.failed = true;
                    return None;
                }
                self.anchor = Some(curr);
                self.consume(prev, curr)
            }
        }
    }

    /// The trace to store when the first lap was a record.
    #[must_use]
    pub fn trace(&self) -> Option<&GhostTrace> {
        self.trace.as_ref()
    }

    fn consume(&mut self, prev: Sample, curr: Sample) -> Option<GhostLap> {
        let finishes = prev.distance_m < self.length_m && curr.distance_m + 1.0e-6 >= self.length_m;
        let end_s = if finishes {
            feed::lerp_time(prev, curr, self.length_m)
        } else {
            curr.elapsed_s
        };
        if !end_s.is_finite() || end_s > MAX_GHOST_S {
            self.failed = true;
            return None;
        }
        self.fill_seconds(prev, curr, end_s, !finishes);
        if !finishes {
            return None;
        }
        let previous_s = self.best_s;
        let record = match previous_s {
            None => true,
            Some(best) => end_s < best,
        };
        self.reported = true;
        if record {
            self.trace = GhostTrace::try_new(self.samples_m.clone(), end_s, self.length_m, today());
        }
        Some(GhostLap {
            time_s: end_s,
            previous_s,
            record: record && self.trace.is_some(),
        })
    }

    fn fill_seconds(&mut self, prev: Sample, curr: Sample, up_to_s: f64, include_end: bool) {
        loop {
            if self.samples_m.len() >= MAX_SAMPLES {
                break;
            }
            let t = self.next_second as f64;
            if t > curr.elapsed_s + 1.0e-9 || t > up_to_s + 1.0e-9 {
                break;
            }
            if !include_end && t >= up_to_s - 1.0e-9 {
                break;
            }
            let floor = self.samples_m.last().copied().unwrap_or(0.0);
            let distance = feed::lerp_distance(prev, curr, t).clamp(floor, self.length_m);
            self.samples_m.push(distance);
            self.next_second += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record_lap(
        length_m: f64,
        speed: f64,
        hz: f64,
        existing: Option<GhostTrace>,
    ) -> (GhostRecorder, GhostLap) {
        let mut recorder = GhostRecorder::new(length_m, existing);
        let _ = recorder.tick(0.0, 0.0);
        let dt = 1.0 / hz;
        let mut t = 0.0;
        let mut finished = None;
        loop {
            t += dt;
            let d = speed * t;
            if let Some(lap) = recorder.tick(t, d) {
                finished = Some(lap);
            }
            if d >= length_m {
                break;
            }
        }
        (recorder, finished.expect("first lap"))
    }

    #[test]
    fn distance_and_gap_follow_a_constant_speed_lap() {
        let length = 1_000.0;
        let speed = 10.0;
        let (recorder, _) = record_lap(length, speed, 144.0, None);
        let trace = recorder.trace().expect("first lap is a record").clone();
        assert!((trace.end_s() - length / speed).abs() < 1e-6);
        assert!((trace.distance_at(1.5).expect("mid") - 15.0).abs() < 1e-6);
        assert!(trace.distance_at(trace.end_s() + 0.1).is_none());
        assert!((trace.distance_at(trace.end_s()).expect("end") - length).abs() < 1e-6);
        let ahead = trace.gap(10.0, 150.0).expect("ahead");
        assert!((ahead.meters - 50.0).abs() < 1e-6);
        assert!((ahead.seconds - 5.0).abs() < 1e-6);
        let behind = trace.gap(10.0, 50.0).expect("behind");
        assert!((behind.meters + 50.0).abs() < 1e-6);
        assert!((behind.seconds + 5.0).abs() < 1e-6);
    }

    #[test]
    fn a_replay_of_the_ride_has_zero_gap() {
        let length = 1_000.0;
        let speeds = [5.0, 8.25, 11.5, 14.0];
        for speed in speeds {
            let (recorder, _) = record_lap(length, speed, 60.0, None);
            let trace = recorder.trace().expect("trace");
            let end = trace.end_s();
            let mut state = 0x1234_5678_9abc_def0u64;
            for i in 0..=40 {
                let t = end * f64::from(i) / 40.0;
                let d = speed * t;
                let gap = trace.gap(t, d).expect("gap");
                assert!(
                    gap.meters.abs() < 1e-4,
                    "speed {speed} t {t} m {}",
                    gap.meters
                );
                assert!(
                    gap.seconds.abs() < 1e-4,
                    "speed {speed} t {t} s {}",
                    gap.seconds
                );
            }
            for _ in 0..20 {
                state = state
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(1);
                let unit = (state >> 33) as f64 / ((1u64 << 31) as f64);
                let t = end * unit;
                let d = speed * t;
                let gap = trace.gap(t, d).expect("random gap");
                assert!(gap.meters.abs() < 1e-4 && gap.seconds.abs() < 1e-4);
            }
        }
    }

    #[test]
    fn only_a_faster_standing_start_replaces_the_ghost() {
        let (slow, slow_lap) = record_lap(500.0, 5.0, 30.0, None);
        assert!(slow_lap.record);
        let stored = slow.trace().expect("stored").clone();
        let (not_faster, lap) = record_lap(500.0, 4.0, 30.0, Some(stored.clone()));
        assert!(!lap.record);
        assert!(not_faster.trace().is_none());
        assert!((lap.previous_s.expect("old") - slow_lap.time_s).abs() < 1e-6);

        let (faster, lap) = record_lap(500.0, 10.0, 30.0, Some(stored));
        assert!(lap.record);
        assert!(faster.trace().expect("new").end_s() < lap.previous_s.expect("old"));
    }

    #[test]
    fn a_mid_course_start_or_a_huge_gap_is_not_a_ghost() {
        let mut mid = GhostRecorder::new(1_000.0, None);
        assert!(mid.tick(0.0, 100.0).is_none());
        assert!(mid.tick(90.0, 1_000.0).is_none());
        assert!(mid.trace().is_none());

        let mut jumped = GhostRecorder::new(1_000.0, None);
        let _ = jumped.tick(0.0, 0.0);
        let _ = jumped.tick(10.0, 100.0);
        assert!(jumped.tick(12.0, 900.0).is_none());
        assert!(jumped.tick(20.0, 1_000.0).is_none());
        assert!(jumped.trace().is_none());
    }

    #[test]
    fn a_lap_longer_than_three_hours_is_not_stored() {
        let mut recorder = GhostRecorder::new(108_010.0, None);
        let _ = recorder.tick(0.0, 0.0);
        let mut reported = false;
        for second in 1..=10_801 {
            let event = recorder.tick(f64::from(second), f64::from(second) * 10.0);
            if event.is_some() {
                reported = true;
            }
        }
        assert!(!reported);
        assert!(recorder.trace().is_none());

        let (kept, _) = record_lap(1_000.0, 10.0, 20.0, None);
        assert!(kept.trace().is_some());
    }
}
