#!/usr/bin/env python3
"""Set the single <release> version and date in the AppStream metainfo."""

import re
import sys
from pathlib import Path

USAGE = "usage: stamp-release.py METAINFO.xml VERSION DATE"


def main(argv: list[str]) -> int:
    if len(argv) != 4:
        print(USAGE, file=sys.stderr)
        return 2
    path, version, date = argv[1:]
    if not re.fullmatch(r"[0-9][0-9A-Za-z._+-]*", version):
        print(f"stamp: bad version {version!r}", file=sys.stderr)
        return 1
    if not re.fullmatch(r"\d{4}-\d{2}-\d{2}", date):
        print(f"stamp: bad date {date!r}", file=sys.stderr)
        return 1
    file = Path(path)
    text = file.read_text(encoding="utf-8")
    new = f'<release version="{version}" date="{date}"/>'
    updated, count = re.subn(
        r'<release version="[^"]*" date="[^"]*"/>',
        new,
        text,
        count=1,
    )
    if count != 1:
        print("stamp: release element not found", file=sys.stderr)
        return 1
    file.write_text(updated, encoding="utf-8")
    print(f"stamp: {path} {version} {date}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
