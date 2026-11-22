# Smart trainers

Penguin Pass drives a smart trainer over Bluetooth Low Energy. After it
reads the GATT table it picks one telemetry stream and, when control is
on, one resistance wire. The scan list is a separate choice: a device is
listed only when it advertises Fitness Machine (`0x1826`), Cycling Power
(`0x1818`), or Tacx FE-C (`6e40fec1-…`). A Zwift Play, Click, or Ride
controller advertises the Zwift service `0xFC82` and none of those, so it
does not show up as a trainer. Controllers are covered in
`docs/controllers.md`. The byte layouts live in `pp-ftms`. This page is
which trainers those layouts are meant to cover, and what has actually
been ridden.

Frame details for FTMS and Tacx FE-C are in `docs/bluetooth.md`.

## Families

A row is the protocol the trainer speaks, not a promise that this checkout
has been ridden on that model. The verification section below says which
rows have been on a real bike.

| Family | Models | How Penguin Pass talks to it |
| --- | --- | --- |
| Wahoo | KICKR v6, KICKR CORE, KICKR CORE 2, KICKR MOVE, KICKR v4, KICKR v5, KICKR SNAP (2020 and later) | FTMS, when the Fitness Machine control point is present |
| Wahoo, legacy | KICKR v1, v2, and v3, early KICKR SNAP, KICKR BIKE | Wahoo trainer control (`A026E005-0A7D-4AB3-97FA-F1500F9FEB8B`) when FTMS control is absent. Power still comes from Cycling Power |
| Zwift | Zwift Hub, Zwift Hub One, Zwift Ride (the KICKR CORE with a Cog) | FTMS when that control point is present. The Zwift trainer service `0xFC82` is the fallback when FTMS, FE-C, and Wahoo control are all absent |
| JetBlack | Victory, Volt | FTMS, and the Zwift `0xFC82` service on the models that advertise it |
| Garmin / Tacx | Neo 2T, Neo 3M, Flux S, Flux 2, Flow Smart | Tacx FE-C, or FTMS when the control point is in the GATT table. The Neo 2T in this room advertises Cycling Power and FE-C |
| Elite | Justo 2, Justo, Direto XR, Direto XR-T, Suito-T, Rivo, Avanti | FTMS or FE-C, whichever control characteristic the trainer exposes |
| Other FTMS trainers and smart bikes | Saris H3, H3+, and M2; Van Rysel D100 and D500; Magene T300 and T200; Wattbike Atom; Stages SB20; Schwinn IC4, IC8, and Bowflex C6; Kinetic Smart Control | FTMS |

Two extra sources are not resistance protocols:

- **Cycling Speed and Cadence** (`0x1816`, measurement `0x2A5B`). When the
  characteristic is present, Penguin Pass subscribes. Crank cadence from
  it fills the sample only when the primary cadence is missing or older
  than 2.5 seconds, so a 1 Hz CSC stream does not fight a fresh indoor-bike
  or cycling-power cadence.
- **Cycling Power** (`0x1818`) on its own, including a power meter with no
  trainer control. Power and crank cadence are read. The session stays
  connected and reports `control not permitted`, which the ride already
  treats as reading only.

## Which wire is used

Telemetry, first match:

1. Indoor Bike Data (`0x2AD2`)
2. Cycling Power Measurement (`0x2A63`)
3. Tacx FE-C notifications
4. Zwift riding data (opcode `0x03` on the Zwift notify characteristic)

CSC is subscribed as well whenever it is present. It is not one of the
four. It only contributes cadence, and only when the chosen stream's
cadence has gone quiet.

Resistance, when the "control trainer" setting is on, first match:

1. FTMS control point (`0x2AD9`). Request control, start, then indoor-bike
   simulation or target power. Unchanged from the FTMS path.
2. Tacx FE-C write. User configuration, then grade or target power. A stop
   writes 0% grade. See `docs/bluetooth.md`.
3. Wahoo trainer control. Subscribe, unlock with `20 EE FC`, then send
   rider-plus-bike weight, rolling resistance, and the wind coefficient.
   Grade and ERG follow. A rejected unlock drops the link. A rejected
   parameter write is logged and the session still connects. A stop writes
   0% grade.
4. Zwift trainer write. `RideOn`, then bike and rider weight, then grade
   (wind, drag, rolling resistance) or target power. A stop writes 0%
   grade. The response characteristic is subscribed when the trainer has it.
   The session does not wait on it.

A modern KICKR CORE or JetBlack Victory that exposes FTMS together with
Wahoo control and `0xFC82` uses FTMS. The other two are not written.

Read-only mode never writes, on any of the four wires.

The connect line names the choice:

```text
pp-trainer: connected "<name>" services=[...] telemetry=ftms control=ftms
```

`telemetry` is `ftms`, `cps`, `fec`, or `zwift`. `control` is `ftms`,
`fec`, `wahoo`, `zwift`, or `none`.

## Virtual gears

Zwift Cog and Zwift Ride virtual shifting use 24 gear ratios (`1`..=`24`,
default `12`). Ratios come from `pp-ftms` (`ZWIFT_VIRTUAL_GEAR_RATIOS` and
`ZwiftPhysical::gear_ratio_x10000`). Penguin Pass now sends gear changes over
the Zwift trainer service write (`0xFC82`) when the connected bike exposes
that characteristic. Grade and ERG on dual-protocol trainers (KICKR CORE
Zwift One, Zwift Hub One, JetBlack Victory) still go over FTMS (`0x2AD9`);
virtual gear is a separate write that does not wait for the FTMS
control-point response.

Shift from a Zwift Click, Play, or Ride (shift buttons and paddles) or from
the keyboard: `,` down, `.` up. Trainers without `0xFC82` ignore gear commands;
the same pad inputs fall back to simulated power.

## Verification status

### Ridden here

- **Garmin Tacx Neo 2T.** Power and hill resistance on a real ride. The
  trainer advertises Cycling Power and FE-C, not FTMS, so that ride took
  the FE-C control path. See the roadmap in `README.md` and
  `docs/trainer-trace-plan.md` for what is still open on that bike
  (standing-start feel, a traced ERG workout).
- **Zwift controllers** (Play, Click, Ride). Buttons and steering, not
  trainer resistance. See `docs/controllers.md`. A controller that only
  advertises `0xFC82` is not listed by the trainer scan.

### State machine only

These paths are exercised by `Engine` against `FakePeripheral` in
`crates/pp-trainer`. Nothing in that suite opens a radio.

- Legacy Wahoo KICKR (Cycling Power plus Wahoo control, no FTMS): unlock,
  simulation parameters, grade, target power, and 0% on stop.
- Zwift Hub / Ride fallback (Zwift notify and write, no FTMS, FE-C, or
  Wahoo control): `RideOn`, physical, simulation, target power, riding-data
  power, cadence, speed, and heart rate, and 0% on stop.
- Virtual gears over `0xFC82` on pure-Zwift and FTMS+Zwift profiles:
  `RideOn` when needed, `SetPhysical` with gear ratios, clamping, and no
  duplicate shifts.
- Still waiting on a real Zwift Cog / Ride ride to confirm shifting feel on
  hardware.
- FTMS plus CSC: indoor-bike power with CSC crank cadence filling RPM, and
  CSC staying quiet while indoor-bike cadence is fresh.
- A trainer that exposes FTMS, Wahoo control, and `0xFC82` together stays
  on FTMS.
- A device that advertises only `0xFC82` is not a trainer.

### Waiting on a ride

Every other model in the table above. A useful report is:

```text
./x scan
./x play --trace
```

`./x scan` prints the trainers the adapter can see. `./x play --trace`
writes `target/traces/<date-time>/ble.csv`. The `channel` column uses
`wahoo`, `zwift`, and `csc` for the paths on this page, alongside the
existing `ibd`, `cps`, `fec`, and `ftms-cp` rows. The connect line's
`telemetry=` and `control=` values say which wire the session chose.
Wake the trainer, close any other app that is paired to it, and send the
trace folder with the model name.
