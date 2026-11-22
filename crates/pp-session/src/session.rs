//! The ride: physics, trainer, and the TV dashboard on one frame tick.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;

use pp_bots::PlayerInput;
use pp_cast::{CastConfig, CastServer, CourseProfile, ProfilePoint as CastPoint};
use pp_core::{Telemetry, TrainerCommand, TrainerSample};
use pp_course::{Course, Ride};
use pp_trainer::{
    ConnectionState, DiscoveredTrainer, HeartRateLink, SessionMode, SimTrainer, Trainer,
};

use crate::connection::{self, ConnectionStatus, LinkPhase, ResistanceMode};
use crate::course_sel::{self, CourseChoice, CourseOption, RidePhase};
use crate::events::{EventQueue, RideEvent};
use crate::group::{ErgSend, Group};
use crate::heart::{self, HeartRateFactory, SensorEntry};
use crate::link::{BleTrainerFactory, TrainerFactory, TrainerLink};
use crate::play::Play;
use crate::readout::{LiveReadout, PowerWindow};
use crate::settings::{LoadedSettings, Settings};
use crate::stats::RideStats;
use crate::trace::{RideTrace, SampleStamp, TraceTick};

/// Smallest Zwift virtual gear sent over trainer service `0xFC82`.
pub const MIN_VIRTUAL_GEAR: u8 = 1;
/// Largest Zwift virtual gear sent over trainer service `0xFC82`.
pub const MAX_VIRTUAL_GEAR: u8 = 24;
/// Default virtual gear at ride start.
pub const DEFAULT_VIRTUAL_GEAR: u8 = 12;

/// Keyboard power change, in watts, applied by [`Session::nudge_sim_power`].
pub const SIM_POWER_STEP_W: u16 = 25;

/// Highest simulated target power, in watts.
pub const SIM_POWER_MAX_W: u16 = 1500;

/// Cadence reported by the simulated rider while power is above 0 W, in revolutions per minute.
///
/// Coasting (0 W) reports 0 rpm.
pub const SIM_PEDAL_CADENCE_RPM: f32 = 85.0;

/// Spacing of the elevation chart sent to the TV dashboard, in meters.
pub const DASHBOARD_PROFILE_STEP_M: f64 = 20.0;

/// How long a trainer stays on the pairing list after its last advertisement, in seconds.
///
/// One missed packet does not remove the row. Eight seconds is several
/// advertisement intervals for a trainer that broadcasts about once a second.
pub const PAIRING_HOLD_S: f64 = 8.0;

/// Where the TV dashboard is in its background start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DashboardRun {
    /// The dashboard is switched off.
    Off,
    /// A bind is in progress on a background thread. URLs are not ready yet.
    Starting,
    /// The dashboard is listening.
    Running,
    /// The last start failed. [`SessionStatus::dashboard_error`] has the reason.
    Failed,
}

/// Title-card facts for the course the session is riding.
#[derive(Debug, Clone, PartialEq)]
pub struct CourseFacts {
    /// Course name.
    pub name: String,
    /// Lap length, in meters. On a route that is not a loop this is the route length.
    pub length_m: f64,
    /// Total ascent of one lap, in meters.
    pub total_ascent_m: f32,
    /// Steepest uphill grade, in percent.
    pub steepest_up_pct: f32,
    /// Steepest downhill grade, in percent. Negative is downhill.
    pub steepest_down_pct: f32,
    /// `true` when the course returns to its start line.
    ///
    /// This session only rides closed loops, so this is `true`.
    pub is_loop: bool,
}

/// One sample of the road ahead of the rider.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AheadPoint {
    /// Distance ahead of the rider, in meters.
    pub ahead_m: f64,
    /// Distance from the start line, in meters, wrapped onto the lap.
    pub distance_m: f64,
    /// Elevation above the course datum, in meters.
    pub elevation_m: f32,
    /// Grade, in percent. Positive is uphill.
    pub grade_pct: f32,
}

/// One row of the pairing list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairingEntry {
    /// Bluetooth address or the id the trainer reported.
    pub id: String,
    /// Advertised name.
    pub name: String,
    /// Received signal strength, in dBm, when the adapter reported it.
    pub rssi_dbm: Option<i16>,
    /// Signal bars, from 0 to 4. See [`crate::signal_bars`].
    pub bars: u8,
    /// `true` when this row matches [`Settings::last_trainer_id`](crate::Settings::last_trainer_id).
    pub remembered: bool,
    /// Advertises the Fitness Machine Service.
    pub ftms: bool,
    /// Advertises the Cycling Power Service.
    pub cycling_power: bool,
    /// Advertises the Tacx FE-C service.
    pub fec: bool,
}

struct SeenTrainer {
    id: String,
    name: String,
    rssi_dbm: Option<i16>,
    last_seen_s: f64,
    ftms: bool,
    cycling_power: bool,
    fec: bool,
}

struct Observation {
    physics: TrainerSample,
    power_w: Option<f32>,
    cadence_rpm: Option<f32>,
    heart_rate_bpm: Option<u8>,
}

/// Who is supplying power for the ride.
#[derive(Debug, Clone, PartialEq)]
pub enum ActiveSource {
    /// Keyboard or gamepad target on [`SimTrainer`].
    Simulated,
    /// A Bluetooth trainer, including one that is retrying after a drop.
    Trainer {
        /// Name advertised by the trainer, or the id used to connect.
        name: String,
        /// Link lifecycle. [`ConnectionState::Disconnected`] and
        /// [`ConnectionState::Failed`] stay on this source so the keyboard
        /// target cannot push the bike while the rider is stopped.
        state: ConnectionState,
    },
}

/// Plain snapshot for the HUD.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionStatus {
    /// Trainer that currently feeds [`Session::tick`].
    pub source: ActiveSource,
    /// One line for the HUD, such as `Simulated rider` or
    /// `Tacx Neo 2T: connected, resistance follows the road`.
    ///
    /// A trainer warning from [`Trainer::last_warning`] is appended in
    /// parentheses when one is present.
    pub line: String,
    /// Dashboard URLs. Empty when the dashboard is off or failed to bind.
    pub dashboard_urls: Vec<String>,
    /// Why the dashboard is not running, when it was requested and failed.
    ///
    /// `None` when the dashboard is running, starting, or switched off.
    pub dashboard_error: Option<String>,
    /// Whether the dashboard is off, starting, running, or failed.
    pub dashboard: DashboardRun,
}

/// Injected Bluetooth factory and dashboard bind, for tests and the game.
///
/// [`SessionOptions::default`] is the real trainer and an all-interfaces
/// bind. Tests pass a fake factory and a loopback port so they never touch
/// Bluetooth or a fixed port.
pub struct SessionOptions {
    /// Called from [`Session::start_scan`] the first time a trainer is needed.
    pub trainer_factory: Box<dyn TrainerFactory>,
    /// Interface the dashboard binds. The game uses an unspecified address
    /// so a TV on the LAN can open the page. Tests use loopback.
    pub dashboard_host: IpAddr,
    /// Port override. `None` uses [`Settings::tv_dashboard_port`].
    ///
    /// `Some(0)` asks the operating system for a free port each time the
    /// dashboard starts. A fixed override ignores later port edits in
    /// [`Settings`] until the session is rebuilt.
    pub dashboard_port: Option<u16>,
}

impl Default for SessionOptions {
    fn default() -> Self {
        Self {
            trainer_factory: Box::new(BleTrainerFactory),
            dashboard_host: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            dashboard_port: None,
        }
    }
}

/// One ride: course physics, the active trainer, settings, and the dashboard.
///
/// The simulated rider is active until a Bluetooth trainer reaches
/// [`ConnectionState::Connected`]. Dropping the session joins the short-lived
/// threads that dispose of a trainer or a dashboard. That join can take a few
/// seconds and belongs on shutdown, not in the frame loop.
pub struct Session {
    settings: Settings,
    ride: Ride,
    sim: SimTrainer,
    sim_power_w: u16,
    trainer: Option<Box<dyn TrainerLink>>,
    factory: Box<dyn TrainerFactory>,
    want_trainer: bool,
    using_trainer: bool,
    saw_connected: bool,
    requested_id: Option<String>,
    auto_id: Option<String>,
    spawn_error: Option<String>,
    connect_error: Option<String>,
    switching_mode: bool,
    dash: Mutex<DashState>,
    opener: Arc<dyn DashboardOpener>,
    dashboard_host: IpAddr,
    dashboard_port_override: Option<u16>,
    drop_threads: Vec<JoinHandle<()>>,
    clock_s: f64,
    power: PowerWindow,
    stats: RideStats,
    events: EventQueue,
    seen: Vec<SeenTrainer>,
    ride_ended: bool,
    steepest_up_pct: f32,
    steepest_down_pct: f32,
    link_lost: bool,
    control_notified: bool,
    stored_phase: LinkPhase,
    last_dashboard_error: Option<String>,
    phase: RidePhase,
    choice: CourseChoice,
    catalog: Option<Vec<CourseOption>>,
    trace: RideTrace,
    last_grade_sent: Option<f32>,
    traced_speed_mps: f32,
    heart_factory: Option<Box<dyn HeartRateFactory>>,
    heart: Option<Box<dyn HeartRateLink>>,
    heart_error: Option<String>,
    heart_was_connected: bool,
    recorder: Option<pp_record::Recorder>,
    rides_dir_override: Option<PathBuf>,
    settings_path: Option<PathBuf>,
    last_saved_ride: Option<PathBuf>,
    utc_offset_s: i32,
    play_state: Play,
    group: Group,
    virtual_gear: u8,
}

impl Session {
    /// Start in [`RidePhase::Riding`] on `settings.course` with the simulated rider.
    ///
    /// The default course is Penguin Pass. A saved course that cannot be
    /// generated falls back to Penguin Pass and queues
    /// [`RideEvent::CourseUnavailable`]. When the dashboard is enabled this
    /// binds it. A bind failure is stored on [`SessionStatus::dashboard_error`]
    /// and does not fail construction. Bluetooth is not opened until
    /// [`Session::start_scan`] or [`Session::connect`].
    #[must_use]
    pub fn new(settings: Settings) -> Self {
        Self::with_options(settings, SessionOptions::default())
    }

    /// [`Session::new`] with a caller-supplied trainer factory and dashboard bind.
    #[must_use]
    pub fn with_options(settings: Settings, options: SessionOptions) -> Self {
        course_sel::warm_catalog();
        let mut settings = settings.normalized();
        let (course, choice, fallback) =
            course_sel::open_course(&settings.course, settings.course_reversed);
        settings.course = choice.clone();
        let mut sim = SimTrainer::new("Simulated rider");
        sim.set_power(0);
        sim.set_cadence(0.0);
        let ride = Ride::new(course, settings.rider_profile());
        let (steepest_up_pct, steepest_down_pct) = grade_extremes(ride.course());
        let mut session = Self {
            settings,
            ride,
            sim,
            sim_power_w: 0,
            trainer: None,
            factory: options.trainer_factory,
            want_trainer: false,
            using_trainer: false,
            saw_connected: false,
            requested_id: None,
            auto_id: None,
            spawn_error: None,
            connect_error: None,
            switching_mode: false,
            dash: Mutex::new(DashState::default()),
            opener: Arc::new(CastOpener),
            dashboard_host: options.dashboard_host,
            dashboard_port_override: options.dashboard_port,
            drop_threads: Vec::new(),
            clock_s: 0.0,
            power: PowerWindow::new(),
            stats: RideStats::new(),
            events: EventQueue::default(),
            seen: Vec::new(),
            ride_ended: false,
            steepest_up_pct,
            steepest_down_pct,
            link_lost: false,
            control_notified: false,
            stored_phase: LinkPhase::Idle,
            last_dashboard_error: None,
            phase: RidePhase::Riding,
            choice,
            catalog: None,
            trace: RideTrace::from_env(),
            last_grade_sent: None,
            traced_speed_mps: 0.0,
            heart_factory: None,
            heart: None,
            heart_error: None,
            heart_was_connected: false,
            recorder: None,
            rides_dir_override: None,
            settings_path: None,
            last_saved_ride: None,
            utc_offset_s: 0,
            play_state: Play::new(),
            group: Group::new(),
            virtual_gear: DEFAULT_VIRTUAL_GEAR,
        };
        if let Some(detail) = fallback {
            session.events.push(RideEvent::CourseUnavailable { detail });
        }
        session.start_dashboard();
        session.attach_files();
        session.start_recording();
        session
    }

    /// Directory FIT ride files are written to.
    ///
    /// [`Settings::rides_folder`] wins. Otherwise this is a `rides` folder
    /// next to the settings file passed to [`Self::set_settings_path`].
    /// [`Self::set_rides_dir`] replaces both. With none of those set, the
    /// returned path is `rides` and nothing is written.
    #[must_use]
    pub fn rides_dir(&self) -> PathBuf {
        self.resolved_rides_dir()
            .unwrap_or_else(|| PathBuf::from("rides"))
    }

    /// Path of the last ride file this session wrote.
    #[must_use]
    pub fn last_saved_ride(&self) -> Option<PathBuf> {
        self.last_saved_ride.clone()
    }

    /// Write rides to `dir`, or stop overriding the settings folder when `None`.
    ///
    /// Tests pass a temporary directory. A missing directory is created on
    /// the first save. Without this, without [`Settings::rides_folder`], and
    /// without [`Self::set_settings_path`], ending a ride writes nothing.
    pub fn set_rides_dir(&mut self, dir: Option<PathBuf>) {
        self.rides_dir_override = dir;
        self.attach_files();
    }

    /// Local time minus UTC, in minutes, for ride file names started afterwards.
    ///
    /// `0` until this is called. The FIT timestamps stay UTC. A later call
    /// does not rename a ride that is already being recorded.
    pub fn set_utc_offset_minutes(&mut self, minutes: i32) {
        self.utc_offset_s = minutes.saturating_mul(60);
    }

    /// Remember the settings file so rides default to `<its folder>/rides`.
    pub fn set_settings_path(&mut self, path: impl Into<PathBuf>) {
        self.settings_path = Some(path.into());
        self.attach_files();
    }

    /// Write each later tick to `dir/ride.csv`, or stop when `dir` is `None`.
    ///
    /// Construction opens the same file when `PENGUIN_PASS_TRACE_DIR` names a
    /// directory. That variable is read only then. Tests pass the directory
    /// here. A missing directory is created. If the file cannot be opened,
    /// one warning is printed and the ride continues.
    pub fn set_trace_dir(&mut self, dir: Option<PathBuf>) {
        self.trace.flush();
        self.trace = match dir {
            Some(dir) => RideTrace::open(&dir),
            None => RideTrace::off(),
        };
    }

    /// Install the heart-rate strap link, or drop it when `factory` is `None`.
    ///
    /// Nothing is spawned until this is called. A remembered
    /// [`Settings::heart_rate_id`] is connected immediately. Straps that are
    /// only nearby are listed and left alone until [`Self::connect_heart_rate`].
    /// The trainer link is not touched.
    pub fn set_heart_rate_factory(&mut self, factory: Option<Box<dyn HeartRateFactory>>) {
        self.disconnect_heart();
        self.heart_factory = factory;
        self.open_heart();
    }

    /// Straps seen by the strap link, strongest first.
    ///
    /// Empty when no strap link is installed. `remembered` matches
    /// [`Settings::heart_rate_id`] or [`Settings::heart_rate_name`].
    /// [`crate::signal_bars`] is the same rule as the trainer pairing list.
    #[must_use]
    pub fn heart_rate_entries(&self) -> Vec<SensorEntry> {
        let Some(link) = self.heart.as_ref() else {
            return Vec::new();
        };
        let saved_id = self.settings.heart_rate_id.as_deref();
        let saved_name = self.settings.heart_rate_name.as_deref();
        let mut entries: Vec<SensorEntry> = link
            .discovered()
            .iter()
            .map(|sensor| heart::entry(sensor, saved_id, saved_name))
            .collect();
        entries.sort_by(|left, right| {
            right
                .bars
                .cmp(&left.bars)
                .then_with(|| rssi_rank(right.rssi_dbm).cmp(&rssi_rank(left.rssi_dbm)))
                .then_with(|| left.name.cmp(&right.name))
                .then_with(|| left.id.cmp(&right.id))
        });
        entries
    }

    /// Connect the strap link to `id` and remember it.
    ///
    /// `id` is a Bluetooth address or a name fragment. Does nothing when no
    /// strap link is installed. The trainer is left as it is.
    pub fn connect_heart_rate(&mut self, id: &str) {
        let query = id.trim();
        if query.is_empty() || self.heart.is_none() {
            return;
        }
        let matched = self.heart.as_ref().and_then(|link| {
            link.discovered().into_iter().find(|sensor| {
                sensor.id.eq_ignore_ascii_case(query)
                    || sensor
                        .name
                        .to_ascii_lowercase()
                        .contains(&query.to_ascii_lowercase())
            })
        });
        if self.heart_was_connected {
            let name = self
                .heart
                .as_ref()
                .and_then(|link| link.name())
                .or_else(|| self.settings.heart_rate_name.clone())
                .unwrap_or_else(|| "Heart rate".to_owned());
            self.events.push(RideEvent::HeartRateLost { name });
            self.heart_was_connected = false;
        }
        if let Some(sensor) = matched {
            self.settings.heart_rate_id = Some(sensor.id);
            self.settings.heart_rate_name = Some(sensor.name);
        } else {
            self.settings.heart_rate_id = Some(query.to_owned());
        }
        if let Some(link) = self.heart.as_mut() {
            link.connect(query);
        }
        self.note_heart_events();
    }

    /// Forget the remembered strap and disconnect it.
    ///
    /// Later launches do not reconnect. The trainer link stays up.
    pub fn forget_heart_rate(&mut self) {
        self.settings.heart_rate_id = None;
        self.settings.heart_rate_name = None;
        if let Some(link) = self.heart.as_mut() {
            link.disconnect();
        }
        self.note_heart_events();
    }

    /// Lifecycle of the strap link.
    ///
    /// [`ConnectionState::Disconnected`] when no link was installed.
    #[must_use]
    pub fn heart_rate_state(&self) -> ConnectionState {
        if let Some(link) = self.heart.as_ref() {
            return link.state();
        }
        if let Some(err) = &self.heart_error {
            return ConnectionState::Failed(err.clone());
        }
        ConnectionState::Disconnected
    }

    /// Name of the connected strap, or the remembered name when the link is down.
    #[must_use]
    pub fn heart_rate_name(&self) -> Option<String> {
        self.heart
            .as_ref()
            .and_then(|link| link.name())
            .or_else(|| self.settings.heart_rate_name.clone())
    }

    /// Advance the ride by `dt_s` seconds and return the new telemetry.
    ///
    /// A zero, negative, or non-finite `dt_s` does not move the bike. The
    /// rest of the tick still runs: a trainer that has just connected becomes
    /// the active source, a grade is sent when the link is up and
    /// [`Settings::control_trainer`] is set, and telemetry is published when
    /// the dashboard is already listening. The call does not sleep, join, or
    /// wait on the network. A dashboard that is still starting receives nothing.
    ///
    /// Power comes from the active source. A real trainer with no fresh
    /// sample, or a link that is not [`ConnectionState::Connected`], counts
    /// as 0 W. The last wattage is not reused.
    pub fn tick(&mut self, dt_s: f64) -> Telemetry {
        self.drain_strava();
        self.reap_drops();
        self.poll_dashboard();
        self.note_heart_events();
        if self.phase == RidePhase::Ended {
            let frame = self.ride.telemetry();
            self.publish_dashboard(frame);
            self.note_dashboard_event();
            return frame;
        }
        if self.phase == RidePhase::Ready {
            return self.tick_ready(dt_s);
        }
        self.poll_auto();
        self.observe_link();
        let dt = accepted_dt(dt_s);
        self.clock_s += dt;
        let observed = self.observe_power();
        let telemetry = self.step_physics(dt_s, &observed.physics);
        let power_w = observed.power_w.unwrap_or(0.0);
        self.group
            .tick_bots(dt as f32, &self.player_input(&telemetry, power_w));
        let (workout_events, erg) =
            self.group
                .tick_workout(dt as f32, power_w, observed.cadence_rpm);
        for event in workout_events {
            self.events.push(event);
        }
        self.apply_erg(erg);
        self.power.record(self.clock_s, dt, observed.power_w);
        if let Some(event) = self.stats.record(
            dt,
            &telemetry,
            observed.power_w,
            observed.cadence_rpm,
            observed.heart_rate_bpm,
            self.power.average_1s_full(self.clock_s),
        ) {
            self.events.push(event);
        }
        for event in self
            .play_state
            .after_step(&telemetry, dt, self.settings.arcade)
        {
            self.events.push(event);
        }
        self.record_sample(&telemetry);
        self.note_link_events();
        self.refresh_pairing();
        self.send_grade(telemetry.grade_pct);
        self.trace_tick(dt, &observed, telemetry.speed_mps, &telemetry);
        self.publish_dashboard(telemetry);
        self.note_dashboard_event();
        telemetry
    }

    fn tick_ready(&mut self, dt_s: f64) -> Telemetry {
        self.poll_auto();
        self.observe_link();
        let dt = accepted_dt(dt_s);
        self.clock_s += dt;
        let observed = self.observe_power();
        self.power.record(self.clock_s, dt, observed.power_w);
        self.note_link_events();
        self.refresh_pairing();
        self.send_grade(0.0);
        let motion = self.ride.telemetry();
        self.trace_tick(dt, &observed, motion.speed_mps, &motion);
        let mut frame = self.ride.telemetry();
        if let Some(power) = observed.power_w {
            frame.power_w = power;
        }
        frame.cadence_rpm = observed.cadence_rpm;
        frame.heart_rate_bpm = observed.heart_rate_bpm;
        self.publish_dashboard(frame);
        self.note_dashboard_event();
        frame
    }

    /// Add `steps` times [`SIM_POWER_STEP_W`] watts to the simulated target.
    ///
    /// Negative `steps` ease off. The result is clamped to `0..=`[`SIM_POWER_MAX_W`].
    /// Cadence is 0 rpm at 0 W and [`SIM_PEDAL_CADENCE_RPM`] otherwise.
    /// Ignored while a real trainer is the active source, including while
    /// that trainer's link is down.
    pub fn nudge_sim_power(&mut self, steps: i32) {
        if self.using_trainer {
            return;
        }
        let delta = steps.saturating_mul(i32::from(SIM_POWER_STEP_W));
        let next = i32::from(self.sim_power_w).saturating_add(delta);
        let clamped = next.clamp(0, i32::from(SIM_POWER_MAX_W));
        self.sim_power_w = u16::try_from(clamped).unwrap_or(SIM_POWER_MAX_W);
        self.apply_sim_power();
    }

    /// Set the simulated target power, in watts, clamped to `0..=`[`SIM_POWER_MAX_W`].
    ///
    /// Cadence follows the same rule as [`Session::nudge_sim_power`]. Ignored
    /// while a real trainer is the active source, including while that
    /// trainer's link is down.
    pub fn set_sim_power(&mut self, watts: u16) {
        if self.using_trainer {
            return;
        }
        self.sim_power_w = watts.min(SIM_POWER_MAX_W);
        self.apply_sim_power();
    }

    /// Simulated target power, in watts. Unchanged while a real trainer is active.
    #[must_use]
    pub fn sim_power_w(&self) -> u16 {
        self.sim_power_w
    }

    /// Current Zwift virtual gear (`1`..=`24`).
    #[must_use]
    pub fn virtual_gear(&self) -> u8 {
        self.virtual_gear
    }

    /// True when trainer control is on and the link reports Zwift `0xFC82` gear
    /// writes.
    #[must_use]
    pub fn virtual_shifting_available(&self) -> bool {
        self.settings.control_trainer
            && self
                .trainer
                .as_ref()
                .is_some_and(|trainer| trainer.supports_virtual_shifting())
    }

    /// Shift virtual gear by `delta` when [`Self::virtual_shifting_available`].
    ///
    /// Returns the new gear after clamping, or `None` when shifting is not
    /// available.
    pub fn shift_virtual_gear(&mut self, delta: i8) -> Option<u8> {
        if !self.virtual_shifting_available() {
            return None;
        }
        let next = (self.virtual_gear as i16 + i16::from(delta))
            .clamp(i16::from(MIN_VIRTUAL_GEAR), i16::from(MAX_VIRTUAL_GEAR))
            as u8;
        self.virtual_gear = next;
        if let Some(trainer) = self.trainer.as_mut() {
            let _ = trainer.send(TrainerCommand::SetVirtualGear { gear: next });
        }
        Some(next)
    }

    /// Start scanning, creating the Bluetooth trainer if it does not exist yet.
    ///
    /// A second call while the trainer object is alive does nothing. If the
    /// factory cannot start Bluetooth, the failure is stored on
    /// [`SessionStatus::line`] and the simulated rider stays active.
    pub fn start_scan(&mut self) {
        if self.trainer.is_some() {
            return;
        }
        self.connect_error = None;
        match self.factory.spawn_configured(
            self.trainer_mode(),
            Some(self.settings.rider_mass_kg),
            Some(self.settings.bike_mass_kg),
        ) {
            Ok(trainer) => {
                self.trainer = Some(trainer);
                self.spawn_error = None;
            }
            Err(err) => {
                self.spawn_error = Some(err);
            }
        }
    }

    /// Trainers seen by the active scan. Empty when no scan is running.
    #[must_use]
    pub fn discovered(&self) -> Vec<DiscoveredTrainer> {
        self.trainer
            .as_ref()
            .map(|trainer| trainer.discovered())
            .unwrap_or_default()
    }

    /// Queue a connection to `id_or_name` and scan first if needed.
    ///
    /// The trainer becomes the active source on a later [`Session::tick`]
    /// once its state is [`ConnectionState::Connected`]. `last_trainer_id`
    /// is updated at that moment, not when this method returns. An empty id
    /// or a Bluetooth failure is reported on [`SessionStatus`] and does not
    /// panic.
    pub fn connect(&mut self, id_or_name: &str) {
        let id = id_or_name.trim();
        if id.is_empty() {
            self.connect_error = Some("trainer id is empty".to_owned());
            return;
        }
        self.auto_id = None;
        self.start_scan();
        if self.trainer.is_none() {
            return;
        }
        self.requested_id = Some(id.to_owned());
        let Some(trainer) = self.trainer.as_ref() else {
            return;
        };
        match trainer.connect(id) {
            Ok(()) => {
                self.connect_error = None;
                self.want_trainer = true;
                self.observe_link();
            }
            Err(err) => {
                self.connect_error = Some(err.to_string());
            }
        }
    }

    /// Leave the real trainer and return to the simulated rider.
    ///
    /// The trainer object is dropped on a `pp-session-drop` thread. Dropping
    /// a [`BleTrainer`](pp_trainer::BleTrainer) waits for its radio thread,
    /// which can take about three seconds; doing that here would stall the
    /// frame. The helper exits when the drop finishes. [`Session`]'s
    /// destructor joins any helper that is still running, so the thread is
    /// not detached. `last_trainer_id` is left as it is.
    pub fn disconnect(&mut self) {
        self.want_trainer = false;
        self.using_trainer = false;
        self.saw_connected = false;
        self.requested_id = None;
        self.auto_id = None;
        self.connect_error = None;
        self.switching_mode = false;
        self.link_lost = false;
        self.stored_phase = LinkPhase::Idle;
        self.control_notified = false;
        if let Some(trainer) = self.trainer.take() {
            self.defer_drop(trainer);
        }
    }

    /// Scan and connect to [`Settings::last_trainer_id`] when that trainer appears.
    ///
    /// Does nothing when the id is missing or blank. The connect is queued
    /// from this call when the trainer is already in [`Session::discovered`],
    /// and otherwise from a later [`Session::tick`]. Matching is the same
    /// rule the Bluetooth trainer uses: the address, or a case-insensitive
    /// name fragment.
    pub fn auto_connect(&mut self) {
        let Some(id) = self
            .settings
            .last_trainer_id
            .clone()
            .filter(|id| !id.trim().is_empty())
        else {
            return;
        };
        self.auto_id = Some(id);
        self.start_scan();
        self.poll_auto();
    }

    /// HUD snapshot. Does not change the ride.
    #[must_use]
    pub fn status(&self) -> SessionStatus {
        self.poll_dashboard();
        let (dashboard_urls, dashboard_error, dashboard) = {
            let dash = lock_dash(&self.dash);
            let dashboard_urls = dash
                .server
                .as_ref()
                .map(|server| server.urls())
                .unwrap_or_default();
            (dashboard_urls, dash.error.clone(), dash.phase)
        };
        SessionStatus {
            source: self.active_source(),
            line: self.status_line(),
            dashboard_urls,
            dashboard_error,
            dashboard,
        }
    }

    /// Replace settings.
    ///
    /// Trainer difficulty applies on the next [`Session::tick`]. Rider mass,
    /// bike mass, and FTP apply immediately, including mid-ride: speed,
    /// distance, and elapsed time stay put. [`Session::rider`] matches
    /// [`Session::settings`] after this returns. Changing
    /// [`Settings::control_trainer`] while a trainer is open respawns it in
    /// the new mode and, if a link was requested, reconnects. The status line
    /// says so. The dashboard is started, stopped, or rebound on a background
    /// thread when the enabled flag or the effective port changes. This call
    /// does not wait for that thread.
    pub fn apply_settings(&mut self, new: Settings) {
        let new = new.normalized();
        let old_endpoint = self.dashboard_endpoint();
        let rider_changed = new.rider_profile() != self.settings.rider_profile();
        let control_changed = new.control_trainer != self.settings.control_trainer;
        let reversed_changed = new.course_reversed != self.settings.course_reversed;
        let previous_reversed = self.settings.course_reversed;
        let requested = new.course.clone();
        let choice = self.choice.clone();
        self.settings = new;
        self.settings.course = choice.clone();
        if requested != choice {
            if let Err(detail) = self.set_course(requested) {
                self.settings.course_reversed = previous_reversed;
                self.events.push(RideEvent::CourseUnavailable { detail });
                if rider_changed {
                    self.ride.set_rider(self.settings.rider_profile());
                }
            }
        } else if reversed_changed {
            if let Err(detail) = self.reload_course_direction() {
                self.settings.course_reversed = previous_reversed;
                self.events.push(RideEvent::CourseUnavailable { detail });
                if rider_changed {
                    self.ride.set_rider(self.settings.rider_profile());
                }
            }
        } else if rider_changed {
            self.ride.set_rider(self.settings.rider_profile());
        }
        if control_changed {
            self.reopen_trainer_for_mode();
        }
        if self.dashboard_endpoint() != old_endpoint {
            self.restart_dashboard();
        }
    }

    /// Settings currently stored, already normalized.
    #[must_use]
    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// Latest telemetry, without advancing the ride.
    #[must_use]
    pub fn telemetry(&self) -> Telemetry {
        self.ride.telemetry()
    }

    /// Course the session is riding, for the 3D world and the HUD profile.
    #[must_use]
    pub fn course(&self) -> &Course {
        self.ride.course()
    }

    /// Title, riding, or summary.
    ///
    /// A new session is [`RidePhase::Riding`]. [`Self::hold_at_start`] waits
    /// at the title. [`Self::ride_ended`] is true only in [`RidePhase::Ended`].
    #[must_use]
    pub fn phase(&self) -> RidePhase {
        self.phase
    }

    /// Zero the ride and wait at the title.
    ///
    /// Distance, the ride clock, laps, and the summary go back to zero.
    /// The phase becomes [`RidePhase::Ready`]. The trainer connection and
    /// the simulated power target stay. The next ticks keep trainer I/O,
    /// scanning, pairing, the live readout, and the TV dashboard running,
    /// and they send grade 0 instead of the road grade.
    pub fn hold_at_start(&mut self) {
        self.phase = RidePhase::Ready;
        self.ride_ended = false;
        self.stats.clear();
        self.power.clear();
        self.rebuild_ride();
        self.recorder = None;
    }

    /// Leave [`RidePhase::Ready`] and start the ride clock from zero.
    ///
    /// Does nothing when the phase is [`RidePhase::Riding`] or
    /// [`RidePhase::Ended`].
    pub fn begin_ride(&mut self) {
        if self.phase != RidePhase::Ready {
            return;
        }
        self.phase = RidePhase::Riding;
        self.ride_ended = false;
        self.start_recording();
    }

    /// Course currently loaded.
    #[must_use]
    pub fn course_choice(&self) -> &CourseChoice {
        &self.choice
    }

    /// The proof-of-concept course list.
    ///
    /// Computed once per process and cached on this session. The first call
    /// waits if the background build started in [`Self::new`] is still running.
    #[must_use]
    pub fn course_catalog(&mut self) -> &[CourseOption] {
        if self.catalog.is_none() {
            self.catalog = Some(course_sel::shared_catalog());
        }
        self.catalog.as_deref().unwrap_or(&[])
    }

    /// Replace one generated catalog row with the next seed.
    ///
    /// Index 0 is Penguin Pass and returns `None`. An index past the end
    /// returns `None`. The ride stays on [`Self::course_choice`] until
    /// [`Self::set_course`].
    pub fn reroll(&mut self, index: usize) -> Option<&CourseOption> {
        if self.catalog.is_none() {
            self.catalog = Some(course_sel::shared_catalog());
        }
        let CourseChoice::Generated {
            terrain,
            distance_km,
            seed,
        } = self.catalog.as_ref()?.get(index)?.choice.clone()
        else {
            return None;
        };
        let choice = CourseChoice::Generated {
            terrain,
            distance_km,
            seed: seed.wrapping_add(1),
        };
        let option = course_sel::course_option(&choice).ok()?;
        self.catalog.as_mut()?[index] = option;
        self.catalog.as_ref()?.get(index)
    }

    /// Build `choice`, swap it in, and reset the ride to zero.
    ///
    /// The phase, the trainer connection, and the other settings stay.
    /// [`Settings::course_reversed`] is honored, so a reversed session stays
    /// reversed. Course facts, the road ahead, the summary, and the lap
    /// length follow the new course. The TV dashboard is told, and
    /// [`RideEvent::CourseSelected`] is queued. The event name ends with
    /// ` (Reverse)` when the course is ridden backward. On failure the old
    /// course stays and the string is a plain sentence.
    ///
    /// # Errors
    ///
    /// Returns a sentence when the route cannot be generated. Penguin Pass
    /// does not fail.
    pub fn set_course(&mut self, choice: CourseChoice) -> Result<(), String> {
        let course = course_sel::build_course_dir(&choice, self.settings.course_reversed)?;
        self.commit_course(course, choice);
        Ok(())
    }

    /// Whether the loaded course is ridden backward.
    #[must_use]
    pub fn course_reversed(&self) -> bool {
        self.settings.course_reversed
    }

    /// Ride the current course forward or backward.
    ///
    /// `Ok(())` and no event when `reversed` already matches. Otherwise the
    /// course is rebuilt from [`Self::course_choice`], the ride returns to
    /// the start line, segment records use a `:rev` key when `reversed` is
    /// true, the TV course profile is refreshed, and settings are saved when
    /// a settings path is known. [`RideEvent::CourseSelected`] carries
    /// [`CourseChoice::label_with_direction`], so the toast ends with
    /// ` (Reverse)` when `reversed` is true.
    ///
    /// # Errors
    ///
    /// Returns a sentence when the route cannot be generated. The previous
    /// direction stays loaded.
    pub fn set_course_reversed(&mut self, reversed: bool) -> Result<(), String> {
        if self.settings.course_reversed == reversed {
            return Ok(());
        }
        let choice = self.choice.clone();
        let course = course_sel::build_course_dir(&choice, reversed)?;
        self.settings.course_reversed = reversed;
        self.commit_course(course, choice);
        if let Some(path) = self.settings_path.clone() {
            let _ = self.save_settings(&path);
        }
        Ok(())
    }

    fn reload_course_direction(&mut self) -> Result<(), String> {
        let choice = self.choice.clone();
        let course = course_sel::build_course_dir(&choice, self.settings.course_reversed)?;
        self.commit_course(course, choice);
        Ok(())
    }

    fn commit_course(&mut self, course: Course, choice: CourseChoice) {
        let riding = self.phase == RidePhase::Riding;
        if riding {
            self.finish_play();
            self.save_ride();
        }
        let (steepest_up_pct, steepest_down_pct) = grade_extremes(&course);
        self.ride = Ride::new(course, self.settings.rider_profile());
        self.steepest_up_pct = steepest_up_pct;
        self.steepest_down_pct = steepest_down_pct;
        self.stats.clear();
        self.power.clear();
        self.choice = choice.clone();
        self.settings.course = choice;
        let name = self
            .choice
            .label_with_direction(self.settings.course_reversed);
        self.events.push(RideEvent::CourseSelected { name });
        self.push_dashboard_course();
        if riding {
            self.start_recording();
        } else {
            self.open_play();
        }
    }

    /// Rider profile the physics are using right now.
    ///
    /// [`Session::apply_settings`] updates this immediately. [`Session::restart`]
    /// builds a new ride from the same profile and returns the bike to the
    /// start line.
    #[must_use]
    pub fn rider(&self) -> pp_core::RiderProfile {
        *self.ride.rider()
    }

    /// Put the bike back on the start line and apply the current rider profile.
    ///
    /// Elapsed time, distance, speed, and lap return to zero. Ride statistics
    /// and the toast queue are cleared. The simulated power target and the
    /// trainer connection are left as they are. A ride ended with
    /// [`Session::end_ride`] runs again.
    pub fn restart(&mut self) {
        self.finish_play();
        self.phase = RidePhase::Riding;
        self.ride_ended = false;
        self.stats.clear();
        self.power.clear();
        self.events.clear();
        self.clock_s = 0.0;
        self.rebuild_ride();
        self.save_ride();
        self.start_recording();
    }

    /// HUD numbers for this moment, separate from the physics sample.
    ///
    /// A real trainer whose link is down or whose sample is stale reports
    /// `None` for power and cadence. The physics still uses 0 W. Coasting on
    /// the simulated rider, or on a connected trainer that reports 0 W, is
    /// `Some(0.0)`.
    #[must_use]
    pub fn readout(&self) -> LiveReadout {
        let observed = self.observe_power();
        LiveReadout {
            power_w: observed.power_w,
            cadence_rpm: observed.cadence_rpm,
            heart_rate_bpm: observed.heart_rate_bpm,
            power_3s_w: self.power.average_3s(self.clock_s),
        }
    }

    /// Link facts for the trainer chip and the pairing headline.
    #[must_use]
    pub fn connection(&self) -> ConnectionStatus {
        let phase = self.link_phase();
        let detail = self.failure_detail();
        let failed = matches!(phase, LinkPhase::Failed);
        let bluetooth_available = self.bluetooth_available();
        ConnectionStatus {
            source: self.active_source(),
            trainer_name: self.trainer_name(),
            phase,
            resistance: self.resistance_mode(),
            bluetooth_available,
            failure_reason: failed
                .then(|| connection::plain_failure(detail.as_deref().unwrap_or(""))),
            failure_detail: detail,
        }
    }

    /// Trainers for the pairing list, strongest first.
    ///
    /// A trainer that disappears for one advertisement stays until
    /// [`PAIRING_HOLD_S`] has passed. Ties break by name, then id. The
    /// remembered trainer is flagged.
    #[must_use]
    pub fn pairing(&mut self) -> Vec<PairingEntry> {
        self.refresh_pairing();
        let mut entries: Vec<PairingEntry> = self
            .seen
            .iter()
            .map(|seen| PairingEntry {
                id: seen.id.clone(),
                name: seen.name.clone(),
                rssi_dbm: seen.rssi_dbm,
                bars: crate::signal::signal_bars(seen.rssi_dbm),
                remembered: self.trainer_is_remembered(&seen.id, &seen.name),
                ftms: seen.ftms,
                cycling_power: seen.cycling_power,
                fec: seen.fec,
            })
            .collect();
        entries.sort_by(|left, right| {
            right
                .bars
                .cmp(&left.bars)
                .then_with(|| rssi_rank(right.rssi_dbm).cmp(&rssi_rank(left.rssi_dbm)))
                .then_with(|| left.name.cmp(&right.name))
                .then_with(|| left.id.cmp(&right.id))
        });
        entries
    }

    /// Drop the current scan and start a new one.
    ///
    /// Works whether or not a scan is already running. The ride returns to
    /// the simulated rider. `last_trainer_id` is kept, and the pairing list
    /// starts empty.
    pub fn search_again(&mut self) {
        self.seen.clear();
        self.disconnect();
        self.spawn_error = None;
        self.start_scan();
    }

    /// Totals so far. After [`Session::end_ride`] these no longer change.
    #[must_use]
    pub fn summary(&self) -> crate::stats::RideSummary {
        let mut summary = self.stats.summary(&self.ride.telemetry());
        let (fish, best_combo, segments) = self.play_state.summary_bits();
        summary.fish = fish;
        summary.best_combo = best_combo;
        summary.segments = segments;
        summary.workout_result = self.group.workout_result();
        summary.strava_uploaded = self.group.strava_uploaded();
        summary
    }

    /// Segments, the ghost, fish, and the powerup slot for this frame.
    #[must_use]
    pub fn play(&self) -> crate::play::PlayView {
        let frame = self.ride.telemetry();
        let mut view = self
            .play_state
            .view(self.settings.arcade, self.settings.ghost, &frame);
        let power_w = frame.power_w;
        let player = self.player_input(&frame, power_w);
        view.bots = self.group.bots();
        view.draft_saving_pct = self.group.draft_saving_pct(&player);
        view.workout = self.group.workout_view();
        view.workout_profile = self.group.workout_profile();
        view.workout_erg_w = self.group.erg_w();
        view
    }

    /// Lane the rider is in, from `0.0` to `8.0`. The game calls this every frame.
    ///
    /// `0.0` is the left asphalt edge and `8.0` is the gravel right shoulder.
    /// A fish is collected when this value is within half a slot of the fish.
    /// Non-finite values are ignored.
    pub fn set_lane_position(&mut self, slot: f32) {
        self.play_state.set_lane(slot);
    }

    /// Start the powerup held in the slot.
    ///
    /// Does nothing when the slot is empty or arcade mode is off. The effect
    /// changes the physics for its duration and does not change trainer grade,
    /// recorded watts, or the FIT power channel.
    pub fn activate_powerup(&mut self) {
        if !self.settings.arcade {
            return;
        }
        if let Some(kind) = self.play_state.activate() {
            self.events.push(RideEvent::PowerupActivated { kind });
        }
    }

    /// Lifetime rides, badges, and unlocked kits.
    #[must_use]
    pub fn profile(&self) -> crate::play::ProfileView {
        self.play_state.profile_view()
    }

    /// Built-in workouts, then any `.zwo` files beside the settings.
    #[must_use]
    pub fn workouts(&self) -> Vec<crate::play::WorkoutSummary> {
        Group::workouts(self.bundle_dir().as_deref())
    }

    /// The workout selected for the next ride, if one is selected.
    #[must_use]
    pub fn selected_workout_id(&self) -> Option<&str> {
        self.group.selected_workout_id()
    }

    /// Select a workout, or `None` for a free ride.
    ///
    /// An unknown id is ignored. The runner reloads from the start.
    pub fn select_workout(&mut self, id: Option<&str>) {
        let dir = self.bundle_dir();
        self.group.select(id, dir.as_deref(), self.settings.ftp_w);
    }

    /// Scale workout targets by this percent, clamped to `-25..=25`.
    ///
    /// A ramp test ignores the nudge.
    pub fn set_workout_bias(&mut self, percent: i8) {
        self.group.set_bias(percent);
    }

    /// End the current workout step and start the next one.
    pub fn skip_workout_step(&mut self) {
        let (events, erg) = self.group.skip();
        for event in events {
            self.events.push(event);
        }
        self.apply_erg(erg);
    }

    /// Store a ramp-test FTP and save settings when a settings path is known.
    pub fn accept_ramp_ftp(&mut self, ftp_w: u16) {
        self.settings.ftp_w = ftp_w;
        self.settings = self.settings.normalized();
        self.ride.set_rider(self.settings.rider_profile());
        if let Some(path) = self.settings_path.clone() {
            let _ = self.save_settings(&path);
        }
    }

    /// Point Strava at these origins. Tests use a local server.
    ///
    /// Nothing is opened until a settings path or rides directory is set.
    pub fn set_strava_endpoints(&mut self, endpoints: pp_strava::Endpoints) {
        self.group.set_endpoints(endpoints);
        if let Some(dir) = self.bundle_dir() {
            self.group.ensure_strava(&dir);
        }
    }

    /// What the Strava menu can show without waiting.
    #[must_use]
    pub fn strava_state(&self) -> pp_strava::StravaState {
        self.group.strava_state()
    }

    /// Start the Strava login. The string is the page to open.
    ///
    /// `None` when no settings directory is configured.
    pub fn begin_strava_connect(&mut self) -> Option<String> {
        self.group.begin_strava_connect()
    }

    /// Stop a Strava login that is in progress.
    pub fn cancel_strava_connect(&mut self) {
        self.group.cancel_strava_connect();
    }

    /// Forget the Strava login and keep the application keys.
    pub fn disconnect_strava(&mut self) {
        self.group.disconnect_strava();
    }

    /// Upload the last saved FIT file, if there is one and it has not been sent.
    pub fn upload_last_ride_to_strava(&mut self) {
        let Some(path) = self.last_saved_ride.clone() else {
            return;
        };
        let name = self.course().name().to_owned();
        self.group.upload_file(&path, &name, false);
    }

    /// Freeze the ride and the summary, and ease a controlled trainer to flat.
    ///
    /// Later [`Session::tick`] calls publish the frozen telemetry and do not
    /// move the bike until [`Session::restart`].
    pub fn end_ride(&mut self) {
        if self.ride_ended {
            return;
        }
        self.stats.finish();
        self.finish_play();
        self.ride_ended = true;
        self.phase = RidePhase::Ended;
        self.send_flat_grade();
        self.trace.flush();
        self.save_ride();
    }

    /// Whether [`Session::end_ride`] is in effect.
    #[must_use]
    pub fn ride_ended(&self) -> bool {
        self.phase == RidePhase::Ended
    }

    /// Facts for the title card and the course line.
    ///
    /// Every course this session rides is a closed loop, so [`CourseFacts::is_loop`]
    /// is true and [`Session::distance_left_m`] is the rest of the current lap.
    #[must_use]
    pub fn course_facts(&self) -> CourseFacts {
        let course = self.ride.course();
        CourseFacts {
            name: course.name().to_owned(),
            length_m: course.length_m(),
            total_ascent_m: course.total_ascent_m(),
            steepest_up_pct: self.steepest_up_pct,
            steepest_down_pct: self.steepest_down_pct,
            is_loop: true,
        }
    }

    /// Distance still to ride, in meters.
    ///
    /// On a loop this is the distance left in the current lap.
    #[must_use]
    pub fn distance_left_m(&self) -> f64 {
        let length = self.ride.course().length_m();
        let at = self.ride.telemetry().lap_distance_m;
        (length - at).clamp(0.0, length)
    }

    /// Fill `out` with the road from the rider through `window_m` ahead.
    ///
    /// `step_m` is the spacing, in meters. The first sample is the rider and
    /// the last is the end of the window. Distance wraps at the lap. `out`
    /// is cleared and its capacity is reused, so a steady window does not
    /// allocate again. A bad step or window clears `out`. At most 4096
    /// samples are written.
    pub fn fill_ahead(&self, window_m: f64, step_m: f64, out: &mut Vec<AheadPoint>) {
        out.clear();
        if !window_m.is_finite() || window_m < 0.0 || !step_m.is_finite() || step_m < 1.0e-3 {
            return;
        }
        let course = self.ride.course();
        let length = course.length_m();
        let origin = self.ride.telemetry().lap_distance_m;
        let mut ahead = 0.0;
        loop {
            let distance = if length > 0.0 {
                (origin + ahead).rem_euclid(length)
            } else {
                0.0
            };
            out.push(AheadPoint {
                ahead_m: ahead,
                distance_m: distance,
                elevation_m: course.elevation_at(distance),
                grade_pct: course.grade_pct_at(distance),
            });
            if ahead >= window_m - 1.0e-9 || out.len() >= 4096 {
                break;
            }
            let mut next = ahead + step_m;
            if next > window_m {
                next = window_m;
            }
            if next <= ahead {
                break;
            }
            ahead = next;
        }
    }

    /// Take the toasts queued since the last drain.
    ///
    /// The queue keeps at most [`crate::EVENT_QUEUE_CAP`] events and drops
    /// the oldest when it is full.
    pub fn drain_events(&mut self) -> Vec<RideEvent> {
        self.events.drain()
    }

    /// Remember a settings load so a damaged file can raise a toast.
    ///
    /// Call this once after [`Settings::load`] when the warning should be
    /// shown. A missing file has no warning and queues nothing.
    pub fn note_settings_load(&mut self, loaded: &LoadedSettings) {
        if let Some(detail) = loaded.warning.clone() {
            self.events.push(RideEvent::SettingsFileDamaged { detail });
        }
    }

    /// Write the current settings and queue [`RideEvent::SettingsSaved`] on success.
    ///
    /// # Errors
    ///
    /// Returns the same errors as [`Settings::save`]. A failed save queues nothing.
    pub fn save_settings(&mut self, path: impl AsRef<Path>) -> Result<(), crate::SettingsError> {
        self.settings.save(path)?;
        self.events.push(RideEvent::SettingsSaved);
        Ok(())
    }

    fn apply_sim_power(&mut self) {
        self.sim.set_power(self.sim_power_w);
        let cadence = if self.sim_power_w == 0 {
            0.0
        } else {
            SIM_PEDAL_CADENCE_RPM
        };
        self.sim.set_cadence(cadence);
    }

    fn send_grade(&mut self, grade_pct: f32) {
        if self.group.holding_erg() {
            return;
        }
        if self.using_trainer && !self.controlling_trainer() {
            return;
        }
        let scaled = grade_pct * self.settings.trainer_difficulty;
        if !scaled.is_finite() {
            return;
        }
        let command = TrainerCommand::SetGrade { grade_pct: scaled };
        if self.using_trainer {
            let Some(trainer) = self.trainer.as_mut() else {
                return;
            };
            if !matches!(trainer.state(), ConnectionState::Connected) {
                return;
            }
            let _ = trainer.send(command);
            self.last_grade_sent = Some(scaled);
            return;
        }
        let _ = self.sim.send(command);
        self.last_grade_sent = Some(scaled);
    }

    fn observe_link(&mut self) {
        if !self.want_trainer {
            return;
        }
        let Some(trainer) = self.trainer.as_ref() else {
            return;
        };
        if !matches!(trainer.state(), ConnectionState::Connected) {
            return;
        }
        self.using_trainer = true;
        self.saw_connected = true;
        self.switching_mode = false;
        self.connect_error = None;
        if let Some(id) = &self.requested_id
            && self.settings.last_trainer_id.as_ref() != Some(id)
        {
            self.settings.last_trainer_id = Some(id.clone());
        }
    }

    fn poll_auto(&mut self) {
        let Some(id) = self.auto_id.clone() else {
            return;
        };
        if self.trainer.is_none() {
            return;
        }
        if !self
            .discovered()
            .iter()
            .any(|device| device_matches(device, &id))
        {
            return;
        }
        self.auto_id = None;
        self.connect(&id);
    }

    fn active_source(&self) -> ActiveSource {
        if self.using_trainer
            && let Some(trainer) = self.trainer.as_ref()
        {
            return ActiveSource::Trainer {
                name: self.display_name(),
                state: trainer.state(),
            };
        }
        ActiveSource::Simulated
    }

    fn status_line(&self) -> String {
        let Some(trainer) = self.trainer.as_ref() else {
            if let Some(err) = &self.connect_error {
                return format!("Could not connect: {err}");
            }
            if let Some(err) = &self.spawn_error {
                return format!("Bluetooth unavailable: {err}");
            }
            return "Simulated rider".to_owned();
        };
        let warning = trainer.last_warning();
        if self.switching_mode {
            return append_warning(
                format!(
                    "{}: reconnecting, {}",
                    self.display_name(),
                    self.resistance_phrase()
                ),
                warning,
            );
        }
        if let Some(err) = &self.connect_error
            && !self.using_trainer
        {
            return append_warning(format!("Could not connect: {err}"), warning);
        }
        let line = match trainer.state() {
            ConnectionState::Scanning => "Scanning for trainers".to_owned(),
            ConnectionState::Connecting => format!("Connecting to {}", self.display_name()),
            ConnectionState::Connected => format!(
                "{}: connected, {}",
                self.display_name(),
                self.resistance_phrase()
            ),
            ConnectionState::Disconnected | ConnectionState::Failed(_) if self.saw_connected => {
                format!("{}: connection lost, retrying", self.display_name())
            }
            ConnectionState::Disconnected => "Simulated rider".to_owned(),
            ConnectionState::Failed(reason) => format!("Bluetooth unavailable: {reason}"),
        };
        append_warning(line, warning)
    }

    fn display_name(&self) -> String {
        let Some(trainer) = self.trainer.as_ref() else {
            return "trainer".to_owned();
        };
        if matches!(trainer.state(), ConnectionState::Connected) || self.saw_connected {
            let name = trainer.name();
            if !name.is_empty() && name != "FTMS" {
                return name;
            }
        }
        if let Some(id) = &self.requested_id {
            let found = trainer.discovered();
            if let Some(device) = found.iter().find(|device| device_matches(device, id)) {
                return device.name.clone();
            }
            return id.clone();
        }
        let name = trainer.name();
        if name.is_empty() {
            "trainer".to_owned()
        } else {
            name
        }
    }

    fn observe_power(&self) -> Observation {
        if !self.using_trainer {
            let sample = self.sim.latest_sample().unwrap_or(TrainerSample {
                power_w: Some(0),
                cadence_rpm: Some(0.0),
                ..TrainerSample::default()
            });
            return self.with_strap_heart(Observation {
                physics: sample,
                power_w: sample.power_w.map(f32::from),
                cadence_rpm: sample.cadence_rpm,
                heart_rate_bpm: sample.heart_rate_bpm,
            });
        }
        let down = Observation {
            physics: zero_power(),
            power_w: None,
            cadence_rpm: None,
            heart_rate_bpm: None,
        };
        let Some(trainer) = self.trainer.as_ref() else {
            return self.with_strap_heart(down);
        };
        if !matches!(trainer.state(), ConnectionState::Connected) {
            return self.with_strap_heart(down);
        }
        let Some(sample) = trainer.latest_sample() else {
            return self.with_strap_heart(down);
        };
        self.with_strap_heart(Observation {
            physics: TrainerSample {
                power_w: Some(sample.power_w.unwrap_or(0)),
                cadence_rpm: sample.cadence_rpm,
                speed_kph: sample.speed_kph,
                heart_rate_bpm: sample.heart_rate_bpm,
            },
            power_w: sample.power_w.map(f32::from),
            cadence_rpm: sample.cadence_rpm,
            heart_rate_bpm: sample.heart_rate_bpm,
        })
    }

    fn with_strap_heart(&self, mut observed: Observation) -> Observation {
        if let Some(bpm) = self
            .heart
            .as_ref()
            .and_then(|link| heart::strap_bpm_u8(link.as_ref()))
        {
            observed.heart_rate_bpm = Some(bpm);
            observed.physics.heart_rate_bpm = Some(bpm);
        }
        observed
    }

    fn open_heart(&mut self) {
        let Some(factory) = self.heart_factory.as_mut() else {
            return;
        };
        match factory.spawn() {
            Ok(mut link) => {
                if let Some(id) = self.settings.heart_rate_id.clone() {
                    link.connect(&id);
                }
                self.heart = Some(link);
                self.heart_error = None;
                self.note_heart_events();
            }
            Err(err) => {
                self.heart = None;
                self.heart_error = Some(err);
            }
        }
    }

    fn disconnect_heart(&mut self) {
        if let Some(mut link) = self.heart.take() {
            link.disconnect();
        }
        self.heart_error = None;
        self.note_heart_events();
    }

    fn note_heart_events(&mut self) {
        let connected = self
            .heart
            .as_ref()
            .is_some_and(|link| heart::is_connected(&link.state()));
        if connected && !self.heart_was_connected {
            let name = self
                .heart
                .as_ref()
                .and_then(|link| link.name())
                .or_else(|| self.settings.heart_rate_name.clone())
                .unwrap_or_else(|| "Heart rate".to_owned());
            if self.settings.heart_rate_name.is_none() {
                self.settings.heart_rate_name = Some(name.clone());
            }
            self.events.push(RideEvent::HeartRateConnected { name });
            self.heart_was_connected = true;
        } else if !connected && self.heart_was_connected {
            let name = self
                .settings
                .heart_rate_name
                .clone()
                .or_else(|| self.heart.as_ref().and_then(|link| link.name()))
                .unwrap_or_else(|| "Heart rate".to_owned());
            self.events.push(RideEvent::HeartRateLost { name });
            self.heart_was_connected = false;
        }
    }

    fn link_phase(&self) -> LinkPhase {
        if self.spawn_error.is_some() && self.trainer.is_none() {
            return LinkPhase::Failed;
        }
        if self.switching_mode {
            return LinkPhase::Reconnecting;
        }
        let Some(trainer) = self.trainer.as_ref() else {
            return LinkPhase::Idle;
        };
        match trainer.state() {
            ConnectionState::Scanning => LinkPhase::Searching,
            ConnectionState::Connecting => LinkPhase::Connecting,
            ConnectionState::Connected => LinkPhase::Connected,
            ConnectionState::Disconnected if self.saw_connected => LinkPhase::Reconnecting,
            ConnectionState::Failed(_) if self.saw_connected => LinkPhase::Reconnecting,
            ConnectionState::Failed(_) => LinkPhase::Failed,
            ConnectionState::Disconnected => LinkPhase::Idle,
        }
    }

    fn bluetooth_available(&self) -> bool {
        if self.spawn_error.is_some() {
            return false;
        }
        !matches!(
            self.trainer.as_ref().map(|trainer| trainer.state()),
            Some(ConnectionState::Failed(reason))
                if !self.saw_connected && adapter_failure(&reason)
        )
    }

    fn failure_detail(&self) -> Option<String> {
        if let Some(err) = &self.spawn_error {
            return Some(err.clone());
        }
        if let Some(err) = &self.connect_error {
            return Some(err.clone());
        }
        match self.trainer.as_ref().map(|trainer| trainer.state()) {
            Some(ConnectionState::Failed(reason)) => Some(reason),
            _ => None,
        }
    }

    fn trainer_name(&self) -> Option<String> {
        self.trainer.as_ref().map(|_| self.display_name())
    }

    fn resistance_mode(&self) -> ResistanceMode {
        if self.controlling_trainer() {
            ResistanceMode::Controlled
        } else {
            ResistanceMode::ReadingOnly
        }
    }

    fn controlling_trainer(&self) -> bool {
        self.settings.control_trainer && !self.control_refused()
    }

    fn control_refused(&self) -> bool {
        connection::control_was_refused(
            self.trainer
                .as_ref()
                .and_then(|trainer| trainer.last_warning())
                .as_deref(),
        )
    }

    fn note_link_events(&mut self) {
        let phase = self.link_phase();
        if phase != self.stored_phase {
            let name = self.trainer_name().unwrap_or_else(|| "Trainer".to_owned());
            match phase {
                LinkPhase::Connected if self.link_lost => {
                    self.events.push(RideEvent::TrainerReconnected { name });
                    self.link_lost = false;
                }
                LinkPhase::Connected => {
                    self.events.push(RideEvent::TrainerConnected { name });
                }
                LinkPhase::Reconnecting | LinkPhase::Failed if self.link_phase_was_connected() => {
                    self.events.push(RideEvent::TrainerLinkLost { name });
                    self.link_lost = true;
                }
                _ => {}
            }
            self.stored_phase = phase;
        }
        if phase == LinkPhase::Connected && self.control_refused() && !self.control_notified {
            self.control_notified = true;
            let name = self.trainer_name().unwrap_or_else(|| "Trainer".to_owned());
            self.events.push(RideEvent::ControlRefused { name });
        }
        if phase != LinkPhase::Connected {
            self.control_notified = false;
        }
    }

    fn link_phase_was_connected(&self) -> bool {
        self.stored_phase == LinkPhase::Connected
    }

    fn note_dashboard_event(&mut self) {
        let error = {
            let dash = lock_dash(&self.dash);
            if dash.phase == DashboardRun::Failed {
                dash.error.clone()
            } else {
                None
            }
        };
        if error != self.last_dashboard_error {
            if let Some(reason) = error.clone() {
                self.events.push(RideEvent::DashboardFailed { reason });
            }
            self.last_dashboard_error = error;
        }
    }

    fn refresh_pairing(&mut self) {
        if let Some(trainer) = self.trainer.as_ref() {
            for found in trainer.discovered() {
                if let Some(seen) = self.seen.iter_mut().find(|seen| seen.id == found.id) {
                    seen.name = found.name;
                    seen.rssi_dbm = found.rssi_dbm;
                    seen.last_seen_s = self.clock_s;
                    seen.ftms = found.ftms;
                    seen.cycling_power = found.cycling_power;
                    seen.fec = found.fec;
                } else {
                    self.seen.push(SeenTrainer {
                        id: found.id,
                        name: found.name,
                        rssi_dbm: found.rssi_dbm,
                        last_seen_s: self.clock_s,
                        ftms: found.ftms,
                        cycling_power: found.cycling_power,
                        fec: found.fec,
                    });
                }
            }
        }
        let now = self.clock_s;
        self.seen
            .retain(|seen| now - seen.last_seen_s <= PAIRING_HOLD_S);
    }

    fn trainer_is_remembered(&self, id: &str, name: &str) -> bool {
        self.settings
            .last_trainer_id
            .as_deref()
            .is_some_and(|remembered| {
                let remembered = remembered.trim();
                !remembered.is_empty()
                    && (id.eq_ignore_ascii_case(remembered)
                        || name
                            .to_ascii_lowercase()
                            .contains(&remembered.to_ascii_lowercase()))
            })
    }

    fn trace_tick(
        &mut self,
        dt_s: f64,
        observed: &Observation,
        game_speed_mps: f32,
        frame: &Telemetry,
    ) {
        if !self.trace.is_recording() {
            self.traced_speed_mps = game_speed_mps;
            return;
        }
        let previous_speed_mps = self.traced_speed_mps;
        self.traced_speed_mps = game_speed_mps;
        let phase = match self.phase {
            RidePhase::Ready => "ready",
            RidePhase::Riding => "riding",
            RidePhase::Ended => "ended",
        };
        let source = if !self.using_trainer {
            "sim"
        } else if self
            .trainer
            .as_ref()
            .is_some_and(|trainer| matches!(trainer.state(), ConnectionState::Connected))
        {
            "trainer"
        } else {
            "none"
        };
        let link = self
            .trainer
            .as_ref()
            .map(|trainer| match trainer.state() {
                ConnectionState::Disconnected => "disconnected",
                ConnectionState::Scanning => "scanning",
                ConnectionState::Connecting => "connecting",
                ConnectionState::Connected => "connected",
                ConnectionState::Failed(_) => "failed",
            })
            .unwrap_or("idle");
        self.trace.record(&TraceTick {
            clock_s: self.clock_s,
            dt_s,
            phase,
            source,
            link,
            sample: SampleStamp {
                power_w: observed.power_w,
                cadence_rpm: observed.cadence_rpm,
                trainer_speed_kph: observed.physics.speed_kph,
                heart_rate_bpm: observed.heart_rate_bpm,
            },
            previous_speed_mps,
            game_speed_mps,
            grade_pct: frame.grade_pct,
            grade_sent_pct: self.last_grade_sent,
            distance_m: frame.distance_m,
            power_3s_w: self.power.average_3s(self.clock_s),
        });
    }

    fn resolved_rides_dir(&self) -> Option<PathBuf> {
        if let Some(dir) = &self.rides_dir_override {
            return Some(dir.clone());
        }
        if let Some(dir) = &self.settings.rides_folder {
            return Some(dir.clone());
        }
        self.settings_path.as_ref().and_then(|path| {
            path.parent()
                .map(|parent| parent.join("rides"))
                .filter(|parent| !parent.as_os_str().is_empty())
        })
    }

    fn bundle_dir(&self) -> Option<PathBuf> {
        if let Some(path) = &self.settings_path {
            return path.parent().and_then(|parent| {
                if parent.as_os_str().is_empty() {
                    None
                } else {
                    Some(parent.to_path_buf())
                }
            });
        }
        if let Some(dir) = &self.rides_dir_override {
            return Some(dir.clone());
        }
        self.settings.rides_folder.clone()
    }

    fn attach_files(&mut self) {
        let Some(dir) = self.bundle_dir() else {
            return;
        };
        if self.play_state.has_dir() && self.ride.telemetry().distance_m > 1.0 {
            self.group.ensure_strava(&dir);
            return;
        }
        let choice = self.choice.clone();
        let arcade = self.settings.arcade;
        self.play_state
            .attach(dir.clone(), self.ride.course(), &choice, arcade);
        self.group.ensure_strava(&dir);
    }

    fn open_play(&mut self) {
        let choice = self.choice.clone();
        let arcade = self.settings.arcade;
        let count = self.settings.bots_count;
        let ftp_w = self.settings.ftp_w;
        let dir = self.bundle_dir();
        let reversed = self.ride.course().is_reversed();
        self.play_state
            .open(self.ride.course(), &choice, arcade, reversed);
        self.group
            .on_ride_start(self.ride.course(), &choice, count, ftp_w, dir.as_deref());
    }

    fn finish_play(&mut self) {
        if self.phase == RidePhase::Ready {
            return;
        }
        let ascent_m = self.stats.summary(&self.ride.telemetry()).ascent_m;
        let frame = self.ride.telemetry();
        let offset_s = self.utc_offset_s;
        for event in self.play_state.finish(&frame, ascent_m, offset_s) {
            self.events.push(event);
        }
    }

    fn step_physics(&mut self, dt_s: f64, sample: &TrainerSample) -> Telemetry {
        let base = self.settings.rider_profile();
        let modifiers = self.play_state.modifiers();
        let assisted = self.settings.arcade && modifiers != pp_arcade::PhysicsModifiers::IDENTITY;
        let before = self.ride.telemetry();
        let power_w = f32::from(sample.power_w.unwrap_or(0));
        let count = self.settings.bots_count;
        let choice = self.choice.clone();
        self.group.sync_count(self.ride.course(), &choice, count);
        let draft = self
            .group
            .draft_factor(&self.player_input(&before, power_w));
        let mut profile = if assisted {
            modifiers.apply(base)
        } else {
            base
        };
        if draft.is_finite() && (draft - 1.0).abs() > 1.0e-4 {
            profile.cda_m2 *= draft;
        }
        let changed = profile != base;
        if changed {
            self.ride.set_rider(profile);
        }
        let frame = self.ride.step(dt_s, sample);
        if changed {
            self.ride.set_rider(base);
        }
        self.play_state.set_assisted(assisted);
        frame
    }

    fn player_input(&self, frame: &Telemetry, power_w: f32) -> PlayerInput {
        PlayerInput {
            total_distance_m: frame.distance_m,
            speed_mps: frame.speed_mps,
            power_w,
            slot: self.play_state.lane(),
            mass_kg: self.settings.rider_profile().rider_mass_kg,
        }
    }

    fn apply_erg(&mut self, send: ErgSend) {
        match send {
            ErgSend::Keep => {}
            ErgSend::Hold(watts) => {
                if self.send_target_power(watts) {
                    self.group.note_erg(Some(watts));
                }
            }
            ErgSend::Release => self.group.note_erg(None),
        }
    }

    fn send_target_power(&mut self, watts: u16) -> bool {
        if self.using_trainer && !self.controlling_trainer() {
            return false;
        }
        let command = TrainerCommand::SetTargetPower { watts };
        if self.using_trainer {
            let Some(trainer) = self.trainer.as_mut() else {
                return false;
            };
            if !matches!(trainer.state(), ConnectionState::Connected) {
                return false;
            }
            let _ = trainer.send(command);
            return true;
        }
        let _ = self.sim.send(command);
        true
    }

    fn drain_strava(&mut self) {
        for event in self.group.drain_strava() {
            self.events.push(event);
        }
    }

    #[cfg(test)]
    pub(crate) fn sim_grade_and_target(&self) -> (Option<f32>, Option<u16>) {
        (self.sim.last_grade_pct(), self.sim.last_target_power_w())
    }

    fn start_recording(&mut self) {
        if self.phase != RidePhase::Riding {
            return;
        }
        self.open_play();
        let rider = self.rider();
        let trainer_name = if self.using_trainer {
            self.trainer_name().filter(|name| !name.is_empty())
        } else {
            Some("Simulated rider".to_owned())
        };
        self.recorder = Some(pp_record::Recorder::new(pp_record::RideMeta {
            started_at: std::time::SystemTime::now(),
            course_name: self.course().name().to_owned(),
            rider_mass_kg: rider.rider_mass_kg,
            bike_mass_kg: rider.bike_mass_kg,
            ftp_w: rider.ftp_w,
            trainer_name,
            game_version: env!("CARGO_PKG_VERSION").to_owned(),
            utc_offset_s: self.utc_offset_s,
        }));
    }

    fn record_sample(&mut self, frame: &Telemetry) {
        if self.phase != RidePhase::Riding || self.recorder.is_none() {
            return;
        }
        let readout = self.readout();
        let Some(recorder) = self.recorder.as_mut() else {
            return;
        };
        recorder.push(pp_record::RideSample {
            elapsed_s: frame.elapsed_s,
            power_w: readout.power_w,
            cadence_rpm: readout.cadence_rpm,
            heart_rate_bpm: readout.heart_rate_bpm.map(u16::from),
            speed_mps: frame.speed_mps,
            distance_m: frame.distance_m,
            altitude_m: frame.elevation_m,
            grade_pct: frame.grade_pct,
            lap: frame.lap,
        });
    }

    fn save_ride(&mut self) {
        let Some(recorder) = self.recorder.take() else {
            return;
        };
        let Some(dir) = self.resolved_rides_dir() else {
            return;
        };
        match recorder.finish().write(&dir) {
            Ok(pp_record::WriteOutcome::Written(path)) => {
                eprintln!("pp-session: ride {}", path.display());
                self.last_saved_ride = Some(path.clone());
                let course_name = self.course().name().to_owned();
                self.group
                    .maybe_auto_upload(&path, self.settings.strava_auto_upload, &course_name);
                self.events.push(RideEvent::RideSaved { path });
            }
            Ok(pp_record::WriteOutcome::Skipped(reason)) => {
                let reason = match reason {
                    pp_record::SkipReason::TooShort { .. }
                    | pp_record::SkipReason::TooShortDistance { .. } => {
                        "Ride too short to save".to_owned()
                    }
                };
                self.events.push(RideEvent::RideNotSaved { reason });
            }
            Err(err) => {
                self.events.push(RideEvent::RideNotSaved {
                    reason: format!("Could not save the ride: {err}"),
                });
            }
        }
    }

    fn send_flat_grade(&mut self) {
        if !self.using_trainer || !self.controlling_trainer() {
            return;
        }
        let Some(trainer) = self.trainer.as_mut() else {
            return;
        };
        if !matches!(trainer.state(), ConnectionState::Connected) {
            return;
        }
        let _ = trainer.send(TrainerCommand::SetGrade { grade_pct: 0.0 });
        self.last_grade_sent = Some(0.0);
    }

    fn trainer_mode(&self) -> SessionMode {
        if self.settings.control_trainer {
            SessionMode::Control
        } else {
            SessionMode::ReadOnly
        }
    }

    fn resistance_phrase(&self) -> &'static str {
        if self.controlling_trainer() {
            "resistance follows the road"
        } else if self.control_refused() {
            "reports power only and cannot change resistance"
        } else {
            "reading only"
        }
    }

    /// Drop the open trainer and spawn another in the current control mode.
    ///
    /// A trainer the rider had asked to use is connected again. Power stays
    /// on that trainer (0 W until the new link is up) so the keyboard target
    /// does not take over during the swap.
    fn reopen_trainer_for_mode(&mut self) {
        let reconnect = if self.want_trainer || self.using_trainer || self.saw_connected {
            self.requested_id
                .clone()
                .or_else(|| self.settings.last_trainer_id.clone())
        } else {
            None
        };
        let had_trainer = self.trainer.is_some();
        if let Some(trainer) = self.trainer.take() {
            self.defer_drop(trainer);
        }
        if !had_trainer {
            return;
        }
        self.switching_mode = reconnect.is_some();
        self.start_scan();
        let Some(id) = reconnect else {
            return;
        };
        self.requested_id = Some(id.clone());
        self.want_trainer = true;
        self.using_trainer = true;
        let Some(trainer) = self.trainer.as_ref() else {
            return;
        };
        match trainer.connect(&id) {
            Ok(()) => self.connect_error = None,
            Err(err) => self.connect_error = Some(err.to_string()),
        }
    }

    fn dashboard_endpoint(&self) -> Option<SocketAddr> {
        if !self.settings.tv_dashboard_enabled {
            return None;
        }
        let port = self
            .dashboard_port_override
            .unwrap_or(self.settings.tv_dashboard_port);
        Some(SocketAddr::new(self.dashboard_host, port))
    }

    fn restart_dashboard(&mut self) {
        self.stop_dashboard();
        self.start_dashboard();
    }

    fn stop_dashboard(&mut self) {
        let server = {
            let mut dash = lock_dash(&self.dash);
            dash.phase = DashboardRun::Off;
            dash.error = None;
            if let Some(pending) = dash.current.take() {
                pending.cancel.store(true, Ordering::SeqCst);
                dash.retired.push(pending);
            }
            dash.server.take()
        };
        if let Some(server) = server {
            self.defer_drop(server);
        }
    }

    fn start_dashboard(&mut self) {
        self.stop_dashboard();
        let Some(addr) = self.dashboard_endpoint() else {
            return;
        };
        let course = cast_profile(self.ride.course());
        let opener = Arc::clone(&self.opener);
        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_thread = Arc::clone(&cancel);
        let (tx, rx) = mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("pp-session-dash".to_owned())
            .spawn(move || {
                let result = opener.open(addr, course);
                if cancel_thread.load(Ordering::SeqCst) {
                    drop(result);
                    let _ = tx.send(DashMsg::Cancelled);
                    return;
                }
                let message = match result {
                    Ok(server) => DashMsg::Ready(server),
                    Err(err) => DashMsg::Failed(err),
                };
                let _ = tx.send(message);
            });
        let mut dash = lock_dash(&self.dash);
        match spawned {
            Ok(handle) => {
                dash.phase = DashboardRun::Starting;
                dash.error = None;
                dash.current = Some(PendingDash {
                    rx,
                    handle: Some(handle),
                    cancel,
                });
            }
            Err(err) => {
                dash.phase = DashboardRun::Failed;
                dash.error = Some(format!("could not start the dashboard thread: {err}"));
            }
        }
    }

    fn push_dashboard_course(&self) {
        let profile = cast_profile(self.ride.course());
        let mut dash = lock_dash(&self.dash);
        if dash.phase == DashboardRun::Running {
            if let Some(server) = dash.server.as_ref() {
                server.set_course(profile);
            } else {
                dash.pending_course = Some(profile);
                return;
            }
            dash.pending_course = None;
        } else {
            dash.pending_course = Some(profile);
        }
    }

    fn publish_dashboard(&self, telemetry: Telemetry) {
        self.poll_dashboard();
        let status = self.cast_status();
        let mut dash = lock_dash(&self.dash);
        if dash.phase == DashboardRun::Running {
            let pending = dash.pending_course.take();
            if let Some(server) = &dash.server {
                if let Some(profile) = pending {
                    server.set_course(profile);
                }
                server.publish(telemetry);
                server.publish_status(status);
            }
        }
    }

    fn cast_status(&self) -> pp_cast::RideStatus {
        let trainer = self.using_trainer;
        let frame = self.ride.telemetry();
        let view = self
            .play_state
            .view(self.settings.arcade, self.settings.ghost, &frame);
        let draft = self
            .group
            .draft_saving_pct(&self.player_input(&frame, frame.power_w));
        let link = match self.link_phase() {
            LinkPhase::Searching | LinkPhase::Connecting => pp_cast::LinkPhase::Searching,
            LinkPhase::Connected => pp_cast::LinkPhase::Connected,
            LinkPhase::Reconnecting => pp_cast::LinkPhase::Reconnecting,
            LinkPhase::Idle | LinkPhase::Failed => pp_cast::LinkPhase::Unknown,
        };
        pp_cast::RideStatus {
            ftp_w: Some(self.settings.ftp_w),
            source: if trainer {
                pp_cast::RideSource::Trainer
            } else {
                pp_cast::RideSource::Simulated
            },
            trainer_name: trainer.then(|| self.trainer_name()).flatten(),
            link: if trainer {
                link
            } else {
                pp_cast::LinkPhase::Unknown
            },
            resistance: trainer.then(|| match self.resistance_mode() {
                ResistanceMode::Controlled => pp_cast::ResistanceMode::Controlled,
                ResistanceMode::ReadingOnly => pp_cast::ResistanceMode::ReadingOnly,
            }),
            power_3s_w: self.power.average_3s(self.clock_s),
            workout: self.cast_workout(),
            segment: view.active_segment.as_ref().map(cast_segment),
            draft_saving_pct: (draft > 0).then_some(draft),
            fish_collected: optional_arcade_count(self.settings.arcade, view.fish_count),
            fish_combo: optional_arcade_count(self.settings.arcade, view.combo),
            active_powerup: view.powerup_active.map(|power| pp_cast::PowerupStatus {
                name: power.kind.to_string(),
                remaining_s: power.remaining_s.max(0.0),
            }),
            course_reversed: self.settings.course_reversed,
        }
    }

    fn cast_workout(&self) -> Option<pp_cast::WorkoutStatus> {
        let view = self.group.workout_view()?;
        let name = self.group.workout_name()?.to_owned();
        let span = view.total_elapsed_s + view.total_remaining_s;
        let progress = if span.is_finite() && span > 0.0 {
            (view.total_elapsed_s / span).clamp(0.0, 1.0)
        } else {
            0.0
        };
        Some(pp_cast::WorkoutStatus {
            name,
            block_label: view.label,
            target_w: rounded_watts(view.target_w),
            remaining_s: rounded_seconds(view.step_remaining_s),
            progress,
            erg_active: self.group.holding_erg(),
        })
    }

    fn poll_dashboard(&self) {
        let mut dash = lock_dash(&self.dash);
        reap_handles(&mut dash.drop_threads);
        let retired = std::mem::take(&mut dash.retired);
        let mut still = Vec::new();
        for pending in retired {
            match take_message(&pending.rx) {
                Inbox::Waiting => still.push(pending),
                Inbox::Message(message) => settle_pending(pending, message, false, &mut dash),
            }
        }
        dash.retired = still;
        if let Some(pending) = dash.current.take() {
            match take_message(&pending.rx) {
                Inbox::Waiting => dash.current = Some(pending),
                Inbox::Message(message) => settle_pending(pending, message, true, &mut dash),
            }
        }
    }

    fn rebuild_ride(&mut self) {
        let course = self.ride.course().clone();
        self.ride = Ride::new(course, self.settings.rider_profile());
    }

    fn defer_drop(&mut self, value: impl Send + 'static) {
        self.reap_drops();
        // If the thread cannot be started, dropping the closure drops `value`
        // on this thread. That is the fallback when the process is out of threads.
        if let Ok(handle) = std::thread::Builder::new()
            .name("pp-session-drop".to_owned())
            .spawn(move || drop(value))
        {
            self.drop_threads.push(handle);
        }
    }

    fn reap_drops(&mut self) {
        let pending = std::mem::take(&mut self.drop_threads);
        for handle in pending {
            if handle.is_finished() {
                let _ = handle.join();
            } else {
                self.drop_threads.push(handle);
            }
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.trace.flush();
        self.finish_play();
        self.save_ride();
        if let Some(trainer) = self.trainer.take() {
            self.defer_drop(trainer);
        }
        let (server, mut handles) = {
            let mut dash = lock_dash(&self.dash);
            dash.phase = DashboardRun::Off;
            if let Some(pending) = dash.current.take() {
                pending.cancel.store(true, Ordering::SeqCst);
                handles_of(pending, &mut dash);
            }
            let retired = std::mem::take(&mut dash.retired);
            for pending in retired {
                pending.cancel.store(true, Ordering::SeqCst);
                handles_of(pending, &mut dash);
            }
            let server = dash.server.take();
            let handles = std::mem::take(&mut dash.drop_threads);
            (server, handles)
        };
        if let Some(server) = server {
            self.defer_drop(server);
        }
        self.drop_threads.append(&mut handles);
        for handle in self.drop_threads.drain(..) {
            let _ = handle.join();
        }
    }
}

trait DashboardHost: Send {
    fn publish(&self, telemetry: Telemetry);
    fn publish_status(&self, status: pp_cast::RideStatus);
    fn set_course(&self, course: CourseProfile);
    fn urls(&self) -> Vec<String>;
}

impl DashboardHost for CastServer {
    fn publish(&self, telemetry: Telemetry) {
        CastServer::publish(self, telemetry);
    }

    fn publish_status(&self, status: pp_cast::RideStatus) {
        CastServer::publish_status(self, status);
    }

    fn set_course(&self, course: CourseProfile) {
        CastServer::set_course(self, course);
    }

    fn urls(&self) -> Vec<String> {
        CastServer::urls(self)
    }
}

trait DashboardOpener: Send + Sync {
    fn open(
        &self,
        bind: SocketAddr,
        course: CourseProfile,
    ) -> Result<Box<dyn DashboardHost>, String>;
}

struct CastOpener;

impl DashboardOpener for CastOpener {
    fn open(
        &self,
        bind: SocketAddr,
        course: CourseProfile,
    ) -> Result<Box<dyn DashboardHost>, String> {
        match CastServer::start(CastConfig { bind, course }) {
            Ok(server) => Ok(Box::new(server)),
            Err(err) => Err(err.to_string()),
        }
    }
}

struct DashState {
    phase: DashboardRun,
    error: Option<String>,
    server: Option<Box<dyn DashboardHost>>,
    pending_course: Option<CourseProfile>,
    current: Option<PendingDash>,
    retired: Vec<PendingDash>,
    drop_threads: Vec<JoinHandle<()>>,
}

impl Default for DashState {
    fn default() -> Self {
        Self {
            phase: DashboardRun::Off,
            error: None,
            server: None,
            pending_course: None,
            current: None,
            retired: Vec::new(),
            drop_threads: Vec::new(),
        }
    }
}

struct PendingDash {
    rx: Receiver<DashMsg>,
    handle: Option<JoinHandle<()>>,
    cancel: Arc<AtomicBool>,
}

enum DashMsg {
    Ready(Box<dyn DashboardHost>),
    Failed(String),
    Cancelled,
}

enum Inbox {
    Waiting,
    Message(DashMsg),
}

fn lock_dash(mutex: &Mutex<DashState>) -> MutexGuard<'_, DashState> {
    mutex.lock().unwrap_or_else(|err| err.into_inner())
}

fn take_message(rx: &Receiver<DashMsg>) -> Inbox {
    match rx.try_recv() {
        Ok(message) => Inbox::Message(message),
        Err(TryRecvError::Empty) => Inbox::Waiting,
        Err(TryRecvError::Disconnected) => Inbox::Message(DashMsg::Failed(
            "dashboard thread stopped before it was listening".to_owned(),
        )),
    }
}

fn settle_pending(mut pending: PendingDash, message: DashMsg, install: bool, dash: &mut DashState) {
    if let Some(handle) = pending.handle.take() {
        let _ = handle.join();
    }
    let cancelled = pending.cancel.load(Ordering::SeqCst);
    match message {
        DashMsg::Ready(server) if install && !cancelled => {
            dash.server = Some(server);
            dash.phase = DashboardRun::Running;
            dash.error = None;
        }
        DashMsg::Ready(server) => {
            if let Some(handle) = spawn_drop(server) {
                dash.drop_threads.push(handle);
            }
        }
        DashMsg::Failed(err) if install && !cancelled => {
            dash.server = None;
            dash.phase = DashboardRun::Failed;
            dash.error = Some(err);
        }
        DashMsg::Failed(_) | DashMsg::Cancelled => {}
    }
}

fn spawn_drop(value: impl Send + 'static) -> Option<JoinHandle<()>> {
    std::thread::Builder::new()
        .name("pp-session-drop".to_owned())
        .spawn(move || drop(value))
        .ok()
}

fn reap_handles(handles: &mut Vec<JoinHandle<()>>) {
    let pending = std::mem::take(handles);
    for handle in pending {
        if handle.is_finished() {
            let _ = handle.join();
        } else {
            handles.push(handle);
        }
    }
}

fn handles_of(mut pending: PendingDash, dash: &mut DashState) {
    drop(pending.rx);
    if let Some(handle) = pending.handle.take() {
        dash.drop_threads.push(handle);
    }
}

fn accepted_dt(dt_s: f64) -> f64 {
    if dt_s.is_finite() && dt_s > 0.0 {
        dt_s.min(pp_course::MAX_DT_S)
    } else {
        0.0
    }
}

fn grade_extremes(course: &Course) -> (f32, f32) {
    let length = course.length_m();
    if length <= 0.0 {
        return (0.0, 0.0);
    }
    let mut up = f32::MIN;
    let mut down = f32::MAX;
    let mut distance = 0.0;
    while distance <= length {
        let grade = course.grade_pct_at(distance);
        up = up.max(grade);
        down = down.min(grade);
        distance += 1.0;
    }
    (up, down)
}

fn adapter_failure(reason: &str) -> bool {
    let reason = reason.to_ascii_lowercase();
    reason.contains("adapter") || reason.contains("bluetooth") || reason.contains("not available")
}

fn rssi_rank(rssi_dbm: Option<i16>) -> i16 {
    rssi_dbm.unwrap_or(i16::MIN)
}

fn zero_power() -> TrainerSample {
    TrainerSample {
        power_w: Some(0),
        ..TrainerSample::default()
    }
}

fn device_matches(device: &DiscoveredTrainer, id: &str) -> bool {
    let needle = id.to_ascii_lowercase();
    device.id.eq_ignore_ascii_case(id) || device.name.to_ascii_lowercase().contains(&needle)
}

fn append_warning(line: String, warning: Option<String>) -> String {
    match warning {
        Some(warning) if !warning.is_empty() => format!("{line} ({warning})"),
        _ => line,
    }
}

fn cast_segment(segment: &crate::play::ActiveSegment) -> pp_cast::SegmentStatus {
    pp_cast::SegmentStatus {
        name: segment.name.clone(),
        elapsed_s: finite_f32(segment.elapsed_s),
        progress: segment.fraction.clamp(0.0, 1.0),
        delta_s: segment.split_s.map(|seconds| finite_f32(-seconds)),
    }
}

fn optional_arcade_count(arcade: bool, count: u32) -> Option<u32> {
    (arcade && count > 0).then_some(count)
}

fn finite_f32(value: f64) -> f32 {
    if value.is_finite() { value as f32 } else { 0.0 }
}

fn rounded_watts(watts: Option<f32>) -> u16 {
    let Some(watts) = watts else {
        return 0;
    };
    if !watts.is_finite() || watts <= 0.0 {
        return 0;
    }
    let rounded = watts.round();
    if rounded >= f32::from(u16::MAX) {
        u16::MAX
    } else {
        rounded as u16
    }
}

fn rounded_seconds(seconds: f32) -> u32 {
    if !seconds.is_finite() || seconds <= 0.0 {
        return 0;
    }
    let rounded = seconds.round();
    if rounded >= 4_294_967_295.0 {
        u32::MAX
    } else {
        rounded as u32
    }
}

fn cast_profile(course: &Course) -> CourseProfile {
    CourseProfile {
        name: course.name().to_owned(),
        length_m: course.length_m(),
        points: course
            .profile(DASHBOARD_PROFILE_STEP_M)
            .into_iter()
            .map(|point| CastPoint {
                distance_m: point.distance_m,
                elevation_m: point.elevation_m,
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connection::{LinkPhase, ResistanceMode};
    use crate::events::RideEvent;
    use crate::link::TrainerFactory;
    use crate::settings::LoadedSettings;
    use crate::{CourseChoice, HeartRateFactory, RidePhase, TerrainPreset};
    use pp_core::{RiderProfile, TrainerCommand, TrainerSample};
    use pp_course::Ride;
    use pp_trainer::{
        ConnectionState, DiscoveredSensor, DiscoveredTrainer, HeartRateLink, SimHeartRate, Trainer,
        TrainerError,
    };
    use std::net::{Ipv4Addr, TcpListener};
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex, MutexGuard};
    use std::time::{Duration, Instant};

    fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
        mutex.lock().unwrap_or_else(|err| err.into_inner())
    }

    struct FakeState {
        state: ConnectionState,
        name: String,
        sample: Option<TrainerSample>,
        grades: Vec<f32>,
        fail_send: bool,
        discovered: Vec<DiscoveredTrainer>,
        connects: Vec<String>,
        warning: Option<String>,
        drop_delay: Duration,
        drop_count: u32,
        modes: Vec<SessionMode>,
        sends: u32,
        spawns: Vec<(SessionMode, Option<f32>, Option<f32>)>,
        virtual_shifting: bool,
        virtual_gears: Vec<u8>,
    }

    #[derive(Clone)]
    struct Handle(Arc<Mutex<FakeState>>);

    impl Handle {
        fn new() -> Self {
            Self(Arc::new(Mutex::new(FakeState {
                state: ConnectionState::Scanning,
                name: "FTMS".to_owned(),
                sample: None,
                grades: Vec::new(),
                fail_send: false,
                discovered: Vec::new(),
                connects: Vec::new(),
                warning: None,
                drop_delay: Duration::ZERO,
                drop_count: 0,
                modes: Vec::new(),
                sends: 0,
                spawns: Vec::new(),
                virtual_shifting: false,
                virtual_gears: Vec::new(),
            })))
        }

        fn set_state(&self, state: ConnectionState) {
            lock(&self.0).state = state;
        }

        fn set_sample(&self, sample: Option<TrainerSample>) {
            lock(&self.0).sample = sample;
        }

        fn set_power(&self, watts: u16) {
            self.set_sample(Some(TrainerSample {
                power_w: Some(watts),
                cadence_rpm: Some(SIM_PEDAL_CADENCE_RPM),
                ..TrainerSample::default()
            }));
        }

        fn grades(&self) -> Vec<f32> {
            lock(&self.0).grades.clone()
        }

        fn clear_grades(&self) {
            lock(&self.0).grades.clear();
        }

        fn set_fail_send(&self, fail: bool) {
            lock(&self.0).fail_send = fail;
        }

        fn set_discovered(&self, found: Vec<DiscoveredTrainer>) {
            lock(&self.0).discovered = found;
        }

        fn connects(&self) -> Vec<String> {
            lock(&self.0).connects.clone()
        }

        fn set_warning(&self, warning: Option<String>) {
            lock(&self.0).warning = warning;
        }

        fn set_drop_delay(&self, delay: Duration) {
            lock(&self.0).drop_delay = delay;
        }

        fn drop_count(&self) -> u32 {
            lock(&self.0).drop_count
        }

        fn modes(&self) -> Vec<SessionMode> {
            lock(&self.0).modes.clone()
        }

        fn spawns(&self) -> Vec<(SessionMode, Option<f32>, Option<f32>)> {
            lock(&self.0).spawns.clone()
        }

        fn sends(&self) -> u32 {
            lock(&self.0).sends
        }

        fn set_virtual_shifting(&self, supported: bool) {
            lock(&self.0).virtual_shifting = supported;
        }

        fn virtual_gears(&self) -> Vec<u8> {
            lock(&self.0).virtual_gears.clone()
        }
    }

    struct FakeTrainer {
        inner: Arc<Mutex<FakeState>>,
    }

    impl Drop for FakeTrainer {
        fn drop(&mut self) {
            let delay = lock(&self.inner).drop_delay;
            if !delay.is_zero() {
                std::thread::sleep(delay);
            }
            let mut state = lock(&self.inner);
            state.drop_count = state.drop_count.saturating_add(1);
        }
    }

    impl Trainer for FakeTrainer {
        fn latest_sample(&self) -> Option<TrainerSample> {
            lock(&self.inner).sample
        }

        fn send(&mut self, command: TrainerCommand) -> Result<(), TrainerError> {
            let mut state = lock(&self.inner);
            state.sends = state.sends.saturating_add(1);
            if state.fail_send {
                return Err(TrainerError::Message("send failed".to_owned()));
            }
            if !matches!(state.state, ConnectionState::Connected) {
                return Err(TrainerError::NotConnected);
            }
            match command {
                TrainerCommand::SetGrade { grade_pct } => state.grades.push(grade_pct),
                TrainerCommand::SetVirtualGear { gear } => state.virtual_gears.push(gear),
                TrainerCommand::SetTargetPower { .. } => {}
            }
            Ok(())
        }

        fn supports_virtual_shifting(&self) -> bool {
            lock(&self.inner).virtual_shifting
        }

        fn state(&self) -> ConnectionState {
            lock(&self.inner).state.clone()
        }

        fn name(&self) -> String {
            lock(&self.inner).name.clone()
        }

        fn last_warning(&self) -> Option<String> {
            lock(&self.inner).warning.clone()
        }
    }

    impl TrainerLink for FakeTrainer {
        fn discovered(&self) -> Vec<DiscoveredTrainer> {
            lock(&self.inner).discovered.clone()
        }

        fn connect(&self, id_or_name: &str) -> Result<(), TrainerError> {
            let id = id_or_name.trim();
            if id.is_empty() {
                return Err(TrainerError::Message("trainer id or name is empty".into()));
            }
            let mut state = lock(&self.inner);
            state.connects.push(id.to_owned());
            state.state = ConnectionState::Connecting;
            if let Some(device) = state
                .discovered
                .iter()
                .find(|device| device_matches(device, id))
            {
                state.name = device.name.clone();
            } else {
                state.name = id.to_owned();
            }
            Ok(())
        }
    }

    struct FakeFactory {
        inner: Arc<Mutex<FakeState>>,
    }

    impl TrainerFactory for FakeFactory {
        fn spawn(&mut self) -> Result<Box<dyn TrainerLink>, String> {
            self.spawn_mode(SessionMode::Control)
        }

        fn spawn_mode(&mut self, mode: SessionMode) -> Result<Box<dyn TrainerLink>, String> {
            lock(&self.inner).modes.push(mode);
            Ok(Box::new(FakeTrainer {
                inner: Arc::clone(&self.inner),
            }))
        }

        fn spawn_configured(
            &mut self,
            mode: SessionMode,
            rider_mass_kg: Option<f32>,
            bike_mass_kg: Option<f32>,
        ) -> Result<Box<dyn TrainerLink>, String> {
            lock(&self.inner)
                .spawns
                .push((mode, rider_mass_kg, bike_mass_kg));
            self.spawn_mode(mode)
        }
    }

    struct FailingFactory;

    impl TrainerFactory for FailingFactory {
        fn spawn(&mut self) -> Result<Box<dyn TrainerLink>, String> {
            Err("bluetooth adapter not found".to_owned())
        }
    }

    fn offline() -> Settings {
        Settings {
            tv_dashboard_enabled: false,
            ..Settings::default()
        }
    }

    fn session_with(settings: Settings, handle: &Handle) -> Session {
        Session::with_options(
            settings,
            SessionOptions {
                trainer_factory: Box::new(FakeFactory {
                    inner: Arc::clone(&handle.0),
                }),
                dashboard_host: IpAddr::V4(Ipv4Addr::LOCALHOST),
                dashboard_port: Some(0),
            },
        )
    }

    fn neo() -> DiscoveredTrainer {
        DiscoveredTrainer {
            id: "neo-1".to_owned(),
            name: "Tacx Neo 2T".to_owned(),
            rssi_dbm: Some(-45),
            ftms: true,
            cycling_power: true,
            fec: true,
        }
    }

    fn grade_span(course: &Course) -> (f32, f32) {
        let mut min_grade = f32::MAX;
        let mut max_grade = f32::MIN;
        let mut distance = 0.0;
        let length = course.length_m();
        while distance <= length {
            let grade = course.grade_pct_at(distance);
            min_grade = min_grade.min(grade);
            max_grade = max_grade.max(grade);
            distance += 10.0;
        }
        (min_grade, max_grade)
    }

    fn ride_until(session: &mut Session, handle: &Handle, target: f32) {
        handle.set_power(1500);
        for _ in 0..800 {
            let frame = session.tick(1.0);
            if target > 0.0 && frame.grade_pct >= target {
                return;
            }
            if target < 0.0 && frame.grade_pct <= target {
                return;
            }
        }
        panic!(
            "grade stayed {} after riding, wanted {target}",
            session.telemetry().grade_pct
        );
    }

    fn assert_grade(session: &mut Session, handle: &Handle, difficulty: f32) {
        let mut settings = session.settings().clone();
        settings.trainer_difficulty = difficulty;
        session.apply_settings(settings);
        handle.clear_grades();
        let frame = session.tick(0.0);
        let grades = handle.grades();
        assert_eq!(grades.len(), 1, "difficulty {difficulty} grades {grades:?}");
        let expected = frame.grade_pct * difficulty;
        assert!(
            (grades[0] - expected).abs() < 1e-3,
            "sent {} wanted {expected} at grade {}",
            grades[0],
            frame.grade_pct
        );
    }

    #[test]
    fn new_starts_on_the_simulated_rider() {
        let mut session = Session::new(offline());
        assert!(matches!(session.status().source, ActiveSource::Simulated));
        assert_eq!(session.status().line, "Simulated rider");
        assert!(session.status().dashboard_urls.is_empty());
        assert!(session.course().length_m() > 0.0);
        assert!(!session.course().name().is_empty());
        session.set_sim_power(100);
        let frame = session.tick(1.0);
        assert_eq!(frame.power_w, 100.0);
        assert!(frame.distance_m > 0.0);
    }

    #[test]
    fn tick_matches_a_hand_stepped_ride_and_zero_dt_is_a_noop() {
        let mut session = Session::new(offline());
        let mut ride = Ride::new(Course::penguin_pass(), RiderProfile::default());
        let sample = TrainerSample {
            power_w: Some(200),
            cadence_rpm: Some(SIM_PEDAL_CADENCE_RPM),
            ..TrainerSample::default()
        };
        session.set_sim_power(200);
        for dt in [0.0, 1.0 / 60.0, 0.0, 0.05, 0.25, 1.0, f64::NAN] {
            let from_session = session.tick(dt);
            let from_ride = ride.step(dt, &sample);
            assert_eq!(from_session, from_ride);
        }
        let before = session.telemetry();
        assert!(before.distance_m > 0.0);
        assert_eq!(session.tick(0.0), before);
        assert_eq!(session.telemetry(), before);
    }

    #[test]
    fn nudge_and_set_sim_power_clamp_and_set_cadence() {
        let mut session = Session::new(offline());
        session.nudge_sim_power(1);
        assert_eq!(session.sim_power_w(), SIM_POWER_STEP_W);
        let frame = session.tick(0.1);
        assert_eq!(frame.power_w, f32::from(SIM_POWER_STEP_W));
        assert_eq!(frame.cadence_rpm, Some(SIM_PEDAL_CADENCE_RPM));

        session.set_sim_power(5_000);
        assert_eq!(session.sim_power_w(), SIM_POWER_MAX_W);
        session.set_sim_power(0);
        let frame = session.tick(0.1);
        assert_eq!(frame.power_w, 0.0);
        assert_eq!(frame.cadence_rpm, Some(0.0));
        session.nudge_sim_power(-3);
        assert_eq!(session.sim_power_w(), 0);
    }

    #[test]
    fn grade_command_is_road_grade_times_difficulty() {
        let handle = Handle::new();
        handle.set_discovered(vec![neo()]);
        let mut session = session_with(offline(), &handle);
        let (min_grade, max_grade) = grade_span(session.course());
        assert!(max_grade > 1.0, "course max grade {max_grade}");
        assert!(min_grade < -1.0, "course min grade {min_grade}");

        session.tick(0.05);
        assert!(
            handle.grades().is_empty(),
            "sim ticks must not send to the trainer"
        );

        session.connect("neo-1");
        handle.set_state(ConnectionState::Connected);
        session.tick(0.0);
        assert!(matches!(
            session.status().source,
            ActiveSource::Trainer {
                state: ConnectionState::Connected,
                ..
            }
        ));

        ride_until(&mut session, &handle, 1.0);
        assert!(session.telemetry().grade_pct > 1.0);
        for difficulty in [0.0, 0.5, 1.0] {
            assert_grade(&mut session, &handle, difficulty);
        }

        ride_until(&mut session, &handle, -1.0);
        assert!(session.telemetry().grade_pct < -1.0);
        for difficulty in [0.0, 0.5, 1.0] {
            assert_grade(&mut session, &handle, difficulty);
        }

        handle.clear_grades();
        handle.set_state(ConnectionState::Disconnected);
        let frame = session.tick(0.05);
        assert!(
            handle.grades().is_empty(),
            "disconnected trainer got a grade"
        );
        assert_eq!(frame.power_w, 0.0);

        handle.set_state(ConnectionState::Connected);
        handle.set_fail_send(true);
        handle.set_power(220);
        let frame = session.tick(0.5);
        assert_eq!(frame.power_w, 220.0);
        assert!(frame.elapsed_s > 0.0);
        assert!(handle.grades().is_empty());
    }

    #[test]
    fn stale_sample_is_zero_watts_and_a_drop_does_not_switch_to_sim() {
        let handle = Handle::new();
        handle.set_discovered(vec![neo()]);
        let mut session = session_with(offline(), &handle);
        session.set_sim_power(30);
        session.connect("neo-1");
        handle.set_state(ConnectionState::Connected);
        handle.set_power(1500);
        session.tick(0.0);
        let powered = session.tick(0.2);
        assert_eq!(powered.power_w, 1500.0);

        handle.set_sample(None);
        let stale = session.tick(0.2);
        assert_eq!(stale.power_w, 0.0);

        handle.set_power(1500);
        handle.set_state(ConnectionState::Disconnected);
        session.set_sim_power(400);
        session.nudge_sim_power(10);
        assert_eq!(session.sim_power_w(), 30);
        let dropped = session.tick(0.5);
        assert_eq!(dropped.power_w, 0.0);
        assert!(matches!(
            session.status().source,
            ActiveSource::Trainer {
                state: ConnectionState::Disconnected,
                ..
            }
        ));
        assert_eq!(
            session.status().line,
            "Tacx Neo 2T: connection lost, retrying"
        );

        handle.set_state(ConnectionState::Failed("timeout".to_owned()));
        let failed = session.tick(0.2);
        assert_eq!(failed.power_w, 0.0);
        assert!(session.status().line.contains("connection lost, retrying"));

        handle.set_state(ConnectionState::Connected);
        handle.set_power(180);
        let resumed = session.tick(0.2);
        assert_eq!(resumed.power_w, 180.0);
        assert_eq!(resumed.cadence_rpm, Some(SIM_PEDAL_CADENCE_RPM));
    }

    #[test]
    fn connect_flow_updates_the_active_source_and_remembers_the_id() {
        let handle = Handle::new();
        handle.set_discovered(vec![neo()]);
        let mut session = session_with(offline(), &handle);
        assert!(session.settings().last_trainer_id.is_none());

        session.set_sim_power(25);
        session.start_scan();
        assert_eq!(session.status().line, "Scanning for trainers");
        assert_eq!(session.discovered().len(), 1);
        assert_eq!(session.discovered()[0].name, "Tacx Neo 2T");

        session.connect("neo-1");
        assert_eq!(session.status().line, "Connecting to Tacx Neo 2T");
        assert!(matches!(session.status().source, ActiveSource::Simulated));
        assert!(session.settings().last_trainer_id.is_none());

        handle.set_state(ConnectionState::Connected);
        session.tick(0.0);
        assert_eq!(session.settings().last_trainer_id.as_deref(), Some("neo-1"));
        match session.status().source {
            ActiveSource::Trainer { name, state } => {
                assert_eq!(name, "Tacx Neo 2T");
                assert_eq!(state, ConnectionState::Connected);
            }
            ActiveSource::Simulated => panic!("trainer should be the source"),
        }
        assert_eq!(
            session.status().line,
            "Tacx Neo 2T: connected, resistance follows the road"
        );

        handle.set_warning(Some("control not permitted".to_owned()));
        assert_eq!(
            session.status().line,
            "Tacx Neo 2T: connected, reports power only and cannot change resistance (control not permitted)"
        );

        session.set_sim_power(900);
        session.nudge_sim_power(4);
        assert_eq!(session.sim_power_w(), 25);

        handle.set_drop_delay(Duration::from_millis(250));
        let started = Instant::now();
        session.disconnect();
        let elapsed = started.elapsed();
        assert!(
            elapsed < Duration::from_millis(100),
            "disconnect took {elapsed:?}"
        );
        assert!(matches!(session.status().source, ActiveSource::Simulated));
        assert_eq!(session.status().line, "Simulated rider");
        assert!(session.discovered().is_empty());
        session.set_sim_power(40);
        assert_eq!(session.tick(0.5).power_w, 40.0);

        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline && handle.drop_count() == 0 {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(handle.drop_count() >= 1, "trainer drop did not finish");
    }

    #[test]
    fn bluetooth_unavailable_factory_sets_a_status() {
        let mut session = Session::with_options(
            offline(),
            SessionOptions {
                trainer_factory: Box::new(FailingFactory),
                dashboard_host: IpAddr::V4(Ipv4Addr::LOCALHOST),
                dashboard_port: Some(0),
            },
        );
        session.start_scan();
        let status = session.status();
        assert!(matches!(status.source, ActiveSource::Simulated));
        assert_eq!(
            status.line,
            "Bluetooth unavailable: bluetooth adapter not found"
        );
        session.set_sim_power(50);
        assert_eq!(session.tick(0.2).power_w, 50.0);
        session.connect("neo-1");
        assert!(matches!(session.status().source, ActiveSource::Simulated));
        assert!(session.status().line.contains("Bluetooth unavailable"));
    }

    #[test]
    fn auto_connect_waits_until_the_remembered_trainer_appears() {
        let handle = Handle::new();
        let mut settings = offline();
        settings.last_trainer_id = Some("Neo".to_owned());
        let mut session = session_with(settings, &handle);

        session.auto_connect();
        assert!(handle.connects().is_empty());
        assert_eq!(session.status().line, "Scanning for trainers");

        handle.set_discovered(vec![DiscoveredTrainer {
            id: "AA:BB:CC:DD:EE:FF".to_owned(),
            name: "Tacx Neo 2T".to_owned(),
            rssi_dbm: None,
            ftms: true,
            cycling_power: true,
            fec: true,
        }]);
        session.tick(0.0);
        assert_eq!(handle.connects(), vec!["Neo".to_owned()]);
        assert_eq!(session.status().line, "Connecting to Tacx Neo 2T");

        handle.set_state(ConnectionState::Connected);
        session.tick(0.0);
        assert!(matches!(
            session.status().source,
            ActiveSource::Trainer {
                state: ConnectionState::Connected,
                ..
            }
        ));
        assert_eq!(session.settings().last_trainer_id.as_deref(), Some("Neo"));

        let idle = Handle::new();
        let mut session = session_with(offline(), &idle);
        session.auto_connect();
        assert!(idle.connects().is_empty());
        assert_eq!(session.status().line, "Simulated rider");
    }

    #[test]
    fn apply_settings_updates_difficulty_and_rider_immediately() {
        let mut session = Session::new(offline());
        let mut settings = session.settings().clone();
        settings.rider_mass_kg = 90.0;
        settings.bike_mass_kg = 11.0;
        settings.ftp_w = 260;
        settings.trainer_difficulty = 0.25;
        session.apply_settings(settings);
        assert_eq!(session.rider(), session.settings().rider_profile());
        assert_eq!(session.rider().rider_mass_kg, 90.0);
        assert_eq!(session.rider().bike_mass_kg, 11.0);
        assert_eq!(session.rider().ftp_w, 260);
        assert_eq!(session.rider().cda_m2, RiderProfile::default().cda_m2);
        assert_eq!(session.settings().trainer_difficulty, 0.25);

        session.set_sim_power(200);
        let moving = session.tick(1.0);
        assert!(moving.elapsed_s > 0.0);
        let mut settings = session.settings().clone();
        settings.rider_mass_kg = 60.0;
        settings.bike_mass_kg = 7.0;
        settings.ftp_w = 180;
        session.apply_settings(settings);
        assert_eq!(session.settings().rider_mass_kg, 60.0);
        assert_eq!(session.rider(), session.settings().rider_profile());
        assert_eq!(session.telemetry().distance_m, moving.distance_m);
        assert_eq!(session.telemetry().elapsed_s, moving.elapsed_s);
        assert_eq!(session.telemetry().speed_mps, moving.speed_mps);
        let continued = session.tick(0.5);
        assert!(continued.elapsed_s > moving.elapsed_s);

        session.restart();
        let frame = session.telemetry();
        assert_eq!(frame.elapsed_s, 0.0);
        assert_eq!(frame.distance_m, 0.0);
        assert_eq!(frame.lap_distance_m, 0.0);
        assert_eq!(frame.speed_mps, 0.0);
        assert_eq!(frame.lap, 0);
        assert_eq!(session.rider().rider_mass_kg, 60.0);
    }

    #[test]
    fn control_trainer_off_sends_nothing_and_reconnects_in_the_new_mode() {
        let handle = Handle::new();
        handle.set_discovered(vec![neo()]);
        let mut session = session_with(offline(), &handle);
        session.set_sim_power(25);
        session.connect("neo-1");
        handle.set_state(ConnectionState::Connected);
        handle.set_power(200);
        session.tick(0.0);
        assert_eq!(
            session.status().line,
            "Tacx Neo 2T: connected, resistance follows the road"
        );
        assert_eq!(handle.modes(), vec![SessionMode::Control]);
        let sent = handle.sends();
        assert!(sent >= 1, "control mode should send grade, sends {sent}");

        let mut settings = session.settings().clone();
        settings.control_trainer = false;
        session.apply_settings(settings);
        assert_eq!(
            session.status().line,
            "Tacx Neo 2T: reconnecting, reading only"
        );
        assert_eq!(
            handle.modes(),
            vec![SessionMode::Control, SessionMode::ReadOnly]
        );
        assert!(matches!(
            session.status().source,
            ActiveSource::Trainer { .. }
        ));
        session.set_sim_power(500);
        assert_eq!(session.sim_power_w(), 25);

        handle.set_state(ConnectionState::Connected);
        handle.set_power(160);
        handle.clear_grades();
        let before = handle.sends();
        let frame = session.tick(0.4);
        assert_eq!(frame.power_w, 160.0);
        assert_eq!(handle.sends(), before);
        assert!(handle.grades().is_empty());
        assert_eq!(
            session.status().line,
            "Tacx Neo 2T: connected, reading only"
        );

        let mut settings = session.settings().clone();
        settings.control_trainer = true;
        session.apply_settings(settings);
        assert_eq!(
            session.status().line,
            "Tacx Neo 2T: reconnecting, resistance follows the road"
        );
        handle.set_state(ConnectionState::Connected);
        session.tick(0.0);
        assert_eq!(
            session.status().line,
            "Tacx Neo 2T: connected, resistance follows the road"
        );
        assert!(handle.sends() > before);
    }

    #[test]
    fn shift_virtual_gear_sends_command_and_clamps() {
        let handle = Handle::new();
        handle.set_discovered(vec![neo()]);
        handle.set_virtual_shifting(true);
        let mut session = session_with(Settings::default(), &handle);
        session.connect("neo-1");
        handle.set_state(ConnectionState::Connected);
        assert_eq!(session.virtual_gear(), DEFAULT_VIRTUAL_GEAR);
        assert!(session.virtual_shifting_available());

        for expected in (DEFAULT_VIRTUAL_GEAR + 1)..=MAX_VIRTUAL_GEAR {
            assert_eq!(session.shift_virtual_gear(1), Some(expected));
        }
        assert_eq!(
            handle.virtual_gears(),
            (DEFAULT_VIRTUAL_GEAR + 1..=MAX_VIRTUAL_GEAR).collect::<Vec<_>>()
        );
        assert_eq!(session.virtual_gear(), MAX_VIRTUAL_GEAR);
        assert_eq!(*handle.virtual_gears().last().unwrap(), MAX_VIRTUAL_GEAR);

        handle.set_virtual_shifting(false);
        assert!(!session.virtual_shifting_available());
        let sent = handle.virtual_gears().len();
        assert_eq!(session.shift_virtual_gear(-1), None);
        assert_eq!(handle.virtual_gears().len(), sent);

        let mut settings = session.settings().clone();
        settings.control_trainer = false;
        session.apply_settings(settings);
        handle.set_virtual_shifting(true);
        assert!(!session.virtual_shifting_available());
        assert_eq!(session.shift_virtual_gear(1), None);
    }

    #[test]
    fn dashboard_publishes_telemetry_and_toggles_with_settings() {
        let mut session = Session::with_options(
            Settings::default(),
            SessionOptions {
                trainer_factory: Box::new(FailingFactory),
                dashboard_host: IpAddr::V4(Ipv4Addr::LOCALHOST),
                dashboard_port: Some(0),
            },
        );
        let status = wait_dashboard(&mut session, |status| {
            !status.dashboard_urls.is_empty() || status.dashboard_error.is_some()
        });
        assert_eq!(status.dashboard, DashboardRun::Running);
        assert!(status.dashboard_error.is_none());
        let urls = status.dashboard_urls;
        assert_eq!(urls.len(), 1, "{urls:?}");
        assert!(urls[0].starts_with("http://127.0.0.1:"), "{urls:?}");

        session.set_sim_power(180);
        let frame = session.tick(0.5);
        let body = fetch_telemetry(&urls[0]);
        assert_eq!(body, frame);
        let cast_status = fetch_status(&urls[0]);
        assert_eq!(cast_status.ftp_w, Some(session.settings().ftp_w));
        assert_eq!(cast_status.source, pp_cast::RideSource::Simulated);
        assert_eq!(cast_status.power_3s_w, session.readout().power_3s_w);

        let mut settings = session.settings().clone();
        settings.trainer_difficulty = 0.25;
        session.apply_settings(settings);
        assert_eq!(session.status().dashboard_urls, urls);

        let mut settings = session.settings().clone();
        settings.tv_dashboard_enabled = false;
        session.apply_settings(settings);
        assert!(session.status().dashboard_urls.is_empty());
        assert!(session.status().dashboard_error.is_none());
        assert_eq!(session.tick(0.1).power_w, 180.0);

        let mut settings = session.settings().clone();
        settings.tv_dashboard_enabled = true;
        session.apply_settings(settings);
        let status = wait_dashboard(&mut session, |status| {
            !status.dashboard_urls.is_empty() || status.dashboard_error.is_some()
        });
        assert_eq!(status.dashboard, DashboardRun::Running);
        let urls = status.dashboard_urls;
        assert_eq!(urls.len(), 1, "{urls:?}");
        let frame = session.tick(0.1);
        assert_eq!(fetch_telemetry(&urls[0]), frame);
    }

    #[test]
    fn dashboard_port_in_use_is_reported_and_the_ride_continues() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("reserve port");
        let port = listener.local_addr().expect("addr").port();
        let mut session = Session::with_options(
            Settings::default(),
            SessionOptions {
                trainer_factory: Box::new(FailingFactory),
                dashboard_host: IpAddr::V4(Ipv4Addr::LOCALHOST),
                dashboard_port: Some(port),
            },
        );
        let status = wait_dashboard(&mut session, |status| status.dashboard_error.is_some());
        assert_eq!(status.dashboard, DashboardRun::Failed);
        let error = status.dashboard_error.expect("bind error");
        assert!(error.contains("bind"), "{error}");
        assert!(status.dashboard_urls.is_empty());
        session.set_sim_power(70);
        assert_eq!(session.tick(0.2).power_w, 70.0);
        drop(listener);
    }

    #[test]
    fn apply_settings_rebinds_the_dashboard_port() {
        let port_a = free_port();
        let port_b = free_port();
        let mut session = Session::with_options(
            Settings {
                tv_dashboard_port: port_a,
                ..Settings::default()
            },
            SessionOptions {
                trainer_factory: Box::new(FailingFactory),
                dashboard_host: IpAddr::V4(Ipv4Addr::LOCALHOST),
                dashboard_port: None,
            },
        );
        let status = wait_dashboard(&mut session, |status| {
            !status.dashboard_urls.is_empty() || status.dashboard_error.is_some()
        });
        assert!(
            status.dashboard_error.is_none(),
            "{:?}",
            status.dashboard_error
        );
        let urls = status.dashboard_urls;
        assert!(urls[0].contains(&format!(":{port_a}/")), "{urls:?}");

        let mut settings = session.settings().clone();
        settings.tv_dashboard_port = port_b;
        session.apply_settings(settings);
        let status = wait_dashboard(&mut session, |status| {
            status
                .dashboard_urls
                .first()
                .is_some_and(|url| url.contains(&format!(":{port_b}/")))
                || status.dashboard_error.is_some()
        });
        assert!(
            status.dashboard_error.is_none(),
            "{:?}",
            status.dashboard_error
        );
        let urls = status.dashboard_urls;
        assert!(urls[0].contains(&format!(":{port_b}/")), "{urls:?}");
        assert!(!urls[0].contains(&format!(":{port_a}/")), "{urls:?}");
    }

    #[test]
    fn dashboard_start_does_not_block_and_cancel_frees_the_port() {
        let port = free_port();
        let mut session = Session::with_options(
            offline(),
            SessionOptions {
                trainer_factory: Box::new(FailingFactory),
                dashboard_host: IpAddr::V4(Ipv4Addr::LOCALHOST),
                dashboard_port: Some(port),
            },
        );
        session.opener = Arc::new(SlowOpener {
            delay: Duration::from_millis(400),
        });

        let mut settings = session.settings().clone();
        settings.tv_dashboard_enabled = true;
        let started = Instant::now();
        session.apply_settings(settings);
        let elapsed = started.elapsed();
        assert!(
            elapsed < Duration::from_millis(150),
            "apply_settings took {elapsed:?}"
        );
        let status = session.status();
        assert_eq!(status.dashboard, DashboardRun::Starting);
        assert!(status.dashboard_urls.is_empty());
        assert!(status.dashboard_error.is_none());

        let started = Instant::now();
        session.tick(0.016);
        let elapsed = started.elapsed();
        assert!(
            elapsed < Duration::from_millis(150),
            "tick took {elapsed:?}"
        );

        let mut settings = session.settings().clone();
        settings.tv_dashboard_enabled = false;
        let started = Instant::now();
        session.apply_settings(settings);
        let elapsed = started.elapsed();
        assert!(
            elapsed < Duration::from_millis(150),
            "stopping took {elapsed:?}"
        );
        assert_eq!(session.status().dashboard, DashboardRun::Off);

        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            match TcpListener::bind((Ipv4Addr::LOCALHOST, port)) {
                Ok(listener) => {
                    drop(listener);
                    break;
                }
                Err(_) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(err) => panic!("port {port} stayed taken: {err}"),
            }
        }
    }

    struct SlowOpener {
        delay: Duration,
    }

    impl DashboardOpener for SlowOpener {
        fn open(
            &self,
            bind: SocketAddr,
            course: CourseProfile,
        ) -> Result<Box<dyn DashboardHost>, String> {
            std::thread::sleep(self.delay);
            CastOpener.open(bind, course)
        }
    }

    fn wait_dashboard(
        session: &mut Session,
        mut ready: impl FnMut(&SessionStatus) -> bool,
    ) -> SessionStatus {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let status = session.status();
            if ready(&status) {
                return status;
            }
            if Instant::now() > deadline {
                panic!(
                    "dashboard did not settle: {:?} {}",
                    status.dashboard,
                    status.dashboard_error.unwrap_or_default()
                );
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn readout_uses_dashes_when_the_link_is_down_and_zero_when_coasting() {
        let mut session = Session::new(offline());
        session.set_sim_power(0);
        session.tick(0.2);
        let coast = session.readout();
        assert_eq!(coast.power_w, Some(0.0));
        assert_eq!(coast.cadence_rpm, Some(0.0));
        assert_eq!(coast.heart_rate_bpm, None);

        let handle = Handle::new();
        handle.set_discovered(vec![neo()]);
        let mut session = session_with(offline(), &handle);
        session.connect("neo-1");
        handle.set_state(ConnectionState::Connected);
        handle.set_sample(Some(TrainerSample {
            power_w: Some(0),
            cadence_rpm: Some(0.0),
            heart_rate_bpm: Some(120),
            ..TrainerSample::default()
        }));
        session.tick(0.2);
        let coasting = session.readout();
        assert_eq!(coasting.power_w, Some(0.0));
        assert_eq!(coasting.cadence_rpm, Some(0.0));
        assert_eq!(coasting.heart_rate_bpm, Some(120));

        handle.set_sample(None);
        handle.set_state(ConnectionState::Disconnected);
        session.tick(0.2);
        let lost = session.readout();
        assert_eq!(lost.power_w, None);
        assert_eq!(lost.cadence_rpm, None);
        assert_eq!(lost.heart_rate_bpm, None);
        assert_eq!(session.telemetry().power_w, 0.0);
    }

    #[test]
    fn three_second_power_ignores_frame_rate_and_ages_out_gaps() {
        let mut fine = Session::new(offline());
        let mut coarse = Session::new(offline());
        fine.set_sim_power(100);
        coarse.set_sim_power(100);
        for _ in 0..30 {
            fine.tick(0.1);
        }
        coarse.tick(3.0);
        let fine_avg = fine.readout().power_3s_w.expect("fine average");
        let coarse_avg = coarse.readout().power_3s_w.expect("coarse average");
        assert!((fine_avg - 100.0).abs() < 1.0, "{fine_avg}");
        assert!(
            (coarse_avg - fine_avg).abs() < 1.0,
            "{coarse_avg} vs {fine_avg}"
        );

        let handle = Handle::new();
        let mut session = session_with(offline(), &handle);
        session.connect("neo-1");
        handle.set_state(ConnectionState::Connected);
        handle.set_power(180);
        session.tick(1.0);
        assert!(session.readout().power_3s_w.is_some());
        handle.set_sample(None);
        session.tick(4.0);
        assert_eq!(session.readout().power_3s_w, None);
        assert_eq!(session.readout().power_w, None);
    }

    #[test]
    fn connection_status_names_the_phase_and_a_plain_reason() {
        let mut session = Session::with_options(
            offline(),
            SessionOptions {
                trainer_factory: Box::new(FailingFactory),
                dashboard_host: IpAddr::V4(Ipv4Addr::LOCALHOST),
                dashboard_port: Some(0),
            },
        );
        assert_eq!(session.connection().phase, LinkPhase::Idle);
        session.start_scan();
        let failed = session.connection();
        assert_eq!(failed.phase, LinkPhase::Failed);
        assert!(!failed.bluetooth_available);
        assert_eq!(
            failed.failure_reason.as_deref(),
            Some("Bluetooth is off, or this computer has no Bluetooth adapter.")
        );
        assert!(failed.failure_detail.is_some());

        let handle = Handle::new();
        handle.set_discovered(vec![neo()]);
        let mut session = session_with(offline(), &handle);
        session.connect("neo-1");
        assert_eq!(session.connection().phase, LinkPhase::Connecting);
        handle.set_state(ConnectionState::Connected);
        session.tick(0.0);
        let up = session.connection();
        assert_eq!(up.phase, LinkPhase::Connected);
        assert_eq!(up.resistance, ResistanceMode::Controlled);
        assert_eq!(up.trainer_name.as_deref(), Some("Tacx Neo 2T"));
        assert!(up.bluetooth_available);
        handle.set_warning(Some("control not permitted".to_owned()));
        assert_eq!(session.connection().resistance, ResistanceMode::ReadingOnly);
        assert_eq!(
            session.status().line,
            "Tacx Neo 2T: connected, reports power only and cannot change resistance (control not permitted)"
        );
        handle.set_state(ConnectionState::Disconnected);
        session.tick(0.0);
        assert_eq!(session.connection().phase, LinkPhase::Reconnecting);
        assert!(session.status().line.contains("connection lost, retrying"));
    }

    #[test]
    fn pairing_list_sorts_holds_and_search_again_restarts() {
        let handle = Handle::new();
        let mut settings = offline();
        settings.last_trainer_id = Some("neo-1".to_owned());
        let mut session = session_with(settings, &handle);
        handle.set_discovered(vec![
            DiscoveredTrainer {
                id: "b".to_owned(),
                name: "Bravo".to_owned(),
                rssi_dbm: Some(-88),
                ftms: false,
                cycling_power: true,
                fec: false,
            },
            DiscoveredTrainer {
                id: "a2".to_owned(),
                name: "Alpha".to_owned(),
                rssi_dbm: Some(-65),
                ftms: true,
                cycling_power: false,
                fec: false,
            },
            DiscoveredTrainer {
                id: "neo-1".to_owned(),
                name: "Tacx Neo 2T".to_owned(),
                rssi_dbm: Some(-65),
                ftms: true,
                cycling_power: true,
                fec: true,
            },
            DiscoveredTrainer {
                id: "mystery".to_owned(),
                name: "Hidden".to_owned(),
                rssi_dbm: None,
                ftms: false,
                cycling_power: false,
                fec: false,
            },
        ]);
        session.start_scan();
        session.tick(0.2);
        let rows = session.pairing();
        assert_eq!(rows[0].id, "a2");
        assert_eq!(rows[0].bars, 3);
        assert!(rows.iter().any(|row| row.id == "neo-1" && row.remembered));
        assert!(rows.last().unwrap().bars <= rows.first().unwrap().bars);
        let hidden = rows.iter().find(|row| row.id == "mystery").unwrap();
        assert_eq!(hidden.bars, 0);
        assert!(!hidden.remembered);
        let bravo = rows.iter().find(|row| row.id == "b").unwrap();
        assert!(bravo.cycling_power);
        assert!(!bravo.ftms && !bravo.fec);
        let neo = rows.iter().find(|row| row.id == "neo-1").unwrap();
        assert!(neo.ftms && neo.fec);
        let alpha = rows.iter().find(|row| row.id == "a2").unwrap();
        assert!(alpha.ftms);
        assert!(!alpha.fec);

        handle.set_discovered(vec![]);
        session.tick(1.0);
        assert!(session.pairing().iter().any(|row| row.id == "neo-1"));
        session.tick(8.0);
        assert!(session.pairing().is_empty());

        let before = handle.modes().len();
        session.search_again();
        assert!(session.pairing().is_empty());
        assert!(handle.modes().len() > before);
        assert_eq!(session.connection().phase, LinkPhase::Searching);
        assert!(matches!(session.status().source, ActiveSource::Simulated));
    }

    #[test]
    fn trainer_spawn_receives_the_rider_mass_and_a_later_edit_waits_for_the_next_link() {
        let handle = Handle::new();
        let mut session = session_with(offline(), &handle);
        session.start_scan();
        assert_eq!(
            handle.spawns(),
            vec![(SessionMode::Control, Some(75.0), Some(9.0))]
        );

        let mut settings = session.settings().clone();
        settings.rider_mass_kg = 90.0;
        settings.bike_mass_kg = 11.0;
        session.apply_settings(settings);
        assert_eq!(session.rider().rider_mass_kg, 90.0);
        assert_eq!(session.rider().bike_mass_kg, 11.0);
        assert_eq!(handle.spawns().len(), 1);

        session.disconnect();
        session.start_scan();
        assert_eq!(
            handle.spawns(),
            vec![
                (SessionMode::Control, Some(75.0), Some(9.0)),
                (SessionMode::Control, Some(90.0), Some(11.0)),
            ]
        );
    }

    #[test]
    fn summary_smooths_power_excludes_gaps_and_end_ride_holds_the_bike() {
        let mut session = Session::new(offline());
        session.set_sim_power(1_500);
        session.tick(0.016);
        assert_eq!(
            session.summary().max_power_w,
            0.0,
            "one sample is not a maximum"
        );
        for _ in 0..60 {
            session.tick(1.0 / 60.0);
        }
        let steady = session.summary();
        assert!(
            steady.average_power_w > 1_000.0,
            "{}",
            steady.average_power_w
        );
        assert!(steady.max_power_w > 1_000.0);
        assert!(steady.average_cadence_rpm.unwrap() > 80.0);
        assert!(steady.average_heart_rate_bpm.is_none());
        assert!(steady.elapsed_s > 0.9);

        session.set_sim_power(0);
        for _ in 0..60 {
            session.tick(1.0 / 60.0);
        }
        let mixed = session.summary();
        assert!(mixed.average_power_w < steady.average_power_w);
        assert!(
            (mixed.average_cadence_rpm.unwrap() - steady.average_cadence_rpm.unwrap()).abs() < 1.0
        );

        let handle = Handle::new();
        let mut session = session_with(offline(), &handle);
        session.connect("neo-1");
        handle.set_state(ConnectionState::Connected);
        handle.set_power(200);
        session.tick(2.0);
        let before = session.summary().average_power_w;
        handle.set_sample(None);
        session.tick(2.0);
        let after = session.summary().average_power_w;
        assert!(
            (after - before).abs() < 1.0,
            "gap changed the average: {before} -> {after}"
        );

        let distance = session.telemetry().distance_m;
        session.end_ride();
        assert!(session.ride_ended());
        assert_eq!(handle.grades().last().copied(), Some(0.0));
        session.tick(5.0);
        assert_eq!(session.telemetry().distance_m, distance);
        assert_eq!(session.summary().distance_m, distance);
        session.restart();
        assert!(!session.ride_ended());
        assert_eq!(session.telemetry().elapsed_s, 0.0);
        assert_eq!(session.summary().elapsed_s, 0.0);
        assert!(session.summary().laps.is_empty());
    }

    #[test]
    fn a_completed_lap_is_recorded_and_ascent_is_climbed_height() {
        let mut session = Session::new(offline());
        session.set_sim_power(1_500);
        let mut guard = 0;
        let mut saw_lap = false;
        while session.telemetry().lap < 2 && guard < 800 {
            session.tick(1.0);
            saw_lap |= session.drain_events().iter().any(|event| {
                matches!(event, RideEvent::LapCompleted { lap: 1, time_s } if *time_s > 1.0)
            });
            guard += 1;
        }
        assert!(session.telemetry().lap >= 2, "did not finish a lap");
        let summary = session.summary();
        assert!(summary.ascent_m > 1.0, "ascent {}", summary.ascent_m);
        assert!(
            summary
                .laps
                .iter()
                .any(|lap| lap.lap == 1 && lap.time_s > 1.0)
        );
        assert!(saw_lap, "lap event was not queued");
    }

    #[test]
    fn course_facts_and_the_ahead_window_wrap_and_reuse_the_buffer() {
        let mut session = Session::new(offline());
        let facts = session.course_facts();
        assert!(!facts.name.is_empty());
        assert!(facts.is_loop);
        assert!(facts.length_m > 0.0);
        assert!(facts.total_ascent_m > 0.0);
        assert!(facts.steepest_up_pct > 0.0);
        assert!(facts.steepest_down_pct < 0.0);
        assert!((session.distance_left_m() - facts.length_m).abs() < 1.0);

        let mut ahead = Vec::new();
        session.fill_ahead(1_000.0, 250.0, &mut ahead);
        assert_eq!(ahead.first().unwrap().ahead_m, 0.0);
        assert!((ahead.last().unwrap().ahead_m - 1_000.0).abs() < 1.0e-6);
        let capacity = ahead.capacity();
        session.fill_ahead(1_000.0, 250.0, &mut ahead);
        assert_eq!(ahead.capacity(), capacity);

        session.set_sim_power(1_500);
        let mut guard = 0;
        while session.telemetry().lap_distance_m < facts.length_m - 50.0 && guard < 800 {
            session.tick(1.0);
            guard += 1;
        }
        session.fill_ahead(200.0, 50.0, &mut ahead);
        assert!(
            ahead.iter().any(|point| point.distance_m < 180.0),
            "window did not wrap: {:?}",
            ahead
                .iter()
                .map(|point| point.distance_m)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn events_report_the_toasts_and_drop_the_oldest_when_full() {
        let dir = std::env::temp_dir().join(format!(
            "pp-session-events-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let mut session = Session::new(offline());
        session.note_settings_load(&LoadedSettings {
            settings: offline(),
            warning: Some("could not parse settings".to_owned()),
        });
        session
            .save_settings(dir.join("penguin-pass.toml"))
            .unwrap();
        let events = session.drain_events();
        assert!(
            events
                .iter()
                .any(|event| matches!(event, RideEvent::SettingsFileDamaged { .. }))
        );
        assert!(
            events
                .iter()
                .any(|event| matches!(event, RideEvent::SettingsSaved))
        );

        let handle = Handle::new();
        handle.set_discovered(vec![neo()]);
        let mut session = session_with(offline(), &handle);
        session.connect("neo-1");
        handle.set_state(ConnectionState::Connected);
        session.tick(0.0);
        handle.set_state(ConnectionState::Disconnected);
        session.tick(0.0);
        handle.set_state(ConnectionState::Connected);
        session.tick(0.0);
        handle.set_warning(Some("control not permitted".to_owned()));
        session.tick(0.0);
        let events = session.drain_events();
        assert!(
            events
                .iter()
                .any(|event| matches!(event, RideEvent::TrainerConnected { .. }))
        );
        assert!(
            events
                .iter()
                .any(|event| matches!(event, RideEvent::TrainerLinkLost { .. }))
        );
        assert!(
            events
                .iter()
                .any(|event| matches!(event, RideEvent::TrainerReconnected { .. }))
        );
        assert!(
            events
                .iter()
                .any(|event| matches!(event, RideEvent::ControlRefused { .. }))
        );

        for index in 0..40 {
            session.note_settings_load(&LoadedSettings {
                settings: offline(),
                warning: Some(index.to_string()),
            });
        }
        let drained = session.drain_events();
        assert_eq!(drained.len(), crate::EVENT_QUEUE_CAP);
        assert!(
            matches!(drained.last(), Some(RideEvent::SettingsFileDamaged { detail }) if detail == "39")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn tick_with_the_interface_calls_stays_under_a_millisecond() {
        let mut session = Session::new(offline());
        session.set_sim_power(180);
        let mut ahead = Vec::new();
        let started = Instant::now();
        let frames = 600;
        for _ in 0..frames {
            session.tick(1.0 / 60.0);
            let _ = session.readout();
            let _ = session.connection();
            let _ = session.summary();
            session.fill_ahead(1_000.0, 4.0, &mut ahead);
        }
        let per_frame = started.elapsed() / frames;
        eprintln!("interface tick cost {per_frame:?}");
        assert!(
            per_frame < Duration::from_millis(5),
            "tick plus HUD calls took {per_frame:?}"
        );
    }

    #[test]
    fn ready_holds_the_bike_and_sends_a_flat_grade() {
        let handle = Handle::new();
        handle.set_discovered(vec![neo()]);
        let mut session = session_with(offline(), &handle);
        assert_eq!(session.phase(), RidePhase::Riding);
        assert!(!session.ride_ended());

        session.connect("neo-1");
        handle.set_state(ConnectionState::Connected);
        session.tick(0.0);
        ride_until(&mut session, &handle, 1.0);
        let ridden = session.telemetry();
        assert!(ridden.distance_m > 0.0);
        assert!(ridden.elapsed_s > 0.0);

        session.hold_at_start();
        assert_eq!(session.phase(), RidePhase::Ready);
        assert!(!session.ride_ended());
        assert_eq!(session.telemetry().distance_m, 0.0);
        assert_eq!(session.telemetry().elapsed_s, 0.0);
        assert_eq!(session.summary().distance_m, 0.0);
        handle.set_power(400);
        handle.clear_grades();
        let held = session.tick(1.0);
        assert_eq!(held.distance_m, 0.0);
        assert_eq!(held.elapsed_s, 0.0);
        assert_eq!(held.speed_mps, 0.0);
        assert_eq!(held.lap, 0);
        assert_eq!(session.summary().elapsed_s, 0.0);
        assert_eq!(session.summary().distance_m, 0.0);
        let grades = handle.grades();
        assert!(!grades.is_empty(), "a ready tick commands the trainer");
        assert!(grades.iter().all(|grade| *grade == 0.0), "{grades:?}");
        assert_eq!(session.readout().power_w, Some(400.0));
        assert_eq!(session.connection().phase, LinkPhase::Connected);

        session.begin_ride();
        assert_eq!(session.phase(), RidePhase::Riding);
        let started = session.tick(0.5);
        assert!(started.elapsed_s > 0.0 && started.elapsed_s < 1.0);
        assert!(started.distance_m > 0.0);

        session.end_ride();
        assert_eq!(session.phase(), RidePhase::Ended);
        assert!(session.ride_ended());
        session.begin_ride();
        assert_eq!(session.phase(), RidePhase::Ended);
        session.restart();
        assert_eq!(session.phase(), RidePhase::Riding);
        assert!(!session.ride_ended());
    }

    #[test]
    fn set_course_resets_the_ride_and_keeps_phase_and_the_trainer() {
        let handle = Handle::new();
        handle.set_discovered(vec![neo()]);
        let mut session = session_with(offline(), &handle);
        session.connect("neo-1");
        handle.set_state(ConnectionState::Connected);
        session.tick(0.0);
        session.hold_at_start();
        let before = session.course_facts();
        let _ = session.drain_events();
        let choice = CourseChoice::Generated {
            terrain: TerrainPreset::Rolling,
            distance_km: 5,
            seed: 3,
        };
        session.set_course(choice.clone()).expect("rolling 5 km");
        assert_eq!(session.phase(), RidePhase::Ready);
        assert_eq!(session.course_choice(), &choice);
        let facts = session.course_facts();
        assert_eq!(facts.name, "Rolling 5 km");
        assert!(
            (facts.length_m - 5_000.0).abs() < 200.0,
            "{}",
            facts.length_m
        );
        assert_ne!(facts.length_m, before.length_m);
        let mut ahead = Vec::new();
        session.fill_ahead(200.0, 20.0, &mut ahead);
        assert!(ahead.len() > 2, "{ahead:?}");
        assert!(ahead.last().expect("ahead").ahead_m <= 200.0);
        assert_eq!(session.summary().distance_m, 0.0);
        assert_eq!(session.distance_left_m(), facts.length_m);
        assert_eq!(session.telemetry().elapsed_s, 0.0);
        assert_eq!(session.connection().phase, LinkPhase::Connected);
        assert!(session.drain_events().iter().any(|event| {
            matches!(event, RideEvent::CourseSelected { name } if name == "Rolling 5 km")
        }));

        let err = session
            .set_course(CourseChoice::Generated {
                terrain: TerrainPreset::Desert,
                distance_km: 1,
                seed: 7,
            })
            .expect_err("shorter than the generator allows");
        assert!(err.contains("could not build"), "{err}");
        assert_eq!(session.course_choice(), &choice);
        assert_eq!(session.course_facts().name, "Rolling 5 km");
        assert_eq!(session.connection().phase, LinkPhase::Connected);
    }

    #[test]
    fn saved_course_missing_or_unbuildable_falls_back_to_penguin_pass() {
        let dir = std::env::temp_dir().join(format!(
            "pp-session-course-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).expect("temp");
        let path = dir.join("old.toml");
        std::fs::write(&path, "ftp_w = 210\ntv_dashboard_enabled = false\n").expect("write");
        let loaded = crate::Settings::load(&path);
        assert!(loaded.warning.is_none(), "{:?}", loaded.warning);
        assert_eq!(loaded.settings.course, CourseChoice::PenguinPass);
        assert_eq!(loaded.settings.ftp_w, 210);
        let mut session = Session::new(loaded.settings);
        assert_eq!(session.course_choice(), &CourseChoice::PenguinPass);
        assert_eq!(session.course_facts().name, "Penguin Pass");
        assert!(
            !session
                .drain_events()
                .iter()
                .any(|event| { matches!(event, RideEvent::CourseUnavailable { .. }) })
        );

        let mut settings = offline();
        settings.ftp_w = 240;
        settings.course = CourseChoice::Generated {
            terrain: TerrainPreset::Desert,
            distance_km: 1,
            seed: 7,
        };
        let mut session = Session::new(settings);
        assert_eq!(session.course_choice(), &CourseChoice::PenguinPass);
        assert_eq!(session.settings().ftp_w, 240);
        assert_eq!(session.course_facts().name, "Penguin Pass");
        assert!(session.drain_events().iter().any(|event| {
            matches!(event, RideEvent::CourseUnavailable { detail } if detail.contains("could not build"))
        }));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn catalog_is_fixed_cached_and_reroll_changes_one_seed() {
        let mut session = Session::new(offline());
        let started = Instant::now();
        let catalog = session.course_catalog();
        eprintln!("course_catalog first call {:?}", started.elapsed());
        assert_eq!(catalog.len(), 9);
        assert_eq!(catalog[0].name, "Penguin Pass");
        assert_eq!(catalog[0].theme, "alpine");
        assert!(matches!(catalog[0].choice, CourseChoice::PenguinPass));
        assert_eq!(catalog[1].name, "Rolling 5 km");
        assert_eq!(catalog[2].name, "Rolling 10 km");
        assert_eq!(catalog[3].name, "Mountain 10 km");
        assert_eq!(catalog[3].theme, "alpine");
        assert_eq!(catalog[4].name, "Desert 10 km");
        assert_eq!(catalog[4].theme, "desert");
        assert_eq!(catalog[5].name, "Flat 10 km");
        assert_eq!(catalog[5].theme, "alpine");
        assert_eq!(catalog[6].name, "Rolling 20 km");
        assert_eq!(catalog[7].name, "Desert 20 km");
        assert_eq!(catalog[7].theme, "desert");
        assert!((catalog[1].length_m - 5_000.0).abs() < 200.0);
        assert!((catalog[5].length_m - 10_000.0).abs() < 200.0);
        assert!(
            (catalog[6].length_m - 20_000.0).abs() < 1_000.0,
            "{}",
            catalog[6].length_m
        );
        assert!(
            (catalog[7].length_m - 20_000.0).abs() < 1_000.0,
            "{}",
            catalog[7].length_m
        );
        for option in catalog {
            assert!(option.profile.len() <= 64, "{}", option.profile.len());
            assert!(option.profile.len() >= 2, "{}", option.name);
        }
        let cached_at = Instant::now();
        let again = session.course_catalog();
        let cached = cached_at.elapsed();
        eprintln!("course_catalog cached call {cached:?}");
        assert!(cached < Duration::from_millis(50), "{cached:?}");
        assert_eq!(again[4].name, "Desert 10 km");

        assert!(session.reroll(0).is_none());
        assert!(session.reroll(8).is_none());
        assert!(session.reroll(9).is_none());
        let rolled = session.reroll(5).expect("new flat route");
        match &rolled.choice {
            CourseChoice::Generated {
                terrain,
                distance_km,
                seed,
            } => {
                assert_eq!(*terrain, TerrainPreset::Flat);
                assert_eq!(*distance_km, 10);
                assert_eq!(*seed, 2);
            }
            CourseChoice::PenguinPass => panic!("flat row should stay generated"),
            CourseChoice::LonelyRoad => panic!("flat row should stay generated"),
        }
        assert_eq!(session.course_catalog()[5].name, "Flat 10 km");
        assert_eq!(session.course_choice(), &CourseChoice::PenguinPass);
    }

    #[test]
    fn set_course_updates_the_dashboard_profile() {
        let mut session = Session::with_options(
            Settings::default(),
            SessionOptions {
                trainer_factory: Box::new(FailingFactory),
                dashboard_host: IpAddr::V4(Ipv4Addr::LOCALHOST),
                dashboard_port: Some(0),
            },
        );
        let status = wait_dashboard(&mut session, |status| {
            !status.dashboard_urls.is_empty() || status.dashboard_error.is_some()
        });
        assert_eq!(status.dashboard, DashboardRun::Running);
        let urls = status.dashboard_urls;
        let choice = CourseChoice::Generated {
            terrain: TerrainPreset::Rolling,
            distance_km: 5,
            seed: 3,
        };
        session.set_course(choice).expect("rolling 5 km");
        session.tick(0.0);
        let profile = fetch_course(&urls[0]);
        assert_eq!(profile.name, "Rolling 5 km");
        assert!((profile.length_m - 5_000.0).abs() < 200.0);
    }

    #[test]
    fn a_reversed_setting_opens_backward_and_set_course_keeps_it() {
        let mut settings = offline();
        settings.course_reversed = true;
        let mut session = Session::new(settings);
        assert!(session.course_reversed());
        assert!(session.course().is_reversed());
        session
            .set_course(CourseChoice::Generated {
                terrain: TerrainPreset::Rolling,
                distance_km: 5,
                seed: 3,
            })
            .expect("rolling 5 km");
        assert!(session.course().is_reversed());
        assert!(session.course_reversed());
        assert!(session.drain_events().iter().any(|event| {
            matches!(event, RideEvent::CourseSelected { name } if name == "Rolling 5 km (Reverse)")
        }));
    }

    #[test]
    fn set_course_reversed_mirrors_the_profile_and_splits_records() {
        let dir = std::env::temp_dir().join(format!(
            "pp-session-rev-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).expect("temp");
        let settings_path = dir.join("settings.toml");
        let records_path = dir.join("records.toml");
        let mut session = Session::new(offline());
        session.set_settings_path(&settings_path);
        assert!(!session.course_reversed());
        assert!(!session.course().is_reversed());
        session.set_course_reversed(false).expect("unchanged");
        assert!(
            !session
                .drain_events()
                .iter()
                .any(|event| matches!(event, RideEvent::CourseSelected { .. }))
        );

        session.set_sim_power(1_200);
        let _forward = finish_any_segment(&mut session, 900.0);
        let forward = session.course().clone();
        session.set_course_reversed(true).expect("reverse");
        assert!(session.course_reversed());
        assert!(session.course().is_reversed());
        let length = forward.length_m();
        let sample = length * 0.37;
        let mirrored = (length - sample).rem_euclid(length);
        let mirrored_gap =
            (session.course().elevation_at(sample) - forward.elevation_at(mirrored)).abs();
        assert!(
            mirrored_gap < 0.01_f32,
            "reversed elevation should match the mirrored forward point, gap {mirrored_gap}"
        );
        let same_gap = (session.course().elevation_at(sample) - forward.elevation_at(sample)).abs();
        assert!(
            same_gap > 0.05_f32,
            "the sample should move when the course turns around, gap {same_gap}"
        );
        assert!(session.drain_events().iter().any(|event| {
            matches!(event, RideEvent::CourseSelected { name } if name == "Penguin Pass (Reverse)")
        }));
        let loaded = crate::Settings::load(&settings_path);
        assert_eq!(loaded.warning, None, "{:?}", loaded.warning);
        assert!(loaded.settings.course_reversed);
        let records = std::fs::read_to_string(&records_path).expect("forward records");
        assert!(records.contains("key = \"penguin_pass\""), "{records}");
        assert!(
            !records.contains("penguin_pass:rev"),
            "reverse efforts stay out of the forward file: {records}"
        );
        assert!(
            session
                .play()
                .segments
                .iter()
                .all(|segment| segment.best_s.is_none()),
            "reverse should not inherit the forward best"
        );

        session.set_course_reversed(true).expect("still reversed");
        assert!(
            !session
                .drain_events()
                .iter()
                .any(|event| matches!(event, RideEvent::CourseSelected { .. }))
        );

        session.set_sim_power(1_200);
        let _backward = finish_any_segment(&mut session, 900.0);
        session.end_ride();
        let records = std::fs::read_to_string(&records_path).expect("both records");
        assert!(records.contains("key = \"penguin_pass\""), "{records}");
        assert!(records.contains("key = \"penguin_pass:rev\""), "{records}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cast_segment_reports_ahead_as_a_negative_delta() {
        let segment = crate::ActiveSegment {
            id: "climb".to_owned(),
            name: "Summit climb".to_owned(),
            elapsed_s: 12.5,
            fraction: 0.25,
            split_s: Some(1.5),
        };
        let status = super::cast_segment(&segment);
        assert_eq!(status.name, "Summit climb");
        assert!((status.elapsed_s - 12.5_f32).abs() < 1.0e-4_f32);
        assert!((status.progress - 0.25_f32).abs() < 1.0e-4_f32);
        let delta = status.delta_s.expect("ahead of the best");
        assert!((delta + 1.5_f32).abs() < 1.0e-3_f32, "{delta}");
    }

    #[test]
    fn cast_status_fills_the_dashboard_fields_and_streams() {
        let mut session = Session::with_options(
            Settings::default(),
            SessionOptions {
                trainer_factory: Box::new(FailingFactory),
                dashboard_host: IpAddr::V4(Ipv4Addr::LOCALHOST),
                dashboard_port: Some(0),
            },
        );
        let status = wait_dashboard(&mut session, |status| {
            !status.dashboard_urls.is_empty() || status.dashboard_error.is_some()
        });
        assert_eq!(status.dashboard, DashboardRun::Running);
        let urls = status.dashboard_urls;
        session.tick(0.0);
        let forward = fetch_course(&urls[0]);

        session.set_course_reversed(true).expect("reverse");
        session.tick(0.0);
        let backward = fetch_course(&urls[0]);
        assert_eq!(forward.length_m, backward.length_m);
        assert_ne!(forward.points, backward.points);
        assert!(session.course().is_reversed());

        session.select_workout(Some("endurance-20"));
        session.set_sim_power(180);
        session.tick(1.0);
        let cast = session.cast_status();
        assert!(cast.course_reversed);
        assert!(cast.fish_collected.is_none());
        assert!(cast.fish_combo.is_none());
        assert!(cast.active_powerup.is_none());
        let segment = cast.segment.expect("a lap or a climb is open");
        assert!(!segment.name.is_empty());
        assert!((0.0..=1.0).contains(&segment.progress));
        let workout = cast.workout.expect("endurance");
        assert_eq!(workout.name, "Endurance");
        assert_eq!(workout.block_label, "Warm up");
        assert!(workout.target_w > 0, "{}", workout.target_w);
        assert!(workout.remaining_s > 0);
        assert!(workout.erg_active);
        assert!((0.0..=1.0).contains(&workout.progress));

        let streamed = fetch_status(&urls[0]);
        assert!(streamed.course_reversed);
        assert_eq!(
            streamed.workout.as_ref().map(|item| item.name.as_str()),
            Some("Endurance")
        );
        assert_eq!(
            streamed.workout.as_ref().map(|item| item.erg_active),
            Some(true)
        );
        assert!(streamed.segment.is_some());

        session
            .set_course(CourseChoice::Generated {
                terrain: TerrainPreset::Flat,
                distance_km: 2,
                seed: 2,
            })
            .expect("flat");
        assert!(session.course().is_reversed());
        collect_one_fish(&mut session);
        let cast = session.cast_status();
        let fish = cast.fish_collected.expect("a fish was collected");
        assert!(fish > 0);
        let combo = cast.fish_combo.expect("the collect starts a combo");
        assert!(combo > 0);
        session.tick(0.0);
        let streamed = fetch_status(&urls[0]);
        assert_eq!(streamed.fish_collected, Some(fish));
        assert_eq!(streamed.fish_combo, Some(combo));
        assert!(streamed.course_reversed);

        let saving = ride_until_draft(&mut session);
        let cast = session.cast_status();
        assert_eq!(cast.draft_saving_pct, Some(saving));
        assert!(cast.fish_collected.is_some());
        session.tick(0.0);
        let streamed = fetch_status(&urls[0]);
        assert_eq!(streamed.draft_saving_pct, Some(saving));
        assert!(streamed.fish_collected.is_some());
        assert!(streamed.course_reversed);
        if let Some(power) = streamed.active_powerup {
            assert!(!power.name.is_empty());
            assert!(power.remaining_s >= 0.0);
        }
    }

    #[test]
    fn ride_trace_records_each_tick_until_the_ride_ends() {
        let dir = trace_dir("rows");
        let handle = Handle::new();
        handle.set_discovered(vec![neo()]);
        let mut session = session_with(offline(), &handle);
        session.set_trace_dir(Some(dir.clone()));
        session.hold_at_start();
        session.tick(0.1);

        session.begin_ride();
        session.connect("neo-1");
        handle.set_state(ConnectionState::Connected);
        let first = TrainerSample {
            power_w: Some(100),
            cadence_rpm: Some(80.0),
            speed_kph: Some(15.0),
            heart_rate_bpm: None,
        };
        handle.set_sample(Some(first));
        session.tick(0.2);
        session.tick(0.2);
        handle.set_sample(Some(TrainerSample {
            power_w: Some(140),
            ..first
        }));
        session.tick(0.2);
        session.end_ride();
        session.tick(0.2);
        session.tick(0.2);
        drop(session);

        let text = std::fs::read_to_string(dir.join("ride.csv")).expect("trace file");
        let mut lines = text.lines();
        assert_eq!(
            lines.next(),
            Some(concat!(
                "unix_ms,clock_s,dt_s,phase,source,link,power_w,cadence_rpm,",
                "trainer_speed_kph,heart_rate_bpm,sample_changed,sample_age_ms,",
                "game_speed_kph,game_accel_mps2,grade_pct,grade_sent_pct,distance_m,power_3s_w"
            ))
        );
        let rows: Vec<&str> = lines.collect();
        assert_eq!(rows.len(), 4, "{text}");
        let mut previous_ms = 0u64;
        let changed: Vec<&str> = rows
            .iter()
            .map(|row| {
                let cells: Vec<&str> = row.split(',').collect();
                assert_eq!(cells.len(), 18, "{row}");
                assert!(
                    cells[11].is_empty(),
                    "sample age is not on the sample: {row}"
                );
                let unix_ms: u64 = cells[0].parse().expect("unix_ms");
                assert!(unix_ms >= previous_ms, "{unix_ms} then {previous_ms}");
                previous_ms = unix_ms;
                cells[10]
            })
            .collect();
        assert_eq!(rows[0].split(',').nth(3), Some("ready"));
        assert_eq!(rows[0].split(',').nth(4), Some("sim"));
        assert_eq!(rows[0].split(',').nth(8), Some(""));
        assert_eq!(rows[1].split(',').nth(3), Some("riding"));
        assert_eq!(rows[1].split(',').nth(4), Some("trainer"));
        assert_eq!(rows[1].split(',').nth(5), Some("connected"));
        assert_eq!(rows[1].split(',').nth(6), Some("100.0"));
        assert_eq!(rows[1].split(',').nth(7), Some("80.0"));
        assert_eq!(rows[1].split(',').nth(8), Some("15.000"));
        assert_eq!(rows[1].split(',').nth(9), Some(""));
        assert_eq!(changed, ["1", "1", "0", "1"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn ride_trace_is_absent_without_a_directory_and_a_bad_directory_still_rides() {
        let quiet = trace_dir("quiet");
        let mut session = Session::new(offline());
        session.set_trace_dir(None);
        session.set_sim_power(30);
        assert_eq!(session.tick(0.2).power_w, 30.0);
        assert!(!quiet.join("ride.csv").exists());
        drop(session);

        let blocked = quiet.join("not-a-directory");
        std::fs::write(&blocked, b"x").expect("blocker");
        let mut session = Session::new(offline());
        session.set_trace_dir(Some(blocked));
        session.set_sim_power(40);
        assert_eq!(session.tick(0.2).power_w, 40.0);
        assert!(!quiet.join("ride.csv").exists());
        let _ = std::fs::remove_dir_all(&quiet);
    }

    #[test]
    fn heart_rate_strap_wins_falls_back_and_stays_off_the_trainer() {
        let trainer = Handle::new();
        trainer.set_discovered(vec![neo()]);
        let strap = strap_sim();
        strap.lock().set_discovered(vec![hrm()]);
        strap.lock().set_bpm(Some(150));
        let mut session = session_with(offline(), &trainer);
        session.set_heart_rate_factory(Some(Box::new(strap.factory())));
        assert_eq!(session.heart_rate_state(), ConnectionState::Scanning);
        assert!(
            session
                .heart_rate_entries()
                .iter()
                .any(|row| row.id == "hrm-1")
        );
        assert!(session.drain_events().is_empty());

        session.connect_heart_rate("hrm-1");
        assert_eq!(session.heart_rate_state(), ConnectionState::Connected);
        assert_eq!(session.heart_rate_name().as_deref(), Some("HRM-Dual"));
        assert_eq!(session.settings().heart_rate_id.as_deref(), Some("hrm-1"));
        let connected = session.drain_events();
        assert_eq!(
            connected
                .iter()
                .filter(|event| matches!(event, RideEvent::HeartRateConnected { .. }))
                .count(),
            1
        );

        session.connect("neo-1");
        trainer.set_state(ConnectionState::Connected);
        trainer.set_sample(Some(TrainerSample {
            power_w: Some(180),
            cadence_rpm: Some(85.0),
            heart_rate_bpm: Some(100),
            ..TrainerSample::default()
        }));
        let frame = session.tick(0.2);
        assert_eq!(frame.heart_rate_bpm, Some(150));
        assert_eq!(session.readout().heart_rate_bpm, Some(150));
        assert_eq!(session.connection().phase, LinkPhase::Connected);
        session.tick(0.2);
        assert!(
            !session
                .drain_events()
                .iter()
                .any(|event| matches!(event, RideEvent::HeartRateConnected { .. }))
        );

        strap.lock().set_bpm(None);
        let quiet = session.tick(0.2);
        assert_eq!(quiet.heart_rate_bpm, Some(100));
        assert_eq!(session.connection().phase, LinkPhase::Connected);

        let grades_before = trainer.grades().len();
        strap.lock().drop_link();
        session.tick(0.2);
        let lost = session.drain_events();
        assert_eq!(
            lost.iter()
                .filter(|event| matches!(event, RideEvent::HeartRateLost { .. }))
                .count(),
            1
        );
        session.tick(0.2);
        assert!(
            !session
                .drain_events()
                .iter()
                .any(|event| matches!(event, RideEvent::HeartRateLost { .. }))
        );
        assert!(trainer.grades().len() > grades_before);
        assert_eq!(session.connection().phase, LinkPhase::Connected);

        session.forget_heart_rate();
        assert_eq!(session.settings().heart_rate_id, None);
        assert_eq!(session.settings().heart_rate_name, None);
        assert_eq!(session.heart_rate_state(), ConnectionState::Disconnected);
        assert_eq!(session.connection().phase, LinkPhase::Connected);
    }

    #[test]
    fn remembered_strap_connects_at_start_and_an_unremembered_one_does_not() {
        let remembered = strap_sim();
        remembered.lock().set_discovered(vec![hrm(), other_strap()]);
        let mut settings = offline();
        settings.heart_rate_id = Some("hrm-1".to_owned());
        settings.heart_rate_name = Some("HRM-Dual".to_owned());
        let mut session = Session::new(settings);
        session.set_heart_rate_factory(Some(Box::new(remembered.factory())));
        assert_eq!(session.heart_rate_state(), ConnectionState::Connected);
        assert_eq!(session.heart_rate_name().as_deref(), Some("HRM-Dual"));
        assert!(
            session
                .heart_rate_entries()
                .iter()
                .any(|row| row.id == "hrm-1" && row.remembered && row.bars == 4)
        );
        assert_eq!(
            session
                .drain_events()
                .iter()
                .filter(|event| matches!(event, RideEvent::HeartRateConnected { .. }))
                .count(),
            1
        );

        let nearby = strap_sim();
        nearby.lock().set_discovered(vec![hrm()]);
        let mut session = Session::new(offline());
        session.set_heart_rate_factory(Some(Box::new(nearby.factory())));
        assert_eq!(session.heart_rate_state(), ConnectionState::Scanning);
        assert!(session.drain_events().is_empty());
        assert!(!session.heart_rate_entries()[0].remembered);

        let mut bare = Session::new(offline());
        assert_eq!(bare.heart_rate_state(), ConnectionState::Disconnected);
        assert!(bare.heart_rate_entries().is_empty());
        bare.connect_heart_rate("hrm-1");
        assert_eq!(bare.heart_rate_state(), ConnectionState::Disconnected);
        assert_eq!(bare.settings().heart_rate_id, None);
    }

    fn strap_sim() -> HeartSim {
        HeartSim(Arc::new(Mutex::new(SimHeartRate::new())))
    }

    fn hrm() -> DiscoveredSensor {
        DiscoveredSensor {
            id: "hrm-1".to_owned(),
            name: "HRM-Dual".to_owned(),
            rssi_dbm: Some(-55),
        }
    }

    fn other_strap() -> DiscoveredSensor {
        DiscoveredSensor {
            id: "other".to_owned(),
            name: "Tickr".to_owned(),
            rssi_dbm: Some(-82),
        }
    }

    struct HeartSim(Arc<Mutex<SimHeartRate>>);

    impl HeartSim {
        fn lock(&self) -> std::sync::MutexGuard<'_, SimHeartRate> {
            self.0.lock().unwrap_or_else(|err| err.into_inner())
        }

        fn factory(&self) -> HeartSimFactory {
            HeartSimFactory(Arc::clone(&self.0))
        }
    }

    struct HeartSimFactory(Arc<Mutex<SimHeartRate>>);

    struct HeartSimLink(Arc<Mutex<SimHeartRate>>);

    impl HeartRateFactory for HeartSimFactory {
        fn spawn(&mut self) -> Result<Box<dyn HeartRateLink>, String> {
            Ok(Box::new(HeartSimLink(Arc::clone(&self.0))))
        }
    }

    impl HeartRateLink for HeartSimLink {
        fn state(&self) -> ConnectionState {
            self.0.lock().unwrap_or_else(|err| err.into_inner()).state()
        }

        fn discovered(&self) -> Vec<DiscoveredSensor> {
            self.0
                .lock()
                .unwrap_or_else(|err| err.into_inner())
                .discovered()
        }

        fn connect(&mut self, id_or_name: &str) {
            self.0
                .lock()
                .unwrap_or_else(|err| err.into_inner())
                .connect(id_or_name);
        }

        fn disconnect(&mut self) {
            self.0
                .lock()
                .unwrap_or_else(|err| err.into_inner())
                .disconnect();
        }

        fn bpm(&self) -> Option<u16> {
            self.0.lock().unwrap_or_else(|err| err.into_inner()).bpm()
        }

        fn name(&self) -> Option<String> {
            self.0.lock().unwrap_or_else(|err| err.into_inner()).name()
        }
    }

    #[test]
    fn a_long_ride_is_saved_and_a_short_one_is_not() {
        let dir = trace_dir("saved");
        let trainer = Handle::new();
        trainer.set_discovered(vec![neo()]);
        let strap = strap_sim();
        strap.lock().set_discovered(vec![hrm()]);
        strap.lock().set_bpm(Some(142));
        let mut session = session_with(offline(), &trainer);
        session.set_rides_dir(Some(dir.clone()));
        session.set_heart_rate_factory(Some(Box::new(strap.factory())));
        session.connect_heart_rate("hrm-1");
        session.connect("neo-1");
        trainer.set_state(ConnectionState::Connected);
        trainer.set_sample(Some(TrainerSample {
            power_w: Some(250),
            cadence_rpm: Some(90.0),
            heart_rate_bpm: Some(100),
            ..TrainerSample::default()
        }));
        for _ in 0..75 {
            session.tick(1.0);
        }
        session.end_ride();
        let saved = session
            .drain_events()
            .into_iter()
            .find_map(|event| match event {
                RideEvent::RideSaved { path } => Some(path),
                RideEvent::RideNotSaved { reason } => panic!("ride not saved: {reason}"),
                _ => None,
            })
            .expect("saved event");
        assert_eq!(session.last_saved_ride().as_ref(), Some(&saved));
        assert!(saved.starts_with(&dir), "{}", saved.display());
        let records = fit_records(&saved);
        assert!(records.len() >= 60, "{}", records.len());
        let heart = fit_number(fit_field(&records[10], "heart_rate")).expect("heart");
        assert!(
            (heart - 142.0).abs() < 1.0,
            "strap heart {heart}, not the trainer's 100"
        );
        let power = fit_number(fit_field(&records[10], "power")).expect("power");
        assert!(power > 200.0, "{power}");
        drop(session);
        assert_eq!(fit_files(&dir).len(), 1);

        let short = trace_dir("short");
        let mut session = Session::new(offline());
        session.set_rides_dir(Some(short.clone()));
        session.set_sim_power(200);
        for _ in 0..20 {
            session.tick(1.0);
        }
        session.end_ride();
        let reason = session
            .drain_events()
            .into_iter()
            .find_map(|event| match event {
                RideEvent::RideNotSaved { reason } => Some(reason),
                RideEvent::RideSaved { path } => panic!("short ride wrote {}", path.display()),
                _ => None,
            });
        assert_eq!(reason.as_deref(), Some("Ride too short to save"));
        assert!(fit_files(&short).is_empty());
        assert!(session.last_saved_ride().is_none());
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&short);
    }

    #[test]
    fn ready_ticks_are_not_recorded_and_restart_starts_a_second_file() {
        let dir = trace_dir("two");
        let mut session = Session::new(offline());
        session.set_rides_dir(Some(dir.clone()));
        session.hold_at_start();
        session.set_sim_power(220);
        for _ in 0..30 {
            session.tick(1.0);
        }
        assert!(
            !session
                .drain_events()
                .iter()
                .any(|event| matches!(event, RideEvent::RideSaved { .. }))
        );
        assert!(fit_files(&dir).is_empty());

        session.begin_ride();
        for _ in 0..75 {
            session.tick(1.0);
        }
        session.restart();
        assert_eq!(fit_files(&dir).len(), 1);
        let first = session.last_saved_ride().expect("first file");
        for _ in 0..75 {
            session.tick(1.0);
        }
        session.end_ride();
        assert_eq!(fit_files(&dir).len(), 2);
        let second = session.last_saved_ride().expect("second file");
        assert_ne!(first, second);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_unwritable_rides_directory_does_not_stop_the_ride_and_drop_writes() {
        let dir = trace_dir("drop");
        let blocker = dir.join("not-a-directory");
        std::fs::write(&blocker, b"x").expect("blocker");
        let mut session = Session::new(offline());
        session.set_rides_dir(Some(blocker));
        session.set_sim_power(220);
        for _ in 0..75 {
            session.tick(1.0);
        }
        session.end_ride();
        let mut reason = None;
        for event in session.drain_events() {
            if let RideEvent::RideNotSaved { reason: text } = event {
                reason = Some(text);
                break;
            }
        }
        let reason = reason.expect("not saved");
        assert!(reason.starts_with("Could not save the ride:"), "{reason}");
        session.restart();
        assert_eq!(session.tick(0.2).power_w, 220.0);
        drop(session);

        let kept = trace_dir("quit");
        {
            let mut session = Session::new(offline());
            session.set_rides_dir(Some(kept.clone()));
            session.set_sim_power(220);
            for _ in 0..75 {
                session.tick(1.0);
            }
        }
        assert_eq!(fit_files(&kept).len(), 1, "drop mid-ride writes the file");
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&kept);
    }

    #[test]
    fn one_hour_synthetic_ride_write_time() {
        let dir = trace_dir("hour");
        let mut recorder = pp_record::Recorder::new(pp_record::RideMeta {
            started_at: std::time::SystemTime::UNIX_EPOCH
                + std::time::Duration::from_secs(1_700_000_000),
            course_name: "Hour".to_owned(),
            rider_mass_kg: 75.0,
            bike_mass_kg: 9.0,
            ftp_w: 200,
            trainer_name: Some("Simulated rider".to_owned()),
            game_version: "0.1.0".to_owned(),
            utc_offset_s: 0,
        });
        for second in 0..3600 {
            recorder.push(pp_record::RideSample {
                elapsed_s: f64::from(second) + 0.5,
                power_w: Some(200.0),
                cadence_rpm: Some(85.0),
                heart_rate_bpm: Some(140),
                speed_mps: 8.0,
                distance_m: f64::from(second) * 8.0,
                altitude_m: 12.0,
                grade_pct: 1.0,
                lap: 1,
            });
        }
        let activity = recorder.finish();
        let started = std::time::Instant::now();
        let outcome = activity.write(&dir).expect("write hour");
        eprintln!("RIDE_WRITE_1H {:?}", started.elapsed());
        assert!(matches!(outcome, pp_record::WriteOutcome::Written(_)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn set_utc_offset_minutes_changes_the_file_name_only() {
        let utc_dir = trace_dir("utc-name");
        let local_dir = trace_dir("local-name");
        let utc = saved_name(&utc_dir, None);
        let local = saved_name(&local_dir, Some(-480));
        let delta = name_minutes(&local) - name_minutes(&utc);
        assert!(
            (-481..=-479).contains(&delta),
            "local {local} utc {utc} delta {delta}"
        );
        let _ = std::fs::remove_dir_all(&utc_dir);
        let _ = std::fs::remove_dir_all(&local_dir);
    }

    fn saved_name(dir: &std::path::Path, offset_minutes: Option<i32>) -> String {
        let mut session = Session::new(offline());
        session.set_rides_dir(Some(dir.to_path_buf()));
        if let Some(minutes) = offset_minutes {
            session.set_utc_offset_minutes(minutes);
            session.hold_at_start();
            session.begin_ride();
        }
        session.set_sim_power(220);
        for _ in 0..75 {
            session.tick(1.0);
        }
        session.end_ride();
        session
            .last_saved_ride()
            .expect("saved")
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned()
    }

    fn name_minutes(file_name: &str) -> i64 {
        let stamp = &file_name[..15];
        let year: i32 = stamp[0..4].parse().unwrap();
        let month: u32 = stamp[5..7].parse().unwrap();
        let day: u32 = stamp[8..10].parse().unwrap();
        let hour: u32 = stamp[11..13].parse().unwrap();
        let minute: u32 = stamp[13..15].parse().unwrap();
        let year = if month <= 2 { year - 1 } else { year };
        let era = if year >= 0 { year } else { year - 399 } / 400;
        let year_of_era = (year - era * 400) as u64;
        let month_prime = if month > 2 { month - 3 } else { month + 9 };
        let day_of_year = (153 * month_prime + 2) / 5 + day - 1;
        let day_of_era =
            year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year as u64;
        let days = era as i64 * 146_097 + day_of_era as i64 - 719_468;
        days * 1_440 + i64::from(hour) * 60 + i64::from(minute)
    }

    fn fit_files(dir: &std::path::Path) -> Vec<PathBuf> {
        let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
            .expect("rides dir")
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "fit"))
            .collect();
        files.sort();
        files
    }

    fn fit_records(path: &std::path::Path) -> Vec<fitparser::FitDataRecord> {
        let mut file = std::fs::File::open(path).expect("open fit");
        let options = std::collections::HashSet::new();
        fitparser::de::from_reader_with_options(&mut file, &options)
            .expect("parse fit")
            .into_iter()
            .filter(|message| message.kind() == fitparser::profile::MesgNum::Record)
            .collect()
    }

    fn fit_field<'a>(
        message: &'a fitparser::FitDataRecord,
        name: &str,
    ) -> Option<&'a fitparser::Value> {
        message
            .fields()
            .iter()
            .find(|field| field.name() == name)
            .map(|field| field.value())
    }

    fn fit_number(value: Option<&fitparser::Value>) -> Option<f64> {
        use fitparser::Value;
        match value? {
            Value::UInt8(v) | Value::UInt8z(v) | Value::Byte(v) | Value::Enum(v) => {
                Some(f64::from(*v))
            }
            Value::UInt16(v) | Value::UInt16z(v) => Some(f64::from(*v)),
            Value::UInt32(v) | Value::UInt32z(v) => Some(f64::from(*v)),
            Value::Float32(v) => Some(f64::from(*v)),
            Value::Float64(v) => Some(*v),
            _ => None,
        }
    }

    fn trace_dir(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "pp-session-trace-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        std::fs::create_dir_all(&path).expect("trace dir");
        path
    }

    fn free_port() -> u16 {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind");
        let port = listener.local_addr().expect("addr").port();
        assert!(port >= 1024, "ephemeral port {port}");
        port
    }

    fn finish_any_segment(session: &mut Session, max_s: f64) -> String {
        let mut elapsed = 0.0;
        while elapsed < max_s {
            session.tick(0.5);
            elapsed += 0.5;
            for event in session.drain_events() {
                if let RideEvent::SegmentFinished { name, .. } = event {
                    return name;
                }
            }
        }
        panic!(
            "no segment finished by {:.0} m",
            session.telemetry().distance_m
        );
    }

    fn collect_one_fish(session: &mut Session) {
        session.set_sim_power(700);
        let mut elapsed = 0.0;
        let fish = loop {
            if let Some(fish) = session.play().fish_ahead.into_iter().next() {
                break fish;
            }
            session.tick(0.15);
            elapsed += 0.15;
            if elapsed > 120.0 {
                panic!("no fish ahead by {:.0} m", session.telemetry().distance_m);
            }
        };
        let target_m = fish.total_distance_m;
        let slot = f32::from(fish.slot);
        while session.telemetry().distance_m + 3.0 < target_m && elapsed < 200.0 {
            session.tick(0.15);
            elapsed += 0.15;
            let _ = session.drain_events();
        }
        session.set_lane_position(slot);
        while session.telemetry().distance_m < target_m + 2.0 && elapsed < 220.0 {
            session.set_lane_position(slot);
            session.tick(0.1);
            elapsed += 0.1;
            let _ = session.drain_events();
        }
        assert!(
            session.play().fish_count > 0,
            "fish at {target_m:.0} m was not collected, now {:.0} m",
            session.telemetry().distance_m
        );
    }

    fn ride_until_draft(session: &mut Session) -> u8 {
        session.set_sim_power(220);
        let mut closest = f64::MAX;
        for _ in 0..6_000 {
            let me = session.telemetry().distance_m;
            if let Some(bot) = session
                .play()
                .bots
                .iter()
                .filter(|bot| bot.total_distance_m > me)
                .min_by(|left, right| {
                    left.total_distance_m
                        .partial_cmp(&right.total_distance_m)
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
            {
                closest = closest.min(bot.total_distance_m - me);
                session.set_lane_position(bot.slot);
            }
            session.tick(0.05);
            let saving = session.play().draft_saving_pct;
            if saving > 0 {
                return saving;
            }
        }
        panic!("never sat in a draft, closest gap {closest:.1} m");
    }

    fn fetch_telemetry(base: &str) -> Telemetry {
        let url = format!("{base}api/telemetry");
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(5))
            .no_proxy()
            .build()
            .expect("client");
        let mut last = String::new();
        for _ in 0..25 {
            match client.get(&url).send() {
                Ok(response) => match response.json::<Telemetry>() {
                    Ok(telemetry) => return telemetry,
                    Err(err) => last = err.to_string(),
                },
                Err(err) => last = err.to_string(),
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("GET {url} failed: {last}");
    }

    fn fetch_course(base: &str) -> pp_cast::CourseProfile {
        let url = format!("{base}api/course");
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(5))
            .no_proxy()
            .build()
            .expect("client");
        let mut last = String::new();
        for _ in 0..25 {
            match client.get(&url).send() {
                Ok(response) => match response.json::<pp_cast::CourseProfile>() {
                    Ok(course) => return course,
                    Err(err) => last = err.to_string(),
                },
                Err(err) => last = err.to_string(),
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("GET {url} failed: {last}");
    }

    fn fetch_status(base: &str) -> pp_cast::RideStatus {
        let url = format!("{base}api/status");
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(5))
            .no_proxy()
            .build()
            .expect("client");
        let mut last = String::new();
        for _ in 0..25 {
            match client.get(&url).send() {
                Ok(response) => match response.json::<pp_cast::RideStatus>() {
                    Ok(status) => return status,
                    Err(err) => last = err.to_string(),
                },
                Err(err) => last = err.to_string(),
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("GET {url} failed: {last}");
    }
}
