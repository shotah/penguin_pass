//! FTMS session state machine.
//!
//! The machine is synchronous. Bluetooth I/O is someone else's job: this
//! module emits [`OutAction`]s and consumes [`InEvent`]s. The BlueZ thread
//! performs the actions. Tests perform them on [`tests::FakePeripheral`],
//! which answers control-point writes the way a trainer does.
//!
//! Time is a millisecond counter the caller passes in, so tests never sleep.

use pp_core::TrainerCommand;
#[cfg(test)]
use pp_core::TrainerSample;
use pp_ftms::{
    CYCLING_POWER_MEASUREMENT_UUID, ControlPointRequest, ControlPointResponse, ControlResult,
    CyclingPowerMeasurement, FEC_NOTIFY_UUID, FITNESS_MACHINE_CONTROL_POINT_UUID,
    FITNESS_MACHINE_STATUS_UUID, FecChecksum, FecData, FecMessage, HEART_RATE_MEASUREMENT_UUID,
    HeartRateMeasurement, INDOOR_BIKE_DATA_UUID, IndoorBikeData, IndoorBikeSimulation,
    OPCODE_REQUEST_CONTROL, OPCODE_SET_INDOOR_BIKE_SIMULATION, OPCODE_SET_TARGET_POWER,
    OPCODE_START_OR_RESUME, TrackResistance, UserConfiguration, crank_cadence_rpm,
};
use uuid::Uuid;

use crate::ConnectionState;
use crate::config::{SessionMode, TrainerConfig, duration_ms};
#[cfg(test)]
use crate::sample_is_fresh;
use crate::trace::{BleTrace, TraceRow, hex_bytes};

/// What the peripheral can do, after service discovery.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DeviceProfile {
    pub indoor_bike: bool,
    pub cycling_power: bool,
    pub heart_rate: bool,
    pub control_point: bool,
    /// Tacx FE-C notify characteristic (`6e40fec2-…`).
    pub fec_notify: bool,
    /// Tacx FE-C write characteristic (`6e40fec3-…`).
    pub fec_write: bool,
}

/// Something that happened on the link, or a command from the game.
#[derive(Debug)]
pub(crate) enum InEvent {
    /// Service discovery finished.
    Ready(DeviceProfile),
    /// A notification or an indication.
    Value { uuid: Uuid, bytes: Vec<u8> },
    /// The link dropped.
    LinkLost,
    /// `connect` failed before the link was up.
    ConnectFailed { reason: String },
    /// A grade or power command from the game.
    Command(TrainerCommand),
}

/// Work the link owner must perform. Order is significant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum OutAction {
    /// Enable notifications or indications on this characteristic.
    Subscribe(Uuid),
    /// Write the control point and wait for the response indication.
    WriteControl(Vec<u8>),
    /// Write one FE-C frame to the Tacx UART characteristic. No response is required.
    WriteFec(Vec<u8>),
    /// Drop the current link.
    Disconnect,
    /// The backoff elapsed. Open the link again.
    Reconnect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Idle,
    Connecting,
    Handshaking,
    Ready,
    /// Control was refused. Stay on the link and do not take it back by force.
    Held,
    Backoff,
    Stopped,
}

/// Which write path the connected trainer accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ControlWire {
    None,
    Ftms,
    Fec,
}

/// How long to wait for a trainer frame before the first FE-C write.
///
/// Notifications arrive several times a second. Waiting lets the first page
/// 55 and page 51 use the checksum the trainer itself just sent. After this,
/// with nothing received, the write uses the ANT XOR.
const FEC_CHECKSUM_HOLD_MS: u64 = 2_000;

#[derive(Debug, Clone, Copy)]
enum Pending {
    Grade(f32),
    Power(u16),
}

struct Awaiting {
    opcode: u8,
    deadline_ms: u64,
}

#[derive(Clone, Copy)]
struct Stamp<T> {
    value: T,
    at_ms: u64,
}

#[derive(Clone, Copy, Default)]
struct Reading {
    speed_kph: Option<Stamp<f32>>,
    cadence_rpm: Option<Stamp<f32>>,
    power_w: Option<Stamp<u16>>,
    heart_rate_bpm: Option<Stamp<u8>>,
}

/// One fresh field, for the BlueZ thread to stamp with wall-clock time.
#[derive(Clone, Copy)]
pub(crate) struct FieldClock<T> {
    pub value: T,
    pub at_ms: u64,
}

/// Latest value of each measurement, with the engine clock it arrived on.
#[derive(Clone, Copy, Default)]
pub(crate) struct FieldClocks {
    pub speed_kph: Option<FieldClock<f32>>,
    pub cadence_rpm: Option<FieldClock<f32>>,
    pub power_w: Option<FieldClock<u16>>,
    pub heart_rate_bpm: Option<FieldClock<u8>>,
}

/// Handshake, coalescing, and sample assembly for one trainer.
pub(crate) struct Engine {
    config: TrainerConfig,
    phase: Phase,
    state: ConnectionState,
    name: String,
    profile: Option<DeviceProfile>,
    reading: Reading,
    last_crank: Option<pp_ftms::CrankRevolutionData>,
    pending: Option<Pending>,
    inflight: Option<Pending>,
    last_sent_grade: Option<f32>,
    last_sent_power: Option<u16>,
    rejected_grade: Option<f32>,
    rejected_power: Option<u16>,
    last_command_ms: Option<u64>,
    awaiting: Option<Awaiting>,
    consecutive_timeouts: u8,
    recontrol_attempted: bool,
    warning: Option<String>,
    backoff: std::time::Duration,
    reconnect_at_ms: Option<u64>,
    reconnect_armed: bool,
    wire: ControlWire,
    /// `ftms`, `cps`, or `fec`. `none` until service discovery finishes.
    telemetry: &'static str,
    /// `ftms`, `fec`, or `none`.
    control: &'static str,
    /// Checksum on the latest valid FE-C frame. `None` until one arrives.
    fec_checksum: Option<FecChecksum>,
    /// Page 55 is waiting for the checksum hold to lift.
    fec_user_pending: bool,
    /// Engine time at which the hold ends if no frame has arrived.
    fec_hold_until_ms: Option<u64>,
    fec_checksum_noted: bool,
    fec_bad_noted: bool,
    trace: BleTrace,
    /// Short service ids for the connected-state trace row.
    services: String,
    mono_ms: u64,
}

impl Engine {
    pub(crate) fn new(config: TrainerConfig) -> Self {
        let backoff = config.reconnect_initial;
        Self {
            config,
            phase: Phase::Idle,
            state: ConnectionState::Disconnected,
            name: String::new(),
            profile: None,
            reading: Reading::default(),
            last_crank: None,
            pending: None,
            inflight: None,
            last_sent_grade: None,
            last_sent_power: None,
            rejected_grade: None,
            rejected_power: None,
            last_command_ms: None,
            awaiting: None,
            consecutive_timeouts: 0,
            recontrol_attempted: false,
            warning: None,
            backoff,
            reconnect_at_ms: None,
            reconnect_armed: false,
            wire: ControlWire::None,
            telemetry: "none",
            control: "none",
            fec_checksum: None,
            fec_user_pending: false,
            fec_hold_until_ms: None,
            fec_checksum_noted: false,
            fec_bad_noted: false,
            trace: BleTrace::off(),
            services: String::new(),
            mono_ms: 0,
        }
    }

    /// Install a log opened by the caller.
    ///
    /// [`BleTrace::from_env`] is the spawn path. Tests pass [`BleTrace::open`]
    /// and do not read the environment.
    pub(crate) fn set_trace_log(&mut self, trace: BleTrace) {
        self.trace = trace;
    }

    /// `ftms`, `cps`, or `fec` once a data characteristic has been chosen.
    pub(crate) fn telemetry_label(&self) -> &'static str {
        self.telemetry
    }

    /// `ftms`, `fec`, or `none`, matching the write path actually taken.
    pub(crate) fn control_label(&self) -> &'static str {
        self.control
    }

    pub(crate) fn state(&self) -> ConnectionState {
        self.state.clone()
    }

    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    /// The sample built from fields that are still inside the staleness window.
    #[cfg(test)]
    pub(crate) fn sample(&self, now_ms: u64) -> Option<TrainerSample> {
        self.assemble(now_ms)
    }

    /// Each measurement with the engine time it last arrived.
    pub(crate) fn fields(&self) -> FieldClocks {
        FieldClocks {
            speed_kph: clock(self.reading.speed_kph),
            cadence_rpm: clock(self.reading.cadence_rpm),
            power_w: clock(self.reading.power_w),
            heart_rate_bpm: clock(self.reading.heart_rate_bpm),
        }
    }

    /// No control-point write is waiting and no command is queued.
    pub(crate) fn control_settled(&self) -> bool {
        self.awaiting.is_none() && self.pending.is_none()
    }

    /// The most recent non-fatal control-point problem, if any.
    pub(crate) fn warning(&self) -> Option<&str> {
        self.warning.as_deref()
    }

    /// Short GATT service ids, copied into the connected-state trace row.
    pub(crate) fn set_services(&mut self, services: &str) {
        self.services = services.to_string();
    }

    /// Begin a connection attempt. Clears the previous sample and crank memory.
    pub(crate) fn start_connect(&mut self, now_ms: u64, name: impl Into<String>) {
        let resumed = self.phase == Phase::Connecting;
        self.mono_ms = now_ms;
        self.name = name.into();
        self.phase = Phase::Connecting;
        self.state = ConnectionState::Connecting;
        self.profile = None;
        self.reading = Reading::default();
        self.last_crank = None;
        self.pending = None;
        self.inflight = None;
        self.last_sent_grade = None;
        self.last_sent_power = None;
        self.rejected_grade = None;
        self.rejected_power = None;
        self.last_command_ms = None;
        self.awaiting = None;
        self.consecutive_timeouts = 0;
        self.recontrol_attempted = false;
        self.warning = None;
        self.reconnect_armed = false;
        self.wire = ControlWire::None;
        self.telemetry = "none";
        self.control = "none";
        self.fec_checksum = None;
        self.fec_user_pending = false;
        self.fec_hold_until_ms = None;
        self.fec_checksum_noted = false;
        self.fec_bad_noted = false;
        if !resumed {
            self.trace_state("connecting");
        }
    }

    /// Forget the link. Used when the owner is shutting down.
    ///
    /// On an FE-C control session this first queues a 0% grade, the same
    /// release the game asks for when a ride ends. Read-only mode and the
    /// FTMS path return no write: FTMS grade 0 is the caller's
    /// [`pp_core::TrainerCommand::SetGrade`].
    pub(crate) fn stop(&mut self) -> Vec<OutAction> {
        let mut release = self.take_user_config();
        release.extend(self.release_flat());
        self.phase = Phase::Stopped;
        self.awaiting = None;
        self.reconnect_armed = false;
        self.pending = None;
        self.state = ConnectionState::Disconnected;
        self.trace_state("disconnected");
        self.trace.flush();
        release
    }

    /// A 0% FE-C grade, when this session is allowed to write one.
    ///
    /// Called on shutdown and when a live FE-C link is about to drop. It
    /// ignores the grade delta and the rate limit so the trainer is left flat.
    pub(crate) fn release_flat(&mut self) -> Vec<OutAction> {
        if self.config.mode != SessionMode::Control || self.wire != ControlWire::Fec {
            return Vec::new();
        }
        if matches!(self.phase, Phase::Idle | Phase::Stopped | Phase::Connecting) {
            return Vec::new();
        }
        let Ok(bytes) = (TrackResistance {
            grade_pct: 0.0,
            rolling_resistance: Some(self.config.crr),
        })
        .encode(self.outgoing_checksum()) else {
            return Vec::new();
        };
        self.last_sent_grade = Some(0.0);
        self.trace_fec_tx(&bytes, "grade=0");
        vec![OutAction::WriteFec(bytes)]
    }

    pub(crate) fn handle(&mut self, now_ms: u64, event: InEvent) -> Vec<OutAction> {
        self.mono_ms = now_ms;
        if self.phase == Phase::Stopped {
            return Vec::new();
        }
        match event {
            InEvent::Ready(profile) => self.on_ready(now_ms, profile),
            InEvent::Value { uuid, bytes } => self.on_value(now_ms, uuid, &bytes),
            InEvent::LinkLost => self.on_link_lost(now_ms),
            InEvent::ConnectFailed { reason } => self.on_connect_failed(now_ms, reason),
            InEvent::Command(command) => self.on_command(now_ms, command),
        }
    }

    /// Timeouts, the grade rate limit, and reconnect backoff.
    pub(crate) fn poll(&mut self, now_ms: u64) -> Vec<OutAction> {
        self.mono_ms = now_ms;
        if self.phase == Phase::Stopped {
            return Vec::new();
        }
        if let Some(waiting) = &self.awaiting
            && now_ms >= waiting.deadline_ms
        {
            let opcode = waiting.opcode;
            self.awaiting = None;
            return self.on_timeout(now_ms, opcode);
        }
        let mut actions = self.flush(now_ms);
        if self.reconnect_armed && self.reconnect_at_ms.is_some_and(|at| now_ms >= at) {
            self.reconnect_armed = false;
            self.phase = Phase::Connecting;
            self.state = ConnectionState::Connecting;
            self.trace_state("reconnecting");
            actions.push(OutAction::Reconnect);
        }
        actions
    }

    fn on_ready(&mut self, now_ms: u64, profile: DeviceProfile) -> Vec<OutAction> {
        self.profile = Some(profile);
        let mut actions = Vec::new();
        let data = if profile.indoor_bike {
            self.telemetry = "ftms";
            Some(INDOOR_BIKE_DATA_UUID)
        } else if profile.cycling_power {
            self.telemetry = "cps";
            Some(CYCLING_POWER_MEASUREMENT_UUID)
        } else if profile.fec_notify {
            self.telemetry = "fec";
            Some(FEC_NOTIFY_UUID)
        } else {
            None
        };
        let Some(data) = data else {
            self.fail_and_backoff(now_ms, "no indoor bike, cycling power, or FE-C data");
            actions.push(OutAction::Disconnect);
            return actions;
        };
        actions.push(OutAction::Subscribe(data));
        if !profile.indoor_bike && profile.fec_notify && data != FEC_NOTIFY_UUID {
            actions.push(OutAction::Subscribe(FEC_NOTIFY_UUID));
        }
        if profile.heart_rate {
            actions.push(OutAction::Subscribe(HEART_RATE_MEASUREMENT_UUID));
        }
        if self.config.mode == SessionMode::ReadOnly {
            self.wire = ControlWire::None;
            self.control = "none";
            self.mark_connected();
            return actions;
        }
        if profile.control_point {
            self.wire = ControlWire::Ftms;
            self.control = "ftms";
            actions.push(OutAction::Subscribe(FITNESS_MACHINE_CONTROL_POINT_UUID));
            let bytes = vec![OPCODE_REQUEST_CONTROL];
            self.trace_control(&bytes, "request control");
            actions.push(OutAction::WriteControl(bytes));
            self.arm_wait(now_ms, OPCODE_REQUEST_CONTROL);
            self.phase = Phase::Handshaking;
            return actions;
        }
        if profile.fec_write {
            self.wire = ControlWire::Fec;
            self.control = "fec";
            self.fec_user_pending = true;
            self.fec_hold_until_ms = Some(now_ms.saturating_add(FEC_CHECKSUM_HOLD_MS));
            self.mark_connected();
            actions.extend(self.flush(now_ms));
            return actions;
        }
        // Power and cadence still flow. The warning is the phrase the session
        // crate already treats as "resistance is not available".
        self.wire = ControlWire::None;
        self.control = "none";
        self.note("control not permitted");
        self.mark_connected();
        actions
    }

    fn on_value(&mut self, now_ms: u64, uuid: Uuid, bytes: &[u8]) -> Vec<OutAction> {
        if self.trace.is_recording() {
            self.record_rx(uuid, bytes);
        }
        if uuid == FITNESS_MACHINE_CONTROL_POINT_UUID {
            return self.on_control(now_ms, bytes);
        }
        if uuid == INDOOR_BIKE_DATA_UUID {
            self.on_indoor(now_ms, bytes);
        } else if uuid == CYCLING_POWER_MEASUREMENT_UUID {
            let prefer_power = self
                .profile
                .as_ref()
                .is_some_and(|profile| !profile.indoor_bike);
            if prefer_power {
                self.on_power(now_ms, bytes);
            }
        } else if uuid == FEC_NOTIFY_UUID {
            return self.on_fec(now_ms, bytes);
        } else if uuid == HEART_RATE_MEASUREMENT_UUID {
            self.on_hr(now_ms, bytes);
        }
        Vec::new()
    }

    fn on_indoor(&mut self, now_ms: u64, bytes: &[u8]) {
        let Ok(data) = IndoorBikeData::parse(bytes) else {
            return;
        };
        if let Some(speed) = data.instantaneous_speed_kph {
            remember(&mut self.reading.speed_kph, speed, now_ms);
        }
        if let Some(cadence) = data.instantaneous_cadence_rpm {
            remember(&mut self.reading.cadence_rpm, cadence, now_ms);
        }
        if let Some(power) = data.instantaneous_power_w {
            remember(&mut self.reading.power_w, non_negative_watts(power), now_ms);
        }
        if let Some(heart) = data.heart_rate_bpm {
            remember(&mut self.reading.heart_rate_bpm, heart, now_ms);
        }
    }

    fn on_power(&mut self, now_ms: u64, bytes: &[u8]) {
        let Ok(data) = CyclingPowerMeasurement::parse(bytes) else {
            return;
        };
        let cadence = data.crank.and_then(|crank| {
            let rpm = self
                .last_crank
                .and_then(|previous| crank_cadence_rpm(previous, crank));
            self.last_crank = Some(crank);
            rpm
        });
        remember(
            &mut self.reading.power_w,
            non_negative_watts(data.instantaneous_power_w),
            now_ms,
        );
        if let Some(rpm) = cadence {
            remember(&mut self.reading.cadence_rpm, rpm, now_ms);
        }
    }

    fn on_fec(&mut self, now_ms: u64, bytes: &[u8]) -> Vec<OutAction> {
        let message = match FecMessage::parse(bytes) {
            Ok(message) => message,
            Err(err) => {
                self.note_bad_checksum(bytes, &err);
                return Vec::new();
            }
        };
        let first = self.observe_checksum(message.checksum, message.page_number());
        self.apply_fec(now_ms, &message);
        if first {
            self.flush(now_ms)
        } else {
            Vec::new()
        }
    }

    fn apply_fec(&mut self, now_ms: u64, message: &FecMessage) {
        match message.data {
            FecData::General(data) => {
                if self.telemetry == "ftms" {
                    return;
                }
                if let Some(speed) = data.speed_kph() {
                    remember(&mut self.reading.speed_kph, speed, now_ms);
                }
                if let Some(heart) = data.heart_rate_bpm {
                    remember(&mut self.reading.heart_rate_bpm, heart, now_ms);
                }
            }
            FecData::Trainer(data) => {
                // Cycling Power Measurement wins when that characteristic is
                // open. Page 25 fills power and cadence only on an FE-C-only link.
                if self.telemetry != "fec" {
                    return;
                }
                if let Some(power) = data.instantaneous_power_w {
                    remember(&mut self.reading.power_w, power, now_ms);
                }
                if let Some(rpm) = data.cadence_rpm {
                    remember(&mut self.reading.cadence_rpm, f32::from(rpm), now_ms);
                }
            }
            FecData::TrackResistance(_) | FecData::UserConfiguration(_) => {}
        }
    }

    fn observe_checksum(&mut self, checksum: FecChecksum, page: u8) -> bool {
        let first = self.fec_checksum.is_none();
        self.fec_checksum = Some(checksum);
        if first && !self.fec_checksum_noted {
            self.fec_checksum_noted = true;
            eprintln!(
                "pp-trainer: fe-c checksum={} first-page={page}",
                checksum.label()
            );
        }
        first
    }

    fn note_bad_checksum(&mut self, bytes: &[u8], err: &pp_ftms::FtmsError) {
        let pp_ftms::FtmsError::Malformed {
            detail: "bad checksum",
            ..
        } = err
        else {
            return;
        };
        if self.fec_bad_noted {
            return;
        }
        self.fec_bad_noted = true;
        let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
        eprintln!("pp-trainer: fe-c checksum failed bytes={hex}");
    }

    fn outgoing_checksum(&self) -> FecChecksum {
        self.fec_checksum.unwrap_or(FecChecksum::Xor)
    }

    fn fec_held(&self, now_ms: u64) -> bool {
        self.wire == ControlWire::Fec
            && self.fec_checksum.is_none()
            && self.fec_hold_until_ms.is_some_and(|at| now_ms < at)
    }

    fn take_user_config(&mut self) -> Vec<OutAction> {
        if !self.fec_user_pending {
            return Vec::new();
        }
        self.fec_user_pending = false;
        match user_configuration(&self.config).encode(self.outgoing_checksum()) {
            Ok(bytes) => {
                self.trace_fec_tx(&bytes, "user configuration");
                vec![OutAction::WriteFec(bytes)]
            }
            Err(_) => Vec::new(),
        }
    }

    fn on_hr(&mut self, now_ms: u64, bytes: &[u8]) {
        let Ok(data) = HeartRateMeasurement::parse(bytes) else {
            return;
        };
        remember(
            &mut self.reading.heart_rate_bpm,
            data.heart_rate_bpm_u8(),
            now_ms,
        );
    }

    fn on_control(&mut self, now_ms: u64, bytes: &[u8]) -> Vec<OutAction> {
        let Ok(response) = ControlPointResponse::parse(bytes) else {
            return Vec::new();
        };
        let Some(waiting) = &self.awaiting else {
            return Vec::new();
        };
        if response.request_opcode != waiting.opcode {
            return Vec::new();
        }
        let opcode = response.request_opcode;
        self.awaiting = None;
        match (opcode, response.result) {
            (OPCODE_REQUEST_CONTROL, ControlResult::Success) => {
                self.arm_wait(now_ms, OPCODE_START_OR_RESUME);
                let bytes = vec![OPCODE_START_OR_RESUME];
                self.trace_control(&bytes, "start or resume");
                vec![OutAction::WriteControl(bytes)]
            }
            (OPCODE_REQUEST_CONTROL, ControlResult::ControlNotPermitted) => {
                self.fail_hold(response.result.to_string());
                Vec::new()
            }
            (OPCODE_REQUEST_CONTROL, other) => {
                self.fail_and_backoff(now_ms, other.to_string());
                vec![OutAction::Disconnect]
            }
            (OPCODE_START_OR_RESUME, ControlResult::Success) => {
                self.mark_connected();
                self.flush(now_ms)
            }
            (OPCODE_START_OR_RESUME, other) => {
                self.note(format!("start or resume: {other}"));
                self.mark_connected();
                self.flush(now_ms)
            }
            (
                OPCODE_SET_INDOOR_BIKE_SIMULATION | OPCODE_SET_TARGET_POWER,
                ControlResult::Success,
            ) => {
                self.consecutive_timeouts = 0;
                self.recontrol_attempted = false;
                self.inflight = None;
                self.flush(now_ms)
            }
            (
                OPCODE_SET_INDOOR_BIKE_SIMULATION | OPCODE_SET_TARGET_POWER,
                ControlResult::ControlNotPermitted,
            ) => self.on_control_lost(now_ms),
            (OPCODE_SET_INDOOR_BIKE_SIMULATION | OPCODE_SET_TARGET_POWER, other) => {
                self.remember_rejection();
                self.note(other.to_string());
                Vec::new()
            }
            (_, ControlResult::ControlNotPermitted) => {
                self.fail_hold(response.result.to_string());
                Vec::new()
            }
            (_, other) => {
                self.fail_and_backoff(now_ms, other.to_string());
                vec![OutAction::Disconnect]
            }
        }
    }

    fn on_command(&mut self, now_ms: u64, command: TrainerCommand) -> Vec<OutAction> {
        if self.config.mode != SessionMode::Control || self.phase != Phase::Ready {
            return Vec::new();
        }
        self.pending = Some(match command {
            TrainerCommand::SetGrade { grade_pct } => Pending::Grade(grade_pct),
            TrainerCommand::SetTargetPower { watts } => Pending::Power(watts),
        });
        self.flush(now_ms)
    }

    fn flush(&mut self, now_ms: u64) -> Vec<OutAction> {
        if self.phase != Phase::Ready || self.awaiting.is_some() {
            return Vec::new();
        }
        if self.fec_held(now_ms) {
            return Vec::new();
        }
        let mut actions = self.take_user_config();
        if self.wire == ControlWire::None {
            self.pending = None;
            return actions;
        }
        let Some(pending) = self.pending else {
            return actions;
        };
        if self.rate_limited(now_ms) {
            return actions;
        }
        actions.extend(match pending {
            Pending::Grade(grade) => self.flush_grade(now_ms, grade),
            Pending::Power(watts) => self.flush_power(now_ms, watts),
        });
        actions
    }

    fn rate_limited(&self, now_ms: u64) -> bool {
        let Some(last) = self.last_command_ms else {
            return false;
        };
        now_ms.saturating_sub(last) < duration_ms(self.config.min_command_interval)
    }

    fn flush_grade(&mut self, now_ms: u64, grade: f32) -> Vec<OutAction> {
        if self.rejected_grade.is_some_and(|rejected| {
            !grade_changed(rejected, grade, self.config.min_grade_delta_pct)
        }) {
            self.pending = None;
            return Vec::new();
        }
        if let Some(previous) = self.last_sent_grade
            && !grade_changed(previous, grade, self.config.min_grade_delta_pct)
        {
            self.pending = None;
            return Vec::new();
        }
        self.pending = None;
        self.rejected_grade = None;
        self.last_sent_grade = Some(grade);
        self.last_command_ms = Some(now_ms);
        if self.wire == ControlWire::Fec {
            let Ok(bytes) = (TrackResistance {
                grade_pct: grade,
                rolling_resistance: Some(self.config.crr),
            })
            .encode(self.outgoing_checksum()) else {
                return Vec::new();
            };
            if self.trace.is_recording() {
                self.trace_fec_tx(&bytes, &format!("grade={grade}"));
            }
            return vec![OutAction::WriteFec(bytes)];
        }
        let simulation = IndoorBikeSimulation {
            wind_speed_mps: self.config.wind_speed_mps,
            grade_pct: grade,
            crr: self.config.crr,
            cw_kg_per_m: self.config.cw_kg_per_m,
        };
        let Ok(bytes) = ControlPointRequest::SetIndoorBikeSimulation(simulation).encode() else {
            return Vec::new();
        };
        if self.trace.is_recording() {
            self.trace_control(&bytes, &format!("grade={grade}"));
        }
        self.inflight = Some(Pending::Grade(grade));
        self.arm_wait(now_ms, OPCODE_SET_INDOOR_BIKE_SIMULATION);
        vec![OutAction::WriteControl(bytes)]
    }

    fn flush_power(&mut self, now_ms: u64, watts: u16) -> Vec<OutAction> {
        if self.wire != ControlWire::Ftms {
            self.pending = None;
            if self.wire == ControlWire::Fec {
                self.note("target power is not available");
            }
            return Vec::new();
        }
        if self.rejected_power == Some(watts) || self.last_sent_power == Some(watts) {
            self.pending = None;
            return Vec::new();
        }
        let request = ControlPointRequest::from_trainer_command(
            TrainerCommand::SetTargetPower { watts },
            IndoorBikeSimulation::standard(0.0),
        );
        let Ok(bytes) = request.encode() else {
            self.pending = None;
            return Vec::new();
        };
        if self.trace.is_recording() {
            self.trace_control(&bytes, &format!("target-power={watts}"));
        }
        self.pending = None;
        self.rejected_power = None;
        self.inflight = Some(Pending::Power(watts));
        self.last_sent_power = Some(watts);
        self.last_command_ms = Some(now_ms);
        self.arm_wait(now_ms, OPCODE_SET_TARGET_POWER);
        vec![OutAction::WriteControl(bytes)]
    }

    fn on_link_lost(&mut self, now_ms: u64) -> Vec<OutAction> {
        self.awaiting = None;
        if !matches!(self.state, ConnectionState::Failed(_)) {
            self.state = ConnectionState::Disconnected;
            self.trace_state("disconnected");
        }
        if self.reconnect_armed {
            return Vec::new();
        }
        self.phase = Phase::Backoff;
        self.arm_reconnect(now_ms);
        Vec::new()
    }

    fn on_connect_failed(&mut self, now_ms: u64, reason: String) -> Vec<OutAction> {
        self.awaiting = None;
        if self.trace.is_recording() {
            self.trace_state(&format!("failed: {reason}"));
        }
        self.state = ConnectionState::Failed(reason);
        if self.reconnect_armed {
            return Vec::new();
        }
        self.phase = Phase::Backoff;
        self.arm_reconnect(now_ms);
        Vec::new()
    }

    fn mark_connected(&mut self) {
        self.phase = Phase::Ready;
        self.state = ConnectionState::Connected;
        self.backoff = self.config.reconnect_initial;
        self.reconnect_armed = false;
        if self.trace.is_recording() {
            let note = format!(
                "connected \"{}\" services=[{}] telemetry={} control={}",
                self.name, self.services, self.telemetry, self.control
            );
            self.trace_state(&note);
        }
    }

    fn fail_hold(&mut self, reason: impl Into<String>) {
        let reason = reason.into();
        self.phase = Phase::Held;
        self.warning = Some(reason.clone());
        if self.trace.is_recording() {
            self.trace_state(&format!("failed: {reason}"));
        }
        self.state = ConnectionState::Failed(reason);
        self.awaiting = None;
        self.pending = None;
        self.inflight = None;
        self.reconnect_armed = false;
    }

    fn fail_and_backoff(&mut self, now_ms: u64, reason: impl Into<String>) {
        let reason = reason.into();
        self.phase = Phase::Backoff;
        self.warning = Some(reason.clone());
        if self.trace.is_recording() {
            self.trace_state(&format!("failed: {reason}"));
        }
        self.state = ConnectionState::Failed(reason);
        self.awaiting = None;
        self.pending = None;
        self.inflight = None;
        self.arm_reconnect(now_ms);
    }

    fn on_timeout(&mut self, now_ms: u64, opcode: u8) -> Vec<OutAction> {
        match opcode {
            OPCODE_REQUEST_CONTROL => {
                self.fail_and_backoff(now_ms, "control point timed out");
                vec![OutAction::Disconnect]
            }
            OPCODE_START_OR_RESUME => {
                self.note("start or resume timed out");
                self.mark_connected();
                self.flush(now_ms)
            }
            OPCODE_SET_INDOOR_BIKE_SIMULATION | OPCODE_SET_TARGET_POWER => {
                self.on_command_timeout(now_ms)
            }
            _ => {
                self.fail_and_backoff(now_ms, "control point timed out");
                vec![OutAction::Disconnect]
            }
        }
    }

    fn on_command_timeout(&mut self, now_ms: u64) -> Vec<OutAction> {
        self.consecutive_timeouts = self.consecutive_timeouts.saturating_add(1);
        self.remember_rejection();
        self.note("control point timed out");
        if self.consecutive_timeouts >= 2 {
            self.fail_and_backoff(now_ms, "control point timed out");
            return vec![OutAction::Disconnect];
        }
        Vec::new()
    }

    fn on_control_lost(&mut self, now_ms: u64) -> Vec<OutAction> {
        if self.recontrol_attempted {
            self.fail_hold("control not permitted");
            return Vec::new();
        }
        self.recontrol_attempted = true;
        self.note("control not permitted; requesting control again");
        if let Some(command) = self.inflight.take() {
            self.pending = Some(command);
        }
        self.phase = Phase::Handshaking;
        self.arm_wait(now_ms, OPCODE_REQUEST_CONTROL);
        let bytes = vec![OPCODE_REQUEST_CONTROL];
        self.trace_control(&bytes, "request control");
        vec![OutAction::WriteControl(bytes)]
    }

    fn remember_rejection(&mut self) {
        match self.inflight.take() {
            Some(Pending::Grade(grade)) => self.rejected_grade = Some(grade),
            Some(Pending::Power(watts)) => self.rejected_power = Some(watts),
            None => {}
        }
    }

    fn note(&mut self, warning: impl Into<String>) {
        self.warning = Some(warning.into());
    }

    #[cfg(test)]
    fn assemble(&self, now_ms: u64) -> Option<TrainerSample> {
        let sample = TrainerSample {
            power_w: fresh(&self.reading.power_w, now_ms, self.config.staleness),
            cadence_rpm: fresh(&self.reading.cadence_rpm, now_ms, self.config.staleness),
            speed_kph: fresh(&self.reading.speed_kph, now_ms, self.config.staleness),
            heart_rate_bpm: fresh(&self.reading.heart_rate_bpm, now_ms, self.config.staleness),
        };
        if sample.power_w.is_none()
            && sample.cadence_rpm.is_none()
            && sample.speed_kph.is_none()
            && sample.heart_rate_bpm.is_none()
        {
            None
        } else {
            Some(sample)
        }
    }

    fn arm_wait(&mut self, now_ms: u64, opcode: u8) {
        self.awaiting = Some(Awaiting {
            opcode,
            deadline_ms: now_ms.saturating_add(duration_ms(self.config.control_timeout)),
        });
    }

    fn arm_reconnect(&mut self, now_ms: u64) {
        let delay = duration_ms(self.backoff);
        self.reconnect_at_ms = Some(now_ms.saturating_add(delay));
        self.reconnect_armed = true;
        let doubled = self.backoff.saturating_mul(2);
        self.backoff = if doubled > self.config.reconnect_max {
            self.config.reconnect_max
        } else {
            doubled
        };
    }

    fn trace_state(&mut self, note: &str) {
        self.trace_row("state", "", "", None, None, None, None, note);
    }

    fn trace_control(&mut self, bytes: &[u8], note: &str) {
        if !self.trace.is_recording() {
            return;
        }
        let hex = hex_bytes(bytes);
        self.trace_row("tx", "ftms-cp", &hex, None, None, None, None, note);
    }

    fn trace_fec_tx(&mut self, bytes: &[u8], note: &str) {
        if !self.trace.is_recording() {
            return;
        }
        let hex = hex_bytes(bytes);
        let page = bytes.get(4).copied();
        self.trace_row("tx", "fec", &hex, page, None, None, None, note);
    }

    fn record_rx(&mut self, uuid: Uuid, bytes: &[u8]) {
        let channel = channel_name(uuid);
        let hex = hex_bytes(bytes);
        let mut fec_page = None;
        let mut power = None;
        let mut cadence = None;
        let mut speed = None;
        let mut note = String::new();
        if uuid == FEC_NOTIFY_UUID {
            match FecMessage::parse(bytes) {
                Ok(message) => {
                    fec_page = Some(message.page_number());
                    match message.data {
                        FecData::General(data) => speed = data.speed_kph(),
                        FecData::Trainer(data) => {
                            power = data.instantaneous_power_w.map(i32::from);
                            cadence = data.cadence_rpm.map(f32::from);
                        }
                        FecData::TrackResistance(_) | FecData::UserConfiguration(_) => {}
                    }
                }
                Err(err) => {
                    if bytes.len() > 4 {
                        fec_page = Some(bytes[4]);
                    }
                    if err.to_string().contains("bad checksum") {
                        note = "bad checksum".to_string();
                    }
                }
            }
        } else if uuid == CYCLING_POWER_MEASUREMENT_UUID {
            if let Ok(data) = CyclingPowerMeasurement::parse(bytes) {
                power = Some(i32::from(data.instantaneous_power_w));
                cadence = data.crank.and_then(|crank| {
                    self.last_crank
                        .and_then(|previous| crank_cadence_rpm(previous, crank))
                });
            }
        } else if uuid == INDOOR_BIKE_DATA_UUID
            && let Ok(data) = IndoorBikeData::parse(bytes)
        {
            power = data.instantaneous_power_w.map(i32::from);
            cadence = data.instantaneous_cadence_rpm;
            speed = data.instantaneous_speed_kph;
        }
        self.trace_row("rx", &channel, &hex, fec_page, power, cadence, speed, &note);
    }

    #[allow(clippy::too_many_arguments)]
    fn trace_row(
        &mut self,
        dir: &str,
        channel: &str,
        bytes_hex: &str,
        fec_page: Option<u8>,
        power_w: Option<i32>,
        cadence_rpm: Option<f32>,
        speed_kph: Option<f32>,
        note: &str,
    ) {
        if !self.trace.is_recording() {
            return;
        }
        self.trace.record(TraceRow {
            mono_ms: self.mono_ms,
            dir,
            channel,
            bytes_hex,
            fec_page,
            power_w,
            cadence_rpm,
            speed_kph,
            note,
        });
    }
}

fn channel_name(uuid: Uuid) -> String {
    if uuid == INDOOR_BIKE_DATA_UUID {
        "ibd".to_string()
    } else if uuid == CYCLING_POWER_MEASUREMENT_UUID {
        "cps".to_string()
    } else if uuid == FEC_NOTIFY_UUID {
        "fec".to_string()
    } else if uuid == FITNESS_MACHINE_CONTROL_POINT_UUID {
        "ftms-cp".to_string()
    } else if uuid == FITNESS_MACHINE_STATUS_UUID {
        "ftms-status".to_string()
    } else if uuid == HEART_RATE_MEASUREMENT_UUID {
        "hr".to_string()
    } else {
        uuid.to_string()
    }
}

fn user_configuration(config: &crate::config::TrainerConfig) -> UserConfiguration {
    UserConfiguration {
        user_weight_kg: finite_kg(config.rider_mass_kg),
        bike_weight_kg: finite_kg(config.bike_mass_kg),
        wheel_diameter_offset_mm: None,
        wheel_diameter_m: None,
        gear_ratio: None,
    }
}

fn finite_kg(kg: Option<f32>) -> Option<f32> {
    kg.filter(|kg| kg.is_finite())
}

fn grade_changed(previous: f32, next: f32, min_delta: f32) -> bool {
    (next - previous).abs() + 1.0e-4 >= min_delta
}

fn non_negative_watts(watts: i16) -> u16 {
    u16::try_from(watts).unwrap_or(0)
}

fn remember<T: Copy>(slot: &mut Option<Stamp<T>>, value: T, now_ms: u64) {
    *slot = Some(Stamp {
        value,
        at_ms: now_ms,
    });
}

#[cfg(test)]
fn fresh<T: Copy>(
    slot: &Option<Stamp<T>>,
    now_ms: u64,
    staleness: std::time::Duration,
) -> Option<T> {
    let stamp = slot.as_ref()?;
    let age = std::time::Duration::from_millis(now_ms.saturating_sub(stamp.at_ms));
    if sample_is_fresh(age, staleness) {
        Some(stamp.value)
    } else {
        None
    }
}

fn clock<T: Copy>(slot: Option<Stamp<T>>) -> Option<FieldClock<T>> {
    slot.map(|stamp| FieldClock {
        value: stamp.value,
        at_ms: stamp.at_ms,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    struct FakePeripheral {
        result_code: u8,
        auto_respond: bool,
        respond_limit: Option<usize>,
        responses_sent: usize,
        script: std::collections::VecDeque<u8>,
        events: std::collections::VecDeque<InEvent>,
        subs: Vec<Uuid>,
        writes: Vec<Vec<u8>>,
        fec: Vec<Vec<u8>>,
    }

    impl FakePeripheral {
        fn new(result_code: u8) -> Self {
            Self {
                result_code,
                auto_respond: true,
                respond_limit: None,
                responses_sent: 0,
                script: std::collections::VecDeque::new(),
                events: std::collections::VecDeque::new(),
                subs: Vec::new(),
                writes: Vec::new(),
                fec: Vec::new(),
            }
        }

        fn apply(&mut self, action: &OutAction) {
            match action {
                OutAction::Subscribe(uuid) => self.subs.push(*uuid),
                OutAction::WriteFec(bytes) => self.fec.push(bytes.clone()),
                OutAction::WriteControl(bytes) => {
                    let opcode = bytes.first().copied();
                    self.writes.push(bytes.clone());
                    let allowed = self
                        .respond_limit
                        .is_none_or(|limit| self.responses_sent < limit);
                    if self.auto_respond
                        && allowed
                        && let Some(opcode) = opcode
                        && self.subs.contains(&FITNESS_MACHINE_CONTROL_POINT_UUID)
                    {
                        let code = self.script.pop_front().unwrap_or(self.result_code);
                        self.responses_sent += 1;
                        self.events.push_back(InEvent::Value {
                            uuid: FITNESS_MACHINE_CONTROL_POINT_UUID,
                            bytes: vec![0x80, opcode, code],
                        });
                    }
                }
                OutAction::Disconnect => self.events.push_back(InEvent::LinkLost),
                OutAction::Reconnect => {}
            }
        }

        fn pop(&mut self) -> Option<InEvent> {
            self.events.pop_front()
        }
    }

    struct Harness {
        engine: Engine,
        fake: FakePeripheral,
        now: u64,
        trace: Vec<OutAction>,
    }

    impl Harness {
        fn control() -> Self {
            Self::with_result(SessionMode::Control, 0x01)
        }

        fn with_result(mode: SessionMode, result_code: u8) -> Self {
            Self {
                engine: Engine::new(TrainerConfig {
                    mode,
                    ..TrainerConfig::default()
                }),
                fake: FakePeripheral::new(result_code),
                now: 0,
                trace: Vec::new(),
            }
        }

        fn apply_all(&mut self, actions: Vec<OutAction>) {
            for action in actions {
                self.trace.push(action.clone());
                self.fake.apply(&action);
            }
        }

        fn pump(&mut self) {
            for _ in 0..16 {
                let mut progress = false;
                while let Some(event) = self.fake.pop() {
                    progress = true;
                    let actions = self.engine.handle(self.now, event);
                    self.apply_all(actions);
                }
                let actions = self.engine.poll(self.now);
                if !actions.is_empty() {
                    progress = true;
                    self.apply_all(actions);
                }
                if !progress {
                    break;
                }
            }
        }

        fn connect(&mut self, profile: DeviceProfile) {
            self.engine.start_connect(self.now, "Tacx Neo 2T");
            let actions = self.engine.handle(self.now, InEvent::Ready(profile));
            self.apply_all(actions);
            self.pump();
        }

        fn command(&mut self, command: TrainerCommand) {
            let actions = self.engine.handle(self.now, InEvent::Command(command));
            self.apply_all(actions);
            self.pump();
        }

        fn notify(&mut self, uuid: Uuid, bytes: Vec<u8>) {
            let actions = self.engine.handle(self.now, InEvent::Value { uuid, bytes });
            self.apply_all(actions);
            self.pump();
        }
    }

    fn full_profile() -> DeviceProfile {
        DeviceProfile {
            indoor_bike: true,
            cycling_power: true,
            heart_rate: true,
            control_point: true,
            fec_notify: false,
            fec_write: false,
        }
    }

    #[test]
    fn handshake_subscribes_then_requests_control_then_starts() {
        let mut harness = Harness::control();
        harness.connect(full_profile());
        assert_eq!(
            harness.fake.subs,
            vec![
                INDOOR_BIKE_DATA_UUID,
                HEART_RATE_MEASUREMENT_UUID,
                FITNESS_MACHINE_CONTROL_POINT_UUID,
            ]
        );
        assert_eq!(
            harness.fake.writes,
            vec![vec![OPCODE_REQUEST_CONTROL], vec![OPCODE_START_OR_RESUME]]
        );
        assert_eq!(harness.engine.state(), ConnectionState::Connected);
    }

    #[test]
    fn falls_back_to_cycling_power_when_indoor_bike_is_absent() {
        let profile = DeviceProfile {
            indoor_bike: false,
            cycling_power: true,
            heart_rate: false,
            control_point: true,
            fec_notify: false,
            fec_write: false,
        };
        let mut harness = Harness::control();
        harness.connect(profile);
        assert_eq!(
            harness.fake.subs,
            vec![
                CYCLING_POWER_MEASUREMENT_UUID,
                FITNESS_MACHINE_CONTROL_POINT_UUID
            ]
        );
        assert_eq!(harness.engine.state(), ConnectionState::Connected);
    }

    #[test]
    fn read_only_does_not_write_the_control_point() {
        let mut harness = Harness::with_result(SessionMode::ReadOnly, 0x01);
        harness.connect(full_profile());
        assert_eq!(
            harness.fake.subs,
            vec![INDOOR_BIKE_DATA_UUID, HEART_RATE_MEASUREMENT_UUID]
        );
        assert!(harness.fake.writes.is_empty());
        assert_eq!(harness.engine.state(), ConnectionState::Connected);
        harness.command(TrainerCommand::SetGrade { grade_pct: 4.0 });
        assert!(harness.fake.writes.is_empty());
    }

    #[test]
    fn control_not_permitted_does_not_send_simulation() {
        let mut harness = Harness::with_result(SessionMode::Control, 0x05);
        harness.connect(full_profile());
        assert_eq!(harness.fake.writes, vec![vec![OPCODE_REQUEST_CONTROL]]);
        match harness.engine.state() {
            ConnectionState::Failed(reason) => {
                assert!(reason.contains("control not permitted"), "{reason}");
            }
            other => panic!("expected failure, got {other:?}"),
        }
        harness.command(TrainerCommand::SetGrade { grade_pct: 8.0 });
        assert_eq!(harness.fake.writes.len(), 1);
        assert!(
            !harness
                .trace
                .iter()
                .any(|action| matches!(action, OutAction::Disconnect | OutAction::Reconnect))
        );
    }

    #[test]
    fn grade_is_coalesced_and_rate_limited() {
        let mut harness = Harness::control();
        harness.connect(full_profile());
        harness.command(TrainerCommand::SetGrade { grade_pct: 1.0 });
        harness.now = 100;
        harness.command(TrainerCommand::SetGrade { grade_pct: 1.05 });
        harness.command(TrainerCommand::SetGrade { grade_pct: 6.0 });
        harness.now = 200;
        harness.command(TrainerCommand::SetGrade { grade_pct: 7.5 });
        let grades_before = grade_payloads(&harness.fake.writes);
        assert_eq!(grades_before, vec![grade_bytes(1.0)]);

        harness.now = 500;
        harness.pump();
        let grades = grade_payloads(&harness.fake.writes);
        assert_eq!(grades, vec![grade_bytes(1.0), grade_bytes(7.5)]);
    }

    #[test]
    fn negative_grade_uses_the_codec_bytes() {
        let mut harness = Harness::control();
        harness.connect(full_profile());
        harness.command(TrainerCommand::SetGrade { grade_pct: -10.0 });
        assert_eq!(
            grade_payloads(&harness.fake.writes),
            vec![vec![0x11, 0x00, 0x00, 0x18, 0xFC, 0x28, 0x33]]
        );
    }

    #[test]
    fn control_timeout_fails_and_later_reconnects() {
        let profile = full_profile();
        let mut harness = Harness::with_result(SessionMode::Control, 0x01);
        harness.fake.auto_respond = false;
        harness.connect(profile);
        assert_eq!(harness.fake.writes, vec![vec![OPCODE_REQUEST_CONTROL]]);
        harness.now = 2_000;
        harness.pump();
        match harness.engine.state() {
            ConnectionState::Failed(reason) => assert!(reason.contains("timed out"), "{reason}"),
            other => panic!("expected timeout, got {other:?}"),
        }
        assert!(
            !harness
                .trace
                .iter()
                .any(|action| matches!(action, OutAction::Reconnect))
        );
        harness.now = 2_000 + 1_000;
        harness.pump();
        assert_eq!(harness.engine.state(), ConnectionState::Connecting);
        assert!(
            harness
                .trace
                .iter()
                .any(|action| matches!(action, OutAction::Reconnect))
        );
    }

    #[test]
    fn link_loss_waits_out_the_backoff_then_reconnects() {
        let mut harness = Harness::control();
        harness.connect(full_profile());
        let actions = harness.engine.handle(harness.now, InEvent::LinkLost);
        harness.apply_all(actions);
        assert_eq!(harness.engine.state(), ConnectionState::Disconnected);
        harness.pump();
        assert_eq!(harness.engine.state(), ConnectionState::Disconnected);

        harness.now = 999;
        harness.pump();
        assert_eq!(harness.engine.state(), ConnectionState::Disconnected);

        harness.now = 1_000;
        harness.pump();
        assert_eq!(harness.engine.state(), ConnectionState::Connecting);
        assert!(
            harness
                .trace
                .iter()
                .any(|action| matches!(action, OutAction::Reconnect))
        );
    }

    #[test]
    fn sample_goes_stale_and_cadence_needs_two_crank_readings() {
        let mut harness = Harness::control();
        harness.connect(full_profile());
        // Flags 0x0044: speed, cadence, power. 30.00 km/h, 90.0 rpm, 200 W. Heart rate absent.
        harness.notify(
            INDOOR_BIKE_DATA_UUID,
            vec![0x44, 0x00, 0xB8, 0x0B, 0xB4, 0x00, 0xC8, 0x00],
        );
        let sample = harness.engine.sample(harness.now).unwrap();
        assert_eq!(sample.power_w, Some(200));
        assert_eq!(sample.cadence_rpm, Some(90.0));
        assert!((sample.speed_kph.unwrap() - 30.0).abs() < 0.001);

        harness.notify(HEART_RATE_MEASUREMENT_UUID, vec![0x00, 142]);
        assert_eq!(
            harness.engine.sample(harness.now).unwrap().heart_rate_bpm,
            Some(142)
        );

        harness.now = 3_000;
        assert!(harness.engine.sample(harness.now).is_some());
        harness.now = 3_001;
        assert!(harness.engine.sample(harness.now).is_none());

        let power_profile = DeviceProfile {
            indoor_bike: false,
            cycling_power: true,
            heart_rate: false,
            control_point: true,
            fec_notify: false,
            fec_write: false,
        };
        let mut power = Harness::control();
        power.connect(power_profile);
        let first = crank_packet(100, 0);
        let second = crank_packet(110, 1024);
        power.notify(CYCLING_POWER_MEASUREMENT_UUID, first);
        assert_eq!(power.engine.sample(0).unwrap().cadence_rpm, None);
        assert_eq!(power.engine.sample(0).unwrap().power_w, Some(250));
        power.notify(CYCLING_POWER_MEASUREMENT_UUID, second);
        let rpm = power.engine.sample(0).unwrap().cadence_rpm.unwrap();
        assert!((rpm - 600.0).abs() < 0.01, "{rpm}");
    }

    #[test]
    fn split_indoor_bike_notifications_merge_until_each_field_expires() {
        let mut harness = Harness::control();
        harness.connect(full_profile());
        // Speed and cadence, no power. Flags 0x0004.
        harness.notify(
            INDOOR_BIKE_DATA_UUID,
            vec![0x04, 0x00, 0xB8, 0x0B, 0xB4, 0x00],
        );
        let early = harness.engine.sample(0).unwrap();
        assert_eq!(early.power_w, None);
        assert_eq!(early.cadence_rpm, Some(90.0));
        assert!((early.speed_kph.unwrap() - 30.0).abs() < 0.001);

        harness.now = 2_000;
        // More Data set, power only. Flags 0x0041, 200 W.
        harness.notify(INDOOR_BIKE_DATA_UUID, vec![0x41, 0x00, 0xC8, 0x00]);
        let merged = harness.engine.sample(harness.now).unwrap();
        assert_eq!(merged.power_w, Some(200));
        assert_eq!(merged.cadence_rpm, Some(90.0));
        assert!((merged.speed_kph.unwrap() - 30.0).abs() < 0.001);

        harness.now = 3_000;
        let still = harness.engine.sample(harness.now).unwrap();
        assert_eq!(still.cadence_rpm, Some(90.0));
        assert_eq!(still.power_w, Some(200));

        harness.now = 3_001;
        let aged = harness.engine.sample(harness.now).unwrap();
        assert_eq!(aged.speed_kph, None);
        assert_eq!(aged.cadence_rpm, None);
        assert_eq!(aged.power_w, Some(200));

        harness.now = 5_001;
        assert!(harness.engine.sample(harness.now).is_none());
    }

    #[test]
    fn cycling_power_keeps_cadence_when_a_later_packet_has_only_power() {
        let profile = DeviceProfile {
            indoor_bike: false,
            cycling_power: true,
            heart_rate: false,
            control_point: true,
            fec_notify: false,
            fec_write: false,
        };
        let mut harness = Harness::control();
        harness.connect(profile);
        harness.notify(CYCLING_POWER_MEASUREMENT_UUID, crank_packet(100, 0));
        harness.notify(CYCLING_POWER_MEASUREMENT_UUID, crank_packet(110, 1024));
        let with_cadence = harness.engine.sample(0).unwrap();
        assert_eq!(with_cadence.power_w, Some(250));
        assert!((with_cadence.cadence_rpm.unwrap() - 600.0).abs() < 0.01);

        harness.now = 1_000;
        // Power only, flags clear. Cadence from the previous crank pair must stay.
        harness.notify(CYCLING_POWER_MEASUREMENT_UUID, vec![0x00, 0x00, 0x64, 0x00]);
        let merged = harness.engine.sample(harness.now).unwrap();
        assert_eq!(merged.power_w, Some(100));
        assert!((merged.cadence_rpm.unwrap() - 600.0).abs() < 0.01);

        harness.now = 3_001;
        let aged = harness.engine.sample(harness.now).unwrap();
        assert_eq!(aged.cadence_rpm, None);
        assert_eq!(aged.power_w, Some(100));
    }

    #[test]
    fn start_rejected_still_reaches_connected_and_accepts_grade() {
        let mut harness = Harness::control();
        harness.fake.script = std::collections::VecDeque::from([0x01, 0x04]);
        harness.connect(full_profile());
        assert_eq!(harness.engine.state(), ConnectionState::Connected);
        let warning = harness.engine.warning().unwrap();
        assert!(warning.contains("operation failed"), "{warning}");
        harness.command(TrainerCommand::SetGrade { grade_pct: 3.0 });
        assert_eq!(grade_payloads(&harness.fake.writes), vec![grade_bytes(3.0)]);
        assert!(!disconnected(&harness.trace));
    }

    #[test]
    fn start_timeout_still_reaches_connected() {
        let mut harness = Harness::control();
        harness.fake.respond_limit = Some(1);
        harness.connect(full_profile());
        harness.now = 2_000;
        harness.pump();
        assert_eq!(harness.engine.state(), ConnectionState::Connected);
        let warning = harness.engine.warning().unwrap();
        assert!(warning.contains("timed out"), "{warning}");
        assert!(!disconnected(&harness.trace));
    }

    #[test]
    fn rejected_grade_is_not_resent_and_telemetry_continues() {
        let mut harness = Harness::control();
        harness.fake.script = std::collections::VecDeque::from([0x01, 0x01, 0x03]);
        harness.connect(full_profile());
        harness.command(TrainerCommand::SetGrade { grade_pct: 4.0 });
        harness.now = 500;
        harness.command(TrainerCommand::SetGrade { grade_pct: 4.0 });
        assert_eq!(grade_payloads(&harness.fake.writes), vec![grade_bytes(4.0)]);
        assert_eq!(harness.engine.state(), ConnectionState::Connected);
        harness.notify(
            INDOOR_BIKE_DATA_UUID,
            vec![0x40, 0x00, 0x00, 0x00, 0xC8, 0x00],
        );
        assert_eq!(
            harness.engine.sample(harness.now).unwrap().power_w,
            Some(200)
        );
        assert!(!disconnected(&harness.trace));
    }

    #[test]
    fn repeated_command_timeouts_drop_the_link() {
        let mut harness = Harness::control();
        harness.connect(full_profile());
        harness.fake.auto_respond = false;
        harness.command(TrainerCommand::SetGrade { grade_pct: 1.0 });
        harness.now = 2_000;
        harness.pump();
        assert_eq!(harness.engine.state(), ConnectionState::Connected);
        assert!(!disconnected(&harness.trace));

        harness.now = 2_500;
        harness.command(TrainerCommand::SetGrade { grade_pct: 3.0 });
        harness.now = 4_500;
        harness.pump();
        match harness.engine.state() {
            ConnectionState::Failed(reason) => assert!(reason.contains("timed out"), "{reason}"),
            other => panic!("expected failure, got {other:?}"),
        }
        assert!(disconnected(&harness.trace));
    }

    #[test]
    fn control_lost_retries_request_control_once_then_holds() {
        let mut harness = Harness::control();
        harness.fake.script = std::collections::VecDeque::from([0x01, 0x01, 0x05, 0x05]);
        harness.connect(full_profile());
        harness.command(TrainerCommand::SetGrade { grade_pct: 2.0 });
        match harness.engine.state() {
            ConnectionState::Failed(reason) => {
                assert!(reason.contains("control not permitted"), "{reason}");
            }
            other => panic!("expected held failure, got {other:?}"),
        }
        assert_eq!(
            harness
                .fake
                .writes
                .iter()
                .filter(|bytes| bytes.first() == Some(&OPCODE_REQUEST_CONTROL))
                .count(),
            2
        );
        harness.notify(
            INDOOR_BIKE_DATA_UUID,
            vec![0x40, 0x00, 0x00, 0x00, 0x64, 0x00],
        );
        assert_eq!(harness.engine.sample(0).unwrap().power_w, Some(100));
        assert!(!disconnected(&harness.trace));
    }

    #[test]
    fn a_bad_notification_does_not_drop_the_link() {
        let mut harness = Harness::control();
        harness.connect(full_profile());
        harness.notify(INDOOR_BIKE_DATA_UUID, vec![0x44]);
        assert_eq!(harness.engine.state(), ConnectionState::Connected);
        assert!(harness.engine.sample(0).is_none());
    }

    fn fec_profile(cycling_power: bool) -> DeviceProfile {
        DeviceProfile {
            indoor_bike: false,
            cycling_power,
            heart_rate: false,
            control_point: false,
            fec_notify: true,
            fec_write: true,
        }
    }

    #[test]
    fn fec_holds_writes_for_a_frame_then_uses_the_observed_checksum() {
        let mut harness = Harness::control();
        harness.connect(fec_profile(false));
        assert_eq!(harness.fake.subs, vec![FEC_NOTIFY_UUID]);
        assert_eq!(harness.engine.control_label(), "fec");
        assert!(harness.fake.fec.is_empty());
        harness.command(TrainerCommand::SetGrade { grade_pct: 1.0 });
        assert!(
            harness.fake.fec.is_empty(),
            "page 55 and page 51 wait for a trainer frame"
        );

        // Page 25 from the trainer. Trailer 0xEB is the Tacx sum.
        harness.notify(
            FEC_NOTIFY_UUID,
            vec![
                0xA4, 0x09, 0x4E, 0x05, 0x19, 0x07, 0x5A, 0xE8, 0x03, 0xFA, 0x00, 0x30, 0xEB,
            ],
        );
        assert_eq!(
            harness.fake.fec[0],
            user_configuration(&harness.engine.config)
                .encode(FecChecksum::Sum)
                .unwrap()
        );
        assert_eq!(
            harness.fake.fec[1],
            vec![
                0xA4, 0x09, 0x4F, 0x05, 0x33, 0xFF, 0xFF, 0xFF, 0xFF, 0x84, 0x4E, 0x50, 0xAE,
            ]
        );

        // The next frame uses the ANT XOR (trailer 0x83). The following grade follows it.
        harness.now = 500;
        harness.notify(
            FEC_NOTIFY_UUID,
            vec![
                0xA4, 0x09, 0x4E, 0x05, 0x19, 0x07, 0x5A, 0xE8, 0x03, 0xFA, 0x00, 0x30, 0x83,
            ],
        );
        harness.command(TrainerCommand::SetGrade { grade_pct: 6.0 });
        let grade = TrackResistance {
            grade_pct: 6.0,
            rolling_resistance: Some(harness.engine.config.crr),
        }
        .encode(FecChecksum::Xor)
        .unwrap();
        assert_eq!(harness.fake.fec.last().unwrap(), &grade);
    }

    #[test]
    fn fec_uses_xor_when_no_frame_arrives_within_two_seconds() {
        let mut harness = Harness::control();
        harness.connect(fec_profile(false));
        harness.command(TrainerCommand::SetGrade { grade_pct: 1.0 });
        harness.now = 1_999;
        harness.pump();
        assert!(harness.fake.fec.is_empty());

        harness.now = 2_000;
        harness.pump();
        assert_eq!(
            harness.fake.fec[0],
            vec![
                0xA4, 0x09, 0x4F, 0x05, 0x37, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00, 0x00, 0x2F,
            ]
        );
        assert_eq!(
            harness.fake.fec[1],
            vec![
                0xA4, 0x09, 0x4F, 0x05, 0x33, 0xFF, 0xFF, 0xFF, 0xFF, 0x84, 0x4E, 0x50, 0x4E,
            ]
        );
    }

    #[test]
    fn fec_read_only_never_writes_and_shutdown_sends_flat_only_in_control() {
        let mut harness = Harness::with_result(SessionMode::ReadOnly, 0x01);
        harness.connect(fec_profile(true));
        assert_eq!(
            harness.fake.subs,
            vec![CYCLING_POWER_MEASUREMENT_UUID, FEC_NOTIFY_UUID]
        );
        assert!(harness.fake.fec.is_empty());
        harness.command(TrainerCommand::SetGrade { grade_pct: 4.0 });
        assert!(harness.fake.fec.is_empty());
        let actions = harness.engine.stop();
        assert!(actions.is_empty());

        let mut control = Harness::control();
        control.connect(fec_profile(false));
        control.command(TrainerCommand::SetGrade { grade_pct: 3.0 });
        let release = control.engine.stop();
        // Nothing has been heard yet, so the held page 55 and the flat page 51
        // both go out with the ANT XOR.
        assert_eq!(
            release,
            vec![
                OutAction::WriteFec(vec![
                    0xA4, 0x09, 0x4F, 0x05, 0x37, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00, 0x00, 0x2F,
                ]),
                OutAction::WriteFec(vec![
                    0xA4, 0x09, 0x4F, 0x05, 0x33, 0xFF, 0xFF, 0xFF, 0xFF, 0x20, 0x4E, 0x50, 0xEA,
                ]),
            ]
        );
    }

    #[test]
    fn cycling_power_without_a_control_point_stays_up_and_does_not_write() {
        let profile = DeviceProfile {
            indoor_bike: false,
            cycling_power: true,
            heart_rate: false,
            control_point: false,
            fec_notify: false,
            fec_write: false,
        };
        let mut harness = Harness::control();
        harness.connect(profile);
        assert_eq!(harness.engine.state(), ConnectionState::Connected);
        assert_eq!(harness.engine.control_label(), "none");
        assert_eq!(harness.engine.telemetry_label(), "cps");
        let warning = harness.engine.warning().unwrap();
        assert!(warning.contains("control not permitted"), "{warning}");
        harness.command(TrainerCommand::SetGrade { grade_pct: 5.0 });
        assert!(harness.fake.writes.is_empty());
        assert!(harness.fake.fec.is_empty());
        assert!(harness.engine.stop().is_empty());
    }

    #[test]
    fn fec_page_25_supplies_power_when_cycling_power_is_absent() {
        let mut harness = Harness::control();
        harness.connect(fec_profile(false));
        // Page 25: cadence 90, power 250. Checksum is the Tacx sum.
        harness.notify(
            FEC_NOTIFY_UUID,
            vec![
                0xA4, 0x09, 0x4E, 0x05, 0x19, 0x07, 0x5A, 0xE8, 0x03, 0xFA, 0x00, 0x30, 0xEB,
            ],
        );
        let sample = harness.engine.sample(0).unwrap();
        assert_eq!(sample.power_w, Some(250));
        assert_eq!(sample.cadence_rpm, Some(90.0));

        // Page 16: 10.000 m/s = 36 km/h, heart rate 120. Speed raw 10000 = 0x2710.
        // Checksum filled below after a parse in the assertion's sibling vector.
        let mut general = vec![
            0xA4, 0x09, 0x4E, 0x05, 0x10, 0x19, 0x00, 0x00, 0x10, 0x27, 120, 0x20,
        ];
        let sum = general[1..]
            .iter()
            .fold(0u8, |acc, byte| acc.wrapping_add(*byte));
        general.push(sum);
        harness.notify(FEC_NOTIFY_UUID, general);
        let sample = harness.engine.sample(0).unwrap();
        assert!((sample.speed_kph.unwrap() - 36.0).abs() < 1.0e-3);
        assert_eq!(sample.heart_rate_bpm, Some(120));
        assert_eq!(sample.power_w, Some(250));
    }

    #[test]
    fn a_bad_fec_notification_does_not_drop_the_link() {
        let mut harness = Harness::control();
        harness.connect(fec_profile(false));
        harness.notify(FEC_NOTIFY_UUID, vec![0x00, 0x01, 0x02]);
        assert_eq!(harness.engine.state(), ConnectionState::Connected);
        assert!(harness.engine.sample(0).is_none());
    }

    #[test]
    fn ble_trace_logs_each_kind_of_row_and_stays_optional() {
        let dir = std::env::temp_dir().join(format!(
            "pp-ble-trace-{}-{}",
            std::process::id(),
            crate::trace::BLE_TRACE_HEADER.len()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let quiet = Engine::new(TrainerConfig::default());
        assert!(!quiet.trace.is_recording());
        assert!(!dir.join("ble.csv").exists());

        let blocker = dir.join("not-a-directory");
        std::fs::write(&blocker, b"x").unwrap();
        let mut blocked = Engine::new(TrainerConfig::default());
        blocked.set_trace_log(crate::trace::BleTrace::open(&blocker));
        assert!(!blocked.trace.is_recording());
        blocked.start_connect(0, "Tacx");
        let actions = blocked.handle(
            0,
            InEvent::Ready(DeviceProfile {
                indoor_bike: false,
                cycling_power: true,
                heart_rate: false,
                control_point: true,
                fec_notify: false,
                fec_write: false,
            }),
        );
        assert!(!actions.is_empty());
        assert_eq!(blocked.state(), ConnectionState::Connecting);

        let mut engine = Engine::new(TrainerConfig::default());
        engine.set_trace_log(crate::trace::BleTrace::open(&dir));
        engine.set_services("1818,6e40fec1");
        engine.start_connect(0, "Tacx Neo 2T");
        let _ = engine.handle(
            0,
            InEvent::Ready(DeviceProfile {
                indoor_bike: false,
                cycling_power: true,
                heart_rate: false,
                control_point: false,
                fec_notify: true,
                fec_write: true,
            }),
        );
        engine.handle(
            10,
            InEvent::Value {
                uuid: CYCLING_POWER_MEASUREMENT_UUID,
                bytes: crank_packet(100, 0),
            },
        );
        engine.handle(
            20,
            InEvent::Value {
                uuid: CYCLING_POWER_MEASUREMENT_UUID,
                bytes: crank_packet(110, 1024),
            },
        );
        let page16 = fec_broadcast(0x10, [0x19, 0x00, 0x00, 0x10, 0x27, 0xFF, 0x00]);
        engine.handle(
            30,
            InEvent::Value {
                uuid: FEC_NOTIFY_UUID,
                bytes: page16.clone(),
            },
        );
        let page25 = vec![
            0xA4, 0x09, 0x4E, 0x05, 0x19, 0x07, 0x5A, 0xE8, 0x03, 0xFA, 0x00, 0x30, 0xEB,
        ];
        engine.handle(
            40,
            InEvent::Value {
                uuid: FEC_NOTIFY_UUID,
                bytes: page25.clone(),
            },
        );
        let mut bad = page25.clone();
        bad[12] ^= 0xFF;
        engine.handle(
            50,
            InEvent::Value {
                uuid: FEC_NOTIFY_UUID,
                bytes: bad.clone(),
            },
        );
        let _ = engine.handle(
            60,
            InEvent::Command(TrainerCommand::SetGrade { grade_pct: 3.5 }),
        );
        let _ = engine.stop();

        let text = std::fs::read_to_string(dir.join("ble.csv")).unwrap();
        let mut lines = text.lines();
        assert_eq!(lines.next().unwrap(), crate::trace::BLE_TRACE_HEADER);
        let rows: Vec<Vec<String>> = lines.map(split_csv).collect();
        assert!(rows.iter().all(|row| row.len() == 10), "{rows:?}");
        let mut previous = 0u64;
        for row in &rows {
            let unix: u64 = row[0].parse().unwrap();
            assert!(unix >= previous);
            previous = unix;
            assert!(!row.iter().any(|cell| cell == "NaN" || cell == "None"));
        }
        let cps = rows
            .iter()
            .find(|row| row[2] == "rx" && row[3] == "cps" && row[6] == "250" && row[7] == "600")
            .expect("cps row");
        assert_eq!(cps[4], crate::trace::hex_bytes(&crank_packet(110, 1024)));
        assert_eq!(cps[1], "20");
        let general = rows
            .iter()
            .find(|row| row[2] == "rx" && row[3] == "fec" && row[5] == "16")
            .expect("page 16");
        assert_eq!(general[4], crate::trace::hex_bytes(&page16));
        assert_eq!(general[8], "36");
        let specific = rows
            .iter()
            .find(|row| row[2] == "rx" && row[3] == "fec" && row[5] == "25" && row[6] == "250")
            .expect("page 25");
        assert_eq!(specific[4], crate::trace::hex_bytes(&page25));
        assert_eq!(specific[7], "90");
        let grade = rows
            .iter()
            .find(|row| row[2] == "tx" && row[9] == "grade=3.5")
            .expect("grade write");
        assert_eq!(grade[3], "fec");
        assert_eq!(grade[5], "51");
        assert!(!grade[4].is_empty());
        let broken = rows
            .iter()
            .find(|row| row[9] == "bad checksum")
            .expect("bad checksum");
        assert_eq!(broken[2], "rx");
        assert_eq!(broken[3], "fec");
        assert_eq!(broken[4], crate::trace::hex_bytes(&bad));
        let state = rows
            .iter()
            .find(|row| row[2] == "state" && row[9].starts_with("connected "))
            .expect("connected");
        assert!(state[9].contains("telemetry=cps"));
        assert!(state[9].contains("control=fec"));
        assert!(state[9].contains("services=[1818,6e40fec1]"));
        assert!(
            rows.iter()
                .any(|row| row[2] == "state" && row[9] == "connecting")
        );
        assert!(
            rows.iter()
                .any(|row| row[2] == "state" && row[9] == "disconnected")
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    fn fec_broadcast(page: u8, payload: [u8; 7]) -> Vec<u8> {
        let mut frame = vec![0xA4, 0x09, 0x4E, 0x05, page];
        frame.extend_from_slice(&payload);
        let xor = frame.iter().fold(0u8, |acc, byte| acc ^ byte);
        frame.push(xor);
        frame
    }

    fn split_csv(line: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut cell = String::new();
        let mut quoted = false;
        let mut chars = line.chars().peekable();
        while let Some(ch) = chars.next() {
            match ch {
                '"' if quoted && chars.peek() == Some(&'"') => {
                    cell.push('"');
                    let _ = chars.next();
                }
                '"' => quoted = !quoted,
                ',' if !quoted => out.push(std::mem::take(&mut cell)),
                _ => cell.push(ch),
            }
        }
        out.push(cell);
        out
    }

    fn disconnected(trace: &[OutAction]) -> bool {
        trace
            .iter()
            .any(|action| matches!(action, OutAction::Disconnect))
    }

    fn grade_bytes(grade_pct: f32) -> Vec<u8> {
        ControlPointRequest::SetIndoorBikeSimulation(IndoorBikeSimulation::standard(grade_pct))
            .encode()
            .unwrap()
    }

    fn grade_payloads(writes: &[Vec<u8>]) -> Vec<Vec<u8>> {
        writes
            .iter()
            .filter(|bytes| bytes.first() == Some(&0x11))
            .cloned()
            .collect()
    }

    fn crank_packet(revolutions: u16, event: u16) -> Vec<u8> {
        let mut bytes = vec![0x20, 0x00, 0xFA, 0x00];
        bytes.extend_from_slice(&revolutions.to_le_bytes());
        bytes.extend_from_slice(&event.to_le_bytes());
        bytes
    }
}
