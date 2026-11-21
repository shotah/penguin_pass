"""The ride HUD (the chosen concept: the bottom console) as SVG.

Geometry constants here are the ones tabulated in docs/ui.md. All numbers
are in the 1280 x 800 design frame.
"""
import pp_ui as u
from pp_ui import (BAD, DUSK, GOOD, INK, MIST, RAMP, SLATE, SNOW, SUN, WARN, ZONES, caption, circle, cw, line,
                   panel, poly, rect, text, tw)

M = 32                      # safe margin
CON_X, CON_W = M, 1280 - 2 * M
CON_H = 124
CON_Y = 800 - M - CON_H     # 644
NUM_Y = 76                  # baseline of every main number, from the console top
LAB_Y = 104                 # baseline of every label under a number
SKY_H = 30                  # course skyline height above the console
PAD = 20


def value_unit(x, baseline, value, unit, size=48, weight=700, fill=SNOW, unit_size=20, gap=6, unit_fill=MIST,
               anchor="start"):
    """A number with its unit on the same baseline. Returns (svg, width)."""
    vw = tw(value, size, weight)
    uw = tw(unit, unit_size, 500) if unit else 0
    total = vw + (gap + uw if unit else 0)
    x0 = x if anchor == "start" else x - total
    out = text(x0, baseline, value, size, weight, fill)
    if unit:
        out += text(x0 + vw + gap, baseline, unit, unit_size, 500, unit_fill)
    return out, total


def zone_bar(x, y, w, zi, h=8, gap=4, live=True):
    """Seven segments, Z1 to Z7 left to right. The current one is solid and
    4 px taller; the rest are the same colours at 30 %."""
    out = ""
    seg = (w - gap * 6) / 7.0
    for i in range(7):
        active = live and i == zi
        out += rect(x + i * (seg + gap), y - (4 if active else 0), seg, h + (4 if active else 0), 2, RAMP[i],
                    None if active else 0.30)
    return out


def chip(x, y, parts, anchor="start", h=36, track=True):
    """Pill with an icon and text runs. parts: list of ("icon", fn, width) or
    ("text", string, size, weight, fill). Returns (svg, width)."""
    width = 14
    for part in parts:
        width += (part[2] if part[0] == "icon" else tw(part[1], part[2], part[3])) + 8
    width += 14 - 8
    x0 = x if anchor == "start" else x - width
    out = panel(x0, y, width, h, h / 2.0, 0.88, 0.14, track)
    cx = x0 + 14
    for part in parts:
        if part[0] == "icon":
            out += part[1](cx, y + h / 2.0)
            cx += part[2] + 8
        else:
            _, s, size, weight, fill = part
            out += text(cx, y + h / 2.0 + size * 0.35, s, size, weight, fill)
            cx += tw(s, size, weight) + 8
    return out, width


def trainer_chip(x, y, state, name="Tacx Neo 2T", anchor="start", track=True):
    """Connection chip. Each state has its own mark, not only its own colour:
    solid disc (controlled), ring (reading only), broken ring (reconnecting),
    keyboard (simulated rider)."""
    if state == "controlled":
        icon = ("icon", lambda cx, cy: circle(cx + 7, cy, 7, GOOD), 14)
        parts = [icon, ("text", name, 20, 600, SNOW), ("text", "Resistance on", 18, 500, MIST)]
    elif state == "reading":
        icon = ("icon", lambda cx, cy: circle(cx + 7, cy, 5.5, "none", None, GOOD, 3), 14)
        parts = [icon, ("text", name, 20, 600, SNOW), ("text", "Reading only", 18, 500, MIST)]
    elif state == "reconnecting":
        icon = ("icon", lambda cx, cy: u.spinner(cx + 8, cy, 7, WARN, 3), 16)
        parts = [icon, ("text", name, 20, 600, SNOW), ("text", "Reconnecting", 18, 500, WARN)]
    else:
        icon = ("icon", lambda cx, cy: u.icon_keyboard(cx, cy - 11, 22, MIST), 22)
        parts = [icon, ("text", "Simulated rider", 20, 600, SNOW)]
    return chip(x, y, parts, anchor, 36, track)


def tv_chip(x, y, address, anchor="end", track=True):
    parts = [("icon", lambda cx, cy: u.icon_tv(cx, cy - 10, 20, MIST), 20), ("text", address, 18, 500, MIST)]
    return chip(x, y, parts, anchor, 36, track)


def menu_hint(x, y, keyboard=False, anchor="end", track=True):
    if keyboard:
        icon = ("icon", lambda cx, cy: u.keycap(cx, cy, "Esc", 26, MIST, 18)[0], u.keycap(0, 0, "Esc", 26, MIST, 18)[1])
    else:
        icon = ("icon", lambda cx, cy: u.pad_menu(cx + 16, cy, 32, 22, MIST), 32)
    return chip(x, y, [icon, ("text", "Menu", 18, 500, MIST)], anchor, 36, track)


def zone_ladder(x, y, zi, live=True, w=16, seg=12, gap=2):
    """Seven stacked segments, Z1 at the bottom, 96 px tall in all. The
    current zone is solid and 6 px wider; the others are the same colours at
    34 %, so the order stays visible."""
    out = ""
    for i in range(7):
        sy = y + (6 - i) * (seg + gap)
        active = live and i == zi
        out += rect(x, sy, w + (6 if active else 0), seg, 2, RAMP[i], None if active else 0.34)
    return out


RIDDEN = u.RIDDEN


def skyline(course, x, y, w, h, lap_d, total=None):
    """Whole-course profile standing on the console's top edge (y is that
    edge). Ridden part grey, road ahead in gradient colours, the next
    kilometre under a heavier top line. `total` overrides the span shown
    (a point-to-point route); default is one lap."""
    length = total or course.length
    columns = int(w / 4)
    floor = course.low - 0.22 * max(course.top - course.low, 60.0)
    span = max(course.top - floor, 60.0)

    def at(d):
        return course.at(min(max(d, 0.0), course.length - 0.01))[0]

    def px(d):
        return x + w * d / length

    def py(e):
        return y - (e - floor) / span * h

    pts = [(px(length * i / columns), py(at(length * i / columns))) for i in range(columns + 1)]
    mx, my = px(lap_d), py(at(lap_d))
    out = poly([(x, y)] + pts + [(x + w, y)], INK, 0.88, INK, 5)
    # ridden: one grey shape up to the rider
    done = [p for p in pts if p[0] < mx] + [(mx, my)]
    if len(done) > 1:
        out += poly([(x, y)] + done + [(mx, y)], RIDDEN)
    # ahead: one column per 4 px, coloured by its mean grade
    for i in range(columns):
        d0, d1 = length * i / columns, length * (i + 1) / columns
        if d1 <= lap_d:
            continue
        d0 = max(d0, lap_d)
        grade = (at(d1) - at(d0)) / max(d1 - d0, 1e-6) * 100.0
        out += poly([(px(d0), y), (px(d0), py(at(d0))), (px(d1) + 0.4, py(at(d1))), (px(d1) + 0.4, y)],
                    u.grade_color(grade))
    out += u.polyline(pts, SNOW, 1.25, 0.75)
    near_end = min(lap_d + 1000.0, length)
    near = [(mx, my)] + [p for p in pts if mx < p[0] < px(near_end)] + [(px(near_end), py(at(near_end)))]
    out += u.polyline(near, SNOW, 3)
    out += line(mx, y, mx, my, INK, 5) + line(mx, y, mx, my, SNOW, 2)
    out += circle(mx, my, 6, SNOW, None, INK, 2.5)
    return out


def console(s, course):
    """The instrument. s: ride state dict (see gen.py STATES).

    Left to right: power (zone ladder, hero number, zone name), body
    (cadence, heart), road (speed, grade, next kilometre), ride (time,
    distance, elevation, lap). The course skyline stands on the top edge.
    """
    x, y, w, h = CON_X, CON_Y, CON_W, CON_H
    lap_d = s["distance"] % course.length if s["distance"] else 0.0
    out = skyline(course, x + 12, y, w - 24, SKY_H, lap_d, s.get("route_length"))
    u.AREA.append((x + 12, y - SKY_H * 0.6, w - 24, SKY_H * 0.6))
    out += panel(x, y, w, h, 12)
    num_y, lab_y = y + NUM_Y, y + LAB_Y
    power = s["power"]
    coasting = power is not None and power <= 0
    zi = u.zone_index(power, s["ftp"]) if power else 0

    # --- power -------------------------------------------------------------
    out += zone_ladder(x + PAD, y + 14, zi, live=bool(power))
    hx = x + PAD + 16 + 20
    if power is None:
        out += text(hx, num_y, "\u2014", 88, 700, DUSK)
        out += caption(hx, lab_y, "No signal", WARN)
    else:
        part, _ = value_unit(hx, num_y, str(power), "W", 88, 700, SNOW, 24, 8)
        out += part
        label = "Coasting" if coasting else f"{ZONES[zi][0]} {ZONES[zi][1]}"
        out += caption(hx, lab_y, label, MIST if coasting else RAMP[zi])
    div = x + 280
    out += line(div + 0.5, y + 16, div + 0.5, y + h - 16, SNOW, 1, 0.14)

    # --- body: cadence, heart; road: speed, grade ---------------------------
    def cell(cx, value, label, fill=SNOW):
        if value is None:
            return text(cx, num_y, "\u2014", 52, 700, DUSK) + text(cx, lab_y, label, 18, 500, DUSK)
        return text(cx, num_y, value, 52, 700, fill) + text(cx, lab_y, label, 18, 500, MIST)

    cx = div + 24
    out += cell(cx, None if s["cadence"] is None else str(s["cadence"]), "rpm")
    cx += 116
    out += cell(cx, None if s["hr"] is None else str(s["hr"]), "bpm")
    cx += 116
    out += line(cx - 11.5, y + 16, cx - 11.5, y + h - 16, SNOW, 1, 0.14)
    cx += 12
    speed = s["speed"]
    out += cell(cx, None if speed is None else (f"{speed:.1f}" if speed < 99.95 else f"{speed:.0f}"), "km/h")
    cx += 132
    grade = s["grade"]
    sign = "+" if grade >= 0.05 else "\u2212" if grade <= -0.05 else ""
    out += cell(cx, f"{sign}{abs(grade):.1f}", "% grade", u.grade_color(grade))
    cx += 148

    # --- next kilometre ------------------------------------------------------
    list_x = x + w - PAD - 150
    chart_w = list_x - 28 - cx
    top, bottom = y + 20, num_y + 2
    chart, py = u.profile_chart(course, cx, top, chart_w, bottom - top, lap_d, lap_d + 1000.0, 4, 60, SNOW, 0.92,
                                wrap=True)
    out += chart
    for i in (1, 2, 3):
        tx = cx + chart_w * i / 4.0
        out += line(tx, top, tx, bottom, INK, 1.5, 0.5)
    out += line(cx, bottom + 0.5, cx + chart_w, bottom + 0.5, SNOW, 1, 0.3)
    out += circle(cx + 1, py(course.at(lap_d)[0]), 5, SNOW, None, INK, 2)
    out += text(cx, lab_y, "next 1 km", 18, 500, MIST)
    out += line(list_x - 14.5, y + 16, list_x - 14.5, y + h - 16, SNOW, 1, 0.14)

    # --- ride: time, distance, elevation, lap --------------------------------
    right = x + w - PAD
    km = s["distance"] / 1000.0
    last = ("Lap", str(s["lap"]), "") if s["lap"] is not None else \
        ("Left", f"{(course.length - s['distance']) / 1000.0:.1f}", "km")
    rows = (("Time", u.fmt_clock(s["elapsed"]), ""), ("Dist", f"{km:.2f}" if km < 99.995 else f"{km:.1f}", "km"),
            ("Elev", f"{s['elevation']:.0f}", "m"), last)
    for i, (cap, value, unit) in enumerate(rows):
        base = lab_y - (3 - i) * 25
        out += caption(list_x, base, cap)
        part, _ = value_unit(right, base, value, unit, 22, 600, SNOW, 18, 4, MIST, "end")
        out += part
    return out


def hud(s, course, hint=None):
    """Chips along the top, console along the bottom."""
    out = ""
    part, _ = trainer_chip(M, M, s.get("trainer", "controlled"), s.get("trainer_name", "Tacx Neo 2T"))
    out += part
    part, width = menu_hint(1280 - M, M, s.get("keyboard", False))
    out += part
    if s.get("tv"):
        part, _ = tv_chip(1280 - M - width - 8, M, s["tv"])
        out += part
    out += console(s, course)
    return out


def power_hint(keyboard=True):
    """Shown only with the simulated rider: how to change its power. Sits
    above the power end of the console, clear of the skyline."""
    y = CON_Y - SKY_H - 12 - 40
    if keyboard:
        up, w1 = u.key_arrow(0, 0, "up", 26, SNOW)
        parts = [("icon", lambda cx, cy: u.key_arrow(cx, cy, "up", 26, SNOW)[0] + u.key_arrow(cx + 30, cy, "down", 26, SNOW)[0], 56)]
    else:
        parts = [("icon", lambda cx, cy: u.pad_dpad(cx + 13, cy, 26, ("up", "down"), SNOW), 26)]
    parts.append(("text", "Power", 20, 600, SNOW))
    parts.append(("text", "\u00b110 W", 18, 500, MIST))
    out, _ = chip(M, y, parts, "start", 40)
    return out
