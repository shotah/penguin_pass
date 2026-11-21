"""Reference sheets for docs/ui.md: tokens, components, console anatomy."""
import hud as H
import menus as m
import pp_ui as u
from pp_ui import (BAD, DUSK, EMBER, GOOD, INK, MIST, NIGHT, RAMP, SLATE, SNOW, SUN, WARN, ZONES, caption, circle, cw,
                   line, rect, text, tw)

GUIDE = "#FF4FD8"


def swatch(x, y, name, color, note="", w=148, h=64, dark_text=False):
    out = rect(x, y, w, h, 10, color) + rect(x + 0.5, y + 0.5, w - 1, h - 1, 9.5, "none", None, SNOW, 0.2, 1)
    out += text(x, y + h + 24, name, 20, 600)
    out += text(x, y + h + 46, color.upper(), 18, 500, MIST)
    if note:
        out += text(x, y + h + 68, note, 18, 500, MIST)
    return out


def tokens():
    out = rect(0, 0, 1280, 800, 0, "#060A13")
    out += text(40, 56, "Penguin Pass UI tokens", 32, 700)
    out += caption(40, 100, "Surfaces and text")
    items = [("ink", INK, "every surface"), ("night", NIGHT, "raised well"), ("slate", SLATE, "tracks, off"),
             ("snow", SNOW, "text primary"), ("mist", MIST, "text secondary"), ("dusk", DUSK, "disabled, absent"),
             ("ridden", u.RIDDEN, "profile behind you")]
    for i, (name, color, note) in enumerate(items):
        out += swatch(40 + i * 172, 116, name, color, note)
    out += caption(40, 290, "Accent and state")
    items = [("sun", SUN, "focus, brand"), ("ember", EMBER, "brand only"), ("good", GOOD, "connected, on"),
             ("warn", WARN, "reconnecting"), ("bad", BAD, "failed, error")]
    for i, (name, color, note) in enumerate(items):
        out += swatch(40 + i * 172, 306, name, color, note)
    out += caption(40, 480, "Effort ramp: power zones (percent of FTP) and gradient")
    zone_notes = ["up to 55 %", "56 to 75 %", "76 to 90 %", "91 to 105 %", "106 to 120 %", "121 to 150 %", "over 150 %"]
    grade_notes = ["flat, \u22121 to +1 %", "downhill, under \u22121 %", "+1 to +3 %", "+3 to +6 %", "+6 to +9 %",
                   "+9 to +12 %", "+12 % and over"]
    for i in range(7):
        x = 40 + i * 172
        out += rect(x, 496, 148, 64, 10, RAMP[i])
        out += text(x + 12, 538, f"ramp-{i + 1}", 22, 700, INK)
        out += text(x, 584, f"{ZONES[i][0]} {ZONES[i][1]}", 20, 600)
        out += text(x, 606, zone_notes[i], 18, 500, MIST)
        out += text(x, 632, RAMP[i].upper(), 18, 500, MIST)
        out += text(x, 658, grade_notes[i], 18, 500, SNOW)
    out += caption(40, 706, "Type: Barlow Semi Condensed, tabular figures")
    samples = [("88", 700, "345"), ("52", 700, "62.4"), ("36", 700, "Settings"), ("32", 700, "Start ride"),
               ("24", 600, "Rider weight"), ("22", 600, "0:07:32"), ("20", 500, "Explanation"), ("18", 500, "km/h")]
    x = 40
    for size, weight, sample in samples:
        px = int(size)
        shown = min(px, 64)
        out += text(x, 776, sample, shown, weight)
        width = tw(sample, shown, weight)
        out += text(x + width + 6, 776, size, 18, 500, SUN)
        x += width + 6 + tw(size, 18, 500) + 30
    return out


def components():
    out = rect(0, 0, 1280, 1400, 0, "#060A13")
    out += u.plate("summit-chase", 1280, 800, 0, -260)
    out += rect(0, 0, 1280, 300, 0, INK, 0.25)
    out += rect(0, 300, 1280, 1100, 0, "#060A13")
    out += text(40, 52, "Components", 32, 700)
    # chips over a bright scene
    out += caption(40, 92, "Chips, over the brightest scene")
    x = 40
    for state in ("controlled", "reading", "reconnecting", "sim"):
        part, w = H.trainer_chip(x, 108, state, track=False)
        out += part
        x += w + 16
    x = 40
    part, w = H.tv_chip(x, 160, "192.168.1.20:7878", "start", track=False)
    out += part
    x += w + 16
    part, w = H.menu_hint(x, 160, False, "start", track=False)
    out += part
    x += w + 16
    part, w = H.menu_hint(x, 160, True, "start", track=False)
    out += part
    x += w + 16
    out += u.group(H.power_hint(True), f"translate({x - 32} {160 - (H.CON_Y - H.SKY_H - 52)})")
    x += 220
    out += u.group(H.power_hint(False), f"translate({x - 32} {160 - (H.CON_Y - H.SKY_H - 52)})")
    out += m.toast(260, 224, "ok", "Settings saved")
    out += m.toast(660, 224, "warn", "Settings file was damaged. Defaults are in use.")
    out += m.toast(1080, 224, "bad", "Trainer disconnected")

    # rows
    out += caption(40, 348, "List rows: normal, focused, pressed, disabled, editing")
    y = 364
    w = 580
    out += m.surface(28, y - 4, w + 24, 5 * 50 + 6, 14)
    out += m.row(40, y, w, "Rider weight", "value", "75.0 kg")
    out += m.row(40, y + 50, w, "Rider weight", "value", "75.0 kg", focused=True)
    out += m.row(40, y + 100, w, "Restart ride", "action", focused=True, pressed=True)
    out += m.row(40, y + 150, w, "Search again", "action", disabled=True)
    out += m.row(40, y + 200, w, "Rider weight", "value", "75.5 kg", focused=True, editing=True)
    labels = ["normal", "focused", "pressed", "disabled", "editing"]
    x2 = 664
    out += m.surface(x2 - 12, y - 4, w + 24, 5 * 50 + 6, 14)
    out += m.row(x2, y, w, "Full screen", "toggle", on=True)
    out += m.row(x2, y + 50, w, "Full screen", "toggle", on=True, focused=True)
    out += m.row(x2, y + 100, w, "Full screen", "toggle", on=False)
    out += m.row(x2, y + 150, w, "Full screen", "toggle", on=False, focused=True)
    out += m.row(x2, y + 200, w, "Trainer", "nav", "Tacx Neo 2T", sub="Connected", sub_fill=GOOD)

    # buttons, segmented, seed
    out += caption(40, 668, "Buttons, choice, signal")
    out += m.button(40, 684, 220, "Keep riding", True)
    out += m.button(276, 684, 220, "Restart")
    out += m.segmented(520, 690, 400, ["Flat", "Rolling", "Mountain", "Desert"], 2, True)
    for bars in range(5):
        x = 960 + bars * 62
        out += u.signal(x, 730, bars, SNOW)
        out += text(x, 756, str(bars), 18, 500, MIST)

    # glyphs
    out += caption(40, 812, "Controller glyphs (generic shapes) and their keyboard twins")
    x = 40
    for kind, label in (("A", "Select"), ("B", "Back"), ("X", "Random"), ("Y", "Camera"), ("menu", "Menu"),
                        ("ud", "Move"), ("lr", "Change"), ("dpad", "Move")):
        draw, gw = m._glyph(kind, False)
        out += draw(x, 852)
        out += text(x + gw + 8, 859, label, 20, 500)
        draw, kw = m._glyph(kind, True)
        out += draw(x, 900)
        x += max(gw + 8 + tw(label, 20, 500), kw) + 36

    # zone ladder and chart
    out += caption(40, 968, "Zone ladder, each zone lit")
    for zi in range(7):
        x = 40 + zi * 172
        out += rect(x - 8, 984, 156, 124, 12, INK) + H.zone_ladder(x + 8, 998, zi)
        out += text(x + 44, 1040, f"{ZONES[zi][0]}", 32, 700)
        out += caption(x + 44, 1068, ZONES[zi][1], RAMP[zi])
    out += caption(40, 1160, "Gradient colours on a profile, with the ridden part grey")
    course = u.penguin_pass()
    chart, _ = u.profile_chart(course, 40, 1180, 1200, 150, 0, course.length, 4, 60, SNOW, 0.95, 1400.0,
                               floor=course.low - 20)
    out += chart + line(40, 1330.5, 1240, 1330.5, SNOW, 1, 0.3)
    for i in range(6):
        out += text(40 + 1200 * i / 5.0, 1360, f"{i} km", 18, 500, MIST, "start" if i == 0 else "end" if i == 5 else "middle")
    return out


def anatomy(state, course):
    """The console with its construction lines. All numbers are px in the
    1280 x 800 design frame. Returns (svg, height)."""
    shift, height = 512, 336
    out = u.plate("summit-chase") + rect(0, 0, 1280, 800, 0, INK, 0.6)
    out += H.console(state, course)
    for x, label, row in ((32, "32", 0), (52, "52 ladder", 1), (88, "88 hero", 2), (312, "312", 0),
                          (336, "336 cadence", 1), (452, "452 heart", 2), (556, "556", 0), (580, "580 speed", 1),
                          (712, "712 grade", 2), (860, "860 chart", 1), (1050, "1050", 0), (1063, "1063", 2),
                          (1078, "1078 list", 1), (1228, "1228", 0), (1248, "1248", 2)):
        out += line(x + 0.5, shift + 30 + row * 22, x + 0.5, 772, GUIDE, 1, 0.9)
        if x > 1200:
            out += text(x - 4, shift + 28 + row * 22, label, 16, 600, GUIDE, "end")
        else:
            out += text(x + 4, shift + 28 + row * 22, label, 16, 600, GUIDE)
    for y in (614, 644, 720, 748, 768):
        out += line(8, y + 0.5, 1272, y + 0.5, GUIDE, 1, 0.75)
    out = u.group(out, f"translate(0 {-shift})")
    out += rect(0, 292, 1280, 44, 0, "#060A13")
    x = 32
    for key, label in (("y 614", "skyline top"), ("y 644", "console top"), ("y 720", "number baseline"),
                       ("y 748", "label baseline"), ("y 768", "console bottom"), ("x", "labelled on each line")):
        out += text(x, 320, key, 16, 700, GUIDE) + text(x + tw(key, 16, 700) + 6, 320, label, 16, 500, SNOW)
        x += tw(key, 16, 700) + 6 + tw(label, 16, 500) + 32
    return out, height
