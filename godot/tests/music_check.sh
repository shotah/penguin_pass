#!/usr/bin/env bash
# Headless MusicPlayer check. Does not open Bluetooth and does not launch the ride.
# Godot is $GODOT when that is set, otherwise the flatpak, matching ./x smoke.
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
mkdir -p "$ROOT/godot/.godot"
: >"$ROOT/godot/.godot/extension_list.cfg"
for ext in "$ROOT"/godot/*.gdextension; do
    [[ -e "$ext" ]] || continue
    printf 'res://%s\n' "$(basename "$ext")" >>"$ROOT/godot/.godot/extension_list.cfg"
done
if [[ -n "${GODOT:-}" ]]; then
    godot_cmd=("$GODOT")
else
    godot_cmd=(flatpak run org.godotengine.Godot)
fi
exec "${godot_cmd[@]}" --headless --path "$ROOT/godot" --script res://tests/music_check.gd
