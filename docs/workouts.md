# Workouts

`pp-workout` is the workout plan, the clock that walks it, and the watts an ERG trainer should hold. It does not draw, open Bluetooth, or send a command. The session owns the ride clock and the trainer. The screen owns the pixels.

Power on a step is a fraction of FTP. `1.0` is threshold. Time is seconds. Watts in the runner are that fraction times the FTP passed to `WorkoutRun::new`.

## FTP and zones

FTP is `Settings.ftp_w` in `pp-session`: a `u16` in watts, from 50 to 600, default 200. The same number is copied onto `pp_core::RiderProfile.ftp_w`. Pass `f32::from(settings.ftp_w)` into the run. The run keeps that value; a settings change means a new run.

Zones are Coggan's seven levels in `pp_session::power_zone`. The ride console calls that function and draws `Z3 TEMPO` and the rest. The TV page in `pp-cast` uses the same bounds. This crate does not carry a zone table. Colour a target by calling `power_zone` with the target watts and the same FTP. Sweet spot, over-under, and the other names below are workout labels, not zone names.

`pp-workout` does not depend on `pp-session`. The session will depend on this crate, and `pp-session` already pulls in the trainer.

## The plan

`Workout` is `id`, `name`, `description`, `steps`, and `ramp_test`.

| Step | What it holds |
| --- | --- |
| Warmup, Cooldown, Ramp | A straight line from `power_low` to `power_high`. |
| Steady | One fraction for the whole block. |
| Intervals | `repeat` pairs of a hard piece and an easy piece. |
| FreeRide, MaxEffort | No target. `show_power` is only for the graph. |

Each step has a duration, an optional cadence, and cues (`offset_s` from the start of that step; for a repeat, from the start of the whole set). An empty cue is dropped.

`duration_s` is the sum of the steps. A repeat counts every on piece and every off piece.

`intensity` is the time-weighted mean fraction, leaving out free-ride and max-effort seconds, and a stress score when one can be computed. The score is Coggan's: one sample per second, a 30-second rolling average, the fourth power of those averages, the mean of the powers, then the fourth root. That root is the normalised fraction of FTP. The score is `seconds * NP² * 100 / 3600`. An hour at FTP scores 100. The score is `None` when the targeted work is under 30 seconds, or when the plan contains a free ride or a max effort, because those seconds have no power to put in the window.

`profile` is one bar per expanded piece, for a bar graph. `start_power` and `end_power` are the two ends, as fractions of FTP. They are equal on a steady block. `open` is set on free ride and max effort.

## Built-in workouts

`Library::builtin()` is this list, in this order. Ids do not change.

| Id | Name | Duration | Shape |
| --- | ---: | --- | --- |
| `easy-spin` | Easy spin | 30:00 | 3 min from 40% to 55%, 22 min at 60%, 5 min from 55% to 40% |
| `endurance-20` | Endurance | 20:00 | 2 min from 45% to 60%, 16 min at 65%, 2 min from 60% to 45% |
| `sweet-spot-3x10` | Sweet spot | 56:00 | 6 min from 45% to 75%, 3× (10 min at 91%, 5 min at 55%), 5 min from 55% to 40% |
| `threshold-2x20` | Threshold | 60:00 | 8 min from 45% to 80%, 20 min at 100%, 5 min at 55%, 20 min at 100%, 7 min from 55% to 40% |
| `over-unders` | Over-unders | 48:00 | 8 min warm-up, 4× (2 min at 105%, 2 min at 95%), 4 min at 55%, the same four again, 4 min cool-down |
| `vo2-5x3` | VO2 | 46:00 | 10 min warm-up, 5× (3 min at 120%, 3 min at 50%), 6 min cool-down |
| `pyramid` | Pyramid | 40:00 | 8 min warm-up, 1-2-3-4-3-2-1 min at 120% with easy minutes between, 4 min cool-down |
| `thirties` | 30/30s | 30:00 | 10 min warm-up, 15× (30 s at 150%, 30 s at 45%), 5 min cool-down |
| `sprints` | Sprints | 36:00 | 10 min warm-up, 8× (15 s at 200%, 2:15 at 50%), 6 min cool-down |
| `ramp-test` | Ramp test | 29:00 | 5 min from 40% to 55%, then 1 min steps from 60% to 180% adding 6% each minute (21 steps), 3 min cool-down |

The ramp test is the only plan with `ramp_test` set. The estimate is 75% of the target of the last steady step whose full minute elapsed, rounded to the nearest watt. The warm-up and the cool-down are not scored. A step that is skipped, or left in the middle, is not scored. The 75% is the usual ramp-test rule: a one-minute step is above threshold, and three quarters of the last minute a rider can finish is the threshold estimate. The nudge is ignored on this plan, so the estimate stays on the protocol powers.

The steps run up to 180% of the FTP the rider entered. A rider who finishes every step gets 75% of that last step, which is 135% of the FTP they started with. The test ran out of steps before they did. They should be tested again from a higher FTP.

## Zwift files

`Library::load_dir(path)` reads every `.zwo` in that folder, not in subfolders, in file-name order. The game passes a `workouts` folder beside the settings file. A missing folder is an error. One bad file is a warning; the other files are still returned.

The id is the file name without `.zwo`. The name and description come from those elements. A missing name uses the id.

`parse_zwo` reads `workout_file`. Inside `workout` it understands `Warmup`, `Cooldown`, `SteadyState`, `Ramp`, `IntervalsT`, `FreeRide`, and `MaxEffort`, with `Duration`, `Power`, `PowerLow`, `PowerHigh`, `Repeat`, `OnDuration`, `OffDuration`, `OnPower`, `OffPower`, `Cadence`, `CadenceResting`, and `textevent` (`timeoffset`, `message`). Power is a fraction, as in the file (`0.75`, not `75`).

An unknown element is skipped and named in `warnings`. A missing or unreadable required attribute skips that step and names it. The rest of the file is kept. A file that is not XML, or whose root is not `workout_file`, is skipped by `load_dir` and returned as `Err` by `parse_zwo`. Imported plans are not ramp tests.

`roxmltree` parses the XML. It is MIT. It is not in the `pp-godot` licence list until the game links this crate. Re-run `./x licenses` when that happens.

## What the session calls

Build the lists when the rider opens the workout picker.

```rust
let builtin = Library::builtin();
let imported = Library::load_dir(workouts_dir)?;
```

Starting a workout copies the plan. `ftp_w` is the settings value.

```rust
let mut run = WorkoutRun::new(&workout, f32::from(settings.ftp_w));
let mut erg = ErgGuide::new();
```

Each frame, only while the ride is running (a paused frame does not call `tick`):

```rust
let events = run.tick(dt_s, power_w, cadence_rpm);
let view = run.view();
let pedaling = match cadence_rpm {
    Some(rpm) => rpm >= 5.0,
    None => power_w >= 15.0,
};
let command = erg.update(dt_s, run.target_w(), pedaling);
```

`tick` takes any positive `dt`. The first tick emits `StepStarted` for the first step. A cue at the start of a step is emitted with it. `StepEndingSoon { seconds: 5 }` is emitted once, when a step longer than five seconds crosses five seconds remaining. `Finished` is emitted once. Events are not repeated.

`skip_step` moves to the next step immediately. The skipped remainder is not ridden and does not complete a ramp-test step. Call `tick` afterwards, even with `dt` of 0, to take the `StepStarted` or `Finished` the skip queued. `finished` is already true if the skip ended the plan.

`set_bias` scales every target by that percent. The range is −25 to +25; anything outside is clamped. A ramp test ignores it.

`result` is safe before the end. `completed_s` is time that was ticked. `planned_s` is the plan. A skip finishes the workout with `completed_s` shorter than `planned_s`. `in_range_fraction` is judged seconds inside the band, divided by judged seconds, or `0.0` when nothing has been judged.

## What the trainer is sent

`target_w` is `None` on a free ride, a max effort, and after the end. Otherwise it is the watts to hold, including the nudge. A ramp moves in a straight line from its start to its end.

`ErgGuide::update` returns whole watts and `send`. Send only when `send` is set. A number changes at most once a second, so a ramp is a staircase. A jump of more than 15 W eases from the previous command, or from half the new target when there is no previous command, over 3 seconds. The first frame of a workout therefore sends about half the opening target, then walks up.

`watts: None` means release. Do not encode 0 W for that: page 49 treats 0 as a real target. Go back to the grade command the session already sends (`TrainerCommand::SetGrade`). While `watts` is `Some`, send target power and do not also send grade. The two commands fight. A release, and the first number after one, are sent on that frame. Only a change from one number to another waits out the second.

The release rule: `pedaling == false` for one second drops the target, so a stopped crank is not loaded. Pedalling again eases up from half the target. A gap shorter than a second keeps the last command.

`TrainerCommand::SetTargetPower { watts }` already exists. For a Tacx, encode it as FE-C page 49:

```rust
TargetPower { watts: f32::from(watts) }.encode(checksum)
```

Page 49 is an acknowledged ANT frame, same wrapper as page 51. Byte 0 of the page is `0x31`. Bytes 1–5 are `0xFF`. Bytes 6–7 are the target in units of 0.25 W, little-endian, from 0 W to 4000 W. `0xFFFF` is invalid and is never sent. 200 W is 800 counts, `0x20 0x03`. The Tacx sum checksum of that frame is `0xAC`. The ANT XOR checksum is `0x0A`. Values outside 0…4000 W clamp. A non-finite value is an error.

The encoder is `pp_ftms::TargetPower`. It is not a `FecData` variant. Parsing a page 49 the trainer might echo is unchanged, so `pp-trainer` did not need a new match arm.

For an FTMS trainer the control-point opcode was already there: `0x05`, then `sint16` watts, little-endian, resolution 1 W. `ControlPointRequest::SetTargetPower { watts: 200 }` is `05 C8 00`. `-20` is `05 EC FF`. Values that do not fit in `sint16` saturate, so 40000 W encodes as `05 FF 7F`. No second encoder was added.

`control_trainer == false` still means the game sends nothing.

## What the screen draws

The picker lists `Library::builtin().workouts` and the imported list. Show `name`, `description`, `duration_s`, and `intensity`.

During the ride, `view` is one frame:

| Field | Use |
| --- | --- |
| `label` | `Sweet spot 2 of 3`, `Recover`, `Warm up`, `Under`, `60%` |
| `step_index`, `step_count` | Which piece, after repeats are expanded |
| `target_w` | The number to hold. Hide it when `None` |
| `target_low_w`, `target_high_w` | The in-range band around that number, not the two ends of a ramp |
| `cadence_target` | The cadence to show. The sample cadence is separate |
| `step_elapsed_s`, `step_remaining_s` | This piece |
| `total_elapsed_s`, `total_remaining_s` | The plan. A skip moves these forward |
| `next_label`, `next_target_w` | The following piece. `None` on the last |
| `in_range` | `None` during the grace, on an open step, and after the end |
| `cue` | Text for 8 seconds after the cue fires |
| `bias_percent` | The nudge in effect |

`profile` is the bar graph for the whole plan. Draw an open bar differently (`open` is set, and the trainer is not holding it).

`result.ramp_test_ftp_w` is the number to offer as the new FTP when a ramp test ends. `None` if no scored minute was finished.

## In range

The band is ±5% of the instantaneous target, and at least ±5 W either side. 200 W is in range from 190 W to 210 W, inclusive. The first 3 seconds of a step are not judged, so a step change does not count as a miss. A step shorter than 6 seconds uses half its length as the grace instead. Open steps are not judged. Cadence is stored on the step result and does not affect the fraction.

## Choices to confirm

- The band above: ±5%, at least ±5 W, with 3 seconds of grace (half the step when the step is under 6 seconds).
- ERG releases after 1 second of not pedalling, rather than holding the target on a stopped crank. Not pedalling means cadence under 5 rpm, or, when no cadence is reported, power under 15 W.
- The ramp-test rule above: 75% of the last finished one-minute step, bias ignored, ceiling 180% of the FTP the rider entered.
