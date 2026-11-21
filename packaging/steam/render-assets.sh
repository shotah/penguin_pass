#!/usr/bin/env bash
# Render the Steam store and library images from the SVGs in assets/src.
# Requires rsvg-convert, python3, and GdkPixbuf (gi). Does not install packages.
# Wordmarks use Barlow Semi Condensed Bold from the game font directory.
set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
ROOT=$(cd "$SCRIPT_DIR/../.." && pwd)
SRC="$SCRIPT_DIR/assets/src"
OUT="$SCRIPT_DIR/assets"
FONT_DIR="$ROOT/godot/assets/fonts/barlow-semi-condensed"
FONT_FILE="$FONT_DIR/BarlowSemiCondensed-Bold.ttf"
LOGO="$ROOT/docs/img/logo.svg"

if [[ ! -f "$FONT_FILE" ]]; then
  echo "missing font: $FONT_FILE" >&2
  exit 1
fi
if [[ ! -f "$LOGO" ]]; then
  echo "missing logo: $LOGO" >&2
  exit 1
fi
if ! command -v rsvg-convert >/dev/null 2>&1; then
  echo "rsvg-convert is required" >&2
  exit 1
fi

if grep -E '<text|<image' "$SRC/library-hero.svg" >/dev/null; then
  echo "library-hero.svg must not contain text or an embedded image" >&2
  exit 1
fi

TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
mkdir -p "$TMP/cache"

cat > "$TMP/fonts.conf" << EOF
<?xml version="1.0"?>
<!DOCTYPE fontconfig SYSTEM "fonts.dtd">
<fontconfig>
  <dir>$FONT_DIR</dir>
  <cachedir>$TMP/cache</cachedir>
</fontconfig>
EOF
export FONTCONFIG_FILE="$TMP/fonts.conf"

matched=$(fc-match -f '%{file}\n' "Barlow Semi Condensed:weight=700")
case "$matched" in
  *BarlowSemiCondensed-Bold.ttf) ;;
  *)
    echo "fontconfig did not select Barlow Semi Condensed Bold (got: $matched)" >&2
    exit 1
    ;;
esac

render() {
  local svg="$1" w="$2" h="$3" dest="$4"
  rsvg-convert -w "$w" -h "$h" -o "$dest" "$svg"
}

render "$SRC/header-capsule.svg" 920 430 "$OUT/header-capsule-920x430.png"
render "$SRC/small-capsule.svg" 462 174 "$OUT/small-capsule-462x174.png"
render "$SRC/main-capsule.svg" 1232 706 "$OUT/main-capsule-1232x706.png"
render "$SRC/vertical-capsule.svg" 748 896 "$OUT/vertical-capsule-748x896.png"
render "$SRC/library-capsule.svg" 600 900 "$OUT/library-capsule-600x900.png"
render "$SRC/library-hero.svg" 3840 1240 "$OUT/library-hero-3840x1240.png"
render "$SRC/library-logo.svg" 1280 720 "$OUT/library-logo-1280x720.png"
render "$LOGO" 256 256 "$OUT/shortcut-icon-256x256.png"

python3 - "$LOGO" "$TMP/app-icon.svg" << 'PY'
import re
import sys

src, dst = sys.argv[1], sys.argv[2]
text = open(src, encoding="utf-8").read()
text = text.replace(' clip-path="url(#badge)"', "")
text = re.sub(
    r'\n  <rect x="4" y="4" width="504" height="504" rx="92"[^/]*/>\n',
    "\n",
    text,
)
if 'clip-path="url(#badge)"' in text or 'rx="92"' in text:
    raise SystemExit("could not remove the rounded badge clip from the app icon")
open(dst, "w", encoding="utf-8").write(text)
PY

render "$TMP/app-icon.svg" 184 184 "$TMP/app-icon.png"

python3 - "$TMP/app-icon.png" "$OUT/app-icon-184x184.jpg" << 'PY'
import sys

import gi

gi.require_version("GdkPixbuf", "2.0")
from gi.repository import GdkPixbuf

src, dst = sys.argv[1], sys.argv[2]
pb = GdkPixbuf.Pixbuf.new_from_file(src)
if pb.get_has_alpha():
    flat = GdkPixbuf.Pixbuf.new(GdkPixbuf.Colorspace.RGB, False, 8, pb.get_width(), pb.get_height())
    flat.fill(0xFFFFFFFF)
    pb.composite(
        flat,
        0,
        0,
        pb.get_width(),
        pb.get_height(),
        0,
        0,
        1,
        1,
        GdkPixbuf.InterpType.NEAREST,
        255,
    )
    pb = flat
if not pb.savev(dst, "jpeg", ["quality"], ["90"]):
    raise SystemExit("jpeg save failed")
PY

python3 - "$OUT" << 'PY'
import struct
import sys
from pathlib import Path

out = Path(sys.argv[1])

def png_info(path):
    data = path.read_bytes()
    if data[:8] != b"\x89PNG\r\n\x1a\n":
        raise SystemExit(f"{path.name} is not a PNG")
    if data[12:16] != b"IHDR":
        raise SystemExit(f"{path.name} missing IHDR")
    w, h = struct.unpack(">II", data[16:24])
    color = data[25]
    return w, h, color

def jpeg_size(path):
    data = path.read_bytes()
    if data[:2] != b"\xff\xd8":
        raise SystemExit(f"{path.name} is not a JPEG")
    i = 2
    while i + 9 < len(data):
        if data[i] != 0xFF:
            i += 1
            continue
        marker = data[i + 1]
        if marker in (0xC0, 0xC1, 0xC2):
            h, w = struct.unpack(">HH", data[i + 5 : i + 9])
            return w, h
        if marker in (0xD8, 0x01):
            i += 2
            continue
        if marker == 0xD9:
            break
        if i + 4 > len(data):
            break
        length = struct.unpack(">H", data[i + 2 : i + 4])[0]
        i += 2 + length
    raise SystemExit(f"{path.name} has no JPEG frame")

expected_png = {
    "header-capsule-920x430.png": (920, 430),
    "small-capsule-462x174.png": (462, 174),
    "main-capsule-1232x706.png": (1232, 706),
    "vertical-capsule-748x896.png": (748, 896),
    "library-capsule-600x900.png": (600, 900),
    "library-hero-3840x1240.png": (3840, 1240),
    "library-logo-1280x720.png": (1280, 720),
    "shortcut-icon-256x256.png": (256, 256),
}

for name, (w, h) in expected_png.items():
    path = out / name
    got = png_info(path)
    if got[0] != w or got[1] != h:
        raise SystemExit(f"{name} is {got[0]}x{got[1]}, expected {w}x{h}")

logo = png_info(out / "library-logo-1280x720.png")
if logo[2] not in (4, 6):
    raise SystemExit(f"library logo is not a transparent PNG (color type {logo[2]})")

hero = png_info(out / "library-hero-3840x1240.png")
if hero[2] in (4, 6):
    raise SystemExit("library hero should be an opaque PNG")

jpg = out / "app-icon-184x184.jpg"
jw, jh = jpeg_size(jpg)
if (jw, jh) != (184, 184):
    raise SystemExit(f"app icon is {jw}x{jh}, expected 184x184")

import gi

gi.require_version("GdkPixbuf", "2.0")
from gi.repository import GdkPixbuf

def pixels(path):
    pb = GdkPixbuf.Pixbuf.new_from_file(str(path))
    return pb, bytes(pb.get_pixels()), pb.get_rowstride(), pb.get_n_channels()

def at(raw, rs, nc, x, y):
    i = y * rs + x * nc
    return tuple(raw[i : i + nc])

pb, raw, rs, nc = pixels(jpg)
for x, y in ((0, 0), (183, 0), (0, 183), (183, 183)):
    r, g, b = at(raw, rs, nc, x, y)[:3]
    if r + g + b < 90:
        raise SystemExit(f"app icon corner {(x, y)} is near black {(r, g, b)}")

pb, raw, rs, nc = pixels(out / "library-logo-1280x720.png")
if nc < 4:
    raise SystemExit("library logo pixbuf has no alpha")
corner = at(raw, rs, nc, 4, 4)
if corner[3] != 0:
    raise SystemExit(f"library logo corner is not transparent {corner}")
body = at(raw, rs, nc, 212, 360)
if body[3] < 250:
    raise SystemExit(f"library logo mascot is not opaque {body}")

pb, raw, rs, nc = pixels(out / "shortcut-icon-256x256.png")
if nc < 4 or at(raw, rs, nc, 2, 2)[3] != 0:
    raise SystemExit("shortcut icon should keep the logo badge transparent corners")

print("dimensions ok")
for name in list(expected_png) + ["app-icon-184x184.jpg"]:
    path = out / name
    print(f"{path.stat().st_size:10d}  {name}")
PY
