"""Title screen and the TV dashboard web page."""
import hud as H
import menus as m
import pp_ui as u
from pp_ui import (BAD, DUSK, GOOD, INK, MIST, NIGHT, RAMP, SLATE, SNOW, SUN, WARN, ZONES, caption, circle, cw, line,
                   poly, rect, text, tw)

TITLE_DEFS = (
    '<linearGradient id="left-scrim" x1="0" y1="0" x2="1" y2="0">'
    f'<stop offset="0" stop-color="{INK}" stop-opacity="0.86"/>'
    f'<stop offset="0.6" stop-color="{INK}" stop-opacity="0.5"/>'
    f'<stop offset="1" stop-color="{INK}" stop-opacity="0"/></linearGradient>'
    '<linearGradient id="foot-scrim" x1="0" y1="0" x2="0" y2="1">'
    f'<stop offset="0" stop-color="{INK}" stop-opacity="0"/>'
    f'<stop offset="1" stop-color="{INK}" stop-opacity="0.7"/></linearGradient>'
)

MENU_X, MENU_Y, MENU_W, ITEM_H, ITEM_PITCH = 72, 284, 400, 64, 68


def trainer_state(x, base, state, name, fg):
    """Trainer state, set after the menu label: a mark and words."""
    if state == "connected":
        return circle(x + 7, base - 7, 7, INK if fg == INK else GOOD) + text(x + 24, base, name, 20, 500, fg)
    if state == "searching":
        return u.spinner(x + 8, base - 7, 7, fg, 2.5) + text(x + 26, base, "Searching", 20, 500, fg)
    return circle(x + 7, base - 7, 5.5, "none", None, fg, 2) + text(x + 24, base, "Not paired", 20, 500, fg)


def title_menu(focus=0, trainer=("connected", "Tacx Neo 2T"), x=MENU_X, y=MENU_Y):
    out = ""
    for i, label in enumerate(("Start ride", "Trainer", "Settings", "Quit")):
        iy = y + i * ITEM_PITCH
        focused = i == focus
        if focused:
            out += rect(x, iy, MENU_W, ITEM_H, 14, SUN)
        fg, fg2 = (INK, INK) if focused else (SNOW, MIST)
        base = iy + ITEM_H / 2.0 + 11
        out += text(x + 28, base, label, 32, 700 if focused else 600, fg)
        if i == 1:
            out += trainer_state(x + 28 + tw(label, 32, 700) + 20, base - 2, trainer[0], trainer[1],
                                 INK if focused else SNOW)
    return out


def course_card(course, x=MENU_X, y=600):
    w, h = MENU_W, 96
    out = rect(x, y, w, h, 12, INK, 0.72) + rect(x + 0.5, y + 0.5, w - 1, h - 1, 11.5, "none", None, SNOW, 0.14, 1)
    out += caption(x + 28, y + 32, "Course")
    out += text(x + 28, y + 60, course.name, 24, 600)
    out += text(x + 28, y + 82, f"{course.length / 1000:.1f} km loop \u00b7 {course.ascent:.0f} m ascent", 18, 500, MIST)
    cw_, ch_ = 150, 40
    cx, cy = x + w - 28 - cw_, y + 30
    chart, _ = u.profile_chart(course, cx, cy, cw_, ch_, 0, course.length, 3, 60, SNOW, 0.92,
                               floor=course.low - 25, sw=1.5)
    out += chart + line(cx, cy + ch_ + 0.5, cx + cw_, cy + ch_ + 0.5, SNOW, 1, 0.3)
    return out


def wordmark(x, y, size=112):
    """Brand lockup: the badge, then PENGUIN / PASS on two lines."""
    out = u.logo(x, y, size)
    tx = x + size + 24
    fs = size * 0.46
    out += text(tx, y + size * 0.44, "PENGUIN", fs, 700, SNOW, tracking=0.03)
    out += text(tx, y + size * 0.44 + fs * 0.98, "PASS", fs, 700, SUN, tracking=0.03)
    return out


def title_frame(course, focus=0, trainer=("connected", "Tacx Neo 2T"), keyboard=False):
    """Everything on the title screen that lives in the 1280 x 800 frame."""
    out = wordmark(72, 80, 120)
    out += title_menu(focus, trainer)
    out += course_card(course)
    out += text(72, 761, "v0.1.0", 18, 500, MIST)
    out += m.hints([("ud", "Move"), ("A", "Select")], keyboard)
    return out


def title(course, plate="switchback-chase", **kw):
    out = u.plate(plate)
    out += f'<rect x="0" y="0" width="720" height="800" fill="url(#left-scrim)"/>'
    out += f'<rect x="0" y="640" width="1280" height="160" fill="url(#foot-scrim)"/>'
    out += title_frame(course, **kw)
    return out


def title_1080(course, plate="tv-1080", **kw):
    # The title camera keeps the rider right of the menu. The ride shot has
    # the rider dead centre, so it is enlarged 1.15 times and shifted right
    # here to stand in for that framing.
    out = u.plate(plate, 2208, 1242, 0, -124)
    out += f'<rect x="0" y="0" width="1068" height="1080" fill="url(#left-scrim)"/>'
    out += f'<rect x="0" y="864" width="1920" height="216" fill="url(#foot-scrim)"/>'
    out += u.group(title_frame(course, **kw), "translate(96 0) scale(1.35)")
    return out


# ---------------------------------------------------------- TV dashboard ---

def tv_ladder(x, y, zi, w, seg, gap):
    out = ""
    for i in range(7):
        sy = y + (6 - i) * (seg + gap)
        active = i == zi
        out += rect(x, sy, w + (w * 0.4 if active else 0), seg, seg * 0.18, RAMP[i], None if active else 0.34)
    return out


def tv_status(right, cy, state, size=28):
    label = {"live": "Live", "reconnecting": "Reconnecting"}[state]
    color = GOOD if state == "live" else WARN
    w = tw(label, size, 600)
    r = size * 0.32
    out = text(right, cy + size * 0.35, label, size, 600, color, "end")
    if state == "live":
        out += circle(right - w - r - 10, cy, r, color)
    else:
        out += u.spinner(right - w - r - 12, cy, r, color, 3)
    return out


def tv_dashboard(course, s, state="live"):
    """The web page at 1920 x 1080: no 3D view, numbers for the sofa."""
    W, Hh = 1920, 1080
    out = rect(0, 0, W, Hh, 0, INK)
    mx = 72
    # header
    out += u.logo(mx, 44, 72, "tvlogo")
    out += text(mx + 96, 92, "Penguin Pass", 44, 700)
    out += text(mx + 96 + tw("Penguin Pass", 44, 700) + 24, 92, "5.0 km loop", 28, 500, MIST)
    out += tv_status(W - mx, 80, state)
    # stats in the header, right of centre
    hx = W - mx - 260
    for label, value, unit in (("Lap", str(s["lap"]), ""), ("Elev", f"{s['elevation']:.0f}", "m"),
                               ("Dist", f"{s['distance'] / 1000:.2f}", "km"), ("Time", u.fmt_clock(s["elapsed"]), "")):
        part, width = H.value_unit(hx, 96, value, unit, 44, 600, SNOW, 28, 8, MIST, "end")
        out += part
        hx -= width + 14
        out += caption(hx, 96, label, MIST, "end", 24)
        hx -= cw(label, 24) + 48
    dim = 0.4 if state == "reconnecting" else None
    # power
    px, py, pw, ph = mx, 148, 760, 548
    out += rect(px, py, pw, ph, 24, NIGHT)
    zi = u.zone_index(s["power"], s["ftp"])
    out += tv_ladder(px + 48, py + 56, zi, 44, 54, 8)
    body = text(px + 152, py + 366, str(s["power"]), 330, 700)
    body += text(px + 152 + tw(str(s["power"]), 330, 700) + 20, py + 366, "W", 72, 500, MIST)
    out += u.group(body, None, dim)
    out += caption(px + 152, py + 452, f"{ZONES[zi][0]} {ZONES[zi][1]}", RAMP[zi], "start", 48)
    out += caption(px + 152, py + 500, "Power, 3 s average", MIST, "start", 24)
    # four tiles
    tx0, tw_, th_, gap = px + pw + 24, 0, 262, 24
    tw_ = (W - mx - tx0 - gap) / 2.0
    grade = s["grade"]
    sign = "+" if grade >= 0.05 else "\u2212" if grade <= -0.05 else ""
    tiles = [(f"{s['speed']:.1f}", "km/h", SNOW), (f"{sign}{abs(grade):.1f}", "% grade", u.grade_color(grade)),
             (str(s["cadence"]), "rpm", SNOW), (None if s["hr"] is None else str(s["hr"]), "bpm", SNOW)]
    for i, (value, label, fill) in enumerate(tiles):
        tx = tx0 + (i % 2) * (tw_ + gap)
        ty = py + (i // 2) * (th_ + gap)
        out += rect(tx, ty, tw_, th_, 24, NIGHT)
        if value is None:
            out += text(tx + 40, ty + 168, "\u2014", 160, 700, DUSK) + text(tx + 40, ty + 226, label, 36, 500, DUSK)
        else:
            out += u.group(text(tx + 40, ty + 168, value, 160, 700, fill), None, dim)
            out += text(tx + 40, ty + 226, label, 36, 500, MIST)
    # course
    cy0 = py + ph + 24
    ch = Hh - 56 - cy0
    out += rect(mx, cy0, W - 2 * mx, ch, 24, NIGHT)
    lap_d = s["distance"] % course.length
    zoom_w = 520
    sx, sw_ = mx + 40, W - 2 * mx - 80 - zoom_w - 56
    base = cy0 + ch - 64
    sky_h = ch - 64 - 56
    floor = course.low - 0.15 * (course.top - course.low)
    span = course.top - floor
    chart, py_ = u.profile_chart(course, sx, base - sky_h, sw_, sky_h, 0, course.length, 6, 60, SNOW, 0.95, lap_d,
                                 floor=floor, sw=3)
    out += chart
    out += line(sx, base + 1, sx + sw_, base + 1, SNOW, 2, 0.3)
    mxp = sx + sw_ * lap_d / course.length
    myp = py_(course.at(lap_d)[0])
    out += line(mxp, base, mxp, myp, SNOW, 4) + circle(mxp, myp, 13, SNOW, None, INK, 5)
    out += caption(sx, cy0 + 44, "Course", MIST, "start", 24)
    for i in range(6):
        tx = sx + sw_ * i / 5.0
        out += text(tx, base + 40, f"{course.length / 1000 * i / 5.0:.0f} km", 24, 500, MIST,
                    "start" if i == 0 else "end" if i == 5 else "middle")
    zx = sx + sw_ + 56
    out += caption(zx, cy0 + 44, "Next 1 km", MIST, "start", 24)
    chart, py2 = u.profile_chart(course, zx, base - sky_h, zoom_w, sky_h, lap_d, lap_d + 1000, 6, 60, SNOW, 0.95,
                                 wrap=True, sw=3)
    out += chart + line(zx, base + 1, zx + zoom_w, base + 1, SNOW, 2, 0.3)
    for i in (1, 2, 3):
        out += line(zx + zoom_w * i / 4.0, base - sky_h, zx + zoom_w * i / 4.0, base, INK, 3, 0.5)
    out += circle(zx + 2, py2(course.at(lap_d)[0]), 13, SNOW, None, INK, 5)
    for i, label in enumerate(("now", "500 m", "1 km")):
        out += text(zx + zoom_w * i / 2.0, base + 40, label, 24, 500, MIST, ("start", "middle", "end")[i])
    return out


def tv_phone(course, s, state="live"):
    """The same page on a phone held upright: 390 x 844 CSS px."""
    W, Hh = 390, 844
    out = rect(0, 0, W, Hh, 0, INK)
    mx = 16
    out += u.logo(mx, 52, 36, "phlogo") + text(mx + 46, 78, "Penguin Pass", 22, 700)
    out += tv_status(W - mx, 70, state, 18)
    dim = 0.4 if state == "reconnecting" else None
    # power
    py = 104
    out += rect(mx, py, W - 2 * mx, 196, 16, NIGHT)
    zi = u.zone_index(s["power"], s["ftp"])
    out += tv_ladder(mx + 18, py + 22, zi, 16, 19, 3)
    body = text(mx + 58, py + 128, str(s["power"]), 128, 700) + text(mx + 58 + tw(str(s["power"]), 128, 700) + 8, py + 128, "W", 30, 500, MIST)
    out += u.group(body, None, dim)
    out += caption(mx + 58, py + 170, f"{ZONES[zi][0]} {ZONES[zi][1]}", RAMP[zi], "start", 22)
    # tiles
    ty0 = py + 196 + 12
    tw_ = (W - 2 * mx - 12) / 2.0
    grade = s["grade"]
    sign = "+" if grade >= 0.05 else "\u2212" if grade <= -0.05 else ""
    tiles = [(f"{s['speed']:.1f}", "km/h", SNOW), (f"{sign}{abs(grade):.1f}", "% grade", u.grade_color(grade)),
             (str(s["cadence"]), "rpm", SNOW), (None if s["hr"] is None else str(s["hr"]), "bpm", SNOW)]
    for i, (value, label, fill) in enumerate(tiles):
        tx = mx + (i % 2) * (tw_ + 12)
        ty = ty0 + (i // 2) * (108 + 12)
        out += rect(tx, ty, tw_, 108, 16, NIGHT)
        if value is None:
            out += text(tx + 18, ty + 64, "\u2014", 56, 700, DUSK) + text(tx + 18, ty + 92, label, 18, 500, DUSK)
        else:
            out += u.group(text(tx + 18, ty + 64, value, 56, 700, fill), None, dim) + text(tx + 18, ty + 92, label, 18, 500, MIST)
    # course
    cy0 = ty0 + 2 * 120
    ch = 168
    out += rect(mx, cy0, W - 2 * mx, ch, 16, NIGHT)
    lap_d = s["distance"] % course.length
    sx, sw_ = mx + 18, W - 2 * mx - 36
    base = cy0 + ch - 44
    floor = course.low - 0.15 * (course.top - course.low)
    chart, py_ = u.profile_chart(course, sx, base - 72, sw_, 72, 0, course.length, 3, 60, SNOW, 0.95, lap_d, floor=floor,
                                 sw=1.5)
    out += caption(sx, cy0 + 30, "Course", MIST, "start", 16)
    out += chart + line(sx, base + 0.5, sx + sw_, base + 0.5, SNOW, 1, 0.3)
    mxp, myp = sx + sw_ * lap_d / course.length, py_(course.at(lap_d)[0])
    out += line(mxp, base, mxp, myp, SNOW, 2) + circle(mxp, myp, 6, SNOW, None, INK, 2.5)
    out += text(sx, base + 28, "0 km", 16, 500, MIST) + text(sx + sw_, base + 28, f"{course.length / 1000:.0f} km", 16, 500, MIST, "end")
    # ride line
    fy = cy0 + ch + 12
    out += rect(mx, fy, W - 2 * mx, 76, 16, NIGHT)
    cells = (("Time", u.fmt_clock(s["elapsed"])), ("Dist", f"{s['distance'] / 1000:.2f} km"),
             ("Elev", f"{s['elevation']:.0f} m"), ("Lap", str(s["lap"])))
    xs = [mx + 18, mx + 116, mx + 214, mx + 300]
    for (label, value), cx in zip(cells, xs):
        out += caption(cx, fy + 30, label, MIST, "start", 16) + text(cx, fy + 58, value, 22, 600)
    return out
