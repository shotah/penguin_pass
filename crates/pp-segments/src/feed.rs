//! One step of the ride clock, shared by the segment timer and the ghost.

/// A single tick that jumps farther than this, in metres, is a break.
///
/// Ordinary frames stay far below it: 30 Hz at 30 m/s is about a metre.
/// A jump past this does not count as riding through the distance in between.
pub const MAX_TICK_DISTANCE_M: f64 = 500.0;

/// How close a sample must be to a start line to count as already on it, in metres.
pub(crate) const LINE_EPS_M: f64 = 1.0e-3;

#[derive(Clone, Copy, Debug)]
pub(crate) struct Sample {
    pub elapsed_s: f64,
    pub distance_m: f64,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum Step {
    /// This sample is not usable. Keep the previous anchor.
    Ignore,
    /// First usable sample.
    Start(Sample),
    /// Distance jumped. Drop any open effort and anchor here.
    Break(Sample),
    /// Time moved forward and distance did not go backwards.
    Advance { prev: Sample, curr: Sample },
}

pub(crate) fn classify(anchor: Option<Sample>, elapsed_s: f64, distance_m: f64) -> Step {
    if !elapsed_s.is_finite() || !distance_m.is_finite() {
        return Step::Ignore;
    }
    let curr = Sample {
        elapsed_s,
        distance_m,
    };
    let Some(prev) = anchor else {
        return Step::Start(curr);
    };
    if elapsed_s + 1.0e-9 < prev.elapsed_s || distance_m + 1.0e-6 < prev.distance_m {
        return Step::Ignore;
    }
    // Distance moved but the clock did not. That would time a segment at zero.
    if distance_m > prev.distance_m + 1.0e-6 && elapsed_s <= prev.elapsed_s + 1.0e-12 {
        return Step::Ignore;
    }
    if distance_m - prev.distance_m > MAX_TICK_DISTANCE_M {
        return Step::Break(curr);
    }
    if distance_m <= prev.distance_m + 1.0e-6 && elapsed_s <= prev.elapsed_s + 1.0e-12 {
        return Step::Ignore;
    }
    Step::Advance { prev, curr }
}

pub(crate) fn lerp_time(prev: Sample, curr: Sample, distance_m: f64) -> f64 {
    let span = curr.distance_m - prev.distance_m;
    if span <= 1.0e-12 {
        return curr.elapsed_s;
    }
    let weight = ((distance_m - prev.distance_m) / span).clamp(0.0, 1.0);
    prev.elapsed_s + weight * (curr.elapsed_s - prev.elapsed_s)
}

pub(crate) fn lerp_distance(prev: Sample, curr: Sample, elapsed_s: f64) -> f64 {
    let span = curr.elapsed_s - prev.elapsed_s;
    if span <= 1.0e-12 {
        return curr.distance_m;
    }
    let weight = ((elapsed_s - prev.elapsed_s) / span).clamp(0.0, 1.0);
    prev.distance_m + weight * (curr.distance_m - prev.distance_m)
}
