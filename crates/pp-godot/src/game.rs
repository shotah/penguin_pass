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
use godot::classes::{
    INode3D, Input, InputEvent, InputEventJoypadButton, InputEventJoypadMotion, InputEventKey,
    Node3D, Time,
};
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
    lane: pp_view::lane::Lane,
    controllers: Option<Box<dyn pp_controllers::ControllerLink>>,
    pad_up: bool,
    pad_down: bool,
    lane_moved: bool,
    pad_badge: crate::hud::PadBadge,
    input_mode: pp_view::prompts::InputMode,
    input_locked: bool,
    pad_preview: bool,
    trainer_preview: Option<(String, String)>,
    ride_file: Option<pp_view::rides::RideFileNote>,
    fish: Option<crate::fish::FishSchool>,
    fish_preview: bool,
    fish_count: u32,
    fish_combo: u32,
    banners: Option<crate::banners::Banners>,
    atmosphere: Option<Gd<crate::world::Atmosphere>>,
    bots: Option<crate::bots::BotField>,
    ghost_rider: Option<Gd<Rider>>,
    ghost_distance_m: f64,
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
    /// Millisecond clock when the process may quit. The audio server needs
    /// real time, not a handful of uncapped frames, to drop playbacks.
    quit_at_ms: u64,
    sound: Option<Gd<crate::sound::SoundPlayer>>,
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
            lane: pp_view::lane::Lane::new(pp_view::lane::LaneSpec::right_lane(
                crate::road::ASPHALT_WIDTH_M * 0.5,
                crate::road::CENTER_LINE_HALF_M,
                crate::road::EDGE_LINE_M,
            )),
            controllers: None,
            pad_up: false,
            pad_down: false,
            lane_moved: false,
            pad_badge: crate::hud::PadBadge::Off,
            input_mode: pp_view::prompts::InputMode::Keyboard,
            input_locked: false,
            pad_preview: false,
            trainer_preview: None,
            ride_file: None,
            fish: None,
            fish_preview: false,
            fish_count: 0,
            fish_combo: 0,
            banners: None,
            atmosphere: None,
            bots: None,
            ghost_rider: None,
            ghost_distance_m: 0.0,
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
            quit_at_ms: 0,
            sound: None,
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
        if let Some(mode) = launch.input_mode {
            self.input_mode = mode;
            self.input_locked = true;
        } else if !Input::singleton().get_connected_joypads().is_empty() {
            self.input_mode = pp_view::prompts::InputMode::Gamepad;
        }
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
        self.atmosphere = world_root.try_get_node_as::<crate::world::Atmosphere>("Atmosphere");
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
        let school = {
            let mut root = self.base_mut();
            crate::fish::FishSchool::new(&mut root)
        };
        self.fish = Some(school);
        let banners = {
            let mut root = self.base_mut();
            crate::banners::Banners::new(&mut root)
        };
        self.banners = Some(banners);
        let bots = {
            let mut root = self.base_mut();
            crate::bots::BotField::new(&mut root)
        };
        self.bots = Some(bots);
        let mut ghost = Rider::new_alloc();
        ghost.set_name("Ghost");
        ghost.bind_mut().mark_ghost();
        ghost.set_visible(false);
        self.base_mut().add_child(&ghost);
        self.ghost_rider = Some(ghost);

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
        let game = self.to_gd();
        if let Some(menu) = self.menu.as_mut() {
            menu.bind_pointer(&game);
        }
        let no_hud = self.launch.as_ref().is_some_and(|launch| launch.no_hud);
        if let Some(screens) = self.screens.as_mut() {
            screens.set_toasts(!no_hud);
            screens.bind_pointer(&game);
        }
        self.refresh_screens();
        let title_ms = stamp(title_mark);

        let music_mark = Time::singleton().get_ticks_msec();
        self.start_music(skip_title);
        self.start_sound();
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
        if self.quit_at_ms > 0 {
            if Time::singleton().get_ticks_msec() >= self.quit_at_ms {
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
            self.keys
                .poll_keys(dt, up || self.pad_up, down || self.pad_down);
            if Input::singleton().is_action_just_pressed("lane_left") {
                self.lane.nudge(false);
                self.lane_moved = true;
            }
            if Input::singleton().is_action_just_pressed("lane_right") {
                self.lane.nudge(true);
                self.lane_moved = true;
            }
            if Input::singleton().is_action_just_pressed("activate_powerup") {
                self.activate_powerup();
            }
            if self.session().play().workout.is_some() {
                if Input::singleton().is_action_just_pressed("workout_easier") {
                    self.nudge_workout(-5);
                }
                if Input::singleton().is_action_just_pressed("workout_harder") {
                    self.nudge_workout(5);
                }
            }
            let watts = self.keys.target_power_w().round() as u16;
            self.session_mut().set_sim_power(watts);
        } else {
            self.poll_shell(dt);
            if self.shell.holds_rider() {
                self.session_mut().set_sim_power(0);
            }
        }
        self.sync_rider_preset();
        self.sync_zwift();
        self.poll_zwift();
        self.show_events();
        let telemetry = self.step_ride(dt);
        self.present(&telemetry, dt);
        self.refresh_screens();
        self.sync_music();
        self.sync_sound(dt);
        self.announce_track();
        if let Some(screens) = self.screens.as_mut() {
            screens.tick_toast(dt);
        }
    }

    fn unhandled_input(&mut self, event: Gd<InputEvent>) {
        self.note_device(&event);
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
        if let Some(mut hud) = self.hud.clone() {
            hud.bind_mut().clear_finish();
        }
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

    /// The `PENGUIN_PASS_SOUND` line, so the smoke test can read it back.
    #[func]
    pub fn sound_line(&self) -> GString {
        let line = self
            .sound
            .as_ref()
            .map(|sound| sound.bind().report_line())
            .unwrap_or_else(|| "PENGUIN_PASS_SOUND files=0 missing=1".to_owned());
        GString::from(line.as_str())
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
        menu.open(self.session_mut());
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
            Screen::Progress => "progress",
            Screen::Workout => "workout",
        };
        GString::from(name)
    }

    /// Return to the title at the start line.
    #[func]
    pub fn debug_show_title(&mut self) {
        self.session_mut().hold_at_start();
        self.session_mut().set_sim_power(0);
        self.fish_preview = false;
        self.shell = Shell::new(false);
        if let Some(menu) = self.menu.as_mut() {
            menu.close();
        }
        self.sync_music();
    }

    /// Show the title menu without sending the rider back to the start.
    #[func]
    pub fn debug_park_title(&mut self) {
        self.fish_preview = false;
        self.shell = Shell::new(false);
        if let Some(menu) = self.menu.as_mut() {
            menu.close();
        }
        self.sync_music();
    }

    /// Select a built-in workout by list index. Negative clears it.
    #[func]
    pub fn debug_select_workout(&mut self, index: i64) {
        if index < 0 {
            self.session_mut().select_workout(None);
            return;
        }
        let plans = self.session().workouts();
        if let Some(plan) = plans.get(index as usize) {
            let id = plan.id.clone();
            self.session_mut().select_workout(Some(&id));
        }
    }

    /// Wear this kit on the current rider. The id is saved with the settings.
    #[func]
    pub fn debug_set_kit(&mut self, id: GString) {
        let mut settings = self.session().settings().clone();
        settings.kit = id.to_string();
        self.session_mut().apply_settings(settings);
        self.sync_rider_preset();
    }

    /// Finish one fast lap so the next ride has a ghost and segment bests.
    #[func]
    pub fn debug_bank_best(&mut self) {
        self.debug_start_ride();
        self.debug_set_target_power(260.0);
        self.debug_seek(5300.0, 260.0, 0.0);
        self.session_mut().restart();
        self.ghost_distance_m = 0.0;
        self.shell = pp_view::shell::Shell::new(true);
    }

    /// Use the held powerup. No menu.
    #[func]
    pub fn debug_activate_powerup(&mut self) {
        self.session_mut().activate_powerup();
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
            self.menu_command(command, true);
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

    /// The pointer moved onto a title, pause, course, or summary row.
    #[func]
    fn on_screen_hover(&mut self, index: i64) {
        if index >= 0 {
            self.shell.hover(index as usize);
            self.refresh_screens();
        }
    }

    /// The pointer moved onto a settings row.
    #[func]
    fn on_menu_hover(&mut self, index: i64) {
        if index < 0 {
            return;
        }
        let Some(mut menu) = self.menu.take() else {
            return;
        };
        menu.hover(index as usize);
        menu.refresh(self.session_mut());
        self.menu = Some(menu);
    }

    /// Lay a pattern of fish ahead of the rider, for pictures. No session field.
    #[func]
    pub fn debug_show_fish(&mut self) {
        self.fish_preview = true;
        self.fish_count = 7;
        self.fish_combo = 3;
    }

    /// Show a controller chip from [`pp_controllers::SimControllers`]. No radio.
    ///
    /// `mode` is `scanning`, `1`, `2`, or `unavailable`.
    #[func]
    pub fn debug_show_controllers(&mut self, mode: GString) {
        use pp_controllers::{
            ControllerLink, DeviceKind, DiscoveredController, Side, SimControllers,
        };
        let mut sim = SimControllers::new();
        let mode = mode.to_string();
        match mode.as_str() {
            "1" | "2" => {
                let count = if mode == "2" { 2 } else { 1 };
                let found = (0..count)
                    .map(|index| DiscoveredController {
                        id: format!("SIM:{index}"),
                        name: format!("Play {index}"),
                        rssi_dbm: Some(-40),
                        kind: DeviceKind::Play,
                        side: Some(if index == 0 { Side::Left } else { Side::Right }),
                    })
                    .collect();
                sim.set_discovered(found);
                sim.connect_all();
            }
            "unavailable" => sim.unavailable(),
            _ => {
                sim.connect_all();
            }
        }
        self.controllers = Some(Box::new(sim));
        self.pad_preview = true;
        self.sync_zwift();
    }

    /// Trainer chip text for a screenshot of a long name.
    #[func]
    pub fn debug_trainer_label(&mut self, name: GString, detail: GString) {
        self.trainer_preview = Some((name.to_string(), detail.to_string()));
    }

    /// Put the rider in a lane slot and finish the ease.
    ///
    /// `0` is the left asphalt edge and `8` is the gravel right shoulder.
    #[func]
    pub fn debug_set_lane(&mut self, slot: i64) {
        let last = i64::from(pp_view::lane::SLOTS - 1);
        let target = slot.clamp(0, last) as u8;
        for _ in 0..pp_view::lane::SLOTS {
            self.lane.nudge(false);
        }
        for _ in 0..target {
            self.lane.nudge(true);
        }
        self.lane_moved = true;
        for _ in 0..12 {
            self.lane.step(0.05);
        }
    }

    /// Show the named rider preset.
    #[func]
    pub fn debug_set_rider(&mut self, id: GString) {
        let mut settings = self.session().settings().clone();
        settings.rider = id.to_string();
        self.session_mut().apply_settings(settings);
        self.sync_rider_preset();
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
        let hide_hud = self.launch.as_ref().is_some_and(|launch| launch.no_hud);
        let show = self.shell.shows_hud() && !hide_hud;
        let hud_node = self.hud.clone();
        if let Some(mut hud_node) = hud_node {
            hud_node.set_visible(show);
            let mut hud = hud_node.bind_mut();
            hud.apply_telemetry(telemetry);
            hud.set_status(&status);
            hud.set_tv_url(&tv);
            hud.present(self.session(), dt);
            if let Some((name, detail)) = self.trainer_preview.clone() {
                hud.debug_trainer_text(&name, &detail);
            }
            // Up for the opening of the ride, and until the first lane change.
            hud.set_lane_hint(!self.lane_moved);
            hud.set_pad_badge(self.pad_badge);
            let fish_line = if self.session().settings().arcade {
                pp_view::arcade::fish_counter(self.fish_count, self.fish_combo)
            } else {
                String::new()
            };
            hud.set_fish_text(&fish_line);
        }
        let distance = telemetry.lap_distance_m;
        let course = self.session().course().clone();
        let (position, tangent) = pose(&course, distance);
        let ahead = course.tangent_at(distance + 8.0);
        let turn = tangent.x * ahead[2] - tangent.z * ahead[0];
        let lane = self.lane.step(dt as f32);
        let lane_spec = self.lane.spec();
        let _slot = lane_spec.slot_fraction(lane.offset_m);
        self.session_mut()
            .set_lane_position(lane_spec.slot_fraction(lane.offset_m));
        let play = self.session().play();
        let arcade = self.session().settings().arcade;
        self.fish_count = play.fish_count;
        self.fish_combo = play.combo;
        let spots = if self.fish_preview {
            preview_fish(telemetry.distance_m)
        } else if arcade {
            play.fish_ahead
                .iter()
                .map(|fish| crate::fish::FishSpot {
                    index: fish.index,
                    distance_m: fish.total_distance_m,
                    slot: fish.slot,
                    golden: fish.golden,
                })
                .collect()
        } else {
            Vec::new()
        };
        let volume = f32::from(self.session().settings().music_volume) / 100.0;
        if let Some(fish) = self.fish.as_mut() {
            fish.sync(
                &spots,
                &course,
                lane_spec,
                dt as f32,
                volume,
                self.fish_combo,
            );
        }
        if let Some(banners) = self.banners.as_mut() {
            banners.sync(&play.segments, &course);
        }
        place_ghost(
            &mut self.ghost_rider,
            &mut self.ghost_distance_m,
            &play,
            &course,
            lane.slot,
            lane_spec,
            dt,
        );
        if let Some(mut hud) = self.hud.clone() {
            hud.bind_mut().set_chrome(ride_chrome(&play));
        }
        self.push_prompts();
        let side = crate::road::flat_right(tangent);
        let position = position + side * lane.offset_m;
        let lean = (turn * telemetry.speed_mps * 0.18 + lane.lean_rad).clamp(-0.35, 0.35);
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
        if let Some(bots) = self.bots.as_mut() {
            bots.sync(&play.bots, &course, lane_spec, telemetry.distance_m, dt);
        }
        self.apply_sky(position, tangent, telemetry.elapsed_s as f32, dt as f32);
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

    fn apply_sky(&mut self, position: Vector3, forward: Vector3, ride_s: f32, dt: f32) {
        let Some(mut atmosphere) = self.atmosphere.clone() else {
            return;
        };
        let settings = self.session().settings();
        let state = pp_world::ride_atmosphere(
            &self.theme_name,
            parse_time(&settings.time_of_day),
            parse_weather(&settings.weather),
            ride_s,
            1,
        );
        atmosphere
            .bind_mut()
            .apply_atmosphere(&state, position, forward, dt);
    }

    fn nudge_workout(&mut self, delta: i8) {
        let current = self
            .session()
            .play()
            .workout
            .map(|workout| workout.bias_percent)
            .unwrap_or(0);
        let next = (i16::from(current) + i16::from(delta)).clamp(-25, 25) as i8;
        self.session_mut().set_workout_bias(next);
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
        menu.open(self.session_mut());
        for _ in 0..17 {
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
            self.menu_command(Command::Back, true);
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
        self.note_ui(command, fresh);
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
            ShellEffect::ToggleCourseDirection => self.toggle_course_direction(),
            ShellEffect::Reroll(index) => {
                self.session_mut().reroll(index);
            }
            ShellEffect::OpenRides => self.open_rides_folder(),
            ShellEffect::PickWorkout(id) => {
                self.session_mut().select_workout(id.as_deref());
            }
            ShellEffect::WorkoutBias(delta) => self.nudge_workout(delta),
            ShellEffect::SkipWorkout => self.session_mut().skip_workout_step(),
            ShellEffect::UploadStrava => self.session_mut().upload_last_ride_to_strava(),
            ShellEffect::AcceptFtp(watts) => self.session_mut().accept_ramp_ftp(watts),
            ShellEffect::Menu(action) => {
                let path = self.settings_path().clone();
                let mut menu = self.menu.take().expect("menu");
                match action {
                    pp_view::menu::Action::Scan => {
                        menu.open(self.session_mut());
                        let _ = menu.handle(Command::Accept, self.session_mut(), &path);
                    }
                    pp_view::menu::Action::OpenHeart => {
                        menu.show_heart(self.session_mut());
                    }
                    _ => menu.open(self.session_mut()),
                }
                self.menu = Some(menu);
            }
        }
        if self.shell.screen() == Screen::Settings
            && self.menu.as_ref().is_some_and(|menu| !menu.is_open())
        {
            let mut menu = self.menu.take().expect("menu");
            menu.open(self.session_mut());
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
        if self.shell.screen() == Screen::Progress {
            let profile = self.session().profile();
            let page = progress_page(&profile);
            let rows = vec![
                pp_view::shell::ShellRow {
                    label: String::new(),
                    detail: String::new(),
                };
                page.badges.len().max(1)
            ];
            self.shell.set_progress(rows, Vec::new());
            if let Some(screens) = self.screens.as_mut() {
                screens.set_progress_page(page);
            }
        }
        let workouts = self.session().workouts();
        let plans = workout_plans(self.settings_path());
        let ftp_w = self.session().settings().ftp_w;
        let chosen = self.session().selected_workout_id().map(str::to_owned);
        let workout_name = chosen
            .as_ref()
            .and_then(|id| workouts.iter().find(|plan| &plan.id == id))
            .map(|plan| plan.name.clone())
            .unwrap_or_else(|| "Free ride".to_owned());
        let mut workout_rows = vec![pp_view::shell::ShellRow {
            label: "Free ride".to_owned(),
            detail: String::new(),
        }];
        let mut workout_ids = vec![None];
        let mut samples = vec![Vec::new()];
        for plan in &workouts {
            workout_rows.push(pp_view::shell::ShellRow {
                label: plan.name.clone(),
                detail: workout_detail(plan, &plans, ftp_w),
            });
            workout_ids.push(Some(plan.id.clone()));
            samples.push(sample_bars(&plan.profile));
        }
        if let Some(screens) = self.screens.as_mut() {
            screens.set_workout_samples(samples);
        }
        self.shell
            .set_workouts(&workout_name, workout_rows, workout_ids);
        let cards = self.course_cards();
        let totals = self.session().summary();
        let upload = matches!(
            self.session().strava_state(),
            pp_session::StravaState::Connected { .. }
        ) && self.ride_file.as_ref().is_some_and(|note| note.saved)
            && !totals.strava_uploaded;
        let ftp = totals
            .workout_result
            .as_ref()
            .and_then(|result| result.ramp_test_ftp_w)
            .filter(|watts| watts.is_finite() && *watts > 0.0)
            .map(|watts| watts.round() as u16);
        self.shell.set_summary_extra(upload, ftp);
        let summary_line = format!(
            "{:.0} s · {:.2} km · {:.0} m up · {:.0} W avg",
            totals.elapsed_s,
            totals.distance_m / 1000.0,
            totals.ascent_m,
            totals.average_power_w
        );
        let reversed = self.session().course_reversed();
        let course_label = self
            .session()
            .course_choice()
            .label_with_direction(reversed);
        self.shell.show_course_reversed(reversed);
        let view = self.shell.view(
            &course_label,
            &trainer,
            &detail,
            &cards,
            &summary_line,
            self.session()
                .heart_rate_name()
                .as_deref()
                .unwrap_or("Not paired"),
        );
        let profiles: Vec<Vec<(f32, f32)>> =
            cards.iter().map(|card| card.profile.clone()).collect();
        let figures = if self.shell.screen() == Screen::Summary {
            summary_figures(&totals)
        } else {
            Vec::new()
        };
        let ride_file = self.ride_file.clone();
        if let Some(screens) = self.screens.as_mut() {
            screens.show(&view, &profiles, &figures);
            let note = (self.shell.screen() == Screen::Summary).then_some(ride_file.as_ref());
            screens.set_ride_file(note.flatten());
        }
    }

    fn course_cards(&mut self) -> Vec<CourseCard> {
        if self.shell.screen() != Screen::Course {
            return Vec::new();
        }
        let reversed = self.session().course_reversed();
        self.session_mut()
            .course_catalog()
            .iter()
            .map(|option| {
                let profile = if reversed {
                    mirrored_profile(&option.profile)
                } else {
                    option.profile.clone()
                };
                let max_grade_pct = if reversed {
                    (-option.min_grade_pct).max(0.0)
                } else {
                    option.max_grade_pct
                };
                CourseCard {
                    name: option.name.clone(),
                    length_m: option.length_m,
                    ascent_m: option.ascent_m as f32,
                    max_grade_pct,
                    profile,
                }
            })
            .collect()
    }

    fn toggle_course_direction(&mut self) {
        let reversed = !self.session().course_reversed();
        let theme = self.session().course_choice().theme_name().to_owned();
        if let Err(err) = self.session_mut().set_course_reversed(reversed) {
            godot_error!("Penguin Pass: {err}");
            return;
        }
        self.rebuild_world(&theme);
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
        self.atmosphere = world_root.try_get_node_as::<crate::world::Atmosphere>("Atmosphere");
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

    fn sync_rider_preset(&mut self) {
        let id = self.session().settings().rider.clone();
        let kit = self.session().settings().kit.clone();
        if let Some(rider) = self.rider.as_mut() {
            let mut rider = rider.bind_mut();
            if rider.preset_id() != id {
                rider.set_preset(&id);
            }
            rider.set_kit(&kit);
        }
    }

    fn sync_zwift(&mut self) {
        if self.pad_preview {
            let state = self.controllers.as_ref().map(|link| link.state());
            self.pad_badge = pad_badge(true, state.as_ref());
            let status = state.as_ref().map(zwift_word).unwrap_or_default();
            if let Some(menu) = self.menu.as_mut() {
                menu.set_controller_status(&status);
            }
            return;
        }
        let enabled = self.session().settings().zwift_controllers;
        let real = self
            .launch
            .as_ref()
            .is_some_and(|launch| launch.mode == crate::launch::TrainerMode::Real);
        let status = if !enabled {
            String::new()
        } else if !real {
            "unavailable".to_owned()
        } else if self.controllers.is_none() {
            match pp_controllers::spawn_ble_controllers() {
                Ok(mut link) => {
                    link.connect_all();
                    let word = zwift_word(&link.state());
                    self.controllers = Some(link);
                    word
                }
                Err(_) => "unavailable".to_owned(),
            }
        } else {
            self.controllers
                .as_ref()
                .map(|link| zwift_word(&link.state()))
                .unwrap_or_default()
        };
        if !enabled || !real {
            if let Some(mut link) = self.controllers.take() {
                link.disconnect();
            }
            self.pad_up = false;
            self.pad_down = false;
        }
        let state = self.controllers.as_ref().map(|link| link.state());
        self.pad_badge = if !enabled {
            crate::hud::PadBadge::Off
        } else if !real {
            crate::hud::PadBadge::Unavailable
        } else {
            pad_badge(true, state.as_ref())
        };
        if let Some(menu) = self.menu.as_mut() {
            menu.set_controller_status(&status);
        }
    }

    fn note_device(&mut self, event: &Gd<InputEvent>) {
        if self.input_locked || event.is_echo() {
            return;
        }
        if event.clone().try_cast::<InputEventKey>().is_ok() {
            self.input_mode = pp_view::prompts::InputMode::Keyboard;
        } else if event.clone().try_cast::<InputEventJoypadButton>().is_ok() {
            self.input_mode = pp_view::prompts::InputMode::Gamepad;
        } else if let Ok(motion) = event.clone().try_cast::<InputEventJoypadMotion>()
            && motion.get_axis_value().abs() >= 0.5
        {
            self.input_mode = pp_view::prompts::InputMode::Gamepad;
        }
    }

    fn push_prompts(&mut self) {
        let play = self.session().play();
        let prompts = pp_view::prompts::ride_prompts(
            self.input_mode,
            play.powerup_held.is_some(),
            play.workout.is_some(),
        );
        if let Some(mut hud) = self.hud.clone() {
            hud.bind_mut().set_prompts(prompts);
        }
        let footer = pp_view::prompts::menu_footer(self.input_mode);
        if let Some(screens) = self.screens.as_mut() {
            screens.set_footer(&footer);
        }
        if let Some(menu) = self.menu.as_mut() {
            menu.set_footer(&footer);
        }
    }

    fn poll_zwift(&mut self) {
        let Some(link) = self.controllers.as_mut() else {
            return;
        };
        let events = link.drain_events();
        if !self.input_locked
            && events
                .iter()
                .any(|event| matches!(event, pp_controllers::ControllerEvent::Pressed { .. }))
        {
            self.input_mode = pp_view::prompts::InputMode::Zwift;
        }
        let riding = self.shell.screen() == Screen::Riding;
        let skip_shift = pp_view::controllers::batch_has_paddle(&events);
        let actions: Vec<_> = events
            .iter()
            .filter_map(|event| {
                if let pp_controllers::ControllerEvent::Connected { name, .. } = event {
                    return Some(format!("{name} connected"));
                }
                if let pp_controllers::ControllerEvent::Disconnected { name, .. } = event {
                    return Some(format!("{name} disconnected"));
                }
                None
            })
            .collect();
        for event in &events {
            let Some(action) = pp_view::controllers::map_event(event, riding, skip_shift) else {
                continue;
            };
            match event {
                pp_controllers::ControllerEvent::Released { button, .. } => {
                    if matches!(
                        button,
                        pp_controllers::Button::Up
                            | pp_controllers::Button::Plus
                            | pp_controllers::Button::ShiftUp
                            | pp_controllers::Button::ShiftUpRight
                            | pp_controllers::Button::ShiftDownRight
                            | pp_controllers::Button::PowerUpRight
                    ) {
                        self.pad_up = false;
                    }
                    if matches!(
                        button,
                        pp_controllers::Button::Down
                            | pp_controllers::Button::Minus
                            | pp_controllers::Button::ShiftDown
                            | pp_controllers::Button::ShiftDownLeft
                            | pp_controllers::Button::ShiftUpLeft
                            | pp_controllers::Button::PowerUpLeft
                    ) {
                        self.pad_down = false;
                    }
                }
                _ => self.apply_pad(action),
            }
        }
        for text in actions {
            if let Some(screens) = self.screens.as_mut() {
                screens.toast(&text, 3.0);
            }
        }
    }

    fn apply_pad(&mut self, action: pp_view::controllers::PadAction) {
        use pp_view::controllers::PadAction;
        match action {
            PadAction::LaneLeft => {
                self.lane.nudge(false);
                self.lane_moved = true;
            }
            PadAction::LaneRight => {
                self.lane.nudge(true);
                self.lane_moved = true;
            }
            PadAction::PowerUp => self.pad_up = true,
            PadAction::PowerDown => self.pad_down = true,
            PadAction::Camera => self.first_person = !self.first_person,
            PadAction::NextTrack => self.skip_track(),
            PadAction::Pause => self.on_menu_key(),
            PadAction::MenuUp => self.shell_command(Command::Up, true),
            PadAction::MenuDown => self.shell_command(Command::Down, true),
            PadAction::MenuLeft => self.shell_command(Command::Left, true),
            PadAction::MenuRight => self.shell_command(Command::Right, true),
            PadAction::Accept => self.shell_command(Command::Accept, true),
            PadAction::Back => self.on_menu_key(),
            PadAction::Activate => self.activate_powerup(),
        }
    }

    fn show_events(&mut self) {
        let events = self.session_mut().drain_events();
        for event in events {
            self.note_sound_event(&event);
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
                pp_session::RideEvent::HeartRateConnected { name } => {
                    (format!("{name} heart rate connected"), 3.0)
                }
                pp_session::RideEvent::HeartRateLost { name } => {
                    (format!("{name} heart rate lost"), 6.0)
                }
                pp_session::RideEvent::RideSaved { path } => {
                    let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
                    self.ride_file = Some(pp_view::rides::saved_note(
                        &path,
                        home.as_deref(),
                        pp_view::rides::FOLDER_CHARS,
                    ));
                    if self.shell.screen() == Screen::Summary {
                        continue;
                    }
                    ("Ride saved".to_owned(), 4.0)
                }
                pp_session::RideEvent::RideNotSaved { reason } => {
                    self.ride_file = Some(pp_view::rides::unsaved_note(&reason));
                    if self.shell.screen() == Screen::Summary {
                        continue;
                    }
                    (reason, 6.0)
                }
                pp_session::RideEvent::SegmentEntered { name } => (name, 2.5),
                pp_session::RideEvent::SegmentFinished {
                    name,
                    time_s,
                    previous_best_s,
                    record,
                } => {
                    let clock = pp_view::arcade::segment_clock(time_s);
                    let line = if record {
                        "New record".to_owned()
                    } else if let Some(best) = previous_best_s {
                        let (text, ahead) = pp_view::arcade::split_text(Some(best - time_s));
                        if ahead {
                            format!("{text}  ahead of your best")
                        } else {
                            format!("{text}  behind your best")
                        }
                    } else {
                        "First finish".to_owned()
                    };
                    if let Some(mut hud) = self.hud.clone() {
                        hud.bind_mut()
                            .show_finish(&format!("{name}  {clock}"), &line);
                    }
                    continue;
                }
                pp_session::RideEvent::FishCollected { .. } => continue,
                pp_session::RideEvent::FishMissed => continue,
                pp_session::RideEvent::PowerupAwarded { kind } => (
                    format!(
                        "{} ready",
                        pp_view::arcade::powerup_name(&format!("{kind:?}"))
                    ),
                    2.0,
                ),
                pp_session::RideEvent::PowerupActivated { kind } => (
                    format!("{} on", pp_view::arcade::powerup_name(&format!("{kind:?}"))),
                    1.5,
                ),
                pp_session::RideEvent::PowerupExpired { kind } => (
                    format!(
                        "{} ended",
                        pp_view::arcade::powerup_name(&format!("{kind:?}"))
                    ),
                    1.5,
                ),
                pp_session::RideEvent::BadgeEarned { name, .. } => (format!("Badge: {name}"), 4.0),
                pp_session::RideEvent::GhostBeaten { .. } => {
                    ("You beat your ghost".to_owned(), 4.0)
                }
                pp_session::RideEvent::WorkoutStepStarted { label, target_w } => {
                    let target = pp_view::extras::workout_target(target_w);
                    (format!("{label}  {target}"), 2.5)
                }
                pp_session::RideEvent::WorkoutStepEndingSoon { seconds } => {
                    (format!("{seconds} s left in this step"), 2.0)
                }
                pp_session::RideEvent::WorkoutCue { text } => (text, 3.0),
                pp_session::RideEvent::WorkoutFinished { ramp_test_ftp_w } => {
                    if let Some(watts) = ramp_test_ftp_w {
                        (format!("Estimated FTP  {watts} W"), 6.0)
                    } else {
                        ("Workout complete".to_owned(), 4.0)
                    }
                }
                pp_session::RideEvent::StravaConnected { athlete } => {
                    (format!("Strava  {athlete}"), 4.0)
                }
                pp_session::RideEvent::StravaConnectFailed { reason } => (reason, 6.0),
                pp_session::RideEvent::StravaUploadStarted => {
                    ("Uploading to Strava".to_owned(), 3.0)
                }
                pp_session::RideEvent::StravaUploaded { .. } => {
                    ("Uploaded to Strava".to_owned(), 4.0)
                }
                pp_session::RideEvent::StravaUploadFailed { reason, duplicate } => {
                    if duplicate {
                        ("Already on Strava".to_owned(), 4.0)
                    } else {
                        (reason, 6.0)
                    }
                }
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
                for _ in 0..5 {
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
        let pressed = if input.is_action_just_pressed("menu_accept") {
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
        if let Some(command) = pressed {
            self.menu_command(command, true);
        } else if let Some(command) = self.held_menu_step(dt) {
            self.menu_command(command, false);
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
        self.apply_sound_settings();
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
        if self.launch.as_ref().is_some_and(|launch| launch.no_music) {
            return;
        }
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
        if self.quit_at_ms > 0 {
            return;
        }
        self.release_music();
        self.quit_at_ms = Time::singleton().get_ticks_msec().saturating_add(400);
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

    fn start_sound(&mut self) {
        let mut sound = crate::sound::SoundPlayer::new_alloc();
        sound.set_name("Sound");
        self.base_mut().add_child(&sound);
        let settings = self.session().settings().clone();
        let capture = self
            .launch
            .as_ref()
            .and_then(|launch| launch.sound_capture.clone());
        let mute = capture.is_none() && self.launch.as_ref().is_some_and(|launch| launch.no_sound);
        {
            let mut player = sound.bind_mut();
            player.set_volumes(
                if mute { 0 } else { settings.ambient_volume },
                if mute { 0 } else { settings.effects_volume },
            );
            if let Some(path) = capture.as_ref() {
                player.arm_capture(&path.to_string_lossy());
            }
        }
        self.sound = Some(sound);
    }

    fn apply_sound_settings(&mut self) {
        if self
            .launch
            .as_ref()
            .is_some_and(|launch| launch.sound_capture.is_some())
        {
            return;
        }
        let settings = self.session().settings().clone();
        if let Some(sound) = self.sound.as_mut() {
            sound
                .bind_mut()
                .set_volumes(settings.ambient_volume, settings.effects_volume);
        }
    }

    fn sync_sound(&mut self, dt: f64) {
        let input = self.ride_sound_input();
        let quit = self
            .sound
            .as_mut()
            .is_some_and(|sound| sound.bind_mut().sync(input, dt as f32, None));
        if let Some(watts) = self
            .sound
            .as_ref()
            .and_then(|sound| sound.bind().demo_power_w())
        {
            let watts = watts.clamp(0.0, 1500.0).round() as u16;
            self.session_mut().set_sim_power(watts);
            self.keys.set_target_power(f32::from(watts));
        }
        if quit {
            self.request_quit();
        }
    }

    fn ride_sound_input(&self) -> pp_view::sound::RideInput {
        let telemetry = self.session().telemetry();
        let settings = self.session().settings();
        let screen = self.shell.screen();
        pp_view::sound::RideInput {
            speed_mps: telemetry.speed_mps,
            cadence_rpm: telemetry.cadence_rpm,
            power_w: telemetry.power_w,
            zone: pp_session::power_zone(telemetry.power_w, settings.ftp_w).index,
            heart_bpm: telemetry.heart_rate_bpm,
            grade_pct: telemetry.grade_pct,
            elevation_m: telemetry.elevation_m,
            theme: if self.theme_name == "desert" {
                pp_view::sound::Ambience::Desert
            } else {
                pp_view::sound::Ambience::Alpine
            },
            riding: matches!(screen, Screen::Riding | Screen::Pause | Screen::ConfirmEnd),
            paused: matches!(
                screen,
                Screen::Pause | Screen::ConfirmEnd | Screen::Settings
            ),
            breathing: settings.rider_breathing,
        }
    }

    fn note_sound_event(&mut self, event: &pp_session::RideEvent) {
        use pp_session::RideEvent;
        use pp_view::sound::SoundEvent;
        let sound = match event {
            RideEvent::FishCollected { golden, combo } => SoundEvent::Fish {
                golden: *golden,
                combo: *combo,
            },
            RideEvent::FishMissed => SoundEvent::ComboBroken,
            RideEvent::PowerupAwarded { .. } => SoundEvent::PowerupAwarded,
            RideEvent::PowerupActivated { .. } => SoundEvent::PowerupActivated,
            RideEvent::PowerupExpired { .. } => SoundEvent::PowerupExpired,
            RideEvent::SegmentEntered { .. } => SoundEvent::SegmentEntered,
            RideEvent::SegmentFinished { record, .. } => {
                SoundEvent::SegmentFinished { record: *record }
            }
            RideEvent::GhostBeaten { .. } => SoundEvent::GhostBeaten,
            RideEvent::BadgeEarned { .. } => SoundEvent::BadgeEarned,
            RideEvent::LapCompleted { .. } => SoundEvent::LapCompleted,
            RideEvent::RideSaved { .. } => SoundEvent::RideSaved,
            _ => return,
        };
        self.push_sound(sound);
    }

    fn note_ui(&mut self, command: Command, fresh: bool) {
        use pp_view::sound::SoundEvent;
        let event = match command {
            Command::Up | Command::Down | Command::Left | Command::Right if fresh => {
                SoundEvent::MenuMove
            }
            Command::Accept => SoundEvent::MenuSelect,
            Command::Back => SoundEvent::MenuBack,
            _ => return,
        };
        self.push_sound(event);
    }

    fn push_sound(&mut self, event: pp_view::sound::SoundEvent) {
        if let Some(sound) = self.sound.as_mut() {
            sound.bind_mut().push_event(event);
        }
    }

    fn menu_command(&mut self, command: Command, fresh: bool) {
        self.note_ui(command, fresh);
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
        if effect == Effect::OpenRides {
            self.open_rides_folder();
        }
        self.apply_music_settings();
    }

    fn activate_powerup(&mut self) {
        if self.shell.screen() != Screen::Riding {
            return;
        }
        self.session_mut().activate_powerup();
    }

    fn open_rides_folder(&mut self) {
        let dir = self.session().rides_dir();
        if let Err(err) = std::fs::create_dir_all(&dir) {
            godot_error!("Penguin Pass: could not create the rides folder: {err}");
            return;
        }
        let path = dir.display().to_string();
        let error = godot::classes::Os::singleton().shell_open(&GString::from(path.as_str()));
        if error != godot::global::Error::OK {
            godot_error!("Penguin Pass: could not open the rides folder");
        }
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
                bars: 0,
                ftms: device.ftms,
                cycling_power: device.cycling_power,
                fec: device.fec,
                remembered: false,
            })
            .collect(),
        music_report: String::new(),
        phase: pp_session::LinkPhase::Idle,
        failure_reason: String::new(),
        connected_name: String::new(),
        controller_status: String::new(),
        straps: Vec::new(),
        heart_line: String::new(),
        riders: Vec::new(),
        kits: Vec::new(),
        strava: crate::menu::strava_row(&session.strava_state()),
    }
}

fn summary_figures(totals: &pp_session::RideSummary) -> Vec<(String, String)> {
    let speed = if totals.elapsed_s > 0.5 {
        format!("{:.1}", totals.distance_m / totals.elapsed_s * 3.6)
    } else {
        "—".to_owned()
    };
    let lap_times: Vec<(u32, f64)> = totals
        .laps
        .iter()
        .map(|lap| (lap.lap, lap.time_s))
        .collect();
    let mut figures = vec![
        (
            "Time".to_owned(),
            pp_view::format::format_clock(totals.elapsed_s),
        ),
        (
            "Distance".to_owned(),
            format!("{:.2} km", totals.distance_m / 1000.0),
        ),
        ("Climbing".to_owned(), format!("{:.0} m", totals.ascent_m)),
        (
            "Average power".to_owned(),
            format!("{:.0} W", totals.average_power_w),
        ),
        (
            "Best power".to_owned(),
            format!("{:.0} W", totals.max_power_w),
        ),
        ("Average speed".to_owned(), format!("{speed} km/h")),
        (
            "Laps".to_owned(),
            pp_view::format::format_summary_laps(&lap_times),
        ),
        ("Fish".to_owned(), totals.fish.to_string()),
        ("Best combo".to_owned(), totals.best_combo.to_string()),
    ];
    if let Some(result) = &totals.workout_result {
        figures.push((
            "Workout".to_owned(),
            pp_view::extras::workout_result_line(result.in_range_fraction, result.average_power_w),
        ));
        if let Some(watts) = result.ramp_test_ftp_w.filter(|watts| watts.is_finite()) {
            figures.push((
                "Estimated FTP".to_owned(),
                format!("{:.0} W", watts.round()),
            ));
        }
    }
    for segment in &totals.segments {
        let mark = if segment.record { "record" } else { "" };
        figures.push((
            segment.name.clone(),
            format!("{}  {mark}", pp_view::arcade::segment_clock(segment.time_s)),
        ));
    }
    figures
}

fn workout_plans(settings: &std::path::Path) -> Vec<pp_workout::Workout> {
    let mut workouts = pp_workout::Library::builtin().workouts;
    if let Some(dir) = settings.parent()
        && let Ok(extra) = pp_workout::Library::load_dir(dir.join("workouts"))
    {
        workouts.extend(extra.workouts);
    }
    workouts
}

fn workout_detail(
    plan: &pp_session::WorkoutSummary,
    plans: &[pp_workout::Workout],
    ftp_w: u16,
) -> String {
    let Some(workout) = plans.iter().find(|workout| workout.id == plan.id) else {
        return pp_view::extras::workout_length(plan.duration_s);
    };
    pp_view::extras::workout_metrics(
        workout.duration_s(),
        workout.tss(),
        workout.intensity_factor(),
        workout.estimated_kj(ftp_w),
    )
}

/// Forward thumbnail, flipped along the lap, matching a reversed course profile.
fn mirrored_profile(profile: &[(f32, f32)]) -> Vec<(f32, f32)> {
    let Some(length) = profile.last().map(|point| point.0) else {
        return Vec::new();
    };
    if profile.len() < 2 || length <= f32::EPSILON {
        return profile.to_vec();
    }
    profile
        .iter()
        .map(|(distance, _)| {
            let wrapped = distance.rem_euclid(length);
            let source = (length - wrapped).rem_euclid(length);
            (*distance, profile_elevation(profile, source))
        })
        .collect()
}

fn profile_elevation(profile: &[(f32, f32)], distance: f32) -> f32 {
    let mut elevation = profile[0].1;
    for window in profile.windows(2) {
        if distance <= window[1].0 {
            let width = (window[1].0 - window[0].0).max(0.001);
            let t = ((distance - window[0].0) / width).clamp(0.0, 1.0);
            return window[0].1 + (window[1].1 - window[0].1) * t;
        }
        elevation = window[1].1;
    }
    elevation
}

fn sample_bars(profile: &[pp_session::WorkoutBar]) -> Vec<f32> {
    if profile.is_empty() {
        return Vec::new();
    }
    let total = profile
        .iter()
        .map(|bar| bar.duration_s.max(0.0))
        .sum::<f32>()
        .max(0.001);
    (0..12)
        .map(|index| {
            let at = total * (index as f32 + 0.5) / 12.0;
            let mut cursor = 0.0;
            let mut value = if profile[0].open {
                -1.0
            } else {
                profile[0].start_ftp
            };
            for bar in profile {
                let next = cursor + bar.duration_s.max(0.0);
                value = if bar.open { -1.0 } else { bar.start_ftp };
                if at <= next {
                    break;
                }
                cursor = next;
            }
            value
        })
        .collect()
}

fn progress_page(profile: &pp_session::ProfileView) -> crate::screens::ProgressPage {
    let streak = if profile.streak_days == 1 {
        "1 day streak".to_owned()
    } else {
        format!("{} day streak", profile.streak_days)
    };
    let totals = format!(
        "{} rides    {:.0} km    {:.0} m up\n{}    {} fish    {streak}",
        profile.rides,
        profile.distance_m / 1000.0,
        profile.ascent_m,
        pp_view::format::format_clock(profile.time_s),
        profile.fish,
    );
    let badges = profile
        .badges
        .iter()
        .map(|badge| {
            let fraction = if badge.earned || badge.target <= 0.0 {
                1.0
            } else {
                (badge.current / badge.target).clamp(0.0, 1.0) as f32
            };
            let kit = badge
                .kit
                .as_deref()
                .map(|id| format!("Unlocks {} kit", pp_rider::Kit::get(id).name))
                .unwrap_or_default();
            crate::screens::ProgressBadge {
                name: badge.name.clone(),
                description: badge.description.clone(),
                earned: badge.earned,
                fraction,
                kit,
            }
        })
        .collect();
    crate::screens::ProgressPage { totals, badges }
}

fn ride_chrome(play: &pp_session::PlayView) -> crate::hud::RideChrome {
    let mut chrome = crate::hud::RideChrome::default();
    if let Some(segment) = play
        .active_segment
        .as_ref()
        .filter(|segment| segment.id != "lap")
    {
        let (split, ahead) = pp_view::arcade::split_text(segment.split_s);
        chrome.segment_name.clone_from(&segment.name);
        chrome.segment_time = pp_view::arcade::segment_clock(segment.elapsed_s);
        chrome.segment_split = split;
        chrome.segment_ahead = ahead;
        chrome.segment_fraction = segment.fraction.clamp(0.0, 1.0);
    }
    if let Some(ghost) = &play.ghost
        && ghost.gap_m.abs() <= 300.0
    {
        chrome.ghost = pp_view::arcade::ghost_chip(ghost.gap_m as f32, ghost.gap_s as f32);
    }
    if let Some(active) = play.powerup_active {
        chrome.power_name = pp_view::arcade::powerup_name(&format!("{:?}", active.kind)).to_owned();
        chrome.power_kind = power_code(active.kind);
        chrome.power_active = true;
        chrome.power_fill = if active.duration_s > 0.05 {
            (active.remaining_s / active.duration_s).clamp(0.0, 1.0)
        } else {
            0.0
        };
    } else if let Some(held) = play.powerup_held {
        chrome.power_name = pp_view::arcade::powerup_name(&format!("{held:?}")).to_owned();
        chrome.power_kind = power_code(held);
    }
    if let Some(text) = pp_view::extras::draft_chip(play.draft_saving_pct) {
        chrome.draft = text;
    }
    if let Some(workout) = &play.workout {
        chrome.workout_title =
            pp_view::extras::workout_title(&workout.label, workout.step_index, workout.step_count);
        chrome.workout_target = pp_view::extras::workout_target(workout.target_w);
        chrome.workout_range = match workout.in_range {
            Some(true) => 1,
            Some(false) => 2,
            None => 3,
        };
        chrome.workout_clock = pp_view::extras::workout_clock(workout.step_remaining_s);
        chrome.workout_next =
            pp_view::extras::next_interval(workout.next_label.as_deref(), workout.next_target_w);
        chrome.workout_bias = pp_view::extras::bias_label(workout.bias_percent);
        chrome.workout_erg = play.workout_erg_w.is_some();
        chrome.workout_cue = workout.cue.clone().unwrap_or_default();
        chrome.workout_bars = play
            .workout_profile
            .iter()
            .map(|bar| if bar.open { -1.0 } else { bar.start_ftp })
            .collect();
        let span = workout.total_elapsed_s + workout.total_remaining_s;
        chrome.workout_cursor = if span > 0.05 {
            (workout.total_elapsed_s / span).clamp(0.0, 1.0)
        } else {
            0.0
        };
    }
    chrome
}

fn place_ghost(
    ghost_rider: &mut Option<Gd<Rider>>,
    ghost_distance_m: &mut f64,
    play: &pp_session::PlayView,
    course: &pp_course::Course,
    slot: u8,
    spec: pp_view::lane::LaneSpec,
    dt: f64,
) {
    let Some(view) = play
        .ghost
        .as_ref()
        .filter(|ghost| ghost.gap_m.abs() <= 300.0)
    else {
        if let Some(node) = ghost_rider.as_mut() {
            node.set_visible(false);
        }
        return;
    };
    if *ghost_distance_m <= 0.0 {
        *ghost_distance_m = view.total_distance_m;
    }
    let speed = if dt > 1.0e-3 {
        ((view.total_distance_m - *ghost_distance_m) / dt).abs() as f32
    } else {
        6.0
    };
    *ghost_distance_m = view.total_distance_m;
    // One slot to the rider's left, and always on the pavement. Slot 0 is
    // the left edge, so the ghost steps inboard. Slot 8 is the gravel
    // shoulder and stays clear for the player.
    let paved_last = pp_view::lane::PAVED_SLOTS.saturating_sub(1);
    let ghost_slot = if slot == 0 {
        1.min(paved_last)
    } else {
        slot.saturating_sub(1).min(paved_last)
    };
    let offset = spec.offset_for_slot(ghost_slot);
    let (position, tangent) = pose(course, view.total_distance_m);
    let origin = position + crate::road::flat_right(tangent) * offset;
    let cadence = if speed < 0.8 {
        75.0
    } else {
        (speed * 11.0).clamp(50.0, 110.0)
    };
    if let Some(node) = ghost_rider.as_mut() {
        node.set_visible(true);
        let mut ghost = node.bind_mut();
        ghost.set_ride_state(160.0, 200.0, 0.0, speed.clamp(2.0, 20.0));
        ghost.place(
            origin,
            tangent,
            view.total_distance_m,
            Some(cadence),
            dt,
            0.0,
        );
    }
}

fn parse_time(id: &str) -> pp_world::TimeOfDay {
    match id {
        "auto" => pp_world::TimeOfDay::Auto,
        "dawn" => pp_world::TimeOfDay::Dawn,
        "sunset" => pp_world::TimeOfDay::Sunset,
        "night" => pp_world::TimeOfDay::Night,
        _ => pp_world::TimeOfDay::Day,
    }
}

fn parse_weather(id: &str) -> pp_world::Weather {
    match id {
        "auto" => pp_world::Weather::Auto,
        "cloudy" => pp_world::Weather::Cloudy,
        "rain" => pp_world::Weather::Rain,
        "snow" => pp_world::Weather::Snow,
        "fog" => pp_world::Weather::Fog,
        _ => pp_world::Weather::Clear,
    }
}

fn power_code(kind: pp_session::PowerupKind) -> u8 {
    match kind {
        pp_session::PowerupKind::Feather => 1,
        pp_session::PowerupKind::Aero => 2,
        pp_session::PowerupKind::Burst => 3,
    }
}

fn pad_badge(on: bool, state: Option<&pp_controllers::LinkState>) -> crate::hud::PadBadge {
    use pp_controllers::LinkState;
    if !on {
        return crate::hud::PadBadge::Off;
    }
    match state {
        Some(LinkState::Connected(count)) if *count > 0 => {
            crate::hud::PadBadge::Count(u8::try_from(*count).unwrap_or(2).min(2))
        }
        Some(LinkState::Unavailable | LinkState::Failed(_)) | None => {
            crate::hud::PadBadge::Unavailable
        }
        _ => crate::hud::PadBadge::Scanning,
    }
}

fn zwift_word(state: &pp_controllers::LinkState) -> String {
    match state {
        pp_controllers::LinkState::Connected(1) => "1 connected".to_owned(),
        pp_controllers::LinkState::Connected(count) => format!("{count} connected"),
        pp_controllers::LinkState::Unavailable | pp_controllers::LinkState::Failed(_) => {
            "unavailable".to_owned()
        }
        _ => "scanning".to_owned(),
    }
}

fn preview_fish(distance_m: f64) -> Vec<crate::fish::FishSpot> {
    (0..3u8)
        .map(|slot| crate::fish::FishSpot {
            index: u32::from(slot),
            distance_m: distance_m + 18.0 + f64::from(slot) * 14.0,
            slot,
            golden: slot == 2,
        })
        .collect()
}

fn pose(course: &Course, lap_distance_m: f64) -> (Vector3, Vector3) {
    let position = course.position_at(lap_distance_m);
    let tangent = course.tangent_at(lap_distance_m);
    (
        Vector3::new(position[0], position[1] + 0.06, position[2]),
        Vector3::new(tangent[0], tangent[1], tangent[2]),
    )
}
