//! How a session obtains a Bluetooth trainer without naming BlueZ in the
//! frame loop.

use pp_trainer::{
    BleTrainer, DiscoveredTrainer, SessionMode, Trainer, TrainerConfig, TrainerError,
};

/// A trainer the session can scan and connect, in addition to [`Trainer`].
///
/// Methods return promptly. They do not wait on the radio.
pub trait TrainerLink: Trainer {
    /// Trainers seen since scanning started.
    fn discovered(&self) -> Vec<DiscoveredTrainer>;

    /// Queue a connection to a Bluetooth address or a name fragment.
    ///
    /// # Errors
    ///
    /// Returns an error when `id_or_name` is empty or the trainer cannot
    /// accept the request. The ride keeps running either way.
    fn connect(&self, id_or_name: &str) -> Result<(), TrainerError>;
}

/// Builds the trainer [`Session`](crate::Session) uses for a real bike.
///
/// The default implementation is [`BleTrainerFactory`]. Tests supply a fake.
pub trait TrainerFactory: Send {
    /// Start a trainer in control mode and begin scanning.
    ///
    /// # Errors
    ///
    /// The string is a short reason Bluetooth could not be started. The
    /// session records it on [`crate::SessionStatus`] and keeps the simulated
    /// rider. A missing adapter is not a panic.
    fn spawn(&mut self) -> Result<Box<dyn TrainerLink>, String>;

    /// Start a trainer in `mode`.
    ///
    /// [`crate::Session`] reaches this through [`Self::spawn_configured`]
    /// unless a factory overrides that method. The mode is
    /// [`SessionMode::Control`] when [`crate::Settings::control_trainer`] is
    /// set, and [`SessionMode::ReadOnly`] when it is not. The default ignores
    /// `mode` and calls [`Self::spawn`], so existing factories keep working.
    /// [`BleTrainerFactory`] honors `mode`.
    fn spawn_mode(&mut self, mode: SessionMode) -> Result<Box<dyn TrainerLink>, String> {
        let _ = mode;
        self.spawn()
    }

    /// Start a trainer in `mode` and tell it the rider and bicycle mass.
    ///
    /// Masses are kilograms. `None` leaves the trainer's stored weight.
    /// [`crate::Session`] passes the settings masses here. A weight edited
    /// while a trainer is already connected is sent on the next spawn, not
    /// on the open link. The default ignores the masses and calls
    /// [`Self::spawn_mode`], so a factory written against `spawn_mode` keeps
    /// compiling. [`BleTrainerFactory`] copies the masses into
    /// [`TrainerConfig`].
    fn spawn_configured(
        &mut self,
        mode: SessionMode,
        rider_mass_kg: Option<f32>,
        bike_mass_kg: Option<f32>,
    ) -> Result<Box<dyn TrainerLink>, String> {
        let _ = (rider_mass_kg, bike_mass_kg);
        self.spawn_mode(mode)
    }
}

/// Opens a [`BleTrainer`] in control mode.
///
/// [`BleTrainer::spawn_with`](pp_trainer::BleTrainer::spawn_with) starts
/// scanning on a background thread. Spawn failure is returned to the session.
/// A missing Bluetooth adapter shows up later as
/// [`ConnectionState::Failed`](pp_trainer::ConnectionState::Failed).
#[derive(Debug, Default, Clone, Copy)]
pub struct BleTrainerFactory;

fn spawn_trainer(
    mode: SessionMode,
    rider_mass_kg: Option<f32>,
    bike_mass_kg: Option<f32>,
) -> Result<Box<dyn TrainerLink>, String> {
    let config = TrainerConfig {
        mode,
        rider_mass_kg,
        bike_mass_kg,
        ..TrainerConfig::default()
    };
    match BleTrainer::spawn_with(config) {
        Ok(trainer) => Ok(Box::new(trainer)),
        Err(err) => Err(err.to_string()),
    }
}

impl TrainerFactory for BleTrainerFactory {
    fn spawn(&mut self) -> Result<Box<dyn TrainerLink>, String> {
        self.spawn_mode(SessionMode::Control)
    }

    fn spawn_mode(&mut self, mode: SessionMode) -> Result<Box<dyn TrainerLink>, String> {
        self.spawn_configured(mode, None, None)
    }

    fn spawn_configured(
        &mut self,
        mode: SessionMode,
        rider_mass_kg: Option<f32>,
        bike_mass_kg: Option<f32>,
    ) -> Result<Box<dyn TrainerLink>, String> {
        spawn_trainer(mode, rider_mass_kg, bike_mass_kg)
    }
}

impl TrainerLink for BleTrainer {
    fn discovered(&self) -> Vec<DiscoveredTrainer> {
        BleTrainer::discovered(self)
    }

    fn connect(&self, id_or_name: &str) -> Result<(), TrainerError> {
        BleTrainer::connect(self, id_or_name)
    }
}
