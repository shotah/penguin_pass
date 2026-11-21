//! How this process was started.
//!
//! `--fake-trainer` and `--no-bluetooth` are the only modes tests may use.
//! A normal launch, with neither flag, is the one that may open the player's
//! trainer when they pick it in the menu.

use std::net::{IpAddr, Ipv4Addr};
use std::path::PathBuf;

use godot::classes::{Os, ProjectSettings, Time};
use godot::prelude::*;
use pp_session::{HeartRateFactory, Session, SessionOptions, Settings, TrainerFactory};
use pp_trainer::{DiscoveredSensor, HeartRateLink, SimHeartRate};
use pp_view::launch::{LaunchEnv, LaunchFlags, TrainerMode as ViewMode, parse_launch_in};

use crate::fake_trainer::{FakeFactory, FakeHub, UnavailableFactory};

/// Which trainer factory this process is allowed to build.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TrainerMode {
    /// The player's Bluetooth trainer. Tests must not use this.
    Real,
    /// Two named fakes. No radio.
    Fake,
    /// Scan and connect report that Bluetooth is off.
    Off,
}

/// Flags and the settings file for this process.
#[derive(Clone)]
pub struct Launch {
    /// Trainer factory to install.
    pub mode: TrainerMode,
    /// Absolute settings path.
    pub settings_path: PathBuf,
    /// `Some(0)` asks the OS for a free dashboard port.
    pub dashboard_port: Option<u16>,
    /// Bind the dashboard on loopback instead of every interface.
    pub dashboard_loopback: bool,
    /// After the first frame, confirm Quit in the menu.
    pub quit_from_menu: bool,
    /// Skip the title screen and start already riding.
    pub skip_title: bool,
    /// Play title, start, end, and quit. The release exit check.
    pub play_through: bool,
    /// Start with the music bus muted.
    pub no_music: bool,
    /// Start with ambience and effects silent.
    pub no_sound: bool,
    /// Record a scripted ride to this wav and then quit.
    pub sound_capture: Option<std::path::PathBuf>,
    /// Hide the console, chips, and toasts.
    pub no_hud: bool,
    /// Forced prompts. `None` follows the last real button.
    pub input_mode: Option<pp_view::prompts::InputMode>,
}

impl Launch {
    /// Read `--` user arguments, then `PENGUIN_PASS_*` environment variables.
    ///
    /// The environment is how `smoke.gd` forces a fake trainer. `./x smoke`
    /// does not forward arguments after `--`.
    pub fn from_engine() -> Self {
        let collected = cmdline(Os::singleton().get_cmdline_user_args());
        let engine = cmdline(Os::singleton().get_cmdline_args());
        let flags = parse_launch_in(&collected, &read_env(), &engine);
        Self::from_flags(flags)
    }

    fn from_flags(flags: LaunchFlags) -> Self {
        let mode = match flags.mode {
            ViewMode::Real => TrainerMode::Real,
            ViewMode::Fake => TrainerMode::Fake,
            ViewMode::Off => TrainerMode::Off,
        };
        Self {
            mode,
            settings_path: flags.settings_path.unwrap_or_else(default_settings_path),
            dashboard_port: flags.dashboard_port,
            dashboard_loopback: flags.dashboard_loopback,
            quit_from_menu: flags.quit_from_menu,
            skip_title: flags.skip_title,
            play_through: flags.play_through,
            no_music: flags.no_music,
            no_sound: flags.no_sound,
            sound_capture: flags.sound_capture,
            no_hud: flags.no_hud,
            input_mode: flags.input_mode,
        }
    }
}

fn read_env() -> LaunchEnv {
    LaunchEnv {
        fake_trainer: env_string("PENGUIN_PASS_FAKE_TRAINER"),
        no_bluetooth: env_string("PENGUIN_PASS_NO_BLUETOOTH"),
        settings: env_string("PENGUIN_PASS_SETTINGS"),
        dashboard_port: env_string("PENGUIN_PASS_DASHBOARD_PORT"),
        dashboard_loopback: env_string("PENGUIN_PASS_DASHBOARD_LOOPBACK"),
        no_music: env_string("PENGUIN_PASS_NO_MUSIC"),
        no_sound: env_string("PENGUIN_PASS_NO_SOUND"),
        no_hud: env_string("PENGUIN_PASS_NO_HUD"),
    }
}

/// Build the session for this launch. The fake hub is `Some` only in fake mode.
pub fn open_session(
    launch: &Launch,
    settings: Settings,
) -> (Session, Option<std::sync::Arc<FakeHub>>) {
    let host = if launch.dashboard_loopback {
        IpAddr::V4(Ipv4Addr::LOCALHOST)
    } else {
        IpAddr::V4(Ipv4Addr::UNSPECIFIED)
    };
    match launch.mode {
        TrainerMode::Real => {
            let mut session = Session::with_options(
                settings,
                SessionOptions {
                    trainer_factory: Box::new(pp_session::BleTrainerFactory),
                    dashboard_host: host,
                    dashboard_port: launch.dashboard_port,
                },
            );
            session.set_heart_rate_factory(Some(Box::new(pp_session::BleHeartRateFactory)));
            session.set_settings_path(&launch.settings_path);
            stamp_local_time(&mut session);
            (session, None)
        }
        TrainerMode::Fake => {
            let (factory, hub) = FakeFactory::pair();
            let mut session =
                session_with(settings, Box::new(factory), host, launch.dashboard_port);
            session.set_heart_rate_factory(Some(Box::new(FakeStrap)));
            session.set_settings_path(&launch.settings_path);
            stamp_local_time(&mut session);
            (session, Some(hub))
        }
        TrainerMode::Off => {
            let mut session = session_with(
                settings,
                Box::new(UnavailableFactory),
                host,
                launch.dashboard_port,
            );
            session.set_settings_path(&launch.settings_path);
            stamp_local_time(&mut session);
            (session, None)
        }
    }
}

/// Godot's `bias` is minutes east of UTC. This machine (PST) reports `-480`.
fn stamp_local_time(session: &mut Session) {
    let zone = Time::singleton().get_time_zone_from_system();
    let minutes = zone
        .get("bias")
        .and_then(|value| value.try_to::<i32>().ok())
        .unwrap_or(0);
    session.set_utc_offset_minutes(minutes);
}

struct FakeStrap;

impl HeartRateFactory for FakeStrap {
    fn spawn(&mut self) -> Result<Box<dyn HeartRateLink>, String> {
        let mut strap = SimHeartRate::new();
        strap.set_discovered(vec![DiscoveredSensor {
            id: "FAKE:HR".to_owned(),
            name: "Fake strap".to_owned(),
            rssi_dbm: Some(-60),
        }]);
        Ok(Box::new(strap))
    }
}

fn session_with(
    settings: Settings,
    factory: Box<dyn TrainerFactory>,
    host: IpAddr,
    port: Option<u16>,
) -> Session {
    Session::with_options(
        settings,
        SessionOptions {
            trainer_factory: factory,
            dashboard_host: host,
            dashboard_port: port,
        },
    )
}

fn default_settings_path() -> PathBuf {
    let global = ProjectSettings::singleton().globalize_path("user://settings.toml");
    PathBuf::from(global.to_string())
}

fn cmdline(args: godot::builtin::PackedStringArray) -> Vec<String> {
    let mut collected = Vec::new();
    for index in 0..args.len() {
        if let Some(arg) = args.get(index) {
            collected.push(arg.to_string());
        }
    }
    collected
}

fn env_string(name: &str) -> Option<String> {
    let from_godot = Os::singleton().get_environment(name).to_string();
    if !from_godot.is_empty() {
        return Some(from_godot);
    }
    std::env::var(name).ok().filter(|value| !value.is_empty())
}
