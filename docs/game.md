# Penguin Pass game

The playable scene is a Godot 4.7 project whose only authored scene is
`godot/main.tscn`: one node, the Rust class `Game`. Everything else is
created in `Game.ready`.

## Node tree

```text
Game (Node3D)
├── World (Node3D)
│   ├── WorldEnvironment
│   ├── Sun (DirectionalLight3D)
│   ├── Terrain chunks (MeshInstance3D, one per 128 m cell)
│   ├── Distant peaks
│   ├── Road (MeshInstance3D)
│   ├── Scatter chunks (MultiMeshInstance3D, one mesh per chunk)
│   ├── Guard rails, reflector posts, kilometre posts
│   └── StartArch (Node3D)
├── Rider (Node3D)
├── CameraRig (Node3D)
│   └── Camera (Camera3D)
└── Hud (CanvasLayer)
    └── Margin
        └── Column
            ├── Top (PanelContainer)   power, speed, cadence, heart, grade, distance, lap, time, elevation
            ├── Spacer
            └── Bottom (PanelContainer)
                ├── Elevation
                └── StatusRow          trainer status, TV url
```

`Hud` and `ElevationStrip` are Rust `Control` classes. The strip draws the
lap profile from `course.profile`. The top band is about 15 % of a 1280x800
frame and the elevation strip about 10 %, both semi-transparent. Power stays
the largest number. The `hud_*_text` accessors are unchanged.

## Controls

The window opens at 1280x800. Stretch mode is `canvas_items` with aspect
`expand`, so the HUD scales up on a 1080p or 4K TV instead of sitting in a
corner. Vsync is on. The ride does not pause while the menu is open.

| Action | Keyboard | Controller |
| --- | --- | --- |
| Raise simulated power | Up, W | D-pad up, right trigger |
| Lower simulated power | Down, S | D-pad down, left trigger |
| Chase / handlebar camera | C | Y |
| Full screen | F11 | |
| Open or close the menu | Esc | Start |
| Move in the menu | Arrows | D-pad or left stick |
| Change a value | Left, Right | D-pad left / right |
| Select | Enter, Space | A |
| Back | Esc | B |

Hold a direction and the step repeats. Simulated power stays in 0..1500 W
and only changes while the simulated rider is the source. C switches between
the chase camera and a handlebar view.

The bottom line is the session status: the simulated rider and the Up/Down
hint, or the trainer's name and whether resistance follows the road. Until a
trainer has been paired, the first few seconds say how to open the menu.
Heart rate appears when the trainer sends it. The TV slot shows the dashboard
address when that page is running.

## Session

`Game` owns one `pp_session::Session`. Each frame still goes through the
three seams: read the keyboard sample, `Session::tick` (which reads the
active trainer, steps the ride, sends grade times difficulty, and publishes
the dashboard), then remember the snapshot. Up and Down call
`nudge_sim_power` while no trainer is the source.

Settings load from `user://settings.toml` unless `--settings <path>` is
passed. A warning from a bad file is shown on the status line for the first
10 seconds. `auto_connect` runs once at startup. Full screen follows the
saved setting.

| Flag | Effect |
| --- | --- |
| `--settings <path>` | Read and write this file instead of `user://settings.toml` |
| `--fake-trainer` | Discover two named fakes. No Bluetooth |
| `--no-bluetooth` | Scanning reports that Bluetooth is off. The ride continues |
| `--dashboard-port <n>` | Bind the TV page on this port. `0` picks a free port |
| `--dashboard-loopback` | Bind the TV page on 127.0.0.1 only |

Tests and `./godot/tests/shoot_set.sh` always pass `--fake-trainer` or
`--no-bluetooth`, a temporary settings file, and `--dashboard-port 0`.
`./x smoke` does not forward arguments, so `smoke.gd` sets the same switches
in the environment before the scene loads.

The menu is Esc / Start. It changes rider weight, bike weight, FTP, trainer
difficulty, whether resistance follows the road, the TV page, and full
screen. Changes apply immediately and are saved. Restart and Quit each ask
for a yes. A failed save shows a message and leaves the ride running.

## World

`pp_course::Course::penguin_pass` is the 5 km loop. At startup the game
samples the centerline about every 2 m, and every 1 m where the tangent
turns. That polyline becomes a 9 m ribbon: 6 m of mid-grey asphalt with a
dashed centre line and solid edge lines, gravel shoulders, and a grass
verge. The ribbon is flat across its width.

The earlier "airport runway" look was the chase camera: about 2 m high,
4.6 m back, nearly level, looking 40 m down a straight. At that height a
6 m black ribbon filled the horizontal field of view and had no verge to
read against. The chase camera is now lower and closer (about 1.5 m high,
3 m back) and the ribbon has shoulders.

Terrain is chunked on a 128 m grid. Cells within about 80 m of the road use
4 m quads; the rest use 16 m, and chunks far from both the origin and the
road are skipped. A painted 8 m distance grid keeps height lookups cheap.
Within about 5 m of the centreline the ground sits just under the road, then
blends out with a slope cap. Vertex colours come from the theme's ground
layers (meadow, forest floor, rock on steep faces, patchy then solid snow).
A world-space noise in the shader breaks the bands up. UVs are world-space
so a real albedo can replace the colour later. There is no lifted edge wall.
Three rings of ridged silhouettes stand just outside the playable terrain.
Each ring is a closed wall of flat-shaded facets with an irregular skyline,
snow above a wobbling snowline, and a lighter, bluer colour as it recedes.
The skirt starts below the outer terrain so the chunks cover the join. A
theme can ask for mesas instead of ridges. The section is optional; a file
without it uses the alpine defaults.

Scatter is one `MultiMeshInstance3D` per mesh per chunk. Each chunk is
generated from its coordinates, the theme seed, and the layer seed, so a
neighbour's contents do not change it and the order chunks are visited does
not matter. Grass and flowers stop drawing a few tens of metres out. Nothing
is placed inside 5 m of the centreline. Guard rails sit on the outside of
bends and where the ground drops beside the road. Reflector posts stand
every 24 m, with a taller post near each 500 m. The start/finish arch is
about 18 m down the road so the chase camera rides toward it and under it.

The look lives in `godot/themes/alpine.json`. A bad or missing file is logged
and the built-in copy of that same file is used. `Game` always loads alpine
for now.

When `res://assets/manifest.json` is present, scatter ids and ground texture
ids resolve through it. Models load with `GLTFDocument` and images with
`Image.load`, which is the method in `docs/assets.md`. The imported scene
graph is freed after the mesh is taken, so those nodes are not still alive
at shutdown. A missing library, a broken file, or an unknown id is logged
once and that kind falls back to the procedural mesh or the flat colour.
After the world is built the game prints
`PENGUIN_PASS_ASSETS models=<n> textures=<n> sky=<0 or 1>` for the files that
actually loaded. The rider stays procedural: the library has no road bicycle.
Guard rails, posts, and the arch stay procedural too.

The menu, HUD strings, theme checks, scatter rules, and backdrop are plain
Rust in `crates/pp-view`. `pp-godot` draws them. Simulated power steps by 25 W.
Holding Up or Down, after a short delay, adds about 100 W per second.

### Theme schema

```text
name, seed
sky:    top, horizon, ground_horizon, ground_bottom, curve, energy   (RGB 0..1)
sun:    pitch, yaw, energy, color, shadow_distance
fog:    density, aerial, color, energy
backdrop (optional):
  rings, inner_margin_m, ring_gap_m, base_drop_m,
  height_m[min,max], snow_fraction, segments, style ("ridges" or "mesas")
ground[]:
  name, color, texture, normal, altitude[min,max], slope[min,max], blend
scatter[]:
  kind, ids[], density_per_ha, altitude[min,max], max_slope,
  min_road_m, max_road_m, scale[min,max], random_yaw, tilt_to_slope,
  cluster (0 even .. 1 peaks only), cluster_frequency, seed, visibility_m
```

`texture`, `normal`, and `ids` are optional and empty until an asset library
is wired. `kind` selects the built-in placeholder (`tree`, `bush`, `grass`,
`rock`). `min_road_m` is clamped to 5 m. Placement is deterministic for a
given course, theme, and seed.

## Ride

`Game` owns a `pp_session::Session`. Each frame:

1. `read_trainer_sample` notes the keyboard target.
2. `integrate_ride` calls `Session::tick`, which reads the active trainer, steps the course, and sends the scaled grade.
3. `publish_telemetry` keeps the latest snapshot. The dashboard publish is inside the tick.
4. The rider node is placed at `position_at(lap_distance)` along `tangent_at`, and the HUD is refreshed.

Lap 0 means the bike has not moved yet. The first movement starts lap 1. Settings are saved when a menu value changes and when the menu closes. On this machine, with the Flatpak Godot, that file is
`/home/deck/.var/app/org.godotengine.Godot/data/godot/app_userdata/Penguin Pass/settings.toml`.

## Renderer and timing

The project uses the Mobile renderer (`Forward Mobile` in the Godot log).
On this machine (Radeon 890M), before this graphics pass, a 1280x800 run
measured scene build about 0.5–0.8 s and, over 240 frames, average 7.0 ms,
95th percentile 7.7 ms, max 8.5 ms.

After the procedural pass, `PENGUIN_PASS_READY_MS` was about 0.3–0.5 s and a
240-frame switchback sample averaged 6.95 ms (95th percentile 7.33 ms).
With the asset library loaded, world build is about 1.0–1.8 s. Over 240
frames at the switchback the average was 6.95 ms, the 95th percentile
7.70 ms, and the max 8.81 ms. Short samples at the other viewpoints,
including 1920x1080, stayed in that band. The script run does not sit on vsync.

## Screenshots

`godot/tests/shot.gd` loads the main scene, steps the ride to a distance,
waits, and writes a PNG. Godot's flatpak has its own `/tmp`, so pass a path
on the host home directory:

```text
flatpak run org.godotengine.Godot \
  --path /home/deck/repos/training_game/godot \
  --script res://tests/shot.gd -- \
  --out /home/deck/penguin-pass.png \
  --width 1280 --height 800 \
  --distance 340 --power 250 --camera chase
```

`--camera first` is the handlebar view. `--bench` prints
`PENGUIN_PASS_FRAME_MS` after 240 frames.

The standard set is one process and one world build. It writes stable names
under `docs/img/shots/` and overwrites the previous iteration. It does not
touch the earlier pictures in `docs/img/`.

```text
./godot/tests/shoot_set.sh
```

That writes `start-chase`, `start-fp`, `switchback-chase`, `switchback-fp`,
`climb-chase`, `climb-fp`, `summit-chase`, `summit-fp`, `descent-chase`,
`descent-fp` at 1280x800, and `tv-1080` at 1920x1080. Each line
`PENGUIN_PASS_VIEW` is that viewpoint's frame time.

## Rough edges

The rider is still a procedural cyclist: a strong jersey and a readable bike,
with box limbs and a ball helmet. The library has no road bicycle. The front
wheel in first person is only a sliver under the stem because the elevation
strip covers the bottom of the frame. Distant peaks are cones, not a range.
Ground textures are photographs next to painted trees, so the meadow is
quieter than the foliage. Snow is an altitude blend of the snow photo, lifted
toward white so it does not go brown, and it is not a climate. Guard rails
are boxes. On the steep descent the physics speed tops out at 80 km/h.
