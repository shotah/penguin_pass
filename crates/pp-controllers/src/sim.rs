//! A controller link with no radio.
//!
//! Tests and the game's fake mode push events and a discovered list. Nothing
//! here sleeps or opens a socket.

use crate::{ControllerEvent, ControllerLink, DiscoveredController, LinkState};

/// Scripted stand-in for [`crate::spawn_ble_controllers`].
///
/// Starts [`LinkState::Idle`]. [`ControllerLink::connect_all`] marks every
/// discovered controller connected and queues a [`ControllerEvent::Connected`]
/// for each one that was not already up. [`ControllerLink::disconnect`] queues
/// the matching [`ControllerEvent::Disconnected`] events.
#[derive(Debug, Clone)]
pub struct SimControllers {
    state: LinkState,
    discovered: Vec<DiscoveredController>,
    connected: Vec<DiscoveredController>,
    events: Vec<ControllerEvent>,
}

impl SimControllers {
    /// An idle link with nothing discovered.
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: LinkState::Idle,
            discovered: Vec::new(),
            connected: Vec::new(),
            events: Vec::new(),
        }
    }

    /// Queue an event for the next [`ControllerLink::drain_events`] call.
    pub fn push(&mut self, event: ControllerEvent) {
        self.events.push(event);
    }

    /// Replace the discovered list. Does not connect or disconnect by itself.
    pub fn set_discovered(&mut self, found: Vec<DiscoveredController>) {
        self.discovered = found;
    }

    /// [`LinkState::Failed`]. Later `connect_all` calls do nothing.
    pub fn fail(&mut self, reason: impl Into<String>) {
        self.state = LinkState::Failed(reason.into());
    }

    /// [`LinkState::Unavailable`]. Later `connect_all` calls do nothing.
    pub fn unavailable(&mut self) {
        self.state = LinkState::Unavailable;
    }
}

impl Default for SimControllers {
    fn default() -> Self {
        Self::new()
    }
}

impl ControllerLink for SimControllers {
    fn state(&self) -> LinkState {
        self.state.clone()
    }

    fn discovered(&self) -> Vec<DiscoveredController> {
        self.discovered.clone()
    }

    fn connect_all(&mut self) {
        if matches!(self.state, LinkState::Failed(_) | LinkState::Unavailable) {
            return;
        }
        if self.discovered.is_empty() {
            self.state = LinkState::Scanning;
            return;
        }
        for device in &self.discovered {
            if self.connected.iter().any(|live| live.id == device.id) {
                continue;
            }
            self.events.push(ControllerEvent::Connected {
                id: device.id.clone(),
                name: device.name.clone(),
                side: device.side,
                kind: device.kind,
            });
            self.connected.push(device.clone());
        }
        self.state = LinkState::Connected(self.connected.len());
    }

    fn disconnect(&mut self) {
        for device in self.connected.drain(..) {
            self.events.push(ControllerEvent::Disconnected {
                id: device.id,
                name: device.name,
            });
        }
        if !matches!(self.state, LinkState::Failed(_) | LinkState::Unavailable) {
            self.state = LinkState::Idle;
        }
    }

    fn drain_events(&mut self) -> Vec<ControllerEvent> {
        std::mem::take(&mut self.events)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Button, DeviceKind, Side};

    fn play(id: &str, side: Side) -> DiscoveredController {
        DiscoveredController {
            id: id.to_string(),
            name: format!("Play {id}"),
            rssi_dbm: Some(-50),
            kind: DeviceKind::Play,
            side: Some(side),
        }
    }

    #[test]
    fn connect_all_then_disconnect_is_scripted() {
        let mut sim = SimControllers::new();
        assert_eq!(sim.state(), LinkState::Idle);
        assert!(sim.drain_events().is_empty());

        sim.set_discovered(vec![play("AA", Side::Left), play("BB", Side::Right)]);
        sim.connect_all();
        assert_eq!(sim.state(), LinkState::Connected(2));
        let events = sim.drain_events();
        assert!(matches!(
            &events[0],
            ControllerEvent::Connected {
                side: Some(Side::Left),
                ..
            }
        ));
        assert!(matches!(
            &events[1],
            ControllerEvent::Connected {
                side: Some(Side::Right),
                ..
            }
        ));

        sim.connect_all();
        assert!(sim.drain_events().is_empty());

        sim.push(ControllerEvent::Pressed {
            id: "AA".into(),
            side: Some(Side::Left),
            button: Button::Left,
        });
        sim.push(ControllerEvent::Battery {
            id: "AA".into(),
            name: "Play AA".into(),
            percent: 80,
        });
        assert_eq!(sim.drain_events().len(), 2);
        assert!(sim.drain_events().is_empty());

        sim.disconnect();
        assert_eq!(sim.state(), LinkState::Idle);
        let events = sim.drain_events();
        assert_eq!(events.len(), 2);
        assert!(matches!(events[0], ControllerEvent::Disconnected { .. }));
    }

    #[test]
    fn a_failed_link_does_not_connect() {
        let mut sim = SimControllers::new();
        sim.set_discovered(vec![play("AA", Side::Left)]);
        sim.fail("radio off");
        sim.connect_all();
        assert_eq!(sim.state(), LinkState::Failed("radio off".into()));
        assert!(sim.drain_events().is_empty());
        sim.unavailable();
        sim.connect_all();
        assert_eq!(sim.state(), LinkState::Unavailable);
    }

    #[test]
    fn connect_with_nothing_in_range_scans() {
        let mut sim = SimControllers::new();
        sim.connect_all();
        assert_eq!(sim.state(), LinkState::Scanning);
    }
}
