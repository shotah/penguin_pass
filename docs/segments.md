# Segments, records, and the ghost

`pp-segments` finds something to chase on a course and times it. It does not link Godot, Bluetooth, or the network. The session owns the ride clock and calls into this crate. The drawing stays in the front end.

Distance below is metres along the 3D centerline of one lap. Grade is percent, positive uphill. Time is seconds on the ride clock.

## What a segment is

A segment is a stretch of one lap.

| Kind | What it is |
| --- | --- |
| Lap | The whole lap, from the start line back to itself. |
| Climb | A sustained uphill. |
| Sprint | The flattest stretch of 200 to 300 m that is not on a climb. |

`start_m` and `end_m` are metres from the start line. When `end_m` is greater than `start_m`, the segment stays inside the lap. When `end_m` is smaller, it crosses the start line. The lap runs from 0 to the lap length.

Segments do not overlap, except that the lap covers all of them.

The id is stable for a given course, so a record survives a restart.

| Id | Segment |
| --- | --- |
| `lap` | The lap. |
| `sprint` | The one sprint. |
| `climb-273` | A climb whose start rounds to that metre. |

The longest climb is named after the course with any trailing route distance stripped (`Penguin Pass climb`, `Rolling climb`, `Desert climb`). Any other climb is numbered in lap order (`Climb 2`, `Climb 3`, …). The start arch shows the segment name and its actual length (for example `Desert climb` and `750 m` on two lines). The sprint is `Sprint`. The lap is `Lap`.

## How they are found

`detect` samples the grade every metre.

A climb is at least 400 m with a mean grade of at least 3%. The body is where the grade stays at or above 2.5%. A downhill nick of up to 80 m, or a gentler breather of up to 150 m, stays inside the climb. The start walks back up to 150 m while the grade is still at least 0.5%, so the ramp is included and the flat in front of it is not. The finish is the crest. If the span averages under 3%, metres are dropped from the weaker end until it qualifies or it gets shorter than 400 m. The same course always produces the same segments.

The sprint is the window of 200 to 300 m, outside every climb, with the lowest mean absolute grade. Ties go to the longer window, then the one that starts earlier. A flat course, whose grade never holds 3% for 400 m, is a sprint and the lap.

On the built-in loop the climb runs from the bottom of the pass to the crest, and the sprint is the flat that straddles the start line.

| Segment | Start | End | Length | Average grade |
| --- | ---: | ---: | ---: | ---: |
| Lap | 0 | 5000 | 5000 m | 0% |
| Penguin Pass climb | 273 | 2619 | 2346 m | +7.0% |
| Sprint | 4754 | 54 | 300 m | 0% |

The published generated routes, at the seeds in `docs/route-generator.md` and the course picker:

| Route | Climbs | Sprint |
| --- | ---: | --- |
| Flat 2 km, seed 1 | 0 | 231 m across the line, 0% |
| Flat 10 km, seed 1 | 0 | 300 m across the line, 0% |
| Rolling 5 km, seed 3 | 4, each about 3% | 200 m across the line, 0% |
| Rolling 10 km, seed 3 | 8, the longest 827 m at 3.1% | 300 m across the line, 0% |
| Mountain 10 km, seed 11 | 1, 3598 m at 6.7% | 300 m across the line, 0% |
| Desert 10 km, seed 7 | 1, 748 m at 4.6% | 300 m from the line, 0% |

## What is stored

One TOML file holds the records and the ghost. The session chooses the path. A sensible place is `segments.toml` next to the settings file. They share a file so a crash cannot save a record without its ghost, or the other way around.

The write goes to a temporary name in the same directory and is renamed over the destination. A missing file loads as empty records and is not created. A blank file is empty. A file that does not parse is moved to `<name>.bad` and replaced with an empty file. An unreadable path loads as empty records and is left in place.

Each course entry is the caller's key plus the lap length in metres. Efforts and the ghost are applied only when that length matches the loaded course within a millimetre. A rebuilt route of a different length does not inherit the old times. Saving a result for the new length replaces that course's entry.

An effort stores the segment id, the finish time, the UTC date (`YYYY-MM-DD`), and ten split times: the clock at 10%, 20%, … through the finish. A slower time does not replace a stored effort. An equal time does not either.

The ghost is the fastest first lap from a standing start. The first sample has to be within 10 m of the start line. Sample `i` is the distance in metres at `i` seconds. When the lap finishes between seconds, `end_s` is that time and `end_m` is the lap length. A one-hour lap is 3,600 samples. A first lap longer than three hours is not stored. A later lap in the same ride is not compared.

## What the session calls

When a course loads, detect the segments and open the file. `key` is a stable string the session chooses, such as `penguin_pass`.

```text
segments = detect(&course)
records  = Records::load(path)
timer    = SegmentTimer::new(segments, &records.efforts(key, course.length_m()))
ghost    = GhostRecorder::new(course.length_m(), records.ghost(key, course.length_m()))
```

Pass total distance, `Telemetry::distance_m`, which keeps counting across laps. Lap distance that resets to zero is a backwards step and is ignored. When the ride begins on the start line, the first sample should be that line, usually `(0.0, 0.0)`.

Each frame, after the ride steps:

```text
events    = timer.tick(elapsed_s, distance_m)
ghost_lap = ghost.tick(elapsed_s, distance_m)
progress  = timer.in_progress()
```

`events` is in distance order. Crossing a line that finishes one segment and starts the next emits the finish first.

| Event | Meaning |
| --- | --- |
| `Entered { segment_id }` | The rider crossed that start line going forward. |
| `Finished { segment_id, time_s, previous_best_s, record }` | They had entered it, and reached the finish. `record` is true when this time is strictly faster. `previous_best_s` is the old best, or none on the first effort. |

A segment counts only when it is entered at its start line. Starting already inside one does not produce a time. Each crossing is a new attempt, so each lap is a new try. The clock keeps running while the rider is stopped inside a segment. Entry and exit times are linear between the two samples of a tick, so the same motion agrees within a millisecond at 30, 60, and 144 Hz.

A sample with time or distance going backwards, or a non-finite sample, is ignored. One tick that jumps more than 500 m drops any open segment and does not award a finish.

`in_progress` is the open segments, lap first. Each row has `elapsed_s`, `fraction` (0 at the start line, 1 at the finish), and `split_s`. The split is seconds ahead of the best effort at this fraction, interpolated between the stored tenths. Positive means this attempt is ahead. It is absent when the segment has no best yet.

`ghost_lap` is present once, when the first lap finishes from a standing start. `record` means it beat the stored ghost. `ghost.trace()` is then the trace to save. `GhostTrace::distance_at(elapsed_s)` is the ghost's distance, or none after the trace ends. `gap(elapsed_s, rider_distance_m)` is metres and seconds, both positive when the rider is ahead. It is absent once the rider is past the distance the ghost reached.

Save when a finish sets a record, when the ghost lap sets a record, and at the end of the ride. Not every frame.

```text
for effort in timer.efforts() {
    records.set_effort(key, course.length_m(), effort)
}
if let Some(trace) = ghost.trace() {
    records.set_ghost(key, course.length_m(), trace.clone())
}
records.save(path)
```

`efforts()` is the bests after this ride, sorted by id. `set_effort` and `set_ghost` keep the stored value when the new one is not strictly faster.
