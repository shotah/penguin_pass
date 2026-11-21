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
use crate::rider::Rider;
use crate::sim::{self, SimRider};
use crate::world;

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
        }
    }

    fn ready(&mut self) {
        if self.started {
            return;
        }
        self.started = true;
        let started_ms = Time::singleton().get_ticks_msec();
        input_map::install();
        self.base_mut().set_process_mode(ProcessMode::ALWAYS);
        if let Some(mut window) = self.base().get_window() {
            window.set_title("Penguin Pass");
        }
        Input::singleton().set_mouse_mode(MouseMode::VISIBLE);

        let launch = Launch::from_engine();
        let loaded = Settings::load(&launch.settings_path);
        self.settings_warning = loaded.warning.clone().unwrap_or_default();
        if let Some(warning) = &loaded.warning {
            godot_warn!("Penguin Pass: {warning}");
        }
        let (mut session, hub) = launch::open_session(&launch, loaded.settings);
        session.set_sim_power(150);
        self.keys.set_target_power(150.0);
        session.auto_connect();
        menu::apply_window_mode(session.settings().fullscreen);
        let course = session.course().clone();
        self.hub = hub;
        self.launch = Some(launch);
        self.session = Some(session);
        godot_print!("PENGUIN_PASS_SETTINGS {}", self.settings_path().display());

        let mut world_root = Node3D::new_alloc();
        world_root.set_name("World");
        let (theme, fallback) = crate::theme::load_or_builtin();
        let catalog = crate::assets::Catalog::load();
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

        self.rider = Some(rider);
        self.camera = Some(camera);
        self.hud = Some(hud);
        let mut parent = self.base_mut().clone();
        self.menu = Some(RideMenu::new(&mut parent));

        let telemetry = self.session().telemetry();
        self.present(&telemetry, 0.0);
        let ready_ms = Time::singleton().get_ticks_msec();
        godot_print!("PENGUIN_PASS_READY_MS {}", ready_ms - started_ms);
    }

    fn process(&mut self, delta: f64) {
        // A stalled frame should not fling the bike. Hold-to-repeat uses the same cap.
        let dt = delta.clamp(0.0, 0.1);
        let menu_open = self.menu.as_ref().is_some_and(RideMenu::is_open);
        if menu_open {
            self.poll_menu(dt);
        } else {
            let up = Input::singleton().is_action_pressed("power_up");
            let down = Input::singleton().is_action_pressed("power_down");
            self.keys.poll_keys(dt, up, down);
            let watts = self.keys.target_power_w().round() as u16;
            self.session_mut().set_sim_power(watts);
        }
        let telemetry = self.step_ride(dt);
        self.present(&telemetry, dt);
        if self.menu.as_ref().is_some_and(RideMenu::is_open) {
            let mut menu = self.menu.take().expect("menu");
            menu.refresh(self.session());
            self.menu = Some(menu);
        }
    }

    fn unhandled_input(&mut self, event: Gd<InputEvent>) {
        if event.is_echo() {
            return;
        }
        if event.is_action_pressed("pause_menu") || event.is_action_pressed("menu_back") {
            self.on_menu_key();
        } else if event.is_action_pressed("toggle_camera") {
            if self.menu.as_ref().is_some_and(RideMenu::is_open) {
                return;
            }
            self.first_person = !self.first_person;
            let last = self.last;
            self.present(&last, 0.0);
        } else if event.is_action_pressed("toggle_fullscreen") {
            self.toggle_fullscreen();
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
        let (theme, _) = crate::theme::load_or_builtin();
        let samples = crate::world::terrain::sample_centerline(self.session().course());
        let raw: Vec<[f32; 3]> = samples
            .iter()
            .map(|sample| [sample.position.x, sample.position.y, sample.position.z])
            .collect();
        let field = crate::world::terrain::Field::new(&raw, theme.seed as i32);
        crate::world::scenery::recompute_checksum(&field, &theme).1
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
        let mut menu = self.menu.take().expect("menu");
        menu.open(self.session());
        self.menu = Some(menu);
    }

    /// Close the settings menu.
    #[func]
    pub fn debug_menu_close(&mut self) {
        if let Some(menu) = self.menu.as_mut() {
            menu.close();
        }
        self.save_settings();
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
        self.menu_command(command);
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
        if let Some(hud) = self.hud.as_mut() {
            let mut hud = hud.bind_mut();
            hud.apply_telemetry(telemetry);
            hud.set_status(&status);
            hud.set_tv_url(&tv);
        }
        let distance = telemetry.lap_distance_m;
        let course = self.session().course().clone();
        let (position, tangent) = pose(&course, distance);
        let ahead = course.tangent_at(distance + 8.0);
        let turn = tangent.x * ahead[2] - tangent.z * ahead[0];
        let lean = (turn * telemetry.speed_mps * 0.18).clamp(-0.3, 0.3);
        if let Some(rider) = self.rider.as_mut() {
            rider.bind_mut().place(
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
            camera.bind_mut().follow(transform, dt, first_person);
        }
    }

    fn toggle_fullscreen(&mut self) {
        let mut settings = self.session().settings().clone();
        settings.fullscreen = !settings.fullscreen;
        menu::apply_window_mode(settings.fullscreen);
        self.session_mut().apply_settings(settings);
        self.save_settings();
    }

    fn on_menu_key(&mut self) {
        let open = self.menu.as_ref().is_some_and(RideMenu::is_open);
        if open {
            self.menu_command(Command::Back);
            return;
        }
        let mut menu = self.menu.take().expect("menu");
        menu.open(self.session());
        self.menu = Some(menu);
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

    fn menu_command(&mut self, command: Command) {
        let path = self.settings_path().clone();
        let Some(mut menu) = self.menu.take() else {
            return;
        };
        let effect = menu.handle(command, self.session_mut(), &path);
        self.menu = Some(menu);
        if effect == Effect::Quit {
            self.base().get_tree().quit();
        }
        if effect == Effect::Restarted {
            let telemetry = self.session().telemetry();
            self.present(&telemetry, 0.0);
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

fn pose(course: &Course, lap_distance_m: f64) -> (Vector3, Vector3) {
    let position = course.position_at(lap_distance_m);
    let tangent = course.tangent_at(lap_distance_m);
    (
        Vector3::new(position[0], position[1] + 0.06, position[2]),
        Vector3::new(tangent[0], tangent[1], tangent[2]),
    )
}
