# Zwift controllers

Penguin Pass can take steering, menus, and a few ride actions from a Zwift
Click, Play, or Ride. `pp-controllers` discovers those controllers over
Bluetooth LE and turns their reports into plain events. Nothing in the game
calls it yet. The next slice maps the events onto actions.

The byte layout is the one in the owner's bridge,
[zwift-to-mywhoosh-rust](https://github.com/shotah/zwift-to-mywhoosh-rust)
commit `26c0dabac2cad065d2ec741f33df0aafa1a7bbd8`, files `src/zwift.rs` and
`src/ble.rs`. That repository is an application (it types MyWhoosh
shortcuts). It is not a library, it has no licence file, and it depends on
`btleplug` 0.11 plus `enigo`. This workspace is on `btleplug` 0.13. A git
dependency would pull the keyboard bridge into the game, so the protocol is
reimplemented here. No new crates were added. Bluetooth uses the same
`btleplug`, `tokio`, `uuid`, and `futures-util` versions as `pp-trainer`.
The four protobuf messages are decoded in `crates/pp-controllers/src/protocol.rs`
instead of taking `prost`.

## Supported devices

`src/zwift.rs` names the devices and the first manufacturer-data byte.
Company id `2378`.

| Device | Byte | Owner's README |
| --- | --- | --- |
| Zwift Click v1 | `0x09` | Supported. No side. |
| Zwift Play, right | `0x02` | Supported |
| Zwift Play, left | `0x03` | Supported |
| Zwift Ride, right | `0x07` | Supported |
| Zwift Ride, left | `0x08` | Supported |
| Zwift Click v2, right | `0x0A` | Partial. Needs an unlock from the Zwift app first |
| Zwift Click v2, left | `0x0B` | Same |

Click v2 is parsed with the Ride message, which is what `parse_notification`
does. Two controllers, a left and a right, can be connected together.

## How a connection will work

Wake the controller. It must not already be connected to Zwift, Companion,
or the MyWhoosh bridge. One controller accepts one BLE central.

`spawn_ble_controllers` scans and does not connect. `connect_all` connects
every Zwift controller on the list, and any that show up later. A dropped
link is tried again about a second after the controller is advertising.
`disconnect` drops the links and stops retrying. The scan keeps going until
the handle is dropped.

The frame loop calls `drain_events` once per frame. That call does not wait
on the radio. The queue holds at most 512 events and drops the oldest past
that.

The owner's code never bonds and never writes anything except the six-byte
handshake. Whether BlueZ still pops a pairing dialog on the first connect
has not been tried.

List what is nearby without connecting:

```text
target/debug/pp-controller-scan
target/debug/pp-controller-scan --scan-seconds 8
```

`./x` does not grow a `controllers` command in this change. The lines that
would add one, beside the existing `scan` command:

```bash
cmd_controllers() {
    run_cargo build -p pp-controllers --bin pp-controller-scan
    "$ROOT/target/debug/pp-controller-scan" "$@"
}
```

```bash
if [[ "${1:-}" == "controllers" ]]; then
    shift
    cmd_controllers "$@"
    exit $?
fi
```

And add `controllers` to the `usage` list.

## Protocol

There is no key exchange and no encryption in `src/zwift.rs` or `src/ble.rs`.
The handshake is the ASCII bytes `RideOn` (`52 69 64 65 4F 6E`), written to
the sync RX characteristic, without response when the characteristic allows
it. Other public notes on Zwift Play describe an RC4 handshake after that.
This crate does not send it. The owner's bridge, which they use with this
hardware, does not either.

Service `00000001-19ca-4651-86e5-fa29dcdd09d1`:

| Characteristic | UUID | Role |
| --- | --- | --- |
| Async | `00000002-19ca-4651-86e5-fa29dcdd09d1` | Notify. Button reports |
| Sync RX | `00000003-19ca-4651-86e5-fa29dcdd09d1` | Write `RideOn` |
| Sync TX | `00000004-19ca-4651-86e5-fa29dcdd09d1` | Notify. Subscribed, then ignored |

`src/zwift.rs` also defines Ride service
`0000fc82-0000-1000-8000-00805f9b34fb`. The scanner does not filter on it.
A device is Zwift when manufacturer id 2378 carries one of the type bytes
above.

A report's first byte is the message type. The rest is protobuf. Integers
are active-low: `0` means pressed. A Ride bitmask bit that is clear is
pressed.

| Device | Type | Body, from the `prost` structs in `src/zwift.rs` |
| --- | --- | --- |
| Click v1 | `0x37` | `ClickKeyPadStatus`: `button_plus` = 1, `button_minus` = 2, both `int32` |
| Play | `0x07` | `PlayKeyPadStatus`: `right_pad` = 1 (`0` is the right controller), `button_y_up` = 2, `button_z_left` = 3, `button_a_right` = 4, `button_b_down` = 5, `button_shift` = 6, `button_on` = 7, `analog_lr` = 8 (`sint32`), `analog_ud` = 9 (`sint32`) |
| Ride, Click v2 | `0x07` | `RideKeyPadStatus`: `button_map` = 1 (`uint32`), repeated `analog_paddles` = 3. Each paddle is `location` = 1 (`0` left, `1` right) and `analog_value` = 2 (`sint32`) |

Play maps the face fields by side, matching `parse_play`:

| Field | Left (`right_pad != 0`) | Right (`right_pad == 0`) |
| --- | --- | --- |
| `button_y_up` | `Up` | `Y` |
| `button_z_left` | `Left` | `Z` |
| `button_a_right` | `Right` | `A` |
| `button_b_down` | `Down` | `B` |
| `button_shift` | `ShiftDown` | `ShiftUp` |
| `button_on` | `OnOff` | `OnOff` |
| `analog_lr` absolute value 100 | same shift button | same shift button |

`analog_ud` is emitted as `Paddle` on the vertical axis. `parse_play` reads
the field and does not use it.

Ride masks from `ride_mask` in `src/zwift.rs`:

| Bit | Mask | Button |
| --- | --- | --- |
| `LEFT` | `0x00001` | `Left` |
| `UP` | `0x00002` | `Up` |
| `RIGHT` | `0x00004` | `Right` |
| `DOWN` | `0x00008` | `Down` |
| `A` | `0x00010` | `A` |
| `B` | `0x00020` | `B` |
| `Y` | `0x00040` | `Y` |
| `Z` | `0x00080` | `Z` |
| `SHFT_UP_L` | `0x00100` | `ShiftUpLeft` |
| `SHFT_DN_L` | `0x00200` | `ShiftDownLeft` |
| `POWERUP_L` | `0x00400` | `PowerUpLeft` |
| `ONOFF_L` | `0x00800` | `OnOffLeft` |
| `SHFT_UP_R` | `0x01000` | `ShiftUpRight` |
| `SHFT_DN_R` | `0x02000` | `ShiftDownRight` |
| `POWERUP_R` | `0x04000` | `PowerUpRight` |
| `ONOFF_R` | `0x08000` | `OnOffRight` |

The owner's MyWhoosh map sends both left shift bits, the left power-up bit,
and a left paddle as gear down, and the right-hand set as gear up. The
names above follow the bit names, not that collapse. Their paddle threshold
is an absolute value of at least 25 (`ANALOG_PADDLE_THRESHOLD`). They do
not say what full scale is. A paddle sample is `ControllerEvent::Paddle`
and is not also a button, so a wiring layer that honors both the bits and
the paddle for one pull will step power twice.

`Connected.side` is the manufacturer byte. Play button events take their
side from `right_pad`, which is what `parse_play` does.

A body that is not valid protobuf is ignored, and buttons that were held
stay held. The owner's `prost` error path would instead look like every
button was released. An empty body of a known type is valid protobuf, so
every field is `0` and every button looks pressed. Their comment says idle
and battery frames are other message types, and those are ignored. Battery
is not decoded. `ControllerEvent::Battery` is on the enum so the wiring
code can match it; the radio link does not emit it. `SimControllers` can.

The owner's repository has no captured packets and no protocol tests. The
vectors in `crates/pp-controllers/src/protocol.rs` are the protobuf encoding
of the field numbers above, and each test names the source file.

## Buttons

`Up`, `Down`, `Left`, `Right`, `A`, `B`, `Y`, `Z`, `Plus`, `Minus`,
`ShiftUp`, `ShiftDown`, `ShiftUpLeft`, `ShiftDownLeft`, `ShiftUpRight`,
`ShiftDownRight`, `PowerUpLeft`, `PowerUpRight`, `OnOff`, `OnOffLeft`,
`OnOffRight`.

`Plus` and `Minus` are the Click v1 keys. `ShiftUp` and `ShiftDown` are the
Play paddles. The suffixed names are Ride bits. `OnOff` is the Play power
button; the event's `side` says which controller.

Press and release are edges. A held pad does not repeat. Steer and
simulated power, if they should keep applying while the button is down,
are tracked by the wiring code from `Pressed` until `Released`.

There is no analog steer. Left and right are digital. The analog samples
are the shift paddles.

## Proposed mapping

Not implemented. Today's actions, from `crates/pp-godot/src/input_map.rs`,
are `power_up`, `power_down`, `menu_up`, `menu_down`, `menu_left`,
`menu_right`, `menu_accept`, `menu_back`, `pause_menu`, `toggle_camera`,
and `music_next`. Lane position is not an input action yet. The rider pose
already has `steer_rad`.

Use the pad as a menu while a menu is open, and as steer plus power while
riding. That is how the Deck d-pad already shares `power_up` with `menu_up`.

| Control | Riding | Menu |
| --- | --- | --- |
| `Left`, Ride `LEFT` | steer left | `menu_left` |
| `Right`, Ride `RIGHT` | steer right | `menu_right` |
| `Up`, Ride `UP` | `power_up` | `menu_up` |
| `Down`, Ride `DOWN` | `power_down` | `menu_down` |
| `A`, Ride `A` | | `menu_accept` |
| `B`, Ride `B` | | `menu_back` |
| `Y`, Ride `Y` | `toggle_camera` | |
| `Z`, Ride `Z` | `music_next` | |
| `OnOff`, `OnOffLeft`, `OnOffRight` | `pause_menu` | |
| `Plus`, `ShiftUp`, `ShiftUpRight`, `ShiftDownRight`, `PowerUpRight`, right paddle past the owner's threshold | `power_up` | |
| `Minus`, `ShiftDown`, `ShiftDownLeft`, `ShiftUpLeft`, `PowerUpLeft`, left paddle | `power_down` | |

`menu_back` currently also opens pause, so pause is the power button rather
than `B`. The left shift row follows the owner's working gear-down map,
including `ShiftUpLeft`, and should be checked on the hardware before anyone
trusts the `UP` in the bit name.

A Play paddle at exactly ±100 also emits `ShiftUp` or `ShiftDown`. Apply
the button or the `Paddle` sample, not both.

Click v1 only has `Plus` and `Minus`, so it can change power and nothing else.

`SimControllers` is the link to drive this from tests and the fake rider.
`set_discovered` plus `connect_all` queues `Connected`. `push` queues
anything else, including `Battery`.

## What a scan shows now

A read-only `bluetoothctl --timeout 12 scan on` on 20 Nov 2026 saw the Tacx
Neo 2T (`C4:AB:D7:28:06:F0`), a Samsung Family Hub, a microwave, a range, a
washer, a dryer, a Govee light, and a few unnamed addresses. No name
contained Zwift, Play, Click, or Ride. The controllers were not awake, or
not in the room. `pp-controller-scan` is the filtered check; it does not
connect.

## Unverified on the real controllers

- The scan lists each controller with the right name, side, and kind.
- `RideOn` by itself starts button reports. If a Play stays silent, it may
  want the RC4 handshake this crate does not send.
- Click v2 reports after the Zwift app has unlocked it, and the Ride layout
  is the right one.
- Edges match the table, in particular Ride `SHFT_UP_L` against the owner's
  gear-down mapping.
- `analog_ud` is unused, noise, or a second axis.
- Ride paddle full-scale, and whether one pull sets both the bit and the
  analog sample.
- Idle frames are not an empty `0x07` or `0x37`. An empty body looks like
  every button is down.
- The battery frame layout, if `Battery` should come from the radio.
- Both controllers stay up together, a dropped one comes back, and BlueZ
  does not demand a bond the owner's code never creates.
- Shutdown with a live link does not trip the BlueZ "no match" panic. The
  scan path drops the adapter event stream, waits, then stops the scan,
  which is the same order as `pp-trainer`.
