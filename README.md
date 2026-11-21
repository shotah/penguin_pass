# Penguin Pass

<img src="docs/img/logo.svg" alt="Penguin Pass logo" width="160">

A native Linux indoor-cycling game. Pair a smart trainer over Bluetooth,
ride a 5 km mountain loop, and put the ride on the TV. No Wine, no
controller hacks.

Built for SteamOS and a Garmin Tacx Neo 2T first. Any trainer that speaks
the standard Bluetooth Fitness Machine Service (FTMS) should work.

## Status

Early build. See [Roadmap](#roadmap) for what works and what does not.

## Why

Zwift and MyWhoosh do not ship for Linux, and neither casts to a TV from
a Steam Deck. Penguin Pass is a Godot 4 game with its core in Rust, so it
runs natively on SteamOS and can ship on Steam.

## How it works

```text
Tacx Neo 2T  <-- BLE / FTMS -->  pp-trainer  -->  pp-course (physics)
                                     |                  |
                                     v                  v
                                 pp-godot (GDExtension, Godot 4 scene)
                                     |
                                     v
                                 pp-cast  -->  TV / tablet browser
```

- The trainer reports power, cadence, and speed. The game owns the
  physics: it turns your power into speed on the current slope.
- The game sends the current grade back to the trainer, so climbs feel
  like climbs.
- With no trainer connected, a simulated rider drives the game from the
  keyboard. Development and tests do not need the bike.

## Courses

A course is a data file, not code. `crates/pp-course/courses/penguin_pass.json`
lists the road as straights and arcs plus a gradient profile, and
`Course::from_str` loads any other file in the same format. The 3D world
(terrain, road, scenery) is generated from the course at startup, so a
new route does not need hand-built scenery. See `docs/course.md`.

Not built yet: choosing between courses in the game, per-course themes
(alpine, desert), and routes generated from a seed and a distance.

## Layout

| Path | What it holds |
| --- | --- |
| `crates/pp-core` | Shared types: telemetry snapshot, units, rider profile |
| `crates/pp-ftms` | Bluetooth GATT codec for FTMS, Cycling Power, Heart Rate. No I/O |
| `crates/pp-course` | Course data, grade lookup, bike physics |
| `crates/pp-trainer` | `Trainer` trait, BLE trainer (BlueZ), simulated trainer |
| `crates/pp-cast` | LAN TV dashboard: HTTP page plus live telemetry stream |
| `crates/pp-session` | Ride loop without Godot: settings file, trainer connect, TV dashboard |
| `crates/pp-world` | Which world chunks to load around the rider, and per-chunk seeds |
| `crates/pp-rider` | The bike and rider as generated meshes, with pedalling poses |
| `crates/pp-routegen` | Routes generated from a seed, a distance, and a terrain preset |
| `crates/pp-view` | Interface logic without Godot: formatting, menu state, HUD values, theme parsing |
| `crates/pp-godot` | GDExtension library: world, HUD, rider, camera |
| `godot/` | Godot project: `project.godot`, scenes, export presets |
| `godot/assets/` | CC0 models, textures, and sky. Credits in `docs/assets.md` |
| `docs/` | Design notes and the Steam release checklist |
| `tools/` | Build container definition and helper scripts |

## Requirements

- SteamOS or another Linux with BlueZ
- Godot 4.7 (`flatpak install flathub org.godotengine.Godot`)
- `podman` (ships with SteamOS)
- `rustup` is optional on the host; the build runs in a container

SteamOS has no C compiler, so Rust cannot link on the host. All cargo
commands run inside a build container based on the Steam Runtime 4 SDK.
The same container produces binaries that are safe to ship on Steam.

## Commands

SteamOS does not ship `make` either, so the command runner is the `./x`
script. The `Makefile` calls the same script on hosts that have `make`.

| Command | What it does |
| --- | --- |
| `./x toolbox` | Build the build container (once) |
| `./x build` | Build the GDExtension library into `godot/bin/` |
| `./x test` | Run all Rust tests |
| `./x fmt` | `cargo fmt --check` |
| `./x clippy` | `cargo clippy -D warnings` |
| `./x lint` | `./x fmt`, then `./x clippy` |
| `./x typecheck` | `cargo check` across the workspace |
| `./x smoke` | Load the game headless in Godot and check it starts |
| `./x run` | Open the project with the Godot flatpak, with Bluetooth access |
| `./x scan` | List nearby trainers over Bluetooth |
| `./x fetch` | Download the official Godot editor and export templates, checksum-checked, into `tools/godot/` |
| `./x play` | Build if needed and start the game full screen. `--windowed`, `--resolution WxH` |
| `./x export` | Export the player build into `dist/penguin-pass-linux-x86_64/` |
| `./x package` | Make a versioned `.tar.gz` of the export and print its SHA-256 |
| `./x licenses` | Write `docs/third-party-licenses.md` from the crates the game ships |

Narrow `test`, `fmt`, `clippy`, `lint`, and `typecheck` to one crate
with `CRATE=`, for example `CRATE=pp-core ./x test`.
`PROFILE=release ./x build` makes the release library.

`PP_NATIVE=1` runs cargo on the host instead of in the build container.
`GODOT=/path/to/godot ./x smoke` uses that binary instead of the Godot
flatpak. CI sets both.

## Checks

After `git init`, run `lefthook install`. `pre-commit` runs `./x fmt`
and `./x clippy` in parallel when Rust or Cargo files are staged.
`pre-push` runs `./x typecheck`, then `./x test`. GitHub Actions runs
`./x lint`, `./x typecheck`, `./x test`, and `./x smoke` on
`ubuntu-latest`. Details are in `docs/dev-workflow.md`.

## Riding

1. Wake the trainer (plug it in or pedal).
2. Close any other app paired to it. A trainer accepts one Bluetooth
   controller at a time.
3. `./x play`, pick the trainer, ride.

Without a trainer, the game starts with the simulated rider. Hold `Up`
to push more watts and `Down` to ease off. `docs/play.md` covers the
desktop entry and adding the game to Steam as a non-Steam game.

## TV

Three tiers, in the order they arrive:

1. **Steam Link / Remote Play.** Works for any Steam game, including a
   non-Steam shortcut. Install Steam Link on the TV and stream.
2. **TV dashboard.** The game serves a web page on the LAN with live
   power, speed, cadence, grade, and course position. Open it from the
   TV's browser.
3. **Chromecast video.** Cast the game view itself. Not built yet.

## Roadmap

- [x] M0: workspace, build container, Godot loads the Rust extension
- [ ] M1: FTMS codec, course data, bike physics, all with tests
- [ ] M2: BLE trainer connection, simulated trainer
- [ ] M3: 5 km mountain loop, HUD, ride session
- [ ] M4: TV dashboard
- [ ] M4: settings menu (rider and bike weight, FTP, trainer difficulty,
  trainer pairing, TV dashboard on or off), saved between sessions
- [ ] M5: Steam export presets and release checklist
- [ ] M5: Flatpak bundle built by GitHub Actions and attached to GitHub
  Releases. A Flathub listing is a separate, later step with its own rules
- [ ] M6: free CC0 art assets, textured terrain and road, a real rider
- [ ] M7: themes as data (alpine, desert) and a course picker
- [ ] M8: generated routes from a seed and a chosen distance, with the
  world streamed in chunks; then an endless mode
- [ ] Later: Chromecast video, ANT+, heart-rate straps

## Steam

Publishing needs a Steamworks partner account and an app ID. Those are
owner tasks. See `docs/steam-release.md`.

## License

Not chosen yet.
