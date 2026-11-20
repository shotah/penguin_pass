# Bluetooth trainer

Penguin Pass talks to the trainer over Bluetooth Low Energy, using the
standard Fitness Machine Service. A background thread owns the BlueZ
connection. The game thread only reads the latest power, cadence, speed,
and heart rate, and queues a grade when the slope changes. Nothing in the
frame loop waits on the radio.

## Scan and connect

Wake the trainer (plug it in, or turn the pedals), then:

```text
./x scan
```

That builds `pp-scan` and runs it on the host for about eight seconds. It
prints trainers that are advertising the Fitness Machine Service.

To watch live data without changing resistance:

```text
./x scan --connect "Neo" --seconds 10
```

The argument can be the Bluetooth address or part of the trainer name.
This path only subscribes to measurements. It does not request control.

```text
./x scan --connect "Neo" --grade 2.5
```

does request control and sets a 2.5% grade, then sets the grade back to
0% before it disconnects. Leave that for a ride, not for a quick check.

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
  the adapter on or off.
- No other app is connected to it.
- The trainer is advertising the standard service (`0x1826`). A unit
  stuck in a phone-app pairing screen may not show up until that screen
  is cleared.
- Pairing is not required for a normal FTMS session. If the deck has
  already paired the trainer, leave that pairing alone.

## Godot flatpak

The game process has to be allowed to talk to BlueZ on the system bus.
`./x run` already launches Godot with `--system-talk-name=org.bluez`.
A flatpak launch that omits that flag can scan nothing and cannot connect.
