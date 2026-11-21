# Ride session

`pp-session` is the Godot-free ride. It owns the Penguin Pass [`Ride`](../crates/pp-course/src/ride.rs), the active trainer, a copy of the user settings, and the optional TV dashboard. The frame loop calls `Session::tick` and reads plain data back. It does not link Godot.

## What a tick does

Once per frame, with `dt_s` in seconds:

1. Read the active trainer. A Bluetooth trainer with no fresh sample, or a link that is not connected, is 0 W. The previous wattage is not kept.
2. Step the ride. A zero, negative, or non-finite `dt_s` does not move the bike.
3. If that trainer is connected and `control_trainer` is true, send `road_grade * trainer_difficulty`. The trainer crate already coalesces grades, so the session sends every frame and does not add a second limiter. A send error does not fail the frame. When `control_trainer` is false, the tick sends no command.
4. Publish telemetry when the dashboard is already listening. A dashboard that is still starting is skipped.
5. Return the telemetry.

The simulated rider is a `SimTrainer`. `nudge_sim_power` steps by 25 W and `set_sim_power` sets a target, both clamped to 0..=1500 W. Cadence is 0 rpm at 0 W and 85 rpm otherwise. Those calls do nothing while a real trainer is the active source, including while its link is down.

## Trainer difficulty

`trainer_difficulty` is the fraction of the real road grade sent to the trainer, from 0.0 to 1.0. The default is 0.5.

On-screen speed always uses the real grade. Difficulty only scales what the legs feel. `0.0` holds the trainer flat. `1.0` sends the road grade unchanged. It does not change telemetry, the course, or the physics.

## Connect, disconnect, reconnect

The session starts on the simulated rider and does not open Bluetooth until `start_scan`, `connect`, or `auto_connect`.

`start_scan` builds the trainer through the factory. The real factory is `BleTrainer::spawn_with`. `control_trainer` true uses control mode. `control_trainer` false uses read-only mode: the game reads power, cadence, and heart rate, and it does not take control or send grade. The trainer stays in whatever mode the rider set on it. If Bluetooth cannot start, the status line is `Bluetooth unavailable: ...` and the ride continues.

Turning `control_trainer` off or on while a trainer is open drops that connection on a background thread and opens a new one in the new mode. If a link was requested, the session connects to the same id again. Until that link is up the keyboard rider stays out, and the status line says `reconnecting`.

`connect` queues a connection. The trainer becomes the active source only once its state is connected, and `last_trainer_id` is updated then. The caller saves settings when it wants that id kept.

If a connected trainer later reports disconnected or failed, the session keeps the trainer object. The trainer reconnects on its own with backoff. Power is 0 W until samples resume. The simulated rider is not swapped in: a rider who stops pedaling is not pushed by the keyboard target.

`disconnect` returns to the simulated rider. Dropping a `BleTrainer` can block for about three seconds while the radio thread exits, so the session moves that value onto a short-lived `pp-session-drop` thread and returns immediately. The thread exits when the drop finishes. Dropping the `Session` joins any helper still running, which is for shutdown, not for a frame. The remembered trainer id is left in place.

`auto_connect` scans and connects to `last_trainer_id` when that address or name shows up in the scan. It does nothing when the id is missing.

Status lines the HUD can show:

- `Simulated rider`
- `Scanning for trainers`
- `Connecting to Tacx Neo 2T`
- `Tacx Neo 2T: connected, resistance follows the road`
- `Tacx Neo 2T: connected, reading only`
- `Tacx Neo 2T: reconnecting, reading only`
- `Tacx Neo 2T: connection lost, retrying`
- `Bluetooth unavailable: ...`

A trainer warning is appended in parentheses, for example `Tacx Neo 2T: connected, resistance follows the road (control not permitted)`.

`status().dashboard` is `Off`, `Starting`, `Running`, or `Failed`. URLs are present only while `Running`. `dashboard_error` is set only when `Failed`. Starting the dashboard, and rebinding it when the port changes, returns immediately. The bind runs on a `pp-session-dash` thread. Telemetry published while that thread is still starting is not sent. Dropping the session joins the thread.

## Rider weight during a ride

`apply_settings` calls `Ride::set_rider`. Mass, bike mass, and FTP take effect on the next tick. Speed, distance, lap, and elapsed time stay where they are. `rider()` and `settings().rider_profile()` match after `apply_settings`. `restart()` still returns the bike to the start line and keeps the current profile. CdA and Crr stay at the `RiderProfile` defaults. Difficulty still applies on the next tick and does not change on-screen speed.

The settings mockup `settings-weight-note.png` says a weight change waits for restart. That note is outdated. Rider, bike, and FTP changes are live on the next tick.

## Settings file

TOML. Unknown keys are ignored. Missing keys use defaults, so an older file still loads. A missing file is the defaults and is not created until a save. An unreadable or malformed file loads as defaults, returns a warning, and is not overwritten. Out-of-range numbers are clamped to the nearest bound. NaN is replaced with the field default. `+infinity` uses the upper bound and `-infinity` the lower bound. Those corrections are named in the warning and are written back only on the next save.

Save writes a temporary file in the same directory and renames it over the destination. The parent directory is created if needed.

```toml
# Penguin Pass settings
#
# Masses are kilograms. ftp_w is watts. trainer_difficulty is the fraction
# of the real road grade sent to the trainer (0.0 to 1.0). On-screen speed
# always uses the real grade. This only scales what the legs feel.

rider_mass_kg = 75.0
bike_mass_kg = 9.0
ftp_w = 200
trainer_difficulty = 0.5
last_trainer_id = "AA:BB:CC:DD:EE:FF"
tv_dashboard_enabled = true
tv_dashboard_port = 7878
fullscreen = true
control_trainer = true
```

| Field | Default | Valid |
| --- | --- | --- |
| `rider_mass_kg` | 75 | 30 to 200 kg |
| `bike_mass_kg` | 9 | 4 to 30 kg |
| `ftp_w` | 200 | 50 to 600 W |
| `trainer_difficulty` | 0.5 | 0.0 to 1.0 |
| `last_trainer_id` | absent | trainer address or name |
| `tv_dashboard_enabled` | true | true or false |
| `tv_dashboard_port` | 7878 | 1024 to 65535 |
| `fullscreen` | true | true or false |
| `control_trainer` | true | true sends grade; false only reads |

`fullscreen` is stored here. Godot applies it to the window. The session does not.

## Where Godot stores the file

The session takes a path. It does not choose one. Godot should pass the absolute path of `user://penguin-pass.toml` (`ProjectSettings.globalize_path`). That directory is the project user data folder, which on a desktop Linux build is under the app's Godot user data (`user://`), typically `~/.local/share/godot/app_userdata/<project name>/penguin-pass.toml`. Load it when the game starts. Save it when the rider leaves the settings screen or after a trainer connects, not every frame. Call `note_settings_load` with the load result so a damaged file can toast, and `save_settings` so a successful save can toast.

## For the interface

The HUD refreshes numbers a few times a second. `tick` still runs every frame. These calls are what the screens read.

**HUD numbers.** `readout`. Power and cadence are `None` (an em dash) when a real trainer is the source and the link is down or the sample is stale. The physics still uses 0 W, which is what `telemetry` shows. Coasting is `Some(0)`. Heart rate is `None` when no strap is reporting. The hero power is `power_3s_w`, the mean of known power over the last 3 seconds of session time. Unknown gaps are left out of that mean and still push older samples off the window, so a gap longer than 3 seconds clears it. `power_zone(power, ftp)` is the ladder: Z1 Recovery through Z7 Sprint. An exact percent boundary stays in the lower zone (55 % of FTP is Z1). At 0 W the HUD hides the ladder itself; the function still returns Z1.

**Ride list.** `telemetry` for time, distance, elevation, and the current lap. `distance_left_m` for `LEFT` on a route. This session's course is a loop, so that distance is the rest of the current lap.

**Next kilometre and skyline.** `fill_ahead(1000.0, step_m, &mut buffer)` for the road ahead. It wraps at the lap and reuses the buffer. `course().profile(step_m)` is the whole lap for the skyline. `course_facts` is the name, length, total ascent, and the steepest grades.

**Connection chip.** `connection`, not the status line. `phase` is idle, searching, connecting, connected, reconnecting, or failed. `resistance` is `Controlled` or `ReadingOnly`. Reading only covers both the settings toggle and a trainer that refused control. `bluetooth_available` is false when the adapter cannot start. `failure_reason` is one sentence; `failure_detail` is the raw text for the log. `status().line` is unchanged.

**TV chip.** `status().dashboard`, `dashboard_urls`, and `dashboard_error`. Each publish also sends `CastServer::publish_status` with FTP, the link phase, the resistance mode, and the 3-second power.

**Pairing rows.** `pairing` returns the list, strongest first, ties by name then id. `signal_bars` maps RSSI to 0..=4 and `signal_word` is the label. Unknown RSSI is 0 bars. A trainer stays for `PAIRING_HOLD_S` (8 s) after it was last seen. `remembered` flags `last_trainer_id`. `search_again` drops the current scan and starts a new one. The scan only lists Fitness Machine devices, so the list does not say whether a unit can control resistance.

**Summary.** `summary` at any time, then `end_ride` to freeze it. `end_ride` also sets a controlled trainer back to 0 % grade. Ticks after that do not move the bike until `restart`, which clears the totals. Elapsed time is the ride clock. Moving time is time spent above 0.5 m/s. Ascent is elevation gained, not the course total. Average power includes known zeros and leaves out time while the link was down. Maximum power is the highest full 1-second average, not one sample. Average cadence ignores zeros. Heart rate is reported only when a strap covered at least 5 seconds and 10 % of the elapsed time. `laps` has each lap's time, average power, and average speed.

**Title card.** `course_facts` for the name, length, and ascent. `connection` for the trainer mark.

**Toasts.** `drain_events`. The queue holds 32 and drops the oldest. It reports a trainer connecting, the link dropping, a reconnect, control refused, a lap finished, a dashboard bind failure, `save_settings`, and `note_settings_load` when the file was damaged.

**Power hint.** `SIM_POWER_STEP_W` is 25 W. The simulated-rider mockup still draws ±10 W.
