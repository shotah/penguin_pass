//! Zwift controllers for Penguin Pass.
//!
//! [`spawn_ble_controllers`] scans for Zwift Click, Play, and Ride controllers
//! and, once [`ControllerLink::connect_all`] is called, keeps a link to each
//! one. The game thread only calls [`ControllerLink::drain_events`] once per
//! frame. That call does not wait on Bluetooth.
//!
//! [`SimControllers`] is the same interface with no radio, for tests and the
//! game's fake mode.
//!
//! The byte layout comes from the owner's bridge,
//! `shotah/zwift-to-mywhoosh-rust` commit `26c0dab`, files `src/zwift.rs` and
//! `src/ble.rs`. That repository is an application (it types MyWhoosh
//! shortcuts), not a library, and it has no licence file. This crate
//! reimplements the protocol. It does not depend on that repository.

#![forbid(unsafe_code)]

mod ble;
mod protocol;
mod sim;

pub use protocol::{
    PLAY_PADDLE_FULL, RIDE_ON, RIDE_PADDLE_PRESS, ZWIFT_ASYNC_CHAR, ZWIFT_CUSTOM_SERVICE,
    ZWIFT_MANUFACTURER_ID, ZWIFT_RIDE_CUSTOM_SERVICE, ZWIFT_SYNC_RX_CHAR, ZWIFT_SYNC_TX_CHAR,
};
pub use sim::SimControllers;

use std::fmt;

/// Which hand a controller sits on, when the advertisement says so.
///
/// Zwift Click v1 does not carry a side. Click v2, Play, and Ride do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Side {
    /// Left hood or left Click.
    Left,
    /// Right hood or right Click.
    Right,
}

impl Side {
    /// `left` or `right`, for logs.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Right => "right",
        }
    }
}

impl fmt::Display for Side {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.label())
    }
}

/// Which Zwift controller family an advertisement identified.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeviceKind {
    /// Zwift Click v1. One device, plus and minus, no side byte.
    Click,
    /// Zwift Click v2. The owner's bridge parses it with the Ride message.
    ///
    /// Their README says v2 stays silent until the Zwift app has unlocked it.
    ClickV2,
    /// Zwift Play, one BLE device per side.
    Play,
    /// Zwift Ride, one BLE device per side.
    Ride,
}

impl DeviceKind {
    /// Short name for logs and the scan tool.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Click => "Zwift Click",
            Self::ClickV2 => "Zwift Click V2",
            Self::Play => "Zwift Play",
            Self::Ride => "Zwift Ride",
        }
    }
}

impl fmt::Display for DeviceKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.label())
    }
}

/// A physical control.
///
/// Names follow the bit and field names in the owner's `src/zwift.rs`. Play's
/// face buttons are named for the right controller (`Y`, `Z`, `A`, `B`); the
/// left controller's pad is `Up`, `Down`, `Left`, and `Right`. Ride shift and
/// power buttons keep the left and right suffix from the bitmask. The owner's
/// MyWhoosh map collapses several of those bits into one gear key; this enum
/// does not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Button {
    /// Left Play pad up, or the Ride `UP` bit.
    Up,
    /// Left Play pad down, or the Ride `DOWN` bit.
    Down,
    /// Left Play pad left, or the Ride `LEFT` bit.
    Left,
    /// Left Play pad right, or the Ride `RIGHT` bit.
    Right,
    /// Right Play `A`, or the Ride `A` bit.
    A,
    /// Right Play `B`, or the Ride `B` bit.
    B,
    /// Right Play `Y`, or the Ride `Y` bit.
    Y,
    /// Right Play `Z`, or the Ride `Z` bit.
    Z,
    /// Zwift Click `+`.
    Plus,
    /// Zwift Click `-`.
    Minus,
    /// Right Play shift paddle, digital click or analog end-stop.
    ShiftUp,
    /// Left Play shift paddle, digital click or analog end-stop.
    ShiftDown,
    /// Ride bit `SHFT_UP_L`.
    ShiftUpLeft,
    /// Ride bit `SHFT_DN_L`.
    ShiftDownLeft,
    /// Ride bit `SHFT_UP_R`.
    ShiftUpRight,
    /// Ride bit `SHFT_DN_R`.
    ShiftDownRight,
    /// Ride bit `POWERUP_L`.
    PowerUpLeft,
    /// Ride bit `POWERUP_R`.
    PowerUpRight,
    /// Play power button. [`Side`] on the event says which controller.
    OnOff,
    /// Ride bit `ONOFF_L`.
    OnOffLeft,
    /// Ride bit `ONOFF_R`.
    OnOffRight,
}

/// Which analog axis a [`ControllerEvent::Paddle`] sample is.
///
/// These are shift paddles, not a steering axis. Steering on Play and Ride
/// is the digital pad (`Left` and `Right`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PaddleAxis {
    /// Play field `analog_lr`, or a Ride paddle (`location` selects the side).
    Lateral,
    /// Play field `analog_ud`. The owner's bridge reads this field and ignores it.
    Vertical,
}

/// Something the frame loop should apply.
///
/// `id` is the Bluetooth address from the advertisement, stable for a
/// session. Button events are edges: a held pad stays silent until it
/// changes. The wiring code keeps its own held set if steer or power should
/// repeat while the button is down.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControllerEvent {
    /// A control went down. `side` is the controller that sent it.
    ///
    /// For Play, `side` comes from the message's `right_pad` field (`0` is
    /// right, anything else is left), which is what the owner's parser uses.
    /// For every other device it is the side from the manufacturer byte.
    Pressed {
        /// Bluetooth address of the controller.
        id: String,
        /// Hand, or `None` for a Zwift Click v1.
        side: Option<Side>,
        /// The control that went down.
        button: Button,
    },
    /// A control came up. Same `side` rules as [`Self::Pressed`].
    Released {
        /// Bluetooth address of the controller.
        id: String,
        /// Hand, or `None` for a Zwift Click v1.
        side: Option<Side>,
        /// The control that came up.
        button: Button,
    },
    /// An analog shift paddle moved.
    ///
    /// `raw` is the device `sint32`. Play's end stop is ±[`PLAY_PADDLE_FULL`].
    /// The owner's Ride path treats an absolute value of at least
    /// [`RIDE_PADDLE_PRESS`] as a shift. Ride full-scale beyond that threshold
    /// is not in their code. A Play paddle at exactly ±100 also produces
    /// [`Button::ShiftUp`] or [`Button::ShiftDown`]; use one of the two when
    /// mapping to power, or a full pull will count twice.
    Paddle {
        /// Bluetooth address of the controller.
        id: String,
        /// Which paddle. Play uses the controller's side. Ride uses `location`.
        side: Side,
        /// Lateral paddle, or Play's unused vertical axis.
        axis: PaddleAxis,
        /// Signed device reading.
        raw: i32,
    },
    /// The handshake finished and button reports can arrive.
    Connected {
        /// Bluetooth address.
        id: String,
        /// Advertised name, or the address when the peripheral has none.
        name: String,
        /// Side from the manufacturer byte. Click v1 is `None`.
        side: Option<Side>,
        /// Controller family.
        kind: DeviceKind,
    },
    /// The link dropped, or the game asked to disconnect.
    Disconnected {
        /// Bluetooth address.
        id: String,
        /// Name captured when the link came up.
        name: String,
    },
    /// Battery percentage.
    ///
    /// The owner's decoder skips battery frames and does not document their
    /// layout, so the BLE link does not emit this. [`SimControllers`] can,
    /// for the wiring code to match on.
    Battery {
        /// Bluetooth address.
        id: String,
        /// Controller name.
        name: String,
        /// Charge, from 0 to 100.
        percent: u8,
    },
}

/// A Zwift controller seen while scanning. Connecting is separate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredController {
    /// Bluetooth address, for example `AA:BB:CC:DD:EE:FF`.
    pub id: String,
    /// Advertised name, or the address when the peripheral has none.
    pub name: String,
    /// Received signal strength, in dBm, when the adapter reported it.
    pub rssi_dbm: Option<i16>,
    /// Click, Play, or Ride.
    pub kind: DeviceKind,
    /// Hand, from the manufacturer type byte. `None` for Click v1.
    pub side: Option<Side>,
}

/// Where the controller session is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkState {
    /// Not scanning. [`SimControllers`] starts here.
    ///
    /// A BLE link uses this only after it has shut down.
    Idle,
    /// Looking for controllers. None are connected.
    Scanning,
    /// `connect_all` is on, nothing is up yet, and a controller is in range
    /// or a connection attempt is in flight.
    Connecting,
    /// `n` controllers finished the handshake and have not dropped.
    Connected(usize),
    /// The Bluetooth session failed. The string is a short reason for the log.
    Failed(String),
    /// This machine has no Bluetooth adapter.
    Unavailable,
}

impl fmt::Display for LinkState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Idle => formatter.write_str("idle"),
            Self::Scanning => formatter.write_str("scanning"),
            Self::Connecting => formatter.write_str("connecting"),
            Self::Connected(count) => write!(formatter, "connected ({count})"),
            Self::Failed(reason) => write!(formatter, "failed: {reason}"),
            Self::Unavailable => formatter.write_str("unavailable"),
        }
    }
}

/// What the frame loop needs from the controllers.
///
/// Methods return promptly. They do not wait on a radio, a socket, or a timer.
pub trait ControllerLink: Send {
    /// The current lifecycle state.
    fn state(&self) -> LinkState;

    /// Zwift controllers seen since the scan started, strongest signal first.
    fn discovered(&self) -> Vec<DiscoveredController>;

    /// Connect every discovered Zwift controller and keep retrying.
    ///
    /// Returns when the request is recorded. Poll [`Self::state`] for
    /// [`LinkState::Connected`] or [`LinkState::Failed`]. A later advertisement
    /// is connected too. Two controllers, left and right, stay up together.
    fn connect_all(&mut self);

    /// Drop every link and stop retrying. Scanning continues on a BLE link.
    fn disconnect(&mut self);

    /// Events queued since the last call, in arrival order.
    ///
    /// Non-blocking. The BLE queue keeps at most 512 events and drops the
    /// oldest past that, so a stalled frame loop cannot grow it without bound.
    fn drain_events(&mut self) -> Vec<ControllerEvent>;
}

/// Start a background scan for Zwift controllers.
///
/// The thread scans until the returned value is dropped. Nothing is connected
/// until [`ControllerLink::connect_all`].
///
/// # Errors
///
/// Returns an error when the background thread cannot be started. A missing
/// adapter shows up later as [`LinkState::Unavailable`].
pub fn spawn_ble_controllers() -> Result<Box<dyn ControllerLink>, String> {
    ble::spawn()
}
