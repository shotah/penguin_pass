#!/usr/bin/env python3
"""List a Godot 4 PCK, or check that project files were stored raw."""

import os
import struct
import sys

PACK_DIR_ENCRYPTED = 1
PACK_REL_FILEBASE = 2


def _load(path):
    with open(path, "rb") as handle:
        data = handle.read()
    if data[:4] == b"GDPC":
        return data, 0
    start = data.rfind(b"GDPC")
    if start < 0:
        raise SystemExit(f"pck: no GDPC header in {path}")
    return data, start


def list_files(path):
    data, start = _load(path)
    off = start + 4
    version, major, minor, patch = struct.unpack_from("<4I", data, off)
    off += 16
    if version not in (2, 3, 4):
        raise SystemExit(f"pck: unsupported pack version {version}")
    flags = struct.unpack_from("<I", data, off)[0]
    off += 4
    file_base = struct.unpack_from("<Q", data, off)[0]
    off += 8
    if flags & PACK_DIR_ENCRYPTED:
        raise SystemExit("pck: encrypted directory is not supported")
    if version in (3, 4) or (version == 2 and flags & PACK_REL_FILEBASE):
        file_base += start
    if version in (3, 4):
        dir_offset = struct.unpack_from("<Q", data, off)[0] + start
    else:
        dir_offset = off + 64
    off = dir_offset
    count = struct.unpack_from("<I", data, off)[0]
    off += 4
    files = []
    for _ in range(count):
        length = struct.unpack_from("<I", data, off)[0]
        off += 4
        name = data[off : off + length].decode("utf-8", "replace").rstrip("\x00")
        off += length
        file_off, size = struct.unpack_from("<QQ", data, off)
        off += 16
        off += 16  # md5
        off += 4  # flags
        files.append((name, file_base + file_off, size))
    return version, major, minor, patch, files


def main(argv):
    if len(argv) < 2:
        print("usage: pck_list.py PACK [--check PROJECT_DIR]", file=sys.stderr)
        return 1
    version, major, minor, patch, files = list_files(argv[1])
    print(f"pck version {version} godot {major}.{minor}.{patch} files {len(files)}")
    if len(argv) == 2:
        for name, _off, size in files:
            print(f"{size}\t{name}")
        return 0
    if len(argv) != 4 or argv[2] != "--check":
        print("usage: pck_list.py PACK [--check PROJECT_DIR]", file=sys.stderr)
        return 1
    packed = {}
    for name, _off, size in files:
        packed[name] = size
        if name.startswith("res://"):
            packed[name[len("res://") :]] = size
    missing = []
    mismatch = []
    checked = 0
    root = argv[3]
    for folder in ("assets", "themes"):
        base = os.path.join(root, folder)
        for dirpath, dirnames, names in os.walk(base):
            dirnames[:] = [d for d in dirnames if not d.startswith(".") and d != "_incoming"]
            for name in names:
                if name.startswith(".") or name.endswith(".import") or name.endswith(".uid"):
                    continue
                full = os.path.join(dirpath, name)
                rel = os.path.relpath(full, root).replace(os.sep, "/")
                checked += 1
                if rel not in packed:
                    missing.append(rel)
                    continue
                want = os.path.getsize(full)
                if packed[rel] != want:
                    mismatch.append(f"{rel} packed {packed[rel]} source {want}")
    if missing or mismatch:
        print(f"pck check failed: {len(missing)} missing, {len(mismatch)} size mismatches", file=sys.stderr)
        for line in missing[:20]:
            print(f"missing {line}", file=sys.stderr)
        for line in mismatch[:20]:
            print(f"size {line}", file=sys.stderr)
        return 1
    print(f"pck check ok: {checked} assets and theme files match source size")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
