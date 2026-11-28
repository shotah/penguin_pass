#!/usr/bin/env bash
# Export Penguin Pass for linux, windows, or macos using the Linux Godot editor.

set -euo pipefail

TARGET="${1:-}"
case "$TARGET" in
    linux|windows|macos) ;;
    *)
        echo "usage: export-player.sh linux|windows|macos" >&2
        exit 1
        ;;
esac

ROOT=$(cd "$(dirname "$0")/.." && pwd)
GODOT="$ROOT/tools/godot/4.7.2/Godot_v4.7.2-stable_linux.x86_64"
GODOT_DIR="$ROOT/tools/godot/4.7.2"
TPZ="$GODOT_DIR/Godot_v4.7.2-stable_export_templates.tpz"

case "$TARGET" in
    linux)
        PRESET="Linux/X11 x86_64"
        LIB="libpp_godot.release.so"
        OUT="$ROOT/dist/penguin-pass-linux-x86_64"
        OUT_PATH="$OUT/penguin-pass.x86_64"
        LOG="$ROOT/dist/logs/export-linux.log"
        TEMPLATES=(linux_debug.x86_64 linux_release.x86_64)
        ;;
    windows)
        PRESET="Windows Desktop x86_64"
        LIB="pp_godot.release.dll"
        OUT="$ROOT/dist/penguin-pass-windows-x86_64"
        OUT_PATH="$OUT/penguin-pass.exe"
        LOG="$ROOT/dist/logs/export-windows.log"
        TEMPLATES=(windows_debug_x86_64.exe windows_release_x86_64.exe)
        ;;
    macos)
        PRESET="macOS"
        LIB="libpp_godot.release.dylib"
        OUT="$ROOT/dist/penguin-pass-macos"
        OUT_PATH="$OUT/penguin-pass.app"
        LOG="$ROOT/dist/logs/export-macos.log"
        TEMPLATES=(macos.zip)
        ;;
esac

STAGE="$ROOT/dist/export-stage-${TARGET}"

[[ -x "$GODOT" ]] || { echo "export: Godot editor missing. Run ./x fetch" >&2; exit 1; }

extract_template() {
    local tpl="$1"
    local dest="$GODOT_DIR/templates/$tpl"
    if [[ -f "$dest" ]]; then
        return 0
    fi
    if [[ ! -f "$TPZ" ]]; then
        echo "export: template missing: $dest (and $TPZ not found)" >&2
        return 1
    fi
    echo "export: extracting templates/$tpl from $TPZ" >&2
    unzip -qo "$TPZ" "templates/$tpl" -d "$GODOT_DIR"
    if [[ "$tpl" == linux_* ]]; then
        chmod +x "$dest"
    fi
}

for tpl in "${TEMPLATES[@]}"; do
    if [[ ! -f "$GODOT_DIR/templates/$tpl" ]]; then
        extract_template "$tpl"
    fi
    [[ -f "$GODOT_DIR/templates/$tpl" ]] || {
        echo "export: export template still missing: $GODOT_DIR/templates/$tpl" >&2
        exit 1
    }
done

[[ -f "$ROOT/godot/bin/$LIB" ]] || {
    echo "export: release library missing ($LIB). Run PROFILE=release ./x build" >&2
    exit 1
}

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
cp -a "$ROOT/godot/bin/$LIB" "$STAGE/bin/"
cp -a "$ROOT/godot/assets" "$ROOT/godot/themes" "$STAGE/"
if [[ -d "$ROOT/godot/routes" ]]; then
    cp -a "$ROOT/godot/routes" "$STAGE/"
fi
rm -rf "$STAGE/assets/third_party/_incoming"

python3 - "$STAGE" <<'PY'
import secrets
import sys
from pathlib import Path

stage = Path(sys.argv[1])
exts = {
    ".gltf", ".glb", ".png", ".jpg", ".jpeg", ".hdr", ".exr",
    ".svg", ".webp", ".bmp", ".tga", ".json",
    ".mp3", ".ogg", ".wav",
}
count = 0
for folder in ("assets", "themes", "routes"):
    base = stage / folder
    if not base.is_dir():
        continue
    for path in base.rglob("*"):
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

sed -i "s|\\.\\./tools/|$ROOT/tools/|g" "$STAGE/export_presets.cfg"

{
    echo "=== $(date -Is) import ==="
} >"$LOG"

set +e
"$GODOT" --headless --path "$STAGE" --import >>"$LOG" 2>&1
import_code=$?
set -e
if [[ "$import_code" -ne 0 ]]; then
    echo "export: import failed (exit $import_code); last 80 lines of $LOG:" >&2
    tail -n 80 "$LOG" >&2
    exit "$import_code"
fi

sample=$(find "$STAGE/assets" -name '*.gltf.import' -print -quit 2>/dev/null || true)
if [[ -z "$sample" ]] || ! grep -q 'importer="keep"' "$sample"; then
    echo "export: Godot did not keep the raw glTF importer ($sample)" >&2
    exit 1
fi

{
    echo "=== $(date -Is) export-release ==="
} >>"$LOG"

set +e
"$GODOT" --headless --path "$STAGE" --export-release "$PRESET" "$OUT_PATH" >>"$LOG" 2>&1
export_code=$?
set -e
if [[ "$export_code" -ne 0 ]]; then
    echo "export: export-release failed (exit $export_code); last 80 lines of $LOG:" >&2
    tail -n 80 "$LOG" >&2
    exit "$export_code"
fi

case "$TARGET" in
    linux)
        if [[ ! -x "$OUT/penguin-pass.x86_64" ]]; then
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
        ;;
    windows)
        [[ -f "$OUT/penguin-pass.exe" ]] || {
            echo "export: penguin-pass.exe missing. See $LOG" >&2
            exit 1
        }
        [[ -f "$OUT/penguin-pass.pck" ]] || {
            echo "export: penguin-pass.pck missing" >&2
            exit 1
        }
        if [[ ! -f "$OUT/pp_godot.release.dll" && ! -f "$OUT/bin/pp_godot.release.dll" ]]; then
            echo "export: pp_godot.release.dll missing" >&2
            exit 1
        fi
        python3 "$ROOT/tools/pck_list.py" "$OUT/penguin-pass.pck" --check "$STAGE"
        ;;
    macos)
        MACOS_BIN="$OUT/penguin-pass.app/Contents/MacOS/Penguin Pass"
        [[ -x "$MACOS_BIN" ]] || {
            echo "export: $MACOS_BIN missing or not executable" >&2
            exit 1
        }
        [[ -f "$OUT/penguin-pass.app/Contents/Resources/Penguin Pass.pck" ]] || {
            echo "export: Penguin Pass.pck missing in .app bundle" >&2
            exit 1
        }
        [[ -f "$OUT/penguin-pass.app/Contents/Frameworks/libpp_godot.release.dylib" ]] || {
            echo "export: libpp_godot.release.dylib missing in .app bundle" >&2
            exit 1
        }
        [[ -f "$OUT/penguin-pass.app/Contents/_CodeSignature/CodeResources" ]] || {
            echo "export: CodeResources missing (codesign may have failed)" >&2
            exit 1
        }
        python3 "$ROOT/tools/pck_list.py" \
            "$OUT/penguin-pass.app/Contents/Resources/Penguin Pass.pck" --check "$STAGE"
        ;;
esac

mkdir -p "$OUT/licenses"
cp -a "$ROOT/LICENSE" "$OUT/licenses/LICENSE"
cp -a "$ROOT/docs/assets.md" "$OUT/licenses/assets.md"
if [[ -f "$ROOT/tools/godot/4.7.2/LICENSE.txt" ]]; then
    cp -a "$ROOT/tools/godot/4.7.2/LICENSE.txt" "$OUT/licenses/GODOT-LICENSE.txt"
fi
if [[ -f "$ROOT/tools/godot/4.7.2/COPYRIGHT.txt" ]]; then
    cp -a "$ROOT/tools/godot/4.7.2/COPYRIGHT.txt" "$OUT/licenses/GODOT-COPYRIGHT.txt"
fi
while IFS= read -r -d '' file; do
    rel=${file#"$ROOT/godot/"}
    dest="$OUT/licenses/$rel"
    mkdir -p "$(dirname "$dest")"
    cp -a "$file" "$dest"
done < <(find "$ROOT/godot/assets" -type f \( -name 'LICENSE.txt' -o -name 'SOURCE.md' -o -name 'README.md' \) ! -path '*/_incoming/*' -print0)

echo "export: $OUT"
find "$OUT" -maxdepth 3 -printf '%s %p\n' 2>/dev/null | sort -n || find "$OUT" -maxdepth 3 | head -50
echo "export: directory size $(du -sh "$OUT" | awk '{print $1}')"