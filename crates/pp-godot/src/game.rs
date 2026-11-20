//! Ride session. Owns the world, the rider, the camera, and the HUD.
//!
//! Each frame reads a trainer sample, integrates the ride, publishes telemetry,
//! then moves the bike and the HUD. The keyboard rider fills the sample until
//! `pp-trainer` exists. The integrator is the flat-road stand-in until
//! `pp-course` exists.

use godot::classes::input::MouseMode;
use godot::classes::node::PhysicsInterpolationMode;
use godot::classes::node::ProcessMode;
use godot::classes::window::Mode as WindowMode;
use godot::classes::{INode3D, Input, InputEvent, Node3D, Time};
use godot::obj::{Base, Gd, WithBaseField};
use godot::prelude::*;
use pp_core::{Telemetry, TrainerSample};

use crate::camera::CameraRig;
use crate::hud::Hud;
use crate::input_map;
use crate::rider::Rider;
use crate::sim::{self, SimRider};
use crate::world;

/// Root of the Penguin Pass scene. `main.tscn` is this one node.
#[derive(GodotClass)]
#[class(base = Node3D)]
pub struct Game {
    base: Base<Node3D>,
    sim: SimRider,
    hud: Option<Gd<Hud>>,
    rider: Option<Gd<Rider>>,
    camera: Option<Gd<CameraRig>>,
    first_person: bool,
    paused: bool,
    last: Telemetry,
    /// Set when the world has been built, so `ready` is idempotent on reload.
    started: bool,
}

#[godot_api]
impl INode3D for Game {
    fn init(base: Base<Node3D>) -> Self {
        Self {
            base,
            sim: SimRider::default(),
            hud: None,
            rider: None,
            camera: None,
            first_person: false,
            paused: false,
            last: Telemetry::default(),
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

        let mut world_root = Node3D::new_alloc();
        world_root.set_name("World");
        world::build_environment(&mut world_root);
        let _road = world::build_placeholder(&mut world_root);
        self.base_mut().add_child(&world_root);

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
            let mut hud = hud.bind_mut();
            hud.set_profile(&sim::placeholder_profile(50.0), sim::LAP_M);
            hud.set_status(sim::STATUS);
            hud.set_tv_url("");
        }

        self.rider = Some(rider);
        self.camera = Some(camera);
        self.hud = Some(hud);

        let sample = self.sim.sample();
        let telemetry = self.sim.telemetry(&sample);
        self.present(&telemetry, 0.0);
        let ready_ms = Time::singleton().get_ticks_msec();
        godot_print!("PENGUIN_PASS_READY_MS {}", ready_ms - started_ms);
    }

    fn process(&mut self, delta: f64) {
        if self.paused {
            return;
        }
        // A stalled frame should not fling the bike. Hold-to-repeat uses the same cap.
        let dt = delta.clamp(0.0, 0.1);
        let up = Input::singleton().is_action_pressed("power_up");
        let down = Input::singleton().is_action_pressed("power_down");
        self.sim.poll_keys(dt, up, down);
        let telemetry = self.step_ride(dt);
        self.present(&telemetry, dt);
    }

    fn unhandled_input(&mut self, event: Gd<InputEvent>) {
        if event.is_echo() {
            return;
        }
        if event.is_action_pressed("toggle_camera") {
            self.first_person = !self.first_person;
            let last = self.last;
            self.present(&last, 0.0);
        } else if event.is_action_pressed("toggle_fullscreen") {
            self.toggle_fullscreen();
        } else if event.is_action_pressed("pause_menu") {
            self.on_pause_menu();
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
        self.sim.distance_m()
    }

    /// Keyboard power target, in watts.
    #[func]
    pub fn power_w(&self) -> f64 {
        f64::from(self.sim.target_power_w())
    }

    /// Game speed, in kilometers per hour.
    #[func]
    pub fn speed_kph(&self) -> f64 {
        f64::from(self.sim.speed_mps()) * 3.6
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
        self.sim.set_target_power(watts as f32);
    }

    /// Step the ride by `seconds` in fixed 1/60 s ticks, without drawing.
    #[func]
    pub fn debug_advance(&mut self, seconds: f64) {
        let mut left = seconds.max(0.0);
        while left > 0.0 {
            let dt = left.min(1.0 / 60.0);
            let telemetry = self.step_ride(dt);
            self.present(&telemetry, 0.0);
            left -= dt;
        }
    }

    /// Park the ride at a distance, power, and elapsed time. Used by screenshots.
    #[func]
    pub fn debug_seek(&mut self, distance_m: f64, power_w: f64, elapsed_s: f64) {
        self.sim.seek(distance_m, elapsed_s, power_w as f32);
        let sample = self.sim.sample();
        let telemetry = self.sim.telemetry(&sample);
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
        let sample = TrainerSample {
            power_w: Some(self.sim.target_power_w().round() as u16),
            cadence_rpm: None,
            speed_kph: None,
            heart_rate_bpm: None,
        };
        let telemetry = self.sim.telemetry(&sample);
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
}

impl Game {
    /// One simulation step: sample, integrate, publish.
    fn step_ride(&mut self, dt: f64) -> Telemetry {
        let sample = self.read_trainer_sample();
        let telemetry = self.integrate_ride(dt, &sample);
        self.publish_telemetry(&telemetry);
        telemetry
    }

    /// Latest trainer reading.
    ///
    /// Plug-in: when `pp-trainer` is linked, call `trainer.latest_sample()`
    /// and fall back to [`SimRider::sample`] when it returns `None`.
    /// Grade commands go out through `trainer.send(TrainerCommand::SetGrade { .. })`.
    fn read_trainer_sample(&mut self) -> TrainerSample {
        self.sim.sample()
    }

    /// Integrate `dt` seconds against `sample`.
    ///
    /// Plug-in: replace the body with `self.ride.step(dt, sample)` once
    /// `pp-course` is a dependency. The returned [`Telemetry`] is unchanged.
    fn integrate_ride(&mut self, dt: f64, sample: &TrainerSample) -> Telemetry {
        self.sim.step(dt, sample)
    }

    /// Hand one snapshot to the TV dashboard.
    ///
    /// Plug-in: `pp-cast` publishes from here. The HUD is updated separately
    /// in [`Self::present`].
    fn publish_telemetry(&mut self, telemetry: &Telemetry) {
        self.last = *telemetry;
    }

    fn present(&mut self, telemetry: &Telemetry, dt: f64) {
        self.last = *telemetry;
        if let Some(hud) = self.hud.as_mut() {
            hud.bind_mut().apply_telemetry(telemetry);
        }
        let (position, tangent) = world::placeholder_pose(telemetry.distance_m);
        if let Some(rider) = self.rider.as_mut() {
            rider
                .bind_mut()
                .place(position, tangent, telemetry.distance_m);
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
        let Some(mut window) = self.base().get_window() else {
            return;
        };
        let mode = window.get_mode();
        if mode == WindowMode::FULLSCREEN || mode == WindowMode::EXCLUSIVE_FULLSCREEN {
            window.set_mode(WindowMode::WINDOWED);
        } else {
            window.set_mode(WindowMode::FULLSCREEN);
        }
    }

    fn on_pause_menu(&mut self) {
        if self.paused {
            self.base().get_tree().quit();
            return;
        }
        self.paused = true;
        self.base().get_tree().set_pause(true);
        if let Some(hud) = self.hud.as_mut() {
            hud.bind_mut().set_status("Paused — Esc or Start to quit");
        }
    }

    fn hud_mut(&mut self, read: impl FnOnce(&mut Hud) -> GString) -> GString {
        match self.hud.as_mut() {
            Some(hud) => read(&mut hud.bind_mut()),
            None => GString::new(),
        }
    }
}
