"""Penguin Pass UI mockup kit: tokens, text metrics, and SVG parts.

Standard library only. `gen.py` composes these into the mockup SVGs. The
token names here are the names used in docs/ui.md.
"""
import json
import math
import os
import re
import struct

HERE = os.path.dirname(os.path.abspath(__file__))
UI = os.path.normpath(os.path.join(HERE, ".."))
ROOT = os.path.normpath(os.path.join(UI, "..", "..", ".."))
FONT_DIR = os.path.join(ROOT, "godot", "assets", "fonts", "barlow-semi-condensed")
FAMILY = "Barlow Semi Condensed"

# ---------------------------------------------------------------- tokens ---

INK = "#0B1220"        # penguin black: every surface
NIGHT = "#12203F"      # raised surface on ink (rows, wells)
SLATE = "#39425A"      # road grey: tracks, dividers, empty bars
SNOW = "#F4F8FF"       # text primary
MIST = "#B4C0D4"       # text secondary
DUSK = "#7C8AA3"       # text tertiary, disabled
SUN = "#FFD166"        # focus, brand accent
EMBER = "#F08A4B"      # sunrise orange, brand only
GOOD = "#3DD68C"       # positive
WARN = "#FFB23E"       # warning
BAD = "#FF5A52"        # danger

RIDDEN = "#66728A"     # course profile behind the rider

# Effort ramp. Index 0..6. Power zones use it as Z1..Z7.
RAMP = ["#9AA8BD", "#4FA8FF", "#3DD68C", "#FFD166", "#FF9A3C", "#FF5A52", "#D277FF"]
ZONES = [
    ("Z1", "Recovery", 0.00),
    ("Z2", "Endurance", 0.55),
    ("Z3", "Tempo", 0.75),
    ("Z4", "Threshold", 0.90),
    ("Z5", "VO2 max", 1.05),
    ("Z6", "Anaerobic", 1.20),
    ("Z7", "Sprint", 1.50),
]


def zone_index(power_w, ftp_w):
    ratio = power_w / ftp_w
    index = 0
    for i, (_, _, low) in enumerate(ZONES):
        if ratio > low:
            index = i
    return index


def grade_index(grade_pct):
    """Gradient scale: which ramp colour a grade uses."""
    if grade_pct < -1.0:
        return 1          # descent: blue
    if grade_pct < 1.0:
        return 0          # flat: grey
    if grade_pct < 3.0:
        return 2          # green
    if grade_pct < 6.0:
        return 3          # yellow
    if grade_pct < 9.0:
        return 4          # orange
    if grade_pct < 12.0:
        return 5          # red
    return 6              # violet


def grade_color(grade_pct):
    return RAMP[grade_index(grade_pct)]


# ----------------------------------------------------------------- fonts ---

class Font:
    def __init__(self, path):
        data = open(path, "rb").read()
        count = struct.unpack(">H", data[4:6])[0]
        tabs = {}
        for index in range(count):
            tag, _sum, offset, length = struct.unpack(">4sIII", data[12 + 16 * index:28 + 16 * index])
            tabs[tag.decode()] = (offset, length)
        self.upm = struct.unpack(">H", data[tabs["head"][0] + 18:tabs["head"][0] + 20])[0]
        hhea = tabs["hhea"][0]
        metrics = struct.unpack(">H", data[hhea + 34:hhea + 36])[0]
        hmtx = tabs["hmtx"][0]
        self.adv = [struct.unpack(">H", data[hmtx + 4 * i:hmtx + 4 * i + 2])[0] for i in range(metrics)]
        self.map = self._cmap(data, tabs["cmap"][0])
        self.tnum = self._tabular(data, tabs["post"])

    def _cmap(self, data, base):
        count = struct.unpack(">H", data[base + 2:base + 4])[0]
        best = None
        for index in range(count):
            _plat, _enc, offset = struct.unpack(">HHI", data[base + 4 + 8 * index:base + 12 + 8 * index])
            if struct.unpack(">H", data[base + offset:base + offset + 2])[0] == 4:
                best = base + offset
        seg = struct.unpack(">H", data[best + 6:best + 8])[0] // 2
        ends = struct.unpack(f">{seg}H", data[best + 14:best + 14 + 2 * seg])
        at = best + 16 + 2 * seg
        starts = struct.unpack(f">{seg}H", data[at:at + 2 * seg])
        at += 2 * seg
        deltas = struct.unpack(f">{seg}h", data[at:at + 2 * seg])
        at += 2 * seg
        ranges = struct.unpack(f">{seg}H", data[at:at + 2 * seg])
        mapping = {}
        for i in range(seg):
            for code in range(starts[i], ends[i] + 1):
                if code == 0xFFFF:
                    continue
                if ranges[i] == 0:
                    glyph = (code + deltas[i]) & 0xFFFF
                else:
                    where = at + 2 * i + ranges[i] + 2 * (code - starts[i])
                    glyph = struct.unpack(">H", data[where:where + 2])[0]
                    if glyph:
                        glyph = (glyph + deltas[i]) & 0xFFFF
                if glyph:
                    mapping[code] = glyph
        return mapping

    def _tabular(self, data, post):
        """Advance of the `tnum` digits (glyph zero.tf)."""
        offset, length = post
        glyphs = struct.unpack(">H", data[offset + 32:offset + 34])[0]
        index = struct.unpack(f">{glyphs}H", data[offset + 34:offset + 34 + 2 * glyphs])
        names = []
        at = offset + 34 + 2 * glyphs
        while at < offset + length:
            size = data[at]
            names.append(data[at + 1:at + 1 + size].decode("latin1"))
            at += 1 + size
        for glyph, name_index in enumerate(index):
            if name_index >= 258 and names[name_index - 258] == "zero.tf":
                return self.adv[min(glyph, len(self.adv) - 1)]
        raise SystemExit("font has no tabular digits")

    def advance(self, char):
        glyph = self.map.get(ord(char))
        if glyph is None:
            raise SystemExit(f"glyph missing from the game font: U+{ord(char):04X} {char!r}")
        return self.adv[min(glyph, len(self.adv) - 1)]


FONTS = {
    500: Font(os.path.join(FONT_DIR, "BarlowSemiCondensed-Medium.ttf")),
    600: Font(os.path.join(FONT_DIR, "BarlowSemiCondensed-SemiBold.ttf")),
    700: Font(os.path.join(FONT_DIR, "BarlowSemiCondensed-Bold.ttf")),
}


NUMBER = re.compile(r"^[0-9 .,:+\u2212\u2014\u00b1%-]+( ?(kg|W|km/h|km|m|rpm|bpm|%))?$")


def is_number(s):
    """True for a figure, with or without its unit. Figures get tabular
    digits; names, addresses and sentences keep proportional ones."""
    return bool(NUMBER.match(s)) and any(c.isdigit() for c in s)


def _layout(s, size, weight, tracking, tabular=None):
    """Per-character x offsets and the total width."""
    font = FONTS[weight]
    scale = size / font.upm
    if tabular is None:
        tabular = is_number(s)
    xs = []
    pos = 0.0
    for char in s:
        advance = font.advance(char) * scale
        if char.isdigit() and tabular:
            cell = font.tnum * scale
            xs.append(pos + (cell - advance) / 2.0)
            pos += cell
        else:
            xs.append(pos)
            pos += advance
        pos += tracking * size
    return xs, pos - (tracking * size if s else 0.0)


def tw(s, size, weight=600, tracking=0.0):
    """Width of a string in px (figures measured with tabular digits)."""
    return _layout(s, size, weight, tracking)[1]


def esc(s):
    return s.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")


def text(x, y, s, size, weight=600, fill=SNOW, anchor="start", opacity=None, tracking=0.0):
    """One line of text. Digits are set on the font's tabular advance.

    librsvg ignores OpenType `tnum`, so every glyph is placed by hand on the
    same advances Godot will use with `tnum` on.
    """
    xs, total = _layout(s, size, weight, tracking)
    start = x if anchor == "start" else x - total / 2.0 if anchor == "middle" else x - total
    attrs = f'font-size="{size:g}" font-weight="{weight}" fill="{fill}"'
    if opacity is not None:
        attrs += f' fill-opacity="{opacity:g}"'
    if is_number(s) or tracking:
        positions = " ".join(f"{start + offset:.2f}" for offset in xs)
        return f'<text x="{positions}" y="{y:g}" {attrs}>{esc(s)}</text>'
    return f'<text x="{start:.2f}" y="{y:g}" {attrs}>{esc(s)}</text>'


def caption(x, y, s, fill=MIST, anchor="start", size=18, opacity=None):
    """Small caps label: SemiBold, upper case, tracked +6 %."""
    return text(x, y, s.upper(), size, 600, fill, anchor, opacity, tracking=0.06)


def cw(s, size=18):
    return tw(s.upper(), size, 600, 0.06)


# ------------------------------------------------------------ primitives ---

def rect(x, y, w, h, r=0, fill=INK, opacity=None, stroke=None, stroke_opacity=None, sw=1):
    out = f'<rect x="{x:g}" y="{y:g}" width="{w:g}" height="{h:g}"'
    if r:
        out += f' rx="{r:g}"'
    out += f' fill="{fill}"'
    if opacity is not None:
        out += f' fill-opacity="{opacity:g}"'
    if stroke:
        out += f' stroke="{stroke}" stroke-width="{sw:g}"'
        if stroke_opacity is not None:
            out += f' stroke-opacity="{stroke_opacity:g}"'
    return out + "/>"


AREA = []  # (x, y, w, h) of every HUD panel drawn, for the coverage figure


def panel(x, y, w, h, r=12, opacity=0.88, border=0.14, track=True):
    """HUD surface: ink fill, 1 px inside hairline. A StyleBoxFlat."""
    if track:
        AREA.append((x, y, w, h))
    out = rect(x, y, w, h, r, INK, opacity)
    if border:
        out += rect(x + 0.5, y + 0.5, w - 1, h - 1, max(r - 0.5, 0), "none", None, SNOW, border, 1)
    return out


def coverage(width=1280, height=800):
    return sum(w * h for _, _, w, h in AREA) / float(width * height)


def line(x1, y1, x2, y2, stroke=SNOW, sw=1, opacity=None, cap="butt"):
    out = f'<line x1="{x1:g}" y1="{y1:g}" x2="{x2:g}" y2="{y2:g}" stroke="{stroke}" stroke-width="{sw:g}" stroke-linecap="{cap}"'
    if opacity is not None:
        out += f' stroke-opacity="{opacity:g}"'
    return out + "/>"


def poly(points, fill=SNOW, opacity=None, stroke=None, sw=1, join="round"):
    pts = " ".join(f"{x:.2f},{y:.2f}" for x, y in points)
    out = f'<polygon points="{pts}" fill="{fill}"'
    if opacity is not None:
        out += f' fill-opacity="{opacity:g}"'
    if stroke:
        out += f' stroke="{stroke}" stroke-width="{sw:g}" stroke-linejoin="{join}"'
    return out + "/>"


def polyline(points, stroke=SNOW, sw=2, opacity=None):
    pts = " ".join(f"{x:.2f},{y:.2f}" for x, y in points)
    out = f'<polyline points="{pts}" fill="none" stroke="{stroke}" stroke-width="{sw:g}" stroke-linejoin="round" stroke-linecap="round"'
    if opacity is not None:
        out += f' stroke-opacity="{opacity:g}"'
    return out + "/>"


def circle(cx, cy, r, fill=SNOW, opacity=None, stroke=None, sw=1, dash=None):
    out = f'<circle cx="{cx:g}" cy="{cy:g}" r="{r:g}" fill="{fill}"'
    if opacity is not None:
        out += f' fill-opacity="{opacity:g}"'
    if stroke:
        out += f' stroke="{stroke}" stroke-width="{sw:g}"'
    if dash:
        out += f' stroke-dasharray="{dash}"'
    return out + "/>"


def arc(cx, cy, r, a0, a1, stroke=SNOW, sw=4, opacity=None, cap="butt"):
    """Stroke arc. Angles in degrees, clockwise from 12 o'clock."""
    def point(angle):
        rad = math.radians(angle - 90.0)
        return cx + r * math.cos(rad), cy + r * math.sin(rad)
    x0, y0 = point(a0)
    x1, y1 = point(a1)
    large = 1 if abs(a1 - a0) > 180 else 0
    out = (f'<path d="M{x0:.2f} {y0:.2f} A{r:g} {r:g} 0 {large} 1 {x1:.2f} {y1:.2f}" fill="none" '
           f'stroke="{stroke}" stroke-width="{sw:g}" stroke-linecap="{cap}"')
    if opacity is not None:
        out += f' stroke-opacity="{opacity:g}"'
    return out + "/>"


def svg(width, height, body, defs=""):
    return (f'<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" '
            f'width="{width}" height="{height}" viewBox="0 0 {width} {height}" '
            f'font-family="{FAMILY}">\n<defs>{defs}</defs>\n{body}\n</svg>\n')


def plate(name, width=1280, height=800, x=0, y=0):
    return (f'<image xlink:href="plates/{name}.png" x="{x:g}" y="{y:g}" width="{width:g}" '
            f'height="{height:g}" preserveAspectRatio="xMidYMid slice"/>')


def group(body, transform=None, opacity=None, clip=None):
    attrs = ""
    if transform:
        attrs += f' transform="{transform}"'
    if opacity is not None:
        attrs += f' opacity="{opacity:g}"'
    if clip:
        attrs += f' clip-path="url(#{clip})"'
    return f"<g{attrs}>{body}</g>"


# ----------------------------------------------------------------- icons ---

def tri(cx, cy, size, direction, fill=SNOW, opacity=None):
    """Solid arrow head. direction: left, right, up, down."""
    h = size / 2.0
    pts = {
        "right": [(cx - h * 0.8, cy - h), (cx + h * 0.8, cy), (cx - h * 0.8, cy + h)],
        "left": [(cx + h * 0.8, cy - h), (cx - h * 0.8, cy), (cx + h * 0.8, cy + h)],
        "up": [(cx - h, cy + h * 0.8), (cx, cy - h * 0.8), (cx + h, cy + h * 0.8)],
        "down": [(cx - h, cy - h * 0.8), (cx, cy + h * 0.8), (cx + h, cy - h * 0.8)],
    }[direction]
    return poly(pts, fill, opacity)


def chevron(cx, cy, size, fill=SNOW, opacity=None, direction="right"):
    s = size / 2.0
    sign = 1 if direction == "right" else -1
    pts = [(cx - sign * s * 0.5, cy - s), (cx + sign * s * 0.5, cy), (cx - sign * s * 0.5, cy + s)]
    out = (f'<polyline points="{" ".join(f"{x:.1f},{y:.1f}" for x, y in pts)}" fill="none" stroke="{fill}" '
           f'stroke-width="{max(2.0, size / 6.0):g}" stroke-linecap="round" stroke-linejoin="round"')
    if opacity is not None:
        out += f' stroke-opacity="{opacity:g}"'
    return out + "/>"


def pad_button(cx, cy, letter, d=28, fill=SNOW, ink=INK):
    """Generic face button: a disc with the letter. No vendor artwork."""
    size = d * 0.68
    return circle(cx, cy, d / 2.0, fill) + text(cx, cy + size * 0.35, letter, size, 700, ink, "middle")


def pad_menu(cx, cy, w=36, h=26, fill=SNOW, ink=INK):
    """Generic Start/menu button: a pill with three bars."""
    out = rect(cx - w / 2.0, cy - h / 2.0, w, h, h / 2.0, fill)
    for dy in (-5, 0, 5):
        out += line(cx - 7, cy + dy, cx + 7, cy + dy, ink, 2.5, None, "round")
    return out


def pad_dpad(cx, cy, d=28, active=("left", "right"), fill=SNOW):
    """Generic D-pad: a cross; the arms that do something are solid."""
    arm = d * 0.34
    half = d / 2.0
    out = rect(cx - arm / 2.0, cy - half, arm, d, 2, fill, 0.32)
    out += rect(cx - half, cy - arm / 2.0, d, arm, 2, fill, 0.32)
    arms = {
        "up": (cx - arm / 2.0, cy - half, arm, half - arm / 2.0 + 1),
        "down": (cx - arm / 2.0, cy + arm / 2.0 - 1, arm, half - arm / 2.0 + 1),
        "left": (cx - half, cy - arm / 2.0, half - arm / 2.0 + 1, arm),
        "right": (cx + arm / 2.0 - 1, cy - arm / 2.0, half - arm / 2.0 + 1, arm),
    }
    for name in active:
        x, y, w, h = arms[name]
        out += rect(x, y, w, h, 2, fill)
    return out


def keycap(x, cy, label, h=28, fill=SNOW, size=18):
    """Keyboard key: outlined cap. Returns (svg, width)."""
    w = max(h, tw(label, size, 600) + 16)
    out = rect(x + 0.75, cy - h / 2.0 + 0.75, w - 1.5, h - 1.5, 6, "none", None, fill, None, 1.5)
    out += text(x + w / 2.0, cy + size * 0.35, label, size, 600, fill, "middle")
    return out, w


def key_arrow(x, cy, direction, h=28, fill=SNOW):
    out = rect(x + 0.75, cy - h / 2.0 + 0.75, h - 1.5, h - 1.5, 6, "none", None, fill, None, 1.5)
    out += tri(x + h / 2.0, cy, 11, direction, fill)
    return out, h


def signal(x, y, bars, fill=SNOW, h=20):
    """Four ascending bars; `bars` of them solid. y is the baseline."""
    out = ""
    for i in range(4):
        bh = h * (0.35 + 0.65 * i / 3.0)
        out += rect(x + i * 8, y - bh, 5, bh, 1.5, fill, None if i < bars else 0.25)
    return out


def icon_tv(x, y, s=20, fill=SNOW):
    """Screen with a stand, in an s-by-s box at x, y (top left)."""
    out = rect(x + 1, y + 2.5, s - 2, s * 0.62, 2.5, "none", None, fill, None, 2)
    out += line(x + s * 0.3, y + s - 2, x + s * 0.7, y + s - 2, fill, 2, None, "round")
    return out


def icon_keyboard(x, y, s=20, fill=SNOW):
    out = rect(x + 1, y + 4, s - 2, s - 8, 2.5, "none", None, fill, None, 2)
    for i in range(3):
        out += rect(x + 4.5 + i * 4.2, y + 7.5, 2, 2, 0, fill)
    out += line(x + 6, y + s - 7.5, x + s - 6, y + s - 7.5, fill, 2)
    return out


def icon_heart(cx, cy, s=18, fill=SNOW, opacity=None):
    k = s / 24.0
    d = (f"M{cx:.2f} {cy + 9 * k:.2f} C{cx - 14 * k:.2f} {cy - 1 * k:.2f} {cx - 9 * k:.2f} {cy - 11 * k:.2f} "
         f"{cx:.2f} {cy - 4 * k:.2f} C{cx + 9 * k:.2f} {cy - 11 * k:.2f} {cx + 14 * k:.2f} {cy - 1 * k:.2f} "
         f"{cx:.2f} {cy + 9 * k:.2f} Z")
    out = f'<path d="{d}" fill="{fill}"'
    if opacity is not None:
        out += f' fill-opacity="{opacity:g}"'
    return out + "/>"


def icon_check(cx, cy, s=20, stroke=INK, sw=3):
    k = s / 20.0
    pts = [(cx - 6 * k, cy + 0.5 * k), (cx - 1.5 * k, cy + 5 * k), (cx + 6.5 * k, cy - 4.5 * k)]
    return polyline(pts, stroke, sw)


def icon_cross(cx, cy, s=20, stroke=INK, sw=3):
    k = s / 20.0 * 5
    return line(cx - k, cy - k, cx + k, cy + k, stroke, sw, None, "round") + line(cx - k, cy + k, cx + k, cy - k, stroke, sw, None, "round")


def icon_warning(cx, cy, s=24, fill=WARN, ink=INK):
    h = s * 0.9
    pts = [(cx, cy - h / 2.0), (cx + s / 2.0, cy + h / 2.0), (cx - s / 2.0, cy + h / 2.0)]
    out = poly(pts, fill, None, fill, 3)
    out += line(cx, cy - h * 0.18, cx, cy + h * 0.14, ink, 2.6, None, "round")
    out += circle(cx, cy + h * 0.33, 1.6, ink)
    return out


def spinner(cx, cy, r=10, stroke=SNOW, sw=3, at=40):
    """Busy ring: a faint ring and a 100 degree bright arc. Rotates 1 turn/s."""
    return circle(cx, cy, r, "none", None, stroke, sw).replace("/>", ' stroke-opacity="0.25"/>') + arc(cx, cy, r, at, at + 100, stroke, sw, None, "round")


def toggle(x, cy, on, focused=False):
    """Switch 56 x 30. On: knob right and a tick. Off: knob left."""
    w, h = 56, 30
    if focused:
        track, knob, mark = INK, SUN, INK
    else:
        track, knob, mark = (GOOD if on else SLATE), (INK if on else MIST), (GOOD if on else INK)
    out = rect(x, cy - h / 2.0, w, h, h / 2.0, track, None if (on or focused) else 1)
    if focused and not on:
        out = rect(x + 1.25, cy - h / 2.0 + 1.25, w - 2.5, h - 2.5, h / 2.0, "none", None, INK, None, 2.5)
        knob = INK
    kx = x + w - h / 2.0 if on else x + h / 2.0
    out += circle(kx, cy, h / 2.0 - 4, knob)
    if on:
        out += icon_check(kx, cy, 13, track if not focused else INK, 2.5)
    return out


# ---------------------------------------------------------------- course ---

class Course:
    def __init__(self, name, length_m, samples, outline=None):
        self.name = name
        self.length = length_m
        self.samples = samples  # [distance, elevation, grade]
        self.outline = outline or []
        self.ascent = sum(max(0.0, b[1] - a[1]) for a, b in zip(samples, samples[1:]))
        self.top = max(s[1] for s in samples)
        self.low = min(s[1] for s in samples)

    def at(self, distance):
        """(elevation, grade) at a distance, wrapping around the lap."""
        d = distance % self.length
        step = self.samples[1][0] - self.samples[0][0]
        i = min(int(d / step), len(self.samples) - 2)
        a, b = self.samples[i], self.samples[i + 1]
        t = (d - a[0]) / step
        return a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t


def penguin_pass():
    raw = json.load(open(os.path.join(HERE, "course_penguin_pass.json")))
    return Course(raw["name"], raw["length_m"], raw["profile"], raw["outline"])


def generated_course(name, length_km, seed, hilliness=1.0, step=25.0):
    """A stand-in for a generated route: smooth pseudo-random grades."""
    state = seed & 0x7FFFFFFF or 1

    def rnd():
        nonlocal state
        state = (state * 1103515245 + 12345) & 0x7FFFFFFF
        return state / float(0x7FFFFFFF)

    count = int(length_km * 1000 / step)
    # climbs a few kilometres long, on top of two or three big ranges
    knots = max(5, int(length_km / 3.0))
    values = [(rnd() * 2 - 1) * 6.5 * hilliness for _ in range(knots + 2)]
    values[0] = values[1] = 0.0
    big = [(rnd() * 2 - 1) * 2.5 * hilliness for _ in range(max(3, knots // 5) + 2)]
    samples = []
    elevation = 0.0
    for i in range(count + 1):
        t = i / count
        a = t * knots
        k = int(a)
        f = a - k
        f = f * f * (3 - 2 * f)
        grade = values[k] + (values[min(k + 1, knots + 1)] - values[k]) * f
        b = t * (len(big) - 2)
        kb = int(b)
        fb = b - kb
        grade += big[kb] + (big[kb + 1] - big[kb]) * fb
        samples.append([i * step, elevation, grade])
        elevation += grade / 100.0 * step
    low = min(s[1] for s in samples)
    for s in samples:
        s[1] -= low
    return Course(name, length_km * 1000.0, samples)


def profile_chart(course, x, y, w, h, d0, d1, bin_px=4.0, min_span=60.0, top_stroke=SNOW,
                  fill_opacity=0.92, done_until=None, wrap=False, floor=None, sw=2.0):
    """Gradient-coloured elevation area from distance d0 to d1 (metres).

    Each column of about `bin_px` px is filled with the ramp colour of its
    mean grade; a light line runs along the top. Columns before `done_until`
    are dimmed (already ridden). The vertical scale spans at least `min_span`
    metres so a flat road draws flat.
    """
    columns = max(2, int(round(w / bin_px)))
    step = (d1 - d0) / columns
    points = []
    for i in range(columns + 1):
        d = d0 + i * step
        if wrap or 0 <= d <= course.length:
            elevation, _ = course.at(d if wrap else min(max(d, 0), course.length - 0.01))
            # keep laps continuous when the window crosses the line
            points.append((d, elevation))
        else:
            points.append((d, None))
    known = [e for _, e in points if e is not None]
    low = min(known) if floor is None else floor
    span = max(max(known) - low, min_span)
    headroom = 0.92

    def py(elevation):
        return y + h - (elevation - low) / span * h * headroom

    out = ""
    top = []
    for i in range(columns):
        (da, ea), (db, eb) = points[i], points[i + 1]
        if ea is None or eb is None:
            continue
        xa = x + w * i / columns
        xb = x + w * (i + 1) / columns
        grade = (eb - ea) / (db - da) * 100.0
        if wrap and abs(grade) > 40:
            grade = 0.0
        done = done_until is not None and db <= done_until + 1e-6
        out += poly([(xa, y + h), (xa, py(ea)), (xb + 0.4, py(eb)), (xb + 0.4, y + h)],
                    RIDDEN if done else grade_color(grade), None if done else fill_opacity)
        if not top:
            top.append((xa, py(ea)))
        top.append((xb, py(eb)))
    if top_stroke and len(top) > 1:
        out += polyline(top, top_stroke, sw, 0.95)
    return out, py


def fmt_clock(seconds):
    seconds = int(seconds)
    return f"{seconds // 3600}:{seconds % 3600 // 60:02d}:{seconds % 60:02d}"


def logo(x, y, size, uid="logo"):
    """The brand mark from docs/img/logo.svg, inlined at `size` px."""
    raw = open(os.path.join(ROOT, "docs", "img", "logo.svg")).read()
    inner = raw[raw.index(">", raw.index("<svg")) + 1:raw.rindex("</svg>")]
    for name in ("badge", "sky", "glow", "snow", "title", "desc"):
        inner = inner.replace(f'id="{name}"', f'id="{uid}-{name}"').replace(f"url(#{name})", f"url(#{uid}-{name})")
    start = inner.index("<title")
    inner = inner[:start] + inner[inner.index("</desc>") + 7:]
    return f'<g transform="translate({x:g} {y:g}) scale({size / 512.0:g})">{inner}</g>'
