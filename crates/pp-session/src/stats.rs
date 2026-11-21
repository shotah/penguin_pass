//! Running ride totals and the frozen summary.

use pp_core::Telemetry;

use crate::events::RideEvent;

/// Speed above which elapsed time counts as moving, in meters per second.
///
/// About 1.8 km/h. A rider who is stopped does not add moving time. A slow
/// roll does.
pub const MOVING_SPEED_MPS: f32 = 0.5;

/// Heart-rate time, in seconds, required before the summary reports a heart rate.
const MIN_HEART_TIME_S: f64 = 5.0;

/// Share of elapsed time a strap must cover before the summary reports it.
const MIN_HEART_SHARE: f64 = 0.10;

/// One lap, finished or still in progress.
#[derive(Debug, Clone, PartialEq)]
pub struct LapRecord {
    /// Lap number. The first lap is 1.
    pub lap: u32,
    /// Time spent on this lap, in seconds.
    pub time_s: f64,
    /// Distance on this lap, in meters.
    pub distance_m: f64,
    /// Average power on this lap, in watts. Known zeros count.
    pub average_power_w: f32,
    /// Average speed on this lap, in meters per second.
    pub average_speed_mps: f32,
}

/// Totals for the HUD list and the ride-summary screen.
#[derive(Debug, Clone, PartialEq)]
pub struct RideSummary {
    /// Ride clock, in seconds. Matches [`Telemetry::elapsed_s`](pp_core::Telemetry::elapsed_s).
    pub elapsed_s: f64,
    /// Time spent moving, in seconds. See [`MOVING_SPEED_MPS`].
    pub moving_s: f64,
    /// Distance ridden, in meters.
    pub distance_m: f64,
    /// Elevation gained, in meters. Descents are not subtracted.
    pub ascent_m: f32,
    /// Average power, in watts.
    ///
    /// Known zeros count. Time while the trainer link is down is left out of
    /// both the energy and the time, so a gap is not stored as 0 W.
    /// `0.0` when power was never known.
    pub average_power_w: f32,
    /// Highest 1-second average power, in watts.
    ///
    /// A single sample cannot set this. The 1-second window has to be full.
    pub max_power_w: f32,
    /// Average cadence, in revolutions per minute, ignoring zeros and gaps.
    ///
    /// `None` when the rider never pedaled with a known cadence.
    pub average_cadence_rpm: Option<f32>,
    /// Average heart rate, in beats per minute.
    ///
    /// `None` unless a strap was reporting for at least 5 seconds and at
    /// least 10 % of the elapsed time.
    pub average_heart_rate_bpm: Option<f32>,
    /// Highest heart rate seen, in beats per minute, under the same rule.
    pub max_heart_rate_bpm: Option<u8>,
    /// Lap records, including the lap in progress when the ride has not ended.
    pub laps: Vec<LapRecord>,
}

#[derive(Clone, Copy)]
struct OpenLap {
    start_elapsed: f64,
    start_distance: f64,
    energy: f64,
    power_time: f64,
}

#[derive(Clone)]
pub(crate) struct RideStats {
    moving_s: f64,
    ascent_m: f32,
    last_elevation_m: Option<f32>,
    energy: f64,
    power_time: f64,
    max_power_w: f32,
    cadence_sum: f64,
    cadence_time: f64,
    heart_sum: f64,
    heart_time: f64,
    max_heart: Option<u8>,
    closed: Vec<LapRecord>,
    open: Option<OpenLap>,
    open_lap: u32,
    ended: bool,
}

impl RideStats {
    pub(crate) fn new() -> Self {
        Self {
            moving_s: 0.0,
            ascent_m: 0.0,
            last_elevation_m: None,
            energy: 0.0,
            power_time: 0.0,
            max_power_w: 0.0,
            cadence_sum: 0.0,
            cadence_time: 0.0,
            heart_sum: 0.0,
            heart_time: 0.0,
            max_heart: None,
            closed: Vec::new(),
            open: None,
            open_lap: 0,
            ended: false,
        }
    }

    pub(crate) fn clear(&mut self) {
        *self = Self::new();
    }

    /// Fold one accepted timestep. Returns a lap-completed event when the
    /// rider crosses the start line.
    pub(crate) fn record(
        &mut self,
        dt: f64,
        frame: &Telemetry,
        power_w: Option<f32>,
        cadence_rpm: Option<f32>,
        heart_rate_bpm: Option<u8>,
        max_1s_w: Option<f32>,
    ) -> Option<RideEvent> {
        if self.ended || dt <= 0.0 {
            return None;
        }
        if frame.speed_mps > MOVING_SPEED_MPS {
            self.moving_s += dt;
        }
        if let Some(previous) = self.last_elevation_m {
            let rise = frame.elevation_m - previous;
            if rise.is_finite() && rise > 0.0 {
                self.ascent_m += rise;
            }
        }
        self.last_elevation_m = Some(frame.elevation_m);
        if let Some(power) = power_w.filter(|power| power.is_finite()) {
            self.energy += f64::from(power) * dt;
            self.power_time += dt;
        }
        if let Some(max) = max_1s_w.filter(|power| power.is_finite()) {
            self.max_power_w = self.max_power_w.max(max);
        }
        if let Some(cadence) = cadence_rpm.filter(|cadence| cadence.is_finite() && *cadence > 0.0) {
            self.cadence_sum += f64::from(cadence) * dt;
            self.cadence_time += dt;
        }
        if let Some(heart) = heart_rate_bpm {
            self.heart_sum += f64::from(heart) * dt;
            self.heart_time += dt;
            self.max_heart = Some(self.max_heart.map_or(heart, |max| max.max(heart)));
        }
        self.note_lap(dt, frame, power_w)
    }

    pub(crate) fn finish(&mut self) {
        if self.ended {
            return;
        }
        self.ended = true;
        if let Some(record) = self.open_record(f64::MAX, f64::MAX)
            && (record.time_s > 0.05 || record.distance_m > 1.0)
        {
            self.closed.push(record);
        }
        self.open = None;
    }

    pub(crate) fn summary(&self, frame: &Telemetry) -> RideSummary {
        let mut laps = self.closed.clone();
        if !self.ended
            && let Some(record) = self.open_record(frame.elapsed_s, frame.distance_m)
            && (record.time_s > 0.0 || record.distance_m > 0.0)
        {
            laps.push(record);
        }
        let heart_ok = self.heart_time >= MIN_HEART_TIME_S
            && frame.elapsed_s > 0.0
            && self.heart_time >= MIN_HEART_SHARE * frame.elapsed_s;
        RideSummary {
            elapsed_s: frame.elapsed_s,
            moving_s: self.moving_s,
            distance_m: frame.distance_m,
            ascent_m: self.ascent_m,
            average_power_w: if self.power_time > 0.0 {
                (self.energy / self.power_time) as f32
            } else {
                0.0
            },
            max_power_w: self.max_power_w,
            average_cadence_rpm: (self.cadence_time > 0.0)
                .then(|| (self.cadence_sum / self.cadence_time) as f32),
            average_heart_rate_bpm: heart_ok.then(|| (self.heart_sum / self.heart_time) as f32),
            max_heart_rate_bpm: heart_ok.then_some(self.max_heart).flatten(),
            laps,
        }
    }

    fn note_lap(&mut self, dt: f64, frame: &Telemetry, power_w: Option<f32>) -> Option<RideEvent> {
        if frame.lap == 0 {
            return None;
        }
        if self.open.is_none() {
            self.open_lap = frame.lap;
            self.open = Some(self.fresh_lap(dt, frame));
        }
        if let Some(power) = power_w.filter(|power| power.is_finite())
            && let Some(open) = self.open.as_mut()
        {
            open.energy += f64::from(power) * dt;
            open.power_time += dt;
        }
        if frame.lap == self.open_lap {
            return None;
        }
        let finished = self.open_lap;
        let record = self.open_record(frame.elapsed_s, frame.distance_m);
        self.open_lap = frame.lap;
        self.open = Some(self.fresh_lap(0.0, frame));
        let record = record?;
        let time_s = record.time_s;
        self.closed.push(record);
        Some(RideEvent::LapCompleted {
            lap: finished,
            time_s,
        })
    }

    fn fresh_lap(&self, dt: f64, frame: &Telemetry) -> OpenLap {
        OpenLap {
            start_elapsed: (frame.elapsed_s - dt).max(0.0),
            start_distance: (frame.distance_m - f64::from(frame.speed_mps) * dt).max(0.0),
            energy: 0.0,
            power_time: 0.0,
        }
    }

    fn open_record(&self, elapsed_s: f64, distance_m: f64) -> Option<LapRecord> {
        let open = self.open.as_ref()?;
        let time_s = (elapsed_s - open.start_elapsed).max(0.0);
        let distance_m = (distance_m - open.start_distance).max(0.0);
        let average_power_w = if open.power_time > 0.0 {
            (open.energy / open.power_time) as f32
        } else {
            0.0
        };
        let average_speed_mps = if time_s > 0.0 {
            (distance_m / time_s) as f32
        } else {
            0.0
        };
        Some(LapRecord {
            lap: self.open_lap,
            time_s,
            distance_m,
            average_power_w,
            average_speed_mps,
        })
    }
}

impl Default for RideStats {
    fn default() -> Self {
        Self::new()
    }
}
