//! The workout plan, its duration, and the bars used to draw it.
//!
//! Power on a step is a fraction of FTP. `1.0` is threshold. A warm-up,
//! cool-down, or ramp stores the fraction at each end and the runner moves
//! in a straight line between them. Free ride and max effort have no target.
//! [`Workout::intensity_factor`], [`Workout::tss`], and [`Workout::estimated_kj`]
//! turn those fractions into a training load.

/// One line of text shown during a step.
#[derive(Debug, Clone, PartialEq)]
pub struct Cue {
    /// Seconds from the start of the step. For a repeat, from the start of the whole set.
    pub offset_s: f32,
    /// Short text. Empty text is dropped when the plan is expanded.
    pub text: String,
}

/// A straight change from one fraction of FTP to another.
#[derive(Debug, Clone, PartialEq)]
pub struct RampStep {
    /// Length of the ramp, in seconds.
    pub duration_s: f32,
    /// Fraction of FTP at the start.
    pub power_low: f32,
    /// Fraction of FTP at the end.
    pub power_high: f32,
    /// Cadence to show, in revolutions per minute.
    pub cadence_rpm: Option<u16>,
    /// Cues, timed from the start of this ramp.
    pub cues: Vec<Cue>,
}

/// One steady target.
#[derive(Debug, Clone, PartialEq)]
pub struct SteadyStep {
    /// Length of the block, in seconds.
    pub duration_s: f32,
    /// Fraction of FTP.
    pub power: f32,
    /// Cadence to show, in revolutions per minute.
    pub cadence_rpm: Option<u16>,
    /// Cues, timed from the start of this block.
    pub cues: Vec<Cue>,
    /// Label drawn for the block. `None` becomes `Steady`.
    pub label: Option<String>,
}

/// `repeat` pairs of a hard piece and an easy piece.
#[derive(Debug, Clone, PartialEq)]
pub struct IntervalStep {
    /// How many on/off pairs to expand into.
    pub repeat: u16,
    /// Length of each hard piece, in seconds.
    pub on_s: f32,
    /// Length of each easy piece, in seconds. `0.0` expands to hard pieces only.
    pub off_s: f32,
    /// Fraction of FTP during the hard piece.
    pub on_power: f32,
    /// Fraction of FTP during the easy piece.
    pub off_power: f32,
    /// Cadence to show during the hard piece.
    pub on_cadence: Option<u16>,
    /// Cadence to show during the easy piece.
    pub off_cadence: Option<u16>,
    /// Cues, timed from the start of the whole set.
    pub cues: Vec<Cue>,
    /// Hard pieces are labelled `{label} 2 of 3`. `None` becomes `Hard`.
    pub on_label: Option<String>,
    /// Easy pieces use this text. `None` becomes `Recover`.
    pub off_label: Option<String>,
}

/// A section with no ERG target.
#[derive(Debug, Clone, PartialEq)]
pub struct OpenStep {
    /// Length of the section, in seconds.
    pub duration_s: f32,
    /// Cues, timed from the start of this section.
    pub cues: Vec<Cue>,
    /// Fraction of FTP drawn on the profile. Not held by the trainer.
    pub show_power: Option<f32>,
}

/// One part of a [`Workout`], before repeats are expanded.
#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    /// Rising or falling power at the start of a ride.
    Warmup(RampStep),
    /// Rising or falling power at the end of a ride.
    Cooldown(RampStep),
    /// Rising or falling power in the middle of a ride.
    Ramp(RampStep),
    /// A constant fraction of FTP.
    Steady(SteadyStep),
    /// Repeated hard and easy pieces.
    Intervals(IntervalStep),
    /// No target. The rider chooses the power.
    FreeRide(OpenStep),
    /// No target. The rider goes as hard as they can.
    MaxEffort(OpenStep),
}

impl Step {
    /// Length of this step, in seconds. Repeats include every on and off piece.
    #[must_use]
    pub fn duration_s(&self) -> f32 {
        match self {
            Self::Warmup(step) | Self::Cooldown(step) | Self::Ramp(step) => {
                positive(step.duration_s)
            }
            Self::Steady(step) => positive(step.duration_s),
            Self::Intervals(step) => {
                let once = positive(step.on_s) + positive(step.off_s);
                once * f32::from(step.repeat)
            }
            Self::FreeRide(step) | Self::MaxEffort(step) => positive(step.duration_s),
        }
    }
}

/// Planned difficulty of a workout, independent of who rides it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Intensity {
    /// Time-weighted mean of the planned fraction of FTP.
    ///
    /// Free-ride and max-effort seconds are left out. `0.0` when no second
    /// has a target.
    pub average_ftp_fraction: f32,
    /// Planned stress score, using Coggan's normalised power.
    ///
    /// `None` when the targeted work is under 30 seconds, or when the plan
    /// contains a free ride or a max effort. Those sections have no power to
    /// put in the 30-second window, so the score is left out rather than
    /// invented. The score assumes every target is held. It does not depend
    /// on the absolute FTP.
    pub stress_score: Option<f32>,
}

/// One bar of the profile graph.
#[derive(Debug, Clone, PartialEq)]
pub struct ProfileBar {
    /// Seconds from the start of the workout to the left edge.
    pub start_s: f32,
    /// Width of the bar, in seconds.
    pub duration_s: f32,
    /// Fraction of FTP at the left edge. `None` when the section is open and has nothing to draw.
    pub start_power: Option<f32>,
    /// Fraction of FTP at the right edge. Equal to `start_power` for a steady block.
    pub end_power: Option<f32>,
    /// Short label, such as `Sweet spot 2 of 3` or `Recover`.
    pub label: String,
    /// `true` for free ride and max effort. The trainer does not hold `start_power`.
    pub open: bool,
}

/// A named plan.
#[derive(Debug, Clone, PartialEq)]
pub struct Workout {
    /// Stable id. Built-ins do not change. An imported file uses its file name.
    pub id: String,
    /// Name shown in the list.
    pub name: String,
    /// One or two sentences under the name.
    pub description: String,
    /// Steps in ride order. Repeats are still one step here.
    pub steps: Vec<Step>,
    /// When `true`, the run estimates FTP as 75% of the last steady step the
    /// rider finishes. Warm-up, cool-down, and ramps are not scored.
    pub ramp_test: bool,
}

impl Workout {
    /// Sum of the step lengths, in seconds.
    #[must_use]
    pub fn duration_s(&self) -> f32 {
        self.steps.iter().map(Step::duration_s).sum()
    }

    /// Average fraction of FTP, and a stress score when the plan can support one.
    #[must_use]
    pub fn intensity(&self) -> Intensity {
        let atoms = expand(self);
        let mut weighted = 0.0_f32;
        let mut targeted = 0.0_f32;
        let mut any_open = false;
        for atom in &atoms {
            match atom.shape {
                Shape::Open => any_open = true,
                Shape::Hold(power) => {
                    weighted += power * atom.duration_s;
                    targeted += atom.duration_s;
                }
                Shape::Ramp { low, high } => {
                    weighted += (low + high) * 0.5 * atom.duration_s;
                    targeted += atom.duration_s;
                }
            }
        }
        let average = if targeted > 0.0 {
            weighted / targeted
        } else {
            0.0
        };
        let stress_score = if any_open {
            None
        } else {
            stress_score(&atoms).map(|score| score as f32)
        };
        Intensity {
            average_ftp_fraction: average,
            stress_score,
        }
    }

    /// Intensity Factor: the fourth-root mean of the planned fraction of FTP.
    ///
    /// Each targeted second is one sample. A ramp is sampled at the middle of
    /// each second, so the slope is weighted by time. Free-ride and max-effort
    /// seconds have no target and are left out. An empty plan, or a plan with
    /// no targeted second, is `0.0`.
    #[must_use]
    pub fn intensity_factor(&self) -> f32 {
        let sampled = target_samples(self);
        if sampled.count == 0 {
            return 0.0;
        }
        let mean = sampled.fourths / sampled.count as f64;
        if !mean.is_finite() || mean <= 0.0 {
            return 0.0;
        }
        let factor = mean.powf(0.25);
        if factor.is_finite() {
            factor as f32
        } else {
            0.0
        }
    }

    /// Training Stress Score from [`Self::intensity_factor`].
    ///
    /// `duration_s * IF² / 36`, which is `hours * IF² * 100`, rounded to the
    /// nearest integer. An hour at FTP scores 100. An hour at 70% of FTP
    /// scores 49. The duration is the whole plan. The result is clamped to `u16`.
    #[must_use]
    pub fn tss(&self) -> u16 {
        let factor = f64::from(self.intensity_factor());
        let score = f64::from(self.duration_s()) * factor * factor / 36.0;
        rounded_u16(score)
    }

    /// Estimated mechanical work, in kilojoules, if every target is held.
    ///
    /// Each targeted second contributes `fraction * ftp_w` joules. The sum is
    /// divided by 1000 and rounded. Open seconds contribute nothing. An hour
    /// at 250 W is 900. The result is clamped to `u16`.
    #[must_use]
    pub fn estimated_kj(&self, ftp_w: u16) -> u16 {
        let sampled = target_samples(self);
        let joules = sampled.fraction_sum * f64::from(ftp_w);
        rounded_u16(joules / 1000.0)
    }

    /// One bar per expanded piece, in order. Repeats are already unrolled.
    #[must_use]
    pub fn profile(&self) -> Vec<ProfileBar> {
        expand(self)
            .into_iter()
            .map(|atom| ProfileBar {
                start_s: atom.start_s,
                duration_s: atom.duration_s,
                start_power: atom.show_low,
                end_power: atom.show_high,
                label: atom.label,
                open: atom.open,
            })
            .collect()
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum Shape {
    Hold(f32),
    Ramp { low: f32, high: f32 },
    Open,
}

#[derive(Debug, Clone)]
pub(crate) struct Atom {
    pub label: String,
    pub start_s: f32,
    pub duration_s: f32,
    pub shape: Shape,
    pub cadence: Option<u16>,
    pub cues: Vec<Cue>,
    pub scored: bool,
    pub open: bool,
    pub show_low: Option<f32>,
    pub show_high: Option<f32>,
}

impl Atom {
    pub(crate) fn fraction_at(&self, elapsed_s: f32) -> Option<f32> {
        match self.shape {
            Shape::Open => None,
            Shape::Hold(power) => Some(power),
            Shape::Ramp { low, high } => {
                let t = if self.duration_s <= 0.0 {
                    0.0
                } else {
                    (elapsed_s / self.duration_s).clamp(0.0, 1.0)
                };
                Some(low + (high - low) * t)
            }
        }
    }
}

pub(crate) fn expand(workout: &Workout) -> Vec<Atom> {
    let mut atoms = Vec::new();
    let mut start = 0.0_f32;
    for step in &workout.steps {
        let before = atoms.len();
        match step {
            Step::Warmup(ramp) => push_ramp(&mut atoms, "Warm up", ramp, false),
            Step::Cooldown(ramp) => push_ramp(&mut atoms, "Cool down", ramp, false),
            Step::Ramp(ramp) => push_ramp(&mut atoms, "Ramp", ramp, false),
            Step::Steady(steady) => {
                let label = steady.label.as_deref().unwrap_or("Steady");
                push_hold(
                    &mut atoms,
                    label,
                    positive(steady.duration_s),
                    steady.power,
                    steady.cadence_rpm,
                    &steady.cues,
                    workout.ramp_test,
                );
            }
            Step::Intervals(intervals) => push_intervals(&mut atoms, intervals),
            Step::FreeRide(open) => {
                push_open(&mut atoms, "Free ride", open);
            }
            Step::MaxEffort(open) => {
                push_open(&mut atoms, "Max effort", open);
            }
        }
        for atom in &mut atoms[before..] {
            atom.start_s = start;
            start += atom.duration_s;
        }
    }
    atoms
}

fn push_ramp(out: &mut Vec<Atom>, label: &str, step: &RampStep, scored: bool) {
    let duration = positive(step.duration_s);
    if duration <= 0.0 || !step.power_low.is_finite() || !step.power_high.is_finite() {
        return;
    }
    out.push(Atom {
        label: label.to_owned(),
        start_s: 0.0,
        duration_s: duration,
        shape: Shape::Ramp {
            low: step.power_low,
            high: step.power_high,
        },
        cadence: step.cadence_rpm,
        cues: sorted_cues(&step.cues),
        scored,
        open: false,
        show_low: Some(step.power_low),
        show_high: Some(step.power_high),
    });
}

fn push_hold(
    out: &mut Vec<Atom>,
    label: &str,
    duration: f32,
    power: f32,
    cadence: Option<u16>,
    cues: &[Cue],
    scored: bool,
) {
    if duration <= 0.0 || !power.is_finite() {
        return;
    }
    out.push(Atom {
        label: label.to_owned(),
        start_s: 0.0,
        duration_s: duration,
        shape: Shape::Hold(power),
        cadence,
        cues: sorted_cues(cues),
        scored,
        open: false,
        show_low: Some(power),
        show_high: Some(power),
    });
}

fn push_open(out: &mut Vec<Atom>, label: &str, step: &OpenStep) {
    let duration = positive(step.duration_s);
    if duration <= 0.0 {
        return;
    }
    let show = step.show_power.filter(|power| power.is_finite());
    out.push(Atom {
        label: label.to_owned(),
        start_s: 0.0,
        duration_s: duration,
        shape: Shape::Open,
        cadence: None,
        cues: sorted_cues(&step.cues),
        scored: false,
        open: true,
        show_low: show,
        show_high: show,
    });
}

fn push_intervals(out: &mut Vec<Atom>, step: &IntervalStep) {
    let on_s = positive(step.on_s);
    let off_s = positive(step.off_s);
    if step.repeat == 0 || (on_s <= 0.0 && off_s <= 0.0) {
        return;
    }
    if !step.on_power.is_finite() || !step.off_power.is_finite() {
        return;
    }
    let cues = sorted_cues(&step.cues);
    let on_name = step.on_label.as_deref().unwrap_or("Hard");
    let off_name = step.off_label.as_deref().unwrap_or("Recover");
    let mut cursor = 0.0_f32;
    for index in 1..=step.repeat {
        if on_s > 0.0 {
            let end = cursor + on_s;
            let last = off_s <= 0.0 && index == step.repeat;
            push_hold(
                out,
                &format!("{on_name} {index} of {repeat}", repeat = step.repeat),
                on_s,
                step.on_power,
                step.on_cadence,
                &cues_between(&cues, cursor, end, last),
                false,
            );
            cursor = end;
        }
        if off_s > 0.0 {
            let end = cursor + off_s;
            let last = index == step.repeat;
            push_hold(
                out,
                off_name,
                off_s,
                step.off_power,
                step.off_cadence,
                &cues_between(&cues, cursor, end, last),
                false,
            );
            cursor = end;
        }
    }
}

fn cues_between(cues: &[Cue], start: f32, end: f32, include_end: bool) -> Vec<Cue> {
    cues.iter()
        .filter(|cue| {
            cue.offset_s >= start && (cue.offset_s < end || (include_end && cue.offset_s <= end))
        })
        .map(|cue| Cue {
            offset_s: cue.offset_s - start,
            text: cue.text.clone(),
        })
        .collect()
}

fn sorted_cues(cues: &[Cue]) -> Vec<Cue> {
    let mut cues: Vec<Cue> = cues
        .iter()
        .filter(|cue| !cue.text.is_empty() && cue.offset_s.is_finite() && cue.offset_s >= 0.0)
        .cloned()
        .collect();
    cues.sort_by(|left, right| {
        left.offset_s
            .partial_cmp(&right.offset_s)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    cues
}

fn positive(seconds: f32) -> f32 {
    if seconds.is_finite() && seconds > 0.0 {
        seconds
    } else {
        0.0
    }
}

/// Coggan stress score from one sample per second.
///
/// The 30-second rolling average starts at the 30th sample. Each average is
/// raised to the fourth power, those powers are averaged, and the fourth
/// root is the normalised fraction of FTP. The stress score is
/// `seconds * NP² * 100 / 3600`, which is 100 for an hour at FTP.
fn stress_score(atoms: &[Atom]) -> Option<f64> {
    let mut samples = Vec::new();
    for atom in atoms {
        let count = whole_seconds(atom.duration_s);
        if count == 0 {
            continue;
        }
        match atom.shape {
            Shape::Open => return None,
            Shape::Hold(power) => {
                let power = f64::from(power);
                samples.extend(std::iter::repeat_n(power, count));
            }
            Shape::Ramp { low, high } => {
                let low = f64::from(low);
                let high = f64::from(high);
                let steps = count as f64;
                for index in 0..count {
                    let t = (index as f64 + 0.5) / steps;
                    samples.push(low + (high - low) * t);
                }
            }
        }
    }
    if samples.len() < 30 {
        return None;
    }
    let mut window: f64 = samples[..30].iter().sum();
    let mut fourths = (window / 30.0).powi(4);
    let mut windows = 1.0_f64;
    for index in 30..samples.len() {
        window += samples[index] - samples[index - 30];
        fourths += (window / 30.0).powi(4);
        windows += 1.0;
    }
    let normalized = (fourths / windows).powf(0.25);
    let seconds = samples.len() as f64;
    Some(seconds * normalized * normalized * 100.0 / 3600.0)
}

fn whole_seconds(duration_s: f32) -> usize {
    if !duration_s.is_finite() || duration_s <= 0.0 {
        return 0;
    }
    let rounded = duration_s.round();
    if rounded >= 86_400.0 {
        86_400
    } else {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        {
            rounded as usize
        }
    }
}

struct TargetSamples {
    fourths: f64,
    fraction_sum: f64,
    count: usize,
}

/// One sample per targeted second. Ramps use the same midpoint as [`stress_score`].
fn target_samples(workout: &Workout) -> TargetSamples {
    let mut fourths = 0.0_f64;
    let mut fraction_sum = 0.0_f64;
    let mut count = 0_usize;
    for atom in expand(workout) {
        let seconds = whole_seconds(atom.duration_s);
        if seconds == 0 {
            continue;
        }
        match atom.shape {
            Shape::Open => {}
            Shape::Hold(power) => {
                let power = f64::from(power);
                let steps = seconds as f64;
                fourths += power.powi(4) * steps;
                fraction_sum += power * steps;
                count += seconds;
            }
            Shape::Ramp { low, high } => {
                let low = f64::from(low);
                let high = f64::from(high);
                let steps = seconds as f64;
                for index in 0..seconds {
                    let t = (index as f64 + 0.5) / steps;
                    let power = low + (high - low) * t;
                    fourths += power.powi(4);
                    fraction_sum += power;
                    count += 1;
                }
            }
        }
    }
    TargetSamples {
        fourths,
        fraction_sum,
        count,
    }
}

fn rounded_u16(value: f64) -> u16 {
    if !value.is_finite() || value <= 0.0 {
        return 0;
    }
    let rounded = value.round();
    if rounded >= f64::from(u16::MAX) {
        u16::MAX
    } else {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        {
            rounded as u16
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn steady(seconds: f32, power: f32) -> Step {
        Step::Steady(SteadyStep {
            duration_s: seconds,
            power,
            cadence_rpm: None,
            cues: Vec::new(),
            label: None,
        })
    }

    fn workout(steps: Vec<Step>) -> Workout {
        Workout {
            id: "t".to_owned(),
            name: "T".to_owned(),
            description: String::new(),
            steps,
            ramp_test: false,
        }
    }

    #[test]
    fn an_hour_at_ftp_scores_one_hundred() {
        let plan = workout(vec![steady(3600.0, 1.0)]);
        let intensity = plan.intensity();
        assert!((intensity.average_ftp_fraction - 1.0).abs() < 1e-6);
        let score = intensity.stress_score.expect("score");
        assert!((score - 100.0).abs() < 1e-3, "{score}");
    }

    #[test]
    fn thirty_minutes_at_eighty_percent_scores_thirty_two() {
        let plan = workout(vec![steady(1800.0, 0.8)]);
        let score = plan.intensity().stress_score.expect("score");
        assert!((score - 32.0).abs() < 1e-3, "{score}");
    }

    #[test]
    fn a_ramp_averages_its_two_ends() {
        let plan = workout(vec![Step::Ramp(RampStep {
            duration_s: 100.0,
            power_low: 0.4,
            power_high: 0.8,
            cadence_rpm: None,
            cues: Vec::new(),
        })]);
        let intensity = plan.intensity();
        assert!((intensity.average_ftp_fraction - 0.6).abs() < 1e-6);
        assert!(intensity.stress_score.is_some());
    }

    #[test]
    fn open_sections_drop_the_stress_score_and_stay_out_of_the_average() {
        let plan = workout(vec![
            steady(60.0, 0.5),
            Step::FreeRide(OpenStep {
                duration_s: 60.0,
                cues: Vec::new(),
                show_power: None,
            }),
            steady(60.0, 1.0),
        ]);
        let intensity = plan.intensity();
        assert!((intensity.average_ftp_fraction - 0.75).abs() < 1e-6);
        assert!(intensity.stress_score.is_none());
        assert!((plan.duration_s() - 180.0).abs() < 1e-6);
    }

    #[test]
    fn short_work_has_no_stress_score() {
        let plan = workout(vec![steady(20.0, 1.0)]);
        assert!(plan.intensity().stress_score.is_none());
    }

    #[test]
    fn intervals_expand_into_labelled_bars() {
        let plan = workout(vec![Step::Intervals(IntervalStep {
            repeat: 2,
            on_s: 10.0,
            off_s: 5.0,
            on_power: 1.2,
            off_power: 0.5,
            on_cadence: Some(100),
            off_cadence: Some(80),
            cues: vec![Cue {
                offset_s: 0.0,
                text: "Go".to_owned(),
            }],
            on_label: Some("Sweet spot".to_owned()),
            off_label: None,
        })]);
        let bars = plan.profile();
        assert_eq!(bars.len(), 4);
        assert_eq!(bars[0].label, "Sweet spot 1 of 2");
        assert_eq!(bars[1].label, "Recover");
        assert_eq!(bars[2].label, "Sweet spot 2 of 2");
        assert_eq!(bars[3].label, "Recover");
        assert!((bars[0].duration_s - 10.0).abs() < 1e-6);
        assert!((bars[1].duration_s - 5.0).abs() < 1e-6);
        assert!((bars[0].start_power.unwrap() - 1.2).abs() < 1e-6);
        assert!((bars[1].start_power.unwrap() - 0.5).abs() < 1e-6);
        assert!((bars[2].start_s - 15.0).abs() < 1e-6);
        assert!(!bars[0].open);
        let atoms = expand(&plan);
        assert_eq!(atoms[0].cues.len(), 1);
        assert!(atoms[1].cues.is_empty());
    }

    #[test]
    fn an_hour_at_ftp_has_if_one_tss_one_hundred_and_nine_hundred_kj() {
        let plan = workout(vec![steady(3600.0, 1.0)]);
        let factor = plan.intensity_factor();
        assert!((factor - 1.0).abs() < 1e-5, "{factor}");
        assert_eq!(plan.tss(), 100);
        assert_eq!(plan.estimated_kj(250), 900);
        assert_eq!(plan.estimated_kj(0), 0);
    }

    #[test]
    fn an_hour_at_seventy_percent_has_if_point_seven_and_tss_forty_nine() {
        let plan = workout(vec![steady(3600.0, 0.70)]);
        let factor = plan.intensity_factor();
        assert!((factor - 0.70).abs() < 1e-5, "{factor}");
        assert_eq!(plan.tss(), 49);
    }

    #[test]
    fn ramps_and_empty_plans_stay_finite() {
        let empty = workout(vec![]);
        assert_eq!(empty.intensity_factor(), 0.0);
        assert_eq!(empty.tss(), 0);
        assert_eq!(empty.estimated_kj(250), 0);

        let zero = workout(vec![steady(0.0, 1.0)]);
        assert_eq!(zero.intensity_factor(), 0.0);
        assert_eq!(zero.tss(), 0);
        assert_eq!(zero.estimated_kj(200), 0);

        let open = workout(vec![Step::FreeRide(OpenStep {
            duration_s: 120.0,
            cues: Vec::new(),
            show_power: Some(0.5),
        })]);
        assert_eq!(open.intensity_factor(), 0.0);
        assert_eq!(open.tss(), 0);
        assert_eq!(open.estimated_kj(250), 0);

        let ramp = workout(vec![Step::Ramp(RampStep {
            duration_s: 600.0,
            power_low: 0.50,
            power_high: 1.00,
            cadence_rpm: None,
            cues: Vec::new(),
        })]);
        let factor = ramp.intensity_factor();
        assert!(factor.is_finite(), "{factor}");
        // A rising ramp's fourth-root mean sits above the arithmetic mean, 0.75.
        assert!(factor > 0.75, "{factor}");
        assert!(factor < 1.0, "{factor}");
        assert!(ramp.tss() > 0);
        // Midpoint samples of a straight ramp sum to the trapezoid:
        // 600 s * 0.75 * 200 W / 1000 = 90 kJ.
        assert_eq!(ramp.estimated_kj(200), 90);

        let short = workout(vec![Step::Warmup(RampStep {
            duration_s: 10.0,
            power_low: 0.40,
            power_high: 0.80,
            cadence_rpm: None,
            cues: Vec::new(),
        })]);
        let short_factor = short.intensity_factor();
        assert!(short_factor.is_finite(), "{short_factor}");
        assert!(short_factor > 0.60, "{short_factor}");
        assert!(short_factor < 0.80, "{short_factor}");
        // Ten seconds at this height rounds the score down to 0.
        assert_eq!(short.tss(), 0);
        assert!(short.estimated_kj(180) > 0);

        let long = workout(vec![steady(3_000_000.0, 1.0)]);
        assert!((long.intensity_factor() - 1.0).abs() < 1e-5);
        assert_eq!(long.tss(), u16::MAX);
        let hard = workout(vec![steady(7200.0, 2.0)]);
        assert_eq!(hard.estimated_kj(u16::MAX), u16::MAX);
    }
}
