#!/usr/bin/env python3
"""Make HUD-free background plates from the game screenshots.

The shots under docs/img/shots/ already carry the old HUD (two translucent
panels with white text and the old elevation strip). A new HUD mocked on top
of that would be judged against ghost text. This script removes the old HUD
so the mockups sit on the scene alone:

1. The panels are `StyleBoxFlat` fills of a known colour and opacity, so the
   scene behind them is recovered exactly by inverting the blend.
2. The elevation strip's green fill is inverted the same way.
3. Text, lines, panel borders and corners cannot be inverted. They are
   masked and filled from the nearest clean pixels in eight directions.

Reads docs/img/shots/*.png (never written). Writes
docs/img/ui/src/plates/<name>.png. Needs python3 with pycairo, which SteamOS
ships. No other packages.

    python3 docs/img/ui/tools/clean_plates.py
"""
import os
import sys

import cairo

HERE = os.path.dirname(os.path.abspath(__file__))
SHOTS = os.path.normpath(os.path.join(HERE, "..", "..", "shots"))
PLATES = os.path.normpath(os.path.join(HERE, "..", "src", "plates"))

# From crates/pp-godot/src/hud.rs (ride_theme and paint).
PANEL_RGB = (0.04 * 255, 0.06 * 255, 0.09 * 255)
PANEL_ALPHA = 0.58
FILL_RGB = (0.45 * 255, 0.72 * 255, 0.55 * 255)
FILL_ALPHA = 0.55

# The standard set written by godot/tests/shoot_set.sh. Other pictures in
# docs/img/shots/ (menus) are not plain ride frames and are left alone.
STANDARD = ["start-chase", "start-fp", "switchback-chase", "switchback-fp", "climb-chase", "climb-fp",
            "summit-chase", "summit-fp", "descent-chase", "descent-fp", "tv-1080"]

DIRECTIONS = [(1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (1, -1), (-1, 1), (-1, -1)]


def clamp(value):
    return 0 if value < 0 else 255 if value > 255 else int(value + 0.5)


def clean(src, dst):
    surface = cairo.ImageSurface.create_from_png(src)
    width, height = surface.get_width(), surface.get_height()
    stride = surface.get_stride()
    data = bytearray(surface.get_data())  # B, G, R, X per pixel
    mask = bytearray(width * height)

    def get(x, y):
        at = y * stride + x * 4
        return data[at + 2], data[at + 1], data[at]

    def put(x, y, rgb):
        at = y * stride + x * 4
        data[at + 2], data[at + 1], data[at] = clamp(rgb[0]), clamp(rgb[1]), clamp(rgb[2])

    left, right = 16, width - 17
    top_panel = (left, 10, right, 156)
    bottom_panel = (left, height - 94, right, height - 11)
    baseline = height - 49  # last row of the old strip's green fill

    # Old strip: find the white profile line per column, then smooth it so
    # the marker's vertical line does not count as profile.
    strip_top = bottom_panel[1] + 3
    tops = {}

    def strip_bright(rgb):
        return rgb[0] > 135 or rgb[1] > 165 or rgb[2] > 145

    for x in range(left + 3, right - 2):
        for y in range(strip_top, baseline + 1):
            if strip_bright(get(x, y)):
                tops[x] = y
                break
    smooth = {}
    columns = sorted(tops)
    for x in columns:
        window = sorted(tops[c] for c in range(x - 40, x + 41) if c in tops)
        smooth[x] = window[len(window) // 2]

    fill_marked = bytearray(width * height)
    for x in columns:
        for y in range(smooth[x], baseline + 1):
            fill_marked[y * width + x] = 1
        # Everything bright in this column of the strip is line or marker.
        for y in range(strip_top, baseline + 2):
            if strip_bright(get(x, y)):
                mask[y * width + x] = 1

    for panel in (top_panel, bottom_panel):
        x0, y0, x1, y1 = panel
        for y in range(y0, y1 + 1):
            for x in range(x0, x1 + 1):
                rgb = get(x, y)
                index = y * width + x
                edge = min(x - x0, x1 - x, y - y0, y1 - y)
                corner = (min(x - x0, x1 - x) < 10) and (min(y - y0, y1 - y) < 10)
                if edge < 3 or corner:
                    mask[index] = 1
                    continue
                if fill_marked[index]:
                    rgb = [(rgb[c] - FILL_ALPHA * FILL_RGB[c]) / (1.0 - FILL_ALPHA) for c in range(3)]
                elif max(rgb) > 126:
                    mask[index] = 1
                    continue
                scene = [(rgb[c] - PANEL_ALPHA * PANEL_RGB[c]) / (1.0 - PANEL_ALPHA) for c in range(3)]
                put(x, y, scene)

    # Grow the mask by 2 px so antialiased fringes go with their glyphs.
    for _ in range(2):
        grown = bytearray(mask)
        for panel in (top_panel, bottom_panel):
            x0, y0, x1, y1 = panel
            for y in range(y0, y1 + 1):
                row = y * width
                for x in range(x0, x1 + 1):
                    if mask[row + x]:
                        continue
                    if mask[row + x - 1] or mask[row + x + 1] or mask[row + x - width] or mask[row + x + width]:
                        grown[row + x] = 1
        mask = grown
    # The fill's upper edge is antialiased against the line: mask it too.
    for x in columns:
        for y in (smooth[x] - 1, smooth[x], smooth[x] + 1, baseline, baseline + 1):
            mask[y * width + x] = 1

    # Fill masked pixels from the nearest clean pixel in eight directions.
    filled = {}
    for panel in (top_panel, bottom_panel):
        x0, y0, x1, y1 = panel
        for y in range(y0 - 2, y1 + 3):
            for x in range(x0 - 2, x1 + 3):
                if not mask[y * width + x]:
                    continue
                total = [0.0, 0.0, 0.0]
                weight_sum = 0.0
                for dx, dy in DIRECTIONS:
                    cx, cy = x, y
                    for step in range(1, 80):
                        cx += dx
                        cy += dy
                        if cx < 0 or cy < 0 or cx >= width or cy >= height:
                            break
                        if not mask[cy * width + cx]:
                            weight = 1.0 / (step * step)
                            rgb = get(cx, cy)
                            total[0] += rgb[0] * weight
                            total[1] += rgb[1] * weight
                            total[2] += rgb[2] * weight
                            weight_sum += weight
                            break
                if weight_sum > 0.0:
                    filled[(x, y)] = [channel / weight_sum for channel in total]
    for (x, y), rgb in filled.items():
        put(x, y, rgb)

    out = cairo.ImageSurface.create_for_data(data, cairo.FORMAT_RGB24, width, height, stride)
    out.write_to_png(dst)
    return width, height, sum(mask)


def main():
    os.makedirs(PLATES, exist_ok=True)
    names = sys.argv[1:] or [f"{name}.png" for name in STANDARD]
    for name in names:
        width, height, masked = clean(os.path.join(SHOTS, name), os.path.join(PLATES, name))
        print(f"{name}: {width}x{height}, {masked} pixels filled")


if __name__ == "__main__":
    main()
