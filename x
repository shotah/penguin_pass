#!/usr/bin/env bash
# Penguin Pass command runner. The Makefile mirrors these targets.
# Cargo runs in the Steam Runtime toolbox image.
# PP_NATIVE=1 runs cargo on the host instead (CI runners have a C toolchain).
# GODOT=/path/to/godot selects the binary for smoke. The default is the flatpak.
# ./x fetch downloads the official Godot editor and export templates.
# ./x play launches the game. ./x export and ./x package build the player folder.
# ./x release bumps the workspace version, tags v<version>, and pushes it.

set -euo pipefail

ROOT=$(cd "$(dirname "$0")" && pwd)
cd "$ROOT"

IMAGE=${IMAGE:-penguin-pass-toolbox}
CARGO_VOLUME=penguin-pass-cargo
PROFILE=${PROFILE:-debug}
CRATE=${CRATE:-}

run_cargo() {
    # --root DIR runs cargo against a copy. Release --dry-run uses it so the
    # real Cargo.toml and Cargo.lock are not written.
    local cargo_root=$ROOT
    if [[ "${1:-}" == "--root" ]]; then
        cargo_root=$2
        shift 2
    fi
    if [[ "${PP_NATIVE:-}" == 1 ]]; then
        if [[ "$cargo_root" == "$ROOT" ]]; then
            cargo "$@"
        else
            cargo --manifest-path "$cargo_root/Cargo.toml" "$@"
        fi
        return
    fi
    if ! podman image exists "$IMAGE"; then
        echo "error: toolbox image '$IMAGE' is not built. Run ./x toolbox" >&2
        exit 1
    fi
    podman run --rm \
        -v "$cargo_root:/src" \
        -v "${CARGO_VOLUME}:/usr/local/cargo/registry" \
        -w /src \
        -e CARGO_HOME=/usr/local/cargo \
        -e CARGO_TERM_COLOR=always \
        "$IMAGE" \
        cargo "$@"
}

usage() {
    echo "usage: ./x {toolbox|build|test|fmt|clippy|lint|typecheck|smoke|run|scan|controllers|fetch|play|export|package|licenses|release}" >&2
    echo "play:  ./x play [--windowed|--fullscreen] [--resolution WxH] [--quit-after N] [--scratch-data] [--] [game args]" >&2
    echo "release: ./x release [--dry-run|--no-push] {patch|minor|major|X.Y.Z}" >&2
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
    "${godot_cmd[@]}" --headless --audio-driver Dummy --path "$ROOT/godot" \
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

# Lists Zwift controllers that are awake. Read-only: it never connects.
cmd_controllers() {
    run_cargo build -p pp-controllers --bin pp-controller-scan
    "$ROOT/target/debug/pp-controller-scan" "$@"
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
        # A separate process, so a failed build stops there and leaves the
        # last library that worked in place.
        if ! PROFILE=debug "$ROOT/x" build; then
            [[ -f "$ROOT/godot/bin/libpp_godot.debug.so" ]] || exit 1
            echo "play: the build failed (errors above). Starting the last build that worked." >&2
        fi
    fi
    # --trace records this ride to target/traces/<stamp>/ for diagnosis.
    local args=() arg
    for arg in "$@"; do
        if [[ "$arg" == --trace ]]; then
            PENGUIN_PASS_TRACE_DIR="$ROOT/target/traces/$(date +%Y%m%d-%H%M%S)"
            mkdir -p "$PENGUIN_PASS_TRACE_DIR"
            export PENGUIN_PASS_TRACE_DIR
            echo "play: tracing this ride to $PENGUIN_PASS_TRACE_DIR" >&2
        else
            args+=("$arg")
        fi
    done
    exec "$ROOT/tools/penguin-pass.sh" "${args[@]}"
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

# Version is [workspace.package] version in the root Cargo.toml.
# Cargo.lock is refreshed with `cargo update --workspace --offline`, which
# rewrites workspace packages and leaves registry crates that are already
# locked. The metainfo release element is stamped by the Flatpak workflow.
release_current_version() {
    local file=$1
    awk '
        /^\[/ { section = $0 }
        section == "[workspace.package]" && /^version = "/ {
            if (match($0, /"[^"]*"/)) {
                print substr($0, RSTART + 1, RLENGTH - 2)
                exit
            }
        }
    ' "$file"
}

release_write_version() {
    local file=$1 new=$2 tmp got
    tmp=$(mktemp)
    awk -v ver="$new" '
        /^\[/ { section = $0 }
        section == "[workspace.package]" && /^version = "/ {
            sub(/"[^"]*"/, "\"" ver "\"")
        }
        { print }
    ' "$file" >"$tmp"
    mv "$tmp" "$file"
    got=$(release_current_version "$file")
    [[ "$got" == "$new" ]] || {
        echo "release: could not set the workspace version in $file" >&2
        return 1
    }
}

# Print the new X.Y.Z, or fail if spec is not patch, minor, major, or X.Y.Z.
release_next_version() {
    local cur=$1 spec=$2
    local -a p
    if [[ "$spec" == patch || "$spec" == minor || "$spec" == major ]]; then
        if [[ ! "$cur" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
            echo "release: current version is not X.Y.Z ($cur)" >&2
            return 1
        fi
        IFS=. read -r -a p <<<"$cur"
        case "$spec" in
            patch) p[2]=$((10#${p[2]} + 1)) ;;
            minor) p[1]=$((10#${p[1]} + 1)); p[2]=0 ;;
            major) p[0]=$((10#${p[0]} + 1)); p[1]=0; p[2]=0 ;;
        esac
        echo "${p[0]}.${p[1]}.${p[2]}"
        return 0
    fi
    if [[ "$spec" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
        echo "$spec"
        return 0
    fi
    echo "release: version must be patch, minor, major, or X.Y.Z (got: $spec)" >&2
    return 1
}

# Return 0 when $1 is strictly newer than $2. Both are X.Y.Z.
release_version_gt() {
    local -a a b
    local i
    IFS=. read -r -a a <<<"$1"
    IFS=. read -r -a b <<<"$2"
    for i in 0 1 2; do
        if ((10#${a[i]} > 10#${b[i]})); then
            return 0
        fi
        if ((10#${a[i]} < 10#${b[i]})); then
            return 1
        fi
    done
    return 1
}

release_parse_branch_line() {
    local line=$1
    line=${line#\#\# }
    line=${line%%...*}
    printf '%s\n' "$line"
}

release_require_clean() {
    local porcelain=$1
    if [[ -n "$porcelain" ]]; then
        echo "release: working tree is not clean" >&2
        return 1
    fi
}

release_refresh_lock() {
    local dir=$1
    echo "+ cargo update --workspace --offline"
    run_cargo --root "$dir" update --workspace --offline
}

# Copy the manifests and bump them in DIR. Does not touch $ROOT.
release_bump_copy() {
    local dir=$1 new=$2
    mkdir -p "$dir"
    cp "$ROOT/Cargo.toml" "$ROOT/Cargo.lock" "$dir/"
    cp -a "$ROOT/crates" "$dir/crates"
    release_write_version "$dir/Cargo.toml" "$new"
    release_refresh_lock "$dir"
}

release_restore_files() {
    local backup=$1 dest=$2
    cp "$backup/Cargo.toml" "$dest/Cargo.toml"
    cp "$backup/Cargo.lock" "$dest/Cargo.lock"
}

# These three git commands are what a release runs after the checks.
# --dry-run prints them and does not run them. --no-push runs the first two.
release_git() {
    local new=$1
    echo "+ git commit -am \"Release v${new}\""
    if [[ "$release_dry" -eq 0 ]]; then
        git commit -am "Release v${new}"
    fi
    echo "+ git tag -a \"v${new}\" -m \"Release v${new}\""
    if [[ "$release_dry" -eq 0 ]]; then
        git tag -a "v${new}" -m "Release v${new}"
    fi
    echo "+ git push origin HEAD \"v${new}\""
    if [[ "$release_dry" -eq 0 && "$release_no_push" -eq 0 ]]; then
        git push origin HEAD "v${new}"
    fi
    echo "release: https://github.com/shotah/penguin_pass/actions"
    echo "release: https://github.com/shotah/penguin_pass/releases"
}

release_self_test() {
    local cur work backup before after bad
    cur=0.1.0
    [[ "$(release_next_version "$cur" patch)" == 0.1.1 ]]
    [[ "$(release_next_version "$cur" minor)" == 0.2.0 ]]
    [[ "$(release_next_version "$cur" major)" == 1.0.0 ]]
    [[ "$(release_next_version "$cur" 1.4.5)" == 1.4.5 ]]
    [[ "$(release_next_version 1.9.0 minor)" == 1.10.0 ]]
    release_version_gt 0.1.1 0.1.0
    release_version_gt 0.2.0 0.1.9
    release_version_gt 1.0.0 0.9.9
    release_version_gt 1.10.0 1.9.0
    if release_version_gt 0.1.0 0.1.0; then
        echo "release self-test: equal versions compared newer" >&2
        exit 1
    fi
    if release_version_gt 0.0.9 0.1.0; then
        echo "release self-test: a lower version compared newer" >&2
        exit 1
    fi
    if release_next_version "$cur" 0.0.1 >/dev/null; then
        if release_version_gt 0.0.1 "$cur"; then
            echo "release self-test: 0.0.1 should not be newer than 0.1.0" >&2
            exit 1
        fi
    else
        echo "release self-test: 0.0.1 should parse" >&2
        exit 1
    fi
    local spec
    for spec in 1.2 foo v1.2.3 1.2.3-rc1 1.2.3.4 ""; do
        if release_next_version "$cur" "$spec" >/dev/null 2>&1; then
            echo "release self-test: accepted malformed spec: ${spec:-<empty>}" >&2
            exit 1
        fi
    done
    [[ "$(release_parse_branch_line '## main...origin/main')" == main ]]
    [[ "$(release_parse_branch_line '## main...origin/main [ahead 1]')" == main ]]
    [[ "$(release_parse_branch_line '## main')" == main ]]
    [[ "$(release_parse_branch_line '## HEAD (no branch)')" == "HEAD (no branch)" ]]
    [[ "$(release_status_branch_live)" == main ]]
    if release_require_clean " M Cargo.toml" 2>/dev/null; then
        echo "release self-test: dirty status was accepted" >&2
        exit 1
    fi
    release_require_clean ""
    work=$(mktemp -d)
    backup=$(mktemp -d)
    cp "$ROOT/Cargo.toml" "$work/Cargo.toml"
    cp "$ROOT/Cargo.lock" "$work/Cargo.lock"
    cp "$work/Cargo.toml" "$backup/Cargo.toml"
    cp "$work/Cargo.lock" "$backup/Cargo.lock"
    release_write_version "$work/Cargo.toml" 0.1.1
    grep -q '^version = "0.1.1"$' "$work/Cargo.toml"
    release_restore_files "$backup" "$work"
    grep -q '^version = "0.1.0"$' "$work/Cargo.toml"
    rm -rf "$work" "$backup"
    before=$(sha256sum "$ROOT/Cargo.toml" "$ROOT/Cargo.lock")
    work=$(mktemp -d)
    release_bump_copy "$work" 0.1.1
    bad=$(diff -u "$ROOT/Cargo.lock" "$work/Cargo.lock" | awk '
        /^[-+]/ && !/^[-+]{3}/ && !/^[-+]version = "0\.1\.[01]"$/ { print }
    ' || true)
    if [[ -n "$bad" ]]; then
        echo "release self-test: lockfile changed more than the workspace version" >&2
        printf '%s\n' "$bad" >&2
        rm -rf "$work"
        exit 1
    fi
    diff -q "$ROOT/Cargo.toml" "$work/Cargo.toml" >/dev/null && {
        echo "release self-test: copy of Cargo.toml did not change" >&2
        rm -rf "$work"
        exit 1
    }
    rm -rf "$work"
    after=$(sha256sum "$ROOT/Cargo.toml" "$ROOT/Cargo.lock")
    if [[ "$before" != "$after" ]]; then
        echo "release self-test: real Cargo.toml or Cargo.lock changed" >&2
        exit 1
    fi
    echo "release: 0.1.0 -> 0.1.1"
    release_dry=1
    release_no_push=0
    release_git 0.1.1
    echo "release self-test: ok"
}

release_status_branch_live() {
    local line
    line=$(git status --branch --porcelain)
    line=${line%%$'\n'*}
    release_parse_branch_line "$line"
}

cmd_release() {
    local spec="" release_dry=0 release_no_push=0
    local cur new backup branch work
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --dry-run) release_dry=1 ;;
            --no-push) release_no_push=1 ;;
            --self-test) release_self_test; return ;;
            -h|--help) usage ;;
            --) shift; break ;;
            -*)
                echo "release: unknown flag: $1" >&2
                usage
                ;;
            *)
                if [[ -n "$spec" ]]; then
                    echo "release: too many arguments" >&2
                    usage
                fi
                spec=$1
                ;;
        esac
        shift
    done
    while [[ $# -gt 0 ]]; do
        if [[ -n "$spec" ]]; then
            echo "release: too many arguments" >&2
            usage
        fi
        spec=$1
        shift
    done
    if [[ "$release_dry" -eq 1 && "$release_no_push" -eq 1 ]]; then
        echo "release: use either --dry-run or --no-push" >&2
        exit 1
    fi
    [[ -n "$spec" ]] || usage
    cur=$(release_current_version "$ROOT/Cargo.toml")
    [[ -n "$cur" ]] || {
        echo "release: no version under [workspace.package] in Cargo.toml" >&2
        exit 1
    }
    new=$(release_next_version "$cur" "$spec") || exit 1
    if ! release_version_gt "$new" "$cur"; then
        echo "release: ${new} is not newer than ${cur}" >&2
        exit 1
    fi
    release_require_clean "$(git status --porcelain)"
    branch=$(release_status_branch_live)
    if [[ "$branch" != main ]]; then
        echo "release: current branch is '${branch}', not main" >&2
        exit 1
    fi
    # Tag existence is a read of git. The commit, tag, and push below are the
    # writes. --dry-run still runs this check, then skips the writes.
    echo "+ git rev-parse --verify --quiet \"refs/tags/v${new}\""
    if git rev-parse --verify --quiet "refs/tags/v${new}" >/dev/null; then
        echo "release: tag v${new} already exists" >&2
        exit 1
    fi
    echo "release: ${cur} -> ${new}"
    if [[ "$release_dry" -eq 1 ]]; then
        work=$(mktemp -d)
        release_bump_copy "$work" "$new"
        rm -rf "$work"
        release_git "$new"
        return 0
    fi
    backup=$(mktemp -d)
    cp "$ROOT/Cargo.toml" "$ROOT/Cargo.lock" "$backup/"
    if ! release_write_version "$ROOT/Cargo.toml" "$new"; then
        release_restore_files "$backup" "$ROOT"
        echo "release: restored Cargo.toml and Cargo.lock" >&2
        rm -rf "$backup"
        exit 1
    fi
    if ! release_refresh_lock "$ROOT"; then
        release_restore_files "$backup" "$ROOT"
        echo "release: restored Cargo.toml and Cargo.lock" >&2
        rm -rf "$backup"
        exit 1
    fi
    if ! "$ROOT/x" lint || ! "$ROOT/x" typecheck || ! "$ROOT/x" test; then
        release_restore_files "$backup" "$ROOT"
        echo "release: restored Cargo.toml and Cargo.lock" >&2
        rm -rf "$backup"
        exit 1
    fi
    rm -rf "$backup"
    release_git "$new"
}

# scan forwards its remaining arguments to pp-scan. Every other command
# still takes exactly one word.
if [[ "${1:-}" == "scan" ]]; then
    shift
    cmd_scan "$@"
    exit $?
fi

if [[ "${1:-}" == "controllers" ]]; then
    shift
    cmd_controllers "$@"
    exit $?
fi

if [[ "${1:-}" == "play" ]]; then
    shift
    cmd_play "$@"
    exit $?
fi

if [[ "${1:-}" == "release" ]]; then
    shift
    cmd_release "$@"
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
