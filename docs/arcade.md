# Arcade

`pp-arcade` is the fish, the powerups, and the rider profile. It does not
draw, talk to a trainer, or change the rider's real watts. The session
applies a mass or drag change to a copy of the rider profile and integrates
that copy. The trainer still measures the watts the rider makes.

Lane geometry is plain numbers. Pass `5` for the slot count. Slot 0 is
nearest the centre line and slot 4 is nearest the edge. The view eases a
one-slot move over 0.3 s. At 60 km/h that move covers 5 m, so a pattern
never puts an *n*-slot jump closer than *n* × 5 m.

## Fish

`FishField::generate(course_length_m, grade_at, seed, slots)` lays one lap.
The same length, grade profile, seed, and slot count always lay the same
fish. A non-finite grade is treated as flat. A lap shorter than 150 m, or
zero slots, lays nothing.

Fish sit in the closed interval from 100 m to `length - 50` m. The first
group starts at 100 m. After that, the clear road from the last fish of one
group to the first fish of the next is 150 m to 400 m. Inside a group the
spacing is one constant from 6 m up to 10 m.

The grade at the start of the group picks the shape and the gap:

| Grade | Shapes | Fish in a line or zigzag | Gap after the group |
| --- | --- | --- | --- |
| 5% and up | Line only | 5 | 320 m to 400 m |
| 2% up to 5% | Line or diagonal | 5 or 6 | 240 m to 320 m |
| Below 2%, including descents | Line, diagonal, zigzag, or a golden fish | 5 to 8 | 150 m to 250 m |

A diagonal is one fish per slot, from one edge of the lane to the other.
A zigzag steps one slot at a time and turns back at the line. A golden fish
is a group of one, two slots off the previous group's last slot (or off the
middle slot when it is the first group). About one easy group in ten is
golden. Steep road never gets one.

Penguin Pass with seed `1` lays 17 groups and 88 fish. Two of them:

```text
  100 m  diagonal  4 3 2 1 0     every 7.1 m
 3409 m  zigzag    2 1 0 1 2 3 4 every 6.3 m
```

The diagonal starts on the edge and steps in to the centre side. The
zigzag starts in the middle, touches the centre line, and weaves out to
the edge. The climb above is mostly lines of five. One golden fish sits at
4602 m in slot 2, two slots off the diagonal that ended in slot 0. A flat
10 km course with the same seed lays 43 groups and 228 fish.

Fish come back each lap. `FishRun` is the per-ride state.

`tick(prev_distance_m, distance_m, slot)` collects every fish in the open
interval `(prev, distance]`. `slot` is the rider's position at
`distance_m`, as a fraction from 0.0 to 4.0. A fish is collected when that
fraction is within half a slot of the fish, including the boundary, so 2.5
can take a fish in slot 2 or slot 3. Anything else is a miss, and a miss
resets the combo. The combo counts consecutive fish along the road, so a
fish in the wrong slot breaks it. Totals count every lap. Reporting the
same interval twice emits nothing.

`ahead(distance_m, window_m)` returns the fish whose next appearance is
inside that window, nearest first. A fish already passed this lap shows
again only when the next lap's copy is inside the window.

Use the eased offset, not the destination slot:

```text
t = (offset_m - inner_m) / (outer_m - inner_m)
slot = t * 4
```

The whole interval is judged at the slot from the end of the frame. At
30 Hz and 80 km/h a frame is under a metre, and fish in a group are at
least 6 m apart.

## Powerups

One slot holds one powerup that has not been used. `award` while the slot
is full is dropped. `activate` empties the slot and starts the effect, so
another powerup can be awarded while one is running. Activating during an
effect replaces it and does not emit `Expired`. `modifiers` is identity
until an effect is running: holding a powerup does not change the bike.

The kind comes from a seeded draw weighted by the grade. The reason is
recorded and does not change the kind. A non-finite grade is treated as
flat. Weights are Feather, Aero, Burst, out of 100:

| Grade | Feather | Aero | Burst |
| --- | --- | --- | --- |
| 4% and up | 70 | 15 | 15 |
| Between −2% and 4% | 25 | 45 | 30 |
| −2% and below | 10 | 50 | 40 |

For a 75 kg rider and a 9 kg bike (84 kg, CdA 0.32 m², Crr 0.004) at a
steady 200 W, equilibrium speed from `pp-course`:

| | Duration | Change | Flat | 6% climb |
| --- | --- | --- | --- | --- |
| Nothing | | | 34.2 km/h | 13.0 km/h |
| Feather | 20 s | −10 kg total mass | 34.5 km/h | 14.5 km/h |
| Aero | 25 s | drag area × 0.75 | 37.5 km/h | 13.2 km/h |
| Burst | 8 s | drag area × 0.45 | 43.9 km/h | 13.4 km/h |

Feather is the climb tool. Aero and Burst are the flat and descent tools.
None of them doubles the speed. The watts passed into the force balance
stay 200 W.

`modifiers()` returns `PhysicsModifiers { mass_delta_kg, cda_scale }`.
`apply` adds the mass delta to the total, keeps the bike mass when it
still fits, and multiplies CdA. Total mass stays at or above 1 kg. FTP is
copied unchanged. Apply it to the settings profile every frame, not to the
previous modified copy, and do not write it back to settings.

## Badges

`Badge::catalog()` is the list, in the order a ride earns them. Ids do not
change. `progress` returns `(current, target)` in the badge's unit and does
not cap `current`. Distance and climb are meters. The hour badge is
seconds.

| Id | Name | What it takes | Kit |
| --- | --- | --- | --- |
| `first-ride` | First splash | A ride of at least a minute | `kit-sunrise` |
| `first-loop` | Round the mountain | A lap of Penguin Pass | |
| `distance-10` | 10 km | 10 km in total | |
| `distance-50` | 50 km | 50 km in total | |
| `distance-100` | 100 km | 100 km in total | |
| `distance-500` | 500 km | 500 km in total | |
| `distance-1000` | 1,000 km | 1,000 km in total | |
| `climb-1000` | Glacier line | 1,000 m climbed | `kit-glacier` |
| `climb-5000` | High country | 5,000 m climbed | |
| `climb-8849` | Above Everest | 8,849 m climbed | `kit-kom-dots` |
| `fish-100` | A handful | 100 fish | |
| `fish-1000` | A shoal | 1,000 fish | |
| `fish-10000` | The school | 10,000 fish | |
| `combo-25` | Clean line | A combo of 25 | |
| `streak-3` | Three days | Ten minutes on 3 days in a row | |
| `streak-7` | One week | Ten minutes on 7 days in a row | |
| `streak-30` | A month | Ten minutes on 30 days in a row | |
| `every-course` | Every road | Penguin Pass, flat, rolling, mountain, and desert | |
| `segment-record` | Segment record | A segment record | |
| `hour-ride` | One hour | A ride of at least an hour | |

Kits are ids only. A later pass gives them colours.

A ride under 60 s changes nothing: no totals, no streak, no badge. A ride
of at least 60 s and under 600 s updates totals and badges but does not
move the streak and does not consume the day. An impossible calendar day
does not move the streak either. The next local day continues the streak.
A gap starts it again at one. A ride dated before the last streak day
leaves the streak where it is. Each badge is earned once.

Course ids for the every-course badge are `penguin_pass`, `flat`,
`rolling`, `mountain`, and `desert`. `penguin_pass` matches the settings
file. Generated routes use the terrain name. Any other id is remembered
and does not count.

## Profile file

TOML. The session chooses the path. Put `arcade.toml` next to the settings
file.

A missing file is a fresh profile and is not created. A file of only
comments is a fresh profile and is left in place. A file that is not TOML
is moved to `<name>.bad` (`arcade.toml` becomes `arcade.toml.bad`) and
replaced with a fresh profile. Unknown keys are ignored. Missing keys load
as zero, so an older file still opens. Negative numbers load as zero.
Unknown badge ids are kept.

Save writes a temporary file in the same directory and renames it over the
destination. The parent directory is created if needed. Kits are not
stored; they follow the badges.

```toml
version = 1
rides = 4
time_s = 7200.0
distance_m = 42000.0
ascent_m = 900.0
fish = 120
golden_fish = 4
best_combo = 18
longest_ride_s = 3600.0
streak_days = 3
penguin_pass_laps = 2
segment_records = 0
courses = ["desert", "penguin_pass"]

[streak_last]
year = 2026
month = 11
day = 20

[[badges]]
id = "first-ride"
earned = { year = 2026, month = 11, day = 18 }
```

## Calls

Build the field when the course is built. Use seed `1` for Penguin Pass so
the loop keeps its fish, and the route seed for a generated course.

```rust
let field = FishField::generate(
    course.length_m(),
    |distance_m| course.grade_pct_at(distance_m),
    seed,
    5,
);
let mut fish = FishRun::new(&field);
let mut power = PowerupSlot::new(seed);
```

Each frame, after the ride steps, with `prev_m` the total distance from
the previous frame:

```rust
let events = fish.tick(prev_m, telemetry.distance_m, slot);
for event in events {
    match event {
        FishEvent::Collected { golden: true, .. } => {
            let _ = power.award(AwardReason::GoldenFish, telemetry.grade_pct);
        }
        FishEvent::Collected { combo, .. } if combo > 0 && combo % 10 == 0 => {
            let _ = power.award(AwardReason::Combo, telemetry.grade_pct);
        }
        FishEvent::Collected { .. } | FishEvent::Missed { .. } => {}
    }
}
let _ = power.tick(dt_s);
let draw = fish.ahead(telemetry.distance_m, 120.0);
ride.set_rider(power.modifiers().apply(settings.rider_profile()));
```

The powerup button calls `power.activate()`. A finished segment calls
`power.award(AwardReason::Segment, grade_pct)`. A finished lap calls
`power.award(AwardReason::Lap, grade_pct)`. A full slot returns `None`.

At the end of the ride, copy the fish totals and the best combo off
`FishRun` into a `RideOutcome`. Set `course_id`, `penguin_pass_laps`, and
`segment_record`. Pass the local date as year, month, day.

```rust
let mut profile = Profile::load(path);
let new_badges = profile.apply_ride(&outcome, local_date);
profile.save(path)?;
```

`new_badges` is the badges this ride just earned, in catalog order.
`profile.kits()` is the unlocked kit ids. `Badge::catalog()` and
`profile.progress(badge)` fill a progress screen.
