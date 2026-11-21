//! From ride telemetry to a [`PoseInput`], frame by frame.
//!
//! [`RideAnimator`] holds the little state the picture needs between frames:
//! the crank angle, the smoothed steering and effort, and whether the rider
//! is out of the saddle or tucked. It has no engine dependency.

use std::f32::consts::{FRAC_PI_2, PI};
use std::f64::consts::TAU;

use crate::pose::PoseInput;
use crate::rig::WHEEL_RADIUS;

/// Cadence at or below this is treated as not pedalling, in rpm.
const MIN_CADENCE_RPM: f32 = 0.5;
/// Largest distance one frame may cover and still count as riding, in
/// meters. A bigger step is a seek or a restart, not speed.
const MAX_STEP_M: f64 = 6.0;
/// Time constant of the speed estimate, in seconds.
const SPEED_TAU_S: f32 = 0.25;

/// Steering angle per radian of lean at walking pace.
const STEER_PER_LEAN: f32 = 0.35;
/// Largest steering angle the animator asks for, in radians (about 7 degrees).
const STEER_LIMIT_RAD: f32 = 0.12;
/// Below this speed the full steering angle is shown, in meters per second.
/// Above it the angle shrinks with speed, as it does on a real bike.
const STEER_FULL_SPEED_MPS: f32 = 5.0;
/// Smallest share of the steering angle kept at high speed.
const STEER_MIN_SHARE: f32 = 0.3;
/// Time constant of the steering, in seconds.
const STEER_TAU_S: f32 = 0.12;

/// Power over FTP that counts as a full effort.
const FULL_EFFORT_RATIO: f32 = 1.2;
/// Effort guessed on a flat road when the power is unknown.
const GUESS_EFFORT_FLAT: f32 = 0.35;
/// Effort the guess adds per percent of grade.
const GUESS_EFFORT_PER_PCT: f32 = 0.05;
/// Time constant of the effort, in seconds.
const EFFORT_TAU_S: f32 = 0.8;

/// The rider stands at or above this grade, in percent.
const STAND_ON_GRADE_PCT: f32 = 8.0;
/// A standing rider sits again below this grade, in percent.
const STAND_OFF_GRADE_PCT: f32 = 6.0;
/// The rider stands below this cadence, in rpm.
const STAND_ON_CADENCE_RPM: f32 = 68.0;
/// A standing rider sits again above this cadence, in rpm.
const STAND_OFF_CADENCE_RPM: f32 = 76.0;
/// Time to get out of the saddle or back into it, in seconds.
const STAND_BLEND_S: f32 = 0.6;

/// The rider tucks at or above this speed, in meters per second (45 km/h).
const TUCK_ON_SPEED_MPS: f32 = 12.5;
/// A tucked rider sits up below this speed, in meters per second (38 km/h).
const TUCK_OFF_SPEED_MPS: f32 = 10.5;
/// The rider tucks only on a descent at least this steep, in percent.
const TUCK_ON_GRADE_PCT: f32 = -2.0;
/// Time to drop into the tuck or sit up from it, in seconds.
const TUCK_BLEND_S: f32 = 0.8;

/// Shortest time between two changes of posture, in seconds.
const POSTURE_DWELL_S: f32 = 1.0;
/// Coasting time before the cranks are levelled or the rider tucks, in seconds.
const COAST_SETTLE_S: f32 = 0.35;
/// Time constant of levelling the cranks, in seconds.
const LEVEL_TAU_S: f32 = 0.25;

/// What the game knows about the ride this frame.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct RideSample {
    /// Total distance ridden, in meters. The wheels turn with it.
    pub distance_m: f64,
    /// Cadence, in revolutions per minute. `None` when no sensor reports.
    /// The cranks hold still at `None` and at zero.
    pub cadence_rpm: Option<f32>,
    /// Roll of the bike into the bend, in radians. Positive leans to the
    /// rider's right. The bars turn the same way.
    pub lean_rad: f32,
    /// Road grade, in percent. Positive is uphill.
    pub grade_pct: f32,
    /// Speed, in meters per second. `None` lets the animator estimate it
    /// from the change in `distance_m`.
    pub speed_mps: Option<f32>,
    /// Power as a fraction of FTP: 1.0 is riding at threshold. `None` lets
    /// the animator guess the effort from the cadence and the grade.
    pub power_ratio: Option<f32>,
}

/// Turns one [`RideSample`] per frame into a [`PoseInput`].
///
/// The wheels follow distance and the cranks follow cadence. The bars turn
/// a little into the bend. The rider gets out of the saddle on a steep
/// climb at a low cadence and tucks when coasting down a fast descent, each
/// with hysteresis so the posture does not flicker.
///
/// ```
/// use pp_rider::{RideAnimator, RideSample, RiderModel, RiderStyle};
///
/// let model = RiderModel::build(&RiderStyle::default());
/// let mut animator = RideAnimator::new();
/// let mut distance_m = 0.0;
/// for _ in 0..120 {
///     distance_m += 8.0 / 60.0;
///     let input = animator.step(
///         &RideSample {
///             distance_m,
///             cadence_rpm: Some(90.0),
///             ..RideSample::default()
///         },
///         1.0 / 60.0,
///     );
///     let _pose = model.pose(&input);
/// }
/// assert!(animator.crank_angle_rad() > 18.0);
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct RideAnimator {
    crank_rad: f64,
    wheel_rad: f64,
    level_offset_rad: f32,
    coast_s: f32,
    steer_rad: f32,
    effort: f32,
    stand: f32,
    tuck: f32,
    standing: bool,
    tucked: bool,
    since_switch_s: f32,
    speed_mps: f32,
    last_distance_m: Option<f64>,
}

impl Default for RideAnimator {
    fn default() -> Self {
        Self::new()
    }
}

impl RideAnimator {
    /// A seated rider at rest with level cranks.
    pub fn new() -> Self {
        Self {
            crank_rad: 0.0,
            wheel_rad: 0.0,
            level_offset_rad: 0.0,
            coast_s: 0.0,
            steer_rad: 0.0,
            effort: 0.0,
            stand: 0.0,
            tuck: 0.0,
            standing: false,
            tucked: false,
            since_switch_s: POSTURE_DWELL_S,
            speed_mps: 0.0,
            last_distance_m: None,
        }
    }

    /// Advance by `dt_s` seconds and return the pose input for this frame.
    ///
    /// A `dt_s` of zero (or less) is a cut, not a frame: nothing turns, and
    /// the steering, effort, and posture jump to where `sample` puts them.
    /// Use it after a seek or a restart.
    pub fn step(&mut self, sample: &RideSample, dt_s: f64) -> PoseInput {
        let cut = dt_s.is_nan() || dt_s <= 0.0;
        let dt_s = if cut { 0.0 } else { dt_s };
        let dt = dt_s as f32;

        if sample.distance_m.is_finite() {
            self.wheel_rad = sample.distance_m / f64::from(WHEEL_RADIUS);
            self.track_speed(sample, dt_s);
        }

        let cadence = sample
            .cadence_rpm
            .filter(|rpm| rpm.is_finite() && *rpm > MIN_CADENCE_RPM);
        if let Some(rpm) = cadence {
            self.crank_rad += f64::from(rpm) * dt_s * TAU / 60.0;
        }
        let power_ratio = sample.power_ratio.filter(|ratio| ratio.is_finite());
        // With no cadence sensor, only a rider known to be making power is
        // pedalling. Otherwise still cranks are a rider who is coasting.
        let coasting = match sample.cadence_rpm {
            Some(_) => cadence.is_none(),
            None => power_ratio.is_none_or(|ratio| ratio < 0.1),
        };
        self.coast_s = if coasting { self.coast_s + dt } else { 0.0 };
        let settled_coast = coasting && (cut || self.coast_s >= COAST_SETTLE_S);

        let grade = finite_or_zero(sample.grade_pct);
        self.since_switch_s += dt;
        self.update_stand(grade, cadence, cut);
        self.update_tuck(grade, settled_coast, coasting, cut);

        let effort_target = match (cadence, power_ratio) {
            (None, _) => 0.0,
            (Some(_), Some(ratio)) => (ratio / FULL_EFFORT_RATIO).clamp(0.0, 1.0),
            (Some(_), None) => (GUESS_EFFORT_FLAT + GUESS_EFFORT_PER_PCT * grade).clamp(0.15, 1.0),
        };
        let share = (STEER_FULL_SPEED_MPS / self.speed_mps.max(1.0e-3)).clamp(STEER_MIN_SHARE, 1.0);
        let steer_target = (STEER_PER_LEAN * finite_or_zero(sample.lean_rad) * share)
            .clamp(-STEER_LIMIT_RAD, STEER_LIMIT_RAD);
        let level_target = if settled_coast {
            level_offset(self.crank_rad)
        } else {
            0.0
        };

        self.effort = approach(self.effort, effort_target, dt, EFFORT_TAU_S, cut);
        self.steer_rad = approach(self.steer_rad, steer_target, dt, STEER_TAU_S, cut);
        self.level_offset_rad = approach(self.level_offset_rad, level_target, dt, LEVEL_TAU_S, cut);
        self.stand = ramp(self.stand, self.standing, dt, STAND_BLEND_S, cut);
        self.tuck = ramp(self.tuck, self.tucked, dt, TUCK_BLEND_S, cut);
        self.pose_input()
    }

    /// The pose input of the last [`step`](Self::step).
    pub fn pose_input(&self) -> PoseInput {
        PoseInput {
            crank_angle_rad: (self.crank_rad + f64::from(self.level_offset_rad)).rem_euclid(TAU)
                as f32,
            wheel_angle_rad: self.wheel_rad.rem_euclid(TAU) as f32,
            steer_rad: self.steer_rad,
            effort: self.effort,
            stand: smoothstep(self.stand),
            tuck: smoothstep(self.tuck),
        }
    }

    /// Crank rotation since the start, in radians, not wrapped. It advances
    /// with cadence and holds still when cadence is zero or missing; the
    /// levelling of the cranks while coasting is not part of it.
    pub fn crank_angle_rad(&self) -> f64 {
        self.crank_rad
    }

    /// Wheel rotation for the distance ridden, in radians, not wrapped.
    pub fn wheel_angle_rad(&self) -> f64 {
        self.wheel_rad
    }

    /// Speed the animator is working with, in meters per second: the one
    /// given in the sample, or its own estimate from the distance.
    pub fn speed_mps(&self) -> f32 {
        self.speed_mps
    }

    /// `true` while the rider is out of the saddle or getting there.
    pub fn is_standing(&self) -> bool {
        self.standing
    }

    /// `true` while the rider is tucked or dropping into the tuck.
    pub fn is_tucked(&self) -> bool {
        self.tucked
    }

    fn track_speed(&mut self, sample: &RideSample, dt_s: f64) {
        let moved = self
            .last_distance_m
            .map(|last| sample.distance_m - last)
            .filter(|moved| (0.0..=MAX_STEP_M).contains(moved));
        self.last_distance_m = Some(sample.distance_m);
        if let Some(speed) = sample.speed_mps.filter(|speed| speed.is_finite()) {
            self.speed_mps = speed.max(0.0);
        } else if let Some(moved) = moved
            && dt_s > 0.0
        {
            let measured = (moved / dt_s) as f32;
            self.speed_mps = approach(self.speed_mps, measured, dt_s as f32, SPEED_TAU_S, false);
        }
    }

    fn update_stand(&mut self, grade: f32, cadence: Option<f32>, cut: bool) {
        let wanted = if self.standing {
            grade >= STAND_OFF_GRADE_PCT && cadence.is_some_and(|rpm| rpm <= STAND_OFF_CADENCE_RPM)
        } else {
            grade >= STAND_ON_GRADE_PCT && cadence.is_some_and(|rpm| rpm < STAND_ON_CADENCE_RPM)
        };
        if wanted != self.standing && (cut || self.since_switch_s >= POSTURE_DWELL_S) {
            self.standing = wanted;
            self.since_switch_s = 0.0;
        }
    }

    fn update_tuck(&mut self, grade: f32, settled_coast: bool, coasting: bool, cut: bool) {
        let wanted = if self.tucked {
            coasting && !self.standing && self.speed_mps >= TUCK_OFF_SPEED_MPS
        } else {
            settled_coast
                && !self.standing
                && self.speed_mps >= TUCK_ON_SPEED_MPS
                && grade <= TUCK_ON_GRADE_PCT
        };
        if wanted != self.tucked && (cut || self.since_switch_s >= POSTURE_DWELL_S) {
            self.tucked = wanted;
            self.since_switch_s = 0.0;
        }
    }
}

/// Offset that turns the cranks to the nearest level position, in radians,
/// within a quarter turn either way.
fn level_offset(crank_rad: f64) -> f32 {
    let past_level = crank_rad.rem_euclid(std::f64::consts::PI) as f32;
    if past_level < FRAC_PI_2 {
        -past_level
    } else {
        PI - past_level
    }
}

/// Move `value` toward `target` with time constant `tau_s`. A cut jumps.
fn approach(value: f32, target: f32, dt: f32, tau_s: f32, cut: bool) -> f32 {
    if cut {
        target
    } else {
        value + (target - value) * (1.0 - (-dt / tau_s).exp())
    }
}

/// Move a 0..=1 blend toward on or off at a steady rate. A cut jumps.
fn ramp(value: f32, on: bool, dt: f32, duration_s: f32, cut: bool) -> f32 {
    let target = if on { 1.0 } else { 0.0 };
    if cut {
        target
    } else if on {
        (value + dt / duration_s).min(1.0)
    } else {
        (value - dt / duration_s).max(0.0)
    }
}

fn smoothstep(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn finite_or_zero(value: f32) -> f32 {
    if value.is_finite() { value } else { 0.0 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pose::MAX_STEER_RAD;

    const DT: f64 = 1.0 / 60.0;

    /// Ride for `seconds` at a steady speed, with `sample` as the template.
    fn ride(animator: &mut RideAnimator, sample: RideSample, speed_mps: f64, seconds: f64) {
        let mut sample = sample;
        for _ in 0..(seconds / DT).round() as usize {
            sample.distance_m =
                animator.wheel_angle_rad() * f64::from(WHEEL_RADIUS) + speed_mps * DT;
            animator.step(&sample, DT);
        }
    }

    fn pedalling(rpm: f32, grade_pct: f32) -> RideSample {
        RideSample {
            cadence_rpm: Some(rpm),
            grade_pct,
            ..RideSample::default()
        }
    }

    fn coasting(grade_pct: f32) -> RideSample {
        RideSample {
            cadence_rpm: Some(0.0),
            grade_pct,
            ..RideSample::default()
        }
    }

    #[test]
    fn cranks_follow_cadence_and_hold_without_it() {
        let mut animator = RideAnimator::new();
        ride(&mut animator, pedalling(90.0, 0.0), 8.0, 1.0);
        let turned = animator.crank_angle_rad();
        assert!((turned - 1.5 * TAU).abs() < 1.0e-6, "{turned}");
        ride(&mut animator, coasting(0.0), 8.0, 1.0);
        assert_eq!(animator.crank_angle_rad(), turned);
        let missing = RideSample::default();
        ride(&mut animator, missing, 8.0, 1.0);
        assert_eq!(animator.crank_angle_rad(), turned);
        ride(&mut animator, pedalling(60.0, 0.0), 8.0, 0.5);
        assert!((animator.crank_angle_rad() - turned - 0.5 * TAU).abs() < 1.0e-6);
    }

    #[test]
    fn wheels_follow_distance_and_stay_wrapped() {
        let mut animator = RideAnimator::new();
        let sample = RideSample {
            distance_m: 123_456.78,
            cadence_rpm: Some(85.0),
            ..RideSample::default()
        };
        let input = animator.step(&sample, DT);
        let expected = 123_456.78 / f64::from(WHEEL_RADIUS);
        assert!((animator.wheel_angle_rad() - expected).abs() < 1.0e-9);
        assert!((0.0..std::f32::consts::TAU).contains(&input.wheel_angle_rad));
        assert!((f64::from(input.wheel_angle_rad) - expected.rem_euclid(TAU)).abs() < 1.0e-4);
        assert!((0.0..std::f32::consts::TAU).contains(&input.crank_angle_rad));
    }

    #[test]
    fn bars_turn_into_the_bend_smoothly_and_stay_small() {
        let mut animator = RideAnimator::new();
        let mut sample = pedalling(85.0, 0.0);
        ride(&mut animator, sample, 4.0, 1.0);
        assert_eq!(animator.pose_input().steer_rad, 0.0);
        sample.lean_rad = 0.3;
        let mut last = 0.0;
        let mut largest_step = 0.0_f32;
        for _ in 0..120 {
            sample.distance_m += 4.0 * DT;
            let steer = animator.step(&sample, DT).steer_rad;
            assert!(steer >= last, "the bars must not swing back");
            largest_step = largest_step.max(steer - last);
            last = steer;
        }
        assert!(last > 0.08 && last <= STEER_LIMIT_RAD, "{last}");
        assert!(largest_step < 0.02, "{largest_step}");
        const { assert!(STEER_LIMIT_RAD < MAX_STEER_RAD) };

        // The other way, and less at speed.
        sample.lean_rad = -0.3;
        ride(&mut animator, sample, 4.0, 2.0);
        let slow = animator.pose_input().steer_rad;
        assert!((slow + last).abs() < 1.0e-3, "{slow} against {last}");
        ride(&mut animator, sample, 18.0, 3.0);
        let fast = animator.pose_input().steer_rad;
        assert!(fast < 0.0 && fast.abs() < 0.4 * slow.abs(), "{fast}");
    }

    #[test]
    fn rider_stands_on_a_steep_climb_at_a_low_cadence_only() {
        for (rpm, grade, stands) in [
            (55.0, 10.0, true),
            (85.0, 10.0, false),
            (55.0, 3.0, false),
            (0.0, 10.0, false),
        ] {
            let mut animator = RideAnimator::new();
            ride(&mut animator, pedalling(rpm, grade), 3.0, 3.0);
            assert_eq!(animator.is_standing(), stands, "{rpm} rpm at {grade} %");
            let stand = animator.pose_input().stand;
            assert_eq!(stand, if stands { 1.0 } else { 0.0 });
        }
        // No cadence sensor: the rider stays in the saddle.
        let mut animator = RideAnimator::new();
        let blind = RideSample {
            grade_pct: 12.0,
            power_ratio: Some(1.1),
            ..RideSample::default()
        };
        ride(&mut animator, blind, 3.0, 3.0);
        assert!(!animator.is_standing());
    }

    #[test]
    fn posture_does_not_flicker_when_the_ride_hovers_at_a_threshold() {
        let mut animator = RideAnimator::new();
        let mut switches = 0;
        let mut was = false;
        let mut last = 0.0_f32;
        let mut distance = 0.0;
        for frame in 0..(60 * 30) {
            let t = frame as f32 / 60.0;
            // Cadence wobbles across the stand threshold twice a second,
            // the grade across its own threshold on a different beat.
            let sample = RideSample {
                distance_m: distance,
                cadence_rpm: Some(68.0 + 3.0 * (t * 12.0).sin()),
                grade_pct: 8.0 + 0.8 * (t * 7.0).sin(),
                ..RideSample::default()
            };
            distance += 3.0 * DT;
            let stand = animator.step(&sample, DT).stand;
            assert!((stand - last).abs() < 0.05, "stand jumped at frame {frame}");
            last = stand;
            if animator.is_standing() != was {
                switches += 1;
                was = animator.is_standing();
            }
        }
        // Once up, the rider stays up: both wobbles sit inside the bands.
        assert_eq!(switches, 1);
        assert_eq!(last, 1.0);
    }

    #[test]
    fn rider_sits_down_when_the_road_eases_or_the_legs_spin_up() {
        for release in [pedalling(55.0, 4.0), pedalling(90.0, 10.0), coasting(10.0)] {
            let mut animator = RideAnimator::new();
            ride(&mut animator, pedalling(55.0, 10.0), 3.0, 3.0);
            assert!(animator.is_standing());
            ride(&mut animator, release, 3.0, 0.2);
            let part_way = animator.pose_input().stand;
            assert!(part_way > 0.0 && part_way < 1.0, "{part_way}");
            ride(&mut animator, release, 3.0, 1.0);
            assert!(!animator.is_standing());
            assert_eq!(animator.pose_input().stand, 0.0);
        }
    }

    #[test]
    fn rider_tucks_when_coasting_down_a_fast_descent() {
        let mut animator = RideAnimator::new();
        ride(&mut animator, pedalling(95.0, -6.0), 16.0, 3.0);
        assert!(!animator.is_tucked(), "a pedalling rider does not tuck");
        assert!(
            (animator.speed_mps() - 16.0).abs() < 0.1,
            "speed is estimated"
        );

        let mut last = 0.0_f32;
        let mut sample = coasting(-6.0);
        sample.distance_m = animator.wheel_angle_rad() * f64::from(WHEEL_RADIUS);
        for _ in 0..180 {
            sample.distance_m += 16.0 * DT;
            let tuck = animator.step(&sample, DT).tuck;
            assert!(tuck >= last && tuck - last < 0.05);
            last = tuck;
        }
        assert!(animator.is_tucked());
        assert_eq!(last, 1.0);
        assert!(animator.pose_input().effort < 0.05);

        // Pedalling again sits the rider up.
        ride(&mut animator, pedalling(95.0, -6.0), 16.0, 2.5);
        assert!(!animator.is_tucked());
        assert_eq!(animator.pose_input().tuck, 0.0);
    }

    #[test]
    fn rider_does_not_tuck_slowly_or_on_the_flat_and_sits_up_when_slow() {
        let mut animator = RideAnimator::new();
        ride(&mut animator, coasting(-6.0), 9.0, 3.0);
        assert!(!animator.is_tucked(), "32 km/h is not fast");
        let mut animator = RideAnimator::new();
        ride(&mut animator, coasting(0.0), 16.0, 3.0);
        assert!(!animator.is_tucked(), "the flat is not a descent");

        // Tucked on the descent, the rider holds it across the run-out and
        // sits up once the speed has gone.
        let mut animator = RideAnimator::new();
        ride(&mut animator, coasting(-6.0), 16.0, 3.0);
        assert!(animator.is_tucked());
        ride(&mut animator, coasting(0.0), 14.0, 3.0);
        assert!(animator.is_tucked());
        // Between the two speed thresholds nothing changes.
        ride(&mut animator, coasting(0.0), 11.5, 3.0);
        assert!(animator.is_tucked());
        ride(&mut animator, coasting(0.0), 9.0, 3.0);
        assert!(!animator.is_tucked());
        ride(&mut animator, coasting(-6.0), 11.5, 3.0);
        assert!(!animator.is_tucked());
    }

    #[test]
    fn a_given_speed_is_used_instead_of_the_estimate() {
        let mut animator = RideAnimator::new();
        let sample = RideSample {
            cadence_rpm: Some(0.0),
            grade_pct: -7.0,
            speed_mps: Some(17.0),
            ..RideSample::default()
        };
        // The distance does not move at all; the speed comes from the game.
        for _ in 0..120 {
            animator.step(&sample, DT);
        }
        assert_eq!(animator.speed_mps(), 17.0);
        assert!(animator.is_tucked());
    }

    #[test]
    fn effort_follows_power_and_has_a_guess_without_it() {
        let settle = |sample: RideSample| {
            let mut animator = RideAnimator::new();
            ride(&mut animator, sample, 6.0, 6.0);
            animator.pose_input().effort
        };
        let with_power = |ratio: f32| RideSample {
            power_ratio: Some(ratio),
            ..pedalling(85.0, 0.0)
        };
        assert!(settle(with_power(0.0)) < 0.01);
        assert!((settle(with_power(0.6)) - 0.5).abs() < 0.01);
        assert!((settle(with_power(2.0)) - 1.0).abs() < 0.01);
        let flat = settle(pedalling(85.0, 0.0));
        let climb = settle(pedalling(85.0, 8.0));
        let descent = settle(pedalling(85.0, -8.0));
        assert!((0.3..0.4).contains(&flat), "{flat}");
        assert!(climb > flat + 0.3 && climb <= 1.0, "{climb}");
        assert!(descent > 0.1 && descent < flat, "{descent}");
        assert!(settle(coasting(8.0)) < 0.01, "no effort without pedalling");

        // It moves smoothly.
        let mut animator = RideAnimator::new();
        let mut last = 0.0_f32;
        for _ in 0..240 {
            let effort = animator.step(&with_power(1.2), DT).effort;
            assert!(effort >= last && effort - last < 0.03);
            last = effort;
        }
    }

    #[test]
    fn coasting_levels_the_cranks_without_changing_the_reported_angle() {
        for start_s in [0.10, 0.27, 0.40, 0.61] {
            let mut animator = RideAnimator::new();
            ride(&mut animator, pedalling(85.0, 0.0), 8.0, start_s);
            let reported = animator.crank_angle_rad();
            let shown_before = animator.pose_input().crank_angle_rad;
            let mut last = shown_before;
            let mut sample = coasting(0.0);
            sample.distance_m = animator.wheel_angle_rad() * f64::from(WHEEL_RADIUS);
            for _ in 0..150 {
                sample.distance_m += 8.0 * DT;
                let shown = animator.step(&sample, DT).crank_angle_rad;
                let moved = (shown - last + PI).rem_euclid(2.0 * PI) - PI;
                assert!(moved.abs() < 0.12, "the cranks jumped by {moved}");
                last = shown;
            }
            assert_eq!(animator.crank_angle_rad(), reported);
            let off_level = (last + FRAC_PI_2).rem_euclid(PI) - FRAC_PI_2;
            assert!(
                off_level.abs() < 0.01,
                "cranks rest {off_level} rad off level"
            );
            let turned = (last - shown_before + PI).rem_euclid(2.0 * PI) - PI;
            assert!(turned.abs() <= FRAC_PI_2 + 0.01, "the short way round");

            // Pedalling again, the shown cranks rejoin the reported ones.
            ride(&mut animator, pedalling(85.0, 0.0), 8.0, 2.0);
            let shown = f64::from(animator.pose_input().crank_angle_rad);
            let reported = animator.crank_angle_rad().rem_euclid(TAU);
            assert!((shown - reported).abs() < 1.0e-3);
        }
    }

    #[test]
    fn a_cut_jumps_to_the_new_state() {
        let mut animator = RideAnimator::new();
        ride(&mut animator, pedalling(85.0, 0.0), 8.0, 2.0);
        let crank = animator.crank_angle_rad();
        let seek = RideSample {
            distance_m: 2_500.0,
            cadence_rpm: Some(55.0),
            lean_rad: -0.2,
            grade_pct: 11.0,
            speed_mps: None,
            power_ratio: Some(1.2),
        };
        let input = animator.step(&seek, 0.0);
        assert_eq!(animator.crank_angle_rad(), crank, "a cut turns nothing");
        assert_eq!(input.stand, 1.0);
        assert_eq!(input.effort, 1.0);
        assert!(input.steer_rad < -0.02);
        // The jump in distance is not speed.
        assert!((animator.speed_mps() - 8.0).abs() < 0.1);

        // A cut to a coasting descent tucks at once when the speed is given.
        let descent = RideSample {
            distance_m: 3_800.0,
            cadence_rpm: Some(0.0),
            grade_pct: -8.0,
            speed_mps: Some(19.0),
            ..RideSample::default()
        };
        let input = animator.step(&descent, 0.0);
        assert_eq!((input.stand, input.tuck, input.effort), (0.0, 1.0, 0.0));
        let off_level = (input.crank_angle_rad + FRAC_PI_2).rem_euclid(PI) - FRAC_PI_2;
        assert!(off_level.abs() < 1.0e-4);
    }

    #[test]
    fn nonsense_input_is_survived() {
        let mut animator = RideAnimator::new();
        let bad = RideSample {
            distance_m: f64::NAN,
            cadence_rpm: Some(f32::NAN),
            lean_rad: f32::INFINITY,
            grade_pct: f32::NAN,
            speed_mps: Some(f32::NAN),
            power_ratio: Some(f32::NAN),
        };
        for dt in [DT, 0.0, -1.0, f64::NAN, 5.0] {
            let input = animator.step(&bad, dt);
            for value in [
                input.crank_angle_rad,
                input.wheel_angle_rad,
                input.steer_rad,
                input.effort,
                input.stand,
                input.tuck,
            ] {
                assert!(value.is_finite());
            }
        }
        assert_eq!(animator.crank_angle_rad(), 0.0);
    }

    #[test]
    fn the_same_ride_gives_the_same_frames() {
        let run = || {
            let mut animator = RideAnimator::new();
            let mut out = Vec::new();
            let mut distance = 0.0;
            for frame in 0..600 {
                let t = frame as f32 / 60.0;
                distance += f64::from(6.0 + 4.0 * (t * 0.7).sin()) * DT;
                out.push(animator.step(
                    &RideSample {
                        distance_m: distance,
                        cadence_rpm: Some(60.0 + 30.0 * (t * 0.9).sin()),
                        lean_rad: 0.3 * (t * 1.3).sin(),
                        grade_pct: 9.0 * (t * 0.4).sin(),
                        ..RideSample::default()
                    },
                    DT,
                ));
            }
            out
        };
        assert_eq!(run(), run());
    }
}
