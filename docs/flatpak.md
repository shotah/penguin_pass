# Flatpak bundle

Penguin Pass can be packed as one file,
`penguin-pass-<version>-x86_64.flatpak`, and installed with
`flatpak install --user`. The file is built by GitHub Actions from the
same player directory as `./x export`. Rust is compiled on the runner,
not inside the Flatpak sandbox.

The workflow is `.github/workflows/flatpak.yml`. It runs when a tag
named `v*` is pushed, and when someone starts it by hand. It does not
run on a normal push to `main`, so it does not block development.

A manual run uploads the bundle as a workflow artifact. A tag run does
that and also attaches the file to the GitHub Release for the tag. If
the release does not exist yet, the workflow creates it.

## Runtime

The bundle uses `org.freedesktop.Platform` **26.08**. That runtime's
glibc is **2.44** (freedesktop-sdk 26.08.2 and later).

The library is linked on a GitHub-hosted `ubuntu-26.04` runner. Ubuntu
26.04 ships glibc **2.43**. A binary linked there needs at most
`GLIBC_2.43`, which 2.44 provides. The previous runtime, 25.08, has
glibc 2.42 and cannot load a library built on this runner.

The move of `ubuntu-latest` to Ubuntu 26.04 was scheduled to finish on
19 November 2026. The Flatpak jobs pin `ubuntu-26.04` anyway, so a
later move of `ubuntu-latest` cannot link the library against a glibc
newer than the runtime. The export job prints `ldd --version` and
stops if `penguin-pass.x86_64` or `libpp_godot.release.so` needs a
GLIBC symbol newer than 2.44.

The Godot 4.7.2 export template in a local export needs only
`GLIBC_2.28`. The tight requirement is the Rust library built on the
runner, which also needs `libdbus-1.so.3` and `libgcc_s.so.1`. Those
are in the runtime. The job log lists every `NEEDED` library.

The first install also downloads this runtime (a few hundred megabytes)
if it is not already installed. The bundle records the public
repository that publishes the runtime, so `flatpak install` can fetch
it. The workflow only downloads that runtime. It does not publish the
app anywhere.

## Run the workflow

The workflow file has to be on the commit that runs.

**By hand.** Push the commit to `main`. On GitHub, open
**Actions**, choose **Flatpak**, then **Run workflow**, then
**Run workflow** on `main`. When the run is green, open the run and
download the **flatpak-bundle** artifact. GitHub wraps artifacts in a
zip. Unzip it. Inside is `penguin-pass-<version>-x86_64.flatpak`.
The version is `version` in `Cargo.toml` (currently `0.1.0`).

**From a tag.** Push a tag such as `v0.1.0` on a commit that contains
this workflow. The version in the filename is the tag with the leading
`v` removed. The **Releases** page for that tag has the `.flatpak`
attached directly, not inside a zip. The release body includes the
SHA-256. A manual run does not create a release.

## Install and run

From the directory that contains the file:

```bash
flatpak install --user ./penguin-pass-0.1.0-x86_64.flatpak
flatpak run io.github.shotah.PenguinPass
```

Full screen, the same flag `./x play` passes:

```bash
flatpak run io.github.shotah.PenguinPass --fullscreen
```

Update a bundle you already installed by running `flatpak install
--user` on the new file. Remove it with
`flatpak uninstall --user io.github.shotah.PenguinPass`.

Settings are per app, under the Flatpak data directory, not in the
path `./x play` uses. On this machine that is:

`/home/deck/.var/app/io.github.shotah.PenguinPass/data/godot/app_userdata/Penguin Pass/`

A music folder set in the game is an absolute path. The sandbox can
read the XDG Music directory (`~/Music` when that is the music dir)
and nothing else in the home directory. Bundled tracks are inside the
game pack and need no extra permission. A folder outside Music is not
visible.

## Steam Deck, as a non-Steam game

Game Mode starts a program, not a Flatpak id. A one-line script is the
shortcut. These clicks were not run in Game Mode here. They are the
desktop-client steps, same as `docs/play.md`.

1. In Desktop Mode, save this as `~/.local/bin/penguin-pass-flatpak.sh`
   and mark it executable (`chmod +x` on that file):

```bash
#!/bin/sh
exec flatpak run io.github.shotah.PenguinPass --fullscreen "$@"
```

2. Open Steam. Menu **Games**, then **Add a Non-Steam Game to My Library**.
3. **Browse**, and choose `~/.local/bin/penguin-pass-flatpak.sh`.
4. Name it Penguin Pass. The icon after a user install is
   `~/.local/share/flatpak/exports/share/icons/hicolor/256x256/apps/io.github.shotah.PenguinPass.png`.
5. Switch to Game Mode. The shortcut is in the library.
6. On the TV, Steam Link or Remote Play streams it the same way as any
   other non-Steam shortcut.

## Permissions

Each line is one finish argument. This is the whole list.

| Argument | Why |
| --- | --- |
| `--device=dri` | GPU. Vulkan, and Godot's GL fallback. |
| `--device=input` | Game controllers. They show up as evdev devices under `/dev/input`. Flatpak 1.15.6 added this (stable in 1.16, January 2025). This Deck has Flatpak 1.18.4, and Ubuntu 26.04, which builds the bundle, ships Flatpak 1.16.6, so both sides understand it. `--device=all` would also expose cameras, raw USB, and every other device node. Controllers do not need that. |
| `--share=ipc` | X11 shared memory, for a session that is not Wayland. |
| `--share=network` | The game serves the TV dashboard as a web page on the LAN. |
| `--socket=wayland` | The window on a Wayland session. Desktop Mode on this Deck is Wayland. |
| `--socket=fallback-x11` | The window on X11, and only when Wayland is not available. Game Mode's gamescope may be that case. |
| `--socket=pulseaudio` | Audio. PipeWire on SteamOS provides this socket. |
| `--system-talk-name=org.bluez` | Talk to BlueZ on the system bus, to find and control a trainer. This is a filtered bus, not access to every system service. |
| `--filesystem=xdg-music:ro` | Read the rider's Music folder. Nothing else in the home directory is visible. |

There is no blanket home access, and no `--device=all`.

## Known risks

Bluetooth from inside the sandbox has not been tried. The game calls
`org.bluez` over D-Bus. The permission allows that name. BlueZ also
passes file descriptors for some radio operations, and a Flatpak bus
proxy does not always pass those through. A useful result is a scan
that lists the trainer. A failure is a scan that finds nothing, or a
connect error, while the simulated rider still runs. Widen nothing
until a log shows the proxy is what blocked it.

Nothing in this bundle has been installed on a machine. The first
green GitHub run is the first time the file exists. The release export
also still aborts on quit (exit 134), which is the same player binary
`./x export` produces. See `docs/steam-release.md`.

If the window never appears from Game Mode, the session may be X11
while Wayland is also set. `fallback-x11` is the permission for a
session without Wayland. That path was not tried.

## If the first run fails

Open the failed job and read the step named in the log.

1. **Install the freedesktop runtime.** `flatpak install` of
   `org.freedesktop.Platform//26.08` failed. Ubuntu 26.04's Flatpak is
   1.16.6. If the log says the runtime needs a newer Flatpak, that is
   this step.
2. **Export the player.** Godot's headless export failed. The
   `export-log` artifact is `dist/logs/export.log`. A missing library
   in that log is a package the runner does not have. The editor
   binary's direct dependencies are only libc.
3. **Validate the metainfo and the desktop file.** `appstreamcli` or
   `desktop-file-validate` printed an error. The message names the tag
   or the desktop key. A pedantic note about uppercase letters in the
   app id is not a failure. The id is `io.github.shotah.PenguinPass`
   on purpose.
4. **Build the bundle.** `flatpak-builder` or `install-player.sh`
   failed. `icons/64.png: not found` means the icon files were copied
   to the build root instead of `icons/`. `File not found` on
   `../../dist/penguin-pass-linux-x86_64` means the export directory
   was not where the manifest looks. That path is allowed when
   flatpak-builder is not run with `--sandbox`, which this workflow
   does not pass.
5. **Check glibc symbols fit the runtime.** A shipped binary needs a
   GLIBC symbol newer than 2.44. The step prints the symbol and the
   runner's `ldd --version`.
6. **Check the export layout.** The downloaded player directory does
   not contain `penguin-pass.x86_64`, the pck, or the `.so`. The step
   lists the tree.
7. **Attach the bundle to the GitHub Release.** `gh` could not create
   or upload the release. `Resource not accessible by integration`
   means the token was not allowed `contents: write`.

A bundle that installs and then will not start is a later failure.
`flatpak run io.github.shotah.PenguinPass` prints Godot's log. A line
`GLIBC_2.xx not found` or `libdbus-1.so.3: cannot open shared object`
means the runtime is older than the library the export job linked.
The export log lists the versions it checked.

## Before the first public release

- The game's licence is not chosen. The metainfo says
  `LicenseRef-PenguinPass-TODO` on `project_license`. Replace that with
  the real SPDX expression. `metadata_license` is already `CC0-1.0`.
- Add a screenshots URL. The metainfo has a comment showing the
  `<screenshots>` block. The image has to be an https PNG or JPEG.
- Tags are `v` plus the version, for example `v0.1.0`. The bundle
  filename uses the rest (`0.1.0`). A manual run uses `version` in
  `Cargo.toml` instead. Make those two numbers the same before you tag.

A Flathub listing is a separate later step the owner does by hand.
