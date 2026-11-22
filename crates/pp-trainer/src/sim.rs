//! A rider with no Bluetooth radio.
//!
//! The game drives power and cadence from the keyboard. Tests drive the same
//! setters and read back the last grade.

use std::time::Duration;

use pp_core::{TrainerCommand, TrainerSample};

use crate::config::duration_ms;
use crate::error::TrainerError;
use crate::{ConnectionState, SAMPLE_STALENESS, Trainer, sample_is_fresh};

/// Deterministic stand-in for a smart trainer.
///
/// Samples do not go stale on the wall clock. Call [`Self::advance`] to move
/// the trainer's clock. A reading is fresh while its age is less than or
/// equal to [`SAMPLE_STALENESS`].
#[derive(Debug, Clone)]
pub struct SimTrainer {
    name: String,
    sample: TrainerSample,
    has_sample: bool,
    sample_at_ms: u64,
    now_ms: u64,
    staleness: Duration,
    last_grade_pct: Option<f32>,
    last_target_power_w: Option<u16>,
    last_virtual_gear: Option<u8>,
}

impl SimTrainer {
    /// A connected simulated rider with no sample yet.
    ///
    /// `name` is what [`Trainer::name`] returns. The state is [`ConnectionState::Connected`].
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            sample: TrainerSample::default(),
            has_sample: false,
            sample_at_ms: 0,
            now_ms: 0,
            staleness: SAMPLE_STALENESS,
            last_grade_pct: None,
            last_target_power_w: None,
            last_virtual_gear: None,
        }
    }

    /// Set instantaneous power, in watts, and mark the sample fresh.
    pub fn set_power(&mut self, watts: u16) {
        self.sample.power_w = Some(watts);
        self.touch();
    }

    /// Set cadence, in revolutions per minute, and mark the sample fresh.
    pub fn set_cadence(&mut self, rpm: f32) {
        self.sample.cadence_rpm = Some(rpm);
        self.touch();
    }

    /// Set the trainer-reported speed, in kilometers per hour, and mark the sample fresh.
    ///
    /// The ride does not use this for physics. It is the same field a real
    /// trainer fills in on [`TrainerSample`].
    pub fn set_speed_kph(&mut self, kph: f32) {
        self.sample.speed_kph = Some(kph);
        self.touch();
    }

    /// Move the simulated clock forward.
    pub fn advance(&mut self, dt: Duration) {
        self.now_ms = self.now_ms.saturating_add(duration_ms(dt));
    }

    /// Last grade passed to [`Trainer::send`], in percent. `None` until one is sent.
    #[must_use]
    pub fn last_grade_pct(&self) -> Option<f32> {
        self.last_grade_pct
    }

    /// Last target power passed to [`Trainer::send`], in watts.
    #[must_use]
    pub fn last_target_power_w(&self) -> Option<u16> {
        self.last_target_power_w
    }

    fn touch(&mut self) {
        self.has_sample = true;
        self.sample_at_ms = self.now_ms;
    }
}

impl Trainer for SimTrainer {
    fn latest_sample(&self) -> Option<TrainerSample> {
        if !self.has_sample {
            return None;
        }
        let age = Duration::from_millis(self.now_ms.saturating_sub(self.sample_at_ms));
        if sample_is_fresh(age, self.staleness) {
            Some(self.sample)
        } else {
            None
        }
    }

    fn send(&mut self, command: TrainerCommand) -> Result<(), TrainerError> {
        match command {
            TrainerCommand::SetGrade { grade_pct } => {
                if !grade_pct.is_finite() {
                    return Err(TrainerError::Message("grade is not finite".into()));
                }
                self.last_grade_pct = Some(grade_pct);
            }
            TrainerCommand::SetTargetPower { watts } => {
                self.last_target_power_w = Some(watts);
            }
            TrainerCommand::SetVirtualGear { gear } => {
                self.last_virtual_gear = Some(gear);
            }
        }
        Ok(())
    }

    fn state(&self) -> ConnectionState {
        ConnectionState::Connected
    }

    fn name(&self) -> String {
        self.name.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyboard_rider_records_grade_and_goes_stale() {
        let mut rider = SimTrainer::new("keyboard");
        assert_eq!(rider.name(), "keyboard");
        assert_eq!(rider.state(), ConnectionState::Connected);
        assert_eq!(rider.latest_sample(), None);
        assert_eq!(rider.last_grade_pct(), None);

        rider.set_power(180);
        rider.set_cadence(85.0);
        let sample = rider.latest_sample().unwrap();
        assert_eq!(sample.power_w, Some(180));
        assert_eq!(sample.cadence_rpm, Some(85.0));

        rider
            .send(TrainerCommand::SetGrade { grade_pct: 6.5 })
            .unwrap();
        rider
            .send(TrainerCommand::SetTargetPower { watts: 220 })
            .unwrap();
        assert_eq!(rider.last_grade_pct(), Some(6.5));
        assert_eq!(rider.last_target_power_w(), Some(220));
        assert!(
            rider
                .send(TrainerCommand::SetGrade {
                    grade_pct: f32::NAN,
                })
                .is_err()
        );
        assert_eq!(rider.last_grade_pct(), Some(6.5));

        rider.advance(SAMPLE_STALENESS);
        assert!(rider.latest_sample().is_some());
        rider.advance(Duration::from_millis(1));
        assert_eq!(rider.latest_sample(), None);

        rider.set_power(50);
        assert_eq!(rider.latest_sample().unwrap().power_w, Some(50));
    }

    #[test]
    fn trait_object_accepts_a_sim_rider() {
        let mut rider = SimTrainer::new("dyn");
        let trainer: &mut dyn Trainer = &mut rider;
        trainer
            .send(TrainerCommand::SetGrade { grade_pct: -2.0 })
            .unwrap();
        assert_eq!(rider.last_grade_pct(), Some(-2.0));
    }
}
