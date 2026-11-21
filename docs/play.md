# Playing Penguin Pass

This is how to start the game on this Steam Deck, from the desktop and as a
non-Steam game. The store build is a later step, described in
`docs/steam-release.md`.

The launcher uses the official Godot 4.7.2 editor binary, downloaded into
`tools/godot/` and checked against Godot's published `SHA512-SUMS.txt`. It is
not the flatpak. The flatpak is sandboxed: every launch needs
`--system-talk-name=org.bluez`, it has a private `/tmp`, and a headless
import of this project has already aborted. The official binary is a normal
program, so the system D-Bus (and therefore BlueZ) is reachable without a
flatpak permission, and the same download is what `./x export` uses. The
copy under `tools/godot/` survives a reboot. `/tmp/godot-4.7.2` does not.

`./x fetch` downloads and checks that binary and the export templates.
`./x play` downloads the editor binary if it is missing, builds the debug
library if it is missing or older than the Rust sources, and starts the game.

## First launch

From the repo root:

```bash
./x fetch
./x play
```

`./x fetch` is once, until Godot is upgraded. `./x play` is the game.
The first play builds the library inside the Steam Runtime toolbox, which
takes longer than later plays. The command passes Godot's `--fullscreen`.
On a normal start the game loads your settings, starts the LAN TV page when
that is enabled, and auto-connects to a remembered trainer if there is one.
With no trainer it uses the simulated rider. Flags for a run that must not
touch Bluetooth are below, and the full list is in `docs/game.md`.

A window that does not cover the screen:

```bash
./x play --windowed --resolution 1280x800
```

`--quit-after 90` quits after that many frames. That is a Godot flag, useful
for a short check. Leave it off when you want to ride.

`./x play` can be started from any directory. It finds the repo from its own
path.

Anything after a literal `--` is passed to the game:

```bash
./x play --windowed -- --no-bluetooth --settings /tmp/penguin-pass-settings.toml --dashboard-port 0
```

| Flag | Effect |
| --- | --- |
| `--settings <path>` | Settings file, instead of the one under `user://` |
| `--fake-trainer` | Two fake trainers, no radio |
| `--no-bluetooth` | Do not use Bluetooth |
| `--dashboard-port <n>` | TV page port. `0` picks a free port |
| `--dashboard-loopback` | TV page listens on 127.0.0.1 only |

`--scratch-data` (or `PENGUIN_PASS_SCRATCH_DATA=1`) points Godot's XDG
directories at `dist/xdg/` so a check does not read or write your settings.
A normal `./x play` does not set those variables. Settings then live in the
usual place for the official Godot binary on this machine:

`/home/deck/.local/share/godot/app_userdata/Penguin Pass/settings.toml`

That is not the Flatpak path in `docs/game.md`. This launcher does not use
the Flatpak.

## Desktop entry

`tools/penguin-pass.desktop` is not installed. It points at
`tools/penguin-pass.sh` and uses `godot/icon.svg`. The script does not build
and does not need a terminal. Run `./x play` once so the library exists,
then either:

- In the file manager, open `tools/penguin-pass.desktop` (KDE may ask you to
  trust it the first time), or
- Copy that file to `~/.local/share/applications/` yourself if you want it in
  the application menu.

If the repo moves, edit the `Exec` and `Icon` lines. They are absolute paths
for this checkout. The script itself finds the repo from its own location.

The script returns the game's exit code. Settings and the log use your
normal directories, described below. `--scratch-data` is the switch that
keeps a test run inside `dist/xdg/`.

## Steam, as a non-Steam game

This puts the game in the library so Game Mode can see it and Steam Link or
Remote Play can send it to a TV. These clicks were not run in Game Mode here.
They are the desktop-client steps.

1. In Desktop Mode, open Steam.
2. Menu **Games**, then **Add a Non-Steam Game to My Library**.
3. **Browse**, and choose `/home/deck/repos/training_game/tools/penguin-pass.sh`.
4. Name it Penguin Pass. In the shortcut's properties you can set the icon
   to `/home/deck/repos/training_game/godot/icon.svg`.
5. Switch to Game Mode. The shortcut is in your library with the other games.
6. On the TV, install Steam Link (or use Remote Play) and start Penguin Pass
   from the library. Steam streams a non-Steam shortcut the same way it
   streams a store game.

The shortcut runs the last build. It does not compile. After you change the
game, run `./x play` or `./x build` on the desktop before you go back to
Game Mode.

Game Mode, Steam Link, and Remote Play were not tested from this session.
What was tested is the desktop launch, written up at the bottom.

## Controls

The simulated rider is the keyboard, or the Steam Deck gamepad. Hold the
power keys and the 10 W step repeats. Esc or Start opens the menu (rider
and bike weight, FTP, trainer difficulty, the TV page, full screen, restart,
quit). The bindings and the menu are written up in `docs/game.md`.

| Action | Keyboard | Steam Deck |
| --- | --- | --- |
| More power | Up, W | D-pad up, right trigger |
| Less power | Down, S | D-pad down, left trigger |
| Chase camera or handlebars | C | Y |
| Full screen | F11 | |
| Menu | Esc | Start |

Power stays between 0 and 1500 W. Closing the window also leaves the game.

## Where settings and the log are

A normal launch does not redirect `XDG_DATA_HOME`, `XDG_CONFIG_HOME`,
`XDG_CACHE_HOME`, or `XDG_STATE_HOME`. Godot then stores `user://` for the
official binary at:

`/home/deck/.local/share/godot/app_userdata/Penguin Pass/`

The settings file there is `settings.toml`, unless you pass `--settings`.

The log is under the XDG state directory, not under `dist/`. `dist/` is
build output and is wiped without warning, and a player's log should
outlive that. On this machine, with `XDG_STATE_HOME` unset, the file is:

`/home/deck/.local/state/penguin-pass/play.log`

Each launch appends a header with the working directory and the Godot
arguments, then the game's output, then an exit line. A line
`PENGUIN_PASS_READY_MS` followed by a number means the scene finished
starting. That number is milliseconds.

`--scratch-data` sets the four XDG variables to `dist/xdg/{data,config,cache,state}`.
The log for that run is `dist/xdg/state/penguin-pass/play.log`.
`tools/penguin-pass.sh --print-paths` prints the paths it would use and
does not start the game.

## What is not working yet

- The store package is `./x export`, not this launcher. See
  `docs/steam-release.md`. The release export starts, then aborts during
  shutdown (exit 134). `./x play` exits 0.
- Pairing a trainer from inside Steam's Linux runtime container has not
  been tested. `./x play` does not connect to a trainer.
- Game Mode and streaming to a TV were not tested here.
- There is no course picker, and no Chromecast video. The settings menu is in the game; see `docs/game.md`.
- The project window mode is still 1280x800 windowed. `./x play` asks
  Godot for full screen on the command line. A double-clicked export does
  not, until `project.godot` sets the window mode or the Steam launch
  options pass `--fullscreen`.

## What was run on this machine

`./x smoke` printed `PENGUIN_PASS_SMOKE_OK 0.1.0` and exited 0.

`./x play --windowed --resolution 1280x800 --quit-after 90` was run twice
from the repo, then once as `/home/deck/repos/training_game/x play` from
`/tmp`. Each printed `PENGUIN_PASS_READY_MS` and exited 0. The launcher
script was also started from `/tmp`; its log line recorded `cwd=/tmp` and
it exited 0. A full-screen session was not left up. The default `./x play`
passes Godot's `--fullscreen` and was not the run used for those checks.
