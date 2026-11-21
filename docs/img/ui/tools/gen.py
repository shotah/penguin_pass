#!/usr/bin/env python3
"""Write every mockup SVG under docs/img/ui/src/.

    python3 docs/img/ui/tools/gen.py

Standard library only. `render.sh` turns the SVGs into PNGs.
"""
import os

import hud as H
import menus as m
import pp_ui as u
import screens as sc
import sheets
from pp_ui import INK, MIST, SNOW, SUN, text

SRC = os.path.join(u.UI, "src")
COURSE = u.penguin_pass()
LONG = u.generated_course("New route", 100, 129, 1.6)
ROUTE = u.generated_course("New route", 42, 172, 1.9)
FTP = 250
TV = "192.168.1.20:7878"

STATES = {
    # steady tempo on the first ramp
    "steady": dict(plate="switchback-chase", power=215, ftp=FTP, speed=16.4, cadence=86, hr=141, grade=4.9,
                   distance=360.0, lap=1, elapsed=58, elevation=2, trainer="controlled", tv=TV),
    # hard effort on the steepest pitch
    "climb": dict(plate="climb-chase", power=345, ftp=FTP, speed=11.8, cadence=68, hr=176, grade=10.0,
                  distance=2105.0, lap=1, elapsed=452, elevation=134, trainer="controlled", tv=TV),
    # bright snow, no heart-rate strap, trainer that only reports power
    "summit": dict(plate="summit-chase", power=188, ftp=FTP, speed=23.8, cadence=92, hr=None, grade=2.5,
                   distance=2500.0, lap=1, elapsed=609, elevation=158, trainer="reading", tv=None),
    # coasting downhill
    "descent": dict(plate="descent-chase", power=0, ftp=FTP, speed=62.4, cadence=0, hr=128, grade=-9.8,
                    distance=3810.0, lap=1, elapsed=714, elevation=80, trainer="controlled", tv=TV),
    # the link dropped: power and cadence are unknown, the bike coasts
    "reconnecting": dict(plate="summit-chase", power=None, ftp=FTP, speed=19.2, cadence=None, hr=152, grade=2.5,
                         distance=2500.0, lap=1, elapsed=609, elevation=158, trainer="reconnecting", tv=TV),
    # no trainer: the keyboard drives
    "simulated": dict(plate="start-chase", power=150, ftp=FTP, speed=14.9, cadence=85, hr=None, grade=0.0,
                      distance=10.0, lap=1, elapsed=3, elevation=0, trainer="sim", tv=None, keyboard=True),
    "first-person": dict(plate="climb-fp", power=262, ftp=FTP, speed=13.1, cadence=79, hr=163, grade=8.6,
                         distance=900.0, lap=1, elapsed=175, elevation=40, trainer="controlled", tv=TV),
    # second lap, four-digit power: the widest the console gets
    "sprint": dict(plate="start-chase", power=1085, ftp=FTP, speed=48.6, cadence=118, hr=181, grade=0.0,
                   distance=4986.0 + 120.0, lap=2, elapsed=786, elevation=0, trainer="controlled", tv=TV),
}
LIVE = STATES["steady"]


def write(name, body, width=1280, height=800, defs=""):
    open(os.path.join(SRC, f"{name}.svg"), "w").write(u.svg(width, height, body, defs))


def hud_body(state, course=COURSE):
    body = H.hud(state, course)
    if state.get("trainer") == "sim":
        body += H.power_hint(state.get("keyboard", False))
    if state.get("trainer") == "reconnecting":
        body += m.toast(640, 32, "warn", "Trainer disconnected. Reconnecting.")
    return body


def hud_mock(name, state, course=COURSE):
    u.AREA.clear()
    write(name, u.plate(state["plate"]) + hud_body(state, course))
    print(f"{name}: HUD covers {u.coverage() * 100:.1f} % of the frame")


def over_ride(body, state=LIVE):
    """A menu over the running ride. The console and chips are hidden while
    a menu is open; the live strip in the menu header carries power and
    speed instead."""
    return u.plate(state["plate"]) + body


def main():
    # --- ride HUD --------------------------------------------------------
    for key in STATES:
        hud_mock(f"hud-{key}", STATES[key])
    at = 71050.0
    long_state = dict(plate="climb-chase", power=228, ftp=FTP, speed=13.9, cadence=88, hr=149,
                      grade=LONG.at(at)[1], distance=at, lap=None, elapsed=11560, elevation=LONG.at(at)[0],
                      trainer="controlled", tv=TV)
    hud_mock("hud-long-route", long_state, LONG)
    u.AREA.clear()
    tv_state = STATES["first-person"] | dict(plate="tv-1080")
    body = u.plate("tv-1080", 1920, 1080) + u.group(hud_body(tv_state), "translate(96 0) scale(1.35)")
    write("hud-tv-1080", body, 1920, 1080)

    # --- title -----------------------------------------------------------
    write("title", sc.title(COURSE), defs=sc.TITLE_DEFS)
    write("title-tv-1080", sc.title_1080(COURSE), 1920, 1080, sc.TITLE_DEFS)
    write("title-searching", sc.title(COURSE, focus=1, trainer=("searching", "")), defs=sc.TITLE_DEFS)
    write("title-keyboard", sc.title(COURSE, focus=2, trainer=("none", ""), keyboard=True),
          defs=sc.TITLE_DEFS)

    # --- settings ----------------------------------------------------------
    write("settings-default", over_ride(m.settings(LIVE)))
    write("settings-edit-value", over_ride(m.settings(LIVE, "difficulty", True, dict(difficulty="55 %"))))
    write("settings-toggle", over_ride(m.settings(LIVE, "follow", values=dict(follow=False), pressed=False)))
    write("settings-weight-note", over_ride(m.settings(
        LIVE, "rider", True, dict(rider="76.5 kg"), note="Changed. The new weight applies when the ride restarts.")))
    write("settings-tv-error", over_ride(m.settings(
        LIVE, "tv", tv_error="Could not start: port 7878 is in use", keyboard=True)))

    # --- pairing -----------------------------------------------------------
    for state in ("searching", "found", "connecting", "connected", "failed", "unavailable"):
        write(f"pairing-{state}", over_ride(m.pairing(state, live=LIVE)))

    # --- dialog, toast -------------------------------------------------------
    base = over_ride(m.settings(LIVE, "restart", show_hints=False))
    write("confirm-restart", base + m.dialog("Restart ride?", ["You go back to the start line.",
                                                                "Time and distance return to zero."],
                                              "Keep riding", "Restart"))
    base = over_ride(m.settings(LIVE, "quit", show_hints=False))
    write("confirm-quit", base + m.dialog("Quit this ride?", ["The ride ends here and you see your summary."],
                                           "Keep riding", "Quit"))
    body = u.plate("switchback-chase") + H.hud(LIVE, COURSE)
    body += m.toast(640, 32, "ok", "Settings saved")
    body += m.toast(640, 92, "warn", "Settings file was damaged. Defaults are in use.")
    body += m.toast(640, 152, "bad", "Trainer disconnected")
    write("toasts", body)

    # --- summary, course picker ---------------------------------------------
    write("summary", u.plate("summit-chase") + m.summary(COURSE))
    write("summary-no-heart", u.plate("summit-chase") + m.summary(COURSE, heart=False, keyboard=True))
    write("course-picker", u.plate("climb-chase") + m.course_picker(COURSE, ROUTE, "loop"))
    write("course-picker-new-route", u.plate("climb-chase") + m.course_picker(COURSE, ROUTE, "new"))

    # --- reference sheets ------------------------------------------------------
    write("sheet-tokens", sheets.tokens())
    write("sheet-components", sheets.components(), 1280, 1400)
    body, height = sheets.anatomy(STATES["climb"], COURSE)
    write("sheet-console-anatomy", body, 1280, height)

    # --- TV dashboard ---------------------------------------------------------
    write("tv-dashboard-1080", sc.tv_dashboard(COURSE, STATES["climb"]), 1920, 1080)
    write("tv-dashboard-reconnecting", sc.tv_dashboard(COURSE, STATES["climb"], "reconnecting"), 1920, 1080)
    write("tv-dashboard-phone", sc.tv_phone(COURSE, STATES["climb"]), 390, 844)


if __name__ == "__main__":
    main()
