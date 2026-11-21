//! Heart-rate strap session, without Bluetooth.
//!
//! The machine is synchronous. The BlueZ thread performs [`OutAction`]s and
//! feeds [`InEvent`]s. Tests do the same with a clock they pass in. A reading
//! stays visible for [`crate::SAMPLE_STALENESS`]. A strap that reports "no
//! skin contact" blanks the value immediately.

use pp_ftms::HeartRateMeasurement;

use crate::ConnectionState;
use crate::config::{TrainerConfig, duration_ms};
use crate::sample_is_fresh;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Scanning,
    Connecting,
    Ready,
    Backoff,
}

/// Something that happened on the link, or a request from the game.
#[derive(Debug)]
pub(crate) enum InEvent {
    /// The strap was found and a connection is starting. `name` is the device name.
    Resolved { name: String },
    /// Service discovery finished. `measurement` is the Heart Rate Measurement characteristic.
    Ready { measurement: bool },
    /// A notification on Heart Rate Measurement.
    Value(Vec<u8>),
    /// The radio link dropped.
    LinkLost,
    /// `connect` failed after the strap had been resolved.
    ConnectFailed { reason: String },
    /// No advertising strap matched the query. Do not retry on a timer.
    NotFound { query: String },
    /// The rider asked to drop the strap and not reconnect.
    Disconnect,
}

/// Work the link owner must perform.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum OutAction {
    /// Enable notifications on Heart Rate Measurement. This is the only write.
    Subscribe,
    /// Drop the current link.
    Disconnect,
    /// The backoff elapsed. Open the link again.
    Reconnect,
}

struct Stamp {
    bpm: u16,
    at_ms: u64,
}

/// Subscribe, staleness, skin contact, and reconnect backoff for one strap.
pub(crate) struct Engine {
    phase: Phase,
    state: ConnectionState,
    name: String,
    /// The rider asked to stay with this strap. Cleared by [`InEvent::Disconnect`].
    wants_link: bool,
    reading: Option<Stamp>,
    /// The latest packet said the sensor supports contact and none is detected.
    contact_off: bool,
    staleness_ms: u64,
    backoff: std::time::Duration,
    reconnect_initial: std::time::Duration,
    reconnect_max: std::time::Duration,
    reconnect_at_ms: Option<u64>,
    reconnect_armed: bool,
}

impl Engine {
    pub(crate) fn new() -> Self {
        let config = TrainerConfig::default();
        Self {
            phase: Phase::Scanning,
            state: ConnectionState::Scanning,
            name: String::new(),
            wants_link: false,
            reading: None,
            contact_off: false,
            staleness_ms: duration_ms(config.staleness),
            backoff: config.reconnect_initial,
            reconnect_initial: config.reconnect_initial,
            reconnect_max: config.reconnect_max,
            reconnect_at_ms: None,
            reconnect_armed: false,
        }
    }

    pub(crate) fn state(&self) -> ConnectionState {
        self.state.clone()
    }

    /// Name for a log line, including while a connection attempt is failing.
    pub(crate) fn label(&self) -> &str {
        if self.name.is_empty() {
            "heart-rate"
        } else {
            &self.name
        }
    }

    /// Name of the connected strap.
    pub(crate) fn connected_name(&self) -> Option<&str> {
        if self.state == ConnectionState::Connected && !self.name.is_empty() {
            Some(&self.name)
        } else {
            None
        }
    }

    /// Beats per minute, when the latest packet is fresh and on skin.
    pub(crate) fn bpm(&self, now_ms: u64) -> Option<u16> {
        if self.contact_off {
            return None;
        }
        let stamp = self.reading.as_ref()?;
        let age = std::time::Duration::from_millis(now_ms.saturating_sub(stamp.at_ms));
        if sample_is_fresh(age, std::time::Duration::from_millis(self.staleness_ms)) {
            Some(stamp.bpm)
        } else {
            None
        }
    }

    /// Engine time of the sample [`Self::bpm`] would return.
    pub(crate) fn sample_at_ms(&self, now_ms: u64) -> Option<u64> {
        self.bpm(now_ms)?;
        self.reading.as_ref().map(|stamp| stamp.at_ms)
    }

    pub(crate) fn handle(&mut self, now_ms: u64, event: InEvent) -> Vec<OutAction> {
        match event {
            InEvent::Resolved { name } => {
                self.name = name;
                self.phase = Phase::Connecting;
                self.state = ConnectionState::Connecting;
                self.reading = None;
                self.contact_off = false;
                self.wants_link = true;
                self.reconnect_armed = false;
                Vec::new()
            }
            InEvent::Ready { measurement } => self.on_ready(now_ms, measurement),
            InEvent::Value(bytes) => {
                self.on_value(now_ms, &bytes);
                Vec::new()
            }
            InEvent::LinkLost => self.on_link_lost(now_ms),
            InEvent::ConnectFailed { reason } => self.on_connect_failed(now_ms, reason),
            InEvent::NotFound { query } => {
                self.wants_link = false;
                self.reconnect_armed = false;
                self.phase = Phase::Scanning;
                self.state =
                    ConnectionState::Failed(format!("no heart-rate strap matches {query}"));
                Vec::new()
            }
            InEvent::Disconnect => {
                self.wants_link = false;
                self.reconnect_armed = false;
                self.reading = None;
                self.contact_off = false;
                self.phase = Phase::Scanning;
                self.state = ConnectionState::Disconnected;
                self.name.clear();
                vec![OutAction::Disconnect]
            }
        }
    }

    pub(crate) fn poll(&mut self, now_ms: u64) -> Vec<OutAction> {
        if self.reconnect_armed && self.reconnect_at_ms.is_some_and(|at| now_ms >= at) {
            self.reconnect_armed = false;
            self.phase = Phase::Connecting;
            self.state = ConnectionState::Connecting;
            return vec![OutAction::Reconnect];
        }
        Vec::new()
    }

    fn on_ready(&mut self, now_ms: u64, measurement: bool) -> Vec<OutAction> {
        if !measurement {
            self.fail_and_backoff(now_ms, "no heart rate measurement");
            return vec![OutAction::Disconnect];
        }
        self.phase = Phase::Ready;
        self.state = ConnectionState::Connected;
        self.backoff = self.reconnect_initial;
        self.reconnect_armed = false;
        vec![OutAction::Subscribe]
    }

    fn on_value(&mut self, now_ms: u64, bytes: &[u8]) {
        let Ok(measurement) = HeartRateMeasurement::parse(bytes) else {
            return;
        };
        let on_skin = !measurement.sensor_contact_supported || measurement.sensor_contact_detected;
        if on_skin {
            self.contact_off = false;
            self.reading = Some(Stamp {
                bpm: measurement.heart_rate_bpm,
                at_ms: now_ms,
            });
        } else {
            self.contact_off = true;
        }
    }

    fn on_link_lost(&mut self, now_ms: u64) -> Vec<OutAction> {
        self.reading = None;
        self.contact_off = false;
        if !self.wants_link {
            self.phase = Phase::Scanning;
            self.state = ConnectionState::Disconnected;
            return Vec::new();
        }
        self.phase = Phase::Backoff;
        self.state = ConnectionState::Disconnected;
        self.arm_reconnect(now_ms);
        Vec::new()
    }

    fn on_connect_failed(&mut self, now_ms: u64, reason: String) -> Vec<OutAction> {
        self.state = ConnectionState::Failed(reason);
        if !self.wants_link {
            self.phase = Phase::Scanning;
            return Vec::new();
        }
        self.phase = Phase::Backoff;
        self.arm_reconnect(now_ms);
        Vec::new()
    }

    fn fail_and_backoff(&mut self, now_ms: u64, reason: impl Into<String>) {
        self.state = ConnectionState::Failed(reason.into());
        self.phase = Phase::Backoff;
        self.arm_reconnect(now_ms);
    }

    fn arm_reconnect(&mut self, now_ms: u64) {
        let delay = duration_ms(self.backoff);
        self.reconnect_at_ms = Some(now_ms.saturating_add(delay));
        self.reconnect_armed = true;
        let doubled = self.backoff.saturating_mul(2);
        self.backoff = if doubled > self.reconnect_max {
            self.reconnect_max
        } else {
            doubled
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ready(engine: &mut Engine) {
        let actions = engine.handle(
            0,
            InEvent::Resolved {
                name: "HRM-Dual".into(),
            },
        );
        assert!(actions.is_empty());
        let actions = engine.handle(0, InEvent::Ready { measurement: true });
        assert_eq!(actions, vec![OutAction::Subscribe]);
        assert_eq!(engine.state(), ConnectionState::Connected);
    }

    #[test]
    fn eight_and_sixteen_bit_values_and_staleness() {
        let mut engine = Engine::new();
        ready(&mut engine);
        engine.handle(0, InEvent::Value(vec![0x00, 72]));
        assert_eq!(engine.bpm(0), Some(72));
        engine.handle(1_000, InEvent::Value(vec![0x01, 0x2C, 0x01]));
        assert_eq!(engine.bpm(1_000), Some(300));
        assert_eq!(engine.bpm(1_000 + 3_000), Some(300));
        assert_eq!(engine.bpm(1_000 + 3_001), None);
    }

    #[test]
    fn no_skin_contact_blanks_immediately_and_a_later_packet_restores_it() {
        let mut engine = Engine::new();
        ready(&mut engine);
        // Contact supported and detected, 80 bpm. Bits 1 and 2.
        engine.handle(0, InEvent::Value(vec![0x06, 80]));
        assert_eq!(engine.bpm(0), Some(80));
        // Contact supported, not detected.
        engine.handle(500, InEvent::Value(vec![0x04, 80]));
        assert_eq!(engine.bpm(500), None);
        // Contact not supported. The number is still a reading.
        engine.handle(800, InEvent::Value(vec![0x00, 64]));
        assert_eq!(engine.bpm(800), Some(64));
    }

    #[test]
    fn a_bad_packet_does_not_drop_the_link() {
        let mut engine = Engine::new();
        ready(&mut engine);
        engine.handle(0, InEvent::Value(vec![0x00]));
        assert_eq!(engine.state(), ConnectionState::Connected);
        assert_eq!(engine.bpm(0), None);
    }

    #[test]
    fn link_loss_waits_out_the_backoff_then_reconnects() {
        let mut engine = Engine::new();
        ready(&mut engine);
        let actions = engine.handle(100, InEvent::LinkLost);
        assert!(actions.is_empty());
        assert_eq!(engine.state(), ConnectionState::Disconnected);
        assert_eq!(engine.bpm(100), None);
        assert!(engine.poll(1_099).is_empty());
        assert_eq!(engine.poll(1_100), vec![OutAction::Reconnect]);
        assert_eq!(engine.state(), ConnectionState::Connecting);
    }

    #[test]
    fn disconnect_does_not_reconnect() {
        let mut engine = Engine::new();
        ready(&mut engine);
        assert_eq!(
            engine.handle(50, InEvent::Disconnect),
            vec![OutAction::Disconnect]
        );
        assert_eq!(engine.state(), ConnectionState::Disconnected);
        assert_eq!(engine.connected_name(), None);
        assert!(engine.poll(30_000).is_empty());
    }

    #[test]
    fn a_missing_measurement_fails_and_retries() {
        let mut engine = Engine::new();
        let _ = engine.handle(0, InEvent::Resolved { name: "Odd".into() });
        assert_eq!(
            engine.handle(0, InEvent::Ready { measurement: false }),
            vec![OutAction::Disconnect]
        );
        assert!(matches!(engine.state(), ConnectionState::Failed(_)));
        assert_eq!(engine.poll(1_000), vec![OutAction::Reconnect]);
    }

    #[test]
    fn not_found_does_not_retry_on_a_timer() {
        let mut engine = Engine::new();
        let _ = engine.handle(
            0,
            InEvent::NotFound {
                query: "nope".into(),
            },
        );
        assert!(
            matches!(engine.state(), ConnectionState::Failed(reason) if reason.contains("nope"))
        );
        assert!(engine.poll(60_000).is_empty());
    }
}
