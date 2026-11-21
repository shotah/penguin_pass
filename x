#!/usr/bin/env bash
# Penguin Pass command runner. The Makefile mirrors these targets.
# Cargo runs in the Steam Runtime toolbox image.
# PP_NATIVE=1 runs cargo on the host instead (CI runners have a C toolchain).
# GODOT=/path/to/godot selects the binary for smoke. The default is the flatpak.
# ./x fetch downloads the official Godot editor and export templates.
# ./x play launches the game. ./x export and ./x package build the player folder.

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
    echo "usage: ./x {toolbox|build|test|fmt|clippy|lint|typecheck|smoke|run|scan|fetch|play|export|package|licenses}" >&2
    echo "play:  ./x play [--windowed|--fullscreen] [--resolution WxH] [--quit-after N] [--scratch-data] [--] [game args]" >&2
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
    # Copy beside the destination, then rename. Overwriting in place cuts
    # the library out from under a running game, which dies with SIGBUS.
    cp -f "$target_dir/libpp_godot.so" "$dest.new"
    mv -f "$dest.new" "$dest"
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

GODOT_VERSION=4.7.2
GODOT_TAG="${GODOT_VERSION}-stable"
GODOT_DIR="$ROOT/tools/godot/${GODOT_VERSION}"
GODOT_BASE_URL="https://github.com/godotengine/godot/releases/download/${GODOT_TAG}"
GODOT_EDITOR_ZIP="Godot_v${GODOT_TAG}_linux.x86_64.zip"
GODOT_TEMPLATES_TPZ="Godot_v${GODOT_TAG}_export_templates.tpz"
GODOT_BIN="$GODOT_DIR/Godot_v${GODOT_TAG}_linux.x86_64"

download_godot_sums() {
    mkdir -p "$GODOT_DIR"
    curl -fsSL -o "$GODOT_DIR/SHA512-SUMS.txt" "${GODOT_BASE_URL}/SHA512-SUMS.txt"
}

verify_godot_archive() {
    local name="$1"
    [[ -f "$GODOT_DIR/$name" ]] || return 1
    [[ -f "$GODOT_DIR/SHA512-SUMS.txt" ]] || return 1
    (
        cd "$GODOT_DIR"
        grep -E "[[:space:]]${name}\$" SHA512-SUMS.txt | sha512sum -c -
    )
}

ensure_godot_archive() {
    local name="$1"
    if verify_godot_archive "$name"; then
        return 0
    fi
    if [[ -f "/tmp/godot-${GODOT_VERSION}/$name" ]]; then
        cp -f "/tmp/godot-${GODOT_VERSION}/$name" "$GODOT_DIR/$name"
        if verify_godot_archive "$name"; then
            echo "fetch: reused verified /tmp/godot-${GODOT_VERSION}/$name"
            return 0
        fi
        rm -f "$GODOT_DIR/$name"
    fi
    echo "fetch: downloading $name"
    curl -fL --retry 3 --retry-delay 2 -o "$GODOT_DIR/$name" "${GODOT_BASE_URL}/$name"
    verify_godot_archive "$name"
}

ensure_godot_editor() {
    if [[ -x "$GODOT_BIN" ]]; then
        return 0
    fi
    download_godot_sums
    ensure_godot_archive "$GODOT_EDITOR_ZIP"
    unzip -qo "$GODOT_DIR/$GODOT_EDITOR_ZIP" -d "$GODOT_DIR"
    chmod +x "$GODOT_BIN"
}

ensure_godot_templates() {
    local release="$GODOT_DIR/templates/linux_release.x86_64"
    local debug="$GODOT_DIR/templates/linux_debug.x86_64"
    if [[ -x "$release" && -x "$debug" ]]; then
        return 0
    fi
    download_godot_sums
    ensure_godot_archive "$GODOT_TEMPLATES_TPZ"
    unzip -qo "$GODOT_DIR/$GODOT_TEMPLATES_TPZ" \
        "templates/linux_release.x86_64" \
        "templates/linux_debug.x86_64" \
        "templates/version.txt" \
        -d "$GODOT_DIR"
    chmod +x "$release" "$debug"
    grep -q "^${GODOT_VERSION}.stable" "$GODOT_DIR/templates/version.txt"
}

ensure_godot_licenses() {
    local lic="$GODOT_DIR/LICENSE.txt"
    local copy="$GODOT_DIR/COPYRIGHT.txt"
    # These two text files are not listed in SHA512-SUMS.txt. They come from
    # the same git tag as the release. sha256 is recorded beside them.
    if [[ ! -s "$lic" ]]; then
        curl -fsSL -o "$lic" \
            "https://raw.githubusercontent.com/godotengine/godot/${GODOT_TAG}/LICENSE.txt"
    fi
    if [[ ! -s "$copy" ]]; then
        curl -fsSL -o "$copy" \
            "https://raw.githubusercontent.com/godotengine/godot/${GODOT_TAG}/COPYRIGHT.txt"
    fi
    sha256sum "$lic" "$copy" >"$GODOT_DIR/license-sha256.txt"
}

library_is_stale() {
    local so="$1"
    local -a paths=("$ROOT/Cargo.toml" "$ROOT/crates")
    [[ -f "$so" ]] || return 0
    [[ -f "$ROOT/Cargo.lock" ]] && paths+=("$ROOT/Cargo.lock")
    [[ -f "$ROOT/rust-toolchain.toml" ]] && paths+=("$ROOT/rust-toolchain.toml")
    local newer
    newer=$(find "${paths[@]}" -newer "$so" \
        \( -name '*.rs' -o -name 'Cargo.toml' -o -name 'Cargo.lock' -o -name 'rust-toolchain.toml' \) \
        -print -quit)
    [[ -n "$newer" ]]
}

cmd_fetch() {
    download_godot_sums
    ensure_godot_archive "$GODOT_EDITOR_ZIP"
    ensure_godot_archive "$GODOT_TEMPLATES_TPZ"
    if [[ ! -x "$GODOT_BIN" || "$GODOT_DIR/$GODOT_EDITOR_ZIP" -nt "$GODOT_BIN" ]]; then
        unzip -qo "$GODOT_DIR/$GODOT_EDITOR_ZIP" -d "$GODOT_DIR"
        chmod +x "$GODOT_BIN"
    fi
    local release="$GODOT_DIR/templates/linux_release.x86_64"
    local debug="$GODOT_DIR/templates/linux_debug.x86_64"
    if [[ ! -x "$release" || ! -x "$debug" || "$GODOT_DIR/$GODOT_TEMPLATES_TPZ" -nt "$release" ]]; then
        unzip -qo "$GODOT_DIR/$GODOT_TEMPLATES_TPZ" \
            "templates/linux_release.x86_64" \
            "templates/linux_debug.x86_64" \
            "templates/version.txt" \
            -d "$GODOT_DIR"
        chmod +x "$release" "$debug"
    fi
    grep -q "^${GODOT_VERSION}.stable" "$GODOT_DIR/templates/version.txt"
    ensure_godot_licenses
    echo "fetch: editor $GODOT_BIN"
    echo "fetch: release template $release"
    echo "fetch: debug template $debug"
    echo "fetch: template version $(cat "$GODOT_DIR/templates/version.txt")"
}

cmd_play() {
    ensure_godot_editor
    if library_is_stale "$ROOT/godot/bin/libpp_godot.debug.so"; then
        echo "play: debug library missing or older than the Rust sources, building" >&2
        local saved_profile="$PROFILE"
        PROFILE=debug
        cmd_build
        PROFILE="$saved_profile"
    fi
    exec "$ROOT/tools/penguin-pass.sh" "$@"
}

cmd_export() {
    ensure_godot_editor
    ensure_godot_templates
    ensure_godot_licenses
    if library_is_stale "$ROOT/godot/bin/libpp_godot.release.so"; then
        echo "export: release library missing or older than the Rust sources, building" >&2
        local saved_profile="$PROFILE"
        PROFILE=release
        cmd_build
        PROFILE="$saved_profile"
    fi
    "$ROOT/tools/export-linux.sh"
}

cmd_licenses() {
    local meta triple
    mkdir -p "$ROOT/dist/logs"
    meta="$ROOT/dist/logs/cargo-metadata.json"
    if [[ "${PP_NATIVE:-}" == 1 ]]; then
        triple=$(rustc -vV | sed -n 's/^host: //p')
    else
        if ! podman image exists "$IMAGE"; then
            echo "error: toolbox image '$IMAGE' is not built. Run ./x toolbox" >&2
            exit 1
        fi
        triple=$(podman run --rm "$IMAGE" rustc -vV | sed -n 's/^host: //p')
    fi
    [[ -n "$triple" ]] || { echo "licenses: could not read the rustc host triple" >&2; exit 1; }
    run_cargo metadata --format-version 1 --locked --filter-platform "$triple" >"$meta"
    python3 "$ROOT/tools/licenses.py" "$meta" "$ROOT/docs/third-party-licenses.md"
    echo "licenses: wrote $ROOT/docs/third-party-licenses.md"
}

cmd_package() {
    local dir="$ROOT/dist/penguin-pass-linux-x86_64"
    local ver name archive size hash
    [[ -f "$dir/penguin-pass.x86_64" ]] || {
        echo "package: $dir is missing. Run ./x export" >&2
        exit 1
    }
    ver=$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT/Cargo.toml" | head -n 1)
    [[ -n "$ver" ]] || ver="0.1.0"
    name="penguin-pass-${ver}-linux-x86_64.tar.gz"
    archive="$ROOT/dist/$name"
    tar -C "$ROOT/dist" -czf "$archive" penguin-pass-linux-x86_64
    size=$(stat -c '%s' "$archive")
    hash=$(sha256sum "$archive" | awk '{print $1}')
    echo "$archive"
    echo "size: $size bytes"
    echo "sha256: $hash"
}

# scan forwards its remaining arguments to pp-scan. Every other command
# still takes exactly one word.
if [[ "${1:-}" == "scan" ]]; then
    shift
    cmd_scan "$@"
    exit $?
fi

if [[ "${1:-}" == "play" ]]; then
    shift
    cmd_play "$@"
    exit $?
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
    fetch) cmd_fetch ;;
    export) cmd_export ;;
    package) cmd_package ;;
    licenses) cmd_licenses ;;
    *) usage ;;
esac
