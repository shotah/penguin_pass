#!/usr/bin/env bash
# Export the Linux player build into dist/penguin-pass-linux-x86_64/.
# Called by ./x export after the release library and Godot templates exist.
# The stage copy is throwaway. The source project is not modified.

set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
GODOT="$ROOT/tools/godot/4.7.2/Godot_v4.7.2-stable_linux.x86_64"
TEMPLATE_RELEASE="$ROOT/tools/godot/4.7.2/templates/linux_release.x86_64"
TEMPLATE_DEBUG="$ROOT/tools/godot/4.7.2/templates/linux_debug.x86_64"
STAGE="$ROOT/dist/export-stage"
OUT="$ROOT/dist/penguin-pass-linux-x86_64"
LOG="$ROOT/dist/logs/export.log"
PRESET="Linux/X11 x86_64"

[[ -x "$GODOT" ]] || { echo "export: Godot editor missing. Run ./x fetch" >&2; exit 1; }
[[ -f "$TEMPLATE_RELEASE" && -f "$TEMPLATE_DEBUG" ]] || {
    echo "export: export templates missing. Run ./x fetch" >&2
    exit 1
}
[[ -f "$ROOT/godot/bin/libpp_godot.release.so" ]] || {
    echo "export: release library missing. Run PROFILE=release ./x build" >&2
    exit 1
}

# Two exports share dist/export-stage. Hold this lock for the rest of the
# script so a second ./x export waits instead of deleting the stage.
mkdir -p "$ROOT/dist"
exec 9>"$ROOT/dist/export.lock"
if ! flock -n 9; then
    echo "export: waiting; another export holds $ROOT/dist/export.lock" >&2
    flock 9
fi

mkdir -p "$ROOT/dist/logs"
rm -rf "$STAGE" "$OUT"
mkdir -p "$STAGE/bin" "$OUT"

for f in project.godot icon.svg main.tscn penguin_pass.gdextension export_presets.cfg; do
    cp -a "$ROOT/godot/$f" "$STAGE/$f"
    if [[ -f "$ROOT/godot/$f.uid" ]]; then
        cp -a "$ROOT/godot/$f.uid" "$STAGE/$f.uid"
    fi
done
cp -a "$ROOT/godot/bin/libpp_godot.release.so" "$STAGE/bin/"
cp -a "$ROOT/godot/assets" "$ROOT/godot/themes" "$STAGE/"
rm -rf "$STAGE/assets/third_party/_incoming"

# Godot's importer would replace glTF and images with its own resources.
# importer=keep stores the original bytes at the res:// path, which is what
# GLTFDocument and Image.load read. These files exist only in the stage copy.
python3 - "$STAGE" <<'PY'
import secrets
import sys
from pathlib import Path

stage = Path(sys.argv[1])
exts = {
    ".gltf", ".glb", ".png", ".jpg", ".jpeg", ".hdr", ".exr",
    ".svg", ".webp", ".bmp", ".tga", ".json",
}
count = 0
for folder in ("assets", "themes"):
    for path in (stage / folder).rglob("*"):
        if not path.is_file() or path.suffix.lower() not in exts:
            continue
        if "_incoming" in path.parts:
            continue
        rel = path.relative_to(stage).as_posix()
        uid = "uid://" + secrets.token_hex(8)
        path.with_name(path.name + ".import").write_text(
            "[remap]\n\n"
            'importer="keep"\n'
            f'uid="{uid}"\n\n'
            "[deps]\n\n"
            f'source_file="res://{rel}"\n'
            "dest_files=[]\n",
            encoding="utf-8",
        )
        count += 1
print(f"export: wrote {count} keep-import sidecars in the stage copy")
PY

python3 - "$STAGE/export_presets.cfg" "$TEMPLATE_DEBUG" "$TEMPLATE_RELEASE" <<'PY'
import sys
from pathlib import Path

path, debug, release = sys.argv[1:]
text = Path(path).read_text(encoding="utf-8")
text = text.replace(
    'custom_template/debug="../tools/godot/4.7.2/templates/linux_debug.x86_64"',
    f'custom_template/debug="{debug}"',
)
text = text.replace(
    'custom_template/release="../tools/godot/4.7.2/templates/linux_release.x86_64"',
    f'custom_template/release="{release}"',
)
Path(path).write_text(text, encoding="utf-8")
PY

{
    echo "=== $(date -Is) import ==="
} >"$LOG"

set +e
"$GODOT" --headless --path "$STAGE" --import >>"$LOG" 2>&1
import_code=$?
set -e
echo "export: import exited $import_code"
if [[ "$import_code" -ne 0 ]]; then
    echo "export: import failed; the log is $LOG" >&2
    exit "$import_code"
fi

sample=$(find "$STAGE/assets" -name '*.gltf.import' -print -quit)
if [[ -z "$sample" ]] || ! grep -q 'importer="keep"' "$sample"; then
    echo "export: Godot did not keep the raw glTF importer ($sample)" >&2
    exit 1
fi

{
    echo "=== $(date -Is) export-release ==="
} >>"$LOG"
"$GODOT" --headless --path "$STAGE" --export-release "$PRESET" "$OUT/penguin-pass.x86_64" >>"$LOG" 2>&1

if [[ ! -x "$OUT/penguin-pass.x86_64" ]]; then
    # Godot names the binary from the path we passed. Make the bit explicit.
    chmod +x "$OUT/penguin-pass.x86_64" 2>/dev/null || true
fi
[[ -f "$OUT/penguin-pass.x86_64" ]] || {
    echo "export: binary was not written. See $LOG" >&2
    exit 1
}
chmod +x "$OUT/penguin-pass.x86_64"

pck=$(find "$OUT" -maxdepth 1 -name '*.pck' -print -quit)
[[ -n "$pck" ]] || { echo "export: no pck next to the binary" >&2; exit 1; }

if [[ ! -f "$OUT/bin/libpp_godot.release.so" && ! -f "$OUT/libpp_godot.release.so" ]]; then
    mkdir -p "$OUT/bin"
    cp -a "$STAGE/bin/libpp_godot.release.so" "$OUT/bin/"
fi

python3 "$ROOT/tools/pck_list.py" "$pck" --check "$STAGE"

mkdir -p "$OUT/licenses"
cp -a "$ROOT/docs/assets.md" "$OUT/licenses/assets.md"
if [[ -f "$ROOT/tools/godot/4.7.2/LICENSE.txt" ]]; then
    cp -a "$ROOT/tools/godot/4.7.2/LICENSE.txt" "$OUT/licenses/GODOT-LICENSE.txt"
fi
if [[ -f "$ROOT/tools/godot/4.7.2/COPYRIGHT.txt" ]]; then
    cp -a "$ROOT/tools/godot/4.7.2/COPYRIGHT.txt" "$OUT/licenses/GODOT-COPYRIGHT.txt"
fi
# Mirror godot/assets/... under licenses/assets/... so pack licences stay
# next to the names a reader expects, without a doubled third_party/.
while IFS= read -r -d '' file; do
    rel=${file#"$ROOT/godot/"}
    dest="$OUT/licenses/$rel"
    mkdir -p "$(dirname "$dest")"
    cp -a "$file" "$dest"
done < <(find "$ROOT/godot/assets" -type f \( -name 'LICENSE.txt' -o -name 'SOURCE.md' -o -name 'README.md' \) ! -path '*/_incoming/*' -print0)

echo "export: $OUT"
find "$OUT" -maxdepth 2 -printf '%s %p\n' | sort -n
echo "export: directory size $(du -sh "$OUT" | awk '{print $1}')"
