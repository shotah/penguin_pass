//! Live HUD numbers, including the 3-second power average.

/// How far back [`PowerWindow`] looks for the hero power number, in seconds.
pub const POWER_AVERAGE_WINDOW_S: f64 = 3.0;

/// Smoothing window for maximum power, in seconds.
///
/// A single noisy sample cannot set the maximum. The window has to be full
/// before a new maximum is accepted.
pub const MAX_POWER_WINDOW_S: f64 = 1.0;

/// Most samples kept in the power window.
///
/// At 60 fps the 3-second window holds about 180 samples. The cap stops a
/// flood of tiny timesteps from growing the buffer.
const POWER_RING_CAP: usize = 512;

/// One tick of known power.
#[derive(Clone, Copy)]
struct PowerSample {
    /// Session clock at the end of the tick, in seconds.
    t1: f64,
    /// Length of the tick, in seconds.
    dt: f64,
    /// Power during the tick, in watts.
    power: f32,
}

/// Time-weighted power over recent session time.
///
/// The clock is the sum of accepted timesteps, not wall time, so the average
/// does not depend on the frame rate. A tick whose power is unknown (the
/// link is down or the sample is stale) advances the clock and contributes
/// nothing: it is not stored as 0 W. Samples older than the window age out,
/// so a gap longer than the window clears the average.
#[derive(Clone)]
pub(crate) struct PowerWindow {
    samples: std::collections::VecDeque<PowerSample>,
}

impl PowerWindow {
    pub(crate) fn new() -> Self {
        Self {
            samples: std::collections::VecDeque::with_capacity(64),
        }
    }

    pub(crate) fn clear(&mut self) {
        self.samples.clear();
    }

    /// Record `dt` seconds ending at `now_s`. `power_w` is `None` when unknown.
    pub(crate) fn record(&mut self, now_s: f64, dt: f64, power_w: Option<f32>) {
        if dt > 0.0
            && let Some(power) = power_w
            && power.is_finite()
        {
            if self.samples.len() == POWER_RING_CAP {
                self.samples.pop_front();
            }
            self.samples.push_back(PowerSample {
                t1: now_s,
                dt,
                power,
            });
        }
        self.expire(now_s - POWER_AVERAGE_WINDOW_S);
    }

    /// Mean known power over the last [`POWER_AVERAGE_WINDOW_S`], in watts.
    ///
    /// `None` until at least one known sample is inside the window.
    pub(crate) fn average_3s(&self, now_s: f64) -> Option<f32> {
        self.average(now_s, POWER_AVERAGE_WINDOW_S)
    }

    /// Mean known power over the last [`MAX_POWER_WINDOW_S`], in watts.
    ///
    /// `None` until that window is full, so a short spike is not a maximum.
    pub(crate) fn average_1s_full(&self, now_s: f64) -> Option<f32> {
        let (moment, weight) = self.moment(now_s, MAX_POWER_WINDOW_S);
        if weight + 1.0e-6 < MAX_POWER_WINDOW_S {
            None
        } else {
            Some((moment / weight) as f32)
        }
    }

    fn average(&self, now_s: f64, window_s: f64) -> Option<f32> {
        let (moment, weight) = self.moment(now_s, window_s);
        if weight <= 1.0e-9 {
            None
        } else {
            Some((moment / weight) as f32)
        }
    }

    fn moment(&self, now_s: f64, window_s: f64) -> (f64, f64) {
        let start = now_s - window_s;
        let mut moment = 0.0;
        let mut weight = 0.0;
        for sample in &self.samples {
            let t0 = sample.t1 - sample.dt;
            let from = t0.max(start);
            let to = sample.t1.min(now_s);
            let overlap = to - from;
            if overlap > 0.0 {
                moment += f64::from(sample.power) * overlap;
                weight += overlap;
            }
        }
        (moment, weight)
    }

    fn expire(&mut self, start: f64) {
        while self
            .samples
            .front()
            .is_some_and(|sample| sample.t1 <= start)
        {
            self.samples.pop_front();
        }
    }
}

impl Default for PowerWindow {
    fn default() -> Self {
        Self::new()
    }
}

/// Numbers the HUD prints for one frame.
///
/// `None` is an em dash. A real zero (coasting) is `Some(0.0)`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LiveReadout {
    /// Instant power, in watts. `None` when a real trainer is the source and
    /// the link is down or the sample is stale.
    pub power_w: Option<f32>,
    /// Cadence, in revolutions per minute. `None` when it is unknown.
    pub cadence_rpm: Option<f32>,
    /// Heart rate, in beats per minute. `None` when no strap is reporting.
    pub heart_rate_bpm: Option<u8>,
    /// Mean known power over the last [`POWER_AVERAGE_WINDOW_S`], in watts.
    ///
    /// `None` until a known sample is inside the window. Unknown gaps are
    /// left out of the mean and still age the older samples off the window.
    pub power_3s_w: Option<f32>,
}
