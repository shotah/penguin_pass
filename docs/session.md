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

The spawn copies `rider_mass_kg` and `bike_mass_kg` into the trainer config. Tacx FE-C uses the rider weight (page 55) so a slope feels like the rider's weight. Changing either mass in settings updates the on-screen physics immediately. The open trainer keeps the weight it was given until the next connection.

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
music_volume = 60
music_folder = "/home/rider/Music/rides"
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
| `course` | `penguin_pass` | `"penguin_pass"`, or `{ terrain, distance_km, seed }` |
| `music_volume` | 60 | 0 to 100 |
| `music_folder` | absent | path to the rider's music folder |
| `rider` | `nils` | any name; `nils` and `sigrid` are the usual ones |
| `zwift_controllers` | false | true or false |
| `heart_rate_id` | absent | Bluetooth address of a heart-rate strap |
| `heart_rate_name` | absent | advertised name of that strap |

`fullscreen` is stored here. Godot applies it to the window. The session does not.

`course` is the route loaded at the next start. `terrain` is `flat`, `rolling`, `mountain`, or `desert`. An older file with no `course` key loads Penguin Pass. A saved course that cannot be generated also loads Penguin Pass, and the session queues `CourseUnavailable` instead of crashing.

`music_volume` is the playback level Godot applies, from 0 to 100. `music_folder` is the rider's own folder of music files. An older file without these keys loads volume 60 and no folder. A folder that is not on disk is kept. `Settings::missing_music_folder` returns that path so the menu can say it is missing.

`rider` is the on-screen rider. A missing key loads `nils`. Any other text is kept. `zwift_controllers` loads `false` when the key is missing. `heart_rate_id` and `heart_rate_name` remember a strap. Delete them, or call `forget_heart_rate`, to forget it. An older file without these keys loads `nils`, controllers off, and no strap.

## Saved rides

Ending a ride writes one FIT file, marked cycling / virtual activity, with no map. A ride under 60 seconds or under 100 meters is not written. `end_ride` finishes the file. `restart` and `set_course` during a ride finish the current file and start another. Quitting mid-ride writes on drop. The title screen (`Ready`) records nothing; `hold_at_start` drops the open recording without writing it.

`RideSaved` carries the path. `RideNotSaved` carries one sentence, such as `Ride too short to save` or `Could not save the ride: ...`. `last_saved_ride` is the last path this session wrote.

Files go in `rides_folder` when that settings key is set. Otherwise they go in a `rides` directory next to the settings file passed to `set_settings_path`. `set_rides_dir` replaces both, which is how tests choose a temporary directory. With none of those set, nothing is written. The directory is created on the first save. For a normal `./x play` the settings file is `user://settings.toml`, so once the game passes that path the files land in `/home/deck/.local/share/godot/app_userdata/Penguin Pass/rides`.

The ride trace is a separate CSV and is not this file. The file name uses local time. Call `set_utc_offset_minutes` with the system's offset before the ride starts. The timestamps inside the FIT file stay UTC.

## Ride trace

Set `PENGUIN_PASS_TRACE_DIR` to a directory before the session is built, or call `Session::set_trace_dir` with that directory. `./x play --trace` creates `target/traces/<stamp>/` and exports the variable. Unset or empty means no file and no work on a tick. The variable is read once, when the session is constructed.

The file is `<dir>/ride.csv`. The directory is created if it is missing. On open the session prints `pp-session: trace <absolute path>` to stderr. If the file cannot be opened it prints `pp-session: trace not opened <path>: <reason>` and the ride continues.

One row per tick in `ready` and `riding`. Rows stop after `end_ride`. The writer is buffered, flushed at least once a second, and flushed again on `end_ride` and when the session is dropped. pp-trainer may later write `<dir>/ble.csv` beside this file. `unix_ms` is the wall clock in both, so the rows can be joined.

| Column | Meaning |
| --- | --- |
| `unix_ms` | Wall clock, milliseconds since the Unix epoch, read once per tick |
| `clock_s` | Session clock after the tick, seconds |
| `dt_s` | Frame duration the tick accepted, seconds |
| `phase` | `ready` or `riding` |
| `source` | `sim`, `trainer`, or `none` |
| `link` | `idle`, `scanning`, `connecting`, `connected`, `disconnected`, or `failed` |
| `power_w` | Watts this tick fed to the physics and the summary. Empty when the source did not report power |
| `cadence_rpm` | Cadence this tick fed onward. Empty when it was not reported |
| `trainer_speed_kph` | Wheel speed on the trainer sample. Empty when the sample has none |
| `heart_rate_bpm` | Heart rate this tick fed onward. Empty when it was not reported |
| `sample_changed` | `1` when this tick's sample differs from the previous tick, otherwise `0` |
| `sample_age_ms` | Age of the sample. Empty: the trainer sample has no timestamp |
| `game_speed_kph` | Speed from the bike model |
| `game_accel_mps2` | Change in game speed over the tick, divided by `dt_s`. Empty when `dt_s` is 0 |
| `grade_pct` | Course grade under the rider |
| `grade_sent_pct` | Grade most recently commanded to the trainer. Empty until one is sent |
| `distance_m` | Distance ridden |
| `power_3s_w` | Three-second mean power. Empty when the window has no sample |

Seconds and speeds use three decimal places. Watts use one. An empty cell means the value does not exist. The file never writes `NaN` or `None`.

## Where Godot stores the file

The session takes a path. It does not choose one. Godot should pass the absolute path of `user://penguin-pass.toml` (`ProjectSettings.globalize_path`). That directory is the project user data folder, which on a desktop Linux build is under the app's Godot user data (`user://`), typically `~/.local/share/godot/app_userdata/<project name>/penguin-pass.toml`. Load it when the game starts. Save it when the rider leaves the settings screen or after a trainer connects, not every frame. Call `note_settings_load` with the load result so a damaged file can toast, and `save_settings` so a successful save can toast.

## For the interface

The HUD refreshes numbers a few times a second. `tick` still runs every frame. These calls are what the screens read.

**HUD numbers.** `readout`. Power and cadence are `None` (an em dash) when a real trainer is the source and the link is down or the sample is stale. The physics still uses 0 W, which is what `telemetry` shows. Coasting is `Some(0)`. Heart rate is `None` when no strap is reporting. The hero power is `power_3s_w`, the mean of known power over the last 3 seconds of session time. Unknown gaps are left out of that mean and still push older samples off the window, so a gap longer than 3 seconds clears it. `power_zone(power, ftp)` is the ladder: Z1 Recovery through Z7 Sprint. An exact percent boundary stays in the lower zone (55 % of FTP is Z1). At 0 W the HUD hides the ladder itself; the function still returns Z1.

**Ride list.** `telemetry` for time, distance, elevation, and the current lap. `distance_left_m` for `LEFT` on a route. This session's course is a loop, so that distance is the rest of the current lap.

**Next kilometre and skyline.** `fill_ahead(1000.0, step_m, &mut buffer)` for the road ahead. It wraps at the lap and reuses the buffer. `course().profile(step_m)` is the whole lap for the skyline. `course_facts` is the name, length, total ascent, and the steepest grades.

**Connection chip.** `connection`, not the status line. `phase` is idle, searching, connecting, connected, reconnecting, or failed. `resistance` is `Controlled` or `ReadingOnly`. Reading only covers the settings toggle. When the trainer itself refuses control, `resistance` is still `ReadingOnly` and the status line says the trainer reports power only and cannot change resistance. `plain_failure` turns the trainer's `control not permitted` text into that same sentence. `bluetooth_available` is false when the adapter cannot start. `failure_reason` is one sentence when the link failed; `failure_detail` is the raw text for the log.

**TV chip.** `status().dashboard`, `dashboard_urls`, and `dashboard_error`. Each publish also sends `CastServer::publish_status` with FTP, the link phase, the resistance mode, and the 3-second power.

**Heart-rate strap.** The strap link is separate from the trainer. Nothing is spawned until `set_heart_rate_factory`. Pass `BleHeartRateFactory` for a real strap, a factory that returns `SimHeartRate` for the fake mode, and leave it unset when Bluetooth is off. A remembered `heart_rate_id` is connected as soon as the factory is set. `heart_rate_entries` is the nearby list, strongest first, with the same signal bars as the trainer list. `connect_heart_rate` remembers that strap. `forget_heart_rate` clears the two settings fields and disconnects. `heart_rate_state` is the trainer crate's `ConnectionState`. `heart_rate_name` is the live name, or the remembered name when the link is down. The readout, the summary, the TV telemetry, and the ride trace use the strap's bpm when it has one, and the trainer's bpm otherwise. `HeartRateConnected` and `HeartRateLost` fire once per transition. A strap drop does not touch the trainer or the grade.

**Pairing rows.** `pairing` returns the list, strongest first, ties by name then id. `signal_bars` maps RSSI to 0..=4 and `signal_word` is the label. Unknown RSSI is 0 bars. A trainer stays for `PAIRING_HOLD_S` (8 s) after it was last seen. `remembered` flags `last_trainer_id`. `search_again` drops the current scan and starts a new one. Each row carries `ftms`, `cycling_power`, and `fec` from the advertisement. A device can take resistance when `ftms` or `fec` is set. A row with only `cycling_power` is a power meter.

**Summary.** `summary` at any time, then `end_ride` to freeze it. `end_ride` also sets a controlled trainer back to 0 % grade. Ticks after that do not move the bike until `restart`, which clears the totals. Elapsed time is the ride clock. Moving time is time spent above 0.5 m/s. Ascent is elevation gained, not the course total. Average power includes known zeros and leaves out time while the link was down. Maximum power is the highest full 1-second average, not one sample. Average cadence ignores zeros. Heart rate is reported only when a strap covered at least 5 seconds and 10 % of the elapsed time. `laps` has each lap's time, average power, and average speed.

**Title card.** `course_facts` for the name, length, and ascent. `connection` for the trainer mark.

**Title screen.** A new session starts in `Riding`, so a game that never calls these keeps moving. `hold_at_start` zeros the ride and waits in `Ready`. In `Ready`, `tick` still scans, pairs, updates `readout`, and publishes the dashboard, but distance, the ride clock, laps, and `summary` stay put, and a controlled trainer is sent grade 0. Pedalling does not move the bike. `begin_ride` leaves `Ready` and starts the clock from zero. It does nothing in `Riding` or `Ended`. `end_ride` is `Ended`, and `ride_ended` agrees. `restart` still clears the ride and runs it (`Riding`). `phase` reports the three.

**Course picker.** `course_catalog` is six routes, built once in the background at startup and cached: Penguin Pass, Rolling 5 km (seed 3), Rolling 10 km (seed 3), Mountain 10 km (seed 11), Desert 10 km (seed 7), Flat 10 km (seed 1). Each row has length, ascent, grade range, theme, and at most 64 profile points. `set_course` builds the choice, resets the ride to zero, keeps the current phase and the trainer, refreshes facts, the road ahead, the summary, and the lap length, tells the TV, and queues `CourseSelected`. A failure leaves the old course and returns a sentence. `reroll(index)` replaces one generated row with the next seed (`None` for Penguin Pass or a bad index) and does not change the ride. `CourseChoice::theme_name` is `desert` or `alpine`. `label` is `Penguin Pass` or `Desert 10 km`.

**Toasts.** `drain_events`. The queue holds 32 and drops the oldest. It reports a trainer connecting, the link dropping, a reconnect, control refused, a lap finished, a dashboard bind failure, `save_settings`, `note_settings_load` when the file was damaged, `CourseSelected`, and `CourseUnavailable` when a saved course could not be built.

**TV course.** The dashboard profile follows `set_course`. `GET /api/course` returns the new outline, and an open page refreshes it.

**Music.** `settings().music_volume` is 0 to 100, default 60. `settings().music_folder` is the rider's folder, or absent. `missing_music_folder` is that path when it is not on disk.

**Power hint.** `SIM_POWER_STEP_W` is 25 W. The simulated-rider mockup still draws ±10 W.

## Segments, ghost, fish, and badges

`play` is the per-frame snapshot: one lap of segments, the open segment, the ghost, fish within about 150 m, the combo, and the powerup slot. `set_lane_position` takes a slot from 0.0 to 4.0 and is what collects a fish. The game calls it every frame. `activate_powerup` starts the held powerup. `profile` is the lifetime screen.

`settings.arcade` (default true) turns fish and powerups on. `settings.ghost` (default true) shows the stored ghost. With ghost display off, the ghost is still recorded. Both keys are missing from older settings files, which load as true.

`records.toml` and `arcade.toml` sit next to the settings file. They are read and written only when a settings path, a rides directory, or `rides_folder` was given, so a test that sets none of those never touches a folder. A file that does not parse does not stop the ride. A failed save is one warning line.

The built-in loop's record key is `penguin_pass`. A generated route's record key is the terrain name, the distance in kilometres, and the seed, so two deserts do not share records. The badge for riding every course uses the terrain name alone (`flat`, `rolling`, `mountain`, `desert`) or `penguin_pass`. Fish on the built-in loop use seed 1. A generated route uses its own seed.

Segments and the ghost advance only while the phase is `Riding`. The ghost replays from the start of the ride. `GhostBeaten` fires when the first lap beats the stored lap. A powerup changes the rider profile used for that frame's physics only. Trainer grade, recorded watts, and the FIT power channel stay as measured. `pp-segments` has no assisted flag, so a segment or a first lap ridden while a powerup was active is shown, and it is not stored as a best or as the ghost.

At the end of a ride the session adds the distance, ascent, time, fish, best combo, course, Penguin Pass laps, and whether a clean segment record fell to the profile, using the local date from `set_utc_offset_minutes`. Each new badge is a `BadgeEarned` event. Simulated rides count. The ride summary includes the fish count, the best combo, and each segment finish (name, time, and whether it was stored).

## Bots, workouts, and Strava

`settings.bots_count` is how many computer riders share the road, from 0 to 24, default 8. `0` leaves `play().bots` empty and the draft saving at 0. Otherwise the peloton is rebuilt when the course loads, the ride restarts, or the count changes. The built-in loop uses seed 1. A generated route uses its route seed. Each riding frame multiplies the bike's drag area by the draft factor, after any powerup drag scale, then steps the bike, then moves the bots. `draft_saving_pct` is `(1 - draft_factor) * 100`, rounded, and capped at 40.

`settings.time_of_day` is `day` (the default), `auto`, `dawn`, `sunset`, or `night`. `settings.weather` is `clear` (the default), `auto`, `cloudy`, `rain`, `snow`, or `fog`. Older files load those defaults.

`workouts` lists the built-in plans, then every `.zwo` file in `workouts` next to the settings file. `select_workout(None)` is a free ride. `select_workout(Some(id))` reloads that plan from the start. An unknown id is left as it was. `set_workout_bias` nudges targets by -25 to +25 percent. A ramp test ignores the nudge. `skip_workout_step` moves to the next step. While the phase is `Riding`, the plan is ticked. When the ERG helper says to send a number, the session sends `TrainerCommand::SetTargetPower` and does not send grade. `None` releases ERG and grade commands resume. `play().workout_erg_w` is the last target that was sent. `control_trainer` false still sends nothing. `accept_ramp_ftp` stores the new FTP and writes the settings file when a path is known. The summary carries `workout_result` after the plan ends.

`settings.strava_auto_upload` defaults to true. The Strava store is `strava.toml` next to the settings file, and it is opened only when a settings path or rides directory was given. No request is made until connect or upload. Each `tick` drains Strava events into the ride event queue, including after the ride has ended. A FIT file that was just written is uploaded when auto-upload is on and Strava is connected. `upload_last_ride_to_strava` sends the last saved file if it has not already been sent. `set_strava_endpoints` is for tests that point the link at a local server.
