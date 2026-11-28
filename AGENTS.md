# Agents

Open this file before searching the tree. It names the crate, the file, and the doc for each part of Penguin Pass.

The game is Godot 4.7. The only authored scene is [`godot/main.tscn`](godot/main.tscn): one `Game` node. Rust builds the scene tree, the meshes, and the HUD. The ride logic lives in Godot-free crates, and [`crates/pp-godot`](crates/pp-godot) draws the result.

[`README.md`](README.md) is the player-facing overview. [`CONTRIBUTING.md`](CONTRIBUTING.md) is the setup and the pull-request checklist. [`docs/dev-workflow.md`](docs/dev-workflow.md) is hooks and CI.

## Rules

1. Use the tables below, then the matching doc. When the table names a crate and you still need a symbol, search inside that crate. Search the whole repo only for a name this file does not mention.
2. Keep the change in the crate that already owns the behavior. [`pp-view`](crates/pp-view) decides strings, menus, colours, and layout numbers. [`pp-godot`](crates/pp-godot) draws and holds every Godot handle. [`pp-session`](crates/pp-session) ticks the ride. [`pp-course`](crates/pp-course) integrates the bike. [`pp-ftms`](crates/pp-ftms) is byte codecs. [`pp-trainer`](crates/pp-trainer) and [`pp-controllers`](crates/pp-controllers) own the radio.
3. The frame loop returns immediately. `Session::tick`, `Trainer::latest_sample`, and `Trainer::send` do not wait. Bluetooth, the companion dashboard, Strava, and controller I/O stay on background threads.
4. Tests use `--no-bluetooth` and `--fake-trainer`. A test does not open BlueZ or the public network. Strava tests talk to a local mock. Headless play is `./x play -- --no-bluetooth --fake-trainer --audio-driver Dummy`.
5. Run Cargo through [`./x`](x). Narrow with `CRATE=pp-session ./x test`. `PP_NATIVE=1` is for a host that can link Rust, which is how CI runs. `PROFILE=release ./x build` writes the release library into `godot/bin/`.
6. A behavior change updates the matching file under [`docs/`](docs/) in the same change. Update [`README.md`](README.md) when the player-facing summary changes.
7. Trust the code when a doc still says the session does not call it yet. FIT recording is already in [`session.rs`](crates/pp-session/src/session.rs). Strava is already in [`group.rs`](crates/pp-session/src/group.rs). [`docs/multiplayer.md`](docs/multiplayer.md) is still a design: there is no multiplayer crate.
8. A generated route is a byte-for-byte promise. The same `RouteSpec` keeps producing the same JSON. Read [`docs/route-generator.md`](docs/route-generator.md) before editing [`pp-routegen`](crates/pp-routegen).
9. New art and audio is CC0, or a licence [`docs/assets.md`](docs/assets.md) already allows. Put `LICENSE.txt` and `SOURCE.md` in the pack directory and add a row to that doc. Leave `godot/.godot/` and Godot `.import` files untracked. The game loads assets at runtime from [`godot/assets/manifest.json`](godot/assets/manifest.json).
10. Units are plain numbers. The field name carries the unit, in [`crates/pp-core`](crates/pp-core/src/lib.rs). The world is right-handed, Y up, X east, Z north. The bike faces -Z. The rider's left is -X.
11. Make the smallest change that solves the task. The workspace is `crates/*` ([`Cargo.toml`](Cargo.toml)), edition 2024. Add a new crate only when none of the ones below already own the job.

## Where to change it

| Task | Doc | Code |
| --- | --- | --- |
| Frame loop, scene tree, what a ride shows | [game.md](docs/game.md) | [`pp-godot/src/game.rs`](crates/pp-godot/src/game.rs), [`godot/main.tscn`](godot/main.tscn) |
| HUD numbers and the bottom console | [ui.md](docs/ui.md) | [`pp-view/src/hud.rs`](crates/pp-view/src/hud.rs), [`pp-view/src/console.rs`](crates/pp-view/src/console.rs), [`pp-godot/src/hud.rs`](crates/pp-godot/src/hud.rs) |
| Title, pause, summary, course list | [ui.md](docs/ui.md) | [`pp-view/src/shell.rs`](crates/pp-view/src/shell.rs), [`pp-godot/src/screens.rs`](crates/pp-godot/src/screens.rs) |
| Settings menu and pairing screen | [ui.md](docs/ui.md), [session.md](docs/session.md) | [`pp-view/src/menu.rs`](crates/pp-view/src/menu.rs), [`pp-godot/src/menu.rs`](crates/pp-godot/src/menu.rs) |
| Button prompts (keyboard, Deck, Zwift) | [ui.md](docs/ui.md), [controllers.md](docs/controllers.md) | [`pp-view/src/prompts.rs`](crates/pp-view/src/prompts.rs) |
| Shared colours, type, panels | [ui.md](docs/ui.md) | [`pp-godot/src/ui_style.rs`](crates/pp-godot/src/ui_style.rs) |
| Launch flags (`--fake-trainer`, `--settings`, `--trace`) | [play.md](docs/play.md) | [`pp-view/src/launch.rs`](crates/pp-view/src/launch.rs), [`pp-godot/src/launch.rs`](crates/pp-godot/src/launch.rs) |
| Keyboard power and hold-to-repeat | [play.md](docs/play.md) | [`pp-view/src/power.rs`](crates/pp-view/src/power.rs), [`pp-view/src/hold.rs`](crates/pp-view/src/hold.rs), [`pp-godot/src/sim.rs`](crates/pp-godot/src/sim.rs) |
| Key and Steam Deck bindings | [play.md](docs/play.md) | [`pp-godot/src/input_map.rs`](crates/pp-godot/src/input_map.rs) |
| Ride tick, trainer connect, grade send | [session.md](docs/session.md) | [`pp-session/src/session.rs`](crates/pp-session/src/session.rs), [`pp-session/src/link.rs`](crates/pp-session/src/link.rs) |
| Settings file | [session.md](docs/session.md) | [`pp-session/src/settings.rs`](crates/pp-session/src/settings.rs) |
| Power zones | [session.md](docs/session.md) | [`pp-session/src/zones.rs`](crates/pp-session/src/zones.rs) |
| Course list, reverse, generated presets | [course.md](docs/course.md), [route-generator.md](docs/route-generator.md) | [`pp-session/src/course_sel.rs`](crates/pp-session/src/course_sel.rs) |
| Road geometry and bike physics | [course.md](docs/course.md) | [`pp-course/src/course.rs`](crates/pp-course/src/course.rs), [`physics.rs`](crates/pp-course/src/physics.rs), [`ride.rs`](crates/pp-course/src/ride.rs) |
| Authored route JSON | [course.md](docs/course.md) | [`godot/routes/penguin_pass/route.json`](godot/routes/penguin_pass/route.json), [`godot/routes/lonely_road/route.json`](godot/routes/lonely_road/route.json) |
| Seeded route generator | [route-generator.md](docs/route-generator.md) | [`pp-routegen/src/`](crates/pp-routegen/src) |
| Which world chunks exist | [world-chunks.md](docs/world-chunks.md) | [`pp-world/src/`](crates/pp-world/src) |
| Terrain, scatter, landmarks, sky | [world.md](docs/world.md), [world-chunks.md](docs/world-chunks.md) | [`pp-godot/src/world/`](crates/pp-godot/src/world), [`pp-view/src/relief.rs`](crates/pp-view/src/relief.rs), [`scatter.rs`](crates/pp-view/src/scatter.rs), [`roadbed.rs`](crates/pp-view/src/roadbed.rs) |
| Theme JSON | [world.md](docs/world.md) | [`godot/themes/`](godot/themes), [`pp-view/src/theme.rs`](crates/pp-view/src/theme.rs), [`pp-godot/src/theme.rs`](crates/pp-godot/src/theme.rs) |
| Road mesh | [world.md](docs/world.md) | [`pp-godot/src/road.rs`](crates/pp-godot/src/road.rs) |
| Camera | [game.md](docs/game.md) | [`pp-godot/src/camera.rs`](crates/pp-godot/src/camera.rs) |
| Rider mesh, pose, kits | [rider.md](docs/rider.md) | [`pp-rider/src/`](crates/pp-rider/src), [`pp-godot/src/rider.rs`](crates/pp-godot/src/rider.rs) |
| Rider preview (no game library) | [rider.md](docs/rider.md) | [`tools/rider-preview/`](tools/rider-preview) |
| Lateral lanes and the gravel shoulder | [game.md](docs/game.md) | [`pp-view/src/lane.rs`](crates/pp-view/src/lane.rs) |
| Smart trainer protocols | [trainers.md](docs/trainers.md), [bluetooth.md](docs/bluetooth.md) | [`pp-ftms/src/`](crates/pp-ftms/src), [`pp-trainer/src/ble.rs`](crates/pp-trainer/src/ble.rs) |
| Simulated trainer | [bluetooth.md](docs/bluetooth.md) | [`pp-trainer/src/sim.rs`](crates/pp-trainer/src/sim.rs), [`pp-godot/src/fake_trainer.rs`](crates/pp-godot/src/fake_trainer.rs) |
| Trainer scan binary | [trainers.md](docs/trainers.md) | [`pp-trainer/src/bin/pp-scan.rs`](crates/pp-trainer/src/bin/pp-scan.rs) (`./x scan`) |
| Heart-rate strap | [bluetooth.md](docs/bluetooth.md) | [`pp-trainer/src/heart.rs`](crates/pp-trainer/src/heart.rs), [`heart_ble.rs`](crates/pp-trainer/src/heart_ble.rs), [`pp-session/src/heart.rs`](crates/pp-session/src/heart.rs) |
| Hardware traces | [trainer-trace-plan.md](docs/trainer-trace-plan.md) | [`pp-trainer/src/trace.rs`](crates/pp-trainer/src/trace.rs), [`pp-session/src/trace.rs`](crates/pp-session/src/trace.rs) |
| Zwift Click, Play, Ride | [controllers.md](docs/controllers.md) | [`pp-controllers/src/`](crates/pp-controllers/src), [`pp-view/src/controllers.rs`](crates/pp-view/src/controllers.rs) |
| Controller scan binary | [controllers.md](docs/controllers.md) | [`pp-controllers/src/bin/pp-controller-scan.rs`](crates/pp-controllers/src/bin/pp-controller-scan.rs) (`./x controllers`) |
| Pace riders and drafting | [bots.md](docs/bots.md) | [`pp-bots/src/`](crates/pp-bots/src), [`pp-session/src/group.rs`](crates/pp-session/src/group.rs), [`pp-godot/src/bots.rs`](crates/pp-godot/src/bots.rs) |
| Fish, powerups, badges | [arcade.md](docs/arcade.md) | [`pp-arcade/src/`](crates/pp-arcade/src), [`pp-session/src/play.rs`](crates/pp-session/src/play.rs), [`pp-view/src/arcade.rs`](crates/pp-view/src/arcade.rs), [`pp-godot/src/fish.rs`](crates/pp-godot/src/fish.rs) |
| Timed segments, records, ghost | [segments.md](docs/segments.md) | [`pp-segments/src/`](crates/pp-segments/src), [`pp-session/src/play.rs`](crates/pp-session/src/play.rs) |
| Segment arches | [segments.md](docs/segments.md) | [`pp-godot/src/banners.rs`](crates/pp-godot/src/banners.rs) |
| Structured workouts, ERG, `.zwo` | [workouts.md](docs/workouts.md) | [`pp-workout/src/`](crates/pp-workout/src), [`pp-session/src/group.rs`](crates/pp-session/src/group.rs) |
| FIT ride file | [recording.md](docs/recording.md) | [`pp-record/src/`](crates/pp-record/src), [`pp-session/src/session.rs`](crates/pp-session/src/session.rs) |
| Strava connect and upload | [strava.md](docs/strava.md) | [`pp-strava/src/`](crates/pp-strava/src), [`pp-session/src/group.rs`](crates/pp-session/src/group.rs) |
| Companion dashboard page | [companion-dashboard.md](docs/companion-dashboard.md), [dashboard.md](docs/dashboard.md) | [`pp-cast/src/server.rs`](crates/pp-cast/src/server.rs), [`status.rs`](crates/pp-cast/src/status.rs), [`dashboard.html`](crates/pp-cast/src/dashboard.html) |
| Music | [music.md](docs/music.md) | [`godot/assets/music/`](godot/assets/music), [`pp-view/src/music.rs`](crates/pp-view/src/music.rs), [`pp-godot/src/music.rs`](crates/pp-godot/src/music.rs) |
| Ambience and effects | [sound.md](docs/sound.md) | [`godot/assets/sfx/`](godot/assets/sfx), [`pp-view/src/sound.rs`](crates/pp-view/src/sound.rs), [`pp-godot/src/sound.rs`](crates/pp-godot/src/sound.rs) |
| Time of day and weather | [world.md](docs/world.md) | [`pp-world/src/atmosphere.rs`](crates/pp-world/src/atmosphere.rs), [`pp-godot/src/world/atmosphere.rs`](crates/pp-godot/src/world/atmosphere.rs) |
| Asset packs | [assets.md](docs/assets.md) | [`godot/assets/`](godot/assets), [`pp-godot/src/assets.rs`](crates/pp-godot/src/assets.rs) |
| Flatpak | [flatpak.md](docs/flatpak.md) | [`packaging/flatpak/`](packaging/flatpak) |
| Steam store and SteamPipe | [steam-release.md](docs/steam-release.md), [steam-assets.md](docs/steam-assets.md) | [`packaging/steam/`](packaging/steam), [`tools/steam/`](tools/steam) |
| Numbered release | [release.md](docs/release.md) | [`./x`](x) `release` |
| Windows and macOS | [cross-platform.md](docs/cross-platform.md) | [`.github/workflows/windows.yml`](.github/workflows/windows.yml), [`macos.yml`](.github/workflows/macos.yml) |
| Public website | | [`site/`](site), [`.github/workflows/pages.yml`](.github/workflows/pages.yml) |
| Other people on the same road | [multiplayer.md](docs/multiplayer.md) | Design only. No crate yet. |

## Crates

Each crate's `src/lib.rs` says what it owns. Read that comment before the rest of the crate.

| Crate | Owns |
| --- | --- |
| [`pp-core`](crates/pp-core) | Shared types: telemetry, units, rider profile. One file, [`src/lib.rs`](crates/pp-core/src/lib.rs). |
| [`pp-ftms`](crates/pp-ftms) | GATT codecs: FTMS, cycling power, FE-C, Wahoo, Zwift, CSC, heart rate. No sockets. Modules: `control`, `csc`, `cycling_power`, `fec`, `heart_rate`, `indoor_bike`, `wahoo`, `wire`, `zwift`, `uuid`. |
| [`pp-course`](crates/pp-course) | Centerline, grade, `Ride::step`. Compiles the two route JSON files in `godot/routes/`. |
| [`pp-trainer`](crates/pp-trainer) | `Trainer` trait, BlueZ (`ble.rs`), keyboard rider (`sim.rs`), heart-rate strap, BLE trace. |
| [`pp-controllers`](crates/pp-controllers) | Zwift controller radio (`ble.rs`), byte layout (`protocol.rs`), `SimControllers` (`sim.rs`). |
| [`pp-routegen`](crates/pp-routegen) | Seeded courses. `generate` is closed and stable. `endless` is the future open road. |
| [`pp-world`](crates/pp-world) | Chunk planner, heightfield, landmark placement, atmosphere. No meshes. |
| [`pp-rider`](crates/pp-rider) | Procedural bike and rider meshes, kits, pedalling pose. No engine types. |
| [`pp-view`](crates/pp-view) | Everything the screens decide, with no `Gd` handle. File list below. |
| [`pp-bots`](crates/pp-bots) | Peloton, pace and company riders, draft factor. |
| [`pp-workout`](crates/pp-workout) | Workout plan (`model.rs`), built-ins (`builtin.rs`), `.zwo` (`zwo.rs`), ERG target (`erg.rs`). |
| [`pp-arcade`](crates/pp-arcade) | Fish field, powerups, badge profile. Powerups change mass and drag, never the rider's watts. |
| [`pp-segments`](crates/pp-segments) | Segment detection, timer, personal records, best-lap ghost. |
| [`pp-record`](crates/pp-record) | One FIT activity per ride (`fit.rs`, `recorder.rs`). |
| [`pp-strava`](crates/pp-strava) | Keys, token, and FIT upload (`api.rs`, `store.rs`, `pages.rs`). |
| [`pp-cast`](crates/pp-cast) | LAN dashboard: HTTP plus a live telemetry push. HTML is [`dashboard.html`](crates/pp-cast/src/dashboard.html). |
| [`pp-session`](crates/pp-session) | The Godot-free ride. File list below. |
| [`pp-godot`](crates/pp-godot) | GDExtension. Entry is [`godot/penguin_pass.gdextension`](godot/penguin_pass.gdextension). File list below. |

### `pp-session`

| File | What a tick and a menu find there |
| --- | --- |
| [`session.rs`](crates/pp-session/src/session.rs) | Physics, trainer, dashboard, and the FIT recorder on one tick. |
| [`settings.rs`](crates/pp-session/src/settings.rs) | TOML preferences. A missing or broken file loads defaults. |
| [`play.rs`](crates/pp-session/src/play.rs) | Segments, ghost, fish, powerups, badges. Writes `records.toml` and `arcade.toml`. |
| [`group.rs`](crates/pp-session/src/group.rs) | Bots, the workout runner, and the Strava link (`strava.toml`). |
| [`course_sel.rs`](crates/pp-session/src/course_sel.rs) | Ride phase and the picked course, including generated presets. |
| [`zones.rs`](crates/pp-session/src/zones.rs) | Coggan zones for the HUD and the dashboard. |
| [`readout.rs`](crates/pp-session/src/readout.rs) | Live HUD numbers, including the 3-second power average. |
| [`stats.rs`](crates/pp-session/src/stats.rs) | Running totals and the frozen summary. |
| [`heart.rs`](crates/pp-session/src/heart.rs) | Heart-rate strap, separate from the trainer. |
| [`link.rs`](crates/pp-session/src/link.rs) | How the session obtains a trainer without naming BlueZ on the frame. |
| [`connection.rs`](crates/pp-session/src/connection.rs) | Structured link status for the trainer chip and the pairing screen. |
| [`signal.rs`](crates/pp-session/src/signal.rs) | Signal bars for the pairing list. |
| [`events.rs`](crates/pp-session/src/events.rs) | Bounded toast queue. |
| [`trace.rs`](crates/pp-session/src/trace.rs) | Optional per-tick ride log. Idle unless a directory was set. |

### `pp-view`

Godot calls these and draws the value. A new HUD string, menu row, or colour rule goes here first, with a unit test, and the matching `pp-godot` file only paints it.

| File | Decides |
| --- | --- |
| [`hud.rs`](crates/pp-view/src/hud.rs) | HUD strings from telemetry. |
| [`console.rs`](crates/pp-view/src/console.rs) | Ride-console strings, zone, trainer chip. |
| [`format.rs`](crates/pp-view/src/format.rs) | Number and grade-colour formatting. |
| [`menu.rs`](crates/pp-view/src/menu.rs) | Settings menu state machine. |
| [`shell.rs`](crates/pp-view/src/shell.rs) | Title, pause, summary, course screens. |
| [`prompts.rs`](crates/pp-view/src/prompts.rs) | Button names for the last device the rider used. |
| [`launch.rs`](crates/pp-view/src/launch.rs) | Command-line and environment flags. |
| [`lane.rs`](crates/pp-view/src/lane.rs) | Nine road positions, from the left edge onto the gravel shoulder. |
| [`power.rs`](crates/pp-view/src/power.rs) | Keyboard power target. |
| [`hold.rs`](crates/pp-view/src/hold.rs) | Hold-to-repeat timing. |
| [`controllers.rs`](crates/pp-view/src/controllers.rs) | Zwift events mapped onto the ride and the menus. |
| [`arcade.rs`](crates/pp-view/src/arcade.rs) | Text for fish, segments, the ghost, powerups, and badges. |
| [`extras.rs`](crates/pp-view/src/extras.rs) | Text for the sky, the bots, workouts, and Strava. |
| [`music.rs`](crates/pp-view/src/music.rs) | Playlist rules. |
| [`sound.rs`](crates/pp-view/src/sound.rs) | Ambience and effect levels. |
| [`theme.rs`](crates/pp-view/src/theme.rs) | Theme JSON checks. |
| [`elevation.rs`](crates/pp-view/src/elevation.rs) | Elevation-strip geometry. |
| [`profile.rs`](crates/pp-view/src/profile.rs) | Course skyline and the next-kilometre chart. |
| [`rides.rs`](crates/pp-view/src/rides.rs) | How a saved ride is named on the summary. |
| [`relief.rs`](crates/pp-view/src/relief.rs) | Ground height beside the road. |
| [`roadbed.rs`](crates/pp-view/src/roadbed.rs) | Centerline queries where the road folds back. |
| [`scatter.rs`](crates/pp-view/src/scatter.rs) | Tree and prop placement. |

### `pp-godot`

| File | Draws |
| --- | --- |
| [`game.rs`](crates/pp-godot/src/game.rs) | The `Game` node. Owns the world, rider, camera, HUD, and menu. |
| [`launch.rs`](crates/pp-godot/src/launch.rs) | Turns process flags into a session. Default settings path is `user://settings.toml`. |
| [`hud.rs`](crates/pp-godot/src/hud.rs) | Bottom ride console. |
| [`screens.rs`](crates/pp-godot/src/screens.rs) | Title, pause, summary, course list, confirm, toasts. |
| [`menu.rs`](crates/pp-godot/src/menu.rs) | Settings overlay. The ride keeps ticking. |
| [`ui_style.rs`](crates/pp-godot/src/ui_style.rs) | Shared colours, type, and surfaces. |
| [`input_map.rs`](crates/pp-godot/src/input_map.rs) | Keyboard and Steam Deck bindings, installed from code. |
| [`sim.rs`](crates/pp-godot/src/sim.rs) | Keyboard stand-in. Power is stored on the session. |
| [`fake_trainer.rs`](crates/pp-godot/src/fake_trainer.rs) | In-process trainers. No Bluetooth adapter. |
| [`camera.rs`](crates/pp-godot/src/camera.rs) | Chase camera and handlebar view. |
| [`rider.rs`](crates/pp-godot/src/rider.rs) | Turns `pp-rider` surfaces into meshes. |
| [`road.rs`](crates/pp-godot/src/road.rs) | Road ribbon from the centerline. |
| [`routes.rs`](crates/pp-godot/src/routes.rs) | Scans `res://routes/` and `user://routes/`. A user route with the same id wins. |
| [`theme.rs`](crates/pp-godot/src/theme.rs) | Reads `res://themes/<name>.json`. |
| [`world/mod.rs`](crates/pp-godot/src/world/mod.rs) | Sky, sun, and `build_course`. |
| [`world/terrain.rs`](crates/pp-godot/src/world/terrain.rs) | Chunked heightfield. |
| [`world/scenery.rs`](crates/pp-godot/src/world/scenery.rs) | Scatter, guard rails, posts, start arch. |
| [`world/landmarks.rs`](crates/pp-godot/src/world/landmarks.rs) | Roadside landmarks and the horizon peak. |
| [`world/atmosphere.rs`](crates/pp-godot/src/world/atmosphere.rs) | Applies time of day and weather to the sun, sky, fog, road, and rider. |
| [`bots.rs`](crates/pp-godot/src/bots.rs) | Computer riders as real rider meshes. |
| [`fish.rs`](crates/pp-godot/src/fish.rs) | Fish meshes. |
| [`banners.rs`](crates/pp-godot/src/banners.rs) | Climb and sprint arches. |
| [`music.rs`](crates/pp-godot/src/music.rs) | Two players crossfading on the Music bus. |
| [`sound.rs`](crates/pp-godot/src/sound.rs) | Loads sfx, keeps buses, starts one-shots. |
| [`assets.rs`](crates/pp-godot/src/assets.rs) | Runtime library from `res://assets/manifest.json`. |
| [`mesh_util.rs`](crates/pp-godot/src/mesh_util.rs) | Shared materials and box meshes. |

## Godot project and player files

| Path | What it is |
| --- | --- |
| [`godot/project.godot`](godot/project.godot) | Project file. Main scene `res://main.tscn`. Viewport 1280×800. Mobile renderer. |
| [`godot/main.tscn`](godot/main.tscn) | The one authored scene. |
| [`godot/penguin_pass.gdextension`](godot/penguin_pass.gdextension) | Loads `res://bin/libpp_godot.*` (`.so`, `.dll`, `.dylib`). |
| [`godot/bin/`](godot/bin) | `./x build` output. |
| [`godot/routes/<id>/route.json`](godot/routes/penguin_pass/route.json) | Authored courses. Built-ins are Penguin Pass and Lonely Road. Extra folders are picked up by [`routes.rs`](crates/pp-godot/src/routes.rs). |
| [`godot/themes/`](godot/themes) | `alpine.json`, `desert.json`, `farmland.json`. `PENGUIN_PASS_THEME` overrides the name. |
| [`godot/assets/manifest.json`](godot/assets/manifest.json) | Asset index. |
| [`godot/assets/landmarks/`](godot/assets/landmarks) | CC0 landmark packs, each with `LICENSE.txt` and `SOURCE.md`. |
| [`godot/assets/music/music.json`](godot/assets/music/music.json) | Bundled track list. |
| [`godot/assets/sfx/`](godot/assets/sfx) | Ride, UI, and ambience clips. |
| [`godot/tests/smoke.gd`](godot/tests/smoke.gd) | Headless load check. `./x smoke` looks for `PENGUIN_PASS_SMOKE_OK`. |
| [`godot/icon.svg`](godot/icon.svg) | Window icon. |

On a normal `./x play`, Godot resolves `user://` to the project user directory. On this Linux layout that is `~/.local/share/godot/app_userdata/Penguin Pass/`. Override the settings file with `--settings` or `PENGUIN_PASS_SETTINGS`.

| File next to the settings | Written by |
| --- | --- |
| `settings.toml` | [`settings.rs`](crates/pp-session/src/settings.rs) |
| `rides/*.fit` | [`session.rs`](crates/pp-session/src/session.rs). `rides_folder` in settings overrides the directory. |
| `records.toml` | [`play.rs`](crates/pp-session/src/play.rs). Reverse efforts use a `:rev` key. |
| `arcade.toml` | [`play.rs`](crates/pp-session/src/play.rs) |
| `strava.toml` | [`group.rs`](crates/pp-session/src/group.rs) |
| `workouts/` | Extra `.zwo` files, loaded beside the built-in library. |
| `user://routes/<id>/` | Player routes. Same shape as `godot/routes/`. |

`./x play --trace` writes a folder under `target/traces/<date-time>/`. The report that matters is `ble.csv`. The plan for what to ride is [`docs/trainer-trace-plan.md`](docs/trainer-trace-plan.md).

## Tools, packaging, CI

| Path | What it is |
| --- | --- |
| [`x`](x) | Command runner. The [`Makefile`](Makefile) calls the same script. |
| [`tools/Containerfile`](tools/Containerfile) | Steam Runtime toolbox image (`./x toolbox`). |
| [`tools/export-linux.sh`](tools/export-linux.sh), [`tools/export-player.sh`](tools/export-player.sh) | Player export used by `./x export`. |
| [`tools/licenses.py`](tools/licenses.py) | Regenerates [`docs/third-party-licenses.md`](docs/third-party-licenses.md) (`./x licenses`). |
| [`tools/godot/`](tools/godot) | Official editor and export templates from `./x fetch`. Checksums against Godot's `SHA512-SUMS.txt`. |
| [`tools/rider-preview/`](tools/rider-preview) | Standalone Godot project for the rider. It does not load the game library. |
| [`tools/steam/`](tools/steam) | SteamPipe upload templates. Placeholders. |
| [`packaging/flatpak/`](packaging/flatpak) | Manifest, metainfo, install scripts. |
| [`packaging/steam/assets/`](packaging/steam/assets) | Store capsules. [`render-assets.sh`](packaging/steam/render-assets.sh) rebuilds them. |
| [`lefthook.yml`](lefthook.yml) | `pre-commit` runs `./x fmt` and `./x clippy`. `pre-push` runs `./x typecheck`, then `./x test`. |
| [`.github/workflows/ci.yml`](.github/workflows/ci.yml) | `lint`, `typecheck`, `test`, `smoke` on Ubuntu, with `PP_NATIVE=1`. |
| [`.github/workflows/flatpak.yml`](.github/workflows/flatpak.yml) | Flatpak bundle. |
| [`.github/workflows/desktop-player.yml`](.github/workflows/desktop-player.yml) | Desktop player export. |
| [`site/`](site) | Public site. [`pages.yml`](.github/workflows/pages.yml) publishes it. |

## Docs

Play and the screen:

- [play.md](docs/play.md) — start the game on this machine, desktop entry, non-Steam shortcut
- [game.md](docs/game.md) — the node tree, cameras, and what the ride screen contains
- [ui.md](docs/ui.md) — interface design, colours, type, and the 1280×800 frame
- [session.md](docs/session.md) — what `Session::tick` does, difficulty, connect, and reconnect

The ride:

- [course.md](docs/course.md) — Penguin Pass geometry and the bike model
- [route-generator.md](docs/route-generator.md) — seeded routes and the stable JSON
- [world.md](docs/world.md) — ground, themes, time of day, weather
- [world-chunks.md](docs/world-chunks.md) — which chunks load around the rider
- [rider.md](docs/rider.md) — procedural bike and rider
- [bots.md](docs/bots.md) — pace riders and the draft
- [arcade.md](docs/arcade.md) — fish, powerups, badges
- [segments.md](docs/segments.md) — climbs, sprints, laps, records, ghost
- [workouts.md](docs/workouts.md) — built-in workouts, `.zwo`, ERG
- [recording.md](docs/recording.md) — FIT file contents and where `rides/` goes
- [strava.md](docs/strava.md) — player API keys and upload
- [music.md](docs/music.md) — bundled tracks and the player's folder
- [sound.md](docs/sound.md) — ambience, coasting, UI sounds
- [assets.md](docs/assets.md) — packs, licences, credits

Hardware:

- [trainers.md](docs/trainers.md) — which trainers the protocols cover
- [bluetooth.md](docs/bluetooth.md) — FTMS, FE-C, Wahoo, Zwift, CSC, heart rate
- [controllers.md](docs/controllers.md) — Zwift Click, Play, and Ride
- [trainer-trace-plan.md](docs/trainer-trace-plan.md) — what to ride on real hardware, and what is still unconfirmed

The second screen:

- [companion-dashboard.md](docs/companion-dashboard.md) — the LAN page
- [dashboard.md](docs/dashboard.md) — status fields that page shows

Shipping:

- [flatpak.md](docs/flatpak.md) — bundle and publish
- [release.md](docs/release.md) — `./x release`
- [steam-release.md](docs/steam-release.md) — Steamworks checklist
- [steam-assets.md](docs/steam-assets.md) — capsule sizes
- [steam-store-copy.md](docs/steam-store-copy.md) — store page text
- [cross-platform.md](docs/cross-platform.md) — Windows and macOS
- [third-party-licenses.md](docs/third-party-licenses.md) — generated crate list (`./x licenses`)
- [dev-workflow.md](docs/dev-workflow.md) — lefthook and GitHub Actions

Not built:

- [multiplayer.md](docs/multiplayer.md) — rooms over Steam, still a design

Pictures live under [`docs/img/`](docs/img). UI mockups are SVG under `docs/img/ui/src/`, regenerated by `docs/img/ui/render.sh`. They are pictures of the design, not screenshots of the game.

## Commands

`./x` usage: `toolbox`, `build`, `test`, `test-unit`, `test-integration`, `fmt`, `clippy`, `lint`, `typecheck`, `smoke`, `run`, `scan`, `controllers`, `fetch`, `play`, `export`, `package`, `licenses`, `release`.

| Check | Command |
| --- | --- |
| One crate | `CRATE=pp-view ./x test` |
| Format and clippy | `./x lint` |
| Typecheck | `./x typecheck` |
| The game loads | `./x smoke` |
| Fast unit tests | `./x test-unit` |
| Integration tests | `./x test-integration` |

After a Rust change, run `./x typecheck` and `./x lint`, then the tests for the crate you touched. `./x test` is the full suite. `./x smoke` after a change that affects loading the extension or the scene.
