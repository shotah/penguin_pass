#!/usr/bin/env python3
"""Round 1: three ride-HUD layout concepts over the same screenshot.

Writes src/hud-concept-{a,b,c}.svg. Kept as the record of what was compared;
the chosen concept is developed in gen.py.
"""
import os

import pp_ui as u
from pp_ui import (BAD, DUSK, GOOD, INK, MIST, RAMP, SLATE, SNOW, SUN, WARN, ZONES, caption, circle, cw, line,
                   panel, poly, rect, text, tw)

SRC = os.path.join(u.UI, "src")
COURSE = u.penguin_pass()

# The same moment in all three: the 8.6 % ramp at 0.90 km, FTP 250 W.
STATE = dict(plate="climb-chase", power=265, ftp=250, speed=11.9, cadence=78, hr=158, grade=8.6,
             distance=900.0, lap=1, elapsed=175, elevation=40)


def unit_after(x, baseline, value, unit, size, weight=700, fill=SNOW, unit_size=20, gap=6, anchor="start"):
    """Value with a small unit on the same baseline. Returns (svg, width)."""
    vw = tw(value, size, weight)
    uw = tw(unit, unit_size, 500)
    total = vw + gap + uw
    x0 = x if anchor == "start" else x - total if anchor == "end" else x - total / 2.0
    out = text(x0, baseline, value, size, weight, fill)
    out += text(x0 + vw + gap, baseline, unit, unit_size, 500, MIST)
    return out, total


def zone_bar(x, y, w, zi, h=8, gap=3):
    out = ""
    seg = (w - gap * 6) / 7.0
    for i in range(7):
        active = i == zi
        out += rect(x + i * (seg + gap), y - (4 if active else 0), seg, h + (4 if active else 0), 2, RAMP[i],
                    None if active else 0.30)
    return out


def chips(s, y=32):
    out = ""
    label = "Tacx Neo 2T"
    sub = "Resistance on"
    w = 16 + 14 + 10 + tw(label, 20, 600) + 12 + tw(sub, 18, 500) + 16
    out += panel(32, y, w, 36, 18)
    out += circle(32 + 16 + 7, y + 18, 6, GOOD)
    out += text(32 + 40, y + 25, label, 20, 600)
    out += text(32 + 40 + tw(label, 20, 600) + 12, y + 25, sub, 18, 500, MIST)
    tv = "192.168.1.20:7878"
    w2 = 16 + 22 + 8 + tw(tv, 18, 500) + 16
    x2 = 1248 - w2
    out += panel(x2, y, w2, 36, 18)
    out += u.icon_tv(x2 + 14, y + 8, 20, MIST)
    out += text(x2 + 44, y + 24.5, tv, 18, 500, MIST)
    return out


def concept_a(s):
    """Top bar: one band across the top, course ribbon under it."""
    zi = u.zone_index(s["power"], s["ftp"])
    zc = RAMP[zi]
    out = u.plate(s["plate"])
    x, y, w, h = 32, 32, 1216, 150
    out += panel(x, y, w, h)
    base = y + 78
    # left: ride progress
    cx = x + 28
    for cap, value, unit in (("Time", u.fmt_clock(s["elapsed"]), ""), ("Distance", f"{s['distance'] / 1000:.2f}", "km"),
                             ("Lap", str(s["lap"]), "")):
        out += caption(cx, y + 30, cap)
        part, width = unit_after(cx, base - 12, value, unit, 32, 600) if unit else (text(cx, base - 12, value, 32, 600), tw(value, 32, 600))
        out += part
        cx += max(width, cw(cap)) + 28
    # centre: power hero with speed left and cadence, heart right
    mid = x + w / 2.0
    pw = tw(str(s["power"]), 80, 700)
    out += text(mid, base + 6, str(s["power"]), 80, 700, SNOW, "middle")
    out += text(mid + pw / 2.0 + 8, base + 6, "W", 24, 500, MIST)
    out += caption(mid - pw / 2.0 - 12, y + 30, "Power", MIST, "end")
    out += caption(mid + pw / 2.0 + 12, y + 30, f"{ZONES[zi][0]} {ZONES[zi][1]}", zc)
    part, _ = unit_after(mid - pw / 2.0 - 150, base - 6, f"{s['speed']:.1f}", "km/h", 44, 700)
    out += caption(mid - pw / 2.0 - 150, y + 30, "Speed") if False else ""
    out += part
    xr = mid + pw / 2.0 + 64
    part, wdt = unit_after(xr, base - 6, str(s["cadence"]), "rpm", 44, 700)
    out += part
    part, _ = unit_after(xr + wdt + 32, base - 6, str(s["hr"]), "bpm", 44, 700)
    out += part
    # right: road
    rx = x + w - 28
    gc = u.grade_color(s["grade"])
    part, gw = unit_after(rx, base - 12, f"+{s['grade']:.1f}", "%", 32, 600, gc, 20, 6, "end")
    out += part + caption(rx, y + 30, "Grade", MIST, "end")
    part, _ = unit_after(rx - gw - 36, base - 12, str(s["elevation"]), "m", 32, 600, SNOW, 20, 6, "end")
    out += part + caption(rx - gw - 36, y + 30, "Elev", MIST, "end")
    # zone bar under the hero
    out += zone_bar(mid - 150, y + 96, 300, zi)
    # course ribbon along the bottom of the band
    chart, py = u.profile_chart(COURSE, x + 20, y + 112, w - 40, 28, 0, COURSE.length, 4, 60, SNOW, 0.9,
                                s["distance"], sw=1.5)
    out += chart
    mx = x + 20 + (w - 40) * s["distance"] / COURSE.length
    out += line(mx, y + 108, mx, y + 142, SNOW, 2) + circle(mx, py(s["elevation"]), 5, SNOW, None, INK, 2)
    # chips move to the bottom corners
    out += chips(s, 732)
    return out


def concept_b(s):
    """Bottom console: a full-width bike computer under the road."""
    zi = u.zone_index(s["power"], s["ftp"])
    zc = RAMP[zi]
    out = u.plate(s["plate"])
    x, y, w, h = 32, 628, 1216, 140
    out += panel(x, y, w, h)
    # power block
    bx = x + 28
    out += caption(bx, y + 32, "Power")
    out += caption(bx + cw("Power") + 14, y + 32, f"{ZONES[zi][0]} {ZONES[zi][1]}", zc)
    out += text(bx, y + 104, str(s["power"]), 88, 700)
    out += text(bx + tw(str(s["power"]), 88, 700) + 8, y + 104, "W", 24, 500, MIST)
    out += zone_bar(bx, y + 118, 214, zi)
    # speed, cadence, heart
    cx = x + 272
    out += line(cx, y + 20, cx, y + h - 20, SNOW, 1, 0.14)
    cx += 28
    for cap, value, unit, width in (("Speed", f"{s['speed']:.1f}", "km/h", 150), ("Cadence", str(s["cadence"]), "rpm", 116),
                                    ("Heart", str(s["hr"]), "bpm", 116)):
        out += caption(cx, y + 32, cap)
        out += text(cx, y + 92, value, 52, 700)
        out += text(cx, y + 120, unit, 20, 500, MIST)
        cx += width
    # road: next kilometre
    rx = x + 690
    out += line(rx - 20, y + 20, rx - 20, y + h - 20, SNOW, 1, 0.14)
    gc = u.grade_color(s["grade"])
    out += caption(rx, y + 32, "Grade")
    out += text(rx, y + 92, f"+{s['grade']:.1f}", 52, 700, gc)
    out += text(rx, y + 120, "%", 20, 500, MIST)
    cx0, cw0 = rx + 132, 190
    out += caption(cx0, y + 32, "Next 1 km")
    chart, py = u.profile_chart(COURSE, cx0, y + 44, cw0, 78, s["distance"], s["distance"] + 1000, 5, 60)
    out += chart
    out += circle(cx0, py(s["elevation"]), 5, SNOW, None, INK, 2)
    # ride progress
    px = x + 1036
    out += line(px - 20, y + 20, px - 20, y + h - 20, SNOW, 1, 0.14)
    rows = (("Time", u.fmt_clock(s["elapsed"]), ""), ("Dist", f"{s['distance'] / 1000:.2f}", "km"),
            ("Elev", str(s["elevation"]), "m"))
    for i, (cap, value, unit) in enumerate(rows):
        yy = y + 40 + i * 36
        out += caption(px, yy, cap)
        vx = x + w - 28
        if unit:
            part, _ = unit_after(vx, yy + 2, value, unit, 28, 600, SNOW, 18, 5, "end")
            out += part
        else:
            out += text(vx, yy + 2, value, 28, 600, SNOW, "end")
    out += chips(s, 32)
    return out


def concept_c(s):
    """Cockpit: power dial bottom centre, clusters in the corners."""
    zi = u.zone_index(s["power"], s["ftp"])
    zc = RAMP[zi]
    out = u.plate(s["plate"])
    # centre-bottom dial
    cx, cy, r = 640, 712, 92
    out += panel(cx - 128, cy - 112, 256, 168, 24)
    edges = [0.0, 0.55, 0.75, 0.90, 1.05, 1.20, 1.50, 2.0]
    sweep0, sweep = -112.0, 224.0
    for i in range(7):
        a0 = sweep0 + sweep * edges[i] / 2.0 + 1.2
        a1 = sweep0 + sweep * edges[i + 1] / 2.0 - 1.2
        out += u.arc(cx, cy, r, a0, a1, RAMP[i], 12 if i == zi else 7, None if i == zi else 0.38)
    ratio = min(s["power"] / s["ftp"], 2.0)
    import math
    ang = math.radians(sweep0 + sweep * ratio / 2.0 - 90)
    out += circle(cx + r * math.cos(ang), cy + r * math.sin(ang), 8, SNOW, None, INK, 3)
    out += text(cx, cy + 14, str(s["power"]), 72, 700, SNOW, "middle")
    out += text(cx, cy + 40, "W", 20, 500, MIST, "middle")
    out += caption(cx, cy - 48, f"{ZONES[zi][0]} {ZONES[zi][1]}", zc, "middle")
    # bottom left: speed, cadence, heart
    out += panel(32, 664, 400, 104)
    lx = 60
    for cap, value, unit, width in (("Speed", f"{s['speed']:.1f}", "km/h", 150), ("Cadence", str(s["cadence"]), "rpm", 116),
                                    ("Heart", str(s["hr"]), "bpm", 100)):
        out += caption(lx, 696, cap)
        part, _ = unit_after(lx, 748, value, unit, 44, 700, SNOW, 18, 5)
        out += part
        lx += width if False else 0
        lx += {"Speed": 164, "Cadence": 124, "Heart": 0}[cap]
    # bottom right: road
    out += panel(848, 664, 400, 104)
    gc = u.grade_color(s["grade"])
    out += caption(876, 696, "Grade")
    part, gw = unit_after(876, 748, f"+{s['grade']:.1f}", "%", 44, 700, gc, 18, 5)
    out += part
    chart, py = u.profile_chart(COURSE, 1020, 684, 204, 66, s["distance"], s["distance"] + 1000, 5, 60)
    out += chart + circle(1020, py(s["elevation"]), 5, SNOW, None, INK, 2)
    # top left: ride; top right: chips
    out += panel(32, 32, 396, 76)
    tx = 60
    for cap, value, unit in (("Time", u.fmt_clock(s["elapsed"]), ""), ("Dist", f"{s['distance'] / 1000:.2f}", "km"),
                             ("Elev", str(s["elevation"]), "m"), ("Lap", str(s["lap"]), "")):
        out += caption(tx, 62, cap)
        if unit:
            part, width = unit_after(tx, 94, value, unit, 28, 600, SNOW, 18, 4)
        else:
            part, width = text(tx, 94, value, 28, 600), tw(value, 28, 600)
        out += part
        tx += max(width, cw(cap)) + 24
    # chips, right aligned and stacked
    tv = "192.168.1.20:7878"
    w2 = 16 + 22 + 8 + tw(tv, 18, 500) + 16
    out += panel(1248 - w2, 76, w2, 36, 18) + u.icon_tv(1248 - w2 + 14, 84, 20, MIST) + text(1248 - w2 + 44, 100.5, tv, 18, 500, MIST)
    label, sub = "Tacx Neo 2T", "Resistance on"
    w1 = 16 + 14 + 10 + tw(label, 20, 600) + 12 + tw(sub, 18, 500) + 16
    x1 = 1248 - w1
    out += panel(x1, 32, w1, 36, 18) + circle(x1 + 23, 50, 6, GOOD) + text(x1 + 40, 57, label, 20, 600)
    out += text(x1 + 40 + tw(label, 20, 600) + 12, 57, sub, 18, 500, MIST)
    return out


def main():
    for name, build in (("a", concept_a), ("b", concept_b), ("c", concept_c)):
        u.AREA.clear()
        body = build(STATE)
        open(os.path.join(SRC, f"hud-concept-{name}.svg"), "w").write(u.svg(1280, 800, body))
        print(f"hud-concept-{name}: HUD covers {u.coverage() * 100:.1f} % of the frame")


if __name__ == "__main__":
    main()
