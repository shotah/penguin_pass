//! Keyboard rider used until a real trainer and `pp-course` are wired in.
//!
//! Phase 1 keeps this path small: power comes from the keyboard, speed is a
//! flat-road balance of rolling and aero drag, and the elevation profile is a
//! stand-in cosine so the HUD has something to draw. Phase 2 replaces
//! [`SimRider::step`] with `pp_course::Ride::step`.

use pp_core::{Telemetry, TrainerSample};

/// Placeholder lap length, in meters. Matches the real course once it lands.
pub const LAP_M: f64 = 5_000.0;

/// Lowest keyboard power target, in watts.
pub const POWER_MIN_W: f32 = 0.0;

/// Highest keyboard power target, in watts.
pub const POWER_MAX_W: f32 = 1_500.0;

/// Keyboard step, in watts.
pub const POWER_STEP_W: f32 = 10.0;

/// Status line shown while the keyboard rider is the input source.
pub const STATUS: &str = "Simulated rider — Up/Down to change power";

const MASS_KG: f32 = 84.0;
const CRR: f32 = 0.004;
const CDA_M2: f32 = 0.32;
const AIR_DENSITY: f32 = 1.15;
const GRAVITY: f32 = 9.81;
/// How quickly speed settles toward the flat-road equilibrium.
const SPEED_TAU_S: f64 = 3.0;
const HOLD_DELAY_S: f64 = 0.35;
const REPEAT_START_S: f64 = 0.16;
const REPEAT_FAST_S: f64 = 0.04;

/// Keyboard stand-in for one trainer.
///
/// Plug-in: `pp-trainer`'s `Trainer` (`latest_sample`, `send`, `state`, `name`)
/// replaces [`SimRider::sample`] inside `Game::read_trainer_sample`.
#[derive(Debug, Clone)]
pub struct SimRider {
    target_power_w: f32,
    speed_mps: f32,
    distance_m: f64,
    elapsed_s: f64,
    hold_up_s: f64,
    hold_down_s: f64,
    repeat_up_s: f64,
    repeat_down_s: f64,
    prev_up: bool,
    prev_down: bool,
}

impl Default for SimRider {
    fn default() -> Self {
        Self {
            target_power_w: 150.0,
            speed_mps: 0.0,
            distance_m: 0.0,
            elapsed_s: 0.0,
            hold_up_s: 0.0,
            hold_down_s: 0.0,
            repeat_up_s: 0.0,
            repeat_down_s: 0.0,
            prev_up: false,
            prev_down: false,
        }
    }
}

impl SimRider {
    /// Current keyboard power target, in watts.
    pub fn target_power_w(&self) -> f32 {
        self.target_power_w
    }

    /// Speed from the flat-road integration, in meters per second.
    pub fn speed_mps(&self) -> f32 {
        self.speed_mps
    }

    /// Session distance, in meters.
    pub fn distance_m(&self) -> f64 {
        self.distance_m
    }

    /// Clamp and store a power target.
    pub fn set_target_power(&mut self, watts: f32) {
        self.target_power_w = watts.clamp(POWER_MIN_W, POWER_MAX_W);
    }

    /// Jump the ride to a distance without rendering. Speed snaps to equilibrium.
    pub fn seek(&mut self, distance_m: f64, elapsed_s: f64, power_w: f32) {
        self.distance_m = distance_m.max(0.0);
        self.elapsed_s = elapsed_s.max(0.0);
        self.set_target_power(power_w);
        self.speed_mps = equilibrium_speed(self.target_power_w);
    }

    /// Hold-to-repeat power changes. `up` and `down` are level (held), not edges.
    pub fn poll_keys(&mut self, dt: f64, up: bool, down: bool) {
        let dt = dt.max(0.0);
        if up && !down {
            self.arm_repeat(dt, true);
        } else {
            self.hold_up_s = 0.0;
            self.repeat_up_s = 0.0;
        }
        if down && !up {
            self.arm_repeat(dt, false);
        } else {
            self.hold_down_s = 0.0;
            self.repeat_down_s = 0.0;
        }
        self.prev_up = up && !down;
        self.prev_down = down && !up;
    }

    /// Sample the keyboard rider the way a trainer would report one tick.
    pub fn sample(&self) -> TrainerSample {
        let power = self.target_power_w.round().clamp(POWER_MIN_W, POWER_MAX_W);
        TrainerSample {
            power_w: Some(power as u16),
            cadence_rpm: Some(cadence_rpm(power)),
            speed_kph: None,
            heart_rate_bpm: None,
        }
    }

    /// Advance the flat-road ride. `sample` supplies power and the optional sensors.
    pub fn step(&mut self, dt: f64, sample: &TrainerSample) -> Telemetry {
        let dt = dt.max(0.0);
        let power = sample.power_w.map(f32::from).unwrap_or(0.0);
        let target = equilibrium_speed(power);
        let blend = 1.0 - (-dt / SPEED_TAU_S).exp();
        self.speed_mps += (target - self.speed_mps) * blend as f32;
        if self.speed_mps < 0.0 || !self.speed_mps.is_finite() {
            self.speed_mps = 0.0;
        }
        self.distance_m += f64::from(self.speed_mps) * dt;
        self.elapsed_s += dt;
        self.telemetry(sample)
    }

    /// Snapshot at the current distance, using `sample` for the sensor fields.
    pub fn telemetry(&self, sample: &TrainerSample) -> Telemetry {
        let lap_distance_m = self.distance_m.rem_euclid(LAP_M);
        let lap = (self.distance_m / LAP_M).floor() as u32 + 1;
        Telemetry {
            power_w: sample.power_w.map(f32::from).unwrap_or(0.0),
            cadence_rpm: sample.cadence_rpm,
            heart_rate_bpm: sample.heart_rate_bpm,
            speed_mps: self.speed_mps,
            distance_m: self.distance_m,
            lap_distance_m,
            lap,
            grade_pct: placeholder_grade(lap_distance_m),
            elevation_m: placeholder_elevation(lap_distance_m),
            elapsed_s: self.elapsed_s,
        }
    }

    fn arm_repeat(&mut self, dt: f64, up: bool) {
        let fresh = if up { !self.prev_up } else { !self.prev_down };
        if fresh {
            self.bump(up);
            if up {
                self.hold_up_s = 0.0;
                self.repeat_up_s = 0.0;
            } else {
                self.hold_down_s = 0.0;
                self.repeat_down_s = 0.0;
            }
        }
        if up {
            self.hold_up_s += dt;
        } else {
            self.hold_down_s += dt;
        }
        let held = if up { self.hold_up_s } else { self.hold_down_s };
        if held <= HOLD_DELAY_S {
            return;
        }
        if up {
            self.repeat_up_s += dt;
        } else {
            self.repeat_down_s += dt;
        }
        let interval = repeat_interval(held);
        let mut steps = 0;
        while steps < 40 {
            let ready = if up {
                self.repeat_up_s >= interval
            } else {
                self.repeat_down_s >= interval
            };
            if !ready {
                break;
            }
            if up {
                self.repeat_up_s -= interval;
            } else {
                self.repeat_down_s -= interval;
            }
            self.bump(up);
            steps += 1;
        }
    }

    fn bump(&mut self, up: bool) {
        let delta = if up { POWER_STEP_W } else { -POWER_STEP_W };
        self.set_target_power(self.target_power_w + delta);
    }
}

/// Cadence the keyboard rider reports. Coasting is a real `0`, not a missing sensor.
pub fn cadence_rpm(power_w: f32) -> f32 {
    if power_w < 1.0 { 0.0 } else { 85.0 }
}

/// Flat-road steady speed for `power_w`, in meters per second.
pub fn equilibrium_speed(power_w: f32) -> f32 {
    if power_w <= 0.5 {
        return 0.0;
    }
    let roll = CRR * MASS_KG * GRAVITY;
    let drag = 0.5 * AIR_DENSITY * CDA_M2;
    let mut speed = (power_w / drag).cbrt().clamp(0.5, 40.0);
    for _ in 0..6 {
        let force = roll * speed + drag * speed * speed * speed - power_w;
        let slope = roll + 3.0 * drag * speed * speed;
        speed -= force / slope;
        if speed < 0.2 {
            speed = 0.2;
        }
    }
    if speed.is_finite() { speed } else { 0.0 }
}

/// Stand-in elevation, in meters. A 80 m wave over the 5 km lap.
pub fn placeholder_elevation(distance_m: f64) -> f32 {
    let phase = lap_phase(distance_m);
    (1_250.0 + 40.0 * (1.0 - phase.cos())) as f32
}

/// Grade of [`placeholder_elevation`], in percent.
pub fn placeholder_grade(distance_m: f64) -> f32 {
    let phase = lap_phase(distance_m);
    let slope = 40.0 * phase.sin() * (std::f64::consts::TAU / LAP_M);
    (slope * 100.0) as f32
}

/// Profile samples for the elevation strip. `step_m` is the spacing.
pub fn placeholder_profile(step_m: f64) -> Vec<(f64, f32)> {
    let step = step_m.max(1.0);
    let mut points = Vec::new();
    let mut distance = 0.0;
    while distance <= LAP_M {
        points.push((distance, placeholder_elevation(distance)));
        distance += step;
    }
    if points.last().is_none_or(|(d, _)| (*d - LAP_M).abs() > 0.1) {
        points.push((LAP_M, placeholder_elevation(0.0)));
    }
    points
}

fn lap_phase(distance_m: f64) -> f64 {
    distance_m.rem_euclid(LAP_M) / LAP_M * std::f64::consts::TAU
}

fn repeat_interval(held_s: f64) -> f64 {
    let t = ((held_s - HOLD_DELAY_S) / 1.4).clamp(0.0, 1.0);
    REPEAT_START_S - t * (REPEAT_START_S - REPEAT_FAST_S)
}
