//! In-process trainers for tests and screenshots.
//!
//! Nothing here opens a Bluetooth adapter. `--fake-trainer` discovers two
//! named bikes after a short delay. `--no-bluetooth` reports that the radio
//! is unavailable. A real trainer is only constructed when the player launches
//! the game without either flag.

use std::sync::{Arc, Mutex};

use pp_core::{TrainerCommand, TrainerSample};
use pp_session::{
    ConnectionState, DiscoveredTrainer, SessionMode, Trainer, TrainerError, TrainerFactory,
    TrainerLink,
};

const DISCOVER_AFTER_S: f32 = 0.2;
const CONNECT_AFTER_S: f32 = 0.15;
const FAKE_POWER_W: u16 = 210;
const FAKE_CADENCE: f32 = 88.0;
const FAKE_HEART: u8 = 142;

/// Shared clock and the last grade a fake trainer was asked to hold.
pub struct FakeHub {
    inner: Mutex<Inner>,
}

struct Inner {
    elapsed: f32,
    state: ConnectionState,
    connect_at: Option<f32>,
    requested: Option<String>,
    name: String,
    last_grade: Option<f32>,
    grade_commands: u32,
}

impl FakeHub {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new(Inner {
                elapsed: 0.0,
                state: ConnectionState::Scanning,
                connect_at: None,
                requested: None,
                name: String::new(),
                last_grade: None,
                grade_commands: 0,
            }),
        })
    }

    /// Move the fake clock. Discovery and the connect handshake use this, not the wall clock.
    pub fn advance(&self, dt: f32) {
        let mut inner = lock(&self.inner);
        inner.elapsed += dt.max(0.0);
        if let Some(at) = inner.connect_at
            && inner.elapsed >= at + CONNECT_AFTER_S
            && matches!(inner.state, ConnectionState::Connecting)
        {
            inner.state = ConnectionState::Connected;
        }
    }

    /// Last grade command, in percent. `None` until one arrives.
    pub fn last_grade(&self) -> Option<f32> {
        lock(&self.inner).last_grade
    }

    /// How many grade commands have been recorded.
    pub fn grade_commands(&self) -> u32 {
        lock(&self.inner).grade_commands
    }

    fn begin_scan(&self) {
        let mut inner = lock(&self.inner);
        inner.state = ConnectionState::Scanning;
        inner.connect_at = None;
        inner.name.clear();
    }

    fn devices(elapsed: f32) -> Vec<DiscoveredTrainer> {
        if elapsed < DISCOVER_AFTER_S {
            return Vec::new();
        }
        vec![
            DiscoveredTrainer {
                id: "FAKE:NEO".to_owned(),
                name: "Fake Neo 2T".to_owned(),
                rssi_dbm: Some(-48),
                ftms: false,
                cycling_power: true,
                fec: true,
            },
            DiscoveredTrainer {
                id: "FAKE:KICKR".to_owned(),
                name: "Fake Kickr".to_owned(),
                rssi_dbm: Some(-72),
                ftms: true,
                cycling_power: true,
                fec: false,
            },
        ]
    }
}

/// Factory installed by `--fake-trainer`.
pub struct FakeFactory {
    hub: Arc<FakeHub>,
}

impl FakeFactory {
    /// A factory and the hub the game reads grades from.
    pub fn pair() -> (Self, Arc<FakeHub>) {
        let hub = FakeHub::new();
        (
            Self {
                hub: Arc::clone(&hub),
            },
            hub,
        )
    }
}

impl TrainerFactory for FakeFactory {
    fn spawn(&mut self) -> Result<Box<dyn TrainerLink>, String> {
        self.spawn_mode(SessionMode::Control)
    }

    fn spawn_mode(&mut self, _mode: SessionMode) -> Result<Box<dyn TrainerLink>, String> {
        self.hub.begin_scan();
        Ok(Box::new(FakeLink {
            hub: Arc::clone(&self.hub),
        }))
    }
}

/// Factory installed by `--no-bluetooth`. Spawn never touches a radio.
pub struct UnavailableFactory;

impl TrainerFactory for UnavailableFactory {
    fn spawn(&mut self) -> Result<Box<dyn TrainerLink>, String> {
        Err("Bluetooth is turned off for this run".to_owned())
    }
}

struct FakeLink {
    hub: Arc<FakeHub>,
}

impl Trainer for FakeLink {
    fn latest_sample(&self) -> Option<TrainerSample> {
        let inner = lock(&self.hub.inner);
        if !matches!(inner.state, ConnectionState::Connected) {
            return None;
        }
        Some(TrainerSample {
            power_w: Some(FAKE_POWER_W),
            cadence_rpm: Some(FAKE_CADENCE),
            heart_rate_bpm: Some(FAKE_HEART),
            speed_kph: None,
        })
    }

    fn send(&mut self, command: TrainerCommand) -> Result<(), TrainerError> {
        match command {
            TrainerCommand::SetGrade { grade_pct } => {
                if !grade_pct.is_finite() {
                    return Err(TrainerError::Message("grade is not finite".into()));
                }
                let mut inner = lock(&self.hub.inner);
                inner.last_grade = Some(grade_pct);
                inner.grade_commands = inner.grade_commands.saturating_add(1);
            }
            TrainerCommand::SetTargetPower { .. } => {}
        }
        Ok(())
    }

    fn state(&self) -> ConnectionState {
        lock(&self.hub.inner).state.clone()
    }

    fn name(&self) -> String {
        let inner = lock(&self.hub.inner);
        if inner.name.is_empty() {
            "Fake trainer".to_owned()
        } else {
            inner.name.clone()
        }
    }
}

impl TrainerLink for FakeLink {
    fn discovered(&self) -> Vec<DiscoveredTrainer> {
        let inner = lock(&self.hub.inner);
        FakeHub::devices(inner.elapsed)
    }

    fn connect(&self, id_or_name: &str) -> Result<(), TrainerError> {
        let id = id_or_name.trim();
        if id.is_empty() {
            return Err(TrainerError::Message("trainer id is empty".into()));
        }
        let mut inner = lock(&self.hub.inner);
        let devices = FakeHub::devices(inner.elapsed.max(DISCOVER_AFTER_S));
        let Some(device) = devices.iter().find(|device| {
            device.id.eq_ignore_ascii_case(id)
                || device
                    .name
                    .to_ascii_lowercase()
                    .contains(&id.to_ascii_lowercase())
        }) else {
            inner.state = ConnectionState::Failed(format!("no trainer named {id}"));
            return Err(TrainerError::Message(format!("no trainer named {id}")));
        };
        inner.name = device.name.clone();
        inner.requested = Some(device.id.clone());
        inner.state = ConnectionState::Connecting;
        inner.connect_at = Some(inner.elapsed);
        Ok(())
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|err| err.into_inner())
}
