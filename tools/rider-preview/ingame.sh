#!/usr/bin/env bash
# In-game pictures and timings of the rider.
#
#   tools/rider-preview/ingame.sh                 all pictures into docs/img/rider/
#   ONLY=sequence,stand tools/rider-preview/ingame.sh
#   OUT=/some/dir tools/rider-preview/ingame.sh   write somewhere else
#   PRESET=sigrid PREFIX=rider2- tools/rider-preview/ingame.sh
#   tools/rider-preview/ingame.sh bench           timings at the switchback
#   tools/rider-preview/ingame.sh world           course build stats and world pictures
#   tools/rider-preview/ingame.sh shot ARGS...    godot/tests/shot.gd with ARGS
#
# Uses the game library already in godot/bin; run ./x build first. Every
# launch passes --no-bluetooth, a scratch settings file, and
# --dashboard-port 0, so this never opens Bluetooth or the owner's settings.
# GODOT=/path/to/godot selects the binary. The default is the flatpak.
set -euo pipefail

HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/../.." && pwd)
OUT=${OUT:-$ROOT/docs/img/rider}
ONLY=${ONLY:-}

if [[ -n "${GODOT:-}" ]]; then
    godot_cmd=("$GODOT")
else
    godot_cmd=(flatpak run org.godotengine.Godot)
fi

# The flatpak has its own /tmp, so the settings file lives in the repo.
mkdir -p "$HERE/.cache" "$ROOT/godot/.godot"
SETTINGS="$HERE/.cache/ingame-settings.toml"
printf 'fullscreen = false\ndashboard_enabled = true\n' >"$SETTINGS"
SAFE=(--no-bluetooth --no-music --no-sound --settings "$SETTINGS" --dashboard-port 0 --dashboard-loopback)

# Script mode loads only the extensions named in this list (see ./x smoke).
if [[ ! -s "$ROOT/godot/.godot/extension_list.cfg" ]]; then
    printf 'res://penguin_pass.gdextension\n' >"$ROOT/godot/.godot/extension_list.cfg"
fi

run() {
    local script=$1
    shift
    local log code
    log="$HERE/.cache/ingame.log"
    set +e
    "${godot_cmd[@]}" --audio-driver Dummy --path "$ROOT/godot" --script "$script" -- "${SAFE[@]}" "$@" >"$log" 2>&1
    code=$?
    set -e
    grep -E '^(RIDER_|WORLD_|PENGUIN_PASS_(SHOT|FRAME_MS|READY_MS|VIEW|WORLD))|leaked|ObjectDB' "$log" || true
    if [[ "$code" -ne 0 ]]; then
        echo "ingame: Godot exited $code; full log follows" >&2
        cat "$log" >&2
        exit "$code"
    fi
}

case "${1:-pictures}" in
    pictures)
        args=(--out-dir "$OUT" --repo "$ROOT")
        [[ -n "$ONLY" ]] && args+=(--only "$ONLY")
        [[ -n "${PRESET:-}" ]] && args+=(--preset "$PRESET")
        [[ -n "${PREFIX:-}" ]] && args+=(--prefix "$PREFIX")
        run "$HERE/ingame.gd" "${args[@]}"
        ;;
    bench)
        run "$HERE/ingame.gd" --bench
        ;;
    world)
        run "$HERE/world.gd" --out-dir "$ROOT/docs/img/world" --rider-dir "$ROOT/docs/img/rider"
        ;;
    shot)
        shift
        run res://tests/shot.gd "$@"
        ;;
    *)
        echo "usage: ingame.sh [pictures|bench|shot ARGS...]" >&2
        exit 2
        ;;
esac
