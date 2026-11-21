#!/bin/sh
# Copy the exported player into the Flatpak prefix.
# flatpak-builder runs this with the sources already unpacked in the
# current directory. DEST defaults to /app, which is that prefix.
set -eu

dest=${DEST:-/app}
game="$dest/lib/penguin-pass"

mkdir -p "$game" \
    "$dest/bin" \
    "$dest/share/applications" \
    "$dest/share/metainfo" \
    "$dest/share/icons/hicolor/scalable/apps" \
    "$dest/share/licenses/io.github.shotah.PenguinPass"

for item in *; do
    case "$item" in
        penguin-pass.sh|install-player.sh|*.desktop|*.metainfo.xml|*.svg|icons)
            ;;
        *)
            cp -a "$item" "$game/"
            ;;
    esac
done

test -f "$game/penguin-pass.x86_64"
test -f "$game/penguin-pass.pck"
if [ ! -f "$game/libpp_godot.release.so" ] && [ ! -f "$game/bin/libpp_godot.release.so" ]; then
    echo "install-player: release library missing from the export" >&2
    find "$game" -name '*.so' -print >&2
    exit 1
fi

# Artifact zips do not always keep the executable bit.
chmod 755 "$game/penguin-pass.x86_64"

test -f penguin-pass.sh
test -f io.github.shotah.PenguinPass.desktop
test -f io.github.shotah.PenguinPass.metainfo.xml
test -f io.github.shotah.PenguinPass.svg
test -f icons/64.png
test -f icons/128.png
test -f icons/256.png
test -f icons/512.png

install -Dm755 penguin-pass.sh "$dest/bin/penguin-pass"
install -Dm644 io.github.shotah.PenguinPass.desktop \
    "$dest/share/applications/io.github.shotah.PenguinPass.desktop"
install -Dm644 io.github.shotah.PenguinPass.metainfo.xml \
    "$dest/share/metainfo/io.github.shotah.PenguinPass.metainfo.xml"
install -Dm644 io.github.shotah.PenguinPass.svg \
    "$dest/share/icons/hicolor/scalable/apps/io.github.shotah.PenguinPass.svg"

if [ -d icons ]; then
    for size in 64 128 256 512; do
        png="icons/${size}.png"
        if [ -f "$png" ]; then
            install -Dm644 "$png" \
                "$dest/share/icons/hicolor/${size}x${size}/apps/io.github.shotah.PenguinPass.png"
        fi
    done
fi

if [ -d "$game/licenses" ]; then
    cp -a "$game/licenses/." "$dest/share/licenses/io.github.shotah.PenguinPass/"
fi

find "$dest" -type f -printf '%s %p\n' | sort -n
