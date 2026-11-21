#!/usr/bin/env bash
# Re-take the standard Penguin Pass screenshot set into docs/img/shots/.
# Always uses the fake trainer. This script never opens Bluetooth.
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
DIR="$ROOT/docs/img/shots"
mkdir -p "$DIR"
mkdir -p "$ROOT/godot/.godot"
SETTINGS="$ROOT/godot/.godot/shot-settings.toml"
cat >"$SETTINGS" <<'EOF'
fullscreen = false
tv_dashboard_enabled = true
tv_dashboard_port = 7878
EOF
GODOT_BIN=${GODOT:-flatpak run org.godotengine.Godot}
$GODOT_BIN --path "$ROOT/godot" --script res://tests/shot.gd -- \
    --fake-trainer \
    --settings "$SETTINGS" --dashboard-port 0 --dashboard-loopback \
    --set --dir "$DIR" --power 250
