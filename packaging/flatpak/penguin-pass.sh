#!/bin/sh
# Launch the exported player. Godot finds the pck and the GDExtension
# library next to the real binary, so the working directory does not matter.
exec /app/lib/penguin-pass/penguin-pass.x86_64 "$@"
