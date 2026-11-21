//! Keyboard stand-in for a trainer.
//!
//! The repeat timing lives in `pp-view`. Power itself is stored on the session.

use pp_core::TrainerSample;
use pp_view::power::KeyPower;

pub use pp_view::power::{POWER_MAX_W, POWER_MIN_W};

/// Status line shown while the keyboard rider is the input source.
pub const STATUS: &str = pp_view::hud::SIM_STATUS;

/// Keyboard stand-in for one trainer.
#[derive(Debug, Clone, Default)]
pub struct SimRider {
    keys: KeyPower,
}

impl SimRider {
    /// Current keyboard power target, in watts.
    pub fn target_power_w(&self) -> f32 {
        self.keys.target_w()
    }

    /// Clamp and store a power target.
    pub fn set_target_power(&mut self, watts: f32) {
        self.keys.set_target(watts);
    }

    /// Hold-to-repeat power changes. `up` and `down` are level (held), not edges.
    pub fn poll_keys(&mut self, dt: f64, up: bool, down: bool) {
        self.keys.poll(dt, up, down);
    }

    /// Sample the keyboard rider the way a trainer would report one tick.
    pub fn sample(&self) -> TrainerSample {
        let power = self
            .target_power_w()
            .round()
            .clamp(POWER_MIN_W, POWER_MAX_W);
        TrainerSample {
            power_w: Some(power as u16),
            cadence_rpm: Some(KeyPower::cadence_rpm(power)),
            speed_kph: None,
            heart_rate_bpm: None,
        }
    }
}
