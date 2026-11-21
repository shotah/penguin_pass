//! Time integration of a ride along a [`Course`](crate::Course).

use pp_core::{RiderProfile, Telemetry, TrainerSample};

use crate::course::Course;
use crate::physics::acceleration_mps2;

/// Fastest speed [`Ride`] will report, in meters per second.
///
/// 80 km/h. An indoor trainer has no brakes, so a long descent is held here
/// instead of running away. [`acceleration_mps2`](crate::acceleration_mps2)
/// itself does not apply this cap.
pub const MAX_SPEED_MPS: f32 = 80.0 / 3.6;

/// Longest single integration step [`Ride::step`] takes, in seconds.
///
/// Larger `dt_s` values are split into equal substeps of at most this long.
/// The split is exact: the substeps add up to the sanitized `dt_s`.
pub const MAX_STEP_S: f64 = 0.05;

/// Longest `dt_s` [`Ride::step`] will integrate, in seconds.
///
/// A hitch longer than this is clamped. The ride stays finite if a caller
/// passes a huge or non-finite timestep. Zero, negative, and non-finite
/// timesteps leave the ride unchanged.
pub const MAX_DT_S: f64 = 60.0;

/// A rider moving along a [`Course`].
///
/// Integration is semi-implicit Euler: acceleration is evaluated at the
/// current speed and grade, speed is updated, then distance advances at the
/// new speed. Speed is clamped to `0..=MAX_SPEED_MPS`, so the bike stops on
/// a climb instead of rolling backwards and cannot exceed the descent cap.
/// Substeps of at most [`MAX_STEP_S`] keep a 0.25 s frame, or a longer hitch
/// up to [`MAX_DT_S`], stable. The same inputs always produce the same
/// telemetry.
///
/// The elapsed clock stays at zero until the first sample with positive
/// power, then it advances on every later step, including steps spent
/// coasting or stopped. Physics still integrates before that first pedal
/// stroke, so a ride that begins on a descent can roll; Penguin Pass starts
/// on flat ground, so it does not.
#[derive(Debug)]
pub struct Ride {
    course: Course,
    rider: RiderProfile,
    speed_mps: f32,
    distance_m: f64,
    lap_distance_m: f64,
    lap: u32,
    elapsed_s: f64,
    started: bool,
    last: Telemetry,
}

impl Ride {
    /// Start of a ride, stopped on the start line, lap 0.
    ///
    /// The initial snapshot carries the course grade and elevation at 0 m
    /// and zero power, speed, distance, and elapsed time.
    #[must_use]
    pub fn new(course: Course, rider: RiderProfile) -> Self {
        let mut ride = Self {
            course,
            rider,
            speed_mps: 0.0,
            distance_m: 0.0,
            lap_distance_m: 0.0,
            lap: 0,
            elapsed_s: 0.0,
            started: false,
            last: Telemetry::default(),
        };
        ride.last = ride.snapshot(0.0, None, None);
        ride
    }

    /// Advances the ride by `dt_s` seconds using `sample.power_w`.
    ///
    /// Missing power is 0 W. Cadence and heart rate are copied through.
    /// Trainer-reported speed is ignored; game speed comes from the bike
    /// model. A timestep of 0, a negative timestep, or a non-finite timestep
    /// returns the previous snapshot and changes nothing.
    pub fn step(&mut self, dt_s: f64, sample: &TrainerSample) -> Telemetry {
        let dt = sanitize_dt(dt_s);
        if dt == 0.0 {
            return self.last;
        }
        let power_w = f32::from(sample.power_w.unwrap_or(0));
        if power_w > 0.0 {
            self.started = true;
        }
        if self.started {
            self.elapsed_s += dt;
        }
        let n = substep_count(dt);
        let h = dt / n as f64;
        for _ in 0..n {
            self.integrate(h, power_w);
        }
        self.last = self.snapshot(power_w, sample.cadence_rpm, sample.heart_rate_bpm);
        self.last
    }

    /// Telemetry from the most recent [`Ride::step`], or the initial snapshot.
    #[must_use]
    pub fn telemetry(&self) -> Telemetry {
        self.last
    }

    /// The course this ride is on.
    #[must_use]
    pub fn course(&self) -> &Course {
        &self.course
    }

    /// The rider this ride is integrating.
    #[must_use]
    pub fn rider(&self) -> &RiderProfile {
        &self.rider
    }

    /// Replace the rider used by later steps.
    ///
    /// Speed, distance, lap, elapsed time, and the last telemetry snapshot
    /// stay as they are. The profile is stored as given, the same way
    /// [`Ride::new`] stores its rider. A non-positive or non-finite total
    /// mass cannot poison the ride: the next step treats that mass as zero
    /// acceleration, the same rule [`acceleration_mps2`](crate::acceleration_mps2)
    /// already applies, so speed stays finite.
    pub fn set_rider(&mut self, rider: RiderProfile) {
        self.rider = rider;
    }

    fn integrate(&mut self, h: f64, power_w: f32) {
        let grade = self.course.grade_pct_at(self.lap_distance_m);
        let a = acceleration_mps2(self.speed_mps, power_w, grade, &self.rider);
        let mut speed = self.speed_mps + a * h as f32;
        if !speed.is_finite() {
            speed = self.speed_mps;
        }
        speed = speed.clamp(0.0, MAX_SPEED_MPS);
        let ds = f64::from(speed) * h;
        self.speed_mps = speed;
        self.advance(ds);
    }

    fn advance(&mut self, ds: f64) {
        if !ds.is_finite() || ds <= 0.0 {
            return;
        }
        let starting = self.lap == 0;
        self.distance_m += ds;
        self.lap_distance_m += ds;
        let len = self.course.length_m();
        let mut crossings = 0u32;
        if len > 0.0 {
            let spans = (self.lap_distance_m / len).floor();
            if spans >= 1.0 {
                self.lap_distance_m -= spans * len;
                crossings = if spans < f64::from(u32::MAX) {
                    spans as u32
                } else {
                    u32::MAX
                };
            }
            if !(0.0..len).contains(&self.lap_distance_m) {
                self.lap_distance_m = self.lap_distance_m.rem_euclid(len);
            }
        }
        self.lap = if starting {
            1 + crossings
        } else {
            self.lap.saturating_add(crossings)
        };
    }

    fn snapshot(
        &self,
        power_w: f32,
        cadence_rpm: Option<f32>,
        heart_rate_bpm: Option<u8>,
    ) -> Telemetry {
        let lap_distance_m = self.lap_distance_m;
        Telemetry {
            power_w,
            cadence_rpm,
            heart_rate_bpm,
            speed_mps: self.speed_mps,
            distance_m: self.distance_m,
            lap_distance_m,
            lap: self.lap,
            grade_pct: self.course.grade_pct_at(lap_distance_m),
            elevation_m: self.course.elevation_at(lap_distance_m),
            elapsed_s: self.elapsed_s,
        }
    }
}

fn sanitize_dt(dt_s: f64) -> f64 {
    if dt_s.is_finite() && dt_s > 0.0 {
        dt_s.min(MAX_DT_S)
    } else {
        0.0
    }
}

fn substep_count(dt: f64) -> u64 {
    let n = (dt / MAX_STEP_S).ceil();
    if n.is_finite() && n > 1.0 {
        n as u64
    } else {
        1
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;
    use crate::physics::{LOW_SPEED_FLOOR_MPS, acceleration_mps2};
    use pp_core::RiderProfile;

    fn flat_circle(radius: f64) -> Course {
        let angle = std::f64::consts::TAU;
        let length = radius * angle;
        let json = serde_json::json!({
            "name": "Flat circle",
            "start": {"x": 0.0, "z": 0.0, "heading_rad": 0.0},
            "segments": [{"type": "arc", "radius_m": radius, "angle_rad": angle}],
            "grade": [{"type": "hold", "length_m": length, "grade_pct": 0.0}]
        });
        Course::from_str(&json.to_string()).expect("flat circle closes")
    }

    fn sample(power_w: u16) -> TrainerSample {
        TrainerSample {
            power_w: Some(power_w),
            cadence_rpm: Some(90.0),
            heart_rate_bpm: Some(150),
            speed_kph: Some(99.0),
        }
    }

    #[test]
    fn zero_and_non_finite_dt_change_nothing() {
        let mut ride = Ride::new(flat_circle(40.0), RiderProfile::default());
        let before = ride.telemetry();
        assert_eq!(before.lap, 0);
        assert_eq!(ride.step(0.0, &sample(200)), before);
        assert_eq!(ride.step(-1.0, &sample(200)), before);
        assert_eq!(ride.step(f64::NAN, &sample(200)), before);
        assert_eq!(ride.telemetry(), before);
        assert_eq!(ride.telemetry().elapsed_s, 0.0);
    }

    #[test]
    fn clock_starts_on_the_first_positive_power_and_keeps_running() {
        let mut ride = Ride::new(flat_circle(40.0), RiderProfile::default());
        let coast = TrainerSample::default();
        let held = ride.step(2.0, &coast);
        assert_eq!(held.elapsed_s, 0.0);
        assert_eq!(held.power_w, 0.0);
        assert_eq!(held.distance_m, 0.0);
        assert_eq!(held.lap, 0);

        let moving = ride.step(
            1.5,
            &TrainerSample {
                power_w: Some(100),
                cadence_rpm: Some(85.0),
                heart_rate_bpm: Some(140),
                ..TrainerSample::default()
            },
        );
        assert!((moving.elapsed_s - 1.5).abs() < 1e-9);
        assert_eq!(moving.power_w, 100.0);
        assert_eq!(moving.cadence_rpm, Some(85.0));
        assert_eq!(moving.heart_rate_bpm, Some(140));
        assert!(moving.distance_m > 0.0);
        assert_eq!(moving.lap, 1);
        assert!(moving.speed_mps > 0.0);
        // Trainer speed is not the game speed.
        assert!(moving.speed_mps < 20.0);

        let still = ride.step(1.0, &TrainerSample::default());
        assert!((still.elapsed_s - 2.5).abs() < 1e-9);
        assert_eq!(still.power_w, 0.0);
        assert!(still.cadence_rpm.is_none());
    }

    #[test]
    fn flat_ride_reaches_the_same_speed_as_the_force_balance() {
        let rider = RiderProfile::default();
        let mut ride = Ride::new(flat_circle(80.0), rider);
        let sample = sample(200);
        for _ in 0..(180.0 / 0.05) as i32 {
            ride.step(0.05, &sample);
        }
        let speed = ride.telemetry().speed_mps;
        let mut lo = LOW_SPEED_FLOOR_MPS;
        let mut hi = 40.0;
        for _ in 0..60 {
            let mid = 0.5 * (lo + hi);
            if acceleration_mps2(mid, 200.0, 0.0, &rider) > 0.0 {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        let expected = 0.5 * (lo + hi);
        assert!(
            (speed - expected).abs() < 0.1,
            "ride {speed} m/s vs balance {expected} m/s"
        );
        assert!(ride.telemetry().grade_pct.abs() < 1e-3);
        assert!(ride.telemetry().elevation_m.abs() < 1e-2);
        assert!((ride.rider().total_mass_kg() - 84.0).abs() < 1e-6);
    }

    #[test]
    fn descent_cap_holds_a_huge_power() {
        let mut ride = Ride::new(flat_circle(100.0), RiderProfile::default());
        let sample = sample(5_000);
        for _ in 0..(120.0 / 0.05) as i32 {
            let frame = ride.step(0.05, &sample);
            assert!(frame.speed_mps <= MAX_SPEED_MPS + 1e-4);
            assert!(frame.speed_mps >= 0.0);
        }
        assert!(
            (ride.telemetry().speed_mps - MAX_SPEED_MPS).abs() < 0.05,
            "expected the cap, got {} m/s",
            ride.telemetry().speed_mps
        );
    }

    #[test]
    fn coasting_on_the_flat_stops_and_never_goes_negative() {
        let mut ride = Ride::new(flat_circle(60.0), RiderProfile::default());
        let push = sample(300);
        for _ in 0..400 {
            ride.step(0.05, &push);
        }
        assert!(ride.telemetry().speed_mps > 3.0);
        let coast = TrainerSample {
            power_w: Some(0),
            ..TrainerSample::default()
        };
        let mut saw_zero = false;
        for _ in 0..20_000 {
            let frame = ride.step(0.05, &coast);
            assert!(frame.speed_mps >= 0.0);
            if frame.speed_mps == 0.0 {
                saw_zero = true;
                break;
            }
        }
        assert!(
            saw_zero,
            "coast did not stop, speed {}",
            ride.telemetry().speed_mps
        );
        for _ in 0..50 {
            assert_eq!(ride.step(0.05, &coast).speed_mps, 0.0);
        }
    }

    #[test]
    fn penguin_pass_laps_wrap_and_distance_stays_monotonic() {
        let course = Course::penguin_pass();
        let length = course.length_m();
        let mut ride = Ride::new(course, RiderProfile::default());
        let sample = sample(2_000);
        let mut previous = 0.0;
        let mut guard = 0;
        while ride.telemetry().distance_m < length * 2.0 + 150.0 {
            let frame = ride.step(0.25, &sample);
            assert!(frame.distance_m + 1e-6 >= previous);
            assert!(frame.lap_distance_m >= 0.0);
            assert!(frame.lap_distance_m < length);
            assert!(frame.speed_mps >= 0.0 && frame.speed_mps <= MAX_SPEED_MPS + 1e-4);
            assert!(frame.grade_pct.is_finite());
            let expected_grade = ride.course().grade_pct_at(frame.lap_distance_m);
            assert!((frame.grade_pct - expected_grade).abs() < 1e-4);
            if frame.distance_m > 0.0 {
                let expected_lap = (frame.distance_m / length).floor() as u32 + 1;
                assert_eq!(frame.lap, expected_lap);
            }
            previous = frame.distance_m;
            guard += 1;
            assert!(guard < 20_000, "ride did not cover two laps");
        }
        let frame = ride.telemetry();
        assert!(frame.lap >= 3);
        assert!(frame.distance_m > length * 2.0);
        assert!(frame.lap_distance_m < length);
    }

    #[test]
    fn reversed_penguin_pass_finishes_a_lap() {
        let course = Course::penguin_pass().reversed();
        assert!(course.is_reversed());
        assert_eq!(course.name(), "Penguin Pass");
        let length = course.length_m();
        let mut ride = Ride::new(course, RiderProfile::default());
        let sample = sample(250);
        let mut previous = 0.0;
        let mut guard = 0;
        while ride.telemetry().distance_m < length {
            let frame = ride.step(0.25, &sample);
            assert!(frame.speed_mps.is_finite(), "speed {}", frame.speed_mps);
            assert!(frame.power_w.is_finite() && frame.power_w > 0.0);
            assert!(frame.distance_m.is_finite() && frame.distance_m > 0.0);
            assert!(frame.distance_m + 1e-6 >= previous);
            assert!(frame.grade_pct.is_finite());
            assert!(frame.elevation_m.is_finite());
            assert!(frame.speed_mps >= 0.0 && frame.speed_mps <= MAX_SPEED_MPS + 1e-4);
            previous = frame.distance_m;
            guard += 1;
            assert!(guard < 40_000, "reversed ride did not finish a lap");
        }
        let frame = ride.telemetry();
        assert!(frame.lap >= 1);
        assert!(frame.distance_m >= length);
    }

    #[test]
    fn same_inputs_produce_identical_telemetry() {
        let seq = [
            (0.0_f64, 0_u16),
            (0.016, 180),
            (0.05, 200),
            (0.25, 220),
            (1.0, 0),
            (10.0, 250),
            (0.0, 400),
        ];
        let mut a = Ride::new(Course::penguin_pass(), RiderProfile::default());
        let mut b = Ride::new(Course::penguin_pass(), RiderProfile::default());
        for (dt, power) in seq {
            let frame_a = a.step(dt, &sample(power));
            let frame_b = b.step(dt, &sample(power));
            assert_eq!(frame_a, frame_b);
        }
    }

    #[test]
    fn a_huge_timestep_stays_finite() {
        let mut ride = Ride::new(Course::penguin_pass(), RiderProfile::default());
        let frame = ride.step(1.0e9, &sample(250));
        assert!(frame.speed_mps.is_finite());
        assert!(frame.distance_m.is_finite());
        assert!(frame.elapsed_s.is_finite());
        assert!(frame.speed_mps >= 0.0);
        assert!(frame.speed_mps <= MAX_SPEED_MPS + 1e-4);
        assert!(frame.elapsed_s <= MAX_DT_S + 1e-9);
        assert!(frame.distance_m > 0.0);
        assert!(frame.lap >= 1);
    }

    fn mass(rider_mass_kg: f32) -> RiderProfile {
        RiderProfile {
            rider_mass_kg,
            ..RiderProfile::default()
        }
    }

    #[test]
    fn set_rider_keeps_state_and_a_heavier_rider_accelerates_less() {
        let course = Course::penguin_pass();
        let sample = sample(200);
        let mut light = Ride::new(course.clone(), mass(60.0));
        let mut heavy = Ride::new(course, mass(60.0));
        light.step(1.0, &sample);
        heavy.step(1.0, &sample);
        assert_eq!(light.telemetry(), heavy.telemetry());

        let before = heavy.telemetry();
        heavy.set_rider(mass(140.0));
        assert_eq!(heavy.telemetry(), before);
        assert_eq!(heavy.rider().rider_mass_kg, 140.0);
        assert_eq!(light.rider().rider_mass_kg, 60.0);
        assert_eq!(heavy.telemetry().distance_m, before.distance_m);
        assert_eq!(heavy.telemetry().elapsed_s, before.elapsed_s);
        assert_eq!(heavy.telemetry().lap, before.lap);
        assert_eq!(heavy.telemetry().speed_mps, before.speed_mps);

        let light_frame = light.step(0.5, &sample);
        let heavy_frame = heavy.step(0.5, &sample);
        assert!(
            light_frame.speed_mps > heavy_frame.speed_mps,
            "light {} m/s, heavy {} m/s",
            light_frame.speed_mps,
            heavy_frame.speed_mps
        );
        assert!(heavy_frame.distance_m > before.distance_m);
        assert!(heavy_frame.elapsed_s > before.elapsed_s);
        assert_eq!(heavy_frame.grade_pct, light_frame.grade_pct);
    }

    #[test]
    fn set_rider_with_non_finite_or_non_positive_mass_stays_finite() {
        let sample = sample(180);
        for rider in [
            RiderProfile {
                rider_mass_kg: f32::NAN,
                bike_mass_kg: f32::INFINITY,
                cda_m2: f32::NAN,
                crr: f32::NAN,
                ..RiderProfile::default()
            },
            RiderProfile {
                rider_mass_kg: 0.0,
                bike_mass_kg: 0.0,
                ..RiderProfile::default()
            },
            RiderProfile {
                rider_mass_kg: -10.0,
                bike_mass_kg: -3.0,
                ..RiderProfile::default()
            },
        ] {
            let mut ride = Ride::new(Course::penguin_pass(), mass(75.0));
            ride.step(1.0, &sample);
            let speed = ride.telemetry().speed_mps;
            assert!(speed > 0.0);
            ride.set_rider(rider);
            let frame = ride.step(1.0, &sample);
            assert!(frame.speed_mps.is_finite());
            assert!(frame.distance_m.is_finite());
            assert!(frame.elapsed_s.is_finite());
            assert!(
                (frame.speed_mps - speed).abs() < 1e-3,
                "bad mass changed speed from {speed} to {}",
                frame.speed_mps
            );
        }
    }
}
