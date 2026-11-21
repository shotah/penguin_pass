# Sound

Ambience and effects sit on two buses, `Ambient` and `Effects`, beside the
existing `Music` bus. The levels are decided in `pp-view` (`sound` module)
and played by `SoundPlayer` in `pp-godot`. Nobody on the machine that added
these files has heard them. Lengths, peaks, and seam gaps below are measured
from the files. Judge them by ear before shipping.

## What plays

Alpine rides get birds and a light breeze. The breeze gets louder with
height and with a steeper grade. Every so often a cowbell or a short stream
is scheduled. The stream file is not in the build: the only CC0 recording
found could not be downloaded (Wikimedia returned 429). The scheduler still
asks for it, and the player skips a missing file.

Desert rides get a drier wind loop and a bed of cicadas with crickets under
them. A hawk is scheduled the same way. No CC0 hawk call was found. The
calls on Wikimedia Commons were CC-BY-SA, so they were not taken.

The tyre hum and the drivetrain whirr are computed (pitch and level follow
speed, cadence, and power) and written to the log. No CC0 recording of
either was found, so those two layers are silent. The freewheel is a real
recording of an object brushed across bicycle spokes, not a freehub pawl.
It plays only while coasting: rolling faster than a walk (above 2 m/s to
start, below 1.3 m/s to stop) and the legs are quiet (cadence under 8 rpm
or power under 8 W to start; both cadence at least 18 rpm and power at least
20 W to stop). An unreported cadence is not treated as zero. The level fades
in and out over 0.2 seconds. Its pitch follows speed.

Wind in the ears starts above 30 km/h and is stronger on a descent. Breathing
follows the power zone, blended with heart rate when a strap is connected.
It is quiet, it is off unless Rider breathing is On, and it stops while
paused. The title screen keeps a quieter ambience and no bike. Pause and the
settings page fade the bike out and leave the bed playing.

Short cues (fish, powerups, segments, the lap bell, menus) are on the
effects bus. The fish pitch rises with the combo. The old generated fish
blip is gone.

## Settings and silence

`ambient_volume` and `effects_volume` are 0–100, stored like `music_volume`.
`rider_breathing` is a bool and defaults to off. Older settings files pick
up the defaults.

`--no-sound` starts both buses silent. `--sound` keeps them on. Smoke, the
picture and bench scripts, `--headless`, and `--quit-after` are silent
unless `--sound` or `--sound-capture` is on the command line.
`--sound-capture <file.wav>` records about nine seconds of a scripted ride
(pedal, sprint, coast, one fish, pause) and quits. That launch should also
pass Godot's `--audio-driver Dummy` so the mix is real and the speakers are
not.

The log line `PENGUIN_PASS_SOUND files=<n> missing=<n>` is printed at
startup. `missing` counts required files only. A layer line
`PENGUIN_PASS_SOUND <name> level=.. pitch=..` is printed when that layer
moves, and `coasting=0` or `coasting=1` when the freewheel gate flips.

## What was measured

This was not listened to. A capture with Godot's dummy driver
(`--sound-capture`, music off, buses at 80) wrote 8.82 s at 44.1 kHz.
The loudest sample was 0.263, so the mix did not clip. Per-second RMS was
about 0.027, 0.015, 0.037, 0.027, 0.028, 0.025, 0.041, 0.046, then 0.008
on the last second, which is the pause: ambience only. The freewheel is
not separable from the wind in that file with a crude high-band check.
The log is the evidence for the gate: `coasting=0` and `ratchet=0.00`
while pedalling and sprinting, `coasting=1` and `ratchet=0.62` on the
descent, then back to `0.00` after the pause fade. Tyre and drivetrain
levels move in that log and are not in the file.

Two bench runs of 240 frames, same build, dummy driver: average 6.95 ms
with sound forced off, 7.09 ms with `--sound`. That is 0.14 ms on the
average. The 95th percentile was 9.15 ms and 10.09 ms, and the slowest
frame was 10.73 ms and 12.21 ms. One pair of runs, so the tails include
noise.

## First ride

Listen for these, in order:

1. On the title, birds and a soft breeze, or wind and insects on a desert
   course. No bike yet.
2. As you start, a short open cue, then a road layer that is still missing,
   so the first thing under the music is the breeze plus, once you are
   turning the pedals, nothing from the chain. That gap is the missing
   drivetrain recording.
3. Above 30 km/h, a higher wind. On a descent it should be obvious.
4. Stop pedalling while still rolling. A clicking texture fades in over
   about a fifth of a second and rises in pitch with speed. It is spoke
   clicks, and it may not read as a freehub. It should fade out when you
   pedal again, not click off.
5. Turn Rider breathing on. It should stay quiet and deepen when the zone
   climbs. Turn it off if it bothers you.
6. Collect a fish. The pitch should step up with the combo. A golden fish
   is a different chime. A lap ends with a bicycle bell.
7. Open the menu. Ambience stays, quieter. Move, confirm, and back should
   be soft clicks, not beeps.
