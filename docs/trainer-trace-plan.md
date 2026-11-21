# Real-trainer trace & hardware test plan

This is the checklist for capturing real data on your Garmin Tacx Neo 2T
(and optional heart-rate strap) so we can fix standing-start acceleration
and verify ERG workouts from real numbers rather than guesses.

No agent connects to real Bluetooth devices. Every step below is run by
hand when you are on the bike.

## 1. Standing-start acceleration trace (main goal)

Why: the bike feels sluggish getting moving from 0 km/h on the Neo 2T
compared to the instant push you feel on the pedals. On the Tacx path
today, power and cadence come from Cycling Power Measurement (~1 Hz),
while FE-C page 25 instantaneous power is decoded and dropped. Before
changing anything, we want the raw trace.

Steps (about 3 minutes on the bike):

1. Start the game with tracing on:
   ```bash
   ./x play --trace
   ```
   The terminal prints the output folder, e.g.
   `target/traces/20261121-093000/`.
2. Pair your **Tacx Neo 2T** (if it does not reconnect automatically).
3. Pick **Flat 10 km** (so hill grade does not mix into the start test)
   and press **Start ride**.
4. Do **three standing starts**:
   - Sit completely still (0 rpm, 0 W) for **5 seconds** so the bike
     comes to a full stop.
   - Push hard out of the gate for **10 seconds** (standing or firm
     seated start).
   - Stop pedalling and let it coast back to 0 km/h.
   - Repeat twice more.
5. Finish with **~45–60 seconds of steady pedalling**, then pause and
   **End ride** (so the trace flushes cleanly and a FIT file is written).

What we will inspect in `target/traces/<timestamp>/`:

- `ble.csv`: exact arrival timing and payloads of CPS vs FE-C pages
  (especially page 25 instantaneous power and page 16 wheel speed) at
  the moment your foot goes down.
- `ride.csv`: `sample_age_ms`, `power_w`, `trainer_speed_kph`,
  `game_speed_kph`, and `game_accel_mps2` across the first 3 seconds of
  each start.

From those two files we can tell immediately whether the delay is CPS
reporting latency (~1 s), FE-C page 25 being ignored, or physics
inertia — and fix the real cause.

## 2. Optional checks while you are already on the bike

You can fold any of these into the same ride or a second short ride
(still under `./x play --trace`):

- **ERG structured workout:** pick **Easy spin** (or any short workout)
  from the title screen and ride 2–3 minutes across a warm-up ramp or
  interval change. Check that the Neo 2T holds target watts smoothly,
  releases resistance ~1 second after you stop pedalling, and eases back
  in when you resume.
- **Heart-rate strap:** pair your Garmin HRM (or other BLE HR strap)
  from the title screen and confirm live bpm appears on the console and
  inside the saved `.fit` file.
- **Audio mix:** with speakers or headphones on, check ambient sound,
  downhill wind rush, the coasting click when you stop pedalling while
  rolling, and optional **Rider breathing** in Settings.
- **Strava FIT upload:** upload one saved `.fit` file from
  `~/.local/share/godot/app_userdata/Penguin Pass/rides/` to Strava
  (either by hand first or via **Settings → Connect Strava**) and check
  how Strava labels the sport type, power, cadence, heart rate, and
  distance.

## 3. Sharing the result

Paste the trace folder path (e.g. `target/traces/20261121-093000`) into
the chat along with a one-line note on how it felt. We will analyse
`ride.csv` and `ble.csv` and tune the trainer path from real data.
