# Windows and macOS

Penguin Pass is still developed on Linux. This note is what the tree
claims for the other two desktops, and what has been verified so far.

## What is in the tree

`godot/penguin_pass.gdextension` lists the Windows and macOS libraries
next to the Linux ones. The names match Cargo's cdylib: `pp_godot` is
`pp_godot.dll` on Windows and `libpp_godot.dylib` on macOS. The macOS
lines have no architecture suffix, which is the GDExtension form for
one library used on every Mac arch. The CI build is a universal binary
(`lipo` of arm64 and x86_64 on `macos-latest`).

`godot/project.godot` sets
`textures/vram_compression/import_etc2_astc=true` under `[rendering]`.
Godot 4.7.2 requires that import flag for universal and arm64 macOS
exports.

`godot/export_presets.cfg` keeps the Linux preset and adds Windows
Desktop x86_64 and macOS. On the macOS preset,
`codesign/codesign=1` turns on Godot's built-in ad-hoc codesigning (works
from Linux and macOS), and
`codesign/entitlements/disable_library_validation=true` replaces the
older `entitlements/additional` key. Local export from Linux verified
that `com.apple.security.cs.disable-library-validation` is embedded in
the Mach-O signature blob and that
`Contents/_CodeSignature/CodeResources` is written.

`.github/workflows/macos.yml` and `.github/workflows/windows.yml` each
run Rust tests, clippy, and a release library build on `macos-latest`
and `windows-latest`. They do not wait on each other. A `v*` tag or a
manual run exports that desktop's player zip. The export job runs on
generic Linux (`ubuntu-26.04`) via the shared
`.github/actions/godot-export` composite action and
`tools/export-player.sh` (the same path as `.github/workflows/flatpak.yml`).
All three workflows share one cached Linux Godot 4.7.2 editor and export
templates archive (`godot-4.7.2-stable-linux-x86_64-editor-and-templates`).
A tag attaches that zip to the GitHub Release. Neither cross-desktop
workflow updates the Flatpak `latest` prerelease.

## Export keys checked in the 4.7.2 editor binary

The Linux editor `tools/godot/4.7.2/Godot_v4.7.2-stable_linux.x86_64`
contains these strings, so they are real preset keys:

- `platform` values `Windows Desktop` and `macOS`
- `binary_format/architecture`, and the macOS choices `universal,x86_64,arm64`
- `texture_format/s3tc_bptc` and `texture_format/etc2_astc`. A universal
  Mac export requires both. The binary says so.
- `application/additional_plist_content`
- `export/distribution_type` with the enum text `Testing,Distribution`
- `codesign/codesign` and `codesign/entitlements/disable_library_validation`
- The sentence that an ad-hoc signed app needs the Disable Library
  Validation entitlement to load a dynamic library

There is no `privacy/bluetooth_always_usage_description` and no
`NSBluetoothAlwaysUsageDescription` key in that binary. Bluetooth is
written into `application/additional_plist_content` instead:

`NSBluetoothAlwaysUsageDescription` and
`NSBluetoothPeripheralUsageDescription`, both set to "Penguin Pass
connects to smart bike trainers, heart-rate straps, and cycling
controllers over Bluetooth."

`export/distribution_type=0` is the first enum item, `Testing`. That is
the ad-hoc path. It is not notarized.

The macOS template inside Godot's 4.7.2 export archive is
`templates/macos.zip`, and the binary inside it is named
`godot_macos_release.universal`. The Windows templates are
`templates/windows_release_x86_64.exe` and the debug pair.

## macOS bundle layout (export output)

Verified layout under `dist/penguin-pass-macos/`:

- Binary: `penguin-pass.app/Contents/MacOS/Penguin Pass`
- PCK: `penguin-pass.app/Contents/Resources/Penguin Pass.pck`
- GDExtension: `penguin-pass.app/Contents/Frameworks/libpp_godot.release.dylib`
- `licenses/` sits next to `penguin-pass.app` in that folder, outside the
  signed `.app` bundle.

## Steam

`tools/steam/app_build.vdf` points at three depots. Placeholders:
`YOUR_LINUX_DEPOT_ID`, `YOUR_WINDOWS_DEPOT_ID`, `YOUR_MACOS_DEPOT_ID`.
Steam Cloud paths, and the rule to leave `strava.toml` out, are in
`docs/steam-release.md`.

Windows launch option: executable `penguin-pass.exe`, arguments
`--fullscreen`. macOS launch option: `penguin-pass.app`, arguments
`--fullscreen`. The main executable inside the bundle is
`Contents/MacOS/Penguin Pass`.

## Verified

- `crates/` has one Unix-only filesystem call, in `pp-strava`. It is
  behind `#[cfg(unix)]`. `pp-godot`, `pp-view`, and `pp-session` do
  not import `std::os::unix`.
- Windows build and run validated (native library and player export).
- macOS Rust tests, clippy, and universal `lipo` on `macos-latest` in CI.
- Linux, Windows, and macOS exports via `tools/export-player.sh` on Linux
  (including `pck_list.py` checks, macOS ad-hoc codesign, and macOS zip
  executable-bit verification in CI).
- Ad-hoc codesign on a Linux export: Disable Library Validation in the
  signature blob and `CodeResources` present.

## Not verified

- Running the exported `.app` on a real Mac (Bluetooth, gamepad,
  Gatekeeper, and ad-hoc launch).
- Bluetooth, gamepads, and the window have not been tried on Windows or
  macOS hardware.
- Steam Cloud roots for WinAppData and MacAppSupport were not checked
  against a Steam install on those systems.