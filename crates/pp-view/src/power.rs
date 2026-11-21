//! Keyboard power target. One tap is one step. Holding ramps at about 100 W/s.

use pp_session::SIM_POWER_STEP_W;

/// Lowest keyboard power target, in watts.
pub const POWER_MIN_W: f32 = 0.0;

/// Highest keyboard power target, in watts.
pub const POWER_MAX_W: f32 = 1_500.0;

/// Watts added by one tap or one repeat. Matches [`SIM_POWER_STEP_W`].
pub const POWER_STEP_W: f32 = SIM_POWER_STEP_W as f32;

const HOLD_DELAY_S: f64 = 0.35;
/// 25 W every quarter second is 100 W per second.
const REPEAT_EVERY_S: f64 = 0.25;

/// Keyboard stand-in for the trainer power target.
#[derive(Debug, Clone)]
pub struct KeyPower {
    target_w: f32,
    hold_up_s: f64,
    hold_down_s: f64,
    repeat_up_s: f64,
    repeat_down_s: f64,
    prev_up: bool,
    prev_down: bool,
}

impl Default for KeyPower {
    fn default() -> Self {
        Self {
            target_w: 150.0,
            hold_up_s: 0.0,
            hold_down_s: 0.0,
            repeat_up_s: 0.0,
            repeat_down_s: 0.0,
            prev_up: false,
            prev_down: false,
        }
    }
}

impl KeyPower {
    /// Current keyboard power target, in watts.
    pub fn target_w(&self) -> f32 {
        self.target_w
    }

    /// Clamp and store a power target.
    pub fn set_target(&mut self, watts: f32) {
        self.target_w = watts.clamp(POWER_MIN_W, POWER_MAX_W);
    }

    /// Hold-to-repeat. `up` and `down` are level (held), not edges.
    pub fn poll(&mut self, dt: f64, up: bool, down: bool) {
        let dt = dt.max(0.0);
        if up && !down {
            self.arm(dt, true);
        } else {
            self.hold_up_s = 0.0;
            self.repeat_up_s = 0.0;
        }
        if down && !up {
            self.arm(dt, false);
        } else {
            self.hold_down_s = 0.0;
            self.repeat_down_s = 0.0;
        }
        self.prev_up = up && !down;
        self.prev_down = down && !up;
    }

    /// Cadence the keyboard rider reports. Coasting is a real `0`.
    pub fn cadence_rpm(power_w: f32) -> f32 {
        if power_w < 1.0 { 0.0 } else { 85.0 }
    }

    fn arm(&mut self, dt: f64, up: bool) {
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
        let mut steps = 0;
        while steps < 40 {
            let ready = if up {
                self.repeat_up_s >= REPEAT_EVERY_S
            } else {
                self.repeat_down_s >= REPEAT_EVERY_S
            };
            if !ready {
                break;
            }
            if up {
                self.repeat_up_s -= REPEAT_EVERY_S;
            } else {
                self.repeat_down_s -= REPEAT_EVERY_S;
            }
            self.bump(up);
            steps += 1;
        }
    }

    fn bump(&mut self, up: bool) {
        let delta = if up { POWER_STEP_W } else { -POWER_STEP_W };
        self.set_target(self.target_w + delta);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_tap_is_one_step_and_a_hold_ramps_at_about_100_watts_per_second() {
        let mut keys = KeyPower::default();
        keys.poll(0.016, true, false);
        keys.poll(0.016, false, false);
        assert_eq!(keys.target_w(), 175.0);

        let mut held = KeyPower::default();
        held.poll(0.016, true, false);
        let mut time = 0.016;
        while time < 0.35 {
            held.poll(0.016, true, false);
            time += 0.016;
        }
        assert_eq!(held.target_w(), 175.0, "the delay must not add steps");
        let before = held.target_w();
        for _ in 0..4 {
            held.poll(0.25, true, false);
        }
        let gained = held.target_w() - before;
        assert!((gained - 100.0).abs() < 1.0, "gained {gained}");
    }

    #[test]
    fn power_clamps_and_coasting_reports_zero_cadence() {
        let mut keys = KeyPower::default();
        keys.set_target(10_000.0);
        assert_eq!(keys.target_w(), POWER_MAX_W);
        keys.set_target(-5.0);
        assert_eq!(keys.target_w(), 0.0);
        assert_eq!(KeyPower::cadence_rpm(0.0), 0.0);
        assert_eq!(KeyPower::cadence_rpm(150.0), 85.0);
        keys.poll(0.016, false, true);
        assert_eq!(keys.target_w(), 0.0);
    }
}
