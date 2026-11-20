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
    CyclingPowerMeasurement, FITNESS_MACHINE_CONTROL_POINT_UUID, HEART_RATE_MEASUREMENT_UUID,
    HeartRateMeasurement, INDOOR_BIKE_DATA_UUID, IndoorBikeData, IndoorBikeSimulation,
    OPCODE_REQUEST_CONTROL, OPCODE_SET_INDOOR_BIKE_SIMULATION, OPCODE_SET_TARGET_POWER,
    OPCODE_START_OR_RESUME, crank_cadence_rpm,
};
use uuid::Uuid;

use crate::ConnectionState;
use crate::config::{SessionMode, TrainerConfig, duration_ms};
#[cfg(test)]
use crate::sample_is_fresh;

/// What the peripheral can do, after service discovery.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DeviceProfile {
    pub indoor_bike: bool,
    pub cycling_power: bool,
    pub heart_rate: bool,
    pub control_point: bool,
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
        }
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

    /// Begin a connection attempt. Clears the previous sample and crank memory.
    pub(crate) fn start_connect(&mut self, name: impl Into<String>) {
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
    }

    /// Forget the link. Used when the owner is shutting down.
    pub(crate) fn stop(&mut self) {
        self.phase = Phase::Stopped;
        self.awaiting = None;
        self.reconnect_armed = false;
        self.pending = None;
        self.state = ConnectionState::Disconnected;
    }

    pub(crate) fn handle(&mut self, now_ms: u64, event: InEvent) -> Vec<OutAction> {
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
            actions.push(OutAction::Reconnect);
        }
        actions
    }

    fn on_ready(&mut self, now_ms: u64, profile: DeviceProfile) -> Vec<OutAction> {
        self.profile = Some(profile);
        let mut actions = Vec::new();
        let data = if profile.indoor_bike {
            Some(INDOOR_BIKE_DATA_UUID)
        } else if profile.cycling_power {
            Some(CYCLING_POWER_MEASUREMENT_UUID)
        } else {
            None
        };
        let Some(data) = data else {
            self.fail_and_backoff(now_ms, "no indoor bike or cycling power data");
            actions.push(OutAction::Disconnect);
            return actions;
        };
        actions.push(OutAction::Subscribe(data));
        if profile.heart_rate {
            actions.push(OutAction::Subscribe(HEART_RATE_MEASUREMENT_UUID));
        }
        if self.config.mode == SessionMode::ReadOnly {
            self.mark_connected();
            return actions;
        }
        if !profile.control_point {
            self.fail_and_backoff(now_ms, "no fitness machine control point");
            actions.push(OutAction::Disconnect);
            return actions;
        }
        actions.push(OutAction::Subscribe(FITNESS_MACHINE_CONTROL_POINT_UUID));
        actions.push(OutAction::WriteControl(vec![OPCODE_REQUEST_CONTROL]));
        self.arm_wait(now_ms, OPCODE_REQUEST_CONTROL);
        self.phase = Phase::Handshaking;
        actions
    }

    fn on_value(&mut self, now_ms: u64, uuid: Uuid, bytes: &[u8]) -> Vec<OutAction> {
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
                vec![OutAction::WriteControl(vec![OPCODE_START_OR_RESUME])]
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
        let Some(pending) = self.pending else {
            return Vec::new();
        };
        if self.rate_limited(now_ms) {
            return Vec::new();
        }
        match pending {
            Pending::Grade(grade) => self.flush_grade(now_ms, grade),
            Pending::Power(watts) => self.flush_power(now_ms, watts),
        }
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
        let simulation = IndoorBikeSimulation {
            wind_speed_mps: self.config.wind_speed_mps,
            grade_pct: grade,
            crr: self.config.crr,
            cw_kg_per_m: self.config.cw_kg_per_m,
        };
        let Ok(bytes) = ControlPointRequest::SetIndoorBikeSimulation(simulation).encode() else {
            self.pending = None;
            return Vec::new();
        };
        self.pending = None;
        self.rejected_grade = None;
        self.inflight = Some(Pending::Grade(grade));
        self.last_sent_grade = Some(grade);
        self.last_command_ms = Some(now_ms);
        self.arm_wait(now_ms, OPCODE_SET_INDOOR_BIKE_SIMULATION);
        vec![OutAction::WriteControl(bytes)]
    }

    fn flush_power(&mut self, now_ms: u64, watts: u16) -> Vec<OutAction> {
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
    }

    fn fail_hold(&mut self, reason: impl Into<String>) {
        let reason = reason.into();
        self.phase = Phase::Held;
        self.warning = Some(reason.clone());
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
        vec![OutAction::WriteControl(vec![OPCODE_REQUEST_CONTROL])]
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
            }
        }

        fn apply(&mut self, action: &OutAction) {
            match action {
                OutAction::Subscribe(uuid) => self.subs.push(*uuid),
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
            self.engine.start_connect("Tacx Neo 2T");
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
