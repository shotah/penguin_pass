# Penguin Pass command runner.
#
# SteamOS does not ship GNU make. ./x is the implementation; this file
# mirrors those targets for hosts that have make.
#
#   toolbox     Build the Steam Runtime toolbox image
#   build       Build the workspace and copy the GDExtension into godot/bin/
#   test        cargo test (CRATE=name for one crate)
#   fmt         cargo fmt --check (CRATE= optional)
#   clippy      cargo clippy -D warnings (CRATE= optional)
#   lint        fmt and clippy (CRATE= optional)
#   typecheck   cargo check (CRATE= optional)
#   smoke       Headless Godot load of the Rust extension
#   run         Launch Godot with per-run Bluetooth D-Bus access
#   scan        List nearby FTMS trainers via pp-scan on the host
#
# PROFILE=release selects a release build. The default is debug.
# PP_NATIVE=1 runs cargo on the host. GODOT=/path/to/godot selects the
# smoke binary. The default smoke launch is the Godot flatpak.

PROFILE ?= debug

.PHONY: toolbox build test fmt clippy lint typecheck smoke run scan

toolbox build test fmt clippy lint typecheck smoke run scan:
	PROFILE="$(PROFILE)" CRATE="$(CRATE)" ./x $@
