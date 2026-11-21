//! Command-line and environment flags. The engine path is resolved by the caller.

use std::path::PathBuf;

/// Which trainer factory this process is allowed to build.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrainerMode {
    /// The player's Bluetooth trainer. Tests must not use this.
    Real,
    /// Two named fakes. No radio.
    Fake,
    /// Scan and connect report that Bluetooth is off.
    Off,
}

/// What the process asked for, before the default settings path is filled in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaunchFlags {
    /// Trainer factory to install.
    pub mode: TrainerMode,
    /// Settings file from `--settings` or the environment. `None` means the default.
    pub settings_path: Option<PathBuf>,
    /// `Some(0)` asks the OS for a free dashboard port.
    pub dashboard_port: Option<u16>,
    /// Bind the dashboard on loopback instead of every interface.
    pub dashboard_loopback: bool,
    /// Open the menu and confirm Quit. For the release-export exit check.
    pub quit_from_menu: bool,
    /// Skip the title screen and start the ride. Scripted shots and smoke use this.
    pub skip_title: bool,
    /// Title, start, ride, end, then quit. The release exit check.
    pub play_through: bool,
    /// Start silent. A settings change can still raise the volume.
    pub no_music: bool,
    /// Start with ambience and effects silent. A settings change can raise them.
    ///
    /// Also set for smoke, pictures, bench runs, and `--quit-after`, unless
    /// `--sound` or `--sound-capture` was given.
    pub no_sound: bool,
    /// Write a mix of a scripted ride to this wav, then quit.
    pub sound_capture: Option<PathBuf>,
    /// Hide the console, chips, and toasts. World screenshots use this.
    pub no_hud: bool,
}

/// Environment values the game already read. Empty strings count as unset.
#[derive(Clone, Debug, Default)]
pub struct LaunchEnv {
    /// `PENGUIN_PASS_FAKE_TRAINER`.
    pub fake_trainer: Option<String>,
    /// `PENGUIN_PASS_NO_BLUETOOTH`.
    pub no_bluetooth: Option<String>,
    /// `PENGUIN_PASS_SETTINGS`.
    pub settings: Option<String>,
    /// `PENGUIN_PASS_DASHBOARD_PORT`.
    pub dashboard_port: Option<String>,
    /// `PENGUIN_PASS_DASHBOARD_LOOPBACK`.
    pub dashboard_loopback: Option<String>,
    /// `PENGUIN_PASS_NO_MUSIC`.
    pub no_music: Option<String>,
    /// `PENGUIN_PASS_NO_SOUND`.
    pub no_sound: Option<String>,
    /// `PENGUIN_PASS_NO_HUD`.
    pub no_hud: Option<String>,
}

/// User arguments win over the environment. Later flags win over earlier ones.
pub fn parse_launch(args: &[String], env: &LaunchEnv) -> LaunchFlags {
    parse_launch_in(args, env, &[])
}

/// [`parse_launch`] plus Godot's own arguments (`--headless`, `--quit-after`, the script).
pub fn parse_launch_in(args: &[String], env: &LaunchEnv, engine_args: &[String]) -> LaunchFlags {
    let mut mode = env_mode(env);
    let mut settings_path = nonempty(&env.settings).map(PathBuf::from);
    let mut dashboard_port = nonempty(&env.dashboard_port).and_then(|value| value.parse().ok());
    let mut dashboard_loopback = is_flag(env.dashboard_loopback.as_deref());
    let mut quit_from_menu = false;
    let mut skip_title = false;
    let mut play_through = false;
    let mut no_music = is_flag(env.no_music.as_deref());
    let mut no_sound = is_flag(env.no_sound.as_deref());
    let mut sound_explicit = false;
    let mut sound_capture = None;
    let mut no_hud = is_flag(env.no_hud.as_deref());
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--fake-trainer" => mode = TrainerMode::Fake,
            "--no-bluetooth" => mode = TrainerMode::Off,
            "--dashboard-loopback" => dashboard_loopback = true,
            "--quit-from-menu" => quit_from_menu = true,
            "--skip-title" => skip_title = true,
            "--play-through" => play_through = true,
            "--no-music" => no_music = true,
            "--no-sound" => {
                no_sound = true;
                sound_explicit = false;
            }
            "--sound" => {
                no_sound = false;
                sound_explicit = true;
            }
            "--no-hud" => no_hud = true,
            "--sound-capture" => {
                if let Some(value) = args.get(index + 1) {
                    sound_capture = Some(PathBuf::from(value));
                    no_sound = false;
                    sound_explicit = true;
                    index += 1;
                }
            }
            "--settings" => {
                if let Some(value) = args.get(index + 1) {
                    settings_path = Some(PathBuf::from(value));
                    index += 1;
                }
            }
            "--dashboard-port" => {
                if let Some(value) = args.get(index + 1) {
                    if let Ok(port) = value.parse::<u16>() {
                        dashboard_port = Some(port);
                    }
                    index += 1;
                }
            }
            _ => {}
        }
        index += 1;
    }
    if !sound_explicit && (automated(args) || automated(engine_args)) {
        no_sound = true;
    }
    LaunchFlags {
        mode,
        settings_path,
        dashboard_port,
        dashboard_loopback,
        quit_from_menu,
        skip_title,
        play_through,
        no_music,
        no_sound,
        sound_capture,
        no_hud,
    }
}

fn automated(args: &[String]) -> bool {
    args.iter().any(|arg| {
        arg == "--quit-after"
            || arg == "--headless"
            || arg == "--bench"
            || arg.ends_with("smoke.gd")
            || arg.ends_with("shot.gd")
            || arg.ends_with("hud_shot.gd")
            || arg.ends_with("ingame.gd")
            || arg.ends_with("world.gd")
    })
}

fn env_mode(env: &LaunchEnv) -> TrainerMode {
    if is_flag(env.fake_trainer.as_deref()) {
        TrainerMode::Fake
    } else if is_flag(env.no_bluetooth.as_deref()) {
        TrainerMode::Off
    } else {
        TrainerMode::Real
    }
}

fn is_flag(value: Option<&str>) -> bool {
    matches!(value, Some("1") | Some("true") | Some("yes"))
}

fn nonempty(value: &Option<String>) -> Option<&str> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arguments_override_the_environment_and_the_later_flag_wins() {
        let env = LaunchEnv {
            fake_trainer: Some("1".to_owned()),
            settings: Some("/tmp/env.toml".to_owned()),
            dashboard_port: Some("9".to_owned()),
            ..LaunchEnv::default()
        };
        let args = vec![
            "--no-bluetooth".to_owned(),
            "--settings".to_owned(),
            "/tmp/args.toml".to_owned(),
            "--dashboard-port".to_owned(),
            "0".to_owned(),
            "--dashboard-loopback".to_owned(),
            "--fake-trainer".to_owned(),
            "--quit-from-menu".to_owned(),
            "--skip-title".to_owned(),
            "--play-through".to_owned(),
            "--no-music".to_owned(),
            "--no-hud".to_owned(),
        ];
        let flags = parse_launch(&args, &env);
        assert_eq!(flags.mode, TrainerMode::Fake);
        assert_eq!(
            flags.settings_path.unwrap(),
            PathBuf::from("/tmp/args.toml")
        );
        assert_eq!(flags.dashboard_port, Some(0));
        assert!(flags.dashboard_loopback);
        assert!(flags.quit_from_menu);
        assert!(flags.skip_title);
        assert!(flags.play_through);
        assert!(flags.no_music);
        assert!(flags.no_hud);
    }

    #[test]
    fn a_bare_launch_is_the_real_trainer_and_the_default_path() {
        let flags = parse_launch(&[], &LaunchEnv::default());
        assert_eq!(flags.mode, TrainerMode::Real);
        assert!(!flags.quit_from_menu);
        assert!(!flags.skip_title);
        assert!(!flags.play_through);
        assert!(!flags.no_music);
        assert!(!flags.no_sound);
        assert!(flags.sound_capture.is_none());
        assert!(!flags.no_hud);
        assert!(flags.settings_path.is_none());
        assert!(flags.dashboard_port.is_none());
        assert!(!flags.dashboard_loopback);
    }

    #[test]
    fn a_bad_port_is_ignored_and_blank_env_is_unset() {
        let env = LaunchEnv {
            no_bluetooth: Some("yes".to_owned()),
            settings: Some("  ".to_owned()),
            dashboard_port: Some("nope".to_owned()),
            ..LaunchEnv::default()
        };
        let flags = parse_launch(&["--dashboard-port".to_owned(), "lots".to_owned()], &env);
        assert_eq!(flags.mode, TrainerMode::Off);
        assert!(flags.settings_path.is_none());
        assert!(flags.dashboard_port.is_none());
    }

    #[test]
    fn automated_runs_are_silent_until_sound_is_asked_for() {
        let engine = vec!["--headless".to_owned(), "res://tests/smoke.gd".to_owned()];
        let flags = parse_launch_in(&[], &LaunchEnv::default(), &engine);
        assert!(flags.no_sound);
        let flags = parse_launch_in(
            &["--sound".to_owned()],
            &LaunchEnv::default(),
            &["--quit-after".to_owned(), "90".to_owned()],
        );
        assert!(!flags.no_sound);
        let flags = parse_launch(
            &[
                "--bench".to_owned(),
                "--no-sound".to_owned(),
                "--sound".to_owned(),
            ],
            &LaunchEnv::default(),
        );
        assert!(!flags.no_sound);
        let flags = parse_launch_in(
            &["--sound-capture".to_owned(), "mix.wav".to_owned()],
            &LaunchEnv::default(),
            &["--headless".to_owned()],
        );
        assert!(!flags.no_sound);
        assert_eq!(flags.sound_capture.unwrap(), PathBuf::from("mix.wav"));
        let env = LaunchEnv {
            no_sound: Some("1".to_owned()),
            ..LaunchEnv::default()
        };
        assert!(parse_launch(&[], &env).no_sound);
    }
}
