# Penguin Pass command runner.
#
# SteamOS does not ship GNU make. ./x is the implementation; this file
# mirrors those targets for hosts that have make.
#
#   toolbox     Build the Steam Runtime toolbox image
#   build       Build the workspace and copy the GDExtension into godot/bin/
#   test        cargo test — full suite (CRATE=name for one crate)
#   test-unit   fast lib tests (--lib); pre-push hook
#   test-integration  integration tests (--test '*'); CI and manual
#   fmt         cargo fmt --check (CRATE= optional)
#   clippy      cargo clippy -D warnings (CRATE= optional)
#   lint        fmt and clippy (CRATE= optional)
#   typecheck   cargo check (CRATE= optional)
#   smoke       Headless Godot load of the Rust extension
#   run         Launch Godot with per-run Bluetooth D-Bus access
#   scan        List nearby FTMS trainers via pp-scan on the host
#   fetch       Download and checksum the Godot editor and export templates
#   play        Build the debug library if needed and launch the game
#   export      Export dist/penguin-pass-linux-x86_64
#   package     Tar the export and print its size and SHA-256
#   licenses    Write docs/third-party-licenses.md from cargo metadata
#   release     Bump, tag v<version>, and push. Default bump is patch.
#               BUMP=minor, BUMP=major, or BUMP=X.Y.Z selects another version.
#               DRY=1 is --dry-run. NOPUSH=1 commits and tags without pushing.
#
# PROFILE=release selects a release build. The default is debug.
# PP_NATIVE=1 runs cargo on the host. GODOT=/path/to/godot selects the
# smoke binary. The default smoke launch is the Godot flatpak.
# Window flags belong on ./x play, not on make play.

PROFILE ?= debug
BUMP ?= patch

.PHONY: toolbox build test test-unit test-integration fmt clippy lint typecheck smoke run scan fetch play export package licenses release

toolbox build test test-unit test-integration fmt clippy lint typecheck smoke run scan fetch play export package licenses:
	PROFILE="$(PROFILE)" CRATE="$(CRATE)" ./x $@

release:
	./x release $(if $(filter 1,$(DRY)),--dry-run,) $(if $(filter 1,$(NOPUSH)),--no-push,) "$(BUMP)"
