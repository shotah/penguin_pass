# Steam release checklist

Penguin Pass is a native Linux game. The player build is
`dist/penguin-pass-linux-x86_64/`, made by `./x export` and packed by
`./x package`. Nothing in this repo uploads to Steam or stores a password.

Pages below were opened while writing this list. A line marked unverified
was not checked against a Valve or Godot page in that pass.

## 1. Steamworks account and the app fee

1. Sign up at [Steamworks](https://partner.steamgames.com/steamdirect).
   The paperwork asks for bank and tax details and identity verification.
   Valve says that review takes 15 to 30 days.
2. Pay the Steam Direct fee: **$100 USD** (or the local equivalent) for each
   app you want to sell. Valve's [fee page](https://partner.steamgames.com/doc/gettingstarted/appfee)
   says the fee is not refundable, and that it is paid back in the payment
   after the product has at least **$1,000** adjusted gross revenue from
   Steam store sales or in-app purchases. Steam Wallet cannot pay the fee.
   VAT or GST may be added. The invoice is issued to the personal Steam
   account that paid.
3. Two waits, from the [Steam Direct signup page](https://partner.steamgames.com/steamdirect):
   21 days after the fee before the first release, and a public Coming Soon
   page up for at least two weeks before release.

## 2. App ID and depots

Follow [Uploading to Steam](https://partner.steamgames.com/doc/sdk/uploading)
and [Platforms](https://partner.steamgames.com/doc/store/application/platforms).

1. On the app's Steamworks home, copy the **app ID**.
2. Open **SteamPipe → Depots**. Name the depot (for example "Linux content").
   Set the OS to Linux if this depot is Linux-only. Save, then publish those
   changes.
3. Add the depot to the Developer Comp package so your own accounts can
   install it. The page for that is Associated Packages & DLC.
4. Do not tick the public "Supported Operating Systems" Linux box until you
   are ready to release. Valve's platforms page says to leave that until
   release, then set it, set the store page system requirements, and move
   the build to the Default branch.

`tools/steam/app_build.vdf` and `tools/steam/depot_build_linux.vdf` are the
upload scripts. Replace every `YOUR_APP_ID` and `YOUR_LINUX_DEPOT_ID`.
`tools/steam/build.sh` only prints the steamcmd line. It does not log in.

```text
steamcmd +login YOUR_BUILD_ACCOUNT +run_app_build /absolute/path/tools/steam/app_build.vdf +quit
```

Use a dedicated build account with "Edit App Metadata" and "Publish App
Changes To Steam", as the uploading page describes. Set `Preview` to `1`
in `app_build.vdf` the first time. Valve says a preview build writes a local
manifest and does not upload. The content root is the folder from
`./x export`. The executable in the depot is `penguin-pass.x86_64`.

## 3. Which Steam Linux Runtime

The library is compiled in the Steam Runtime 4 SDK image named in
`tools/Containerfile`. Valve's [Steam Runtime README](https://github.com/ValveSoftware/steam-runtime/blob/master/README.md)
says new native Linux games should be built in that SDK, then selected as
**Steam Linux Runtime 4.0** in Steamworks under **Installation → Linux
Runtime**. The same page says the default for a newly published native Linux
game is still the old scout runtime, and that Steam Linux Runtime 4.0 is
installed with `steam steam://install/4183110` (app ID 4183110).

After `PROFILE=release ./x build` on this machine, `objdump -T` on
`godot/bin/libpp_godot.release.so` shows **GLIBC_2.35** as the newest glibc
symbol. This machine's glibc is 2.43 (`ldd --version`). A player whose
system glibc is older than 2.35 cannot load the `.so` unless Steam runs it
inside a runtime that has a new enough glibc.

Valve's steamrt4 platform source list
([source-required list](https://repo.steampowered.com/steamrt4/images/latest-public-beta/com.valvesoftware.SteamRuntime.Platform-amd64%2Ci386-steamrt4.source-required.txt))
includes glibc 2.41, which is newer than GLIBC_2.35. That is the runtime to
select. Leaving the default scout runtime (the README places Steam Runtime 1
under `ubuntu12_32`) will not provide GLIBC_2.35.

What `readelf -d` reports for the release library just built, and for the
Godot template from the earlier export:

| File | NEEDED |
| --- | --- |
| `libpp_godot.release.so` | `libdbus-1.so.3`, `libgcc_s.so.1`, `libm.so.6`, `libc.so.6`, `ld-linux-x86-64.so.2` |
| `penguin-pass.x86_64` (Godot 4.7.2 release template) | `librt.so.1`, `libpthread.so.0`, `libdl.so.2`, `libm.so.6`, `libc.so.6`, `ld-linux-x86-64.so.2` |

`ldd` on this SteamOS resolves every direct dependency of the `.so`,
including `libdbus-1.so.3` to `/usr/lib/libdbus-1.so.3`. That copy of
libdbus also needs `libsystemd.so.0` (`readelf -d` on the system library).
The same steamrt4 source list includes `dbus` 1.16.2 (which ships
`libdbus-1.so.3`), `systemd` 257 (which ships `libsystemd.so.0`), `glibc`
2.41, and `gcc-14` (which ships `libgcc_s.so.1`). The runtime container
itself was not booted here.

The Godot template's list is short because the official binary loads
Wayland, X11, and Vulkan with `dlopen`. An earlier exported run printed a
Vulkan line for the Radeon 890M, so those libraries were found on this
machine. That run was before this library linked D-Bus.

## 4. Launch options

In Steamworks, add one Linux launch option. Valve's uploading page says the
executable path is relative to the depot root, with no leading slash.

| Field | Value |
| --- | --- |
| Executable | `penguin-pass.x86_64` |
| Arguments | `--fullscreen` |
| Working directory | empty |

`--fullscreen` is accepted by the release export template (`--help` marks
it available on release templates). The project file still opens a 1280x800
window, so without that argument a double-clicked binary is windowed.
Setting full screen in `project.godot` is described at the end of this file.

Do not point the store launch option at `tools/penguin-pass.sh` or at the
flatpak. Those are the desktop proof of concept.

## 5. Steam Deck and Steam Machine review

Valve's checklist is [Steam Deck and Steam Machine Compatibility Review](https://partner.steamgames.com/doc/steamdeck/compat).
A native Linux build is what they test when one exists. Proton is the
fallback if the native build fails. Items that apply here:

- **Controller.** The ride is playable with Deck buttons: D-pad and the
  triggers change power, Y switches camera, Start pauses and a second Start
  quits. Those bindings are installed in code (`crates/pp-godot/src/input_map.rs`).
  The game does not call the Steam Input API. The review page says glyphs
  must match the controller in use, and recommends the Steam Input API for
  that. There is no on-screen button prompt in the current HUD. A later
  prompt that draws keyboard letters would fail this check.
- **Text input.** The review requires an on-screen keyboard, via Steamworks
  or a built-in one, if the game asks the player to type. The settings menu
  is not built, so there is no text field yet. Adding a rider name or an FTP
  field later has to meet this.
- **Default controller config.** Players must not change an in-game setting
  to enable the gamepad. The bindings are installed at startup, which matches
  that rule. It was not submitted for review.
- **Resolution and text.** The project is 1280x800, which the review page
  lists as the preferred Deck resolution. The smallest text must be at least
  9 pixels high at 1280x800, and they recommend 12. That height was not
  measured here.
- **Frame rate.** The review wants a default that holds 30 fps at 800p on
  Deck and 1080p on Steam Machine. `docs/game.md` records about 7 ms frames
  on this machine's desktop GPU. Deck and Machine were not tested.
- **Launcher.** The review says a launcher must itself be navigable with a
  controller, and recommends not having one. The store build should start
  `penguin-pass.x86_64` directly.
- **No "unsupported hardware" dialog.** The game does not show one.

Request the review from the app's Technical Tools page after a normal build
review has passed. The compat page says that order is required.

## 6. Bluetooth inside the runtime

Reaching BlueZ on the system D-Bus from inside Steam Linux Runtime 4.0 was
**not tested**. It is the main technical risk for a trainer. The game
library does not talk to BlueZ yet. When it does, it will use the system
bus name `org.bluez`, the same way `./x scan` does on the host. The runtime
is a container. No Valve page opened for this list says whether that
container can see the host's `org.bluez`.

Test, without taking control of a trainer:

1. Install the runtime: `steam steam://install/4183110`.
2. Find the tool's `run` script in the Steam library folder for that app.
   The directory name on disk was not checked here.
3. Run a read-only call:

```text
<path-to-SteamLinuxRuntime-4>/run -- dbus-send --system --dest=org.bluez \
  --print-reply / org.freedesktop.DBus.Introspectable.Introspect
```

A useful result is XML that mentions `org.bluez`. A failure is a
"name not found" or a disconnected bus. Do not call `Connect`, and do not
set a grade. After that, the same check from the exported game is: start it
under the runtime, leave the simulated rider on, and see whether a future
scan can list adapters. Listing is enough. Setting resistance is a separate
test, done only when you mean to ride.

## 7. Store assets

Sizes are from [Graphical Assets](https://partner.steamgames.com/doc/store/assets)
(the page says the larger capsules have been required since August 2024).

Required:

| Asset | Size |
| --- | --- |
| Header capsule | 920 x 430 |
| Small capsule | 462 x 174 |
| Main capsule | 1232 x 706 |
| Vertical capsule | 748 x 896 |
| Screenshots | at least 1920 x 1080, 16:9 |
| Shortcut icon | 256 x 256, png or ico |
| App icon | 184 x 184 jpg |
| Library capsule | 600 x 900 |
| Library hero | 3840 x 1240 png |
| Library logo | 1280 wide and/or 720 tall, png |
| Library header | 920 x 430 |

`docs/img/shots/` already has 1280x800 pictures and one 1920x1080 TV shot.
The store screenshots need to be at least 1920x1080. A trailer is not in
that required table. Trailer file rules were not opened, so they are
unverified.

## 8. Licences and credits

The game's own licence is still "Not chosen yet" (`README.md`). Pick one
before the store page goes up. Steam's subscriber rules were not re-read
for this list; the store will not accept a build you cannot say you have
the right to sell.

**Art.** Every file under `godot/assets/` is CC0 1.0, as recorded in
`docs/assets.md` and in each pack's `LICENSE.txt`. The Quaternius file
points at <https://creativecommons.org/publicdomain/zero/1.0/>. CC0 does
not require credit. The credits table in `docs/assets.md` is still the
text to put on the store page. `./x export` copies `docs/assets.md` to
`licenses/assets.md` and copies each pack's `LICENSE.txt` and `SOURCE.md`
to `licenses/assets/...`.

**Godot.** Godot is MIT. The engine's [licence compliance page](https://docs.godotengine.org/en/stable/about/complying_with_licenses.html)
says the MIT notice has to ship with the game, and that third-party code
inside Godot is listed in `COPYRIGHT.txt`, which should be included and
renamed so it is not confused with the game's own licence. `./x export`
writes `licenses/GODOT-LICENSE.txt` and `licenses/GODOT-COPYRIGHT.txt`,
taken from the `4.7.2-stable` tag. Those two text files are not in
Godot's `SHA512-SUMS.txt`. Their SHA-256 is in
`tools/godot/4.7.2/license-sha256.txt` after `./x fetch`. The export
templates and the editor zip are checked against `SHA512-SUMS.txt`.

**Rust crates.** Do not type licence terms from memory. From the toolbox
image, print the `license` field of every crate the workspace builds:

```bash
podman run --rm \
  -v "$PWD:/src" \
  -v penguin-pass-cargo:/usr/local/cargo/registry \
  -w /src \
  -e CARGO_HOME=/usr/local/cargo \
  penguin-pass-toolbox \
  cargo tree --workspace --prefix none -f '{p} {l}'
```

That field is the SPDX string from each crate's `Cargo.toml`. The text you
must ship is the `LICENSE` file in that crate (cargo registry, or the
crate's repository), not a paraphrase. `pp-godot` is what the game library
links today. `pp-trainer` brings `btleplug` and `dbus` when the trainer is
wired into the scene. Re-run the command after that change and copy any
new notices into `licenses/`.

This session did not finish that `cargo tree` run. The builds used the
toolbox image, and a later `podman image exists penguin-pass-toolbox`
failed, so the list was not captured. Run the command above rather than
pasting a guessed table into the store page.

## 9. Still unverified

- Game Mode, Steam Link, and Remote Play.
- Steam Linux Runtime 4.0 actually launching this binary, and BlueZ from
  inside it (section 6).
- The release export's shutdown. It prints `PENGUIN_PASS_READY_MS`, then
  glibc reports `double free or corruption` and the process dies with
  signal ABRT (exit 134). A debug export of the same project exited 0.
  Details are in the report that added this file. A store build that
  aborts on quit will fail review.
- Deck text height, Deck frame rate, and any future text entry.
- Trailer encoding rules.
- The exact on-disk folder name of Steam Linux Runtime 4.0.
- A full third-party Rust licence list (the command is above).
- Whether this Steam Deck's host glibc, rather than the runtime container,
  is what a store install will use. On this machine the host glibc is 2.43
  and the binary ran without the container.

## Changes in files this packaging does not own

`godot/project.godot`, under `[display]`, so a double-clicked export starts
full screen without a launch argument. In the editor that is
Display → Window → Size → Mode → Fullscreen. The integer written for that
mode was not checked against a Godot class page in this session, so set it
from the editor rather than by typing a number. Until it lands, the Steam
launch option `--fullscreen` covers the store build, and `./x play` passes
`--fullscreen` itself.

`README.md` command table, four rows: `./x fetch`, `./x play`, `./x export`,
`./x package`.

The Godot crate, one log line after assets load, so an export log can prove
the counts instead of only the absence of errors:

```text
PENGUIN_PASS_ASSETS models=<n> textures=<n> sky=<0 or 1>
```

And the release-only shutdown abort above. A debug export and `./x play`
both exit 0, so the failure is specific to the release library or to
godot-rust's release shutdown in the export template.

No CI change is required for a player to launch the game. A release job
would download the 1.2 GB export-template archive and run `./x export`.
That was not added.
