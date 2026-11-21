//! Zwift controller bytes.
//!
//! Ported from `shotah/zwift-to-mywhoosh-rust` commit `26c0dab`:
//! `src/zwift.rs` (UUIDs, manufacturer type bytes, `RideOn`, message types,
//! protobuf fields, Ride bit masks, analog thresholds) and `src/ble.rs`
//! (write `RideOn` to sync RX, read button reports from the async
//! characteristic). That code has no key exchange and no encryption. Button
//! fields are active-low: `0` means pressed.
//!
//! There are no captured packets in that repository. The tests encode the
//! field numbers from `src/zwift.rs` and cite them next to the byte vector.

use std::collections::{BTreeMap, BTreeSet};

use uuid::Uuid;

use crate::{Button, DeviceKind, PaddleAxis, Side};

/// Zwift's Bluetooth company id (`src/zwift.rs` `ZWIFT_MANUFACTURER_ID`).
pub const ZWIFT_MANUFACTURER_ID: u16 = 2378;

/// Custom service `00000001-19ca-4651-86e5-fa29dcdd09d1`.
pub const ZWIFT_CUSTOM_SERVICE: Uuid = Uuid::from_u128(0x0000_0001_19ca_4651_86e5_fa29_dcdd_09d1);

/// Ride service `0000fc82-0000-1000-8000-00805f9b34fb`.
///
/// The owner's scanner does not filter on this. Identification is the
/// manufacturer byte. The UUID is here because `src/zwift.rs` defines it.
pub const ZWIFT_RIDE_CUSTOM_SERVICE: Uuid =
    Uuid::from_u128(0x0000_fc82_0000_1000_8000_0080_5f9b_34fb);

/// Async button reports. Notify. `00000002-19ca-4651-86e5-fa29dcdd09d1`.
pub const ZWIFT_ASYNC_CHAR: Uuid = Uuid::from_u128(0x0000_0002_19ca_4651_86e5_fa29_dcdd_09d1);

/// Sync RX. The host writes [`RIDE_ON`] here. `00000003-19ca-4651-86e5-fa29dcdd09d1`.
pub const ZWIFT_SYNC_RX_CHAR: Uuid = Uuid::from_u128(0x0000_0003_19ca_4651_86e5_fa29_dcdd_09d1);

/// Sync TX. The host subscribes. `00000004-19ca-4651-86e5-fa29dcdd09d1`.
pub const ZWIFT_SYNC_TX_CHAR: Uuid = Uuid::from_u128(0x0000_0004_19ca_4651_86e5_fa29_dcdd_09d1);

/// Handshake payload. ASCII `RideOn`. No further bytes are written.
pub const RIDE_ON: &[u8] = &[0x52, 0x69, 0x64, 0x65, 0x4F, 0x6E];

/// Play `analog_lr` magnitude that the owner's parser treats as a full shift.
pub const PLAY_PADDLE_FULL: i32 = 100;

/// Ride paddle magnitude the owner's parser treats as a shift.
///
/// Their code uses `abs >= 25`. It does not say what full-scale is.
pub const RIDE_PADDLE_PRESS: i32 = 25;

// Manufacturer type byte. `src/zwift.rs`.
const PLAY_RIGHT: u8 = 0x02;
const PLAY_LEFT: u8 = 0x03;
const RIDE_RIGHT: u8 = 0x07;
const RIDE_LEFT: u8 = 0x08;
const CLICK_V1: u8 = 0x09;
const CLICK_V2_RIGHT: u8 = 0x0A;
const CLICK_V2_LEFT: u8 = 0x0B;

// First byte of an async notification. `src/zwift.rs`.
const CLICK_NOTIFICATION: u8 = 0x37;
const CONTROLLER_NOTIFICATION: u8 = 0x07;

// Ride `button_map` masks. A clear bit is pressed. `src/zwift.rs` `ride_mask`.
const RIDE_LEFT_BIT: u32 = 0x0000_0001;
const RIDE_UP: u32 = 0x0000_0002;
const RIDE_RIGHT_BIT: u32 = 0x0000_0004;
const RIDE_DOWN: u32 = 0x0000_0008;
const RIDE_A: u32 = 0x0000_0010;
const RIDE_B: u32 = 0x0000_0020;
const RIDE_Y: u32 = 0x0000_0040;
const RIDE_Z: u32 = 0x0000_0080;
const RIDE_SHFT_UP_L: u32 = 0x0000_0100;
const RIDE_SHFT_DN_L: u32 = 0x0000_0200;
const RIDE_POWERUP_L: u32 = 0x0000_0400;
const RIDE_ONOFF_L: u32 = 0x0000_0800;
const RIDE_SHFT_UP_R: u32 = 0x0000_1000;
const RIDE_SHFT_DN_R: u32 = 0x0000_2000;
const RIDE_POWERUP_R: u32 = 0x0000_4000;
const RIDE_ONOFF_R: u32 = 0x0000_8000;

/// Family and hand from a manufacturer payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DeviceIdentity {
    pub kind: DeviceKind,
    pub side: Option<Side>,
}

/// Match the first manufacturer byte the way `DeviceType::from_manufacturer_byte` does.
#[must_use]
pub(crate) fn identify(manufacturer_id: u16, data: &[u8]) -> Option<DeviceIdentity> {
    if manufacturer_id != ZWIFT_MANUFACTURER_ID {
        return None;
    }
    let byte = *data.first()?;
    match byte {
        CLICK_V1 => Some(DeviceIdentity {
            kind: DeviceKind::Click,
            side: None,
        }),
        CLICK_V2_LEFT => Some(DeviceIdentity {
            kind: DeviceKind::ClickV2,
            side: Some(Side::Left),
        }),
        CLICK_V2_RIGHT => Some(DeviceIdentity {
            kind: DeviceKind::ClickV2,
            side: Some(Side::Right),
        }),
        PLAY_LEFT => Some(DeviceIdentity {
            kind: DeviceKind::Play,
            side: Some(Side::Left),
        }),
        PLAY_RIGHT => Some(DeviceIdentity {
            kind: DeviceKind::Play,
            side: Some(Side::Right),
        }),
        RIDE_LEFT => Some(DeviceIdentity {
            kind: DeviceKind::Ride,
            side: Some(Side::Left),
        }),
        RIDE_RIGHT => Some(DeviceIdentity {
            kind: DeviceKind::Ride,
            side: Some(Side::Right),
        }),
        _ => None,
    }
}

/// One decoded change, before the BLE layer stamps the device id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProtocolEvent {
    Pressed {
        side: Option<Side>,
        button: Button,
    },
    Released {
        side: Option<Side>,
        button: Button,
    },
    Paddle {
        side: Side,
        axis: PaddleAxis,
        raw: i32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Held {
    side: Option<Side>,
    button: Button,
}

struct Frame {
    buttons: BTreeSet<Held>,
    paddles: Vec<(Side, PaddleAxis, i32)>,
}

/// Remembers the last button and paddle sample for one controller.
///
/// A malformed body is ignored. Held buttons stay held, so a corrupt packet
/// does not look like a release of everything. The owner's `prost` decode
/// turns a decode error into an empty press list, which then releases every
/// button. That difference is deliberate.
#[derive(Debug, Default)]
pub(crate) struct InputDecoder {
    held: BTreeSet<Held>,
    paddles: BTreeMap<(Side, PaddleAxis), i32>,
}

impl InputDecoder {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Decode one async notification. Never panics.
    pub(crate) fn push(&mut self, identity: DeviceIdentity, raw: &[u8]) -> Vec<ProtocolEvent> {
        let Some(frame) = decode_frame(identity, raw) else {
            return Vec::new();
        };
        let mut events = diff_buttons(&mut self.held, &frame.buttons);
        events.extend(diff_paddles(&mut self.paddles, &frame.paddles));
        events
    }
}

fn diff_buttons(held: &mut BTreeSet<Held>, next: &BTreeSet<Held>) -> Vec<ProtocolEvent> {
    let mut events = Vec::new();
    for button in held.difference(next) {
        events.push(ProtocolEvent::Released {
            side: button.side,
            button: button.button,
        });
    }
    for button in next.difference(held) {
        events.push(ProtocolEvent::Pressed {
            side: button.side,
            button: button.button,
        });
    }
    *held = next.clone();
    events
}

fn diff_paddles(
    previous: &mut BTreeMap<(Side, PaddleAxis), i32>,
    paddles: &[(Side, PaddleAxis, i32)],
) -> Vec<ProtocolEvent> {
    let mut events = Vec::new();
    for &(side, axis, raw) in paddles {
        match previous.get(&(side, axis)).copied() {
            Some(prior) if prior == raw => {}
            None if raw == 0 => {
                previous.insert((side, axis), 0);
            }
            _ => {
                previous.insert((side, axis), raw);
                events.push(ProtocolEvent::Paddle { side, axis, raw });
            }
        }
    }
    events
}

fn decode_frame(identity: DeviceIdentity, raw: &[u8]) -> Option<Frame> {
    let (msg_type, body) = raw.split_first()?;
    match (identity.kind, *msg_type) {
        (DeviceKind::Click, CLICK_NOTIFICATION) => decode_click(body).map(frame_click),
        (DeviceKind::Play, CONTROLLER_NOTIFICATION) => decode_play(body).map(frame_play),
        (DeviceKind::Ride | DeviceKind::ClickV2, CONTROLLER_NOTIFICATION) => {
            decode_ride(body).map(|status| frame_ride(identity.side, status))
        }
        _ => None,
    }
}

struct ClickStatus {
    plus: i32,
    minus: i32,
}

struct PlayStatus {
    right_pad: i32,
    y_up: i32,
    z_left: i32,
    a_right: i32,
    b_down: i32,
    shift: i32,
    on: i32,
    analog_lr: i32,
    analog_ud: i32,
}

struct RideStatus {
    map: u32,
    paddles: Vec<(i32, i32)>,
}

fn frame_click(status: ClickStatus) -> Frame {
    let mut buttons = BTreeSet::new();
    if status.plus == 0 {
        buttons.insert(Held {
            side: None,
            button: Button::Plus,
        });
    }
    if status.minus == 0 {
        buttons.insert(Held {
            side: None,
            button: Button::Minus,
        });
    }
    Frame {
        buttons,
        paddles: Vec::new(),
    }
}

fn frame_play(status: PlayStatus) -> Frame {
    // `src/zwift.rs`: `right_pad == 0` is the right controller.
    let side = if status.right_pad == 0 {
        Side::Right
    } else {
        Side::Left
    };
    let mut buttons = BTreeSet::new();
    let mut press = |active, button| {
        if active {
            buttons.insert(Held {
                side: Some(side),
                button,
            });
        }
    };
    let analog_click = status.analog_lr.abs() == PLAY_PADDLE_FULL;
    if side == Side::Right {
        press(status.y_up == 0, Button::Y);
        press(status.z_left == 0, Button::Z);
        press(status.a_right == 0, Button::A);
        press(status.b_down == 0, Button::B);
        press(status.shift == 0 || analog_click, Button::ShiftUp);
    } else {
        press(status.y_up == 0, Button::Up);
        press(status.z_left == 0, Button::Left);
        press(status.a_right == 0, Button::Right);
        press(status.b_down == 0, Button::Down);
        press(status.shift == 0 || analog_click, Button::ShiftDown);
    }
    press(status.on == 0, Button::OnOff);
    Frame {
        buttons,
        paddles: vec![
            (side, PaddleAxis::Lateral, status.analog_lr),
            (side, PaddleAxis::Vertical, status.analog_ud),
        ],
    }
}

fn frame_ride(device_side: Option<Side>, status: RideStatus) -> Frame {
    let mut buttons = BTreeSet::new();
    let mut press = |mask, button| {
        if (status.map & mask) == 0 {
            buttons.insert(Held {
                side: device_side,
                button,
            });
        }
    };
    press(RIDE_LEFT_BIT, Button::Left);
    press(RIDE_UP, Button::Up);
    press(RIDE_RIGHT_BIT, Button::Right);
    press(RIDE_DOWN, Button::Down);
    press(RIDE_A, Button::A);
    press(RIDE_B, Button::B);
    press(RIDE_Y, Button::Y);
    press(RIDE_Z, Button::Z);
    press(RIDE_SHFT_UP_L, Button::ShiftUpLeft);
    press(RIDE_SHFT_DN_L, Button::ShiftDownLeft);
    press(RIDE_POWERUP_L, Button::PowerUpLeft);
    press(RIDE_ONOFF_L, Button::OnOffLeft);
    press(RIDE_SHFT_UP_R, Button::ShiftUpRight);
    press(RIDE_SHFT_DN_R, Button::ShiftDownRight);
    press(RIDE_POWERUP_R, Button::PowerUpRight);
    press(RIDE_ONOFF_R, Button::OnOffRight);

    let mut paddles = Vec::new();
    for (location, raw) in status.paddles {
        let Some(side) = paddle_side(location) else {
            continue;
        };
        paddles
            .retain(|(paddle_side, axis, _)| *paddle_side != side || *axis != PaddleAxis::Lateral);
        paddles.push((side, PaddleAxis::Lateral, raw));
    }
    Frame { buttons, paddles }
}

fn paddle_side(location: i32) -> Option<Side> {
    // `src/zwift.rs` `RideAnalogKeyPress`: 0 is left, 1 is right.
    match location {
        0 => Some(Side::Left),
        1 => Some(Side::Right),
        _ => None,
    }
}

fn decode_click(data: &[u8]) -> Option<ClickStatus> {
    // ClickKeyPadStatus: int32 button_plus = 1, button_minus = 2.
    let mut plus = 0;
    let mut minus = 0;
    walk(
        data,
        |field, value| match field {
            1 => plus = varint_i32(value),
            2 => minus = varint_i32(value),
            _ => {}
        },
        |_, _| Some(()),
    )?;
    Some(ClickStatus { plus, minus })
}

fn decode_play(data: &[u8]) -> Option<PlayStatus> {
    // PlayKeyPadStatus fields 1-7 are int32, 8 and 9 are sint32.
    let mut status = PlayStatus {
        right_pad: 0,
        y_up: 0,
        z_left: 0,
        a_right: 0,
        b_down: 0,
        shift: 0,
        on: 0,
        analog_lr: 0,
        analog_ud: 0,
    };
    walk(
        data,
        |field, value| match field {
            1 => status.right_pad = varint_i32(value),
            2 => status.y_up = varint_i32(value),
            3 => status.z_left = varint_i32(value),
            4 => status.a_right = varint_i32(value),
            5 => status.b_down = varint_i32(value),
            6 => status.shift = varint_i32(value),
            7 => status.on = varint_i32(value),
            8 => status.analog_lr = zigzag_i32(value),
            9 => status.analog_ud = zigzag_i32(value),
            _ => {}
        },
        |_, _| Some(()),
    )?;
    Some(status)
}

fn decode_ride(data: &[u8]) -> Option<RideStatus> {
    // RideKeyPadStatus: uint32 button_map = 1, repeated RideAnalogKeyPress paddles = 3.
    let mut map = 0;
    let mut paddles = Vec::new();
    walk(
        data,
        |field, value| {
            if field == 1 {
                map = value as u32;
            }
        },
        |field, bytes| {
            if field == 3 && paddles.len() < 8 {
                paddles.push(decode_paddle(bytes)?);
            }
            Some(())
        },
    )?;
    Some(RideStatus { map, paddles })
}

fn decode_paddle(data: &[u8]) -> Option<(i32, i32)> {
    // RideAnalogKeyPress: int32 location = 1, sint32 analog_value = 2.
    let mut location = 0;
    let mut analog = 0;
    walk(
        data,
        |field, value| match field {
            1 => location = varint_i32(value),
            2 => analog = zigzag_i32(value),
            _ => {}
        },
        |_, _| Some(()),
    )?;
    Some((location, analog))
}

fn varint_i32(value: u64) -> i32 {
    value as i32
}

fn zigzag_i32(value: u64) -> i32 {
    let bits = value as u32;
    ((bits >> 1) as i32) ^ -((bits & 1) as i32)
}

/// Walk a protobuf message. Returns `None` when the bytes are truncated or
/// use a wire type this decoder will not skip. Unknown fields of a known
/// wire type are skipped.
fn walk<F, G>(data: &[u8], mut on_varint: F, mut on_bytes: G) -> Option<()>
where
    F: FnMut(u32, u64),
    G: FnMut(u32, &[u8]) -> Option<()>,
{
    let mut index = 0;
    while index < data.len() {
        let tag = read_varint(data, &mut index)?;
        let field = u32::try_from(tag >> 3).ok()?;
        match tag & 7 {
            0 => {
                let value = read_varint(data, &mut index)?;
                on_varint(field, value);
            }
            1 => {
                index = index.checked_add(8)?;
                if index > data.len() {
                    return None;
                }
            }
            2 => {
                let len = usize::try_from(read_varint(data, &mut index)?).ok()?;
                let end = index.checked_add(len)?;
                if end > data.len() {
                    return None;
                }
                on_bytes(field, &data[index..end])?;
                index = end;
            }
            5 => {
                index = index.checked_add(4)?;
                if index > data.len() {
                    return None;
                }
            }
            _ => return None,
        }
    }
    Some(())
}

fn read_varint(data: &[u8], index: &mut usize) -> Option<u64> {
    let mut value = 0u64;
    for step in 0u32..10 {
        let byte = *data.get(*index)?;
        *index += 1;
        if step == 9 {
            if byte > 1 {
                return None;
            }
            if byte == 1 {
                value |= 1_u64 << 63;
            }
            return Some(value);
        }
        value |= u64::from(byte & 0x7F) << (step * 7);
        if byte & 0x80 == 0 {
            return Some(value);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn click() -> DeviceIdentity {
        DeviceIdentity {
            kind: DeviceKind::Click,
            side: None,
        }
    }

    fn play() -> DeviceIdentity {
        DeviceIdentity {
            kind: DeviceKind::Play,
            side: Some(Side::Left),
        }
    }

    fn ride(side: Side) -> DeviceIdentity {
        DeviceIdentity {
            kind: DeviceKind::Ride,
            side: Some(side),
        }
    }

    #[test]
    fn handshake_is_the_ride_on_bytes_from_zwift_rs() {
        // src/zwift.rs: `const RIDE_ON: &[u8] = &[0x52, 0x69, 0x64, 0x65, 0x4F, 0x6E];`
        assert_eq!(RIDE_ON, b"RideOn");
        assert_eq!(RIDE_ON, [0x52, 0x69, 0x64, 0x65, 0x4F, 0x6E]);
    }

    #[test]
    fn service_uuids_match_zwift_rs() {
        // src/zwift.rs `Uuid::from_bytes` lists, byte 0 first.
        assert_eq!(
            ZWIFT_CUSTOM_SERVICE.to_string(),
            "00000001-19ca-4651-86e5-fa29dcdd09d1"
        );
        assert_eq!(
            ZWIFT_ASYNC_CHAR.to_string(),
            "00000002-19ca-4651-86e5-fa29dcdd09d1"
        );
        assert_eq!(
            ZWIFT_SYNC_RX_CHAR.to_string(),
            "00000003-19ca-4651-86e5-fa29dcdd09d1"
        );
        assert_eq!(
            ZWIFT_SYNC_TX_CHAR.to_string(),
            "00000004-19ca-4651-86e5-fa29dcdd09d1"
        );
        assert_eq!(
            ZWIFT_RIDE_CUSTOM_SERVICE.to_string(),
            "0000fc82-0000-1000-8000-00805f9b34fb"
        );
        assert_eq!(ZWIFT_MANUFACTURER_ID, 2378);
    }

    #[test]
    fn manufacturer_bytes_match_zwift_rs() {
        // src/zwift.rs: PLAY_RIGHT 0x02, PLAY_LEFT 0x03, RIDE_RIGHT 0x07,
        // RIDE_LEFT 0x08, CLICK_V1 0x09, CLICK_V2_RIGHT 0x0A, CLICK_V2_LEFT 0x0B.
        let cases = [
            (0x09_u8, DeviceKind::Click, None),
            (0x0B, DeviceKind::ClickV2, Some(Side::Left)),
            (0x0A, DeviceKind::ClickV2, Some(Side::Right)),
            (0x03, DeviceKind::Play, Some(Side::Left)),
            (0x02, DeviceKind::Play, Some(Side::Right)),
            (0x08, DeviceKind::Ride, Some(Side::Left)),
            (0x07, DeviceKind::Ride, Some(Side::Right)),
        ];
        for (byte, kind, side) in cases {
            let identity = identify(ZWIFT_MANUFACTURER_ID, &[byte]).unwrap();
            assert_eq!(identity.kind, kind, "byte {byte:#x}");
            assert_eq!(identity.side, side, "byte {byte:#x}");
        }
        assert!(identify(ZWIFT_MANUFACTURER_ID, &[]).is_none());
        assert!(identify(ZWIFT_MANUFACTURER_ID, &[0x01]).is_none());
        assert!(identify(0, &[0x03]).is_none());
    }

    #[test]
    fn click_edges_follow_the_zero_is_pressed_rule() {
        // Same vectors as `click_plus_press_and_release`, kept as the check.
        const PLUS_PRESSED: &[u8] = &[0x37, 0x08, 0x00, 0x10, 0x01];
        const BOTH_RELEASED: &[u8] = &[0x37, 0x08, 0x01, 0x10, 0x01];
        const BOTH_PRESSED: &[u8] = &[0x37, 0x08, 0x00, 0x10, 0x00];

        let mut decoder = InputDecoder::new();
        assert_eq!(
            decoder.push(click(), PLUS_PRESSED),
            vec![ProtocolEvent::Pressed {
                side: None,
                button: Button::Plus,
            }]
        );
        assert_eq!(
            decoder.push(click(), BOTH_RELEASED),
            vec![ProtocolEvent::Released {
                side: None,
                button: Button::Plus,
            }]
        );
        let both = decoder.push(click(), BOTH_PRESSED);
        assert_eq!(
            both,
            vec![
                ProtocolEvent::Pressed {
                    side: None,
                    button: Button::Plus,
                },
                ProtocolEvent::Pressed {
                    side: None,
                    button: Button::Minus,
                },
            ]
        );
    }

    #[test]
    fn play_right_a_and_left_pad_follow_zwift_rs() {
        // src/zwift.rs PlayKeyPadStatus. right_pad == 0 is the right controller.
        // Fields: 1 right_pad, 2 y/up, 3 z/left, 4 a/right, 5 b/down,
        // 6 shift, 7 on, 8 analog_lr (sint32), 9 analog_ud (sint32).
        // Type byte PLAY_NOTIFICATION = 0x07. 0 is pressed.
        // Right controller, only A pressed, analogs at 0.
        const RIGHT_A: &[u8] = &[
            0x07, 0x08, 0x00, 0x10, 0x01, 0x18, 0x01, 0x20, 0x00, 0x28, 0x01, 0x30, 0x01, 0x38,
            0x01, 0x40, 0x00, 0x48, 0x00,
        ];
        // Left controller (right_pad = 1), only z/left pressed.
        const LEFT_PAD: &[u8] = &[
            0x07, 0x08, 0x01, 0x10, 0x01, 0x18, 0x00, 0x20, 0x01, 0x28, 0x01, 0x30, 0x01, 0x38,
            0x01, 0x40, 0x00, 0x48, 0x00,
        ];

        let mut decoder = InputDecoder::new();
        assert_eq!(
            decoder.push(play(), RIGHT_A),
            vec![ProtocolEvent::Pressed {
                side: Some(Side::Right),
                button: Button::A,
            }]
        );
        let mut other = InputDecoder::new();
        assert_eq!(
            other.push(play(), LEFT_PAD),
            vec![ProtocolEvent::Pressed {
                side: Some(Side::Left),
                button: Button::Left,
            }]
        );
    }

    #[test]
    fn play_analog_end_stop_is_a_shift_and_a_paddle() {
        // src/zwift.rs: `analog_lr.abs() == 100` is a shift, and the right
        // controller's shift is `InGameAction::ShiftUp`. sint32 -100 zigzag
        // encodes as 199 (0xC7). Field 8 tag is 0x40.
        // right_pad = 0, buttons released (1), analog_lr = -100, analog_ud = 0.
        const RIGHT_PULL: &[u8] = &[
            0x07, 0x08, 0x00, 0x10, 0x01, 0x18, 0x01, 0x20, 0x01, 0x28, 0x01, 0x30, 0x01, 0x38,
            0x01, 0x40, 0xC7, 0x01, 0x48, 0x00,
        ];
        // +99 is not the end stop. Zigzag 99 is 198, varint C6 01.
        const NOT_FULL: &[u8] = &[
            0x07, 0x08, 0x00, 0x10, 0x01, 0x18, 0x01, 0x20, 0x01, 0x28, 0x01, 0x30, 0x01, 0x38,
            0x01, 0x40, 0xC6, 0x01, 0x48, 0x00,
        ];

        let mut decoder = InputDecoder::new();
        assert_eq!(
            decoder.push(play(), RIGHT_PULL),
            vec![
                ProtocolEvent::Pressed {
                    side: Some(Side::Right),
                    button: Button::ShiftUp,
                },
                ProtocolEvent::Paddle {
                    side: Side::Right,
                    axis: PaddleAxis::Lateral,
                    raw: -100,
                },
            ]
        );
        let mut other = InputDecoder::new();
        assert_eq!(
            other.push(play(), NOT_FULL),
            vec![ProtocolEvent::Paddle {
                side: Side::Right,
                axis: PaddleAxis::Lateral,
                raw: 99,
            }]
        );
    }

    #[test]
    fn ride_left_bit_is_active_low() {
        // src/zwift.rs ride_mask::LEFT = 0x1, pressed when the bit is clear.
        // All 16 known bits set is 0xFFFF (nothing pressed): tag 1, varint
        // FF FF 03. Clearing bit 0 is 0xFFFE: FE FF 03.
        const RELEASED: &[u8] = &[0x07, 0x08, 0xFF, 0xFF, 0x03];
        const LEFT_DOWN: &[u8] = &[0x07, 0x08, 0xFE, 0xFF, 0x03];

        let mut decoder = InputDecoder::new();
        assert!(decoder.push(ride(Side::Left), RELEASED).is_empty());
        assert_eq!(
            decoder.push(ride(Side::Left), LEFT_DOWN),
            vec![ProtocolEvent::Pressed {
                side: Some(Side::Left),
                button: Button::Left,
            }]
        );
        assert_eq!(
            decoder.push(ride(Side::Left), RELEASED),
            vec![ProtocolEvent::Released {
                side: Some(Side::Left),
                button: Button::Left,
            }]
        );
    }

    #[test]
    fn ride_paddle_is_not_a_second_button() {
        // src/zwift.rs RideAnalogKeyPress location = 1, analog_value = 2.
        // location 1 is the right paddle. Threshold is 25, but this crate
        // reports the sample and leaves the bit buttons to the bitmask.
        // Map 0xFFFF, then field 3: location 1, zigzag(25) = 50 = 0x32.
        const PADDLE: &[u8] = &[
            0x07, 0x08, 0xFF, 0xFF, 0x03, 0x1A, 0x04, 0x08, 0x01, 0x10, 0x32,
        ];
        let mut decoder = InputDecoder::new();
        assert_eq!(
            decoder.push(ride(Side::Right), PADDLE),
            vec![ProtocolEvent::Paddle {
                side: Side::Right,
                axis: PaddleAxis::Lateral,
                raw: 25,
            }]
        );
        assert_eq!(RIDE_PADDLE_PRESS, 25);
        assert_eq!(PLAY_PADDLE_FULL, 100);
    }

    #[test]
    fn click_v2_uses_the_ride_message() {
        // src/zwift.rs parses ClickV2Left/Right with `parse_ride` and type 0x07.
        const LEFT_DOWN: &[u8] = &[0x07, 0x08, 0xFE, 0xFF, 0x03];
        let identity = DeviceIdentity {
            kind: DeviceKind::ClickV2,
            side: Some(Side::Right),
        };
        let mut decoder = InputDecoder::new();
        assert_eq!(
            decoder.push(identity, LEFT_DOWN),
            vec![ProtocolEvent::Pressed {
                side: Some(Side::Right),
                button: Button::Left,
            }]
        );
    }

    #[test]
    fn a_corrupt_body_does_not_release_a_held_button() {
        const PLUS_PRESSED: &[u8] = &[0x37, 0x08, 0x00, 0x10, 0x01];
        let mut decoder = InputDecoder::new();
        assert_eq!(decoder.push(click(), PLUS_PRESSED).len(), 1);

        // Truncated varint, huge length, bad wire type, empty, wrong type.
        let junk: &[&[u8]] = &[
            &[],
            &[0x37, 0x80],
            &[0x37, 0x0A, 0x7F],
            &[0x37, 0x0F],
            &[0x00, 0xFF, 0xFF],
            &[0x07],
            &[0xFF; 32],
            // A bare 0x37 is a valid empty Click message (both buttons
            // pressed). It is covered by `empty_button_body_uses_protobuf_defaults`
            // and is not junk.
        ];
        for sample in junk {
            assert!(
                decoder.push(click(), sample).is_empty(),
                "junk {sample:?} changed state"
            );
        }
        const BOTH_RELEASED: &[u8] = &[0x37, 0x08, 0x01, 0x10, 0x01];
        assert_eq!(
            decoder.push(click(), BOTH_RELEASED),
            vec![ProtocolEvent::Released {
                side: None,
                button: Button::Plus,
            }]
        );
    }

    #[test]
    fn arbitrary_bytes_do_not_panic() {
        let samples: &[&[u8]] = &[
            &[],
            &[0x00],
            &[0x37],
            &[0x07],
            &[
                0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x02,
            ],
            &[0x07, 0x1A, 0xFF, 0xFF, 0xFF, 0xFF, 0x0F],
            &[
                0x37, 0x08, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x01, 0x10, 0x01,
            ],
        ];
        let identities = [
            click(),
            play(),
            ride(Side::Left),
            DeviceIdentity {
                kind: DeviceKind::ClickV2,
                side: Some(Side::Left),
            },
        ];
        for identity in identities {
            let mut decoder = InputDecoder::new();
            for sample in samples {
                let _ = decoder.push(identity, sample);
            }
        }
    }

    #[test]
    fn skipped_wire_types_do_not_hide_a_later_field() {
        // Field 1 = 0, then a fixed32 field 10 (tag 0x55, four zero bytes),
        // then field 2 = 1. Plus is pressed, minus is not.
        const WITH_FIXED32: &[u8] = &[0x37, 0x08, 0x00, 0x55, 0x00, 0x00, 0x00, 0x00, 0x10, 0x01];
        let mut decoder = InputDecoder::new();
        assert_eq!(
            decoder.push(click(), WITH_FIXED32),
            vec![ProtocolEvent::Pressed {
                side: None,
                button: Button::Plus,
            }]
        );
    }

    #[test]
    fn empty_button_body_uses_protobuf_defaults() {
        // An empty protobuf message leaves every int at 0, and 0 is pressed.
        // src/zwift.rs returns that from a successful decode. Idle frames in
        // the owner's comments are a different message type, not an empty 0x07.
        let mut decoder = InputDecoder::new();
        let events = decoder.push(click(), &[CLICK_NOTIFICATION]);
        assert_eq!(events.len(), 2);

        let mut ride_decoder = InputDecoder::new();
        let events = ride_decoder.push(ride(Side::Left), &[CONTROLLER_NOTIFICATION]);
        assert_eq!(events.len(), 16);
    }
}
