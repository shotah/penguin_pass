#!/usr/bin/env python3
"""Sample the Penguin Pass course into a small JSON the mockups draw from.

Reads godot/routes/penguin_pass/route.json (never written) and writes
docs/img/ui/tools/course_penguin_pass.json: elevation and grade every 5 m and
the road centreline every 10 m. The mockup generator reads only the snapshot,
so a later course edit does not silently change the pictures.

    python3 docs/img/ui/tools/snapshot_course.py
"""
import json
import math
import os

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.normpath(os.path.join(HERE, "..", "..", "..", ".."))
SOURCE = os.path.join(ROOT, "godot", "routes", "penguin_pass", "route.json")
OUT = os.path.join(HERE, "course_penguin_pass.json")
STEP = 5.0


def smoothstep(t):
    return t * t * (3.0 - 2.0 * t)


def grade_at(spans, s):
    """Grade in percent at distance s. Mirrors pp-course's hold/smooth/wave."""
    start = 0.0
    previous = 0.0
    for span in spans:
        length = span["length_m"]
        if s <= start + length:
            local = s - start
            kind = span["type"]
            if kind == "hold":
                return span["grade_pct"]
            if kind == "smooth":
                return previous + (span["to_grade_pct"] - previous) * smoothstep(local / length)
            omega = math.tau * span["cycles"] / length
            return span["amplitude_pct"] * math.sin(omega * local)
        start += length
        kind = span["type"]
        previous = span["grade_pct"] if kind == "hold" else span.get("to_grade_pct", 0.0)
    return previous


def main():
    course = json.load(open(SOURCE))
    spans = course["grade"]
    length = sum(span["length_m"] for span in spans)

    profile = []
    elevation = 0.0
    fine = 0.5
    steps = int(round(length / fine))
    every = int(STEP / fine)
    ascent = 0.0
    for index in range(steps + 1):
        s = index * fine
        grade = grade_at(spans, min(s, length))
        if index % every == 0:
            profile.append([round(s, 1), round(elevation, 2), round(grade, 2)])
        rise = grade / 100.0 * fine
        elevation += rise
        if rise > 0:
            ascent += rise

    x, z, heading = course["start"]["x"], course["start"]["z"], course["start"]["heading_rad"]
    outline = [[round(x, 1), round(z, 1)]]
    for segment in course["segments"]:
        if segment["type"] == "line":
            pieces = max(1, int(segment["length_m"] / 10.0))
            for _ in range(pieces):
                x += math.cos(heading) * segment["length_m"] / pieces
                z += math.sin(heading) * segment["length_m"] / pieces
                outline.append([round(x, 1), round(z, 1)])
        else:
            angle, radius = segment["angle_rad"], segment["radius_m"]
            pieces = max(4, int(abs(angle) * radius / 10.0))
            for _ in range(pieces):
                step = angle / pieces
                chord = 2.0 * radius * math.sin(abs(step) / 2.0)
                heading += step / 2.0
                x += math.cos(heading) * chord
                z += math.sin(heading) * chord
                heading += step / 2.0
                outline.append([round(x, 1), round(z, 1)])

    json.dump(
        {
            "name": course["name"],
            "length_m": round(length, 1),
            "ascent_m": round(ascent, 1),
            "max_elevation_m": round(max(p[1] for p in profile), 1),
            "profile": profile,
            "outline": outline,
        },
        open(OUT, "w"),
        separators=(",", ":"),
    )
    print(f"{course['name']}: {length:.0f} m, ascent {ascent:.0f} m, "
          f"top {max(p[1] for p in profile):.0f} m, end {profile[-1][1]:.1f} m, "
          f"{len(profile)} samples, {len(outline)} outline points")
    close = math.hypot(outline[0][0] - outline[-1][0], outline[0][1] - outline[-1][1])
    print(f"outline closes within {close:.1f} m")


if __name__ == "__main__":
    main()
