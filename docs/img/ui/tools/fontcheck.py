#!/usr/bin/env python3
"""Check a TTF for tabular digits and the glyphs the UI needs.

Pure standard library (no fontTools on SteamOS).

    python3 fontcheck.py path/to/Font.ttf [more.ttf ...]
"""
import struct
import sys

NEEDED = "0123456789 .,:%+-\u2212\u2014\u2013\u00b7\u2026\u203a\u2039\u00d7\u00b0/()'\"?!&"


def tables(data):
    count = struct.unpack(">H", data[4:6])[0]
    out = {}
    for index in range(count):
        tag, _sum, offset, length = struct.unpack(">4sIII", data[12 + 16 * index:28 + 16 * index])
        out[tag.decode()] = (offset, length)
    return out


def cmap(data, tabs):
    base = tabs["cmap"][0]
    count = struct.unpack(">H", data[base + 2:base + 4])[0]
    best = None
    for index in range(count):
        platform, encoding, offset = struct.unpack(">HHI", data[base + 4 + 8 * index:base + 12 + 8 * index])
        fmt = struct.unpack(">H", data[base + offset:base + offset + 2])[0]
        if fmt == 4 and platform in (0, 3):
            best = base + offset
    if best is None:
        raise SystemExit("no format 4 cmap")
    seg = struct.unpack(">H", data[best + 6:best + 8])[0] // 2
    ends = struct.unpack(f">{seg}H", data[best + 14:best + 14 + 2 * seg])
    start_at = best + 16 + 2 * seg
    starts = struct.unpack(f">{seg}H", data[start_at:start_at + 2 * seg])
    delta_at = start_at + 2 * seg
    deltas = struct.unpack(f">{seg}h", data[delta_at:delta_at + 2 * seg])
    range_at = delta_at + 2 * seg
    ranges = struct.unpack(f">{seg}H", data[range_at:range_at + 2 * seg])
    mapping = {}
    for index in range(seg):
        for code in range(starts[index], ends[index] + 1):
            if code == 0xFFFF:
                continue
            if ranges[index] == 0:
                glyph = (code + deltas[index]) & 0xFFFF
            else:
                at = range_at + 2 * index + ranges[index] + 2 * (code - starts[index])
                glyph = struct.unpack(">H", data[at:at + 2])[0]
                if glyph:
                    glyph = (glyph + deltas[index]) & 0xFFFF
            if glyph:
                mapping[code] = glyph
    return mapping


def advances(data, tabs):
    hhea = tabs["hhea"][0]
    count = struct.unpack(">H", data[hhea + 34:hhea + 36])[0]
    hmtx = tabs["hmtx"][0]
    return [struct.unpack(">H", data[hmtx + 4 * i:hmtx + 4 * i + 2])[0] for i in range(count)]


def main():
    for path in sys.argv[1:]:
        data = open(path, "rb").read()
        tabs = tables(data)
        head = tabs["head"][0]
        upm = struct.unpack(">H", data[head + 18:head + 20])[0]
        mapping = cmap(data, tabs)
        adv = advances(data, tabs)

        def width(char):
            glyph = mapping.get(ord(char))
            if glyph is None:
                return None
            return adv[min(glyph, len(adv) - 1)]

        digits = {d: width(d) for d in "0123456789"}
        tabular = len(set(digits.values())) == 1
        missing = [f"U+{ord(c):04X}" for c in NEEDED if width(c) is None]
        os2 = tabs["OS/2"][0]
        cap = struct.unpack(">h", data[os2 + 88:os2 + 90])[0]
        xh = struct.unpack(">h", data[os2 + 86:os2 + 88])[0]
        print(path)
        print(f"  unitsPerEm {upm}  capHeight {cap}  xHeight {xh}")
        print(f"  digit advances {sorted(set(digits.values()))}  tabular by default: {tabular}")
        for char in " .,:%+-\u2212\u2014\u00b7":
            print(f"  U+{ord(char):04X} advance {width(char)}")
        print(f"  missing: {missing or 'none'}")


if __name__ == "__main__":
    main()
