# Windows and macOS

Penguin Pass is still developed on Linux. This note is what the tree
claims for the other two desktops, and what has not been run.

## What is in the tree

`godot/penguin_pass.gdextension` lists the Windows and macOS libraries
next to the Linux ones. The names match Cargo's cdylib: `pp_godot` is
`pp_godot.dll` on Windows and `libpp_godot.dylib` on macOS. The macOS
lines have no architecture suffix, which is the GDExtension form for
one library used on every Mac arch. The CI build is meant to be a
universal binary (`lipo` of arm64 and x86_64). That `lipo` has not run.

`godot/export_presets.cfg` keeps the Linux preset and adds Windows
Desktop x86_64 and macOS. The keys that were checked against the Godot
4.7.2 editor binary on this machine are listed below. No export was
run.

`.github/workflows/cross-platform.yml` runs tests, clippy, and a
release library build on `windows-latest` and `macos-latest`. A `v*`
tag or a manual run also exports a zip and, for a tag, attaches it to
that GitHub Release. It does not update the Flatpak `latest` prerelease.

## Export keys checked in the 4.7.2 editor binary

The Linux editor `tools/godot/4.7.2/Godot_v4.7.2-stable_linux.x86_64`
contains these strings, so they are real preset keys:

- `platform` values `Windows Desktop` and `macOS`
- `binary_format/architecture`, and the macOS choices `universal,x86_64,arm64`
- `texture_format/s3tc_bptc` and `texture_format/etc2_astc`. A universal
  Mac export requires both. The binary says so.
- `application/additional_plist_content`
- `export/distribution_type` with the enum text `Testing,Distribution`
- `entitlements/additional`
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
the ad-hoc path. It is not notarized. The integer was not confirmed by
running the exporter.

The macOS template inside Godot's 4.7.2 export archive is
`templates/macos.zip`, and the binary inside it is named
`godot_macos_release.universal`. The Windows templates are
`templates/windows_release_x86_64.exe` and the debug pair.

## Steam

`tools/steam/app_build.vdf` points at three depots. Placeholders:
`YOUR_LINUX_DEPOT_ID`, `YOUR_WINDOWS_DEPOT_ID`, `YOUR_MACOS_DEPOT_ID`.
Steam Cloud paths, and the rule to leave `strava.toml` out, are in
`docs/steam-release.md`.

Windows launch option, once a depot exists: executable
`penguin-pass.exe`, arguments `--fullscreen`. macOS launch option:
`penguin-pass.app`. The binary inside that app was not produced here,
so the exact `Contents/MacOS` name is not known yet.

## Verified on this machine

- `crates/` has one Unix-only filesystem call, in `pp-strava`. It is
  behind `#[cfg(unix)]`. `pp-godot`, `pp-view`, and `pp-session` do
  not import `std::os::unix`.
- `CRATE=pp-strava` test, lint, and typecheck were run here. The exit
  codes are in the report that added this file.
- `appstream` is unrelated. The Godot 4.7.2 binary strings above were
  read locally. The workflow YAML was parsed and checked with actionlint.

## Not verified

- A Windows or macOS compiler has not built this workspace.
- `cargo test`, clippy, and the `pp-godot` cdylib have not run on
  `windows-latest` or `macos-latest`.
- The x86_64 Mac cross-compile and `lipo` have not run.
- Godot has not exported the Windows or macOS preset. The ad-hoc
  signature, the Bluetooth plist, and the library-validation
  entitlement are untested on a Mac.
- Bluetooth, gamepads, and the window have not been tried on either
  system.
- Steam Cloud roots for WinAppData and MacAppSupport were not checked
  against a Steam install.
