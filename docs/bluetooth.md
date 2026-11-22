# Bluetooth trainer

Penguin Pass talks to the trainer over Bluetooth Low Energy. A background
thread owns the BlueZ connection. The game thread only reads the latest
power, cadence, speed, and heart rate, and queues a grade when the slope
changes. Nothing in the frame loop waits on the radio.

The trainer is used through whichever control path its GATT table actually
has. The choice is made after connecting, not from the advertisement.

| GATT contents | Telemetry | Resistance |
| --- | --- | --- |
| Fitness Machine control point (0x2AD9) | Indoor Bike Data when that characteristic is present, otherwise Cycling Power | FTMS indoor-bike simulation, unchanged |
| No FTMS control point, but the Tacx FE-C service is present | Cycling Power when present, otherwise FE-C pages 25 and 16 | FE-C page 51 (track resistance) |
| Cycling Power only | Cycling Power (power, and cadence once two crank readings have arrived) | None. The session stays connected and reports `control not permitted`, which the UI already treats as reading only |

FE-C notifications (page 16) still contribute speed and heart rate when the
link is not using Indoor Bike Data. Page 25 is used for power and cadence
only when Cycling Power is absent, so the two power sources do not fight.

Read-only mode (`SessionMode::ReadOnly`, the "control trainer" setting off)
never writes, on either path. On disconnect or exit, an FE-C control session
writes grade 0% before it drops the link. The game also sends grade 0%
itself; the trainer crate repeats it so a shutdown cannot leave the last
climb loaded.

## Scan and connect

Wake the trainer (plug it in, or turn the pedals), then:

```text
./x scan
```

That builds `pp-scan` and runs it on the host for about eight seconds. It
prints trainers that advertise any of these:

- Fitness Machine Service, `0x1826`
- Cycling Power, `0x1818`
- Tacx FE-C, `6e40fec1-b5a3-f393-e0a9-e50e24dcca9e`

Each line has the address, signal, name, and which of `ftms`, `power`, and
`fe-c` the advertisement carried. A dishwasher, washer, fridge, CPAP, or
headphones does not advertise those services and is not listed. The scan
filter itself is empty, because a filter of only `0x1826` hides a Tacx Neo
2T: that trainer advertises Cycling Power and FE-C, and does not advertise
FTMS.

Under that list, a second heading shows heart-rate straps advertising the
Heart Rate Service (`0x180D`). The strap scan is a separate BlueZ session
from the trainer scan. A strap only advertises while it is worn, and an
ANT+-only strap (some older Garmin HRMs) does not appear.

To watch live data without changing resistance:

```text
./x scan --connect "Neo" --seconds 10
```

The argument can be the Bluetooth address or part of the trainer name.
This path only subscribes to measurements. It does not request control and
does not write FE-C.

```text
./x scan --connect "Neo" --grade 2.5
```

does request control and sets a 2.5% grade, then sets the grade back to
0% before it disconnects. Leave that for a ride, not for a quick check.

On a successful connect the game logs one line to stderr:

```text
pp-trainer: connected "<name>" services=[1826,1818,6e40fec1] telemetry=cps control=fec
```

`services` is the GATT table, shortened (`1826` is FTMS, `6e40fec1` is the
Tacx service). `telemetry` is `ftms`, `cps`, or `fec`. `control` is `ftms`,
`fec`, or `none`. A failure is one line, `pp-trainer: connect failed "<name>": <reason>`.

## FE-C frames

Tacx carries ANT+ FE-C pages on a Nordic UART-style service. This is the
layout the codec implements. It has not been confirmed on the Neo 2T in
this room; the scan has.

Service `6e40fec1-b5a3-f393-e0a9-e50e24dcca9e`. Notifications (trainer to
app) are `6e40fec2-…`. Writes (app to trainer) are `6e40fec3-…`, without
response when the characteristic allows it.

Each message is 13 bytes: sync `0xA4`, length `0x09`, message id (`0x4E`
broadcast from the trainer, `0x4F` acknowledged to the trainer), channel
`0x05`, an 8-byte data page, and a checksum. Two checksums are in use, and
they disagree:

- ANT Message Protocol, GoldenCheetah's `ANTMessage`, and the worked example
  in Tacx's "FE-C over BLE" how-to (a page 16 the trainer itself sends,
  checksum `0x30`) XOR every byte, including the sync byte.
- The how-to's prose and pycycling sum every byte except the sync byte.
  qdomyos-zwift's Neo 2T grade command uses that sum plus one, and message
  id `0x4E`.

The trainer almost certainly checks incoming commands with the same routine
it uses to build the frames it sends. Penguin Pass accepts either checksum
on the way in and remembers which one matched. Writes use the variant from
the latest valid frame. Before any frame has been seen, writes use the ANT
XOR. Message id stays `0x4F` and the channel stays `0x05`. The first page 55
and the first page 51 wait until a frame arrives or 2 seconds pass,
whichever is first, so that first write already uses the observed variant.
The first decoded frame logs `pp-trainer: fe-c checksum=<xor|sum> first-page=<n>`.
A frame that fails both checksums logs once:
`pp-trainer: fe-c checksum failed bytes=<hex>`.

Page 16 (0x10), general data, trainer to app. Byte 0 is the page number.

| Byte | Field |
| --- | --- |
| 1 | Equipment type. `25` is a trainer |
| 2 | Elapsed time, 0.25 s, rolls over |
| 3 | Distance, meters, rolls over at 256 |
| 4–5 | Speed, little-endian, 0.001 m/s. `0xFFFF` is invalid |
| 6 | Heart rate, bpm. `0xFF` is invalid |
| 7 | Low nibble capabilities, high nibble FE state |

Page 25 (0x19), specific trainer data.

| Byte | Field |
| --- | --- |
| 1 | Event count |
| 2 | Cadence, rpm. `0xFF` is invalid |
| 3–4 | Accumulated power, little-endian watts |
| 5 and the low nibble of 6 | Instantaneous power, 12 bits. `0xFFF` is invalid |
| 6 high nibble | Trainer status |
| 7 | Low nibble flags, high nibble FE state |

Page 51 (0x33), track resistance. This is the grade command. Bytes 1–4 are
`0xFF`. Bytes 5–6 are grade, little-endian, in 0.01% with a −200% offset:
`raw = (grade_pct + 200) × 100`. The value is clamped to −200%…+200% so
`0xFFFF` (invalid) is never sent. Byte 7 is the rolling-resistance
coefficient in units of `5e-5`; `0xFF` means the trainer's default. The
game's coefficient (default 0.004) is sent. Grade writes use the same 0.1
percentage-point delta and the same 500 ms rate limit as FTMS.

Page 55 (0x37), user configuration, sent once when an FE-C control session
connects. Byte 3 is `0xFF`.

| Bytes | Field |
| --- | --- |
| 1–2 | Rider mass, little-endian, 0.01 kg. `0xFFFF` when the config has no mass |
| 4 low nibble | Wheel-diameter offset, millimeters. `0xF` is the default |
| 4 high nibble and byte 5 | Bicycle mass, 12 bits, 0.05 kg. `0xFFF` when the config has no mass |
| 6 | Wheel diameter, 0.01 m. `0` when unknown |
| 7 | Gear ratio, 0.03. `0` is invalid |

Page 252 (0xFC), Tacx Neo road feel. This is not an ANT+ FE-C profile page; it
is pycycling's `set_neo_modes` with isokinetic mode off. Byte 0 is `0xFC`.
Byte 1 is `0x00`. Byte 2 is the isokinetic flag (`0`). Byte 3 is isokinetic
speed (`0`). Byte 4 is `0x00`. Byte 5 is the surface pattern. Byte 6 is
intensity, 0–100. Byte 7 is `0x00`.

Surface pattern numbers match pycycling's `RoadSurface`: smooth `0`,
concrete plates `1`, cattle grid `2`, hard cobblestones `3`, soft cobblestones
`4`, brick road `5`, off-road `6`, gravel `7`, ice `8`, wooden boards `9`.
Intensity is `0` for smooth and `40` for every other surface. This crate
never sends `100` or the `255` default sentinel: pycycling notes 50% is already
strong on a Neo and 100% is untested and may damage the trainer. The checksum
rule is the same as page 51: XOR of every byte including sync, or the low 8
bits of the sum of every byte except sync. Road feel does not change grade. The FE-C control session
writes the page when the surface changes, and writes surface 0 when the link
releases. FTMS, Wahoo, and Zwift ignore the command. It does not use the
grade rate limit.

`TrainerConfig` carries `rider_mass_kg` and `bike_mass_kg` when the session
has them. An absent mass is the invalid sentinel, and the trainer keeps its
own value for that field. Grade control does not depend on the page.

## Sources

Checked before encoding:

- Tacx, "FE-C over BLE", 17 February 2015 (the how-to distributed with the
  public Tacx BLE examples). It names the three UUIDs, channel 5, the 13-byte
  ANT frame, and the checksum as the sum of every byte except `0xA4`. Its
  worked page-16 example (`…10 19 04 00 00 00 FF 24`, checksum `30`) does
  not match that sum; `0x30` is the XOR of every preceding byte, including
  sync. Outgoing frames follow that example until a live frame says otherwise.
- ANT+ Fitness Equipment profile, for the page 16, 25, 51, and 55 field
  widths, the −200% grade offset, and the invalid sentinels.
- ANT Message Protocol and Usage, for the serial frame (sync, length, id,
  data, XOR checksum). GoldenCheetah's `ANTMessage` constructor implements
  that XOR and its `fecSetTrackResistance` is what GoldenCheetah writes to
  `6e40fec3-…`.
- pycycling `tacx_trainer_control.py` (`_send_fec_cmd`, `set_track_resistance`,
  `set_user_configuration`, `set_neo_modes`, `RoadSurface`). The checksum is
  the Tacx sum. Page 55 puts the
  wheel-diameter offset in the low nibble of byte 4 and the bicycle-weight
  low nibble in the high nibble. That matches the notes for this change.
- qdomyos-zwift `tacxneo2.cpp`. Grade uses the sum excluding sync, plus one.
  Target power and user configuration in that file use XOR, and the message
  id is `0x4E` rather than `0x4F`. This crate sends `0x4F` and whichever of
  the XOR and the Tacx sum the trainer's own frames use.

## What is verified on this hardware

Verified: `./x scan` lists the owner's Tacx Neo 2T, which advertises
Cycling Power and FE-C and does not advertise FTMS.

Not verified, because nothing in this change connects to or writes to the
trainer:

- Whether the GATT table contains FTMS (0x1826 / 0x2AD9) after connect.
  The connect log line is the check. `control=ftms` means the FTMS path.
  `control=fec` means page 51. `control=none` means power and cadence only.
- Whether page 51 actually changes resistance, and whether page 55 with
  real masses changes inertia.
- Which checksum the Neo 2T puts on its notifications. The log line
  `fe-c checksum=xor` or `checksum=sum` is that answer, and the grade
  writes use the same one. `checksum failed` means a frame matched neither.
- The bluez-async shutdown panic. The scan drops its D-Bus event stream and
  waits briefly before the adapter is dropped, which is the ordering that
  avoids `remove_match` returning "No match with that id found". Confirm by
  running `./x scan` and seeing no panic on the way out.

## Heart-rate strap

The trainer and a chest strap are two devices. The Neo 2T does not report
heart rate, so the strap connects on its own and can drop or reconnect
without disturbing the trainer. The strap link is read-only: the only radio
write is enabling notifications on Heart Rate Measurement (`0x2A37`).

Both the 8-bit and 16-bit heart-rate formats are accepted. When the strap
says it supports skin contact and none is detected, the bpm goes blank
immediately. Otherwise a reading stays for 3 seconds, the same window as
trainer power. After a drop the link reconnects to the same strap, with the
same backoff as the trainer. An explicit disconnect does not reconnect.

On connect the log line is `pp-trainer: heart-rate connected "<name>"`.
A failure is `pp-trainer: heart-rate connect failed "<name>": <reason>`.

The strap has its own thread and its own BlueZ session. The trainer keeps
its session. Opening the two D-Bus connections at the same instant drops
one of them, so that setup is serialized; after that the scans run together.
BlueZ merges discovery filters.

Wear the strap before scanning. Take it off and it stops advertising, and
a contact-aware strap blanks the bpm while it is still connected.

## Trace

`./x play --trace` creates a directory and sets `PENGUIN_PASS_TRACE_DIR`.
The trainer link reads that variable once, when it is spawned. Unset or
empty means no file and no work on a packet. The file is `<dir>/ble.csv`.
On open the link prints `pp-trainer: trace <absolute path>`. If the file
cannot be opened it prints one warning and the ride continues.

`ride.csv` and `ble.csv` share `unix_ms`, wall-clock milliseconds since the
Unix epoch, as the first column. Each row is one trainer-link event: a
notification (`rx`), a write (`tx`), or a connection change (`state`).
`mono_ms` is the link's own clock. `channel` is `ibd`, `cps`, `fec`,
`ftms-cp`, `ftms-status`, `hr`, or the characteristic UUID. `bytes_hex` is
the raw payload. `fec_page` is the FE-C page number. `power_w`,
`cadence_rpm`, and `speed_kph` are filled when that one message carries
them, including FE-C page 25 even though the Tacx path keeps using Cycling
Power for the sample. `note` is the state summary, `bad checksum`, or a
short write description such as `grade=3.5`. Rows are buffered, flushed
about once a second, and flushed again when the link stops. The heart-rate
strap is not traced.

## One controller at a time

The trainer grants control to one client. If another app already has it,
the control-point response is "Control Not Permitted". Penguin Pass asks
for control once more, and if that is refused it stops sending grades
while it keeps reading telemetry. Quit Zwift, MyWhoosh, or any other
training app before connecting. The trainer also keeps a single Bluetooth
link; a second app often will not see it until the first one disconnects.

## When the trainer is not in the list

- The trainer is awake and within a few metres of the deck.
- `bluetoothctl show` reports `Powered: yes`. Penguin Pass does not turn
  the adapter on or off. (btleplug's own discovery call can power an
  adapter that is already off; this program does not add another power
  change.)
- No other app is connected to it.
- The advertisement includes FTMS, Cycling Power, or the Tacx FE-C service.
  A unit stuck in a phone-app pairing screen may not show up until that
  screen is cleared.
- Pairing is not required for a normal session. If the deck has already
  paired the trainer, leave that pairing alone.

## Godot flatpak

The game process has to be allowed to talk to BlueZ on the system bus.
`./x run` already launches Godot with `--system-talk-name=org.bluez`.
A flatpak launch that omits that flag can scan nothing and cannot connect.
