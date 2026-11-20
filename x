#!/usr/bin/env bash
# Penguin Pass command runner. The Makefile mirrors these targets.
# Cargo runs in the Steam Runtime toolbox image.
# PP_NATIVE=1 runs cargo on the host instead (CI runners have a C toolchain).
# GODOT=/path/to/godot selects the binary for smoke. The default is the flatpak.

set -euo pipefail

ROOT=$(cd "$(dirname "$0")" && pwd)
cd "$ROOT"

IMAGE=${IMAGE:-penguin-pass-toolbox}
CARGO_VOLUME=penguin-pass-cargo
PROFILE=${PROFILE:-debug}
CRATE=${CRATE:-}

run_cargo() {
    if [[ "${PP_NATIVE:-}" == 1 ]]; then
        cargo "$@"
        return
    fi
    if ! podman image exists "$IMAGE"; then
        echo "error: toolbox image '$IMAGE' is not built. Run ./x toolbox" >&2
        exit 1
    fi
    podman run --rm \
        -v "$ROOT:/src" \
        -v "${CARGO_VOLUME}:/usr/local/cargo/registry" \
        -w /src \
        -e CARGO_HOME=/usr/local/cargo \
        -e CARGO_TERM_COLOR=always \
        "$IMAGE" \
        cargo "$@"
}

usage() {
    echo "usage: ./x {toolbox|build|test|fmt|clippy|lint|typecheck|smoke|run|scan}" >&2
    exit 1
}

cmd_toolbox() {
    podman build -t "$IMAGE" -f "$ROOT/tools/Containerfile" "$ROOT/tools"
}

cmd_build() {
    local target_dir dest
    case "$PROFILE" in
        debug)
            target_dir=target/debug
            dest=godot/bin/libpp_godot.debug.so
            run_cargo build --workspace
            ;;
        release)
            target_dir=target/release
            dest=godot/bin/libpp_godot.release.so
            run_cargo build --workspace --release
            ;;
        *)
            echo "PROFILE must be debug or release (got: $PROFILE)" >&2
            exit 1
            ;;
    esac
    mkdir -p godot/bin
    cp -f "$target_dir/libpp_godot.so" "$dest"
}

cmd_test() {
    if [[ -n "$CRATE" ]]; then
        run_cargo test -p "$CRATE"
    else
        run_cargo test --workspace
    fi
}

cmd_fmt() {
    if [[ -n "$CRATE" ]]; then
        run_cargo fmt -p "$CRATE" --check
    else
        run_cargo fmt --all --check
    fi
}

cmd_clippy() {
    if [[ -n "$CRATE" ]]; then
        run_cargo clippy -p "$CRATE" --all-targets -- -D warnings
    else
        run_cargo clippy --workspace --all-targets -- -D warnings
    fi
}

cmd_lint() {
    cmd_fmt
    cmd_clippy
}

cmd_typecheck() {
    if [[ -n "$CRATE" ]]; then
        run_cargo check -p "$CRATE" --all-targets
    else
        run_cargo check --workspace --all-targets
    fi
}

cmd_smoke() {
    local log code
    # Script mode loads only the extensions named in .godot/extension_list.cfg.
    # `godot --import` is what normally writes that file, but Godot 4.7.2's
    # flatpak aborts (exit 134) while loading editor docks after the scan.
    # The gdext book says to write this one-line list when the editor has
    # not generated it. Refresh it from the .gdextension files on each run.
    mkdir -p "$ROOT/godot/.godot"
    : >"$ROOT/godot/.godot/extension_list.cfg"
    local ext
    for ext in "$ROOT"/godot/*.gdextension; do
        [[ -e "$ext" ]] || continue
        printf 'res://%s\n' "$(basename "$ext")" >>"$ROOT/godot/.godot/extension_list.cfg"
    done
    local -a godot_cmd
    if [[ -n "${GODOT:-}" ]]; then
        godot_cmd=("$GODOT")
    else
        godot_cmd=(flatpak run org.godotengine.Godot)
    fi
    log=$(mktemp)
    set +e
    "${godot_cmd[@]}" --headless --path "$ROOT/godot" \
        --script res://tests/smoke.gd >"$log" 2>&1
    code=$?
    set -e
    cat "$log"
    if [[ "$code" -ne 0 ]]; then
        echo "smoke: Godot exited $code" >&2
        rm -f "$log"
        exit "$code"
    fi
    if ! grep -q 'PENGUIN_PASS_SMOKE_OK' "$log"; then
        echo "smoke: Rust extension did not report PENGUIN_PASS_SMOKE_OK" >&2
        rm -f "$log"
        exit 1
    fi
    rm -f "$log"
}

cmd_run() {
    flatpak run --system-talk-name=org.bluez org.godotengine.Godot --path godot
}

cmd_scan() {
    run_cargo build -p pp-trainer --bin pp-scan
    "$ROOT/target/debug/pp-scan" "$@"
}

# scan forwards its remaining arguments to pp-scan. Every other command
# still takes exactly one word.
if [[ "${1:-}" == "scan" ]]; then
    shift
    cmd_scan "$@"
    exit 0
fi

[[ $# -eq 1 ]] || usage

case "$1" in
    toolbox) cmd_toolbox ;;
    build) cmd_build ;;
    test) cmd_test ;;
    fmt) cmd_fmt ;;
    clippy) cmd_clippy ;;
    lint) cmd_lint ;;
    typecheck) cmd_typecheck ;;
    smoke) cmd_smoke ;;
    run) cmd_run ;;
    *) usage ;;
esac
