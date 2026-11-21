//! Walking a [`Workout`](crate::Workout) against live power samples.
//!
//! Call [`WorkoutRun::tick`] only while the ride is running. Skipping a
//! frame is how the workout pauses. A tick may be any positive `dt`.

use crate::model::{Atom, Workout, expand};

/// Half-width of the in-range band, as a fraction of the target.
pub const RANGE_FRACTION: f32 = 0.05;
/// The band is at least this many watts either side of the target.
pub const RANGE_FLOOR_W: f32 = 5.0;
/// Seconds after a step change before power is judged.
///
/// A step shorter than six seconds uses half its length instead, so a short
/// piece is still scored.
pub const RANGE_GRACE_S: f32 = 3.0;
/// [`WorkoutEvent::StepEndingSoon`] fires when this many seconds remain.
pub const ENDING_SOON_S: f32 = 5.0;
/// How long a cue stays on [`WorkoutView::cue`] after it fires.
pub const CUE_HOLD_S: f32 = 8.0;
/// Largest absolute intensity nudge, in percent of the planned target.
pub const BIAS_LIMIT: i8 = 25;
/// Fraction of the last completed ramp-test step that becomes the FTP estimate.
pub const RAMP_TEST_FACTOR: f32 = 0.75;

/// What changed on this tick. Each event is emitted once.
#[derive(Debug, Clone, PartialEq)]
pub enum WorkoutEvent {
    /// A step became the current one, including the first step.
    StepStarted {
        /// Index in the expanded plan, starting at 0.
        index: usize,
        /// Short label, such as `Sweet spot 2 of 3`.
        label: String,
        /// Target at the start of the step, in watts. `None` when the step is open.
        target_w: Option<f32>,
        /// Length of the step, in seconds.
        duration_s: f32,
    },
    /// The current step has [`ENDING_SOON_S`] seconds or less left.
    ///
    /// `seconds` is that horizon, always 5. Steps of five seconds or less
    /// do not emit it. A tick that jumps across the horizon still emits it
    /// once, before the step ends.
    StepEndingSoon {
        /// The warning horizon, in seconds.
        seconds: u8,
    },
    /// A cue's time was reached.
    Cue {
        /// The cue text.
        text: String,
    },
    /// The plan has ended, including when the last step is skipped.
    Finished,
}

/// One expanded step, as ridden so far.
#[derive(Debug, Clone, PartialEq)]
pub struct StepResult {
    /// Index in the expanded plan.
    pub index: usize,
    /// Short label.
    pub label: String,
    /// Planned length, in seconds.
    pub planned_s: f32,
    /// Seconds actually ticked. Skipped time is not included.
    pub completed_s: f32,
    /// Mean power while this step was being ridden. `0.0` if it was not.
    pub average_power_w: f32,
    /// Judged seconds inside the band, divided by judged seconds.
    ///
    /// `0.0` when the step was not judged. The grace after a step change is
    /// not judged, and neither is an open step.
    pub in_range_fraction: f32,
    /// Mean cadence for the samples that reported one.
    pub average_cadence_rpm: Option<f32>,
}

/// Totals for the run so far. Safe to read before the workout ends.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkoutResult {
    /// Seconds ticked. Skipped time is not included.
    pub completed_s: f32,
    /// Planned length, in seconds.
    pub planned_s: f32,
    /// Judged seconds inside the band, divided by judged seconds.
    ///
    /// `0.0` when nothing has been judged yet.
    pub in_range_fraction: f32,
    /// Mean power over the ticked seconds. `0.0` before the first tick with time.
    pub average_power_w: f32,
    /// One entry per expanded step, including steps not yet reached.
    pub steps: Vec<StepResult>,
    /// FTP estimate from a ramp test, in watts, rounded to the nearest watt.
    ///
    /// `None` until a scored step has been finished, on a workout that is not
    /// a ramp test, and when FTP was 0. The estimate is [`RAMP_TEST_FACTOR`]
    /// times the protocol target of the last steady step whose full length
    /// elapsed. A skip does not finish a step. The intensity nudge is ignored
    /// on a ramp test, so the protocol target is the one that is scored.
    pub ramp_test_ftp_w: Option<f32>,
}

/// The current step, for the screen. Allocates the strings it returns.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkoutView {
    /// Index of the current step. Stays on the last step after the end.
    pub step_index: usize,
    /// Expanded step count.
    pub step_count: usize,
    /// Short label of the current step. Empty when the plan has no steps.
    pub label: String,
    /// Target right now, in watts. `None` in free ride, max effort, and after the end.
    pub target_w: Option<f32>,
    /// Low edge of the in-range band, in watts.
    pub target_low_w: Option<f32>,
    /// High edge of the in-range band, in watts.
    pub target_high_w: Option<f32>,
    /// Cadence to show, in revolutions per minute.
    pub cadence_target: Option<u16>,
    /// Seconds already ticked in this step.
    pub step_elapsed_s: f32,
    /// Seconds left in this step.
    pub step_remaining_s: f32,
    /// Position in the plan, in seconds. A skip moves this forward.
    pub total_elapsed_s: f32,
    /// Planned seconds after `total_elapsed_s`.
    pub total_remaining_s: f32,
    /// Label of the following step.
    pub next_label: Option<String>,
    /// Target at the start of the following step, in watts.
    pub next_target_w: Option<f32>,
    /// Whether the latest sample sits in the band.
    ///
    /// `None` during the grace, on an open step, and after the end.
    pub in_range: Option<bool>,
    /// Cue text, for [`CUE_HOLD_S`] seconds after it fires.
    pub cue: Option<String>,
    /// Intensity nudge in effect, from -25 to [`BIAS_LIMIT`].
    pub bias_percent: i8,
}

#[derive(Debug, Clone)]
struct Accum {
    label: String,
    planned_s: f32,
    completed_s: f32,
    power_integral: f32,
    judged_s: f32,
    in_range_s: f32,
    cadence_integral: f32,
    cadence_time: f32,
}

impl Accum {
    fn from_atom(atom: &Atom) -> Self {
        Self {
            label: atom.label.clone(),
            planned_s: atom.duration_s,
            completed_s: 0.0,
            power_integral: 0.0,
            judged_s: 0.0,
            in_range_s: 0.0,
            cadence_integral: 0.0,
            cadence_time: 0.0,
        }
    }

    fn to_result(&self, index: usize) -> StepResult {
        StepResult {
            index,
            label: self.label.clone(),
            planned_s: self.planned_s,
            completed_s: self.completed_s,
            average_power_w: if self.completed_s > 0.0 {
                self.power_integral / self.completed_s
            } else {
                0.0
            },
            in_range_fraction: if self.judged_s > 0.0 {
                self.in_range_s / self.judged_s
            } else {
                0.0
            },
            average_cadence_rpm: if self.cadence_time > 0.0 {
                Some(self.cadence_integral / self.cadence_time)
            } else {
                None
            },
        }
    }
}

/// Live position in a [`Workout`](crate::Workout).
#[derive(Debug, Clone)]
pub struct WorkoutRun {
    atoms: Vec<Atom>,
    steps: Vec<Accum>,
    ftp_w: f32,
    bias_percent: i8,
    index: usize,
    step_elapsed: f32,
    cursor_s: f32,
    ridden_s: f32,
    planned_s: f32,
    finished: bool,
    finish_emitted: bool,
    need_start: bool,
    grace_left: f32,
    ending_sent: bool,
    next_cue: usize,
    cue_text: Option<String>,
    cue_offset: f32,
    live_in_range: Option<bool>,
    judged_s: f32,
    in_range_s: f32,
    power_integral: f32,
    ramp_ftp_w: Option<f32>,
    ramp_test: bool,
    sample_cadence: Option<f32>,
}

impl WorkoutRun {
    /// Copy the plan. `ftp_w` is the rider's FTP in watts. A non-finite or
    /// non-positive FTP is treated as 0, and every target becomes 0 W.
    #[must_use]
    pub fn new(workout: &Workout, ftp_w: f32) -> Self {
        let atoms = expand(workout);
        let planned_s = atoms
            .last()
            .map(|atom| atom.start_s + atom.duration_s)
            .unwrap_or(0.0);
        let steps = atoms.iter().map(Accum::from_atom).collect();
        let empty = atoms.is_empty();
        Self {
            atoms,
            steps,
            ftp_w: if ftp_w.is_finite() && ftp_w > 0.0 {
                ftp_w
            } else {
                0.0
            },
            bias_percent: 0,
            index: 0,
            step_elapsed: 0.0,
            cursor_s: 0.0,
            ridden_s: 0.0,
            planned_s,
            finished: empty,
            finish_emitted: false,
            need_start: !empty,
            grace_left: 0.0,
            ending_sent: false,
            next_cue: 0,
            cue_text: None,
            cue_offset: 0.0,
            live_in_range: None,
            judged_s: 0.0,
            in_range_s: 0.0,
            power_integral: 0.0,
            ramp_ftp_w: None,
            ramp_test: workout.ramp_test,
            sample_cadence: None,
        }
    }

    /// Advance the plan. `dt_s` is the frame length. A non-positive or
    /// non-finite `dt_s` delivers pending events and does not move time.
    ///
    /// `power_w` is the rider's power for this frame. `cadence_rpm` is
    /// recorded into the step average when it is finite and non-negative.
    /// Cadence does not affect the in-range test.
    pub fn tick(&mut self, dt_s: f32, power_w: f32, cadence_rpm: Option<f32>) -> Vec<WorkoutEvent> {
        let mut events = Vec::new();
        if self.finish_emitted {
            return events;
        }
        self.sample_cadence = cadence_rpm.filter(|rpm| rpm.is_finite() && *rpm >= 0.0);
        let power = if power_w.is_finite() {
            power_w.max(0.0)
        } else {
            0.0
        };
        if self.atoms.is_empty() {
            return self.emit_finished(events);
        }
        if self.need_start {
            self.emit_start(&mut events);
        }
        if self.finished {
            return self.emit_finished(events);
        }
        let dt = if dt_s.is_finite() && dt_s > 0.0 {
            dt_s
        } else {
            0.0
        };
        if dt > 0.0 {
            self.advance(dt, power, &mut events);
        }
        self.refresh_live(power);
        events
    }

    /// Current step, targets, and the cue, for one frame of the screen.
    #[must_use]
    pub fn view(&self) -> WorkoutView {
        let count = self.atoms.len();
        if count == 0 {
            return WorkoutView {
                step_index: 0,
                step_count: 0,
                label: String::new(),
                target_w: None,
                target_low_w: None,
                target_high_w: None,
                cadence_target: None,
                step_elapsed_s: 0.0,
                step_remaining_s: 0.0,
                total_elapsed_s: 0.0,
                total_remaining_s: 0.0,
                next_label: None,
                next_target_w: None,
                in_range: None,
                cue: None,
                bias_percent: self.bias_percent,
            };
        }
        let atom = &self.atoms[self.index];
        let finished = self.finished;
        let target = if finished {
            None
        } else {
            self.target_at(self.step_elapsed)
        };
        let (low, high) = target.map(band).unzip();
        let (next_label, next_target_w) = self.next_preview();
        let cue = self.shown_cue();
        WorkoutView {
            step_index: self.index,
            step_count: count,
            label: atom.label.clone(),
            target_w: target,
            target_low_w: low,
            target_high_w: high,
            cadence_target: atom.cadence,
            step_elapsed_s: self.step_elapsed,
            step_remaining_s: if finished {
                0.0
            } else {
                (atom.duration_s - self.step_elapsed).max(0.0)
            },
            total_elapsed_s: self.cursor_s,
            total_remaining_s: (self.planned_s - self.cursor_s).max(0.0),
            next_label,
            next_target_w,
            in_range: if finished { None } else { self.live_in_range },
            cue: if finished { None } else { cue },
            bias_percent: self.bias_percent,
        }
    }

    /// Watts to hold right now. `None` in free ride, max effort, and after the end.
    ///
    /// A ramp moves in a straight line from its start power to its end power.
    /// The intensity nudge scales the result. A ramp test does not.
    #[must_use]
    pub fn target_w(&self) -> Option<f32> {
        if self.finished || self.atoms.is_empty() {
            None
        } else {
            self.target_at(self.step_elapsed)
        }
    }

    /// Nudge every target by `percent` percent. Values outside
    /// [`-BIAS_LIMIT`] to [`BIAS_LIMIT`] are clamped. A ramp test ignores
    /// the call so the FTP estimate stays on the protocol powers.
    pub fn set_bias(&mut self, percent: i8) {
        if self.ramp_test {
            return;
        }
        self.bias_percent = percent.clamp(-BIAS_LIMIT, BIAS_LIMIT);
    }

    /// Leave the current step. The skipped remainder is not ridden and does
    /// not complete a ramp-test step. Call [`Self::tick`] afterwards, with
    /// any `dt`, to take the [`WorkoutEvent::StepStarted`] or
    /// [`WorkoutEvent::Finished`] that the skip queued.
    pub fn skip_step(&mut self) {
        if self.finished || self.atoms.is_empty() {
            return;
        }
        let duration = self.atoms[self.index].duration_s;
        if self.index + 1 >= self.atoms.len() {
            self.step_elapsed = duration;
            self.cursor_s = self.atoms[self.index].start_s + duration;
            self.finished = true;
            self.need_start = false;
            self.live_in_range = None;
            self.cue_text = None;
            return;
        }
        self.index += 1;
        self.step_elapsed = 0.0;
        self.cursor_s = self.atoms[self.index].start_s;
        self.need_start = true;
        self.cue_text = None;
        self.live_in_range = None;
    }

    /// `true` once the last step has ended or been skipped.
    #[must_use]
    pub fn finished(&self) -> bool {
        self.finished
    }

    /// Totals so far. A finished ramp test includes the estimate.
    #[must_use]
    pub fn result(&self) -> WorkoutResult {
        WorkoutResult {
            completed_s: self.ridden_s,
            planned_s: self.planned_s,
            in_range_fraction: if self.judged_s > 0.0 {
                self.in_range_s / self.judged_s
            } else {
                0.0
            },
            average_power_w: if self.ridden_s > 0.0 {
                self.power_integral / self.ridden_s
            } else {
                0.0
            },
            steps: self
                .steps
                .iter()
                .enumerate()
                .map(|(index, step)| step.to_result(index))
                .collect(),
            ramp_test_ftp_w: self.ramp_ftp_w,
        }
    }

    fn emit_finished(&mut self, mut events: Vec<WorkoutEvent>) -> Vec<WorkoutEvent> {
        self.finished = true;
        self.finish_emitted = true;
        self.live_in_range = None;
        events.push(WorkoutEvent::Finished);
        events
    }

    fn emit_start(&mut self, events: &mut Vec<WorkoutEvent>) {
        let atom = &self.atoms[self.index];
        let target = self.target_at(0.0);
        events.push(WorkoutEvent::StepStarted {
            index: self.index,
            label: atom.label.clone(),
            target_w: target,
            duration_s: atom.duration_s,
        });
        self.need_start = false;
        self.grace_left = grace_for(atom.duration_s);
        self.ending_sent = false;
        self.next_cue = 0;
        self.cue_text = None;
        self.fire_cues_through(0.0, events);
    }

    fn advance(&mut self, mut left: f32, power: f32, events: &mut Vec<WorkoutEvent>) {
        while left > 0.0 && !self.finished {
            let used = self.advance_one(left, power, events);
            if used <= 0.0 {
                break;
            }
            left -= used;
        }
    }

    fn advance_one(&mut self, left: f32, power: f32, events: &mut Vec<WorkoutEvent>) -> f32 {
        let duration = self.atoms[self.index].duration_s;
        let remain = (duration - self.step_elapsed).max(0.0);
        if remain <= 0.0 {
            self.complete_step(events);
            return 0.0;
        }
        let slice = left.min(remain);
        let from = self.step_elapsed;
        let to = from + slice;
        self.fire_cues_between(from, to, events);
        self.note_ending(duration, from, to, events);
        self.account(slice, power);
        self.step_elapsed = to;
        self.cursor_s = self.atoms[self.index].start_s + to;
        self.ridden_s += slice;
        if to >= duration - 1.0e-4 {
            self.step_elapsed = duration;
            self.cursor_s = self.atoms[self.index].start_s + duration;
            self.complete_step(events);
        }
        slice
    }

    fn complete_step(&mut self, events: &mut Vec<WorkoutEvent>) {
        self.note_ramp();
        if self.index + 1 >= self.atoms.len() {
            self.finished = true;
            self.finish_emitted = true;
            self.live_in_range = None;
            events.push(WorkoutEvent::Finished);
            return;
        }
        self.index += 1;
        self.step_elapsed = 0.0;
        self.cursor_s = self.atoms[self.index].start_s;
        self.emit_start(events);
    }

    fn note_ramp(&mut self) {
        let atom = &self.atoms[self.index];
        if !atom.scored || self.ftp_w <= 0.0 {
            return;
        }
        let Some(fraction) = atom.fraction_at(0.0) else {
            return;
        };
        let watts = fraction * self.ftp_w;
        if watts.is_finite() && watts >= 0.0 {
            self.ramp_ftp_w = Some((watts * RAMP_TEST_FACTOR).round());
        }
    }

    fn account(&mut self, slice: f32, power: f32) {
        let grace = slice.min(self.grace_left.max(0.0));
        let judged = slice - grace;
        self.grace_left -= grace;
        if judged > 0.0 {
            let mid = self.step_elapsed + grace + judged * 0.5;
            if let Some(target) = self.target_at(mid) {
                self.judged_s += judged;
                self.steps[self.index].judged_s += judged;
                if in_band(power, target) {
                    self.in_range_s += judged;
                    self.steps[self.index].in_range_s += judged;
                }
            }
        }
        self.power_integral += power * slice;
        self.steps[self.index].power_integral += power * slice;
        self.steps[self.index].completed_s += slice;
        if let Some(rpm) = self.sample_cadence {
            self.steps[self.index].cadence_integral += rpm * slice;
            self.steps[self.index].cadence_time += slice;
        }
    }

    fn note_ending(&mut self, duration: f32, from: f32, to: f32, events: &mut Vec<WorkoutEvent>) {
        if self.ending_sent || duration <= ENDING_SOON_S {
            return;
        }
        let before = duration - from;
        let after = duration - to;
        if before > ENDING_SOON_S && after <= ENDING_SOON_S {
            self.ending_sent = true;
            events.push(WorkoutEvent::StepEndingSoon { seconds: 5 });
        }
    }

    fn fire_cues_through(&mut self, at: f32, events: &mut Vec<WorkoutEvent>) {
        let cues_len = self.atoms[self.index].cues.len();
        while self.next_cue < cues_len {
            let offset = self.atoms[self.index].cues[self.next_cue].offset_s;
            if offset > at {
                break;
            }
            let text = self.atoms[self.index].cues[self.next_cue].text.clone();
            self.push_cue(offset, text, events);
            self.next_cue += 1;
        }
    }

    fn fire_cues_between(&mut self, from: f32, to: f32, events: &mut Vec<WorkoutEvent>) {
        let cues_len = self.atoms[self.index].cues.len();
        while self.next_cue < cues_len {
            let offset = self.atoms[self.index].cues[self.next_cue].offset_s;
            if offset > to {
                break;
            }
            if offset > from {
                let text = self.atoms[self.index].cues[self.next_cue].text.clone();
                self.push_cue(offset, text, events);
            }
            self.next_cue += 1;
        }
    }

    fn push_cue(&mut self, offset: f32, text: String, events: &mut Vec<WorkoutEvent>) {
        self.cue_text = Some(text.clone());
        self.cue_offset = offset;
        events.push(WorkoutEvent::Cue { text });
    }

    fn refresh_live(&mut self, power: f32) {
        if self.finished || self.grace_left > 0.0 {
            self.live_in_range = None;
            return;
        }
        self.live_in_range = self
            .target_at(self.step_elapsed)
            .map(|target| in_band(power, target));
    }

    fn shown_cue(&self) -> Option<String> {
        let text = self.cue_text.clone()?;
        let age = self.step_elapsed - self.cue_offset;
        if (0.0..=CUE_HOLD_S).contains(&age) {
            Some(text)
        } else {
            None
        }
    }

    fn next_preview(&self) -> (Option<String>, Option<f32>) {
        let next = self.index + 1;
        if self.finished || next >= self.atoms.len() {
            return (None, None);
        }
        let atom = &self.atoms[next];
        let target = atom.fraction_at(0.0).map(|fraction| self.scale(fraction));
        (Some(atom.label.clone()), target)
    }

    fn target_at(&self, elapsed_s: f32) -> Option<f32> {
        let fraction = self.atoms.get(self.index)?.fraction_at(elapsed_s)?;
        Some(self.scale(fraction))
    }

    fn scale(&self, fraction: f32) -> f32 {
        if self.ftp_w <= 0.0 {
            return 0.0;
        }
        let bias = 1.0 + f32::from(self.bias_percent) / 100.0;
        (fraction * bias * self.ftp_w).max(0.0)
    }
}

fn grace_for(duration_s: f32) -> f32 {
    let half = duration_s * 0.5;
    if half < RANGE_GRACE_S {
        half.max(0.0)
    } else {
        RANGE_GRACE_S
    }
}

fn band(target_w: f32) -> (f32, f32) {
    let half = (target_w.abs() * RANGE_FRACTION).max(RANGE_FLOOR_W);
    (target_w - half, target_w + half)
}

fn in_band(power_w: f32, target_w: f32) -> bool {
    let (low, high) = band(target_w);
    power_w >= low && power_w <= high
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin::workouts;
    use crate::model::{Cue, IntervalStep, OpenStep, RampStep, SteadyStep, Step};

    fn steady(seconds: f32, power: f32, label: &str) -> Step {
        Step::Steady(SteadyStep {
            duration_s: seconds,
            power,
            cadence_rpm: Some(90),
            cues: Vec::new(),
            label: Some(label.to_owned()),
        })
    }

    fn plan(steps: Vec<Step>, ramp_test: bool) -> Workout {
        Workout {
            id: "t".to_owned(),
            name: "T".to_owned(),
            description: String::new(),
            steps,
            ramp_test,
        }
    }

    fn labels(events: &[WorkoutEvent]) -> Vec<String> {
        events
            .iter()
            .filter_map(|event| match event {
                WorkoutEvent::StepStarted { label, .. } => Some(label.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn variable_steps_land_on_the_same_step() {
        let workout = plan(
            vec![
                steady(10.0, 0.5, "One"),
                steady(20.0, 0.8, "Two"),
                steady(10.0, 0.4, "Three"),
            ],
            false,
        );
        let mut fine = WorkoutRun::new(&workout, 200.0);
        let mut left = 25.0_f32;
        let mut fine_events = Vec::new();
        while left > 0.0 {
            fine_events.extend(fine.tick(0.5, 160.0, Some(90.0)));
            left -= 0.5;
        }
        let mut coarse = WorkoutRun::new(&workout, 200.0);
        let coarse_events = coarse.tick(25.0, 160.0, Some(90.0));
        assert_eq!(fine.index, 1);
        assert_eq!(coarse.index, 1);
        assert!((fine.step_elapsed - 15.0).abs() < 1e-3);
        assert!((coarse.step_elapsed - 15.0).abs() < 1e-3);
        assert_eq!(labels(&fine_events), labels(&coarse_events));
        assert_eq!(
            labels(&fine_events),
            vec!["One".to_owned(), "Two".to_owned()]
        );
        let soon = fine_events
            .iter()
            .filter(|event| matches!(event, WorkoutEvent::StepEndingSoon { seconds: 5 }))
            .count();
        assert_eq!(soon, 2);
    }

    #[test]
    fn a_ramp_moves_in_a_straight_line() {
        let workout = plan(
            vec![Step::Warmup(RampStep {
                duration_s: 10.0,
                power_low: 0.50,
                power_high: 1.00,
                cadence_rpm: None,
                cues: Vec::new(),
            })],
            false,
        );
        let mut run = WorkoutRun::new(&workout, 200.0);
        let _ = run.tick(0.0, 0.0, None);
        assert!((run.target_w().unwrap() - 100.0).abs() < 1e-3);
        let _ = run.tick(5.0, 150.0, None);
        assert!((run.target_w().unwrap() - 150.0).abs() < 1e-3);
        let view = run.view();
        assert!((view.target_low_w.unwrap() - 142.5).abs() < 1e-2);
        assert!((view.target_high_w.unwrap() - 157.5).abs() < 1e-2);
        run.set_bias(10);
        assert!((run.target_w().unwrap() - 165.0).abs() < 1e-2);
    }

    #[test]
    fn intervals_skip_bias_and_a_pause() {
        let workout = plan(
            vec![Step::Intervals(IntervalStep {
                repeat: 3,
                on_s: 10.0,
                off_s: 10.0,
                on_power: 1.0,
                off_power: 0.5,
                on_cadence: Some(95),
                off_cadence: None,
                cues: vec![Cue {
                    offset_s: 0.0,
                    text: "Go".to_owned(),
                }],
                on_label: Some("Sweet spot".to_owned()),
                off_label: Some("Recover".to_owned()),
            })],
            false,
        );
        let mut run = WorkoutRun::new(&workout, 200.0);
        let first = run.tick(4.0, 200.0, Some(95.0));
        assert_eq!(labels(&first), vec!["Sweet spot 1 of 3".to_owned()]);
        assert!(first.iter().any(|event| matches!(
            event,
            WorkoutEvent::Cue { text } if text == "Go"
        )));
        let again = run.tick(0.0, 200.0, Some(95.0));
        assert!(
            again.is_empty(),
            "a pause, and a repeated cue, emit nothing"
        );
        assert!((run.view().total_elapsed_s - 4.0).abs() < 1e-4);
        run.skip_step();
        assert_eq!(run.view().label, "Recover");
        assert!((run.view().total_elapsed_s - 10.0).abs() < 1e-3);
        let after = run.tick(0.0, 100.0, None);
        assert_eq!(labels(&after), vec!["Recover".to_owned()]);
        run.set_bias(40);
        assert_eq!(run.view().bias_percent, 25);
        assert!((run.target_w().unwrap() - 125.0).abs() < 1e-2);
        run.set_bias(-40);
        assert_eq!(run.bias_percent, -25);
        assert!((run.target_w().unwrap() - 75.0).abs() < 1e-2);
    }

    #[test]
    fn in_range_ignores_the_grace_and_counts_the_rest() {
        let workout = plan(vec![steady(20.0, 1.0, "Hold")], false);
        let mut run = WorkoutRun::new(&workout, 200.0);
        let _ = run.tick(1.0, 200.0, Some(90.0));
        assert_eq!(run.view().in_range, None);
        let _ = run.tick(2.0, 200.0, Some(90.0));
        assert_eq!(run.view().in_range, Some(true));
        let _ = run.tick(10.0, 200.0, Some(90.0));
        assert_eq!(run.view().in_range, Some(true));
        let _ = run.tick(7.0, 100.0, Some(80.0));
        assert!(run.finished());
        let result = run.result();
        assert!((result.completed_s - 20.0).abs() < 1e-3);
        assert!((result.planned_s - 20.0).abs() < 1e-3);
        // 3 s grace, then 10 s in and 7 s out.
        assert!((result.in_range_fraction - 10.0 / 17.0).abs() < 1e-3);
        let step = &result.steps[0];
        assert!((step.average_power_w - (200.0 * 13.0 + 100.0 * 7.0) / 20.0).abs() < 1e-2);
        assert!(
            (step.average_cadence_rpm.unwrap() - (90.0 * 13.0 + 80.0 * 7.0) / 20.0).abs() < 1e-2
        );
        let edges = band(200.0);
        assert!((edges.0 - 190.0).abs() < 1e-3);
        assert!((edges.1 - 210.0).abs() < 1e-3);
        assert!(in_band(190.0, 200.0));
        assert!(!in_band(189.0, 200.0));
    }

    #[test]
    fn events_fire_once_across_a_short_plan() {
        let workout = plan(
            vec![
                Step::Steady(SteadyStep {
                    duration_s: 10.0,
                    power: 0.8,
                    cadence_rpm: None,
                    cues: vec![
                        Cue {
                            offset_s: 0.0,
                            text: "Start".to_owned(),
                        },
                        Cue {
                            offset_s: 4.0,
                            text: "Middle".to_owned(),
                        },
                    ],
                    label: Some("Hold".to_owned()),
                }),
                Step::FreeRide(OpenStep {
                    duration_s: 5.0,
                    cues: Vec::new(),
                    show_power: None,
                }),
            ],
            false,
        );
        let mut run = WorkoutRun::new(&workout, 200.0);
        let mut events = Vec::new();
        events.extend(run.tick(0.25, 160.0, None));
        events.extend(run.tick(9.75, 160.0, None));
        events.extend(run.tick(5.0, 50.0, None));
        events.extend(run.tick(1.0, 50.0, None));
        let starts: Vec<_> = events
            .iter()
            .filter(|event| matches!(event, WorkoutEvent::StepStarted { .. }))
            .collect();
        assert_eq!(starts.len(), 2);
        let cues: Vec<_> = events
            .iter()
            .filter_map(|event| match event {
                WorkoutEvent::Cue { text } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(cues, vec!["Start", "Middle"]);
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, WorkoutEvent::StepEndingSoon { .. }))
                .count(),
            1
        );
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, WorkoutEvent::Finished))
                .count(),
            1
        );
        assert!(run.target_w().is_none());
        assert!(run.finished());
    }

    #[test]
    fn the_ramp_test_uses_the_last_finished_minute() {
        let workout = workouts()
            .into_iter()
            .find(|workout| workout.id == "ramp-test")
            .expect("ramp");
        let mut run = WorkoutRun::new(&workout, 200.0);
        assert_eq!(run.view().step_count, 23);
        let _ = run.tick(300.0, 100.0, Some(85.0));
        assert!(run.result().ramp_test_ftp_w.is_none());
        let _ = run.tick(60.0, 120.0, Some(90.0));
        assert_eq!(run.result().ramp_test_ftp_w, Some(90.0));
        let _ = run.tick(30.0, 140.0, Some(90.0));
        assert_eq!(run.result().ramp_test_ftp_w, Some(90.0));
        run.skip_step();
        let _ = run.tick(0.0, 140.0, None);
        assert_eq!(run.result().ramp_test_ftp_w, Some(90.0));
        let _ = run.tick(60.0, 150.0, Some(90.0));
        // 60% was finished, 66% was skipped, 72% was then finished.
        // 0.72 * 200 * 0.75 = 108.
        assert_eq!(run.result().ramp_test_ftp_w, Some(108.0));
        run.set_bias(25);
        assert_eq!(run.bias_percent, 0);
        assert_eq!(run.view().bias_percent, 0);
    }

    #[test]
    fn an_open_step_has_no_target_and_a_floor_band_still_works() {
        let workout = plan(
            vec![Step::MaxEffort(OpenStep {
                duration_s: 20.0,
                cues: Vec::new(),
                show_power: Some(1.5),
            })],
            false,
        );
        let mut run = WorkoutRun::new(&workout, 200.0);
        let _ = run.tick(1.0, 400.0, None);
        assert!(run.target_w().is_none());
        assert_eq!(run.view().in_range, None);
        assert_eq!(run.view().label, "Max effort");
    }
}
