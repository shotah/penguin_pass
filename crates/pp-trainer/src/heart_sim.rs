//! A scripted heart-rate strap for tests and the game's fake mode.
//!
//! It does not open a radio. The caller decides what is nearby, what the
//! strap reports, and when the link drops.

use crate::{ConnectionState, DiscoveredSensor, HeartRateLink};

/// A heart-rate strap whose advertisement, connection, and bpm are set by the caller.
///
/// [`HeartRateLink::bpm`] is the value from [`Self::set_bpm`] while the strap
/// is connected, and `None` after [`Self::drop_link`] or [`HeartRateLink::disconnect`].
/// The sim does not reconnect on its own.
#[derive(Debug, Clone)]
pub struct SimHeartRate {
    state: ConnectionState,
    discovered: Vec<DiscoveredSensor>,
    connected: Option<DiscoveredSensor>,
    bpm: Option<u16>,
}

impl SimHeartRate {
    /// Scanning, with nothing discovered and no bpm.
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: ConnectionState::Scanning,
            discovered: Vec::new(),
            connected: None,
            bpm: None,
        }
    }

    /// Replace the straps a scan would have found.
    pub fn set_discovered(&mut self, sensors: Vec<DiscoveredSensor>) {
        self.discovered = sensors;
    }

    /// The bpm reported while connected. `None` is no skin contact or no sample.
    pub fn set_bpm(&mut self, bpm: Option<u16>) {
        self.bpm = bpm;
    }

    /// Drop the link. The bpm stays stored for a later [`HeartRateLink::connect`].
    pub fn drop_link(&mut self) {
        self.connected = None;
        self.state = ConnectionState::Disconnected;
    }
}

impl Default for SimHeartRate {
    fn default() -> Self {
        Self::new()
    }
}

impl HeartRateLink for SimHeartRate {
    fn state(&self) -> ConnectionState {
        self.state.clone()
    }

    fn discovered(&self) -> Vec<DiscoveredSensor> {
        self.discovered.clone()
    }

    fn connect(&mut self, id_or_name: &str) {
        let query = id_or_name.trim();
        if query.is_empty() {
            self.state = ConnectionState::Failed("heart-rate strap id or name is empty".into());
            self.connected = None;
            return;
        }
        let needle = query.to_ascii_lowercase();
        let found = self.discovered.iter().find(|sensor| {
            sensor.id.eq_ignore_ascii_case(query)
                || sensor.name.to_ascii_lowercase().contains(&needle)
        });
        match found.cloned() {
            Some(sensor) => {
                self.connected = Some(sensor);
                self.state = ConnectionState::Connected;
            }
            None => {
                self.connected = None;
                self.state =
                    ConnectionState::Failed(format!("no heart-rate strap matches {query}"));
            }
        }
    }

    fn disconnect(&mut self) {
        self.connected = None;
        self.state = ConnectionState::Disconnected;
    }

    fn bpm(&self) -> Option<u16> {
        if self.state != ConnectionState::Connected {
            return None;
        }
        self.bpm
    }

    fn name(&self) -> Option<String> {
        self.connected.as_ref().map(|sensor| sensor.name.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strap() -> DiscoveredSensor {
        DiscoveredSensor {
            id: "AA:BB:CC:DD:EE:FF".into(),
            name: "HRM-Dual".into(),
            rssi_dbm: Some(-60),
        }
    }

    #[test]
    fn connect_reports_bpm_until_drop_or_disconnect() {
        let mut sim = SimHeartRate::new();
        assert!(sim.discovered().is_empty());
        assert_eq!(sim.state(), ConnectionState::Scanning);
        sim.set_discovered(vec![strap()]);
        sim.set_bpm(Some(88));
        assert_eq!(sim.bpm(), None);

        sim.connect("hrm");
        assert_eq!(sim.state(), ConnectionState::Connected);
        assert_eq!(sim.name().as_deref(), Some("HRM-Dual"));
        assert_eq!(sim.bpm(), Some(88));

        sim.set_bpm(None);
        assert_eq!(sim.bpm(), None);
        sim.set_bpm(Some(90));
        assert_eq!(sim.bpm(), Some(90));

        sim.drop_link();
        assert_eq!(sim.state(), ConnectionState::Disconnected);
        assert_eq!(sim.bpm(), None);
        assert_eq!(sim.name(), None);

        sim.connect("AA:BB:CC:DD:EE:FF");
        assert_eq!(sim.bpm(), Some(90));
        sim.disconnect();
        assert_eq!(sim.state(), ConnectionState::Disconnected);
        assert_eq!(sim.bpm(), None);
    }

    #[test]
    fn a_missing_strap_fails_without_a_bpm() {
        let mut sim = SimHeartRate::new();
        sim.set_bpm(Some(70));
        sim.connect("missing");
        assert!(matches!(sim.state(), ConnectionState::Failed(_)));
        assert_eq!(sim.bpm(), None);
        sim.connect("  ");
        assert!(matches!(sim.state(), ConnectionState::Failed(_)));
    }
}
