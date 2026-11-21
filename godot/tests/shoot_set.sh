#!/usr/bin/env bash
# Re-take the standard Penguin Pass screenshot set into docs/img/shots/.
# Always uses the fake trainer. This script never opens Bluetooth.
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
DIR="$ROOT/docs/img/shots"
mkdir -p "$DIR"
SHOT_DIR=$(mktemp -d /tmp/penguin-pass-shot-XXXXXX)
SETTINGS="$SHOT_DIR/settings.toml"
cat >"$SETTINGS" <<'EOF'
fullscreen = false
tv_dashboard_enabled = true
tv_dashboard_port = 7878
EOF
GODOT_BIN=${GODOT:-flatpak run org.godotengine.Godot}
$GODOT_BIN --audio-driver Dummy --path "$ROOT/godot" --script res://tests/shot.gd -- \
    --no-sound \
    --fake-trainer \
    --settings "$SETTINGS" --dashboard-port 0 --dashboard-loopback \
    --set --dir "$DIR" --power 250
