//! Ride session. Owns the world, the rider, the camera, the HUD, and the menu.
//!
//! Each frame reads the trainer through [`pp_session::Session`], steps the
//! ride, publishes telemetry, then moves the bike and the HUD. The session
//! owns the physics, the trainer link, and the TV dashboard.

use std::path::PathBuf;
use std::sync::Arc;

use godot::classes::input::MouseMode;
use godot::classes::node::PhysicsInterpolationMode;
use godot::classes::node::ProcessMode;
use godot::classes::{INode3D, Input, InputEvent, Node3D, Time};
use godot::obj::{Base, Gd, WithBaseField};
use godot::prelude::*;
use pp_core::{Telemetry, TrainerSample};
use pp_course::Course;
use pp_session::{Session, Settings};

use crate::camera::CameraRig;
use crate::fake_trainer::FakeHub;
use crate::hud::Hud;
use crate::input_map;
use crate::launch::{self, Launch, TrainerMode};
use crate::menu::{self, Command, Effect, RideMenu};
use crate::music::{MusicPlayer, MusicScene};
use crate::rider::Rider;
use crate::screens::Screens;
use crate::sim::{self, SimRider};
use crate::world;
use pp_view::shell::{CourseCard, Screen, Shell, ShellEffect};

/// Root of the Penguin Pass scene. `main.tscn` is this one node.
#[derive(GodotClass)]
#[class(base = Node3D)]
pub struct Game {
    base: Base<Node3D>,
    session: Option<Session>,
    /// Keyboard repeat. Power itself lives on the session.
    keys: SimRider,
    hub: Option<Arc<FakeHub>>,
    launch: Option<Launch>,
    menu: Option<RideMenu>,
    shell: Shell,
    screens: Option<Screens>,
    music: Option<Gd<MusicPlayer>>,
    music_dialog: Option<Gd<godot::classes::FileDialog>>,
    music_scene: MusicScene,
    music_report: String,
    last_announced: Option<String>,
    play_step: u32,
    settings_warning: String,
    hud: Option<Gd<Hud>>,
    rider: Option<Gd<Rider>>,
    camera: Option<Gd<CameraRig>>,
    first_person: bool,
    last: Telemetry,
    theme_name: String,
    theme_fallback: bool,
    scatter_count: i32,
    scatter_checksum: i64,
    scatter_violations: i32,
    asset_models: i32,
    asset_textures: i32,
    asset_sky: i32,
    unknown_kind_empty: bool,
    menu_hold: pp_view::hold::MenuHold,
    /// Set when the world has been built, so `ready` is idempotent on reload.
    started: bool,
    /// Milliseconds from `ready` start to the parts line.
    ready_ms: u64,
    /// Frames left after the music players were stopped, before the process quits.
    quit_frames: u8,
}

#[godot_api]
impl INode3D for Game {
    fn init(base: Base<Node3D>) -> Self {
        Self {
            base,
            session: None,
            keys: SimRider::default(),
            hub: None,
            launch: None,
            menu: None,
            shell: Shell::new(false),
            screens: None,
            music: None,
            music_dialog: None,
            music_scene: MusicScene::Title,
            music_report: String::new(),
            last_announced: None,
            play_step: 0,
            settings_warning: String::new(),
            hud: None,
            rider: None,
            camera: None,
            first_person: false,
            last: Telemetry::default(),
            theme_name: String::new(),
            theme_fallback: false,
            scatter_count: 0,
            scatter_checksum: 0,
            scatter_violations: 0,
            asset_models: 0,
            asset_textures: 0,
            asset_sky: 0,
            unknown_kind_empty: true,
            menu_hold: pp_view::hold::MenuHold::default(),
            started: false,
            ready_ms: 0,
            quit_frames: 0,
        }
    }

    fn ready(&mut self) {
        if self.started {
            return;
        }
        self.started = true;
        let started_ms = Time::singleton().get_ticks_msec();
        let stamp = |mark: u64| Time::singleton().get_ticks_msec() - mark;
        input_map::install();
        self.base_mut().set_process_mode(ProcessMode::ALWAYS);
        if let Some(mut window) = self.base().get_window() {
            window.set_title("Penguin Pass");
        }
        Input::singleton().set_mouse_mode(MouseMode::VISIBLE);
        let preamble_ms = stamp(started_ms);

        let session_mark = Time::singleton().get_ticks_msec();
        let launch = Launch::from_engine();
        let skip_title = launch.skip_title;
        let loaded = Settings::load(&launch.settings_path);
        let load_warning = loaded.warning.clone();
        self.settings_warning = loaded.warning.clone().unwrap_or_default();
        if let Some(warning) = &loaded.warning {
            godot_warn!("Penguin Pass: {warning}");
        }
        let (mut session, hub) = launch::open_session(&launch, loaded.settings);
        self.shell = Shell::new(skip_title);
        if skip_title {
            session.set_sim_power(150);
            self.keys.set_target_power(150.0);
        } else {
            session.hold_at_start();
            session.set_sim_power(0);
            self.keys.set_target_power(0.0);
        }
        session.note_settings_load(&pp_session::LoadedSettings {
            settings: session.settings().clone(),
            warning: load_warning,
        });
        session.auto_connect();
        menu::apply_window_mode(session.settings().fullscreen);
        let course = session.course().clone();
        self.hub = hub;
        self.launch = Some(launch);
        self.session = Some(session);
        godot_print!("PENGUIN_PASS_SETTINGS {}", self.settings_path().display());
        let session_ms = stamp(session_mark);

        let assets_mark = Time::singleton().get_ticks_msec();
        let mut world_root = Node3D::new_alloc();
        world_root.set_name("World");
        let catalog = crate::assets::Catalog::load();
        let assets_ms = stamp(assets_mark);
        let world_mark = Time::singleton().get_ticks_msec();
        let (theme, fallback) = crate::theme::load_or_builtin();
        self.unknown_kind_empty = !catalog.has_models("not-a-real-kind", &[]);
        self.theme_name = theme.name.clone();
        self.theme_fallback = fallback;
        world::build_environment(&mut world_root, &theme, &catalog);
        let report = world::build_course(&mut world_root, &course, &theme, &catalog);
        self.scatter_count = report.count;
        self.scatter_checksum = report.checksum;
        self.scatter_violations = report.violations;
        self.base_mut().add_child(&world_root);
        self.asset_models = catalog.loaded_models();
        self.asset_textures = catalog.loaded_textures();
        self.asset_sky = catalog.loaded_sky();
        godot_print!(
            "PENGUIN_PASS_ASSETS models={} textures={} sky={}",
            self.asset_models,
            self.asset_textures,
            self.asset_sky
        );
        let world_ms = stamp(world_mark);

        let hud_mark = Time::singleton().get_ticks_msec();
        let mut rider = Rider::new_alloc();
        rider.set_name("Rider");
        rider.set_physics_interpolation_mode(PhysicsInterpolationMode::OFF);
        self.base_mut().add_child(&rider);

        let mut camera = CameraRig::new_alloc();
        camera.set_name("CameraRig");
        camera.set_physics_interpolation_mode(PhysicsInterpolationMode::OFF);
        self.base_mut().add_child(&camera);

        let mut hud = Hud::new_alloc();
        hud.set_name("Hud");
        self.base_mut().add_child(&hud);

        {
            let profile: Vec<(f64, f32)> = course
                .profile(20.0)
                .iter()
                .map(|point| (point.distance_m, point.elevation_m))
                .collect();
            let mut hud = hud.bind_mut();
            hud.set_profile(&profile, course.length_m());
            hud.set_status(sim::STATUS);
            hud.set_tv_url("");
        }

        if !skip_title {
            hud.set_visible(false);
        }
        self.rider = Some(rider);
        self.camera = Some(camera);
        self.hud = Some(hud);
        let hud_ms = stamp(hud_mark);

        let title_mark = Time::singleton().get_ticks_msec();
        let mut parent = self.base_mut().clone();
        self.menu = Some(RideMenu::new(&mut parent));
        self.screens = Some(Screens::new(&mut parent));
        self.refresh_screens();
        let title_ms = stamp(title_mark);

        let music_mark = Time::singleton().get_ticks_msec();
        self.start_music(skip_title);
        let music_ms = stamp(music_mark);

        let tail_mark = Time::singleton().get_ticks_msec();
        let telemetry = self.session().telemetry();
        self.present(&telemetry, 0.0);
        let tail_ms = stamp(tail_mark);
        let ready_ms = Time::singleton().get_ticks_msec() - started_ms;
        self.ready_ms = ready_ms;
        let other_ms = preamble_ms + tail_ms;
        godot_print!("PENGUIN_PASS_READY_MS {ready_ms}");
        godot_print!(
            "PENGUIN_PASS_READY_PARTS session={session_ms} assets={assets_ms} world={world_ms} hud={hud_ms} music={music_ms} title={title_ms} other={other_ms}"
        );
    }

    fn process(&mut self, delta: f64) {
        if self.quit_frames > 0 {
            self.quit_frames -= 1;
            if self.quit_frames == 0 {
                self.base().get_tree().quit();
            }
            return;
        }
        if self
            .launch
            .as_ref()
            .is_some_and(|launch| launch.quit_from_menu)
        {
            self.confirm_menu_quit();
            return;
        }
        self.advance_play_through();
        // A stalled frame should not fling the bike. Hold-to-repeat uses the same cap.
        let dt = delta.clamp(0.0, 0.1);
        if self.shell.screen() == Screen::Riding {
            let up = Input::singleton().is_action_pressed("power_up");
            let down = Input::singleton().is_action_pressed("power_down");
            self.keys.poll_keys(dt, up, down);
            let watts = self.keys.target_power_w().round() as u16;
            self.session_mut().set_sim_power(watts);
        } else {
            self.poll_shell(dt);
            if self.shell.holds_rider() {
                self.session_mut().set_sim_power(0);
            }
        }
        self.show_events();
        let telemetry = self.step_ride(dt);
        self.present(&telemetry, dt);
        self.refresh_screens();
        self.sync_music();
        self.announce_track();
        if let Some(screens) = self.screens.as_mut() {
            screens.tick_toast(dt);
        }
    }

    fn unhandled_input(&mut self, event: Gd<InputEvent>) {
        if event.is_echo() {
            return;
        }
        if event.is_action_pressed("pause_menu") || event.is_action_pressed("menu_back") {
            self.on_menu_key();
        } else if event.is_action_pressed("toggle_camera") {
            if self.shell.screen() != Screen::Riding {
                return;
            }
            self.first_person = !self.first_person;
            let last = self.last;
            self.present(&last, 0.0);
        } else if event.is_action_pressed("toggle_fullscreen") {
            self.toggle_fullscreen();
        } else if event.is_action_pressed("music_next") && self.shell.screen() == Screen::Riding {
            self.skip_track();
        } else {
            return;
        }
        if let Some(mut viewport) = self.base().get_viewport() {
            viewport.set_input_as_handled();
        }
    }
}

#[godot_api]
impl Game {
    /// Session distance, in meters.
    #[func]
    pub fn distance_m(&self) -> f64 {
        self.session().telemetry().distance_m
    }

    /// Keyboard power target, in watts.
    #[func]
    pub fn power_w(&self) -> f64 {
        f64::from(self.session().sim_power_w())
    }

    /// Game speed, in kilometers per hour.
    #[func]
    pub fn speed_kph(&self) -> f64 {
        f64::from(self.session().telemetry().speed_kph())
    }

    /// Road grade, in percent.
    #[func]
    pub fn grade_pct(&self) -> f64 {
        f64::from(self.session().telemetry().grade_pct)
    }

    /// Lap length of the loaded course, in meters.
    #[func]
    pub fn course_length_m(&self) -> f64 {
        self.session().course().length_m()
    }

    /// Where the course says the rider is, including the same lift as the mesh.
    #[func]
    pub fn course_position(&self) -> Vector3 {
        pose(
            self.session().course(),
            self.session().telemetry().lap_distance_m,
        )
        .0
    }

    /// Current lap. The first lap is 1.
    #[func]
    pub fn lap(&self) -> i64 {
        i64::from(self.last.lap.max(1))
    }

    /// `true` when the handlebar camera is active.
    #[func]
    pub fn camera_is_first_person(&self) -> bool {
        self.first_person
    }

    /// Rider origin, for a headless position check.
    #[func]
    pub fn rider_position(&mut self) -> Vector3 {
        self.rider
            .as_mut()
            .map(|rider| rider.bind_mut().rider_origin())
            .unwrap_or(Vector3::ZERO)
    }

    /// Power digits on the HUD.
    #[func]
    pub fn hud_power_text(&mut self) -> GString {
        self.hud_mut(|hud| hud.power_text())
    }

    /// Speed digits on the HUD.
    #[func]
    pub fn hud_speed_text(&mut self) -> GString {
        self.hud_mut(|hud| hud.speed_text())
    }

    /// Cadence digits on the HUD.
    #[func]
    pub fn hud_cadence_text(&mut self) -> GString {
        self.hud_mut(|hud| hud.cadence_text())
    }

    /// Heart-rate digits on the HUD.
    #[func]
    pub fn hud_heart_text(&mut self) -> GString {
        self.hud_mut(|hud| hud.heart_text())
    }

    /// Grade digits on the HUD.
    #[func]
    pub fn hud_grade_text(&mut self) -> GString {
        self.hud_mut(|hud| hud.grade_text())
    }

    /// Distance digits on the HUD.
    #[func]
    pub fn hud_distance_text(&mut self) -> GString {
        self.hud_mut(|hud| hud.distance_text())
    }

    /// Lap digits on the HUD.
    #[func]
    pub fn hud_lap_text(&mut self) -> GString {
        self.hud_mut(|hud| hud.lap_text())
    }

    /// Elapsed-time text on the HUD.
    #[func]
    pub fn hud_elapsed_text(&mut self) -> GString {
        self.hud_mut(|hud| hud.elapsed_text())
    }

    /// Elevation digits on the HUD.
    #[func]
    pub fn hud_elevation_text(&mut self) -> GString {
        self.hud_mut(|hud| hud.elevation_text())
    }

    /// Status line.
    #[func]
    pub fn hud_status_text(&mut self) -> GString {
        self.hud_mut(|hud| hud.status_text())
    }

    /// TV dashboard slot.
    #[func]
    pub fn hud_tv_text(&mut self) -> GString {
        self.hud_mut(|hud| hud.tv_text())
    }

    /// Samples loaded into the elevation strip.
    #[func]
    pub fn profile_point_count(&self) -> i32 {
        self.hud
            .as_ref()
            .map(|hud| hud.bind().profile_point_count())
            .unwrap_or(0)
    }

    /// Clamp and store the keyboard power target.
    #[func]
    pub fn debug_set_target_power(&mut self, watts: f64) {
        let clamped = (watts as f32).clamp(0.0, 1500.0);
        self.keys.set_target_power(clamped);
        self.session_mut().set_sim_power(clamped.round() as u16);
    }

    /// Step the ride by `seconds` in fixed 1/60 s ticks, without drawing.
    #[func]
    pub fn debug_advance(&mut self, seconds: f64) {
        let mut left = seconds.max(0.0);
        while left > 0.0 {
            let dt = left.min(1.0 / 60.0);
            let telemetry = self.step_ride(dt);
            self.present(&telemetry, dt);
            left -= dt;
        }
    }

    /// Step physics, without drawing, until session distance reaches `distance_m`.
    ///
    /// `elapsed_s` is unused: the ride clock is whatever the steps accumulate.
    /// Power is the keyboard target used for every step.
    #[func]
    pub fn debug_seek(&mut self, distance_m: f64, power_w: f64, _elapsed_s: f64) {
        self.debug_set_target_power(power_w);
        let target = distance_m.max(0.0);
        if self.session().telemetry().distance_m > target + 0.5 {
            self.session_mut().restart();
        }
        let mut guard = 0;
        while self.session().telemetry().distance_m + 0.4 < target && guard < 20_000 {
            self.integrate_ride(0.5, &self.keys.sample());
            guard += 1;
        }
        if self.session().telemetry().distance_m < 0.05 {
            self.integrate_ride(0.05, &self.keys.sample());
        }
        let telemetry = self.session().telemetry();
        self.present(&telemetry, 0.0);
    }

    /// Switch camera mode. `enabled` selects the handlebar view.
    #[func]
    pub fn debug_set_first_person(&mut self, enabled: bool) {
        self.first_person = enabled;
        let last = self.last;
        self.present(&last, 0.0);
    }

    /// Push a snapshot whose cadence and heart rate are missing.
    #[func]
    pub fn debug_show_missing_cadence(&mut self) {
        let mut telemetry = self.session().telemetry();
        telemetry.cadence_rpm = None;
        telemetry.heart_rate_bpm = None;
        self.present(&telemetry, 0.0);
    }

    /// Show the LAN dashboard address. Empty clears it.
    ///
    /// Plug-in: `pp-cast` calls this with the URL it is serving.
    #[func]
    pub fn set_tv_url(&mut self, url: GString) {
        if let Some(hud) = self.hud.as_mut() {
            hud.bind_mut().set_tv_url(&url.to_string());
        }
    }

    /// Loaded theme name.
    #[func]
    pub fn theme_name(&self) -> GString {
        GString::from(&self.theme_name)
    }

    /// glTF files that loaded. Zero when `godot/assets/` is absent.
    #[func]
    pub fn asset_model_count(&self) -> i32 {
        self.asset_models
    }

    /// The `PENGUIN_PASS_ASSETS` line, so the smoke test can read it back.
    #[func]
    pub fn asset_line(&self) -> GString {
        GString::from(
            format!(
                "PENGUIN_PASS_ASSETS models={} textures={} sky={}",
                self.asset_models, self.asset_textures, self.asset_sky
            )
            .as_str(),
        )
    }

    /// An unknown scatter kind names no library mesh, so the placeholder is used.
    #[func]
    pub fn unknown_kind_is_empty(&self) -> bool {
        self.unknown_kind_empty
    }

    /// True when the theme file was missing or rejected.
    #[func]
    pub fn theme_fell_back(&self) -> bool {
        self.theme_fallback
    }

    /// True when a broken theme document is rejected by the parser.
    #[func]
    pub fn debug_bad_theme_falls_back(&self) -> bool {
        crate::theme::parse("{").is_err()
    }

    /// Scatter instances placed beside the road.
    #[func]
    pub fn scatter_count(&self) -> i64 {
        i64::from(self.scatter_count)
    }

    /// Checksum of scatter transforms.
    #[func]
    pub fn scatter_checksum(&self) -> i64 {
        self.scatter_checksum
    }

    /// Recompute the checksum from the theme and course. Same seed, same value.
    #[func]
    pub fn debug_scatter_checksum_again(&self) -> i64 {
        let theme = if self.theme_fallback {
            crate::theme::Theme::builtin()
        } else {
            crate::theme::load_named(&self.theme_name).0
        };
        crate::world::scatter_checksum(self.session().course(), &theme)
    }

    /// Scatter instances that landed inside the road keep-out.
    #[func]
    pub fn scatter_road_violations(&self) -> i64 {
        i64::from(self.scatter_violations)
    }

    /// Wheel rotation in radians.
    #[func]
    pub fn wheel_angle(&mut self) -> f64 {
        self.rider
            .as_mut()
            .map(|rider| rider.bind_mut().wheel_angle())
            .unwrap_or(0.0)
    }

    /// Crank rotation in radians.
    #[func]
    pub fn crank_angle(&mut self) -> f64 {
        self.rider
            .as_mut()
            .map(|rider| rider.bind_mut().crank_angle())
            .unwrap_or(0.0)
    }

    /// Warning from the settings file, or empty when the file was fine.
    #[func]
    pub fn settings_warning(&self) -> GString {
        GString::from(self.settings_warning.as_str())
    }

    /// Absolute settings path this process is using.
    #[func]
    pub fn settings_file(&self) -> GString {
        GString::from(self.settings_path().display().to_string().as_str())
    }

    /// Open the settings menu.
    #[func]
    pub fn debug_menu_open(&mut self) {
        self.shell.open_settings();
        let mut menu = self.menu.take().expect("menu");
        menu.open(self.session());
        self.menu = Some(menu);
    }

    /// Close the settings menu and return to the ride.
    #[func]
    pub fn debug_menu_close(&mut self) {
        if let Some(menu) = self.menu.as_mut() {
            menu.close();
        }
        self.shell.close_menus();
        self.restore_power_hint();
        self.save_settings();
    }

    /// Which screen is up: title, ride, pause, settings, summary, course.
    #[func]
    pub fn debug_screen(&self) -> GString {
        let name = match self.shell.screen() {
            Screen::Title => "title",
            Screen::Riding => "ride",
            Screen::Pause => "pause",
            Screen::Settings => "settings",
            Screen::ConfirmEnd => "confirm-end",
            Screen::ConfirmQuit => "confirm-quit",
            Screen::Summary => "summary",
            Screen::Course => "course",
        };
        GString::from(name)
    }

    /// Return to the title at the start line.
    #[func]
    pub fn debug_show_title(&mut self) {
        self.session_mut().hold_at_start();
        self.session_mut().set_sim_power(0);
        self.shell = Shell::new(false);
        if let Some(menu) = self.menu.as_mut() {
            menu.close();
        }
        self.sync_music();
    }

    /// Leave the title and start the ride.
    #[func]
    pub fn debug_start_ride(&mut self) {
        if self.shell.screen() == Screen::Title {
            self.shell_command(Command::Accept, true);
        }
    }

    /// Load a generated course. Returns false until the session can swap courses.
    #[func]
    pub fn debug_load_course(&mut self, terrain: GString, distance_km: i64, seed: i64) -> bool {
        let Some(preset) = pp_session::TerrainPreset::parse(terrain.to_string().as_str()) else {
            return false;
        };
        let Ok(distance_km) = u16::try_from(distance_km) else {
            return false;
        };
        let Ok(seed) = u64::try_from(seed) else {
            return false;
        };
        let choice = pp_session::CourseChoice::Generated {
            terrain: preset,
            distance_km,
            seed,
        };
        self.load_choice(choice)
    }

    /// One menu command: up, down, left, right, accept, back.
    #[func]
    pub fn debug_menu_action(&mut self, action: GString) {
        let command = match action.to_string().as_str() {
            "up" => Command::Up,
            "down" => Command::Down,
            "left" => Command::Left,
            "right" => Command::Right,
            "accept" => Command::Accept,
            "back" => Command::Back,
            _ => return,
        };
        if self.menu.as_ref().is_some_and(RideMenu::is_open) {
            self.menu_command(command);
            if self.menu.as_ref().is_some_and(|menu| !menu.is_open()) {
                self.shell.leave_settings();
            }
        } else {
            self.shell_command(command, true);
        }
    }

    /// Title of the open menu page.
    #[func]
    pub fn debug_menu_title(&self) -> GString {
        self.menu
            .as_ref()
            .map(|menu| GString::from(menu.title().as_str()))
            .unwrap_or_default()
    }

    /// Explanation line under the menu rows.
    #[func]
    pub fn debug_menu_detail(&self) -> GString {
        self.menu
            .as_ref()
            .map(|menu| GString::from(menu.detail().as_str()))
            .unwrap_or_default()
    }

    /// Highlighted menu row.
    #[func]
    pub fn debug_menu_row(&self) -> GString {
        self.menu
            .as_ref()
            .map(|menu| GString::from(menu.selected_text().as_str()))
            .unwrap_or_default()
    }

    /// Whether the menu overlay is showing.
    #[func]
    pub fn debug_menu_open_state(&self) -> bool {
        self.menu.as_ref().is_some_and(RideMenu::is_open)
    }

    /// Whether the settings panel node is actually visible.
    #[func]
    pub fn debug_settings_visible(&self) -> bool {
        self.menu.as_ref().is_some_and(RideMenu::is_visible)
    }

    /// Drop a scan the trainer panel started, so the ride returns to the simulated rider.
    #[func]
    pub fn debug_clear_trainer(&mut self) {
        self.session_mut().disconnect();
    }

    /// Escape and the pad's B button both take this path.
    #[func]
    pub fn debug_press_back(&mut self) {
        self.on_menu_key();
    }

    /// Milliseconds spent in `ready`, matching `PENGUIN_PASS_READY_MS`.
    #[func]
    pub fn debug_ready_ms(&self) -> i64 {
        i64::try_from(self.ready_ms).unwrap_or(i64::MAX)
    }

    /// `title` or `ride`.
    #[func]
    pub fn debug_music_scene(&self) -> GString {
        GString::from(match self.music_scene {
            MusicScene::Title => "title",
            MusicScene::Ride => "ride",
        })
    }

    /// Ride tracks the player can start. Zero when the node is missing.
    #[func]
    pub fn debug_music_tracks(&self) -> i64 {
        self.music
            .as_ref()
            .map(|music| music.bind().track_count() as i64)
            .unwrap_or(0)
    }

    /// Music bus gain, in decibels.
    #[func]
    pub fn debug_music_bus_db(&self) -> f64 {
        let server = godot::classes::AudioServer::singleton();
        let index = server.get_bus_index("Music");
        if index < 0 {
            return -80.0;
        }
        f64::from(server.get_bus_volume_db(index))
    }

    /// The folder picker chose a directory.
    #[func]
    fn on_music_dir(&mut self, dir: GString) {
        let mut settings = self.session().settings().clone();
        settings.music_folder = Some(std::path::PathBuf::from(dir.to_string()));
        self.session_mut().apply_settings(settings);
        self.apply_music_settings();
        let path = self.settings_path().clone();
        let _ = self.session_mut().save_settings(&path);
    }

    /// Last grade sent to the fake trainer, or -1 when none has been sent.
    #[func]
    pub fn debug_fake_grade(&self) -> f64 {
        self.hub
            .as_ref()
            .and_then(|hub| hub.last_grade())
            .map(f64::from)
            .unwrap_or(-1.0)
    }

    /// How many grade commands the fake trainer has recorded.
    #[func]
    pub fn debug_fake_grade_count(&self) -> i64 {
        self.hub
            .as_ref()
            .map(|hub| i64::from(hub.grade_commands()))
            .unwrap_or(0)
    }

    /// Session status line, without the keyboard hint.
    #[func]
    pub fn debug_status_line(&self) -> GString {
        GString::from(self.session().status().line.as_str())
    }

    /// Names of discovered trainers, strongest first, separated by `|`.
    #[func]
    pub fn debug_discovered(&self) -> GString {
        let mut devices = self.session().discovered();
        devices.sort_by(|left, right| {
            right
                .rssi_dbm
                .unwrap_or(i16::MIN)
                .cmp(&left.rssi_dbm.unwrap_or(i16::MIN))
        });
        let names: Vec<String> = devices.into_iter().map(|device| device.name).collect();
        GString::from(names.join("|").as_str())
    }

    /// Rider mass the physics are using, in kilograms.
    #[func]
    pub fn debug_rider_mass_kg(&self) -> f64 {
        f64::from(self.session().rider().rider_mass_kg)
    }

    /// Trainer difficulty currently stored, as a fraction.
    #[func]
    pub fn debug_difficulty(&self) -> f64 {
        f64::from(self.session().settings().trainer_difficulty)
    }

    /// Whether resistance commands are enabled.
    #[func]
    pub fn debug_control_trainer(&self) -> bool {
        self.session().settings().control_trainer
    }

    /// Build a fresh session from the settings file, the way a second launch would.
    #[func]
    pub fn debug_reload_settings(&mut self) -> f64 {
        let launch = self.launch.clone().expect("launch");
        let loaded = Settings::load(&launch.settings_path);
        let (mut session, hub) = launch::open_session(&launch, loaded.settings);
        session.set_sim_power(self.keys.target_power_w().round() as u16);
        let mass = f64::from(session.rider().rider_mass_kg);
        self.session = Some(session);
        self.hub = hub;
        mass
    }

    /// Replace the session with one whose Bluetooth factory always fails.
    ///
    /// The world stays. The ride continues on the simulated rider.
    #[func]
    pub fn debug_use_no_bluetooth(&mut self) {
        let mut launch = self.launch.clone().expect("launch");
        launch.mode = TrainerMode::Off;
        let settings = self.session().settings().clone();
        let (session, _) = launch::open_session(&launch, settings);
        self.session = Some(session);
        self.hub = None;
        self.launch = Some(launch);
    }

    /// Point saves at `path` so a test can force a failed save.
    #[func]
    pub fn debug_set_settings_path(&mut self, path: GString) {
        if let Some(launch) = self.launch.as_mut() {
            launch.settings_path = PathBuf::from(path.to_string());
        }
    }
}

impl Game {
    /// One simulation step: sample, integrate, publish.
    fn step_ride(&mut self, dt: f64) -> Telemetry {
        let sample = self.read_trainer_sample();
        let telemetry = self.integrate_ride(dt, &sample);
        self.publish_telemetry(&telemetry);
        telemetry
    }

    /// Latest keyboard reading.
    ///
    /// The session reads the active trainer inside [`Session::tick`]. This
    /// seam stays so a later source can still be inspected before the step.
    /// The value returned here is what the simulated rider would report.
    fn read_trainer_sample(&mut self) -> TrainerSample {
        self.keys.sample()
    }

    /// Integrate `dt` seconds. The session applies the real trainer sample,
    /// sends the scaled grade, and publishes to the dashboard.
    fn integrate_ride(&mut self, dt: f64, _sample: &TrainerSample) -> Telemetry {
        self.advance_fake(dt as f32);
        self.session_mut().tick(dt)
    }

    /// Remember the snapshot. The dashboard publish happens inside the session tick.
    fn publish_telemetry(&mut self, telemetry: &Telemetry) {
        self.last = *telemetry;
    }

    fn advance_fake(&self, dt: f32) {
        if let Some(hub) = &self.hub {
            hub.advance(dt);
        }
    }

    fn session(&self) -> &Session {
        self.session.as_ref().expect("session")
    }

    fn session_mut(&mut self) -> &mut Session {
        self.session.as_mut().expect("session")
    }

    fn settings_path(&self) -> &PathBuf {
        &self.launch.as_ref().expect("launch").settings_path
    }

    fn status_sentence(&self, telemetry: &Telemetry) -> String {
        let status = self.session().status();
        pp_view::hud::status_sentence(
            telemetry.elapsed_s,
            &status.line,
            &self.settings_warning,
            self.session().settings().last_trainer_id.is_none(),
        )
    }

    fn tv_sentence(&self) -> String {
        self.session()
            .status()
            .dashboard_urls
            .first()
            .cloned()
            .unwrap_or_default()
    }

    fn present(&mut self, telemetry: &Telemetry, dt: f64) {
        self.last = *telemetry;
        let status = self.status_sentence(telemetry);
        let tv = self.tv_sentence();
        let show = self.shell.shows_hud();
        let hud_node = self.hud.clone();
        if let Some(mut hud_node) = hud_node {
            hud_node.set_visible(show);
            let mut hud = hud_node.bind_mut();
            hud.apply_telemetry(telemetry);
            hud.set_status(&status);
            hud.set_tv_url(&tv);
            hud.present(self.session(), dt);
        }
        let distance = telemetry.lap_distance_m;
        let course = self.session().course().clone();
        let (position, tangent) = pose(&course, distance);
        let ahead = course.tangent_at(distance + 8.0);
        let turn = tangent.x * ahead[2] - tangent.z * ahead[0];
        let lean = (turn * telemetry.speed_mps * 0.18).clamp(-0.3, 0.3);
        let ftp_w = f32::from(self.session().settings().ftp_w);
        if let Some(rider) = self.rider.as_mut() {
            let mut rider = rider.bind_mut();
            rider.set_ride_state(
                telemetry.power_w,
                ftp_w,
                telemetry.grade_pct,
                telemetry.speed_mps,
            );
            rider.place(
                position,
                tangent,
                telemetry.distance_m,
                telemetry.cadence_rpm,
                dt,
                lean,
            );
        }
        let rider_xf = self
            .rider
            .as_ref()
            .map(|rider| rider.get_global_transform());
        let first_person = self.first_person;
        if let (Some(camera), Some(transform)) = (self.camera.as_mut(), rider_xf) {
            let title_frame = self.shell.screen() == Screen::Title;
            camera
                .bind_mut()
                .follow(transform, dt, first_person, title_frame);
        }
    }

    fn toggle_fullscreen(&mut self) {
        let mut settings = self.session().settings().clone();
        settings.fullscreen = !settings.fullscreen;
        menu::apply_window_mode(settings.fullscreen);
        self.session_mut().apply_settings(settings);
        self.save_settings();
    }

    fn confirm_menu_quit(&mut self) {
        if let Some(launch) = self.launch.as_mut() {
            launch.quit_from_menu = false;
        }
        let path = self.settings_path().clone();
        let mut menu = self.menu.take().expect("menu");
        menu.open(self.session());
        for _ in 0..11 {
            menu.handle(Command::Down, self.session_mut(), &path);
        }
        menu.handle(Command::Accept, self.session_mut(), &path);
        let effect = menu.handle(Command::Accept, self.session_mut(), &path);
        self.menu = Some(menu);
        if effect == Effect::Quit {
            self.request_quit();
        }
    }

    fn on_menu_key(&mut self) {
        if self.shell.screen() == Screen::Settings {
            self.menu_command(Command::Back);
            if self.menu.as_ref().is_some_and(|menu| !menu.is_open()) {
                self.shell.leave_settings();
            }
            self.sync_music();
            return;
        }
        self.shell_command(Command::Back, true);
        self.sync_music();
    }

    fn poll_shell(&mut self, dt: f64) {
        if self.shell.screen() == Screen::Settings {
            self.poll_menu(dt);
            if self.menu.as_ref().is_some_and(|menu| !menu.is_open()) {
                self.shell.leave_settings();
            }
            return;
        }
        let input = Input::singleton();
        let fresh = if input.is_action_just_pressed("menu_accept") {
            Some(Command::Accept)
        } else if input.is_action_just_pressed("menu_up") {
            Some(Command::Up)
        } else if input.is_action_just_pressed("menu_down") {
            Some(Command::Down)
        } else if input.is_action_just_pressed("menu_left") {
            Some(Command::Left)
        } else if input.is_action_just_pressed("menu_right") {
            Some(Command::Right)
        } else {
            None
        };
        if let Some(command) = fresh {
            self.shell_command(command, true);
            return;
        }
        if let Some(command) = self.held_menu_step(dt) {
            self.shell_command(command, false);
        }
    }

    fn shell_command(&mut self, command: Command, fresh: bool) {
        let count = if self.shell.screen() == Screen::Course {
            self.session_mut().course_catalog().len()
        } else {
            1
        };
        let snap = menu_snapshot(self.session());
        let effect = self.shell.handle(command, fresh, &snap, count);
        self.apply_shell(effect);
    }

    fn apply_shell(&mut self, effect: ShellEffect) {
        match effect {
            ShellEffect::None => {}
            ShellEffect::BeginRide | ShellEffect::RideAgain => {
                self.session_mut().hold_at_start();
                self.session_mut().begin_ride();
                self.keys.set_target_power(150.0);
                self.session_mut().set_sim_power(150);
                self.restore_power_hint();
            }
            ShellEffect::Resume => self.restore_power_hint(),
            ShellEffect::Quit => self.request_quit(),
            ShellEffect::EndRide => {
                self.session_mut().end_ride();
            }
            ShellEffect::BackToTitle => {
                self.session_mut().hold_at_start();
                self.session_mut().set_sim_power(0);
                self.keys.set_target_power(0.0);
            }
            ShellEffect::PickCourse(index) => self.pick_course(index),
            ShellEffect::Menu(action) => {
                let path = self.settings_path().clone();
                let mut menu = self.menu.take().expect("menu");
                if matches!(action, pp_view::menu::Action::Scan) {
                    menu.open(self.session());
                    let _ = menu.handle(Command::Accept, self.session_mut(), &path);
                } else {
                    menu.open(self.session());
                }
                self.menu = Some(menu);
            }
        }
        if self.shell.screen() == Screen::Settings
            && self.menu.as_ref().is_some_and(|menu| !menu.is_open())
        {
            let mut menu = self.menu.take().expect("menu");
            menu.open(self.session());
            self.menu = Some(menu);
        }
        self.sync_music();
    }

    fn refresh_screens(&mut self) {
        let facts = self.session().course_facts();
        let trainer = self.session().connection().trainer_name.unwrap_or_else(|| {
            if self.session().connection().phase == pp_session::LinkPhase::Searching {
                "Searching".to_owned()
            } else {
                "Not paired".to_owned()
            }
        });
        let detail = format!(
            "{:.1} km · {:.0} m ascent",
            facts.length_m / 1000.0,
            facts.total_ascent_m
        );
        let cards = self.course_cards();
        let totals = self.session().summary();
        let summary_line = format!(
            "{:.0} s · {:.2} km · {:.0} m up · {:.0} W avg",
            totals.elapsed_s,
            totals.distance_m / 1000.0,
            totals.ascent_m,
            totals.average_power_w
        );
        let view = self
            .shell
            .view(&facts.name, &trainer, &detail, &cards, &summary_line);
        if let Some(screens) = self.screens.as_mut() {
            screens.show(&view);
        }
    }

    fn course_cards(&mut self) -> Vec<CourseCard> {
        if self.shell.screen() != Screen::Course {
            return Vec::new();
        }
        self.session_mut()
            .course_catalog()
            .iter()
            .map(|option| CourseCard {
                name: option.name.clone(),
                length_m: option.length_m,
                ascent_m: option.ascent_m as f32,
                max_grade_pct: option.max_grade_pct,
            })
            .collect()
    }

    fn pick_course(&mut self, index: usize) {
        let Some(choice) = self
            .session_mut()
            .course_catalog()
            .get(index)
            .map(|option| option.choice.clone())
        else {
            return;
        };
        let _ = self.load_choice(choice);
    }

    fn load_choice(&mut self, choice: pp_session::CourseChoice) -> bool {
        let theme = choice.theme_name().to_owned();
        if let Err(err) = self.session_mut().set_course(choice) {
            godot_error!("Penguin Pass: {err}");
            return false;
        }
        self.rebuild_world(&theme);
        true
    }

    fn rebuild_world(&mut self, theme_name: &str) {
        if let Some(node) = self.base().get_node_or_null("World") {
            node.free();
        }
        let course = self.session().course().clone();
        let mut world_root = Node3D::new_alloc();
        world_root.set_name("World");
        let (theme, fallback) = crate::theme::load_named(theme_name);
        let catalog = crate::assets::Catalog::load();
        self.theme_name = theme.name.clone();
        self.theme_fallback = fallback;
        world::build_environment(&mut world_root, &theme, &catalog);
        let report = world::build_course(&mut world_root, &course, &theme, &catalog);
        self.scatter_count = report.count;
        self.scatter_checksum = report.checksum;
        self.scatter_violations = report.violations;
        self.base_mut().add_child(&world_root);
        let profile: Vec<(f64, f32)> = course
            .profile(20.0)
            .iter()
            .map(|point| (point.distance_m, point.elevation_m))
            .collect();
        if let Some(hud) = self.hud.as_mut() {
            hud.bind_mut().set_profile(&profile, course.length_m());
        }
    }

    fn show_events(&mut self) {
        let events = self.session_mut().drain_events();
        for event in events {
            let (text, seconds) = match event {
                pp_session::RideEvent::SettingsSaved => ("Settings saved".to_owned(), 3.0),
                pp_session::RideEvent::SettingsFileDamaged { .. } => (
                    "Settings file was damaged. Defaults are in use.".to_owned(),
                    6.0,
                ),
                pp_session::RideEvent::TrainerConnected { name } => {
                    (format!("{name} connected"), 3.0)
                }
                pp_session::RideEvent::TrainerLinkLost { name } => {
                    (format!("{name} disconnected. Reconnecting."), 6.0)
                }
                pp_session::RideEvent::TrainerReconnected { name } => {
                    (format!("{name} reconnected"), 3.0)
                }
                pp_session::RideEvent::ControlRefused { name } => {
                    (format!("{name} is read only"), 6.0)
                }
                pp_session::RideEvent::DashboardFailed { reason } => (reason, 6.0),
                pp_session::RideEvent::LapCompleted { lap, .. } => {
                    (format!("Lap {lap} complete"), 3.0)
                }
                pp_session::RideEvent::CourseSelected { name } => (format!("Course: {name}"), 3.0),
                pp_session::RideEvent::CourseUnavailable { detail } => (detail, 6.0),
            };
            if let Some(screens) = self.screens.as_mut() {
                screens.toast(&text, seconds);
            }
        }
    }

    fn advance_play_through(&mut self) {
        let armed = self
            .launch
            .as_ref()
            .is_some_and(|launch| launch.play_through);
        if !armed {
            return;
        }
        self.play_step += 1;
        match self.play_step {
            8 => self.shell_command(Command::Accept, true),
            40 => {
                self.shell_command(Command::Back, true);
                self.shell_command(Command::Down, true);
                self.shell_command(Command::Down, true);
                self.shell_command(Command::Down, true);
                self.shell_command(Command::Accept, true);
                self.shell_command(Command::Right, true);
                self.shell_command(Command::Accept, true);
            }
            70 => {
                self.shell_command(Command::Down, true);
                self.shell_command(Command::Accept, true);
            }
            90 => {
                for _ in 0..4 {
                    self.shell_command(Command::Down, true);
                }
                self.shell_command(Command::Accept, true);
                self.shell_command(Command::Right, true);
                self.shell_command(Command::Accept, true);
            }
            _ => {}
        }
    }

    fn poll_menu(&mut self, dt: f64) {
        let input = Input::singleton();
        let command = if input.is_action_just_pressed("menu_accept") {
            Some(Command::Accept)
        } else if input.is_action_just_pressed("menu_up") {
            Some(Command::Up)
        } else if input.is_action_just_pressed("menu_down") {
            Some(Command::Down)
        } else if input.is_action_just_pressed("menu_left") {
            Some(Command::Left)
        } else if input.is_action_just_pressed("menu_right") {
            Some(Command::Right)
        } else {
            self.held_menu_step(dt)
        };
        if let Some(command) = command {
            self.menu_command(command);
        }
    }

    fn held_menu_step(&mut self, dt: f64) -> Option<Command> {
        let input = Input::singleton();
        let held = if input.is_action_pressed("menu_up") {
            Some(Command::Up)
        } else if input.is_action_pressed("menu_down") {
            Some(Command::Down)
        } else if input.is_action_pressed("menu_left") {
            Some(Command::Left)
        } else if input.is_action_pressed("menu_right") {
            Some(Command::Right)
        } else {
            None
        };
        let Some(command) = held else {
            self.menu_hold.poll(dt, false);
            return None;
        };
        self.menu_hold.poll(dt, true).then_some(command)
    }

    fn start_music(&mut self, skip_title: bool) {
        let mut music = MusicPlayer::new_alloc();
        music.set_name("Music");
        self.base_mut().add_child(&music);
        let scene = if skip_title {
            MusicScene::Ride
        } else {
            MusicScene::Title
        };
        self.music_scene = scene;
        let settings = self.session().settings().clone();
        let mute = self.launch.as_ref().is_some_and(|launch| launch.no_music);
        {
            let mut player = music.bind_mut();
            // Volume first, so a silent launch never reads a track.
            player.set_volume_percent(if mute { 0 } else { settings.music_volume });
            player.set_scene(scene);
            let report = player.set_user_folder(settings.music_folder.as_deref());
            self.music_report = report.summary();
        }
        self.music = Some(music);
        self.push_music_report();
    }

    fn apply_music_settings(&mut self) {
        let settings = self.session().settings().clone();
        let folder = settings.music_folder.clone();
        let volume = settings.music_volume;
        if let Some(music) = &mut self.music {
            let mut player = music.bind_mut();
            player.set_volume_percent(volume);
            let report = player.set_user_folder(folder.as_deref());
            self.music_report = report.summary();
        }
        self.push_music_report();
    }

    fn push_music_report(&mut self) {
        let report = self.music_report.clone();
        if let Some(menu) = self.menu.as_mut() {
            menu.set_music_report(&report);
        }
    }

    fn sync_music(&mut self) {
        let screen = self.shell.screen();
        let duck = matches!(
            screen,
            Screen::Pause | Screen::ConfirmEnd | Screen::Settings
        );
        if screen != Screen::Settings {
            let scene = match screen {
                Screen::Riding | Screen::Pause | Screen::ConfirmEnd => MusicScene::Ride,
                _ => MusicScene::Title,
            };
            if scene != self.music_scene {
                self.music_scene = scene;
                if let Some(music) = &mut self.music {
                    music.bind_mut().set_scene(scene);
                }
            }
        }
        if let Some(music) = &mut self.music {
            music.bind_mut().set_ducked(duck);
        }
    }

    fn skip_track(&mut self) {
        if let Some(music) = &mut self.music {
            music.bind_mut().next_track();
        }
        self.last_announced = None;
        self.announce_track();
    }

    fn announce_track(&mut self) {
        let title = self
            .music
            .as_ref()
            .and_then(|music| music.bind().now_playing());
        let Some(title) = title else {
            return;
        };
        if self.last_announced.as_deref() == Some(title.as_str()) {
            return;
        }
        self.last_announced = Some(title.clone());
        if let Some(screens) = self.screens.as_mut() {
            screens.toast(&format!("Now playing: {title}"), 3.0);
        }
    }

    fn attach_music_dialog(&mut self) {
        use godot::classes::file_dialog::{Access, FileMode};
        let mut dialog = godot::classes::FileDialog::new_alloc();
        dialog.set_name("MusicFolderDialog");
        dialog.set_file_mode(FileMode::OPEN_DIR);
        dialog.set_access(Access::FILESYSTEM);
        let server = godot::classes::DisplayServer::singleton();
        let headless = server.get_name() == "headless";
        dialog.set_use_native_dialog(!headless);
        dialog.set_title("Music folder");
        let callable = Callable::from_object_method(&self.to_gd(), "on_music_dir");
        dialog.connect(&StringName::from("dir_selected"), &callable);
        self.base_mut().add_child(&dialog);
        self.music_dialog = Some(dialog);
    }

    fn open_music_dialog(&mut self) {
        if self.music_dialog.is_none() {
            self.attach_music_dialog();
        }
        if let Some(dialog) = &mut self.music_dialog {
            dialog.set_size(Vector2i::new(720, 500));
            dialog.popup_centered();
        }
    }

    /// Stop the streams, free the player, then quit on a later frame.
    ///
    /// The audio server drops a playback on a later mix, not inside `free`.
    /// Quitting in the same call leaks the stream and the playback.
    fn request_quit(&mut self) {
        if self.quit_frames > 0 {
            return;
        }
        self.release_music();
        self.quit_frames = 8;
    }

    fn release_music(&mut self) {
        if let Some(music) = self.music.take()
            && music.is_instance_valid()
        {
            music.free();
        }
    }

    fn restore_power_hint(&mut self) {
        if let Some(hud) = self.hud.as_mut() {
            hud.bind_mut().restore_power_hint();
        }
    }

    fn menu_command(&mut self, command: Command) {
        let path = self.settings_path().clone();
        let Some(mut menu) = self.menu.take() else {
            return;
        };
        let effect = menu.handle(command, self.session_mut(), &path);
        self.menu = Some(menu);
        if effect == Effect::Quit {
            self.request_quit();
        }
        if effect == Effect::Restarted {
            let telemetry = self.session().telemetry();
            self.present(&telemetry, 0.0);
        }
        if effect == Effect::ChooseMusicFolder {
            self.open_music_dialog();
        }
        self.apply_music_settings();
    }

    fn save_settings(&mut self) {
        let path = self.settings_path().clone();
        if let Err(err) = self.session().settings().save(&path) {
            godot_error!("Penguin Pass: could not save settings: {err}");
        }
    }

    fn hud_mut(&mut self, read: impl FnOnce(&mut Hud) -> GString) -> GString {
        match self.hud.as_mut() {
            Some(hud) => read(&mut hud.bind_mut()),
            None => GString::new(),
        }
    }
}

fn menu_snapshot(session: &Session) -> pp_view::menu::Snapshot {
    let status = session.status();
    pp_view::menu::Snapshot {
        settings: session.settings().clone(),
        status_line: status.line,
        dashboard: status.dashboard,
        dashboard_urls: status.dashboard_urls,
        dashboard_error: status.dashboard_error,
        devices: session
            .discovered()
            .into_iter()
            .map(|device| pp_view::menu::Device {
                id: device.id,
                name: device.name,
                rssi_dbm: device.rssi_dbm,
                power_only: !device.ftms && !device.fec,
            })
            .collect(),
        music_report: String::new(),
    }
}

fn pose(course: &Course, lap_distance_m: f64) -> (Vector3, Vector3) {
    let position = course.position_at(lap_distance_m);
    let tangent = course.tangent_at(lap_distance_m);
    (
        Vector3::new(position[0], position[1] + 0.06, position[2]),
        Vector3::new(tangent[0], tangent[1], tangent[2]),
    )
}
