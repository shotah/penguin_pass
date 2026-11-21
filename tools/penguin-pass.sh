#!/usr/bin/env bash
# Launch Penguin Pass the way a player starts it.
# Safe to run from any working directory. Does not build.
# A normal launch uses the player's XDG directories and does not redirect them.
# --scratch-data (or PENGUIN_PASS_SCRATCH_DATA=1) points XDG at dist/xdg/
# for automated checks. Arguments after -- are Godot user args for the game.
#
#   tools/penguin-pass.sh
#   tools/penguin-pass.sh --windowed --resolution 1280x800 --quit-after 120
#   tools/penguin-pass.sh --print-paths
#   tools/penguin-pass.sh --scratch-data -- --no-bluetooth --settings /tmp/s.toml --dashboard-port 0

set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
GODOT="$ROOT/tools/godot/4.7.2/Godot_v4.7.2-stable_linux.x86_64"

windowed=0
resolution=""
quit_after=""
print_paths=0
scratch=0
if [[ "${PENGUIN_PASS_SCRATCH_DATA:-}" == 1 ]]; then
    scratch=1
fi
user_args=()

usage() {
    echo "usage: tools/penguin-pass.sh [--windowed|--fullscreen] [--resolution WxH] [--quit-after N] [--scratch-data] [--print-paths] [-- game-args...]" >&2
    exit 1
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --windowed) windowed=1; shift ;;
        --fullscreen) windowed=0; shift ;;
        --scratch-data) scratch=1; shift ;;
        --print-paths) print_paths=1; shift ;;
        --resolution)
            [[ $# -ge 2 ]] || usage
            resolution="$2"
            shift 2
            ;;
        --quit-after)
            [[ $# -ge 2 ]] || usage
            quit_after="$2"
            shift 2
            ;;
        --)
            shift
            user_args=("$@")
            break
            ;;
        -h|--help) usage ;;
        *) echo "play: unknown argument: $1" >&2; usage ;;
    esac
done

if [[ -n "$resolution" && ! "$resolution" =~ ^[0-9]+x[0-9]+$ ]]; then
    echo "play: --resolution must look like 1280x800" >&2
    exit 1
fi
if [[ -n "$quit_after" && ! "$quit_after" =~ ^[0-9]+$ ]]; then
    echo "play: --quit-after must be a frame count" >&2
    exit 1
fi

if [[ "$scratch" -eq 1 ]]; then
    mkdir -p "$ROOT/dist/xdg/data" "$ROOT/dist/xdg/config" "$ROOT/dist/xdg/cache" "$ROOT/dist/xdg/state"
    export XDG_DATA_HOME="$ROOT/dist/xdg/data"
    export XDG_CONFIG_HOME="$ROOT/dist/xdg/config"
    export XDG_CACHE_HOME="$ROOT/dist/xdg/cache"
    export XDG_STATE_HOME="$ROOT/dist/xdg/state"
fi

# Player logs follow the XDG state dir so they survive a wiped dist/.
# With --scratch-data that dir is dist/xdg/state, so checks stay in the repo.
STATE_HOME="${XDG_STATE_HOME:-$HOME/.local/state}"
LOG="$STATE_HOME/penguin-pass/play.log"
DATA_HOME="${XDG_DATA_HOME:-$HOME/.local/share}"
SETTINGS="$DATA_HOME/godot/app_userdata/Penguin Pass/settings.toml"

if [[ "$print_paths" -eq 1 ]]; then
    echo "scratch=$scratch"
    if [[ -n "${XDG_DATA_HOME+x}" ]]; then
        echo "XDG_DATA_HOME=$XDG_DATA_HOME"
    else
        echo "XDG_DATA_HOME=unset"
    fi
    if [[ -n "${XDG_CONFIG_HOME+x}" ]]; then
        echo "XDG_CONFIG_HOME=$XDG_CONFIG_HOME"
    else
        echo "XDG_CONFIG_HOME=unset"
    fi
    if [[ -n "${XDG_CACHE_HOME+x}" ]]; then
        echo "XDG_CACHE_HOME=$XDG_CACHE_HOME"
    else
        echo "XDG_CACHE_HOME=unset"
    fi
    if [[ -n "${XDG_STATE_HOME+x}" ]]; then
        echo "XDG_STATE_HOME=$XDG_STATE_HOME"
    else
        echo "XDG_STATE_HOME=unset"
    fi
    echo "settings=$SETTINGS"
    echo "log=$LOG"
    exit 0
fi

mkdir -p "$(dirname "$LOG")"

fail() {
    echo "play: $*" | tee -a "$LOG" >&2
    exit 1
}

[[ -x "$GODOT" ]] || fail "Godot binary missing ($GODOT). Run ./x fetch"
[[ -f "$ROOT/godot/bin/libpp_godot.debug.so" ]] || fail "debug library missing. Run ./x build"

args=(--path "$ROOT/godot")
if [[ "$windowed" -eq 1 ]]; then
    args+=(--windowed)
else
    args+=(--fullscreen)
fi
if [[ -n "$resolution" ]]; then
    args+=(--resolution "$resolution")
fi
if [[ -n "$quit_after" ]]; then
    args+=(--quit-after "$quit_after")
fi
if [[ ${#user_args[@]} -gt 0 ]]; then
    args+=(-- "${user_args[@]}")
fi

{
    printf '=== %s cwd=%s scratch=%s ' "$(date -Is)" "$PWD" "$scratch"
    printf '%q ' "${args[@]}"
    printf '===\n'
} >>"$LOG"

set +e
if [[ -t 1 ]]; then
    "$GODOT" "${args[@]}" 2>&1 | tee -a "$LOG"
    code=${PIPESTATUS[0]}
else
    "$GODOT" "${args[@]}" >>"$LOG" 2>&1
    code=$?
fi
set -e
printf '=== exit %s ===\n' "$code" >>"$LOG"
exit "$code"
