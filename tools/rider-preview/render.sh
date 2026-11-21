#!/usr/bin/env bash
# Regenerate the rider preview pictures into docs/img/rider/.
#
#   tools/rider-preview/render.sh
#
# Environment:
#   OUT=dir        write the PNGs somewhere else inside the repository
#   ONLY=a,b       render only these jobs: turnaround, chase, first, strip,
#                  style, closeups, arraymesh, steer, stand, tuck, compare
#   BIKE_ONLY=1    leave the rider out
#   PRESET=id      which rider to export (default nils, the reference)
#   PREFIX=text    prepended to each PNG name, so a second rider does not
#                  overwrite the pictures already in docs/img/rider
#   PP_NATIVE=1    run cargo on the host instead of in the build container
#   GODOT=/path    Godot binary to use instead of the flatpak
#
# Godot needs a real window to render, and the flatpak has a private /tmp,
# so every file it touches stays inside the repository.

set -euo pipefail

HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/../.." && pwd)
OUT=${OUT:-$ROOT/docs/img/rider}
ONLY=${ONLY:-}
GLB=$HERE/.cache
IMAGE=${IMAGE:-penguin-pass-toolbox}

# Same invocation as run_cargo in ./x.
run_cargo() {
    if [[ "${PP_NATIVE:-}" == 1 ]]; then
        (cd "$ROOT" && cargo "$@")
        return
    fi
    podman run --rm \
        -v "$ROOT:/src" \
        -v penguin-pass-cargo:/usr/local/cargo/registry \
        -w /src \
        -e CARGO_HOME=/usr/local/cargo \
        -e CARGO_TERM_COLOR=always \
        "$IMAGE" \
        cargo "$@"
}

run_cargo build -p pp-rider --example export_glb --example export_arrays
EXPORT=$ROOT/target/debug/examples/export_glb

rm -rf "$GLB"
mkdir -p "$GLB" "$OUT"

extra=()
if [[ "${BIKE_ONLY:-}" == 1 ]]; then
    extra+=(--bike-only)
fi
if [[ -n "${PRESET:-}" ]]; then
    extra+=(--preset "$PRESET")
fi

"$EXPORT" --out "$GLB/rest.glb" --crank 25 --wheel 20 "${extra[@]}"
"$EXPORT" --out "$GLB/bike.glb" --crank 25 --wheel 20 --bike-only
"$EXPORT" --out "$GLB/chase.glb" --crank 55 --wheel 140 --effort 0.5 "${extra[@]}"
for deg in 0 45 90 135 180 225 270 315; do
    "$EXPORT" --out "$GLB/strip-$(printf '%03d' "$deg").glb" \
        --crank "$deg" --wheel "$((deg * 2))" --effort 0.6 "${extra[@]}"
done
if [[ "${BIKE_ONLY:-}" != 1 ]]; then
    # The same pose as chase.glb, as plain arrays, to check the Godot recipe.
    "$ROOT/target/debug/examples/export_arrays" --out "$GLB/chase.json" \
        --crank 55 --wheel 140 --effort 0.5
    "$EXPORT" --out "$GLB/steer.glb" --crank 120 --wheel 50 --effort 1 --steer 20 "${extra[@]}"
    "$EXPORT" --out "$GLB/stand.glb" --crank 60 --wheel 80 --effort 0.8 --stand 1 "${extra[@]}"
    "$EXPORT" --out "$GLB/tuck.glb" --crank 0 --wheel 200 --tuck 1 "${extra[@]}"
fi
if [[ "${PAIR:-}" == 1 ]]; then
    "$EXPORT" --out "$GLB/left-chase.glb" --crank 55 --wheel 140 --effort 0.5 --preset nils
    "$EXPORT" --out "$GLB/right-chase.glb" --crank 55 --wheel 140 --effort 0.5 --preset sigrid
    "$EXPORT" --out "$GLB/left-rest.glb" --crank 25 --wheel 20 --preset nils
    "$EXPORT" --out "$GLB/right-rest.glb" --crank 25 --wheel 20 --preset sigrid
fi

args=(--glb-dir "$GLB" --out-dir "$OUT" --repo "$ROOT")
if [[ -n "$ONLY" ]]; then
    args+=(--only "$ONLY")
fi
if [[ -n "${PREFIX:-}" ]]; then
    args+=(--prefix "$PREFIX")
fi

if [[ -n "${GODOT:-}" ]]; then
    godot_cmd=("$GODOT")
else
    godot_cmd=(flatpak run org.godotengine.Godot)
fi

log=$(mktemp)
set +e
"${godot_cmd[@]}" --path "$HERE" --script res://preview.gd -- "${args[@]}" >"$log" 2>&1
code=$?
set -e
grep -E 'RIDER_PREVIEW|ERROR|SCRIPT ERROR|Parse Error' "$log" || true
if [[ "$code" -ne 0 ]] || ! grep -q 'RIDER_PREVIEW_DONE' "$log"; then
    echo "render: Godot exited $code; full log follows" >&2
    cat "$log" >&2
    rm -f "$log"
    exit 1
fi
rm -f "$log"
