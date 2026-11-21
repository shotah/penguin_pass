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

The session takes a path. It does not choose one. Godot should pass the absolute path of `user://penguin-pass.toml` (`ProjectSettings.globalize_path`). That directory is the project user data folder, which on a desktop Linux build is under the app's Godot user data (`user://`), typically `~/.local/share/godot/app_userdata/<project name>/penguin-pass.toml`. Load it when the game starts. Save it when the rider leaves the settings screen or after a trainer connects, not every frame.
