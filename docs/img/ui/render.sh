#!/usr/bin/env bash
# Regenerate every mockup PNG in this folder from src/*.svg with the bundled
# game font. Nothing is installed: rsvg-convert is pointed at
# godot/assets/fonts/ through fonts.conf.
#
#   docs/img/ui/render.sh            all mockups
#   docs/img/ui/render.sh hud-       only names starting with "hud-"
#   REGEN=1 docs/img/ui/render.sh    rebuild the SVGs from tools/gen.py first
#
# Needs rsvg-convert (ships with SteamOS). REGEN=1 also needs python3.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$here"
export FONTCONFIG_FILE="$here/fonts.conf"
prefix="${1:-}"

if [ "${REGEN:-0}" = "1" ]; then
  python3 tools/concepts.py
  python3 tools/gen.py
fi

mkdir -p half
count=0
for svg in src/"$prefix"*.svg; do
  [ -e "$svg" ] || continue
  name="$(basename "$svg" .svg)"
  case "$name" in
    tv-dashboard-phone) rsvg-convert -z 2 "$svg" -o "$name.png" ;;  # 390 x 844 CSS px at 2x
    *) rsvg-convert "$svg" -o "$name.png" ;;
  esac
  # The ride HUD is also rendered at half size: the legibility check.
  case "$name" in
    hud-*) rsvg-convert -z 0.5 "$svg" -o "half/$name.png" ;;
  esac
  count=$((count + 1))
done
echo "rendered $count mockups into $here"
