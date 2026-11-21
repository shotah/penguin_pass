# Course and bike physics

`pp-course` is the road and the longitudinal bike model. It does not
link Godot or Bluetooth. The Godot extension samples the centerline to
build the mesh and calls `Ride::step` each frame.

## Penguin Pass

Closed loop. The start line is on the south side, in the middle of a
short flat, heading west. The road climbs north through five switchbacks
on the west side, rolls east along the ridge, descends through three
wider bends on the east side, and returns to the line.

| | |
| --- | --- |
| Lap length | 5000.000 m along the 3D centerline |
| Total ascent | 169.3 m (equals the descent) |
| Average climb grade | 7.3% from 304 m to 2473 m |
| Elevation | 0 m at the line, crest 163.4 m at 2884 m |
| Grade | −10.83% to +10.00%, sampled every 1 m |
| Steepest grade change | 0.21 percentage points per meter |
| Minimum horizontal radius | 44.3 m (climb hairpins) |
| Minimum plan clearance | 57.0 m, ignoring the 60 m of road on either side of a point |
| Footprint | 955.1 m east-west by 443.1 m north-south, centered on the origin |

The flat is the first and last ~250 m. The climb, about 300 m to 2470 m,
changes pitch with the road: the straights sit around 7.6–8.8%, the
hairpins ease to about 5.2%, a 3.5% breather covers the middle
switchback (about 1140–1240 m along the horizontal path), and a 10%
kick of about 200 m is the last straight before the ridge. The ridge,
about 2480–3010 m horizontally, is two rolls of ±7%. The descent,
about 3010–4740 m, runs from about −9% to −10.8%, with a 120 m easing
to −5.5% on a bend. A default rider at a steady 200 W takes 15.4
minutes for the lap, about 11.6 of them on the climb.

Distance below is meters along the 3D centerline. The grade spans in
the file are measured along horizontal distance, which is a few tenths
of a percent shorter. Elevation is meters above the start line. Grade
is percent, positive uphill.

```text
3D m   grade
   0   .......... flat
 500   #######...  7.6%
 700   #####.....  5.2% hairpin
1000   ########..  8.6%
1200   ###.......  3.5% breather
1500   ########..  8.0%
1700   #####.....  5.2% hairpin
2000   #########.  8.8%
2100   #####.....  5.2% hairpin
2400   ########## 10% kick
2600   ###....... ridge roll
3100   #########. descent
4300   ######.... −5.5% easing
4700   .......... flat
```

| Distance (m) | Elevation (m) | Grade (%) |
| --- | --- | --- |
| 0 | 0.00 | 0.00 |
| 100 | 0.00 | 0.00 |
| 200 | 0.00 | 0.00 |
| 300 | 0.35 | 1.80 |
| 400 | 4.59 | 6.19 |
| 500 | 12.05 | 7.60 |
| 600 | 19.63 | 7.60 |
| 700 | 26.63 | 5.20 |
| 800 | 31.82 | 5.26 |
| 900 | 39.87 | 8.60 |
| 1000 | 48.44 | 8.60 |
| 1100 | 57.01 | 8.60 |
| 1200 | 61.73 | 3.50 |
| 1300 | 66.87 | 8.00 |
| 1400 | 74.84 | 8.00 |
| 1500 | 82.82 | 8.00 |
| 1600 | 90.02 | 5.20 |
| 1700 | 95.22 | 5.52 |
| 1800 | 103.57 | 8.80 |
| 1900 | 112.33 | 8.80 |
| 2000 | 121.10 | 8.80 |
| 2100 | 127.02 | 5.20 |
| 2200 | 132.49 | 8.43 |
| 2300 | 142.36 | 10.00 |
| 2400 | 152.31 | 10.00 |
| 2500 | 157.60 | 2.18 |
| 2600 | 163.06 | 3.07 |
| 2700 | 159.40 | −6.58 |
| 2800 | 159.20 | 6.39 |
| 2900 | 163.14 | −2.59 |
| 3000 | 157.68 | −2.67 |
| 3100 | 153.54 | −9.03 |
| 3200 | 144.55 | −9.03 |
| 3300 | 134.61 | −10.43 |
| 3400 | 124.23 | −10.43 |
| 3500 | 113.85 | −10.43 |
| 3600 | 103.39 | −10.83 |
| 3700 | 92.62 | −10.83 |
| 3800 | 81.85 | −10.71 |
| 3900 | 72.13 | −9.63 |
| 4000 | 62.54 | −9.63 |
| 4100 | 52.96 | −9.63 |
| 4200 | 43.37 | −9.45 |
| 4300 | 37.31 | −5.52 |
| 4400 | 30.57 | −9.63 |
| 4500 | 20.98 | −9.63 |
| 4600 | 11.40 | −9.63 |
| 4700 | 1.88 | −8.32 |
| 4800 | 0.00 | 0.00 |
| 4900 | 0.00 | 0.00 |
| 5000 | 0.00 | 0.00 |

## Coordinates

Meters, right-handed, Y up. A position is `[x, y, z]`: `x` east, `y`
elevation, `z` north. Heading zero points toward +x. A positive arc
angle turns toward +z (left if you are facing +x).

The tangent from `tangent_at` is the direction of travel, already a
unit vector. Point the rider along that vector. Godot's camera forward
is −Z; this course does not use that as its travel direction.

Distance arguments wrap with Euclidean remainder, including negatives
and distances past the lap.

## Data file

`crates/pp-course/courses/penguin_pass.json` is embedded with
`include_str!` and parsed with serde. `Course::penguin_pass` returns
that loop. Any other text goes through `FromStr` (`str::parse` or
`Course::from_str` with the trait in scope). Invalid data is a
`CourseError`, not a panic.

The file is a horizontal path plus a grade profile along horizontal
distance. Segment lengths and arc radii are meters. Arc angles are
radians, positive left. Grade spans must sum to the horizontal length,
meet at the same grade (no steps), stay within ±100%, and bring the
elevation back to the datum. Penguin Pass itself stays within ±12%.

```json
{
  "name": "Penguin Pass",
  "start": { "x": 0.0, "z": 0.0, "heading_rad": 3.141592653589793 },
  "segments": [
    { "type": "line", "length_m": 246.046 },
    { "type": "arc", "radius_m": 44.288, "angle_rad": -3.141592653589793 }
  ],
  "grade": [
    { "type": "hold", "length_m": 246.046, "grade_pct": 0.0 },
    { "type": "smooth", "length_m": 268.016, "to_grade_pct": 8.516 },
    { "type": "wave", "length_m": 529.0, "amplitude_pct": 7.0, "cycles": 2 }
  ]
}
```

`smooth` eases from the previous grade to `to_grade_pct` with a
smoothstep, so the grade and its slope match the spans on either side.
`wave` is `amplitude_pct * sin(2π * cycles * s / length)` and must start
from 0%. The checked-in loop was generated by
`crates/pp-course/examples/generate_penguin_pass.rs`: the same straights
and arcs as before (so the plan and the radius stay put), scaled until
the 3D length is 5000 m, then a varied climb, two ridge rolls, and a
varied descent whose areas cancel. The example writes a scratch file
under `/tmp`, checks it, and replaces the embedded JSON with one rename.

## Bike model

Point mass. Grade percent is rise over horizontal run, so the road
angle is `theta = atan(grade_pct / 100)`.

```text
F = P / max(v, 1 m/s)
    - m g sin(theta)
    - m g Crr cos(theta)
    - 0.5 * rho * CdA * v^2
a = F / m
```

| Constant | Value |
| --- | --- |
| `g` | 9.80665 m/s² |
| `rho` | 1.225 kg/m³ |
| Low-speed floor | 1 m/s |
| Descent cap | 80 km/h (22.222 m/s) |
| Integration step | at most 0.05 s |
| Longest `dt` | 60 s, then clamped |

`m` is rider mass plus bike mass. `Crr` and `CdA` come from
`RiderProfile`. FTP is not used. Negative power is coasting. Below
1 m/s the propulsive force is `P / 1 m/s`, so a standing start does not
divide by zero and does not explode. At and above 1 m/s, `force * speed`
equals `P` before the resistance terms.

`acceleration_mps2` returns that acceleration and does not clamp it.
`Ride` integrates with semi-implicit Euler, holds speed in
`0..=80 km/h`, and sub-steps any `dt` above 0.05 s. The bike stops on a
climb; it does not roll backwards. The elapsed clock stays at 0 until
the first sample with power above 0 W, then it keeps running while the
rider coasts or stands. A zero, negative, or non-finite `dt` leaves the
ride unchanged.

`Ride::set_rider` replaces the profile used by later steps. Speed,
distance, lap, and elapsed time stay where they are. The profile is
stored as given, the same as `Ride::new`. A non-positive or non-finite
total mass does not poison the ride: the next step treats that mass as
zero acceleration, so speed stays finite.

For the default rider (75 kg + 9 kg, CdA 0.32 m², Crr 0.004) the force
balance settles at 34.2 km/h at 200 W on the flat, and 12.6 km/h at
250 W on an 8% grade.
