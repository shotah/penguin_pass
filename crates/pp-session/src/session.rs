//! The ride: physics, trainer, and the TV dashboard on one frame tick.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;

use pp_cast::{CastConfig, CastServer, CourseProfile, ProfilePoint as CastPoint};
use pp_core::{Telemetry, TrainerCommand, TrainerSample};
use pp_course::{Course, Ride};
use pp_trainer::{ConnectionState, DiscoveredTrainer, SessionMode, SimTrainer, Trainer};

use crate::connection::{self, ConnectionStatus, LinkPhase, ResistanceMode};
use crate::events::{EventQueue, RideEvent};
use crate::link::{BleTrainerFactory, TrainerFactory, TrainerLink};
use crate::readout::{LiveReadout, PowerWindow};
use crate::settings::{LoadedSettings, Settings};
use crate::stats::RideStats;

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
}

struct SeenTrainer {
    id: String,
    name: String,
    rssi_dbm: Option<i16>,
    last_seen_s: f64,
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
}

impl Session {
    /// Start on Penguin Pass with the simulated rider and `settings`.
    ///
    /// When the dashboard is enabled this binds it. A bind failure is stored
    /// on [`SessionStatus::dashboard_error`] and does not fail construction.
    /// Bluetooth is not opened until [`Session::start_scan`] or
    /// [`Session::connect`].
    #[must_use]
    pub fn new(settings: Settings) -> Self {
        Self::with_options(settings, SessionOptions::default())
    }

    /// [`Session::new`] with a caller-supplied trainer factory and dashboard bind.
    #[must_use]
    pub fn with_options(settings: Settings, options: SessionOptions) -> Self {
        let settings = settings.normalized();
        let mut sim = SimTrainer::new("Simulated rider");
        sim.set_power(0);
        sim.set_cadence(0.0);
        let ride = Ride::new(Course::penguin_pass(), settings.rider_profile());
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
        };
        session.start_dashboard();
        session
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
        self.reap_drops();
        self.poll_dashboard();
        if self.ride_ended {
            let frame = self.ride.telemetry();
            self.publish_dashboard(frame);
            self.note_dashboard_event();
            return frame;
        }
        self.poll_auto();
        self.observe_link();
        let dt = accepted_dt(dt_s);
        self.clock_s += dt;
        let observed = self.observe_power();
        let telemetry = self.ride.step(dt_s, &observed.physics);
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
        self.note_link_events();
        self.refresh_pairing();
        self.send_grade(telemetry.grade_pct);
        self.publish_dashboard(telemetry);
        self.note_dashboard_event();
        telemetry
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
        match self.factory.spawn_mode(self.trainer_mode()) {
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
        self.settings = new;
        if rider_changed {
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

    /// Penguin Pass course, for the 3D world and the HUD profile.
    #[must_use]
    pub fn course(&self) -> &Course {
        self.ride.course()
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
        self.ride_ended = false;
        self.stats.clear();
        self.power.clear();
        self.events.clear();
        self.clock_s = 0.0;
        self.rebuild_ride();
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
        self.stats.summary(&self.ride.telemetry())
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
        self.ride_ended = true;
        self.send_flat_grade();
    }

    /// Whether [`Session::end_ride`] is in effect.
    #[must_use]
    pub fn ride_ended(&self) -> bool {
        self.ride_ended
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
            return;
        }
        let _ = self.sim.send(command);
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
            return Observation {
                physics: sample,
                power_w: sample.power_w.map(f32::from),
                cadence_rpm: sample.cadence_rpm,
                heart_rate_bpm: sample.heart_rate_bpm,
            };
        }
        let down = Observation {
            physics: zero_power(),
            power_w: None,
            cadence_rpm: None,
            heart_rate_bpm: None,
        };
        let Some(trainer) = self.trainer.as_ref() else {
            return down;
        };
        if !matches!(trainer.state(), ConnectionState::Connected) {
            return down;
        }
        let Some(sample) = trainer.latest_sample() else {
            return down;
        };
        Observation {
            physics: TrainerSample {
                power_w: Some(sample.power_w.unwrap_or(0)),
                cadence_rpm: sample.cadence_rpm,
                speed_kph: sample.speed_kph,
                heart_rate_bpm: sample.heart_rate_bpm,
            },
            power_w: sample.power_w.map(f32::from),
            cadence_rpm: sample.cadence_rpm,
            heart_rate_bpm: sample.heart_rate_bpm,
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
                } else {
                    self.seen.push(SeenTrainer {
                        id: found.id,
                        name: found.name,
                        rssi_dbm: found.rssi_dbm,
                        last_seen_s: self.clock_s,
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
    }

    fn trainer_mode(&self) -> SessionMode {
        if self.settings.control_trainer {
            SessionMode::Control
        } else {
            SessionMode::ReadOnly
        }
    }

    fn resistance_phrase(&self) -> &'static str {
        if self.settings.control_trainer {
            "resistance follows the road"
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

    fn publish_dashboard(&self, telemetry: Telemetry) {
        self.poll_dashboard();
        let status = self.cast_status();
        let dash = lock_dash(&self.dash);
        if dash.phase == DashboardRun::Running
            && let Some(server) = &dash.server
        {
            server.publish(telemetry);
            server.publish_status(status);
        }
    }

    fn cast_status(&self) -> pp_cast::RideStatus {
        let trainer = self.using_trainer;
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
        }
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
    fn urls(&self) -> Vec<String>;
}

impl DashboardHost for CastServer {
    fn publish(&self, telemetry: Telemetry) {
        CastServer::publish(self, telemetry);
    }

    fn publish_status(&self, status: pp_cast::RideStatus) {
        CastServer::publish_status(self, status);
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
    use pp_core::{RiderProfile, TrainerCommand, TrainerSample};
    use pp_course::Ride;
    use pp_trainer::{ConnectionState, DiscoveredTrainer, Trainer, TrainerError};
    use std::net::{Ipv4Addr, TcpListener};
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

        fn sends(&self) -> u32 {
            lock(&self.0).sends
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
            if let TrainerCommand::SetGrade { grade_pct } = command {
                state.grades.push(grade_pct);
            }
            Ok(())
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
            "Tacx Neo 2T: connected, resistance follows the road (control not permitted)"
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
            delay: Duration::from_millis(250),
        });

        let mut settings = session.settings().clone();
        settings.tv_dashboard_enabled = true;
        let started = Instant::now();
        session.apply_settings(settings);
        let elapsed = started.elapsed();
        assert!(
            elapsed < Duration::from_millis(40),
            "apply_settings took {elapsed:?}"
        );
        let status = session.status();
        assert_eq!(status.dashboard, DashboardRun::Starting);
        assert!(status.dashboard_urls.is_empty());
        assert!(status.dashboard_error.is_none());

        let started = Instant::now();
        session.tick(0.016);
        let elapsed = started.elapsed();
        assert!(elapsed < Duration::from_millis(40), "tick took {elapsed:?}");

        let mut settings = session.settings().clone();
        settings.tv_dashboard_enabled = false;
        let started = Instant::now();
        session.apply_settings(settings);
        let elapsed = started.elapsed();
        assert!(
            elapsed < Duration::from_millis(40),
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
            },
            DiscoveredTrainer {
                id: "a2".to_owned(),
                name: "Alpha".to_owned(),
                rssi_dbm: Some(-65),
            },
            DiscoveredTrainer {
                id: "neo-1".to_owned(),
                name: "Tacx Neo 2T".to_owned(),
                rssi_dbm: Some(-65),
            },
            DiscoveredTrainer {
                id: "mystery".to_owned(),
                name: "Hidden".to_owned(),
                rssi_dbm: None,
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
        while session.telemetry().lap < 2 && guard < 800 {
            session.tick(1.0);
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
        assert!(session.drain_events().iter().any(|event| matches!(
            event,
            RideEvent::LapCompleted { lap: 1, time_s } if *time_s > 1.0
        )));
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
            per_frame < Duration::from_millis(1),
            "tick plus HUD calls took {per_frame:?}"
        );
    }

    fn free_port() -> u16 {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind");
        let port = listener.local_addr().expect("addr").port();
        assert!(port >= 1024, "ephemeral port {port}");
        port
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
