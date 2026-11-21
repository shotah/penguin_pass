"""Menu parts and the menu screens: settings, pairing, dialog, toast,
summary, course picker. Geometry is the 1280 x 800 design frame; the
constants are the ones tabulated in docs/ui.md."""
import pp_ui as u
from pp_ui import (BAD, DUSK, GOOD, INK, MIST, NIGHT, RAMP, SLATE, SNOW, SUN, WARN, caption, circle, cw, line,
                   poly, rect, text, tw)

PX, PW = 220, 840           # menu panel left edge and width
PY, PH = 92, 624            # menu panel top and height
ROW_H = 48
ROW_PITCH = 50
GROUP_GAP = 14
HINT_CY = 754


def scrim(opacity=0.66, width=1280, height=800):
    return rect(0, 0, width, height, 0, INK, opacity)


def surface(x, y, w, h, r=16, opacity=0.94):
    """Menu surface: ink at 94 %, 1 px snow hairline at 12 %."""
    return rect(x, y, w, h, r, INK, opacity) + rect(x + 0.5, y + 0.5, w - 1, h - 1, r - 0.5, "none", None, SNOW, 0.12, 1)


# ------------------------------------------------------------ hint bar ---

KEYS = {"A": "Enter", "B": "Esc", "menu": "Esc", "Y": "C", "X": "R"}


def _glyph(kind, keyboard, fill=SNOW):
    """Returns (draw(x, cy) -> svg, width)."""
    if keyboard:
        if kind in ("ud", "lr", "dpad"):
            arrows = {"ud": ("up", "down"), "lr": ("left", "right"), "dpad": ("up", "down", "left", "right")}[kind]

            def draw(x, cy):
                out = ""
                for i, direction in enumerate(arrows):
                    out += u.key_arrow(x + i * 32, cy, direction, 28, fill)[0]
                return out
            return draw, len(arrows) * 32 - 4
        label = KEYS.get(kind, kind)
        width = u.keycap(0, 0, label, 28, fill)[1]
        return (lambda x, cy: u.keycap(x, cy, label, 28, fill)[0]), width
    if kind == "menu":
        return (lambda x, cy: u.pad_menu(x + 18, cy, 36, 26, fill)), 36
    if kind in ("ud", "lr", "dpad"):
        active = {"ud": ("up", "down"), "lr": ("left", "right"), "dpad": ("up", "down", "left", "right")}[kind]
        return (lambda x, cy: u.pad_dpad(x + 14, cy, 28, active, fill)), 28
    return (lambda x, cy: u.pad_button(x + 14, cy, kind, 28, fill)), 28


def hints(items, keyboard=False, right=1248, cy=HINT_CY, fill=SNOW):
    """Hint bar, right aligned. items: [(glyph, label)], most important last
    (it lands nearest the right thumb)."""
    out = ""
    x = right
    for kind, label in reversed(items):
        draw, gw = _glyph(kind, keyboard, fill)
        lw = tw(label, 20, 500)
        x -= lw
        out += text(x, cy + 7, label, 20, 500, fill)
        x -= gw + 8
        out += draw(x, cy)
        x -= 28
    return out


# ------------------------------------------------------------ list rows ---

def row(x, y, w, label, kind="action", value="", focused=False, editing=False, disabled=False, on=None,
        sub="", sub_fill=None, at_min=False, at_max=False, pressed=False, icon=None, h=ROW_H):
    """One settings row.

    kind: action (label only), nav (value and chevron), value (stepper),
    toggle (switch). Focused: sun fill, ink text. Editing: the value sits in
    an ink chip while it is changing.
    """
    out = ""
    cy = y + h / 2.0
    base = cy + 8.5
    if focused:
        out += rect(x, y, w, h, 10, "#E9BC52" if pressed else SUN)
    fg = INK if focused else (DUSK if disabled else SNOW)
    fg2 = INK if focused else (DUSK if disabled else MIST)
    lx = x + 20
    if icon:
        out += icon(lx, cy, fg)
        lx += 34
    out += text(lx, base, label, 24, 600 if focused else 500, fg)
    right = x + w - 20
    if kind == "nav":
        out += u.chevron(right - 5, cy, 16, fg)
        vx = right - 26
        out += text(vx, base, value, 24, 600, fg, "end")
        if sub:
            out += text(vx - tw(value, 24, 600) - 12, base - 1, sub, 20, 500, sub_fill if (sub_fill and not focused) else fg2, "end")
    elif kind == "value":
        # arrows are always there (dim when the row is not focused): they
        # say "left and right change this" and keep the right edge aligned
        slot = 112
        mid = right - 22 - slot / 2.0
        arrow = INK if focused else DUSK
        out += u.tri(right - 7, cy, 14, "right", arrow, 0.25 if (at_max and focused) else None)
        out += u.tri(mid - slot / 2.0 - 15, cy, 14, "left", arrow, 0.25 if (at_min and focused) else None)
        if focused and editing:
            out += rect(mid - slot / 2.0 + 2, y + 6, slot - 4, h - 12, 8, INK)
            out += text(mid, base, value, 24, 700, SUN, "middle")
        else:
            out += text(mid, base, value, 24, 700 if focused else 600, fg, "middle")
        if sub:
            out += text(mid - slot / 2.0 - 44, base - 1, sub, 20, 500, sub_fill if (sub_fill and not focused) else fg2, "end")
    elif kind == "toggle":
        out += u.toggle(right - 56, cy, on, focused)
        out += text(right - 56 - 12, base, "On" if on else "Off", 24, 600, fg, "end")
        if sub:
            out += text(right - 56 - 12 - 44, base - 1, sub, 20, 500, sub_fill if (sub_fill and not focused) else fg2, "end")
    return out


def info_line(x, y, w, s, fill=MIST, mark="i"):
    """Explanation for the focused row: a small mark and one line of text."""
    out = circle(x + 11, y, 11, "none", None, fill, 2)
    if mark == "i":
        out += circle(x + 11, y - 5, 1.6, fill) + line(x + 11, y - 1, x + 11, y + 6, fill, 2.4, None, "round")
    else:
        out += line(x + 11, y - 6, x + 11, y + 1, fill, 2.4, None, "round") + circle(x + 11, y + 5.5, 1.6, fill)
    out += text(x + 34, y + 7, s, 20, 500, fill)
    return out


def live_strip(right, y, s):
    """Slim live readout for overlays: the ride goes on behind them."""
    zi = u.zone_index(s["power"], s["ftp"])
    p, v = str(s["power"]), f"{s['speed']:.1f}"
    width = 16 + 12 + 10 + tw(p, 24, 700) + 5 + tw("W", 18, 500) + 22 + tw(v, 24, 700) + 5 + tw("km/h", 18, 500) + 16
    x = right - width
    out = surface(x, y, width, 40, 20)
    cx = x + 16
    out += rect(cx, y + 14, 12, 12, 3, RAMP[zi])
    cx += 22
    out += text(cx, y + 28.5, p, 24, 700)
    cx += tw(p, 24, 700) + 5
    out += text(cx, y + 28.5, "W", 18, 500, MIST)
    cx += tw("W", 18, 500) + 11
    out += line(cx, y + 11, cx, y + 29, SNOW, 1, 0.2)
    cx += 11
    out += text(cx, y + 28.5, v, 24, 700)
    cx += tw(v, 24, 700) + 5
    out += text(cx, y + 28.5, "km/h", 18, 500, MIST)
    return out


def screen_title(s, x=PX, y=68):
    return text(x, y, s, 36, 700)


# -------------------------------------------------------------- settings ---

SETTINGS = [
    # key, label, kind, explanation
    ("trainer", "Trainer", "nav", "Pair a smart trainer, or ride with the simulated rider."),
    ("follow", "Resistance follows the road", "toggle",
     "On: the trainer gets harder on climbs and easier downhill. Off: it only reports power."),
    ("difficulty", "Trainer difficulty", "value",
     "How much of each climb the trainer makes you feel. On-screen speed is not affected."),
    None,
    ("rider", "Rider weight", "value", "Your weight in riding kit. Heavier riders climb slower at the same power."),
    ("bike", "Bike weight", "value", "Bike plus bottles. Added to rider weight to work out your speed."),
    ("ftp", "FTP", "value", "The power you can hold for an hour. It sets the power zones on the ride screen."),
    None,
    ("tv", "TV dashboard", "toggle", "Ride numbers on a TV or phone on the same Wi-Fi. Open the address in its browser."),
    ("fullscreen", "Full screen", "toggle", "Fill the whole display. Off puts the game in a window."),
    None,
    ("restart", "Restart ride", "action", "Go back to the start line. Time and distance return to zero."),
    ("quit", "Quit", "action", "End this ride and see the summary."),
]

DEFAULTS = dict(trainer=("Tacx Neo 2T", "Connected"), follow=True, difficulty="50 %", rider="75.0 kg", bike="9.0 kg",
                ftp="250 W", tv=(True, "192.168.1.20:7878"), fullscreen=True)


def settings(live, focus="trainer", editing=False, values=None, note=None, tv_error=None, keyboard=False,
             pressed=False, show_hints=True):
    vals = dict(DEFAULTS)
    vals.update(values or {})
    out = scrim()
    out += screen_title("Settings")
    out += live_strip(PX + PW, 36, live)
    out += surface(PX, PY, PW, PH)
    y = PY + 16
    explain = ""
    focus_kind = "action"
    for item in SETTINGS:
        if item is None:
            y += GROUP_GAP
            continue
        key, label, kind, why = item
        focused = key == focus
        if focused:
            explain, focus_kind = why, kind
        x, w = PX + 12, PW - 24
        if kind == "nav":
            name, state = vals[key]
            out += row(x, y, w, label, "nav", name, focused, sub=state, sub_fill=GOOD if state == "Connected" else MIST,
                       pressed=pressed and focused)
        elif kind == "toggle":
            if key == "tv":
                on, address = vals[key]
                if tv_error and on:
                    out += row(x, y, w, label, "toggle", on=on, focused=focused, sub=tv_error, sub_fill=BAD)
                else:
                    out += row(x, y, w, label, "toggle", on=on, focused=focused, sub=address if on else "")
            else:
                out += row(x, y, w, label, "toggle", on=vals[key], focused=focused, pressed=pressed and focused)
        elif kind == "value":
            out += row(x, y, w, label, "value", vals[key], focused, editing and focused)
        else:
            out += row(x, y, w, label, "action", focused=focused)
        y += ROW_PITCH
    out += line(PX + 24, PY + PH - 62.5, PX + PW - 24, PY + PH - 62.5, SNOW, 1, 0.12)
    if note:
        out += info_line(PX + 28, PY + PH - 31, PW - 56, note, WARN, "!")
    else:
        out += info_line(PX + 28, PY + PH - 31, PW - 56, explain)
    items = [("ud", "Move")]
    if focus_kind == "value":
        items.append(("lr", "Change"))
    elif focus_kind == "toggle":
        items.append(("A", "Switch"))
    else:
        items.append(("A", "Select"))
    items.append(("B", "Back to ride"))
    if show_hints:
        out += hints(items, keyboard)
    return out


# --------------------------------------------------------------- pairing ---

def device_row(x, y, w, name, kind, bars, state="idle", focused=False, dim=False, h=64):
    """A found trainer: name, what it is, signal strength in bars and words."""
    out = ""
    if focused:
        out += rect(x, y, w, h, 10, SUN)
    fg = INK if focused else (DUSK if dim else SNOW)
    fg2 = INK if focused else (DUSK if dim else MIST)
    out += text(x + 20, y + 29, name, 24, 600, fg)
    out += text(x + 20, y + 52, kind, 18, 500, fg2)
    right = x + w - 20
    if state == "idle":
        words = ["Very weak", "Weak", "Fair", "Good", "Strong"][bars]
        out += u.signal(right - 29, y + 42, bars, fg)
        out += text(right - 44, y + 39, words, 20, 500, fg2, "end")
    elif state == "connecting":
        out += u.spinner(right - 12, y + h / 2.0, 11, fg, 3)
        out += text(right - 36, y + 39, "Connecting", 20, 600, fg, "end")
    elif state == "connected":
        out += circle(right - 12, y + h / 2.0, 12, INK if focused else GOOD) + u.icon_check(right - 12, y + h / 2.0, 15, SUN if focused else INK, 2.8)
        out += text(right - 36, y + 39, "Connected", 20, 600, fg if focused else GOOD, "end")
    elif state == "failed":
        out += circle(right - 12, y + h / 2.0, 12, INK if focused else BAD) + u.icon_cross(right - 12, y + h / 2.0, 14, SUN if focused else INK, 2.8)
        out += text(right - 36, y + 39, "Could not connect", 20, 600, fg if focused else BAD, "end")
    return out


def status_block(x, y, kind, head, sub, sub2=""):
    """Top of the pairing panel: one mark, a headline, a plain sentence."""
    cx, cy = x + 28, y + 30
    if kind == "busy":
        out = u.spinner(cx, cy, 18, SNOW, 4)
    elif kind == "ok":
        out = circle(cx, cy, 20, GOOD) + u.icon_check(cx, cy, 24, INK, 4)
    elif kind == "bad":
        out = circle(cx, cy, 20, BAD) + u.icon_cross(cx, cy, 22, INK, 4)
    else:
        out = u.icon_warning(cx, cy, 40, WARN)
    out += text(x + 68, y + 28, head, 28, 600)
    out += text(x + 68, y + 56, sub, 20, 500, MIST)
    if sub2:
        out += text(x + 68, y + 82, sub2, 20, 500, MIST)
    return out


def tips(x, y, w, lines, title="If your trainer does not appear"):
    h = 44 + 30 * len(lines) + 10
    out = rect(x, y, w, h, 12, NIGHT) + caption(x + 20, y + 30, title)
    for i, s in enumerate(lines):
        yy = y + 62 + i * 30
        out += circle(x + 26, yy - 6, 3, MIST) + text(x + 42, yy, s, 20, 500, SNOW)
    return out


def pairing(state, keyboard=False, live=None):
    out = scrim()
    out += screen_title("Trainer")
    if live:
        out += live_strip(PX + PW, 36, live)
    out += surface(PX, PY, PW, PH)
    x, w = PX + 12, PW - 24
    bx = PX + 28
    devices = [("Tacx Neo 2T 12345", "Smart trainer, resistance control", 4),
               ("Direto XR 0412", "Smart trainer, resistance control", 2),
               ("Power Meter 7C21", "Power only, no resistance control", 1)]
    list_y = PY + 124
    sim_y = PY + PH - 16 - ROW_H
    again_y = sim_y - ROW_PITCH
    key = lambda cx, cy, fg: u.icon_keyboard(cx, cy - 12, 24, fg)
    items = [("ud", "Move"), ("A", "Select"), ("B", "Back")]
    first = again_y if state in ("failed", "unavailable") else sim_y
    out += line(PX + 24, first - 12.5, PX + PW - 24, first - 12.5, SNOW, 1, 0.12)
    tip_lines = ["Wake the trainer: turn the pedals a few times.",
                 "Close other apps or bike computers that may hold the connection.",
                 "A trainer talks to one controller at a time."]

    if state == "searching":
        out += status_block(bx, PY + 24, "busy", "Searching for trainers", "Nothing found yet. This can take a few seconds.")
        out += tips(PX + 24, list_y + 8, PW - 48, tip_lines)
        out += row(x, sim_y, w, "Use the simulated rider", "action", focused=True, icon=key)
    elif state == "found":
        out += status_block(bx, PY + 24, "busy", "Choose your trainer", "3 found. Still searching.")
        for i, (name, kind, bars) in enumerate(devices):
            out += device_row(x, list_y + i * 66, w, name, kind, bars, "idle", focused=i == 0)
        out += row(x, sim_y, w, "Use the simulated rider", "action", icon=key)
        items = [("ud", "Move"), ("A", "Connect"), ("B", "Back")]
    elif state == "connecting":
        out += status_block(bx, PY + 24, "busy", "Connecting to Tacx Neo 2T 12345", "Keep the pedals turning.")
        for i, (name, kind, bars) in enumerate(devices):
            out += device_row(x, list_y + i * 66, w, name, kind, bars, "connecting" if i == 0 else "idle",
                              focused=i == 0, dim=i != 0)
        out += row(x, sim_y, w, "Disconnect and use the simulated rider", "action", icon=key)
        items = [("ud", "Move"), ("B", "Back")]
    elif state == "connected":
        out += status_block(bx, PY + 24, "ok", "Tacx Neo 2T 12345 is connected",
                            "Resistance follows the road. Go back and ride.")
        for i, (name, kind, bars) in enumerate(devices):
            out += device_row(x, list_y + i * 66, w, name, kind, bars, "connected" if i == 0 else "idle",
                              focused=i == 0)
        out += row(x, sim_y, w, "Disconnect and use the simulated rider", "action", icon=key)
        items = [("ud", "Move"), ("A", "Select"), ("B", "Back to ride")]
    elif state == "failed":
        out += status_block(bx, PY + 24, "bad", "Could not connect to Tacx Neo 2T 12345",
                            "The trainer did not answer in time.")
        out += device_row(x, list_y, w, devices[0][0], devices[0][1], 4, "failed")
        out += tips(PX + 24, list_y + 82, PW - 48, tip_lines[:2], "Before you try again")
        out += row(x, again_y, w, "Try again", "action", focused=True)
        out += row(x, sim_y, w, "Use the simulated rider", "action", icon=key)
    else:
        out += status_block(bx, PY + 24, "warn", "Bluetooth is not available",
                            "Bluetooth is off, or this device has no Bluetooth adapter.")
        out += tips(PX + 24, list_y + 8, PW - 48,
                    ["Turn Bluetooth on in the system settings, then search again.",
                     "You can ride now with the simulated rider and pair later."], "What you can do")
        out += row(x, again_y, w, "Search again", "action", focused=True)
        out += row(x, sim_y, w, "Use the simulated rider", "action", icon=key)
    out += hints(items, keyboard)
    return out


# ------------------------------------------------------- dialog and toast ---

def button(x, y, w, label, focused=False, h=56, danger=False):
    if focused:
        return rect(x, y, w, h, 12, SUN) + text(x + w / 2.0, y + h / 2.0 + 8.5, label, 24, 700, INK, "middle")
    out = rect(x + 1, y + 1, w - 2, h - 2, 11, NIGHT, None, SNOW, 0.28, 2)
    return out + text(x + w / 2.0, y + h / 2.0 + 8.5, label, 24, 600, SNOW, "middle")


def dialog(title, lines, safe, action, keyboard=False):
    """Confirm dialog. Focus opens on the safe choice."""
    w = 560
    h = 96 + 30 * len(lines) + 104
    x, y = (1280 - w) / 2.0, (800 - h) / 2.0 - 16
    out = scrim(0.6)
    out += rect(x, y + 8, w, h, 20, "#000000", 0.35)
    out += surface(x, y, w, h, 20, 0.98)
    out += text(x + 32, y + 60, title, 32, 700)
    for i, s in enumerate(lines):
        out += text(x + 32, y + 100 + i * 30, s, 20, 500, MIST)
    by = y + h - 32 - 56
    bw = (w - 64 - 16) / 2.0
    out += button(x + 32, by, bw, safe, True)
    out += button(x + 32 + bw + 16, by, bw, action)
    out += hints([("lr", "Move"), ("A", "Select"), ("B", "Cancel")], keyboard)
    return out


def toast(cx, y, kind, s):
    """Transient message, top centre. kind: ok, warn, bad."""
    width = 20 + 28 + 12 + tw(s, 20, 600) + 24
    x = cx - width / 2.0
    out = rect(x, y + 4, width, 48, 24, "#000000", 0.3)
    out += rect(x, y, width, 48, 24, INK, 0.96) + rect(x + 1, y + 1, width - 2, 46, 23, "none", None, {"ok": GOOD, "warn": WARN, "bad": BAD}[kind], 0.9, 2)
    ix, iy = x + 20 + 14, y + 24
    if kind == "ok":
        out += circle(ix, iy, 13, GOOD) + u.icon_check(ix, iy, 16, INK, 3)
    elif kind == "warn":
        out += u.icon_warning(ix, iy, 28, WARN)
    else:
        out += circle(ix, iy, 13, BAD) + u.icon_cross(ix, iy, 15, INK, 3)
    out += text(x + 60, y + 31, s, 20, 600)
    return out


# --------------------------------------------------------------- summary ---

def stat(x, y, label, value, unit="", size=56, absent=False):
    """Summary figure: caption above, number, unit after."""
    out = caption(x, y, label)
    if absent:
        out += text(x, y + size * 0.72 + 14, "\u2014", size, 700, DUSK)
        out += text(x + tw("\u2014", size, 700) + 8, y + size * 0.72 + 14, "no sensor", 18, 500, DUSK)
        return out
    out += text(x, y + size * 0.72 + 14, value, size, 700)
    if unit:
        out += text(x + tw(value, size, 700) + 8, y + size * 0.72 + 14, unit, 20 if size > 40 else 18, 500, MIST)
    return out


def summary(course, heart=True, keyboard=False):
    out = scrim(0.74)
    x, w = 160, 960
    y, h = 92, 580
    out += surface(x, y, w, h)
    out += caption(x + 40, y + 50, "Ride complete", SUN)
    out += text(x + 40, y + 96, "Penguin Pass", 40, 700)
    out += text(x + w - 40, y + 96, "3 laps of the 5.0 km loop", 20, 500, MIST, "end")
    # the ride as a skyline: three laps
    import hud as H
    sx, sy, sw, sh = x + 40, y + 190, w - 80, 56
    lap_w = sw / 3.0
    for i in range(3):
        chart, _ = u.profile_chart(course, sx + i * lap_w, sy - sh, lap_w, sh, 0, course.length, 4, 60, SNOW, 0.92,
                                   floor=course.low - 30, sw=1.5)
        out += chart
        if i:
            out += line(sx + i * lap_w, sy - sh - 6, sx + i * lap_w, sy + 6, INK, 3) + line(sx + i * lap_w, sy - sh - 6, sx + i * lap_w, sy + 6, SNOW, 1, 0.5)
        out += caption(sx + i * lap_w + 6, sy + 24, f"Lap {i + 1}", MIST)
        out += text(sx + (i + 1) * lap_w - 6, sy + 24, ["0:13:02", "0:12:41", "0:12:58"][i], 18, 500, MIST, "end")
    out += line(sx, sy + 0.5, sx + sw, sy + 0.5, SNOW, 1, 0.3)
    # headline figures
    ty = y + 268
    cols = [x + 40, x + 40 + 300, x + 40 + 600]
    out += stat(cols[0], ty, "Time", "0:38:41")
    out += stat(cols[1], ty, "Distance", "14.96", "km")
    out += stat(cols[2], ty, "Ascent", "507", "m")
    out += line(x + 40, ty + 92.5, x + w - 40, ty + 92.5, SNOW, 1, 0.12)
    sy2 = ty + 120
    cols = [x + 40 + i * 176 for i in range(5)]
    out += stat(cols[0], sy2, "Avg power", "212", "W", 40)
    out += stat(cols[1], sy2, "Max power", "618", "W", 40)
    out += stat(cols[2], sy2, "Avg cadence", "84", "rpm", 40)
    out += stat(cols[3], sy2, "Avg heart", "148", "bpm", 40, absent=not heart)
    out += stat(cols[4], sy2, "Laps", "3", "", 40)
    by = y + h - 40 - 56
    out += button(x + 40, by, 280, "Ride again", True)
    out += button(x + 40 + 296, by, 280, "Back to title")
    out += hints([("lr", "Move"), ("A", "Select")], keyboard)
    return out


# --------------------------------------------------------- course picker ---

def map_outline(points, x, y, w, h, stroke=SNOW, sw=4, rotate=False):
    xs = [p[0] for p in points]
    zs = [p[1] for p in points]
    if rotate:
        xs, zs = zs, [-v for v in xs]
    minx, maxx, minz, maxz = min(xs), max(xs), min(zs), max(zs)
    scale = min(w / (maxx - minx), h / (maxz - minz))
    ox = x + (w - (maxx - minx) * scale) / 2.0
    oy = y + (h - (maxz - minz) * scale) / 2.0
    pts = [(ox + (a - minx) * scale, oy + (b - minz) * scale) for a, b in zip(xs, zs)]
    return pts


def colored_route(pts, course, sw=5):
    """Route line coloured by gradient, on an ink casing."""
    out = u.polyline(pts, INK, sw + 5)
    n = len(pts) - 1
    run = [pts[0]]
    current = None
    for i in range(n):
        d0, d1 = course.length * i / n, course.length * (i + 1) / n
        grade = (course.at(min(d1, course.length - 0.01))[0] - course.at(d0)[0]) / (d1 - d0) * 100.0
        color = u.grade_color(grade)
        if current is not None and color != current:
            out += u.polyline(run, current, sw)
            run = [pts[i]]
        current = color
        run.append(pts[i + 1])
    out += u.polyline(run, current, sw)
    return out


def segmented(x, y, w, options, selected, focused=False, h=44):
    """One choice of several, all visible. Selected: solid; focused row:
    the solid segment is sun."""
    out = rect(x, y, w, h, 10, NIGHT)
    seg = w / len(options)
    for i, name in enumerate(options):
        if i == selected:
            out += rect(x + i * seg + 3, y + 3, seg - 6, h - 6, 8, SUN if focused else SNOW)
            out += text(x + i * seg + seg / 2.0, y + h / 2.0 + 7.5, name, 20, 700, INK, "middle")
        else:
            out += text(x + i * seg + seg / 2.0, y + h / 2.0 + 7.5, name, 20, 500, MIST, "middle")
    return out


def icon_dice(cx, cy, s=24, fill=SNOW, ink=INK):
    out = rect(cx - s / 2.0, cy - s / 2.0, s, s, 5, fill)
    for dx, dy in ((-1, -1), (1, 1), (0, 0), (-1, 1), (1, -1)):
        out += circle(cx + dx * s * 0.24, cy + dy * s * 0.24, s * 0.085, ink)
    return out


def course_picker(loop, generated, mode="loop", keyboard=False):
    """mode: loop (the fixed course is selected) or new (a generated route
    with the seed being edited)."""
    out = scrim(0.74)
    out += screen_title("Choose a course", 64)
    lx, lw = 64, 456
    rx, rw = 544, 672
    top, bottom = 92, 716
    out += surface(lx, top, lw, bottom - top)
    out += surface(rx, top, rw, bottom - top)

    # left: the list of courses
    def card(y, title, sub, selected, focused, h=76):
        o = ""
        if focused:
            o += rect(lx + 12, y, lw - 24, h, 10, SUN)
        elif selected:
            o += rect(lx + 12, y, lw - 24, h, 10, NIGHT) + rect(lx + 13, y + 1, lw - 26, h - 2, 9, "none", None, SUN, None, 2)
        fg, fg2 = (INK, INK) if focused else (SNOW, MIST)
        o += text(lx + 32, y + 33, title, 24, 600, fg) + text(lx + 32, y + 59, sub, 20, 500, fg2)
        if selected:
            o += circle(lx + lw - 44, y + h / 2.0, 12, INK if focused else SUN) + u.icon_check(lx + lw - 44, y + h / 2.0, 15, SUN if focused else INK, 2.8)
        return o

    out += card(top + 16, "Penguin Pass", "5.0 km loop \u00b7 169 m ascent \u00b7 mountain", mode == "loop", mode == "loop")
    new_y = top + 16 + 84
    if mode == "loop":
        out += card(new_y, "New route", "Pick a distance, a terrain and a seed", False, False)
        course = loop
        name, facts = "Penguin Pass", [("Distance", "5.0", "km"), ("Ascent", "169", "m"), ("Highest", "163", "m"), ("Steepest", "10.0", "%")]
        pts = map_outline(loop.outline, rx + 32, top + 84, rw - 64, 236)
        hint_items = [("ud", "Move"), ("A", "Ride this course"), ("B", "Back")]
    else:
        out += card(new_y, "New route", "42 km \u00b7 mountain \u00b7 seed K7PASS", True, False)
        fy = new_y + 96
        fx, fw = lx + 32, lw - 64
        out += caption(fx, fy + 10, "Distance")
        out += rect(fx, fy + 22, fw, 48, 10, NIGHT)
        out += u.tri(fx + 22, fy + 46, 16, "left", MIST) + u.tri(fx + fw - 22, fy + 46, 16, "right", MIST)
        out += text(fx + fw / 2.0, fy + 54.5, "42 km", 24, 600, SNOW, "middle")
        out += caption(fx, fy + 104, "Terrain")
        out += segmented(fx, fy + 116, fw, ["Flat", "Rolling", "Mountain", "Desert"], 2)
        out += caption(fx, fy + 194, "Seed")
        cell_w, gap = 40, 5
        seed = "K7PASS"
        for i, ch in enumerate(seed):
            cx = fx + i * (cell_w + gap)
            active = i == 2
            out += rect(cx, fy + 206, cell_w, 52, 8, SUN if active else NIGHT)
            out += text(cx + cell_w / 2.0, fy + 242, ch, 28, 700, INK if active else SNOW, "middle")
            if active:
                out += u.tri(cx + cell_w / 2.0, fy + 196, 12, "up", SUN) + u.tri(cx + cell_w / 2.0, fy + 268, 12, "down", SUN)
        dx = fx + 6 * (cell_w + gap) + 7
        out += rect(dx, fy + 206, fw - (dx - fx), 52, 8, NIGHT) + icon_dice(dx + 24, fy + 232, 24, SNOW)
        out += text(dx + 44, fy + 238.5, "Random", 18, 600, SNOW)
        out += text(fx, fy + 300, "The same distance, terrain and seed always", 18, 500, MIST)
        out += text(fx, fy + 324, "build the same route.", 18, 500, MIST)
        course = generated
        steepest = max(s[2] for s in generated.samples)
        name, facts = "New route", [("Distance", "42.0", "km"), ("Ascent", f"{generated.ascent:.0f}", "m"),
                                    ("Highest", f"{generated.top:.0f}", "m"), ("Steepest", f"{steepest:.1f}", "%")]
        import math
        raw = []
        for i in range(240):
            t = i / 239.0
            a = t * math.tau * 0.92 + 0.3
            r = 1.0 + 0.28 * math.sin(3 * a + 1.0) + 0.16 * math.sin(7 * a) + 0.07 * math.sin(13 * a + 2)
            raw.append((math.cos(a) * r * 1.5, math.sin(a) * r))
        pts = map_outline(raw, rx + 32, top + 84, rw - 64, 236)
        hint_items = [("lr", "Move"), ("ud", "Change letter"), ("X", "Random"), ("A", "Ride this route"), ("B", "Back")]

    # right: preview
    out += caption(rx + 32, top + 44, "Preview")
    out += text(rx + 32 + cw("Preview") + 16, top + 45, name, 24, 600)
    out += rect(rx + 24, top + 68, rw - 48, 268, 12, NIGHT)
    out += colored_route(pts, course)
    out += circle(pts[0][0], pts[0][1], 8, SNOW, None, INK, 3)
    out += caption(pts[0][0] + 16, pts[0][1] - 12, "Start", SNOW)
    cy0 = top + 352
    chart, _ = u.profile_chart(course, rx + 32, cy0, rw - 64, 112, 0, course.length, 4, 60, SNOW, 0.92,
                               floor=course.low - 0.12 * (course.top - course.low))
    out += chart
    out += line(rx + 32, cy0 + 112.5, rx + rw - 32, cy0 + 112.5, SNOW, 1, 0.3)
    km = course.length / 1000.0
    for i in range(5):
        tx = rx + 32 + (rw - 64) * i / 4.0
        label = f"{km * i / 4.0:.0f} km" if km >= 20 else f"{km * i / 4.0:.1f} km"
        out += text(tx, cy0 + 136, label, 18, 500, MIST, "start" if i == 0 else "end" if i == 4 else "middle")
    fy = top + 528
    for i, (label, value, unit) in enumerate(facts):
        out += stat(rx + 32 + i * 156, fy, label, value, unit, 40)
    out += hints(hint_items, keyboard)
    return out
