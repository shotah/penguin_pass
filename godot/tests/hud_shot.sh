#!/usr/bin/env bash
# Ride HUD pictures. Never opens Bluetooth.
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
DIR="${1:-$ROOT/docs/img/hud/history/round1}"
mkdir -p "$DIR"
DIR=$(cd "$DIR" && pwd)
mkdir -p "$ROOT/godot/.godot"
SETTINGS="$ROOT/godot/.godot/hud-shot-settings.toml"
cat >"$SETTINGS" <<'EOF'
fullscreen = false
tv_dashboard_enabled = false
EOF
GODOT_BIN=${GODOT:-flatpak run org.godotengine.Godot}
$GODOT_BIN --path "$ROOT/godot" --script res://tests/hud_shot.gd -- \
    --fake-trainer \
    --settings "$SETTINGS" \
    --dashboard-port 0 \
    --skip-title \
    --dir "$DIR"
